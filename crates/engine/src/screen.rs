//! Screen recording: the session state that drives a host's capture and the MP4 recorder.
//!
//! The host (the desktop app) installs a [`ScreenIn`]; the UI calls [`ScreenState::tick`] once a
//! frame while a recording runs, which pulls captured frames and feeds them to the recorder. All
//! capture and encoder failures become one message and cancel the recording cleanly: a missing
//! device, a denied permission or a broken stream never panics.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use deckcraft_screen::{CaptureOptions, CaptureStatus, CaptureTarget, RecorderConfig, RecorderOutput, ScreenError, ScreenIn, ScreenRecorder};

/// What a stopped recording left on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoppedRecording {
    pub path: PathBuf,
    pub frames: u64,
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
}

struct Active {
    recorder: ScreenRecorder,
    path: PathBuf,
    fps: u32,
    paused: bool,
    /// A frame is wanted now (the start, or the frame after a resume).
    due: bool,
    /// Earliest time (seconds, UI clock) for the next frame.
    next_at: f64,
    frames: u64,
}

/// The session's screen capture and recording state.
#[derive(Default)]
pub struct ScreenState {
    host: Option<Box<dyn ScreenIn>>,
    active: Option<Active>,
}

impl ScreenState {
    /// Install the host device; `None` makes capture unavailable (headless tests, wasm).
    pub fn set_host(&mut self, host: Option<Box<dyn ScreenIn>>) {
        if let Some(active) = self.active.take() {
            if let Some(host) = self.host.as_mut() {
                host.stop();
            }
            let _ = std::fs::remove_file(active.path);
        }
        self.host = host;
    }

    pub fn host_installed(&self) -> bool {
        self.host.is_some()
    }

    pub fn is_recording(&self) -> bool {
        self.active.is_some()
    }

    pub fn is_paused(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.paused)
    }

    /// Frames encoded so far.
    pub fn frames(&self) -> u64 {
        self.active.as_ref().map_or(0, |a| a.frames)
    }

    /// Recorded length in milliseconds (paused time is not counted).
    pub fn elapsed_ms(&self) -> u64 {
        self.active.as_ref().map_or(0, |a| a.frames.saturating_mul(1000) / u64::from(a.fps.max(1)))
    }

    /// What the host can do right now.
    pub fn status(&mut self) -> CaptureStatus {
        match self.host.as_mut() {
            Some(h) => h.status(),
            None => CaptureStatus::Unsupported,
        }
    }

    /// The displays and windows the host can capture.
    pub fn targets(&mut self) -> Result<Vec<CaptureTarget>, ScreenError> {
        self.host.as_mut().ok_or(ScreenError::Unavailable)?.targets()
    }

    /// Ask the system for capture permission.
    pub fn request_permission(&mut self) -> bool {
        self.host.as_mut().is_some_and(|h| h.request_permission())
    }

    /// Start recording. Any previous recording must have been stopped or cancelled first.
    pub fn start(&mut self, options: CaptureOptions, recorder: RecorderConfig) -> Result<(), ScreenError> {
        if self.active.is_some() {
            return Err(ScreenError::Busy);
        }
        let Some(host) = self.host.as_mut() else { return Err(ScreenError::Unavailable) };
        match host.status() {
            CaptureStatus::Ready => {}
            CaptureStatus::Unsupported => return Err(ScreenError::Unavailable),
            CaptureStatus::PermissionDenied => return Err(ScreenError::PermissionDenied),
        }
        if host.targets().is_ok_and(|t| t.is_empty()) {
            return Err(ScreenError::NoDevice);
        }
        let path = temp_path();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| ScreenError::Failed(format!("couldn't start a screen recording: {e}")))?;
        let recorder = match ScreenRecorder::new(Box::new(file), recorder) {
            Ok(r) => r,
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                return Err(ScreenError::Failed(format!("couldn't start a screen recording: {e}")));
            }
        };
        if let Err(e) = host.start(&options) {
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
        self.active = Some(Active { recorder, path, fps: options.fps.clamp(1, 240), paused: false, due: true, next_at: 0.0, frames: 0 });
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), ScreenError> {
        let Some(active) = self.active.as_mut() else { return Err(ScreenError::NotRecording) };
        active.paused = true;
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), ScreenError> {
        let Some(active) = self.active.as_mut() else { return Err(ScreenError::NotRecording) };
        active.paused = false;
        active.due = true;
        Ok(())
    }

    /// Finish the recording and hand back its file. The caller inserts it and deletes the file;
    /// on an error nothing is left behind.
    pub fn stop(&mut self) -> Result<StoppedRecording, ScreenError> {
        let Some(active) = self.active.take() else { return Err(ScreenError::NotRecording) };
        if let Some(host) = self.host.as_mut() {
            host.stop();
        }
        let path = active.path;
        let out: RecorderOutput = match active.recorder.finish() {
            Ok(o) => o,
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                return Err(ScreenError::Failed(format!("the screen recording couldn't be written: {e}")));
            }
        };
        Ok(StoppedRecording { path, frames: out.frames, width: out.width, height: out.height, duration_ms: out.duration_ms })
    }

    /// Throw the recording away. `true` when one was running.
    pub fn cancel(&mut self) -> bool {
        let Some(active) = self.active.take() else { return false };
        if let Some(host) = self.host.as_mut() {
            host.stop();
        }
        let _ = std::fs::remove_file(&active.path);
        true
    }

    /// Pull the next frame when one is due. Returns a message when the capture failed; the
    /// recording was then cancelled and its file removed.
    pub fn tick(&mut self, now: f64) -> Option<String> {
        let Some(active) = self.active.as_mut() else { return None };
        if active.paused {
            return None;
        }
        let interval = 1.0 / f64::from(active.fps.max(1));
        if !active.due && now + 1.0e-9 < active.next_at {
            return None;
        }
        let Some(host) = self.host.as_mut() else {
            self.cancel();
            return Some(ScreenError::Unavailable.to_string());
        };
        match host.frame() {
            Ok(Some(frame)) => {
                if let Err(e) = active.recorder.push(&frame) {
                    let msg = format!("the screen recording stopped: {e}");
                    self.cancel();
                    return Some(msg);
                }
                active.frames = active.recorder.frames();
                active.due = false;
                active.next_at = now + interval;
                None
            }
            Ok(None) => None,
            Err(e) => {
                self.cancel();
                Some(e.to_string())
            }
        }
    }
}

