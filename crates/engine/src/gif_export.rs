//! Animated GIF of the deck (the Export… dialog; `file.export` with format `gif`).
//!
//! One frame per slide, drawn by the slide renderer and shown for `secondsPerSlide`. Every frame
//! gets its own 256-colour palette (exact when the slide has 256 colours or fewer, NeuQuant
//! otherwise), so a photo on one slide doesn't steal colours from the next. Transitions and
//! build animations are not animated: a slide appears as it looks when its builds have finished.
//!
//! All parameters are hostile: sizes, delays and slide lists are checked against fixed bounds, and
//! the total number of pixels encoded is capped, so a request can't exhaust memory or run for
//! hours.

use deckcraft_model::Presentation;
use serde_json::Value;

use crate::cmd::bad;
use crate::{EngineError, Result};

const CMD: &str = "file.export";

/// Size presets, by frame width in pixels (16:9 slides: 640×360, 960×540, 1280×720, 1920×1080).
pub const SIZE_PRESETS: [(&str, u32); 4] = [("small", 640), ("medium", 960), ("large", 1280), ("extraLarge", 1920)];
/// Frame width when none is given (the `medium` preset).
pub const DEFAULT_WIDTH: u32 = 960;
pub const MIN_WIDTH: u32 = 16;
pub const MAX_WIDTH: u32 = 3840;
/// Longest frame side: a very tall slide is scaled down to fit.
pub const MAX_SIDE: u32 = 3840;
pub const DEFAULT_SECONDS: f64 = 5.0;
/// GIF delays are in hundredths of a second; viewers treat delays under 0.02 s as 0.1 s anyway.
pub const MIN_SECONDS: f64 = 0.1;
pub const MAX_SECONDS: f64 = 600.0;
pub const MAX_FRAMES: usize = 500;
/// Cap on frames × width × height (≈ 96 Full HD frames or 480 frames at 960×540).
pub const MAX_TOTAL_PIXELS: u64 = 200_000_000;
/// NeuQuant sampling: 1 is slowest and best, 30 fastest; 10 is the usual balance.
const QUANTIZER_SPEED: i32 = 10;

/// What to export.
#[derive(Clone, Debug, PartialEq)]
pub struct GifOptions {
    /// Frame width in pixels; the height follows the slide's aspect ratio.
    pub width: u32,
    pub seconds_per_slide: f64,
    /// Loop forever (`true`) or play once.
    pub loop_forever: bool,
    /// Slide indices in play order (default: all).
    pub slides: Option<Vec<usize>>,
    /// Hidden slides are left out unless this is set.
    pub include_hidden: bool,
}

impl Default for GifOptions {
    fn default() -> Self {
        GifOptions { width: DEFAULT_WIDTH, seconds_per_slide: DEFAULT_SECONDS, loop_forever: true, slides: None, include_hidden: false }
    }
}

/// An encoded GIF and what is in it.
#[derive(Clone, Debug)]
pub struct GifOutput {
    pub bytes: Vec<u8>,
    pub frames: usize,
    pub width: u32,
    pub height: u32,
    /// Per-frame delay in hundredths of a second, as written.
    pub delay_cs: u16,
}

fn width_value(v: &Value) -> Result<u32> {
    let err =
        || bad(CMD, format!("`width` must be a whole number of pixels from {MIN_WIDTH} to {MAX_WIDTH}, or one of small, medium, large, extraLarge"));
    match v {
        Value::String(s) => SIZE_PRESETS.iter().find(|(k, _)| k.eq_ignore_ascii_case(s.trim())).map(|(_, w)| *w).ok_or_else(err),
        Value::Number(n) => n.as_u64().and_then(|w| u32::try_from(w).ok()).filter(|w| (MIN_WIDTH..=MAX_WIDTH).contains(w)).ok_or_else(err),
        _ => Err(err()),
    }
}

/// Check a per-slide duration (NaN, infinities and out-of-range values are errors).
pub fn check_seconds(s: f64) -> Result<f64> {
    if (MIN_SECONDS..=MAX_SECONDS).contains(&s) {
        Ok(s)
    } else {
        Err(bad(CMD, format!("`secondsPerSlide` must be from {MIN_SECONDS} to {MAX_SECONDS}")))
    }
}

