//! Laid-out text and click areas of a slide, in slide coordinates, without drawing — for exports
//! that put real text over a raster (PDF text layer, links) and for text extraction.

use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::{Hyperlink, TextBody};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind};
use deckcraft_text::{Opts, TextLayout};
use kurbo::{Affine, Rect};

use crate::{SlideFields, shape_geometry, table};

/// One shape's text as drawn on the slide.
pub struct PlacedText {
    pub layout: TextLayout,
    /// Shape-local → slide points (text rotation included, flips not: see [`crate::text_transform`]).
    pub transform: Affine,
    pub body: TextBody,
}

/// A clickable area: a shape action or a text hyperlink.
pub struct PlacedLink {
    /// Slide-space bounding box.
    pub rect: Rect,
    pub link: Hyperlink,
    /// The shape that owns the link (for a text hyperlink, the shape holding the text).
    pub shape: ShapeId,
    /// A hyperlink on text, not the shape's own action.
    pub text: bool,
}

/// A shape of the slide itself (not of its layout or master) with its slide-space bounding box:
/// what a click in a slide show lands on.
pub struct PlacedShape {
    pub id: ShapeId,
    /// Index into [`Placed::shapes`] of the group this shape is in.
    pub parent: Option<usize>,
    pub is_group: bool,
    /// Slide-space bounding box.
    pub rect: Rect,
    /// The shape's own action (Insert ▸ Action), not its text hyperlinks.
    pub link: Option<Hyperlink>,
}

#[derive(Default)]
pub struct Placed {
    pub texts: Vec<PlacedText>,
    pub links: Vec<PlacedLink>,
    /// The slide's own shapes in drawing order (back to front; a group before its children). Hidden
    /// shapes are left out.
    pub shapes: Vec<PlacedShape>,
}

/// Text and links of slide `index` (master and layout graphics first, as rendered).
pub fn place_slide(pres: &Presentation, index: usize) -> Placed {
    let mut out = Placed::default();
    let Some(slide) = pres.slides.get(index) else { return out };
    let Some(rctx) = Ctx::for_slide(pres, slide) else { return out };
    let fields = SlideFields::new(pres.slide_number(index), pres.header_footer.footer_text.clone(), &crate::RenderOpts::default());
    let (show_layout, show_master) = resolve::show_master_shapes(slide, rctx.layout);
    if show_master {
        let mctx = Ctx::for_master(pres, rctx.master);
        for s in rctx.master.shapes.iter().filter(|s| s.ph.is_none()) {
            place(&mut out, &mctx, &fields, s, Affine::IDENTITY, 0, None, false);
        }
    }
    if show_layout && let Some(l) = rctx.layout {
        let lctx = Ctx::for_layout(pres, rctx.master, l);
        for s in l.shapes.iter().filter(|s| s.ph.is_none()) {
            place(&mut out, &lctx, &fields, s, Affine::IDENTITY, 0, None, false);
        }
    }
    for s in &slide.shapes {
        place(&mut out, &rctx, &fields, s, Affine::IDENTITY, 0, None, true);
    }
    out
}

/// The header, date, footer and page-number text of a notes or handout `master`, laid out in its
/// placeholders (master points) with their own formatting and wrapping, for page `n`. The page
/// number placeholder shows `n` in its first run's formatting.
pub fn place_master_texts(pres: &Presentation, master: &deckcraft_model::Master, n: usize) -> Vec<PlacedText> {
    use deckcraft_model::PhType;
    let mut out = Placed::default();
    let ctx = Ctx::for_master(pres, master);
    let fields = SlideFields::new(u32::try_from(n).unwrap_or(u32::MAX), pres.header_footer.footer_text.clone(), &crate::RenderOpts::default());
    for s in master.shapes.iter().filter(|s| matches!(s.ph_type(), Some(PhType::Header | PhType::Date | PhType::Footer | PhType::SlideNum))) {
        let mut s = s.clone();
        if s.ph_type() == Some(PhType::SlideNum) {
            let mut body = s.text.clone().unwrap_or_default();
            let props =
                body.paragraphs.first().and_then(|p| p.runs.first().map(|r| r.props.clone()).or(Some(p.end_props.clone()))).unwrap_or_default();
            let mut para = body.paragraphs.first().cloned().unwrap_or_default();
            para.runs = vec![deckcraft_model::text::Run::with(n.to_string(), props)];
            body.paragraphs = vec![para];
            s.text = Some(body);
        }
        place(&mut out, &ctx, &fields, &s, Affine::IDENTITY, 0, None, false);
    }
    out.texts
}

