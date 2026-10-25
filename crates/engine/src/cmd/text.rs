//! In-place text editing: entering/leaving text, typing, deleting, caret movement and selection.

use std::sync::Arc;

use deckcraft_geom::{Affine, Point, Rect};
use deckcraft_model::edit::{self as ed, Pos};
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::{Align, AutoFit, Hyperlink, Paragraph, Run, RunKind, RunProps, TextBody};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::{
    CommandSpec, NO_TEXT_POINT, SELECT_TEXT_OR_SHAPE, always, bad, bool_or, cmd, edit_shapes, has_doc, has_slide, has_text, id_param, ok, shape_of,
    str_param, xfrm_of,
};
use crate::{DocState, HistoryEntry, Result, Selection, Session, Target, TextSel, push_undo};

/// Catalogue text-editing, selection, autocorrect, font, IME, and surrounding-deletion commands
/// with their parameter docs and enabled predicates.
pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "font.used", "Fonts in Presentation", [], None, "{} → [family] (theme fonts resolved)", has_doc, fonts_used),
        cmd!(
            "font.replace",
            "Replace Fonts",
            ["Home", "Editing", "Replace"],
            None,
            "{from, to} — every explicit use of family `from`, theme fonts included → {replaced}",
            has_doc,
            replace_font
        ),
        cmd!(noundo "text.edit", "Edit Text", [], Some("Enter"), "{id?, at?: [paragraph, char], end?: bool, cell?: [row, col], notes?: bool}", has_slide, edit),
        cmd!(noundo "text.exit", "Stop Editing Text", [], None, "{}", has_doc, exit),
        cmd!(
            noundo "text.autocorrect",
            "AutoCorrect Options",
            ["Tools"],
            None,
            "{on?, twoInitials?, sentenceCaps?, days?, capsLock?, fractions?, ordinals?, lists?: bool, add?: [from, to], remove?: from} → the options and the replacement list",
            always,
            autocorrect_options
        ),
        cmd!("text.insert", "Type Text", [], None, "{text} (\\n = new paragraph, \\u000b = line break)", has_text, insert),
        // The input method's composition: shown in place, an undo step only once committed.
        CommandSpec {
            id: "text.compose",
            label: "Compose Text",
            menu: &[],
            shortcut: None,
            params: "{text, active?: [start, end]} — an input method's composition (preedit) at the insertion point; empty text cancels it",
            enabled: has_text,
            run: compose,
            journal: false,
            undoable: false,
        },
        cmd!(
            noundo "text.commit",
            "Commit Composition",
            [],
            None,
            "{text} — the input method's result replaces its composition, as one undo step",
            has_text,
            commit
        ),
        cmd!(
            "text.delete",
            "Delete Text",
            [],
            Some("Backspace"),
            "{dir?: backward|forward|wordBackward|wordForward} — the word directions are Ctrl+Backspace/Delete: they work from the start of a selection without deleting it",
            has_text,
            delete
        ),
        // An input method asks for text around the caret to be removed (macOS press-and-hold,
        // dictation, Wayland text-input). Internal: the keyboard and control protocol keep the
        // existing text.delete spelling. `bytes` says the counts are UTF-8 bytes (winit's
        // `Ime::DeleteSurrounding`), otherwise characters, both in the logical text
        // `ime_surrounding` publishes: a paragraph mark or soft line break is one `\n` byte, so a
        // range may cross paragraphs. The selection and pre-edit are kept; byte ranges round
        // inward to whole scalars. A deletion while composing stays in the composition's single
        // undo step.
        //
        // `undoable: false` is deliberate even though a non-composing deletion is undoable: the
        // handler pushes its own history entry from a snapshot taken before the composition is
        // replaced, so the generic post-run undo hook must not add a duplicate. `journal` stays
        // off for the same reason an input-method edit is not a replayable document command.
        CommandSpec {
            id: "text.deleteSurrounding",
            label: "Delete Surrounding Text",
            menu: &[],
            shortcut: None,
            params: "{before, after, bytes?: bool}",
            enabled: has_text,
            run: delete_surrounding,
            journal: false,
            undoable: false,
        },
        cmd!(noundo "text.move", "Move Insertion Point", [], None, "{to: left|right|up|down|wordLeft|wordRight|lineStart|lineEnd|paraStart|paraEnd|paraUp|paraDown|start|end, extend?: bool} — left/right are on screen: in a right-to-left paragraph left goes forward", has_text, move_caret),
        cmd!(noundo "text.select", "Select Text", [], None, "{anchor: [p, c], caret: [p, c]}", has_text, select),
        cmd!(noundo "text.selectAll", "Select All Text", [], None, "{}", has_text, select_all),
        cmd!(noundo "text.selectWord", "Select Word", [], None, "{at?: [p, c]}", has_text, select_word),
        cmd!(noundo "text.selectParagraph", "Select Paragraph", [], None, "{at?: [p, c]}", has_text, select_para),
        cmd!(
            "text.set",
            "Set Text",
            [],
            None,
            "{id?, text, cell?: [r, c]} — replaces the whole text, keeping the first run's formatting",
            has_slide,
            set
        ),
        cmd!(query "text.get", "Get Text", [], None, "{id?} → {text, paragraphs}", has_slide, get),
    ]
}

/// The text body a text selection edits.
pub fn body_of<'a>(st: &'a DocState, t: &TextSel) -> Option<&'a TextBody> {
    if t.notes {
        return st.current_slide().map(|s| &s.notes);
    }
    let sh = st.shape(t.shape)?;
    if let (Some((r, c)), ShapeKind::Table(tb)) = (t.cell, &sh.kind) {
        return tb.cell(r, c).map(|x| &x.text);
    }
    sh.text.as_ref()
}

fn body_mut<'a>(doc: &'a mut Presentation, sel: &Selection, t: &TextSel) -> Option<&'a mut TextBody> {
    if t.notes {
        return doc.slides.get_mut(sel.slide).map(|s| &mut Arc::make_mut(s).notes);
    }
    let shapes = crate::shapes_mut(doc, sel)?;
    let sh = deckcraft_model::find_shape_mut(shapes, t.shape)?;
    if let Some((r, c)) = t.cell {
        if let ShapeKind::Table(tb) = &mut sh.kind {
            return tb.cell_mut(r, c).map(|x| &mut x.text);
        }
        return None;
    }
    Some(sh.text.get_or_insert_with(TextBody::default))
}

/// Maximum number of UTF-8 bytes in an outgoing IME surrounding-text query.
///
/// This matches `egui::output::MAX_IME_SURROUNDING_TEXT_BYTES`; keeping the bound here lets the
/// engine answer without copying a whole body or document.
pub const IME_SURROUNDING_MAX_BYTES: usize = 4000;

/// Bounded committed text around an IME selection, in logical (not visual) order and with UTF-8
/// byte offsets. The current input-method composition (pre-edit) is removed from both the text
/// and the offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImeSurrounding {
    pub text: String,
    pub cursor: usize,
    pub anchor: usize,
}

/// How a soft line break reads in the IME view: a newline, like a paragraph break.
///
/// The model stores it as `\u{b}` ([`RunKind::Break`]); publishing that control character would
/// give the platform a text its own `ime_text` filter never produces, so its idea of the lines
/// and our offsets would disagree. Both are one byte, so offsets do not otherwise change.
fn ime_char(ch: char) -> char {
    if ch == '\u{b}' { '\n' } else { ch }
}

/// Committed characters of one paragraph (the pre-edit skipped), as `(char index, character)`.
fn committed_chars(p: &Paragraph, comp: Option<(usize, usize)>) -> impl Iterator<Item = (usize, char)> + '_ {
    p.runs
        .iter()
        .flat_map(|run| run.content().chars())
        .enumerate()
        .filter_map(move |(i, ch)| (!comp.is_some_and(|(s, e)| i >= s && i < e)).then_some((i, ime_char(ch))))
}

/// The same characters, newest first, without walking the paragraph from its start.
fn committed_chars_rev(p: &Paragraph, comp: Option<(usize, usize)>) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut starts = Vec::with_capacity(p.runs.len());
    let mut at = 0usize;
    for r in &p.runs {
        let n = r.char_len();
        starts.push((at, n));
        at += n;
    }
    p.runs.iter().zip(starts).rev().flat_map(move |(r, (start, n))| {
        let content = r.content();
        content.chars().rev().enumerate().filter_map(move |(k, ch)| {
            let i = start + n - 1 - k;
            (!comp.is_some_and(|(s, e)| i >= s && i < e)).then_some((i, ime_char(ch)))
        })
    })
}

/// The composition's `(paragraph, start, end)` character range, when it is inside the body.
fn composition_range(body: &TextBody, c: Option<&crate::Composition>) -> Option<(usize, usize, usize)> {
    let c = c?;
    let para = body.paragraphs.get(c.start.0)?;
    let start = c.start.1.min(para.char_len());
    let end = start.saturating_add(c.len).min(para.char_len());
    (start < end).then_some((c.start.0, start, end))
}

/// Return the composition character range only when its paragraph index equals `pi`.
fn comp_in(comp: Option<(usize, usize, usize)>, pi: usize) -> Option<(usize, usize)> {
    comp.filter(|(p, _, _)| *p == pi).map(|(_, s, e)| (s, e))
}

/// Clip a model position to the body. An empty body only has position `(0, 0)`.
fn model_pos(body: &TextBody, pos: (usize, usize)) -> Option<(usize, usize)> {
    if body.paragraphs.is_empty() {
        return (pos == (0, 0)).then_some((0, 0));
    }
    body.paragraphs.get(pos.0).map(|p| (pos.0, pos.1.min(p.char_len())))
}

/// Walk forward from `from` over at most `limit` committed characters (or UTF-8 bytes of whole
/// characters), stopping at the end of the body. Returns the position reached and the characters
/// and bytes consumed.
///
/// The returned window is bounded by `limit`, but locating `from` inside a paragraph still walks
/// that paragraph's runs from their start: runs do not cache character lengths, and a position is
/// a character index. A megabyte paragraph therefore costs time proportional to the caret's
/// character offset, even though the copied surrounding text stays within `limit`.
fn walk_forward(
    body: &TextBody,
    comp: Option<(usize, usize, usize)>,
    from: (usize, usize),
    limit: usize,
    bytes: bool,
) -> ((usize, usize), usize, usize) {
    let (mut pi, mut at) = from;
    let (mut chars, mut used) = (0usize, 0usize);
    // `limit` comes from a (possibly hostile) request: every count is checked, never added
    // blindly, so an enormous limit cannot overflow.
    let fits = |used: usize, chars: usize, n: usize| if bytes { used.saturating_add(n) <= limit } else { chars < limit };
    while let Some(para) = body.paragraphs.get(pi) {
        for (i, ch) in committed_chars(para, comp_in(comp, pi)) {
            if i < at {
                continue;
            }
            let n = ch.len_utf8();
            if !fits(used, chars, n) {
                return ((pi, i), chars, used);
            }
            used += n;
            chars += 1;
            at = i + 1;
        }
        if pi + 1 >= body.paragraphs.len() {
            break;
        }
        // The paragraph separator between this paragraph and the next.
        if !fits(used, chars, 1) {
            return ((pi, at), chars, used);
        }
        used += 1;
        chars += 1;
        pi += 1;
        at = 0;
    }
    ((pi, at), chars, used)
}

