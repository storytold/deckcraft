use super::caret::cell_on_slide;
use super::{
    Affine, Arc, AutoFit, Ctx, DocState, Paragraph, Point, Pos, Presentation, Rect, Result, Selection, Session, Shape, ShapeId, Target, TextBody,
    TextSel, Value, ed, resolve, xfrm_of,
};

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

pub(super) fn pos_param(p: &Value, key: &str) -> Option<Pos> {
    let a = p.get(key)?.as_array()?;
    Some((a.first()?.as_u64()? as usize, a.get(1)?.as_u64()? as usize))
}
