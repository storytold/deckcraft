//! Film grain synthesis (7.18.3), applied to output pictures only (references stay clean).

use crate::Picture;
use crate::header::{FilmGrainParams, SequenceHeader};
use crate::spec_tables::{GAUSSIAN_SEQUENCE, MC_IDENTITY};

#[inline]
/// One plane's scaling lookup (7.18.3.4) from its `n` points. The header rejects points whose
/// values don't strictly increase; a pair that still doesn't (defence in depth) is skipped
/// instead of dividing by zero. No points: all zero.
fn scaling_lut(n: usize, xs: &[u8], ys: &[u8]) -> [i32; 256] {
    let mut l = [0i32; 256];
    let n = n.min(xs.len()).min(ys.len());
    let (Some(&x0), Some(&y0)) = (xs.first(), ys.first()) else { return l };
    if n == 0 {
        return l;
    }
    for v in l.iter_mut().take(x0 as usize) {
        *v = y0 as i32;
    }
    for i in 0..n - 1 {
        let (xa, xb, ya, yb) = (xs[i] as i32, xs[i + 1] as i32, ys[i] as i32, ys[i + 1] as i32);
        let dx = xb - xa;
        if dx <= 0 {
            continue;
        }
        let delta = (yb - ya) * ((65536 + (dx >> 1)) / dx);
        for x in 0..dx {
            if let Some(v) = l.get_mut((xa + x) as usize) {
                *v = ya + ((x * delta + 32768) >> 16);
            }
        }
    }
    let (xl, yl) = (xs[n - 1], ys[n - 1]);
    for v in l.iter_mut().skip(xl as usize) {
        *v = yl as i32;
    }
    l
}

fn round2(x: i32, n: u32) -> i32 {
    if n == 0 { x } else { (x + (1 << (n - 1))) >> n }
}

struct Rng(u32);

impl Rng {
    /// get_random_number( bits ) (7.18.3.2)
    fn next(&mut self, bits: u32) -> i32 {
        let r = self.0;
        let bit = (r ^ (r >> 1) ^ (r >> 3) ^ (r >> 12)) & 1;
        let r = (r >> 1) | (bit << 15);
        self.0 = r;
        ((r >> (16 - bits)) & ((1 << bits) - 1)) as i32
    }
}

/// Picture-level parameters shared by the grain synthesis stages.
struct GrainCtx<'a> {
    g: &'a FilmGrainParams,
    bd: u32,
    mono: bool,
    sub_x: usize,
    sub_y: usize,
    w: usize,
    h: usize,
    grain_min: i32,
    grain_max: i32,
}

impl GrainCtx<'_> {
    fn num_planes(&self) -> usize {
        if self.mono { 1 } else { 3 }
    }

    fn grain_shift(&self) -> u32 {
        12 - self.bd + self.g.grain_scale_shift
    }

    fn ar_shift(&self) -> u32 {
        self.g.ar_coeff_shift_minus_6 + 6
    }

    fn lag(&self) -> i32 {
        self.g.ar_coeff_lag as i32
    }

    fn stripe_w(&self) -> usize {
        self.w + 64
    }
}

type GrainTemplate = Vec<[i32; 82]>;

/// The luma, Cb and Cr grain templates (7.18.3.3).
struct Templates {
    luma: GrainTemplate,
    cb: GrainTemplate,
    cr: GrainTemplate,
}

impl Templates {
    fn plane(&self, plane: usize) -> &GrainTemplate {
        match plane {
            0 => &self.luma,
            1 => &self.cb,
            _ => &self.cr,
        }
    }
}

fn luma_ar_sum(g: &FilmGrainParams, luma: &[[i32; 82]], lag: i32, y: usize, x: usize) -> i32 {
    let mut s = 0;
    let mut pos = 0;
    'outer: for dr in -lag..=0 {
        for dc in -lag..=lag {
            if dr == 0 && dc == 0 {
                break 'outer;
            }
            let c = g.ar_coeffs_y_plus_128[pos] as i32 - 128;
            s += luma[(y as i32 + dr) as usize][(x as i32 + dc) as usize] * c;
            pos += 1;
        }
    }
    s
}

fn luma_grain(c: &GrainCtx, rng: &mut Rng) -> GrainTemplate {
    let g = c.g;
    let shift = c.grain_shift();
    let mut luma = vec![[0i32; 82]; 73];
    for row in luma.iter_mut() {
        for v in row.iter_mut() {
            let gv = if g.num_y_points > 0 { GAUSSIAN_SEQUENCE[rng.next(11) as usize] as i32 } else { 0 };
            *v = round2(gv, shift);
        }
    }
    let ar_shift = c.ar_shift();
    let lag = c.lag();
    for y in 3..73 {
        for x in 3..82 - 3 {
            let s = luma_ar_sum(g, &luma, lag, y, x);
            luma[y][x] = (luma[y][x] + round2(s, ar_shift)).clamp(c.grain_min, c.grain_max);
        }
    }
    luma
}