/// `group` is the index in `out.shapes` of the enclosing group; `own` is true for the slide's own
/// shapes (recorded in `out.shapes`), false for the layout's and master's.
#[allow(clippy::too_many_arguments)]
fn place(out: &mut Placed, rctx: &Ctx, fields: &SlideFields, s: &Shape, parent: Affine, depth: usize, group: Option<usize>, own: bool) {
    if depth > 32 || s.hidden {
        return;
    }
    let x = resolve::xfrm(rctx, s);
    if !x.is_finite() {
        return;
    }
    let m = parent * x.affine();
    let rect = m.transform_rect_bbox(Rect::new(0.0, 0.0, x.w, x.h));
    if let Some(link) = &s.click {
        out.links.push(PlacedLink { rect, link: link.clone(), shape: s.id, text: false });
    }
    let me = if own {
        out.shapes.push(PlacedShape { id: s.id, parent: group, is_group: matches!(s.kind, ShapeKind::Group { .. }), rect, link: s.click.clone() });
        Some(out.shapes.len() - 1)
    } else {
        None
    };
    match &s.kind {
        ShapeKind::Group { children, child } => {
            let ch = Rect::new(child.x, child.y, child.x + child.w, child.y + child.h);
            let gm = parent * deckcraft_geom::group_child_affine(&x, ch);
            for c in children {
                place(out, rctx, fields, c, gm, depth + 1, me, own);
            }
        }
        ShapeKind::Table(t) => {
            // Cell text, laid out exactly as the table draws it.
            for (r, c, rect) in table::cell_rects(t, &table::row_heights(rctx, t, fields)) {
                let Some(cell) = t.cell(r, c).filter(|cell| !cell.text.is_empty()) else { continue };
                let body = table::cell_body(rctx, t, r, c, cell);
                let tmp = Shape::table_cell(body.clone());
                place_text(out, rctx, fields, s.id, &tmp, &body, rect, m);
            }
        }
        ShapeKind::Chart(_) | ShapeKind::Ink { .. } => {}
        _ => {
            let Some(body) = s.text.as_ref().filter(|b| !b.is_empty()) else { return };
            let tr = shape_geometry(s, x.w, x.h).text_rect;
            place_text(out, rctx, fields, s.id, s, body, tr, m);
        }
    }
}

/// Lay out `body` in text rect `tr` of shape `s` (shape-local → slide `m`) and record it and its hyperlinks.
#[allow(clippy::too_many_arguments)]
fn place_text(out: &mut Placed, rctx: &Ctx, fields: &SlideFields, owner: ShapeId, s: &Shape, body: &TextBody, tr: Rect, m: Affine) {
    let layout = deckcraft_text::layout(rctx, s, body, &Opts { rect: tr, fields, prompt_color: None, no_shrink: false });
    let m = crate::text_transform(m, tr, layout.rotation);
    // Text hyperlinks: one box per line segment of a linked run.
    for run in layout.runs.iter().filter(|r| r.link && !r.glyphs.is_empty()) {
        let Some(link) = run_link(body, run.para, run.chars.0) else { continue };
        let Some(line) = layout.lines.iter().find(|l| l.para == run.para && l.start <= run.chars.0 && run.chars.0 <= l.end) else { continue };
        // The run's visual extent (right-to-left runs run from right to left).
        let first = run.glyphs.first().map(|g| g.1).unwrap_or(0.0);
        let (x0, x1) = line.span(run.chars.0, run.chars.1).unwrap_or((first, first));
        let r = Rect::new(x0, line.top, x1, line.bottom);
        out.links.push(PlacedLink { rect: m.transform_rect_bbox(r), link, shape: owner, text: true });
    }
    out.texts.push(PlacedText { layout, transform: m, body: body.clone() });
}

/// The hyperlink of the model run containing character `at` of paragraph `para`.
fn run_link(body: &TextBody, para: usize, at: usize) -> Option<Hyperlink> {
    let p = body.paragraphs.get(para)?;
    let mut pos = 0;
    for r in &p.runs {
        let n = r.char_len();
        if at < pos + n.max(1) {
            return r.props.link.clone();
        }
        pos += n;
    }
    None
}

/// The outlines of shape `s`'s text as drawn, in the space `m` maps shape-local points to, and
/// the colour of its first visible run: what Merge Shapes turns letters into. Empty when the
/// shape has no text.
pub fn text_outline(rctx: &Ctx, s: &Shape, m: Affine) -> (kurbo::BezPath, Option<deckcraft_color::Rgba>) {
    let mut out = kurbo::BezPath::new();
    let Some(body) = s.text.as_ref().filter(|b| !b.is_empty()) else { return (out, None) };
    let x = resolve::xfrm(rctx, s);
    let tr = shape_geometry(s, x.w, x.h).text_rect;
    let layout = deckcraft_text::layout(rctx, s, body, &Opts { rect: tr, fields: &deckcraft_text::NoFields, prompt_color: None, no_shrink: false });
    let m = crate::text_transform(m, tr, layout.rotation);
    let mut first = None;
    for run in &layout.runs {
        let k = run.size / run.face.upem.max(1.0);
        let skew = if run.fake_italic { Affine::new([1.0, 0.0, -0.2, 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
        for (gid, gx, gy) in &run.glyphs {
            let path = crate::glyph(&run.face, *gid);
            if path.elements().is_empty() {
                continue;
            }
            if first.is_none() && run.color.a > 0 {
                first = Some(run.color);
            }
            let at = m * crate::glyph_place(run, *gid, *gx, *gy) * skew * Affine::scale(k);
            out.extend(path.elements().iter().map(|e| at * *e));
        }
    }
    (out, first)
}
