//! Temporal noise shaping (ISO/IEC 14496-3 §4.6.9): syntax, coefficient conversion, and the
//! decoder (all-pole) and encoder (all-zero) filters.

use std::f64::consts::FRAC_PI_2;

use deckcraft_bitstream::{BitReader, BitWriter};

use crate::ics::IcsInfo;
use crate::tables::{tns_max_bands, tns_max_order};
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TnsFilter {
    /// Length in scalefactor bands.
    pub length: u8,
    pub order: u8,
    /// false = upward, true = downward.
    pub direction: bool,
    pub coef_compress: bool,
    /// Quantised reflection coefficients (signed, `coef_res + 3 - coef_compress` bits each).
    pub coef: [i8; 12],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TnsWindow {
    pub n_filt: u8,
    /// false = 3-bit, true = 4-bit coefficient resolution.
    pub coef_res: bool,
    pub filt: [TnsFilter; 4],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TnsData {
    pub windows: [TnsWindow; 8],
}

impl TnsData {
    pub fn parse(br: &mut BitReader, info: &IcsInfo) -> Result<TnsData> {
        let short = info.is_short();
        let mut t = TnsData::default();
        for w in 0..info.num_windows() {
            let win = &mut t.windows[w];
            win.n_filt = br.read_bits(if short { 1 } else { 2 })? as u8;
            if win.n_filt == 0 {
                continue;
            }
            win.coef_res = br.read_bits(1)? == 1;
            for f in 0..win.n_filt as usize {
                let filt = &mut win.filt[f];
                filt.length = br.read_bits(if short { 4 } else { 6 })? as u8;
                filt.order = br.read_bits(if short { 3 } else { 5 })? as u8;
                if filt.order as usize > tns_max_order(short) {
                    return Err(Error::Bitstream("TNS order too large"));
                }
                if filt.order > 0 {
                    filt.direction = br.read_bits(1)? == 1;
                    filt.coef_compress = br.read_bits(1)? == 1;
                    let bits = 3 + win.coef_res as u32 - filt.coef_compress as u32;
                    for i in 0..filt.order as usize {
                        let v = br.read_bits(bits)? as i32;
                        let v = if v & (1 << (bits - 1)) != 0 { v - (1 << bits) } else { v };
                        filt.coef[i] = v as i8;
                    }
                }
            }
        }
        Ok(t)
    }

    pub fn write(&self, bw: &mut BitWriter, info: &IcsInfo) {
        let short = info.is_short();
        for w in 0..info.num_windows() {
            let win = &self.windows[w];
            bw.write_bits(win.n_filt as u32, if short { 1 } else { 2 });
            if win.n_filt == 0 {
                continue;
            }
            bw.write_bits(win.coef_res as u32, 1);
            for f in 0..win.n_filt as usize {
                let filt = &win.filt[f];
                bw.write_bits(filt.length as u32, if short { 4 } else { 6 });
                bw.write_bits(filt.order as u32, if short { 3 } else { 5 });
                if filt.order > 0 {
                    bw.write_bits(filt.direction as u32, 1);
                    bw.write_bits(filt.coef_compress as u32, 1);
                    let bits = 3 + win.coef_res as u32 - filt.coef_compress as u32;
                    for i in 0..filt.order as usize {
                        bw.write_bits(filt.coef[i] as i32 as u32 & ((1 << bits) - 1), bits);
                    }
                }
            }
        }
    }

    /// Bits `write` would produce.
    pub fn bit_count(&self, info: &IcsInfo) -> u32 {
        let short = info.is_short();
        let mut n = 0;
        for w in 0..info.num_windows() {
            let win = &self.windows[w];
            n += if short { 1 } else { 2 };
            if win.n_filt == 0 {
                continue;
            }
            n += 1;
            for f in 0..win.n_filt as usize {
                let filt = &win.filt[f];
                n += if short { 7 } else { 11 };
                if filt.order > 0 {
                    n += 2 + filt.order as u32 * (3 + win.coef_res as u32 - filt.coef_compress as u32);
                }
            }
        }
        n
    }
}

/// Dequantise one reflection coefficient.
pub fn dequant_coef(c: i8, coef_res: bool) -> f64 {
    let res = 3 + coef_res as i32;
    let iqfac = ((1 << (res - 1)) as f64 - 0.5) / FRAC_PI_2;
    let iqfac_m = ((1 << (res - 1)) as f64 + 0.5) / FRAC_PI_2;
    let c = c as f64;
    (c / if c >= 0.0 { iqfac } else { iqfac_m }).sin()
}

/// Quantise a reflection coefficient (inverse of [`dequant_coef`], nearest index).
pub fn quant_coef(k: f64, coef_res: bool) -> i8 {
    let res = 3 + coef_res as i32;
    let iqfac = ((1 << (res - 1)) as f64 - 0.5) / FRAC_PI_2;
    let iqfac_m = ((1 << (res - 1)) as f64 + 0.5) / FRAC_PI_2;
    let a = k.clamp(-1.0, 1.0).asin();
    let lo = -(1 << (res - 1));
    let hi = (1 << (res - 1)) - 1;
    let q = if a >= 0.0 { (a * iqfac).round() } else { (a * iqfac_m).round() };
    (q as i32).clamp(lo, hi) as i8
}

/// Reflection coefficients → direct-form LPC `a[0..=order]` (`a[0] = 1`).
pub fn parcor_to_lpc(k: &[f64]) -> [f64; 13] {
    let mut a = [0f64; 13];
    a[0] = 1.0;
    let mut b = [0f64; 13];
    for m in 1..=k.len() {
        for i in 1..m {
            b[i] = a[i] + k[m - 1] * a[m - i];
        }
        a[1..m].copy_from_slice(&b[1..m]);
        a[m] = k[m - 1];
    }
    a
}

/// Filter region `[start, end)` of every TNS filter of window `w`, in spectrum lines of that window.
fn regions(info: &IcsInfo, sf_index: u8, win: &TnsWindow) -> Vec<(usize, usize, usize)> {
    let short = info.is_short();
    let swb = info.swb_offsets(sf_index);
    let num_swb = swb.len() - 1;
    let lim = tns_max_bands(sf_index, short).min(info.max_sfb);
    let mut out = Vec::new();
    let mut bottom = num_swb;
    for f in 0..win.n_filt as usize {
        let filt = &win.filt[f];
        let top = bottom;
        bottom = top.saturating_sub(filt.length as usize);
        if filt.order == 0 {
            continue;
        }
        let start = swb[bottom.min(lim)] as usize;
        let end = swb[top.min(lim)] as usize;
        if end > start {
            out.push((f, start, end));
        }
    }
    out
}

fn lpc_of(filt: &TnsFilter, coef_res: bool) -> [f64; 13] {
    let k: Vec<f64> = filt.coef[..filt.order as usize].iter().map(|&c| dequant_coef(c, coef_res)).collect();
    parcor_to_lpc(&k)
}

/// Decoder: apply the all-pole TNS synthesis filters to `spec` (1024 lines; short windows at 128 stride).
pub fn apply_decoder(spec: &mut [f32], info: &IcsInfo, sf_index: u8, tns: &TnsData) {
    let wlen = if info.is_short() { 128 } else { 1024 };
    for w in 0..info.num_windows() {
        let win = &tns.windows[w];
        for (f, start, end) in regions(info, sf_index, win) {
            let filt = &win.filt[f];
            let a = lpc_of(filt, win.coef_res);
            let order = filt.order as usize;
            let s = &mut spec[w * wlen..(w + 1) * wlen];
            let size = end - start;
            let mut state = [0f64; 12];
            for m in 0..size {
                let idx = if filt.direction { end - 1 - m } else { start + m };
                let mut y = s[idx] as f64;
                for i in 0..order {
                    y -= a[i + 1] * state[i];
                }
                for i in (1..order).rev() {
                    state[i] = state[i - 1];
                }
                if order > 0 {
                    state[0] = y;
                }
                s[idx] = y as f32;
            }
        }
    }
}

/// Encoder: apply the all-zero TNS analysis filters (exact inverse of [`apply_decoder`]).
pub fn apply_encoder(spec: &mut [f32], info: &IcsInfo, sf_index: u8, tns: &TnsData) {
    let wlen = if info.is_short() { 128 } else { 1024 };
    for w in 0..info.num_windows() {
        let win = &tns.windows[w];
        for (f, start, end) in regions(info, sf_index, win) {
            let filt = &win.filt[f];
            let a = lpc_of(filt, win.coef_res);
            let order = filt.order as usize;
            let s = &mut spec[w * wlen..(w + 1) * wlen];
            let size = end - start;
            let mut state = [0f64; 12];
            for m in 0..size {
                let idx = if filt.direction { end - 1 - m } else { start + m };
                let x = s[idx] as f64;
                let mut y = x;
                for i in 0..order {
                    y += a[i + 1] * state[i];
                }
                for i in (1..order).rev() {
                    state[i] = state[i - 1];
                }
                if order > 0 {
                    state[0] = x;
                }
                s[idx] = y as f32;
            }
        }
    }
}

/// Levinson–Durbin on autocorrelation `r[0..=order]`: returns reflection coefficients and the
/// prediction gain `r[0] / error`.
pub fn levinson(r: &[f64], order: usize) -> (Vec<f64>, f64) {
    let mut a = vec![0f64; order + 1];
    a[0] = 1.0;
    let mut err = r[0];
    let mut ks = Vec::with_capacity(order);
    if err <= 0.0 {
        return (vec![0.0; order], 1.0);
    }
    for m in 1..=order {
        let mut acc = r[m];
        for i in 1..m {
            acc += a[i] * r[m - i];
        }
        let k = -acc / err;
        let prev = a.clone();
        for i in 1..m {
            a[i] = prev[i] + k * prev[m - i];
        }
        a[m] = k;
        err *= 1.0 - k * k;
        ks.push(k);
        if err <= r[0] * 1e-9 {
            ks.resize(order, 0.0);
            break;
        }
    }
    (ks, r[0] / err.max(r[0] * 1e-9))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ics::WindowSequence;
    use crate::mdct::WindowShape;

    #[test]
    fn coef_quant_roundtrip() {
        for res in [false, true] {
            let n = if res { 8 } else { 4 };
            for c in -n..n {
                let k = dequant_coef(c as i8, res);
                assert_eq!(quant_coef(k, res), c as i8);
            }
        }
    }

    #[test]
    fn encoder_decoder_filters_are_inverse() {
        let info = IcsInfo { window_sequence: WindowSequence::OnlyLong, window_shape: WindowShape::Sine, max_sfb: 49, ..Default::default() };
        let mut t = TnsData::default();
        t.windows[0].n_filt = 1;
        t.windows[0].coef_res = true;
        t.windows[0].filt[0] = TnsFilter { length: 40, order: 4, direction: true, coef_compress: false, coef: [3, -2, 1, 5, 0, 0, 0, 0, 0, 0, 0, 0] };
        let orig: Vec<f32> = (0..1024).map(|i| ((i * 37 % 101) as f32 - 50.0) * 0.1).collect();
        let mut s = orig.clone();
        apply_encoder(&mut s, &info, 4, &t);
        assert_ne!(s, orig);
        apply_decoder(&mut s, &info, 4, &t);
        for i in 0..1024 {
            assert!((s[i] - orig[i]).abs() < 1e-3);
        }
        let mut bw = BitWriter::new();
        t.write(&mut bw, &info);
        assert_eq!(bw.bit_len() as u32, t.bit_count(&info));
        let d = bw.finish();
        assert_eq!(TnsData::parse(&mut BitReader::new(&d), &info).unwrap(), t);
    }
}
