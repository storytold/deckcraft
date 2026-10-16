//! Drawing surfaces.
//!
//! The renderer draws through [`Canvas`]: `vello_cpu`'s `RenderContext` for pixels, or a vector
//! backend (the PDF export) that turns paths, paints, pictures and glyph runs into its own
//! primitives. What a vector canvas can't express (blur-based effects, pattern fills) is drawn by
//! `vello_cpu` into a pixmap covering just that element and placed as an image
//! ([`Canvas::fallback`]).

use std::sync::Arc;

use deckcraft_color::Rgba;
use deckcraft_text::GlyphRun;
use kurbo::{Affine, BezPath, Point, Rect, Shape as _};
use vello_cpu::{Pixmap, RenderContext, Resources, peniko};

use crate::{Image, MAX_SIDE};

/// Fill rule for [`Canvas::set_fill_rule`].
pub use vello_cpu::peniko::Fill as FillRule;

/// A paint for fills and strokes, in the coordinates of the path it paints.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    /// Straight RGBA (alpha included).
    Solid(Rgba),
    /// Colour stops (offset 0–1, sorted) along `start` → `end`, padded beyond them.
    Linear { start: Point, end: Point, stops: Vec<(f32, Rgba)> },
    /// Colour stops from the point `focus` (0) to the circle of `radius` around `center` (1), with
    /// circles in between moving from one to the other; padded beyond. `focus == center` is a
    /// plain radial gradient.
    Radial { focus: Point, center: Point, radius: f64, stops: Vec<(f32, Rgba)> },
}

impl From<peniko::Color> for Paint {
    fn from(c: peniko::Color) -> Self {
        let p = c.to_rgba8();
        Paint::Solid(Rgba::rgba(p.r, p.g, p.b, p.a))
    }
}

impl From<Rgba> for Paint {
    fn from(c: Rgba) -> Self {
        Paint::Solid(c)
    }
}

impl Paint {
    fn to_vello(&self) -> vello_cpu::PaintType {
        let col = |c: &Rgba| peniko::Color::from_rgba8(c.r, c.g, c.b, c.a);
        let stops = |s: &[(f32, Rgba)]| s.iter().map(|(o, c)| peniko::ColorStop { offset: *o, color: col(c).into() }).collect::<Vec<_>>();
        match self {
            Paint::Solid(c) => col(c).into(),
            Paint::Linear { start, end, stops: s } => peniko::Gradient::new_linear(*start, *end).with_stops(stops(s).as_slice()).into(),
            Paint::Radial { focus, center, radius, stops: s } => {
                let g = if focus == center {
                    peniko::Gradient::new_radial(*center, *radius as f32)
                } else {
                    peniko::Gradient::new_two_point_radial(*focus, 0.0, *center, *radius as f32)
                };
                g.with_stops(stops(s).as_slice()).into()
            }
        }
    }
}

/// How a vector canvas wants what it can't draw itself: rasterized at `scale` pixels per canvas
/// unit, within `clip` (canvas units; nothing outside it is visible).
#[derive(Clone, Copy, Debug)]
pub struct Fallback {
    pub scale: f64,
    pub clip: Rect,
}

/// A picture for [`Canvas::draw_image`]: its pixel grid is `width` × `height` units of the current
/// transform.
pub struct CanvasImage<'a> {
    pub width: f64,
    pub height: f64,
    /// Decoded (and adjusted) pixels; `None` when `encoded` is to be embedded instead.
    pub pixmap: Option<Arc<Pixmap>>,
    /// The original JPEG or PNG file, unmodified, for canvases that embed files as they are.
    pub encoded: Option<&'a Arc<Vec<u8>>>,
}

impl CanvasImage<'_> {
    /// The decoded pixels as straight RGBA, with their size.
    pub fn rgba(&self) -> Option<(Vec<u8>, u32, u32)> {
        let pm = self.pixmap.as_ref()?;
        let mut out = Vec::with_capacity(pm.data().len() * 4);
        for p in pm.data() {
            let a = p.a as u32;
            let un = |c: u8| (c as u32 * 255 + a / 2).checked_div(a).map_or(0, |v| v.min(255) as u8);
            out.extend_from_slice(&[un(p.r), un(p.g), un(p.b), p.a]);
        }
        Some((out, pm.width() as u32, pm.height() as u32))
    }

    /// Identity of the image data (for caches): the file or the pixmap it comes from.
    pub fn key(&self) -> usize {
        match (&self.encoded, &self.pixmap) {
            (Some(e), _) => Arc::as_ptr(e) as usize,
            (None, Some(p)) => Arc::as_ptr(p) as usize,
            (None, None) => 0,
        }
    }
}

/// Where the renderer draws. Transforms are absolute (shape space → canvas units), as in
/// `vello_cpu`; layers nest.
pub trait Canvas {
    fn set_transform(&mut self, t: Affine);
    /// Use [`dyn Canvas::set_paint`], which takes anything that converts to a [`Paint`].
    fn set_paint_to(&mut self, p: Paint);
    fn set_stroke(&mut self, s: kurbo::Stroke);
    fn set_fill_rule(&mut self, r: FillRule);
    fn fill_path(&mut self, p: &BezPath);
    fn stroke_path(&mut self, p: &BezPath);
    fn fill_rect(&mut self, r: &Rect) {
        self.fill_path(&r.to_path(0.1));
    }
    /// A group clipped to `clip` (current transform) and faded by `opacity`, until [`Self::pop_layer`].
    fn push_layer(&mut self, clip: Option<&BezPath>, opacity: Option<f32>);
    fn push_clip_layer(&mut self, clip: &BezPath) {
        self.push_layer(Some(clip), None);
    }
    fn pop_layer(&mut self);
    /// Draw a picture on its pixel grid under the current transform, faded by `alpha`.
    fn draw_image(&mut self, img: &CanvasImage, alpha: f64);
    /// Draw a whole glyph run as text, its origin mapped by `at` (paragraph `text` gives the
    /// characters the run's cells index). `false`: draw it glyph by glyph as outlines instead.
    fn glyph_run(&mut self, _run: &GlyphRun, _text: &str, _at: Affine, _opacity: f64) -> bool {
        false
    }
    /// The pixel context, for effects only pixels can show.
    fn raster(&mut self) -> Option<&mut RenderContext> {
        None
    }
    /// Vector canvases: how to rasterize what they can't draw.
    fn fallback(&self) -> Option<Fallback> {
        None
    }
    /// Place premultiplied pixels over `dest` (canvas units, untransformed).
    fn draw_pixels(&mut self, _img: &Image, _dest: Rect) {}
}

