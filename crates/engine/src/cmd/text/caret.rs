use super::autocorrect::undo_auto_list;
use super::editing::{inherited_aligns, insert_text, promote_rtl_if_needed};
use super::ime::{composition_range, model_pos, walk_backward, walk_forward};
use super::layout::{ctx_for, fit_text_box, layout_for, pos_param};
use super::{
    DocState, HistoryEntry, Pos, Result, Session, ShapeId, ShapeKind, TextBody, Value, bad, body_of, bool_or, ed, edit_shapes, edit_text, id_param,
    json, ok, push_undo, str_param, xfrm_of,
};

/// A table cell's text laid out the way it is drawn, mapped into slide coordinates.
pub(super) fn cell_on_slide(st: &DocState, id: ShapeId, row: usize, col: usize) -> Option<CellText> {
    let sh = st.shape(id)?;
    let ShapeKind::Table(tb) = &sh.kind else { return None };
    let cell = tb.cell(row, col)?;
    let ctx = ctx_for(&st.doc, &st.selection)?;
    let heights = crate::cmd::table_drawn_heights(&st.doc, &st.selection, tb);
    let rect = deckcraft_render::table_cell_rects(tb, &heights).into_iter().find(|(r, c, _)| *r == row && *c == col)?.2;
    let mut body = deckcraft_render::table_cell_body(&ctx, tb, row, col, cell);
    if body.paragraphs.is_empty() {
        body.paragraphs.push(deckcraft_model::text::Paragraph::default());
    }
    let tmp = deckcraft_model::Shape::table_cell(body.clone());
    let layout = deckcraft_text::layout(
        &ctx,
        &tmp,
        &body,
        &deckcraft_text::Opts { rect, fields: &deckcraft_text::NoFields, prompt_color: None, no_shrink: false },
    );
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let to_slide = deckcraft_render::text_transform(x.affine(), rect, layout.rotation);
    Some(CellText { layout, to_slide })
}

pub(super) struct CellText {
    pub(super) layout: deckcraft_text::TextLayout,
    pub(super) to_slide: deckcraft_geom::Affine,
}

pub(super) fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").ok_or_else(|| bad("text.insert", "missing `text`"))?.to_string();
    insert_text(s, &text, None)
}

pub(crate) fn delete_dir(s: &mut Session, dir: &str) -> Result<Value> {
    if dir == "backward" && undo_auto_list(s)? {
        return ok();
    }
    let dir = dir.to_string();
    edit_text(s, move |body, t| {
        let (a, b) = t.ordered();
        let at = if dir == "wordBackward" || dir == "wordForward" {
            // Ctrl+Backspace/Delete work from the start of a selection and leave the rest of it
            // (PowerPoint for Windows).
            ed::delete_word(body, a, dir == "wordForward")
        } else if a != b {
            ed::delete(body, a, b)
        } else {
            let other = match dir.as_str() {
                "forward" => ed::cluster_end(body, a),
                _ => {
                    // Backspace at the start of an indented/bulleted paragraph outdents first.
                    if a.1 == 0
                        && let Some(para) = body.paragraphs.get_mut(a.0)
                        && para.level > 0
                    {
                        para.level -= 1;
                        t.anchor = a;
                        t.caret = a;
                        return;
                    }
                    ed::char_move(body, a, false)
                }
            };
            ed::delete(body, a, other)
        };
        t.anchor = at;
        t.caret = at;
    })?;
    ok()
}

/// A `before`/`after` count from a control or input-method request.
///
/// Input-derived numbers are hostile: a value that does not fit `usize` (on 32-bit wasm, say) is
/// clamped to `usize::MAX` and then bounded by the walk, never truncated to a smaller count that
/// would delete different text than asked.
pub(super) fn count_param(p: &Value, name: &str) -> usize {
    p.get(name).and_then(Value::as_u64).map_or(0, |v| usize::try_from(v).unwrap_or(usize::MAX))
}

