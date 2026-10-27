use super::catalog::{FillMode, Geometry, Handle, SubPath};
use super::*;

// ---------- path builder ----------

pub(super) struct Pb {
    pub(super) p: BezPath,
    pub(super) cur: Point,
}

impl Pb {
    pub(super) fn new() -> Self {
        Pb { p: BezPath::new(), cur: Point::ZERO }
    }
    pub(super) fn m(&mut self, x: f64, y: f64) -> &mut Self {
        self.cur = Point::new(x, y);
        self.p.move_to(self.cur);
        self
    }
    pub(super) fn l(&mut self, x: f64, y: f64) -> &mut Self {
        self.cur = Point::new(x, y);
        self.p.line_to(self.cur);
        self
    }
    pub(super) fn c(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64) -> &mut Self {
        self.cur = Point::new(x, y);
        self.p.curve_to(Point::new(x1, y1), Point::new(x2, y2), self.cur);
        self
    }
    pub(super) fn q(&mut self, x1: f64, y1: f64, x: f64, y: f64) -> &mut Self {
        self.cur = Point::new(x, y);
        self.p.quad_to(Point::new(x1, y1), self.cur);
        self
    }
    /// Elliptic arc continuing from the current point, which lies on the ellipse at parametric
    /// angle `st` (degrees); sweeps `sw` degrees (positive = clockwise on screen).
    pub(super) fn arc(&mut self, wr: f64, hr: f64, st: f64, sw: f64) -> &mut Self {
        let (s, e) = (st.to_radians(), (st + sw).to_radians());
        let c = Point::new(self.cur.x - wr * s.cos(), self.cur.y - hr * s.sin());
        let a = Arc::new(c, Vec2::new(wr.abs(), hr.abs()), s, e - s, 0.0);
        a.to_cubic_beziers(0.05, |p1, p2, p| {
            self.p.curve_to(p1, p2, p);
        });
        self.cur = Point::new(c.x + wr * e.cos(), c.y + hr * e.sin());
        self
    }
    /// A DrawingML `arcTo`: angles are where the ray from the ellipse's centre at that angle
    /// meets it (not the ellipse's parameter), so elliptical arcs start and end where the
    /// standard's shapes expect.
    pub(super) fn arc_spec(&mut self, wr: f64, hr: f64, st: f64, sw: f64) -> &mut Self {
        let par = |a: f64| {
            let a = a.to_radians();
            (wr * a.sin()).atan2(hr * a.cos())
        };
        let t0 = par(st);
        let mut d = par(st + sw) - t0;
        let tau = std::f64::consts::TAU;
        if sw.abs() >= 360.0 {
            d = tau.copysign(sw);
        } else {
            while sw > 0.0 && d < 0.0 {
                d += tau;
            }
            while sw < 0.0 && d > 0.0 {
                d -= tau;
            }
        }
        self.arc(wr, hr, t0.to_degrees(), d.to_degrees())
    }
    pub(super) fn z(&mut self) -> &mut Self {
        self.p.close_path();
        self
    }
    pub(super) fn poly(&mut self, pts: &[(f64, f64)]) -> &mut Self {
        for (i, (x, y)) in pts.iter().enumerate() {
            if i == 0 {
                self.m(*x, *y);
            } else {
                self.l(*x, *y);
            }
        }
        self.z()
    }
    pub(super) fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64) -> &mut Self {
        self.m(cx + rx, cy);
        self.arc(rx, ry, 0.0, 360.0);
        self.z()
    }
    pub(super) fn take(&mut self) -> BezPath {
        std::mem::take(&mut self.p)
    }
}

pub(super) fn fill(p: BezPath) -> SubPath {
    SubPath { path: p, fill: FillMode::Norm, stroke: true, even_odd: false }
}

pub(super) fn fill_eo(p: BezPath) -> SubPath {
    SubPath { path: p, fill: FillMode::Norm, stroke: true, even_odd: true }
}

pub(super) fn shade(p: BezPath, m: FillMode) -> SubPath {
    SubPath { path: p, fill: m, stroke: true, even_odd: false }
}

pub(super) fn stroke_only(p: BezPath) -> SubPath {
    SubPath { path: p, fill: FillMode::None, stroke: true, even_odd: false }
}

pub(super) fn fill_only(p: BezPath) -> SubPath {
    SubPath { path: p, fill: FillMode::Norm, stroke: false, even_odd: false }
}

/// Parametric angle on an rx×ry ellipse for a visual angle `deg` (file angles are visual).
pub(super) fn vis(deg: f64, rx: f64, ry: f64) -> f64 {
    let t = deg.to_radians();
    (rx * t.sin()).atan2(ry * t.cos()).to_degrees()
}

pub(super) fn polygon_regular(n: usize, w: f64, h: f64, phase: f64) -> Vec<(f64, f64)> {
    let pts: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let a = (phase + 360.0 * i as f64 / n as f64).to_radians();
            (a.cos(), a.sin())
        })
        .collect();
    normalize(&pts, w, h)
}

