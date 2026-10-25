//! Edit Points (Shape Format ▸ Edit Shape ▸ Edit Points): turn a shape into a freeform and move,
//! add or delete the vertices of its outline.
//!
//! While points are edited every custom path of the shape is kept in shape-local points (its
//! `w`/`h` equal the shape's size), so a vertex's path coordinates are its offset in the shape.
//! After each change the shape's box is fitted around the outline again, keeping it in place on
//! the slide (rotation and flips included).

use deckcraft_geom::{PathEl, Point, Xfrm};
use deckcraft_model::{CustomPath, Geom, Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, f64_param, has_selection, id_param, ok, str_param, xfrm_of};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "shape.editPoints",
            "Edit Points",
            ["Shape Format", "Insert Shapes", "Edit Shape"],
            None,
            "{on?: bool (default: toggle), id?} — enter or leave Edit Points for one selected shape; entering turns a preset into a freeform",
            has_selection,
            edit_points
        ),
        cmd!(query "shape.points", "Points", [], None, "{id?} → {id, paths: [[[x, y] slide pt]], types: [[smooth|straight|corner]], segments: [[straight|curved|null]]} vertices of a freeform and the segment ending at each", has_selection, points),
        cmd!("shape.movePoint", "Move Point", [], None, "{path?: 0, index, x, y: slide pt, id?}", has_selection, move_point),
        cmd!(
            "shape.addPoint",
            "Add Point",
            [],
            None,
            "{path?: 0, segment: index of the vertex ending it, t?: 0..1 (0.5), id?}",
            has_selection,
            add_point
        ),
        cmd!(
            "shape.pointType",
            "Point Type",
            [],
            None,
            "{path?: 0, index, type: smooth|straight|corner, id?} — smooth: handles in line and equal; straight: in line; corner: independent",
            has_selection,
            point_type
        ),
        cmd!(
            "shape.moveHandle",
            "Move Handle",
            [],
            None,
            "{path?: 0, index, side: in|out, x, y: slide pt, type?: smooth|straight|corner (default: read from the handles), id?}",
            has_selection,
            move_handle
        ),
        cmd!("shape.deletePoint", "Delete Point", [], None, "{path?: 0, index, id?}", has_selection, delete_point),
        cmd!(
            "shape.deleteSegment",
            "Delete Segment",
            [],
            None,
            "{path?: 0, segment: index of the vertex ending it, id?}: removes a segment of a closed outline, which opens there; it starts at the segment's end and ends at its start",
            has_selection,
            delete_segment
        ),
        cmd!(
            "shape.segmentType",
            "Segment Type",
            [],
            None,
            "{path?: 0, segment: index of the vertex ending it, type: straight|curved, id?} — straight drops the segment's handles; curved gives it handles at its thirds",
            has_selection,
            segment_type
        ),
        cmd!(
            "shape.openPath",
            "Open Path",
            [],
            None,
            "{path?: 0, index, id?} — break a closed outline at a point, which becomes its two ends",
            has_selection,
            open_path
        ),
        cmd!(
            "shape.closePath",
            "Close Path",
            [],
            None,
            "{path?: 0, index: any point of the open outline, id?} — join its ends with a straight segment (one point when they meet)",
            has_selection,
            close_path
        ),
    ]
}

/// One path command, in path units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    M(Point),
    L(Point),
    C(Point, Point, Point),
    Q(Point, Point),
    Z,
}

impl Seg {
    pub fn end(&self) -> Option<Point> {
        match self {
            Seg::M(p) | Seg::L(p) | Seg::C(_, _, p) | Seg::Q(_, p) => Some(*p),
            Seg::Z => None,
        }
    }
    fn points_mut(&mut self) -> Vec<&mut Point> {
        match self {
            Seg::M(p) | Seg::L(p) => vec![p],
            Seg::C(a, b, p) => vec![a, b, p],
            Seg::Q(a, p) => vec![a, p],
            Seg::Z => vec![],
        }
    }
}

/// Parses path data (`M x y L x y C … Q … Z`); malformed commands are skipped.
pub fn parse(d: &str) -> Vec<Seg> {
    let toks: Vec<&str> = d.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).collect();
    let num = |i: usize| toks.get(i).and_then(|t| t.parse::<f64>().ok()).filter(|v| v.is_finite());
    let pt = |i: usize| Some(Point::new(num(i)?, num(i + 1)?));
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() && out.len() < 100_000 {
        let (seg, n) = match toks.get(i).copied().unwrap_or("") {
            "M" => (pt(i + 1).map(Seg::M), 3),
            "L" => (pt(i + 1).map(Seg::L), 3),
            "C" => ((|| Some(Seg::C(pt(i + 1)?, pt(i + 3)?, pt(i + 5)?)))(), 7),
            "Q" => ((|| Some(Seg::Q(pt(i + 1)?, pt(i + 3)?)))(), 5),
            "Z" => (Some(Seg::Z), 1),
            _ => (None, 1),
        };
        // Make a closing edge explicit, so it is a segment that can take points.
        if matches!(seg, Some(Seg::Z)) {
            let start = out.iter().rev().find_map(|x| if let Seg::M(p) = x { Some(*p) } else { None });
            let end = out.last().and_then(Seg::end);
            if let (Some(a), Some(b)) = (start, end)
                && (a - b).hypot() > 1e-6
            {
                out.push(Seg::L(a));
            }
        }
        if let Some(s) = seg {
            // A path starts with a move.
            let s = match (out.is_empty(), s) {
                (true, Seg::L(p)) => Seg::M(p),
                (true, Seg::C(..) | Seg::Q(..) | Seg::Z) => {
                    i += n;
                    continue;
                }
                (_, s) => s,
            };
            out.push(s);
        }
        i += n;
    }
    out
}

