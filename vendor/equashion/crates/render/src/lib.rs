//! equaSHion rendering: a laid-out [`Scene`] to premultiplied RGBA pixels ([`render_scene`],
//! with vello_cpu, behind the default `raster` feature) or to an SVG document ([`scene_to_svg`]).
//!
//! Without `raster` only the SVG path is built and `vello_cpu` is not a dependency, which is
//! what a browser UI that draws equations as SVG wants.
//!
//! The raster path is WordCraft's `render_eq` and the `Draw::Glyphs` / `Draw::Fill` /
//! `Draw::Path` branches of its display-list renderer, including synthetic bold (the outline
//! stroked at 3% of the em as well as filled) and synthetic italic (a -0.21 horizontal skew).
//! The SVG path draws the same primitives with the same transforms.
//!
//! ```
//! use equashion_layout::{DEFAULT_FAMILY, FontDb, Rgb, layout_equation};
//!
//! let fonts = FontDb::bundled_only();
//! let m = equashion_math::from_linear(r"x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}");
//! let eq = layout_equation(&fonts, &m, DEFAULT_FAMILY, 24.0, true, Rgb::BLACK, 4.0);
//! let svg = equashion_render::scene_to_svg(&eq.scene, &fonts, DEFAULT_FAMILY);
//! let pixels = equashion_render::render_eq(&eq, &fonts, 2.0);
//! assert!(svg.contains("<rect ") && pixels.width > 0);
//! ```
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod svg;

use kurbo::Affine;

#[cfg(feature = "raster")]
use equashion_layout::{EqLayout, MathFonts, Prim, Rgb, Scene};
#[cfg(feature = "raster")]
use vello_cpu::peniko;
#[cfg(feature = "raster")]
use vello_cpu::{RenderContext, Resources};

pub use svg::{fmt2, scene_to_svg};

/// Largest raster side vello_cpu handles comfortably.
#[cfg(feature = "raster")]
pub const MAX_SIDE: u32 = 16_000;

/// Horizontal shear of synthetic italic (WordCraft's value).
pub const SYNTH_ITALIC_SKEW: f64 = -0.21;

/// Stroke width of synthetic bold, as a fraction of the em (WordCraft's value).
pub const SYNTH_BOLD_STROKE: f64 = 0.03;