/// Scale `pts` so their bounding box fills 0..w × 0..h.
fn normalize(pts: &[(f64, f64)], w: f64, h: f64) -> Vec<(f64, f64)> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in pts {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }
    let sx = if x1 - x0 > 1e-12 { w / (x1 - x0) } else { 0.0 };
    let sy = if y1 - y0 > 1e-12 { h / (y1 - y0) } else { 0.0 };
    pts.iter().map(|(x, y)| ((x - x0) * sx, (y - y0) * sy)).collect()
}

/// An n-pointed star as ECMA-376 draws it: points on an ellipse around (hc, vc · vf) with radii
/// half the box stretched by the preset's `hf` / `vf` (so a five-pointed star fills its box and
/// looks regular in a square), the inner corners `inner` of the way out.
pub(super) fn star(n: usize, w: f64, h: f64, inner: f64) -> Vec<(f64, f64)> {
    let (hf, vf) = match n {
        5 => (1.05146, 1.10557),
        6 => (1.15470, 1.0),
        7 => (1.02572, 1.05210),
        10 => (1.05146, 1.0),
        _ => (1.0, 1.0),
    };
    let (rx, ry, cy) = (w / 2.0 * hf, h / 2.0 * vf, h / 2.0 * vf);
    let mut pts = Vec::with_capacity(n * 2);
    for i in 0..n * 2 {
        let a = (-90.0 + 180.0 * i as f64 / n as f64).to_radians();
        let r = if i % 2 == 0 { 1.0 } else { inner };
        pts.push((w / 2.0 + r * rx * a.cos(), cy + r * ry * a.sin()));
    }
    pts
}

