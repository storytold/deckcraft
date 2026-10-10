//! Decoded pictures shared by every render on this process, keyed by the media bytes' address and
//! the requested size level and adjustments, so each picture is decoded once per size.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use deckcraft_model::style::PictureAdjust;
use vello_cpu::Pixmap;

const BUDGET: usize = 512 << 20;

struct Entry {
    _bytes: Arc<Vec<u8>>,
    pm: Arc<Pixmap>,
    stamp: u64,
}

#[derive(Default)]
struct Cache {
    map: HashMap<(usize, u32, u64), Entry>,
    bytes: usize,
    clock: u64,
    bad: HashMap<usize, Arc<Vec<u8>>>,
    /// GIFs by bytes address and adjustments (`None`: not animated); the bytes stay alive with them.
    anims: HashMap<(usize, u64), (Arc<Vec<u8>>, Option<Arc<Anim>>)>,
}

/// Longest side, total decoded size and frame count kept for one animated GIF; the largest canvas
/// decoded at all; and the decoded size of all cached GIFs together. Hostile GIFs (millions of
/// tiny frames, a huge canvas) stop at these instead of exhausting memory.
const GIF_SIDE: u32 = 1024;
const GIF_BUDGET: usize = 64 << 20;
const GIF_MAX_FRAMES: usize = 1000;
const GIF_MAX_CANVAS: u64 = 4096 * 4096;
const GIF_CACHE_BUDGET: usize = 192 << 20;

/// The frames of an animated GIF and when each one ends, in seconds from the start of a loop.
struct Anim {
    frames: Vec<Arc<Pixmap>>,
    ends: Vec<f64>,
}

impl Anim {
    /// Decoded bytes held by the frames.
    fn bytes(&self) -> usize {
        self.frames.iter().map(|f| f.width() as usize * f.height() as usize * 4).sum()
    }

    /// The frame showing `t` seconds into the (looping) animation and the seconds until the next.
    fn at(&self, t: f64) -> (usize, f64) {
        let total = self.ends.last().copied().unwrap_or(0.0);
        if total <= 0.0 || !t.is_finite() {
            return (0, 1.0);
        }
        let local = t.max(0.0) % total;
        let i = self.ends.partition_point(|e| *e <= local).min(self.ends.len().saturating_sub(1));
        (i, (self.ends.get(i).copied().unwrap_or(total) - local).max(0.001))
    }
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Cache) -> R) -> R {
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Cache::default))
}

fn adjust_key(a: &PictureAdjust) -> u64 {
    let mut h: u64 = 1469598103934665603;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(1099511628211);
    };
    mix(a.brightness.to_bits());
    mix(a.contrast.to_bits());
    mix(a.saturation.unwrap_or(1.0).to_bits());
    mix(a.grayscale as u64);
    mix(a.sharpen.to_bits());
    if let Some(c) = a.clear_color {
        mix(((c.r as u64) << 16) | ((c.g as u64) << 8) | c.b as u64 | 1 << 40);
    }
    h
}

/// Decode encoded bytes (PNG, JPEG, GIF, WebP, BMP, TIFF) to straight RGBA.
pub fn decode(bytes: &[u8]) -> Option<image::RgbaImage> {
    if bytes.len() > 512 << 20 {
        return None;
    }
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 || (w as u64) * (h as u64) > 400_000_000 {
        return None;
    }
    Some(img.to_rgba8())
}

