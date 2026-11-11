//! Screen recording: the session state that drives a host's capture and the MP4 recorder.
//!
//! The host (the desktop app) installs a [`ScreenIn`]; the UI calls [`ScreenState::tick`] once a
//! frame while a recording runs, which pulls captured frames and feeds them to the recorder. A
//! capture or encoder failure ends the take without a panic: frames already recorded are kept and
//! finished ([`ScreenState::take_ended`]), and only a take with nothing in it is thrown away.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
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
    /// Stop was asked for while a full queue held work back: later ticks send what was kept
    /// before the file is requested, without capturing anything.
    draining: bool,
    /// The UI clock when draining began: the take owes nothing past it, however long the queue
    /// takes to accept the retained work.
    drain_from: Option<f64>,
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
    /// Only the web build constructs this (threads are unavailable there); the match arms still
    /// name it on every target.
    #[allow(dead_code)]
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
    /// The picture the queue refused, for the next interval.
    kept: Option<ScreenFrame>,
    /// A finish was requested: the worker writes the file when the queue drains.
    finish: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// A discard was requested: the worker stops before the next job, and the file is not written.
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The worker's event channel closed: it ended without reporting a finished file.
    gone: bool,
}

/// The recorder on the caller's thread (the web build); never constructed on native targets.
#[allow(dead_code)]
struct Inline {
    recorder: Option<ScreenRecorder>,
    frames: u64,
    failure: Option<String>,
    finished: Option<Result<RecorderOutput, RecorderError>>,
}

