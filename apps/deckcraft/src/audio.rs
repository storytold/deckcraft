//! cpal audio devices: output for media playback and the microphone for Record ▸ Audio.
//!
//! The output device pulls mixed samples from the `deckcraft_media::Player`, which makes it the
//! playback clock video frames follow. The microphone runs the same way: a worker thread owns the
//! input stream and its callback appends mono samples, so neither the app nor the UI thread ever
//! runs the device loop.

use std::sync::atomic::{AtomicBool, Ordering};
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
}

impl CpalIn {
    /// Open the default input device and start capturing into `buf`. Runs on the calling thread
    /// only up to the worker's start handshake.
    fn open(&self) -> Result<(cpal::Stream, u32), String> {
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
        let buf = self.buf.clone();
        let slot = self.error.clone();
        let dead = self.dead.clone();
        let full = self.dropped.clone();
        let err = move |e: cpal::StreamError| {
            dead.store(true, Ordering::Relaxed);
            keep_stream_error(&slot, &e);
        };
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => dev.build_input_stream(
                &config,
                move |data: &[f32], _| {
                    if let Ok(mut g) = buf.try_lock() {
                        push_mono(&mut g.samples, data, channels, cap);
                    }
                },
                err,
                None,
            ),
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

    fn report(&self) {
        if let Some(e) = take_stream_error(&self.error) {
            log::warn!("microphone stream error: {e}");
        }
    }
}

impl Drop for CpalIn {
    fn drop(&mut self) {
        self.stop();
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
        let (tx, rx) = mpsc::sync_channel(1);
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_stop = stop.clone();
        let this = CpalIn {
            buf: self.buf.clone(),
            error: self.error.clone(),
            dead: self.dead.clone(),
            dropped: self.dropped.clone(),
            startup: None,
            stop: None,
            worker: None,
        };
        let handle = std::thread::Builder::new()
            .name("deckcraft-mic".into())
            .spawn(move || match this.open() {
                Ok((stream, rate)) => {
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
                    this.report();
                }
                Err(e) => {
                    // The worker's own `Drop` reports and drains the error slot, so keep the
                    // reason where `failure()` reads it *after* that runs: a caller must not be
                    // left with "the microphone is no longer available" instead of the device or
                    // permission error.
                    let error = this.error.clone();
                    this.dead.store(true, Ordering::Relaxed);
                    drop(this);
                    keep_stream_error(&error, &e);
                    let _ = tx.send(Err(e));
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
        if let Some(handle) = self.worker.take() {
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
}
