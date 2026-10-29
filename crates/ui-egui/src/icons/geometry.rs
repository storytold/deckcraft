use super::{
    Align2, BLUE, Color32, DARK, FontId, GOLD, GREEN, Mesh, ORANGE, PI, PURPLE, Painter, PathShape, Pos2, RED, Rect, Shape, Stroke, StrokeKind,
    TextShape, pos2, vec2,
};

// ---------------------------------------------------------------------------------------------
// Drawing toolkit
// ---------------------------------------------------------------------------------------------

/// A logical colour, resolved against the theme ink and the disabled flag.
#[derive(Clone, Copy)]
pub(super) enum C {
    Ink(f32),
    Acc(Color32, f32),
    White,
    None,
}

impl C {
    pub(super) fn a(self, f: f32) -> C {
        match self {
            C::Ink(a) => C::Ink(a * f),
            C::Acc(c, a) => C::Acc(c, a * f),
            other => other,
        }
    }
}

pub(super) const K: C = C::Ink(1.0);

pub(super) const B: C = C::Acc(BLUE, 1.0);

pub(super) const O: C = C::Acc(ORANGE, 1.0);

pub(super) const G: C = C::Acc(GREEN, 1.0);

pub(super) const PU: C = C::Acc(PURPLE, 1.0);

pub(super) const R: C = C::Acc(RED, 1.0);

pub(super) const Y: C = C::Acc(GOLD, 1.0);

pub(super) const D: C = C::Acc(DARK, 1.0);

pub(super) const W: C = C::White;

pub(super) const N: C = C::None;

/// Light tint of an accent, for fills behind outlines.
pub(super) fn t(c: C) -> C {
    c.a(0.25)
}

pub(super) type P2 = (f32, f32);
/// A box as `(x0, y0, x1, y1)`.
pub(super) type Box4 = (f32, f32, f32, f32);

#[derive(Clone, Copy)]
pub(super) enum Glyph {
    Plus,
    Cross,
    Check,
}

pub(super) struct Pen<'a> {
    pub(super) p: &'a Painter,
    pub(super) o: Pos2,
    /// Points per design unit.
    pub(super) k: f32,
    /// Base stroke width in points.
    pub(super) sw: f32,
    pub(super) ink: Color32,
    pub(super) dis: bool,
}

pub(super) fn rad(deg: f32) -> f32 {
    deg * PI / 180.0
}

pub(super) fn arc_pts(c: P2, rx: f32, ry: f32, a0: f32, a1: f32) -> Vec<P2> {
    let n = (((a1 - a0).abs() / 9.0).ceil() as usize).max(4);
    (0..=n)
        .map(|i| {
            let a = rad(a0 + (a1 - a0) * i as f32 / n as f32);
            (c.0 + rx * a.cos(), c.1 + ry * a.sin())
        })
        .collect()
}

pub(super) fn cubic_pts(p0: P2, p1: P2, p2: P2, p3: P2, n: usize) -> Vec<P2> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            (a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1)
        })
        .collect()
}

pub(super) fn quad_pts(p0: P2, p1: P2, p2: P2, n: usize) -> Vec<P2> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            (u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0, u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1)
        })
        .collect()
}

/// Rounds the corners of a polyline by `r` design units.
pub(super) fn round_pts(pts: &[P2], r: f32, closed: bool) -> Vec<P2> {
    let n = pts.len();
    if n < 3 || r <= 0.0 {
        return pts.to_vec();
    }
    let mut out = Vec::with_capacity(n * 6);
    for i in 0..n {
        let cur = pts[i];
        let interior = closed || (i > 0 && i + 1 < n);
        if !interior {
            out.push(cur);
            continue;
        }
        let prev = pts[(i + n - 1) % n];
        let next = pts[(i + 1) % n];
        let d1 = ((prev.0 - cur.0).powi(2) + (prev.1 - cur.1).powi(2)).sqrt();
        let d2 = ((next.0 - cur.0).powi(2) + (next.1 - cur.1).powi(2)).sqrt();
        if d1 < 1e-4 || d2 < 1e-4 {
            out.push(cur);
            continue;
        }
        let rr = r.min(d1 * 0.5).min(d2 * 0.5);
        let a = (cur.0 + (prev.0 - cur.0) / d1 * rr, cur.1 + (prev.1 - cur.1) / d1 * rr);
        let b = (cur.0 + (next.0 - cur.0) / d2 * rr, cur.1 + (next.1 - cur.1) / d2 * rr);
        out.extend(quad_pts(a, cur, b, 4));
    }
    out
}

