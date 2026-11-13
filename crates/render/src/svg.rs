//! SVG pictures (`image/svg+xml`): parsed by `deckcraft-svg` and drawn as vectors into a bitmap of
//! the size the slide needs, so they go through the same crop, tile and recolour path as any other
//! picture.
//!
//! The `viewBox` is fitted into the picture's own size the way SVG's default `preserveAspectRatio`
//! does (centred, both sides visible). Other `preserveAspectRatio` values, filters, masks and clip
//! paths are not applied.

use deckcraft_fonts::{FontDb, FontFace};
use deckcraft_svg::{Anchor, Gradient, GradientKind, Paint, Spread, Svg, Text, TextRun, Units};
use kurbo::{Affine, BezPath, Point, Rect, Shape as _};
use std::sync::Arc;
use vello_cpu::{RenderContext, Resources, peniko};

/// Largest side drawn.
const MAX_SIDE: u32 = 8192;

/// Does `bytes` look like an SVG picture?
pub fn is_svg(bytes: &[u8]) -> bool {
    deckcraft_svg::is_svg(bytes)
}

/// Draw the picture into straight RGBA with its longer side `side` px (transparent where it draws
/// nothing). `None` when the document doesn't parse.
pub fn rasterize(bytes: &[u8], side: u32) -> Option<image::RgbaImage> {
    let svg = deckcraft_svg::parse(bytes)?;
    let (w, h, m) = layout(&svg, side);
    let mut ctx = RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
    let r = Rect::new(0.0, 0.0, w as f64, h as f64);
    ctx.push_clip_layer(&r.to_path(0.1));
    let k = m.determinant().abs().sqrt();
    for it in &svg.items {
        if let Some(t) = &it.text {
            draw_text(&mut ctx, t, m);
            continue;
        }
        let path: BezPath = m * it.path.clone();
        let bb = path.bounding_box();
        if !(bb.x0.is_finite() && bb.y0.is_finite() && bb.x1.is_finite() && bb.y1.is_finite()) || bb.width() > 1e6 || bb.height() > 1e6 {
            continue;
        }
        let bbox = it.path.bounding_box();
        if let Some(f) = &it.fill {
            set_paint(&mut ctx, f, bbox, m);
            ctx.set_fill_rule(if it.even_odd { peniko::Fill::EvenOdd } else { peniko::Fill::NonZero });
            ctx.fill_path(&path);
            ctx.reset_paint_transform();
        }
        if let Some((c, pen)) = &it.stroke {
            set_paint(&mut ctx, c, bbox, m);
            ctx.set_stroke(pen_at(pen, k, &path));
            ctx.stroke_path(&path);
            ctx.reset_paint_transform();
        }
    }
    ctx.set_fill_rule(peniko::Fill::NonZero);
    ctx.pop_layer();
    ctx.flush();
    let mut px = vec![0u8; w as usize * h as usize * 4];
    if let Some(pm) = vello_cpu::PixmapMut::new(w, h, &mut px) {
        ctx.render(pm, &mut Resources::new());
    }
    // Premultiplied to straight.
    for p in px.as_chunks_mut::<4>().0 {
        let a = p[3] as u32;
        if a != 0 && a != 255 {
            for c in p.iter_mut().take(3) {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(w as u32, h as u32, px)
}

/// The picture drawn into a PNG whose longer side is `side` pixels, for the raster fallback an
/// exported package carries next to the vector. `None` when the document doesn't parse.
pub fn rasterize_png(bytes: &[u8], side: u32) -> Option<Vec<u8>> {
    let img = rasterize(bytes, side)?;
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).ok()?;
    Some(buf)
}

/// The bitmap size and the user-units → pixels transform for a target `side`.
fn layout(svg: &Svg, side: u32) -> (u16, u16, Affine) {
    let (pw, ph) = svg.size;
    let k = f64::from(side.clamp(16, MAX_SIDE)) / pw.max(ph).max(1e-9);
    let w = (pw * k).round().clamp(1.0, f64::from(MAX_SIDE)) as u16;
    let h = (ph * k).round().clamp(1.0, f64::from(MAX_SIDE)) as u16;
    let (x, y, uw, uh) = svg.view_box.map(|v| (v[0], v[1], v[2], v[3])).unwrap_or((0.0, 0.0, pw, ph));
    let s = (f64::from(w) / uw).min(f64::from(h) / uh);
    let tx = (f64::from(w) - uw * s) / 2.0 - x * s;
    let ty = (f64::from(h) - uh * s) / 2.0 - y * s;
    (w, h, Affine::translate((tx, ty)) * Affine::scale(s))
}

/// Paint one shape's fill or stroke: a solid colour, or a gradient laid out the way the picture
/// declared it. `m` maps user units to pixels and `bbox` is the shape's box in user units, which an
/// `objectBoundingBox` gradient takes as its unit square.
fn set_paint(ctx: &mut RenderContext, paint: &Paint, bbox: Rect, m: Affine) {
    match paint {
        Paint::Solid(c) => ctx.set_paint(peniko::Color::from_rgba8(c.r, c.g, c.b, c.a)),
        Paint::Gradient(g) => {
            let Some(grad) = peniko_gradient(g) else { return };
            // The gradient's own coordinates → user units → pixels.
            let to_user = match g.units {
                Units::ObjectBoundingBox => {
                    Affine::translate(bbox.origin().to_vec2())
                        * Affine::scale_non_uniform(bbox.width().max(1e-9), bbox.height().max(1e-9))
                        * g.transform
                }
                Units::UserSpace => g.transform,
            };
            ctx.set_paint(grad);
            ctx.set_paint_transform(m * to_user);
        }
    }
}

/// The peniko gradient for a parsed one. A single stop is doubled, so the ramp is solid instead of
/// empty, and the spread pads, repeats or reflects as the picture asks.
fn peniko_gradient(g: &Gradient) -> Option<peniko::Gradient> {
    let mut stops: Vec<peniko::ColorStop> =
        g.stops.iter().map(|(o, c)| peniko::ColorStop { offset: *o as f32, color: peniko::Color::from_rgba8(c.r, c.g, c.b, c.a).into() }).collect();
    let last = *stops.last()?;
    if stops.len() == 1 {
        stops.push(last);
    }
    let mut grad = match g.kind {
        GradientKind::Linear { x1, y1, x2, y2 } => peniko::Gradient::new_linear(Point::new(x1, y1), Point::new(x2, y2)).with_stops(stops.as_slice()),
        GradientKind::Radial { cx, cy, r, fx, fy } => {
            let (c, r) = (Point::new(cx, cy), r.max(1e-6) as f32);
            let f = Point::new(fx, fy);
            // A focus off the centre is a two-point ramp; outside the circle it cannot be, so the
            // circle's own centre stands in rather than a point that would draw nothing.
            if f != c && (f - c).hypot() < f64::from(r) {
                peniko::Gradient::new_two_point_radial(f, 0.0, c, r).with_stops(stops.as_slice())
            } else {
                peniko::Gradient::new_radial(c, r).with_stops(stops.as_slice())
            }
        }
    };
    grad.extend = match g.spread {
        Spread::Pad => peniko::Extend::Pad,
        Spread::Reflect => peniko::Extend::Reflect,
        Spread::Repeat => peniko::Extend::Repeat,
    };
    Some(grad)
}

/// Draw a text element: place its runs along the baselines they set, then draw the glyph outlines.
/// The reader has no fonts, so advances and anchors are resolved here.
fn draw_text(ctx: &mut RenderContext, t: &Text, m: Affine) {
    let to_device = m * t.transform;
    // A run that sets x or y opens a chunk. SVG anchors align each positioned chunk, so a chunk
    // is measured as a whole, while the pen keeps the coordinate the chunk did not set.
    ctx.set_fill_rule(peniko::Fill::NonZero);
    let mut pen = (0.0, 0.0);
    let mut i = 0;
    while i < t.runs.len() {
        let mut j = i + 1;
        while j < t.runs.len() && t.runs[j].at_x.is_none() && t.runs[j].at_y.is_none() {
            j += 1;
        }
        draw_chunk(ctx, &t.runs[i..j], to_device, &mut pen);
        i = j;
    }
    ctx.set_transform(Affine::IDENTITY);
    ctx.reset_paint_transform();
}

/// One placed glyph: its face, the size it is drawn at, and where on the baseline it sits.
struct Placed {
    face: Arc<FontFace>,
    size: f64,
    gid: u32,
    x: f64,
    y: f64,
    run: usize,
}

/// Draw one chunk of runs, advancing the pen from run to run and aligning the chunk by its anchor.
/// `pen` is the position the last chunk ended at, so a run that sets only `x` keeps the baseline.
fn draw_chunk(ctx: &mut RenderContext, runs: &[TextRun], to_device: Affine, pen: &mut (f64, f64)) {
    let (mut pen_x, mut pen_y) = *pen;
    let mut origin = pen_x;
    let mut placed: Vec<Placed> = Vec::new();
    let mut widths: Vec<f64> = vec![0.0; runs.len()];
    for (ri, r) in runs.iter().enumerate() {
        if let Some(x) = r.at_x {
            pen_x = x;
            origin = x;
        }
        if let Some(y) = r.at_y {
            pen_y = y;
        }
        pen_x += r.dx;
        pen_y += r.dy;
        let start = pen_x;
        // Characters the run's face does not have are drawn by a fallback face, so a label keeps
        // its whole text instead of losing the arrows and symbols in it.
        let mut run_glyphs: Vec<(Placed, f64)> = Vec::new();
        for (face, segment) in segments(r, &face_for(r)) {
            let k = r.size / face.upem.max(1.0);
            let shaped = deckcraft_fonts::shape(&face, segment, &[], |c| c);
            for g in &shaped {
                // Letter spacing is charged per glyph; a glyph of a multi-glyph cluster shares its
                // cluster's box and is only a fraction of an em out.
                run_glyphs.push((
                    Placed {
                        face: face.clone(),
                        size: r.size,
                        gid: g.gid,
                        x: f64::from(g.x_offset) * k,
                        y: pen_y - f64::from(g.y_offset) * k,
                        run: ri,
                    },
                    f64::from(g.x_advance) * k + r.spacing,
                ));
            }
        }
        // `shape` hands back a run in text order; a right-to-left run is laid out the other way,
        // so its glyphs are drawn from the last to the first.
        if is_rtl_text(&r.text) {
            run_glyphs.reverse();
        }
        for (mut p, advance) in run_glyphs {
            p.x += pen_x;
            placed.push(p);
            pen_x += advance;
        }
        widths[ri] = pen_x - start;
    }
    *pen = (pen_x, pen_y);
    // The space a run asks for after its last character is not part of the visible line, and the
    // anchor measures the chunk including every `dx` it sets.
    let trailing = runs.last().map(|r| r.spacing).unwrap_or(0.0);
    let total = pen_x - origin - trailing;
    let shift = match runs.first().map(|r| r.anchor).unwrap_or(Anchor::Start) {
        Anchor::Start => 0.0,
        Anchor::Middle => -total / 2.0,
        Anchor::End => -total,
    };
    for p in &placed {
        let run = &runs[p.run];
        if run.fill.is_none() && run.stroke.is_none() {
            continue;
        }
        let k = p.size / p.face.upem.max(1.0);
        let at = to_device * Affine::translate((p.x + shift, p.y)) * Affine::scale(k);
        // The same extent guard the shapes get: a glyph under an absurd transform is dropped
        // rather than flattened into device coordinates nothing can draw.
        let on_screen = at.determinant().abs().sqrt();
        if !(on_screen.is_finite() && on_screen > 1e-9 && on_screen < 1e6) {
            continue;
        }
        let Some(path) = glyph_path(&p.face, p.gid) else { continue };
        ctx.set_transform(at);
        if let Some(fill) = &run.fill {
            // A gradient over glyphs has no box to measure here: its first stop stands in.
            set_flat(ctx, fill);
            ctx.fill_path(path.as_ref());
        }
        // A label can be stroked without a fill, and then it still draws.
        if let Some((paint, stroke)) = &run.stroke {
            set_flat(ctx, paint);
            let k = k.max(1e-9);
            let pen = kurbo::Stroke::new((stroke.width / k).max(1e-4)).with_caps(stroke.start_cap).with_join(stroke.join);
            let pen =
                if stroke.dash_pattern.is_empty() { pen } else { pen.with_dashes(stroke.dash_offset / k, stroke.dash_pattern.iter().map(|d| d / k)) };
            ctx.set_stroke(pen);
            ctx.stroke_path(path.as_ref());
        }
    }
}

/// Whether a run reads right to left: it has a right-to-left character and no strong
/// left-to-right one. A mixed run keeps text order, which misplaces its right-to-left words.
fn is_rtl_text(text: &str) -> bool {
    let mut rtl = false;
    for c in text.chars() {
        if deckcraft_fonts::is_rtl(c) && c.is_alphabetic() {
            rtl = true;
        } else if c.is_alphabetic() && !deckcraft_fonts::is_rtl(c) {
            return false;
        }
    }
    rtl
}

/// Paint with a solid colour; a gradient falls back to its first stop.
fn set_flat(ctx: &mut RenderContext, paint: &Paint) {
    let c = match paint {
        Paint::Solid(c) => *c,
        Paint::Gradient(g) => g.stops.first().map(|(_, c)| *c).unwrap_or(deckcraft_color::Rgba::BLACK),
    };
    ctx.set_paint(peniko::Color::from_rgba8(c.r, c.g, c.b, c.a));
}

/// Split a run's text where its face does not cover the characters, so each stretch is shaped by
/// one face: the run's own, or the database's fallback for the character.
fn segments<'a>(r: &'a TextRun, face: &Arc<FontFace>) -> Vec<(Arc<FontFace>, &'a str)> {
    let db = FontDb::global();
    let mut out: Vec<(Arc<FontFace>, &'a str)> = Vec::new();
    let (mut start, mut current) = (0usize, face.clone());
    for (i, c) in r.text.char_indices() {
        let want = if face.covers(c) { face.clone() } else { db.fallback_for(c, face.id()).unwrap_or_else(|| face.clone()) };
        if !Arc::ptr_eq(&want, &current) {
            if start < i {
                out.push((current, &r.text[start..i]));
            }
            start = i;
            current = want;
        }
    }
    if start < r.text.len() {
        out.push((current, &r.text[start..]));
    }
    out
}

/// The outline of one glyph, or `None` when the face has none (a space, a missing glyph). The
/// outline is the shared, cached path, so a repeated glyph is not outlined again.
fn glyph_path(face: &FontFace, gid: u32) -> Option<Arc<BezPath>> {
    let p = crate::glyph(face, gid);
    (!p.elements().is_empty()).then_some(p)
}

/// The face a run asks for: the first family it names that is installed, then the closest style.
fn face_for(r: &TextRun) -> Arc<FontFace> {
    let db = FontDb::global();
    let style = match (r.bold, r.italic) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (false, false) => "Regular",
    };
    for family in &r.families {
        let family = generic_family(family);
        if db.has_family(family) {
            return db.face(family, style);
        }
    }
    db.face("", style)
}

/// CSS's generic families, named by the families the font database knows.
fn generic_family(name: &str) -> &str {
    match name.to_ascii_lowercase().as_str() {
        "sans-serif" | "cursive" | "fantasy" | "system-ui" => "Helvetica Neue",
        "serif" => "Times New Roman",
        "monospace" => "Courier New",
        _ => name,
    }
}

/// The pen at the layout scale (the picture scatters its own transforms into the geometry).
fn pen_at(p: &kurbo::Stroke, k: f64, path: &BezPath) -> kurbo::Stroke {
    let mut s = kurbo::Stroke::new((p.width * k).max(1e-4)).with_caps(p.start_cap).with_join(p.join).with_miter_limit(p.miter_limit);
    if !p.dash_pattern.is_empty() {
        // A dash shorter than a pixel would ask for unbounded dashes, as on any line.
        let pattern: Vec<f64> = p.dash_pattern.iter().map(|d| d * k).collect();
        s = s.with_dashes(p.dash_offset, crate::stroke::limit_dash_count(path, pattern));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAR: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24'>\
        <path d='M12 2 L15 9 L22 9 L16 14 L18 21 L12 17 L6 21 L8 14 L2 9 L9 9 Z' fill='#ff0000'/></svg>";

    #[test]
    fn draws_a_filled_path_as_a_vector() {
        let img = rasterize(STAR.as_bytes(), 96).expect("draws");
        assert_eq!(img.dimensions(), (96, 96));
        // The star's middle is filled red; its corner is not.
        let mid = img.get_pixel(48, 48);
        assert!(mid[0] > 200 && mid[1] < 60 && mid[3] == 255, "{mid:?}");
        assert_eq!(img.get_pixel(1, 1)[3], 0);
    }

    #[test]
    fn a_picture_becomes_a_png_for_the_export_fallback() {
        let png = rasterize_png(STAR.as_bytes(), 48).expect("rasterizes");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "a PNG");
        // The stars' colours survive the encode: decode it back and look at the middle.
        let img = image::load_from_memory(&png).expect("decodes").to_rgba8();
        assert_eq!(img.dimensions(), (48, 48));
        let mid = img.get_pixel(24, 24);
        assert!(mid[0] > 200 && mid[1] < 60, "{mid:?}");
    }

    #[test]
    fn keeps_the_pictures_aspect() {
        let img = rasterize(b"<svg width='40' height='10' viewBox='0 0 4 1'><rect width='4' height='1' fill='black'/></svg>", 80).expect("draws");
        assert_eq!(img.dimensions(), (80, 20));
    }

    #[test]
    fn a_tiny_dash_pattern_draws_instead_of_expanding() {
        let img = rasterize(
            b"<svg width='20' height='20' viewBox='0 0 20 20'><line x1='0' y1='10' x2='20' y2='10' stroke='black' stroke-width='2' stroke-dasharray='1e-9 1e-9'/></svg>",
            40,
        )
        .expect("draws");
        assert!(img.pixels().any(|p| p[3] > 0), "the dashing line paints something");
    }

    #[test]
    fn broken_documents_draw_nothing() {
        assert!(rasterize(b"not xml", 64).is_none());
        assert!(rasterize(b"<svg", 64).is_none());
    }

    #[test]
    fn a_gradient_fill_ramps_across_the_shape() {
        let img = rasterize(
            b"<svg width='100' height='20' viewBox='0 0 100 20'><defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='0'><stop stop-color='#ff0000'/><stop offset='1' stop-color='#0000ff'/></linearGradient></defs><rect width='100' height='20' fill='url(#g)'/></svg>",
            100,
        )
        .expect("draws");
        let left = img.get_pixel(3, 10);
        let mid = img.get_pixel(50, 10);
        let right = img.get_pixel(96, 10);
        assert!(left[0] > 200 && left[2] < 60, "left {left:?}");
        assert!(right[2] > 200 && right[0] < 60, "right {right:?}");
        assert!(mid[0] > 60 && mid[2] > 60, "mid {mid:?}");
    }

    #[test]
    fn text_is_drawn_at_the_anchor_it_names() {
        let ink = |img: &image::RgbaImage, x0: u32, x1: u32| {
            (0..img.height()).flat_map(|y| (x0..x1).map(move |x| (x, y))).any(|(x, y)| img.get_pixel(x, y)[3] > 128)
        };
        let start = rasterize(
            b"<svg width='100' height='40' viewBox='0 0 100 40'><text x='0' y='32' font-size='26' font-family='sans-serif' fill='#000000'>Hi</text></svg>",
            100,
        )
        .expect("draws");
        assert!(ink(&start, 0, 30), "the glyphs start at x = 0");
        assert!(!ink(&start, 70, 100), "and nothing follows them on the right");
        let end = rasterize(
            b"<svg width='100' height='40' viewBox='0 0 100 40'><text x='100' y='32' font-size='26' font-family='sans-serif' text-anchor='end' fill='#000000'>Hi</text></svg>",
            100,
        )
        .expect("draws");
        assert!(ink(&end, 70, 100), "an end anchor puts the glyphs against x = 100");
        assert!(!ink(&end, 0, 30), "and nothing at the left edge");
    }

    #[test]
    fn a_text_only_picture_keeps_its_size_and_paints() {
        let img = rasterize(
            b"<svg width='60' height='20' viewBox='0 0 60 20'><text x='31' y='15' font-size='12' font-family='sans-serif' text-anchor='middle' fill='#000000'>ok</text></svg>",
            60,
        )
        .expect("draws");
        assert_eq!(img.dimensions(), (60, 20));
        assert!(img.pixels().any(|p| p[3] > 0), "the label paints");
    }

    #[test]
    fn a_label_that_sets_only_one_coordinate_keeps_the_other() {
        // The second chunk sets x only; its baseline stays the one the first chunk used.
        let img = rasterize(
            b"<svg width='200' height='40' viewBox='0 0 200 40'><text y='30' font-size='20' font-family='sans-serif' fill='#000000'><tspan x='0'>H</tspan><tspan x='100'>H</tspan></text></svg>",
            200,
        )
        .expect("draws");
        let ink = |x0: u32, x1: u32| (0..img.height()).flat_map(|y| (x0..x1).map(move |x| (x, y))).any(|(x, y)| img.get_pixel(x, y)[3] > 128);
        assert!(ink(90, 120), "the second glyph sits at its own x");
        assert!(!ink(120, 200), "nothing spills past it");
    }

    #[test]
    fn a_stroked_label_without_a_fill_still_draws() {
        let img = rasterize(
            b"<svg width='60' height='40' viewBox='0 0 60 40'><text x='4' y='30' font-size='24' font-family='sans-serif' fill='none' stroke='#000000' stroke-width='1'>I</text></svg>",
            60,
        )
        .expect("draws");
        assert!(img.pixels().any(|p| p[3] > 128), "an outlined label paints");
    }

    #[test]
    fn a_gradient_in_user_space_follows_its_own_coordinates() {
        // The ramp runs over the first ten of a hundred units, then pads.
        let img = rasterize(
            b"<svg width='100' height='20' viewBox='0 0 100 20'><defs><linearGradient id='g' gradientUnits='userSpaceOnUse' x1='0' y1='0' x2='10' y2='0'><stop stop-color='#00ff00'/><stop offset='1' stop-color='#000000'/></linearGradient></defs><rect width='100' height='20' fill='url(#g)'/></svg>",
            100,
        )
        .expect("draws");
        assert!(img.get_pixel(2, 10)[1] > 180, "{:?}", img.get_pixel(2, 10));
        assert!(img.get_pixel(80, 10)[1] < 20, "{:?}", img.get_pixel(80, 10));
    }
}
