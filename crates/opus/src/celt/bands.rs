//! Band shape decoding: PVQ, spreading, TF changes, band splitting, stereo, folding
//! (RFC 6716 §4.3.4 – §4.3.6).

use super::rate::{Mode, QTHETA_OFFSET, QTHETA_OFFSET_TWOPHASE, get_pulses};
use super::tables::{E_MEANS, EBANDS, NB_EBANDS, ORDERY, SHORT_MDCT};
use crate::range::{BITRES, RangeDecoder, ilog};

pub const SPREAD_NONE: usize = 0;
pub const SPREAD_AGGRESSIVE: usize = 3;

#[inline]
pub fn lcg_rand(seed: u32) -> u32 {
    seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

/// Decodes a PVQ codeword of `k` pulses in `n` dimensions; returns the sum of squares.
fn decode_pulses(y: &mut [i32], n: usize, k: usize, dec: &mut RangeDecoder, v: &mut Vec<u64>) -> f32 {
    // Table of V(nn, kk) for nn in 0..=n, kk in 0..=k (saturating).
    let w = k + 1;
    v.clear();
    v.resize((n + 1) * w, 0);
    v[0] = 1;
    for nn in 1..=n {
        v[nn * w] = 1;
        for kk in 1..=k {
            let a = v[(nn - 1) * w + kk];
            let b = v[nn * w + kk - 1];
            let c = v[(nn - 1) * w + kk - 1];
            v[nn * w + kk] = a.saturating_add(b).saturating_add(c);
        }
    }
    let total = v[n * w + k];
    let ft = total.min(u32::MAX as u64) as u32;
    let mut i = if ft >= 2 { dec.uint(ft) as u64 } else { 0 };
    let mut kk = k;
    let mut ryy = 0f32;
    for j in 0..n {
        let rem = n - j;
        if kk == 0 {
            y[j] = 0;
            continue;
        }
        let vv = |nn: usize, kx: usize| v[nn * w + kx];
        let mut p = (vv(rem - 1, kk) + vv(rem, kk)) / 2;
        let neg = i >= p;
        if neg {
            i -= p;
        }
        let k0 = kk;
        p -= vv(rem - 1, kk);
        while p > i && kk > 0 {
            kk -= 1;
            p -= vv(rem - 1, kk);
        }
        let mag = (k0 - kk) as i32;
        y[j] = if neg { -mag } else { mag };
        i -= p.min(i);
        ryy += (mag * mag) as f32;
    }
    ryy
}

fn exp_rotation1(x: &mut [f32], len: usize, stride: usize, c: f32, s: f32) {
    let ms = -s;
    if len <= stride {
        return;
    }
    for i in 0..len - stride {
        let x1 = x[i];
        let x2 = x[i + stride];
        x[i + stride] = c * x2 + s * x1;
        x[i] = c * x1 + ms * x2;
    }
    if len > 2 * stride {
        for i in (0..=len - 2 * stride - 1).rev() {
            let x1 = x[i];
            let x2 = x[i + stride];
            x[i + stride] = c * x2 + s * x1;
            x[i] = c * x1 + ms * x2;
        }
    }
}

/// Inverse spreading rotation (decoder direction).
fn exp_rotation(x: &mut [f32], len: usize, stride: usize, k: usize, spread: usize) {
    const SPREAD_FACTOR: [usize; 3] = [15, 10, 5];
    if 2 * k >= len || spread == SPREAD_NONE {
        return;
    }
    let factor = SPREAD_FACTOR[spread - 1];
    let gain = len as f32 / (len + factor * k) as f32;
    let theta = 0.5 * gain * gain;
    let c = (0.5 * std::f32::consts::PI * theta).cos();
    let s = (0.5 * std::f32::consts::PI * (1.0 - theta)).cos();
    let mut stride2 = 0;
    if len >= 8 * stride {
        stride2 = 1;
        while (stride2 * stride2 + stride2) * stride + (stride >> 2) < len {
            stride2 += 1;
        }
    }
    let sub = len / stride;
    for i in 0..stride {
        let xs = &mut x[i * sub..(i + 1) * sub];
        if stride2 != 0 {
            exp_rotation1(xs, sub, stride2, s, c);
        }
        exp_rotation1(xs, sub, 1, c, s);
    }
}

fn extract_collapse_mask(iy: &[i32], n: usize, b: usize) -> u32 {
    if b <= 1 {
        return 1;
    }
    let n0 = n / b;
    let mut mask = 0u32;
    for i in 0..b {
        if iy[i * n0..(i + 1) * n0].iter().any(|&v| v != 0) {
            mask |= 1 << i;
        }
    }
    mask
}

pub fn renormalise_vector(x: &mut [f32], gain: f32) {
    let e: f32 = 1e-15 + x.iter().map(|v| v * v).sum::<f32>();
    let g = gain / e.sqrt();
    for v in x.iter_mut() {
        *v *= g;
    }
}

fn haar1(x: &mut [f32], n0: usize, stride: usize) {
    let n0 = n0 >> 1;
    const S: f32 = std::f32::consts::FRAC_1_SQRT_2;
    for i in 0..stride {
        for j in 0..n0 {
            let a = x[stride * 2 * j + i] * S;
            let b = x[stride * (2 * j + 1) + i] * S;
            x[stride * 2 * j + i] = a + b;
            x[stride * (2 * j + 1) + i] = a - b;
        }
    }
}

fn ordery(stride: usize) -> &'static [usize] {
    &ORDERY[stride - 2..2 * stride - 2]
}