/// The luma contribution to the chroma autoregressive filter at chroma position (y, x).
fn chroma_luma_term(c: &GrainCtx, luma: &[[i32; 82]], y: usize, x: usize) -> i32 {
    let mut l = 0;
    let lx = ((x - 3) << c.sub_x) + 3;
    let ly = ((y - 3) << c.sub_y) + 3;
    for i in 0..=c.sub_y {
        for j in 0..=c.sub_x {
            l += luma[ly + i][lx + j];
        }
    }
    round2(l, (c.sub_x + c.sub_y) as u32)
}

fn chroma_ar_sums(c: &GrainCtx, t: &Templates, y: usize, x: usize) -> (i32, i32) {
    let g = c.g;
    let lag = c.lag();
    let mut s0 = 0;
    let mut s1 = 0;
    let mut pos = 0;
    'outer: for dr in -lag..=0 {
        for dc in -lag..=lag {
            let c0 = g.ar_coeffs_cb_plus_128[pos] as i32 - 128;
            let c1 = g.ar_coeffs_cr_plus_128[pos] as i32 - 128;
            if dr == 0 && dc == 0 {
                if g.num_y_points > 0 {
                    let l = chroma_luma_term(c, &t.luma, y, x);
                    s0 += l * c0;
                    s1 += l * c1;
                }
                break 'outer;
            }
            let (yy, xx) = ((y as i32 + dr) as usize, (x as i32 + dc) as usize);
            s0 += t.cb[yy][xx] * c0;
            s1 += t.cr[yy][xx] * c1;
            pos += 1;
        }
    }
    (s0, s1)
}

/// Chroma grain templates; zero for monochrome pictures.
fn chroma_grain(c: &GrainCtx, rng: &mut Rng, luma: GrainTemplate) -> Templates {
    let g = c.g;
    let mut t = Templates { luma, cb: vec![[0i32; 82]; 73], cr: vec![[0i32; 82]; 73] };
    if c.mono {
        return t;
    }
    let shift = c.grain_shift();
    let ar_shift = c.ar_shift();
    let chroma_w = if c.sub_x != 0 { 44 } else { 82 };
    let chroma_h = if c.sub_y != 0 { 38 } else { 73 };
    for (arr, seed, on) in [(&mut t.cb, 0xb524u32, g.num_cb_points > 0), (&mut t.cr, 0x49d8, g.num_cr_points > 0)] {
        rng.0 = g.grain_seed ^ seed;
        for row in arr.iter_mut().take(chroma_h) {
            for v in row.iter_mut().take(chroma_w) {
                let gv = if on || g.chroma_scaling_from_luma { GAUSSIAN_SEQUENCE[rng.next(11) as usize] as i32 } else { 0 };
                *v = round2(gv, shift);
            }
        }
    }
    for y in 3..chroma_h {
        for x in 3..chroma_w - 3 {
            let (s0, s1) = chroma_ar_sums(c, &t, y, x);
            t.cb[y][x] = (t.cb[y][x] + round2(s0, ar_shift)).clamp(c.grain_min, c.grain_max);
            t.cr[y][x] = (t.cr[y][x] + round2(s1, ar_shift)).clamp(c.grain_min, c.grain_max);
        }
    }
    t
}

/// Scaling lookup tables (7.18.3.4), one per plane.
fn scaling_luts(g: &FilmGrainParams, num_planes: usize) -> [[i32; 256]; 3] {
    let mut lut = [[0i32; 256]; 3];
    for (plane, l) in lut.iter_mut().enumerate().take(num_planes) {
        let (n, xs, ys) = if plane == 0 || g.chroma_scaling_from_luma {
            (g.num_y_points, &g.point_y_value, &g.point_y_scaling)
        } else if plane == 1 {
            (g.num_cb_points, &g.point_cb_value, &g.point_cb_scaling)
        } else {
            (g.num_cr_points, &g.point_cr_value, &g.point_cr_scaling)
        };
        *l = scaling_lut(n, xs, ys);
    }
    lut
}

fn scale_lookup(lut: &[[i32; 256]; 3], bd: u32, plane: usize, index: i32) -> i32 {
    let sh = bd - 8;
    let x = index >> sh;
    let rem = index - (x << sh);
    if bd == 8 || x == 255 {
        lut[plane][x as usize]
    } else {
        let start = lut[plane][x as usize];
        let end = lut[plane][x as usize + 1];
        start + round2((end - start) * rem, sh)
    }
}