/// The text rectangle ECMA-376's presetShapeDefinitions give a star (`inner` is the adjust value
/// over 50000), where known: star4's is the square inside its inner points, star5's spans the
/// inner pentagon from its upper side points to its bottom inner point.
pub(super) fn star_text_rect(n: usize, w: f64, h: f64, inner: f64) -> Option<Rect> {
    let (hc, vc) = (w / 2.0, h / 2.0);
    match n {
        4 => {
            let (dx, dy) = (hc * inner * 45f64.to_radians().cos(), vc * inner * 45f64.to_radians().sin());
            Some(Rect::new(hc - dx, vc - dy, hc + dx, vc + dy))
        }
        5 => {
            let (swd2, shd2, svc) = (hc * 1.05146, vc * 1.10557, vc * 1.10557);
            let (iwd2, ihd2) = (swd2 * inner, shd2 * inner);
            let sdx1 = iwd2 * 18f64.to_radians().cos();
            let sdy1 = ihd2 * 54f64.to_radians().sin();
            Some(Rect::new(hc - sdx1, svc - sdy1, hc + sdx1, svc + ihd2))
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
pub(super) enum Corner {
    Square,
    Round(f64),
    Snip(f64),
}

/// A rectangle with each corner (TL, TR, BR, BL) square, rounded or snipped.
pub(super) fn corner_rect(w: f64, h: f64, c: [Corner; 4]) -> BezPath {
    let mut b = Pb::new();
    let size = |k: Corner| match k {
        Corner::Square => 0.0,
        Corner::Round(r) | Corner::Snip(r) => r.max(0.0),
    };
    let s0 = size(c[0]);
    b.m(s0, 0.0);
    // top edge → TR
    let s1 = size(c[1]);
    b.l(w - s1, 0.0);
    match c[1] {
        Corner::Round(r) if r > 0.0 => {
            b.arc(r, r, 270.0, 90.0);
        }
        Corner::Snip(_) => {
            b.l(w, s1);
        }
        _ => {}
    }
    let s2 = size(c[2]);
    b.l(w, h - s2);
    match c[2] {
        Corner::Round(r) if r > 0.0 => {
            b.arc(r, r, 0.0, 90.0);
        }
        Corner::Snip(_) => {
            b.l(w - s2, h);
        }
        _ => {}
    }
    let s3 = size(c[3]);
    b.l(s3, h);
    match c[3] {
        Corner::Round(r) if r > 0.0 => {
            b.arc(r, r, 90.0, 90.0);
        }
        Corner::Snip(_) => {
            b.l(0.0, h - s3);
        }
        _ => {}
    }
    b.l(0.0, s0);
    match c[0] {
        Corner::Round(r) if r > 0.0 => {
            b.arc(r, r, 180.0, 90.0);
        }
        Corner::Snip(_) => {
            b.l(s0, 0.0);
        }
        _ => {}
    }
    b.z();
    b.take()
}

pub(super) fn mid_sites(w: f64, h: f64) -> Vec<Point> {
    vec![Point::new(w / 2.0, 0.0), Point::new(0.0, h / 2.0), Point::new(w / 2.0, h), Point::new(w, h / 2.0)]
}

/// Orientation for single-axis arrows built along +x.
#[derive(Clone, Copy)]
pub(super) enum Dir {
    Right,
    Left,
    Up,
    Down,
}

/// Map canonical (u along the arrow's length L, v across thickness T) to shape space.
pub(super) fn orient(d: Dir, w: f64, h: f64, pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    pts.iter()
        .map(|&(u, v)| match d {
            Dir::Right => (u, v),
            Dir::Left => (w - u, v),
            Dir::Up => (v, h - u),
            Dir::Down => (v, u),
        })
        .collect()
}

pub(super) fn len_thick(d: Dir, w: f64, h: f64) -> (f64, f64) {
    match d {
        Dir::Right | Dir::Left => (w, h),
        Dir::Up | Dir::Down => (h, w),
    }
}

pub(super) fn arrow(d: Dir, w: f64, h: f64, a1: f64, a2: f64) -> Geometry {
    let (l, t) = len_thick(d, w, h);
    let ss = l.min(t);
    let a1 = a1.clamp(0.0, 100000.0) / 100000.0;
    let dx = (ss * a2.clamp(0.0, 100000.0 * l / ss.max(1e-9)) / 100000.0).min(l);
    let y1 = t / 2.0 - t * a1 / 2.0;
    let y2 = t / 2.0 + t * a1 / 2.0;
    let pts = [(0.0, y1), (l - dx, y1), (l - dx, 0.0), (l, t / 2.0), (l - dx, t), (l - dx, y2), (0.0, y2)];
    let pts = orient(d, w, h, &pts);
    let tr = orient(d, w, h, &[(0.0, y1), (l - dx + dx * (y1 / (t / 2.0).max(1e-9)), y2)]);
    let mut text = Rect::from_points(tr[0], tr[1]);
    text = text.abs();
    let mut g = Geometry { paths: vec![fill(Pb::new().poly(&pts).take())], text_rect: text, handles: vec![], sites: mid_sites(w, h) };
    // handle 0: shaft thickness on the head base; handle 1: head length.
    let o = orient(d, w, h, &[(l - dx, t / 2.0), (l, t / 2.0)]);
    let (o0, o1) = (Point::new(o[0].0, o[0].1), Point::new(o[1].0, o[1].1));
    let across = match d {
        Dir::Right | Dir::Left => Vec2::new(0.0, -t / 2.0 / 100000.0),
        Dir::Up | Dir::Down => Vec2::new(-t / 2.0 / 100000.0, 0.0),
    };
    let along = (o0 - o1) * (ss / 100000.0 / dx.max(1e-9));
    g.handles.push(Handle::new(0, o0, across, a1 * 100000.0, 0.0, 100000.0));
    if dx > 1e-9 {
        g.handles.push(Handle::new(1, o1, along, a2, 0.0, 100000.0 * l / ss.max(1e-9)));
    }
    g
}

pub(super) fn double_arrow(vertical: bool, w: f64, h: f64, a1: f64, a2: f64) -> Geometry {
    let (l, t) = if vertical { (h, w) } else { (w, h) };
    let ss = l.min(t);
    let a1 = a1.clamp(0.0, 100000.0) / 100000.0;
    let dx = (ss * a2.max(0.0) / 100000.0).min(l / 2.0);
    let y1 = t / 2.0 - t * a1 / 2.0;
    let y2 = t / 2.0 + t * a1 / 2.0;
    let pts = [(0.0, t / 2.0), (dx, 0.0), (dx, y1), (l - dx, y1), (l - dx, 0.0), (l, t / 2.0), (l - dx, t), (l - dx, y2), (dx, y2), (dx, t)];
    let pts = orient(if vertical { Dir::Down } else { Dir::Right }, w, h, &pts);
    let r = if vertical { Rect::new(y1, dx, y2, h - dx) } else { Rect::new(dx, y1, w - dx, y2) };
    Geometry { paths: vec![fill(Pb::new().poly(&pts).take())], text_rect: r, handles: vec![], sites: mid_sites(w, h) }
}

/// Points given in 21600ths of the box (as ECMA-376 defines many fixed shapes), scaled to it.
pub(super) fn spec_pts(pts: &[(i32, i32)], w: f64, h: f64) -> Vec<(f64, f64)> {
    pts.iter().map(|(x, y)| (w * *x as f64 / 21600.0, h * *y as f64 / 21600.0)).collect()
}

pub(super) fn simple(path: BezPath, text: Rect, w: f64, h: f64) -> Geometry {
    Geometry { paths: vec![fill(path)], text_rect: text, handles: vec![], sites: mid_sites(w, h) }
}

pub(super) fn ellipse_sites(w: f64, h: f64) -> Vec<Point> {
    (0..8)
        .map(|i| {
            let a = (-90.0 - 45.0 * i as f64).to_radians();
            Point::new(w / 2.0 + w / 2.0 * a.cos(), h / 2.0 + h / 2.0 * a.sin())
        })
        .collect()
}

pub(super) fn tip_handle(w: f64, h: f64, tip: Point) -> Handle {
    // A free 2-D handle is represented as index 0 with zero direction; callers move both values.
    Handle { index: 0, pos: tip, origin: Point::new(w / 2.0, h / 2.0), dir: Vec2::new(w / 1e5, h / 1e5), min: -1e6, max: 1e6, angle: false }
}