/// Delete at most the requested character or byte counts around the selection or pre-edit,
/// preserving selection direction and composition boundaries and recording one undo step
/// outside composition.
pub(super) fn delete_surrounding(s: &mut Session, p: &Value) -> Result<Value> {
    let before = count_param(p, "before");
    let after = count_param(p, "after");
    let bytes = bool_or(p, "bytes", false);
    let st = s.doc()?;
    let t = st.selection.text.clone().ok_or_else(|| bad("text.deleteSurrounding", "not editing text"))?;
    let body = body_of(st, &t).ok_or_else(|| bad("text.deleteSurrounding", "the text being edited is gone"))?;
    let comp = composition_range(body, st.composition.as_ref());
    // The ranges are in the same logical coordinates `ime_surrounding` publishes: one byte for a
    // paragraph separator and for a soft line break. A range may therefore cross paragraphs.
    let ordered = {
        let a = model_pos(body, t.anchor).ok_or_else(|| bad("text.deleteSurrounding", "the selection is not in the text"))?;
        let b = model_pos(body, t.caret).ok_or_else(|| bad("text.deleteSurrounding", "the selection is not in the text"))?;
        if a <= b { (a, b) } else { (b, a) }
    };
    // The selection (or the input method's pre-edit) stays exactly where it is. Only whole
    // scalars fully inside the requested before/after ranges around it are removed.
    let (start, end) = match comp {
        Some((pi, cs, ce)) => ((pi, cs), (pi, ce)),
        None => ordered,
    };
    let (from, before_chars, _) = walk_backward(body, comp, start, before, bytes);
    let (to, after_chars, _) = walk_forward(body, comp, end, after, bytes);
    let deleted = before_chars + after_chars;
    if deleted == 0 {
        return Ok(json!({"deleted": 0}));
    }
    let before_doc = st.doc.clone();
    let before_sel = st.selection.clone();
    let composing = st.composition.is_some();
    let vertex_kind = s.tool.vertex_kind.clone();
    edit_text(s, |body, t| {
        // Delete the after-range first, so the positions of the earlier range stay valid; then
        // the text before the selection.
        if after_chars > 0 {
            ed::delete(body, end, to);
        }
        if before_chars > 0 {
            ed::delete(body, from, start);
        }
        t.anchor = remap_after_delete(t.anchor, start, from, before_chars);
        t.caret = remap_after_delete(t.caret, start, from, before_chars);
        t.line_end = None;
    })?;
    if composing {
        if let Some(c) = s.doc_mut()?.composition.as_mut() {
            c.start = if before_chars == 0 {
                start
            } else if from.0 == start.0 {
                (start.0, start.1 - before_chars)
            } else {
                from
            };
        }
    } else {
        push_undo(s.doc_mut()?, HistoryEntry { label: "Delete Text".into(), doc: before_doc, selection: before_sel, vertex_kind });
    }
    Ok(json!({"deleted": deleted}))
}

/// Map a text position across the deletion of `before_chars` characters that begins at `start`
/// and reaches back to `from`. Within the start paragraph its characters shift; from an earlier
/// paragraph those paragraphs merge, so later paragraph numbers shift too.
fn remap_after_delete(pos: (usize, usize), start: (usize, usize), from: (usize, usize), before_chars: usize) -> (usize, usize) {
    if before_chars == 0 || pos < start {
        return pos;
    }
    if from.0 == start.0 {
        // Only endpoints in the start paragraph move; later paragraphs keep their text.
        if pos.0 == start.0 { (pos.0, pos.1 - before_chars) } else { pos }
    } else {
        (from.0 + (pos.0 - start.0), if pos.0 == start.0 { from.1 + (pos.1 - start.1) } else { pos.1 })
    }
}

pub(super) fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    delete_dir(s, str_param(p, "dir").unwrap_or("backward"))
}

/// Is paragraph `para` right-to-left (its lines run from the right)?
fn para_rtl(layout: Option<&deckcraft_text::TextLayout>, body: &TextBody, para: usize) -> bool {
    match layout.and_then(|l| l.lines.iter().find(|x| x.para == para)) {
        Some(line) => line.rtl,
        None => body.paragraphs.get(para).and_then(|x| x.props.rtl).unwrap_or(false),
    }
}

