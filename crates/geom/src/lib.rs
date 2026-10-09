//! DeckCraft geometry: units, shape transforms and preset shape generators.
//!
//! The document model keeps positions in points (1/72 inch) as `f64`. Office Open XML stores EMU
//! (English Metric Units, 12 700 per point); [`emu_to_pt`] and [`pt_to_emu`] convert losslessly for
//! integer EMU values, so files round-trip exactly.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod morph;
pub mod preset;

pub use kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape as KurboShape, Size, Vec2};
use serde::{Deserialize, Serialize};

/// EMU per point.
pub const EMU_PER_PT: f64 = 12_700.0;
/// EMU per inch.
pub const EMU_PER_IN: f64 = 914_400.0;
/// EMU per centimetre.
pub const EMU_PER_CM: f64 = 360_000.0;

pub fn emu_to_pt(emu: i64) -> f64 {
    emu as f64 / EMU_PER_PT
}

/// Points to EMU, rounded. Non-finite input becomes 0; huge values saturate.
pub fn pt_to_emu(pt: f64) -> i64 {
    if !pt.is_finite() {
        return 0;
    }
    let v = (pt * EMU_PER_PT).round();
    if v >= i64::MAX as f64 {
        i64::MAX
    } else if v <= i64::MIN as f64 {
        i64::MIN
    } else {
        v as i64
    }
}

/// Display units for measurements (inches on US English systems, centimetres elsewhere).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Unit {
    #[default]
    Inches,
    Centimeters,
    Points,
    Pixels,
}

impl Unit {
    pub fn points_per_unit(self) -> f64 {
        match self {
            Unit::Inches => 72.0,
            Unit::Centimeters => 72.0 / 2.54,
            Unit::Points => 1.0,
            Unit::Pixels => 0.75,
        }
    }
    pub fn suffix(self) -> &'static str {
        match self {
            Unit::Inches => "\"",
            Unit::Centimeters => " cm",
            Unit::Points => " pt",
            Unit::Pixels => " px",
        }
    }
    /// `2.08"` style text for a length in points.
    pub fn format(self, pt: f64) -> String {
        let v = pt / self.points_per_unit();
        let s = format!("{v:.2}");
        format!("{s}{}", self.suffix())
    }
    /// Parses `2.5`, `2.5"`, `2.5 in`, `6 cm`, `30 pt`, `40px`, `5mm` into points. A bare number uses `self`.
    pub fn parse(self, text: &str) -> Option<f64> {
        let t = text.trim().to_ascii_lowercase();
        let (num, unit) = split_number(&t)?;
        let ppu = match unit.trim() {
            "" => self.points_per_unit(),
            "\"" | "in" | "inch" | "inches" => 72.0,
            "cm" => 72.0 / 2.54,
            "mm" => 72.0 / 25.4,
            "pt" | "pts" => 1.0,
            "px" => 0.75,
            _ => return None,
        };
        let v = num * ppu;
        v.is_finite().then_some(v)
    }
}

fn split_number(t: &str) -> Option<(f64, &str)> {
    let end = t.char_indices().find(|(_, c)| !(c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')).map(|(i, _)| i).unwrap_or(t.len());
    let n: f64 = t.get(..end)?.parse().ok()?;
    Some((n, t.get(end..).unwrap_or("")))
}

/// A shape's placement on its slide (or in its group's child space): offset, extent, rotation
/// (degrees clockwise) and flips. Rotation and flips are about the centre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Xfrm {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub rot: f64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub flip_h: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub flip_v: bool,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl Xfrm {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Xfrm { x, y, w, h, ..Default::default() }
    }
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.x + self.w, self.y + self.h)
    }
    pub fn center(&self) -> Point {
        Point::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    /// Local shape space (0..w, 0..h) → parent space, applying flips and rotation about the centre.
    pub fn affine(&self) -> Affine {
        let c = Vec2::new(self.w / 2.0, self.h / 2.0);
        let sx = if self.flip_h { -1.0 } else { 1.0 };
        let sy = if self.flip_v { -1.0 } else { 1.0 };
        Affine::translate(Vec2::new(self.x, self.y) + c)
            * Affine::rotate(self.rot.to_radians())
            * Affine::scale_non_uniform(sx, sy)
            * Affine::translate(-c)
    }
    /// Axis-aligned bounds of the rotated box in parent space.
    pub fn bounds(&self) -> Rect {
        let a = self.affine();
        let pts = [Point::new(0.0, 0.0), Point::new(self.w, 0.0), Point::new(self.w, self.h), Point::new(0.0, self.h)].map(|p| a * p);
        let mut r = Rect::from_points(pts[0], pts[1]);
        r = r.union_pt(pts[2]).union_pt(pts[3]);
        r
    }
    /// True when no dimension is NaN/inf.
    pub fn is_finite(&self) -> bool {
        [self.x, self.y, self.w, self.h, self.rot].iter().all(|v| v.is_finite())
    }
    /// Rotation normalised to 0..360.
    pub fn rot_norm(&self) -> f64 {
        let r = self.rot % 360.0;
        if r < 0.0 { r + 360.0 } else { r }
    }
}