/// Path data for `segs`.
pub fn write(segs: &[Seg]) -> String {
    let f = |v: f64| {
        let s = format!("{:.3}", v);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s == "-0" { "0".to_string() } else { s.to_string() }
    };
    let p = |q: &Point| format!("{} {}", f(q.x), f(q.y));
    segs.iter()
        .map(|s| match s {
            Seg::M(a) => format!("M {}", p(a)),
            Seg::L(a) => format!("L {}", p(a)),
            Seg::C(a, b, c) => format!("C {} {} {}", p(a), p(b), p(c)),
            Seg::Q(a, b) => format!("Q {} {}", p(a), p(b)),
            Seg::Z => "Z".into(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether segment `i` ends back on its sub-path's start right before a `Z` (the closing edge of
/// a closed outline). Its end is the same vertex as the start, not a vertex of its own.
fn closes_onto_start(segs: &[Seg], i: usize) -> bool {
    if !matches!(segs.get(i + 1), Some(Seg::Z)) {
        return false;
    }
    let start = segs.get(..=i).and_then(|s| s.iter().rev().find_map(|x| if let Seg::M(p) = x { Some(*p) } else { None }));
    match (start, segs.get(i).and_then(Seg::end)) {
        (Some(a), Some(b)) => !matches!(segs.get(i), Some(Seg::M(_))) && (a - b).hypot() < 1e-6,
        _ => false,
    }
}

/// The segments whose end is a vertex, in order.
pub fn vertices(segs: &[Seg]) -> Vec<usize> {
    (0..segs.len()).filter(|i| segs.get(*i).and_then(Seg::end).is_some() && !closes_onto_start(segs, *i)).collect()
}

fn shift(segs: &mut [Seg], i: usize, which: &[usize], d: deckcraft_geom::Vec2) {
    if let Some(s) = segs.get_mut(i) {
        let mut pts = s.points_mut();
        for w in which {
            if let Some(p) = pts.get_mut(*w) {
                **p += d;
            }
        }
    }
}

/// The indices of the points of segment `i` that sit on its end vertex (the end and, for a
/// cubic, its incoming control point).
fn end_points(seg: Option<&Seg>) -> &'static [usize] {
    match seg {
        Some(Seg::C(..)) => &[1, 2],
        Some(Seg::Q(..)) => &[1],
        Some(Seg::M(_) | Seg::L(_)) => &[0],
        _ => &[],
    }
}

/// Moves the vertex ending segment `si` by `d`, with the control points attached to it.
fn move_vertex(segs: &mut [Seg], si: usize, d: deckcraft_geom::Vec2) {
    // A closed sub-path's start is also its closing edge's end.
    let close = if matches!(segs.get(si), Some(Seg::M(_))) {
        (si + 1..segs.len()).take_while(|j| !matches!(segs.get(*j), Some(Seg::M(_)))).find(|j| closes_onto_start(segs, *j))
    } else {
        None
    };
    let own = end_points(segs.get(si));
    shift(segs, si, own, d);
    // The outgoing curve's first control point follows the vertex.
    if matches!(segs.get(si + 1), Some(Seg::C(..))) {
        shift(segs, si + 1, &[0], d);
    }
    if let Some(j) = close {
        let w = end_points(segs.get(j));
        shift(segs, j, w, d);
    }
}

/// The point where segment `si` starts.
fn seg_start(segs: &[Seg], si: usize) -> Option<Point> {
    let prev = segs.get(..si)?;
    match prev.last()? {
        Seg::Z => prev.iter().rev().find_map(|x| if let Seg::M(p) = x { Some(*p) } else { None }),
        s => s.end(),
    }
}

/// Splits segment `si` at `t`, adding a vertex there. Returns false for a move or close.
fn split(segs: &mut Vec<Seg>, si: usize, t: f64) -> bool {
    let Some(p0) = seg_start(segs, si) else { return false };
    let lerp = |a: Point, b: Point| a.lerp(b, t);
    let (a, b) = match segs.get(si).copied() {
        Some(Seg::L(p1)) => (Seg::L(lerp(p0, p1)), Seg::L(p1)),
        Some(Seg::Q(c, p1)) => {
            let (m0, m1) = (lerp(p0, c), lerp(c, p1));
            let m = m0.lerp(m1, t);
            (Seg::Q(m0, m), Seg::Q(m1, p1))
        }
        Some(Seg::C(c1, c2, p1)) => {
            let (a1, a2, a3) = (lerp(p0, c1), lerp(c1, c2), lerp(c2, p1));
            let (b1, b2) = (a1.lerp(a2, t), a2.lerp(a3, t));
            let m = b1.lerp(b2, t);
            (Seg::C(a1, b1, m), Seg::C(b2, a3, p1))
        }
        _ => return false,
    };
    if let Some(s) = segs.get_mut(si) {
        *s = a;
    }
    segs.insert(si + 1, b);
    true
}

/// Removes the vertex ending segment `si`. A sub-path keeps at least two vertices.
fn remove_vertex(segs: &mut Vec<Seg>, si: usize) -> bool {
    // The vertices of `si`'s sub-path.
    let from = segs.get(..=si).and_then(|s| s.iter().rposition(|x| matches!(x, Seg::M(_)))).unwrap_or(0);
    let to = (si + 1..segs.len()).find(|j| matches!(segs.get(*j), Some(Seg::M(_)))).unwrap_or(segs.len());
    let count = vertices(segs).iter().filter(|v| (from..to).contains(*v)).count();
    if count <= 2 {
        return false;
    }
    if matches!(segs.get(si), Some(Seg::M(_))) {
        // The next vertex becomes the start; a closed outline's closing edge now ends there.
        let Some(next) = segs.get(si + 1).and_then(Seg::end) else { return false };
        segs.remove(si + 1);
        if let Some(s) = segs.get_mut(si) {
            *s = Seg::M(next);
        }
        let close = (si + 1..segs.len()).take_while(|j| !matches!(segs.get(*j), Some(Seg::M(_)))).find(|j| matches!(segs.get(j + 1), Some(Seg::Z)));
        if let Some(j) = close
            && let Some(Seg::L(p) | Seg::C(_, _, p) | Seg::Q(_, p)) = segs.get_mut(j)
        {
            *p = next;
        }
        return true;
    }
    // Merge with the next segment: it now starts where the removed vertex's segment started.
    match (segs.get(si).copied(), segs.get(si + 1).copied()) {
        (Some(Seg::C(c1, _, _)), Some(Seg::C(_, c2, p))) => {
            segs.remove(si + 1);
            if let Some(s) = segs.get_mut(si) {
                *s = Seg::C(c1, c2, p);
            }
        }
        _ => {
            segs.remove(si);
        }
    }
    true
}

/// The sub-path holding segment `si`: from its move up to the next move (or the end).
fn subpath(segs: &[Seg], si: usize) -> (usize, usize) {
    let from = segs.get(..=si).and_then(|s| s.iter().rposition(|x| matches!(x, Seg::M(_)))).unwrap_or(0);
    let to = (si + 1..segs.len()).find(|j| matches!(segs.get(*j), Some(Seg::M(_)))).unwrap_or(segs.len());
    (from, to)
}

/// The segment that ends at the vertex ending segment `si`: the closing edge for a closed
/// sub-path's start, none for an open one's.
fn ending_segment(segs: &[Seg], si: usize) -> Option<usize> {
    if matches!(segs.get(si), Some(Seg::M(_))) {
        (si + 1..segs.len()).take_while(|j| !matches!(segs.get(*j), Some(Seg::M(_)))).find(|j| closes_onto_start(segs, *j))
    } else {
        Some(si)
    }
}

/// Whether segment `j` is a straight line or a curve.
fn segment_kind(segs: &[Seg], j: usize) -> Option<&'static str> {
    match segs.get(j)? {
        Seg::L(_) => Some("straight"),
        Seg::C(..) | Seg::Q(..) => Some("curved"),
        Seg::M(_) | Seg::Z => None,
    }
}

/// Makes segment `j` a straight line (dropping its handles) or a curve with handles at its
/// thirds, so it keeps its shape until a handle is dragged.
fn set_segment(segs: &mut [Seg], j: usize, curved: bool) {
    if curved {
        to_cubic(segs, j);
    } else if let Some(s) = segs.get_mut(j)
        && let Seg::C(_, _, p) | Seg::Q(_, p) = *s
    {
        *s = Seg::L(p);
    }
}

/// Opens the closed sub-path through the vertex ending segment `si` there: it now starts and ends
/// on that vertex, as two points. Returns false for an open sub-path.
fn open_at(segs: &mut Vec<Seg>, si: usize) -> bool {
    let (from, to) = subpath(segs, si);
    let Some(z) = to.checked_sub(1).filter(|z| *z > si && matches!(segs.get(*z), Some(Seg::Z))) else { return false };
    let Some(v) = segs.get(si).and_then(Seg::end) else { return false };
    let (Some(after), Some(before)) = (segs.get(si + 1..z), segs.get(from + 1..=si)) else { return false };
    let mut out = vec![Seg::M(v)];
    out.extend_from_slice(after);
    out.extend_from_slice(before);
    segs.splice(from..to, out);
    true
}

/// Deletes the segment ending at the vertex ending segment `si` from its closed sub-path, which
/// opens there: it now starts at the segment's end and ends at its start. Returns false for an
/// open sub-path.
fn delete_segment_at(segs: &mut Vec<Seg>, si: usize) -> bool {
    let (from, _) = subpath(segs, si);
    if !open_at(segs, si) {
        return false;
    }
    // The deleted segment now ends the sub-path.
    let (_, to) = subpath(segs, from);
    if to > from + 1 {
        segs.remove(to - 1);
    }
    true
}

/// Closes the open sub-path through the vertex ending segment `si`: a straight segment joins its
/// ends, or they become one point when they meet. Returns false for a closed sub-path.
fn close_at(segs: &mut Vec<Seg>, si: usize) -> bool {
    let (from, to) = subpath(segs, si);
    let last = to.saturating_sub(1);
    if last <= from || matches!(segs.get(last), Some(Seg::Z)) {
        return false;
    }
    let (Some(Seg::M(start)), Some(end)) = (segs.get(from).copied(), segs.get(last).and_then(Seg::end)) else { return false };
    let mut add = Vec::new();
    if (start - end).hypot() >= 1e-6 {
        add.push(Seg::L(start));
    }
    add.push(Seg::Z);
    segs.splice(to..to, add);
    true
}

/// Custom paths of a preset shape, in shape-local points.
fn to_custom(sh: &Shape, w: f64, h: f64) -> Vec<CustomPath> {
    let geo = deckcraft_render::shape_geometry(sh, w, h);
    geo.paths
        .iter()
        .map(|sp| {
            let segs: Vec<Seg> = sp
                .path
                .elements()
                .iter()
                .map(|e| match *e {
                    PathEl::MoveTo(p) => Seg::M(p),
                    PathEl::LineTo(p) => Seg::L(p),
                    PathEl::QuadTo(a, p) => Seg::Q(a, p),
                    PathEl::CurveTo(a, b, p) => Seg::C(a, b, p),
                    PathEl::ClosePath => Seg::Z,
                })
                .collect();
            CustomPath { w, h, d: write(&segs), fill: sp.fill, stroke: sp.stroke }
        })
        .collect()
}

/// Rescales every path of a custom geometry to shape-local points.
fn normalize(paths: &mut [CustomPath], w: f64, h: f64) {
    for p in paths {
        let (sx, sy) = (if p.w > 0.0 { w / p.w } else { 1.0 }, if p.h > 0.0 { h / p.h } else { 1.0 });
        if (sx - 1.0).abs() > 1e-12 || (sy - 1.0).abs() > 1e-12 {
            let mut segs = parse(&p.d);
            for s in &mut segs {
                for q in s.points_mut() {
                    *q = Point::new(q.x * sx, q.y * sy);
                }
            }
            p.d = write(&segs);
        }
        p.w = w;
        p.h = h;
    }
}

/// Fits the box around the outline, keeping the drawing where it is on the slide.
fn refit(sh: &mut Shape) {
    let (Some(x), Geom::Custom { paths }) = (sh.xfrm, &mut sh.geom) else { return };
    let mut all: Vec<Vec<Seg>> = paths.iter().map(|p| parse(&p.d)).collect();
    let mut bb: Option<deckcraft_geom::Rect> = None;
    for s in all.iter_mut().flatten() {
        for q in s.points_mut() {
            bb = Some(bb.map_or(deckcraft_geom::Rect::from_points(*q, *q), |b| b.union_pt(*q)));
        }
    }
    let Some(bb) = bb else { return };
    let (w, h) = (bb.width().max(1.0), bb.height().max(1.0));
    if bb.x0.abs() < 1e-6 && bb.y0.abs() < 1e-6 && (w - x.w).abs() < 1e-6 && (h - x.h).abs() < 1e-6 {
        return;
    }
    let c = x.affine() * Point::new(bb.x0 + w / 2.0, bb.y0 + h / 2.0);
    let off = deckcraft_geom::Vec2::new(bb.x0, bb.y0);
    for (p, segs) in paths.iter_mut().zip(&mut all) {
        for s in segs.iter_mut() {
            for q in s.points_mut() {
                *q -= off;
            }
        }
        p.d = write(segs);
        p.w = w;
        p.h = h;
    }
    sh.xfrm = Some(Xfrm { x: c.x - w / 2.0, y: c.y - h / 2.0, w, h, ..x });
}

/// The one shape whose points are edited: `id`, or the single selected shape.
fn target(s: &Session, p: &Value, cmd: &str) -> Result<ShapeId> {
    let st = s.doc()?;
    let id = match id_param(p, "id", cmd)? {
        Some(i) => i,
        None => match st.selection.shapes.as_slice() {
            [one] => *one,
            _ => return Err(bad(cmd, "select one shape")),
        },
    };
    let sh = st.shape(id).ok_or_else(|| bad(cmd, "no such shape"))?;
    if !st.shapes().iter().any(|t| t.id == id) {
        return Err(bad(cmd, "a shape in a group can't have its points edited; ungroup it first"));
    }
    if !matches!(sh.kind, ShapeKind::Shape) || sh.is_line() || sh.xfrm.is_none() {
        return Err(bad(cmd, "only a shape with an outline has points to edit"));
    }
    if sh.locked {
        return Err(bad(cmd, "the shape is locked"));
    }
    Ok(id)
}

/// Makes `id` a freeform in shape-local points (the state Edit Points works in).
pub(crate) fn prepare(s: &mut Session, id: ShapeId) -> Result<()> {
    let st = s.doc()?;
    let Some(sh) = st.shape(id) else { return Ok(()) };
    let Some(x) = sh.xfrm else { return Ok(()) };
    let ready = matches!(&sh.geom, Geom::Custom { paths } if paths.iter().all(|p| p.w == x.w && p.h == x.h));
    if ready {
        return Ok(());
    }
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("shape.editPoints", "no slide"))?;
        if let Some(sh) = deckcraft_model::find_shape_mut(list, id) {
            match &mut sh.geom {
                Geom::Custom { paths } => normalize(paths, x.w, x.h),
                Geom::Preset { .. } => {
                    let paths = to_custom(sh, x.w, x.h);
                    sh.geom = Geom::Custom { paths };
                }
            }
        }
        Ok(())
    })
}