pub(super) fn move_caret(s: &mut Session, p: &Value) -> Result<Value> {
    use deckcraft_text::Affinity;
    let to = str_param(p, "to").unwrap_or("right").to_string();
    let extend = bool_or(p, "extend", false);
    let st = s.doc()?;
    let t = st.selection.text.clone().ok_or_else(|| bad("text.move", "not editing text"))?;
    let body = body_of(st, &t).cloned().unwrap_or_default();
    let layout = layout_for(st, &t);
    let lp = |c: Pos| deckcraft_text::Pos { para: c.0, ch: c.1 };
    let caret = t.caret;
    let aff = if t.at_line_end() { Affinity::Upstream } else { Affinity::Downstream };
    // Left and right are on screen: in a right-to-left paragraph left goes forward in the text.
    let rtl = para_rtl(layout.as_ref(), &body, caret.0);
    let to = match (to.as_str(), rtl) {
        ("left", true) => "right",
        ("right", true) => "left",
        ("wordLeft", true) => "wordRight",
        ("wordRight", true) => "wordLeft",
        (x, _) => x,
    }
    .to_string();
    let collapse_to = |left: bool| if left { t.ordered().0 } else { t.ordered().1 };
    let mut upstream = false;
    let new = match to.as_str() {
        "left" if t.is_range() && !extend => collapse_to(true),
        "right" if t.is_range() && !extend => collapse_to(false),
        "left" => ed::char_move(&body, caret, false),
        "right" => ed::char_move(&body, caret, true),
        // Ctrl+arrows with a selection go on from its start (left) or end (right).
        "wordLeft" => ed::word_move(&body, if extend { caret } else { collapse_to(true) }, false),
        "wordRight" => ed::word_move(&body, if extend { caret } else { collapse_to(false) }, true),
        "paraStart" => (caret.0, 0),
        "paraEnd" => (caret.0, body.paragraphs.get(caret.0).map(|x| x.char_len()).unwrap_or(0)),
        "paraUp" => ed::para_up(&body, caret),
        "paraDown" => ed::para_down(&body, caret),
        "start" => (0, 0),
        "end" => ed::end(&body),
        "up" | "down" | "lineStart" | "lineEnd" => match &layout {
            Some(l) => {
                let x = l.caret_drawn(lp(caret), aff).map(|c| c.0).unwrap_or(0.0);
                let (r, a) = match to.as_str() {
                    "up" => l.vertical_affine(lp(caret), aff, false, x),
                    "down" => l.vertical_affine(lp(caret), aff, true, x),
                    "lineStart" => (l.line_bounds_affine(lp(caret), aff).0, Affinity::Downstream),
                    // End stays on its line: after a soft wrap the caret sits at the line's end.
                    _ => l.line_end_caret(lp(caret), aff),
                };
                upstream = a == Affinity::Upstream;
                (r.para, r.ch)
            }
            None => match to.as_str() {
                "up" => (caret.0.saturating_sub(1), caret.1),
                "down" => (caret.0.saturating_add(1).min(body.paragraphs.len().saturating_sub(1)), caret.1),
                "lineStart" => (caret.0, 0),
                _ => (caret.0, body.paragraphs.get(caret.0).map(|x| x.char_len()).unwrap_or(0)),
            },
        },
        other => return Err(bad("text.move", format!("unknown direction `{other}`"))),
    };
    let new = (new.0.min(body.paragraphs.len().saturating_sub(1)), new.1.min(body.paragraphs.get(new.0).map(|x| x.char_len()).unwrap_or(0)));
    s.select(|_, sel| {
        if let Some(tt) = sel.text.as_mut() {
            tt.caret = new;
            tt.line_end = if upstream { Some(new) } else { None };
            if !extend {
                tt.anchor = new;
            }
        }
    })?;
    Ok(json!({"caret": [new.0, new.1], "lineEnd": upstream}))
}

pub(super) fn select(s: &mut Session, p: &Value) -> Result<Value> {
    let a = pos_param(p, "anchor").ok_or_else(|| bad("text.select", "missing `anchor`"))?;
    let c = pos_param(p, "caret").unwrap_or(a);
    s.select(|_, sel| {
        if let Some(t) = sel.text.as_mut() {
            t.anchor = a;
            t.caret = c;
        }
    })?;
    ok()
}

pub(super) fn select_all(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let t = st.selection.text.clone().ok_or_else(|| bad("text.selectAll", "not editing text"))?;
    let end = body_of(st, &t).map(ed::end).unwrap_or((0, 0));
    s.select(|_, sel| {
        if let Some(t) = sel.text.as_mut() {
            t.anchor = (0, 0);
            t.caret = end;
        }
    })?;
    ok()
}

pub(super) fn select_word(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let t = st.selection.text.clone().ok_or_else(|| bad("text.selectWord", "not editing text"))?;
    let at = pos_param(p, "at").unwrap_or(t.caret);
    let body = body_of(st, &t).cloned().unwrap_or_default();
    let (a, b) = ed::word_at(&body, at);
    s.select(|_, sel| {
        if let Some(t) = sel.text.as_mut() {
            t.anchor = a;
            t.caret = b;
        }
    })?;
    ok()
}

pub(super) fn select_para(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let t = st.selection.text.clone().ok_or_else(|| bad("text.selectParagraph", "not editing text"))?;
    let at = pos_param(p, "at").unwrap_or(t.caret);
    let body = body_of(st, &t);
    // Clamp to the last paragraph so a huge index can neither overflow nor select past the body.
    let at = (at.0.min(body.map_or(0, |b| b.paragraphs.len().saturating_sub(1))), at.1);
    let n = body.and_then(|b| b.paragraphs.get(at.0)).map(|x| x.char_len()).unwrap_or(0);
    // The paragraph mark is part of the paragraph (PowerPoint's triple-click selects it too).
    let end = if body.is_some_and(|b| at.0.saturating_add(1) < b.paragraphs.len()) { (at.0.saturating_add(1), 0) } else { (at.0, n) };
    s.select(|_, sel| {
        if let Some(t) = sel.text.as_mut() {
            t.anchor = (at.0, 0);
            t.caret = end;
        }
    })?;
    ok()
}

