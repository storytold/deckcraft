//! Dissolve In and Out: a shape shows through a field of smooth noise.
//!
//! Measured on PowerPoint's video export (1080p and 4K): the pattern is a fixed map, so a part that
//! has shown never goes again as the level grows, and it is the same blobs at every resolution,
//! about 1 to 3 pt across and wider than tall. Here that is value noise on a grid of cells
//! `CELL_W` x `CELL_H` pt in slide space with smooth interpolation (the cell size is the best fit of
//! simulated noise against the measured autocorrelation), seeded by the shape.

use std::sync::{Arc, Mutex, OnceLock};

use kurbo::{Affine, Rect};
use vello_cpu::Mask;

/// Grid cell of the noise, in points.
const CELL_W: f64 = 2.0;
const CELL_H: f64 = 1.4;
/// Most mask pixels one shape gets; a bigger canvas falls back to a fade.
const MAX_MASK_PIXELS: usize = 1 << 26;

/// A hash of a grid corner as a number in `[0, 1)`.
fn corner(x: i64, y: i64, seed: u64) -> f64 {
    let mut z = seed ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// Where the values of the noise fall: `QUANTILES[i]` is the value that `i / 256` of the field lies
/// below. Interpolated noise averages its corners, so its values bunch round the middle; asking for
/// a share of the shape means asking for a value from here.
fn quantiles() -> &'static [f64; 257] {
    static Q: OnceLock<[f64; 257]> = OnceLock::new();
    Q.get_or_init(|| {
        const N: u64 = 1 << 16;
        let mut v: Vec<f64> = (0..N)
            .map(|i| {
                let c = |k: u64| corner(i as i64, k as i64, 0x51ED_270B);
                let (fx, fy) = (smooth(c(4)), smooth(c(5)));
                let top = c(0) + (c(1) - c(0)) * fx;
                let bottom = c(2) + (c(3) - c(2)) * fx;
                top + (bottom - top) * fy
            })
            .collect();
        v.sort_by(f64::total_cmp);
        let mut q = [0.0; 257];
        for (i, slot) in q.iter_mut().enumerate() {
            *slot = v.get(((i as u64 * (N - 1)) / 256) as usize).copied().unwrap_or(0.5);
        }
        q[0] = 0.0;
        q[256] = 1.0;
        q
    })
}

/// The noise value below which `share` (0..1) of the shape lies.
fn value_below(share: f64) -> f64 {
    let q = quantiles();
    let x = share.clamp(0.0, 1.0) * 256.0;
    let i = (x as usize).min(255);
    let f = x - i as f64;
    match (q.get(i), q.get(i + 1)) {
        (Some(a), Some(b)) => a + (b - a) * f,
        _ => 1.0,
    }
}

/// The noise of one shape over an area of the canvas, one byte per pixel (the value in 1/256ths).
/// It depends only on the shape, the area and the view, so the frames of an effect share it: a frame
/// is then a comparison per pixel instead of the interpolation of the noise (about 100 ms a 4K
/// shape, against a few ms).
struct Noise {
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
    values: Vec<u8>,
}

type NoiseKey = (u16, u16, u64, [u64; 4], [u64; 4]);

/// The noise fields of the last few dissolving shapes. A field of a 4K canvas is 8 MB at most.
const CACHED_FIELDS: usize = 6;

fn cache() -> &'static Mutex<Vec<(NoiseKey, Arc<Noise>)>> {
    static C: OnceLock<Mutex<Vec<(NoiseKey, Arc<Noise>)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Vec::new()))
}

