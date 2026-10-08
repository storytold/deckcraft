//! Laid-out text and click areas of a slide, in slide coordinates, without drawing — for exports
//! that put real text over a raster (PDF text layer, links) and for text extraction.

use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::{Hyperlink, TextBody};
use deckcraft_model::{Presentation, Shape, ShapeKind};
use deckcraft_text::{Opts, TextLayout};
use kurbo::{Affine, Rect};

use crate::{SlideFields, shape_geometry, table};

/// One shape's text as drawn on the slide.
pub struct PlacedText {
    pub layout: TextLayout,
    /// Shape-local → slide points (text rotation included).
    pub transform: Affine,
    pub body: TextBody,
}

/// A clickable area: a shape action or a text hyperlink.
pub struct PlacedLink {
    /// Slide-space bounding box.
    pub rect: Rect,
    pub link: Hyperlink,
}

#[derive(Default)]
pub struct Placed {
    pub texts: Vec<PlacedText>,
    pub links: Vec<PlacedLink>,
}

/// Text and links of slide `index` (master and layout graphics first, as rendered).
pub fn place_slide(pres: &Presentation, index: usize) -> Placed {
    let mut out = Placed::default();
    let Some(slide) = pres.slides.get(index) else { return out };
    let Some(rctx) = Ctx::for_slide(pres, slide) else { return out };
    let fields =
        SlideFields { num: pres.slide_number(index), date: pres.header_footer.date_text.clone(), footer: pres.header_footer.footer_text.clone() };
    let (show_layout, show_master) = resolve::show_master_shapes(slide, rctx.layout);
    if show_master {
        let mctx = Ctx::for_master(pres, rctx.master);
        for s in rctx.master.shapes.iter().filter(|s| s.ph.is_none()) {
            place(&mut out, &mctx, &fields, s, Affine::IDENTITY, 0);
        }
    }
    if show_layout && let Some(l) = rctx.layout {
        let lctx = Ctx::for_layout(pres, rctx.master, l);
        for s in l.shapes.iter().filter(|s| s.ph.is_none()) {
            place(&mut out, &lctx, &fields, s, Affine::IDENTITY, 0);
        }
    }
    for s in &slide.shapes {
        place(&mut out, &rctx, &fields, s, Affine::IDENTITY, 0);
    }
    out
}

fn place(out: &mut Placed, rctx: &Ctx, fields: &SlideFields, s: &Shape, parent: Affine, depth: usize) {
    if depth > 32 || s.hidden {
        return;
    }
    let x = resolve::xfrm(rctx, s);
    if !x.is_finite() {
        return;
    }
    let m = parent * x.affine();
    if let Some(link) = &s.click {
        out.links.push(PlacedLink { rect: m.transform_rect_bbox(Rect::new(0.0, 0.0, x.w, x.h)), link: link.clone() });
    }
    match &s.kind {
        ShapeKind::Group { children, child } => {
            let ch = Rect::new(child.x, child.y, child.x + child.w, child.y + child.h);
            let gm = parent * deckcraft_geom::group_child_affine(&x, ch);
            for c in children {
                place(out, rctx, fields, c, gm, depth + 1);
            }
        }
        ShapeKind::Table(t) => {
            // Cell text, laid out exactly as the table draws it.
            for (r, c, rect) in table::cell_rects(t, &table::row_heights(rctx, t, fields)) {
                let Some(cell) = t.cell(r, c).filter(|cell| !cell.text.is_empty()) else { continue };
                let body = table::cell_body(rctx, t, r, c, cell);
                let tmp = Shape { text: Some(body.clone()), ..Default::default() };
                place_text(out, rctx, fields, &tmp, &body, rect, m);
            }
        }
        ShapeKind::Chart(_) | ShapeKind::Ink { .. } => {}
        _ => {
            let Some(body) = s.text.as_ref().filter(|b| !b.is_empty()) else { return };
            let tr = shape_geometry(s, x.w, x.h).text_rect;
            place_text(out, rctx, fields, s, body, tr, m);
        }
    }
}

/// Lay out `body` in text rect `tr` of shape `s` (shape-local → slide `m`) and record it and its hyperlinks.
fn place_text(out: &mut Placed, rctx: &Ctx, fields: &SlideFields, s: &Shape, body: &TextBody, tr: Rect, m: Affine) {
    let layout = deckcraft_text::layout(rctx, s, body, &Opts { rect: tr, fields, prompt_color: None, no_shrink: false });
    let m = if layout.rotation != 0.0 {
        let c = tr.center().to_vec2();
        m * Affine::translate(c) * Affine::rotate(layout.rotation.to_radians()) * Affine::translate(-c)
    } else {
        m
    };
    // Text hyperlinks: one box per line segment of a linked run.
    for run in layout.runs.iter().filter(|r| r.link && !r.glyphs.is_empty()) {
        let Some(link) = run_link(body, run.para, run.chars.0) else { continue };
        let Some(line) = layout.lines.iter().find(|l| l.para == run.para && l.start <= run.chars.0 && run.chars.0 <= l.end) else { continue };
        let x0 = run.glyphs.first().map(|g| g.1).unwrap_or(0.0);
        let last = run.glyphs.last().map(|g| g.1).unwrap_or(x0);
        let x1 = line.caret_x.get(run.chars.1.saturating_sub(line.start)).copied().unwrap_or(last).max(last);
        let r = Rect::new(x0, line.top, x1, line.bottom);
        out.links.push(PlacedLink { rect: m.transform_rect_bbox(r), link });
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