/// Turn leading tabs of a paragraph into its outline level (capped at 8) and strip them.
/// Returns whether any tab was found.
pub(crate) fn apply_leading_tabs(para: &mut deckcraft_model::text::Paragraph) -> bool {
    let tabs = para.runs.first().map(|r| r.text.chars().take_while(|c| *c == '\t').count()).unwrap_or(0);
    if tabs == 0 {
        return false;
    }
    if let Some(r) = para.runs.first_mut() {
        r.text = r.text.trim_start_matches('\t').to_string();
    }
    para.level = tabs.min(8) as u8;
    true
}

/// The Outline editor sends all paragraphs, with tabs recording their levels. Use identifiable
/// section headings to preserve child formatting when sections change order.
fn replace_outline(body: &mut TextBody, mut next: TextBody, mut adjust: impl FnMut(&mut deckcraft_model::text::Paragraph)) {
    let old_text: Vec<_> = body.paragraphs.iter().map(|para| para.text()).collect();
    let new_text: Vec<_> = next
        .paragraphs
        .iter_mut()
        .map(|para| {
            apply_leading_tabs(para);
            para.text()
        })
        .collect();
    let mut used = vec![false; old_text.len()];
    let mut originals = vec![None; new_text.len()];
    let bodies = [&*body, &next];
    let texts: [&[String]; 2] = [&old_text, &new_text];
    let old: Vec<_> = (0..old_text.len()).collect();
    let new: Vec<_> = (0..new_text.len()).collect();
    let all: [&[usize]; 2] = [&old, &new];
    let [old_headings, new_headings] = outline_headings(bodies, texts);
    let groups: Vec<_> = if new_headings.iter().flatten().is_sorted() {
        Vec::new()
    } else {
        for (target, &source) in new_headings.iter().enumerate() {
            if let Some(source) = source {
                originals[target] = Some(source);
                used[source] = true;
            }
        }
        let sections = [outline_sections(&body.paragraphs, &old_headings), outline_sections(&next.paragraphs, &new_headings)];
        group_outline_sections(&sections, all).collect()
    };
    for [old, new] in &groups {
        match_outline_exact(bodies, texts, [old, new], &mut used, &mut originals);
        match_outline_edits(texts, [old, new], &mut used, &mut originals, false);
    }
    // Unchanged lines can move between sections. Reserve them before matching replacements
    // that share no text with the previous line in their section.
    match_outline_exact(bodies, texts, all, &mut used, &mut originals);
    for [old, new] in &groups {
        match_outline_edits(texts, [old, new], &mut used, &mut originals, true);
    }
    match_outline_edits(texts, all, &mut used, &mut originals, true);
    let mut following = originals.iter().flatten().copied().peekable();
    let mut preceding = None;
    for ((para, text), source) in next.paragraphs.iter_mut().zip(&new_text).zip(originals.iter().copied()) {
        if source.is_some() {
            preceding = following.next();
        }
        let base = source.or(preceding).or_else(|| following.peek().copied()).and_then(|i| body.paragraphs.get(i));
        if let Some(base) = base {
            let level = para.level;
            if source.is_some_and(|i| old_text[i] == *text) {
                *para = base.clone();
            } else {
                preserve_outline_run_style(para, base, text);
                adjust(para);
            }
            para.level = level;
        } else {
            adjust(para);
        }
    }
    body.paragraphs = next.paragraphs;
}

/// Match section headings by text and level. A section can gain or lose all of its children
/// when its label on the side with no children identifies a single line.
fn outline_headings(bodies: [&TextBody; 2], texts: [&[String]; 2]) -> [Vec<Option<usize>>; 2] {
    let mut by_heading = std::collections::HashMap::<(&str, u8), [Vec<usize>; 2]>::new();
    for (side, body) in bodies.into_iter().enumerate() {
        for (i, paragraph) in body.paragraphs.iter().enumerate() {
            let text = &texts[side][i];
            if !text.is_empty() {
                by_heading.entry((text, paragraph.level)).or_default()[side].push(i);
            }
        }
    }
    let [mut old, mut new] = bodies.map(|body| vec![None; body.paragraphs.len()]);
    for [sources, targets] in by_heading.into_values() {
        if let (Some((source, source_has_children)), Some((target, target_has_children))) =
            (outline_heading_candidate(&bodies[0].paragraphs, &sources), outline_heading_candidate(&bodies[1].paragraphs, &targets))
            && (source_has_children || target_has_children)
        {
            old[source] = Some(source);
            new[target] = Some(source);
        }
    }
    [old, new]
}

