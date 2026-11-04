//! Screen recording: the session state that drives a host's capture and the MP4 recorder.
//!
//! The host (the desktop app) installs a [`ScreenIn`]; the UI calls [`ScreenState::tick`] once a
//! frame while a recording runs, which pulls captured frames and feeds them to the recorder. A
//! capture or encoder failure ends the take without a panic: frames already recorded are kept and
//! finished ([`ScreenState::take_ended`]), and only a take with nothing in it is thrown away.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError};

use deckcraft_screen::{
    CaptureOptions, CaptureStatus, CaptureTarget, RecorderConfig, RecorderError, RecorderOutput, ScreenError, ScreenFrame, ScreenIn, ScreenRecorder,
};

/// The most frames one delayed tick may catch up with, so a stall cannot encode a burst forever.
const MAX_CATCH_UP: u32 = 240;

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
    encoder: Encoder,
    path: PathBuf,
    fps: u32,
    paused: bool,
    /// A frame is wanted now (the start, or the frame after a resume).
    due: bool,
    /// Earliest time (seconds, UI clock) for the next frame.
    next_at: f64,
    /// Pictures the UI scheduled (the recorder's own count settles when the take finishes).
    frames: u64,
    /// The newest captured picture waiting for its slot; a later capture replaces it.
    pending: Option<ScreenFrame>,
}

/// One scheduled picture for the encoder thread.
#[cfg(not(target_arch = "wasm32"))]
enum Job {
    /// Encode this picture (the first one, or one that changed).
    Encode(ScreenFrame),
    /// Mux the last sample again for `n` intervals (the picture did not change). One job carries a
    /// whole catch-up burst, so a delayed tick cannot flood the queue.
    Repeat(u32),
    /// Write the file and hand the output back.
    Finish,
}

/// What the encoder thread reports.
#[cfg(not(target_arch = "wasm32"))]
enum Event {
    Encoded,
    Failed(String),
    Finished(Result<RecorderOutput, RecorderError>),
}

/// How many jobs may wait for the encoder before the UI drops a repeat.
#[cfg(not(target_arch = "wasm32"))]
const JOB_QUEUE: usize = 4;

/// The recorder. Where the target has threads it runs on one, so the UI thread never encodes; on
/// the web, threads are unavailable and it runs inline.
enum Encoder {
    #[cfg(not(target_arch = "wasm32"))]
    Worker(Worker),
    Inline(Inline),
}

#[cfg(not(target_arch = "wasm32"))]
struct Worker {
    jobs: Option<SyncSender<Job>>,
    events: Receiver<Event>,
    handle: Option<std::thread::JoinHandle<()>>,
    frames: u64,
    failure: Option<String>,
    finished: Option<Result<RecorderOutput, RecorderError>>,
}

/// The recorder on the caller's thread (the web build).
struct Inline {
    recorder: Option<ScreenRecorder>,
    frames: u64,
    failure: Option<String>,
}