/// A premultiplied pixmap of the picture, downsampled so its longer side is near `want` px.
pub fn decode_for(bytes: &Arc<Vec<u8>>, want: f64, adj: &PictureAdjust) -> Option<Arc<Pixmap>> {
    let addr = Arc::as_ptr(bytes) as usize;
    if with(|c| c.bad.contains_key(&addr)) {
        return None;
    }
    // Power-of-two size buckets so zooming doesn't re-decode every frame.
    let bucket = (want.max(16.0).log2().ceil() as u32).min(15);
    let key = (addr, bucket, adjust_key(adj));
    if let Some(pm) = with(|c| {
        c.clock += 1;
        let clock = c.clock;
        c.map.get_mut(&key).map(|e| {
            e.stamp = clock;
            e.pm.clone()
        })
    }) {
        return Some(pm);
    }
    let Some(mut img) = decode(bytes) else {
        with(|c| {
            c.bad.insert(addr, bytes.clone());
        });
        return None;
    };
    let target = 1u32 << bucket;
    let (w, h) = img.dimensions();
    if w.max(h) > target.max(16) && w.max(h) > 4096.min(target * 2) {
        let k = target as f64 / w.max(h) as f64;
        let (nw, nh) = (((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1));
        img = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle);
    }
    apply_adjust(&mut img, adj);
    let (w, h) = img.dimensions();
    let pm = pixmap(&img)?;
    with(|c| {
        c.clock += 1;
        let n = w as usize * h as usize * 4;
        c.bytes += n;
        c.map.insert(key, Entry { _bytes: bytes.clone(), pm: pm.clone(), stamp: c.clock });
        while c.bytes > BUDGET && c.map.len() > 1 {
            let Some(oldest) = c.map.iter().min_by_key(|(_, e)| e.stamp).map(|(k, _)| *k) else { break };
            if let Some(e) = c.map.remove(&oldest) {
                c.bytes = c.bytes.saturating_sub(e.pm.width() as usize * e.pm.height() as usize * 4);
            }
        }
    });
    Some(pm)
}

/// Straight RGBA to a premultiplied pixmap.
fn pixmap(img: &image::RgbaImage) -> Option<Arc<Pixmap>> {
    let (w, h) = img.dimensions();
    if w > u16::MAX as u32 || h > u16::MAX as u32 {
        return None;
    }
    let data: Vec<vello_cpu::color::PremulRgba8> = img
        .pixels()
        .map(|p| {
            let a = p[3] as u16;
            let m = |c: u8| ((c as u16 * a + 127) / 255) as u8;
            vello_cpu::color::PremulRgba8 { r: m(p[0]), g: m(p[1]), b: m(p[2]), a: p[3] }
        })
        .collect();
    Some(Arc::new(Pixmap::from_parts(data, w as u16, h as u16)))
}

/// Decode every frame of an animated GIF (`None` for a still or broken one).
fn decode_anim(bytes: &[u8], adj: &PictureAdjust) -> Option<Anim> {
    use image::{AnimationDecoder, ImageDecoder};
    let mut dec = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    dec.set_limits(image::Limits::default()).ok()?;
    let (w, h) = dec.dimensions();
    if w == 0 || h == 0 || (w as u64) * (h as u64) > GIF_MAX_CANVAS {
        return None;
    }
    let k = (GIF_SIDE as f64 / w.max(h) as f64).min(1.0);
    let (nw, nh) = (((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1));
    let size = nw as usize * nh as usize * 4;
    let (mut frames, mut ends, mut t) = (Vec::new(), Vec::new(), 0.0);
    for f in dec.into_frames() {
        let Ok(f) = f else { break };
        if frames.len() >= GIF_MAX_FRAMES || (frames.len() + 1) * size > GIF_BUDGET {
            log::info!("animated GIF: keeping its first {} frames", frames.len());
            break;
        }
        let (n, d) = f.delay().numer_denom_ms();
        let ms = if d == 0 { 0.0 } else { n as f64 / d as f64 };
        // Like browsers: frames of 10 ms or less show for 100 ms.
        t += if ms <= 10.0 { 0.1 } else { ms / 1000.0 };
        let mut img = f.into_buffer();
        if img.dimensions() != (nw, nh) {
            img = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle);
        }
        apply_adjust(&mut img, adj);
        frames.push(pixmap(&img)?);
        ends.push(t);
    }
    (frames.len() > 1).then_some(Anim { frames, ends })
}

