//! Draw ▸ Stencils ▸ Ruler: the on-slide ruler.
//!
//! PowerPoint's ruler is a view aid, not part of the presentation: it is never saved in the deck.
//! It is a long translucent body with a tick scale and numbers on both edges and a degree dial at
//! its centre. Dragging the body moves it, the wheel over it turns it 1° around the pointer, and
//! Shift+F6 enters manipulation mode where the arrow keys move it and Alt+arrows turn it 15° (with
//! Ctrl or Cmd too, 1°); clicking the dial types an exact angle. Ink drawn near an edge is
//! straightened along that edge, and a dragged object's bounding box snaps to a nearby edge.
//!
//! Everything in this module is pure geometry over [`RulerState`]; the commands that change the
//! state live in `cmd::draw`.

use deckcraft_geom::{Point, Rect, Vec2};
use serde::{Deserialize, Serialize};

/// The ruler body's length in points (just over a 13-inch slide wide).
pub const LENGTH: f64 = 960.0;
/// Half the body's width: its two edge lines sit this far either side of the centre line.
pub const HALF_WIDTH: f64 = 26.0;
/// Radius of the degree dial at the centre, in points (the rotation grip).
pub const DIAL: f64 = 15.0;
/// How near an edge a stroke or a box must come to snap, in points.
pub const SNAP: f64 = 8.0;
/// The furthest the ruler's centre may be placed from the slide origin, in points. A command or a
/// control channel client can send any number; this keeps the geometry finite and drawable.
pub const MAX_OFFSET: f64 = 100_000.0;
/// The angle a new ruler starts at, as PowerPoint shows it when first switched on.
pub const DEFAULT_ANGLE: f64 = 45.0;

/// The session's view state: what is shown on the canvas rather than what is in the document.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ViewState {
    pub ruler: RulerState,
}

/// The ruler's state: view state of the session, never the document.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RulerState {
    pub visible: bool,
    /// Centre of the body, in slide points.
    pub x: f64,
    pub y: f64,
    /// Degrees, 0..360, measured from the +x axis towards +y (y points down the slide).
    pub angle: f64,
    /// Shift+F6: the arrow keys move the ruler instead of the selection.
    pub manipulate: bool,
    /// Whether the ruler has been placed on a page yet; the first show centres it.
    pub placed: bool,
}

impl Default for RulerState {
    fn default() -> Self {
        RulerState { visible: false, x: 0.0, y: 0.0, angle: DEFAULT_ANGLE, manipulate: false, placed: false }
    }
}

impl RulerState {
    /// The state with every value usable: non-finite numbers become defaults, the angle wraps to
    /// 0..360 and the centre is bounded.
    pub fn sanitized(mut self) -> RulerState {
        self.x = clamp_pos(self.x);
        self.y = clamp_pos(self.y);
        self.angle = normalize_angle(self.angle);
        if !self.visible {
            self.manipulate = false;
        }
        self
    }

    /// The body's centre.
    pub fn center(&self) -> Point {
        Point::new(self.x, self.y)
    }
}

fn clamp_pos(v: f64) -> f64 {
    if v.is_finite() { v.clamp(-MAX_OFFSET, MAX_OFFSET) } else { 0.0 }
}

/// Wrap an angle into `0..360`; a non-finite input is `0`.
pub fn normalize_angle(a: f64) -> f64 {
    if !a.is_finite() {
        return 0.0;
    }
    let m = a % 360.0;
    let m = if m < 0.0 { m + 360.0 } else { m };
    if m == 0.0 { 0.0 } else { m }
}

/// The turn one arrow or wheel step asks for: Alt is 15°, Alt+Ctrl (or Alt+Cmd) is 1°, and a
/// plain wheel notch is 1°.
pub fn rotation_step(alt: bool, ctrl: bool) -> f64 {
    if alt && !ctrl { 15.0 } else { 1.0 }
}

/// One edge of the ruler body: a finite segment and its unit direction and normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeLine {
    pub from: Point,
    pub to: Point,
    /// Unit vector along the segment.
    pub dir: Vec2,
    /// Unit normal (`dir` turned 90° clockwise on screen).
    pub normal: Vec2,
}

impl EdgeLine {
    /// Perpendicular distance from `p` to the infinite line the segment lies on.
    pub fn distance(&self, p: Point) -> f64 {
        (p - self.from).cross(self.dir).abs()
    }

