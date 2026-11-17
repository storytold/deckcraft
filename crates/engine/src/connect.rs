//! Connectors glued to shapes: connection sites, snapping line ends to them, and rerouting attached
//! connectors whenever the shapes they connect move, resize or rotate.

use deckcraft_geom::{Affine, Point, Rect, Xfrm};
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

/// Connection sites of a top-level shape `s` in slide coordinates (index = site index of
/// `shape.sites` / `shape.connect`). For a shape inside a group use [`sites_of`] or [`sites_in`].
pub fn sites(doc: &Presentation, sel: &Selection, s: &Shape) -> Vec<Point> {
    sites_in(doc, sel, s, Affine::IDENTITY)
}

/// Connection sites of `s` in slide coordinates, `to_abs` mapping the space its box lives in (the
/// slide, or the child space of the groups around it) onto the slide.
pub fn sites_in(doc: &Presentation, sel: &Selection, s: &Shape, to_abs: Affine) -> Vec<Point> {
    site_list_in(doc, sel, s, to_abs).into_iter().map(|q| q.at).collect()
}

/// [`sites_in`] with what a connector glued to each names and whether it is offered.
///
/// A preset's sites come from its geometry, a custom geometry's from its own list (`a:cxnLst`,
/// `Shape::sites`), and a shape without any offers its box midpoints. A group has none of its
/// own: it offers its members' own sites (a grouped circuit symbol's pins; groups within
/// included, a member without sites of its geometry offers none), each naming the member and its
/// own index, so a connector glued to one keeps it however the group's members change, and
/// after ungrouping. A hidden member's sites are listed but not offered.
pub fn site_list_in(doc: &Presentation, sel: &Selection, s: &Shape, to_abs: Affine) -> Vec<Site> {
    if !has_sites(s) {
        return vec![];
    }
    let x = xfrm_of(doc, sel, s);
    if let ShapeKind::Group { children, child } = &s.kind {
        let mut out = vec![];
        member_sites(children, to_abs * deckcraft_geom::group_child_affine(&x, child.rect()), true, &mut out, 0);
        return out;
    }
    let (local, _) = local_sites(s, x.w, x.h);
    let a = to_abs * x.affine();
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
        if own {
            let m = a * cx.affine();
            out.extend(local.into_iter().enumerate().map(|(i, p)| Site { at: m * p, target: (c.id, i as u32), offered: visible }));
        }
    }
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

/// [`sites_of`] with what a connector glued to each names and whether it is offered.
pub fn site_list_of(doc: &Presentation, sel: &Selection, shapes: &[Shape], id: ShapeId) -> Option<Vec<Site>> {
    locate(doc, sel, shapes, id).map(|(s, a)| site_list_in(doc, sel, s, a))
}

/// Where the site a connector end names (`id`, the shape's own site `idx`) is now, anywhere in
/// `shapes`: a connector glued to a shape that is then grouped keeps following it. An index past
/// the shape's sites means its first. `None` when the shape is gone, or names a group (which has
/// no sites of its own) or a line.
fn site_at(doc: &Presentation, sel: &Selection, shapes: &[Shape], id: ShapeId, idx: u32) -> Option<Point> {
    let (s, to_abs) = locate(doc, sel, shapes, id)?;
    if !has_sites(s) || s.is_group() {
        return None;
    }
    let x = xfrm_of(doc, sel, s);
    let (local, _) = local_sites(s, x.w, x.h);
    let a = to_abs * x.affine();
    local.get(idx as usize).or(local.first()).map(|p| a * *p)
}

/// The sites of a top-level shape that are offered for snapping.
pub fn offered_sites(doc: &Presentation, sel: &Selection, s: &Shape) -> Vec<Point> {
    site_list_in(doc, sel, s, Affine::IDENTITY).into_iter().filter(|q| q.offered).map(|q| q.at).collect()
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
        for q in site_list_in(doc, sel, s, Affine::IDENTITY).into_iter().filter(|q| q.offered) {
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
