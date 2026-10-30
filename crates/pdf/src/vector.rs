//! Vector drawing: the renderer's [`Canvas`] on a krilla surface.
//!
//! Paths become PDF paths, solid colours and linear/radial gradients PDF paints (with their
//! transparency), glyph runs real text in the embedded font subsets, pictures image XObjects (an
//! unchanged JPEG embedded as it is, other pictures losslessly). Whatever the renderer can only
//! draw as pixels (blur-based effects, pattern fills) arrives through [`Canvas::draw_pixels`],
//! rasterized for that element alone.

use std::collections::HashMap;

use craft_pdf::write::{self, WriteError, backend as krilla};
use deckcraft_render::canvas::{Canvas, CanvasImage, Fallback, FillRule as Rule, Paint};
use deckcraft_text::GlyphRun;
use krilla::color::rgb;
use krilla::geom::{Point, Size, Transform};
use krilla::image::Image;
use krilla::num::NormalizedF32;
use krilla::paint::{Fill, FillRule, LineCap, LineJoin, LinearGradient, RadialGradient, SpreadMethod, Stop, Stroke, StrokeDash};
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId, KrillaGlyph};
use kurbo::{Affine, BezPath, Rect};

use crate::{tf, to_path};

/// Fonts and pictures already handed to krilla, shared by every page.
#[derive(Default)]
pub(crate) struct Cache {
    fonts: HashMap<u32, Font>,
    images: HashMap<usize, Option<Image>>,
    /// The pixmaps `images` is keyed by, kept alive so a freed one's address is never reused as a key.
    held: Vec<Box<dyn std::any::Any>>,
}

impl Cache {
    /// The embedded font for `face`. A face that can't be embedded is an error: the export
    /// reports it rather than dropping or substituting text.
    pub fn font(&mut self, face: &deckcraft_fonts::FontFace) -> Result<Font, WriteError> {
        if let Some(font) = self.fonts.get(&face.id()) {
            return Ok(font.clone());
        }
        let font = write::font(face.data(), face.index(), None)?;
        self.fonts.insert(face.id(), font.clone());
        Ok(font)
    }

    fn image(&mut self, img: &CanvasImage) -> Option<Image> {
        let key = img.key();
        if key == 0 {
            return None;
        }
        if let Some(pm) = &img.pixmap
            && !self.images.contains_key(&key)
        {
            self.held.push(Box::new(pm.clone()));
        }
        self.images
            .entry(key)
            .or_insert_with(|| {
                if let Some(data) = img.encoded {
                    let bytes = data.as_slice();
                    // krilla reads a JPEG's headers now (and copies its data as it is), but decodes
                    // a PNG only when the document is written: a broken one would fail the whole
                    // export, so PNGs are checked first.
                    let embedded = if bytes.starts_with(&[0xFF, 0xD8]) {
                        Image::from_jpeg(data.clone().into(), true).ok()
                    } else if deckcraft_render::decode_image(bytes).is_some() {
                        Image::from_png(data.clone().into(), true).ok()
                    } else {
                        None
                    };
                    // A file krilla can't read is decoded and embedded as pixels.
                    return embedded.or_else(|| {
                        let px = deckcraft_render::decode_image(bytes)?;
                        let (w, h) = px.dimensions();
                        Some(Image::from_rgba8(px.into_raw(), w, h))
                    });
                }
                let (px, w, h) = img.rgba()?;
                Some(Image::from_rgba8(px, w, h))
            })
            .clone()
    }
}

fn norm(v: f64) -> NormalizedF32 {
    NormalizedF32::new(if v.is_finite() { v.clamp(0.0, 1.0) as f32 } else { 1.0 }).unwrap_or(NormalizedF32::ONE)
}

fn rgb_of(c: &deckcraft_color::Rgba) -> rgb::Color {
    rgb::Color::new(c.r, c.g, c.b)
}

fn stops(s: &[(f32, deckcraft_color::Rgba)]) -> Vec<Stop> {
    s.iter().map(|(o, c)| Stop { offset: norm(*o as f64), color: rgb_of(c).into(), opacity: norm(c.a as f64 / 255.0) }).collect()
}