    /// Distance from `p` to the segment (past an end counts the along-axis gap too).
    pub fn segment_distance(&self, p: Point) -> f64 {
        let t = (p - self.from).dot(self.dir).clamp(0.0, (self.to - self.from).dot(self.dir));
        (self.from + self.dir * t - p).hypot()
    }

    /// The point of the infinite line nearest `p`.
    pub fn project(&self, p: Point) -> Point {
        self.from + self.dir * (p - self.from).dot(self.dir)
    }
}

/// The two edges of the body, from its centre, angle and length.
pub fn edges(r: &RulerState) -> [EdgeLine; 2] {
    let a = normalize_angle(r.angle).to_radians();
    let dir = Vec2::new(a.cos(), a.sin());
    let normal = Vec2::new(-a.sin(), a.cos());
    let c = r.center();
    let half = LENGTH / 2.0;
    [1.0, -1.0].map(|side| {
        let mid = c + normal * (side * HALF_WIDTH);
        EdgeLine { from: mid - dir * half, to: mid + dir * half, dir, normal }
    })
}

/// The edge nearest `points` (every sample considered) and its distance.
pub fn nearest_edge(r: &RulerState, points: &[Point]) -> Option<(usize, f64)> {
    if points.is_empty() || !points.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
        return None;
    }
    let mut best: Option<(usize, f64)> = None;
    for (i, e) in edges(r).iter().enumerate() {
        let d = points.iter().map(|p| e.distance(*p)).fold(f64::INFINITY, f64::min);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best
}

/// Whether `p` is over the ruler body (the rotated rectangle), in slide points.
pub fn contains(r: &RulerState, p: Point) -> bool {
    if !r.visible || !(p.x.is_finite() && p.y.is_finite()) {
        return false;
    }
    let a = normalize_angle(r.angle).to_radians();
    let local = p - r.center();
    let x = local.x * a.cos() + local.y * a.sin();
    let y = -local.x * a.sin() + local.y * a.cos();
    x.abs() <= LENGTH / 2.0 && y.abs() <= HALF_WIDTH
}

/// Whether `p` is over the degree dial at the centre.
pub fn on_dial(r: &RulerState, p: Point) -> bool {
    r.visible && (p - r.center()).hypot() <= DIAL
}

/// Turn the ruler by `degrees` about `pivot`, keeping that point where it is.
pub fn rotate_about(r: &RulerState, pivot: Point, degrees: f64) -> RulerState {
    if !degrees.is_finite() {
        return *r;
    }
    let a = degrees.to_radians();
    let d = r.center() - pivot;
    let turned = Vec2::new(d.x * a.cos() - d.y * a.sin(), d.x * a.sin() + d.y * a.cos());
    let mut out = *r;
    out.x = clamp_pos(pivot.x + turned.x);
    out.y = clamp_pos(pivot.y + turned.y);
    out.angle = normalize_angle(r.angle + degrees);
    out
}

/// A stroke straightened along the nearest ruler edge, when the ruler is on and the stroke comes
/// within `tol` of an edge. The straight line keeps the stroke's along-edge extent (the
/// projection of its first and last points) and each end's pressure; colour and width are the
/// tool's and are not part of this function.
pub fn straighten(points: &[(f64, f64, f32)], r: &RulerState, tol: f64) -> Option<Vec<(f64, f64, f32)>> {
    if !r.visible || points.len() < 2 {
        return None;
    }
    let samples: Vec<Point> = points.iter().map(|(x, y, _)| Point::new(*x, *y)).collect();
    if !samples.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
        return None;
    }
    let tol = if tol.is_finite() { tol.max(0.0) } else { SNAP };
    let (i, d) = nearest_edge(r, &samples)?;
    if d > tol {
        return None;
    }
    let e = edges(r).get(i).copied()?;
    let (first, last) = (points.first()?, points.last()?);
    let a = e.project(Point::new(first.0, first.1));
    let b = e.project(Point::new(last.0, last.1));
    // A stroke that lands on one spot has no line to draw.
    if (b - a).hypot() < 0.5 {
        return None;
    }
    Some(vec![(a.x, a.y, first.2), (b.x, b.y, last.2)])
}