fn edit_points(s: &mut Session, p: &Value) -> Result<Value> {
    let on = p.get("on").and_then(Value::as_bool).unwrap_or(s.tool.edit_points.is_none());
    s.tool.edit_vertex = None;
    s.tool.edit_segment = None;
    if !on {
        s.tool.edit_points = None;
        return Ok(json!({"editing": Value::Null}));
    }
    let id = target(s, p, "shape.editPoints")?;
    prepare(s, id)?;
    s.tool.edit_points = Some(id);
    Ok(json!({"editing": id.0}))
}

/// Vertices of every path of `sh` in slide points.
pub fn slide_vertices(doc: &deckcraft_model::Presentation, sel: &crate::Selection, sh: &Shape) -> Vec<Vec<Point>> {
    let Geom::Custom { paths } = &sh.geom else { return vec![] };
    let x = xfrm_of(doc, sel, sh);
    let a = x.affine();
    paths
        .iter()
        .map(|p| {
            let (sx, sy) = (if p.w > 0.0 { x.w / p.w } else { 1.0 }, if p.h > 0.0 { x.h / p.h } else { 1.0 });
            let segs = parse(&p.d);
            vertices(&segs).iter().filter_map(|i| segs.get(*i).and_then(Seg::end)).map(|q| a * Point::new(q.x * sx, q.y * sy)).collect()
        })
        .collect()
}

