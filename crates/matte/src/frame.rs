//! Bounded RGBA frames and 8-bit mattes, with the resampling and masking the pipeline needs.

use crate::MatteError;

/// Largest side a frame may have.
pub const MAX_DIM: u32 = 4096;
/// Largest number of pixels a frame or matte may carry (16 MiB of RGBA).
pub const MAX_PIXELS: u32 = MAX_DIM * MAX_DIM;

/// A straight-alpha RGBA8 image. Constructors reject anything that does not fit its dimensions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaFrame {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl RgbaFrame {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, MatteError> {
        let expected = rgba_len(width, height)?;
        if pixels.len() != expected {
            return Err(MatteError::InvalidImage(format!("{width}x{height} needs {expected} bytes, got {}", pixels.len())));
        }
        Ok(RgbaFrame { width, height, pixels })
    }

    /// A frame filled with one colour (`[r, g, b, a]`).
    pub fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Result<Self, MatteError> {
        let len = rgba_len(width, height)?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(len).map_err(|_| MatteError::TooLarge { width, height, limit: MAX_PIXELS })?;
        for _ in 0..width.checked_mul(height).ok_or(MatteError::TooLarge { width, height, limit: MAX_PIXELS })? {
            pixels.extend_from_slice(&rgba);
        }
        RgbaFrame::new(width, height, pixels)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The pixels, moved out. Callers that hand the buffer to an encoder (the preview writes a
    /// PNG from it) do not pay for a second full-frame copy.
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }

    /// The four bytes at `(x, y)`; the caller has already checked the bounds.
    pub fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let s = self.pixels.get(i..i + 4).unwrap_or(&[0, 0, 0, 0]);
        [s[0], s[1], s[2], s[3]]
    }

    /// Write one pixel. Out-of-range coordinates are ignored.
    pub fn set(&mut self, x: u32, y: u32, px: [u8; 4]) {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        if let Some(dst) = self.pixels.get_mut(i..i + 4) {
            dst.copy_from_slice(&px);
        }
    }

    /// Scale to `width` x `height` with bilinear sampling (edge-clamped). Alpha is resampled with
    /// the colour channels.
    pub fn resize(&self, width: u32, height: u32) -> Result<RgbaFrame, MatteError> {
        let len = rgba_len(width, height)?;
        if (width, height) == (self.width, self.height) {
            return Ok(self.clone());
        }
        if width == 0 || height == 0 {
            return Err(MatteError::InvalidImage("zero-sized image".into()));
        }
        let mut out = Vec::new();
        out.try_reserve_exact(len).map_err(|_| MatteError::TooLarge { width, height, limit: MAX_PIXELS })?;
        let sx = self.width as f64 / width as f64;
        let sy = self.height as f64 / height as f64;
        for y in 0..height {
            let fy = ((y as f64 + 0.5) * sy - 0.5).clamp(0.0, (self.height - 1) as f64);
            let y0 = fy.floor() as u32;
            let y1 = (y0 + 1).min(self.height - 1);
            let wy = fy - fy.floor();
            for x in 0..width {
                let fx = ((x as f64 + 0.5) * sx - 0.5).clamp(0.0, (self.width - 1) as f64);
                let x0 = fx.floor() as u32;
                let x1 = (x0 + 1).min(self.width - 1);
                let wx = fx - fx.floor();
                let a = self.at(x0, y0);
                let b = self.at(x1, y0);
                let c = self.at(x0, y1);
                let d = self.at(x1, y1);
                let mut px = [0u8; 4];
                for (i, dst) in px.iter_mut().enumerate() {
                    let top = lerp(a[i] as f64, b[i] as f64, wx);
                    let bottom = lerp(c[i] as f64, d[i] as f64, wx);
                    *dst = lerp(top, bottom, wy).round().clamp(0.0, 255.0) as u8;
                }
                out.extend_from_slice(&px);
            }
        }
        RgbaFrame::new(width, height, out)
    }
}

/// An 8-bit coverage mask (0 = background, 255 = subject).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matte {
    width: u32,
    height: u32,
    alpha: Vec<u8>,
}