/// Walk backward from `from` over at most `limit` committed characters (or UTF-8 bytes of whole
/// characters). Returns the position reached and the characters and bytes consumed.
fn walk_backward(
    body: &TextBody,
    comp: Option<(usize, usize, usize)>,
    from: (usize, usize),
    limit: usize,
    bytes: bool,
) -> ((usize, usize), usize, usize) {
    let (mut pi, mut at) = from;
    let (mut chars, mut used) = (0usize, 0usize);
    let fits = |used: usize, chars: usize, n: usize| if bytes { used.saturating_add(n) <= limit } else { chars < limit };
    loop {
        let Some(para) = body.paragraphs.get(pi) else { return ((pi, at), chars, used) };
        let mut boundary = at.min(para.char_len());
        for (i, ch) in committed_chars_rev(para, comp_in(comp, pi)) {
            if i >= boundary {
                continue;
            }
            let n = ch.len_utf8();
            if !fits(used, chars, n) {
                return ((pi, boundary), chars, used);
            }
            used += n;
            chars += 1;
            boundary = i;
        }
        // This paragraph is consumed down to its start.
        if pi == 0 {
            return ((0, 0), chars, used);
        }
        if !fits(used, chars, 1) {
            return ((pi, 0), chars, used);
        }
        used += 1;
        chars += 1;
        pi -= 1;
        at = usize::MAX; // all of the previous paragraph is before us
    }
}

/// The committed logical text between two model positions (excluding `end`). `None` when it is
/// longer than `cap` bytes: every caller only needs a bounded answer.
fn logical_between(body: &TextBody, comp: Option<(usize, usize, usize)>, start: (usize, usize), end: (usize, usize), cap: usize) -> Option<String> {
    let (a, b) = if start <= end { (start, end) } else { (end, start) };
    let mut out = String::new();
    for pi in a.0..=b.0 {
        let Some(para) = body.paragraphs.get(pi) else { return Some(out) };
        let from = if pi == a.0 { a.1 } else { 0 };
        let to = if pi == b.0 { b.1 } else { usize::MAX };
        for (i, ch) in committed_chars(para, comp_in(comp, pi)) {
            if i < from || i >= to {
                continue;
            }
            out.push(ch);
            if out.len() > cap {
                return None;
            }
        }
        if pi < b.0 {
            out.push('\n');
            if out.len() > cap {
                return None;
            }
        }
    }
    Some(out)
}

/// Committed surrounding text for the text being edited.
///
/// Returns `None` when the body or selection is invalid, or when the selection alone is wider
/// than [`IME_SURROUNDING_MAX_BYTES`]. An empty body is `Some` with empty text, which is distinct
/// from an unavailable context. `cursor > anchor` is preserved for a reversed selection.
///
/// Only a bounded window is measured and copied: the query walks at most the budget out from the
/// selection on each side and does not copy the rest of the body. Finding the selection's
/// character offsets inside one paragraph still scans that paragraph's prefix, so the time is not
/// bounded independently of the paragraph length.
pub fn ime_surrounding(st: &DocState, t: &TextSel) -> Option<ImeSurrounding> {
    let body = body_of(st, t)?;
    let comp = composition_range(body, st.composition.as_ref());
    let caret = model_pos(body, t.caret)?;
    let anchor = model_pos(body, t.anchor)?;
    let (start, end) = if caret <= anchor { (caret, anchor) } else { (anchor, caret) };
    let selected = logical_between(body, comp, start, end, IME_SURROUNDING_MAX_BYTES)?;
    let selection_len = selected.len();
    let budget = IME_SURROUNDING_MAX_BYTES.saturating_sub(selection_len);
    let (_, _, before_available) = walk_backward(body, comp, start, budget, true);
    let (_, _, after_available) = walk_forward(body, comp, end, budget, true);
    let mut before = (budget / 2).min(before_available);
    let mut after = (budget - before).min(after_available);
    before = (budget - after).min(before_available);
    after = (budget - before).min(after_available);
    let (window_start, _, _) = walk_backward(body, comp, start, before, true);
    let (window_end, _, _) = walk_forward(body, comp, end, after, true);
    let before_text = logical_between(body, comp, window_start, start, budget)?;
    let after_text = logical_between(body, comp, end, window_end, budget)?;
    let before_len = before_text.len();
    let mut text = before_text;
    text.push_str(&selected);
    text.push_str(&after_text);
    // The budget arithmetic above already bounds this; refuse rather than publish more.
    if text.len() > IME_SURROUNDING_MAX_BYTES {
        return None;
    }
    // The caret and anchor are the selection's own endpoints, so their offsets inside the
    // window are the window's start plus either end of the selected text.
    let (cursor, anchor) = if caret >= anchor { (before_len + selection_len, before_len) } else { (before_len, before_len + selection_len) };
    Some(ImeSurrounding { text, cursor, anchor })
}

/// Edit the active text body and selection; refits auto-sizing text boxes afterwards.
pub(crate) fn edit_text<T>(s: &mut Session, f: impl FnOnce(&mut TextBody, &mut TextSel) -> T) -> Result<T> {
    let t0 = s.doc()?.selection.text.clone().ok_or_else(|| bad("text", NO_TEXT_POINT))?;
    // Materialise an inherited placeholder box before editing (so autofit can change it).
    let r = s.edit(|doc, sel| {
        let mut t = t0.clone();
        let body = body_mut(doc, sel, &t).ok_or_else(|| bad("text", "the text being edited is gone"))?;
        let r = f(body, &mut t);
        // The saved shrink-on-overflow scale was for the old text: fit the new text afresh. A
        // body whose file carried no `fontScale` keeps none (100 %) here; `fit_shrink_scale`
        // below fills in the computed scale when the new text overflows (#724).
        if let Some(AutoFit::Shrink { font_scale, line_reduction }) = &mut body.body.autofit {
            if font_scale.is_some() {
                *font_scale = Some(1.0);
            }
            *line_reduction = 0.0;
        }
        sel.text = Some(t);
        Ok(r)
    })?;
    if !t0.notes && t0.cell.is_none() {
        fit_text_box(s, t0.shape)?;
    }
    Ok(r)
}

/// Refit a shape after its text changed: remember the shrink-on-overflow scale the layout
/// computes (#724), then grow/shrink an auto-sizing text box (Resize shape to fit text) to its
/// text.
pub fn fit_text_box(s: &mut Session, id: ShapeId) -> Result<()> {
    fit_shrink_scale(s, id)?;
    let st = s.doc()?;
    let Some(sh) = st.shape(id).cloned() else { return Ok(()) };
    let Some(body) = sh.text.clone() else { return Ok(()) };
    let doc = st.doc.clone();
    let sel = st.selection.clone();
    let Some(ctx) = ctx_for(&doc, &sel) else { return Ok(()) };
    let bp = resolve::body(&ctx, &sh);
    if bp.autofit != Some(AutoFit::Shape) {
        return Ok(());
    }
    let x = xfrm_of(&doc, &sel, &sh);
    let geo = deckcraft_render::shape_geometry(&sh, x.w, x.h);
    let fields = deckcraft_text::NoFields;
    let mut nx = x;
    if bp.wrap == Some(false) {
        nx.w = deckcraft_text::fit_width(&ctx, &sh, &body, geo.text_rect, &fields).max(20.0);
    }
    let geo = deckcraft_render::shape_geometry(&sh, nx.w, nx.h);
    let need = deckcraft_text::fit_height(&ctx, &sh, &body, geo.text_rect, &fields);
    let extra = (nx.h - geo.text_rect.height()).max(0.0);
    nx.h = (need + extra).max(8.0);
    if (nx.h - x.h).abs() < 0.01 && (nx.w - x.w).abs() < 0.01 && sh.xfrm.is_some() {
        return Ok(());
    }
    // Change the box in place without an extra undo step (it is part of the edit).
    let st = s.doc_mut()?;
    let mut d = (*st.doc).clone();
    if let Some(list) = crate::shapes_mut(&mut d, &st.selection)
        && let Some(target) = deckcraft_model::find_shape_mut(list, id)
    {
        target.xfrm = Some(nx);
    }
    st.doc = Arc::new(d);
    st.revision += 1;
    Ok(())
}

/// Store the shrink-on-overflow scale the layout computes for `id`, so a save keeps
/// `fontScale`/`lnSpcReduction` in step with what DeckCraft draws (#724). Runs after a text
/// change: a body with no meaningful saved scale — a file's bare `<a:normAutofit/>` (`None`) or
/// the reset an edit leaves (`1.0`, no reduction) — is fitted afresh; a scale the file saved is
/// left as it is, as PowerPoint draws it.
fn fit_shrink_scale(s: &mut Session, id: ShapeId) -> Result<()> {
    let st = s.doc()?;
    let Some(sh) = st.shape(id).cloned() else { return Ok(()) };
    let Some(body) = sh.text.clone() else { return Ok(()) };
    let Some(AutoFit::Shrink { font_scale, line_reduction }) = body.body.autofit else { return Ok(()) };
    if !(font_scale.is_none() || (font_scale == Some(1.0) && line_reduction == 0.0)) {
        return Ok(());
    }
    let doc = st.doc.clone();
    let sel = st.selection.clone();
    let Some(ctx) = ctx_for(&doc, &sel) else { return Ok(()) };
    let x = xfrm_of(&doc, &sel, &sh);
    let tr = deckcraft_render::shape_geometry(&sh, x.w, x.h).text_rect;
    // Lay out as if the scale had just been reset, so the fit is computed even for `None` (which
    // is drawn at 100 % and only fitted again when its text changes).
    let mut probe = body.clone();
    probe.body.autofit = Some(AutoFit::Shrink { font_scale: Some(1.0), line_reduction: 0.0 });
    let opts = deckcraft_text::Opts { rect: tr, fields: &deckcraft_text::NoFields, prompt_color: None, no_shrink: false };
    let l = deckcraft_text::layout(&ctx, &sh, &probe, &opts);
    if l.font_scale >= 1.0 && l.line_reduction <= 0.0 {
        return Ok(());
    }
    // Change the body in place without an extra undo step (it is part of the edit).
    let st = s.doc_mut()?;
    let mut d = (*st.doc).clone();
    let stored = if let Some(list) = crate::shapes_mut(&mut d, &st.selection)
        && let Some(target) = deckcraft_model::find_shape_mut(list, id)
        && let Some(t) = target.text.as_mut()
        && let Some(AutoFit::Shrink { font_scale, line_reduction }) = &mut t.body.autofit
    {
        (*font_scale, *line_reduction) = (Some(l.font_scale), l.line_reduction);
        true
    } else {
        false
    };
    if stored {
        st.doc = Arc::new(d);
        st.revision += 1;
    }
    Ok(())
}