/// Snap `rect`'s nearest side to the nearest ruler edge when it is within `tol`, returning the
/// translation that does it. The translation is along the edge's normal, so a horizontal edge
/// brings the box's parallel side onto the line, and a turned edge brings the box's nearest
/// corner onto it (an axis-aligned box cannot have an edge along a diagonal line). A box the
/// edge already crosses, or runs through, is left alone.
pub fn snap_rect(rect: Rect, r: &RulerState, tol: f64) -> Option<Vec2> {
    if !r.visible || !(rect.x0.is_finite() && rect.y0.is_finite() && rect.x1.is_finite() && rect.y1.is_finite()) {
        return None;
    }
    let tol = if tol.is_finite() { tol.max(0.0) } else { SNAP };
    let half = Vec2::new(rect.width() / 2.0, rect.height() / 2.0);
    let centre = rect.center();
    let mut best: Option<(f64, Vec2)> = None;
    for e in edges(r) {
        let line = Vec2::new(e.from.x, e.from.y).dot(e.normal);
        let mid = Vec2::new(centre.x, centre.y).dot(e.normal);
        // How far the box reaches along the normal: its nearest side is this close to the line.
        let reach = half.x * e.normal.x.abs() + half.y * e.normal.y.abs();
        let signed = mid - line;
        let gap = signed.abs() - reach;
        if gap <= 1e-6 || gap > tol {
            continue;
        }
        let delta = if signed > 0.0 { -e.normal * gap } else { e.normal * gap };
        if best.is_none_or(|(bd, _)| gap < bd) {
            best = Some((gap, delta));
        }
    }
    best.map(|(_, d)| d)
}

