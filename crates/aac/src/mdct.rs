//! MDCT / IMDCT (ISO/IEC 14496-3 §4.6.18) via a DCT-IV computed with an N/4-point complex FFT,
//! plus the sine and Kaiser–Bessel-derived windows.

use std::f64::consts::PI;
use std::sync::OnceLock;

/// Radix-2 complex FFT (forward, `e^{-2πi nk/P}`).
struct Fft {
    n: usize,
    twiddle: Vec<[f32; 2]>,
    rev: Vec<u32>,
}

impl Fft {
    fn new(n: usize) -> Fft {
        assert!(n.is_power_of_two());
        let bits = n.trailing_zeros();
        let rev = (0..n as u32).map(|i| if bits == 0 { 0 } else { i.reverse_bits() >> (32 - bits) }).collect();
        let twiddle = (0..n / 2)
            .map(|k| {
                let a = -2.0 * PI * k as f64 / n as f64;
                [a.cos() as f32, a.sin() as f32]
            })
            .collect();
        Fft { n, twiddle, rev }
    }

    fn run(&self, re: &mut [f32], im: &mut [f32]) {
        let n = self.n;
        for i in 0..n {
            let j = self.rev[i] as usize;
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let step = n / len;
            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let [wr, wi] = self.twiddle[k * step];
                    let a = start + k;
                    let b = a + half;
                    let xr = re[b] * wr - im[b] * wi;
                    let xi = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - xr;
                    im[b] = im[a] - xi;
                    re[a] += xr;
                    im[a] += xi;
                }
            }
            len <<= 1;
        }
    }
}

/// MDCT of `2M` inputs to `M` outputs (unscaled sums):
/// `X[k] = Σ x[n] cos(π/M (n + ½ + M/2)(k + ½))`, and the matching unscaled inverse.
pub struct Mdct {
    m: usize,
    fft: Fft,
    pre: Vec<[f32; 2]>,
    post: Vec<[f32; 2]>,
}

impl Mdct {
    pub fn new(m: usize) -> Mdct {
        let q = m / 2;
        let pre = (0..q)
            .map(|n| {
                let a = -PI * n as f64 / m as f64;
                [a.cos() as f32, a.sin() as f32]
            })
            .collect();
        let post = (0..q)
            .map(|k| {
                let a = -PI * (k as f64 + 0.25) / m as f64;
                [a.cos() as f32, a.sin() as f32]
            })
            .collect();
        Mdct { m, fft: Fft::new(q), pre, post }
    }

    /// DCT-IV: `out[k] = Σ u[n] cos(π/M (n+½)(k+½))`.
    fn dct4(&self, u: &[f32], out: &mut [f32]) {
        let m = self.m;
        let q = m / 2;
        let mut re = vec![0f32; q];
        let mut im = vec![0f32; q];
        for n in 0..q {
            let (a, b) = (u[2 * n], u[m - 1 - 2 * n]);
            let [c, s] = self.pre[n];
            re[n] = a * c - b * s;
            im[n] = a * s + b * c;
        }
        self.fft.run(&mut re, &mut im);
        for k in 0..q {
            let [c, s] = self.post[k];
            let wr = re[k] * c - im[k] * s;
            let wi = re[k] * s + im[k] * c;
            out[2 * k] = wr;
            out[m - 1 - 2 * k] = -wi;
        }
    }

    /// Forward MDCT of `x` (length 2M, already windowed) into `out` (length M).
    pub fn forward(&self, x: &[f32], out: &mut [f32]) {
        let m = self.m;
        let h = m / 2;
        let mut u = vec![0f32; m];
        for n in 0..h {
            // quarters a, b, c, d of length M/2
            u[n] = -x[m + h - 1 - n] - x[m + h + n];
            u[h + n] = x[n] - x[m - 1 - n];
        }
        self.dct4(&u, out);
    }

    /// Inverse MDCT (unscaled): `y[n] = Σ X[k] cos(π/M (n + ½ + M/2)(k + ½))`, `y` has length 2M.
    pub fn inverse(&self, spec: &[f32], y: &mut [f32]) {
        let m = self.m;
        let h = m / 2;
        let mut u = vec![0f32; m];
        self.dct4(spec, &mut u);
        for n in 0..h {
            y[n] = u[h + n];
            y[m - 1 - n] = -u[h + n];
            y[m + h - 1 - n] = -u[n];
            y[m + h + n] = -u[n];
        }
    }
}

/// Cached transforms for 1024 and 128 lines.
pub fn mdct_long() -> &'static Mdct {
    static M: OnceLock<Mdct> = OnceLock::new();
    M.get_or_init(|| Mdct::new(1024))
}
pub fn mdct_short() -> &'static Mdct {
    static M: OnceLock<Mdct> = OnceLock::new();
    M.get_or_init(|| Mdct::new(128))
}