pub(crate) fn ctx_for<'a>(doc: &'a Presentation, sel: &Selection) -> Option<Ctx<'a>> {
    match sel.target {
        Target::Slides => doc.slides.get(sel.slide).and_then(|s| Ctx::for_slide(doc, s)),
        Target::Master { master } => doc.masters.get(master).map(|m| Ctx::for_master(doc, m)),
        t @ (Target::Handout | Target::NotesMaster) => crate::page_master(doc, t).map(|m| Ctx::for_master(doc, m)),
        Target::Layout { master, layout } => doc.masters.get(master).and_then(|m| m.layouts.get(layout).map(|l| Ctx::for_layout(doc, m, l))),
    }
}

/// Lay out the text being edited (for vertical caret movement and hit testing).
pub fn layout_for(st: &DocState, t: &TextSel) -> Option<deckcraft_text::TextLayout> {
    text_frame(st, t).map(|f| f.0)
}

/// The text being edited as it is drawn: its layout, and the transform from layout space to the
/// slide (flips don't mirror text, vertical text is turned). Shapes and table cells alike; the
/// caret, the text selection, hit testing and caret movement all use it. `None` for notes.
pub fn text_frame(st: &DocState, t: &TextSel) -> Option<(deckcraft_text::TextLayout, Affine)> {
    if t.notes {
        return None;
    }
    let sh = st.shape(t.shape)?;
    let ctx = ctx_for(&st.doc, &st.selection)?;
    let lay = |s: &Shape, body: &TextBody, rect: Rect| {
        deckcraft_text::layout(&ctx, s, body, &deckcraft_text::Opts { rect, fields: &deckcraft_text::NoFields, prompt_color: None, no_shrink: false })
    };
    if let Some((r, c)) = t.cell {
        // A cell's text is laid out in the cell's rect with the cell's margins, anchor and the
        // table style's text look, exactly as the table draws it.
        let placed = cell_on_slide(st, t.shape, r, c)?;
        return Some((placed.layout, placed.to_slide));
    }
    let body = sh.text.as_ref()?;
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let tr = deckcraft_render::shape_geometry(sh, x.w, x.h).text_rect;
    let l = lay(sh, body, tr);
    let m = deckcraft_render::text_transform(x.affine(), tr, l.rotation);
    Some((l, m))
}

/// The text position under slide point `p` in the text being edited (its shape or cell).
pub fn text_pos_at(st: &DocState, t: &TextSel, p: Point) -> Pos {
    match text_frame(st, t) {
        Some((l, m)) => {
            let h = l.hit(m.inverse() * p);
            (h.para, h.ch)
        }
        None => (0, 0),
    }
}

/// The insertion point on the slide: its line from top to bottom, in slide coordinates (flips and
/// vertical text as drawn). Text with no paragraph yet (a placeholder about to be typed in) is
/// measured as one empty paragraph, so the caret shows where typing begins. `None` for notes,
/// which are an egui text field.
pub fn caret_line(st: &DocState, t: &TextSel) -> Option<(deckcraft_geom::Point, deckcraft_geom::Point)> {
    if t.notes {
        return None;
    }
    if let Some((row, col)) = t.cell {
        let placed = cell_on_slide(st, t.shape, row, col)?;
        let aff = if t.at_line_end() { deckcraft_text::Affinity::Upstream } else { deckcraft_text::Affinity::Downstream };
        let (cx, top, bot) = placed.layout.caret_drawn(deckcraft_text::Pos { para: t.caret.0, ch: t.caret.1 }, aff)?;
        return Some((placed.to_slide * deckcraft_geom::Point::new(cx, top), placed.to_slide * deckcraft_geom::Point::new(cx, bot)));
    }
    let sh = st.shape(t.shape)?;
    let ctx = ctx_for(&st.doc, &st.selection)?;
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let geo = deckcraft_render::shape_geometry(sh, x.w, x.h);
    let mut body = sh.text.clone().unwrap_or_default();
    if body.paragraphs.is_empty() {
        body.paragraphs.push(deckcraft_model::text::Paragraph::default());
    }
    let l = deckcraft_text::layout(
        &ctx,
        sh,
        &body,
        &deckcraft_text::Opts { rect: geo.text_rect, fields: &deckcraft_text::NoFields, prompt_color: None, no_shrink: false },
    );
    // After End on a wrapped line the caret is drawn at that line's end.
    let aff = if t.at_line_end() { deckcraft_text::Affinity::Upstream } else { deckcraft_text::Affinity::Downstream };
    let (cx, top, bot) = l.caret_drawn(deckcraft_text::Pos { para: t.caret.0, ch: t.caret.1 }, aff)?;
    let m = deckcraft_render::text_transform(x.affine(), geo.text_rect, l.rotation);
    Some((m * deckcraft_geom::Point::new(cx, top), m * deckcraft_geom::Point::new(cx, bot)))
}

/// The colour of text typed at the caret. The canvas draws the caret in it, so the caret shows
/// wherever the text does: light on a dark slide, dark on a light one. `None` when that text
/// has no fill to take a colour from.
pub fn caret_color(st: &DocState, t: &TextSel) -> Option<deckcraft_color::Rgba> {
    if t.notes || t.cell.is_some() {
        return None;
    }
    let sh = st.shape(t.shape)?;
    let body = sh.text.as_ref()?;
    let ctx = ctx_for(&st.doc, &st.selection)?;
    let blank = Paragraph::default();
    let para = body.paragraphs.get(t.caret.0).unwrap_or(&blank);
    let props = resolve::run(&ctx, sh, para, &ed::props_at(body, t.caret));
    Some(resolve::text_color(&ctx, &props)).filter(|c| c.a > 0)
}

fn pos_param(p: &Value, key: &str) -> Option<Pos> {
    let a = p.get(key)?.as_array()?;
    Some((a.first()?.as_u64()? as usize, a.get(1)?.as_u64()? as usize))
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    if bool_or(p, "notes", false) {
        let end = s.doc()?.current_slide().map(|x| ed::end(&x.notes)).unwrap_or((0, 0));
        s.select(|_, sel| {
            sel.text = Some(TextSel { notes: true, anchor: end, caret: end, ..Default::default() });
            sel.shapes.clear();
            sel.cells = None;
        })?;
        return ok();
    }
    let id = match id_param(p, "id", "text.edit")? {
        Some(i) => i,
        None => s.doc()?.selection.shapes.first().copied().ok_or_else(|| bad("text.edit", "select a shape with text"))?,
    };
    let sh = shape_of(s, id)?;
    let cell = p.get("cell").and_then(Value::as_array).and_then(|a| Some((a.first()?.as_u64()? as usize, a.get(1)?.as_u64()? as usize)));
    let body = match (&sh.kind, cell) {
        (ShapeKind::Table(t), Some((r, c))) => t.cell(r, c).map(|x| x.text.clone()).ok_or_else(|| bad("text.edit", "no such cell"))?,
        (ShapeKind::Table(_), None) => return Err(bad("text.edit", "say which `cell` of the table")),
        (ShapeKind::Shape | ShapeKind::Connector { .. }, _) => sh.text.clone().unwrap_or_default(),
        _ => return Err(bad("text.edit", "this object doesn't hold text")),
    };
    let at = pos_param(p, "at").unwrap_or_else(|| ed::end(&body));
    // A shape without a text body gets one on first edit.
    if sh.text.is_none() && cell.is_none() {
        s.edit(|doc, sel| {
            if let Some(list) = crate::shapes_mut(doc, sel)
                && let Some(x) = deckcraft_model::find_shape_mut(list, id)
            {
                x.text = Some(TextBody::default());
            }
            Ok(())
        })?;
    }
    s.select(|_, sel| {
        sel.shapes = vec![id];
        sel.text = Some(TextSel { shape: id, anchor: at, caret: at, cell, notes: false, line_end: None });
        sel.cells = None;
    })?;
    Ok(json!({"id": id, "at": [at.0, at.1]}))
}

fn exit(s: &mut Session, _p: &Value) -> Result<Value> {
    s.select(|_, sel| {
        if let Some(t) = sel.text.take()
            && !t.notes
        {
            sel.shapes = vec![t.shape];
        }
    })?;
    ok()
}

/// Smart quotes and simple autocorrect applied to typed text.
fn typed(s: &Session, body: &TextBody, at: Pos, text: &str) -> String {
    if !s.prefs.smart_quotes || text.chars().count() != 1 {
        return text.to_string();
    }
    let prev = body.paragraphs.get(at.0).and_then(|p| p.text().chars().nth(at.1.wrapping_sub(1)));
    let opening = prev.is_none_or(|c| c.is_whitespace() || "([{".contains(c));
    match text {
        "\"" => (if opening { "“" } else { "”" }).to_string(),
        "'" => (if opening { "‘" } else { "’" }).to_string(),
        _ => text.to_string(),
    }
}

