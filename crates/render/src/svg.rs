//! SVG pictures (`image/svg+xml`): read by `deckcraft-svg` and drawn as vectors into a bitmap of
//! the size the slide needs, so they go through the same crop, tile and recolour path as any other
//! picture.
//!
//! The reader hands the geometry over in the picture's own space, with gradients in user units;
//! text arrives as glyph outlines. Filters, masks and clip paths are not applied by the reader and
//! are reported through its `Skipped` flags; `mask`s are not applied here either.

use deckcraft_svg::{Gradient, GradientKind, Paint, Spread, Svg};
use kurbo::{Affine, BezPath, Point, Rect, Shape as _};
use vello_cpu::{RenderContext, Resources, peniko};

/// Largest side drawn.
const MAX_SIDE: u32 = 8192;

/// Does `bytes` look like an SVG picture?
pub fn is_svg(bytes: &[u8]) -> bool {
    deckcraft_svg::is_svg(bytes)
}

/// Draw the picture into straight RGBA with its longer side `side` px (transparent where it draws
/// nothing). `None` when the document doesn't parse.
/// The picture's own size in user units, `None` when it does not read.
pub fn natural_size(bytes: &[u8]) -> Option<(f64, f64)> {
    deckcraft_svg::parse(bytes).map(|s| (s.size.0, s.size.1))
}

pub fn rasterize(bytes: &[u8], side: u32) -> Option<image::RgbaImage> {
    let svg = deckcraft_svg::parse(bytes)?;
    let (w, h, m) = layout(&svg, side);
    // A picture that uses a filter, mask, clip path, pattern or embedded image needs SVG's own
    // compositing, which the vector mapping does not model. The shared rasterizer draws all of it
    // within a memory budget; when that is refused, the mapped vectors are still better than
    // nothing.
    if svg.skipped.any()
        && let Some(img) = deckcraft_svg::rasterize(bytes, u32::from(w), u32::from(h))
    {
        return Some(img);
    }
    let mut ctx = RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
    let r = Rect::new(0.0, 0.0, w as f64, h as f64);
    ctx.push_clip_layer(&r.to_path(0.1));
    let k = m.determinant().abs().sqrt();
    for it in &svg.items {
        let path: BezPath = m * it.path.clone();
        let bb = path.bounding_box();
        if !(bb.x0.is_finite() && bb.y0.is_finite() && bb.x1.is_finite() && bb.y1.is_finite()) || bb.width() > 1e6 || bb.height() > 1e6 {
            continue;
        }
        // A clipped shape is drawn inside its clip paths, intersected.
        for clip in &it.clips {
            let c: BezPath = m * clip.path.clone();
            ctx.set_fill_rule(if clip.even_odd { peniko::Fill::EvenOdd } else { peniko::Fill::NonZero });
            ctx.push_clip_layer(&c);
        }
        if let Some(f) = &it.fill {
            set_paint(&mut ctx, f, m);
            ctx.set_fill_rule(if it.even_odd { peniko::Fill::EvenOdd } else { peniko::Fill::NonZero });
            ctx.fill_path(&path);
            ctx.reset_paint_transform();
        }
        if let Some((c, pen)) = &it.stroke {
            set_paint(&mut ctx, c, m);
            ctx.set_stroke(pen_at(pen, k, &path));
            ctx.stroke_path(&path);
            ctx.reset_paint_transform();
        }
        for _ in 0..it.clips.len() {
            ctx.pop_layer();
        }
        ctx.set_fill_rule(peniko::Fill::NonZero);
    }
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
    rasterize_png_at(bytes, side, 96.0)
}

/// A picture rendered as a PNG that says it is `dpi` dense. A picture written as a raster copy of
/// a vector is drawn at the vector's own size, which is what the density tells a reader.
pub fn rasterize_png_at(bytes: &[u8], side: u32, dpi: f64) -> Option<Vec<u8>> {
    let img = rasterize(bytes, side)?;
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).ok()?;
    Some(crate::images::stamp_png_dpi(&buf, dpi))
}

/// The bitmap size and the user-units → pixels transform for a target `side`.
fn layout(svg: &Svg, side: u32) -> (u16, u16, Affine) {
    let (pw, ph) = svg.size;
    let k = f64::from(side.clamp(16, MAX_SIDE)) / pw.max(ph).max(1e-9);
    let w = (pw * k).round().clamp(1.0, f64::from(MAX_SIDE)) as u16;
    let h = (ph * k).round().clamp(1.0, f64::from(MAX_SIDE)) as u16;
    // The reader hands the geometry over in the picture's own space (usvg applied its viewBox),
    // so it only has to fill the bitmap, both sides visible and centred.
    let s = (f64::from(w) / pw).min(f64::from(h) / ph);
    let tx = (f64::from(w) - pw * s) / 2.0;
    let ty = (f64::from(h) - ph * s) / 2.0;
    (w, h, Affine::translate((tx, ty)) * Affine::scale(s))
}

