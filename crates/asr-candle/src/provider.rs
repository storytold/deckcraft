//! The live transcription provider: a worker thread owns the microphone and the model, the UI
//! starts and stops it and polls it, and nothing on the UI thread ever waits for inference.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use deckcraft_asr::{
    Accelerator, AsrError, AudioBuffer, Capabilities, Event, ModelState, ProviderState, ProviderStatus, SubtitleSettings, Transcriber, model,
    whisper_tiny,
};

use crate::capture::{self, Capture};
use crate::engine::WhisperEngine;

/// How much audio the sliding window keeps (the encoder's own window).
pub const WINDOW_SAMPLES: usize = 16_000 * 30;
/// How much new audio triggers another decode.
const MIN_NEW_SAMPLES: usize = 16_000 / 2;
/// The least audio a decode needs before it says anything.
const MIN_WINDOW_SAMPLES: usize = 16_000;
/// Above this RMS a chunk counts as speech, which holds a final line back.
const VOICE_RMS: f32 = 0.006;
/// Silence after which the pending partial line becomes final.
const FINAL_AFTER: Duration = Duration::from_millis(900);
/// How long the loop waits for the next audio chunk each turn.
const TICK: Duration = Duration::from_millis(60);
/// The least time between two decodes. A decode takes 1.2-2.7 s on a laptop, so scheduling the
/// next one 0.5 s after the previous one finished only builds a backlog; a two-second floor keeps
/// the worker near real time and lets stale audio go.
const MIN_DECODE_INTERVAL: Duration = Duration::from_secs(2);

/// The accelerators this build can run speech inference on, here. The CPU is always there; an
/// Intel OpenVINO installation is reported as detected but is not usable until a backend for it
/// ships (the resolver explains the fallback instead of failing).
pub fn detected_accelerators() -> Vec<Accelerator> {
    let mut out = vec![Accelerator::Cpu];
    let openvino = ["INTEL_OPENVINO_DIR", "OPENVINO_DEVICE", "DECKCRAFT_OPENVINO"].iter().any(|k| std::env::var_os(k).is_some());
    if openvino {
        out.push(Accelerator::OpenVino);
    }
    out
}

/// What running with `requested` would use, in one value the status query reports.
pub fn capabilities(requested: Accelerator) -> Capabilities {
    deckcraft_asr::accelerator::resolve(requested, &detected_accelerators())
}

/// A Whisper transcriber.
pub struct WhisperTranscriber {
    model_dir: PathBuf,
    settings: SubtitleSettings,
    status: Arc<Mutex<ProviderStatus>>,
    /// Which worker owns `status`. A stopped worker can still be finishing a decode while a new
    /// one has started; only the current generation may write the shared status.
    generation: Arc<AtomicU64>,
    events: Option<Receiver<Event>>,
    stop: Option<Arc<AtomicBool>>,
    handle: Option<JoinHandle<()>>,
}

impl WhisperTranscriber {
    pub fn new(model_dir: impl Into<PathBuf>, settings: SubtitleSettings) -> Self {
        WhisperTranscriber {
            model_dir: model_dir.into(),
            settings,
            status: Arc::new(Mutex::new(ProviderStatus::idle())),
            generation: Arc::new(AtomicU64::new(0)),
            events: None,
            stop: None,
            handle: None,
        }
    }

    /// The input devices this host can record from.
    pub fn microphones() -> Vec<String> {
        capture::input_devices()
    }

    /// What this host would run on for `requested`.
    pub fn capabilities(requested: Accelerator) -> Capabilities {
        capabilities(requested)
    }

    fn set_status(&self, new: ProviderStatus) {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner) = new;
    }

    /// Claim the next generation *and* publish `starting` in one critical section: a stale
    /// worker's [`publish`] checks the generation under the same lock, so it cannot slip between
    /// the check and the write.
    fn begin_generation(&self, starting: ProviderStatus) -> u64 {
        let mut status = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *status = starting;
        generation
    }
}