/// Fills one plane of a noise stripe from the grain template at stripe column `x`.
fn fill_stripe_plane(c: &GrainCtx, src: &GrainTemplate, s: &mut [i32], plane: usize, offsets: (usize, usize), x: usize) {
    let (off_x, off_y) = offsets;
    let stripe_w = c.stripe_w();
    let overlap = c.g.overlap_flag;
    let psx = if plane > 0 { c.sub_x } else { 0 };
    let psy = if plane > 0 { c.sub_y } else { 0 };
    let pox = if psx != 0 { 6 + off_x } else { 9 + off_x * 2 };
    let poy = if psy != 0 { 6 + off_y } else { 9 + off_y * 2 };
    for i in 0..(34 >> psy) {
        for j in 0..(34 >> psx) {
            let mut gv = src[poy + i][pox + j];
            let (idx, blend) = if psx == 0 { (i * stripe_w + x * 2 + j, j < 2) } else { (i * stripe_w + x + j, j == 0) };
            if blend && overlap && x > 0 {
                let old = s[idx];
                gv = if psx != 0 {
                    old * 23 + gv * 22
                } else if j == 0 {
                    old * 27 + gv * 17
                } else {
                    old * 17 + gv * 27
                };
                gv = round2(gv, 5).clamp(c.grain_min, c.grain_max);
            }
            s[idx] = gv;
        }
    }
}

/// Noise stripes (7.18.3.5).
struct Stripes {
    data: Vec<[Vec<i32>; 3]>,
    stripe_w: usize,
    overlap: bool,
    sub_y: usize,
    grain_min: i32,
    grain_max: i32,
}

impl Stripes {
    fn build(c: &GrainCtx, t: &Templates, rng: &mut Rng) -> Stripes {
        let g = c.g;
        let stripe_w = c.stripe_w();
        let n_stripes = (c.h.div_ceil(2)).div_ceil(16).max(1);
        let mut data: Vec<[Vec<i32>; 3]> = (0..n_stripes).map(|_| [vec![0; 34 * stripe_w], vec![0; 34 * stripe_w], vec![0; 34 * stripe_w]]).collect();
        for (luma_num, stripe) in data.iter_mut().enumerate() {
            let mut r = g.grain_seed;
            r ^= (((luma_num * 37 + 178) & 255) << 8) as u32;
            r ^= ((luma_num * 173 + 105) & 255) as u32;
            rng.0 = r;
            let mut x = 0;
            while x < c.w.div_ceil(2) {
                let rand = rng.next(8) as usize;
                let offsets = (rand >> 4, rand & 15);
                for (plane, s) in stripe.iter_mut().enumerate().take(c.num_planes()) {
                    fill_stripe_plane(c, t.plane(plane), s, plane, offsets, x);
                }
                x += 16;
            }
        }
        Stripes { data, stripe_w, overlap: g.overlap_flag, sub_y: c.sub_y, grain_min: c.grain_min, grain_max: c.grain_max }
    }

    fn noise_at(&self, plane: usize, y: usize, x: usize) -> i32 {
        let psy = if plane > 0 { self.sub_y } else { 0 };
        let luma_num = y >> (5 - psy);
        let i = y - (luma_num << (5 - psy));
        let mut gv = self.data[luma_num][plane][i * self.stripe_w + x];
        if psy == 0 {
            if i < 2 && luma_num > 0 && self.overlap {
                let old = self.data[luma_num - 1][plane][(i + 32) * self.stripe_w + x];
                gv = if i == 0 { old * 27 + gv * 17 } else { old * 17 + gv * 27 };
                gv = round2(gv, 5).clamp(self.grain_min, self.grain_max);
            }
        } else if i < 1 && luma_num > 0 && self.overlap {
            let old = self.data[luma_num - 1][plane][(i + 16) * self.stripe_w + x];
            gv = round2(old * 23 + gv * 22, 5).clamp(self.grain_min, self.grain_max);
        }
        gv
    }
}

/// Output clipping range and scaling shift for the blend step (7.18.3.5).
struct BlendRange {
    min_value: i32,
    max_luma: i32,
    max_chroma: i32,
    pix_max: i32,
    scaling_shift: u32,
}

impl BlendRange {
    fn new(c: &GrainCtx, matrix_coefficients: usize) -> BlendRange {
        let bd = c.bd;
        let (min_value, max_luma, max_chroma) = if c.g.clip_to_restricted_range {
            let ml = 235 << (bd - 8);
            (16 << (bd - 8), ml, if matrix_coefficients == MC_IDENTITY { ml } else { 240 << (bd - 8) })
        } else {
            let m = (256 << (bd - 8)) - 1;
            (0, m, m)
        };
        BlendRange { min_value, max_luma, max_chroma, pix_max: (1i32 << bd) - 1, scaling_shift: c.g.grain_scaling_minus_8 + 8 }
    }
}