/// Slide-space rect of a box's corners (rotation ignored: the drag works on bounding boxes).
pub fn bounds_of(points: &[Point]) -> Option<Rect> {
    let mut it = points.iter().filter(|p| p.x.is_finite() && p.y.is_finite());
    let first = *it.next()?;
    let mut r = Rect::new(first.x, first.y, first.x, first.y);
    for p in it {
        r = r.union(Rect::new(p.x, p.y, p.x, p.y));
    }
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ruler(x: f64, y: f64, angle: f64) -> RulerState {
        RulerState { visible: true, x, y, angle, ..Default::default() }
    }

    #[test]
    fn angles_wrap_into_one_turn() {
        for (a, want) in [(0.0, 0.0), (45.0, 45.0), (360.0, 0.0), (-45.0, 315.0), (725.0, 5.0), (-720.0, 0.0)] {
            assert_eq!(normalize_angle(a), want, "{a}");
        }
        assert_eq!(normalize_angle(f64::NAN), 0.0);
        assert_eq!(normalize_angle(f64::INFINITY), 0.0);
        assert_eq!(normalize_angle(-0.0), 0.0);
        assert!(!normalize_angle(-1e-300).to_string().starts_with('-'));
    }

    #[test]
    fn rotation_steps_follow_the_modifiers() {
        assert_eq!(rotation_step(false, false), 1.0);
        assert_eq!(rotation_step(true, false), 15.0);
        assert_eq!(rotation_step(true, true), 1.0, "Alt+Ctrl is the fine step");
    }

    #[test]
    fn edges_sit_either_side_of_the_centre_line() {
        let r = ruler(100.0, 100.0, 0.0);
        // Side +1 is the normal's side: below the centre line, since y points down the slide.
        let [a, b] = edges(&r);
        assert!((a.from.y - (100.0 + HALF_WIDTH)).abs() < 1e-9, "{a:?}");
        assert!((b.from.y - (100.0 - HALF_WIDTH)).abs() < 1e-9, "{b:?}");
        assert!((a.from.x - (100.0 - LENGTH / 2.0)).abs() < 1e-9);
        assert!((a.to.x - (100.0 + LENGTH / 2.0)).abs() < 1e-9);
        // A quarter turn puts the edges to the left and right.
        let r = ruler(100.0, 100.0, 90.0);
        let [a, b] = edges(&r);
        assert!((a.from.x - (100.0 - HALF_WIDTH)).abs() < 1e-9, "{a:?}");
        assert!((b.from.x - (100.0 + HALF_WIDTH)).abs() < 1e-9, "{b:?}");
    }

    #[test]
    fn projecting_a_point_lands_it_on_the_edge() {
        let r = ruler(100.0, 100.0, 0.0);
        let [a, b] = edges(&r);
        let _ = a;
        let q = b.project(Point::new(120.0, 40.0));
        assert!((q.x - 120.0).abs() < 1e-9 && (q.y - (100.0 - HALF_WIDTH)).abs() < 1e-9, "{q:?}");
        assert!(b.distance(Point::new(120.0, 40.0)) > 0.0);
        assert!((b.segment_distance(Point::new(120.0, 40.0)) - (100.0 - HALF_WIDTH - 40.0)).abs() < 1e-6);
    }

    #[test]
    fn a_stroke_near_an_edge_becomes_a_straight_line_along_it() {
        let r = ruler(300.0, 200.0, 0.0);
        let edge_y = 200.0 - HALF_WIDTH;
        // A wobbly stroke just above the top edge.
        let stroke: Vec<(f64, f64, f32)> = (0..20).map(|i| (220.0 + i as f64 * 8.0, edge_y - 3.0 + (i % 3) as f64, 0.6)).collect();
        let line = straighten(&stroke, &r, SNAP).expect("snapped");
        assert_eq!(line.len(), 2, "{line:?}");
        assert!((line[0].1 - edge_y).abs() < 1e-9 && (line[1].1 - edge_y).abs() < 1e-9, "{line:?}");
        assert!((line[0].0 - 220.0).abs() < 1e-9 && (line[1].0 - 372.0).abs() < 1e-9, "extent kept: {line:?}");
        assert_eq!((line[0].2, line[1].2), (0.6, 0.6), "pressure kept");
    }

    #[test]
    fn a_stroke_that_crosses_the_ruler_at_an_angle_projects_along_the_edge() {
        let r = ruler(300.0, 200.0, 0.0);
        let edge_y = 200.0 + HALF_WIDTH;
        // Drawn diagonally across the body, ending just under the lower edge.
        let stroke = [(240.0, 260.0, 1.0), (300.0, 245.0, 1.0), (360.0, 232.0, 1.0)];
        let line = straighten(&stroke, &r, SNAP).expect("the lower edge is nearest within the threshold");
        assert_eq!(line.len(), 2);
        assert!((line[0].1 - edge_y).abs() < 1e-9 && (line[1].1 - edge_y).abs() < 1e-9, "{line:?}");
        assert!((line[0].0 - 240.0).abs() < 1e-9 && (line[1].0 - 360.0).abs() < 1e-9, "{line:?}");
    }

    #[test]
    fn a_stroke_away_from_the_ruler_stays_freehand() {
        let r = ruler(300.0, 200.0, 0.0);
        let stroke = [(100.0, 500.0, 1.0), (400.0, 520.0, 1.0)];
        assert!(straighten(&stroke, &r, SNAP).is_none());
        // Hidden: never snaps.
        let hidden = RulerState { visible: false, ..r };
        assert!(straighten(&stroke, &hidden, 10_000.0).is_none());
        // One sample is not a line.
        assert!(straighten(&stroke[..1], &r, SNAP).is_none());
        // Hostile values are refused, not drawn.
        assert!(straighten(&[(f64::NAN, 1.0, 1.0), (2.0, 3.0, 1.0)], &r, SNAP).is_none());
        // A hostile tolerance falls back to the normal threshold rather than refusing to snap.
        assert!(straighten(&[(300.0, 500.0, 1.0), (340.0, 500.0, 1.0)], &r, f64::NAN).is_none(), "far from the ruler");
        assert!(straighten(&[(300.0, 231.0, 1.0), (340.0, 232.0, 1.0)], &r, f64::NAN).is_some());
        // A degenerate stroke (both ends project to one spot) draws nothing.
        let dot = [(300.0, 260.0, 1.0), (300.0, 261.0, 1.0)];
        assert!(straighten(&dot, &r, SNAP).is_none());
    }

    #[test]
    fn a_box_snaps_its_nearest_side() {
        let r = ruler(300.0, 200.0, 0.0);
        let top = 200.0 - HALF_WIDTH;
        // A box whose top is 3 pt above the top edge.
        let rect = Rect::new(240.0, top - 43.0, 340.0, top - 3.0);
        let d = snap_rect(rect, &r, SNAP).expect("snapped");
        assert!((d.x).abs() < 1e-9 && (d.y - 3.0).abs() < 1e-9, "{d:?}");
        // Wider than the threshold: untouched.
        assert!(snap_rect(Rect::new(240.0, top - 60.0, 340.0, top - 20.0), &r, SNAP).is_none());
        // Straddling the edge: already aligned, no jump.
        assert!(snap_rect(Rect::new(240.0, top - 20.0, 340.0, top + 20.0), &r, SNAP).is_none());
        // A rotated ruler snaps perpendicular to its own edge.
        let r = ruler(0.0, 0.0, 90.0);
        let x = -HALF_WIDTH;
        let d = snap_rect(Rect::new(x + 2.0, -30.0, x + 42.0, 30.0), &r, SNAP).expect("snapped on x");
        assert!((d.x - -2.0).abs() < 1e-9 && d.y.abs() < 1e-9, "{d:?}");
    }

    #[test]
    fn a_box_snaps_to_a_turned_ruler_by_its_nearest_corner() {
        let r = ruler(300.0, 300.0, 45.0);
        let [e, _] = edges(&r);
        let n = e.normal;
        let line = Vec2::new(e.from.x, e.from.y).dot(n);
        // A 60x60 box whose nearest corner comes 4 pt inside the edge's side.
        let reach = 30.0 * n.x.abs() + 30.0 * n.y.abs();
        let at = line + reach + 4.0;
        let centre = Point::new(300.0, 300.0) + n * (at - Vec2::new(300.0, 300.0).dot(n));
        let rect = Rect::new(centre.x - 30.0, centre.y - 30.0, centre.x + 30.0, centre.y + 30.0);
        let d = snap_rect(rect, &r, SNAP).expect("snapped");
        // Along the normal only, and the box now touches the edge.
        assert!((d.x * n.y - d.y * n.x).abs() < 1e-6, "perpendicular: {d:?}");
        let moved = rect + d;
        let reach = 30.0 * n.x.abs() + 30.0 * n.y.abs();
        let mid = Vec2::new(moved.center().x, moved.center().y).dot(n);
        assert!((mid - line).abs() - reach <= 1e-6, "{:?} vs {line}", moved);
        assert!(snap_rect(moved, &r, SNAP).is_none(), "nothing left to snap");
    }

    #[test]
    fn rotating_about_a_point_keeps_it_fixed() {
        let r = ruler(100.0, 100.0, 0.0);
        let pivot = Point::new(100.0, 50.0);
        let out = rotate_about(&r, pivot, 90.0);
        assert!((out.angle - 90.0).abs() < 1e-9);
        // A quarter turn clockwise on screen takes the centre below the pivot to its left.
        assert!((out.x - 50.0).abs() < 1e-9 && (out.y - 50.0).abs() < 1e-9, "{out:?}");
        // Rotating back restores the state.
        let back = rotate_about(&out, pivot, -90.0);
        assert!((back.x - 100.0).abs() < 1e-9 && (back.y - 100.0).abs() < 1e-9);
        assert_eq!(back.angle, 0.0);
    }

    #[test]
    fn hit_tests_follow_the_rotation() {
        let r = ruler(0.0, 0.0, 90.0);
        assert!(contains(&r, Point::new(0.0, 400.0)), "along the turned body");
        assert!(!contains(&r, Point::new(400.0, 0.0)), "across it");
        assert!(on_dial(&r, Point::new(5.0, 0.0)));
        assert!(!on_dial(&r, Point::new(40.0, 0.0)));
    }

    #[test]
    fn hostile_state_is_bounded() {
        let r = RulerState { visible: true, x: f64::INFINITY, y: -f64::INFINITY, angle: f64::NAN, manipulate: true, placed: true };
        let s = r.sanitized();
        assert_eq!((s.x, s.y, s.angle), (0.0, 0.0, 0.0));
        let s = RulerState { visible: true, x: 1e300, y: -1e300, angle: 1e300, ..Default::default() }.sanitized();
        assert_eq!((s.x, s.y), (MAX_OFFSET, -MAX_OFFSET));
        assert!(s.angle.is_finite() && (0.0..360.0).contains(&s.angle));
        let hidden = RulerState { visible: false, manipulate: true, ..Default::default() }.sanitized();
        assert!(!hidden.manipulate, "manipulation mode needs a visible ruler");
    }
}