fn points(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let id = match id_param(p, "id", "shape.points")? {
        Some(i) => i,
        None => *st.selection.shapes.first().ok_or_else(|| bad("shape.points", "select a shape"))?,
    };
    let sh = st.shape(id).ok_or_else(|| bad("shape.points", "no such shape"))?;
    let paths: Vec<Vec<[f64; 2]>> =
        slide_vertices(&st.doc, &st.selection, sh).into_iter().map(|v| v.into_iter().map(|q| [q.x, q.y]).collect()).collect();
    let (mut types, mut kinds): (Vec<Vec<&str>>, Vec<Vec<Option<&str>>>) = (vec![], vec![]);
    if let Geom::Custom { paths } = &sh.geom {
        for cp in paths {
            let segs = parse(&cp.d);
            let vs = vertices(&segs);
            types.push(vs.iter().map(|si| point_kind(&segs, *si)).collect());
            kinds.push(vs.iter().map(|si| ending_segment(&segs, *si).and_then(|j| segment_kind(&segs, j))).collect());
        }
    }
    Ok(json!({"id": id.0, "paths": paths, "types": types, "segments": kinds}))
}

/// Runs `f` on the segments of path `path` of `id` (in shape-local points), then refits the box.
/// `f` gets the segment indices of the vertices and returns whether it changed anything.
pub(crate) fn edit_path(
    s: &mut Session,
    id: ShapeId,
    path: usize,
    cmd: &'static str,
    f: impl Fn(&mut Vec<Seg>, &[usize], &Xfrm) -> Result<bool>,
) -> Result<Value> {
    prepare(s, id)?;
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad(cmd, "no slide"))?;
        let sh = deckcraft_model::find_shape_mut(list, id).ok_or_else(|| bad(cmd, "no such shape"))?;
        let x = sh.xfrm.ok_or_else(|| bad(cmd, "the shape has no position"))?;
        let Geom::Custom { paths } = &mut sh.geom else { return Err(bad(cmd, "not a freeform")) };
        let cp = paths.get_mut(path).ok_or_else(|| bad(cmd, "no such path"))?;
        let mut segs = parse(&cp.d);
        let vs = vertices(&segs);
        if f(&mut segs, &vs, &x)? {
            cp.d = write(&segs);
            refit(sh);
        }
        Ok(())
    })?;
    ok()
}

