//! Camera capture interface: the platform implementation lives in the host application, this
//! module only defines the data and the channel contract.
//!
//! It carries no platform dependency and builds for wasm, where the host injects `None`: the show
//! still records slide timings and narration, it simply has no camera. Capture runs on a worker
//! thread the host owns; [`CameraSession::drain`] is non-blocking so the UI never waits on a
//! device or an OS permission sheet.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use web_time::Instant;

/// What to open. A camera that cannot do exactly this may fall back; the frames say what came.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraConfig {
    /// `None`: the platform's default camera.
    pub device: Option<String>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        CameraConfig { device: None, width: 640, height: 480, fps: 30 }
    }
}

/// The stream a camera actually opened, the first thing a host sends.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraInfo {
    pub device: Option<String>,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
}

/// One captured frame, packed RGBA8.
#[derive(Clone, Debug)]
pub struct CameraFrame {
    /// Increases by one per delivered frame.
    pub seq: u64,
    /// Monotonic receipt time on the show clock; the time to record against.
    pub at: Instant,
    pub width: u32,
    pub height: u32,
    /// Exactly `width × height × 4` bytes.
    pub rgba: Arc<[u8]>,
}

/// Why a camera did not open, or stopped.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CameraError {
    #[error("no camera is connected")]
    NoDevice,
    #[error("camera access was denied")]
    PermissionDenied,
    #[error("the camera is in use by another application")]
    InUse,
    #[error("the camera has no format we can use")]
    UnsupportedFormat,
    #[error("the camera was disconnected")]
    Lost,
    #[error("camera capture isn't supported here")]
    Unsupported,
    #[error("camera: {0}")]
    Backend(String),
}

/// What the worker reports. The show drains these once per pass.
#[derive(Clone, Debug)]
pub enum CameraEvent {
    Opened(CameraInfo),
    Frame(CameraFrame),
    Failed(CameraError),
    Closed,
}

/// A started capture: the worker's bounded event channel and its monotonic start time.
pub struct CameraSession {
    pub started_at: Instant,
    events: Receiver<CameraEvent>,
}

/// Most events one [`CameraSession::drain`] returns; the rest wait for the next pass.
pub const MAX_DRAIN: usize = 256;

impl CameraSession {
    pub fn new(started_at: Instant, events: Receiver<CameraEvent>) -> Self {
        CameraSession { started_at, events }
    }

    /// Take the events that are ready, without blocking. Bounded so one pass cannot spin on a
    /// camera flooding the channel.
    pub fn drain(&self) -> Vec<CameraEvent> {
        let mut out = Vec::new();
        while out.len() < MAX_DRAIN {
            match self.events.try_recv() {
                Ok(e) => out.push(e),
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
            }
        }
        out
    }
}

/// The host-owned camera. `stop` must return without joining the worker.
pub trait CameraIn: Send {
    fn start(&mut self, config: &CameraConfig) -> Result<CameraSession, CameraError>;
    fn stop(&mut self);
}

/// Longest clip one slide may record (10 minutes).
pub const MAX_CLIP_MS: u32 = 10 * 60 * 1000;
/// Largest MP4 one slide may record.
pub const MAX_CLIP_BYTES: usize = 64 << 20;

/// A finished recording, as the model's media part.
#[derive(Clone, Debug)]
pub struct CameoClip {
    pub mp4: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub duration_ms: u32,
    pub frames: u64,
}

/// Turns camera frames into one video-only MP4 for a slide.
///
/// Frames arrive whenever the camera delivers them; the recorder samples them at a constant
/// `fps` on the slide's clock (a slow camera repeats its last frame, a fast one is thinned) so the
/// clip's duration matches the slide's dwell without an audio track to sync against.
pub struct CameoRecorder {
    track: crate::encode::H264Mp4Track,
    width: u32,
    height: u32,
    fps: u32,
    start_ms: u64,
    written: u64,
    /// The most recent frame, already fitted to the target.
    last: Option<Vec<u8>>,
    capped: bool,
    /// Largest clip this recorder keeps, in MP4 bytes ([`MAX_CLIP_BYTES`] unless a test lowers it).
    max_bytes: usize,
}