pub(crate) fn insert_text(s: &mut Session, text: &str, props: Option<RunProps>) -> Result<Value> {
    let inherited = inherited_aligns_for_edit(s);
    let text = {
        let st = s.doc()?;
        match &st.selection.text {
            Some(t) => {
                let body = body_of(st, t).cloned().unwrap_or_default();
                typed(s, &body, t.ordered().0, text)
            }
            None => text.to_string(),
        }
    };
    let pos = edit_text(s, |body, t| {
        let (a, b) = t.ordered();
        let at = ed::delete(body, a, b);
        let props = props.clone().or_else(|| if a != b { Some(ed::props_at(body, at)) } else { unlinked_at_link_end(body, at) });
        let c = ed::insert(body, at, &text, props);
        t.anchor = c;
        t.caret = c;
        let last = body.paragraphs.len().saturating_sub(1);
        for pi in at.0..=c.0.min(last) {
            if let Some(para) = body.paragraphs.get_mut(pi) {
                promote_rtl_if_needed(para, &inherited);
            }
        }
        c
    })?;
    autocorrect(s, text.chars().count() == 1)?;
    // AutoCorrect can move the caret: a replaced word, or a list lead it removed.
    let pos = s.doc()?.selection.text.as_ref().map_or(pos, |t| t.caret);
    Ok(json!({"caret": [pos.0, pos.1]}))
}

/// Per outline level (0..=8), whether shape `id` on the current slide already resolves an
/// alignment for its paragraphs (its own list style, the layout's or the master's). All `false`
/// when the shape can't be resolved (notes, a shape that is gone).
fn inherited_aligns(s: &Session, id: ShapeId) -> [bool; 9] {
    let mut out = [false; 9];
    let Ok(st) = s.doc() else { return out };
    let Some(sh) = st.shape(id) else { return out };
    let Some(ctx) = ctx_for(&st.doc, &st.selection) else { return out };
    for (lvl, set) in out.iter_mut().enumerate() {
        let para = deckcraft_model::text::Paragraph { level: lvl as u8, ..Default::default() };
        *set = resolve::para(&ctx, sh, &para).align.is_some();
    }
    out
}

/// The aligns [`inherited_aligns`] gives for the text being edited (none for notes).
fn inherited_aligns_for_edit(s: &Session) -> [bool; 9] {
    match s.doc().ok().and_then(|st| st.selection.text.clone()) {
        Some(t) if !t.notes => inherited_aligns(s, t.shape),
        _ => [false; 9],
    }
}

/// A paragraph with no direction of its own whose first strong character is right-to-left
/// (Arabic, Hebrew, Kurdish…) becomes right-to-left, and right-aligned when nothing (the
/// paragraph or what it inherits, `inherited`) sets its alignment.
fn promote_rtl_if_needed(para: &mut deckcraft_model::text::Paragraph, inherited: &[bool; 9]) {
    if para.props.rtl.is_none() && deckcraft_model::bidi::first_strong_is_rtl(&para.text()) {
        para.props.rtl = Some(true);
        let inherits_align = inherited.get(usize::from(para.level)).copied().unwrap_or(false);
        if para.props.align.is_none() && !inherits_align {
            para.props.align = Some(Align::Right);
        }
    }
}

/// Typing right after a hyperlink doesn't extend it (as in PowerPoint): at the end of a linked
/// run, the props to type with are the run's without the link. `None` elsewhere.
fn unlinked_at_link_end(body: &TextBody, at: Pos) -> Option<RunProps> {
    let mut props = ed::props_at(body, at);
    props.link.as_ref()?;
    let len = body.paragraphs.get(at.0).map_or(0, |p| p.text().chars().count());
    // props_at(c + 1) is the props of the character at c.
    if at.1 < len && ed::props_at(body, (at.0, at.1 + 1)).link == props.link {
        return None;
    }
    props.link = None;
    Some(props)
}

/// Built-in AutoCorrect replacements (common slips and symbols).
pub const AUTOCORRECT: &[(&str, &str)] = &[
    ("teh", "the"),
    ("adn", "and"),
    ("recieve", "receive"),
    ("recieved", "received"),
    ("seperate", "separate"),
    ("occured", "occurred"),
    ("occurence", "occurrence"),
    ("thier", "their"),
    ("wich", "which"),
    ("acheive", "achieve"),
    ("accross", "across"),
    ("adress", "address"),
    ("alot", "a lot"),
    ("appearence", "appearance"),
    ("arguement", "argument"),
    ("basicly", "basically"),
    ("beggining", "beginning"),
    ("beleive", "believe"),
    ("calender", "calendar"),
    ("commitee", "committee"),
    ("definately", "definitely"),
    ("dont", "don't"),
    ("doesnt", "doesn't"),
    ("didnt", "didn't"),
    ("isnt", "isn't"),
    ("embarass", "embarrass"),
    ("enviroment", "environment"),
    ("existance", "existence"),
    ("foward", "forward"),
    ("freind", "friend"),
    ("goverment", "government"),
    ("happend", "happened"),
    ("i", "I"),
    ("ive", "I've"),
    ("immediatly", "immediately"),
    ("independant", "independent"),
    ("knowlege", "knowledge"),
    ("libary", "library"),
    ("neccessary", "necessary"),
    ("necesary", "necessary"),
    ("noticable", "noticeable"),
    ("occassion", "occasion"),
    ("posession", "possession"),
    ("prefered", "preferred"),
    ("publically", "publicly"),
    ("realy", "really"),
    ("recomend", "recommend"),
    ("refered", "referred"),
    ("relevent", "relevant"),
    ("succesful", "successful"),
    ("tommorow", "tomorrow"),
    ("tomorow", "tomorrow"),
    ("truely", "truly"),
    ("untill", "until"),
    ("wierd", "weird"),
    ("youre", "you're"),
    ("(c)", "©"),
    ("(r)", "®"),
    ("(tm)", "™"),
    ("(e)", "€"),
    ("...", "…"),
    ("->", "→"),
    ("<-", "←"),
    ("=>", "⇒"),
    ("--", "–"),
];

const DAYS: &[&str] = &["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

/// The replacement for `word` from your entries, then the built-in list.
fn replacement(prefs: &crate::Prefs, word: &str) -> Option<String> {
    if let Some((_, to)) = prefs.ac_entries.iter().find(|(k, _)| k == word) {
        return Some(to.clone());
    }
    if let Some((_, to)) = prefs.ac_entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(word)) {
        return Some(to.clone());
    }
    let (k, to) = AUTOCORRECT.iter().find(|(k, _)| k.eq_ignore_ascii_case(word))?;
    if prefs.ac_removed.iter().any(|r| r.eq_ignore_ascii_case(k)) {
        return None;
    }
    // "i" only on its own, lower case.
    if *k == "i" && word != "i" {
        return None;
    }
    Some(if word.chars().next().is_some_and(char::is_uppercase) { ed::change_case(to, "sentence") } else { to.to_string() })
}

/// What a just-typed `word` becomes (`None`: unchanged). `sentence_start`: nothing but spaces
/// or a sentence's end (`.`, `!`, `?`) before it in the paragraph. `typing`: one character was
/// typed (the case rules apply only then).
pub fn correct_word(prefs: &crate::Prefs, word: &str, sentence_start: bool, typing: bool) -> Option<String> {
    let mut out = replacement(prefs, word);
    if typing && out.is_none() {
        let chars: Vec<char> = word.chars().collect();
        let letters = chars.iter().all(|c| c.is_alphabetic());
        if prefs.ac_fractions {
            out = match word {
                "1/2" => Some("½".into()),
                "1/4" => Some("¼".into()),
                "3/4" => Some("¾".into()),
                _ => None,
            };
        }
        if out.is_none() && letters && chars.len() >= 3 {
            let upper = |c: &char| c.is_uppercase();
            let rest_lower = chars.get(2..).is_some_and(|r| r.iter().all(|c| c.is_lowercase()));
            let rest_upper = chars.get(1..).is_some_and(|r| r.iter().all(upper));
            if prefs.ac_two_initials && chars.first().is_some_and(upper) && chars.get(1).is_some_and(upper) && rest_lower {
                out =
                    Some(chars.iter().enumerate().map(|(i, c)| if i == 1 { c.to_lowercase().collect::<String>() } else { c.to_string() }).collect());
            } else if prefs.ac_caps_lock && chars.first().is_some_and(|c| c.is_lowercase()) && rest_upper {
                out = Some(
                    chars
                        .iter()
                        .enumerate()
                        .map(|(i, c)| if i == 0 { c.to_uppercase().collect::<String>() } else { c.to_lowercase().collect() })
                        .collect(),
                );
            }
        }
        if out.is_none() && prefs.ac_days && DAYS.contains(&word) {
            out = Some(ed::change_case(word, "sentence"));
        }
    }
    if typing && prefs.ac_sentence_caps && sentence_start {
        let w = out.clone().unwrap_or_else(|| word.to_string());
        let mut cs = w.chars();
        if let Some(f) = cs.next()
            && f.is_lowercase()
            && w.chars().all(char::is_alphabetic)
        {
            out = Some(f.to_uppercase().chain(cs).collect());
        }
    }
    out.filter(|o| o != word)
}

/// After a space or punctuation, correct the word just typed.
fn autocorrect(s: &mut Session, typing: bool) -> Result<()> {
    if !s.prefs.autocorrect {
        return Ok(());
    }
    let st = s.doc()?;
    let Some(t) = st.selection.text.clone() else { return Ok(()) };
    let Some(body) = body_of(st, &t) else { return Ok(()) };
    let line: Vec<char> = body.paragraphs.get(t.caret.0).map(|p| p.text().chars().collect()).unwrap_or_default();
    let end = t.caret.1;
    if end == 0 || !line.get(end - 1).is_some_and(|c| c.is_whitespace() || ".,;:!?".contains(*c)) {
        return Ok(());
    }
    if typing && s.prefs.ac_lists {
        // A lead such as `i.` or `a.` is kept as typed, so the space after it starts its list.
        let lead: String = line.get(..end).map(|l| l.iter().collect()).unwrap_or_default();
        if auto_list_style(&lead).is_some() || start_auto_list(s, &t, &line, end)? {
            return Ok(());
        }
    }
    let mut word_end = end - 1;
    let mut start = word_end;
    while start > 0 && line.get(start - 1).is_some_and(|c| !c.is_whitespace()) {
        start -= 1;
    }
    // "word," and "word." also count: the punctuation was typed before the space.
    let mut word: String = line.get(start..word_end).map(|w| w.iter().collect()).unwrap_or_default();
    if replacement(&s.prefs, &word).is_none() {
        let trimmed = word.trim_end_matches(|c: char| ".,;:!?".contains(c));
        if !trimmed.is_empty() && trimmed.len() < word.len() && !word.ends_with("...") {
            word_end -= word.chars().count() - trimmed.chars().count();
            word = trimmed.to_string();
        }
    }
    let before = line.get(..start).map(|b| b.iter().rev().find(|c| !c.is_whitespace()).copied()).unwrap_or(None);
    let sentence_start = before.is_none_or(|c| ".!?".contains(c));
    // "word..." becomes "word…", the word corrected too.
    let ellipsis = word.chars().count() > 3 && word.ends_with("...") && replacement(&s.prefs, "...").is_some();
    let rep = if ellipsis {
        let stem = word.trim_end_matches('.');
        Some(format!("{}…", correct_word(&s.prefs, stem, sentence_start, typing).unwrap_or_else(|| stem.to_string())))
    } else {
        correct_word(&s.prefs, &word, sentence_start, typing)
    };
    let Some(rep) = rep else {
        if typing && s.prefs.ac_ordinals && is_ordinal(&word) {
            return superscript_suffix(s, &t, word_end);
        }
        return Ok(());
    };
    let delta = rep.chars().count() as isize - word.chars().count() as isize;
    let st = s.doc_mut()?;
    let mut d = (*st.doc).clone();
    let sel = st.selection.clone();
    if let Some(b) = body_mut(&mut d, &sel, &t) {
        ed::delete(b, (t.caret.0, start), (t.caret.0, word_end));
        ed::insert(b, (t.caret.0, start), &rep, None);
    }
    st.doc = Arc::new(d);
    let c = (t.caret.0, (end as isize + delta).max(0) as usize);
    if let Some(tt) = st.selection.text.as_mut() {
        tt.caret = c;
        tt.anchor = c;
    }
    st.revision += 1;
    Ok(())
}

