//! In-place text editing: entering/leaving text, typing, deleting, caret movement and selection.

use std::sync::Arc;

use deckcraft_geom::{Affine, Point, Rect};
use deckcraft_model::edit::{self as ed, Pos};
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::{Align, AutoFit, Hyperlink, Paragraph, Run, RunKind, RunProps, TextBody};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::*;
use crate::{DocState, Result, Selection, Session, Target, TextSel};

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
            "{on?, twoInitials?, sentenceCaps?, days?, capsLock?, fractions?, ordinals?: bool, add?: [from, to], remove?: from} → the options and the replacement list",
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
    // The text as the editor draws it (an equation being typed shows as its text).
    let Some(body) = sh.text.as_ref().map(|b| as_edited(st, &TextSel { shape: id, ..Default::default() }, b).into_owned()) else { return Ok(()) };
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

/// `body` (text `t` edits) as the editor draws it: the equation being typed shows as its text.
fn as_edited<'a>(st: &DocState, t: &TextSel, body: &'a TextBody) -> std::borrow::Cow<'a, TextBody> {
    match super::equation::shown(st, t) {
        Some(at) => std::borrow::Cow::Owned(deckcraft_text::with_linear_math(body, at)),
        None => std::borrow::Cow::Borrowed(body),
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
    let body = as_edited(st, t, sh.text.as_ref()?);
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let tr = deckcraft_render::shape_geometry(sh, x.w, x.h).text_rect;
    let l = lay(sh, &body, tr);
    let m = deckcraft_render::text_transform(x.affine(), tr, l.rotation);
    Some((l, m))
}

/// The text position under slide point `p` in the text being edited (its shape or cell).
pub fn text_pos_at(st: &DocState, t: &TextSel, p: Point) -> Pos {
    match text_frame(st, t) {
        Some((l, m)) => {
            let q = m.inverse() * p;
            let h = l.hit(q);
            // A click on an equation (not the one shown as text) goes into it, at its end.
            let shown = super::equation::shown(st, t);
            let under = |ahead: bool| {
                let (_, s, e) = ed::math_at(body_of(st, t)?, (h.para, h.ch), ahead)?;
                let hit = |li: &deckcraft_text::LineInfo| {
                    li.para == h.para && (li.top..li.bottom).contains(&q.y) && li.span(s, e).is_some_and(|x| (x.0..=x.1).contains(&q.x))
                };
                (shown != Some((h.para, s)) && l.lines.iter().any(hit)).then_some(e)
            };
            (h.para, under(true).or_else(|| under(false)).unwrap_or(h.ch))
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
    let mut body = as_edited(st, t, sh.text.as_ref().unwrap_or(&TextBody::default())).into_owned();
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
        sel.text = Some(TextSel { shape: id, anchor: at, caret: at, cell, ..Default::default() });
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
    let (text, math) = {
        let st = s.doc()?;
        match &st.selection.text {
            Some(t) => {
                let body = body_of(st, t).cloned().unwrap_or_default();
                // An equation is typed as it is: no smart quotes, no AutoCorrect.
                match super::equation::at(&body, t) {
                    Some(m) if !text.contains(['\n', '\u{b}']) => (text.to_string(), Some(m)),
                    _ => (typed(s, &body, t.ordered().0, text), None),
                }
            }
            None => (text.to_string(), None),
        }
    };
    let pos = edit_text(s, |body, t| {
        if let Some(m) = math {
            let c = super::equation::type_in(body, t, m, &text);
            t.anchor = c;
            t.caret = c;
            return c;
        }
        // A new paragraph or line typed in an equation starts after it.
        if let Some((_, _, e)) = super::equation::at(body, t) {
            t.anchor = (t.caret.0, e);
            t.caret = t.anchor;
        }
        let (a, b) = t.ordered();
        let at = ed::delete(body, a, b);
        // Beside an equation (just left, or before one), text takes the text's formatting.
        let beside = a == b && (t.left_math == Some(at) || ed::math_at(body, at, true).is_some_and(|m| m.1 == at.1));
        let props = props.clone().or_else(|| match (a != b, beside) {
            (true, _) => Some(ed::props_at(body, at)),
            (_, true) => Some(super::equation::text_props(body, at)),
            _ => unlinked_at_link_end(body, at),
        });
        let c = ed::insert(body, at, &text, props);
        t.anchor = c;
        t.caret = c;
        for pi in at.0..=c.0 {
            if let Some(para) = body.paragraphs.get_mut(pi) {
                promote_rtl_if_needed(para, &inherited);
            }
        }
        c
    })?;
    if math.is_none() {
        autocorrect(s, text.chars().count() == 1)?;
    }
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
        "ordinals": pr.ac_ordinals, "entries": list,
    }))
}