/// The decoded animation of GIF bytes, once per bytes and adjustments.
fn anim(bytes: &Arc<Vec<u8>>, adj: &PictureAdjust) -> Option<Arc<Anim>> {
    if !bytes.starts_with(b"GIF8") {
        return None;
    }
    let key = (Arc::as_ptr(bytes) as usize, adjust_key(adj));
    if let Some(a) = with(|c| c.anims.get(&key).map(|e| e.1.clone())) {
        return a;
    }
    let a = decode_anim(bytes, adj).map(Arc::new);
    with(|c| {
        let held: usize = c.anims.values().filter_map(|e| e.1.as_ref()).map(|a| a.bytes()).sum();
        if held + a.as_ref().map_or(0, |a| a.bytes()) > GIF_CACHE_BUDGET || c.anims.len() >= 64 {
            c.anims.clear();
        }
        c.anims.insert(key, (bytes.clone(), a.clone()));
    });
    a
}

/// For an animated GIF: the frame showing `t` seconds into its (looping) play and the seconds until
/// the next frame. `None` for anything else.
pub fn gif_frame(bytes: &Arc<Vec<u8>>, adj: &PictureAdjust, t: f64) -> Option<(usize, f64)> {
    anim(bytes, adj).map(|a| a.at(t))
}

/// The frame of an animated GIF at `t` seconds.
pub(crate) fn gif_pixmap(bytes: &Arc<Vec<u8>>, adj: &PictureAdjust, t: f64) -> Option<Arc<Pixmap>> {
    let a = anim(bytes, adj)?;
    a.frames.get(a.at(t).0).cloned()
}

fn apply_adjust(img: &mut image::RgbaImage, a: &PictureAdjust) {
    let sat = a.saturation.unwrap_or(1.0);
    if a.brightness == 0.0 && a.contrast == 0.0 && sat == 1.0 && !a.grayscale && a.clear_color.is_none() && a.duotone.is_none() {
        return;
    }
    let b = a.brightness.clamp(-1.0, 1.0) * 255.0;
    let c = a.contrast.clamp(-1.0, 1.0);
    let cf = if c >= 0.0 { 1.0 / (1.0 - c * 0.99) } else { 1.0 + c };
    for p in img.pixels_mut() {
        if let Some(cc) = a.clear_color
            && (p[0] as i32 - cc.r as i32).abs() < 8
            && (p[1] as i32 - cc.g as i32).abs() < 8
            && (p[2] as i32 - cc.b as i32).abs() < 8
        {
            p[3] = 0;
            continue;
        }
        let mut rgb = [p[0] as f64, p[1] as f64, p[2] as f64];
        let y = 0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2];
        let s = if a.grayscale { 0.0 } else { sat };
        for v in &mut rgb {
            *v = y + (*v - y) * s;
            *v = (*v - 128.0) * cf + 128.0 + b;
        }
        for (i, v) in rgb.iter().enumerate() {
            if let Some(ch) = p.0.get_mut(i) {
                *ch = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_gif_of_many_tiny_frames_keeps_only_the_frame_cap() {
        use image::{Delay, Frame, Rgba, RgbaImage};
        let frames = (0..GIF_MAX_FRAMES + 200).map(|i| {
            let c = if i % 2 == 0 { [255, 0, 0, 255] } else { [0, 0, 255, 255] };
            Frame::from_parts(RgbaImage::from_pixel(1, 1, Rgba(c)), 0, 0, Delay::from_numer_denom_ms(20, 1))
        });
        let mut out = Vec::new();
        image::codecs::gif::GifEncoder::new(&mut out).encode_frames(frames).expect("gif");
        let a = decode_anim(&out, &PictureAdjust::default()).expect("animated");
        assert_eq!(a.frames.len(), GIF_MAX_FRAMES);
        assert_eq!(a.ends.len(), GIF_MAX_FRAMES);
    }
}