fn noise(key: NoiseKey, cw: usize, rect: (usize, usize, usize, usize), view: [f64; 6], seed: u64) -> Arc<Noise> {
    if let Ok(mut c) = cache().lock() {
        if let Some(at) = c.iter().position(|(k, _)| *k == key) {
            let hit = c.remove(at);
            let n = Arc::clone(&hit.1);
            c.push(hit);
            return n;
        }
    }
    let (x0, x1, y0, y1) = rect;
    let _ = cw;
    let [sx, _, _, sy, tx, ty] = view;
    let mut values = vec![0u8; (x1 - x0) * (y1 - y0)];
    // Per column: the grid cell and the smooth position inside it; the same per row.
    let grid = |px: usize, scale: f64, offset: f64, cell: f64| {
        let g = ((px as f64 + 0.5 - offset) / scale) / cell;
        let i = g.floor();
        (i as i64, smooth(g - i))
    };
    let cols: Vec<(i64, f64)> = (x0..x1).map(|px| grid(px, sx, tx, CELL_W)).collect();
    if let (Some(first), Some(last)) = (cols.iter().map(|c| c.0).min(), cols.iter().map(|c| c.0).max()) {
        // The corners of the current row of cells, and of the one below it.
        let mut row_cache: Option<(i64, Vec<f64>, Vec<f64>)> = None;
        for (row, py) in (y0..y1).enumerate() {
            let (gy, fy) = grid(py, sy, ty, CELL_H);
            if row_cache.as_ref().is_none_or(|r| r.0 != gy) {
                let line = |y: i64| (first..=last + 1).map(|x| corner(x, y, seed)).collect::<Vec<f64>>();
                row_cache = Some((gy, line(gy), line(gy + 1)));
            }
            let Some((_, upper, lower)) = row_cache.as_ref() else { continue };
            let width = x1 - x0;
            let Some(out) = values.get_mut(row * width..(row + 1) * width) else { continue };
            for (slot, (gx, fx)) in out.iter_mut().zip(&cols) {
                let k = (gx - first) as usize;
                let (Some(&a), Some(&b), Some(&c), Some(&d)) = (upper.get(k), upper.get(k + 1), lower.get(k), lower.get(k + 1)) else { continue };
                let top = a + (b - a) * fx;
                let bottom = c + (d - c) * fx;
                *slot = ((top + (bottom - top) * fy) * 256.0).clamp(0.0, 255.0) as u8;
            }
        }
    }
    let field = Arc::new(Noise { x0, x1, y0, y1, values });
    if let Ok(mut c) = cache().lock() {
        c.push((key, Arc::clone(&field)));
        if c.len() > CACHED_FIELDS {
            c.remove(0);
        }
    }
    field
}

/// What a dissolve at `share` shows: a mask for the whole canvas (`w` x `h` px, as the layer needs),
/// nonzero only inside `area` (device pixels). `view` maps slide points to device pixels and only
/// scales and translates. `None` when the canvas is too big for a mask.
pub(crate) fn mask(w: u16, h: u16, area: Rect, view: Affine, seed: u64, share: f64) -> Option<Mask> {
    let (cw, ch) = (usize::from(w), usize::from(h));
    if cw == 0 || ch == 0 || cw * ch > MAX_MASK_PIXELS {
        return None;
    }
    let mut data = vec![0u8; cw * ch];
    let coeffs = view.as_coeffs();
    let [sx, _, _, sy, _, _] = coeffs;
    if sx.abs() < 1e-9 || sy.abs() < 1e-9 || !area.is_finite() {
        return Some(Mask::from_parts(data, w, h));
    }
    // The values are in 256ths: a pixel shows where its value is below the level's.
    let limit = (value_below(share) * 256.0).round() as u16;
    let x0 = area.x0.floor().max(0.0) as usize;
    let x1 = (area.x1.ceil().max(0.0) as usize).min(cw);
    let y0 = area.y0.floor().max(0.0) as usize;
    let y1 = (area.y1.ceil().max(0.0) as usize).min(ch);
    if x0 >= x1 || y0 >= y1 {
        return Some(Mask::from_parts(data, w, h));
    }
    let bits = |a: [f64; 6]| [a[0].to_bits(), a[3].to_bits(), a[4].to_bits(), a[5].to_bits()];
    let key = (w, h, seed, [x0 as u64, x1 as u64, y0 as u64, y1 as u64], bits(coeffs));
    let field = noise(key, cw, (x0, x1, y0, y1), coeffs, seed);
    let width = field.x1 - field.x0;
    for (row, py) in (field.y0..field.y1).enumerate() {
        let (Some(src), Some(out)) = (field.values.get(row * width..(row + 1) * width), data.get_mut(py * cw + field.x0..py * cw + field.x1)) else {
            continue;
        };
        for (slot, v) in out.iter_mut().zip(src) {
            if u16::from(*v) < limit {
                *slot = 255;
            }
        }
    }
    Some(Mask::from_parts(data, w, h))
}
