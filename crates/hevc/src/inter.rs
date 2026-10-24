//! Fractional sample interpolation (8.5.3.3.3) and weighted sample prediction (8.5.3.3.4).

use crate::picture::Frame;
use crate::spec_tables::{CHROMA_FILTER, LUMA_FILTER};

/// Max prediction block size + filter margin.
const WIN: usize = 64 + 7;

/// Reference windows and first-pass rows, reused between blocks (no per-block zeroing).
pub struct McScratch {
    win: Vec<i16>,
    tmp: Vec<i16>,
}

impl Default for McScratch {
    fn default() -> Self {
        McScratch { win: vec![0; WIN * WIN], tmp: vec![0; WIN * WIN] }
    }
}

/// `out[j] = (sum_k f[k] * src[j + k * step]) >> shift` for `W` outputs: taps outer, outputs
/// inner, so each tap is one vector multiply-add across the row (exact integer sums).
#[inline(always)]
fn filt_w<const T: usize, const W: usize>(src: &[i16], step: usize, f: &[i8; T], shift: u32, out: &mut [i16]) {
    let mut acc = [0i32; W];
    for k in 0..T {
        let c = f[k] as i32;
        let Some(p) = src.get(k * step..).and_then(<[_]>::first_chunk::<W>) else {
            return;
        };
        for j in 0..W {
            acc[j] += c * p[j] as i32;
        }
    }
    let Some(o) = out.first_chunk_mut::<W>() else {
        return;
    };
    for j in 0..W {
        o[j] = (acc[j] >> shift) as i16;
    }
}

/// [`filt_w`] for a run of `w` outputs (prediction block widths are fixed sizes).
#[inline(always)]
fn filt<const T: usize>(src: &[i16], step: usize, f: &[i8; T], shift: u32, out: &mut [i16], w: usize) {
    match w {
        2 => filt_w::<T, 2>(src, step, f, shift, out),
        4 => filt_w::<T, 4>(src, step, f, shift, out),
        6 => filt_w::<T, 6>(src, step, f, shift, out),
        8 => filt_w::<T, 8>(src, step, f, shift, out),
        12 => filt_w::<T, 12>(src, step, f, shift, out),
        16 => filt_w::<T, 16>(src, step, f, shift, out),
        24 => filt_w::<T, 24>(src, step, f, shift, out),
        32 => filt_w::<T, 32>(src, step, f, shift, out),
        48 => filt_w::<T, 48>(src, step, f, shift, out),
        64 => filt_w::<T, 64>(src, step, f, shift, out),
        _ => {
            for (j, o) in out[..w].iter_mut().enumerate() {
                let s: i32 = (0..T).map(|k| f[k] as i32 * src[j + k * step] as i32).sum();
                *o = (s >> shift) as i16;
            }
        }
    }
}

/// Separable sub-sample interpolation of a w x h block from `win` (stride WIN, the block's
/// top-left sample at (m, m) for a T-tap filter with m = T / 2 - 1).
#[allow(clippy::too_many_arguments)]
fn interpolate<const T: usize>(
    win: &[i16],
    tmp: &mut [i16],
    fx: usize,
    fy: usize,
    hf: &[i8; T],
    vf: &[i8; T],
    w: usize,
    h: usize,
    shift1: u32,
    shift3: u32,
    out: &mut [i16],
) {
    let m = T / 2 - 1;
    if fx == 0 && fy == 0 {
        for r in 0..h {
            for (o, &s) in out[r * w..r * w + w].iter_mut().zip(&win[(r + m) * WIN + m..]) {
                *o = s << shift3;
            }
        }
    } else if fy == 0 {
        for r in 0..h {
            filt(&win[(r + m) * WIN..], 1, hf, shift1, &mut out[r * w..], w);
        }
    } else if fx == 0 {
        for r in 0..h {
            filt(&win[r * WIN + m..], WIN, vf, shift1, &mut out[r * w..], w);
        }
    } else {
        for r in 0..h + T - 1 {
            filt(&win[r * WIN..], 1, hf, shift1, &mut tmp[r * WIN..], w);
        }
        for r in 0..h {
            filt(&tmp[r * WIN..], WIN, vf, 6, &mut out[r * w..], w);
        }
    }
}

