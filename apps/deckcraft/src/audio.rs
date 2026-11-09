//! cpal audio output for media playback: the device pulls mixed samples from the
//! `deckcraft_media::Player`, which makes it the playback clock video frames follow.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use deckcraft_media::AudioOut;

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

/// Mono samples from an interleaved frame. Stops at `cap` samples; true when it had to drop some.
/// The mono downmix the input callback runs: `f32` samples as they are, every other device format
/// (`I16`, `U16`, `I32`… on ALSA and USB microphones) through cpal's conversion, one frame at a
/// time, so the realtime callback allocates nothing.
fn push_mono_as<T: cpal::SizedSample>(out: &mut Vec<f32>, interleaved: &[T], channels: usize, cap: usize) -> bool
where
    f32: cpal::FromSample<T>,
{
    let ch = channels.max(1);
    let frames = interleaved.len() / ch;
    let room = cap.saturating_sub(out.len());
    let full = frames > room;
    for i in 0..frames.min(room) {
        let mut sum = 0.0f32;
        let mut n = 0u32;
        for c in 0..ch {
            if let Some(s) = interleaved.get(i * ch + c) {
                let v: f32 = cpal::Sample::to_sample(*s);
                if v.is_finite() {
                    sum += v;
                    n += 1;
                }
            }
        }
        if n > 0 {
            out.push((sum / n as f32).clamp(-1.0, 1.0));
        }
    }
    full
}

