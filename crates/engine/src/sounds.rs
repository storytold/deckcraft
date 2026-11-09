//! Built-in transition sounds, synthesised on demand so DeckCraft ships no sound files: short
//! mono 16-bit WAVs named like PowerPoint's (Chime, Click, Camera, Whoosh, Drum Roll, Laser).

use std::f64::consts::TAU;

/// (id, label) of the built-in sounds, in menu order.
pub const BUILTIN: [(&str, &str); 6] =
    [("chime", "Chime"), ("click", "Click"), ("camera", "Camera"), ("whoosh", "Whoosh"), ("drumRoll", "Drum Roll"), ("laser", "Laser")];

const RATE: u32 = 22_050;

/// The WAV bytes of built-in sound `id`, or `None` for an unknown id.
pub fn builtin(id: &str) -> Option<Vec<u8>> {
    let mut noise = Lcg(0x9E37_79B9);
    let samples: Vec<f64> = match id {
        "chime" => tone(1.2, |t| {
            let env = (-t * 3.0).exp() * attack(t, 0.005);
            env * (0.6 * (TAU * 880.0 * t).sin() + 0.3 * (TAU * 1318.5 * t).sin() + 0.15 * (TAU * 1760.0 * t).sin())
        }),
        "click" => tone(0.03, |t| (-t * 300.0).exp() * noise.next()),
        "camera" => tone(0.35, |t| {
            let a = (-t * 250.0).exp();
            let b = if t > 0.18 { (-(t - 0.18) * 180.0).exp() } else { 0.0 };
            (a + 0.8 * b) * noise.next()
        }),
        "whoosh" => {
            let mut lp = 0.0;
            tone(0.7, |t| {
                // Noise through a low-pass filter that opens and closes.
                let k = 0.02 + 0.25 * (std::f64::consts::PI * t / 0.7).sin();
                lp += k * (noise.next() - lp);
                3.0 * lp * (std::f64::consts::PI * t / 0.7).sin()
            })
        }
        "drumRoll" => tone(1.4, |t| {
            let beat = (t * 24.0).fract() / 24.0;
            let swell = 0.4 + 0.6 * (t / 1.4);
            swell * (-beat * 90.0).exp() * (0.7 * noise.next() + 0.5 * (TAU * 110.0 * t).sin())
        }),
        "laser" => {
            let mut phase = 0.0;
            tone(0.5, |t| {
                phase += TAU * (2400.0 * (-t * 5.0).exp() + 200.0) / RATE as f64;
                (-t * 4.0).exp() * attack(t, 0.003) * phase.sin().signum() * 0.35
            })
        }
        _ => return None,
    };
    Some(wav(&samples))
}

fn attack(t: f64, len: f64) -> f64 {
    (t / len).min(1.0)
}

fn tone(secs: f64, mut f: impl FnMut(f64) -> f64) -> Vec<f64> {
    let n = (secs * RATE as f64) as usize;
    (0..n).map(|i| f(i as f64 / RATE as f64)).collect()
}

/// A small deterministic noise source (-1..1).
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f64 / (1u32 << 23) as f64 - 1.0
    }
}

fn wav(samples: &[f64]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&RATE.to_le_bytes());
    v.extend_from_slice(&(RATE * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let x = (s.clamp(-1.0, 1.0) * 0.8 * i16::MAX as f64) as i16;
        v.extend_from_slice(&x.to_le_bytes());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_sound_is_a_playable_wav() {
        for (id, _) in BUILTIN {
            let bytes = builtin(id).unwrap_or_default();
            assert!(bytes.len() > 44, "{id}");
            let info = deckcraft_media::probe(&std::sync::Arc::new(bytes)).expect(id);
            assert!(info.audio.is_some() && info.playable(), "{id}");
            assert!(info.duration() > 0.01 && info.duration() < 2.0, "{id}: {}", info.duration());
        }
        assert!(builtin("nope").is_none());
    }
}
