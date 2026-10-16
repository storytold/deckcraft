//! Units the panels show. The ribbon layout uses PowerPoint's: points for type and lines, inches
//! for sizes and positions. The canvas layout uses pixels at 2 px per point, Figma Slides' scale (a
//! 16:9 slide is 1920 × 1080 px, a 44 pt title is 88 px). Presentations store neither: PPTX keeps
//! EMU (12,700 per point, so 6,350 per pixel here, exactly), and every command takes points.

use crate::{Shell, SlideApp};

/// Pixels per point in the canvas layout.
pub const PX_PER_PT: f64 = 2.0;

/// A unit for values the engine keeps in points: shown = points × `per_pt`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unit {
    pub per_pt: f64,
    pub suffix: &'static str,
}

impl Unit {
    pub fn show(self, pt: f64) -> f64 {
        pt * self.per_pt
    }
    pub fn to_pt(self, shown: f64) -> f64 {
        shown / self.per_pt
    }
}

pub const PX: Unit = Unit { per_pt: PX_PER_PT, suffix: " px" };
pub const PT: Unit = Unit { per_pt: 1.0, suffix: " pt" };
pub const INCH: Unit = Unit { per_pt: 1.0 / 72.0, suffix: "\"" };

pub fn pixels(app: &SlideApp) -> bool {
    app.ui.shell == Shell::Canvas
}

/// Type sizes, line widths, margins, effect sizes.
pub fn small(app: &SlideApp) -> Unit {
    if pixels(app) { PX } else { PT }
}

/// Sizes and positions of objects.
pub fn length(app: &SlideApp) -> Unit {
    if pixels(app) { PX } else { INCH }
}

/// Zoom as shown: in pixels, 100% is one slide pixel per screen point (so a 1920 px slide fits a
/// 1920-point window), as in Figma; otherwise one slide point per screen point.
pub fn zoom_shown(app: &SlideApp, scale: f32) -> f32 {
    if pixels(app) { scale / PX_PER_PT as f32 } else { scale }
}

/// The `view.zoom` percent for a zoom shown as `percent`.
pub fn zoom_percent(app: &SlideApp, percent: f64) -> f64 {
    if pixels(app) { percent * PX_PER_PT } else { percent }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_convert_exactly() {
        assert_eq!(PX.show(44.0), 88.0);
        assert_eq!(PX.to_pt(1920.0), 960.0, "a 16:9 slide is 1920 px wide");
        assert_eq!(PX.to_pt(1.0) * 12700.0, 6350.0, "one pixel is a whole number of EMU");
        assert!((INCH.show(72.0) - 1.0).abs() < 1e-12);
        assert_eq!(PT.show(18.0), 18.0);
    }
}