/// Luma prediction samples (14-bit intermediate precision) of a w x h block at (x, y) displaced by the
/// quarter-sample motion vector `mv`, into `out` (stride w).
#[allow(clippy::too_many_arguments)]
pub fn mc_luma(f: &Frame, x: i32, y: i32, mv: [i16; 2], w: usize, h: usize, out: &mut [i16], sc: &mut McScratch) {
    let bd = f.bit_depth;
    let (fx, fy) = ((mv[0] & 3) as usize, (mv[1] & 3) as usize);
    let xi = x + (mv[0] as i32 >> 2);
    let yi = y + (mv[1] as i32 >> 2);
    let shift1 = bd.min(12) - 8;
    let shift3 = 14 - bd;
    f.luma_window(xi - 3, yi - 3, w + 7, h + 7, &mut sc.win, WIN);
    interpolate::<8>(&sc.win, &mut sc.tmp, fx, fy, &LUMA_FILTER[fx], &LUMA_FILTER[fy], w, h, shift1, shift3, out);
}

/// Chroma prediction samples of a w x h chroma block at chroma position (x, y) with a 1/8-sample
/// vector (4:2:0).
#[allow(clippy::too_many_arguments)]
pub fn mc_chroma(f: &Frame, c: usize, x: i32, y: i32, mv: [i16; 2], w: usize, h: usize, out: &mut [i16], sc: &mut McScratch) {
    let bd = f.bit_depth_c;
    let (fx, fy) = ((mv[0] & 7) as usize, (mv[1] & 7) as usize);
    let xi = x + (mv[0] as i32 >> 3);
    let yi = y + (mv[1] as i32 >> 3);
    let shift1 = bd.min(12) - 8;
    let shift3 = 14 - bd;
    f.chroma_window(c, xi - 1, yi - 1, w + 3, h + 3, &mut sc.win, WIN);
    interpolate::<4>(&sc.win, &mut sc.tmp, fx, fy, &CHROMA_FILTER[fx], &CHROMA_FILTER[fy], w, h, shift1, shift3, out);
}

/// Default weighted prediction, one list (8-262).
pub fn put_uni(p: &[i16], w: usize, h: usize, bd: u32, dst: &mut [u16], ds: usize) {
    let shift = 14 - bd;
    let off = if shift > 0 { 1 << (shift - 1) } else { 0 };
    let max = (1i32 << bd) - 1;
    // Rows as slices and min / max clipping (`clamp` asserts its bounds) so the loop vectorises.
    for (d, s) in dst.chunks_mut(ds).zip(p[..w * h].chunks(w)) {
        for (d, &s) in d[..w].iter_mut().zip(s) {
            *d = ((s as i32 + off) >> shift).max(0).min(max) as u16;
        }
    }
}

/// Default weighted prediction, bi-prediction (8-263).
pub fn put_bi(p0: &[i16], p1: &[i16], w: usize, h: usize, bd: u32, dst: &mut [u16], ds: usize) {
    let shift = 15 - bd;
    let off = 1 << (shift - 1);
    let max = (1i32 << bd) - 1;
    for ((d, a), b) in dst.chunks_mut(ds).zip(p0[..w * h].chunks(w)).zip(p1[..w * h].chunks(w)) {
        for ((d, &a), &b) in d[..w].iter_mut().zip(a).zip(b) {
            *d = ((a as i32 + b as i32 + off) >> shift).max(0).min(max) as u16;
        }
    }
}

/// Explicit weighted prediction, one list (8-265). `o` is the offset already scaled to the bit depth.
pub fn put_weighted_uni(p: &[i16], w: usize, h: usize, bd: u32, log2wd: u32, wt: i32, o: i32, dst: &mut [u16], ds: usize) {
    let max = (1i32 << bd) - 1;
    for r in 0..h {
        for c in 0..w {
            let v = p[r * w + c] as i32 * wt;
            let v = if log2wd >= 1 { ((v + (1 << (log2wd - 1))) >> log2wd) + o } else { v + o };
            dst[r * ds + c] = v.max(0).min(max) as u16;
        }
    }
}

/// Explicit weighted bi-prediction (8-266).
pub fn put_weighted_bi(
    p0: &[i16],
    p1: &[i16],
    w: usize,
    h: usize,
    bd: u32,
    log2wd: u32,
    w0: i32,
    w1: i32,
    o0: i32,
    o1: i32,
    dst: &mut [u16],
    ds: usize,
) {
    let max = (1i32 << bd) - 1;
    for r in 0..h {
        for c in 0..w {
            let i = r * w + c;
            let v = (p0[i] as i32 * w0 + p1[i] as i32 * w1 + ((o0 + o1 + 1) << log2wd)) >> (log2wd + 1);
            dst[r * ds + c] = v.max(0).min(max) as u16;
        }
    }
}