/// A list AutoFormat started: the text being edited right after it, the typed lead with its
/// space, the paragraph's own formatting before, and the revision the change left.
#[derive(Clone, Debug)]
pub struct AutoList {
    sel: TextSel,
    typed: String,
    props: deckcraft_model::text::ParaProps,
    revision: u64,
}

/// The list a paragraph that starts with `lead` and a space or tab becomes, as the command and
/// parameters that make it: `*` •, `-` or `–` –, `--` or `—` ■, `>` ➢, `<>` ◆, and the numbering
/// styles `1.`, `1)`, `I.`, `i.`, `A.`, `a.` and `a)`.
fn auto_list_style(lead: &str) -> Option<(&'static str, Value)> {
    let bullet = |c: &str| Some(("format.bullets", json!({"on": true, "char": c})));
    let number = |scheme: &str| Some(("format.numbering", json!({"on": true, "scheme": scheme})));
    match lead {
        "*" => bullet("•"),
        "-" | "–" => bullet("–"),
        "--" | "—" => bullet("■"),
        ">" => bullet("➢"),
        "<>" => bullet("◆"),
        "1." => number("arabicPeriod"),
        "1)" => number("arabicParenR"),
        "I." => number("romanUcPeriod"),
        "i." => number("romanLcPeriod"),
        "A." => number("alphaUcPeriod"),
        "a." => number("alphaLcPeriod"),
        "a)" => number("alphaLcParenR"),
        _ => None,
    }
}

/// AutoFormat's automatic bulleted and numbered lists: when the caret's paragraph holds only a
/// list lead (`*`, `1.`…) before the space or tab just typed, the lead and the space are removed
/// and the paragraph becomes a list paragraph in that style. `false` when nothing changed.
fn start_auto_list(s: &mut Session, t: &TextSel, line: &[char], end: usize) -> Result<bool> {
    if t.is_range() || !line.get(end - 1).is_some_and(|c| *c == ' ' || *c == '\t') {
        return Ok(false);
    }
    let lead: String = line.get(..end - 1).map(|l| l.iter().collect()).unwrap_or_default();
    let Some((id, params)) = auto_list_style(&lead) else { return Ok(false) };
    let typed: String = line.get(..end).map(|l| l.iter().collect()).unwrap_or_default();
    let para = t.caret.0;
    let props = body_of(s.doc()?, t).and_then(|b| b.paragraphs.get(para)).map(|p| p.props.clone()).unwrap_or_default();
    edit_text(s, |body, tt| {
        ed::delete(body, (para, 0), (para, end));
        tt.anchor = (para, 0);
        tt.caret = (para, 0);
    })?;
    s.execute(id, &params)?;
    let st = s.doc_mut()?;
    if let Some(sel) = st.selection.text.clone() {
        st.auto_list = Some(AutoList { sel, typed, props, revision: st.revision });
    }
    Ok(true)
}

/// Backspace right after AutoFormat started a list: the paragraph gets back the typed lead and
/// space and the formatting it had. `false` when the last change was anything else.
fn undo_auto_list(s: &mut Session) -> Result<bool> {
    let st = s.doc_mut()?;
    let Some(a) = st.auto_list.take() else { return Ok(false) };
    if a.revision != st.revision || st.selection.text.as_ref() != Some(&a.sel) {
        return Ok(false);
    }
    edit_text(s, |body, t| {
        if let Some(p) = body.paragraphs.get_mut(a.sel.caret.0) {
            p.props = a.props.clone();
        }
        let c = ed::insert(body, a.sel.caret, &a.typed, None);
        t.anchor = c;
        t.caret = c;
    })?;
    Ok(true)
}

/// `word` is a number with the ordinal suffix it takes (1st, 22nd, 103rd, 11th), as AutoFormat
/// superscripts it.
fn is_ordinal(word: &str) -> bool {
    let digits = word.trim_end_matches(|c: char| c.is_ascii_lowercase());
    let suffix = word.get(digits.len()..).unwrap_or_default();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) || suffix.len() != 2 {
        return false;
    }
    let d = digits.as_bytes();
    let teen = d.len() >= 2 && d.get(d.len() - 2) == Some(&b'1');
    let want = match d.last() {
        _ if teen => "th",
        Some(b'1') => "st",
        Some(b'2') => "nd",
        Some(b'3') => "rd",
        _ => "th",
    };
    suffix == want
}

/// Superscript the two letters before `end` in the caret's paragraph (an ordinal's suffix).
fn superscript_suffix(s: &mut Session, t: &crate::TextSel, end: usize) -> Result<()> {
    let (a, b) = ((t.caret.0, end.saturating_sub(2)), (t.caret.0, end));
    let st = s.doc_mut()?;
    if body_of(st, t).is_none_or(|body| ed::props_at(body, b).baseline.unwrap_or(0.0) > 0.0) {
        return Ok(());
    }
    let mut d = (*st.doc).clone();
    let sel = st.selection.clone();
    if let Some(body) = body_mut(&mut d, &sel, t) {
        ed::format(body, a, b, &|r| r.baseline = Some(0.3));
    }
    st.doc = Arc::new(d);
    st.revision += 1;
    Ok(())
}

fn autocorrect_options(s: &mut Session, p: &Value) -> Result<Value> {
    let pr = &mut s.prefs;
    for (key, slot) in [
        ("on", &mut pr.autocorrect),
        ("twoInitials", &mut pr.ac_two_initials),
        ("sentenceCaps", &mut pr.ac_sentence_caps),
        ("days", &mut pr.ac_days),
        ("capsLock", &mut pr.ac_caps_lock),
        ("fractions", &mut pr.ac_fractions),
        ("ordinals", &mut pr.ac_ordinals),
        ("lists", &mut pr.ac_lists),
    ] {
        if let Some(v) = p.get(key).and_then(Value::as_bool) {
            *slot = v;
        }
    }
    if let Some(Value::Array(a)) = p.get("add") {
        let (Some(from), Some(to)) = (a.first().and_then(Value::as_str), a.get(1).and_then(Value::as_str)) else {
            return Err(bad("text.autocorrect", "`add` is [from, to]"));
        };
        let from = from.trim();
        if from.is_empty() || from.chars().any(char::is_whitespace) || from.chars().count() > 64 || to.chars().count() > 255 {
            return Err(bad("text.autocorrect", "`from` is one word (up to 64 characters), `to` up to 255"));
        }
        pr.ac_entries.retain(|(k, _)| k != from);
        pr.ac_removed.retain(|k| !k.eq_ignore_ascii_case(from));
        if pr.ac_entries.len() >= 2000 {
            return Err(bad("text.autocorrect", "the list is full"));
        }
        pr.ac_entries.push((from.to_string(), to.to_string()));
    }
    if let Some(from) = str_param(p, "remove") {
        let had = pr.ac_entries.len();
        pr.ac_entries.retain(|(k, _)| k != from);
        if had == pr.ac_entries.len()
            && AUTOCORRECT.iter().any(|(k, _)| k.eq_ignore_ascii_case(from))
            && !pr.ac_removed.iter().any(|r| r.eq_ignore_ascii_case(from))
        {
            pr.ac_removed.push(from.to_string());
        }
    }
    let list: Vec<(String, String)> = pr
        .ac_entries
        .iter()
        .cloned()
        .chain(
            AUTOCORRECT
                .iter()
                .filter(|(k, _)| {
                    !pr.ac_removed.iter().any(|r| r.eq_ignore_ascii_case(k)) && !pr.ac_entries.iter().any(|(e, _)| e.eq_ignore_ascii_case(k))
                })
                .map(|(k, v)| (k.to_string(), v.to_string())),
        )
        .collect();
    Ok(json!({
        "on": pr.autocorrect, "twoInitials": pr.ac_two_initials, "sentenceCaps": pr.ac_sentence_caps,
        "days": pr.ac_days, "capsLock": pr.ac_caps_lock, "fractions": pr.ac_fractions,
        "ordinals": pr.ac_ordinals, "lists": pr.ac_lists, "entries": list,
    }))
}

pub(crate) fn paste_body(s: &mut Session, src: &TextBody) -> Result<Value> {
    let inherited = inherited_aligns_for_edit(s);
    let pos = edit_text(s, |body, t| {
        let (a, b) = t.ordered();
        let at = ed::delete(body, a, b);
        let c = ed::insert_body(body, at, src);
        t.anchor = c;
        t.caret = c;
        let last = body.paragraphs.len().saturating_sub(1);
        for pi in at.0..=c.0.min(last) {
            if let Some(para) = body.paragraphs.get_mut(pi) {
                promote_rtl_if_needed(para, &inherited);
            }
        }
        c
    })?;
    Ok(json!({"caret": [pos.0, pos.1]}))
}