impl CameoRecorder {
    /// `target` is in pixels and both sides are made even; the encoder needs that for 4:2:0.
    pub fn new(target: (u32, u32), fps: u32, qp: u8, start_ms: u64) -> crate::Result<Self> {
        let width = even(target.0.clamp(16, 1280));
        let height = even(target.1.clamp(16, 1280));
        let fps = fps.clamp(1, 60);
        Ok(CameoRecorder {
            track: crate::encode::H264Mp4Track::new(width, height, fps, qp)?,
            width,
            height,
            fps,
            start_ms,
            written: 0,
            last: None,
            capped: false,
            max_bytes: MAX_CLIP_BYTES,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn frames(&self) -> u64 {
        self.written
    }

    /// The clip reached [`MAX_CLIP_MS`] or its byte budget: stop the camera for this slide.
    pub fn capped(&self) -> bool {
        self.capped
    }

    /// Lower the clip's byte budget (tests only).
    #[cfg(test)]
    fn with_max_bytes(mut self, bytes: usize) -> Self {
        self.max_bytes = bytes;
        self
    }

    /// Sample bytes the clip may hold, leaving room for the MP4 container's own boxes.
    fn byte_budget(&self) -> u64 {
        let margin = (self.max_bytes / 64).max(1 << 10);
        self.max_bytes.saturating_sub(margin) as u64
    }

    /// Fit and sample one frame. Frames may arrive late or early; only the target rate is written.
    pub fn push(&mut self, frame: &CameraFrame, now_ms: u64) -> crate::Result<()> {
        let need = frame.width as usize * frame.height as usize * 4;
        if frame.width == 0 || frame.height == 0 || frame.rgba.len() != need {
            return Err(crate::MediaError::Corrupt(format!(
                "a {}×{} camera frame is {need} bytes, not {}",
                frame.width,
                frame.height,
                frame.rgba.len()
            )));
        }
        if now_ms < self.start_ms {
            return Ok(());
        }
        let elapsed = now_ms - self.start_ms;
        if elapsed >= u64::from(MAX_CLIP_MS) {
            self.capped = true;
            self.fill_to(self.max_frames(), None)?;
            return Ok(());
        }
        let fitted = crate::encode::fitted_rgba(&frame.rgba, frame.width, frame.height, self.width, self.height)?;
        let want = elapsed.saturating_mul(u64::from(self.fps)) / 1000;
        self.fill_to(want.min(self.max_frames()), Some(&fitted))?;
        self.last = Some(fitted);
        Ok(())
    }

    /// Write the missing frames up to `want`, repeating `frame` (or the last frame). Stops at the
    /// byte budget, keeping what is written, so an over-long slide keeps a truncated clip.
    fn fill_to(&mut self, want: u64, frame: Option<&[u8]>) -> crate::Result<()> {
        while self.written < want {
            if self.track.bytes() >= self.byte_budget() {
                self.capped = true;
                break;
            }
            let Some(frame) = frame.or(self.last.as_deref()) else { break };
            self.track.push_rgba(frame)?;
            self.written += 1;
        }
        Ok(())
    }

    fn max_frames(&self) -> u64 {
        u64::from(MAX_CLIP_MS) * u64::from(self.fps) / 1000
    }

    /// Close the clip at `now_ms`, filling the last period with the last frame.
    pub fn finish(mut self, now_ms: u64) -> crate::Result<CameoClip> {
        let Some(last) = self.last.take() else {
            return Err(crate::MediaError::Missing("camera frames"));
        };
        let elapsed = now_ms.saturating_sub(self.start_ms).min(u64::from(MAX_CLIP_MS));
        let duration_ms = u32::try_from(elapsed).unwrap_or(MAX_CLIP_MS);
        let total = (u64::from(duration_ms) * u64::from(self.fps) / 1000).max(1).min(self.max_frames());
        self.fill_to(total, Some(&last))?;
        let frames = self.written;
        let width = self.width;
        let height = self.height;
        let mp4 = self.track.finish()?;
        if mp4.len() > MAX_CLIP_BYTES {
            return Err(crate::MediaError::Unsupported(format!("a {} MB camera clip", mp4.len() / (1 << 20))));
        }
        Ok(CameoClip { mp4, width, height, duration_ms, frames })
    }
}

/// Round up to the next even value at least 16.
fn even(v: u32) -> u32 {
    (v.max(16) + 1) & !1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(seq: u64, w: u32, h: u32, shade: u8) -> CameraFrame {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for p in rgba.chunks_mut(4) {
            p.copy_from_slice(&[shade, 255 - shade, shade / 2, 255]);
        }
        CameraFrame { seq, at: Instant::now(), width: w, height: h, rgba: Arc::from(rgba) }
    }

    #[test]
    fn samples_at_the_target_rate_whatever_the_camera_speed() {
        let mut rec = CameoRecorder::new((64, 64), 15, 30, 1_000).expect("recorder");
        // 7 ms, 13 ms and 31 ms after the start: only whole 1/15 s periods are written.
        for (i, at) in [1_007u64, 1_013, 1_031].into_iter().enumerate() {
            rec.push(&frame(i as u64, 32, 32, 40 + i as u8), at).expect("push");
        }
        assert_eq!(rec.frames(), 0, "no period elapsed yet");
        let clip = rec.finish(1_031).expect("finish");
        assert_eq!(clip.duration_ms, 31);
        assert_eq!(clip.frames, 1, "round(31 ms × 15 fps) is one picture");
        assert_eq!((clip.width, clip.height), (64, 64));
        assert_eq!(&clip.mp4[4..8], b"ftyp");
    }

    #[test]
    fn a_slow_camera_repeats_its_last_frame() {
        let mut rec = CameoRecorder::new((32, 32), 10, 30, 0).expect("recorder");
        rec.push(&frame(0, 16, 16, 10), 50).expect("push");
        // One second of slide, a single camera frame: the clip is still 1 s long.
        let clip = rec.finish(1_000).expect("finish");
        assert_eq!(clip.duration_ms, 1_000);
        assert_eq!(clip.frames, 10);
    }

    #[test]
    fn no_frames_is_no_clip() {
        let rec = CameoRecorder::new((32, 32), 15, 30, 0).expect("recorder");
        assert!(rec.finish(500).is_err());
    }

    #[test]
    fn the_duration_is_capped_at_ten_minutes() {
        let mut rec = CameoRecorder::new((32, 32), 1, 30, 0).expect("recorder");
        rec.push(&frame(0, 16, 16, 10), 1_000).expect("push");
        let clip = rec.finish(u64::from(MAX_CLIP_MS) + 5_000).expect("finish");
        assert_eq!(clip.duration_ms, MAX_CLIP_MS);
        assert!(clip.frames <= u64::from(MAX_CLIP_MS) / 1000);
    }

    #[test]
    fn a_clip_stops_at_its_byte_budget_and_keeps_what_it_wrote() {
        let mut rec = CameoRecorder::new((64, 64), 15, 30, 0).expect("recorder").with_max_bytes(8 << 10);
        let mut now = 0u64;
        for i in 0..1_000u64 {
            now = i * 67;
            rec.push(&frame(i, 64, 64, (i % 251) as u8), now).expect("push");
            if rec.capped() {
                break;
            }
        }
        assert!(rec.capped(), "the byte budget ends the clip before the duration cap");
        let clip = rec.finish(now).expect("the clip so far is kept, not rejected");
        assert!(clip.frames > 0);
        assert!(clip.mp4.len() <= MAX_CLIP_BYTES);
    }

    #[test]
    fn bad_frame_sizes_are_rejected() {
        let mut rec = CameoRecorder::new((32, 32), 15, 30, 0).expect("recorder");
        let mut f = frame(0, 16, 16, 10);
        f.rgba = Arc::from(vec![0u8; 5]);
        assert!(rec.push(&f, 100).is_err());
    }

    #[test]
    fn odd_targets_are_made_even() {
        let rec = CameoRecorder::new((33, 65), 15, 30, 0).expect("recorder");
        assert_eq!((rec.width(), rec.height()), (34, 66));
    }
}