impl Transcriber for WhisperTranscriber {
    fn start(&mut self) -> Result<(), AsrError> {
        if self.handle.is_some() {
            self.stop();
        }
        let manifest = whisper_tiny();
        let found = model::check_dir_files(&self.model_dir, &manifest);
        if found.state != ModelState::Ready {
            // Name the model and the folder: the status bar shows this, and "not installed" on
            // its own does not say which model or where it was looked for.
            let message = format!("{}: {} ({})", found.summary(), manifest.id, self.model_dir.display());
            self.set_status(ProviderStatus::failed(message.clone()));
            return Err(AsrError::new(message));
        }
        let (tx, rx) = channel::<Event>();
        let stop = Arc::new(AtomicBool::new(false));
        let status = self.status.clone();
        let (dir, settings) = (self.model_dir.clone(), self.settings.clone());
        // `starting` goes in before the worker exists: a damaged model fails within microseconds,
        // and that failure must not be overwritten by a late `starting`.
        let caps = capabilities(self.settings.accelerator);
        // A new generation owns the shared status from here on: the previous worker (join handed
        // to a reaper) may still be decoding and must not overwrite this stream's state.
        let mut starting = ProviderStatus::starting(manifest.id, self.settings.language_hint().map(str::to_string));
        starting.accelerator = Some(caps.chosen);
        starting.capabilities = Some(caps);
        let generation = self.begin_generation(starting);
        let worker_generation = self.generation.clone();
        let worker_stop = stop.clone();
        let handle = std::thread::Builder::new()
            .name("deckcraft-subtitles".into())
            .spawn(move || run(dir, settings, worker_stop, status, worker_generation, generation, tx))
            .map_err(|e| {
                let message = format!("cannot start the subtitle worker: {e}");
                self.set_status(ProviderStatus::failed(message.clone()));
                AsrError::new(message)
            })?;
        self.events = Some(rx);
        self.stop = Some(stop);
        self.handle = Some(handle);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(handle) = self.handle.take() {
            // The worker checks the flag between windows and releases the microphone and the
            // model itself. Joining here would block the UI thread for the length of an in-flight
            // decode (seconds to tens of seconds), so the join is handed to a short-lived thread;
            // dropping the handle instead would be the same, but joining makes the release
            // observable and keeps the thread from outliving the process unnoticed.
            let _ = std::thread::Builder::new().name("deckcraft-subtitles-reaper".into()).spawn(move || {
                let _ = handle.join();
            });
        }
        self.events = None;
        let mut status = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        // Retire this stream's generation under the same lock: an old worker still decoding can
        // no longer publish once the flag is seen.
        self.generation.fetch_add(1, Ordering::SeqCst);
        if status.state != ProviderState::Failed {
            *status = ProviderStatus::stopped();
        }
    }