fn deinterleave_hadamard(x: &mut [f32], n0: usize, stride: usize, hadamard: bool, tmp: &mut Vec<f32>) {
    let n = n0 * stride;
    tmp.clear();
    tmp.resize(n, 0.0);
    if hadamard {
        let ord = ordery(stride);
        for i in 0..stride {
            for j in 0..n0 {
                tmp[ord[i] * n0 + j] = x[j * stride + i];
            }
        }
    } else {
        for i in 0..stride {
            for j in 0..n0 {
                tmp[i * n0 + j] = x[j * stride + i];
            }
        }
    }
    x[..n].copy_from_slice(tmp);
}

fn interleave_hadamard(x: &mut [f32], n0: usize, stride: usize, hadamard: bool, tmp: &mut Vec<f32>) {
    let n = n0 * stride;
    tmp.clear();
    tmp.resize(n, 0.0);
    if hadamard {
        let ord = ordery(stride);
        for i in 0..stride {
            for j in 0..n0 {
                tmp[j * stride + i] = x[ord[i] * n0 + j];
            }
        }
    } else {
        for i in 0..stride {
            for j in 0..n0 {
                tmp[j * stride + i] = x[i * n0 + j];
            }
        }
    }
    x[..n].copy_from_slice(tmp);
}

#[inline]
fn frac_mul16(a: i32, b: i32) -> i32 {
    (16384 + (a as i16 as i32) * (b as i16 as i32)) >> 15
}

fn bitexact_cos(x: i32) -> i32 {
    let tmp = (4096 + x * x) >> 13;
    let x2 = tmp;
    let x2 = (32767 - x2) + frac_mul16(x2, -7651 + frac_mul16(x2, 8277 + frac_mul16(-626, x2)));
    1 + x2
}

fn bitexact_log2tan(isin: i32, icos: i32) -> i32 {
    let lc = ilog(icos as u32);
    let ls = ilog(isin as u32);
    let icos = icos << (15 - lc);
    let isin = isin << (15 - ls);
    (ls - lc) * (1 << 11) + frac_mul16(isin, frac_mul16(isin, -2597) + 7932) - frac_mul16(icos, frac_mul16(icos, -2597) + 7932)
}

fn isqrt32(v: u32) -> u32 {
    let mut r = (v as f64).sqrt() as u32;
    while (r as u64) * (r as u64) > v as u64 {
        r -= 1;
    }
    while ((r + 1) as u64) * ((r + 1) as u64) <= v as u64 {
        r += 1;
    }
    r
}

fn compute_qn(n: i32, b: i32, offset: i32, pulse_cap: i32, stereo: bool) -> i32 {
    const EXP2_TABLE8: [i32; 8] = [16384, 17866, 19483, 21247, 23170, 25267, 27554, 30048];
    let mut n2 = 2 * n - 1;
    if stereo && n == 2 {
        n2 -= 1;
    }
    let mut qb = sdiv(b + n2 * offset, n2);
    qb = qb.min(b - pulse_cap - (4 << BITRES));
    qb = qb.min(8 << BITRES);
    if qb < (1 << BITRES >> 1) {
        1
    } else {
        let qn = EXP2_TABLE8[(qb & 0x7) as usize] >> (14 - (qb >> BITRES));
        (qn + 1) >> 1 << 1
    }
}

/// C-style truncating signed division.
#[inline]
fn sdiv(a: i32, b: i32) -> i32 {
    a / b
}

pub struct BandCtx<'m, 'd, 'b> {
    pub m: &'m Mode,
    pub dec: &'d mut RangeDecoder<'b>,
    pub i: usize,
    pub intensity: usize,
    pub spread: usize,
    pub tf_change: i32,
    pub remaining_bits: i32,
    pub seed: u32,
    pub disable_inv: bool,
    pub avoid_split_noise: bool,
    pub pvq_v: Vec<u64>,
    pub iy: Vec<i32>,
    pub tmp: Vec<f32>,
}

struct Split {
    inv: bool,
    imid: i32,
    iside: i32,
    delta: i32,
    itheta: i32,
    qalloc: i32,
}

