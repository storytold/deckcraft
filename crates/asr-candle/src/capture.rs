//! Microphone capture with cpal: the system default input, or a named device.
//!
//! The device's realtime callback only converts samples and offers them on a small bounded
//! queue; it never allocates unboundedly, blocks or logs. The provider thread drains the queue.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use cpal::SampleFormat;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use deckcraft_asr::{AsrError, AudioBuffer};

/// The most audio waiting for the provider, in samples (60 s of 48 kHz stereo). The worker decodes
/// on another thread, so the queue absorbs the whole decode; past the cap the oldest audio goes.
pub const MAX_QUEUED_SAMPLES: usize = 48_000 * 2 * 60;

type ErrorSlot = Arc<Mutex<Option<String>>>;

/// An open input device.
pub struct Capture {
    /// Kept so the stream plays; dropping it closes the device.
    _stream: cpal::Stream,
    queue: Arc<Mutex<VecDeque<AudioBuffer>>>,
    error: ErrorSlot,
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Capture {
    /// Open `name` (a device as [`input_devices`] spells it), or the system default when `None`.
    pub fn open(name: Option<&str>) -> Result<Capture, AsrError> {
        let host = cpal::default_host();
        let device = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(wanted) => host
                .input_devices()
                .map_err(|e| AsrError::new(format!("cannot list microphones: {e}")))?
                .find(|d| d.name().is_ok_and(|n| n == wanted))
                .ok_or_else(|| AsrError::new(format!("microphone `{wanted}` is not available")))?,
            None => host.default_input_device().ok_or_else(|| AsrError::new("no microphone"))?,
        };
        let device_name = device.name().unwrap_or_else(|_| "Microphone".to_string());
        let supported = device.default_input_config().map_err(|e| AsrError::new(format!("{device_name}: {e}")))?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.clone().into();
        let (sample_rate, channels) = (config.sample_rate.0, config.channels);
        let queue: Arc<Mutex<VecDeque<AudioBuffer>>> = Arc::default();
        let error: ErrorSlot = Arc::default();
        let slot = error.clone();
        let err = move |e: cpal::StreamError| {
            if let Ok(mut kept) = slot.try_lock()
                && kept.is_none()
            {
                *kept = Some(e.to_string());
            }
        };
        let stream = build(&device, &config, format, queue.clone(), sample_rate, channels, err)?;
        stream.play().map_err(|e| AsrError::new(format!("{device_name}: {e}")))?;
        Ok(Capture { _stream: stream, queue, error, device: device_name, sample_rate, channels })
    }

    /// The next chunk, waiting at most `timeout` (zero returns what is queued right now).
    pub fn recv(&self, timeout: Duration) -> Option<AudioBuffer> {
        let start = Instant::now();
        loop {
            if let Ok(mut q) = self.queue.lock()
                && let Some(chunk) = q.pop_front()
            {
                return Some(chunk);
            }
            if start.elapsed() >= timeout {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// The first stream error since it was last reported.
    pub fn take_error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

fn build(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: SampleFormat,
    queue: Arc<Mutex<VecDeque<AudioBuffer>>>,
    sample_rate: u32,
    channels: u16,
    err: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, AsrError> {
    macro_rules! stream {
        ($ty:ty, $convert:expr) => {{
            let convert = $convert;
            device.build_input_stream(
                config,
                move |data: &[$ty], _| {
                    let samples: Vec<f32> = data.iter().map(|s| convert(*s)).collect();
                    offer(&queue, AudioBuffer::new(samples, sample_rate, channels));
                },
                err,
                None,
            )
        }};
    }
    let stream = match format {
        SampleFormat::F32 => stream!(f32, |s: f32| s),
        SampleFormat::F64 => stream!(f64, |s: f64| s as f32),
        SampleFormat::I16 => stream!(i16, |s: i16| s as f32 / 32_768.0),
        SampleFormat::I32 => stream!(i32, |s: i32| s as f32 / 2_147_483_648.0),
        SampleFormat::I8 => stream!(i8, |s: i8| s as f32 / 128.0),
        SampleFormat::U16 => stream!(u16, |s: u16| (s as f32 - 32_768.0) / 32_768.0),
        SampleFormat::U8 => stream!(u8, |s: u8| (s as f32 - 128.0) / 128.0),
        other => return Err(AsrError::new(format!("the microphone's sample format ({other:?}) is not supported"))),
    };
    stream.map_err(|e| AsrError::new(format!("cannot open the microphone: {e}")))
}

/// Add a chunk to the queue, dropping the oldest audio when the cap is reached. Never blocks a
/// busy queue (the worker holds the lock for microseconds) and never allocates.
fn offer(queue: &Mutex<VecDeque<AudioBuffer>>, chunk: AudioBuffer) {
    let Ok(mut q) = queue.try_lock() else { return };
    q.push_back(chunk);
    let mut total: usize = q.iter().map(|c| c.samples.len()).sum();
    while total > MAX_QUEUED_SAMPLES && q.len() > 1 {
        if let Some(old) = q.pop_front() {
            total = total.saturating_sub(old.samples.len());
        }
    }
}

/// Every input device the host reports, by name.
pub fn input_devices() -> Vec<String> {
    let Ok(devices) = cpal::default_host().input_devices() else { return Vec::new() };
    let mut names: Vec<String> = devices.filter_map(|d| d.name().ok()).collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn listing_devices_is_safe_without_an_audio_system() {
        // A machine with no input still answers with an empty list instead of failing.
        let _ = input_devices();
    }

    #[test]
    fn the_queue_is_bounded_and_keeps_the_newest_audio() {
        let q: Mutex<VecDeque<AudioBuffer>> = Mutex::new(VecDeque::new());
        let chunk = AudioBuffer::new(vec![0.0; MAX_QUEUED_SAMPLES / 4], 48_000, 2);
        for _ in 0..8 {
            offer(&q, chunk.clone());
        }
        let q = q.lock().unwrap_or_else(PoisonError::into_inner);
        let total: usize = q.iter().map(|c| c.samples.len()).sum();
        assert!(total <= MAX_QUEUED_SAMPLES, "{total}");
        assert!(q.len() < 8, "the oldest chunks go");
    }

    #[test]
    fn opening_an_unknown_device_is_an_error_not_a_panic() {
        // A machine with no audio system at all may refuse the default device first, so only an
        // error's content is checked here.
        if let Err(e) = Capture::open(Some("deckcraft-no-such-microphone")) {
            assert!(!e.message().is_empty(), "{e}");
        }
    }
}