impl dyn Canvas + '_ {
    pub fn set_paint(&mut self, p: impl Into<Paint>) {
        self.set_paint_to(p.into());
    }
}

impl Canvas for RenderContext {
    fn set_transform(&mut self, t: Affine) {
        RenderContext::set_transform(self, t);
    }
    fn set_paint_to(&mut self, p: Paint) {
        RenderContext::set_paint(self, p.to_vello());
    }
    fn set_stroke(&mut self, s: kurbo::Stroke) {
        RenderContext::set_stroke(self, s);
    }
    fn set_fill_rule(&mut self, r: FillRule) {
        RenderContext::set_fill_rule(self, r);
    }
    fn fill_path(&mut self, p: &BezPath) {
        RenderContext::fill_path(self, p);
    }
    fn stroke_path(&mut self, p: &BezPath) {
        RenderContext::stroke_path(self, p);
    }
    fn fill_rect(&mut self, r: &Rect) {
        RenderContext::fill_rect(self, r);
    }
    fn push_layer(&mut self, clip: Option<&BezPath>, opacity: Option<f32>) {
        RenderContext::push_layer(self, clip, None, opacity, None, None);
    }
    fn push_clip_layer(&mut self, clip: &BezPath) {
        RenderContext::push_clip_layer(self, clip);
    }
    fn pop_layer(&mut self) {
        RenderContext::pop_layer(self);
    }
    fn draw_image(&mut self, img: &CanvasImage, alpha: f64) {
        let Some(pm) = &img.pixmap else { return };
        let image = vello_cpu::Image {
            image: vello_cpu::ImageSource::Pixmap(pm.clone()),
            sampler: peniko::ImageSampler { quality: peniko::ImageQuality::High, ..Default::default() },
        };
        let faded = alpha < 0.999;
        if faded {
            RenderContext::push_layer(self, None, None, Some(alpha.clamp(0.0, 1.0) as f32), None, None);
        }
        RenderContext::set_paint(self, image);
        RenderContext::fill_rect(self, &Rect::new(0.0, 0.0, pm.width() as f64, pm.height() as f64));
        if faded {
            RenderContext::pop_layer(self);
        }
    }
    fn raster(&mut self) -> Option<&mut RenderContext> {
        Some(self)
    }
}

/// Largest local raster, in pixels.
const MAX_PIXELS: f64 = 40.0e6;

/// On a vector canvas, run `draw` on a transparent pixmap covering `bounds` (canvas units, cut to
/// the fallback's clip) and place the result. `draw` gets the canvas → pixmap transform to prepend
/// to its own.
pub(crate) fn rasterize(ctx: &mut dyn Canvas, fb: Fallback, bounds: Rect, draw: &mut dyn FnMut(&mut RenderContext, Affine)) {
    let b = bounds.intersect(fb.clip);
    if !(b.is_finite() && b.width() > 0.0 && b.height() > 0.0) {
        return;
    }
    let mut k = if fb.scale.is_finite() { fb.scale.clamp(0.05, 20.0) } else { 1.0 };
    if b.area() * k * k > MAX_PIXELS {
        k = (MAX_PIXELS / b.area()).sqrt();
    }
    k = k.min(MAX_SIDE as f64 / b.width()).min(MAX_SIDE as f64 / b.height());
    let w = (b.width() * k).ceil().clamp(1.0, MAX_SIDE as f64) as u16;
    let h = (b.height() * k).ceil().clamp(1.0, MAX_SIDE as f64) as u16;
    let mut v = RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
    draw(&mut v, Affine::scale(k) * Affine::translate(-b.origin().to_vec2()));
    v.flush();
    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    if let Some(pm) = vello_cpu::PixmapMut::new(w, h, &mut pixels) {
        v.render(pm, &mut Resources::new());
    }
    if pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
        return;
    }
    let dest = Rect::new(b.x0, b.y0, b.x0 + w as f64 / k, b.y0 + h as f64 / k);
    ctx.draw_pixels(&Image { width: w as u32, height: h as u32, pixels }, dest);
}

/// Draw what only the pixel context can draw (blurs, blend modes, pixel paints): directly on a
/// pixel canvas; on a vector canvas rasterized over `bounds` (canvas units) and placed, so it is
/// never dropped. `draw` gets the pixel context and the transform to put before its own (the
/// identity on a pixel canvas).
pub(crate) fn pixels(ctx: &mut dyn Canvas, bounds: Rect, draw: &mut dyn FnMut(&mut RenderContext, Affine)) {
    if let Some(px) = ctx.raster() {
        draw(px, Affine::IDENTITY);
    } else if let Some(fb) = ctx.fallback() {
        rasterize(ctx, fb, bounds, draw);
    }
}