fn index_param(p: &Value, key: &str, cmd: &str) -> Result<usize> {
    p.get(key).and_then(Value::as_u64).map(|v| v as usize).ok_or_else(|| bad(cmd, format!("give `{key}`")))
}

fn move_point(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, p, "shape.movePoint")?;
    let path = p.get("path").and_then(Value::as_u64).unwrap_or(0) as usize;
    let index = index_param(p, "index", "shape.movePoint")?;
    let to = match (f64_param(p, "x"), f64_param(p, "y")) {
        (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Point::new(x.clamp(-1e5, 1e5), y.clamp(-1e5, 1e5)),
        _ => return Err(bad("shape.movePoint", "give `x` and `y` in slide points")),
    };
    edit_path(s, id, path, "shape.movePoint", move |segs, vs, x| {
        let si = *vs.get(index).ok_or_else(|| bad("shape.movePoint", "no such point"))?;
        let local = x.affine().inverse() * to;
        let Some(cur) = segs.get(si).and_then(Seg::end) else { return Ok(false) };
        move_vertex(segs, si, local - cur);
        Ok(true)
    })
}

fn add_point(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, p, "shape.addPoint")?;
    let path = p.get("path").and_then(Value::as_u64).unwrap_or(0) as usize;
    let seg = index_param(p, "segment", "shape.addPoint")?;
    let t = f64_param(p, "t").filter(|t| t.is_finite()).unwrap_or(0.5).clamp(0.01, 0.99);
    let r = edit_path(s, id, path, "shape.addPoint", move |segs, vs, _| {
        let si = *vs.get(seg).ok_or_else(|| bad("shape.addPoint", "no such segment"))?;
        // The closing edge of a closed outline ends at the start: split it, not the start.
        let si = ending_segment(segs, si).ok_or_else(|| bad("shape.addPoint", "the first point ends no segment"))?;
        if split(segs, si, t) { Ok(true) } else { Err(bad("shape.addPoint", "no segment ends at that point")) }
    })?;
    forget_kinds(s, path);
    Ok(r)
}

fn delete_point(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, p, "shape.deletePoint")?;
    let path = p.get("path").and_then(Value::as_u64).unwrap_or(0) as usize;
    let index = index_param(p, "index", "shape.deletePoint")?;
    let r = edit_path(s, id, path, "shape.deletePoint", move |segs, vs, _| {
        let si = *vs.get(index).ok_or_else(|| bad("shape.deletePoint", "no such point"))?;
        if remove_vertex(segs, si) { Ok(true) } else { Err(bad("shape.deletePoint", "an outline keeps at least two points")) }
    })?;
    forget_kinds(s, path);
    Ok(r)
}

/// The vertex of `sh` near slide point `p`: (path, index).
pub fn vertex_at(doc: &deckcraft_model::Presentation, sel: &crate::Selection, sh: &Shape, p: Point, tol: f64) -> Option<(usize, usize)> {
    for (pi, vs) in slide_vertices(doc, sel, sh).iter().enumerate() {
        for (i, q) in vs.iter().enumerate() {
            if (*q - p).hypot() <= tol {
                return Some((pi, i));
            }
        }
    }
    None
}

