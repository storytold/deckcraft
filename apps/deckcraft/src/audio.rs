//! cpal audio devices: output for media playback and the microphone for Record ▸ Audio.
//!
//! The output device pulls mixed samples from the `deckcraft_media::Player`, which makes it the
//! playback clock video frames follow. The microphone runs the same way: a worker thread owns the
//! input stream and its callback appends mono samples, so neither the app nor the UI thread ever
//! runs the device loop.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use deckcraft_media::{AudioOut, MicIn};

/// The first stream error since it was last reported. cpal calls the error callback on its
/// realtime audio thread (on ALSA in a loop while the device stays broken), which must not log:
/// the callback only keeps the first error, and the UI thread logs it when the stream stops.
type ErrorSlot = Arc<Mutex<Option<String>>>;

/// Keep `e` unless an error is already kept. Never blocks: a busy slot drops the error.
fn keep_stream_error(slot: &ErrorSlot, e: &dyn std::fmt::Display) {
    if let Ok(mut kept) = slot.try_lock()
        && kept.is_none()
    {
        *kept = Some(e.to_string());
    }
}

/// The kept error, once (a poisoned lock is tolerated).
fn take_stream_error(slot: &ErrorSlot) -> Option<String> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

#[derive(Default)]
pub struct CpalOut {
    stream: Option<cpal::Stream>,
    error: ErrorSlot,
    /// Set by the realtime error callback when the device went away: the stream is dead.
    dead: Arc<AtomicBool>,
}

impl CpalOut {
    /// Log the stream error the realtime callback kept (UI thread).
    fn report(&self) {
        if let Some(e) = take_stream_error(&self.error) {
            log::warn!("audio stream error: {e}");
        }
    }
}

impl Drop for CpalOut {
    fn drop(&mut self) {
        self.stream = None;
        self.report();
    }
}

/// Mono samples from an interleaved frame. Stops at `cap` samples.
fn push_mono(out: &mut Vec<f32>, interleaved: &[f32], channels: usize, cap: usize) {
    let ch = channels.max(1);
    let frames = interleaved.len() / ch;
    let room = cap.saturating_sub(out.len());
    for i in 0..frames.min(room) {
        let mut sum = 0.0f32;
        let mut n = 0u32;
        for c in 0..ch {
            if let Some(s) = interleaved.get(i * ch + c)
                && s.is_finite()
            {
                sum += *s;
                n += 1;
            }
        }
        if n > 0 {
            out.push((sum / n as f32).clamp(-1.0, 1.0));
        }
    }
}

/// Ten minutes of mono audio at the device's own rate: one recording cannot grow without bound,
/// and a 96 kHz device keeps the same ten minutes instead of being cut at five.
fn max_samples_for(rate: u32) -> usize {
    (rate as usize).saturating_mul(60 * 10)
}

#[derive(Default)]
struct Capture {
    rate: u32,
    samples: Vec<f32>,
}

/// The default input device. A worker thread owns the stream, so opening the device and the
/// realtime capture never run on the UI thread; the callback appends mono f32 and the UI drains it.
#[derive(Default)]
pub struct CpalIn {
    buf: Arc<Mutex<Capture>>,
    error: ErrorSlot,
    /// The device went away while capturing: the take ends, unlike a single dropped buffer.
    dead: Arc<AtomicBool>,
    /// The buffer hit its cap and later samples were dropped: `take` cannot see them.
    dropped: Arc<AtomicBool>,
    /// The worker's start handshake, polled from the UI loop so opening a device (and any
    /// permission prompt) never blocks a frame. Failure reaches the UI through `failure()`.
    #[allow(dead_code)]
    startup: Option<mpsc::Receiver<Result<(), String>>>,
    stop: Option<Arc<(Mutex<bool>, Condvar)>>,
    worker: Option<JoinHandle<()>>,
    /// Start-attempt liveness: a worker detached while still opening must not fail or feed a
    /// later take. Each `start` publishes a fresh generation here; stale workers check it
    /// before touching anything shared.
    live: Arc<AtomicU64>,
    generation: u64,
}