pub(crate) fn paste_body(s: &mut Session, src: &TextBody) -> Result<Value> {
    let inherited = inherited_aligns_for_edit(s);
    let pos = edit_text(s, |body, t| {
        // Into an equation: as text on one line.
        if let Some(m) = super::equation::at(body, t) {
            let c = super::equation::type_in(body, t, m, &src.text().replace('\n', " "));
            t.anchor = c;
            t.caret = c;
            return c;
        }
        let (a, b) = t.ordered();
        let at = ed::delete(body, a, b);
        let c = ed::insert_body(body, at, src);
        t.anchor = c;
        t.caret = c;
        for pi in at.0..=c.0 {
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
            s.end_composition();
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
                if let Some((ri, ..)) = super::equation::at(body, t) {
                    let c = ed::math_replace(body, a.0, ri, a.1, b.1, &text);
                    t.anchor = c;
                    t.caret = c;
                    return (a, None);
                }
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
                let e = match ed::math_at(body, start, true).filter(|m| start.1 + len <= m.2) {
                    Some((ri, ..)) => ed::math_replace(body, start.0, ri, start.1, start.1 + len, &text),
                    None => {
                        ed::delete(body, start, (start.0, start.1 + len));
                        ed::insert(body, start, &text, props)
                    }
                };
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
    let tmp = deckcraft_model::Shape { text: Some(body.clone()), ..Default::default() };
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
    if let Some(at) = super::equation::shown(st, &TextSel { shape: id, cell: Some((row, col)), ..Default::default() }) {
        body = deckcraft_text::with_linear_math(&body, at);
    }
    if body.paragraphs.is_empty() {
        body.paragraphs.push(deckcraft_model::text::Paragraph::default());
    }
    let tmp = deckcraft_model::Shape { text: Some(body.clone()), ..Default::default() };
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
    let dir = dir.to_string();
    edit_text(s, move |body, t| {
        let (a, b) = t.ordered();
        // In an equation, deleting edits its text: Backspace the character before the caret,
        // Delete the one after it.
        let math = if a != b {
            super::equation::at(body, t).map(|m| (m.0, a.1, b.1))
        } else if dir == "backward" || dir == "wordBackward" {
            let from = |s: usize| if dir == "wordBackward" { ed::word_move(body, a, false).1.max(s) } else { a.1 - 1 };
            ed::math_at(body, a, false).filter(|m| a.1 > m.1).map(|(ri, s, _)| (ri, from(s), a.1))
        } else {
            let to = |e: usize| if dir == "wordForward" { ed::word_move(body, a, true).1.min(e) } else { a.1 + 1 };
            ed::math_at(body, a, true).map(|(ri, _, e)| (ri, a.1, to(e)))
        };
        if let Some((ri, x, y)) = math {
            let c = ed::math_replace(body, a.0, ri, x, y, "");
            t.anchor = c;
            t.caret = c;
            return;
        }
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
    // Right Arrow at an equation's end leaves it without moving; Left Arrow there goes back in.
    if !extend && !t.is_range() {
        let leave = to == "right" && super::equation::at(&body, &t).is_some_and(|m| m.2 == caret.1);
        let back = to == "left" && t.left_math == Some(caret);
        if leave || back {
            s.select(|_, sel| {
                if let Some(tt) = sel.text.as_mut() {
                    tt.left_math = leave.then_some(caret);
                }
            })?;
            return Ok(json!({"caret": [caret.0, caret.1], "inEquation": back}));
        }
    }
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
                "down" => ((caret.0 + 1).min(body.paragraphs.len().saturating_sub(1)), caret.1),
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
            tt.left_math = None;
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
    let n = body.and_then(|b| b.paragraphs.get(at.0)).map(|x| x.char_len()).unwrap_or(0);
    // The paragraph mark is part of the paragraph (PowerPoint's triple-click selects it too).
    let end = if body.is_some_and(|b| at.0 + 1 < b.paragraphs.len()) { (at.0 + 1, 0) } else { (at.0, n) };
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
    edit_shapes(s, &json!({"ids": ids}), "format", |sh| {
        match &mut sh.kind {
            ShapeKind::Table(t) => {
                for row in &mut t.rows {
                    for c in &mut row.cells {
                        ed::format_all(&mut c.text, f);
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
    // In an equation: all of it.
    if let Some((_, s, e)) = super::equation::at(&body, t) {
        return ((t.caret.0, s), (t.caret.0, e));
    }
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
            for pi in a.0..=b.0 {
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
    edit_shapes(s, &json!({"ids": ids}), "format", |sh| {
        match &mut sh.kind {
            ShapeKind::Table(t) => {
                for row in &mut t.rows {
                    for c in &mut row.cells {
                        c.text.paragraphs.iter_mut().for_each(f);
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