/// The segment of `sh`'s outline near slide point `p`: (path, index of the vertex ending it, t).
pub fn segment_at(doc: &deckcraft_model::Presentation, sel: &crate::Selection, sh: &Shape, p: Point, tol: f64) -> Option<(usize, usize, f64)> {
    let Geom::Custom { paths } = &sh.geom else { return None };
    let x = xfrm_of(doc, sel, sh);
    let local = x.affine().inverse() * p;
    let mut best: Option<(f64, usize, usize, f64)> = None;
    for (pi, cp) in paths.iter().enumerate() {
        let (sx, sy) = (if cp.w > 0.0 { x.w / cp.w } else { 1.0 }, if cp.h > 0.0 { x.h / cp.h } else { 1.0 });
        let segs = parse(&cp.d);
        let vs = vertices(&segs);
        for si in 0..segs.len() {
            let Some(p0) = seg_start(&segs, si) else { continue };
            let at = |t: f64| -> Option<Point> {
                let q = match segs.get(si)? {
                    Seg::L(p1) => p0.lerp(*p1, t),
                    Seg::Q(c, p1) => p0.lerp(*c, t).lerp(c.lerp(*p1, t), t),
                    Seg::C(c1, c2, p1) => {
                        let (a, b, c) = (p0.lerp(*c1, t), c1.lerp(*c2, t), c2.lerp(*p1, t));
                        a.lerp(b, t).lerp(b.lerp(c, t), t)
                    }
                    _ => return None,
                };
                Some(Point::new(q.x * sx, q.y * sy))
            };
            // The vertex that ends this segment (a closing edge ends on the start).
            let end_vertex = if closes_onto_start(&segs, si) {
                let start = segs.get(..si).and_then(|s| s.iter().rposition(|x| matches!(x, Seg::M(_))));
                start.and_then(|m| vs.iter().position(|v| *v == m))
            } else {
                vs.iter().position(|v| *v == si)
            };
            let Some(ev) = end_vertex else { continue };
            let mut near: Option<(f64, f64)> = None;
            for k in 1..32 {
                let t = k as f64 / 32.0;
                if let Some(q) = at(t) {
                    let d = (q - local).hypot();
                    if d <= tol && near.is_none_or(|n| d < n.0) {
                        near = Some((d, t));
                    }
                }
            }
            let Some((mut d, mut t)) = near else { continue };
            // Narrow `t` down to the nearest place on the segment, where Add Point puts the point.
            let mut step = 1.0 / 32.0;
            for _ in 0..4 {
                step /= 8.0;
                let c = t;
                for k in -8..=8 {
                    let u = (c + f64::from(k) * step).clamp(0.0, 1.0);
                    if let Some(q) = at(u) {
                        let e = (q - local).hypot();
                        if e < d {
                            (d, t) = (e, u);
                        }
                    }
                }
            }
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, pi, ev, t));
            }
        }
    }
    best.map(|(_, pi, ev, t)| (pi, ev, t))
}

/// Moves vertex `index` of path `path` of `id` to slide point `to` (pointer drags).
pub(crate) fn move_point_raw(s: &mut Session, id: ShapeId, path: usize, index: usize, to: Point) -> Result<Value> {
    move_point(s, &json!({"id": id.0, "path": path, "index": index, "x": to.x, "y": to.y}))
}

/// Where the control handles of the vertex ending segment `si` live: (segment, point index) for
/// the incoming and the outgoing one, when those segments are cubics.
fn handle_slots(segs: &[Seg], si: usize) -> (Option<(usize, usize)>, Option<(usize, usize)>) {
    let incoming = if matches!(segs.get(si), Some(Seg::M(_))) {
        (si + 1..segs.len()).take_while(|j| !matches!(segs.get(*j), Some(Seg::M(_)))).find(|j| closes_onto_start(segs, *j))
    } else {
        Some(si)
    };
    let inc = incoming.filter(|j| matches!(segs.get(*j), Some(Seg::C(..)))).map(|j| (j, 1));
    let out = Some(si + 1).filter(|j| matches!(segs.get(*j), Some(Seg::C(..)))).map(|j| (j, 0));
    (inc, out)
}

fn get_pt(segs: &[Seg], slot: (usize, usize)) -> Option<Point> {
    match segs.get(slot.0)? {
        Seg::C(a, b, c) => [*a, *b, *c].get(slot.1).copied(),
        _ => None,
    }
}

fn set_pt(segs: &mut [Seg], slot: (usize, usize), p: Point) {
    if let Some(s) = segs.get_mut(slot.0)
        && let Some(q) = s.points_mut().into_iter().nth(slot.1)
    {
        *q = p;
    }
}

/// Turns segment `j` into a cubic with the same shape.
fn to_cubic(segs: &mut [Seg], j: usize) {
    let Some(p0) = seg_start(segs, j) else { return };
    let c = match segs.get(j).copied() {
        Some(Seg::L(p1)) => Seg::C(p0.lerp(p1, 1.0 / 3.0), p0.lerp(p1, 2.0 / 3.0), p1),
        Some(Seg::Q(c, p1)) => Seg::C(p0.lerp(c, 2.0 / 3.0), p1.lerp(c, 2.0 / 3.0), p1),
        _ => return,
    };
    if let Some(s) = segs.get_mut(j) {
        *s = c;
    }
}