/// Window shape (`window_shape` bit): 0 = sine, 1 = Kaiser–Bessel derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowShape {
    #[default]
    Sine,
    Kbd,
}

impl WindowShape {
    pub fn from_bit(b: bool) -> Self {
        if b { WindowShape::Kbd } else { WindowShape::Sine }
    }
    pub fn bit(self) -> u32 {
        (self == WindowShape::Kbd) as u32
    }
}

fn sine_half(m: usize) -> Vec<f32> {
    let n = 2 * m;
    (0..m).map(|i| (PI / n as f64 * (i as f64 + 0.5)).sin() as f32).collect()
}

fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let y = x * x / 4.0;
    for k in 1..60 {
        term *= y / (k as f64 * k as f64);
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

fn kbd_half(m: usize, alpha: f64) -> Vec<f32> {
    // w[n] = sqrt(Σ_{j<=n} W(j) / Σ_{j<=M} W(j)), W the Kaiser kernel of length M+1.
    let kernel: Vec<f64> = (0..=m)
        .map(|j| {
            let r = (j as f64 - m as f64 / 2.0) / (m as f64 / 2.0);
            bessel_i0(PI * alpha * (1.0 - r * r).max(0.0).sqrt())
        })
        .collect();
    let total: f64 = kernel.iter().sum();
    let mut acc = 0.0;
    (0..m)
        .map(|n| {
            acc += kernel[n];
            (acc / total).sqrt() as f32
        })
        .collect()
}

/// Rising half (length N/2) of the window of the given shape; `long` selects N = 2048, else N = 256.
pub fn window(shape: WindowShape, long: bool) -> &'static [f32] {
    static W: OnceLock<[Vec<f32>; 4]> = OnceLock::new();
    let w = W.get_or_init(|| [sine_half(1024), kbd_half(1024, 4.0), sine_half(128), kbd_half(128, 6.0)]);
    let i = match (long, shape) {
        (true, WindowShape::Sine) => 0,
        (true, WindowShape::Kbd) => 1,
        (false, WindowShape::Sine) => 2,
        (false, WindowShape::Kbd) => 3,
    };
    &w[i]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct(x: &[f32]) -> Vec<f64> {
        let m = x.len() / 2;
        (0..m).map(|k| (0..2 * m).map(|n| x[n] as f64 * (PI / m as f64 * (n as f64 + 0.5 + m as f64 / 2.0) * (k as f64 + 0.5)).cos()).sum()).collect()
    }

    #[test]
    fn forward_matches_definition() {
        for m in [128usize, 1024] {
            let x: Vec<f32> = (0..2 * m).map(|i| ((i * 7919) % 101) as f32 / 50.0 - 1.0).collect();
            let mut out = vec![0f32; m];
            Mdct::new(m).forward(&x, &mut out);
            let d = direct(&x);
            for k in 0..m {
                assert!((out[k] as f64 - d[k]).abs() < 1e-3 * (m as f64).sqrt(), "m={m} k={k} {} {}", out[k], d[k]);
            }
        }
    }

    #[test]
    fn inverse_matches_definition() {
        let m = 128;
        let spec: Vec<f32> = (0..m).map(|i| ((i * 31) % 17) as f32 - 8.0).collect();
        let mut y = vec![0f32; 2 * m];
        Mdct::new(m).inverse(&spec, &mut y);
        for n in 0..2 * m {
            let d: f64 = (0..m).map(|k| spec[k] as f64 * (PI / m as f64 * (n as f64 + 0.5 + m as f64 / 2.0) * (k as f64 + 0.5)).cos()).sum();
            assert!((y[n] as f64 - d).abs() < 1e-3, "n={n}");
        }
    }

    #[test]
    fn tdac_perfect_reconstruction() {
        let m = 1024;
        let t = Mdct::new(m);
        for shape in [WindowShape::Sine, WindowShape::Kbd] {
            let w = window(shape, true);
            let win = |n: usize| if n < m { w[n] } else { w[2 * m - 1 - n] };
            let x: Vec<f32> = (0..4 * m).map(|i| ((i as f32) * 0.013).sin() + ((i * 13) % 7) as f32 * 0.05).collect();
            let mut out = vec![0f32; 4 * m];
            for f in 0..3 {
                let seg: Vec<f32> = (0..2 * m).map(|n| x[f * m + n] * win(n)).collect();
                let mut spec = vec![0f32; m];
                t.forward(&seg, &mut spec);
                for v in spec.iter_mut() {
                    *v *= 2.0;
                }
                let mut y = vec![0f32; 2 * m];
                t.inverse(&spec, &mut y);
                for n in 0..2 * m {
                    out[f * m + n] += y[n] * win(n) / m as f32;
                }
            }
            for n in m..3 * m {
                assert!((out[n] - x[n]).abs() < 1e-4, "{shape:?} n={n} {} {}", out[n], x[n]);
            }
        }
    }
}