fn outline_heading_candidate(paragraphs: &[deckcraft_model::text::Paragraph], lines: &[usize]) -> Option<(usize, bool)> {
    let mut headings = lines.iter().copied().filter(|&i| paragraphs.get(i + 1).is_some_and(|next| next.level > paragraphs[i].level));
    match (headings.next(), headings.next(), lines) {
        (Some(heading), None, _) => Some((heading, true)),
        (None, None, [only]) => Some((*only, false)),
        _ => None,
    }
}

/// The closest identifiable heading above each paragraph, named by its original position.
fn outline_sections(paragraphs: &[deckcraft_model::text::Paragraph], headings: &[Option<usize>]) -> Vec<Option<usize>> {
    let mut ancestors = Vec::<(u8, usize)>::new();
    paragraphs
        .iter()
        .zip(headings)
        .map(|(paragraph, heading)| {
            while ancestors.last().is_some_and(|&(level, _)| level >= paragraph.level) {
                ancestors.pop();
            }
            let section = ancestors.last().map(|&(_, source)| source);
            if let Some(source) = heading {
                ancestors.push((paragraph.level, *source));
            }
            section
        })
        .collect()
}

fn group_outline_sections(sections: &[Vec<Option<usize>>; 2], indices: [&[usize]; 2]) -> impl Iterator<Item = [Vec<usize>; 2]> {
    let mut groups = std::collections::HashMap::<usize, [Vec<usize>; 2]>::new();
    for (side, indices) in indices.into_iter().enumerate() {
        for &i in indices {
            if let Some(section) = sections[side][i] {
                groups.entry(section).or_default()[side].push(i);
            }
        }
    }
    groups.into_values()
}

/// Match identical text among the remaining lines in the supplied group.
fn match_outline_exact(
    bodies: [&TextBody; 2],
    [old_text, new_text]: [&[String]; 2],
    [old, new]: [&[usize]; 2],
    used: &mut [bool],
    originals: &mut [Option<usize>],
) {
    let mut by_text = std::collections::HashMap::<&str, [Vec<usize>; 2]>::new();
    for &i in old.iter().filter(|&&i| !used[i]) {
        by_text.entry(&old_text[i]).or_default()[0].push(i);
    }
    for &i in new.iter().filter(|&&i| originals[i].is_none()) {
        by_text.entry(&new_text[i]).or_default()[1].push(i);
    }
    for group in by_text.into_values() {
        match_identical_outline_lines(bodies, group, used, originals);
    }
}

/// Match identical lines by level. Equal-sized groups with a level mismatch keep their order
/// when reindenting the lines.
fn match_identical_outline_lines([body, next]: [&TextBody; 2], [old, new]: [Vec<usize>; 2], used: &mut [bool], originals: &mut [Option<usize>]) {
    use std::collections::{HashMap, VecDeque};
    let mut levels = HashMap::<u8, VecDeque<usize>>::new();
    for &i in &old {
        levels.entry(body.paragraphs[i].level).or_default().push_back(i);
    }
    for &i in &new {
        originals[i] = levels.get_mut(&next.paragraphs[i].level).and_then(VecDeque::pop_front);
    }
    if old.len() == new.len() && new.iter().any(|&i| originals[i].is_none()) {
        for (&target, source) in new.iter().zip(old) {
            originals[target] = Some(source);
        }
    } else {
        for source in new.iter().filter_map(|&i| originals[i]) {
            used[source] = true;
        }
        let mut unused = old.into_iter().filter(|&i| !used[i]);
        for &i in &new {
            if originals[i].is_none() {
                originals[i] = unused.next();
            }
        }
    }
    for source in new.iter().filter_map(|&i| originals[i]) {
        used[source] = true;
    }
}

/// Match edited lines in their original order. When there are excess lines, a shared edge
/// distinguishes an edit from a nearby insertion or deletion.
fn match_outline_edits(
    [old, new]: [&[String]; 2],
    [sources, targets]: [&[usize]; 2],
    used: &mut [bool],
    originals: &mut [Option<usize>],
    allow_replacements: bool,
) {
    let candidates: Vec<_> = sources.iter().copied().filter(|&i| !used[i]).collect();
    let unmatched: Vec<_> = targets.iter().copied().filter(|&i| originals[i].is_none()).collect();
    let (mut old_at, mut new_at) = (0, 0);
    while old_at < candidates.len() && new_at < unmatched.len() {
        let (source, target) = (candidates[old_at], unmatched[new_at]);
        let (old_left, new_left) = (candidates.len() - old_at, unmatched.len() - new_at);
        let score = outline_overlap(&old[source], &new[target]);
        if new_left > old_left && outline_overlap(&old[source], &new[unmatched[new_at + 1]]) > score {
            new_at += 1;
        } else if old_left > new_left && outline_overlap(&old[candidates[old_at + 1]], &new[target]) > score {
            old_at += 1;
        } else {
            if allow_replacements || score > 0 {
                originals[target] = Some(source);
                used[source] = true;
            }
            old_at += 1;
            new_at += 1;
        }
    }
}