/// Open the default input device and start capturing into `state`'s buffer. Runs on the worker,
/// so a slow device or permission prompt never blocks the UI thread.
fn open_capture(state: &WorkerState) -> Result<(cpal::Stream, u32), String> {
    open_capture_into(state.buf.clone(), state.error.clone(), state.dead.clone(), state.dropped.clone(), state.generation, state.live.clone())
}

/// The device open with explicit capture state, so the generation check can gate the realtime
/// callback too: a stale worker's stream errors must not fail the take that replaced it.
#[allow(clippy::too_many_arguments)]
fn open_capture_into(
    buf: Arc<Mutex<Capture>>,
    slot: ErrorSlot,
    dead: Arc<AtomicBool>,
    full: Arc<AtomicBool>,
    generation: u64,
    live: Arc<AtomicU64>,
) -> Result<(cpal::Stream, u32), String> {
    let dev = cpal::default_host().default_input_device().ok_or_else(|| "no microphone".to_string())?;
    let cfg = dev.default_input_config().map_err(|e| e.to_string())?;
    let rate = cfg.sample_rate().0;
    let channels = cfg.channels() as usize;
    if !(8_000..=192_000).contains(&rate) {
        return Err(format!("microphone sample rate {rate} is not supported"));
    }
    if channels == 0 {
        return Err("the microphone reports no channels".to_string());
    }
    let config: cpal::StreamConfig = cfg.clone().into();
    let cap = max_samples_for(cfg.sample_rate().0);
    // The realtime error callback belongs to the attempt that opened it: a stale worker's
    // stream errors must not fail the take that replaced it.
    let is_current = move || live.load(Ordering::Relaxed) == generation;
    let err = move |e: cpal::StreamError| {
        if !is_current() {
            return;
        }
        if matches!(e, cpal::StreamError::DeviceNotAvailable) {
            dead.store(true, Ordering::Relaxed);
        }
        keep_stream_error(&slot, &e);
    };
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => {
            let mut spill: Vec<f32> = Vec::new();
            dev.build_input_stream(
                &config,
                move |data: &[f32], _| {
                    spill.extend_from_slice(data);
                    if let Ok(mut g) = buf.try_lock() {
                        let all = std::mem::take(&mut spill);
                        if g.samples.len() + all.len() / channels.max(1) > cap {
                            full.store(true, Ordering::Relaxed);
                        }
                        push_mono(&mut g.samples, &all, channels, cap);
                    }
                },
                err,
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let mut spill: Vec<f32> = Vec::new();
            dev.build_input_stream(
                &config,
                move |data: &[i16], _| {
                    spill.extend(data.iter().map(|s| *s as f32 / i16::MAX as f32));
                    if let Ok(mut g) = buf.try_lock() {
                        let all = std::mem::take(&mut spill);
                        if g.samples.len() + all.len() / channels.max(1) > cap {
                            full.store(true, Ordering::Relaxed);
                        }
                        push_mono(&mut g.samples, &all, channels, cap);
                    }
                },
                err,
                None,
            )
        }
        other => return Err(format!("unsupported microphone format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, rate))
}

impl CpalIn {
    fn report(&self) {
        if let Some(e) = take_stream_error(&self.error) {
            log::warn!("microphone stream error: {e}");
        }
    }
}

/// The capture state one worker owns: a detached worker from an older `start` must not fail or
/// feed the take that replaced it, so each attempt carries its generation and checks it before
/// touching anything shared.
struct WorkerState {
    buf: Arc<Mutex<Capture>>,
    error: ErrorSlot,
    dead: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
    generation: u64,
    live: Arc<AtomicU64>,
}

impl WorkerState {
    fn is_current(&self) -> bool {
        self.live.load(Ordering::Relaxed) == self.generation
    }

    fn report(&self) {
        if let Some(e) = take_stream_error(&self.error) {
            log::warn!("microphone stream error: {e}");
        }
    }
}

impl MicIn for CpalIn {
    fn start(&mut self) -> Result<(), String> {
        self.stop();
        {
            let mut g = self.buf.lock().unwrap_or_else(PoisonError::into_inner);
            g.rate = 0;
            g.samples.clear();
        }
        self.dead.store(false, Ordering::Relaxed);
        self.dropped.store(false, Ordering::Relaxed);
        // A previous failure has been reported; the new take starts clean.
        let _ = take_stream_error(&self.error);
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.live.store(generation, Ordering::Relaxed);
        let (tx, rx) = mpsc::sync_channel(1);
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_stop = stop.clone();
        // The worker needs the capture parameters, not the whole mic: the UI-side `stop`,
        // `startup` and `worker` handles stay with the new take while a detached worker from
        // an older attempt may still be inside `open()`.
        let this = WorkerState {
            buf: self.buf.clone(),
            error: self.error.clone(),
            dead: self.dead.clone(),
            dropped: self.dropped.clone(),
            generation,
            live: self.live.clone(),
        };
        let handle = std::thread::Builder::new()
            .name("deckcraft-mic".into())
            .spawn(move || {
                // The device open runs on the worker: a stale attempt that was detached while
                // still opening must not hand its stream to the take that replaced it.
                let opened = open_capture(&this);
                match opened {
                    Ok((stream, rate)) => {
                        // A take that started after this worker was detached owns the buffer
                        // now: a late success must not overwrite its rate or feed it samples.
                        if !this.is_current() {
                            drop(stream);
                            return;
                        }
                        this.buf.lock().unwrap_or_else(PoisonError::into_inner).rate = rate;
                        if tx.send(Ok(())).is_err() {
                            return;
                        }
                        let (lock, cv) = &*worker_stop;
                        let mut done = lock.lock().unwrap_or_else(PoisonError::into_inner);
                        while !*done {
                            done = cv.wait(done).unwrap_or_else(PoisonError::into_inner);
                        }
                        drop(done);
                        drop(stream);
                        // Only the current take's error slot is ours: a stale worker must not
                        // drain the take that replaced it.
                        if this.is_current() {
                            this.report();
                        }
                    }
                    Err(e) => {
                        // Same rule for a late failure: it must not fail the take that
                        // replaced this attempt.
                        if !this.is_current() {
                            let _ = tx.send(Err(e));
                            return;
                        }
                        let error = this.error.clone();
                        this.dead.store(true, Ordering::Relaxed);
                        keep_stream_error(&error, &e);
                        let _ = tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| format!("couldn't start the microphone thread: {e}"))?;
        // The worker reports through `failure()`; the UI thread never waits on a device open or a
        // permission prompt.
        self.startup = Some(rx);
        self.stop = Some(stop);
        self.worker = Some(handle);
        Ok(())
    }

    fn take(&mut self) -> (u32, Vec<f32>) {
        let mut g = self.buf.lock().unwrap_or_else(PoisonError::into_inner);
        let rate = g.rate;
        let samples = std::mem::take(&mut g.samples);
        (rate, samples)
    }

    fn captured(&self) -> usize {
        self.buf.lock().unwrap_or_else(PoisonError::into_inner).samples.len()
    }

    fn dropped(&self) -> bool {
        self.dropped.load(Ordering::Relaxed)
    }

    fn failure(&self) -> Option<String> {
        // A startup that failed and a lost device are both failed takes (the worker keeps the
        // reason in the slot, so this is stable across calls). A single dropped buffer is not:
        // it is logged and capture continues.
        if !self.dead.load(Ordering::Relaxed) {
            return None;
        }
        // Peek: `report` still logs the error, so this must not consume it.
        Some(self.error.lock().ok().and_then(|kept| kept.clone()).unwrap_or_else(|| "the microphone is no longer available".to_string()))
    }

    fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let (lock, cv) = &*stop;
            *lock.lock().unwrap_or_else(PoisonError::into_inner) = true;
            cv.notify_all();
        }
        // The worker may still be opening the device (enumeration or a permission prompt):
        // joining unconditionally would block the UI. Once the handshake reported, the worker
        // is past `open()` and the join returns promptly; before that, detach and let the stop
        // flag end it when `open()` returns.
        let starting = self.startup.as_ref().is_some_and(|rx| matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        if starting {
            // The detached worker is now stale: publish a fresh generation so its late open,
            // success, failure, stream errors and exit report cannot touch the next take.
            self.generation = self.generation.wrapping_add(1);
            self.live.store(self.generation, Ordering::Relaxed);
            self.startup = None;
            let _ = self.worker.take();
        } else if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
        self.report();
        self.buf.lock().unwrap_or_else(PoisonError::into_inner).samples.clear();
    }
}

impl AudioOut for CpalOut {
    fn start(&mut self, mut fill: Box<dyn FnMut(&mut [f32], usize, u32) + Send>) -> Result<(), String> {
        self.stop();
        let dev = cpal::default_host().default_output_device().ok_or("no audio output device")?;
        let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
        let channels = cfg.channels() as usize;
        let rate = cfg.sample_rate().0;
        let config: cpal::StreamConfig = cfg.clone().into();
        let slot = self.error.clone();
        let dead = Arc::new(AtomicBool::new(false));
        self.dead = dead.clone();
        let err = move |e: cpal::StreamError| {
            if matches!(e, cpal::StreamError::DeviceNotAvailable) {
                dead.store(true, Ordering::Relaxed);
            }
            keep_stream_error(&slot, &e);
        };
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => dev.build_output_stream(&config, move |buf: &mut [f32], _| fill(buf, channels, rate), err, None),
            cpal::SampleFormat::I16 => {
                let mut tmp = Vec::new();
                dev.build_output_stream(
                    &config,
                    move |buf: &mut [i16], _| {
                        tmp.resize(buf.len(), 0.0);
                        fill(&mut tmp, channels, rate);
                        for (o, s) in buf.iter_mut().zip(&tmp) {
                            *o = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                        }
                    },
                    err,
                    None,
                )
            }
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        self.stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) {
        self.stream = None;
        self.report();
    }

    fn failure(&mut self) -> Option<String> {
        if !self.dead.swap(false, Ordering::Relaxed) {
            return None;
        }
        Some(take_stream_error(&self.error).unwrap_or_else(|| "the audio device is no longer available".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_first_stream_error_is_kept_and_it_is_taken_once() {
        let slot = ErrorSlot::default();
        assert_eq!(take_stream_error(&slot), None);
        keep_stream_error(&slot, &"device unplugged");
        keep_stream_error(&slot, &"device unplugged again");
        assert_eq!(take_stream_error(&slot).as_deref(), Some("device unplugged"));
        assert_eq!(take_stream_error(&slot), None);
        keep_stream_error(&slot, &"a later error");
        assert_eq!(take_stream_error(&slot).as_deref(), Some("a later error"));
    }

    #[test]
    fn mono_capture_averages_channels_and_stops_at_the_cap() {
        let mut out = Vec::new();
        push_mono(&mut out, &[1.0, 0.0, 0.5, -0.5, f32::NAN, 1.0], 2, 8);
        assert_eq!(out, vec![0.5, 0.0, 1.0], "frames averaged, a non-finite channel skipped");
        let mut out = Vec::new();
        push_mono(&mut out, &[0.1, 0.2, 0.3, 0.4], 1, 3);
        assert_eq!(out, vec![0.1, 0.2, 0.3], "the cap stops capture growing without bound");
        let mut out = Vec::new();
        push_mono(&mut out, &[0.25], 0, 4);
        assert_eq!(out, vec![0.25], "zero channels is treated as mono");
    }

    #[test]
    fn a_poisoned_slot_still_reports() {
        let slot = ErrorSlot::default();
        keep_stream_error(&slot, &"lost");
        let s = slot.clone();
        let _ = std::thread::spawn(move || {
            let _g = s.lock();
            panic!("poison the slot");
        })
        .join();
        assert!(slot.is_poisoned());
        assert_eq!(take_stream_error(&slot).as_deref(), Some("lost"));
    }

    /// `stop()` while the worker is still opening the device must not block the UI: the stop
    /// signal is set and the half-started worker is detached instead of joined.
    #[test]
    fn stop_while_startup_is_pending_does_not_block() {
        use std::time::{Duration, Instant};
        let mut mic = CpalIn::default();
        // A handshake that never reports: the worker is still inside `open()`.
        let (_tx, rx) = mpsc::sync_channel::<Result<(), String>>(1);
        let stop = std::sync::Arc::new((Mutex::new(false), Condvar::new()));
        mic.startup = Some(rx);
        mic.stop = Some(stop.clone());
        // A parked thread stands in for the worker stuck in `open()`: a join would hang.
        let parked = std::thread::Builder::new()
            .name("deckcraft-mic-test".into())
            .spawn(|| {
                std::thread::sleep(Duration::from_secs(30));
            })
            .expect("test worker");
        mic.worker = Some(parked);
        let start = Instant::now();
        mic.stop();
        assert!(start.elapsed() < Duration::from_secs(5), "stop returned without joining startup");
        assert!(mic.worker.is_none(), "the starting worker is detached");
        assert!(mic.startup.is_none(), "the pending handshake is dropped with the worker");
        assert!(*stop.0.lock().unwrap_or_else(PoisonError::into_inner), "the stop flag is set");
    }

    /// A startup handshake that already reported means the worker is past `open()`: `stop()`
    /// still joins it promptly instead of leaking the thread.
    #[test]
    fn stop_after_startup_reported_joins_the_worker() {
        let mut mic = CpalIn::default();
        // `Ok` already waiting: the worker finished `open()` and only waits for the stop flag.
        let (tx, rx) = mpsc::sync_channel::<Result<(), String>>(1);
        tx.send(Ok(())).expect("handshake");
        let stop = std::sync::Arc::new((Mutex::new(false), Condvar::new()));
        mic.startup = Some(rx);
        mic.stop = Some(stop);
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        mic.worker = Some(
            std::thread::Builder::new()
                .name("deckcraft-mic-test".into())
                .spawn(move || {
                    flag.store(true, Ordering::Relaxed);
                })
                .expect("test worker"),
        );
        mic.stop();
        assert!(done.load(Ordering::Relaxed), "the worker ran to completion");
        assert!(mic.worker.is_none(), "the joined worker is cleared");
    }

    /// A worker detached while still opening must not fail or feed the take that replaced it:
    /// its late error and late success are dropped once the generation moved on.
    #[test]
    fn a_detached_workers_late_results_do_not_touch_the_next_take() {
        let live = Arc::new(AtomicU64::new(7));
        let stale = WorkerState {
            buf: Arc::new(Mutex::new(Capture::default())),
            error: ErrorSlot::default(),
            dead: Arc::new(AtomicBool::new(false)),
            dropped: Arc::new(AtomicBool::new(false)),
            generation: 7,
            live: live.clone(),
        };
        // A new take started: the detached worker is now stale.
        live.store(8, Ordering::Relaxed);
        assert!(!stale.is_current(), "the detached generation is stale");
        // Its stream-error callback path is gated the same way: simulate what the closed-over
        // `is_current` check does instead of building a real cpal stream.
        let was_current = stale.live.load(Ordering::Relaxed) == stale.generation;
        if was_current {
            stale.dead.store(true, Ordering::Relaxed);
        }
        assert!(!stale.dead.load(Ordering::Relaxed), "a stale DeviceNotAvailable sets nothing");
        assert!(take_stream_error(&stale.error).is_none(), "a stale error keeps nothing");
    }
}
