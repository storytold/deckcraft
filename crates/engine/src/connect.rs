//! Connectors glued to shapes: connection sites, snapping line ends to them, and rerouting attached
//! connectors whenever the shapes they connect move, resize or rotate.

use deckcraft_geom::{Affine, Point, Xfrm};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind};

use crate::Selection;
use crate::cmd::xfrm_of;

/// Connection sites of `s` in slide coordinates (index = site index stored on connectors).
///
/// A preset's sites come from its geometry, a custom geometry's from its own list (`a:cxnLst`,
/// `Shape::sites`), and a shape without any offers its box midpoints. A group has none of its
/// own: it offers its members' own sites (a grouped circuit symbol's pins; groups within
/// included), so a connector can glue to those; a group whose members have none stays without.
pub fn sites(doc: &Presentation, sel: &Selection, s: &Shape) -> Vec<Point> {
    if s.is_line() || matches!(s.kind, ShapeKind::Ink { .. }) {
        return vec![];
    }
    let x = xfrm_of(doc, sel, s);
    if let ShapeKind::Group { children, child } = &s.kind {
        let mut out = vec![];
        member_sites(children, deckcraft_geom::group_child_affine(&x, child.rect()), &mut out, 0);
        return out;
    }
    let mut local = deckcraft_render::shape_geometry(s, x.w, x.h).sites;
    if local.is_empty() {
        local = vec![Point::new(x.w / 2.0, 0.0), Point::new(0.0, x.h / 2.0), Point::new(x.w / 2.0, x.h), Point::new(x.w, x.h / 2.0)];
    }
    let a = x.affine();
    local.into_iter().map(|p| a * p).collect()
}

/// The members' own sites (`Shape::sites`), in walk order, through `a` (child space to slide).
fn member_sites(children: &[Shape], a: Affine, out: &mut Vec<Point>, depth: usize) {
    if depth > 64 {
        return;
    }
    for c in children {
        let Some(cx) = c.xfrm else { continue };
        match &c.kind {
            ShapeKind::Group { children, child } => member_sites(children, a * deckcraft_geom::group_child_affine(&cx, child.rect()), out, depth + 1),
            ShapeKind::Ink { .. } => {}
            _ if c.is_line() => {}
            _ => {
                let m = a * cx.affine();
                out.extend(c.sites.iter().map(|q| m * Point::new(q.x * cx.w, q.y * cx.h)));
            }
        }
    }
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
        let s = snapshot.iter().find(|s| s.id == id)?;
        let v = sites(&view, sel, s);
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
