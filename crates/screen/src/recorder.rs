//! The screen-recording encoder: BGRA frames in, a progressive MP4 out.
//!
//! Frames are converted to RGBA (stripping the backend's row padding), cropped to the selected
//! region, trimmed to even sides and encoded with DeckCraft's H.264 encoder; the NAL units are
//! muxed by the craft-media MP4 writer. Recording is streamed: the sink is a file in the engine
//! (not a growing `Vec`), so a long recording costs no more memory than one frame plus the
//! encoder's reference picture.

use std::io::{Read, Seek, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use deckcraft_h264::{Encoder, EncoderConfig};
use deckcraft_isobmff::{AvcConfig, Brand, CodecConfig, FourCc, Mp4Writer, SampleEntry, TrackConfig, WriteSample, WriterOptions};

use crate::{CaptureRegion, ScreenFrame};

/// Longest recording: 2 hours at the fastest capture rate (60 fps). Frames past it are refused.
pub const MAX_FRAMES: u64 = 2 * 60 * 60 * 60;
/// Largest recording file: 1 GiB. The sink refuses a write past it, so a take can never fill the
/// disk however long the screen changes; two hours at 60 fps of a busy screen reaches it first,
/// and the take keeps what it recorded up to that point.
pub const MAX_BYTES: u64 = 1 << 30;
/// Largest recording side: the H.264 encoder's limit.
pub const MAX_SIDE: u32 = 8192;
/// The H.264 encoder's smallest side.
const MIN_SIDE: u32 = 16;
/// H.264 level 5.1's picture budget in macroblocks (an 8192×1152 picture). PowerPoint's decoder
/// can refuse a bigger level, and a screen recording is played there before anywhere else, so a
/// bigger capture is recorded smaller rather than at a level it may not play. Level 5.1 allows
/// 36,864 macroblocks in a picture and 983,040 per second, so the frame rate counts too: 4K at
/// 60 fps is over the per-second budget even though one of its pictures fits.
const LEVEL_51_MAX_MBS: u64 = 36_864;
/// Level 5.1's macroblock rate (macroblocks per second).
const LEVEL_51_MAX_MBS_PER_S: u64 = 983_040;
/// The largest whole downscale the recorder applies (8K records at 4K).
const MAX_DOWNSCALE: u32 = 8;
/// A key frame at least this often (seconds), so players can seek.
const KEY_EVERY_S: u32 = 2;

/// A seekable sink for the MP4 writer: a file in the engine, a cursor in tests.
pub trait RecorderSink: Read + Write + Seek + Send {}
impl<T: Read + Write + Seek + Send> RecorderSink for T {}

/// A sink that refuses a write past a byte limit, so a take cannot fill the disk. The writer sees
/// a plain I/O error; the recorder maps the shared `over` flag to [`RecorderError::TooLarge`].
struct LimitedSink {
    inner: Box<dyn RecorderSink>,
    limit: u64,
    bytes: Arc<AtomicU64>,
    over: Arc<AtomicBool>,
}

impl Write for LimitedSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.bytes.load(Ordering::Relaxed);
        if written.saturating_add(buf.len() as u64) > self.limit {
            self.over.store(true, Ordering::Relaxed);
            return Err(std::io::Error::other("the screen recording reached its size limit"));
        }
        let n = self.inner.write(buf)?;
        self.bytes.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl Read for LimitedSink {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Seek for LimitedSink {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

/// What to record and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecorderConfig {
    /// The part of each captured frame to keep; `None` keeps all of it.
    pub region: Option<CaptureRegion>,
    /// Frames per second; each pushed frame lasts one tick.
    pub fps: u32,
    /// H.264 quantisation parameter, 10–51 (lower is larger and finer).
    pub quality: u8,
    /// A key frame every this many frames.
    pub key_frame_interval: u32,
    /// The most bytes the file may take; a write past it fails with
    /// [`RecorderError::TooLarge`](RecorderError::TooLarge).
    pub max_bytes: u64,
}

impl RecorderConfig {
    /// A config for `fps`. The rate is not clamped here: the command boundary validates it once
    /// and [`ScreenRecorder::new`] refuses a rate it cannot record, so no layer silently records
    /// at another rate than the caller asked for.
    pub fn new(fps: u32) -> Self {
        RecorderConfig { region: None, fps, quality: 24, key_frame_interval: fps.saturating_mul(KEY_EVERY_S).max(1), max_bytes: MAX_BYTES }
    }
}

/// What a finished recording contains.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecorderOutput {
    pub frames: u64,
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
}

/// Why a recording can't be written.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RecorderError {
    #[error("nothing was recorded")]
    Empty,
    #[error("the captured frame is malformed")]
    BadFrame,
    #[error("the recording region is outside the captured frame")]
    BadRegion,
    #[error("a recording can be at most two hours long")]
    TooLong,
    #[error("the recording reached its size limit")]
    TooLarge,
    #[error("{0}")]
    Encoder(String),
    #[error("{0}")]
    Muxer(String),
    #[error("{0}")]
    Io(String),
}

