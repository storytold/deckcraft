//! The segmentation seam: what a native backend implements, and the two segmenters that need no
//! model (the deterministic ellipse used by tests, and the unavailable one used without an
//! install).

use crate::MatteError;
use crate::frame::{Matte, RgbaFrame};

/// A background segmenter. Called on a worker thread, never on the UI thread: one run can take
/// seconds, and the UI keeps drawing the last matte meanwhile.
pub trait Segmenter: Send {
    /// The subject's coverage at the frame's own size.
    fn matte(&mut self, frame: &RgbaFrame) -> Result<Matte, MatteError>;
    /// A stable name for diagnostics and tests.
    fn name(&self) -> &'static str;
}

/// A deterministic, model-free segmenter: a centred ellipse with a soft edge. Tests and the
/// offscreen preview against a synthetic image use it; it is also the fallback a build without the
/// model can still exercise.
#[derive(Clone, Debug)]
pub struct EllipseSegmenter {
    /// Radius in x, as a fraction of the width.
    pub rx: f64,
    /// Radius in y, as a fraction of the height.
    pub ry: f64,
    /// Soft-edge width, as a fraction of the smaller side (0 is a hard edge).
    pub softness: f64,
}

impl Default for EllipseSegmenter {
    fn default() -> Self {
        EllipseSegmenter { rx: 0.30, ry: 0.40, softness: 0.04 }
    }
}

impl EllipseSegmenter {
    /// The matte an ellipse segmenter produces for a frame, without the trait object.
    pub fn mask(&self, frame: &RgbaFrame) -> Result<Matte, MatteError> {
        let (w, h) = (frame.width(), frame.height());
        let cx = w as f64 / 2.0;
        let cy = h as f64 / 2.0;
        let rx = (self.rx.max(0.0) * w as f64).max(0.5);
        let ry = (self.ry.max(0.0) * h as f64).max(0.5);
        let soft = (self.softness.max(0.0) * f64::from(w.min(h))).max(1.0e-6);
        let mut alpha = Vec::new();
        let len = w as usize * h as usize;
        alpha.try_reserve_exact(len).map_err(|_| MatteError::TooLarge { width: w, height: h, limit: crate::MAX_PIXELS })?;
        for y in 0..h {
            for x in 0..w {
                // Signed distance-like metric: 0 at the ellipse, 1 at one softness beyond it.
                let d = (((x as f64 + 0.5 - cx) / rx).powi(2) + ((y as f64 + 0.5 - cy) / ry).powi(2)).sqrt();
                let edge = (1.0 - d) / (soft / rx.min(ry).max(1.0));
                let a = smoothstep(edge.clamp(-1.0, 1.0));
                alpha.push((a * 255.0).round().clamp(0.0, 255.0) as u8);
            }
        }
        Matte::new(w, h, alpha)
    }
}

fn smoothstep(x: f64) -> f64 {
    let t = (x + 1.0) / 2.0;
    t * t * (3.0 - 2.0 * t)
}

impl Segmenter for EllipseSegmenter {
    fn matte(&mut self, frame: &RgbaFrame) -> Result<Matte, MatteError> {
        self.mask(frame)
    }

    fn name(&self) -> &'static str {
        "ellipse"
    }
}

/// No segmenter: a build without the model, or one whose install failed verification.
#[derive(Clone, Debug)]
pub struct NoSegmenter {
    /// What the user can do (shown in the UI and returned by the engine).
    pub reason: String,
}

impl NoSegmenter {
    pub fn new(reason: impl Into<String>) -> Self {
        NoSegmenter { reason: reason.into() }
    }
}

impl Default for NoSegmenter {
    fn default() -> Self {
        NoSegmenter::new("Camera background segmentation is not available: install the birefnet-lite-512 model")
    }
}

impl Segmenter for NoSegmenter {
    fn matte(&mut self, _frame: &RgbaFrame) -> Result<Matte, MatteError> {
        Err(MatteError::Unavailable(self.reason.clone()))
    }

    fn name(&self) -> &'static str {
        "unavailable"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ellipse_covers_the_centre_and_not_the_corners() {
        let f = RgbaFrame::solid(20, 10, [10, 20, 30, 255]).expect("frame");
        let mut seg = EllipseSegmenter::default();
        let m = seg.matte(&f).expect("matte");
        assert_eq!((m.width(), m.height()), (20, 10));
        assert!(m.coverage(10, 5) >= 250, "centre {}", m.coverage(10, 5));
        assert!(m.coverage(0, 0) <= 5, "corner {}", m.coverage(0, 0));
        // A soft edge lives between them.
        let mid = m.coverage(3, 5);
        assert!(mid < 255 && mid > 0, "edge {mid}");
    }

    #[test]
    fn no_segmenter_names_what_is_missing_and_never_runs() {
        let f = RgbaFrame::solid(2, 2, [0, 0, 0, 255]).expect("frame");
        let mut s = NoSegmenter::default();
        let e = s.matte(&f).expect_err("unavailable");
        assert!(e.message().contains("birefnet-lite-512"), "{}", e.message());
        assert_eq!(s.name(), "unavailable");
    }
}