/// A fresh file in the temporary folder for one recording.
fn temp_path() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    std::env::temp_dir().join(format!("deckcraft-screen-{}-{stamp}-{n}.mp4", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A fake screen device: frames are queued by the test and delivered one per `frame` call.
    struct FakeScreen {
        frames: Arc<Mutex<std::collections::VecDeque<deckcraft_screen::ScreenFrame>>>,
        status: CaptureStatus,
        size: (u32, u32),
    }

    impl FakeScreen {
        fn ready(size: (u32, u32)) -> (Self, Arc<Mutex<std::collections::VecDeque<deckcraft_screen::ScreenFrame>>>) {
            let frames = Arc::new(Mutex::new(std::collections::VecDeque::new()));
            (FakeScreen { frames: frames.clone(), status: CaptureStatus::Ready, size }, frames)
        }
    }

    impl ScreenIn for FakeScreen {
        fn status(&mut self) -> CaptureStatus {
            self.status
        }
        fn targets(&mut self) -> Result<Vec<CaptureTarget>, ScreenError> {
            Ok(vec![CaptureTarget {
                id: 1,
                kind: deckcraft_screen::TargetKind::Display,
                title: "Fake display".into(),
                width: self.size.0,
                height: self.size.1,
            }])
        }
        fn start(&mut self, _o: &CaptureOptions) -> Result<(), ScreenError> {
            Ok(())
        }
        fn frame(&mut self) -> Result<Option<deckcraft_screen::ScreenFrame>, ScreenError> {
            Ok(self.frames.lock().map_err(|_| ScreenError::Failed("the fake host was poisoned".into()))?.pop_front())
        }
        fn stop(&mut self) {}
    }

    fn state_with(host: FakeScreen) -> ScreenState {
        let mut s = ScreenState::default();
        s.set_host(Some(Box::new(host)));
        s
    }

    #[test]
    fn recording_pulls_frames_writes_an_mp4_and_stops() {
        let (host, queue) = FakeScreen::ready((160, 120));
        let mut s = state_with(host);
        s.start(CaptureOptions::default(), RecorderConfig::new(30)).expect("start");
        assert!(s.is_recording());
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [0, 0, 255, 255]));
        assert_eq!(s.tick(0.0), None);
        assert_eq!(s.frames(), 1);
        let stopped = s.stop().expect("stop");
        assert_eq!(stopped.frames, 1);
        assert_eq!((stopped.width, stopped.height), (160, 120));
        assert!(stopped.path.is_file());
        let _ = std::fs::remove_file(&stopped.path);
    }

    #[test]
    fn a_denied_permission_is_one_error() {
        let (mut host, _) = FakeScreen::ready((100, 100));
        host.status = CaptureStatus::PermissionDenied;
        let mut s = state_with(host);
        let e = s.start(CaptureOptions::default(), RecorderConfig::new(30)).unwrap_err();
        assert_eq!(e, ScreenError::PermissionDenied);
        assert!(!s.is_recording());
    }

    #[test]
    fn a_second_start_is_busy_and_cancel_removes_the_file() {
        let (host, _) = FakeScreen::ready((100, 100));
        let mut s = state_with(host);
        s.start(CaptureOptions::default(), RecorderConfig::new(30)).expect("start");
        assert_eq!(s.start(CaptureOptions::default(), RecorderConfig::new(30)), Err(ScreenError::Busy));
        assert!(s.cancel());
        assert!(!s.is_recording());
        assert!(!s.cancel());
    }

    #[test]
    fn no_host_is_unavailable_and_nothing_panics() {
        let mut s = ScreenState::default();
        assert_eq!(s.status(), CaptureStatus::Unsupported);
        assert_eq!(s.start(CaptureOptions::default(), RecorderConfig::new(30)), Err(ScreenError::Unavailable));
        assert_eq!(s.stop(), Err(ScreenError::NotRecording));
        assert_eq!(s.tick(1.0), None);
    }
}