/// Paint one shape's fill or stroke: a solid colour, or a gradient laid out the way the picture
/// declared it. The reader hands gradients over in user units, so `m` maps them to pixels.
fn set_paint(ctx: &mut RenderContext, paint: &Paint, m: Affine) {
    match paint {
        Paint::Solid(c) => ctx.set_paint(peniko::Color::from_rgba8(c.r, c.g, c.b, c.a)),
        Paint::Gradient(g) => {
            let Some(grad) = peniko_gradient(g) else { return };
            ctx.set_paint(grad);
            ctx.set_paint_transform(m * g.transform);
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
mod dpi_tests {
    use super::*;

    #[test]
    fn a_fallback_says_how_dense_it_is() {
        let svg = br#"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><rect width='24' height='24' fill='red'/></svg>"#;
        let png = rasterize_png_at(svg, 512, 2048.0).expect("renders");
        // 2048 dpi is 2048 / 0.0254 = 80629.9 dots per metre, rounded.
        let at = png.windows(4).position(|w| w == b"pHYs").expect("a pHYs chunk");
        let ppm = u32::from_be_bytes([png[at + 4], png[at + 5], png[at + 6], png[at + 7]]);
        assert!((80630..=80631).contains(&ppm), "{ppm}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAR: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24'>\
        <path d='M12 2 L15 9 L22 9 L16 14 L18 21 L12 17 L6 21 L8 14 L2 9 L9 9 Z' fill='#ff0000'/></svg>";

    #[test]
    fn a_translucent_group_is_faded_as_one_layer() {
        // SVG composites the group, then applies its opacity: where the green covers the red, the
        // red does not show through.
        let img = rasterize(
            b"<svg width='30' height='10' viewBox='0 0 30 10'><g opacity='0.5'><rect width='20' height='10' fill='#ff0000'/><rect x='10' width='20' height='10' fill='#00ff00'/></g></svg>",
            30,
        )
        .expect("draws");
        let overlap = img.get_pixel(15, 5);
        let green = img.get_pixel(25, 5);
        assert!(overlap[0] < 10, "no red under the green: {overlap:?}");
        assert!(overlap[1] > 245 && (i16::from(overlap[3]) - i16::from(green[3])).abs() <= 2, "overlap {overlap:?}, green alone {green:?}");
    }

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
    fn a_clip_path_clips_the_vector_shapes() {
        // Clip paths are geometry the mapping carries, so the red stops at the clip edge without
        // the picture being rasterized.
        let img = rasterize(
            b"<svg width='100' height='20' viewBox='0 0 100 20'><defs><clipPath id='c'><rect width='50' height='20'/></clipPath></defs><g clip-path='url(#c)'><rect width='100' height='20' fill='#ff0000'/></g></svg>",
            100,
        )
        .expect("draws");
        assert_eq!(img.get_pixel(10, 10)[0], 255, "inside the clip");
        assert_eq!(img.get_pixel(90, 10)[3], 0, "outside the clip");
    }

    #[test]
    fn a_pattern_fill_tiles_the_vector_shape() {
        let img = rasterize(
            b"<svg width='40' height='40' viewBox='0 0 40 40'><defs><pattern id='p' width='10' height='10' patternUnits='userSpaceOnUse'><circle cx='5' cy='5' r='3' fill='#000000'/></pattern></defs><rect width='40' height='40' fill='url(#p)'/></svg>",
            40,
        )
        .expect("draws");
        // A dot sits at the centre of each 10 px cell, and the gaps between them stay clear.
        assert!(img.get_pixel(5, 5)[3] > 200, "the first dot paints");
        assert_eq!(img.get_pixel(5 + 10, 5)[3], img.get_pixel(5, 5)[3], "cells repeat");
        assert_eq!(img.get_pixel(10, 10)[3], 0, "the gap stays clear");
    }

    #[test]
    fn a_filter_draws_through_the_shared_rasterizer() {
        // A blur spreads the red past the rectangle's own edge.
        let img = rasterize(
            b"<svg width='60' height='60' viewBox='0 0 60 60'><defs><filter id='f'><feGaussianBlur stdDeviation='4'/></filter></defs><rect x='20' y='20' width='20' height='20' fill='#ff0000' filter='url(#f)'/></svg>",
            60,
        )
        .expect("draws");
        // SVG's default filter region is the bounding box grown by 10%, so the blur shows just
        // inside x = 18 rather than across the whole canvas.
        assert!(img.get_pixel(19, 30)[3] > 0, "the blur reaches left of the rectangle");
        assert_eq!(img.get_pixel(2, 30)[3], 0, "and not past its region");
        assert!(img.get_pixel(30, 30)[0] > 100, "the middle still paints");
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