impl Encoder {
    /// Start the encoder for a fresh recorder.
    fn new(recorder: ScreenRecorder) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (job_tx, job_rx) = std::sync::mpsc::sync_channel::<Job>(JOB_QUEUE);
            let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
            let spawned = std::thread::Builder::new().name("deckcraft-encode".into()).spawn(move || encode_loop(recorder, &job_rx, &event_tx));
            match spawned {
                Ok(handle) => {
                    return Encoder::Worker(Worker {
                        jobs: Some(job_tx),
                        events: event_rx,
                        handle: Some(handle),
                        frames: 0,
                        failure: None,
                        finished: None,
                    });
                }
                // No thread to be had: fall back to encoding here rather than losing the take.
                Err(e) => {
                    log::warn!("screen recording: no encoder thread ({e}); recording inline");
                    let _ = job_rx;
                }
            }
            // The recorder moved into the closure only when the thread started; on failure the
            // closure was never called, so it is gone. That path cannot happen with the handle
            // above unless the spawn failed, which drops it — recoverable only by a new recorder.
            Encoder::Inline(Inline { recorder: None, frames: 0, failure: Some("couldn't start the encoder".into()) })
        }
        #[cfg(target_arch = "wasm32")]
        {
            Encoder::Inline(Inline { recorder: Some(recorder), frames: 0, failure: None })
        }
    }

    /// Queue a picture. A full queue drops it: encoding slower than the schedule is a reason to
    /// record fewer pictures, never to grow memory.
    fn encode(&mut self, frame: ScreenFrame) {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Encoder::Worker(w) => {
                if let Some(jobs) = w.jobs.as_ref()
                    && matches!(jobs.try_send(Job::Encode(frame)), Ok(()) | Err(TrySendError::Full(_)))
                {
                    w.frames += 1;
                }
            }
            Encoder::Inline(i) => {
                let Some(recorder) = i.recorder.as_mut() else { return };
                if let Err(e) = recorder.push(&frame) {
                    i.failure = Some(e.to_string());
                }
                i.frames = recorder.frames();
            }
        }
    }

    /// Queue `n` repeats of the last picture (one burst, so a delayed tick stays one job).
    fn repeat(&mut self, n: u32) {
        if n == 0 {
            return;
        }
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Encoder::Worker(w) => {
                if let Some(jobs) = w.jobs.as_ref()
                    && matches!(jobs.try_send(Job::Repeat(n)), Ok(()) | Err(TrySendError::Full(_)))
                {
                    w.frames += u64::from(n);
                }
            }
            Encoder::Inline(i) => {
                let Some(recorder) = i.recorder.as_mut() else { return };
                for _ in 0..n {
                    // Nothing to repeat yet, or a key frame is due: the take waits for the next
                    // captured picture instead of writing a sample that does not exist.
                    if recorder.push_repeat().is_err() {
                        break;
                    }
                }
                i.frames = recorder.frames();
            }
        }
    }

    /// Drain what the worker reported.
    fn poll(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Encoder::Worker(w) = self {
            loop {
                match w.events.try_recv() {
                    Ok(Event::Encoded) => {}
                    Ok(Event::Failed(e)) => w.failure = Some(e),
                    Ok(Event::Finished(out)) => w.finished = Some(out),
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                }
            }
        }
    }

    fn frames(&self) -> u64 {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Encoder::Worker(w) => w.frames,
            Encoder::Inline(i) => i.frames,
        }
    }

    fn failure(&self) -> Option<&str> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Encoder::Worker(w) => w.failure.as_deref(),
            Encoder::Inline(i) => i.failure.as_deref(),
        }
    }

    /// Write the file and hand back what it holds. The worker is joined here, so the file is
    /// complete when this returns.
    fn finish(&mut self) -> Result<RecorderOutput, RecorderError> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Encoder::Worker(w) => {
                if let Some(jobs) = w.jobs.take() {
                    let _ = jobs.send(Job::Finish);
                }
                // The worker reports the finished file (or its failure) and returns.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
                while w.finished.is_none() && std::time::Instant::now() < deadline {
                    match w.events.recv_timeout(std::time::Duration::from_millis(50)) {
                        Ok(Event::Finished(out)) => w.finished = Some(out),
                        Ok(Event::Failed(e)) => w.failure = Some(e),
                        Ok(Event::Encoded) => {}
                        Err(_) => {}
                    }
                }
                if let Some(handle) = w.handle.take() {
                    let _ = handle.join();
                }
                match w.finished.take() {
                    Some(out) => out,
                    None => Err(RecorderError::Muxer(w.failure.clone().unwrap_or_else(|| "the recording didn't finish".into()))),
                }
            }
            Encoder::Inline(i) => match i.recorder.take() {
                Some(recorder) => recorder.finish(),
                None => Err(RecorderError::Empty),
            },
        }
    }

    /// Throw the take away: the worker stops without writing the file.
    fn discard(&mut self) {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Encoder::Worker(w) => {
                w.jobs = None;
                if let Some(handle) = w.handle.take() {
                    let _ = handle.join();
                }
            }
            Encoder::Inline(i) => i.recorder = None,
        }
    }
}