/// One input stream for any device sample format: the callback is written once.
fn input_stream<T: cpal::SizedSample>(
    dev: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    buf: Arc<Mutex<Capture>>,
    counter: Arc<std::sync::atomic::AtomicU64>,
    slot: ErrorSlot,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    f32: cpal::FromSample<T>,
{
    let err = move |e: cpal::StreamError| keep_stream_error(&slot, &e);
    dev.build_input_stream(
        config,
        move |data: &[T], _| match buf.try_lock() {
            Ok(mut g) => {
                let cap = max_samples(g.rate);
                if push_mono_as(&mut g.samples, data, channels.max(1), cap) {
                    g.truncated = true;
                }
            }
            // Never block the realtime callback: count the gap and report it.
            Err(_) => {
                counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        },
        err,
        None,
    )
}

/// Skipped input callbacks that mean audio is really being lost, not one callback that lost a
/// lock race with the UI's own short lock.
const SKIPPED_CALLBACKS_TO_REPORT: u64 = 8;

/// How long the skipped callbacks are counted over: a run inside this window is a real loss.
const SKIPPED_WINDOW: std::time::Duration = std::time::Duration::from_secs(1);

/// Ten minutes of mono audio at `rate`, so one slide cannot grow without bound. The limit follows
/// the device: a 96 kHz stream must not stop after five minutes.
fn max_samples(rate: u32) -> usize {
    (rate as usize).saturating_mul(60 * 10)
}

#[derive(Default)]
struct Capture {
    rate: u32,
    samples: Vec<f32>,
    /// The take reached its length limit and later samples were dropped.
    truncated: bool,
}

/// Default input device. The callback appends mono f32; the UI thread drains it.
pub struct CpalIn {
    stream: Option<cpal::Stream>,
    buf: Arc<Mutex<Capture>>,
    error: ErrorSlot,
    /// Callbacks that found the buffer locked and dropped their samples: a gap in the narration.
    /// The callback must never block on the UI thread's mutex, so it counts the gap here (a shared
    /// atomic) and `problem` reports it.
    skipped: Arc<std::sync::atomic::AtomicU64>,
    /// When the current skip-counting window began: one lost callback per poll is normal, a run
    /// of them inside a second is audio actually going missing.
    window: Option<std::time::Instant>,
}

impl Default for CpalIn {
    fn default() -> Self {
        CpalIn {
            stream: None,
            buf: Arc::new(Mutex::new(Capture::default())),
            error: ErrorSlot::default(),
            skipped: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            window: None,
        }
    }
}

impl CpalIn {
    fn report(&self) {
        if let Some(e) = take_stream_error(&self.error) {
            log::warn!("microphone stream error: {e}");
        }
    }
}

impl Drop for CpalIn {
    fn drop(&mut self) {
        self.stream = None;
        self.report();
    }
}

impl deckcraft_media::MicIn for CpalIn {
    fn start(&mut self) -> Result<(), String> {
        self.stop();
        let dev = cpal::default_host().default_input_device().ok_or_else(|| "no microphone".to_string())?;
        let cfg = dev.default_input_config().map_err(|e| e.to_string())?;
        let channels = cfg.channels() as usize;
        let rate = cfg.sample_rate().0;
        if !(8_000..=192_000).contains(&rate) {
            return Err(format!("microphone sample rate {rate} is not supported"));
        }
        {
            let mut g = self.buf.lock().unwrap_or_else(PoisonError::into_inner);
            g.rate = rate;
            g.samples.clear();
            g.truncated = false;
        }
        self.skipped.store(0, std::sync::atomic::Ordering::Relaxed);
        self.window = None;
        let config: cpal::StreamConfig = cfg.clone().into();
        let buf = self.buf.clone();
        let slot = self.error.clone();
        let counter = self.skipped.clone();
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => input_stream::<f32>(&dev, &config, channels, buf, counter, slot),
            cpal::SampleFormat::I16 => input_stream::<i16>(&dev, &config, channels, buf, counter, slot),
            cpal::SampleFormat::U16 => input_stream::<u16>(&dev, &config, channels, buf, counter, slot),
            cpal::SampleFormat::I32 => input_stream::<i32>(&dev, &config, channels, buf, counter, slot),
            cpal::SampleFormat::U32 => input_stream::<u32>(&dev, &config, channels, buf, counter, slot),
            cpal::SampleFormat::I8 => input_stream::<i8>(&dev, &config, channels, buf, counter, slot),
            cpal::SampleFormat::U8 => input_stream::<u8>(&dev, &config, channels, buf, counter, slot),
            cpal::SampleFormat::F64 => input_stream::<f64>(&dev, &config, channels, buf, counter, slot),
            cpal::SampleFormat::I64 => input_stream::<i64>(&dev, &config, channels, buf, counter, slot),
            cpal::SampleFormat::U64 => input_stream::<u64>(&dev, &config, channels, buf, counter, slot),
            other => return Err(format!("unsupported microphone format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        self.stream = Some(stream);
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

    fn problem(&mut self) -> Option<String> {
        if let Some(e) = take_stream_error(&self.error) {
            return Some(format!("the microphone stopped ({e})"));
        }
        // The UI's own short locks can make a callback skip, so one skip is not a failure: report
        // only a run of them inside one second, which means the capture is actually losing audio.
        // The count is not cleared here — one lost callback in most frames never tripped a
        // per-poll reset, yet adds up to half the audio.
        let now = std::time::Instant::now();
        let start = *self.window.get_or_insert(now);
        let skipped = self.skipped.load(std::sync::atomic::Ordering::Relaxed);
        if skipped >= SKIPPED_CALLBACKS_TO_REPORT {
            self.skipped.store(0, std::sync::atomic::Ordering::Relaxed);
            self.window = Some(now);
            return Some("some microphone audio was dropped while the recorder was busy".to_string());
        }
        if now.duration_since(start) >= SKIPPED_WINDOW {
            self.skipped.store(0, std::sync::atomic::Ordering::Relaxed);
            self.window = Some(now);
        }
        let mut g = self.buf.lock().unwrap_or_else(PoisonError::into_inner);
        std::mem::take(&mut g.truncated).then(|| "the take reached its ten minute limit, so later audio was dropped".to_string())
    }

    fn stop(&mut self) {
        self.stream = None;
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

    /// One skipped callback per UI poll is normal; the count survives the polls, so a steady loss
    /// that never exceeds the threshold in a single frame is still reported.
    #[test]
    fn skipped_callbacks_accumulate_across_polls() {
        let mut mic = CpalIn::default();
        for _ in 0..5 {
            mic.skipped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        assert_eq!(deckcraft_media::MicIn::problem(&mut mic), None, "five is below the threshold");
        assert_eq!(mic.skipped.load(std::sync::atomic::Ordering::Relaxed), 5, "and the count is not thrown away");
        for _ in 0..3 {
            mic.skipped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        assert!(deckcraft_media::MicIn::problem(&mut mic).is_some_and(|m| m.contains("dropped")));
        assert_eq!(mic.skipped.load(std::sync::atomic::Ordering::Relaxed), 0, "reported once, then a new window");
    }

    #[test]
    fn mono_downmix_averages_channels_and_stops_at_the_cap() {
        let mut out = Vec::new();
        push_mono_as::<f32>(&mut out, &[0.5, -0.5, 1.0, 1.0], 2, 10);
        assert_eq!(out, vec![0.0, 1.0]);
        push_mono_as::<f32>(&mut out, &[f32::NAN, 0.5], 2, 10);
        assert_eq!(out.len(), 3);
        assert!((out[2] - 0.5).abs() < 1e-6);
        let before = out.len();
        assert!(push_mono_as::<f32>(&mut out, &[0.25], 1, 3), "the cap dropped a sample, and says so");
        assert_eq!(out.len(), before);
        assert!(!push_mono_as::<f32>(&mut out, &[0.25], 1, 10), "room left: nothing dropped");
    }

    #[test]
    fn the_take_limit_follows_the_device_rate() {
        // Ten minutes at the device's own rate: a 96 kHz stream got half a take under the old
        // fixed 48 kHz cap.
        assert_eq!(max_samples(8_000), 4_800_000);
        assert_eq!(max_samples(48_000), 28_800_000);
        assert_eq!(max_samples(96_000), 57_600_000);
        assert_eq!(max_samples(192_000), 115_200_000);
    }
}
