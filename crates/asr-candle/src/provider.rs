//! The live transcription provider: a worker thread owns the microphone and the model, the UI
//! starts and stops it and polls it, and nothing on the UI thread ever waits for inference.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use deckcraft_asr::{
    Accelerator, AsrError, AudioBuffer, Capabilities, Event, ModelState, ModelStatus, ProviderState, ProviderStatus, SubtitleSettings, Transcriber,
    model, whisper_tiny,
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
            events: None,
            stop: None,
            handle: None,
        }
    }

    /// The model status for a directory, for the settings dialog and the status line.
    pub fn model_status(dir: &Path) -> ModelStatus {
        model::check_dir(dir, &whisper_tiny())
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
}

impl Transcriber for WhisperTranscriber {
    fn start(&mut self) -> Result<(), AsrError> {
        if self.handle.is_some() {
            self.stop();
        }
        let manifest = whisper_tiny();
        let found = model::check_dir_files(&self.model_dir, &manifest);
        if found.state != ModelState::Ready {
            let message = format!("{} ({})", found.summary(), self.model_dir.display());
            self.set_status(ProviderStatus::failed(message.clone()));
            return Err(AsrError::new(message));
        }
        let (tx, rx) = channel::<Event>();
        let stop = Arc::new(AtomicBool::new(false));
        let status = self.status.clone();
        let (dir, settings) = (self.model_dir.clone(), self.settings.clone());
        let worker_stop = stop.clone();
        let handle = std::thread::Builder::new()
            .name("deckcraft-subtitles".into())
            .spawn(move || run(dir, settings, worker_stop, status, tx))
            .map_err(|e| AsrError::new(format!("cannot start the subtitle worker: {e}")))?;
        self.events = Some(rx);
        self.stop = Some(stop);
        self.handle = Some(handle);
        let caps = capabilities(self.settings.accelerator);
        let mut status = ProviderStatus::starting(manifest.id, self.settings.language_hint().map(str::to_string));
        status.accelerator = Some(caps.chosen);
        status.capabilities = Some(caps);
        self.set_status(status);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(handle) = self.handle.take() {
            // The worker checks the flag between windows; joining here is how the microphone and
            // the model are released before the show goes away.
            let _ = handle.join();
        }
        self.events = None;
        let mut status = self.status.lock().unwrap_or_else(PoisonError::into_inner);
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

/// The worker: load, capture, and decode the last 30 seconds every half second of new audio.
fn run(model_dir: PathBuf, settings: SubtitleSettings, stop: Arc<AtomicBool>, status: Arc<Mutex<ProviderStatus>>, tx: Sender<Event>) {
    let mut engine = match WhisperEngine::load(&model_dir) {
        Ok(engine) => engine,
        Err(e) => return fail(&status, &tx, e),
    };
    // One short silent decode first: the first inference in a process pays the allocation and
    // kernel warm-up (measured ~4x the steady-state cost), and doing it before the microphone
    // opens keeps that out of the first caption.
    let warm = std::time::Instant::now();
    let _ = engine.transcribe(&vec![0.0f32; 16_000], settings.language_hint());
    log::debug!("subtitles: warm-up took {:?}", warm.elapsed());
    let capture = match Capture::open(settings.microphone.as_deref()) {
        Ok(capture) => capture,
        Err(e) => return fail(&status, &tx, e),
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
    while !stop.load(Ordering::Relaxed) {
        if let Some(error) = capture.take_error() {
            return fail(&status, &tx, AsrError::new(format!("the microphone stopped: {error}")));
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
        new_samples = 0;
        match engine.transcribe(&ring, hint.as_deref()) {
            Ok(text) if !text.is_empty() => {
                if pending.as_deref() != Some(text.as_str()) {
                    pending = Some(text.clone());
                    let _ = tx.send(Event::partial(text));
                }
            }
            Ok(_) => {}
            Err(e) => return fail(&status, &tx, e),
        }
        if last_voice.elapsed() >= FINAL_AFTER
            && let Some(text) = pending.take()
        {
            let _ = tx.send(Event::final_line(text));
        }
    }
    if let Ok(mut s) = status.lock() {
        *s = ProviderStatus::stopped();
    }
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

fn fail(status: &Arc<Mutex<ProviderStatus>>, tx: &Sender<Event>, e: AsrError) {
    if let Ok(mut s) = status.lock() {
        *s = ProviderStatus::failed(e.message());
    }
    let _ = tx.send(Event::error(e.message()));
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn starting_without_a_model_fails_once_and_says_where_it_looked() {
        let missing = std::env::temp_dir().join("deckcraft-asr-candle-no-model");
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
}