/// The encoder thread: encode or repeat each scheduled picture, then write the file on `Finish`.
#[cfg(not(target_arch = "wasm32"))]
fn encode_loop(recorder: ScreenRecorder, jobs: &Receiver<Job>, events: &std::sync::mpsc::Sender<Event>) {
    let mut recorder = Some(recorder);
    let mut last: Option<ScreenFrame> = None;
    while let Ok(job) = jobs.recv() {
        let Some(rec) = recorder.as_mut() else { return };
        match job {
            Job::Encode(frame) => match rec.push(&frame) {
                Ok(()) => last = Some(frame),
                Err(e) => {
                    let _ = events.send(Event::Failed(e.to_string()));
                    return;
                }
            },
            Job::Repeat(n) => {
                for _ in 0..n {
                    // A key frame that is due (or nothing encoded yet) is encoded from the picture
                    // the worker already holds, so a repeat never loses the schedule.
                    let r = rec.push_repeat().or_else(|_| match last.as_ref() {
                        Some(frame) => rec.push(frame),
                        None => Ok(()),
                    });
                    if let Err(e) = r {
                        let _ = events.send(Event::Failed(e.to_string()));
                        return;
                    }
                }
            }
            Job::Finish => {
                let out = recorder.take().map(ScreenRecorder::finish).unwrap_or(Err(RecorderError::Empty));
                let _ = events.send(Event::Finished(out));
                return;
            }
        }
        let _ = events.send(Event::Encoded);
    }
}

/// The session's screen capture and recording state.
#[derive(Default)]
pub struct ScreenState {
    host: Option<Box<dyn ScreenIn>>,
    active: Option<Active>,
    /// A take a capture or encoder failure finished with frames in it, waiting for the session
    /// to insert it.
    ended: Option<StoppedRecording>,
}