fn outline_overlap(old: &str, new: &str) -> usize {
    let prefix = old.chars().zip(new.chars()).take_while(|(a, b)| a == b).count();
    let suffix = old.chars().rev().zip(new.chars().rev()).take_while(|(a, b)| a == b).count();
    prefix.max(suffix)
}

/// Reuse the ordinary text edit for a changed line that still shares a prefix or suffix. A new
/// line without shared characters starts with its surrounding paragraph's first run style.
fn preserve_outline_run_style(para: &mut deckcraft_model::text::Paragraph, base: &deckcraft_model::text::Paragraph, text: &str) {
    let old = base.text();
    let shares_text = !text.is_empty() && (old.chars().next() == text.chars().next() || old.chars().next_back() == text.chars().next_back());
    if shares_text {
        let mut body = TextBody { paragraphs: vec![base.clone()], ..Default::default() };
        ed::replace_text(&mut body, text);
        if let Some(updated) = body.paragraphs.pop() {
            para.runs = updated.runs;
        }
    } else {
        let props = base.runs.first().map_or(&base.end_props, |run| &run.props);
        for run in &mut para.runs {
            run.props.clone_from(props);
        }
    }
    para.props.clone_from(&base.props);
    para.end_props.clone_from(&base.end_props);
}

pub(super) fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").ok_or_else(|| bad("text.set", "missing `text`"))?.to_string();
    let id = match id_param(p, "id", "text.set")? {
        Some(i) => i,
        None => s.doc()?.selection.shapes.first().copied().ok_or_else(|| bad("text.set", "missing `id` (or select a shape)"))?,
    };
    let cell = p.get("cell").and_then(Value::as_array).and_then(|a| Some((a.first()?.as_u64()? as usize, a.get(1)?.as_u64()? as usize)));
    let inherited = inherited_aligns(s, id);
    let outline = bool_or(p, "outline", false);
    edit_shapes(s, &json!({"id": id}), "text.set", |sh| {
        let target = match (&mut sh.kind, cell) {
            (ShapeKind::Table(t), Some((r, c))) => t.cell_mut(r, c).map(|x| &mut x.text),
            (ShapeKind::Table(_), None) => None,
            _ => Some(sh.text.get_or_insert_with(TextBody::default)),
        };
        let Some(body) = target else { return Err(bad("text.set", "no text here (tables need `cell`)")) };
        let mut nb = TextBody::from_text(&text);
        if outline {
            replace_outline(body, nb, |para| promote_rtl_if_needed(para, &inherited));
            return Ok(());
        }
        let keep = body.paragraphs.first().and_then(|p| p.runs.first()).map(|r| r.props.clone()).unwrap_or_default();
        let levels: Vec<(u8, deckcraft_model::text::ParaProps)> = body.paragraphs.iter().map(|p| (p.level, p.props.clone())).collect();
        for (i, para) in nb.paragraphs.iter_mut().enumerate() {
            // Leading tabs set the outline level.
            if !apply_leading_tabs(para)
                && let Some((lv, pp)) = levels.get(i)
            {
                para.level = *lv;
                para.props = pp.clone();
            }
            promote_rtl_if_needed(para, &inherited);
            for r in &mut para.runs {
                r.props = keep.clone();
            }
        }
        body.paragraphs = nb.paragraphs;
        Ok(())
    })?;
    fit_text_box(s, id)?;
    ok()
}

pub(super) fn get(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let id = match id_param(p, "id", "text.get")? {
        Some(i) => i,
        None => match &st.selection.text {
            Some(t) => t.shape,
            None => st.selection.shapes.first().copied().ok_or_else(|| bad("text.get", "missing `id`"))?,
        },
    };
    let sh = st.shape(id).ok_or_else(|| bad("text.get", "no such shape"))?;
    let t = sh.text.clone().unwrap_or_default();
    Ok(json!({"text": t.text(), "paragraphs": t.paragraphs.iter().map(|p| json!({"level": p.level, "text": p.text()})).collect::<Vec<_>>()}))
}

#[cfg(test)]
mod select_paragraph_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn select_paragraph_huge_at() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "T", "body": "B"})).unwrap();
        s.execute("insert.textBox", &json!({"rect": [10, 10, 200, 50], "text": "hello"})).unwrap();
        s.execute("text.edit", &json!({})).unwrap();
        let r = s.execute("text.selectParagraph", &json!({"at": [u64::MAX, 0]}));
        assert!(r.is_ok(), "{r:?}");
        let t = s.doc().unwrap().selection.text.clone().unwrap();
        // Clamped to the last (only) paragraph, never past the end of the body.
        assert_eq!((t.anchor, t.caret), ((0, 0), (0, 5)));
    }
}