pub(super) fn rrect_pts(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> Vec<P2> {
    round_pts(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)], r, true)
}

/// Star polygon with `n` points.
pub(super) fn star_pts(c: P2, ro: f32, ri: f32, n: usize, rot: f32) -> Vec<P2> {
    (0..n * 2)
        .map(|i| {
            let r = if i % 2 == 0 { ro } else { ri };
            let a = rad(rot - 90.0 + i as f32 * 180.0 / n as f32);
            (c.0 + r * a.cos(), c.1 + r * a.sin())
        })
        .collect()
}

/// Transforms local points (u along `ang`, v across) to design coordinates around `c`.
pub(super) fn tf(c: P2, ang: f32, pts: &[P2]) -> Vec<P2> {
    let (s, co) = rad(ang).sin_cos();
    pts.iter().map(|&(u, v)| (c.0 + u * co - v * s, c.1 + u * s + v * co)).collect()
}

fn point_in_tri(p: Pos2, a: Pos2, b: Pos2, c: Pos2) -> bool {
    let d1 = (p.x - b.x) * (a.y - b.y) - (a.x - b.x) * (p.y - b.y);
    let d2 = (p.x - c.x) * (b.y - c.y) - (b.x - c.x) * (p.y - c.y);
    let d3 = (p.x - a.x) * (c.y - a.y) - (c.x - a.x) * (p.y - a.y);
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Ear-clipping triangulation of a simple polygon.
pub(super) fn triangulate(pts: &[Pos2]) -> Vec<[u32; 3]> {
    let n = pts.len();
    let mut tris = Vec::new();
    if n < 3 {
        return tris;
    }
    let area: f32 = (0..n)
        .map(|i| {
            let a = pts[i];
            let b = pts[(i + 1) % n];
            a.x * b.y - b.x * a.y
        })
        .sum();
    let mut idx: Vec<usize> = (0..n).collect();
    if area < 0.0 {
        idx.reverse();
    }
    while idx.len() > 3 {
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
            let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
            if cross <= 1e-7 {
                continue;
            }
            let blocked = idx.iter().any(|&j| j != ia && j != ib && j != ic && point_in_tri(pts[j], a, b, c));
            if blocked {
                continue;
            }
            tris.push([ia as u32, ib as u32, ic as u32]);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            // Degenerate input: drop a vertex and carry on rather than loop forever.
            idx.remove(0);
        }
    }
    if let [a, b, c] = idx[..] {
        tris.push([a as u32, b as u32, c as u32]);
    }
    tris
}