impl ScreenState {
    /// Install the host device; `None` makes capture unavailable (headless tests, wasm).
    pub fn set_host(&mut self, host: Option<Box<dyn ScreenIn>>) {
        if let Some(mut active) = self.active.take() {
            if let Some(host) = self.host.as_mut() {
                host.stop();
            }
            active.encoder.discard();
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
        // `create_new` refuses a path that already exists (a pre-planted link in the shared temp
        // folder would otherwise be truncated and written through), and the take is created
        // owner-only on Unix so other local users cannot read captured screen content.
        let mut file_options = std::fs::OpenOptions::new();
        file_options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut file_options, 0o600);
        let file = file_options.open(&path).map_err(|e| ScreenError::Failed(format!("couldn't start a screen recording: {e}")))?;
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
        self.active = Some(Active {
            encoder: Encoder::new(recorder),
            path,
            fps: options.fps.clamp(1, 240),
            paused: false,
            due: true,
            next_at: 0.0,
            frames: 0,
            pending: None,
        });
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
        let Some(mut active) = self.active.take() else { return Err(ScreenError::NotRecording) };
        if let Some(host) = self.host.as_mut() {
            host.stop();
        }
        let path = active.path;
        let out: RecorderOutput = match active.encoder.finish() {
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
        let Some(mut active) = self.active.take() else { return false };
        if let Some(host) = self.host.as_mut() {
            host.stop();
        }
        active.encoder.discard();
        let _ = std::fs::remove_file(&active.path);
        true
    }

    /// Pull a captured frame and schedule every picture the schedule is owed. Returns a message
    /// when the capture or the encoder failed; the take then keeps what it recorded
    /// ([`take_ended`](Self::take_ended)) unless it had no frames.
    pub fn tick(&mut self, now: f64) -> Option<String> {
        let active = self.active.as_mut()?;
        active.encoder.poll();
        if let Some(f) = active.encoder.failure().map(str::to_string) {
            let why = format!("the screen recording stopped: {f}");
            return if active.frames > 0 {
                Some(self.end_keeping(&why))
            } else {
                self.cancel();
                Some(why)
            };
        }
        if active.paused {
            // Nothing is encoded while the take is held, but the newest picture is kept: a backend
            // that only sends frames when the screen changes would otherwise resume on the
            // pre-pause content for the whole static period.
            if let Some(Ok(Some(frame))) = self.host.as_mut().map(|h| h.frame()) {
                active.pending = Some(frame);
            }
            return None;
        }
        let interval = 1.0 / f64::from(active.fps.max(1));
        match self.host.as_mut().map(|h| h.frame()) {
            Some(Ok(Some(frame))) => active.pending = Some(frame),
            Some(Ok(None)) => {}
            Some(Err(e)) => {
                let why = e.to_string();
                return if active.frames > 0 {
                    Some(self.end_keeping(&why))
                } else {
                    self.cancel();
                    Some(why)
                };
            }
            None => {
                let why = ScreenError::Unavailable.to_string();
                return if active.frames > 0 {
                    Some(self.end_keeping(&why))
                } else {
                    self.cancel();
                    Some(why)
                };
            }
        }
        let mut why = None;
        if active.due {
            active.due = false;
            if active.pending.is_some() || active.frames > 0 {
                why = schedule_picture(active, 1);
            }
            active.next_at = now + interval;
        } else if now + 1.0e-9 >= active.next_at {
            // Every interval that passed is owed one picture; one job carries them all.
            let owed = ((now - active.next_at) / interval).floor() as u32 + 1;
            let pictures = owed.clamp(1, MAX_CATCH_UP);
            if active.pending.is_some() || active.frames > 0 {
                why = schedule_picture(active, pictures);
            }
            active.next_at += f64::from(pictures) * interval;
            if owed > MAX_CATCH_UP {
                // The backlog past the cap is dropped, not split into bursts: a one minute stall
                // must not keep the encoder busy for minutes afterwards.
                active.next_at = now + interval;
            }
        }
        active.encoder.poll();
        if why.is_none()
            && let Some(e) = active.encoder.failure().map(str::to_string)
        {
            why = Some(format!("the screen recording stopped: {e}"));
        }
        active.frames = active.frames.max(active.encoder.frames());
        let recorded = active.frames > 0;
        match why {
            Some(w) if recorded => Some(self.end_keeping(&w)),
            Some(w) => {
                self.cancel();
                Some(w)
            }
            None => None,
        }
    }

    /// The take a failed capture finished, for the caller to insert like `show.screenRecord
    /// stop` does. Its file is the caller's to delete once the bytes are in the presentation.
    pub fn take_ended(&mut self) -> Option<StoppedRecording> {
        self.ended.take()
    }

    /// End the take the way a source failure should: stop the host, write the MP4 and keep it.
    /// Nothing recorded means nothing to keep, so the file is removed.
    fn end_keeping(&mut self, why: &str) -> String {
        let Some(mut active) = self.active.take() else { return why.to_string() };
        if let Some(host) = self.host.as_mut() {
            host.stop();
        }
        match active.encoder.finish() {
            Ok(out) if out.frames > 0 => {
                self.ended = Some(StoppedRecording {
                    path: active.path,
                    frames: out.frames,
                    width: out.width,
                    height: out.height,
                    duration_ms: out.duration_ms,
                });
                why.to_string()
            }
            Ok(_) | Err(_) => {
                let _ = std::fs::remove_file(&active.path);
                why.to_string()
            }
        }
    }
}

/// Schedule one picture: the newest capture if one is waiting, else a repeat of the last. `Some`
/// is the encoder error when the picture could not be queued.
fn schedule_picture(active: &mut Active, pictures: u32) -> Option<String> {
    if pictures == 0 {
        return None;
    }
    match active.pending.take() {
        Some(frame) => {
            active.encoder.encode(frame);
            active.encoder.repeat(pictures - 1);
        }
        None => active.encoder.repeat(pictures),
    }
    active.encoder.failure().map(str::to_string)
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
        /// Set when the capture source disappears (a closed window, an unplugged display).
        fail: Arc<std::sync::atomic::AtomicBool>,
        status: CaptureStatus,
        size: (u32, u32),
    }

