//! Embedded bitmap glyphs (`sbix` and `CBDT` PNG strikes).
//!
//! Apple Color Emoji stores each emoji as a PNG in an `sbix` strike. `COLR` painting does not
//! see those, so a colour face would otherwise fall through to its outline (often empty).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use deckcraft_fonts::FontFace;
use kurbo::{Affine, Rect};
use skrifa::bitmap::{BitmapData, Origin};
use skrifa::instance::Size;
use skrifa::{GlyphId, MetadataProvider};
use vello_cpu::{RenderContext, peniko};

/// A decoded strike is reused: a slide show paints the same emoji every frame.
const CACHE_MAX: usize = 128;
/// One PNG larger than this is left undrawn (a hostile or corrupt strike).
const MAX_PNG: usize = 4 << 20;
const MAX_SIDE: u32 = 2048;

struct StrikePx {
    pm: Arc<vello_cpu::Pixmap>,
    /// Where the bitmap sits, in y-down font units, origin on the baseline.
    dest: Rect,
}

static CACHE: Mutex<Option<HashMap<(u32, u32, u32, u32), StrikePx>>> = Mutex::new(None);

/// Paint `gid` of `face` from an embedded PNG strike. `at` maps the glyph origin (y down, points)
/// and `scale` is points per font unit. False when the glyph has no PNG strike, so the caller
/// draws the outline. Mask and raw BGRA strikes are not drawn.
pub(crate) fn paint(ctx: &mut RenderContext, face: &FontFace, gid: u32, at: Affine, scale: f64, alpha: f64) -> bool {
    let ppem = scale * face.upem;
    if !(ppem.is_finite() && ppem > 0.0 && ppem < 10_000.0) {
        return false;
    }
    let Some(font) = face.skrifa() else { return false };
    let Some(glyph) = font.bitmap_strikes().glyph_for_size(Size::new(ppem as f32), GlyphId::new(gid)) else {
        return false;
    };
    let key = (face.id(), gid, glyph.ppem_x.to_bits(), glyph.ppem_y.to_bits());
    let cached = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|c| c.get(&key).map(clone_strike));
    let Some(strike) = cached.or_else(|| decode_strike(face, &glyph)) else { return false };
    remember(key, &strike);
    blit(ctx, &strike, at, scale, alpha);
    true
}

fn clone_strike(s: &StrikePx) -> StrikePx {
    StrikePx { pm: s.pm.clone(), dest: s.dest }
}

fn remember(key: (u32, u32, u32, u32), strike: &StrikePx) {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    if cache.len() >= CACHE_MAX && !cache.contains_key(&key) {
        cache.clear();
    }
    cache.entry(key).or_insert_with(|| clone_strike(strike));
}

fn decode_strike(face: &FontFace, glyph: &skrifa::bitmap::BitmapGlyph<'_>) -> Option<StrikePx> {
    let BitmapData::Png(png) = &glyph.data else { return None };
    if png.is_empty() || png.len() > MAX_PNG {
        return None;
    }
    let rgba = image::load_from_memory(png).ok()?.into_rgba8();
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return None;
    }
    let pm = crate::images::premul_pixmap(&rgba)?;
    let (ux, uy) = (face.upem / glyph.ppem_x as f64, face.upem / glyph.ppem_y as f64);
    if !(ux.is_finite() && uy.is_finite() && ux > 0.0 && uy > 0.0) {
        return None;
    }
    let (bw, bh) = (w as f64 * ux, h as f64 * uy);
    // Same placement as Skia's fontations scaler (checked against CoreText): outer bearings are
    // font units, inner bearings are pixels, and an `sbix` strike's origin is the PNG's bottom left.
    let x0 = glyph.bearing_x as f64 + glyph.inner_bearing_x as f64 * ux;
    let mut y0 = -(glyph.bearing_y as f64) - glyph.inner_bearing_y as f64 * uy;
    if glyph.placement_origin == Origin::BottomLeft {
        y0 -= bh;
    }
    let dest = Rect::new(x0, y0, x0 + bw, y0 + bh);
    if !(dest.width().is_finite() && dest.height().is_finite() && dest.width() > 0.0 && dest.height() > 0.0) {
        return None;
    }
    Some(StrikePx { pm, dest })
}

fn blit(ctx: &mut RenderContext, strike: &StrikePx, at: Affine, scale: f64, alpha: f64) {
    let (iw, ih) = (strike.pm.width() as f64, strike.pm.height() as f64);
    let image = vello_cpu::Image {
        image: vello_cpu::ImageSource::Pixmap(strike.pm.clone()),
        sampler: peniko::ImageSampler { quality: peniko::ImageQuality::High, ..Default::default() },
    };
    let m = at * Affine::scale(scale);
    let t = m * Affine::translate(strike.dest.origin().to_vec2()) * Affine::scale_non_uniform(strike.dest.width() / iw, strike.dest.height() / ih);
    ctx.set_transform(t);
    let a = alpha.clamp(0.0, 1.0);
    if a < 0.999 {
        ctx.push_layer(None, None, Some(a as f32), None, None);
    }
    ctx.set_paint(image);
    ctx.fill_rect(&Rect::new(0.0, 0.0, iw, ih));
    if a < 0.999 {
        ctx.pop_layer();
    }
}
