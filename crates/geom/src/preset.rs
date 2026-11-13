//! Preset shape geometry.
//!
//! Each preset is identified by its Office Open XML name (`rect`, `roundRect`, `rightArrow`…) so
//! files round-trip, but every outline here is our own construction in Rust: we re-derive the shapes
//! from what they look like and from the meaning of their adjust values, not from any reference
//! geometry table. Adjust values use the file convention: ratios in 1/100 000 of the shorter side (or
//! of the width/height), angles in 1/60 000 degree.

use kurbo::{Arc, BezPath, Point, Rect, Vec2};
use serde::{Deserialize, Serialize};

mod build;
mod builder;
mod catalog;
mod shapes;

pub use build::build;
use builder::{Corner, Dir, Pb, corner_rect, fill, fill_eo, fill_only, mid_sites, orient, shade, simple, stroke_only, vis};
pub use catalog::{CATALOG, Category, FillMode, Geometry, Handle, PresetInfo, SubPath, info, is_line_like, mitred_by_default};

mod extra;

#[cfg(test)]
mod tests;