impl Pen<'_> {
    fn pt(&self, (x, y): P2) -> Pos2 {
        pos2(self.o.x + x * self.k, self.o.y + y * self.k)
    }

    fn v(&self, pts: &[P2]) -> Vec<Pos2> {
        pts.iter().map(|&p| self.pt(p)).collect()
    }

    fn col(&self, c: C) -> Color32 {
        let dim = if self.dis { 0.35 } else { 1.0 };
        match c {
            C::Ink(a) => self.ink.gamma_multiply(a * dim),
            C::Acc(col, a) => {
                if self.dis {
                    self.ink.gamma_multiply(a * dim)
                } else {
                    col.gamma_multiply(a)
                }
            }
            C::White => {
                if self.dis {
                    // Glyphs on solid fills stay legible as a darker mark.
                    self.ink.gamma_multiply(0.6)
                } else {
                    Color32::WHITE
                }
            }
            C::None => Color32::TRANSPARENT,
        }
    }

    fn stroke(&self, c: C, w: f32) -> Stroke {
        Stroke::new(self.sw * w, self.col(c))
    }

    // --- lines ---

    pub(super) fn line_w(&self, pts: &[P2], c: C, w: f32) {
        if pts.len() < 2 {
            return;
        }
        let col = self.col(c);
        if col == Color32::TRANSPARENT {
            return;
        }
        let v = self.v(pts);
        if let [a, b] = v[..] {
            // Single segments get round caps: draw them as one convex capsule.
            let d = b - a;
            let len = d.length();
            let r = self.sw * w * 0.5;
            if len < 1e-3 {
                self.p.circle_filled(a, r, col);
                return;
            }
            let ang = d.y.atan2(d.x);
            let mut cap = Vec::with_capacity(20);
            for (c, start) in [(b, ang - PI * 0.5), (a, ang + PI * 0.5)] {
                for i in 0..=8 {
                    let t = start + PI * i as f32 / 8.0;
                    cap.push(c + vec2(t.cos(), t.sin()) * r);
                }
            }
            self.p.add(PathShape::convex_polygon(cap, col, Stroke::NONE));
            return;
        }
        self.p.add(PathShape::line(v, Stroke::new(self.sw * w, col)));
    }

    pub(super) fn line(&self, pts: &[P2], c: C) {
        self.line_w(pts, c, 1.0);
    }

    pub(super) fn seg(&self, a: P2, b: P2, c: C) {
        self.line_w(&[a, b], c, 1.0);
    }

    pub(super) fn seg_w(&self, a: P2, b: P2, c: C, w: f32) {
        self.line_w(&[a, b], c, w);
    }

    pub(super) fn rline(&self, pts: &[P2], r: f32, c: C) {
        self.line(&round_pts(pts, r, false), c);
    }

    pub(super) fn rline_w(&self, pts: &[P2], r: f32, c: C, w: f32) {
        self.line_w(&round_pts(pts, r, false), c, w);
    }

    pub(super) fn dashed(&self, pts: &[P2], c: C, dash: f32, gap: f32) {
        let v = self.v(pts);
        self.p.extend(Shape::dashed_line(&v, self.stroke(c, 1.0), dash * self.k, gap * self.k));
    }

    pub(super) fn dashed_w(&self, pts: &[P2], c: C, dash: f32, gap: f32, w: f32) {
        let v = self.v(pts);
        self.p.extend(Shape::dashed_line(&v, self.stroke(c, w), dash * self.k, gap * self.k));
    }

    pub(super) fn dotted(&self, pts: &[P2], c: C, spacing: f32, r: f32) {
        let v = self.v(pts);
        self.p.extend(Shape::dotted_line(&v, self.col(c), spacing * self.k, r * self.sw));
    }

    // --- filled shapes ---

    /// Any simple polygon: filled via triangulation, stroked as a closed path.
    pub(super) fn shape_w(&self, pts: &[P2], fill: C, stroke: C, w: f32) {
        if pts.len() < 3 {
            return;
        }
        let v = self.v(pts);
        let fc = self.col(fill);
        if fc != Color32::TRANSPARENT {
            let mut mesh = Mesh::default();
            for &p in &v {
                mesh.colored_vertex(p, fc);
            }
            for [a, b, c] in triangulate(&v) {
                mesh.add_triangle(a, b, c);
            }
            self.p.add(Shape::mesh(mesh));
            // A hairline in the fill colour anti-aliases the mesh edge.
            if matches!(stroke, C::None) {
                self.p.add(PathShape::closed_line(v.clone(), Stroke::new(0.6_f32.min(self.sw * 0.5), fc)));
            }
        }
        let sc = self.col(stroke);
        if sc != Color32::TRANSPARENT {
            self.p.add(PathShape::closed_line(v, Stroke::new(self.sw * w, sc)));
        }
    }

    pub(super) fn shape(&self, pts: &[P2], fill: C, stroke: C) {
        self.shape_w(pts, fill, stroke, 1.0);
    }

    pub(super) fn rshape(&self, pts: &[P2], r: f32, fill: C, stroke: C) {
        self.shape(&round_pts(pts, r, true), fill, stroke);
    }

    /// Convex polygon (anti-aliased fill).
    pub(super) fn poly(&self, pts: &[P2], fill: C, stroke: C) {
        self.poly_w(pts, fill, stroke, 1.0);
    }

    pub(super) fn poly_w(&self, pts: &[P2], fill: C, stroke: C, w: f32) {
        let v = self.v(pts);
        self.p.add(PathShape::convex_polygon(v, self.col(fill), self.stroke(stroke, w)));
    }

    fn rect_w(&self, (x0, y0, x1, y1): Box4, r: f32, fill: C, stroke: C, w: f32) {
        let rect = Rect::from_min_max(self.pt((x0, y0)), self.pt((x1, y1)));
        self.p.rect(rect, r * self.k, self.col(fill), self.stroke(stroke, w), StrokeKind::Middle);
    }

    pub(super) fn rect(&self, b: Box4, r: f32, fill: C, stroke: C) {
        self.rect_w(b, r, fill, stroke, 1.0);
    }

    fn circle_w(&self, c: P2, r: f32, fill: C, stroke: C, w: f32) {
        self.p.circle(self.pt(c), r * self.k, self.col(fill), self.stroke(stroke, w));
    }

    pub(super) fn circle(&self, c: P2, r: f32, fill: C, stroke: C) {
        self.circle_w(c, r, fill, stroke, 1.0);
    }

    pub(super) fn dot(&self, c: P2, r: f32, fill: C) {
        self.p.circle_filled(self.pt(c), r * self.k, self.col(fill));
    }

    pub(super) fn ellipse(&self, c: P2, rx: f32, ry: f32, fill: C, stroke: C) {
        let mut pts = arc_pts(c, rx, ry, 0.0, 360.0);
        pts.pop();
        self.poly(&pts, fill, stroke);
    }

    // --- arcs and arrows ---

    pub(super) fn arc(&self, c: P2, r: f32, a0: f32, a1: f32, col: C) {
        self.line(&arc_pts(c, r, r, a0, a1), col);
    }

    /// Filled arrow head with its tip at `tip`, pointing along `dir` degrees.
    pub(super) fn head(&self, tip: P2, dir: f32, size: f32, c: C) {
        let (s, co) = rad(dir).sin_cos();
        let hw = size * 0.62;
        let back = (tip.0 - co * size, tip.1 - s * size);
        self.poly(&[tip, (back.0 - s * hw, back.1 + co * hw), (back.0 + s * hw, back.1 - co * hw)], c, N);
    }

    pub(super) fn head_size(&self) -> f32 {
        (2.2 * self.sw / self.k).max(3.6)
    }

    pub(super) fn arrow(&self, a: P2, b: P2, c: C) {
        let dir = (b.1 - a.1).atan2(b.0 - a.0);
        let hs = self.head_size();
        let end = (b.0 - dir.cos() * hs * 0.7, b.1 - dir.sin() * hs * 0.7);
        self.seg(a, end, c);
        self.head(b, dir.to_degrees(), hs, c);
    }

    pub(super) fn arrow2(&self, a: P2, b: P2, c: C) {
        let dir = (b.1 - a.1).atan2(b.0 - a.0);
        let hs = self.head_size();
        let s = (a.0 + dir.cos() * hs * 0.7, a.1 + dir.sin() * hs * 0.7);
        let e = (b.0 - dir.cos() * hs * 0.7, b.1 - dir.sin() * hs * 0.7);
        self.seg(s, e, c);
        self.head(b, dir.to_degrees(), hs, c);
        self.head(a, dir.to_degrees() + 180.0, hs, c);
    }

    /// Polyline ending in an arrow head.
    pub(super) fn path_arrow(&self, pts: &[P2], c: C) {
        let n = pts.len();
        if n < 2 {
            return;
        }
        let a = pts[n - 2];
        let b = pts[n - 1];
        let dir = (b.1 - a.1).atan2(b.0 - a.0);
        let hs = self.head_size();
        let mut body = pts.to_vec();
        body[n - 1] = (b.0 - dir.cos() * hs * 0.7, b.1 - dir.sin() * hs * 0.7);
        self.line(&body, c);
        self.head(b, dir.to_degrees(), hs, c);
    }

    /// Circular arc from `a0` to `a1` degrees with an arrow head at the `a1` end.
    pub(super) fn arc_arrow(&self, c: P2, r: f32, a0: f32, a1: f32, col: C) {
        let hs = self.head_size();
        let sign = if a1 >= a0 { 1.0 } else { -1.0 };
        let back = (hs * 0.6 / r).to_degrees() * sign;
        self.line(&arc_pts(c, r, r, a0, a1 - back), col);
        let tip_a = rad(a1);
        let tip = (c.0 + r * tip_a.cos(), c.1 + r * tip_a.sin());
        // Tangent direction of travel, nudged inward to sit on the arc.
        let dir = a1 + 90.0 * sign - (hs * 0.5 / r).to_degrees() * sign;
        self.head(tip, dir, hs, col);
    }

    pub(super) fn curve(&self, p0: P2, p1: P2, p2: P2, p3: P2, c: C) {
        self.line(&cubic_pts(p0, p1, p2, p3, 24), c);
    }

    pub(super) fn curve_w(&self, p0: P2, p1: P2, p2: P2, p3: P2, c: C, w: f32) {
        self.line_w(&cubic_pts(p0, p1, p2, p3, 24), c, w);
    }

    // --- text ---

    pub(super) fn text(&self, s: &str, at: P2, size: f32, c: C) {
        self.p.text(self.pt(at), Align2::CENTER_CENTER, s, FontId::proportional(size * self.k), self.col(c));
    }

    pub(super) fn text_bold(&self, s: &str, at: P2, size: f32, c: C) {
        let d = 0.32 * self.sw / self.k;
        let reps: &[f32] = if self.dis { &[0.0] } else { &[-d, 0.0, d] };
        for &dx in reps {
            self.text(s, (at.0 + dx, at.1), size, c);
        }
    }

    pub(super) fn text_rot(&self, s: &str, at: P2, size: f32, c: C, angle_deg: f32) {
        let col = self.col(c);
        let galley = self.p.layout_no_wrap(s.to_owned(), FontId::proportional(size * self.k), col);
        let rot = egui::emath::Rot2::from_angle(rad(angle_deg));
        let pos = self.pt(at) - rot * (galley.size() * 0.5);
        self.p.add(TextShape::new(pos, galley, col).with_angle(rad(angle_deg)));
    }

    // --- composite parts ---

    /// Round badge with a white glyph (outline + ink glyph when disabled).
    pub(super) fn badge(&self, c: P2, r: f32, fill: C, glyph: Glyph) {
        let gc = if self.dis {
            self.circle(c, r, N, fill);
            K
        } else {
            self.circle(c, r, fill, N);
            W
        };
        let g = r * 0.5;
        let w = 0.95;
        match glyph {
            Glyph::Plus => {
                self.seg_w((c.0 - g, c.1), (c.0 + g, c.1), gc, w);
                self.seg_w((c.0, c.1 - g), (c.0, c.1 + g), gc, w);
            }
            Glyph::Cross => {
                let g = g * 0.85;
                self.seg_w((c.0 - g, c.1 - g), (c.0 + g, c.1 + g), gc, w);
                self.seg_w((c.0 + g, c.1 - g), (c.0 - g, c.1 + g), gc, w);
            }
            Glyph::Check => {
                self.line_w(&[(c.0 - g, c.1 + 0.1), (c.0 - g * 0.25, c.1 + g * 0.75), (c.0 + g, c.1 - g * 0.6)], gc, w);
            }
        }
    }

    /// Landscape slide frame.
    pub(super) fn frame(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.rect((x0, y0, x1, y1), 1.5, N, K);
    }

    /// Document page with a folded top-right corner.
    pub(super) fn page(&self, x0: f32, y0: f32, x1: f32, y1: f32, fill: C, stroke: C) {
        let f = ((x1 - x0) * 0.32).min(5.0);
        self.rshape(&[(x0, y0), (x1 - f, y0), (x1, y0 + f), (x1, y1), (x0, y1)], 1.2, fill, stroke);
        self.line(&[(x1 - f, y0), (x1 - f, y0 + f), (x1, y0 + f)], stroke);
    }

    /// Picture frame with sun and hills.
    pub(super) fn picture(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let (w, h) = (x1 - x0, y1 - y0);
        let m = w.min(h);
        self.rect((x0, y0, x1, y1), 1.5, N, K);
        self.dot((x1 - w * 0.27, y0 + h * 0.3), m * 0.12, Y);
        let base = y1 - h * 0.14;
        self.poly(&[(x0 + w * 0.12, base), (x0 + w * 0.38, y0 + h * 0.36), (x0 + w * 0.64, base)], G, N);
        self.poly(&[(x0 + w * 0.48, base), (x0 + w * 0.68, y0 + h * 0.56), (x1 - w * 0.12, base)], G.a(0.7), N);
    }

    pub(super) fn magnifier(&self, c: P2, r: f32, lens_fill: C, col: C) {
        self.circle(c, r, lens_fill, col);
        let d = r * 0.707;
        self.seg_w((c.0 + d + 0.6, c.1 + d + 0.6), (c.0 + r * 1.55, c.1 + r * 1.55), col, 1.5);
    }

    fn bubble_pts(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<P2> {
        let r = 2.5;
        let mut pts = arc_pts((x0 + r, y0 + r), r, r, 180.0, 270.0);
        pts.extend(arc_pts((x1 - r, y0 + r), r, r, 270.0, 360.0));
        pts.extend(arc_pts((x1 - r, y1 - r), r, r, 0.0, 90.0));
        let tw = ((x1 - x0) * 0.22).clamp(2.5, 4.5);
        pts.push((x0 + r + 0.8 + tw, y1));
        pts.push((x0 + r - 0.2, y1 + 3.8));
        pts.push((x0 + r + 0.8, y1));
        pts.extend(arc_pts((x0 + r, y1 - r), r, r, 90.0, 180.0));
        pts
    }

    pub(super) fn bubble(&self, x0: f32, y0: f32, x1: f32, y1: f32, fill: C, stroke: C) {
        self.shape(&Self::bubble_pts(x0, y0, x1, y1), fill, stroke);
    }

    pub(super) fn person(&self, cx: f32, top: f32, s: f32, fill: C) {
        self.dot((cx, top + 2.6 * s), 2.6 * s, fill);
        let base = top + 12.0 * s;
        self.poly(&arc_pts((cx, base), 5.2 * s, 5.0 * s, 180.0, 360.0), fill, N);
    }

    pub(super) fn star(&self, c: P2, ro: f32, fill: C, stroke: C) {
        self.shape(&star_pts(c, ro, ro * 0.47, 5, 0.0), fill, stroke);
    }

    pub(super) fn sparkle(&self, c: P2, r: f32, fill: C) {
        self.shape(&star_pts(c, r, r * 0.3, 4, 0.0), fill, N);
    }

    pub(super) fn gear(&self, c: P2, ro: f32, ri: f32, teeth: usize, fill: C, stroke: C) {
        let step = 360.0 / teeth as f32;
        let mut pts = Vec::with_capacity(teeth * 4);
        for i in 0..teeth {
            let base = i as f32 * step;
            for (a, r) in [(-0.3, ri), (-0.17, ro), (0.17, ro), (0.3, ri)] {
                let ang = rad(base + a * step);
                pts.push((c.0 + r * ang.cos(), c.1 + r * ang.sin()));
            }
        }
        self.shape(&pts, fill, stroke);
    }

    pub(super) fn speaker(&self, x: f32, cy: f32, fill: C, stroke: C) {
        self.rshape(
            &[(x, cy - 3.0), (x + 3.8, cy - 3.0), (x + 8.5, cy - 7.5), (x + 8.5, cy + 7.5), (x + 3.8, cy + 3.0), (x, cy + 3.0)],
            0.8,
            fill,
            stroke,
        );
    }

    /// Table grid; `header` fills the first row.
    pub(super) fn table(&self, (x0, y0, x1, y1): Box4, rows: usize, cols: usize, header: C) {
        let rh = (y1 - y0) / rows as f32;
        let cw = (x1 - x0) / cols as f32;
        if !matches!(header, C::None) {
            self.rect((x0, y0, x1, y0 + rh), 1.2, header, N);
        }
        for i in 1..rows {
            let y = y0 + rh * i as f32;
            self.seg_w((x0, y), (x1, y), K, 0.8);
        }
        for j in 1..cols {
            let x = x0 + cw * j as f32;
            self.seg_w((x, y0), (x, y1), K, 0.8);
        }
        self.rect((x0, y0, x1, y1), 1.2, N, K);
    }

    /// Bar chart in the box.
    pub(super) fn bars(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let w = (x1 - x0) / 3.0;
        let h = y1 - y0;
        for (i, (frac, c)) in [(0.5, B), (1.0, O), (0.72, G)].into_iter().enumerate() {
            let bx = x0 + w * i as f32;
            self.rect((bx + w * 0.16, y1 - h * frac, bx + w * 0.84, y1), 0.6, c, N);
        }
        self.seg((x0 - 0.5, y1 + 0.2), (x1 + 0.5, y1 + 0.2), K);
    }

    /// A writing tool lying along `ang` degrees with its point at `tip`.
    pub(super) fn tool(&self, (tip, ang): (P2, f32), (len, hw, cone): (f32, f32, f32), body: C, lead: C, (cap_len, cap): (f32, C)) {
        let m = |pts: &[P2]| tf(tip, ang, pts);
        self.poly(&m(&[(0.0, 0.0), (cone, -hw), (cone, hw)]), N, K);
        self.poly(&m(&[(0.0, 0.0), (cone * 0.42, -hw * 0.42), (cone * 0.42, hw * 0.42)]), lead, N);
        self.poly(&m(&[(cone, -hw), (len - cap_len, -hw), (len - cap_len, hw), (cone, hw)]), body, K);
        if cap_len > 0.0 {
            self.poly(&m(&[(len - cap_len, -hw), (len, -hw), (len, hw), (len - cap_len, hw)]), cap, K);
        }
    }

    /// Paint brush lying along `ang` with bristle tip at `tip`.
    pub(super) fn brush(&self, tip: P2, ang: f32, len: f32, bristle: C) {
        let m = |pts: &[P2]| tf(tip, ang, pts);
        self.shape(&m(&[(0.0, -0.4), (2.5, -2.2), (5.5, -2.2), (5.5, 2.2), (2.5, 2.2), (0.0, 0.4)]), bristle, K);
        self.poly(&m(&[(5.5, -2.0), (7.8, -2.0), (7.8, 2.0), (5.5, 2.0)]), K, K);
        self.line(&m(&[(7.8, -1.1), (len, -1.3), (len, 1.3), (7.8, 1.1)]), K);
    }

    /// Small path-drawn digit 1–3 in a `3 × h` box.
    pub(super) fn digit(&self, d: u8, x: f32, y: f32, h: f32, c: C) {
        let w = 0.8;
        match d {
            1 => self.line_w(&[(x + 0.4, y + 1.2), (x + 2.0, y), (x + 2.0, y + h)], c, w),
            2 => self.line_w(&[(x, y + 1.0), (x + 0.7, y), (x + 2.4, y), (x + 3.1, y + 1.1), (x + 2.9, y + 2.2), (x, y + h), (x + 3.2, y + h)], c, w),
            _ => self.line_w(
                &[(x, y), (x + 3.0, y), (x + 1.3, y + h * 0.42), (x + 2.6, y + h * 0.5), (x + 3.1, y + h * 0.7), (x + 2.5, y + h), (x, y + h)],
                c,
                w,
            ),
        }
    }

    /// Path-drawn capital A.
    pub(super) fn letter_a(&self, x0: f32, top: f32, x1: f32, bottom: f32, c: C, w: f32) {
        let mx = (x0 + x1) * 0.5;
        self.line_w(&[(x0, bottom), (mx, top), (x1, bottom)], c, w);
        let f = 0.62;
        let y = top + (bottom - top) * f;
        let dx = (x1 - x0) * 0.5 * f;
        self.seg_w((mx - dx, y), (mx + dx, y), c, w);
    }

    /// Hand-drawn scribble in the box.
    pub(super) fn scribble(&self, x0: f32, y0: f32, x1: f32, y1: f32, c: C) {
        let (w, h) = (x1 - x0, y1 - y0);
        let p = |u: f32, v: f32| (x0 + w * u, y0 + h * v);
        // A loopy cursive stroke (prolate cycloid), normalised into the box.
        let raw: Vec<P2> = (0..=48)
            .map(|i| {
                let th = i as f32 / 48.0 * 4.0 * PI + 0.6;
                (th - 1.9 * th.sin(), -1.9 * th.cos())
            })
            .collect();
        let (mut lx, mut hx, mut ly, mut hy) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for &(x, y) in &raw {
            lx = lx.min(x);
            hx = hx.max(x);
            ly = ly.min(y);
            hy = hy.max(y);
        }
        let sx = (hx - lx).max(1e-3);
        let sy = (hy - ly).max(1e-3);
        let pts: Vec<P2> = raw.iter().map(|&(x, y)| p((x - lx) / sx, (y - ly) / sy)).collect();
        self.line(&pts, c);
    }

    /// Isometric "layer" diamond.
    pub(super) fn layer(&self, cx: f32, cy: f32, hw: f32, hh: f32, fill: C, stroke: C) {
        self.rshape(&[(cx, cy - hh), (cx + hw, cy), (cx, cy + hh), (cx - hw, cy)], 0.8, fill, stroke);
    }

    /// Open book.
    pub(super) fn book(&self, fill: C, col: C) {
        let left = {
            let mut p = quad_pts((12.0, 7.0), (7.5, 4.0), (2.0, 5.0), 8);
            p.extend(quad_pts((2.0, 18.5), (7.5, 17.5), (12.0, 20.5), 8));
            p
        };
        let right = {
            let mut p = quad_pts((12.0, 7.0), (16.5, 4.0), (22.0, 5.0), 8);
            p.extend(quad_pts((22.0, 18.5), (16.5, 17.5), (12.0, 20.5), 8));
            p
        };
        self.shape(&left, fill, col);
        self.shape(&right, fill, col);
    }

    pub(super) fn monitor(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.rect((x0, y0, x1, y1), 1.5, N, K);
        let cx = (x0 + x1) * 0.5;
        self.seg((cx, y1), (cx, y1 + 3.2), K);
        self.seg((cx - 4.0, y1 + 3.6), (cx + 4.0, y1 + 3.6), K);
    }

    pub(super) fn clock_badge(&self, c: P2, r: f32, fill: C) {
        let hc = if self.dis {
            self.circle(c, r, N, fill);
            K
        } else {
            self.circle(c, r, fill, N);
            W
        };
        self.line_w(&[(c.0, c.1 - r * 0.6), c, (c.0 + r * 0.5, c.1 + r * 0.2)], hc, 0.9);
    }

    pub(super) fn gear_badge(&self, c: P2, r: f32, fill: C) {
        self.gear(c, r, r * 0.74, 8, fill, N);
        if self.dis {
            self.gear(c, r, r * 0.74, 8, N, fill);
        }
        self.dot(c, r * 0.32, if self.dis { N } else { W });
        if self.dis {
            self.circle(c, r * 0.32, N, fill);
        }
    }

    pub(super) fn lock(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let cx = (x0 + x1) * 0.5;
        let r = (x1 - x0) * 0.3;
        let mut sh = vec![(cx - r, y0)];
        sh.extend(arc_pts((cx, y0 - r * 0.7), r, r, 180.0, 360.0));
        sh.push((cx + r, y0));
        self.line(&sh, K);
        self.rect((x0, y0, x1, y1), 1.2, Y, K);
        let ky = (y0 + y1) * 0.5;
        self.dot((cx, ky - 0.4), 1.1, D);
        self.seg_w((cx, ky), (cx, ky + 2.0), D, 0.9);
    }

    pub(super) fn cursor(&self, x: f32, y: f32, s: f32) {
        let p = |u: f32, v: f32| (x + u * s, y + v * s);
        let pts = [p(0.0, 0.0), p(0.0, 15.0), p(3.8, 11.6), p(6.4, 17.0), p(8.6, 16.0), p(6.1, 10.8), p(11.0, 10.8)];
        self.shape(&pts, K, K);
    }
}