/// Present and not `null`.
fn given<'a>(p: &'a Value, key: &str) -> Option<&'a Value> {
    p.get(key).filter(|v| !v.is_null())
}

impl GifOptions {
    /// Read `{width?: px | small|medium|large|extraLarge, secondsPerSlide?, loop?: bool |
    /// forever|once, slides?: [index], includeHidden?}`. Absent or `null` values take their
    /// defaults; anything else that is out of range or of the wrong type is an error.
    pub fn from_params(p: &Value) -> Result<Self> {
        let mut o = GifOptions::default();
        if let Some(v) = given(p, "width") {
            o.width = width_value(v)?;
        }
        if let Some(v) = given(p, "secondsPerSlide") {
            let s = v.as_f64().ok_or_else(|| bad(CMD, "`secondsPerSlide` must be a number"))?;
            o.seconds_per_slide = check_seconds(s)?;
        }
        if let Some(v) = given(p, "loop") {
            o.loop_forever = match v {
                Value::Bool(b) => *b,
                Value::String(s) if s.eq_ignore_ascii_case("forever") => true,
                Value::String(s) if s.eq_ignore_ascii_case("once") => false,
                _ => return Err(bad(CMD, "`loop` must be true (forever), false (once), \"forever\" or \"once\"")),
            };
        }
        if let Some(v) = given(p, "slides") {
            let a = v.as_array().ok_or_else(|| bad(CMD, "`slides` must be a list of slide indices"))?;
            if a.is_empty() {
                return Err(bad(CMD, "`slides` is empty"));
            }
            if a.len() > MAX_FRAMES {
                return Err(bad(CMD, format!("a GIF can have at most {MAX_FRAMES} slides")));
            }
            let list = a
                .iter()
                .map(|x| x.as_u64().and_then(|i| usize::try_from(i).ok()))
                .collect::<Option<Vec<usize>>>()
                .ok_or_else(|| bad(CMD, "`slides` must be a list of slide indices (0, 1, 2…)"))?;
            o.slides = Some(list);
        }
        if let Some(v) = given(p, "includeHidden") {
            o.include_hidden = v.as_bool().ok_or_else(|| bad(CMD, "`includeHidden` must be true or false"))?;
        }
        Ok(o)
    }

    /// The slides that become frames, in order: the chosen ones (or all), without hidden slides
    /// unless `include_hidden`.
    pub fn frames(&self, doc: &Presentation) -> Result<Vec<usize>> {
        let n = doc.slides.len();
        let chosen: Vec<usize> = match &self.slides {
            Some(list) => {
                if let Some(i) = list.iter().find(|i| **i >= n) {
                    return Err(bad(CMD, format!("no slide {i} (the presentation has {n})")));
                }
                list.clone()
            }
            None => (0..n).collect(),
        };
        let frames: Vec<usize> = chosen.into_iter().filter(|i| self.include_hidden || doc.slides.get(*i).is_some_and(|s| !s.hidden)).collect();
        if frames.is_empty() {
            return Err(bad(CMD, "no slides to export (they are all hidden, or the presentation is empty)"));
        }
        if frames.len() > MAX_FRAMES {
            return Err(bad(CMD, format!("a GIF can have at most {MAX_FRAMES} slides; choose a range")));
        }
        Ok(frames)
    }

    /// Delay per frame in hundredths of a second.
    pub fn delay_cs(&self) -> Result<u16> {
        let s = check_seconds(self.seconds_per_slide)?;
        // In range by `check_seconds`: 10..=60_000.
        Ok((s * 100.0).round().clamp(1.0, f64::from(u16::MAX)) as u16)
    }
}

