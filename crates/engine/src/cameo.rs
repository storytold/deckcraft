//! Recorded Cameo: the show-side capture state machine.
//!
//! One camera session serves the whole show. Every slide that carries a Cameo records its own
//! video-only MP4 while it is on screen; the host drives this with [`CameoRun::enter_slide`] when a
//! slide appears, [`CameoRun::poll`] once per pass, and [`CameoRun::leave_slide`] when it goes.
//! [`CameoRun::commit`] turns the finished clips into shapes through the `show.cameo` command.
//!
//! A camera that fails to open, or is unplugged mid-show, never stops the show: the clip finished
//! so far is kept, later slides record no camera, and the status says why.
//!
//! **Not wired into the Record Slide Show UI or state machine yet.** This is the recording hook
//! surface: the show host must call `enter_slide`/`poll`/`leave_slide` on its clock and `commit`
//! at the end. Until it does, no Cameo records. See `ROADMAP.md` and `docs/gaps.md`.

use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_media::{
    CameoCap, CameoClip, CameoRecorder, CameraConfig, CameraError, CameraEvent, CameraIn, CameraSession, MAX_CLIP_BYTES, MAX_CLIP_MS,
};

use crate::{Result, Session};

/// All recorded clips in one show may not exceed this; past it, camera recording stops.
pub const MAX_TOTAL_BYTES: usize = 512 << 20;

/// Where the camera is in the recording: the show reads this for its status line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameoStatus {
    /// The camera opened; no slide is recording right now.
    Ready,
    /// A slide is recording.
    Recording,
    /// No camera: no device, permission denied, or the host has none (web, tests).
    NoCamera,
    /// The camera failed or was unplugged mid-recording.
    Lost,
    /// The clip or the show hit its size or duration cap.
    Capped,
}

impl CameoStatus {
    pub fn is_recording(self) -> bool {
        self == CameoStatus::Recording
    }

    /// Recording cannot start again (or, for `Capped`, must not).
    pub fn is_stopped(self) -> bool {
        matches!(self, CameoStatus::NoCamera | CameoStatus::Lost | CameoStatus::Capped)
    }
}

/// What a [`CameoRun::commit`] did: the slides that now carry a Cameo, and the ones whose clip
/// stayed for a retry with why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitReport {
    pub committed: Vec<usize>,
    pub failed: Vec<(usize, String)>,
}

/// The recording state machine for one show.
pub struct CameoRun {
    camera: Option<Box<dyn CameraIn>>,
    session: Option<CameraSession>,
    recorder: Option<CameoRecorder>,
    slide: Option<usize>,
    status: CameoStatus,
    reason: Option<String>,
    clips: Vec<(usize, CameoClip)>,
    bytes: usize,
    qp: u8,
}

impl CameoRun {
    /// Open the camera now. `None` (web, headless tests) records nothing and reports
    /// [`CameoStatus::NoCamera`].
    pub fn start(camera: Option<Box<dyn CameraIn>>, config: &CameraConfig) -> Self {
        let mut run = Self::without_camera();
        let Some(mut camera) = camera else { return run };
        match camera.start(config) {
            Ok(session) => {
                run.session = Some(session);
                run.status = CameoStatus::Ready;
                run.reason = None;
                run.camera = Some(camera);
            }
            Err(e) => {
                run.status = CameoStatus::NoCamera;
                run.reason = Some(e.to_string());
            }
        }
        run
    }

    /// A run with no camera at all.
    pub fn without_camera() -> Self {
        CameoRun {
            camera: None,
            session: None,
            recorder: None,
            slide: None,
            status: CameoStatus::NoCamera,
            reason: None,
            clips: Vec::new(),
            bytes: 0,
            qp: 30,
        }
    }

    /// Quantisation parameter for the clips (10–51, lower is better and larger).
    pub fn with_qp(mut self, qp: u8) -> Self {
        self.qp = qp.clamp(10, 51);
        self
    }

    pub fn status(&self) -> CameoStatus {
        self.status
    }

