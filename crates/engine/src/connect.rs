//! Connectors glued to shapes: connection sites, snapping line ends to them, and rerouting attached
//! connectors whenever the shapes they connect move, resize or rotate.

use deckcraft_geom::{Affine, Point, Xfrm};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind};

use crate::Selection;
use crate::cmd::xfrm_of;

/// A connection site in slide coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Site {
    pub at: Point,
    /// What a connector glued here names: the shape and that shape's own site index. A group's
    /// sites are its members', so this names the member, as Open XML's `stCxn`/`endCxn` do.
    pub target: (ShapeId, u32),
    /// Offered for snapping and gluing: a hidden member's sites are not, though a connector
    /// already glued to one stays glued.
    pub offered: bool,
}

/// A shape's own sites in its `w`×`h` box: its geometry's (a preset's, a custom geometry's own
/// list, `a:cxnLst`), else the box midpoints; `false` with the midpoints.
fn local_sites(s: &Shape, w: f64, h: f64) -> (Vec<Point>, bool) {
    // A custom geometry's sites are its list (what `shape_geometry` gives too, without building
    // its paths: a slide of grouped symbols is walked on every pointer move).
    let local = match &s.geom {
        deckcraft_model::Geom::Custom { .. } => s.sites.iter().map(|q| Point::new(q.x * w, q.y * h)).collect(),
        deckcraft_model::Geom::Preset { .. } => deckcraft_render::shape_geometry(s, w, h).sites,
    };
    if local.is_empty() {
        (vec![Point::new(w / 2.0, 0.0), Point::new(0.0, h / 2.0), Point::new(w / 2.0, h), Point::new(w, h / 2.0)], false)
    } else {
        (local, true)
    }
}

fn has_sites(s: &Shape) -> bool {
    !s.is_line() && !matches!(s.kind, ShapeKind::Ink { .. })
}

/// Connection sites of `s` (a shape on the edited slide, or a member of a group there) in slide
/// coordinates, in the order the index of `shape.sites` / `shape.connect` refers to.
///
/// A preset's sites come from its geometry, a custom geometry's from its own list (`a:cxnLst`,
/// `Shape::sites`), and a shape without any offers its box midpoints. A group has none of its
/// own: it offers its members' own custom-geometry sites (a grouped circuit symbol's pins;
/// groups within included), each naming the member and its
/// own index, so a connector glued to one keeps it however the group's members change, and
/// after ungrouping. A hidden member's sites are listed but not offered.
pub fn site_list(doc: &Presentation, sel: &Selection, s: &Shape) -> Vec<Site> {
    let (a, w, h) =
        crate::cmd::current_shapes(doc, sel).and_then(|list| locate(doc, sel, list, s.id)).map(|(_, a, w, h)| (a, w, h)).unwrap_or_else(|| {
            let x = xfrm_of(doc, sel, s);
            (x.affine(), x.w, x.h)
        });
    placed_sites(s, a, w, h)
}

/// [`site_list`] of `s` placed by `a` (its box to the slide) with a `w`×`h` box.
fn placed_sites(s: &Shape, a: Affine, w: f64, h: f64) -> Vec<Site> {
    if !has_sites(s) {
        return vec![];
    }
    if let ShapeKind::Group { children, child } = &s.kind {
        let mut out = vec![];
        let g = Xfrm::new(0.0, 0.0, w, h);
        member_sites(children, a * deckcraft_geom::group_child_affine(&g, child.rect()), true, &mut out, 0);
        return out;
    }
    let (local, _) = local_sites(s, w, h);
    local.into_iter().enumerate().map(|(i, p)| Site { at: a * p, target: (s.id, i as u32), offered: true }).collect()
}

fn member_sites(children: &[Shape], a: Affine, visible: bool, out: &mut Vec<Site>, depth: usize) {
    if depth > 64 {
        return;
    }
    for c in children.iter().filter(|c| has_sites(c)) {
        let Some(cx) = c.xfrm else { continue };
        let visible = visible && !c.hidden;
        if let ShapeKind::Group { children, child } = &c.kind {
            member_sites(children, a * deckcraft_geom::group_child_affine(&cx, child.rect()), visible, out, depth + 1);
            continue;
        }
        let (local, own) = local_sites(c, cx.w, cx.h);
        if own && matches!(c.geom, deckcraft_model::Geom::Custom { .. }) {
            let m = a * cx.affine();
            out.extend(local.into_iter().enumerate().map(|(i, p)| Site { at: m * p, target: (c.id, i as u32), offered: visible }));
        }
    }
}

