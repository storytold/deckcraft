//! Y'CbCr → RGBA8 conversion for decoded pictures.

/// Y'CbCr matrix (ISO/IEC 23091-2 MatrixCoefficients).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
}

impl Matrix {
    /// From a `matrix_coefficients` code; unspecified codes pick by picture height (SD → 601).
    pub fn from_code(code: u8, height: u32) -> Matrix {
        match code {
            1 => Matrix::Bt709,
            5 | 6 => Matrix::Bt601,
            9 | 10 => Matrix::Bt2020,
            _ if height <= 576 => Matrix::Bt601,
            _ => Matrix::Bt709,
        }
    }
    /// (Kr, Kb).
    fn k(self) -> (f32, f32) {
        match self {
            Matrix::Bt601 => (0.299, 0.114),
            Matrix::Bt709 => (0.2126, 0.0722),
            Matrix::Bt2020 => (0.2627, 0.0593),
        }
    }
}

/// Borrowed planes of one picture. Chroma planes are `width >> sx` (rounded up) wide.
pub struct Planes<'a, T> {
    pub y: &'a [T],
    pub u: &'a [T],
    pub v: &'a [T],
    pub y_stride: usize,
    pub c_stride: usize,
    pub width: usize,
    pub height: usize,
    /// Chroma subsampling shifts (1 = halved).
    pub sx: u32,
    pub sy: u32,
    pub bits: u32,
    /// No chroma: grey.
    pub mono: bool,
}

/// Convert to tightly packed RGBA8.
pub fn to_rgba<T: Copy + Into<u32>>(p: &Planes<T>, m: Matrix, full_range: bool, max_bytes: u64) -> Option<Vec<u8>> {
    let len = p.width.checked_mul(p.height)?.checked_mul(4)?;
    if len as u64 > max_bytes {
        return None;
    }
    let (kr, kb) = m.k();
    let kg = 1.0 - kr - kb;
    let max = ((1u32 << p.bits) - 1) as f32;
    let scale = (1u32 << p.bits.saturating_sub(8)) as f32;
    // Normalised luma / chroma: Y in 0..1, Cb/Cr in -0.5..0.5.
    let (y_off, y_mul, c_mul) = if full_range { (0.0, 1.0 / max, 1.0 / max) } else { (16.0 * scale, 1.0 / (219.0 * scale), 1.0 / (224.0 * scale)) };
    let mid = (1u32 << (p.bits - 1)) as f32;
    let cr_r = 2.0 * (1.0 - kr);
    let cb_b = 2.0 * (1.0 - kb);
    let cb_g = -2.0 * kb * (1.0 - kb) / kg;
    let cr_g = -2.0 * kr * (1.0 - kr) / kg;
    let mut out = Vec::new();
    out.try_reserve_exact(len).ok()?;
    out.resize(len, 0);
    let px = |v: f32| (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
    for row in 0..p.height {
        let yrow = row * p.y_stride;
        let crow = (row >> p.sy) * p.c_stride;
        let o = &mut out[row * p.width * 4..(row + 1) * p.width * 4];
        for col in 0..p.width {
            let yv: u32 = p.y.get(yrow + col).copied().map(Into::into).unwrap_or(0);
            let yn = (yv as f32 - y_off) * y_mul;
            let (cb, cr) = if p.mono {
                (0.0, 0.0)
            } else {
                let ci = crow + (col >> p.sx);
                let u: u32 = p.u.get(ci).copied().map(Into::into).unwrap_or(mid as u32);
                let v: u32 = p.v.get(ci).copied().map(Into::into).unwrap_or(mid as u32);
                ((u as f32 - mid) * c_mul, (v as f32 - mid) * c_mul)
            };
            let k = col * 4;
            o[k] = px(yn + cr_r * cr);
            o[k + 1] = px(yn + cb_g * cb + cr_g * cr);
            o[k + 2] = px(yn + cb_b * cb);
            o[k + 3] = 255;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(y: u8, u: u8, v: u8, full: bool) -> [u8; 4] {
        let p = Planes { y: &[y], u: &[u], v: &[v], y_stride: 1, c_stride: 1, width: 1, height: 1, sx: 0, sy: 0, bits: 8, mono: false };
        let o = to_rgba(&p, Matrix::Bt709, full, 4).unwrap_or_default();
        [o[0], o[1], o[2], o[3]]
    }

    #[test]
    fn limited_range_black_white_and_red() {
        assert_eq!(one(16, 128, 128, false), [0, 0, 0, 255]);
        assert_eq!(one(235, 128, 128, false), [255, 255, 255, 255]);
        // BT.709 red: Y 63, Cb 102, Cr 240.
        let r = one(63, 102, 240, false);
        assert!(r[0] > 250 && r[1] < 5 && r[2] < 5, "{r:?}");
    }

    #[test]
    fn full_range_grey() {
        assert_eq!(one(128, 128, 128, true), [128, 128, 128, 255]);
    }

    #[test]
    fn ten_bit_white() {
        let y = [940u16];
        let c = [512u16];
        let p = Planes { y: &y, u: &c, v: &c, y_stride: 1, c_stride: 1, width: 1, height: 1, sx: 0, sy: 0, bits: 10, mono: false };
        assert_eq!(to_rgba(&p, Matrix::Bt709, false, 4), Some(vec![255, 255, 255, 255]));
    }
}