    /// Why the camera is unavailable or stopped, for the show's status line.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    pub fn clips(&self) -> &[(usize, CameoClip)] {
        &self.clips
    }

    /// Start recording `slide` at `target` pixels. A slide already recording is closed first.
    ///
    /// Returns whether recording started: `false` when there is no camera or it has stopped, so
    /// the host can tell a slide that records from one that is silently ignored.
    pub fn enter_slide(&mut self, slide: usize, target: (u32, u32), now_ms: u64) -> bool {
        if self.recorder.is_some() {
            self.close(now_ms);
        }
        if self.session.is_none() || self.status.is_stopped() {
            return false;
        }
        match CameoRecorder::new(target, 15, self.qp, now_ms) {
            Ok(rec) => {
                self.recorder = Some(rec);
                self.slide = Some(slide);
                self.status = CameoStatus::Recording;
                true
            }
            Err(e) => {
                self.status = CameoStatus::NoCamera;
                self.reason = Some(e.to_string());
                false
            }
        }
    }

    /// Drain the camera: write frames, and close the clip on a failure. Never blocks, and does at
    /// most [`deckcraft_media::MAX_SAMPLES_PER_CALL`] pictures of encoding however many events the
    /// camera queued; the rest is caught up by later passes.
    pub fn poll(&mut self, now_ms: u64) {
        let Some(session) = &mut self.session else {
            return;
        };
        let drained = session.drain();
        // Time each frame by its own receipt, not by this poll: a batch that queued up while the
        // show was busy would otherwise land on one sample position.
        let newest = drained.events.iter().filter_map(|e| if let CameraEvent::Frame(f) = e { Some(f.at) } else { None }).max();
        let mut lost = None;
        for event in drained.events {
            match event {
                CameraEvent::Frame(frame) => {
                    let at_ms = newest.map_or(now_ms, |n| now_ms.saturating_sub(frame.lag_ms(n)));
                    if let Some(rec) = &mut self.recorder
                        && let Err(e) = rec.push(&frame, at_ms)
                    {
                        lost = Some(CameraError::Backend(e.to_string()));
                        break;
                    }
                }
                CameraEvent::Failed(e) => {
                    lost = Some(e);
                    break;
                }
                CameraEvent::Closed => {
                    lost = Some(CameraError::Lost);
                    break;
                }
                CameraEvent::Opened(_) => {}
            }
        }
        if lost.is_none()
            && let Some(rec) = &mut self.recorder
            && let Err(e) = rec.tick(deckcraft_media::MAX_SAMPLES_PER_CALL)
        {
            lost = Some(CameraError::Backend(e.to_string()));
        }
        if let Some(e) = lost {
            self.close(now_ms);
            self.stop_camera();
            self.status = CameoStatus::Lost;
            self.reason = Some(e.to_string());
            return;
        }
        if let Some(cap) = self.recorder.as_ref().and_then(CameoRecorder::cap) {
            self.close(now_ms);
            if self.status == CameoStatus::Recording {
                // The per-slide cap ends only this clip: the next slide records again.
                self.status = CameoStatus::Ready;
                self.reason = Some(match cap {
                    CameoCap::Duration => format!("the clip reached {} minutes", MAX_CLIP_MS / 60_000),
                    CameoCap::Bytes => format!("the clip reached {} MB", MAX_CLIP_BYTES >> 20),
                });
            }
        }
    }

    /// Stop recording the current slide; its clip is kept if it has any frames. Returns whether a
    /// recording was running.
    pub fn leave_slide(&mut self, now_ms: u64) -> bool {
        let was = self.recorder.is_some();
        self.close(now_ms);
        if self.status == CameoStatus::Recording {
            self.status = CameoStatus::Ready;
        }
        was
    }

    /// Close the current slide, stop the camera, and return every clip in slide order.
    pub fn finish(&mut self, now_ms: u64) -> Vec<(usize, CameoClip)> {
        self.close(now_ms);
        self.stop_camera();
        let mut clips = std::mem::take(&mut self.clips);
        clips.sort_by_key(|(slide, _)| *slide);
        clips
    }

    /// Execute `show.cameo` for every finished clip, through the byte path (no base64/JSON).
    ///
    /// Every clip is attempted; a failure keeps that clip in [`CameoRun::clips`] so the host can
    /// report or retry it, and appears in the report with its slide index and error. `Err` is
    /// returned only when nothing committed at all.
    pub fn commit(&mut self, s: &mut Session, now_ms: u64) -> Result<CommitReport> {
        let clips = self.finish(now_ms);
        let mut report = CommitReport::default();
        let mut first_err = None;
        for (slide, clip) in clips {
            let CameoClip { mp4, width, height, duration_ms, frames } = clip;
            let bytes: deckcraft_media::Bytes = Arc::new(mp4);
            let recorded = crate::cmd::camera::Recorded {
                name: format!("cameo{}.mp4", slide + 1),
                bytes: bytes.clone(),
                slide: Some(slide),
                duration_ms: Some(duration_ms),
                rect: None,
            };
            match crate::cmd::camera::commit_recorded(s, recorded) {
                Ok(_) => report.committed.push(slide),
                Err(e) => {
                    report.failed.push((slide, e.to_string()));
                    let mp4 = match Arc::try_unwrap(bytes) {
                        Ok(v) => v,
                        Err(shared) => (*shared).clone(),
                    };
                    self.clips.push((slide, CameoClip { mp4, width, height, duration_ms, frames }));
                    first_err.get_or_insert(e);
                }
            }
        }
        if report.committed.is_empty()
            && let Some(e) = first_err
        {
            return Err(e);
        }
        if !report.failed.is_empty() {
            self.reason =
                Some(format!("{} of {} camera clips were not committed", report.failed.len(), report.committed.len() + report.failed.len()));
        }
        Ok(report)
    }

    fn close(&mut self, now_ms: u64) {
        let Some(rec) = self.recorder.take() else { return };
        let slide = self.slide.take();
        match rec.finish(now_ms) {
            Ok(clip) => {
                let total = self.bytes.saturating_add(clip.mp4.len());
                if total > MAX_TOTAL_BYTES {
                    self.status = CameoStatus::Capped;
                    self.reason = Some(format!("the show's camera clips reached {} MB", MAX_TOTAL_BYTES >> 20));
                    // Nothing more will be kept, so stop capturing frames that would be thrown away.
                    self.stop_camera();
                    return;
                }
                self.bytes = total;
                if let Some(slide) = slide {
                    self.clips.push((slide, clip));
                }
            }
            Err(e) => {
                // The clip was rejected (too large, or it never got a frame). This ends only this
                // slide, so the reason is reported and the next slide records again.
                if self.status == CameoStatus::Recording {
                    self.status = CameoStatus::Ready;
                }
                self.reason = Some(e.to_string());
            }
        }
    }

    fn stop_camera(&mut self) {
        self.session = None;
        if let Some(camera) = &mut self.camera {
            camera.stop();
        }
        self.camera = None;
    }
}

