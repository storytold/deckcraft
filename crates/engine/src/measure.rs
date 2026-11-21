//! Measuring with ⌥ (Option) held, as canvas editors do (from FigCraft's `distance.rs`): red
//! distance lines between the selection's box and the layer under the pointer, or the slide's
//! edges when the pointer is over the selection or nothing. One box inside the other measures to
//! each edge of the outer one; boxes apart measure the gaps, with dashed extension lines to the
//! other box when the line passes beside it; boxes that overlap measure between matching edges.

use deckcraft_geom::{Point, Rect};
use deckcraft_model::ShapeId;

use crate::Session;
use crate::cmd::xfrm_on_slide;

/// Lines shorter than this (points) aren't drawn.
const EPS: f64 = 1e-6;

/// A distance line in slide points: `len` is shown on its label (dashed extension lines have none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measure {
    pub a: Point,
    pub b: Point,
    pub len: Option<f64>,
}

fn measured(out: &mut Vec<Measure>, a: Point, b: Point) {
    let d = (b - a).hypot();
    if d > EPS && d.is_finite() {
        out.push(Measure { a, b, len: Some(d) });
    }
}

fn extension(out: &mut Vec<Measure>, a: Point, b: Point) {
    if (b - a).hypot() > EPS {
        out.push(Measure { a, b, len: None });
    }
}

fn contains(outer: Rect, inner: Rect) -> bool {
    outer.x0 <= inner.x0 + EPS && outer.y0 <= inner.y0 + EPS && outer.x1 >= inner.x1 - EPS && outer.y1 >= inner.y1 - EPS
}

/// The middle of the overlap of two spans, if they overlap.
fn overlap_mid(a0: f64, a1: f64, b0: f64, b1: f64) -> Option<f64> {
    let (lo, hi) = (a0.max(b0), a1.min(b1));
    (lo <= hi).then_some((lo + hi) / 2.0)
}

fn finite(r: Rect) -> bool {
    [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite())
}

/// Distance lines from box `a` (the selection) to box `b`.
pub fn between(a: Rect, b: Rect) -> Vec<Measure> {
    let mut out = Vec::new();
    if !(finite(a) && finite(b)) {
        return out;
    }
    if contains(b, a) {
        inside(&mut out, a, b);
        return out;
    }
    if contains(a, b) {
        inside(&mut out, b, a);
        return out;
    }
    let ox = overlap_mid(a.x0, a.x1, b.x0, b.x1);
    let oy = overlap_mid(a.y0, a.y1, b.y0, b.y1);
    let overlapping = a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1;
    if let (true, Some(mx), Some(my)) = (overlapping, ox, oy) {
        measured(&mut out, Point::new(a.x0, my), Point::new(b.x0, my));
        measured(&mut out, Point::new(a.x1, my), Point::new(b.x1, my));
        measured(&mut out, Point::new(mx, a.y0), Point::new(mx, b.y0));
        measured(&mut out, Point::new(mx, a.y1), Point::new(mx, b.y1));
        return out;
    }
    let c = a.center();
    let gap_x = if b.x0 >= a.x1 {
        Some((a.x1, b.x0))
    } else if b.x1 <= a.x0 {
        Some((a.x0, b.x1))
    } else {
        None
    };
    if let Some((from, to)) = gap_x {
        let y = oy.unwrap_or(c.y);
        measured(&mut out, Point::new(from, y), Point::new(to, y));
        if oy.is_none() {
            extension(&mut out, Point::new(to, y), Point::new(to, if y < b.y0 { b.y0 } else { b.y1 }));
        }
    }
    let gap_y = if b.y0 >= a.y1 {
        Some((a.y1, b.y0))
    } else if b.y1 <= a.y0 {
        Some((a.y0, b.y1))
    } else {
        None
    };
    if let Some((from, to)) = gap_y {
        let x = ox.unwrap_or(c.x);
        measured(&mut out, Point::new(x, from), Point::new(x, to));
        if ox.is_none() {
            extension(&mut out, Point::new(x, to), Point::new(if x < b.x0 { b.x0 } else { b.x1 }, to));
        }
    }
    out
}

/// From `inner` to each edge of `outer`, along `inner`'s centre lines.
fn inside(out: &mut Vec<Measure>, inner: Rect, outer: Rect) {
    let c = inner.center();
    measured(out, Point::new(outer.x0, c.y), Point::new(inner.x0, c.y));
    measured(out, Point::new(inner.x1, c.y), Point::new(outer.x1, c.y));
    measured(out, Point::new(c.x, outer.y0), Point::new(c.x, inner.y0));
    measured(out, Point::new(c.x, inner.y1), Point::new(c.x, outer.y1));
}

/// While dragging with ⌥: from box `a` to the nearest box on each side among `boxes` (facing
/// it, sharing a span), else to that edge of `frame` (the slide) when `a` is inside it.
pub fn neighbours(a: Rect, boxes: &[Rect], frame: Rect) -> Vec<Measure> {
    let mut out = Vec::new();
    if !finite(a) {
        return out;
    }
    let c = a.center();
    let nearest = |side: u8| -> Option<(f64, f64)> {
        boxes
            .iter()
            .filter_map(|b| {
                let (edge, mid, dist) = match side {
                    0 => (b.x1, overlap_mid(a.y0, a.y1, b.y0, b.y1)?, a.x0 - b.x1),
                    1 => (b.x0, overlap_mid(a.y0, a.y1, b.y0, b.y1)?, b.x0 - a.x1),
                    2 => (b.y1, overlap_mid(a.x0, a.x1, b.x0, b.x1)?, a.y0 - b.y1),
                    _ => (b.y0, overlap_mid(a.x0, a.x1, b.x0, b.x1)?, b.y0 - a.y1),
                };
                (dist >= -EPS).then_some((edge, mid, dist))
            })
            .min_by(|x, y| x.2.total_cmp(&y.2))
            .map(|(edge, mid, _)| (edge, mid))
    };
    for side in 0..4u8 {
        let hit = nearest(side).or_else(|| {
            contains(frame, a).then_some(match side {
                0 => (frame.x0, c.y),
                1 => (frame.x1, c.y),
                2 => (frame.y0, c.x),
                _ => (frame.y1, c.x),
            })
        });
        let Some((edge, mid)) = hit else { continue };
        let (p, q) = match side {
            0 => (Point::new(edge, mid), Point::new(a.x0, mid)),
            1 => (Point::new(a.x1, mid), Point::new(edge, mid)),
            2 => (Point::new(mid, edge), Point::new(mid, a.y0)),
            _ => (Point::new(mid, a.y1), Point::new(mid, edge)),
        };
        measured(&mut out, p, q);
    }
    out
}