/// Sets the point type of the vertex ending segment `si`. Its segments become curves so it has
/// handles; smooth and straight points get their handles in line through the vertex.
fn set_type(segs: &mut [Seg], si: usize, kind: &str) {
    let incoming = if matches!(segs.get(si), Some(Seg::M(_))) {
        (si + 1..segs.len()).take_while(|j| !matches!(segs.get(*j), Some(Seg::M(_)))).find(|j| closes_onto_start(segs, *j))
    } else {
        Some(si)
    };
    if let Some(j) = incoming {
        to_cubic(segs, j);
    }
    to_cubic(segs, si + 1);
    if kind == "corner" {
        return;
    }
    let (Some(v), (Some(hi), Some(ho))) = (segs.get(si).and_then(Seg::end), handle_slots(segs, si)) else { return };
    let (Some(a), Some(b)) = (get_pt(segs, hi), get_pt(segs, ho)) else { return };
    let (li, lo) = ((a - v).hypot(), (b - v).hypot());
    let mut dir = b - a;
    if dir.hypot() < 1e-9 {
        // Both handles on the vertex: point them along the neighbours.
        let prev = seg_start(segs, hi.0).unwrap_or(v);
        let next = segs.get(ho.0).and_then(Seg::end).unwrap_or(v);
        dir = next - prev;
    }
    let len = dir.hypot();
    if len < 1e-9 {
        return;
    }
    let u = dir / len;
    let (li, lo) = if kind == "smooth" {
        let m = if li + lo > 1e-9 { (li + lo) / 2.0 } else { len / 6.0 };
        (m, m)
    } else {
        (li, lo)
    };
    set_pt(segs, hi, v - u * li);
    set_pt(segs, ho, v + u * lo);
}

/// The type a vertex's handles show: smooth (in line, equal), straight (in line) or corner.
pub fn point_kind(segs: &[Seg], si: usize) -> &'static str {
    let (Some(v), (Some(hi), Some(ho))) = (segs.get(si).and_then(Seg::end), handle_slots(segs, si)) else { return "corner" };
    let (Some(a), Some(b)) = (get_pt(segs, hi), get_pt(segs, ho)) else { return "corner" };
    let (da, db) = (a - v, b - v);
    let (la, lb) = (da.hypot(), db.hypot());
    if la < 1e-6 || lb < 1e-6 {
        return "corner";
    }
    let cos = da.dot(db) / (la * lb);
    if cos > -0.9998 {
        "corner"
    } else if (la - lb).abs() <= 0.01 * la.max(lb) {
        "smooth"
    } else {
        "straight"
    }
}

/// Moves handle `side` of the vertex ending segment `si` to `p`; the other handle follows as
/// the point's type says.
fn set_handle(segs: &mut [Seg], si: usize, out: bool, p: Point, kind: Option<&str>) -> bool {
    let kind = kind.unwrap_or_else(|| point_kind(segs, si));
    let (hi, ho) = handle_slots(segs, si);
    let (mine, other) = if out { (ho, hi) } else { (hi, ho) };
    let Some(mine) = mine else { return false };
    let Some(v) = segs.get(si).and_then(Seg::end) else { return false };
    set_pt(segs, mine, p);
    if let Some(o) = other
        && kind != "corner"
        && let Some(q) = get_pt(segs, o)
    {
        let d = p - v;
        let l = d.hypot();
        if l > 1e-9 {
            let lo = if kind == "smooth" { l } else { (q - v).hypot() };
            set_pt(segs, o, v - d / l * lo);
        }
    }
    true
}

/// Handles of vertex `index` of path `path` of `sh` in slide points: (incoming, outgoing, vertex).
pub fn slide_handles(
    doc: &deckcraft_model::Presentation,
    sel: &crate::Selection,
    sh: &Shape,
    path: usize,
    index: usize,
) -> Option<(Option<Point>, Option<Point>, Point)> {
    let Geom::Custom { paths } = &sh.geom else { return None };
    let cp = paths.get(path)?;
    let x = xfrm_of(doc, sel, sh);
    let (sx, sy) = (if cp.w > 0.0 { x.w / cp.w } else { 1.0 }, if cp.h > 0.0 { x.h / cp.h } else { 1.0 });
    let a = x.affine();
    let to = |q: Point| a * Point::new(q.x * sx, q.y * sy);
    let segs = parse(&cp.d);
    let si = *vertices(&segs).get(index)?;
    let v = segs.get(si).and_then(Seg::end)?;
    let (hi, ho) = handle_slots(&segs, si);
    Some((hi.and_then(|s| get_pt(&segs, s)).map(to), ho.and_then(|s| get_pt(&segs, s)).map(to), to(v)))
}

fn point_type(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, p, "shape.pointType")?;
    let path = p.get("path").and_then(Value::as_u64).unwrap_or(0) as usize;
    let index = index_param(p, "index", "shape.pointType")?;
    let kind = match str_param(p, "type") {
        Some(k @ ("smooth" | "straight" | "corner")) => k.to_string(),
        _ => return Err(bad("shape.pointType", "`type` is smooth, straight or corner")),
    };
    // A straight point's handles can be as long as each other; remember the choice for drags.
    s.tool.vertex_kind = Some((id, path, index, kind.clone()));
    edit_path(s, id, path, "shape.pointType", move |segs, vs, _| {
        let si = *vs.get(index).ok_or_else(|| bad("shape.pointType", "no such point"))?;
        set_type(segs, si, &kind);
        Ok(true)
    })
}

fn move_handle(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, p, "shape.moveHandle")?;
    let path = p.get("path").and_then(Value::as_u64).unwrap_or(0) as usize;
    let index = index_param(p, "index", "shape.moveHandle")?;
    let out = match str_param(p, "side") {
        Some("out") => true,
        Some("in") => false,
        _ => return Err(bad("shape.moveHandle", "`side` is in or out")),
    };
    let to = match (f64_param(p, "x"), f64_param(p, "y")) {
        (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Point::new(x.clamp(-1e5, 1e5), y.clamp(-1e5, 1e5)),
        _ => return Err(bad("shape.moveHandle", "give `x` and `y` in slide points")),
    };
    let kind = str_param(p, "type").filter(|k| matches!(*k, "smooth" | "straight" | "corner")).map(str::to_string);
    edit_path(s, id, path, "shape.moveHandle", move |segs, vs, x| {
        let si = *vs.get(index).ok_or_else(|| bad("shape.moveHandle", "no such point"))?;
        if set_handle(segs, si, out, x.affine().inverse() * to, kind.as_deref()) {
            Ok(true)
        } else {
            Err(bad("shape.moveHandle", "that side of the point has no handle"))
        }
    })
}

