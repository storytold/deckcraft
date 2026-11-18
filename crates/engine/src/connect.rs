//! Connectors glued to shapes: connection sites, snapping line ends to them, and rerouting attached
//! connectors whenever the shapes they connect move, resize or rotate.

use deckcraft_geom::{Affine, Point, Rect, Xfrm};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind};

use crate::Selection;
use crate::cmd::xfrm_of;

/// Connection sites of a top-level shape `s` in slide coordinates (index = site index stored on
/// connectors). For a shape inside a group use [`sites_of`] or [`sites_in`].
pub fn sites(doc: &Presentation, sel: &Selection, s: &Shape) -> Vec<Point> {
    sites_in(doc, sel, s, Affine::IDENTITY)
}

/// Connection sites of `s` in slide coordinates, `to_abs` mapping the space its box lives in (the
/// slide, or the child space of the groups around it) onto the slide.
pub fn sites_in(doc: &Presentation, sel: &Selection, s: &Shape, to_abs: Affine) -> Vec<Point> {
    if s.is_line() || matches!(s.kind, ShapeKind::Group { .. } | ShapeKind::Ink { .. }) {
        return vec![];
    }
    let x = xfrm_of(doc, sel, s);
    let mut local = deckcraft_render::shape_geometry(s, x.w, x.h).sites;
    if local.is_empty() {
        local = vec![Point::new(x.w / 2.0, 0.0), Point::new(0.0, x.h / 2.0), Point::new(x.w / 2.0, x.h), Point::new(x.w, x.h / 2.0)];
    }
    let a = to_abs * x.affine();
    local.into_iter().map(|p| a * p).collect()
}

/// The shape `id` anywhere in `shapes` (a group's member included) with the affine that maps the
/// space its box lives in onto the slide: a connector glues to a grouped shape, as in PowerPoint,
/// and its sites are where the group draws them.
pub fn locate<'a>(doc: &Presentation, sel: &Selection, shapes: &'a [Shape], id: ShapeId) -> Option<(&'a Shape, Affine)> {
    fn rec<'a>(doc: &Presentation, sel: &Selection, shapes: &'a [Shape], id: ShapeId, to_abs: Affine, depth: usize) -> Option<(&'a Shape, Affine)> {
        if depth > 64 {
            return None;
        }
        for s in shapes {
            if s.id == id {
                return Some((s, to_abs));
            }
            if let ShapeKind::Group { children, child } = &s.kind {
                let x = xfrm_of(doc, sel, s);
                let ch = Rect::new(child.x, child.y, child.x + child.w, child.y + child.h);
                let nested = to_abs * deckcraft_geom::group_child_affine(&x, ch);
                if let Some(f) = rec(doc, sel, children, id, nested, depth + 1) {
                    return Some(f);
                }
            }
        }
        None
    }
    rec(doc, sel, shapes, id, Affine::IDENTITY, 0)
}

/// Connection sites of shape `id` anywhere in `shapes`, in slide coordinates; `None` when there is
/// no such shape.
pub fn sites_of(doc: &Presentation, sel: &Selection, shapes: &[Shape], id: ShapeId) -> Option<Vec<Point>> {
    locate(doc, sel, shapes, id).map(|(s, a)| sites_in(doc, sel, s, a))
}

/// The nearest connection site within `radius` of `p` on any visible shape except `exclude`.
pub fn nearest_site(
    doc: &Presentation,
    sel: &Selection,
    shapes: &[Shape],
    p: Point,
    radius: f64,
    exclude: Option<ShapeId>,
) -> Option<(ShapeId, u32, Point)> {
    let mut best: Option<(f64, ShapeId, u32, Point)> = None;
    for s in shapes.iter().filter(|s| Some(s.id) != exclude && !s.hidden) {
        for (i, q) in sites(doc, sel, s).into_iter().enumerate() {
            let d = (q - p).hypot();
            if d <= radius && best.is_none_or(|b| d < b.0) {
                best = Some((d, s.id, i as u32, q));
            }
        }
    }
    best.map(|b| (b.1, b.2, b.3))
}

