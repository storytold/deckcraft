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

/// The audio waiting for the provider, with the sample count maintained on push and pop so the
/// realtime callback never sums the queue.
struct Queue {
    chunks: VecDeque<AudioBuffer>,
    total: usize,
}

impl Queue {
    fn new() -> Self {
        Queue { chunks: VecDeque::new(), total: 0 }
    }

    fn pop(&mut self) -> Option<AudioBuffer> {
        let chunk = self.chunks.pop_front()?;
        self.total = self.total.saturating_sub(chunk.samples.len());
        Some(chunk)
    }

    fn push(&mut self, chunk: AudioBuffer) {
        self.total = self.total.saturating_add(chunk.samples.len());
        self.chunks.push_back(chunk);
        while self.total > MAX_QUEUED_SAMPLES && self.chunks.len() > 1 {
            self.pop();
        }
    }
}

/// An open input device.
pub struct Capture {
    /// Kept so the stream plays; dropping it closes the device.
    _stream: cpal::Stream,
    queue: Arc<Mutex<Queue>>,
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
        let queue: Arc<Mutex<Queue>> = Arc::new(Mutex::new(Queue::new()));
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
            // A poisoned queue (a panic in the realtime callback) must not silently stop the
            // captions: the audio already queued is still valid, so recover the guard.
            if let Some(chunk) = self.queue.lock().unwrap_or_else(PoisonError::into_inner).pop() {
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
    queue: Arc<Mutex<Queue>>,
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
/// busy queue (the worker holds the lock for microseconds).
fn offer(queue: &Mutex<Queue>, chunk: AudioBuffer) {
    let Ok(mut q) = queue.try_lock() else { return };
    q.push(chunk);
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
    fn listing_devices_is_safe_and_sorted_without_duplicates() {
        // A machine with no input still answers with an empty list instead of failing.
        let names = input_devices();
        assert!(names.windows(2).all(|w| w[0] <= w[1]), "the list is sorted");
        assert!(names.iter().collect::<std::collections::HashSet<_>>().len() == names.len(), "the list has no duplicates");
    }

    #[test]
    fn the_queue_is_bounded_and_keeps_the_newest_audio() {
        let q: Mutex<Queue> = Mutex::new(Queue::new());
        let chunk = AudioBuffer::new(vec![0.0; MAX_QUEUED_SAMPLES / 4], 48_000, 2);
        for _ in 0..8 {
            offer(&q, chunk.clone());
        }
        let q = q.lock().unwrap_or_else(PoisonError::into_inner);
        assert!(q.total <= MAX_QUEUED_SAMPLES, "{}", q.total);
        assert_eq!(q.total, q.chunks.iter().map(|c| c.samples.len()).sum::<usize>(), "the running total matches the queue");
        assert!(q.chunks.len() < 8, "the oldest chunks go");
    }

    #[test]
    fn a_poisoned_queue_still_hands_back_its_audio() {
        let q: Arc<Mutex<Queue>> = Arc::new(Mutex::new(Queue::new()));
        q.lock().unwrap_or_else(PoisonError::into_inner).push(AudioBuffer::new(vec![0.5; 8], 16_000, 1));
        // Poison the mutex the way a panic in the realtime callback would.
        let poison = q.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poison.lock().expect("fresh lock");
            panic!("callback panic");
        })
        .join();
        assert!(q.is_poisoned(), "the test poisoned the queue");
        // `recv` needs a Capture; the recovery is the same guard, so assert it directly here.
        let chunk = q.lock().unwrap_or_else(PoisonError::into_inner).pop();
        assert_eq!(chunk.map(|c| c.samples.len()), Some(8), "the queued audio survives the poison");
    }

    #[test]
    fn opening_an_unknown_device_is_an_error_not_a_panic() {
        let devices = cpal::default_host().input_devices();
        if devices.is_err() {
            // No audio system at all: the device list itself is the only way to tell, and the
            // open path is covered by the same host call.
            return;
        }
        let Err(err) = Capture::open(Some("deckcraft-no-such-microphone")) else { panic!("an unknown microphone opened") };
        assert!(!err.message().is_empty(), "{err}");
    }
}