/// The type of vertex `index` of path `path` of `sh`.
pub fn kind_of(sh: &Shape, path: usize, index: usize) -> Option<&'static str> {
    let Geom::Custom { paths } = &sh.geom else { return None };
    let segs = parse(&paths.get(path)?.d);
    let si = *vertices(&segs).get(index)?;
    Some(point_kind(&segs, si))
}

/// `shape.moveHandle` for pointer drags.
pub(crate) fn move_handle_raw(s: &mut Session, p: &Value) -> Result<Value> {
    move_handle(s, p)
}

fn segment_type(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, p, "shape.segmentType")?;
    let path = p.get("path").and_then(Value::as_u64).unwrap_or(0) as usize;
    let seg = index_param(p, "segment", "shape.segmentType")?;
    let curved = match str_param(p, "type") {
        Some("straight") => false,
        Some("curved") => true,
        _ => return Err(bad("shape.segmentType", "`type` is straight or curved")),
    };
    // Already that type: nothing to change, so no undo step.
    let want = if curved { "curved" } else { "straight" };
    if s.doc()?.shape(id).is_some_and(|sh| segment_kind_of(sh, path, seg) == Some(want)) {
        return ok();
    }
    edit_path(s, id, path, "shape.segmentType", move |segs, vs, _| {
        let si = *vs.get(seg).ok_or_else(|| bad("shape.segmentType", "no such segment"))?;
        let j = ending_segment(segs, si).ok_or_else(|| bad("shape.segmentType", "the first point of an open outline ends no segment"))?;
        set_segment(segs, j, curved);
        Ok(true)
    })
}

fn open_path(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, p, "shape.openPath")?;
    let path = p.get("path").and_then(Value::as_u64).unwrap_or(0) as usize;
    let index = index_param(p, "index", "shape.openPath")?;
    let r = edit_path(s, id, path, "shape.openPath", move |segs, vs, _| {
        let si = *vs.get(index).ok_or_else(|| bad("shape.openPath", "no such point"))?;
        if open_at(segs, si) { Ok(true) } else { Err(bad("shape.openPath", "the outline is open already")) }
    })?;
    forget_kinds(s, path);
    Ok(r)
}

fn close_path(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, p, "shape.closePath")?;
    let path = p.get("path").and_then(Value::as_u64).unwrap_or(0) as usize;
    let index = index_param(p, "index", "shape.closePath")?;
    let r = edit_path(s, id, path, "shape.closePath", move |segs, vs, _| {
        let si = *vs.get(index).ok_or_else(|| bad("shape.closePath", "no such point"))?;
        if close_at(segs, si) { Ok(true) } else { Err(bad("shape.closePath", "the outline is closed already")) }
    })?;
    forget_kinds(s, path);
    Ok(r)
}

fn delete_segment(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, p, "shape.deleteSegment")?;
    let path = p.get("path").and_then(Value::as_u64).unwrap_or(0) as usize;
    let seg = index_param(p, "segment", "shape.deleteSegment")?;
    let r = edit_path(s, id, path, "shape.deleteSegment", move |segs, vs, _| {
        let si = *vs.get(seg).ok_or_else(|| bad("shape.deleteSegment", "no such segment"))?;
        if ending_segment(segs, si).is_none() {
            return Err(bad("shape.deleteSegment", "the first point of an open outline ends no segment"));
        }
        if delete_segment_at(segs, si) { Ok(true) } else { Err(bad("shape.deleteSegment", "only a segment of a closed outline can be deleted")) }
    })?;
    forget_kinds(s, path);
    Ok(r)
}

/// The point where the segment ending at vertex `index` of path `path` of `sh` starts.
pub fn segment_start_of(sh: &Shape, path: usize, index: usize) -> Option<usize> {
    let Geom::Custom { paths } = &sh.geom else { return None };
    let segs = parse(&paths.get(path)?.d);
    let vs = vertices(&segs);
    let j = ending_segment(&segs, *vs.get(index)?)?;
    vs.iter().position(|v| Some(*v) == j.checked_sub(1))
}

/// Adding, deleting, opening or closing renumbers the points of `path`: a point type remembered
/// for one of them would follow the wrong point.
fn forget_kinds(s: &mut Session, path: usize) {
    if s.tool.vertex_kind.as_ref().is_some_and(|k| k.1 == path) {
        s.tool.vertex_kind = None;
    }
}

/// Whether the segment ending at vertex `index` of path `path` of `sh` is straight or curved.
pub fn segment_kind_of(sh: &Shape, path: usize, index: usize) -> Option<&'static str> {
    let Geom::Custom { paths } = &sh.geom else { return None };
    let segs = parse(&paths.get(path)?.d);
    let si = *vertices(&segs).get(index)?;
    segment_kind(&segs, ending_segment(&segs, si)?)
}

/// Whether the outline through vertex `index` of path `path` of `sh` is closed.
pub fn closed_at(sh: &Shape, path: usize, index: usize) -> Option<bool> {
    let Geom::Custom { paths } = &sh.geom else { return None };
    let segs = parse(&paths.get(path)?.d);
    let si = *vertices(&segs).get(index)?;
    let (_, to) = subpath(&segs, si);
    Some(matches!(segs.get(to.checked_sub(1)?), Some(Seg::Z)))
}