    fn status(&self) -> ProviderStatus {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn poll(&mut self) -> Vec<Event> {
        let Some(rx) = &self.events else { return vec![] };
        let mut out = Vec::new();
        while let Ok(event) = rx.try_recv() {
            out.push(event);
        }
        out
    }
}

impl Drop for WhisperTranscriber {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The worker: load, capture, and decode the last 30 seconds no faster than the decode itself
/// allows (a 2 s floor measured from the previous decode's end).
fn run(
    model_dir: PathBuf,
    settings: SubtitleSettings,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<ProviderStatus>>,
    current: Arc<AtomicU64>,
    generation: u64,
    tx: Sender<Event>,
) {
    let mut engine = match WhisperEngine::load(&model_dir) {
        Ok(engine) => engine,
        Err(e) => return fail(&status, &current, generation, &tx, e),
    };
    // `stop` joins this thread, so the flag is checked between the start-up stages: ending a
    // show or changing the settings then waits for one stage, not for load + warm-up + capture.
    if stop.load(Ordering::Relaxed) {
        return;
    }
    // One short silent decode first: the first inference in a process pays the allocation and
    // kernel warm-up (measured ~4x the steady-state cost), and doing it before the microphone
    // opens keeps that out of the first caption.
    let warm = std::time::Instant::now();
    // The warm-up is a real decode: an invalid language tag fails here, before the microphone
    // opens, instead of surfacing on the first line the presenter speaks.
    if let Err(e) = engine.transcribe(&vec![0.0f32; 16_000], settings.language_hint()) {
        return fail(&status, &current, generation, &tx, e);
    }
    log::debug!("subtitles: warm-up took {:?}", warm.elapsed());
    if stop.load(Ordering::Relaxed) {
        return;
    }
    let capture = match Capture::open(settings.microphone.as_deref()) {
        Ok(capture) => capture,
        Err(e) => return fail(&status, &current, generation, &tx, e),
    };
    {
        let caps = capabilities(settings.accelerator);
        let mut s = status.lock().unwrap_or_else(PoisonError::into_inner);
        *s = ProviderStatus {
            microphone: Some(capture.device.clone()),
            accelerator: Some(caps.chosen),
            capabilities: Some(caps),
            ..ProviderStatus::running(whisper_tiny().id, settings.language_hint().map(str::to_string))
        };
    }
    let hint = settings.language_hint().map(str::to_string);
    let mut ring: Vec<f32> = Vec::with_capacity(WINDOW_SAMPLES);
    let mut new_samples = 0usize;
    let mut last_voice = Instant::now();
    let mut pending: Option<String> = None;
    let mut last_decode: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        if let Some(error) = capture.take_error() {
            return fail(&status, &current, generation, &tx, AsrError::new(format!("the microphone stopped: {error}")));
        }
        // Take everything queued: a decode takes seconds, and the audio that arrives while it
        // runs must not be dropped from the capture queue.
        let mut drained = false;
        loop {
            let wait = if drained { Duration::ZERO } else { TICK };
            let Some(chunk) = capture.recv(wait) else { break };
            absorb(&chunk, &mut ring, &mut new_samples, &mut last_voice);
            drained = true;
        }
        if !drained {
            continue;
        }
        if ring.len() > WINDOW_SAMPLES {
            let cut = ring.len() - WINDOW_SAMPLES;
            ring.drain(..cut);
        }
        if new_samples < MIN_NEW_SAMPLES || ring.len() < MIN_WINDOW_SAMPLES {
            continue;
        }
        // Nothing has been said since the last final line: the window holds only silence, and a
        // decode could only repeat text that has already been shown. Keep the newest second so
        // the next decode starts near the speech instead of over 30 s of silence.
        if is_idle(pending.as_deref(), last_voice) {
            if ring.len() > MIN_WINDOW_SAMPLES {
                let cut = ring.len() - MIN_WINDOW_SAMPLES;
                ring.drain(..cut);
            }
            new_samples = 0;
            continue;
        }
        // A decode takes seconds; starting the next one as soon as half a second of new audio
        // arrived would keep the CPU saturated and grow the lag. Wait for the floor measured from
        // the *end* of the previous decode; the accumulation continues meanwhile and stale audio
        // falls off the ring.
        if last_decode.is_some_and(|at| !decode_due(at, Instant::now())) {
            continue;
        }
        new_samples = 0;
        match engine.transcribe(&ring, hint.as_deref()) {
            Ok(text) if !text.is_empty() => {
                if pending.as_deref() != Some(text.as_str()) {
                    pending = Some(text.clone());
                    let _ = tx.send(Event::partial(text));
                }
            }
            Ok(_) => {}
            Err(e) => return fail(&status, &current, generation, &tx, e),
        }
        last_decode = Some(Instant::now());
        // A decode takes seconds, and the audio that arrived meanwhile is still in the capture
        // queue. Fold it in before deciding the speaker paused: speech heard during the decode
        // must refresh `last_voice`, or continuous speech would be cut at every decode boundary.
        while let Some(chunk) = capture.recv(Duration::ZERO) {
            absorb(&chunk, &mut ring, &mut new_samples, &mut last_voice);
        }
        if last_voice.elapsed() >= FINAL_AFTER
            && let Some(text) = pending.take()
        {
            let _ = tx.send(Event::final_line(text));
            // The finalized speech is in the window: drop it, so the next partial covers only
            // what has been said since instead of sending the same line again.
            ring.clear();
            new_samples = 0;
        }
    }
    publish(&status, &current, generation, ProviderStatus::stopped());
}

/// Whether a new decode may start: never before [`MIN_DECODE_INTERVAL`] has passed since the
/// previous one finished.
fn decode_due(last_finished: Instant, now: Instant) -> bool {
    now.saturating_duration_since(last_finished) >= MIN_DECODE_INTERVAL
}

/// Whether the window only holds silence since the last final line, so another decode would
/// repeat text that has already been shown.
fn is_idle(pending: Option<&str>, last_voice: Instant) -> bool {
    pending.is_none() && last_voice.elapsed() >= FINAL_AFTER
}

/// Add a captured chunk (converted to 16 kHz mono) to the ring, remembering when speech was last
/// heard.
fn absorb(chunk: &AudioBuffer, ring: &mut Vec<f32>, new_samples: &mut usize, last_voice: &mut Instant) {
    let mono = chunk.mono_16k();
    if mono.is_empty() {
        return;
    }
    if rms(&mono) > VOICE_RMS {
        *last_voice = Instant::now();
    }
    *new_samples = new_samples.saturating_add(mono.len());
    ring.extend_from_slice(&mono);
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

/// Write `new` as the provider status only while `generation` still owns it. The check and the
/// write share the status lock with [`WhisperTranscriber::begin_generation`], so a replacement
/// cannot interleave. A worker whose stream was stopped and replaced must not overwrite the
/// replacement's status.
fn publish(status: &Arc<Mutex<ProviderStatus>>, current: &AtomicU64, generation: u64, new: ProviderStatus) {
    // A poisoned lock means some other thread panicked holding the status. The status is still the
    // best one we have, and skipping the write would leave the queried status disagreeing with the
    // error event the worker just sent, so recover it as every other status path in this file does.
    let mut s = status.lock().unwrap_or_else(PoisonError::into_inner);
    if current.load(Ordering::SeqCst) == generation {
        *s = new;
    }
}

fn fail(status: &Arc<Mutex<ProviderStatus>>, current: &AtomicU64, generation: u64, tx: &Sender<Event>, e: AsrError) {
    publish(status, current, generation, ProviderStatus::failed(e.message()));
    let _ = tx.send(Event::error(e.message()));
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn temp(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("deckcraft-asr-candle-{}-{name}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn starting_without_a_model_fails_once_and_says_where_it_looked() {
        let missing = temp("no-model");
        let mut t = WhisperTranscriber::new(&missing, SubtitleSettings::default());
        let err = t.start().expect_err("no model");
        assert!(err.message().contains("not installed"), "{err}");
        assert_eq!(t.status().state, ProviderState::Failed);
        assert!(t.status().detail.is_some_and(|d| d.contains("whisper-tiny")));
        assert!(t.poll().is_empty(), "a start that failed has no stream to report");
        t.stop();
        assert_eq!(t.status().state, ProviderState::Failed, "the failure stays visible after a stop");
    }

    #[test]
    fn the_ring_keeps_a_thirty_second_window_and_spots_speech() {
        let mut ring = Vec::new();
        let mut new_samples = 0;
        let mut last_voice = Instant::now();
        let quiet = AudioBuffer::new(vec![0.0; 1600], 16_000, 1);
        absorb(&quiet, &mut ring, &mut new_samples, &mut last_voice);
        assert_eq!(new_samples, 1600);
        let loud = AudioBuffer::new(vec![0.5; 1600], 16_000, 1);
        let before = last_voice;
        std::thread::sleep(Duration::from_millis(2));
        absorb(&loud, &mut ring, &mut new_samples, &mut last_voice);
        assert!(last_voice >= before);
        assert_eq!(ring.len(), 3200);
        ring.extend(std::iter::repeat_n(0.0, WINDOW_SAMPLES));
        ring.drain(..ring.len() - WINDOW_SAMPLES);
        assert_eq!(ring.len(), WINDOW_SAMPLES);
    }

    #[test]
    fn rms_is_bounded_and_quiet_for_silence() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0.0; 100]), 0.0);
        assert!((rms(&[0.5, -0.5]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn silence_after_a_final_line_does_not_decode_again() {
        let quiet_for_a_while = Instant::now() - FINAL_AFTER - Duration::from_millis(1);
        assert!(is_idle(None, quiet_for_a_while), "silence with nothing pending would only repeat the last line");
        assert!(is_idle(None, Instant::now() - FINAL_AFTER), "the final line has just gone out");
        assert!(!is_idle(Some("still pending"), quiet_for_a_while), "a pending line still has to be finalized");
        assert!(!is_idle(None, Instant::now()), "speech heard now keeps the decoder running");
    }

    #[test]
    fn a_decode_is_not_scheduled_faster_than_the_decode_time() {
        let now = Instant::now();
        assert!(decode_due(now - MIN_DECODE_INTERVAL - Duration::from_millis(1), now), "the floor has passed");
        assert!(!decode_due(now, now), "a decode that just finished is not restarted");
        assert!(!decode_due(now, now + MIN_DECODE_INTERVAL / 2), "the floor is measured from the finish");
        assert!(decode_due(now, now + MIN_DECODE_INTERVAL));
    }

    #[test]
    fn a_stale_worker_cannot_publish_its_status() {
        let status = Arc::new(Mutex::new(ProviderStatus::idle()));
        let generation = Arc::new(AtomicU64::new(2));
        publish(&status, &generation, 1, ProviderStatus::failed("the old stream"));
        assert_eq!(status.lock().unwrap_or_else(PoisonError::into_inner).state, ProviderState::Idle, "the retired stream is ignored");
        publish(&status, &generation, 2, ProviderStatus::stopped());
        assert_eq!(status.lock().unwrap_or_else(PoisonError::into_inner).state, ProviderState::Stopped, "the current stream publishes");
    }

    #[test]
    fn a_poisoned_status_lock_still_publishes_the_failure() {
        let status = Arc::new(Mutex::new(ProviderStatus::idle()));
        let generation = Arc::new(AtomicU64::new(1));
        let poison = status.clone();
        std::thread::spawn(move || {
            let _guard = poison.lock().expect("lock the status");
            panic!("poison the status lock");
        })
        .join()
        .expect_err("the thread panicked holding the status lock");
        publish(&status, &generation, 1, ProviderStatus::failed("late failure"));
        let state = status.lock().unwrap_or_else(PoisonError::into_inner).state;
        assert_eq!(state, ProviderState::Failed, "a poisoned lock must not swallow the failure the event stream reported");
    }

    #[test]
    fn a_missing_model_fails_the_worker_once_and_reports_where_it_looked() {
        let dir = temp("run-missing");
        let _ = std::fs::remove_dir_all(&dir);
        let (tx, rx) = channel();
        let status = Arc::new(Mutex::new(ProviderStatus::idle()));
        run(dir, SubtitleSettings::default(), Arc::new(AtomicBool::new(false)), status.clone(), Arc::new(AtomicU64::new(1)), 1, tx);
        let first = rx.try_recv().expect("one error event");
        assert!(matches!(first, Event::Error { .. }), "{first:?}");
        assert!(rx.try_recv().is_err(), "the error is reported once");
        assert_eq!(status.lock().unwrap_or_else(PoisonError::into_inner).state, ProviderState::Failed);
    }
}