impl Drop for CameoRun {
    fn drop(&mut self) {
        self.stop_camera();
    }
}

/// The camera picture size for a Cameo box in points: 96 dpi, even, a round Cameo square.
pub fn target_px(rect: Xfrm, round: bool) -> (u32, u32) {
    let px = |v: f64| -> u32 {
        if !v.is_finite() || v <= 0.0 {
            return 16;
        }
        let even = (v * 4.0 / 3.0).round() as u32;
        even.clamp(16, 1280) & !1
    };
    if round {
        let side = px(rect.w.max(rect.h));
        (side, side)
    } else {
        (px(rect.w), px(rect.h))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Sender, channel};
    use std::sync::{Arc, Mutex};

    use deckcraft_media::{CameraConfig, CameraError, CameraEvent, CameraFrame, CameraIn, CameraInfo, CameraSession};
    use serde_json::json;
    use web_time::Instant;

    use super::*;

    /// A camera a test drives directly: frames and failures are queued by the test.
    struct Fake {
        events: Arc<Mutex<Option<Sender<CameraEvent>>>>,
        fail_start: Option<CameraError>,
        stopped: Arc<AtomicBool>,
    }

    impl CameraIn for Fake {
        fn start(&mut self, _config: &CameraConfig) -> std::result::Result<CameraSession, CameraError> {
            if let Some(e) = self.fail_start.clone() {
                return Err(e);
            }
            let (tx, rx) = channel();
            *self.events.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
            Ok(CameraSession::new(Instant::now(), rx))
        }

        fn stop(&mut self) {
            self.stopped.store(true, Ordering::SeqCst);
        }
    }

    fn fake() -> (Box<Fake>, Arc<Mutex<Option<Sender<CameraEvent>>>>, Arc<AtomicBool>) {
        let events = Arc::new(Mutex::new(None));
        let stopped = Arc::new(AtomicBool::new(false));
        let camera = Box::new(Fake { events: events.clone(), fail_start: None, stopped: stopped.clone() });
        (camera, events, stopped)
    }

    fn frame(seq: u64, w: u32, h: u32) -> CameraFrame {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for (i, p) in rgba.chunks_mut(4).enumerate() {
            p.copy_from_slice(&[(i % 251) as u8, (seq % 251) as u8, 90, 255]);
        }
        CameraFrame { seq, at: Instant::now(), width: w, height: h, rgba: Arc::from(rgba) }
    }

    fn send(events: &Arc<Mutex<Option<Sender<CameraEvent>>>>, event: CameraEvent) {
        let tx = events.lock().unwrap_or_else(|e| e.into_inner()).clone().expect("camera started");
        tx.send(event).expect("send");
    }

    #[test]
    fn a_show_with_no_camera_records_nothing_and_keeps_running() {
        let mut run = CameoRun::without_camera();
        run.enter_slide(0, (64, 64), 0);
        run.poll(100);
        run.leave_slide(1_000);
        assert_eq!(run.status(), CameoStatus::NoCamera);
        assert!(run.finish(1_000).is_empty());
    }

    #[test]
    fn target_sizes_are_even_and_round_cameos_are_square() {
        let (w, h) = target_px(Xfrm::new(0.0, 0.0, 162.0, 90.0), false);
        assert_eq!((w, h), (216, 120));
        let (w, h) = target_px(Xfrm::new(0.0, 0.0, 162.0, 90.0), true);
        assert_eq!(w, h);
        assert!(w.is_multiple_of(2));
    }

    #[test]
    fn slide_entries_and_leaves_close_one_clip_each() {
        let (camera, events, stopped) = fake();
        let mut run = CameoRun::start(Some(camera), &CameraConfig::default());
        assert_eq!(run.status(), CameoStatus::Ready);
        run.enter_slide(0, (64, 64), 0);
        assert_eq!(run.status(), CameoStatus::Recording);
        send(&events, CameraEvent::Opened(CameraInfo { device: None, width: 640, height: 480, fps: 30.0 }));
        send(&events, CameraEvent::Frame(frame(0, 640, 480)));
        run.poll(500);
        run.leave_slide(1_000);
        assert_eq!(run.status(), CameoStatus::Ready);
        let clips = run.finish(1_000);
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].0, 0);
        assert_eq!(clips[0].1.duration_ms, 1_000);
        assert_eq!(clips[0].1.frames, 15);
        assert!(stopped.load(Ordering::SeqCst), "the camera is stopped when the show ends");
    }

    #[test]
    fn each_slide_with_a_cameo_gets_its_own_clip() {
        let (camera, events, _) = fake();
        let mut run = CameoRun::start(Some(camera), &CameraConfig::default());
        for slide in 0..3u64 {
            run.enter_slide(slide as usize, (32, 32), slide * 1_000);
            send(&events, CameraEvent::Frame(frame(slide, 32, 32)));
            run.poll(slide * 1_000 + 500);
            run.leave_slide(slide * 1_000 + 1_000);
        }
        let clips = run.finish(3_000);
        assert_eq!(clips.iter().map(|(s, _)| *s).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert!(clips.iter().all(|(_, c)| c.frames > 0));
    }

    #[test]
    fn a_camera_lost_mid_slide_keeps_its_clip_and_stops_recording() {
        let (camera, events, stopped) = fake();
        let mut run = CameoRun::start(Some(camera), &CameraConfig::default());
        run.enter_slide(0, (32, 32), 0);
        send(&events, CameraEvent::Frame(frame(0, 32, 32)));
        run.poll(800);
        send(&events, CameraEvent::Failed(CameraError::Lost));
        run.poll(900);
        assert_eq!(run.status(), CameoStatus::Lost);
        assert!(stopped.load(Ordering::SeqCst));
        // Later slides do not record, and the finished slide's clip survives.
        run.enter_slide(1, (32, 32), 1_000);
        run.poll(1_500);
        run.leave_slide(2_000);
        let clips = run.finish(2_000);
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].0, 0);
        assert_eq!(clips[0].1.duration_ms, 900);
    }

    #[test]
    fn a_camera_worker_that_goes_away_ends_the_recording() {
        let (camera, events, stopped) = fake();
        let mut run = CameoRun::start(Some(camera), &CameraConfig::default());
        run.enter_slide(0, (32, 32), 0);
        send(&events, CameraEvent::Frame(frame(0, 32, 32)));
        run.poll(500);
        // The worker drops its sender without sending Closed: the drain synthesises it.
        events.lock().unwrap_or_else(|e| e.into_inner()).take();
        run.poll(1_000);
        assert_eq!(run.status(), CameoStatus::Lost);
        assert!(stopped.load(Ordering::SeqCst));
        assert_eq!(run.finish(1_000).len(), 1, "the clip so far is kept");
    }

    #[test]
    fn a_camera_that_never_opens_reports_no_camera() {
        let events = Arc::new(Mutex::new(None));
        let camera = Box::new(Fake { events, fail_start: Some(CameraError::PermissionDenied), stopped: Arc::new(AtomicBool::new(false)) });
        let mut run = CameoRun::start(Some(camera), &CameraConfig::default());
        assert_eq!(run.status(), CameoStatus::NoCamera);
        assert_eq!(run.reason(), Some("camera access was denied"));
        run.enter_slide(0, (32, 32), 0);
        assert!(run.finish(100).is_empty());
    }

    #[test]
    fn a_capped_clip_only_ends_its_own_slide() {
        let (camera, events, _) = fake();
        let mut run = CameoRun::start(Some(camera), &CameraConfig::default());
        let cap = u64::from(MAX_CLIP_MS);
        run.enter_slide(0, (32, 32), 0);
        send(&events, CameraEvent::Frame(frame(0, 32, 32)));
        run.poll(500);
        // The cap is noticed on the next frame: a camera keeps delivering them.
        send(&events, CameraEvent::Frame(frame(1, 32, 32)));
        run.poll(cap + 1);
        assert_eq!(run.status(), CameoStatus::Ready, "the per-slide cap does not stop the show");
        assert!(run.reason().is_some_and(|r| r.contains("minutes")), "the reason says why the clip ended");
        // The next slide records again: only the show-level cap stops Cameo recording.
        run.enter_slide(1, (32, 32), cap + 2);
        assert_eq!(run.status(), CameoStatus::Recording);
        send(&events, CameraEvent::Frame(frame(1, 32, 32)));
        run.poll(cap + 500);
        run.leave_slide(cap + 1_000);
        let clips = run.finish(cap + 1_000);
        assert_eq!(clips.iter().map(|(s, _)| *s).collect::<Vec<_>>(), vec![0, 1]);
    }

    #[test]
    fn the_show_cap_stops_the_camera_and_later_slides() {
        let (camera, events, stopped) = fake();
        let mut run = CameoRun::start(Some(camera), &CameraConfig::default());
        run.enter_slide(0, (32, 32), 0);
        send(&events, CameraEvent::Frame(frame(0, 32, 32)));
        run.poll(500);
        // Pretend the show has already used its whole byte budget; the next clip exceeds it.
        run.bytes = MAX_TOTAL_BYTES;
        run.leave_slide(1_000);
        assert_eq!(run.status(), CameoStatus::Capped);
        assert!(stopped.load(Ordering::SeqCst), "the camera stops when the show cap is reached");
        assert!(run.clips().is_empty(), "the clip over the cap is not kept");
        run.enter_slide(1, (32, 32), 2_000);
        assert_eq!(run.status(), CameoStatus::Capped, "later slides do not record");
        assert!(run.finish(3_000).is_empty());
    }

    #[test]
    fn a_slide_with_no_frames_says_why_its_clip_is_missing() {
        let (camera, events, _) = fake();
        let mut run = CameoRun::start(Some(camera), &CameraConfig::default());
        run.enter_slide(0, (32, 32), 0);
        run.leave_slide(1_000);
        assert_eq!(run.status(), CameoStatus::Ready);
        assert!(run.reason().is_some_and(|r| r.contains("camera frames")), "the rejected clip reports why");
        // A slide that does get frames still records.
        run.enter_slide(1, (32, 32), 2_000);
        assert_eq!(run.status(), CameoStatus::Recording);
        send(&events, CameraEvent::Frame(frame(1, 32, 32)));
        run.poll(2_500);
        run.leave_slide(3_000);
        assert_eq!(run.finish(3_000).len(), 1);
    }

    #[test]
    fn a_failed_clip_stays_for_the_host_instead_of_being_dropped() {
        let mut s = Session::with_new();
        let mut run = CameoRun::without_camera();
        run.clips = vec![(0, small_clip()), (1, small_clip())];
        let report = run.commit(&mut s, 0).expect("the clip that fits its slide commits");
        assert_eq!(report.committed, vec![0]);
        assert_eq!(report.failed.iter().map(|(sl, _)| *sl).collect::<Vec<_>>(), vec![1], "the rejected slide and its error are reported");
        assert!(report.failed[0].1.contains("no slide 2"), "the report carries the command's own reason: {}", report.failed[0].1);
        assert_eq!(run.clips().iter().map(|(sl, _)| *sl).collect::<Vec<_>>(), vec![1], "the rejected clip is kept");
        // A kept clip can be retried once its slide exists.
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({})).expect("slide");
        let report = run.commit(&mut s, 0).expect("the retry commits");
        assert_eq!(report.committed, vec![1]);
        assert!(run.clips().is_empty(), "a committed clip is not kept");
    }

    #[test]
    fn when_every_clip_fails_commit_is_an_error_and_none_is_dropped() {
        let mut s = Session::with_new();
        let mut run = CameoRun::without_camera();
        run.clips = vec![(42, small_clip()), (43, small_clip())];
        assert!(run.commit(&mut s, 0).is_err(), "nothing committed is an error");
        assert_eq!(run.clips().len(), 2, "both clips stay for a retry");
        let doc = s.doc().expect("doc");
        let mut cameos = 0;
        for slide in &doc.doc.slides {
            deckcraft_model::walk(&slide.shapes, &mut |sh, _| cameos += usize::from(sh.is_cameo()));
        }
        assert_eq!(cameos, 0, "no Cameo was added");
    }

    /// A short real MP4, for tests that only need a clip.
    fn small_clip() -> CameoClip {
        let mut track = deckcraft_media::H264Mp4Track::new(32, 32, 15, 30).expect("track");
        for i in 0..5 {
            track.push_rgba(&frame(i, 32, 32).rgba).expect("push");
        }
        CameoClip { mp4: track.finish().expect("finish"), width: 32, height: 32, duration_ms: 333, frames: 5 }
    }

    #[test]
    fn a_clip_commits_as_a_cameo_shape_on_its_slide() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({})).expect("slide");
        // Build a real MP4 with the shared track, then commit it through the command.
        let mut track = deckcraft_media::H264Mp4Track::new(32, 32, 15, 30).expect("track");
        for i in 0..15 {
            track.push_rgba(&frame(i, 32, 32).rgba).expect("push");
        }
        let mp4 = track.finish().expect("finish");
        let data = crate::cmd::base64_encode(&mp4);
        let r = s.execute("show.cameo", &json!({"slide": 1, "name": "cameo2.mp4", "data": data, "durationMs": 1_000})).expect("commit");
        let id = deckcraft_model::ShapeId(r["id"].as_u64().expect("id") as u32);
        let doc = s.doc().expect("doc");
        let sh = doc.doc.slides[1].shape(id).expect("shape");
        assert!(sh.is_cameo());
        let cameo = sh.cameo.as_ref().expect("cameo");
        assert!(!cameo.live);
        assert!(cameo.autoplay);
        let recorded = doc.doc.media(cameo.recorded.expect("recorded")).expect("media");
        assert_eq!(recorded.content_type, "video/mp4");
        // The command is undoable.
        s.execute("edit.undo", &json!({})).expect("undo");
        let doc = s.doc().expect("doc");
        assert!(doc.doc.slides[1].shape(id).is_none());
    }

    #[test]
    fn a_second_recording_replaces_the_first() {
        let mut s = Session::with_new();
        s.execute("insert.cameo", &json!({})).expect("cameo");
        let mut track = deckcraft_media::H264Mp4Track::new(32, 32, 15, 30).expect("track");
        for i in 0..10 {
            track.push_rgba(&frame(i, 32, 32).rgba).expect("push");
        }
        let mp4 = track.finish().expect("finish");
        let data = crate::cmd::base64_encode(&mp4);
        let r = s.execute("show.cameo", &json!({"data": data, "durationMs": 700})).expect("commit");
        let id = deckcraft_model::ShapeId(r["id"].as_u64().expect("id") as u32);
        let doc = s.doc().expect("doc");
        let mut cameos = vec![];
        deckcraft_model::walk(&doc.doc.slides[0].shapes, &mut |sh, _| {
            if sh.is_cameo() {
                cameos.push(sh.id);
            }
        });
        assert_eq!(cameos, vec![id], "the existing Cameo is reused, not duplicated");
    }

    #[test]
    fn the_wrong_container_is_refused() {
        let mut s = Session::with_new();
        let data = crate::cmd::base64_encode(b"not an mp4");
        let err = s.execute("show.cameo", &json!({"data": data})).expect_err("a text file is not a clip").to_string();
        assert!(err.contains("is not an MP4 video"), "{err}");
        let doc = s.doc().expect("doc");
        let mut cameos = 0;
        for slide in &doc.doc.slides {
            deckcraft_model::walk(&slide.shapes, &mut |sh, _| cameos += usize::from(sh.is_cameo()));
        }
        assert_eq!(cameos, 0, "nothing was added");
    }

    #[test]
    fn target_px_handles_bad_rectangles() {
        let px = |w: f64, h: f64, round: bool| target_px(Xfrm::new(0.0, 0.0, w, h), round);
        assert_eq!(px(0.0, 0.0, false), (16, 16), "zero sizes floor at 16");
        assert_eq!(px(-10.0, 5.0, false), (16, 16), "negative widths floor at 16");
        assert_eq!(px(f64::NAN, 100.0, false), (16, 132), "a non-finite side floors at 16, the other scales");
        assert_eq!(px(f64::INFINITY, 100.0, true), (16, 16), "a non-finite side is 16, not the maximum");
        assert_eq!(px(1e9, 100.0, true), (1280, 1280), "a huge side caps at 1280 and stays even");
        assert_eq!(px(162.0, 100.0, true), (216, 216), "a round cameo is one even square");
        for (w, h) in [(33.0, 65.0), (1.0, 1279.0)] {
            let (a, b) = px(w, h, false);
            assert!(a.is_multiple_of(2) && b.is_multiple_of(2), "{a}×{b} is even");
            assert!((16..=1280).contains(&a) && (16..=1280).contains(&b));
        }
    }

    /// A flood of queued frames is drained for timing but only one pass's worth of pictures is
    /// encoded, so a busy show does not stall on the camera.
    #[test]
    fn a_poll_encodes_at_most_a_budget_of_samples() {
        let (camera, events, _) = fake();
        let mut run = CameoRun::start(Some(camera), &CameraConfig::default());
        run.enter_slide(0, (32, 32), 0);
        // Frames spanning two seconds, all queued before one poll.
        for i in 0..40u64 {
            send(&events, CameraEvent::Frame(frame(i, 32, 32)));
        }
        run.poll(2_000);
        let rec = run.recorder.as_ref().expect("recording");
        assert!(rec.frames() <= deckcraft_media::MAX_SAMPLES_PER_CALL, "one pass wrote {} pictures", rec.frames());
        send(&events, CameraEvent::Frame(frame(41, 32, 32)));
        run.poll(4_000);
        let after = run.recorder.as_ref().map(CameoRecorder::frames).unwrap_or(0);
        assert!(after > deckcraft_media::MAX_SAMPLES_PER_CALL, "the next pass catches up, got {after}");
    }
}
