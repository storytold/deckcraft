use super::autocorrect::undo_auto_list;
use super::editing::{inherited_aligns, insert_text, promote_rtl_if_needed};
use super::ime::{composition_range, model_pos, walk_backward, walk_forward};
use super::layout::{ctx_for, fit_text_box, layout_for, pos_param};
use super::{
    DocState, HistoryEntry, Pos, Result, Session, ShapeId, ShapeKind, TextBody, TextSel, Value, bad, body_of, bool_or, ed, edit_shapes, edit_text,
    id_param, json, ok, push_undo, str_param, xfrm_of,
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
                "forward" => ed::char_move(body, a, true),
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

/// Is paragraph `para` of the text `t` addresses right-to-left on screen (the rule the arrow keys
/// use: Left goes forward in the text there)?
pub fn paragraph_is_rtl(st: &DocState, t: &TextSel, para: usize) -> bool {
    let body = body_of(st, t).cloned().unwrap_or_default();
    para_rtl(layout_for(st, t).as_ref(), &body, para)
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

pub(super) fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").ok_or_else(|| bad("text.set", "missing `text`"))?.to_string();
    let id = match id_param(p, "id", "text.set")? {
        Some(i) => i,
        None => s.doc()?.selection.shapes.first().copied().ok_or_else(|| bad("text.set", "missing `id` (or select a shape)"))?,
    };
    let cell = p.get("cell").and_then(Value::as_array).and_then(|a| Some((a.first()?.as_u64()? as usize, a.get(1)?.as_u64()? as usize)));
    let inherited = inherited_aligns(s, id);
    edit_shapes(s, &json!({"id": id}), "text.set", |sh| {
        let target = match (&mut sh.kind, cell) {
            (ShapeKind::Table(t), Some((r, c))) => t.cell_mut(r, c).map(|x| &mut x.text),
            (ShapeKind::Table(_), None) => None,
            _ => Some(sh.text.get_or_insert_with(TextBody::default)),
        };
        let Some(body) = target else { return Err(bad("text.set", "no text here (tables need `cell`)")) };
        let keep = body.paragraphs.first().and_then(|p| p.runs.first()).map(|r| r.props.clone()).unwrap_or_default();
        let levels: Vec<(u8, deckcraft_model::text::ParaProps)> = body.paragraphs.iter().map(|p| (p.level, p.props.clone())).collect();
        let mut nb = TextBody::from_text(&text);
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