/// A rendered image (premultiplied RGBA8, row-major).
#[cfg(feature = "raster")]
#[derive(Clone, Default)]
pub struct Rendered {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[cfg(feature = "raster")]
impl Rendered {
    /// The pixels with alpha un-premultiplied (straight RGBA8).
    pub fn to_straight(&self) -> Vec<u8> {
        let mut out = self.pixels.clone();
        for px in out.as_chunks_mut::<4>().0 {
            let a = px[3] as u32;
            if a != 0 && a != 255 {
                for c in px.iter_mut().take(3) {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        out
    }
    /// Straight RGBA at (x, y) (transparent outside).
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let i = ((y as usize * self.width as usize) + x as usize) * 4;
        let Some(p) = self.pixels.get(i..i + 4) else { return [0; 4] };
        let a = p[3] as u32;
        if a == 0 {
            return [0; 4];
        }
        let un = |c: u8| ((c as u32 * 255 + a / 2) / a).min(255) as u8;
        [un(p[0]), un(p[1]), un(p[2]), p[3]]
    }
}

#[cfg(feature = "raster")]
fn color(c: Rgb) -> peniko::Color {
    peniko::Color::from_rgba8(c.0, c.1, c.2, 255)
}

/// The transform that places glyph outlines (font units, y down) of a face with `upem` units
/// per em at `size` points, with its origin at (x, y), slanted for synthetic italic.
pub fn glyph_transform(x: f64, y: f64, size: f64, upem: f64, synth_italic: bool) -> Affine {
    let k = size / upem.max(1.0);
    let skew = if synth_italic { Affine::new([1.0, 0.0, SYNTH_ITALIC_SKEW, 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
    Affine::translate((x, y)) * skew * Affine::scale(k)
}

/// Render `scene` at `scale` pixels per point, on `background` (transparent when `None`).
#[cfg(feature = "raster")]
pub fn render_scene(scene: &Scene, fonts: &dyn MathFonts, scale: f64, background: Option<Rgb>) -> Rendered {
    let scale = if scale.is_finite() { scale.clamp(0.1, 16.0) } else { 1.0 };
    let side = |v: f64| if v.is_finite() { (v * scale).ceil().clamp(1.0, MAX_SIDE as f64) as u16 } else { 1 };
    let (w, h) = (side(scene.width), side(scene.height));
    let mut ctx = RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
    if let Some(bg) = background {
        ctx.set_transform(Affine::IDENTITY);
        ctx.set_paint(color(bg));
        ctx.fill_rect(&kurbo::Rect::new(0.0, 0.0, w as f64, h as f64));
    }
    let view = Affine::scale(scale);
    for p in &scene.items {
        draw(&mut ctx, fonts, p, view);
    }
    ctx.flush();
    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    let mut res = Resources::new();
    if let Some(pm) = vello_cpu::PixmapMut::new(w, h, &mut pixels) {
        ctx.render(pm, &mut res);
    }
    Rendered { width: w as u32, height: h as u32, pixels }
}

/// Render an equation laid out by [`equashion_layout::layout_equation`] on white (the equation
/// editor's canvas); `scale` is pixels per point.
#[cfg(feature = "raster")]
pub fn render_eq(l: &EqLayout, fonts: &dyn MathFonts, scale: f64) -> Rendered {
    render_scene(&l.scene, fonts, scale, Some(Rgb::WHITE))
}

#[cfg(feature = "raster")]
fn finite(r: &kurbo::Rect) -> bool {
    r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()
}

#[cfg(feature = "raster")]
fn draw(ctx: &mut RenderContext, fonts: &dyn MathFonts, p: &Prim, view: Affine) {
    match p {
        Prim::Rule(r, c) => {
            if !finite(r) {
                return;
            }
            ctx.set_transform(view);
            ctx.set_paint(color(*c));
            ctx.fill_rect(r);
        }
        Prim::Path(path, c) => {
            if !finite(&kurbo::Shape::bounding_box(path)) {
                return;
            }
            ctx.set_transform(view);
            ctx.set_paint(color(*c));
            ctx.fill_path(path);
        }
        Prim::Glyphs { font, size, glyphs, color: c, synth_bold, synth_italic } => {
            let Some(face) = fonts.face_by_id(*font) else { return };
            ctx.set_paint(color(*c));
            if *synth_bold {
                ctx.set_stroke(kurbo::Stroke::new(face.upem * SYNTH_BOLD_STROKE));
            }
            for (gid, x, y) in glyphs {
                let o = fonts.outline(&face, *gid);
                if o.elements().is_empty() {
                    continue;
                }
                ctx.set_transform(view * glyph_transform(*x as f64, *y as f64, *size as f64, face.upem, *synth_italic));
                ctx.fill_path(&o);
                if *synth_bold {
                    ctx.stroke_path(&o);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use equashion_layout::{DEFAULT_FAMILY, FontDb, layout_equation};
    use equashion_math::from_linear;
    use std::sync::OnceLock;

    fn fonts() -> &'static FontDb {
        static DB: OnceLock<FontDb> = OnceLock::new();
        DB.get_or_init(FontDb::bundled_only)
    }

    fn ink(r: &Rendered) -> usize {
        (0..r.height).flat_map(|y| (0..r.width).map(move |x| (x, y))).filter(|&(x, y)| r.pixel(x, y)[0] < 128).count()
    }

    #[test]
    fn an_equation_renders_dark_pixels_on_white() {
        let l = layout_equation(fonts(), &from_linear("x^2/sqrt(y)"), DEFAULT_FAMILY, 24.0, true, Rgb::BLACK, 4.0);
        let r = render_eq(&l, fonts(), 2.0);
        assert_eq!((r.width, r.height), ((l.width() * 2.0).ceil() as u32, (l.height() * 2.0).ceil() as u32));
        assert_eq!(r.pixel(0, 0), [255, 255, 255, 255], "white canvas");
        assert!(ink(&r) > 200, "glyphs, the bar and the radical are drawn");
    }

    #[test]
    fn a_transparent_scene_has_only_ink() {
        let l = layout_equation(fonts(), &from_linear("a+b"), DEFAULT_FAMILY, 24.0, false, Rgb(200, 0, 0), 2.0);
        let r = render_scene(&l.scene, fonts(), 1.0, None);
        assert_eq!(r.pixel(0, 0), [0; 4]);
        assert!(r.to_straight().as_chunks::<4>().0.iter().any(|p| p[3] == 255 && p[0] == 200 && p[1] == 0));
    }

    #[test]
    fn synthetic_bold_and_italic_change_the_ink() {
        let db = fonts();
        let face = db.face("Inter", "Regular");
        let gid = face.glyph_for('l');
        let glyph = |synth_bold, synth_italic| Scene {
            items: vec![Prim::Glyphs {
                font: face.face_id(),
                size: 40.0,
                glyphs: vec![(gid, 10.0, 40.0)],
                color: Rgb::BLACK,
                synth_bold,
                synth_italic,
            }],
            width: 40.0,
            height: 50.0,
        };
        let plain = render_scene(&glyph(false, false), db, 1.0, Some(Rgb::WHITE));
        let bold = render_scene(&glyph(true, false), db, 1.0, Some(Rgb::WHITE));
        let italic = render_scene(&glyph(false, true), db, 1.0, Some(Rgb::WHITE));
        assert!(ink(&bold) > ink(&plain), "bold is heavier: {} vs {}", ink(&bold), ink(&plain));
        // The skew moves the top of the stem right of the bottom.
        let leftmost = |r: &Rendered, y: u32| (0..r.width).find(|&x| r.pixel(x, y)[0] < 128);
        let (top, bottom) = (leftmost(&italic, 14), leftmost(&italic, 38));
        assert!(top > bottom, "{top:?} {bottom:?}");
        assert_eq!(leftmost(&plain, 14), leftmost(&plain, 38), "upright stem");
    }

    #[test]
    fn hostile_scales_and_sizes_are_clamped() {
        let l = layout_equation(fonts(), &from_linear("x"), DEFAULT_FAMILY, 24.0, false, Rgb::BLACK, 0.0);
        for s in [f64::NAN, f64::INFINITY, -3.0, 0.0, 1e9] {
            let r = render_eq(&l, fonts(), s);
            assert!(r.width >= 1 && r.width <= MAX_SIDE && r.height >= 1);
        }
        let huge = Scene { items: vec![], width: f64::INFINITY, height: 1e12 };
        let r = render_scene(&huge, fonts(), 1.0, None);
        assert!(r.width == 1 && r.height == MAX_SIDE);
    }
}