/// Maps a group's child coordinate space (`ch_off`, `ch_ext`) onto the group's own box.
pub fn group_child_affine(group: &Xfrm, ch: Rect) -> Affine {
    let sx = if ch.width().abs() > 1e-9 { group.w / ch.width() } else { 1.0 };
    let sy = if ch.height().abs() > 1e-9 { group.h / ch.height() } else { 1.0 };
    group.affine() * Affine::scale_non_uniform(sx, sy) * Affine::translate(-ch.origin().to_vec2())
}

/// Distance from `p` to the segment `a`–`b`.
pub fn dist_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let len2 = ab.hypot2();
    if len2 < 1e-12 {
        return (p - a).hypot();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).hypot()
}

/// Is `p` within `tol` of the outline of `path` (flattened)?
pub fn near_path(path: &BezPath, p: Point, tol: f64) -> bool {
    let mut hit = false;
    let mut start = Point::ZERO;
    let mut last = Point::ZERO;
    kurbo::flatten(path.iter(), 0.25, |el| {
        if hit {
            return;
        }
        match el {
            PathEl::MoveTo(q) => {
                start = q;
                last = q;
            }
            PathEl::LineTo(q) => {
                if dist_to_segment(p, last, q) <= tol {
                    hit = true;
                }
                last = q;
            }
            PathEl::ClosePath => {
                if dist_to_segment(p, last, start) <= tol {
                    hit = true;
                }
                last = start;
            }
            _ => {}
        }
    });
    hit
}

/// Snap `v` to the nearest multiple of `step` (step <= 0 returns `v`).
pub fn snap(v: f64, step: f64) -> f64 {
    if step > 0.0 && step.is_finite() { (v / step).round() * step } else { v }
}

/// Flatten a path to move/line/close elements (tolerance 0.2).
pub fn preset_flatten(path: &BezPath, f: &mut dyn FnMut(PathEl)) {
    kurbo::flatten(path.iter(), 0.2, f);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emu_roundtrip_is_exact() {
        for emu in [0i64, 1, 12_699, 12_700, 914_400, 12_192_000, 6_858_000, -5, 123_456_789] {
            assert_eq!(pt_to_emu(emu_to_pt(emu)), emu);
        }
        assert_eq!(pt_to_emu(f64::NAN), 0);
        assert_eq!(pt_to_emu(f64::INFINITY), 0);
    }

    #[test]
    fn unit_parse_and_format() {
        assert_eq!(Unit::Inches.parse("2"), Some(144.0));
        assert_eq!(Unit::Inches.parse("2\""), Some(144.0));
        assert!((Unit::Inches.parse("2.54 cm").unwrap() - 72.0).abs() < 1e-9);
        assert_eq!(Unit::Inches.parse("36pt"), Some(36.0));
        assert_eq!(Unit::Inches.parse("abc"), None);
        assert_eq!(Unit::Inches.parse(""), None);
        assert_eq!(Unit::Inches.format(150.0), "2.08\"");
    }

    #[test]
    fn xfrm_affine_rotation_about_center() {
        let x = Xfrm { x: 10.0, y: 20.0, w: 100.0, h: 50.0, rot: 90.0, ..Default::default() };
        let c = x.affine() * Point::new(50.0, 25.0);
        assert!((c.x - 60.0).abs() < 1e-9 && (c.y - 45.0).abs() < 1e-9);
        let b = x.bounds();
        assert!((b.width() - 50.0).abs() < 1e-9 && (b.height() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn flips_mirror_local_space() {
        let x = Xfrm { x: 0.0, y: 0.0, w: 100.0, h: 50.0, flip_h: true, ..Default::default() };
        let p = x.affine() * Point::new(0.0, 0.0);
        assert!((p.x - 100.0).abs() < 1e-9 && p.y.abs() < 1e-9);
    }

    #[test]
    fn group_child_mapping() {
        let g = Xfrm::new(100.0, 100.0, 200.0, 100.0);
        let a = group_child_affine(&g, Rect::new(0.0, 0.0, 100.0, 50.0));
        let p = a * Point::new(100.0, 50.0);
        assert!((p.x - 300.0).abs() < 1e-9 && (p.y - 200.0).abs() < 1e-9);
    }

    #[test]
    fn segment_distance() {
        assert!((dist_to_segment(Point::new(5.0, 5.0), Point::new(0.0, 0.0), Point::new(10.0, 0.0)) - 5.0).abs() < 1e-9);
        assert!((dist_to_segment(Point::new(0.0, 3.0), Point::new(0.0, 0.0), Point::new(0.0, 0.0)) - 3.0).abs() < 1e-9);
    }
}
