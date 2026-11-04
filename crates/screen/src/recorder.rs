//! The screen-recording encoder: BGRA frames in, a progressive MP4 out.
//!
//! Frames are converted to RGBA (stripping the backend's row padding), cropped to the selected
//! region, trimmed to even sides and encoded with DeckCraft's H.264 encoder; the NAL units are
//! muxed by the craft-media MP4 writer. Recording is streamed: the sink is a file in the engine
//! (not a growing `Vec`), so a long recording costs no more memory than one frame plus the
//! encoder's reference picture.

use std::io::{Read, Seek, Write};

use deckcraft_h264::{Encoder, EncoderConfig};
use deckcraft_isobmff::{AvcConfig, Brand, CodecConfig, FourCc, Mp4Writer, SampleEntry, TrackConfig, WriteSample, WriterOptions};

use crate::{CaptureRegion, ScreenFrame};

/// Longest recording: 2 hours at the fastest capture rate (60 fps). Frames past it are refused.
pub const MAX_FRAMES: u64 = 2 * 60 * 60 * 60;
/// Largest recording side: the H.264 encoder's limit.
pub const MAX_SIDE: u32 = 8192;
/// The H.264 encoder's smallest side.
const MIN_SIDE: u32 = 16;
/// A key frame at least this often (seconds), so players can seek.
const KEY_EVERY_S: u32 = 2;

/// A seekable sink for the MP4 writer: a file in the engine, a cursor in tests.
pub trait RecorderSink: Read + Write + Seek + Send {}
impl<T: Read + Write + Seek + Send> RecorderSink for T {}

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
}

impl RecorderConfig {
    pub fn new(fps: u32) -> Self {
        let fps = fps.clamp(1, 60);
        RecorderConfig { region: None, fps, quality: 24, key_frame_interval: fps * KEY_EVERY_S }
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
    frames: u64,
    /// Reused BGRA→RGBA buffer (one frame).
    rgba: Vec<u8>,
}

impl ScreenRecorder {
    /// A recorder that writes to `sink`. The picture size is taken from the first frame (or the
    /// configured region), so a wrong first frame fails at [`push`](Self::push), not here.
    pub fn new(sink: Box<dyn RecorderSink>, cfg: RecorderConfig) -> Result<Self, RecorderError> {
        if cfg.fps == 0 || cfg.fps > 240 {
            return Err(RecorderError::Encoder("the capture rate must be from 1 to 240 frames per second".into()));
        }
        Ok(ScreenRecorder { sink: Some(sink), cfg, encoder: None, mux: None, track: 0, size: (0, 0), frames: 0, rgba: Vec::new() })
    }

    /// The encoded picture size, once the first frame has been seen.
    pub fn size(&self) -> Option<(u32, u32)> {
        (self.size.0 > 0).then_some(self.size)
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Encode one captured frame. Frames past [`MAX_FRAMES`] are refused.
    pub fn push(&mut self, frame: &ScreenFrame) -> Result<(), RecorderError> {
        if self.frames >= MAX_FRAMES {
            return Err(RecorderError::TooLong);
        }
        let (x, y, w, h) = self.crop_for(frame)?;
        let (w, h) = (w & !1, h & !1);
        if w < MIN_SIDE || h < MIN_SIDE {
            return Err(RecorderError::BadRegion);
        }
        if self.encoder.is_none() {
            self.open(w, h)?;
        } else if self.size != (w, h) {
            // The capture output changed size mid-recording (a display was rotated): refuse
            // rather than mux samples that don't match the track's sample entry.
            return Err(RecorderError::BadFrame);
        }
        self.to_rgba(frame, x, y, w, h);
        let key = self.frames == 0 || self.frames % u64::from(self.cfg.key_frame_interval.max(1)) == 0;
        let Some(encoder) = self.encoder.as_mut() else { return Err(RecorderError::Encoder("the encoder is not ready".into())) };
        let pic = encoder.encode_rgba(&self.rgba, key);
        let data = pic.length_prefixed();
        let Some(mux) = self.mux.as_mut() else { return Err(RecorderError::Muxer("the muxer is not ready".into())) };
        mux.write_sample(self.track, WriteSample { data: &data, duration: 1, composition_offset: 0, is_sync: pic.key })
            .map_err(|e| RecorderError::Muxer(e.to_string()))?;
        self.frames += 1;
        Ok(())
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
        let mut mux = Mp4Writer::new(sink, WriterOptions::new(Brand::Mp4)).map_err(|e| RecorderError::Muxer(e.to_string()))?;
        self.track = mux.add_track(TrackConfig::new(entry, self.cfg.fps)).map_err(|e| RecorderError::Muxer(e.to_string()))?;
        self.mux = Some(mux);
        self.encoder = Some(enc);
        self.size = (w, h);
        Ok(())
    }

    /// BGRA rows → one RGBA picture, dropping the stride's padding.
    fn to_rgba(&mut self, frame: &ScreenFrame, x: usize, y: usize, w: u32, h: u32) {
        let need = w as usize * h as usize * 4;
        self.rgba.clear();
        self.rgba.resize(need, 0);
        for row in 0..h as usize {
            let src_at = (y + row) * frame.stride + x * 4;
            let Some(src) = frame.data.get(src_at..src_at + w as usize * 4) else { continue };
            let dst = &mut self.rgba[row * w as usize * 4..(row + 1) * w as usize * 4];
            for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = 255;
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
        let mut sink = mux.finish_faststart().map_err(|e| RecorderError::Muxer(e.to_string()))?;
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
}
