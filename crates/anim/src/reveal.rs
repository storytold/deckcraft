//! The soft edged reveal of the clip-style entrance effects: Box, Circle, Diamond, Plus, Wipe and
//! Split.
//!
//! Observed in PowerPoint's video export of a synthetic deck (solid shapes, 30 fps): the revealed
//! part is fully opaque and its edge is a linear ramp, with no opacity fade. The ramp is
//! proportional to the shape's visual bounds on each axis, not a width in points, and one rule fits
//! every shape: with `d` a distance from the centre that is 1 at the edge of the bounds, a pixel
//! shows `clamp((edge - d) / RAMP, 0, 1)`. The edge moves linearly from 0 to `1 + RAMP`, so the
//! ramp has just left the bounds when the effect ends. The "in" directions are the mirror image: a
//! soft hole that closes from the edges.

use serde::{Deserialize, Serialize};

/// Width of the soft edge in units of the distance, where the bounds' edge is 1.
pub const RAMP: f64 = 0.14;

/// What the reveal's front looks like: the level lines of the distance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevealShape {
    /// A rectangle with the bounds' proportions.
    Box,
    /// An ellipse through the corners of the bounds.
    Circle,
    /// A rhombus through the corners of the bounds.
    Diamond,
    /// A cross whose arms grow.
    Plus,
    /// A straight front moving right, from the left edge.
    WipeLeft,
    /// A straight front moving left, from the right edge.
    WipeRight,
    /// A straight front moving down, from the top edge.
    WipeTop,
    /// A straight front moving up, from the bottom edge.
    WipeBottom,
    /// Two fronts moving apart (or together) from a vertical line in the middle.
    SplitVertical,
    /// Two fronts moving apart (or together) from a horizontal line in the middle.
    SplitHorizontal,
}

/// A reveal at one moment: the shape of its front, which way it moves and where the edge is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reveal {
    pub shape: RevealShape,
    /// The front closes in from the edges (the "in" directions) instead of opening from the middle.
    pub inward: bool,
    /// Where the front is, in the units of the distance: 0 to `1 + RAMP` going out, 1 to `-RAMP`
    /// coming in.
    pub edge: f64,
}

impl Reveal {
    /// The reveal at progress `p` (0 to 1) of the effect.
    pub fn new(shape: RevealShape, inward: bool, p: f64) -> Reveal {
        let p = if p.is_finite() { p.clamp(0.0, 1.0) } else { 0.0 };
        let travel = p * (1.0 + RAMP);
        Reveal { shape, inward, edge: if inward { 1.0 - travel } else { travel } }
    }

    /// The distance of the point (`u`, `v`) of the bounds (0 to 1 across, y down) from the middle:
    /// 1 on the edge for Box, Wipe and Split, and at the corners for Circle, Diamond and Plus.
    pub fn distance(&self, u: f64, v: f64) -> f64 {
        let (x, y) = ((2.0 * u - 1.0).abs(), (2.0 * v - 1.0).abs());
        match self.shape {
            RevealShape::Box => x.max(y),
            RevealShape::Circle => x.hypot(y) / std::f64::consts::SQRT_2,
            RevealShape::Diamond => (x + y) / 2.0,
            RevealShape::Plus => x.min(y),
            RevealShape::WipeLeft => u,
            RevealShape::WipeRight => 1.0 - u,
            RevealShape::WipeTop => v,
            RevealShape::WipeBottom => 1.0 - v,
            RevealShape::SplitVertical => x,
            RevealShape::SplitHorizontal => y,
        }
    }

    /// How much of the shape shows at the point (`u`, `v`) of the bounds, 0 to 1.
    pub fn alpha(&self, u: f64, v: f64) -> f64 {
        let d = self.distance(u, v);
        let a = if self.inward { (d - self.edge) / RAMP } else { (self.edge - d) / RAMP };
        if a.is_finite() { a.clamp(0.0, 1.0) } else { 0.0 }
    }
}