    impl FakeScreen {
        fn ready(size: (u32, u32)) -> (Self, Arc<Mutex<std::collections::VecDeque<deckcraft_screen::ScreenFrame>>>) {
            let frames = Arc::new(Mutex::new(std::collections::VecDeque::new()));
            let fail = Arc::new(std::sync::atomic::AtomicBool::new(false));
            (FakeScreen { frames: frames.clone(), fail, status: CaptureStatus::Ready, size }, frames)
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
            if self.fail.load(Ordering::Relaxed) {
                return Err(ScreenError::Failed("the capture source is gone".into()));
            }
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

    #[test]
    fn a_static_screen_records_every_interval_and_a_delayed_tick_catches_up() {
        let (host, queue) = FakeScreen::ready((160, 120));
        let mut s = state_with(host);
        s.start(CaptureOptions::default(), RecorderConfig::new(30)).expect("start");
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [0, 0, 255, 255]));
        assert_eq!(s.tick(0.0), None);
        assert_eq!(s.frames(), 1);
        // The device has nothing new, but the schedule is owed one picture per interval.
        assert_eq!(s.tick(0.04), None);
        assert_eq!(s.frames(), 2, "a static screen keeps recording");
        // A UI tick that arrives a second late encodes the intervals it missed.
        assert_eq!(s.tick(1.0), None);
        assert!(s.frames() >= 30 && s.frames() <= 32, "one second at 30 fps is about 30 frames, got {}", s.frames());
        // A stall longer than the catch-up cap drops the rest of the backlog instead of bursting
        // it on every later tick.
        assert_eq!(s.tick(30.0), None);
        let after_stall = s.frames();
        assert!(after_stall - 31 <= u64::from(MAX_CATCH_UP), "one tick is capped");
        assert_eq!(s.tick(30.0 + 1.0 / 30.0), None);
        assert!(s.frames() - after_stall <= 2, "the next interval is one frame again, got {}", s.frames() - after_stall);
        let stopped = s.stop().expect("stop");
        assert!(stopped.frames >= 30);
        let _ = std::fs::remove_file(&stopped.path);
    }

    #[test]
    fn a_capture_that_disappears_keeps_the_frames_it_recorded() {
        let (host, queue) = FakeScreen::ready((160, 120));
        let fail = host.fail.clone();
        let mut s = state_with(host);
        s.start(CaptureOptions::default(), RecorderConfig::new(30)).expect("start");
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [0, 0, 255, 255]));
        assert_eq!(s.tick(0.0), None);
        assert_eq!(s.frames(), 1);
        fail.store(true, Ordering::Relaxed);
        let why = s.tick(1.0).expect("the failure is reported");
        assert!(why.contains("the capture source is gone"), "{why}");
        assert!(!s.is_recording());
        let kept = s.take_ended().expect("the recorded frames are kept");
        assert_eq!((kept.frames, kept.width, kept.height), (1, 160, 120));
        assert!(kept.path.is_file());
        assert_eq!(s.take_ended(), None, "the take is handed over once");
        let _ = std::fs::remove_file(&kept.path);
    }

    #[test]
    fn a_capture_that_disappears_before_a_frame_keeps_nothing() {
        let (host, _queue) = FakeScreen::ready((160, 120));
        let fail = host.fail.clone();
        let mut s = state_with(host);
        s.start(CaptureOptions::default(), RecorderConfig::new(30)).expect("start");
        fail.store(true, Ordering::Relaxed);
        assert!(s.tick(0.0).is_some(), "the failure is reported");
        assert!(!s.is_recording());
        assert!(s.take_ended().is_none(), "nothing was recorded, so no file is kept");
    }

    #[test]
    fn a_paused_take_drops_the_frames_captured_while_it_was_held() {
        let (host, queue) = FakeScreen::ready((160, 120));
        let mut s = state_with(host);
        s.start(CaptureOptions::default(), RecorderConfig::new(30)).expect("start");
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [0, 0, 255, 255]));
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [0, 255, 0, 255]));
        assert_eq!(s.tick(0.0), None);
        assert_eq!(s.frames(), 1);
        s.pause().expect("pause");
        assert_eq!(s.tick(1.0), None);
        assert_eq!(s.frames(), 1, "a held take records nothing");
        assert!(queue.lock().expect("queue").is_empty(), "the paused frame is drained, not queued");
        s.resume().expect("resume");
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [255, 0, 0, 255]));
        assert_eq!(s.tick(2.0), None);
        assert_eq!(s.frames(), 2);
        let _ = s.cancel();
    }
}