struct BlendEnv<'a> {
    c: &'a GrainCtx<'a>,
    lut: &'a [[i32; 256]; 3],
    stripes: &'a Stripes,
    range: &'a BlendRange,
}

impl BlendEnv<'_> {
    fn blend_chroma(&self, py: &[u16], pu: &mut [u16], pv: &mut [u16]) {
        let c = self.c;
        let g = c.g;
        let (w, h, sub_x, sub_y, bd) = (c.w, c.h, c.sub_x, c.sub_y, c.bd);
        let cw = (w + sub_x) >> sub_x;
        let ch = (h + sub_y) >> sub_y;
        for y in 0..ch {
            for x in 0..cw {
                let lx = x << sub_x;
                let ly = y << sub_y;
                let lnx = (lx + 1).min(w - 1);
                let avg = if sub_x != 0 { round2(py[ly * w + lx] as i32 + py[ly * w + lnx] as i32, 1) } else { py[ly * w + lx] as i32 };
                for (plane, out, on, luma_mult, mult, offset) in [
                    (1usize, &mut *pu, g.num_cb_points > 0, g.cb_luma_mult, g.cb_mult, g.cb_offset),
                    (2, &mut *pv, g.num_cr_points > 0, g.cr_luma_mult, g.cr_mult, g.cr_offset),
                ] {
                    if !(on || g.chroma_scaling_from_luma) {
                        continue;
                    }
                    let orig = out[y * cw + x] as i32;
                    let merged = if g.chroma_scaling_from_luma {
                        avg
                    } else {
                        let combined = avg * (luma_mult as i32 - 128) + orig * (mult as i32 - 128);
                        ((combined >> 6) + ((offset as i32 - 256) << (bd - 8))).clamp(0, self.range.pix_max)
                    };
                    let noise = round2(scale_lookup(self.lut, bd, plane, merged) * self.stripes.noise_at(plane, y, x), self.range.scaling_shift);
                    out[y * cw + x] = (orig + noise).clamp(self.range.min_value, self.range.max_chroma) as u16;
                }
            }
        }
    }

    fn blend_luma(&self, py: &mut [u16]) {
        let (w, h) = (self.c.w, self.c.h);
        for y in 0..h {
            for x in 0..w {
                let orig = py[y * w + x] as i32;
                let noise = round2(scale_lookup(self.lut, self.c.bd, 0, orig) * self.stripes.noise_at(0, y, x), self.range.scaling_shift);
                py[y * w + x] = (orig + noise).clamp(self.range.min_value, self.range.max_luma) as u16;
            }
        }
    }
}

pub(crate) fn apply(pic: &mut Picture, g: &FilmGrainParams, seq: &SequenceHeader) {
    let _ = seq;
    let bd = pic.bit_depth as u32;
    let grain_center = 128i32 << (bd - 8);
    let c = GrainCtx {
        g,
        bd,
        mono: pic.mono_chrome,
        sub_x: pic.subsampling_x as usize,
        sub_y: pic.subsampling_y as usize,
        w: pic.width as usize,
        h: pic.height as usize,
        grain_min: -grain_center,
        grain_max: (256i32 << (bd - 8)) - 1 - grain_center,
    };

    // Generate grain (7.18.3.3).
    let mut rng = Rng(g.grain_seed);
    let luma = luma_grain(&c, &mut rng);
    let templates = chroma_grain(&c, &mut rng, luma);
    let lut = scaling_luts(g, c.num_planes());
    let stripes = Stripes::build(&c, &templates, &mut rng);

    // Blend (7.18.3.5).
    let range = BlendRange::new(&c, pic.matrix_coefficients as usize);
    let env = BlendEnv { c: &c, lut: &lut, stripes: &stripes, range: &range };
    if !c.mono {
        let [py, pu, pv] = &mut pic.planes;
        env.blend_chroma(py, pu, pv);
    }
    if g.num_y_points > 0 {
        env.blend_luma(&mut pic.planes[0]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_lut_interpolates_and_tolerates_duplicate_points() {
        let l = scaling_lut(2, &[0, 255], &[0, 255]);
        assert_eq!((l[0], l[128], l[255]), (0, 128, 255));
        // Duplicate and descending x values don't divide by zero.
        let l = scaling_lut(4, &[10, 10, 5, 200], &[20, 40, 60, 80]);
        assert_eq!(l[0], 20);
        assert_eq!(l[255], 80);
        assert_eq!(scaling_lut(0, &[], &[]), [0; 256]);
    }
}
