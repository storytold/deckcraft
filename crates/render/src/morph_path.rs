//! Morph of an outline: a frame shape whose geometry turns from the old shape's into its own (a
//! circle into a star), with the points aligned by `deckcraft_geom::morph`.

use deckcraft_geom::Xfrm;
use deckcraft_geom::morph::{Fade, GeometryMorph};
use deckcraft_geom::preset::Geometry;
use deckcraft_model::{Geom, Shape, ShapeId};
use kurbo::Vec2;

use crate::shape_geometry;

/// A frame shape (see [`crate::render_blend`]) whose outline morphs from `from` into its own
/// geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct PathMorph {
    pub id: ShapeId,
    /// The old shape's geometry.
    pub from: Geom,
    /// The boxes of the old and the new shape on their own slides.
    pub from_box: Xfrm,
    pub to_box: Xfrm,
    /// Progress, 0..1 (already eased).
    pub t: f64,
}

/// `now / own`, or 1 when that is no usable scale (a line's zero height).
fn ratio(now: f64, own: f64) -> f64 {
    let r = now / own;
    if own.abs() > 1e-9 && r.is_finite() { r } else { 1.0 }
}

/// The geometry of frame shape `s` at its current size `w` x `h`: both outlines are built at their
/// own sizes (so the alignment does not change while the box grows), then scaled into the box.
/// With a [`Fade`] per sub-path.
pub(crate) fn geometry(pm: &PathMorph, s: &Shape, w: f64, h: f64) -> (Geometry, Vec<Fade>) {
    let a = shape_geometry(&Shape { geom: pm.from.clone(), ..Shape::default() }, pm.from_box.w, pm.from_box.h);
    let b = shape_geometry(s, pm.to_box.w, pm.to_box.h);
    let scale = |x: Xfrm| Vec2::new(ratio(w, x.w), ratio(h, x.h));
    GeometryMorph::new(&a, &b).at(pm.t, scale(pm.from_box), scale(pm.to_box))
}
