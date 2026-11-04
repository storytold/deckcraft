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
    /// Stop asked for the file to be written: the encoder finishes it while the UI keeps running.
    stopping: bool,
    /// Why the take is ending, for the message the session shows when it inserts the file.
    reason: String,
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
    /// The last job did not fit: the caller re-schedules the work instead of counting it.
    busy: bool,
    /// The picture the queue refused, for the next interval.
    kept: Option<ScreenFrame>,
    /// A finish was requested: the worker writes the file when the queue drains.
    finish: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// A discard was requested: the worker stops before the next job, and the file is not written.
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The worker's event channel closed: it ended without reporting a finished file.
    gone: bool,
}

/// The recorder on the caller's thread (the web build).
struct Inline {
    recorder: Option<ScreenRecorder>,
    frames: u64,
    failure: Option<String>,
    finished: Option<Result<RecorderOutput, RecorderError>>,
}

impl Encoder {
    /// Start the encoder for a fresh recorder.
    fn new(recorder: ScreenRecorder) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (job_tx, job_rx) = std::sync::mpsc::sync_channel::<Job>(JOB_QUEUE);
            let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
            let finish = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let worker_finish = finish.clone();
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let worker_cancel = cancel.clone();
            let spawned = std::thread::Builder::new()
                .name("deckcraft-encode".into())
                .spawn(move || encode_loop(recorder, &job_rx, &event_tx, &worker_finish, &worker_cancel));
            match spawned {
                Ok(handle) => {
                    return Encoder::Worker(Worker {
                        jobs: Some(job_tx),
                        events: event_rx,
                        handle: Some(handle),
                        frames: 0,
                        failure: None,
                        finished: None,
                        busy: false,
                        kept: None,
                        finish,
                        cancel,
                        gone: false,
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
            Encoder::Inline(Inline { recorder: None, frames: 0, failure: Some("couldn't start the encoder".into()), finished: None })
        }
        #[cfg(target_arch = "wasm32")]
        {
            Encoder::Inline(Inline { recorder: Some(recorder), frames: 0, failure: None, finished: None })
        }
    }

    /// Queue a picture; `false` when the queue is full and the caller must keep it.
    fn encode(&mut self, frame: ScreenFrame) -> bool {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Encoder::Worker(w) => {
                let Some(jobs) = w.jobs.as_ref() else { return false };
                match jobs.try_send(Job::Encode(frame)) {
                    Ok(()) => {
                        w.frames += 1;
                        true
                    }
                    // A full queue is a busy encoder, not a picture: the caller keeps the frame for
                    // the next interval, and the count only moves for pictures the muxer gets.
                    Err(TrySendError::Full(Job::Encode(frame))) => {
                        w.busy = true;
                        w.kept = Some(frame);
                        false
                    }
                    Err(_) => false,
                }
            }
            Encoder::Inline(i) => {
                let Some(recorder) = i.recorder.as_mut() else { return false };
                if let Err(e) = recorder.push(&frame) {
                    i.failure = Some(e.to_string());
                }
                i.frames = recorder.frames();
                true
            }
        }
    }

    /// Queue `n` repeats of the last picture (one burst, so a delayed tick stays one job);
    /// `false` when the queue is full.
    fn repeat(&mut self, n: u32) -> bool {
        if n == 0 {
            return false;
        }
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Encoder::Worker(w) => {
                let Some(jobs) = w.jobs.as_ref() else { return false };
                match jobs.try_send(Job::Repeat(n)) {
                    Ok(()) => {
                        w.frames += u64::from(n);
                        true
                    }
                    Err(TrySendError::Full(_)) => {
                        w.busy = true;
                        false
                    }
                    Err(_) => false,
                }
            }
            Encoder::Inline(i) => {
                let Some(recorder) = i.recorder.as_mut() else { return false };
                for _ in 0..n {
                    // Nothing to repeat yet, or a key frame is due: the take waits for the next
                    // captured picture instead of writing a sample that does not exist.
                    if recorder.push_repeat().is_err() {
                        break;
                    }
                }
                i.frames = recorder.frames();
                true
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
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        // The worker ended: either it finished (the event is already queued and
                        // taken above) or it died without writing the file. Remember it so the
                        // take does not wait for a report that will never come.
                        w.gone = true;
                        break;
                    }
                }
            }
        }
    }

    /// The picture the queue refused, if any.
    fn take_kept(&mut self) -> Option<ScreenFrame> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Encoder::Worker(w) => w.kept.take(),
            Encoder::Inline(_) => None,
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

    /// Ask the encoder to write the file and stop, without waiting: the worker drains its queue,
    /// writes the file and reports the result through [`poll`](Self::poll), which the UI calls once
    /// a frame. Stop must not block the UI on encoding and muxing.
    fn request_finish(&mut self) {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Encoder::Worker(w) => {
                w.finish.store(true, Ordering::Relaxed);
                // Dropping the sender lets the worker drain what is queued and then finish.
                w.jobs = None;
            }
            Encoder::Inline(i) => {
                let out = match i.recorder.take() {
                    Some(recorder) => recorder.finish(),
                    None => Err(RecorderError::Empty),
                };
                i.finished = Some(out);
            }
        }
    }

    /// The result of a requested finish, once (the worker reports it through `poll`).
    fn take_finished(&mut self) -> Option<Result<RecorderOutput, RecorderError>> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Encoder::Worker(w) => {
                // A worker that ended without `Finished` (a panic, or an exit before it wrote the
                // file) leaves the take stopping forever: report the failure so `tick` cleans up.
                if w.gone && w.finished.is_none() {
                    let why = w.failure.clone().unwrap_or_else(|| "the encoder stopped".into());
                    w.finished = Some(Err(RecorderError::Muxer(why)));
                }
                if w.finished.is_some()
                    && let Some(handle) = w.handle.take()
                {
                    let _ = handle.join();
                }
                w.finished.take()
            }
            Encoder::Inline(i) => i.finished.take(),
        }
    }

    /// Write the file and hand back what it holds, waiting for the worker. Only the terminal paths
    /// that cannot defer (the web build's inline encoder) use this; the UI calls
    /// [`request_finish`](Self::request_finish) and polls instead.
    #[allow(dead_code)]
    fn finish(&mut self) -> Result<RecorderOutput, RecorderError> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Encoder::Worker(w) => {
                if let Some(jobs) = w.jobs.take() {
                    let _ = jobs.send(Job::Finish);
                }
                // The worker reports the finished file (or its failure) and returns. A closed
                // channel means it is gone: nothing more will arrive, so stop waiting.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
                while w.finished.is_none() && std::time::Instant::now() < deadline {
                    match w.events.recv_timeout(std::time::Duration::from_millis(50)) {
                        Ok(Event::Finished(out)) => w.finished = Some(out),
                        Ok(Event::Failed(e)) => w.failure = Some(e),
                        Ok(Event::Encoded) => {}
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
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
                // Tell the worker to stop before dropping the sender: it checks the flag before
                // each queued job, so the join below does not wait for a backlog to drain.
                w.cancel.store(true, Ordering::Relaxed);
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
/// A set `cancel` stops it at the next job boundary without writing the file, so a discard's join
/// is short even with a full queue.
#[cfg(not(target_arch = "wasm32"))]
fn encode_loop(
    recorder: ScreenRecorder,
    jobs: &Receiver<Job>,
    events: &std::sync::mpsc::Sender<Event>,
    finish: &std::sync::atomic::AtomicBool,
    cancel: &std::sync::atomic::AtomicBool,
) {
    let mut recorder = Some(recorder);
    let mut last: Option<ScreenFrame> = None;
    while !cancel.load(Ordering::Relaxed)
        && let Ok(job) = jobs.recv()
    {
        // The job may have waited behind a cancel; drop it rather than encode it.
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let Some(rec) = recorder.as_mut() else { return };
        match job {
            Job::Encode(frame) => match rec.push(&frame) {
                Ok(()) => last = Some(frame),
                Err(e) => {
                    finish_after_failure(&mut recorder, events, e.to_string());
                    return;
                }
            },
            Job::Repeat(n) => {
                for _ in 0..n {
                    // A discard stops mid-burst: the remaining intervals are not encoded.
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    // A key frame that is due (or nothing encoded yet) is encoded from the picture
                    // the worker already holds, so a repeat never loses the schedule.
                    let r = rec.push_repeat().or_else(|_| match last.as_ref() {
                        Some(frame) => rec.push(frame),
                        None => Ok(()),
                    });
                    if let Err(e) = r {
                        finish_after_failure(&mut recorder, events, e.to_string());
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
    // The queue drained: a requested finish writes the file, a discard leaves it unwritten (the
    // recorder is dropped with the thread, which closes the file).
    if !cancel.load(Ordering::Relaxed)
        && finish.load(Ordering::Relaxed)
        && let Some(rec) = recorder.take()
    {
        let _ = events.send(Event::Finished(rec.finish()));
    }
}

/// The encoder failed mid-take: write the file with what it holds (the caller keeps and inserts
/// it), report the reason and the file in one go, so nothing waits on a channel nobody answers.
#[cfg(not(target_arch = "wasm32"))]
fn finish_after_failure(recorder: &mut Option<ScreenRecorder>, events: &std::sync::mpsc::Sender<Event>, why: String) {
    let out = recorder.take().map(ScreenRecorder::finish).unwrap_or(Err(RecorderError::Empty));
    let _ = events.send(Event::Failed(why));
    let _ = events.send(Event::Finished(out));
}

/// The session's screen capture and recording state.
#[derive(Default)]
pub struct ScreenState {
    host: Option<Box<dyn ScreenIn>>,
    active: Option<Active>,
    /// A take a capture or encoder failure finished with frames in it, waiting for the session
    /// to insert it.
    ended: Option<StoppedRecording>,
    /// Why the ended take stopped (empty for an ordinary Stop).
    ended_reason: Option<String>,
}

impl Drop for ScreenState {
    fn drop(&mut self) {
        // A session dropped while a take records (an automation quit, a closed window) would leave
        // the temporary MP4 — captured screen content, without a `moov` box — behind: throw the
        // take away, which stops the host, drops the encoder and removes the file.
        let _ = self.cancel();
    }
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
        self.active.as_ref().is_some_and(|a| !a.stopping)
    }

    /// Whether a take is being written (Stop or a failure asked for the file).
    pub fn is_finishing(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.stopping)
    }

    pub fn is_paused(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.paused && !a.stopping)
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
            stopping: false,
            reason: String::new(),
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
    /// Stop the take. This only asks the encoder to write the file: the worker drains its queue and
    /// writes the MP4 while the UI keeps running, and the finished take is handed over by
    /// [`take_ended`](Self::take_ended) on a later tick, so Stop never blocks on encoding.
    pub fn stop(&mut self) -> Result<(), ScreenError> {
        let Some(active) = self.active.as_mut() else { return Err(ScreenError::NotRecording) };
        if active.stopping {
            return Err(ScreenError::Busy);
        }
        if let Some(host) = self.host.as_mut() {
            host.stop();
        }
        active.stopping = true;
        active.reason.clear();
        active.encoder.request_finish();
        Ok(())
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
        if active.stopping {
            // Stop already asked for the file: take it when the encoder reports it, and never
            // re-enter the capture path.
            let done = active.encoder.take_finished()?;
            let path = active.path.clone();
            let reason = active.reason.clone();
            self.active = None;
            return match done {
                Ok(out) if out.frames > 0 => {
                    self.ended =
                        Some(StoppedRecording { path, frames: out.frames, width: out.width, height: out.height, duration_ms: out.duration_ms });
                    self.ended_reason = Some(reason);
                    None
                }
                Ok(_) => {
                    let _ = std::fs::remove_file(&path);
                    Some("the screen recording had nothing in it".into())
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&path);
                    Some(format!("the screen recording couldn't be written: {e}"))
                }
            };
        }
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
            // The start waits for the first picture: keeping `due` set means a late first frame
            // (the capturer may still be building) starts the schedule rather than becoming
            // hundreds of owed repeats of itself.
            let scheduled = if active.pending.is_some() || active.frames > 0 {
                let (n, e) = schedule_pictures(active, 1);
                why = e;
                n
            } else {
                0
            };
            if scheduled > 0 {
                active.due = false;
                active.next_at = now + interval;
            }
        } else if now + 1.0e-9 >= active.next_at {
            // Every interval that passed is owed one picture; one job carries them all.
            let owed = ((now - active.next_at) / interval).floor() as u32 + 1;
            let pictures = owed.clamp(1, MAX_CATCH_UP);
            let mut scheduled = 0;
            if active.pending.is_some() || active.frames > 0 {
                let (n, e) = schedule_pictures(active, pictures);
                scheduled = n;
                why = e;
            }
            // Only the pictures the queue took move the schedule: the rest stay owed and are tried
            // again on the next tick.
            active.next_at += f64::from(scheduled) * interval;
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

    /// The take a stop or a capture failure finished, with why it ended.
    pub fn take_ended_with_reason(&mut self) -> Option<(StoppedRecording, String)> {
        let rec = self.ended.take()?;
        Some((rec, self.ended_reason.take().unwrap_or_default()))
    }

    /// End the take the way a source failure should: stop the host, write the MP4 and keep it.
    /// Nothing recorded means nothing to keep, so the file is removed.
    fn end_keeping(&mut self, why: &str) -> String {
        let Some(active) = self.active.as_mut() else { return why.to_string() };
        if let Some(host) = self.host.as_mut() {
            host.stop();
        }
        // The same non-blocking finish a Stop uses: the tick that follows hands the file over
        // once the encoder has written it, with this reason attached.
        active.stopping = true;
        active.reason = why.to_string();
        active.encoder.request_finish();
        why.to_string()
    }
}

/// Schedule one picture: the newest capture if one is waiting, else a repeat of the last. `Some`
/// is the encoder error when the picture could not be queued.
fn schedule_pictures(active: &mut Active, pictures: u32) -> (u32, Option<String>) {
    if pictures == 0 {
        return (0, None);
    }
    // How many of the owed pictures the queue took: the caller only advances the schedule for
    // those, so a busy encoder catches up instead of shortening the take.
    let mut scheduled = 0;
    match active.pending.take() {
        Some(frame) => {
            // A refused picture goes back to the front of the queue and is retried next interval.
            if !active.encoder.encode(frame) {
                active.pending = active.encoder.take_kept();
                return (0, active.encoder.failure().map(str::to_string));
            }
            scheduled = 1;
            if pictures > 1 && active.encoder.repeat(pictures - 1) {
                scheduled += pictures - 1;
            }
        }
        None => {
            if active.encoder.repeat(pictures) {
                scheduled = pictures;
            }
        }
    }
    (scheduled, active.encoder.failure().map(str::to_string))
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

    /// Run ticks until the encoder hands the finished take over (bounded), as the UI does.
    fn pump(s: &mut ScreenState) -> StoppedRecording {
        for _ in 0..500 {
            let _ = s.tick(0.0);
            if let Some(rec) = s.take_ended() {
                return rec;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("the take never arrived");
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
        s.stop().expect("stop");
        assert!(!s.is_recording(), "a stopping take is no longer recording");
        let stopped = pump(&mut s);
        assert_eq!(stopped.frames, 1);
        assert_eq!((stopped.width, stopped.height), (160, 120));
        assert!(stopped.path.is_file());
        let _ = std::fs::remove_file(&stopped.path);
    }

    /// Stop must not wait for the encoder: it answers at once, the file arrives on a later tick,
    /// and the session inserts it then.
    #[test]
    fn stop_returns_before_the_encoder_drains_and_the_take_arrives_on_a_tick() {
        let (host, queue) = FakeScreen::ready((160, 120));
        let mut s = state_with(host);
        s.start(CaptureOptions::default(), RecorderConfig::new(30)).expect("start");
        for _ in 0..8 {
            queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [4, 8, 12, 255]));
        }
        assert_eq!(s.tick(0.0), None);
        let started = std::time::Instant::now();
        s.stop().expect("stop");
        assert!(started.elapsed() < std::time::Duration::from_millis(50), "Stop waited on the encoder: {:?}", started.elapsed());
        assert!(s.take_ended().is_none(), "the file is not ready at once");
        let stopped = pump(&mut s);
        assert!(stopped.frames >= 1, "the queued pictures are written by the worker");
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

    /// A start waits for the capturer's first picture (a picker can delay it for minutes): the
    /// ticks before it must not turn into owed intervals that replay the first frame.
    #[test]
    fn waiting_for_the_first_frame_does_not_owe_pictures() {
        let (host, queue) = FakeScreen::ready((160, 120));
        let mut s = state_with(host);
        s.start(CaptureOptions::default(), RecorderConfig::new(30)).expect("start");
        for i in 0..10 {
            assert_eq!(s.tick(f64::from(i) * 0.1), None);
            assert_eq!(s.frames(), 0, "no picture has arrived yet");
        }
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [7, 7, 7, 255]));
        assert_eq!(s.tick(1.0), None);
        assert_eq!(s.frames(), 1, "the first picture is not replayed for the waiting time");
        s.stop().expect("stop");
        let stopped = pump(&mut s);
        let _ = std::fs::remove_file(&stopped.path);
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
        s.stop().expect("stop");
        let stopped = pump(&mut s);
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
        let kept = pump(&mut s);
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

    /// A worker that ends without a `Finished` event must not leave the take stopping forever:
    /// the tick reports the failure and the unfinished file is removed.
    #[test]
    fn a_worker_that_disappears_without_finishing_fails_the_take() {
        let (host, queue) = FakeScreen::ready((160, 120));
        let mut s = state_with(host);
        s.start(CaptureOptions::default(), RecorderConfig::new(30)).expect("start");
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [1, 2, 3, 255]));
        assert_eq!(s.tick(0.0), None);
        let path = s.active.as_ref().expect("an active take").path.clone();
        {
            // Take the worker apart the way a panic does: it ends without a finish request, so it
            // never sends `Finished` and the event channel closes.
            let active = s.active.as_mut().expect("an active take");
            let Encoder::Worker(w) = &mut active.encoder else { panic!("the test runs with an encoder thread") };
            w.jobs = None;
            if let Some(handle) = w.handle.take() {
                handle.join().expect("the worker exits");
            }
            assert!(w.finished.is_none(), "the worker wrote no file");
        }
        // Stop asks for a file nobody will write; the next tick must answer instead of waiting.
        s.stop().expect("stop");
        assert!(s.is_finishing());
        let why = s.tick(0.0).expect("the missing encoder is reported");
        assert!(why.contains("couldn't be written"), "{why}");
        assert!(!s.is_recording() && !s.is_finishing(), "the take is over");
        assert!(s.take_ended().is_none(), "there is no file to insert");
        assert!(!path.exists(), "the unfinished file is removed");
    }

    /// A discard must not sit in the join while the worker works through its queued backlog.
    #[test]
    fn cancel_is_prompt_with_a_queued_backlog() {
        let (host, queue) = FakeScreen::ready((320, 240));
        let mut s = state_with(host);
        s.start(CaptureOptions::default(), RecorderConfig::new(30)).expect("start");
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(320, 240, [9, 9, 9, 255]));
        assert_eq!(s.tick(0.0), None);
        let mut queued = 0;
        {
            let active = s.active.as_mut().expect("an active take");
            let Encoder::Worker(w) = &mut active.encoder else { panic!("the test runs with an encoder thread") };
            let jobs = w.jobs.as_ref().expect("the queue");
            // Whole catch-up bursts: draining them all would keep the join busy for a while.
            for _ in 0..JOB_QUEUE {
                if jobs.try_send(Job::Repeat(MAX_CATCH_UP)).is_err() {
                    break;
                }
                queued += 1;
            }
        }
        assert!(queued >= JOB_QUEUE - 1, "the queue holds a backlog, got {queued} jobs");
        let path = s.active.as_ref().expect("an active take").path.clone();
        let started = std::time::Instant::now();
        assert!(s.cancel(), "a take was running");
        let elapsed = started.elapsed();
        assert!(elapsed < std::time::Duration::from_secs(1), "cancel waited on the queued jobs: {elapsed:?}");
        assert!(!path.exists(), "a cancelled take leaves no file");
    }
}