/// A krilla paint and its opacity for `p`.
fn paint(p: &Paint) -> (krilla::paint::Paint, NormalizedF32) {
    match p {
        Paint::Solid(c) => (rgb_of(c).into(), norm(c.a as f64 / 255.0)),
        Paint::Linear { stops: s, .. } | Paint::Radial { stops: s, .. } if s.len() < 2 => match s.first() {
            Some((_, c)) => (rgb_of(c).into(), norm(c.a as f64 / 255.0)),
            None => (rgb::Color::black().into(), NormalizedF32::ZERO),
        },
        Paint::Linear { start, end, stops: s } => (
            LinearGradient {
                x1: start.x as f32,
                y1: start.y as f32,
                x2: end.x as f32,
                y2: end.y as f32,
                transform: Transform::identity(),
                spread_method: SpreadMethod::Pad,
                stops: stops(s),
                anti_alias: false,
            }
            .into(),
            NormalizedF32::ONE,
        ),
        Paint::Radial { focus, center, radius, stops: s } => (
            RadialGradient {
                fx: focus.x as f32,
                fy: focus.y as f32,
                fr: 0.0,
                cx: center.x as f32,
                cy: center.y as f32,
                cr: radius.max(1e-3) as f32,
                transform: Transform::identity(),
                spread_method: SpreadMethod::Pad,
                stops: stops(s),
                anti_alias: false,
            }
            .into(),
            NormalizedF32::ONE,
        ),
    }
}

fn stroke(s: &kurbo::Stroke, p: &Paint) -> Stroke {
    let (paint, opacity) = paint(p);
    let dash: Vec<f32> = s.dash_pattern.iter().map(|v| *v as f32).collect();
    Stroke {
        paint,
        width: s.width.max(0.0) as f32,
        miter_limit: s.miter_limit.max(1.0) as f32,
        line_cap: match s.start_cap {
            kurbo::Cap::Butt => LineCap::Butt,
            kurbo::Cap::Round => LineCap::Round,
            kurbo::Cap::Square => LineCap::Square,
        },
        line_join: match s.join {
            kurbo::Join::Bevel => LineJoin::Bevel,
            kurbo::Join::Miter => LineJoin::Miter,
            kurbo::Join::Round => LineJoin::Round,
        },
        opacity,
        dash: (!dash.is_empty() && dash.iter().all(|v| v.is_finite() && *v >= 0.0) && dash.iter().any(|v| *v > 0.0))
            .then_some(StrokeDash { array: dash, offset: s.dash_offset as f32 }),
    }
}

fn finite(t: Affine) -> bool {
    t.as_coeffs().iter().all(|v| v.is_finite())
}

/// The renderer's drawing calls as PDF content on one page.
pub(crate) struct PdfCanvas<'a, 'b> {
    s: &'a mut Surface<'b>,
    cache: &'a mut Cache,
    t: Affine,
    paint: Paint,
    stroke: kurbo::Stroke,
    even_odd: bool,
    /// Per open layer: how many krilla pushes it made.
    layers: Vec<u8>,
    fallback: Fallback,
    /// The first font that could not be embedded.
    error: Option<WriteError>,
}

impl<'a, 'b> PdfCanvas<'a, 'b> {
    pub fn new(s: &'a mut Surface<'b>, cache: &'a mut Cache, fallback: Fallback) -> Self {
        PdfCanvas {
            s,
            cache,
            t: Affine::IDENTITY,
            paint: Paint::Solid(deckcraft_color::Rgba::BLACK),
            stroke: kurbo::Stroke::new(1.0),
            even_odd: false,
            layers: Vec::new(),
            fallback,
            error: None,
        }
    }

    /// Close the layers left open (a drawing that stopped half-way), so pushes and pops balance.
    /// An error when a font could not be embedded.
    pub fn finish(mut self) -> Result<(), WriteError> {
        while !self.layers.is_empty() {
            self.pop_layer();
        }
        self.error.map_or(Ok(()), Err)
    }