impl Encoder {
    /// Start the encoder for a fresh recorder. `Err(recorder)` hands the recorder back when no
    /// thread can be made: the caller then fails the start cleanly (stopping the device and
    /// removing the file) instead of keeping a recorder it could never encode with.
    fn new(recorder: ScreenRecorder) -> Result<Self, ScreenRecorder> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (job_tx, job_rx) = std::sync::mpsc::sync_channel::<Job>(JOB_QUEUE);
            let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
            let finish = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let worker_finish = finish.clone();
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let worker_cancel = cancel.clone();
            // The recorder is handed over only after the thread exists: moving it into the
            // closure first would lose it when the spawn fails, leaving an encoder that can never
            // encode. On failure it comes back through the channel and is returned to the caller.
            let (handoff_tx, handoff_rx) = std::sync::mpsc::sync_channel::<ScreenRecorder>(1);
            let spawned = std::thread::Builder::new().name("deckcraft-encode".into()).spawn(move || {
                let Ok(recorder) = handoff_rx.recv() else { return };
                encode_loop(recorder, &job_rx, &event_tx, &worker_finish, &worker_cancel);
            });
            let handle = match spawned {
                Ok(handle) => handle,
                Err(e) => {
                    log::warn!("screen recording: no encoder thread ({e})");
                    return Err(recorder);
                }
            };
            if let Err(e) = handoff_tx.send(recorder) {
                // The thread ended before it took the recorder (a panic in the thread's setup
                // would): join it and refuse the take.
                let _ = handle.join();
                return Err(e.0);
            }
            Ok(Encoder::Worker(Worker {
                jobs: Some(job_tx),
                events: event_rx,
                handle: Some(handle),
                frames: 0,
                failure: None,
                finished: None,
                kept: None,
                finish,
                cancel,
                gone: false,
            }))
        }
        #[cfg(target_arch = "wasm32")]
        {
            Ok(Encoder::Inline(Inline { recorder: Some(recorder), frames: 0, failure: None, finished: None }))
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
                    // The caller keeps the frame for the next interval and re-schedules it.
                    Err(TrySendError::Full(Job::Encode(frame))) => {
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
                    Err(TrySendError::Full(_)) => false,
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

    /// Whether the encoder can no longer take work: its worker failed or ended, or the queue is
    /// gone. A drain must then finish the take instead of waiting for a slot that never comes.
    fn stalled(&self) -> bool {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Encoder::Worker(w) => w.jobs.is_none() || w.gone || w.failure.is_some(),
            Encoder::Inline(i) => i.failure.is_some() || i.recorder.is_none(),
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

/// The encoder thread: encode or repeat each scheduled picture, then write the file when the queue
/// drains after a finish request.
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
        // take away, which stops the host, drops the encoder and removes the file. A take that
        // ended but was never inserted (its insert failed, or the session quit first) is the same
        // captured content and goes too.
        let _ = self.cancel();
        if let Some(ended) = self.ended.take() {
            let _ = std::fs::remove_file(ended.path);
        }
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
        // An ended take belongs to the host that recorded it; replacing the host must not leave
        // its file behind (the session will not insert it any more).
        if let Some(ended) = self.ended.take() {
            let _ = std::fs::remove_file(ended.path);
        }
        self.host = host;
    }

    pub fn host_installed(&self) -> bool {
        self.host.is_some()
    }

    pub fn is_recording(&self) -> bool {
        self.active.as_ref().is_some_and(|a| !a.stopping && !a.draining)
    }

    /// Whether a take is being written (Stop or a failure asked for the file, or the pictures a
    /// full queue held back are still on their way).
    pub fn is_finishing(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.stopping || a.draining)
    }

    pub fn is_paused(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.paused && !a.stopping && !a.draining)
    }

    /// Frames encoded so far.
    pub fn frames(&self) -> u64 {
        self.active.as_ref().map_or(0, |a| a.frames)
    }

    /// The take's capture rate, for the UI's repaint interval (0 when nothing is recording).
    pub fn fps(&self) -> u32 {
        self.active.as_ref().map_or(0, |a| a.fps)
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
        // The host's target list is not read here: enumerating displays and windows can block on
        // the system picker and the capture worker enumerates anyway, so an empty list is reported
        // by the worker (`NoDevice`) on the first frame instead of freezing the UI at start.
        //
        // A take that ended but was never inserted is superseded by this one: its captured screen
        // content is not left in the temporary folder.
        if let Some(ended) = self.ended.take() {
            let _ = std::fs::remove_file(ended.path);
        }
        // The name carries 128 bits from the OS's per-process random seed and the file is created
        // `create_new`, so a path planted in the shared temporary folder cannot be guessed and
        // truncated (Windows has no safe std ACL API, and the workspace forbids `unsafe`; the
        // temporary folder is the user's own, and owner-only mode covers Unix).
        let named = tempfile::Builder::new()
            .prefix("deckcraft-screen-")
            .suffix(".mp4")
            .rand_bytes(16)
            .tempfile()
            .map_err(|e| ScreenError::Failed(format!("couldn't start a screen recording: {e}")))?;
        let (file, temp) = named.into_parts();
        let path = temp.keep().map_err(|e| ScreenError::Failed(format!("couldn't start a screen recording: {}", e.error)))?;
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
        let encoder = match Encoder::new(recorder) {
            Ok(encoder) => encoder,
            Err(_) => {
                // No encoder thread and no inline recorder for this build: a take that could never
                // encode would fail on its first frame, so refuse it here, stop the device and
                // remove the file.
                host.stop();
                let _ = std::fs::remove_file(&path);
                return Err(ScreenError::Failed("couldn't start a screen recording: no encoder thread could be started".into()));
            }
        };
        self.active = Some(Active {
            encoder,
            path,
            fps: options.fps,
            paused: false,
            due: true,
            next_at: 0.0,
            frames: 0,
            pending: None,
            draining: false,
            drain_from: None,
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
        if active.stopping || active.draining {
            return Err(ScreenError::Busy);
        }
        if let Some(host) = self.host.as_mut() {
            host.stop();
        }
        active.reason.clear();
        // A full queue can hold the newest picture (or an owed interval) back; sending it before
        // the file is asked for keeps the take's last frame and its length (see `tick`).
        if active.pending.is_some() || !active.due {
            active.draining = true;
            active.drain_from = None;
        } else {
            active.stopping = true;
            active.encoder.request_finish();
        }
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
        if active.draining {
            // Stop was asked for while the queue held work back: send it without capturing
            // anything (the host is already stopped), then ask for the file.
            if active.encoder.stalled() {
                // The encoder cannot take the retained work (it failed or ended): write nothing
                // more and let `take_finished` report what happened, rather than draining on
                // every tick.
                active.draining = false;
                active.stopping = true;
                active.encoder.request_finish();
            } else {
                // The take owes intervals only up to the Stop: the wait for a slot must not turn
                // into extra pictures (and extra duration) for every tick it takes.
                let at = *active.drain_from.get_or_insert(now);
                if let Some(e) = drain_retained(active, at) {
                    let reason = format!("the screen recording stopped: {e}");
                    active.reason = reason;
                }
                if active.pending.is_some() || owed_pictures(active, at) > 0 {
                    return None;
                }
                active.draining = false;
                active.stopping = true;
                active.encoder.request_finish();
            }
            // Fall through: a worker that is already gone reports its failure in this tick.
        }
        if active.stopping {
            // Stop already asked for the file: take it when the encoder reports it, and never
            // re-enter the capture path.
            let done = active.encoder.take_finished()?;
            let path = active.path.clone();
            let reason = active.reason.clone();
            // A take whose first frame failed to encode reports `Empty` and the failure together:
            // keep the failure so the message says why, not just that nothing was recorded.
            let failed = active.encoder.failure().map(str::to_string).or_else(|| (!reason.is_empty()).then(|| reason.clone()));
            self.active = None;
            return match done {
                Ok(out) if out.frames > 0 => {
                    self.ended =
                        Some(StoppedRecording { path, frames: out.frames, width: out.width, height: out.height, duration_ms: out.duration_ms });
                    self.ended_reason = Some(reason);
                    None
                }
                // No file was written for an empty take, and the recorder says so: that is not a
                // write failure, so it gets the plain "nothing in it" message — unless the encoder
                // failed first, which is worth saying.
                Ok(_) | Err(RecorderError::Empty) => {
                    let _ = std::fs::remove_file(&path);
                    match failed {
                        Some(why) => Some(format!("the screen recording stopped: {why}")),
                        None => Some("the screen recording had nothing in it".into()),
                    }
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
        let mut why = schedule_owed(active, now, interval);
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

    /// The take a stop or a capture failure finished, with why it ended. Its file is the caller's
    /// to delete once the bytes are in the presentation.
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
        // once the encoder has written it, with this reason attached. Pictures a full queue held
        // back go out first, so the take keeps its last frame.
        active.reason = why.to_string();
        if active.pending.is_some() || !active.due {
            active.draining = true;
            active.drain_from = None;
        } else {
            active.stopping = true;
            active.encoder.request_finish();
        }
        why.to_string()
    }
}

/// Schedule one picture: the newest capture if one is waiting, else a repeat of the last. `Some`
/// is the encoder error when the picture could not be queued.
/// Send the pictures the schedule is owed at `now`: the frame waiting for its slot, then one
/// repeat per interval that passed. Only what the queue took moves the schedule, so a busy
/// encoder catches up instead of shortening the take. Returns an encoder failure, if any.
fn schedule_owed(active: &mut Active, now: f64, interval: f64) -> Option<String> {
    if active.due {
        // The start waits for the first picture: keeping `due` set means a late first frame
        // (the capturer may still be building) starts the schedule rather than becoming
        // hundreds of owed repeats of itself.
        let mut why = None;
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
        return why;
    }
    if now + 1.0e-9 < active.next_at {
        return None;
    }
    // Every interval that passed is owed one picture; one job carries them all.
    let owed = owed_pictures(active, now);
    let pictures = owed.clamp(1, MAX_CATCH_UP);
    let mut why = None;
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
    why
}

/// Intervals the schedule owes at `now` (0 when the next picture is not due yet).
fn owed_pictures(active: &Active, now: f64) -> u32 {
    // A paused take owes nothing: `resume` puts `due` back and restarts the schedule, so the held
    // period must not become repeats at its end (the file would be longer than `elapsed_ms`).
    if active.due || active.paused || now + 1.0e-9 < active.next_at {
        return 0;
    }
    let interval = 1.0 / f64::from(active.fps.max(1));
    (((now - active.next_at) / interval).floor() as u32).saturating_add(1)
}

/// Send what a stop interrupted. A frame the queue refused goes out now rather than at its
/// slot: the take is ending, so the newest picture and the owed intervals are what preserve its
/// content and its length. Returns an encoder failure, if any.
fn drain_retained(active: &mut Active, now: f64) -> Option<String> {
    let interval = 1.0 / f64::from(active.fps.max(1));
    let owed = owed_pictures(active, now);
    let pictures = if active.pending.is_some() { owed.max(1) } else { owed };
    if pictures == 0 {
        return None;
    }
    let (n, why) = schedule_pictures(active, pictures.clamp(1, MAX_CATCH_UP));
    if n > 0 {
        active.due = false;
        active.next_at = now + interval;
    }
    why
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A fake screen device: frames are queued by the test and delivered one per `frame` call.
    struct FakeScreen {
        frames: Arc<Mutex<std::collections::VecDeque<deckcraft_screen::ScreenFrame>>>,
        /// Set when the capture source disappears (a closed window, an unplugged display).
        fail: Arc<std::sync::atomic::AtomicBool>,
        /// How many times the session released the device.
        stops: Arc<std::sync::atomic::AtomicUsize>,
        status: CaptureStatus,
        size: (u32, u32),
    }

    impl FakeScreen {
        fn ready(size: (u32, u32)) -> (Self, Arc<Mutex<std::collections::VecDeque<deckcraft_screen::ScreenFrame>>>) {
            let frames = Arc::new(Mutex::new(std::collections::VecDeque::new()));
            let fail = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            (FakeScreen { frames: frames.clone(), fail, stops, status: CaptureStatus::Ready, size }, frames)
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
        fn stop(&mut self) {
            self.stops.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Run ticks until the encoder hands the finished take over (bounded), as the UI does.
    fn pump(s: &mut ScreenState) -> StoppedRecording {
        // A wall-clock deadline, not a sleep count: the encoder thread may be slow on a loaded
        // machine, and the failure says what was waited for.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            let _ = s.tick(0.0);
            if let Some(rec) = ended(s) {
                return rec;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("the encoder did not hand the take over within 10 s (frames so far: {})", s.frames());
    }

    /// The take the state handed over, without its reason (most tests here only need the file).
    fn ended(s: &mut ScreenState) -> Option<StoppedRecording> {
        s.take_ended_with_reason().map(|(rec, _)| rec)
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

    /// A picture captured after its slot passed sits in `pending`; Stop must still write it (and
    /// the duration it carries) instead of closing the encoder queue on a take that is one frame
    /// short.
    #[test]
    fn a_stop_keeps_the_picture_the_schedule_had_not_sent_yet() {
        let (host, queue) = FakeScreen::ready((160, 120));
        let mut s = state_with(host);
        s.start(CaptureOptions::default(), RecorderConfig::new(30)).expect("start");
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [0, 0, 255, 255]));
        assert_eq!(s.tick(0.0), None);
        assert_eq!(s.frames(), 1, "the first picture starts the schedule");
        // The host already has the next picture, but its slot is 1/30 s away: it waits in
        // `pending`, exactly what a Stop between slots interrupts.
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [0, 255, 0, 255]));
        assert_eq!(s.tick(0.0), None);
        s.stop().expect("stop");
        assert!(s.is_finishing(), "the retained picture is still on its way");
        let stopped = pump(&mut s);
        assert_eq!(stopped.frames, 2, "the picture Stop interrupted is written too");
        assert!(stopped.duration_ms >= 66, "two 30 fps pictures are at least two intervals: {}", stopped.duration_ms);
        let _ = std::fs::remove_file(&stopped.path);
    }

    /// A pause is not recorded: a Stop during the hold writes the one picture it kept, never the
    /// intervals that passed while the take was paused.
    #[test]
    fn a_stop_during_a_pause_adds_the_held_picture_and_no_paused_time() {
        let (host, queue) = FakeScreen::ready((160, 120));
        let mut s = state_with(host);
        s.start(CaptureOptions::default(), RecorderConfig::new(30)).expect("start");
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [1, 1, 1, 255]));
        assert_eq!(s.tick(0.0), None);
        assert_eq!(s.frames(), 1);
        s.pause().expect("pause");
        // Ten seconds of held take: the host keeps handing out the newest picture.
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [2, 2, 2, 255]));
        assert_eq!(s.tick(10.0), None);
        s.stop().expect("stop");
        let stopped = pump(&mut s);
        assert_eq!(stopped.frames, 2, "one recorded picture plus the held one: {stopped:?}");
        assert!(stopped.duration_ms < 200, "the ten paused seconds are not recorded: {} ms", stopped.duration_ms);
        let _ = std::fs::remove_file(&stopped.path);
    }

    /// A drain must not wait for a worker that is gone: the take finishes (and reports the
    /// missing file) instead of holding `draining` while the clock moves.
    #[test]
    fn a_draining_take_whose_worker_is_gone_still_finishes() {
        let (host, queue) = FakeScreen::ready((160, 120));
        let mut s = state_with(host);
        s.start(CaptureOptions::default(), RecorderConfig::new(30)).expect("start");
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [5, 6, 7, 255]));
        assert_eq!(s.tick(0.0), None);
        // The next picture waits for its slot, so Stop drains rather than finishing at once.
        queue.lock().expect("queue").push_back(deckcraft_screen::ScreenFrame::solid(160, 120, [8, 9, 10, 255]));
        assert_eq!(s.tick(0.0), None);
        {
            // Take the worker apart the way a panic does.
            let active = s.active.as_mut().expect("an active take");
            let Encoder::Worker(w) = &mut active.encoder else { panic!("the test runs with an encoder thread") };
            w.jobs = None;
            if let Some(handle) = w.handle.take() {
                handle.join().expect("the worker exits");
            }
        }
        s.stop().expect("stop");
        assert!(s.is_finishing(), "the retained picture is still on its way");
        // The clock moves: a drain that waits for a queue slot would never end.
        let why = (0..5).find_map(|i| s.tick(f64::from(i))).expect("the take ends instead of draining forever");
        assert!(why.contains("couldn't be written"), "{why}");
        assert!(!s.is_recording() && !s.is_finishing(), "the take is over");
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
        s.stop().expect("stop");
        // Stop returns while the encoder still writes: that is the non-blocking contract, and it
        // is checked structurally rather than with a wall-clock bound that a loaded machine trips.
        assert!(s.is_finishing(), "Stop must not wait for the encoder");
        assert!(ended(&mut s).is_none(), "the file is not ready at once");
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

    /// Stop on a screen that never produced a frame still releases the capture device: the host
    /// is stopped at once, the take ends with nothing in it, and no thread is left capturing.
    #[test]
    fn an_idle_stop_releases_the_capture_device() {
        let (host, _queue) = FakeScreen::ready((160, 120));
        let stops = host.stops.clone();
        let mut s = state_with(host);
        s.start(CaptureOptions::default(), RecorderConfig::new(30)).expect("start");
        // No frame was ever delivered, as on a static screen with the capturer still building.
        assert_eq!(s.tick(0.0), None);
        s.stop().expect("stop");
        assert_eq!(stops.load(Ordering::Relaxed), 1, "the device is released even with no frame");
        // The encoder thread needs a moment to write (nothing) and report: wait on a deadline.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut why = None;
        while why.is_none() && std::time::Instant::now() < deadline {
            why = s.tick(0.0);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let why = why.expect("the empty take ends");
        assert!(why.contains("nothing in it"), "{why}");
        assert!(!s.is_recording() && !s.is_finishing());
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
        assert_eq!(ended(&mut s), None, "the take is handed over once");
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
        assert!(ended(&mut s).is_none(), "nothing was recorded, so no file is kept");
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
        assert!(ended(&mut s).is_none(), "there is no file to insert");
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