pub(crate) fn insert_field(s: &mut Session, field: &str, text: &str) -> Result<Value> {
    let field = field.to_string();
    let text = if text.is_empty() { "#".to_string() } else { text.to_string() };
    edit_text(s, move |body, t| {
        let (a, b) = t.ordered();
        let at = ed::delete(body, a, b);
        let props = ed::props_at(body, at);
        let src = TextBody {
            paragraphs: vec![deckcraft_model::text::Paragraph {
                runs: vec![Run { text: text.clone(), props, kind: RunKind::Field { field: field.clone() } }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let c = ed::insert_body(body, at, &src);
        t.anchor = c;
        t.caret = c;
    })?;
    ok()
}

/// Text an input method sends: no paragraph or line breaks (Enter is a key of its own).
fn ime_text(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// Is the composition still where it was put (the paragraph holds all of it)?
fn composition_fits(st: &DocState, c: &crate::Composition) -> bool {
    let Some(t) = &st.selection.text else { return false };
    let Some(body) = body_of(st, t) else { return false };
    body.paragraphs.get(c.start.0).is_some_and(|p| c.start.1.saturating_add(c.len) <= p.char_len())
}

/// `text.compose`: put an input method's preedit at the insertion point (replacing the selection)
/// or replace the composition with the new preedit. An empty preedit cancels: the presentation
/// goes back to how it was before the composition began.
fn compose(s: &mut Session, p: &Value) -> Result<Value> {
    let text = ime_text(str_param(p, "text").ok_or_else(|| bad("text.compose", "missing `text`"))?);
    let n = text.chars().count();
    let active = p.get("active").and_then(Value::as_array).and_then(|a| {
        let (x, y) = (a.first()?.as_u64()? as usize, a.get(1)?.as_u64()? as usize);
        (x < y).then_some((x.min(n), y.min(n)))
    });
    let current = s.doc()?.composition.clone();
    // A composition whose text was changed under it is kept as it is; a new one starts.
    let current = match current {
        Some(c) if !composition_fits(s.doc()?, &c) => {
            // The text changed under the pre-edit: the app settled it, so the platform's copy
            // must be cancelled too.
            s.settle_composition();
            None
        }
        c => c,
    };
    match current {
        None if text.is_empty() => Ok(json!({"composing": false})),
        None => {
            let st = s.doc()?;
            let (before, before_selection) = (st.doc.clone(), st.selection.clone());
            let (start, props) = edit_text(s, |body, t| {
                let (a, b) = t.ordered();
                let at = ed::delete(body, a, b);
                let props = if a != b { Some(ed::props_at(body, at)) } else { unlinked_at_link_end(body, at) };
                let c = ed::insert(body, at, &text, props.clone());
                t.anchor = c;
                t.caret = c;
                (at, props)
            })?;
            s.doc_mut()?.composition = Some(crate::Composition { start, len: n, active, props, before, before_selection });
            Ok(json!({"composing": true, "start": [start.0, start.1], "len": n}))
        }
        Some(c) if text.is_empty() => {
            let st = s.doc_mut()?;
            st.doc = c.before;
            st.selection = c.before_selection;
            st.composition = None;
            st.revision += 1;
            Ok(json!({"composing": false}))
        }
        Some(c) => {
            let (start, len, props) = (c.start, c.len, c.props.clone());
            edit_text(s, |body, t| {
                ed::delete(body, start, (start.0, start.1 + len));
                let e = ed::insert(body, start, &text, props);
                t.anchor = e;
                t.caret = e;
            })?;
            s.doc_mut()?.composition = Some(crate::Composition { len: n, active, ..c });
            Ok(json!({"composing": true, "start": [start.0, start.1], "len": n}))
        }
    }
}

/// `text.commit`: the input method's result replaces the composition (or is typed at the
/// insertion point when there is none) and the whole composition becomes one undo step.
fn commit(s: &mut Session, p: &Value) -> Result<Value> {
    let text = ime_text(str_param(p, "text").ok_or_else(|| bad("text.commit", "missing `text`"))?);
    if s.doc()?.composition.is_some() || !text.is_empty() {
        compose(s, &json!({"text": text}))?;
    }
    s.end_composition();
    let caret = s.doc()?.selection.text.as_ref().map(|t| t.caret).unwrap_or_default();
    Ok(json!({"caret": [caret.0, caret.1]}))
}

/// Underlines (slide coordinates) under character ranges `(para, start, end)` of shape `id`'s
/// text as laid out on the current slide: where spelling squiggles go.
pub fn range_underlines(
    st: &DocState,
    id: deckcraft_model::ShapeId,
    ranges: &[(usize, usize, usize)],
) -> Vec<(deckcraft_geom::Point, deckcraft_geom::Point)> {
    let Some(sh) = st.shape(id) else { return vec![] };
    let Some(l) = layout_for(st, &TextSel { shape: id, ..Default::default() }) else { return vec![] };
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let tr = deckcraft_render::shape_geometry(sh, x.w, x.h).text_rect;
    let m = deckcraft_render::text_transform(x.affine(), tr, l.rotation);
    let mut out = vec![];
    for &(para, a, b) in ranges {
        for (x0, x1, y) in l.composition_underlines(deckcraft_text::Pos { para, ch: a }, deckcraft_text::Pos { para, ch: b }) {
            out.push((m * deckcraft_geom::Point::new(x0, y), m * deckcraft_geom::Point::new(x1, y)));
        }
    }
    out
}

/// Like [`range_underlines`], for the text of table `id`'s cell at `row`, `col`.
pub fn cell_range_underlines(
    st: &DocState,
    id: deckcraft_model::ShapeId,
    row: usize,
    col: usize,
    ranges: &[(usize, usize, usize)],
) -> Vec<(deckcraft_geom::Point, deckcraft_geom::Point)> {
    let Some(sh) = st.shape(id) else { return vec![] };
    let deckcraft_model::ShapeKind::Table(tb) = &sh.kind else { return vec![] };
    let (Some(cell), Some(ctx)) = (tb.cell(row, col), ctx_for(&st.doc, &st.selection)) else { return vec![] };
    let heights = crate::cmd::table_drawn_heights(&st.doc, &st.selection, tb);
    let Some(rect) = deckcraft_render::table_cell_rects(tb, &heights).into_iter().find(|(r, c, _)| *r == row && *c == col).map(|(_, _, r)| r) else {
        return vec![];
    };
    let body = deckcraft_render::table_cell_body(&ctx, tb, row, col, cell);
    let tmp = deckcraft_model::Shape::table_cell(body.clone());
    let l = deckcraft_text::layout(
        &ctx,
        &tmp,
        &body,
        &deckcraft_text::Opts { rect, fields: &deckcraft_text::NoFields, prompt_color: None, no_shrink: false },
    );
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let m = deckcraft_render::text_transform(x.affine(), rect, l.rotation);
    let mut out = vec![];
    for &(para, a, b) in ranges {
        for (x0, x1, y) in l.composition_underlines(deckcraft_text::Pos { para, ch: a }, deckcraft_text::Pos { para, ch: b }) {
            out.push((m * deckcraft_geom::Point::new(x0, y), m * deckcraft_geom::Point::new(x1, y)));
        }
    }
    out
}

/// Underlines under an input method's composition (#101), in slide coordinates: `(from, to,
/// thick)` per line, thick under the part being converted. Empty without a composition, or for
/// notes (an egui text field draws its own composition).
pub fn composition_underlines(st: &DocState) -> Vec<(deckcraft_geom::Point, deckcraft_geom::Point, bool)> {
    let (Some(c), Some(t)) = (&st.composition, &st.selection.text) else { return vec![] };
    if t.notes {
        return vec![];
    }
    if let Some((row, col)) = t.cell {
        let Some(placed) = cell_on_slide(st, t.shape, row, col) else { return vec![] };
        return underline_segments(&placed.layout, placed.to_slide, c);
    }
    let Some(sh) = st.shape(t.shape) else { return vec![] };
    let Some(l) = layout_for(st, t) else { return vec![] };
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let tr = deckcraft_render::shape_geometry(sh, x.w, x.h).text_rect;
    let m = deckcraft_render::text_transform(x.affine(), tr, l.rotation);
    underline_segments(&l, m, c)
}

fn underline_segments(
    layout: &deckcraft_text::TextLayout,
    to_slide: deckcraft_geom::Affine,
    c: &crate::Composition,
) -> Vec<(deckcraft_geom::Point, deckcraft_geom::Point, bool)> {
    let pos = |ch: usize| deckcraft_text::Pos { para: c.start.0, ch: c.start.1 + ch };
    let mut out = vec![];
    let mut add = |a: usize, b: usize, thick: bool| {
        for (x0, x1, y) in layout.composition_underlines(pos(a), pos(b)) {
            out.push((to_slide * deckcraft_geom::Point::new(x0, y), to_slide * deckcraft_geom::Point::new(x1, y), thick));
        }
    };
    add(0, c.len, false);
    if let Some((a, b)) = c.active {
        add(a, b.min(c.len), true);
    }
    out
}

/// A table cell's text laid out the way it is drawn, mapped into slide coordinates.
fn cell_on_slide(st: &DocState, id: ShapeId, row: usize, col: usize) -> Option<CellText> {
    let sh = st.shape(id)?;
    let ShapeKind::Table(tb) = &sh.kind else { return None };
    let cell = tb.cell(row, col)?;
    let ctx = ctx_for(&st.doc, &st.selection)?;
    let heights = super::table_drawn_heights(&st.doc, &st.selection, tb);
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

struct CellText {
    layout: deckcraft_text::TextLayout,
    to_slide: deckcraft_geom::Affine,
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
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
fn count_param(p: &Value, name: &str) -> usize {
    p.get(name).and_then(Value::as_u64).map_or(0, |v| usize::try_from(v).unwrap_or(usize::MAX))
}

/// Delete at most the requested character or byte counts around the selection or pre-edit,
/// preserving selection direction and composition boundaries and recording one undo step
/// outside composition.
fn delete_surrounding(s: &mut Session, p: &Value) -> Result<Value> {
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

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    delete_dir(s, str_param(p, "dir").unwrap_or("backward"))
}

/// Is paragraph `para` right-to-left (its lines run from the right)?
fn para_rtl(layout: Option<&deckcraft_text::TextLayout>, body: &TextBody, para: usize) -> bool {
    match layout.and_then(|l| l.lines.iter().find(|x| x.para == para)) {
        Some(line) => line.rtl,
        None => body.paragraphs.get(para).and_then(|x| x.props.rtl).unwrap_or(false),
    }
}

fn move_caret(s: &mut Session, p: &Value) -> Result<Value> {
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

fn select(s: &mut Session, p: &Value) -> Result<Value> {
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

fn select_all(s: &mut Session, _p: &Value) -> Result<Value> {
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

fn select_word(s: &mut Session, p: &Value) -> Result<Value> {
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

fn select_para(s: &mut Session, p: &Value) -> Result<Value> {
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

fn set(s: &mut Session, p: &Value) -> Result<Value> {
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

fn get(s: &mut Session, p: &Value) -> Result<Value> {
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

/// Apply run formatting to the text selection (or, when not editing, to all text of the
/// selected shapes). A caret without a range formats the word it's in (PowerPoint behaviour),
/// or sets the formatting for what is typed next at a word boundary.
pub(crate) fn format_run(s: &mut Session, f: &(dyn Fn(&mut RunProps) + Send + Sync)) -> Result<Value> {
    let st = s.doc()?;
    if let Some(t) = st.selection.text.clone() {
        let (a, b) = format_range(st, &t);
        let collapsed = a == b;
        edit_text(s, |body, _| {
            if collapsed {
                // Next typed text: put the props on an empty paragraph's end mark or keep pending.
                ed::format(body, a, b, f);
            } else {
                ed::format(body, a, b, f);
            }
        })?;
        if collapsed {
            // Remember as pending typing props by inserting nothing: handled via end mark for
            // empty paragraphs; mid-paragraph carets keep surrounding formatting.
        }
        return ok();
    }
    let ids = st.selection.shapes.clone();
    if ids.is_empty() {
        return Err(bad("format", SELECT_TEXT_OR_SHAPE));
    }
    let cells = st.selection.cells;
    edit_shapes(s, &json!({"ids": ids}), "format", |sh| {
        let id = sh.id;
        match &mut sh.kind {
            ShapeKind::Table(t) => {
                // Only the selected cells, when a range of them is selected.
                let range = super::table::selected_range(cells, id, t);
                for (r, row) in t.rows.iter_mut().enumerate() {
                    for (c, cell) in row.cells.iter_mut().enumerate() {
                        if super::table::in_range(range, r, c) {
                            ed::format_all(&mut cell.text, f);
                        }
                    }
                }
            }
            _ => {
                let body = sh.text.get_or_insert_with(TextBody::default);
                ed::format_all(body, f);
            }
        }
        Ok(())
    })?;
    for id in ids {
        fit_text_box(s, id)?;
    }
    ok()
}

/// What `format_run` formats while editing: the selection, or for a bare caret the word it's in
/// (without its trailing space). Empty when the caret is at a word boundary. Insert Hyperlink
/// edits this same span, so the dialog reads it too.
pub fn format_range(st: &DocState, t: &TextSel) -> (Pos, Pos) {
    let body = body_of(st, t).cloned().unwrap_or_default();
    let (mut a, mut b) = t.ordered();
    if a == b {
        let (wa, wb) = ed::word_at(&body, a);
        let inside = wa.1 < a.1 && a.1 < wb.1;
        if inside {
            a = wa;
            b = wb;
            // Don't include the trailing space.
            let line: Vec<char> = body.paragraphs.get(b.0).map(|p| p.text().chars().collect()).unwrap_or_default();
            while b.1 > a.1 && line.get(b.1 - 1) == Some(&' ') {
                b.1 -= 1;
            }
        }
    }
    (a, b)
}

/// Link the text being edited: the selection or the word at the caret, as `format_run` would.
/// With nothing to link (caret at a word boundary, empty paragraph) `display` is inserted at the
/// caret as linked text in the surrounding formatting, as PowerPoint's Insert Hyperlink does.
pub(crate) fn link_text(s: &mut Session, link: Hyperlink, display: &str) -> Result<Value> {
    let st = s.doc()?;
    let Some(t) = st.selection.text.clone() else { return Err(bad("insert.hyperlink", "not editing text")) };
    let (a, b) = format_range(st, &t);
    if a != b {
        return format_run(s, &move |rp| rp.link = Some(link.clone()));
    }
    if display.is_empty() {
        return Err(bad("insert.hyperlink", "nothing to link: select text or give `text`"));
    }
    let pos = edit_text(s, |body, t| {
        let mut props = ed::props_at(body, a);
        props.link = Some(link);
        let c = ed::insert(body, a, display, Some(props));
        t.anchor = c;
        t.caret = c;
        c
    })?;
    Ok(json!({"caret": [pos.0, pos.1]}))
}

/// Apply paragraph changes to the paragraphs touched by the selection (or all paragraphs of the
/// selected shapes).
pub(crate) fn format_para(s: &mut Session, f: &(dyn Fn(&mut deckcraft_model::text::Paragraph) + Send + Sync)) -> Result<Value> {
    let st = s.doc()?;
    if let Some(t) = st.selection.text.clone() {
        let (a, b) = t.ordered();
        edit_text(s, |body, _| {
            if body.paragraphs.is_empty() {
                body.paragraphs.push(Default::default());
            }
            let last = body.paragraphs.len().saturating_sub(1);
            for pi in a.0..=b.0.min(last) {
                if let Some(p) = body.paragraphs.get_mut(pi) {
                    f(p);
                }
            }
        })?;
        return ok();
    }
    let ids = st.selection.shapes.clone();
    if ids.is_empty() {
        return Err(bad("format", SELECT_TEXT_OR_SHAPE));
    }
    let cells = st.selection.cells;
    edit_shapes(s, &json!({"ids": ids}), "format", |sh| {
        let id = sh.id;
        match &mut sh.kind {
            ShapeKind::Table(t) => {
                let range = super::table::selected_range(cells, id, t);
                for (r, row) in t.rows.iter_mut().enumerate() {
                    for (c, cell) in row.cells.iter_mut().enumerate() {
                        if super::table::in_range(range, r, c) {
                            cell.text.paragraphs.iter_mut().for_each(f);
                        }
                    }
                }
            }
            _ => {
                let body = sh.text.get_or_insert_with(TextBody::default);
                if body.paragraphs.is_empty() {
                    body.paragraphs.push(Default::default());
                }
                body.paragraphs.iter_mut().for_each(f);
            }
        }
        Ok(())
    })?;
    for id in ids {
        fit_text_box(s, id)?;
    }
    ok()
}

/// Apply body (text box) changes to the edited or selected shapes.
pub(crate) fn format_body(s: &mut Session, f: &(dyn Fn(&mut deckcraft_model::text::BodyProps) + Send + Sync)) -> Result<Value> {
    let st = s.doc()?;
    let ids: Vec<ShapeId> = match &st.selection.text {
        Some(t) if !t.notes => vec![t.shape],
        _ => st.selection.shapes.clone(),
    };
    if ids.is_empty() {
        return Err(bad("format", "select a shape"));
    }
    edit_shapes(s, &json!({"ids": ids}), "format", |sh| {
        f(&mut sh.text.get_or_insert_with(TextBody::default).body);
        Ok(())
    })?;
    for id in ids {
        fit_text_box(s, id)?;
    }
    ok()
}

fn fonts_used(s: &mut Session, _: &Value) -> Result<Value> {
    let mut v = crate::fonts::used_fonts(&s.doc()?.doc);
    v.sort_by_key(|f| f.to_lowercase());
    Ok(json!(v))
}

fn replace_font(s: &mut Session, p: &Value) -> Result<Value> {
    let from = str_param(p, "from").map(str::trim).filter(|x| !x.is_empty()).ok_or_else(|| bad("font.replace", "missing `from`"))?.to_string();
    let to = str_param(p, "to").map(str::trim).filter(|x| !x.is_empty()).ok_or_else(|| bad("font.replace", "missing `to`"))?.to_string();
    if to.len() > 256 || to.chars().any(char::is_control) {
        return Err(bad("font.replace", "`to` is not a font name"));
    }
    // Count on a copy first so a replace that finds nothing leaves no undo step.
    let mut probe = (*s.doc()?.doc).clone();
    if crate::fonts::replace_font(&mut probe, &from, &to) == 0 {
        return Ok(json!({"replaced": 0}));
    }
    let n = s.edit(|doc, _| Ok(crate::fonts::replace_font(doc, &from, &to)))?;
    let st = s.doc_mut()?;
    st.missing_fonts = crate::fonts::missing_fonts(&st.doc);
    Ok(json!({"replaced": n}))
}

#[cfg(test)]
mod rtl_promote_tests {
    use deckcraft_model::text::{Align, Paragraph, Run};

    use super::promote_rtl_if_needed;

    fn para(text: &str) -> Paragraph {
        Paragraph { runs: vec![Run::new(text)], ..Default::default() }
    }

    #[test]
    fn right_aligns_only_when_no_alignment_is_set_or_inherited() {
        let mut p = para("مرحبا");
        promote_rtl_if_needed(&mut p, &[false; 9]);
        assert_eq!((p.props.rtl, p.props.align), (Some(true), Some(Align::Right)));
        let mut p = para("مرحبا");
        promote_rtl_if_needed(&mut p, &[true; 9]);
        assert_eq!((p.props.rtl, p.props.align), (Some(true), None));
        let mut p = para("Hello مرحبا");
        promote_rtl_if_needed(&mut p, &[false; 9]);
        assert_eq!((p.props.rtl, p.props.align), (None, None));
    }
}

#[cfg(test)]
mod ime_surrounding_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use serde_json::json;

    use crate::Session;

    /// Test helper: insert and enter a rectangle containing `aé😀b`, returning its shape id.
    fn edit(s: &mut Session) -> u64 {
        let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 240, 60], "text": "aé😀b"})).unwrap();
        let id = r["id"].as_u64().unwrap();
        s.execute("text.edit", &json!({"id": id})).unwrap();
        id
    }

    /// Test helper: return the identified shape's text, or an empty string when it is gone.
    fn text(s: &Session, id: u64) -> String {
        let id = deckcraft_model::ShapeId(u32::try_from(id).unwrap());
        s.doc().unwrap().shape(id).and_then(|sh| sh.text.as_ref()).map(deckcraft_model::text::TextBody::text).unwrap_or_default()
    }

    /// Test helper: set an in-paragraph text selection through the engine command path.
    fn select(s: &mut Session, anchor: usize, caret: usize) {
        s.execute("text.select", &json!({"anchor": [0, anchor], "caret": [0, caret]})).unwrap();
    }

    /// DeleteSurrounding removes only the requested text around the selection. The selection
    /// itself and its direction stay, so the following commit replaces it.
    #[test]
    fn selected_text_survives_the_requested_characters_or_bytes_around_it() {
        let mut s = Session::with_new();
        let id = edit(&mut s);
        select(&mut s, 3, 1); // reversed: caret before anchor, selecting é😀
        s.delete_surrounding(1, 1, false).unwrap();
        assert_eq!(text(&s, id), "é😀", "a and b go; the selected run stays");
        let t = s.doc().unwrap().selection.text.clone().unwrap();
        assert_eq!((t.anchor, t.caret), ((0, 2), (0, 0)), "both endpoints shift and stay reversed");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s, id), "aé😀b");

        select(&mut s, 1, 3);
        s.delete_surrounding(1, 1, true).unwrap();
        assert_eq!(text(&s, id), "é😀", "byte counts around the selection delete a and b too");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s, id), "aé😀b");
    }

    /// A byte range removes only whole scalars fully contained in it. A count that ends inside
    /// a character leaves that character whole (and undeleted), never splitting it.
    #[test]
    fn byte_counts_round_inward_to_whole_characters() {
        let mut s = Session::with_new();
        let id = edit(&mut s);
        select(&mut s, 2, 2); // after é: bytes 0..3
        s.delete_surrounding(1, 0, true).unwrap();
        assert_eq!(text(&s, id), "aé😀b", "one byte inside é is not the whole scalar");
        s.delete_surrounding(2, 0, true).unwrap();
        assert_eq!(text(&s, id), "a😀b", "two bytes reach é's start, so the whole é goes");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s, id), "aé😀b");

        select(&mut s, 2, 2);
        s.delete_surrounding(0, 1, true).unwrap();
        assert_eq!(text(&s, id), "aé😀b", "one byte into the emoji is not the whole scalar");
        s.delete_surrounding(0, 4, true).unwrap();
        assert_eq!(text(&s, id), "aéb", "four bytes contain the whole emoji");
    }

    /// The committed query is logical text with UTF-8 byte offsets; it keeps a reversed
    /// selection, removes the model-stored pre-edit, and treats an empty body as available.
    #[test]
    fn surrounding_text_is_committed_logical_and_bounded() {
        use super::{IME_SURROUNDING_MAX_BYTES, ime_surrounding};

        let mut s = Session::with_new();
        let id = edit(&mut s);
        let st = s.doc().unwrap();
        let t = st.selection.text.clone().unwrap();
        let q = ime_surrounding(st, &t).unwrap();
        assert_eq!((q.text.as_str(), q.cursor, q.anchor), ("aé😀b", 8, 8));

        s.execute("text.select", &json!({"anchor": [0, 3], "caret": [0, 1]})).unwrap();
        let st = s.doc().unwrap();
        let t = st.selection.text.clone().unwrap();
        let q = ime_surrounding(st, &t).unwrap();
        assert_eq!((q.cursor, q.anchor), (1, 7), "reversed selection keeps caret before anchor");
        assert_eq!(q.text, "aé😀b");

        s.execute("text.set", &json!({"id": id, "text": "ab"})).unwrap();
        s.execute("text.move", &json!({"to": "end"})).unwrap();
        s.compose_text("日本", None).unwrap();
        let st = s.doc().unwrap();
        let t = st.selection.text.clone().unwrap();
        let q = ime_surrounding(st, &t).unwrap();
        assert_eq!((q.text.as_str(), q.cursor, q.anchor), ("ab", 2, 2), "pre-edit is not committed text");

        s.execute("text.set", &json!({"id": id, "text": ""})).unwrap();
        let st = s.doc().unwrap();
        let t = st.selection.text.clone().unwrap();
        let q = ime_surrounding(st, &t).unwrap();
        assert_eq!((q.text.as_str(), q.cursor, q.anchor), ("", 0, 0), "empty body is available");

        let long = "a".repeat(IME_SURROUNDING_MAX_BYTES + 1500);
        s.execute("text.set", &json!({"id": id, "text": long})).unwrap();
        s.execute("text.move", &json!({"to": "end"})).unwrap();
        let st = s.doc().unwrap();
        let t = st.selection.text.clone().unwrap();
        let q = ime_surrounding(st, &t).unwrap();
        assert!(q.text.len() <= IME_SURROUNDING_MAX_BYTES, "{}", q.text.len());
        assert_eq!(q.cursor, q.text.len(), "caret stays at the end of the bounded window");
    }

    /// Paragraph marks are part of the logical text the platform sees (`\n`), so a deletion may
    /// remove one and merge the paragraphs, exactly as the published offsets say.
    #[test]
    fn paragraph_marks_are_deletable_logical_text() {
        use super::ime_surrounding;

        let mut s = Session::with_new();
        let id = edit(&mut s);
        s.execute("text.set", &json!({"id": id, "text": "ab\ncd"})).unwrap();
        // The caret at the start of the second paragraph: after "ab" and its paragraph mark.
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 0]})).unwrap();
        let st = s.doc().unwrap();
        let q = ime_surrounding(st, &st.selection.text.clone().unwrap()).unwrap();
        assert_eq!((q.text.as_str(), q.cursor, q.anchor), ("ab\ncd", 3, 3));

        // One byte before the caret is that paragraph mark.
        s.delete_surrounding(1, 0, true).unwrap();
        assert_eq!(text(&s, id), "abcd", "the mark goes and the paragraphs merge");
        let t = s.doc().unwrap().selection.text.clone().unwrap();
        assert_eq!(t.caret, (0, 2), "the caret stays where the merged text continues");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s, id), "ab\ncd");

        // Characters count the same mark.
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 0]})).unwrap();
        s.delete_surrounding(1, 0, false).unwrap();
        assert_eq!(text(&s, id), "abcd");
        s.execute("edit.undo", &json!({})).unwrap();

        // And from the other side: one character after the end of the first paragraph.
        s.execute("text.select", &json!({"anchor": [0, 2], "caret": [0, 2]})).unwrap();
        s.delete_surrounding(0, 1, false).unwrap();
        assert_eq!(text(&s, id), "abcd");
    }

    /// A soft line break (Shift+Enter) is published as a newline, like a paragraph break: the
    /// platform must never see the model's `\u{b}`, which its own text filter would reject.
    #[test]
    fn soft_line_breaks_read_as_newlines() {
        use super::ime_surrounding;

        let mut s = Session::with_new();
        let id = edit(&mut s);
        s.execute("text.autocorrect", &json!({"on": false})).unwrap(); // isolate logical line-break coordinates

        s.execute("text.set", &json!({"id": id, "text": "ab"})).unwrap();
        s.execute("text.select", &json!({"anchor": [0, 1], "caret": [0, 1]})).unwrap();
        s.execute("text.insert", &json!({"text": "\u{b}"})).unwrap();
        assert_eq!(text(&s, id), "a\u{b}b", "a line break inside one paragraph");
        let st = s.doc().unwrap();
        let q = ime_surrounding(st, &st.selection.text.clone().unwrap()).unwrap();
        assert_eq!((q.text.as_str(), q.cursor, q.anchor), ("a\nb", 2, 2));

        // The break is one logical byte, so removing it removes one character.
        s.delete_surrounding(1, 0, true).unwrap();
        assert_eq!(text(&s, id), "ab");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s, id), "a\u{b}b");
    }

    /// A count that does not fit a `usize` (32-bit wasm) is clamped, never truncated to a
    /// different number that would delete other text than the platform asked for.
    #[test]
    fn out_of_range_counts_clamp_instead_of_truncating() {
        let mut s = Session::with_new();
        let id = edit(&mut s);
        select(&mut s, 1, 1);
        s.delete_surrounding(usize::MAX, 0, false).unwrap();
        assert_eq!(text(&s, id), "é😀b", "everything before the caret goes, no panic");
        let expected = usize::try_from(4_294_967_296u64).unwrap_or(usize::MAX);
        assert_eq!(super::count_param(&json!({"before": 4_294_967_296u64}), "before"), expected);
        assert!(expected > 0, "the count is clamped, not wrapped to nothing");
        assert_eq!(super::count_param(&json!({}), "before"), 0);
    }

    /// A non-composing input-method deletion is exactly one undo step; the manual history entry
    /// must not be duplicated by the generic command wrapper.
    #[test]
    fn a_non_composing_ime_deletion_is_one_undo_step() {
        let mut s = Session::with_new();
        let id = edit(&mut s);
        let undo = s.doc().unwrap().history.undo.len();
        select(&mut s, 3, 3);
        s.delete_surrounding(1, 0, false).unwrap();
        assert_eq!(text(&s, id), "aéb");
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1, "one history entry, not two");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s, id), "aé😀b");
    }
}