#[cfg(test)]
mod outline_tests {
    use super::{Session, edit_shapes};
    use serde_json::json;

    #[test]
    fn editing_outline_keeps_the_runs_and_levels_of_the_remaining_paragraphs() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        s.execute("insert.textBox", &json!({"rect": [10, 10, 200, 100], "text": "One\nTwo\nThree\nFour"})).unwrap();
        let id = s.doc().unwrap().selection.shapes[0];
        edit_shapes(&mut s, &json!({"id": id}), "text.set", |shape| {
            let paragraphs = &mut shape.text.as_mut().unwrap().paragraphs;
            paragraphs[0].runs[0].props.bold = Some(true);
            paragraphs[1].level = 1;
            let mut italic = paragraphs[1].runs[0].clone();
            italic.text = "wo".into();
            italic.props.italic = Some(true);
            paragraphs[1].runs[0].text = "T".into();
            paragraphs[1].runs.push(italic);
            paragraphs[2].props.margin_left = Some(4.5);
            Ok(())
        })
        .unwrap();
        let before = s.doc().unwrap().shape(id).unwrap().text.clone().unwrap().paragraphs;

        s.execute("text.set", &json!({"id": id, "text": "\tTwo\nThree\nFour", "outline": true})).unwrap();
        let after = &s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs;
        assert_eq!(after, &before[1..]);
    }

    #[test]
    fn editing_outline_matches_identically_named_lines_at_their_own_levels() {
        let (mut s, id) = outline_with_two_styles("Task\nTask", 1);
        let before = s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs[1].clone();

        s.execute("text.set", &json!({"id": id, "text": "\tTask", "outline": true})).unwrap();
        let after = &s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs;
        assert_eq!(after, &[before]);
    }

    fn outline_with_two_styles(text: &str, second_level: u8) -> (Session, deckcraft_model::ShapeId) {
        let mut s = Session::with_new();
        s.execute("insert.textBox", &json!({"rect": [10, 10, 200, 100], "text": text})).unwrap();
        let id = s.doc().unwrap().selection.shapes[0];
        edit_shapes(&mut s, &json!({"id": id}), "text.set", |shape| {
            let paragraphs = &mut shape.text.as_mut().unwrap().paragraphs;
            paragraphs[0].runs[0].props.bold = Some(true);
            paragraphs[1].level = second_level;
            paragraphs[1].runs[0].props.italic = Some(true);
            Ok(())
        })
        .unwrap();
        (s, id)
    }

    #[test]
    fn an_inserted_outline_line_does_not_consume_the_edited_paragraph() {
        let (mut s, id) = outline_with_two_styles("First\nSecond", 0);
        s.execute("text.set", &json!({"id": id, "text": "New\nFirst!\nSecond", "outline": true})).unwrap();
        let after = &s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs;
        assert_eq!(after[1].text(), "First!");
        assert!(after[1].runs.iter().all(|run| run.props.bold == Some(true) && run.props.italic.is_none()));
        assert_eq!(after[2].text(), "Second");
        assert_eq!(after[2].runs[0].props.italic, Some(true));
    }

    #[test]
    fn reindenting_one_duplicate_outline_line_does_not_swap_its_style() {
        let (mut s, id) = outline_with_two_styles("Task\nTask", 1);
        let mut expected = s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs.clone();
        expected[0].level = 1;
        s.execute("text.set", &json!({"id": id, "text": "\tTask\n\tTask", "outline": true})).unwrap();
        let after = &s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs;
        assert_eq!(after, &expected);
    }

    #[test]
    fn moving_an_outline_section_keeps_duplicate_styles_at_their_levels() {
        let mut s = Session::with_new();
        s.execute("insert.textBox", &json!({"rect": [10, 10, 200, 100], "text": "A\nTask\nB\nSection\nTask"})).unwrap();
        let id = s.doc().unwrap().selection.shapes[0];
        edit_shapes(&mut s, &json!({"id": id}), "text.set", |shape| {
            let paragraphs = &mut shape.text.as_mut().unwrap().paragraphs;
            paragraphs[1].level = 1;
            paragraphs[1].runs[0].props.bold = Some(true);
            paragraphs[3].level = 1;
            paragraphs[4].level = 2;
            paragraphs[4].runs[0].props.italic = Some(true);
            Ok(())
        })
        .unwrap();
        let mut expected = s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs.clone();
        expected.rotate_left(2);

        s.execute("text.set", &json!({"id": id, "text": "B\n\tSection\n\t\tTask\nA\n\tTask", "outline": true})).unwrap();
        let after = &s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs;
        assert_eq!(after, &expected);
    }

    fn outline_with_styled_children(text: &str, [first, second]: [usize; 2]) -> (Session, deckcraft_model::ShapeId) {
        let mut s = Session::with_new();
        s.execute("insert.textBox", &json!({"rect": [10, 10, 200, 100], "text": text})).unwrap();
        let id = s.doc().unwrap().selection.shapes[0];
        edit_shapes(&mut s, &json!({"id": id}), "text.set", |shape| {
            let paragraphs = &mut shape.text.as_mut().unwrap().paragraphs;
            paragraphs[first].level = 1;
            paragraphs[first].runs[0].props.bold = Some(true);
            paragraphs[first].props.margin_left = Some(4.5);
            paragraphs[second].level = 1;
            paragraphs[second].runs[0].props.italic = Some(true);
            paragraphs[second].props.margin_left = Some(8.0);
            Ok(())
        })
        .unwrap();
        (s, id)
    }

    #[test]
    fn moving_an_outline_section_keeps_identical_children_with_their_headings() {
        let (mut s, id) = outline_with_styled_children("A\nTask\nB\nTask", [1, 3]);
        let mut expected = s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs.clone();
        expected.rotate_left(2);

        s.execute("text.set", &json!({"id": id, "text": "B\n\tTask\nA\n\tTask", "outline": true})).unwrap();
        let after = &s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs;
        assert_eq!(after, &expected);
    }

    #[test]
    fn moving_an_outline_section_keeps_its_child_when_the_other_section_becomes_empty() {
        let (mut s, id) = outline_with_styled_children("A\nTask\nB\nTask", [1, 3]);
        let before = s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs.clone();

        s.execute("text.set", &json!({"id": id, "text": "B\n\tTask\nA", "outline": true})).unwrap();
        let after = &s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs;
        assert_eq!(after, &[before[2].clone(), before[3].clone(), before[0].clone()]);

        s.execute("text.set", &json!({"id": id, "text": "A\n\tTask\nB\n\tTask", "outline": true})).unwrap();
        let after = &s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs;
        assert_eq!(after[3], before[3], "B still owns its child when A gains one");
    }

    #[test]
    fn moving_an_outline_section_keeps_unchanged_children_that_swap_sections() {
        let (mut s, id) = outline_with_styled_children("A\nApple\nB\nZebra", [1, 3]);
        let before = s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs.clone();

        s.execute("text.set", &json!({"id": id, "text": "B\n\tApple\nA\n\tZebra", "outline": true})).unwrap();
        let after = &s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs;
        assert_eq!(after, &[before[2].clone(), before[1].clone(), before[0].clone(), before[3].clone()]);
    }

    #[test]
    fn moving_an_outline_section_recognizes_a_heading_that_also_occurs_as_a_leaf() {
        for (leaf_level, edited) in [(1, "B\n\tTask\nA\n\tTask\n\tB"), (0, "B\n\tTask\nA\n\tTask\nB")] {
            let (mut s, id) = outline_with_styled_children("A\nTask\nB\nB\nTask", [1, 4]);
            edit_shapes(&mut s, &json!({"id": id}), "text.set", |shape| {
                shape.text.as_mut().unwrap().paragraphs[2].level = leaf_level;
                Ok(())
            })
            .unwrap();
            let mut expected = s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs.clone();
            expected.rotate_left(3);

            s.execute("text.set", &json!({"id": id, "text": edited, "outline": true})).unwrap();
            let after = &s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs;
            assert_eq!(after, &expected, "{edited:?}");
        }
    }

    #[test]
    fn moving_an_outline_section_keeps_edited_child_styles_and_paragraph_properties() {
        for (original, edited, indices) in [
            ("A\nFirst task\nB\nSecond task", "B\n\tSecond task!\nA\n\tFirst task!", [3, 1]),
            ("A\nFirst task\nB\nSecond task", "A\n\tFirst task!\nB\n\tSecond task!", [1, 3]),
            ("Section\nFirst task\nSection\nSecond task", "Section\n\tSecond task!\nSection\n\tFirst task!", [1, 3]),
            ("A\nTask\nB\nTask draft", "B\n\tTask\nA\n\tTask!", [3, 1]),
            ("A\nApple\nB\nZebra", "B\n\tLion\nA\n\tPlum", [3, 1]),
        ] {
            let (mut s, id) = outline_with_styled_children(original, [1, 3]);
            let before = s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs.clone();

            s.execute("text.set", &json!({"id": id, "text": edited, "outline": true})).unwrap();
            let after = &s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().paragraphs;
            let children = edited.lines().filter_map(|line| line.strip_prefix('\t'));
            for ((paragraph, source), text) in [&after[1], &after[3]].into_iter().zip(indices).zip(children) {
                assert_eq!(paragraph.text(), text);
                assert_eq!(paragraph.props, before[source].props, "{edited:?}");
                assert!(paragraph.runs.iter().all(|run| run.props == before[source].runs[0].props), "{edited:?}");
            }
        }
    }
}