/// One recording in progress.
pub struct ScreenRecorder {
    sink: Option<Box<dyn RecorderSink>>,
    cfg: RecorderConfig,
    encoder: Option<Encoder>,
    mux: Option<Mp4Writer<Box<dyn RecorderSink>>>,
    track: usize,
    size: (u32, u32),
    /// Whole-number downscale of the captured picture (1 = recorded as captured).
    scale: u32,
    frames: u64,
    /// Bytes the sink has accepted, shared with the size-capped writer.
    bytes: Arc<AtomicU64>,
    /// The size cap was reached: every later push answers [`RecorderError::TooLarge`].
    over: Arc<AtomicBool>,
    /// Reused BGRA→RGBA buffer (one frame).
    rgba: Vec<u8>,
}

impl ScreenRecorder {
    /// A recorder that writes to `sink`. The picture size is taken from the first frame (or the
    /// configured region), so a wrong first frame fails at [`push`](Self::push), not here.
    pub fn new(sink: Box<dyn RecorderSink>, cfg: RecorderConfig) -> Result<Self, RecorderError> {
        if !(1..=60).contains(&cfg.fps) {
            return Err(RecorderError::Encoder("the capture rate must be from 1 to 60 frames per second".into()));
        }
        Ok(ScreenRecorder {
            sink: Some(sink),
            cfg,
            encoder: None,
            mux: None,
            track: 0,
            size: (0, 0),
            scale: 1,
            frames: 0,
            bytes: Arc::new(AtomicU64::new(0)),
            over: Arc::new(AtomicBool::new(false)),
            rgba: Vec::new(),
        })
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Encode one captured frame. Frames past [`MAX_FRAMES`] are refused.
    pub fn push(&mut self, frame: &ScreenFrame) -> Result<(), RecorderError> {
        if self.frames >= MAX_FRAMES {
            return Err(RecorderError::TooLong);
        }
        if self.over.load(Ordering::Relaxed) {
            return Err(RecorderError::TooLarge);
        }
        let (x, y, w, h) = self.crop_for(frame)?;
        let (w, h) = (w & !1, h & !1);
        if w < MIN_SIDE || h < MIN_SIDE {
            return Err(RecorderError::BadRegion);
        }
        let (n, ow, oh) = self.scaled(w, h);
        if ow < MIN_SIDE || oh < MIN_SIDE {
            return Err(RecorderError::BadRegion);
        }
        if self.encoder.is_none() {
            self.scale = n;
            self.open(ow, oh)?;
        } else if self.size != (ow, oh) {
            // The capture output changed size mid-recording (a display was rotated): refuse
            // rather than mux samples that don't match the track's sample entry.
            return Err(RecorderError::BadFrame);
        }
        self.fill_rgba(frame, x, y, w, h);
        let key = self.frames == 0 || self.frames.is_multiple_of(u64::from(self.cfg.key_frame_interval.max(1)));
        let Some(encoder) = self.encoder.as_mut() else { return Err(RecorderError::Encoder("the encoder is not ready".into())) };
        let pic = encoder.encode_rgba(&self.rgba, key);
        let data = pic.length_prefixed();
        self.write_sample(&data, pic.key)?;
        Ok(())
    }

    /// Mux one sample, turning a write the size cap refused into [`RecorderError::TooLarge`].
    fn write_sample(&mut self, data: &[u8], is_sync: bool) -> Result<(), RecorderError> {
        let Some(mux) = self.mux.as_mut() else { return Err(RecorderError::Muxer("the muxer is not ready".into())) };
        let r = mux.write_sample(self.track, WriteSample { data, duration: 1, composition_offset: 0, is_sync });
        if self.over.load(Ordering::Relaxed) {
            return Err(RecorderError::TooLarge);
        }
        r.map_err(|e| RecorderError::Muxer(e.to_string()))?;
        self.frames += 1;
        Ok(())
    }

    /// Record a picture that did not change: an all-skip P picture, which is its own reference
    /// picture (`frame_num` advances) and costs a header instead of the BGRA→RGBA, RGB→YUV and
    /// macroblock passes. `BadFrame` when a key frame is due (the caller encodes the picture) or
    /// nothing has been encoded yet.
    pub fn push_repeat(&mut self) -> Result<(), RecorderError> {
        if self.frames >= MAX_FRAMES {
            return Err(RecorderError::TooLong);
        }
        if self.over.load(Ordering::Relaxed) {
            return Err(RecorderError::TooLarge);
        }
        if self.frames == 0 || self.frames.is_multiple_of(u64::from(self.cfg.key_frame_interval.max(1))) {
            return Err(RecorderError::BadFrame);
        }
        let Some(encoder) = self.encoder.as_mut() else { return Err(RecorderError::Encoder("the encoder is not ready".into())) };
        let Some(pic) = encoder.encode_skip() else { return Err(RecorderError::BadFrame) };
        let data = pic.length_prefixed();
        self.write_sample(&data, pic.key)
    }

    /// The region to keep, checked against the frame. Rows and columns past the frame are clipped
    /// (a display can be a pixel smaller than the region the picker remembered).
    fn crop_for(&self, frame: &ScreenFrame) -> Result<(usize, usize, u32, u32), RecorderError> {
        if frame.width == 0 || frame.height == 0 || frame.stride < frame.width as usize * 4 {
            return Err(RecorderError::BadFrame);
        }
        if frame.data.len() < frame.stride.saturating_mul(frame.height as usize) {
            return Err(RecorderError::BadFrame);
        }
        let r = self.cfg.region.unwrap_or(CaptureRegion { x: 0, y: 0, width: frame.width, height: frame.height });
        if r.x < 0 || r.y < 0 {
            return Err(RecorderError::BadRegion);
        }
        let x = r.x as usize;
        let y = r.y as usize;
        if x >= frame.width as usize || y >= frame.height as usize {
            return Err(RecorderError::BadRegion);
        }
        let w = r.width.min(frame.width - r.x as u32);
        let h = r.height.min(frame.height - r.y as u32);
        Ok((x, y, w, h))
    }

    /// The picture size to encode for a cropped capture `w`×`h` and its whole downscale: the
    /// capture itself, or the smallest downscale that fits H.264 level 5.1 so PowerPoint plays
    /// the take (both its picture budget and its macroblock rate at the take's frame rate).
    fn scaled(&self, w: u32, h: u32) -> (u32, u32, u32) {
        let fps = u64::from(self.cfg.fps.max(1));
        let fits = |n: u32| {
            let mbs = u64::from((w / n).div_ceil(16)) * u64::from((h / n).div_ceil(16));
            mbs <= LEVEL_51_MAX_MBS && mbs.saturating_mul(fps) <= LEVEL_51_MAX_MBS_PER_S
        };
        let n = (1..=MAX_DOWNSCALE).find(|n| fits(*n)).unwrap_or(MAX_DOWNSCALE);
        (n, (w / n) & !1, (h / n) & !1)
    }

    /// Start the encoder and the MP4 track at the final picture size.
    fn open(&mut self, w: u32, h: u32) -> Result<(), RecorderError> {
        if w > MAX_SIDE || h > MAX_SIDE {
            return Err(RecorderError::Encoder(format!("a recording is at most {MAX_SIDE} pixels on a side")));
        }
        let enc = Encoder::new(EncoderConfig { width: w, height: h, fps: self.cfg.fps, qp: self.cfg.quality })
            .map_err(|e| RecorderError::Encoder(e.to_string()))?;
        let avc = AvcConfig::new(vec![enc.sps()], vec![enc.pps()], 4);
        let entry =
            SampleEntry::video(FourCc(*b"avc1"), CodecConfig::Avc(avc), u16::try_from(w).unwrap_or(u16::MAX), u16::try_from(h).unwrap_or(u16::MAX));
        let Some(sink) = self.sink.take() else { return Err(RecorderError::Muxer("the recording sink is gone".into())) };
        let sink: Box<dyn RecorderSink> =
            Box::new(LimitedSink { inner: sink, limit: self.cfg.max_bytes, bytes: self.bytes.clone(), over: self.over.clone() });
        // The writer writes the file header here: a byte cap smaller than it is reported as the
        // cap, not as a generic muxer failure.
        let mut mux = Mp4Writer::new(sink, WriterOptions::new(Brand::Mp4))
            .map_err(|e| if self.over.load(Ordering::Relaxed) { RecorderError::TooLarge } else { RecorderError::Muxer(e.to_string()) })?;
        self.track = mux
            .add_track(TrackConfig::new(entry, self.cfg.fps))
            .map_err(|e| if self.over.load(Ordering::Relaxed) { RecorderError::TooLarge } else { RecorderError::Muxer(e.to_string()) })?;
        self.mux = Some(mux);
        self.encoder = Some(enc);
        self.size = (w, h);
        Ok(())
    }

    /// BGRA rows → one RGBA picture, dropping the stride's padding and applying the recording's
    /// whole downscale (a box average per block, so text keeps its edges).
    fn fill_rgba(&mut self, frame: &ScreenFrame, x: usize, y: usize, w: u32, h: u32) {
        let n = self.scale.max(1) as usize;
        let (ow, oh) = (self.size.0 as usize, self.size.1 as usize);
        let need = ow * oh * 4;
        // The buffer keeps its capacity across frames: only a size change reallocates (the
        // recorder refuses a mid-take size change, so in practice this happens once).
        if self.rgba.len() != need {
            self.rgba.clear();
            self.rgba.resize(need, 0);
        }
        let (w, h) = (w as usize, h as usize);
        if n == 1 {
            for row in 0..h {
                let src_at = (y + row) * frame.stride + x * 4;
                let Some(src) = frame.data.get(src_at..src_at + w * 4) else { continue };
                let dst = &mut self.rgba[row * ow * 4..(row + 1) * ow * 4];
                for (d, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(src.as_chunks::<4>().0) {
                    *d = [s[2], s[1], s[0], 255];
                }
            }
            return;
        }
        // The crop guarantees `ow * n <= w` and `oh * n <= h` (the region is clipped to the
        // frame and the scale is whole), so each block is one bounded slice read: no per-pixel
        // `get` and no per-pixel offset multiply beyond the block's column base.
        debug_assert!(ow * n <= w && oh * n <= h, "the downscale stays inside the cropped frame");
        let x0 = x * 4;
        let block = n * 4;
        let count = (n * n) as u32;
        for row in 0..oh {
            let dst = &mut self.rgba[row * ow * 4..(row + 1) * ow * 4];
            for (col, d) in dst.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let cx = x0 + col * block;
                let (mut sum, mut seen) = ([0u32; 3], 0u32);
                for dy in 0..n {
                    let at = (y + row * n + dy) * frame.stride + cx;
                    let Some(src) = frame.data.get(at..at + block) else { continue };
                    for s in src.as_chunks::<4>().0 {
                        sum[0] += u32::from(s[2]);
                        sum[1] += u32::from(s[1]);
                        sum[2] += u32::from(s[0]);
                        seen += 1;
                    }
                }
                // A clipped block (hostile frame data) averages what it saw; a whole one divides
                // by the fixed count.
                let n_seen = if seen == count { count } else { seen.max(1) };
                *d = [(sum[0] / n_seen) as u8, (sum[1] / n_seen) as u8, (sum[2] / n_seen) as u8, 255];
            }
        }
    }

    /// Finish the file (`moov` in front of `mdat`, so a player can start before it is downloaded).
    pub fn finish(self) -> Result<RecorderOutput, RecorderError> {
        if self.frames == 0 {
            return Err(RecorderError::Empty);
        }
        let Some(mux) = self.mux else { return Err(RecorderError::Empty) };
        let (w, h) = self.size;
        let over = self.over.clone();
        let mut sink = mux
            .finish_faststart()
            .map_err(|e| if over.load(Ordering::Relaxed) { RecorderError::TooLarge } else { RecorderError::Muxer(e.to_string()) })?;
        sink.flush().map_err(|e| RecorderError::Io(e.to_string()))?;
        Ok(RecorderOutput {
            frames: self.frames,
            width: w,
            height: h,
            duration_ms: self.frames.saturating_mul(1000) / u64::from(self.cfg.fps.max(1)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        std::env::temp_dir().join(format!("deckcraft-screen-test-{}-{n}-{name}.mp4", std::process::id()))
    }

    fn create(path: &std::path::Path) -> std::fs::File {
        std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(path).expect("create")
    }

    fn record(name: &str, frames: &[ScreenFrame], cfg: RecorderConfig) -> (std::path::PathBuf, RecorderOutput) {
        let path = scratch(name);
        let file = create(&path);
        let mut rec = ScreenRecorder::new(Box::new(file), cfg).expect("recorder");
        for f in frames {
            rec.push(f).expect("push");
        }
        let out = rec.finish().expect("finish");
        (path, out)
    }

    #[test]
    fn a_solid_frame_records_and_the_mp4_is_readable() {
        let frame = ScreenFrame::solid(320, 240, [0, 0, 255, 255]);
        let (path, out) = record("solid", &[frame.clone(), frame], RecorderConfig::new(30));
        assert_eq!(out.frames, 2);
        assert_eq!((out.width, out.height), (320, 240));
        let bytes = std::fs::read(&path).expect("read");
        assert_eq!(&bytes[4..8], b"ftyp", "an MP4 begins with ftyp");
        assert!(bytes.windows(4).any(|w| w == b"moov"));
        assert!(bytes.windows(4).any(|w| w == b"mdat"));
        let file = deckcraft_isobmff::open(bytes.as_slice()).expect("our MP4 demuxes");
        let track = file.tracks.first().expect("one track");
        assert_eq!((track.width, track.height), (320, 240));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_region_is_cropped_before_encoding() {
        let frame = ScreenFrame::solid(200, 120, [0, 0, 255, 255]);
        let mut cfg = RecorderConfig::new(30);
        cfg.region = Some(CaptureRegion::new(64, 40, 65, 65).expect("region"));
        let (path, out) = record("crop", &[frame], cfg);
        assert_eq!((out.width, out.height), (64, 64), "odd sides are trimmed, never padded");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_region_outside_the_frame_or_a_short_buffer_is_refused() {
        let file = create(&scratch("bad"));
        let mut rec = ScreenRecorder::new(Box::new(file), RecorderConfig::new(30)).expect("recorder");
        let mut frame = ScreenFrame::solid(160, 160, [0, 0, 0, 255]);
        frame.data.truncate(16);
        assert_eq!(rec.push(&frame), Err(RecorderError::BadFrame));
        let frame = ScreenFrame::solid(160, 160, [0, 0, 0, 255]);
        let mut cfg = RecorderConfig::new(30);
        cfg.region = Some(CaptureRegion { x: 150, y: 0, width: 32, height: 32 });
        let file = create(&scratch("far"));
        let mut rec = ScreenRecorder::new(Box::new(file), cfg).expect("recorder");
        assert_eq!(rec.push(&frame), Err(RecorderError::BadRegion));
    }

    #[test]
    fn a_recording_without_frames_finishes_as_empty() {
        let file = create(&scratch("empty"));
        let rec = ScreenRecorder::new(Box::new(file), RecorderConfig::new(30)).expect("recorder");
        assert_eq!(rec.finish(), Err(RecorderError::Empty));
    }

    #[test]
    fn a_repeated_frame_is_an_all_skip_picture() {
        let path = scratch("repeat");
        let file = create(&path);
        let mut rec = ScreenRecorder::new(Box::new(file), RecorderConfig::new(30)).expect("recorder");
        let solid = ScreenFrame::solid(96, 64, [9, 9, 9, 255]);
        rec.push(&solid).expect("push");
        rec.push_repeat().expect("an all-skip repeat after the key picture");
        rec.push(&solid).expect("the next picture encodes");
        rec.push_repeat().expect("a repeat of a P picture");
        assert_eq!(rec.frames(), 4);
        let out = rec.finish().expect("finish");
        assert_eq!((out.frames, out.width, out.height), (4, 96, 64));
        let _ = std::fs::remove_file(&path);
        // A key frame that is due is never a repeat: the caller must encode one.
        let file = create(&scratch("repeat-key"));
        let mut rec = ScreenRecorder::new(Box::new(file), RecorderConfig::new(1)).expect("recorder");
        rec.push(&solid).expect("push");
        rec.push_repeat().expect("a repeat");
        // The interval is two frames at 1 fps: the third is a key frame, which must be encoded.
        assert_eq!(rec.push_repeat(), Err(RecorderError::BadFrame), "a due key frame is never a repeat");
    }

    #[test]
    fn the_picture_is_downscaled_only_until_it_fits_level_51() {
        let file = create(&scratch("scaled"));
        let rec = ScreenRecorder::new(Box::new(file), RecorderConfig::new(30)).expect("recorder");
        assert_eq!(rec.scaled(3840, 2160), (1, 3840, 2160), "4K at 30 fps fits level 5.1 as captured");
        // The same capture at 60 fps is over level 5.1's macroblock rate: it records at half size.
        let fast = ScreenRecorder::new(Box::new(create(&scratch("scaled-60"))), RecorderConfig::new(60)).expect("recorder");
        assert_eq!(fast.scaled(3840, 2160), (2, 1920, 1080), "4K at 60 fps is downscaled to keep level 5.1");
        assert_eq!(fast.scaled(1920, 1080), (1, 1920, 1080), "1080p at 60 fps still fits");
        assert_eq!(rec.scaled(5120, 2880), (2, 2560, 1440), "a 5K display records at half size");
        assert_eq!(rec.scaled(7680, 4320), (2, 3840, 2160), "8K records at 4K");
    }

    #[test]
    fn a_bigger_than_level_51_frame_records_at_the_downscaled_size() {
        // Level 5.1 is what PowerPoint's decoder is sure to play; the take keeps it.
        let frame = ScreenFrame::solid(5120, 2880, [10, 20, 30, 255]);
        let (path, out) = record("downscale", &[frame], RecorderConfig::new(30));
        assert_eq!((out.width, out.height), (2560, 1440));
        let file = deckcraft_isobmff::open(std::fs::read(&path).expect("read").as_slice()).expect("demux");
        assert_eq!((file.tracks[0].width, file.tracks[0].height), (2560, 1440));
        let _ = std::fs::remove_file(&path);
    }

    /// A backend row stride wider than the picture (Windows hands one back) is stripped: the
    /// padding bytes never reach the picture.
    #[test]
    fn a_padded_stride_is_stripped() {
        let (w, h) = (16u32, 16u32);
        let tight = w as usize * 4;
        let stride = tight + 8;
        let mut data = vec![0xAAu8; stride * h as usize];
        for row in 0..h as usize {
            for col in 0..w as usize {
                let at = row * stride + col * 4;
                data[at..at + 4].copy_from_slice(&[30, 20, 10, 255]);
            }
        }
        let frame = ScreenFrame::new(w, h, stride, data).expect("a padded frame");
        let mut rec = ScreenRecorder::new(Box::new(create(&scratch("stride"))), RecorderConfig::new(30)).expect("recorder");
        rec.push(&frame).expect("a padded frame records");
        assert_eq!(&rec.rgba[..4], &[10, 20, 30, 255], "BGRA becomes RGBA and the padding is gone");
        assert_eq!(rec.rgba.len(), tight * h as usize);
    }

    /// A region that runs past the right and bottom edges is clipped to the frame instead of
    /// rejecting the take.
    #[test]
    fn a_region_that_runs_past_the_frame_is_clipped() {
        let mut cfg = RecorderConfig::new(30);
        cfg.region = Some(CaptureRegion { x: 150, y: 150, width: 64, height: 64 });
        let frame = ScreenFrame::solid(200, 200, [0, 0, 255, 255]);
        let (path, out) = record("clipped", &[frame], cfg);
        assert_eq!((out.width, out.height), (50, 50));
        let _ = std::fs::remove_file(&path);
    }

    /// The capture output changing size mid-take cannot be muxed as one track: the frame is
    /// refused rather than written at the wrong size.
    #[test]
    fn a_size_change_mid_recording_is_a_bad_frame() {
        let mut rec = ScreenRecorder::new(Box::new(create(&scratch("resize"))), RecorderConfig::new(30)).expect("recorder");
        rec.push(&ScreenFrame::solid(160, 120, [1, 2, 3, 255])).expect("first frame");
        assert_eq!(rec.push(&ScreenFrame::solid(320, 240, [1, 2, 3, 255])), Err(RecorderError::BadFrame));
    }

    #[test]
    fn the_frame_limit_refuses_more_pictures() {
        let mut rec = ScreenRecorder::new(Box::new(create(&scratch("long"))), RecorderConfig::new(30)).expect("recorder");
        rec.frames = MAX_FRAMES;
        assert_eq!(rec.push(&ScreenFrame::solid(64, 64, [0; 4])), Err(RecorderError::TooLong));
        assert_eq!(rec.push_repeat(), Err(RecorderError::TooLong));
    }

    /// The sink refuses a write past the configured byte cap, so a take cannot fill the disk; the
    /// recorder reports it as its own error instead of a generic muxer failure.
    #[test]
    fn the_size_cap_stops_a_recording() {
        let mut cfg = RecorderConfig::new(30);
        cfg.max_bytes = 1;
        let mut rec = ScreenRecorder::new(Box::new(create(&scratch("cap"))), cfg).expect("recorder");
        assert_eq!(rec.push(&ScreenFrame::solid(64, 64, [0, 0, 255, 255])), Err(RecorderError::TooLarge));
        assert_eq!(rec.push(&ScreenFrame::solid(64, 64, [0, 0, 255, 255])), Err(RecorderError::TooLarge));
    }

    #[test]
    fn a_rate_outside_one_to_sixty_is_refused_not_clamped() {
        for fps in [0u32, 61, 240] {
            assert!(ScreenRecorder::new(Box::new(create(&scratch("rate"))), RecorderConfig::new(fps)).is_err(), "{fps}");
        }
    }
}
