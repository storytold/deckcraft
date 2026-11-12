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

impl CameraFrame {
    /// How long before `newest` this frame was received (0 for `newest` itself), so a host that
    /// drains a batch of frames can time each one instead of the whole batch.
    pub fn lag_ms(&self, newest: Instant) -> u64 {
        let lag = newest.saturating_duration_since(self.at).as_millis();
        u64::try_from(lag).unwrap_or(u64::MAX)
    }
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
    /// The worker's channel went away and [`CameraEvent::Closed`] has been reported.
    closed: bool,
}

/// Most events one [`CameraSession::drain`] returns; the rest wait for the next pass.
pub const MAX_DRAIN: usize = 256;

/// What one [`CameraSession::drain`] pass took from the channel.
#[derive(Clone, Debug)]
pub struct Drained {
    pub events: Vec<CameraEvent>,
    /// The pass stopped at [`MAX_DRAIN`] with more events waiting, so the host can tell a flood
    /// from a quiet camera instead of silently losing frames.
    pub truncated: bool,
}

impl CameraSession {
    pub fn new(started_at: Instant, events: Receiver<CameraEvent>) -> Self {
        CameraSession { started_at, events, closed: false }
    }

    /// Take the events that are ready, without blocking. Bounded so one pass cannot spin on a
    /// camera flooding the channel; `truncated` says when the cap cut the pass short, and
    /// [`CameraEvent::Closed`] is reported once however many passes follow.
    pub fn drain(&mut self) -> Drained {
        let mut events = Vec::new();
        let mut truncated = false;
        loop {
            if events.len() >= MAX_DRAIN {
                truncated = true;
                break;
            }
            match self.events.try_recv() {
                Ok(e) => events.push(e),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    // The worker went away without saying so: treat it as an unplugged camera,
                    // once, so a lost device does not read as a new failure on every pass.
                    if !self.closed {
                        self.closed = true;
                        events.push(CameraEvent::Closed);
                    }
                    break;
                }
            }
        }
        Drained { events, truncated }
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

/// Why a clip stopped before its slide did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameoCap {
    /// The slide recorded [`MAX_CLIP_MS`].
    Duration,
    /// The clip reached [`MAX_CLIP_BYTES`].
    Bytes,
}

/// A finished recording, as the model's media part.
#[derive(Clone, Debug)]
pub struct CameoClip {
    pub mp4: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub duration_ms: u32,
    pub frames: u64,
}

/// What [`CameoRecorder::seal`] produced.
pub enum Seal {
    /// The clip still owes samples: seal it again on the returned recorder, on a later pass.
    Pending(CameoRecorder),
    /// Every sample is written: the clip.
    Done(CameoClip),
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
    /// The samples due so far on the slide's clock; [`CameoRecorder::tick`] writes the rest.
    target: u64,
    /// The frame the samples being written show, kept as the camera handed it over so a frame
    /// that is never sampled costs no fitting.
    image: Option<SrcFrame>,
    /// A frame that arrived while the samples before it were still being written: it becomes
    /// `image` once `target` is reached.
    next_image: Option<SrcFrame>,
    /// Once the clip is closing, the sample count it must reach and its duration: fixed on the
    /// first [`CameoRecorder::seal`] so a close spread over several passes agrees with itself.
    sealing: Option<(u64, u32)>,
    /// The image the last written sample shows: the bytes are otherwise only inside the MP4
    /// (tests only).
    #[cfg(test)]
    last_written: Option<Vec<u8>>,
    capped: Option<CameoCap>,
    /// Largest clip this recorder keeps, in MP4 bytes ([`MAX_CLIP_BYTES`] unless a test lowers it).
    max_bytes: usize,
}

/// One camera frame as delivered: fitted only when a sample is actually due.
struct SrcFrame {
    width: u32,
    height: u32,
    rgba: Arc<[u8]>,
}

/// Most samples one push or tick writes, so a long stall is caught up across passes instead of
/// freezing the show on one call. At the 15 fps a Cameo records this is two seconds.
pub const MAX_SAMPLES_PER_CALL: u64 = 30;