impl Matte {
    pub fn new(width: u32, height: u32, alpha: Vec<u8>) -> Result<Self, MatteError> {
        let expected = alpha_len(width, height)?;
        if alpha.len() != expected {
            return Err(MatteError::InvalidImage(format!("{width}x{height} matte needs {expected} bytes, got {}", alpha.len())));
        }
        Ok(Matte { width, height, alpha })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn alpha(&self) -> &[u8] {
        &self.alpha
    }

    pub fn coverage(&self, x: u32, y: u32) -> u8 {
        let i = y as usize * self.width as usize + x as usize;
        self.alpha.get(i).copied().unwrap_or(0)
    }

    /// Nearest-neighbour-free bilinear resize. The mask is smooth, so bilinear keeps its edge.
    pub fn resize(&self, width: u32, height: u32) -> Result<Matte, MatteError> {
        let len = alpha_len(width, height)?;
        if (width, height) == (self.width, self.height) {
            return Ok(self.clone());
        }
        if width == 0 || height == 0 {
            return Err(MatteError::InvalidImage("zero-sized matte".into()));
        }
        let mut out = Vec::new();
        out.try_reserve_exact(len).map_err(|_| MatteError::TooLarge { width, height, limit: MAX_PIXELS })?;
        let sx = self.width as f64 / width as f64;
        let sy = self.height as f64 / height as f64;
        for y in 0..height {
            let fy = ((y as f64 + 0.5) * sy - 0.5).clamp(0.0, (self.height - 1) as f64);
            let y0 = fy.floor() as u32;
            let y1 = (y0 + 1).min(self.height - 1);
            let wy = fy - fy.floor();
            for x in 0..width {
                let fx = ((x as f64 + 0.5) * sx - 0.5).clamp(0.0, (self.width - 1) as f64);
                let x0 = fx.floor() as u32;
                let x1 = (x0 + 1).min(self.width - 1);
                let wx = fx - fx.floor();
                let top = lerp(f64::from(self.coverage(x0, y0)), f64::from(self.coverage(x1, y0)), wx);
                let bottom = lerp(f64::from(self.coverage(x0, y1)), f64::from(self.coverage(x1, y1)), wx);
                out.push(lerp(top, bottom, wy).round().clamp(0.0, 255.0) as u8);
            }
        }
        Matte::new(width, height, out)
    }

    /// Soften the edge with a separable box blur, `radius` pixels, done `passes` times (1-2 keeps
    /// the model's own edge; the mask is already smooth, so more is not needed).
    pub fn feather(&self, radius: u32, passes: u32) -> Matte {
        let radius = radius.min(64);
        if radius == 0 {
            return self.clone();
        }
        let mut alpha = self.alpha.clone();
        let mut scratch = vec![0u8; alpha.len()];
        for _ in 0..passes.min(4) {
            box_blur(&alpha, &mut scratch, self.width, self.height, radius, true);
            box_blur(&scratch, &mut alpha, self.width, self.height, radius, false);
        }
        Matte { width: self.width, height: self.height, alpha }
    }
}

/// The byte length of a `width` x `height` RGBA buffer, enforcing the pipeline's bounds.
pub fn rgba_len(width: u32, height: u32) -> Result<usize, MatteError> {
    pixels(width, height).map(|p| p.saturating_mul(4))
}

/// The byte length of a `width` x `height` single-channel buffer, enforcing the same bounds.
pub fn alpha_len(width: u32, height: u32) -> Result<usize, MatteError> {
    pixels(width, height)
}

fn pixels(width: u32, height: u32) -> Result<usize, MatteError> {
    if width == 0 || height == 0 {
        return Err(MatteError::InvalidImage("zero-sized image".into()));
    }
    let pixels = u64::from(width).checked_mul(u64::from(height)).ok_or(MatteError::TooLarge { width, height, limit: MAX_PIXELS })?;
    if pixels > u64::from(MAX_PIXELS) || width > MAX_DIM || height > MAX_DIM {
        return Err(MatteError::TooLarge { width, height, limit: MAX_PIXELS });
    }
    usize::try_from(pixels).map_err(|_| MatteError::TooLarge { width, height, limit: MAX_PIXELS })
}

pub(crate) fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// One-direction box blur (horizontal when `horizontal`, otherwise vertical), clamping at edges.
///
/// A sliding-window sum, so the cost does not grow with the radius: the blur compositor runs 18
/// passes per frame at radius 24 on the caller's thread. Edge samples repeat the last byte, as the
/// naive window did, so the output is unchanged.
pub(crate) fn box_blur(src: &[u8], dst: &mut [u8], width: u32, height: u32, radius: u32, horizontal: bool) {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 {
        return;
    }
    let r = radius as isize;
    let n = 2 * radius + 1;
    let (lines, len, line_stride, item_stride) = if horizontal { (h, w, w, 1) } else { (w, h, 1, w) };
    for line in 0..lines {
        let base = line * line_stride;
        let at = |i: isize| -> u32 {
            let c = i.clamp(0, len as isize - 1) as usize;
            u32::from(src.get(base + c * item_stride).copied().unwrap_or(0))
        };
        let mut sum: u32 = (-r..=r).map(at).sum();
        for i in 0..len {
            if let Some(v) = dst.get_mut(base + i * item_stride) {
                *v = (sum / n).min(255) as u8;
            }
            sum = sum + at(i as isize + r + 1) - at(i as isize - r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_reject_wrong_sizes_and_oversized_images() {
        assert!(RgbaFrame::new(2, 2, vec![0; 15]).is_err());
        assert!(RgbaFrame::new(0, 2, vec![]).is_err());
        assert!(RgbaFrame::new(MAX_DIM + 1, 1, vec![]).is_err());
        assert!(Matte::new(2, 2, vec![0; 3]).is_err());
        let ok = RgbaFrame::new(2, 2, vec![7; 16]).expect("frame");
        assert_eq!(ok.at(1, 1), [7, 7, 7, 7]);
    }

    /// The sliding-window blur must equal the naive window it replaced, edges included.
    #[test]
    fn the_box_blur_matches_the_naive_window() {
        let (w, h) = (7u32, 5u32);
        let src: Vec<u8> = (0..(w * h) as u8).map(|i| i.wrapping_mul(7)).collect();
        let naive = |horizontal: bool, radius: u32| -> Vec<u8> {
            let (wi, hi) = (w as i64, h as i64);
            let r = radius as i64;
            let n = (2 * r + 1) as u32;
            let mut out = vec![0u8; (w * h) as usize];
            for y in 0..hi {
                for x in 0..wi {
                    let mut sum = 0u32;
                    for d in -r..=r {
                        let (sx, sy) = if horizontal { ((x + d).clamp(0, wi - 1), y) } else { (x, (y + d).clamp(0, hi - 1)) };
                        sum += u32::from(src[(sy * wi + sx) as usize]);
                    }
                    out[(y * wi + x) as usize] = (sum / n).min(255) as u8;
                }
            }
            out
        };
        for radius in [0u32, 1, 3, 9] {
            for horizontal in [true, false] {
                let mut got = vec![0u8; (w * h) as usize];
                box_blur(&src, &mut got, w, h, radius, horizontal);
                assert_eq!(got, naive(horizontal, radius), "radius {radius}, horizontal {horizontal}");
            }
        }
    }

    #[test]
    fn resize_scales_and_keeps_edges() {
        let mut px = Vec::new();
        for y in 0..2 {
            for x in 0..2 {
                px.extend_from_slice(&[if x == 0 { 0 } else { 200 }, if y == 0 { 0 } else { 100 }, 50, 255]);
            }
        }
        let f = RgbaFrame::new(2, 2, px).expect("frame");
        let big = f.resize(4, 4).expect("resize");
        assert_eq!((big.width(), big.height()), (4, 4));
        assert_eq!(big.at(0, 0), [0, 0, 50, 255]);
        assert_eq!(big.at(3, 3), [200, 100, 50, 255]);
        let small = big.resize(1, 1).expect("shrink");
        assert_eq!(small.at(0, 0), [100, 50, 50, 255]);
    }

    #[test]
    fn feather_softens_a_hard_edge_without_moving_the_centre() {
        let mut a = vec![0u8; 64];
        for y in 0..8 {
            for x in 4..8 {
                a[y * 8 + x] = 255;
            }
        }
        let m = Matte::new(8, 8, a).expect("matte");
        let f = m.feather(1, 1);
        assert_eq!(f.coverage(6, 6), 255, "inside stays covered");
        assert_eq!(f.coverage(0, 0), 0, "far side stays clear");
        assert!(f.coverage(3, 4) > 0 && f.coverage(3, 4) < 255, "the edge is soft: {}", f.coverage(3, 4));
        assert_eq!(m.feather(0, 1), m, "radius 0 is the same mask");
    }
}