/// The shape whose sites should show while drawing or dragging a connector end near `p`.
pub fn site_shape_at(doc: &Presentation, sel: &Selection, shapes: &[Shape], p: Point, margin: f64, exclude: Option<ShapeId>) -> Option<ShapeId> {
    shapes
        .iter()
        .rev()
        .filter(|s| Some(s.id) != exclude && !s.hidden && !s.is_line() && !matches!(s.kind, ShapeKind::Ink { .. }))
        .find(|s| xfrm_of(doc, sel, s).bounds().inflate(margin, margin).contains(p))
        .map(|s| s.id)
}

/// Line endpoints (start, end) of a line-like shape's box.
pub fn endpoints(x: &Xfrm) -> (Point, Point) {
    let a = x.affine();
    (a * Point::new(0.0, 0.0), a * Point::new(x.w, x.h))
}

/// Move the ends of connectors glued to shapes on the edited slide to the current site positions.
/// Connections to shapes that no longer exist are dropped.
pub fn reroute(doc: &mut Presentation, sel: &Selection) {
    let Some(list) = crate::shapes_mut(doc, sel) else { return };
    if !list.iter().any(|s| matches!(s.kind, ShapeKind::Connector { start: Some(_), .. } | ShapeKind::Connector { end: Some(_), .. })) {
        return;
    }
    let snapshot: Vec<Shape> = list.clone();
    let view = doc.clone();
    let site = |id: ShapeId, idx: u32| -> Option<Point> {
        // Anywhere in the tree: a connector glued to a shape that is then grouped keeps following it.
        let v = sites_of(&view, sel, &snapshot, id)?;
        v.get(idx as usize).or(v.first()).copied()
    };
    let mut updates: Vec<(usize, Xfrm, bool, bool)> = vec![];
    for (i, s) in snapshot.iter().enumerate() {
        let ShapeKind::Connector { start, end } = &s.kind else { continue };
        if start.is_none() && end.is_none() {
            continue;
        }
        let x = xfrm_of(&view, sel, s);
        let (mut p0, mut p1) = endpoints(&x);
        let a = start.and_then(|(id, k)| site(id, k));
        let b = end.and_then(|(id, k)| site(id, k));
        if let Some(q) = a {
            p0 = q;
        }
        if let Some(q) = b {
            p1 = q;
        }
        let nx = crate::tools::line_xfrm(p0, p1, x.rot);
        let changed = (nx.x - x.x).abs() > 1e-6
            || (nx.y - x.y).abs() > 1e-6
            || (nx.w - x.w).abs() > 1e-6
            || (nx.h - x.h).abs() > 1e-6
            || nx.flip_h != x.flip_h
            || nx.flip_v != x.flip_v;
        let drop_a = start.is_some() && a.is_none();
        let drop_b = end.is_some() && b.is_none();
        if changed || drop_a || drop_b {
            updates.push((i, nx, drop_a, drop_b));
        }
    }
    let Some(list) = crate::shapes_mut(doc, sel) else { return };
    for (i, nx, drop_a, drop_b) in updates {
        if let Some(s) = list.get_mut(i) {
            s.xfrm = Some(nx);
            if let ShapeKind::Connector { start, end } = &mut s.kind {
                if drop_a {
                    *start = None;
                }
                if drop_b {
                    *end = None;
                }
            }
        }
    }
}

/// Glue end `end` (0 = start, 1 = end) of line `id` to `to` (or unglue with `None`). Plain lines
/// become connectors when glued.
pub fn set_glue(list: &mut [Shape], id: ShapeId, end: u8, to: Option<(ShapeId, u32)>) {
    let Some(s) = list.iter_mut().find(|s| s.id == id) else { return };
    if !matches!(s.kind, ShapeKind::Connector { .. }) {
        if to.is_none() || !matches!(s.kind, ShapeKind::Shape) {
            return;
        }
        s.kind = ShapeKind::Connector { start: None, end: None };
    }
    if let ShapeKind::Connector { start, end: e } = &mut s.kind {
        if end == 0 {
            *start = to;
        } else {
            *e = to;
        }
    }
}
