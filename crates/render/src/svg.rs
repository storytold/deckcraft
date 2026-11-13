//! SVG pictures (`image/svg+xml`): parsed by `deckcraft-svg` and drawn as vectors into a bitmap of
//! the size the slide needs, so they go through the same crop, tile and recolour path as any other
//! picture.
//!
//! The `viewBox` is fitted into the picture's own size the way SVG's default `preserveAspectRatio`
//! does (centred, both sides visible). Other `preserveAspectRatio` values, filters, masks and clip
//! paths are not applied.

use deckcraft_svg::Svg;
use kurbo::{Affine, BezPath, Rect, Shape as _};
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
        let path: BezPath = m * it.path.clone();
        let bb = path.bounding_box();
        if !(bb.x0.is_finite() && bb.y0.is_finite() && bb.x1.is_finite() && bb.y1.is_finite()) || bb.width() > 1e6 || bb.height() > 1e6 {
            continue;
        }
        if let Some(f) = it.fill {
            ctx.set_paint(peniko::Color::from_rgba8(f.r, f.g, f.b, f.a));
            ctx.set_fill_rule(if it.even_odd { peniko::Fill::EvenOdd } else { peniko::Fill::NonZero });
            ctx.fill_path(&path);
        }
        if let Some((c, pen)) = &it.stroke {
            ctx.set_paint(peniko::Color::from_rgba8(c.r, c.g, c.b, c.a));
            ctx.set_stroke(pen_at(pen, k, &path));
            ctx.stroke_path(&path);
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
}