    fn draw(&mut self, p: &BezPath, fill: Option<Fill>, stroke: Option<Stroke>) {
        if !finite(self.t) || p.elements().is_empty() {
            return;
        }
        let Some(path) = to_path(p) else { return };
        self.s.push_transform(&tf(self.t));
        self.s.set_fill(fill);
        self.s.set_stroke(stroke);
        self.s.draw_path(&path);
        self.s.set_fill(None);
        self.s.set_stroke(None);
        self.s.pop();
    }
}

impl Canvas for PdfCanvas<'_, '_> {
    fn set_transform(&mut self, t: Affine) {
        self.t = t;
    }
    fn set_paint_to(&mut self, p: Paint) {
        self.paint = p;
    }
    fn set_stroke(&mut self, s: kurbo::Stroke) {
        self.stroke = s;
    }
    fn set_fill_rule(&mut self, r: Rule) {
        self.even_odd = matches!(r, Rule::EvenOdd);
    }
    fn fill_path(&mut self, p: &BezPath) {
        let (paint, opacity) = paint(&self.paint);
        if opacity == NormalizedF32::ZERO {
            return;
        }
        let rule = if self.even_odd { FillRule::EvenOdd } else { FillRule::NonZero };
        self.draw(p, Some(Fill { paint, opacity, rule }), None);
    }
    fn stroke_path(&mut self, p: &BezPath) {
        let sk = stroke(&self.stroke, &self.paint);
        if sk.opacity == NormalizedF32::ZERO {
            return;
        }
        self.draw(p, None, Some(sk));
    }
    fn push_layer(&mut self, clip: Option<&BezPath>, opacity: Option<f32>) {
        let mut n = 0;
        if let Some(c) = clip {
            let mut c = c.clone();
            let path = if finite(self.t) {
                c.apply_affine(self.t);
                to_path(&c)
            } else {
                None
            };
            match path {
                Some(path) => {
                    let rule = if self.even_odd { FillRule::EvenOdd } else { FillRule::NonZero };
                    self.s.push_clip_path(&path, &rule)
                }
                // Nothing can show through an empty clip.
                None => self.s.push_opacity(NormalizedF32::ZERO),
            }
            n += 1;
        }
        if let Some(o) = opacity.filter(|o| *o < 0.999) {
            self.s.push_opacity(norm(o as f64));
            n += 1;
        }
        self.layers.push(n);
    }
    fn pop_layer(&mut self) {
        for _ in 0..self.layers.pop().unwrap_or(0) {
            self.s.pop();
        }
    }
    fn draw_image(&mut self, img: &CanvasImage, alpha: f64) {
        let (Some(size), true) = (Size::from_wh(img.width as f32, img.height as f32), finite(self.t)) else { return };
        // A file that can't be embedded (a header that reads but data that doesn't) shows the
        // missing-picture placeholder, as the pixel canvas does.
        let Some(image) = self.cache.image(img) else {
            deckcraft_render::missing_picture(self, Rect::new(0.0, 0.0, img.width, img.height));
            return;
        };
        self.s.push_transform(&tf(self.t));
        let faded = alpha < 0.999;
        if faded {
            self.s.push_opacity(norm(alpha));
        }
        self.s.draw_image(image, size);
        if faded {
            self.s.pop();
        }
        self.s.pop();
    }
    fn glyph_run(&mut self, run: &GlyphRun, text: &str, at: Affine, opacity: f64) -> bool {
        let (Some(first), true) = (run.glyphs.first(), run.size > 0.0 && run.size.is_finite()) else { return true };
        // Bullets (glyphs without characters) stay outlines: they aren't text to copy.
        if run.cells.len() != run.glyphs.len() {
            return false;
        }
        let font = match self.cache.font(&run.face) {
            Ok(font) => font,
            Err(e) => {
                // The export fails with this error; nothing more to draw for the run.
                self.error.get_or_insert(e);
                return true;
            }
        };
        let (x0, y0) = (first.1, first.2);
        // The characters the run covers, and each glyph's cluster within them.
        let chars: Vec<char> = text.chars().collect();
        let lo = run.chars.0.min(chars.len());
        let hi = run.chars.1.clamp(lo, chars.len());
        let sub: String = chars.get(lo..hi).unwrap_or_default().iter().collect();
        let mut offs: Vec<usize> = sub.char_indices().map(|(b, _)| b).collect();
        offs.push(sub.len());
        let mut heads: Vec<usize> = run.cells.iter().map(|c| c.0.clamp(lo, hi)).collect();
        heads.sort_unstable();
        heads.dedup();
        let range = |gi: usize| {
            let Some(&(h, _)) = run.cells.get(gi) else { return 0..0 };
            let h = h.clamp(lo, hi);
            let end = heads.iter().copied().find(|x| *x > h).unwrap_or(hi);
            let (a, b) = (offs.get(h - lo).copied().unwrap_or(0), offs.get(end - lo).copied().unwrap_or(sub.len()));
            a.min(b)..b
        };
        let upem = run.face.upem.max(1.0);
        let glyphs: Vec<KrillaGlyph> = run
            .glyphs
            .iter()
            .enumerate()
            .map(|(gi, g)| {
                let adv = match run.glyphs.get(gi + 1) {
                    Some(n) => (n.1 - g.1) / run.size,
                    None => run.face.advance(g.0) / upem,
                };
                KrillaGlyph::new(GlyphId::new(g.0), adv as f32, 0.0, (-(g.2 - y0) / run.size) as f32, 0.0, range(gi), None)
            })
            .collect();
        let skew = if run.fake_italic { Affine::new([1.0, 0.0, -0.2, 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
        let t = at * Affine::translate((x0, y0)) * skew;
        if !finite(t) {
            return true;
        }
        // A named instance of a variable font (Bold, Light…) other than the default one: krilla
        // embeds the default instance only; and upright glyphs in vertical text (East Asian
        // `eaVert`, stacked WordArt), each turned about its own em box, which a PDF text run
        // can't express. The renderer draws those glyphs as outlines and the text goes
        // underneath, invisible, for search, selection and copy.
        if run.upright || run.face.location().coords().iter().any(|c| c.to_f32() != 0.0) {
            self.s.push_transform(&tf(t));
            self.s.set_fill(Some(Fill { opacity: NormalizedF32::ZERO, ..Default::default() }));
            self.s.draw_glyphs(Point::from_xy(0.0, 0.0), &glyphs, font, &sub, run.size as f32, false);
            self.s.set_fill(None);
            self.s.pop();
            return false;
        }
        let alpha = run.alpha * opacity;
        let fill = (alpha > 0.0 && run.color.a > 0).then(|| Fill {
            paint: rgb_of(&run.color).into(),
            opacity: norm(alpha * run.color.a as f64 / 255.0),
            ..Default::default()
        });
        let line = |c: &deckcraft_color::Rgba, w: f64, o: f64| Stroke {
            paint: rgb_of(c).into(),
            width: w.max(0.0) as f32,
            line_join: LineJoin::Round,
            opacity: norm(o * c.a as f64 / 255.0),
            ..Default::default()
        };
        let outline = match (&run.outline, &fill) {
            (Some((oc, ow)), _) => Some(line(oc, *ow, opacity)),
            (None, Some(_)) if run.fake_bold => Some(line(&run.color, run.size * 0.025, alpha)),
            _ => None,
        };
        if fill.is_none() && outline.is_none() {
            return true;
        }
        self.s.push_transform(&tf(t));
        self.s.set_fill(fill);
        self.s.set_stroke(outline);
        self.s.draw_glyphs(Point::from_xy(0.0, 0.0), &glyphs, font, &sub, run.size as f32, false);
        self.s.set_fill(None);
        self.s.set_stroke(None);
        self.s.pop();
        true
    }
    fn fallback(&self) -> Option<Fallback> {
        Some(self.fallback)
    }
    fn draw_pixels(&mut self, img: &deckcraft_render::Image, dest: Rect) {
        let Some(size) = Size::from_wh(dest.width() as f32, dest.height() as f32) else { return };
        let image = Image::from_rgba8(img.to_straight(), img.width, img.height);
        self.s.push_transform(&Transform::from_translate(dest.x0 as f32, dest.y0 as f32));
        self.s.draw_image(image, size);
        self.s.pop();
    }
}