impl Session {
    /// While layers are being dragged: the distances from them to their nearest neighbours on
    /// the slide (or its edges), for ⌥ held during the drag.
    pub fn measure_drag(&self) -> Vec<Measure> {
        let Some(ids) = self.tool.moving_ids() else { return vec![] };
        let Some(st) = self.active() else { return vec![] };
        let boxes: Vec<Rect> = ids.iter().filter_map(|id| st.shape(*id)).map(|sh| xfrm_on_slide(&st.doc, &st.selection, sh).bounds()).collect();
        let Some(sel) = boxes.iter().copied().reduce(|a, b| a.union(b)) else { return vec![] };
        let list = st.shapes();
        // The other layers beside them: their group's other layers, else the slide's.
        let siblings: Vec<&deckcraft_model::Shape> = match ids.first().and_then(|id| deckcraft_model::parent_of(list, *id).flatten()) {
            Some(g) => deckcraft_model::find_shape(list, g).map(|g| g.children().iter().collect()).unwrap_or_default(),
            None => list.iter().collect(),
        };
        let others: Vec<Rect> =
            siblings.into_iter().filter(|s| !s.hidden && !ids.contains(&s.id)).map(|s| xfrm_on_slide(&st.doc, &st.selection, s).bounds()).collect();
        let size = st.doc.slide_size;
        neighbours(sel, &others, Rect::new(0.0, 0.0, size.width, size.height))
    }
    /// ⌥-hover at `p`: the lines from the selection to the layer under the pointer (`deep`: the
    /// innermost one, as ⌘ picks), or to the slide's edges over the selection or empty space;
    /// and the box measured to (to outline it). Nothing without a selection.
    pub fn measure_hover(&self, p: Point, tol: f64, deep: bool) -> (Vec<Measure>, Option<Rect>) {
        let Some(st) = self.active() else { return (vec![], None) };
        let boxes: Vec<Rect> =
            st.selection.shapes.iter().filter_map(|id| st.shape(*id)).map(|sh| xfrm_on_slide(&st.doc, &st.selection, sh).bounds()).collect();
        let Some(sel) = boxes.iter().copied().reduce(|a, b| a.union(b)) else { return (vec![], None) };
        let hit = if deep { self.deep_shape_at(p, tol) } else { self.shape_at(p, tol) };
        let target = hit
            .filter(|id| !st.selection.shapes.contains(id))
            .and_then(|id| st.shape(id))
            .map(|sh| xfrm_on_slide(&st.doc, &st.selection, sh).bounds());
        let size = st.doc.slide_size;
        let slide = Rect::new(0.0, 0.0, size.width, size.height);
        match target {
            Some(b) => (between(sel, b), Some(b)),
            None => (between(sel, slide), None),
        }
    }

    fn shape_at(&self, p: Point, tol: f64) -> Option<ShapeId> {
        match self.hit_test(p, tol)? {
            crate::tools::Hit::Shape { id, .. } => Some(id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect::new(x0, y0, x1, y1)
    }

    fn lens(v: &[Measure]) -> Vec<f64> {
        v.iter().filter_map(|m| m.len).collect()
    }

    #[test]
    fn a_dragged_box_measures_to_its_neighbours_or_the_slide() {
        let slide = r(0.0, 0.0, 1000.0, 500.0);
        let a = r(400.0, 200.0, 500.0, 300.0);
        // One neighbour to the right, sharing rows; the slide's edges on the other sides.
        let v = neighbours(a, &[r(600.0, 250.0, 700.0, 350.0), r(600.0, 0.0, 700.0, 50.0)], slide);
        assert_eq!(lens(&v), vec![400.0, 100.0, 200.0, 200.0]);
    }

    #[test]
    fn boxes_apart_measure_the_gap_inside_ones_each_edge() {
        // Side by side, sharing rows: one gap.
        assert_eq!(lens(&between(r(0.0, 0.0, 10.0, 10.0), r(30.0, 2.0, 40.0, 8.0))), vec![20.0]);
        // Diagonal: both gaps with dashed extensions.
        let d = between(r(0.0, 0.0, 10.0, 10.0), r(30.0, 50.0, 40.0, 60.0));
        assert_eq!(lens(&d), vec![20.0, 40.0]);
        assert_eq!(d.iter().filter(|m| m.len.is_none()).count(), 2);
        // Inside: to every edge.
        assert_eq!(lens(&between(r(10.0, 20.0, 30.0, 40.0), r(0.0, 0.0, 100.0, 100.0))), vec![10.0, 70.0, 20.0, 60.0]);
        // Overlapping: matching edges.
        assert_eq!(lens(&between(r(0.0, 0.0, 20.0, 20.0), r(10.0, 5.0, 30.0, 25.0))), vec![10.0, 10.0, 5.0, 5.0]);
    }
}