/// Block geometry for one theta decode: `n` samples, `bb` blocks per band, `b0` initial block count.
#[derive(Clone, Copy)]
struct ThetaShape {
    n: i32,
    bb: usize,
    b0: usize,
    lm: i32,
    stereo: bool,
}

/// Mid/side gains and delta for a decoded angle; also narrows `fill` to the blocks that stay coded.
fn split_gains(itheta: i32, n: i32, bb: usize, fill: &mut u32) -> (i32, i32, i32) {
    if itheta == 0 {
        *fill &= (1 << bb) - 1;
        (32767, 0, -16384)
    } else if itheta == 16384 {
        *fill &= ((1 << bb) - 1) << bb;
        (0, 32767, 16384)
    } else {
        let imid = bitexact_cos(itheta);
        let iside = bitexact_cos(16384 - itheta);
        (imid, iside, frac_mul16((n - 1) << 7, bitexact_log2tan(iside, imid)))
    }
}

impl BandCtx<'_, '_, '_> {
    fn compute_theta(&mut self, shape: ThetaShape, b: &mut i32, fill: &mut u32) -> Split {
        let ThetaShape { n, bb, b0, lm, stereo } = shape;
        let i = self.i;
        let pulse_cap = self.m.log_n[i] + lm * (1 << BITRES);
        let offset = (pulse_cap >> 1) - if stereo && n == 2 { QTHETA_OFFSET_TWOPHASE } else { QTHETA_OFFSET };
        let mut qn = compute_qn(n, *b, offset, pulse_cap, stereo);
        if stereo && i >= self.intensity {
            qn = 1;
        }
        let tell = self.dec.tell_frac() as i32;
        let mut itheta = 0i32;
        let mut inv = false;
        if qn != 1 {
            itheta = self.decode_itheta(qn, n, b0, stereo);
        } else if stereo {
            inv = self.decode_inversion(*b);
        }
        let qalloc = self.dec.tell_frac() as i32 - tell;
        *b -= qalloc;
        let (imid, iside, delta) = split_gains(itheta, n, bb, fill);
        Split { inv, imid, iside, delta, itheta, qalloc }
    }

    /// Decodes the quantised stereo/band angle for `qn > 1` and scales it to the 0..=16384 range.
    fn decode_itheta(&mut self, qn: i32, n: i32, b0: usize, stereo: bool) -> i32 {
        let itheta = if stereo && n > 2 {
            self.decode_itheta_stereo_triangle(qn)
        } else if b0 > 1 || stereo {
            self.dec.uint(qn as u32 + 1) as i32
        } else {
            self.decode_itheta_triangle(qn)
        };
        ((itheta as u32 * 16384) / qn as u32) as i32
    }

    fn decode_itheta_stereo_triangle(&mut self, qn: i32) -> i32 {
        let p0 = 3u32;
        let x0 = (qn / 2) as u32;
        let ft = p0 * (x0 + 1) + x0;
        let fs = self.dec.decode(ft);
        let x = if fs < (x0 + 1) * p0 { fs / p0 } else { x0 + 1 + (fs - (x0 + 1) * p0) };
        let (fl, fh) = if x <= x0 { (p0 * x, p0 * (x + 1)) } else { ((x - 1 - x0) + (x0 + 1) * p0, (x - x0) + (x0 + 1) * p0) };
        self.dec.update(fl, fh, ft);
        x as i32
    }

    fn decode_itheta_triangle(&mut self, qn: i32) -> i32 {
        let half = (qn >> 1) as u32;
        let ft = (half + 1) * (half + 1);
        let fm = self.dec.decode(ft);
        let (fl, fs, itheta);
        if fm < (half * (half + 1)) >> 1 {
            let it = (isqrt32(8 * fm + 1) - 1) >> 1;
            fs = it + 1;
            fl = it * (it + 1) >> 1;
            itheta = it as i32;
        } else {
            let q = qn as u32;
            let it = (2 * (q + 1) - isqrt32(8 * (ft - fm - 1) + 1)) >> 1;
            fs = q + 1 - it;
            fl = ft - ((q + 1 - it) * (q + 2 - it) >> 1);
            itheta = it as i32;
        }
        self.dec.update(fl, fl + fs, ft);
        itheta
    }

    /// Reads the stereo inversion flag when there are enough bits left (never when inversion is disabled).
    fn decode_inversion(&mut self, b: i32) -> bool {
        let inv = b > 2 << BITRES && self.remaining_bits > 2 << BITRES && self.dec.bit_logp(2);
        inv && !self.disable_inv
    }

    fn quant_band_n1(&mut self, x: &mut [f32], y: Option<&mut [f32]>, lowband_out: Option<&mut [f32]>) -> u32 {
        let dec_sign = |ctx: &mut Self| -> f32 {
            let mut sign = 0;
            if ctx.remaining_bits >= 1 << BITRES {
                sign = ctx.dec.bits(1);
                ctx.remaining_bits -= 1 << BITRES;
            }
            if sign != 0 { -1.0 } else { 1.0 }
        };
        x[0] = dec_sign(self);
        if let Some(y) = y {
            y[0] = dec_sign(self);
        }
        if let Some(lo) = lowband_out {
            lo[0] = x[0];
        }
        1
    }

    #[allow(clippy::too_many_arguments)]
    fn quant_partition(
        &mut self,
        x: &mut [f32],
        n: usize,
        mut b: i32,
        mut bb: usize,
        lowband: Option<&[f32]>,
        mut lm: i32,
        gain: f32,
        mut fill: u32,
    ) -> u32 {
        let b0 = bb;
        let i = self.i;
        let cache = self.m.cache(lm, i);
        let cache_max = cache[cache[0] as usize] as i32;
        if lm != -1 && b > cache_max + 12 && n > 2 {
            let n = n >> 1;
            let (xa, ya) = x.split_at_mut(n);
            lm -= 1;
            if bb == 1 {
                fill = (fill & 1) | (fill << 1);
            }
            bb = (bb + 1) >> 1;
            let sp = self.compute_theta(ThetaShape { n: n as i32, bb, b0, lm, stereo: false }, &mut b, &mut fill);
            let mid = sp.imid as f32 / 32768.0;
            let side = sp.iside as f32 / 32768.0;
            let mut delta = sp.delta;
            if b0 > 1 && (sp.itheta & 0x3fff) != 0 {
                if sp.itheta > 8192 {
                    delta -= delta >> (4 - lm);
                } else {
                    delta = 0.min(delta + ((n as i32) << BITRES >> (5 - lm)));
                }
            }
            let mut mbits = 0.max(b.min((b - delta) / 2));
            let mut sbits = b - mbits;
            self.remaining_bits -= sp.qalloc;
            let next_lowband2 = lowband.map(|l| &l[n..]);
            let mut rebalance = self.remaining_bits;
            let mut cm;
            if mbits >= sbits {
                cm = self.quant_partition(xa, n, mbits, bb, lowband, lm, gain * mid, fill);
                rebalance = mbits - (rebalance - self.remaining_bits);
                if rebalance > 3 << BITRES && sp.itheta != 0 {
                    sbits += rebalance - (3 << BITRES);
                }
                cm |= self.quant_partition(ya, n, sbits, bb, next_lowband2, lm, gain * side, fill >> bb) << (b0 >> 1);
            } else {
                cm = self.quant_partition(ya, n, sbits, bb, next_lowband2, lm, gain * side, fill >> bb) << (b0 >> 1);
                rebalance = sbits - (rebalance - self.remaining_bits);
                if rebalance > 3 << BITRES && sp.itheta != 16384 {
                    mbits += rebalance - (3 << BITRES);
                }
                cm |= self.quant_partition(xa, n, mbits, bb, lowband, lm, gain * mid, fill);
            }
            return cm;
        }
        // No split.
        let m = self.m;
        let mut q = m.bits2pulses(i, lm, b);
        let mut curr_bits = m.pulses2bits(i, lm, q);
        self.remaining_bits -= curr_bits;
        while self.remaining_bits < 0 && q > 0 {
            self.remaining_bits += curr_bits;
            q -= 1;
            curr_bits = m.pulses2bits(i, lm, q);
            self.remaining_bits -= curr_bits;
        }
        if q != 0 {
            let k = get_pulses(q) as usize;
            let mut iy = std::mem::take(&mut self.iy);
            iy.clear();
            iy.resize(n, 0);
            let ryy = decode_pulses(&mut iy, n, k, self.dec, &mut self.pvq_v);
            let g = gain / ryy.sqrt();
            for (xv, &yv) in x[..n].iter_mut().zip(iy.iter()) {
                *xv = g * yv as f32;
            }
            exp_rotation(&mut x[..n], n, bb, k, self.spread);
            let cm = extract_collapse_mask(&iy, n, bb);
            self.iy = iy;
            cm
        } else {
            let cm_mask = (1u32 << bb) - 1;
            fill &= cm_mask;
            if fill == 0 {
                x[..n].fill(0.0);
                0
            } else {
                let cm = match lowband {
                    None => {
                        for v in x[..n].iter_mut() {
                            self.seed = lcg_rand(self.seed);
                            *v = ((self.seed as i32) >> 20) as f32;
                        }
                        cm_mask
                    }
                    Some(lb) => {
                        for j in 0..n {
                            self.seed = lcg_rand(self.seed);
                            let tmp = if self.seed & 0x8000 != 0 { 1.0 / 256.0 } else { -1.0 / 256.0 };
                            x[j] = lb[j] + tmp;
                        }
                        fill
                    }
                };
                renormalise_vector(&mut x[..n], gain);
                cm
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn quant_band(
        &mut self,
        x: &mut [f32],
        n: usize,
        b: i32,
        mut bb: usize,
        mut lowband: Option<&mut [f32]>,
        lm: i32,
        lowband_out: Option<&mut [f32]>,
        gain: f32,
        mut fill: u32,
    ) -> u32 {
        let n0 = n;
        let mut n_b = n;
        let b0_orig = bb;
        let mut time_divide = 0;
        let mut recombine = 0;
        let long_blocks = b0_orig == 1;
        let mut tf_change = self.tf_change;
        n_b /= bb;
        if n == 1 {
            return self.quant_band_n1(x, None, lowband_out);
        }
        if tf_change > 0 {
            recombine = tf_change;
        }
        const BIT_INTERLEAVE: [u32; 16] = [0, 1, 1, 1, 2, 3, 3, 3, 2, 3, 3, 3, 2, 3, 3, 3];
        for k in 0..recombine {
            if let Some(lb) = lowband.as_deref_mut() {
                haar1(lb, n >> k, 1 << k);
            }
            fill = BIT_INTERLEAVE[(fill & 0xF) as usize] | (BIT_INTERLEAVE[(fill >> 4) as usize] << 2);
        }
        bb >>= recombine;
        n_b <<= recombine;
        while (n_b & 1) == 0 && tf_change < 0 {
            if let Some(lb) = lowband.as_deref_mut() {
                haar1(lb, n_b, bb);
            }
            fill |= fill << bb;
            bb <<= 1;
            n_b >>= 1;
            time_divide += 1;
            tf_change += 1;
        }
        let b0 = bb;
        let n_b0 = n_b;
        let mut tmp = std::mem::take(&mut self.tmp);
        if b0 > 1
            && let Some(lb) = lowband.as_deref_mut()
        {
            deinterleave_hadamard(lb, n_b >> recombine, b0 << recombine, long_blocks, &mut tmp);
        }
        let mut cm = self.quant_partition(x, n, b, bb, lowband.as_deref(), lm, gain, fill);
        if b0 > 1 {
            interleave_hadamard(x, n_b >> recombine, b0 << recombine, long_blocks, &mut tmp);
        }
        self.tmp = tmp;
        let mut n_b = n_b0;
        bb = b0;
        for _ in 0..time_divide {
            bb >>= 1;
            n_b <<= 1;
            cm |= cm >> bb;
            haar1(x, n_b, bb);
        }
        const BIT_DEINTERLEAVE: [u32; 16] = [0x00, 0x03, 0x0C, 0x0F, 0x30, 0x33, 0x3C, 0x3F, 0xC0, 0xC3, 0xCC, 0xCF, 0xF0, 0xF3, 0xFC, 0xFF];
        for k in 0..recombine {
            cm = BIT_DEINTERLEAVE[(cm & 0xF) as usize];
            haar1(x, n0 >> k, 1 << k);
        }
        bb <<= recombine;
        if let Some(lo) = lowband_out {
            let s = (n0 as f32).sqrt();
            for j in 0..n0 {
                lo[j] = s * x[j];
            }
        }
        cm & ((1u32 << bb) - 1)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn quant_band_stereo(
        &mut self,
        x: &mut [f32],
        y: &mut [f32],
        n: usize,
        mut b: i32,
        bb: usize,
        lowband: Option<&mut [f32]>,
        lm: i32,
        lowband_out: Option<&mut [f32]>,
        mut fill: u32,
    ) -> u32 {
        let orig_fill = fill;
        if n == 1 {
            return self.quant_band_n1(x, Some(y), lowband_out);
        }
        let sp = self.compute_theta(ThetaShape { n: n as i32, bb, b0: bb, lm, stereo: true }, &mut b, &mut fill);
        let mid = sp.imid as f32 / 32768.0;
        let side = sp.iside as f32 / 32768.0;
        let mut cm;
        if n == 2 {
            let mut mbits = b;
            let mut sbits = 0;
            if sp.itheta != 0 && sp.itheta != 16384 {
                sbits = 1 << BITRES;
            }
            mbits -= sbits;
            let c = sp.itheta > 8192;
            self.remaining_bits -= sp.qalloc + sbits;
            let mut sign = 0;
            if sbits != 0 {
                sign = self.dec.bits(1) as i32;
            }
            let sign = (1 - 2 * sign) as f32;
            if c {
                cm = self.quant_band(y, n, mbits, bb, lowband, lm, lowband_out, 1.0, orig_fill);
                x[0] = -sign * y[1];
                x[1] = sign * y[0];
            } else {
                cm = self.quant_band(x, n, mbits, bb, lowband, lm, lowband_out, 1.0, orig_fill);
                y[0] = -sign * x[1];
                y[1] = sign * x[0];
            }
            x[0] *= mid;
            x[1] *= mid;
            y[0] *= side;
            y[1] *= side;
            let t = x[0];
            x[0] = t - y[0];
            y[0] += t;
            let t = x[1];
            x[1] = t - y[1];
            y[1] += t;
        } else {
            let mut mbits = 0.max(b.min((b - sp.delta) / 2));
            let mut sbits = b - mbits;
            self.remaining_bits -= sp.qalloc;
            let mut rebalance = self.remaining_bits;
            if mbits >= sbits {
                cm = self.quant_band(x, n, mbits, bb, lowband, lm, lowband_out, 1.0, fill);
                rebalance = mbits - (rebalance - self.remaining_bits);
                if rebalance > 3 << BITRES && sp.itheta != 0 {
                    sbits += rebalance - (3 << BITRES);
                }
                cm |= self.quant_band(y, n, sbits, bb, None, lm, None, side, fill >> bb);
            } else {
                cm = self.quant_band(y, n, sbits, bb, None, lm, None, side, fill >> bb);
                rebalance = sbits - (rebalance - self.remaining_bits);
                if rebalance > 3 << BITRES && sp.itheta != 16384 {
                    mbits += rebalance - (3 << BITRES);
                }
                cm |= self.quant_band(x, n, mbits, bb, lowband, lm, lowband_out, 1.0, fill);
            }
        }
        if n != 2 {
            stereo_merge(&mut x[..n], &mut y[..n], mid);
        }
        if sp.inv {
            for v in y[..n].iter_mut() {
                *v = -*v;
            }
        }
        cm
    }
}

fn stereo_merge(x: &mut [f32], y: &mut [f32], mid: f32) {
    let mut xp = 0f32;
    let mut side = 0f32;
    for (a, b) in x.iter().zip(y.iter()) {
        xp += a * b;
        side += b * b;
    }
    xp *= mid;
    let mid2 = mid;
    let el = mid2 * mid2 + side - 2.0 * xp;
    let er = mid2 * mid2 + side + 2.0 * xp;
    if er < 6e-4 || el < 6e-4 {
        y.copy_from_slice(x);
        return;
    }
    let lgain = 1.0 / el.sqrt();
    let rgain = 1.0 / er.sqrt();
    for (a, b) in x.iter_mut().zip(y.iter_mut()) {
        let l = mid * *a;
        let r = *b;
        *a = lgain * (l - r);
        *b = rgain * (l + r);
    }
}

/// Decodes all band shapes (`quant_all_bands`, decoder side).
#[allow(clippy::too_many_arguments)]
pub fn quant_all_bands(
    m: &Mode,
    start: usize,
    end: usize,
    x_buf: &mut [f32],
    y_buf: Option<&mut [f32]>,
    collapse_masks: &mut [u32],
    pulses: &[i32; NB_EBANDS],
    short_blocks: bool,
    spread: usize,
    mut dual_stereo: bool,
    intensity: usize,
    tf_res: &[i32; NB_EBANDS],
    total_bits: i32,
    mut balance: i32,
    dec: &mut RangeDecoder,
    lm: usize,
    coded_bands: usize,
    seed: &mut u32,
    disable_inv: bool,
) {
    let mm = 1usize << lm;
    let bb = if short_blocks { mm } else { 1 };
    let norm_offset = mm * EBANDS[start] as usize;
    let norm_len = mm * EBANDS[NB_EBANDS - 1] as usize - norm_offset;
    let c = if y_buf.is_some() { 2 } else { 1 };
    let mut norm = vec![0f32; norm_len];
    let mut norm2 = vec![0f32; if c == 2 { norm_len } else { 0 }];
    let mut scratch = vec![0f32; mm * (EBANDS[NB_EBANDS] - EBANDS[NB_EBANDS - 1]) as usize];
    let mut scratch2 = scratch.clone();
    let mut y_buf = y_buf;
    let mut lowband_offset = 0usize;
    let mut update_lowband = true;
    let mut ctx = BandCtx {
        m,
        dec,
        i: 0,
        intensity,
        spread,
        tf_change: 0,
        remaining_bits: 0,
        seed: *seed,
        disable_inv,
        avoid_split_noise: bb > 1,
        pvq_v: Vec::new(),
        iy: Vec::new(),
        tmp: Vec::new(),
    };
    for i in start..end {
        ctx.i = i;
        let last = i == end - 1;
        let xo = mm * EBANDS[i] as usize;
        let n = mm * EBANDS[i + 1] as usize - xo;
        let tell = ctx.dec.tell_frac() as i32;
        if i != start {
            balance -= tell;
        }
        let remaining_bits = total_bits - tell - 1;
        ctx.remaining_bits = remaining_bits;
        let b = if i < coded_bands {
            let curr_balance = sdiv(balance, 3.min(coded_bands as i32 - i as i32));
            0.max(16383.min((remaining_bits + 1).min(pulses[i] + curr_balance)))
        } else {
            0
        };
        if (mm * EBANDS[i] as usize >= n + norm_offset || i == start + 1) && (update_lowband || lowband_offset == 0) {
            lowband_offset = i;
        }
        if i == start + 1 {
            let n1 = mm * (EBANDS[start + 1] - EBANDS[start]) as usize;
            let n2 = mm * (EBANDS[start + 2] - EBANDS[start + 1]) as usize;
            if n2 > n1 {
                norm.copy_within(2 * n1 - n2..n1, n1);
                if c == 2 {
                    norm2.copy_within(2 * n1 - n2..n1, n1);
                }
            }
        }
        let tf_change = tf_res[i];
        ctx.tf_change = tf_change;
        let mut effective_lowband: Option<usize> = None;
        let (mut x_cm, mut y_cm);
        if lowband_offset != 0 && (spread != SPREAD_AGGRESSIVE || bb > 1 || tf_change < 0) {
            let eff = (mm * EBANDS[lowband_offset] as usize).saturating_sub(norm_offset + n);
            effective_lowband = Some(eff);
            let mut fold_start = lowband_offset;
            loop {
                fold_start -= 1;
                if mm * (EBANDS[fold_start] as usize) <= eff + norm_offset {
                    break;
                }
            }
            let mut fold_end = lowband_offset - 1;
            loop {
                fold_end += 1;
                if !(fold_end < i && mm * (EBANDS[fold_end] as usize) < eff + norm_offset + n) {
                    break;
                }
            }
            x_cm = 0;
            y_cm = 0;
            let mut fi = fold_start;
            loop {
                x_cm |= collapse_masks[fi * c];
                y_cm |= collapse_masks[fi * c + c - 1];
                fi += 1;
                if fi >= fold_end {
                    break;
                }
            }
        } else {
            x_cm = (1u32 << bb) - 1;
            y_cm = x_cm;
        }
        if dual_stereo && i == intensity {
            dual_stereo = false;
            for j in 0..(mm * EBANDS[i] as usize - norm_offset) {
                norm[j] = 0.5 * (norm[j] + norm2[j]);
            }
        }
        let out_off = xo - norm_offset;
        let x = &mut x_buf[xo..xo + n];
        if dual_stereo && let Some(y_buf) = y_buf.as_deref_mut() {
            let y = &mut y_buf[xo..xo + n];
            let lb = effective_lowband.map(|e| {
                scratch[..n].copy_from_slice(&norm[e..e + n]);
                &mut scratch[..n]
            });
            let lo = if last { None } else { Some(&mut norm[out_off..out_off + n]) };
            x_cm = ctx.quant_band(x, n, b / 2, bb, lb, lm as i32, lo, 1.0, x_cm);
            let lb2 = effective_lowband.map(|e| {
                scratch2[..n].copy_from_slice(&norm2[e..e + n]);
                &mut scratch2[..n]
            });
            let lo2 = if last { None } else { Some(&mut norm2[out_off..out_off + n]) };
            y_cm = ctx.quant_band(y, n, b / 2, bb, lb2, lm as i32, lo2, 1.0, y_cm);
        } else {
            let lb = effective_lowband.map(|e| {
                scratch[..n].copy_from_slice(&norm[e..e + n]);
                &mut scratch[..n]
            });
            let lo = if last { None } else { Some(&mut norm[out_off..out_off + n]) };
            if let Some(yb) = y_buf.as_deref_mut() {
                let y = &mut yb[xo..xo + n];
                x_cm = ctx.quant_band_stereo(x, y, n, b, bb, lb, lm as i32, lo, x_cm | y_cm);
            } else {
                x_cm = ctx.quant_band(x, n, b, bb, lb, lm as i32, lo, 1.0, x_cm | y_cm);
            }
            y_cm = x_cm;
        }
        collapse_masks[i * c] = x_cm;
        collapse_masks[i * c + c - 1] = y_cm;
        balance += pulses[i] + tell;
        update_lowband = b > (n as i32) << BITRES;
        ctx.avoid_split_noise = false;
    }
    *seed = ctx.seed;
}

/// Anti-collapse noise injection (RFC 6716 §4.3.5).
#[allow(clippy::too_many_arguments)]
pub fn anti_collapse(
    x_buf: &mut [f32],
    collapse_masks: &[u32],
    lm: usize,
    c: usize,
    size: usize,
    start: usize,
    end: usize,
    log_e: &[f32],
    prev1: &[f32],
    prev2: &[f32],
    pulses: &[i32; NB_EBANDS],
    mut seed: u32,
) {
    for i in start..end {
        let n0 = (EBANDS[i + 1] - EBANDS[i]) as usize;
        let depth = ((1 + pulses[i]) as u32 / n0 as u32) >> lm;
        let thresh = 0.5 * (-0.125 * depth as f32).exp2();
        let sqrt_1 = 1.0 / ((n0 << lm) as f32).sqrt();
        for ch in 0..c {
            let mut p1 = prev1[ch * NB_EBANDS + i];
            let mut p2 = prev2[ch * NB_EBANDS + i];
            if c == 1 {
                p1 = p1.max(prev1[NB_EBANDS + i]);
                p2 = p2.max(prev2[NB_EBANDS + i]);
            }
            let ediff = (log_e[ch * NB_EBANDS + i] - p1.min(p2)).max(0.0);
            let mut r = 2.0 * (-ediff).exp2();
            if lm == 3 {
                r *= std::f32::consts::SQRT_2;
            }
            r = r.min(thresh);
            r *= sqrt_1;
            let off = ch * size + ((EBANDS[i] as usize) << lm);
            let xb = &mut x_buf[off..off + (n0 << lm)];
            let mut renorm = false;
            for k in 0..(1usize << lm) {
                if collapse_masks[i * c + ch] & (1 << k) == 0 {
                    for j in 0..n0 {
                        seed = lcg_rand(seed);
                        xb[(j << lm) + k] = if seed & 0x8000 != 0 { r } else { -r };
                    }
                    renorm = true;
                }
            }
            if renorm {
                renormalise_vector(xb, 1.0);
            }
        }
    }
}

/// Multiplies the normalised shapes by the band energies (`denormalise_bands`).
#[allow(clippy::too_many_arguments)]
pub fn denormalise_bands(
    x: &[f32],
    freq: &mut [f32],
    band_log_e: &[f32],
    mut start: usize,
    mut end: usize,
    mm: usize,
    downsample: usize,
    silence: bool,
) {
    let n = mm * SHORT_MDCT;
    let mut bound = mm * EBANDS[end] as usize;
    if downsample != 1 {
        bound = bound.min(n / downsample);
    }
    if silence {
        bound = 0;
        start = 0;
        end = 0;
    }
    let s0 = mm * EBANDS[start] as usize;
    freq[..s0].fill(0.0);
    for i in start..end {
        let lo = mm * EBANDS[i] as usize;
        let hi = mm * EBANDS[i + 1] as usize;
        let lg = band_log_e[i] + E_MEANS[i];
        let g = lg.min(32.0).exp2();
        for j in lo..hi {
            freq[j] = x[j] * g;
        }
    }
    let from = bound;
    if from < n {
        freq[from..n].fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitexact_trig_endpoints() {
        assert_eq!(bitexact_cos(0), 32768);

        let mid = bitexact_cos(8192);
        assert!((mid - 23170).abs() < 20, "{mid}");
        assert_eq!(bitexact_log2tan(16384, 16384), 0);
    }

    #[test]
    fn isqrt_exact() {
        for v in [0u32, 1, 2, 3, 4, 15, 16, 17, 99, 100, 1 << 20, u32::MAX] {
            let r = isqrt32(v) as u64;
            assert!(r * r <= v as u64 && (r + 1) * (r + 1) > v as u64);
        }
    }

    #[test]
    fn pvq_roundtrip_all_codewords_small() {
        use crate::range::enc::RangeEncoder;
        // Enumerate every index for small N,K, decode, and check uniqueness + sum |y| == K.
        for n in 1..6usize {
            for k in 1..5usize {
                let total = super::super::rate::pvq_v(n, k) as u32;
                let mut seen = std::collections::HashSet::new();
                for idx in 0..total {
                    let mut e = RangeEncoder::new(16);
                    e.uint(idx, total);
                    let buf = e.done();
                    let mut d = RangeDecoder::new(&buf);
                    let mut y = vec![0i32; n];
                    let mut v = Vec::new();
                    let ryy = decode_pulses(&mut y, n, k, &mut d, &mut v);
                    assert_eq!(y.iter().map(|v| v.unsigned_abs() as usize).sum::<usize>(), k);
                    assert_eq!(ryy as i32, y.iter().map(|v| v * v).sum::<i32>());
                    assert!(seen.insert(y.clone()), "duplicate codeword n={n} k={k} idx={idx}");
                }
            }
        }
    }

    #[test]
    fn haar_is_involution() {
        let mut x: Vec<f32> = (0..16).map(|i| i as f32).collect();
        let orig = x.clone();
        haar1(&mut x, 16, 1);
        haar1(&mut x, 16, 1);
        for (a, b) in x.iter().zip(&orig) {
            assert!((a - b).abs() < 1e-4);
        }
        let mut tmp = Vec::new();
        for &h in &[false, true] {
            let mut y = orig.clone();
            deinterleave_hadamard(&mut y, 4, 4, h, &mut tmp);
            interleave_hadamard(&mut y, 4, 4, h, &mut tmp);
            assert_eq!(y, orig);
        }
    }
}