#[cfg(test)]
mod replace_font_tests {
    use serde_json::json;

    use crate::Session;

    fn fonts(s: &mut Session) -> Vec<String> {
        serde_json::from_value(s.execute("font.used", &json!({})).unwrap()).unwrap()
    }

    #[test]
    fn replaces_runs_tables_notes_and_theme_and_undoes() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "T", "body": "B"})).unwrap();
        let theme = s.doc().unwrap().doc.masters[0].theme.fonts.minor.latin.clone();
        s.execute("insert.textBox", &json!({"rect": [10, 10, 200, 50], "text": "Hello"})).unwrap();
        s.execute("text.exit", &json!({})).unwrap();
        s.execute("format.font", &json!({"family": "Comic Neue"})).unwrap();
        assert!(fonts(&mut s).iter().any(|f| f == "Comic Neue"), "{:?}", fonts(&mut s));
        let r = s.execute("font.replace", &json!({"from": "comic neue", "to": "Lato"})).unwrap();
        assert!(r["replaced"].as_u64().unwrap() >= 1, "{r}");
        let used = fonts(&mut s);
        assert!(!used.iter().any(|f| f == "Comic Neue") && used.iter().any(|f| f == "Lato"), "{used:?}");
        // The theme's body font is replaced too, so placeholders follow.
        s.execute("font.replace", &json!({"from": theme, "to": "Lato"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.masters[0].theme.fonts.minor.latin, "Lato");
        assert!(s.doc().unwrap().doc.validate().is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(fonts(&mut s).iter().any(|f| f == "Comic Neue"));
    }

    #[test]
    fn nothing_to_replace_is_no_undo_step_and_bad_names_fail() {
        let mut s = Session::with_new();
        let undo = s.doc().unwrap().history.undo.len();
        assert_eq!(s.execute("font.replace", &json!({"from": "Nope Sans", "to": "Inter"})).unwrap()["replaced"], 0);
        assert_eq!(s.execute("font.replace", &json!({"from": "+mn-lt", "to": "Inter"})).unwrap()["replaced"], 0);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
        assert!(s.execute("font.replace", &json!({"from": "", "to": "Inter"})).is_err());
        assert!(s.execute("font.replace", &json!({"from": "Inter", "to": "a\nb"})).is_err());
        assert!(s.execute("font.replace", &json!({"from": "Inter"})).is_err());
    }
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