/// The shape `id` on the edited slide (`list`), groups within included, with what places its
/// box on the slide and the box's size. A member without a transform of its own is not found.
fn locate<'a>(doc: &Presentation, sel: &Selection, list: &'a [Shape], id: ShapeId) -> Option<(&'a Shape, Affine, f64, f64)> {
    fn rec(children: &[Shape], a: Affine, id: ShapeId, depth: usize) -> Option<(&Shape, Affine, f64, f64)> {
        if depth > 64 {
            return None;
        }
        for c in children {
            let Some(cx) = c.xfrm else { continue };
            if c.id == id {
                return Some((c, a * cx.affine(), cx.w, cx.h));
            }
            if let ShapeKind::Group { children, child } = &c.kind
                && let Some(r) = rec(children, a * deckcraft_geom::group_child_affine(&cx, child.rect()), id, depth + 1)
            {
                return Some(r);
            }
        }
        None
    }
    for s in list {
        let found = s.id == id;
        if !found && !s.is_group() {
            continue;
        }
        let x = xfrm_of(doc, sel, s);
        if found {
            return Some((s, x.affine(), x.w, x.h));
        }
        if let ShapeKind::Group { children, child } = &s.kind
            && let Some(r) = rec(children, deckcraft_geom::group_child_affine(&x, child.rect()), id, 1)
        {
            return Some(r);
        }
    }
    None
}

/// Where the site a connector end names (`id`, the shape's own site `idx`) is now, on the
/// edited slide (`list`): a shape there or a member of a group there. An index past the shape's
/// sites means its first. `None` when the shape is gone, or names a group (which has no sites of
/// its own) or a line.
fn site_at(doc: &Presentation, sel: &Selection, list: &[Shape], id: ShapeId, idx: u32) -> Option<Point> {
    let (s, a, w, h) = locate(doc, sel, list, id)?;
    if !has_sites(s) || s.is_group() {
        return None;
    }
    let (local, _) = local_sites(s, w, h);
    local.get(idx as usize).or(local.first()).map(|p| a * *p)
}

/// The positions of [`site_list`].
pub fn sites(doc: &Presentation, sel: &Selection, s: &Shape) -> Vec<Point> {
    site_list(doc, sel, s).into_iter().map(|q| q.at).collect()
}

/// The sites of a shape on the edited slide that are offered for snapping.
pub fn offered_sites(doc: &Presentation, sel: &Selection, s: &Shape) -> Vec<Point> {
    let x = xfrm_of(doc, sel, s);
    placed_sites(s, x.affine(), x.w, x.h).into_iter().filter(|q| q.offered).map(|q| q.at).collect()
}

/// The nearest offered connection site within `radius` of `p` on any visible shape on the edited
/// slide (`shapes`) except `exclude`: what a connector end there names (the shape, or for a
/// group's site its member, and that one's own site index) and where it is.
pub fn nearest_site(
    doc: &Presentation,
    sel: &Selection,
    shapes: &[Shape],
    p: Point,
    radius: f64,
    exclude: Option<ShapeId>,
) -> Option<(ShapeId, u32, Point)> {
    let mut best: Option<(f64, Site)> = None;
    for s in shapes.iter().filter(|s| Some(s.id) != exclude && !s.hidden) {
        let x = xfrm_of(doc, sel, s);
        for q in placed_sites(s, x.affine(), x.w, x.h).into_iter().filter(|q| q.offered) {
            let d = (q.at - p).hypot();
            if d <= radius && best.is_none_or(|b| d < b.0) {
                best = Some((d, q));
            }
        }
    }
    best.map(|(_, q)| (q.target.0, q.target.1, q.at))
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
    // A connector glued to a group member follows the member wherever the group puts it.
    let site = |id: ShapeId, idx: u32| site_at(&view, sel, &snapshot, id, idx);
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