impl CameoRecorder {
    /// `target` is in pixels and both sides are made even; the encoder needs that for 4:2:0.
    ///
    /// Out-of-range values are clamped, not rejected: sides to 16–1280 and `fps` to 1–60 (the
    /// sizes a Cameo is captured at). [`crate::H264Mp4Track::new`] rejects instead, because a
    /// manually built track must not silently change the picture the caller asked for.
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
            target: 0,
            image: None,
            next_image: None,
            sealing: None,
            #[cfg(test)]
            last_written: None,
            capped: None,
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
        self.capped.is_some()
    }

    /// Which limit stopped the clip, for the show's status line.
    pub fn cap(&self) -> Option<CameoCap> {
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

    /// Take one frame. Frames may arrive late or early; only the target rate is written, and the
    /// writing itself happens in [`CameoRecorder::tick`] so this call stays cheap however long the
    /// show was busy.
    pub fn push(&mut self, frame: &CameraFrame, now_ms: u64) -> crate::Result<()> {
        let need = (frame.width as usize)
            .checked_mul(frame.height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| crate::MediaError::Corrupt("a camera frame wider than memory".into()))?;
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
        let want = (elapsed.saturating_mul(u64::from(self.fps)) / 1000).min(self.max_frames());
        self.target = self.target.max(want);
        let src = SrcFrame { width: frame.width, height: frame.height, rgba: frame.rgba.clone() };
        if self.image.is_none() {
            // Nothing has been written yet, so there is no earlier image to repeat: the first
            // frame is the image from now on, never backdated to the slide's start.
            self.image = Some(src);
        } else if self.written < self.target {
            // The samples still due show the frame that was on screen; this one waits its turn.
            self.next_image = Some(src);
        } else {
            self.image = Some(src);
        }
        if elapsed >= u64::from(MAX_CLIP_MS) {
            self.capped = Some(CameoCap::Duration);
        }
        Ok(())
    }

    /// Write the samples that are due, up to `budget` pictures. Called once per show pass so a
    /// stall (or a slow camera) is caught up over several passes without blocking one.
    pub fn tick(&mut self, budget: u64) -> crate::Result<()> {
        self.fill_to(self.target, budget)
    }

    /// Write the missing samples up to `want`, repeating the current image. Stops at `budget`
    /// samples or the byte budget, keeping what is written, so an over-long slide keeps a
    /// truncated clip and the rest is written by the next pass.
    fn fill_to(&mut self, want: u64, budget: u64) -> crate::Result<()> {
        // A byte-capped clip already holds everything it may: the refused picture became the
        // encoder's reference, so encoding the same frame again would write all-skipped
        // samples the decoder renders from the last muxed picture — frames past the cap
        // showing the wrong picture. `seal` finalizes capped clips on the `Done` path below.
        if self.capped == Some(CameoCap::Bytes) {
            return Ok(());
        }
        if self.written >= want {
            if self.written >= self.target
                && let Some(next) = self.next_image.take()
            {
                self.image = Some(next);
            }
            return Ok(());
        }
        let Some(src) = self.image.take() else { return Ok(()) };
        let fitted = crate::encode::fitted_rgba(&src.rgba, src.width, src.height, self.width, self.height)?;
        let mut left = budget;
        while self.written < want && left > 0 {
            // The finished sample's own size decides: it is already encoded when the track says
            // no, so a refusal ends the clip here — writing more would only change pictures the
            // clip never holds.
            if !self.track.push_rgba_capped(&fitted, self.byte_budget())? {
                self.capped = Some(CameoCap::Bytes);
                break;
            }
            self.note_written(&fitted);
            self.written += 1;
            left -= 1;
        }
        self.image = Some(src);
        if self.written >= self.target
            && let Some(next) = self.next_image.take()
        {
            self.image = Some(next);
        }
        Ok(())
    }

    fn max_frames(&self) -> u64 {
        u64::from(MAX_CLIP_MS) * u64::from(self.fps) / 1000
    }

    /// Remember the image the sample just written shows: the bytes otherwise only live inside the
    /// MP4, and a test has to compare them.
    #[cfg(test)]
    fn note_written(&mut self, frame: &[u8]) {
        self.last_written = Some(frame.to_vec());
    }

    #[cfg(not(test))]
    fn note_written(&mut self, _frame: &[u8]) {}

    /// Close the clip at `now_ms`, writing at most `budget` pictures. While samples remain, the
    /// recorder comes back [`Seal::Pending`] and the caller seals it again on a later pass, so a
    /// long gap is caught up over several passes instead of blocking a slide change on encoding.
    /// The sample count and duration are fixed on the first call, so the passes agree.
    ///
    /// [`CameoRecorder::finish`] is the one-call form for a caller with nowhere to spread the work.
    pub fn seal(mut self, now_ms: u64, budget: u64) -> crate::Result<Seal> {
        if self.image.is_none() {
            return Err(crate::MediaError::Missing("camera frames"));
        }
        let (total, duration_ms) = match self.sealing {
            Some(fixed) => fixed,
            None => {
                let elapsed = now_ms.saturating_sub(self.start_ms).min(u64::from(MAX_CLIP_MS));
                let duration_ms = u32::try_from(elapsed).unwrap_or(MAX_CLIP_MS);
                let total = (u64::from(duration_ms) * u64::from(self.fps) / 1000).max(1).min(self.max_frames());
                self.sealing = Some((total, duration_ms));
                (total, duration_ms)
            }
        };
        self.fill_to(total, budget)?;
        // The byte budget stops the clip early: what is written is the whole clip, as `finish`
        // kept it before `seal` existed. Anything else may be finished by a later pass.
        if self.written < total && self.capped != Some(CameoCap::Bytes) {
            return Ok(Seal::Pending(self));
        }
        let frames = self.written;
        let width = self.width;
        let height = self.height;
        let mp4 = self.track.finish()?;
        if mp4.len() > MAX_CLIP_BYTES {
            return Err(crate::MediaError::Unsupported(format!("a {} MB camera clip", mp4.len() / (1 << 20))));
        }
        Ok(Seal::Done(CameoClip { mp4, width, height, duration_ms, frames }))
    }

    /// Close the clip at `now_ms` in one call, filling the last period with the last frame. A
    /// close that owes many samples does all of them here, so a show host spreads the work with
    /// [`CameoRecorder::seal`] instead.
    pub fn finish(mut self, now_ms: u64) -> crate::Result<CameoClip> {
        loop {
            match self.seal(now_ms, MAX_SAMPLES_PER_CALL)? {
                Seal::Done(clip) => return Ok(clip),
                Seal::Pending(rec) => self = rec,
            }
        }
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

    /// The image the recorder will write for the samples it is on: fitted to the clip's size.
    fn fitted_image(rec: &CameoRecorder) -> Option<Vec<u8>> {
        rec.image.as_ref().map(|f| crate::encode::fitted_rgba(&f.rgba, f.width, f.height, rec.width, rec.height).expect("fit"))
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

    /// A frame that arrives late is not shown before its receipt time: the gap repeats the previous
    /// image and the late frame becomes the next sample, so the clip does not jump ahead.
    #[test]
    fn a_gap_repeats_the_previous_frame_not_the_late_one() {
        let mut rec = CameoRecorder::new((16, 16), 10, 30, 0).expect("recorder");
        let first = frame(0, 8, 8, 10);
        let late = frame(1, 8, 8, 200);
        rec.push(&first, 0).expect("first frame");
        let (w, h) = (rec.width, rec.height);
        let fitted = |f: &CameraFrame| crate::encode::fitted_rgba(&f.rgba, f.width, f.height, w, h).expect("fit");
        // 100 ms at 10 fps: one sample elapsed while only the first frame existed.
        rec.push(&late, 100).expect("a late frame");
        rec.tick(MAX_SAMPLES_PER_CALL).expect("catch up");
        assert_eq!(rec.frames(), 1, "the elapsed sample is written");
        assert_eq!(rec.last_written.as_deref(), Some(fitted(&first).as_slice()), "the gap shows the frame that was on screen");
        assert_eq!(fitted_image(&rec).as_deref(), Some(fitted(&late).as_slice()), "the late frame becomes the next sample");
        rec.push(&late, 200).expect("the next sample");
        rec.tick(MAX_SAMPLES_PER_CALL).expect("catch up");
        assert_eq!(rec.frames(), 2);
        assert_eq!(rec.last_written.as_deref(), Some(fitted(&late).as_slice()), "then it is shown");
    }

    /// A frame that arrives before its first sample period does not backdate the picture: no
    /// sample is written, and the clip starts with the frame that was actually received.
    #[test]
    fn the_first_frame_is_not_backdated_to_the_slide_start() {
        let mut rec = CameoRecorder::new((16, 16), 15, 30, 1_000).expect("recorder");
        rec.push(&frame(0, 8, 8, 90), 1_200).expect("late first frame");
        assert_eq!(rec.frames(), 0, "nothing is written before the first sample");
        assert_eq!(fitted_image(&rec).as_deref(), Some(crate::encode::fitted_rgba(&frame(0, 8, 8, 90).rgba, 8, 8, 16, 16).expect("fit").as_slice()));
        rec.tick(MAX_SAMPLES_PER_CALL).expect("tick");
        assert_eq!(rec.frames(), 3, "the elapsed samples are written on the tick, not backdated on push");
    }

    /// A long stall is caught up over passes: one push/tick writes at most
    /// [`MAX_SAMPLES_PER_CALL`] pictures, never a whole slide's worth.
    #[test]
    fn a_long_stall_is_spread_across_passes() {
        let mut rec = CameoRecorder::new((16, 16), 15, 30, 0).expect("recorder");
        rec.push(&frame(0, 8, 8, 10), 0).expect("first");
        rec.push(&frame(1, 8, 8, 20), 60_000).expect("a minute on one slide");
        assert_eq!(rec.frames(), 0, "push does no encoding");
        rec.tick(MAX_SAMPLES_PER_CALL).expect("one pass");
        assert_eq!(rec.frames(), MAX_SAMPLES_PER_CALL);
        rec.tick(MAX_SAMPLES_PER_CALL).expect("next pass");
        assert_eq!(rec.frames(), 2 * MAX_SAMPLES_PER_CALL);
    }

    /// One large sample cannot push the clip past its byte budget: each sample is refused when
    /// its own finished size would pass the budget, so every class of take finalizes instead of
    /// being rejected.
    #[test]
    fn a_large_sample_cannot_push_the_clip_past_its_budget() {
        let mut rec = CameoRecorder::new((64, 64), 15, 30, 0).expect("recorder").with_max_bytes(1 << 20);
        // A noisy picture: every sample is a full frame, not a skip picture.
        for i in 0..4_000u64 {
            let mut rgba = vec![0u8; 64 * 64 * 4];
            for (n, p) in rgba.chunks_mut(4).enumerate() {
                let v = ((i * 31 + n as u64 * 7) % 251) as u8;
                p.copy_from_slice(&[v, 255 - v, v / 2, 255]);
            }
            let frame = CameraFrame { seq: i, at: Instant::now(), width: 64, height: 64, rgba: Arc::from(rgba) };
            rec.push(&frame, i * 67).expect("push");
            rec.tick(MAX_SAMPLES_PER_CALL).expect("tick");
            if rec.capped() {
                break;
            }
        }
        assert!(rec.capped(), "the budget stops the take");
        assert!(rec.track.bytes() <= rec.byte_budget(), "the samples written stay inside the budget");
        let clip = rec.finish(300_000).expect("a byte-capped take still finalizes");
        assert!(clip.mp4.len() <= MAX_CLIP_BYTES);
        assert!(clip.frames > 0);
    }

    /// A close that owes a long gap is written a budget per call, so the caller can spread it
    /// over its own passes instead of paying for the whole gap on one call.
    #[test]
    fn a_close_that_owes_a_long_gap_is_written_a_budget_at_a_time() {
        let mut rec = CameoRecorder::new((32, 32), 15, 30, 0).expect("recorder");
        rec.push(&frame(0, 16, 16, 10), 0).expect("first");
        // Four seconds of slide with one camera frame: 60 samples are owed when it closes.
        let mut sealed = rec.seal(4_000, MAX_SAMPLES_PER_CALL).expect("seal");
        let mut calls = 1;
        let clip = loop {
            match sealed {
                Seal::Done(clip) => break clip,
                Seal::Pending(rec) => {
                    calls += 1;
                    assert!(calls <= 2, "60 samples fit in two passes of {MAX_SAMPLES_PER_CALL}");
                    sealed = rec.seal(4_000, MAX_SAMPLES_PER_CALL).expect("seal");
                }
            }
        };
        assert_eq!(calls, 2);
        assert_eq!(clip.duration_ms, 4_000, "the duration is fixed when the close starts");
        assert_eq!(clip.frames, 60, "every pass writes its budget of the same clip");
        assert_eq!(&clip.mp4[4..8], b"ftyp");
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
            rec.tick(MAX_SAMPLES_PER_CALL).expect("tick");
            if rec.capped() {
                break;
            }
        }
        assert!(rec.capped(), "the byte budget ends the clip before the duration cap");
        assert_eq!(rec.cap(), Some(CameoCap::Bytes), "the limit that stopped it is reported");
        let clip = rec.finish(now).expect("the clip so far is kept, not rejected");
        assert!(clip.frames > 0);
        assert!(clip.mp4.len() <= MAX_CLIP_BYTES);
    }

    /// A sample larger than the remaining budget is refused on its own size, not on the size of
    /// an earlier one: the refusal ends the clip with what it holds, and the take finalizes.
    #[test]
    fn a_sample_larger_than_the_remaining_budget_is_refused() {
        use crate::encode::H264Mp4Track;

        // The largest sample a 64x64 clip at QP 10 writes: later samples must fit beside it.
        let mut probe = H264Mp4Track::new(64, 64, 15, 10).expect("track");
        let mut noisy = vec![0u8; 64 * 64 * 4];
        for (n, p) in noisy.chunks_mut(4).enumerate() {
            let v = ((n * 7) % 251) as u8;
            p.copy_from_slice(&[v, 255 - v, v / 2, 255]);
        }
        probe.push_rgba(&noisy).expect("probe sample");
        let one = probe.bytes();
        assert!(one > 0, "the probe wrote a sample to measure");
        // A budget that holds samples but not another one of the probe's size: the next sample
        // is refused for its own bytes, and the capped clip still finalizes inside the budget.
        let mut small = H264Mp4Track::new(64, 64, 15, 10).expect("track");
        small.push_rgba(&vec![128u8; 64 * 64 * 4]).expect("small sample");
        let budget = small.bytes() + one.saturating_sub(1);
        assert!(!small.push_rgba_capped(&noisy, budget).expect("checked push"), "the oversized sample is refused");
        assert!(small.bytes() <= budget, "the refused sample wrote nothing: {}", small.bytes());
    }

    /// Once the byte cap refuses a sample, later passes write nothing more: the refused picture
    /// is the encoder's reference, so re-encoding the same frame would slip all-skipped samples
    /// past the cap that the decoder renders from the wrong picture.
    #[test]
    fn no_samples_are_written_after_the_byte_cap() {
        let mut rec = CameoRecorder::new((64, 64), 15, 30, 0).expect("recorder").with_max_bytes(8 << 10);
        let mut now = 0u64;
        for i in 0..1_000u64 {
            now = i * 67;
            rec.push(&frame(i, 64, 64, (i % 251) as u8), now).expect("push");
            rec.tick(MAX_SAMPLES_PER_CALL).expect("tick");
            if rec.capped() {
                break;
            }
        }
        assert_eq!(rec.cap(), Some(CameoCap::Bytes), "the byte budget ended the clip");
        let held = rec.track.bytes();
        let frames = rec.frames();
        // Later passes, and the close itself, must not encode the held frame again.
        rec.tick(MAX_SAMPLES_PER_CALL).expect("tick after the cap");
        assert_eq!(rec.track.bytes(), held, "no sample slipped past the cap");
        assert_eq!(rec.frames(), frames, "the frame count is frozen at the cap");
        let clip = rec.finish(now).expect("the capped clip finalizes");
        assert_eq!(clip.frames, frames, "the close wrote nothing after the cap");
        assert!(clip.mp4.len() <= MAX_CLIP_BYTES);
    }

    /// A clip the byte budget stopped can never write another sample: sealing it returns the clip
    /// it has, never a pending recorder a caller would keep polling for nothing.
    #[test]
    fn a_byte_capped_close_is_done_not_pending() {
        let mut rec = CameoRecorder::new((64, 64), 15, 30, 0).expect("recorder").with_max_bytes(8 << 10);
        let mut now = 0u64;
        for i in 0..1_000u64 {
            now = i * 67;
            rec.push(&frame(i, 64, 64, (i % 251) as u8), now).expect("push");
            rec.tick(MAX_SAMPLES_PER_CALL).expect("tick");
            if rec.capped() {
                break;
            }
        }
        assert!(rec.capped(), "the byte budget ended the clip");
        match rec.seal(now, MAX_SAMPLES_PER_CALL).expect("seal") {
            Seal::Done(clip) => assert!(clip.frames > 0, "the samples written so far are the clip"),
            Seal::Pending(_) => panic!("a byte-capped clip is finished, not waiting for more samples"),
        }
    }

    #[test]
    fn a_dropped_camera_channel_reads_as_closed_once() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut session = CameraSession::new(Instant::now(), rx);
        drop(tx);
        let first = session.drain();
        assert_eq!(first.events.len(), 1, "{:?}", first.events);
        assert!(matches!(first.events.first(), Some(CameraEvent::Closed)));
        assert!(!first.truncated);
        let second = session.drain();
        assert!(second.events.is_empty(), "the loss is reported once, not every pass: {:?}", second.events);
    }

    #[test]
    fn a_flooded_channel_reports_its_truncation() {
        let (tx, rx) = std::sync::mpsc::channel();
        for seq in 0..(MAX_DRAIN + 4) {
            tx.send(CameraEvent::Frame(frame(seq as u64, 16, 16, 1))).expect("send");
        }
        let mut session = CameraSession::new(Instant::now(), rx);
        let d = session.drain();
        assert_eq!(d.events.len(), MAX_DRAIN);
        assert!(d.truncated, "the pass stopped at the cap with more waiting");
        let rest = session.drain();
        assert_eq!(rest.events.len(), 4, "the rest comes next pass");
        assert!(!rest.truncated);
    }

    /// A frame stamped before the slide started is not part of the clip.
    #[test]
    fn a_frame_before_the_slide_start_is_dropped() {
        let mut rec = CameoRecorder::new((16, 16), 15, 30, 5_000).expect("recorder");
        rec.push(&frame(0, 8, 8, 10), 4_000).expect("push");
        assert!(rec.image.is_none(), "an early frame is not kept");
        assert!(rec.finish(6_000).is_err(), "and it cannot make a clip alone");
    }

    /// The target size and frame rate are clamped, and the clamp is the clip's real size.
    #[test]
    fn out_of_range_targets_are_clamped() {
        let rec = CameoRecorder::new((1, 99_999), 0, 30, 0).expect("recorder");
        assert_eq!((rec.width(), rec.height()), (16, 1280));
        let rec = CameoRecorder::new((32, 32), 1_000, 30, 0).expect("recorder");
        assert_eq!(rec.fps, 60);
    }

    #[test]
    fn a_queued_frame_knows_how_old_it_is() {
        let now = Instant::now();
        let mut a = frame(0, 16, 16, 10);
        let mut b = frame(1, 16, 16, 20);
        a.at = now;
        b.at = now + std::time::Duration::from_millis(70);
        assert_eq!(a.lag_ms(b.at), 70);
        assert_eq!(b.lag_ms(b.at), 0);
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