/// Frame size for a slide of `doc`'s size at `width` px: the height follows the aspect ratio and
/// no side exceeds [`MAX_SIDE`].
pub fn frame_size(doc: &Presentation, width: u32) -> (u32, u32) {
    let (sw, sh) = (doc.slide_size.width, doc.slide_size.height);
    let aspect = if sw.is_finite() && sh.is_finite() && sw > 0.0 && sh > 0.0 { sh / sw } else { 9.0 / 16.0 };
    let w = f64::from(width.clamp(MIN_WIDTH, MAX_WIDTH));
    let mut h = w * aspect;
    let mut w2 = w;
    if h > f64::from(MAX_SIDE) {
        w2 = w * f64::from(MAX_SIDE) / h;
        h = f64::from(MAX_SIDE);
    }
    let side = |v: f64| if v.is_finite() { v.round().clamp(1.0, f64::from(MAX_SIDE)) as u32 } else { 1 };
    (side(w2), side(h))
}

/// Straight RGBA of a premultiplied render composited onto white (GIF has no partial alpha, and
/// fully transparent pixels would otherwise become see-through).
fn on_white(img: &deckcraft_render::Image, w: u32, h: u32) -> Vec<u8> {
    let len = (w as usize).saturating_mul(h as usize).saturating_mul(4);
    if img.width != w || img.height != h || img.pixels.len() != len {
        log::error!("GIF export: a slide rendered at {}×{} instead of {w}×{h}; writing a blank frame", img.width, img.height);
        return vec![255; len];
    }
    let mut px = img.pixels.clone();
    for p in px.as_chunks_mut::<4>().0 {
        let back = 255 - p[3];
        p[0] = p[0].saturating_add(back);
        p[1] = p[1].saturating_add(back);
        p[2] = p[2].saturating_add(back);
        p[3] = 255;
    }
    px
}

