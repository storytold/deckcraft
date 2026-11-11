//! Microphone audio: interleaved `f32` buffers, a channel downmix and resampling to the
//! 16 kHz mono every speech model in the manifest takes.
//!
//! A capture device reports whatever it likes — 44.1 kHz stereo, 48 kHz mono, a buffer of
//! non-finite samples — so every conversion clamps to a range and every length is capped.

/// The sample rate speech models expect.
pub const TARGET_RATE: u32 = 16_000;

/// The longest buffer a single conversion keeps: one hour at [`TARGET_RATE`]. Longer input is
/// truncated rather than allocated.
pub const MAX_SAMPLES: usize = TARGET_RATE as usize * 60 * 60;

/// A block of captured audio. `samples` are interleaved by [`AudioBuffer::channels`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl AudioBuffer {
    pub fn new(samples: Vec<f32>, sample_rate: u32, channels: u16) -> Self {
        AudioBuffer { samples, sample_rate, channels }
    }

    /// The frames (one sample per channel) the buffer holds.
    pub fn frames(&self) -> usize {
        let ch = usize::from(self.channels.max(1));
        self.samples.len() / ch
    }

    /// The buffer as mono, averaging the channels (and clamping anything not finite).
    pub fn mono(&self) -> Vec<f32> {
        downmix(&self.samples, self.channels)
    }

    /// The buffer as 16 kHz mono, ready for [`crate::provider`].
    pub fn mono_16k(&self) -> Vec<f32> {
        resample_linear(&self.mono(), self.sample_rate, TARGET_RATE)
    }
}

/// Average interleaved channels into one. A zero channel count has no samples to average, so
/// it yields nothing; a trailing partial frame contributes the channels it has.
pub fn downmix(samples: &[f32], channels: u16) -> Vec<f32> {
    let ch = usize::from(channels);
    let take = samples.len().min(MAX_SAMPLES.saturating_mul(ch.max(1)));
    let samples = &samples[..take];
    match ch {
        0 => Vec::new(),
        1 => samples.iter().map(|s| clean(*s)).collect(),
        _ => samples.chunks(ch).map(|frame| clean(frame.iter().copied().sum::<f32>() / frame.len().max(1) as f32)).collect(),
    }
}

/// Resample mono audio by linear interpolation. Same-rate input comes back unchanged; either
/// rate being zero yields nothing, and the result never exceeds [`MAX_SAMPLES`].
pub fn resample_linear(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == 0 || to == 0 || samples.is_empty() {
        return Vec::new();
    }
    if from == to {
        return samples.iter().take(MAX_SAMPLES).map(|s| clean(*s)).collect();
    }
    let ratio = f64::from(to) / f64::from(from);
    let out_len = ((samples.len() as f64) * ratio).round().max(0.0);
    let out_len = if out_len.is_finite() { (out_len as usize).min(MAX_SAMPLES) } else { 0 };
    let step = f64::from(from) / f64::from(to);
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * step;
        let i0 = pos.floor() as usize;
        let frac = (pos - i0 as f64) as f32;
        let a = samples.get(i0).copied().unwrap_or(0.0);
        let b = samples.get(i0 + 1).copied().unwrap_or(a);
        out.push(clean(a + (b - a) * frac));
    }
    out
}

fn clean(v: f32) -> f32 {
    if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn stereo_is_averaged() {
        assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(downmix(&[1.0, 0.0, 0.5], 2), vec![0.5, 0.5], "a trailing half frame averages what it has");
        assert_eq!(downmix(&[0.25, -0.25], 1), vec![0.25, -0.25]);
    }

    #[test]
    fn hostile_buffers_stay_quiet() {
        assert!(downmix(&[], 0).is_empty());
        assert_eq!(downmix(&[f32::NAN, f32::INFINITY, 2.0, -3.0], 2), vec![0.0, -0.5]);
        let b = AudioBuffer::new(vec![0.0; 8], 0, 2);
        assert_eq!(b.frames(), 4);
        assert!(b.mono_16k().is_empty(), "a zero sample rate has no time base");
        assert_eq!(resample_linear(&[1.0, 2.0], 0, TARGET_RATE), Vec::<f32>::new());
        assert_eq!(resample_linear(&[1.0, 2.0], TARGET_RATE, 0), Vec::<f32>::new());
    }

    #[test]
    fn forty_eight_kilohertz_becomes_sixteen() {
        let n = 48_000;
        let samples: Vec<f32> = (0..n).map(|i| (i as f32 / 4800.0).sin()).collect();
        let out = resample_linear(&samples, 48_000, TARGET_RATE);
        assert_eq!(out.len(), 16_000);
        assert!((out[0] - samples[0]).abs() < 1e-6);
    }

    #[test]
    fn forty41_kilohertz_keeps_its_duration() {
        let n = 44_100;
        let out = resample_linear(&vec![0.5; n], 44_100, TARGET_RATE);
        assert_eq!(out.len(), 16_000);
        assert!(out.iter().all(|s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn frame_count_follows_the_channel_count() {
        let b = AudioBuffer::new(vec![0.0; 32], 16_000, 2);
        assert_eq!(b.frames(), 16);
        let mono = AudioBuffer::new(vec![0.0; 31], 16_000, 4);
        assert_eq!(mono.frames(), 7, "a trailing partial frame is not a frame");
        assert_eq!(AudioBuffer::new(vec![0.0; 31], 16_000, 0).frames(), 31);
    }

    #[test]
    fn conversions_are_bounded() {
        let huge = vec![0.0; MAX_SAMPLES + 100];
        assert!(resample_linear(&huge, 16_000, 8_000).len() <= MAX_SAMPLES);
        assert!(downmix(&huge, 1).len() <= MAX_SAMPLES);
    }
}
