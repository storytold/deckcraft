//! Psychoacoustic model operating on MDCT coefficients.
//!
//! Per scalefactor band: energy; tonality from the spectral flatness measure (SFM) of a
//! neighbourhood of ≥ 16 lines; masking-threshold PSD by spreading the band PSDs with Schroeder's
//! spreading function in the Bark domain; a tonality-dependent masking offset (≈18 dB for tonal,
//! ≈6 dB for noise-like maskers); the absolute threshold of hearing (Terhardt) as a floor; and
//! perceptual entropy (PE) as an estimate of the bits the frame needs.

use crate::tables::{swb_offsets_long, swb_offsets_short};

/// Precomputed per-band data for one window length.
pub struct BandModel {
    pub offsets: &'static [u16],
    /// Linear spreading factor `[maskee][masker]` (power).
    spread: Vec<Vec<f32>>,
    /// ATH as band energy in encoder spectrum units.
    ath: Vec<f32>,
    /// Neighbourhood `[lo, hi)` in lines used for the tonality estimate.
    tonal_span: Vec<(usize, usize)>,
}

fn bark(f: f64) -> f64 {
    13.0 * (0.00076 * f).atan() + 3.5 * (f / 7500.0).powi(2).atan()
}

/// Terhardt's threshold in quiet (dB SPL), capped at 40 dB so content near the band edge is not
/// discarded wholesale (the bandwidth setting limits the coded range instead).
fn ath_db(f: f64) -> f64 {
    let f = (f / 1000.0).max(0.02);
    (3.64 * f.powf(-0.8) - 6.5 * (-0.6 * (f - 3.3).powi(2)).exp() + 1e-3 * f.powi(4)).min(40.0)
}

impl BandModel {
    fn new(sample_rate: u32, offsets: &'static [u16], m: usize) -> BandModel {
        let nb = offsets.len() - 1;
        let hz_per_line = sample_rate as f64 / (2.0 * m as f64);
        let centre: Vec<f64> = (0..nb).map(|b| bark((offsets[b] as f64 + offsets[b + 1] as f64) * 0.5 * hz_per_line)).collect();
        let spread = (0..nb)
            .map(|i| {
                (0..nb)
                    .map(|j| {
                        let dz = centre[i] - centre[j] + 0.474;
                        let db = 15.81 + 7.5 * dz - 17.5 * (1.0 + dz * dz).sqrt();
                        if db < -60.0 { 0.0 } else { 10f64.powf(db / 10.0) as f32 }
                    })
                    .collect()
            })
            .collect();
        // A full-scale (16-bit) sine of power P_ref = 32768²/2 has MDCT energy ≈ 2·M²·P per frame
        // with the encoder's scaling; it is taken as 96 dB SPL. 3 dB of safety margin.
        let p_ref = 32768f64 * 32768.0 / 2.0;
        let ath = (0..nb)
            .map(|b| {
                let mut lo = f64::MAX;
                for k in offsets[b]..offsets[b + 1] {
                    lo = lo.min(ath_db((k as f64 + 0.5) * hz_per_line));
                }
                (2.0 * (m * m) as f64 * p_ref * 10f64.powf((lo - 96.0 - 3.0) / 10.0)) as f32
            })
            .collect();
        let tonal_span = (0..nb)
            .map(|b| {
                let (lo, hi) = (offsets[b] as usize, offsets[b + 1] as usize);
                if hi - lo >= 16 {
                    (lo, hi)
                } else {
                    let c = (lo + hi) / 2;
                    let lo = c.saturating_sub(8);
                    let hi = (lo + 16).min(m);
                    (hi.saturating_sub(16), hi)
                }
            })
            .collect();
        BandModel { offsets, spread, ath, tonal_span }
    }

    pub fn num_bands(&self) -> usize {
        self.offsets.len() - 1
    }
}

pub struct Psy {
    pub long: BandModel,
    pub short: BandModel,
}

/// Per-window, per-band analysis of one channel.
#[derive(Clone, Default)]
pub struct PsyResult {
    /// `[window][band]` energy.
    pub en: Vec<Vec<f32>>,
    /// `[window][band]` masking threshold (allowed noise energy).
    pub thr: Vec<Vec<f32>>,
    pub pe: f32,
}

impl Psy {
    pub fn new(sample_rate: u32, sf_index: u8) -> Psy {
        Psy {
            long: BandModel::new(sample_rate, swb_offsets_long(sf_index), 1024),
            short: BandModel::new(sample_rate, swb_offsets_short(sf_index), 128),
        }
    }

    /// Analyse a spectrum: 1024 lines (long) or 8×128 (short).
    pub fn analyse(&self, spec: &[f32], short: bool) -> PsyResult {
        let (model, nwin, wlen) = if short { (&self.short, 8, 128) } else { (&self.long, 1, 1024) };
        let nb = model.num_bands();
        let mut res = PsyResult { en: Vec::with_capacity(nwin), thr: Vec::with_capacity(nwin), pe: 0.0 };
        let mut psd = vec![0f32; nb];
        let mut tonality = vec![0f32; nb];
        for w in 0..nwin {
            let s = &spec[w * wlen..(w + 1) * wlen];
            let mut en = vec![0f32; nb];
            for b in 0..nb {
                let (lo, hi) = (model.offsets[b] as usize, model.offsets[b + 1] as usize);
                en[b] = s[lo..hi].iter().map(|v| v * v).sum();
                psd[b] = en[b] / (hi - lo) as f32;
                // spectral flatness over the neighbourhood
                let (tlo, thi) = model.tonal_span[b];
                let n = (thi - tlo) as f64;
                let mut sum = 0f64;
                let mut log_sum = 0f64;
                for &v in &s[tlo..thi] {
                    let p = v as f64 * v as f64 + 1e-3;
                    sum += p;
                    log_sum += p.ln();
                }
                let sfm_db = 10.0 * ((log_sum / n).exp() / (sum / n)).log10();
                // real-valued Gaussian noise gives ≈ -5.5 dB; a pure tone ≤ -25 dB
                tonality[b] = ((-sfm_db - 6.0) / 19.0).clamp(0.0, 1.0) as f32;
            }
            let mut thr = vec![0f32; nb];
            for b in 0..nb {
                let row = &model.spread[b];
                let mut sp = 0f32;
                for j in 0..nb {
                    sp += psd[j] * row[j];
                }
                let width = (model.offsets[b + 1] - model.offsets[b]) as f32;
                let offset_db = tonality[b] * 30.0 + (1.0 - tonality[b]) * 6.0;
                let t = sp * width * 10f32.powf(-offset_db / 10.0);
                thr[b] = t.max(model.ath[b]);
                if en[b] > thr[b] {
                    res.pe += width * 0.5 * (en[b] / thr[b]).log2();
                }
            }
            res.en.push(en);
            res.thr.push(thr);
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_is_masked_less_than_noise() {
        let psy = Psy::new(44100, 4);
        let tone: Vec<f32> = (0..1024).map(|k| if k == 100 { 1e6 } else { 0.0 }).collect();
        let mut seed = 1u32;
        let noise: Vec<f32> = (0..1024)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                ((seed >> 8) as f32 / (1 << 24) as f32 - 0.5) * 1e4
            })
            .collect();
        let rt = psy.analyse(&tone, false);
        let rn = psy.analyse(&noise, false);
        let b = psy.long.offsets.iter().position(|&o| o > 100).unwrap() - 1;
        assert!(rt.thr[0][b] < rt.en[0][b] / 30.0);
        assert!(rn.thr[0][b] > rn.en[0][b] / 10.0);
    }
}