/// Render and encode the GIF.
pub fn encode(doc: &Presentation, opts: &GifOptions) -> Result<GifOutput> {
    use image::codecs::gif::{GifEncoder, Repeat};

    let frames = opts.frames(doc)?;
    let delay_cs = opts.delay_cs()?;
    let (w, h) = frame_size(doc, opts.width);
    let total = (frames.len() as u64).saturating_mul(u64::from(w)).saturating_mul(u64::from(h));
    if total > MAX_TOTAL_PIXELS {
        return Err(bad(CMD, format!("{} slides at {w}×{h} is too large for a GIF; choose a smaller size or fewer slides", frames.len())));
    }
    let scale = f64::from(w) / doc.slide_size.width;
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let enc_err = |e: image::ImageError| EngineError::Other(format!("GIF encoding failed: {e}"));
    let mut bytes = Vec::new();
    {
        let mut enc = GifEncoder::new_with_speed(&mut bytes, QUANTIZER_SPEED);
        // No repeat block at all means "play once"; a repeat count of 0 means "forever".
        if opts.loop_forever {
            enc.set_repeat(Repeat::Infinite).map_err(enc_err)?;
        }
        for i in &frames {
            let img = deckcraft_render::render_slide(doc, *i, &deckcraft_render::RenderOpts { scale, size: Some((w, h)), ..Default::default() });
            let buffer = image::RgbaImage::from_raw(w, h, on_white(&img, w, h))
                .ok_or_else(|| EngineError::Other(format!("GIF export: slide {i} has no pixels")))?;
            let delay = image::Delay::from_numer_denom_ms(u32::from(delay_cs) * 10, 1);
            enc.encode_frame(image::Frame::from_parts(buffer, 0, 0, delay)).map_err(enc_err)?;
        }
    }
    Ok(GifOutput { bytes, frames: frames.len(), width: w, height: h, delay_cs })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use image::AnimationDecoder;
    use serde_json::json;

    use super::*;
    use crate::Session;

    const COLOURS: [(&str, [u8; 3]); 4] =
        [("#E02020", [0xE0, 0x20, 0x20]), ("#20A040", [0x20, 0xA0, 0x40]), ("#2040E0", [0x20, 0x40, 0xE0]), ("#F0C000", [0xF0, 0xC0, 0x00])];

    /// Four blank slides with solid backgrounds (red, green, blue, yellow); slide 2 (blue) hidden.
    fn deck() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({"blank": true})).unwrap();
        while s.doc().unwrap().doc.slides.len() < COLOURS.len() {
            s.execute("slide.new", &json!({})).unwrap();
        }
        for (i, (hex, _)) in COLOURS.iter().enumerate() {
            s.execute("design.background", &json!({"color": hex, "index": i})).unwrap();
        }
        s.execute("slide.hide", &json!({"index": 2, "hidden": true})).unwrap();
        s
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("deckcraft-gif-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn decode(bytes: &[u8]) -> (image::metadata::LoopCount, Vec<image::Frame>) {
        let dec = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).unwrap();
        let count = dec.loop_count();
        (count, dec.into_frames().collect_frames().unwrap())
    }

    /// The NETSCAPE2.0 application block's loop count, if the GIF has one.
    fn netscape_loops(bytes: &[u8]) -> Option<u16> {
        let tag = b"NETSCAPE2.0";
        let at = bytes.windows(tag.len()).position(|w| w == tag)?;
        // NETSCAPE2.0, sub-block size 3, id 1, loop count (little endian).
        let b = bytes.get(at + tag.len()..at + tag.len() + 4)?;
        Some(u16::from_le_bytes([b[2], b[3]]))
    }

    fn close(a: [u8; 4], b: [u8; 3]) -> bool {
        (0..3).all(|k| (i32::from(a[k]) - i32::from(b[k])).abs() <= 12) && a[3] == 255
    }

    #[test]
    fn exports_one_frame_per_visible_slide() {
        let mut s = deck();
        let path = temp("deck.gif");
        let r = s.execute("file.export", &json!({"path": path.to_string_lossy(), "width": 320, "secondsPerSlide": 1.5})).unwrap();
        assert_eq!(r["frames"], 3, "{r}");
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"GIF89a"));
        assert_eq!(netscape_loops(&bytes), Some(0), "loops forever by default");
        let (count, frames) = decode(&bytes);
        assert!(matches!(count, image::metadata::LoopCount::Infinite));
        assert_eq!(frames.len(), 3, "the hidden slide is skipped");
        // Slides are 16:9: 320×180.
        let expected = [COLOURS[0].1, COLOURS[1].1, COLOURS[3].1];
        for (f, want) in frames.iter().zip(expected) {
            assert_eq!((f.buffer().width(), f.buffer().height()), (320, 180));
            assert_eq!(f.delay().numer_denom_ms(), (1500, 1));
            let px = f.buffer().get_pixel(160, 90).0;
            assert!(close(px, want), "frame colour {px:?}, expected {want:?}");
        }
        assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(320), Some(180)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn presets_ranges_hidden_slides_and_play_once() {
        let s = deck();
        let doc = &s.doc().unwrap().doc;
        let o = GifOptions::from_params(&json!({"width": "small", "loop": false, "slides": [3, 2, 1], "includeHidden": true})).unwrap();
        let out = encode(doc, &o).unwrap();
        assert_eq!((out.frames, out.width, out.height, out.delay_cs), (3, 640, 360, 500));
        assert_eq!(netscape_loops(&out.bytes), None, "play once: no repeat block");
        let (_, frames) = decode(&out.bytes);
        let want = [COLOURS[3].1, COLOURS[2].1, COLOURS[1].1];
        for (f, w) in frames.iter().zip(want) {
            assert!(close(f.buffer().get_pixel(320, 180).0, w));
            assert_eq!(f.delay().numer_denom_ms(), (5000, 1), "5 seconds by default");
        }
        // A range that holds only the hidden slide has nothing to export.
        let o = GifOptions::from_params(&json!({"slides": [2]})).unwrap();
        assert!(encode(doc, &o).is_err());
        for (preset, w) in SIZE_PRESETS {
            assert_eq!(GifOptions::from_params(&json!({"width": preset})).unwrap().width, w);
        }
        assert_eq!(
            GifOptions::from_params(&json!({"width": "Large", "loop": "once"})).unwrap(),
            GifOptions { width: 1280, loop_forever: false, ..Default::default() }
        );
        assert_eq!(GifOptions::from_params(&json!({"width": null, "secondsPerSlide": null})).unwrap(), GifOptions::default());
    }

    #[test]
    fn hostile_params_are_errors_not_panics() {
        for p in [
            json!({"width": 0}),
            json!({"width": 15}),
            json!({"width": 3841}),
            json!({"width": -5}),
            json!({"width": 1e308}),
            json!({"width": 640.5}),
            json!({"width": "huge"}),
            json!({"width": [640]}),
            json!({"secondsPerSlide": 0}),
            json!({"secondsPerSlide": 0.05}),
            json!({"secondsPerSlide": -1}),
            json!({"secondsPerSlide": 601}),
            json!({"secondsPerSlide": 1e308}),
            json!({"secondsPerSlide": "5"}),
            json!({"loop": 1}),
            json!({"loop": "twice"}),
            json!({"slides": []}),
            json!({"slides": [-1]}),
            json!({"slides": [0.5]}),
            json!({"slides": "1-3"}),
            json!({"slides": vec![0; MAX_FRAMES + 1]}),
            json!({"includeHidden": "yes"}),
        ] {
            assert!(GifOptions::from_params(&p).is_err(), "{p}");
        }
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MIN_POSITIVE, 1e-300] {
            assert!(check_seconds(v).is_err(), "{v}");
            let o = GifOptions { seconds_per_slide: v, ..Default::default() };
            assert!(o.delay_cs().is_err(), "{v}");
        }
        assert_eq!(GifOptions { seconds_per_slide: 600.0, ..Default::default() }.delay_cs().unwrap(), 60_000);
        assert_eq!(GifOptions { seconds_per_slide: 0.1, ..Default::default() }.delay_cs().unwrap(), 10);

        // Through the command too: errors, the presentation untouched, no file written.
        let mut s = deck();
        let path = temp("hostile.gif");
        for p in [
            json!({"width": 99999}),
            json!({"secondsPerSlide": -3}),
            json!({"slides": [0, 99]}),
            json!({"slides": [18446744073709551615u64]}),
            json!({"slides": [2]}),
        ] {
            let mut p = p;
            p["path"] = json!(path.to_string_lossy());
            assert!(s.execute("file.export", &p).is_err(), "{p}");
        }
        assert!(!path.exists());
        assert_eq!(s.doc().unwrap().doc.slides.len(), 4);
    }

    #[test]
    fn sizes_are_bounded() {
        let mut s = deck();
        // A very tall slide is scaled down so no side exceeds MAX_SIDE.
        s.execute("design.slideSize", &json!({"w": 100.0, "h": 4000.0, "scale": "none"})).unwrap();
        let doc = &s.doc().unwrap().doc;
        let (w, h) = frame_size(doc, MAX_WIDTH);
        assert!(w >= 1 && h <= MAX_SIDE && w <= MAX_WIDTH, "{w}×{h}");
        // The total pixel budget stops huge requests before anything is rendered.
        let mut big = Presentation::clone(doc);
        big.slide_size = deckcraft_geom::Size::new(960.0, 540.0);
        let slide = big.slides[0].clone();
        big.slides = vec![slide; 200];
        let o = GifOptions { width: MAX_WIDTH, ..Default::default() };
        let e = encode(&big, &o).unwrap_err().to_string();
        assert!(e.contains("too large"), "{e}");
        // A degenerate slide size falls back to 16:9 rather than dividing by zero.
        let mut odd = Presentation::clone(doc);
        odd.slide_size = deckcraft_geom::Size::new(0.0, f64::NAN);
        assert_eq!(frame_size(&odd, 640), (640, 360));
    }

    #[test]
    fn gif_is_also_a_save_format_and_inferred_from_the_path() {
        let s = deck();
        let bytes = crate::cmd::file::save_bytes(&s.doc().unwrap().doc, "gif").unwrap();
        assert!(bytes.starts_with(b"GIF89a"));
        assert_eq!(crate::cmd::file::format_for_path("Talk.GIF"), "gif");
        let bytes = crate::cmd::file::gif_bytes(&s.doc().unwrap().doc, &json!({"width": 64, "slides": [0]})).unwrap();
        assert_eq!(decode(&bytes).1.len(), 1);
    }
}
