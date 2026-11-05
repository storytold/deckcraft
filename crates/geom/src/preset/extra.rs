//! More preset outlines: line callouts, arrow callouts, curved and circular arrows, curved
//! ribbons, gears, tabs and chart markers. Like the rest of `preset`, every outline is our own
//! construction, fitted to how the shapes look and to what their adjust values mean.

use super::{
    Corner, Dir, FillMode, Geometry, Handle, Pb, Point, Rect, Vec2, corner_rect, fill, fill_eo, fill_only, mid_sites, orient, shade, simple,
    stroke_only, vis,
};

/// Builds the presets this module knows; `None` for any other name.
pub(super) fn build(name: &str, w: f64, h: f64, a: &dyn Fn(usize) -> f64) -> Option<Geometry> {
    let ss = w.min(h);
    let full = Rect::new(0.0, 0.0, w, h);
    let mut b = Pb::new();
    let g = match name {
        "lineInv" => {
            let l = b.m(0.0, h).l(w, 0.0).take();
            Geometry { paths: vec![stroke_only(l)], text_rect: full, handles: vec![], sites: vec![Point::new(0.0, h), Point::new(w, 0.0)] }
        }
        "nonIsoscelesTrapezoid" => {
            let max = 50000.0 * w / ss.max(1e-9);
            let (v1, v2) = (a(0).clamp(0.0, max), a(1).clamp(0.0, max));
            let (d1, d2) = (ss * v1 / 1e5, ss * v2 / 1e5);
            let mut g = simple(b.poly(&[(0.0, h), (d1, 0.0), (w - d2, 0.0), (w, h)]).take(), Rect::new(d1 * 0.66, h * 0.2, w - d2 * 0.66, h), w, h);
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 1e5, 0.0), v1, 0.0, max));
            g.handles.push(Handle::new(1, Point::new(w, 0.0), Vec2::new(-ss / 1e5, 0.0), v2, 0.0, max));
            g
        }
        "pieWedge" => {
            b.m(0.0, h).arc(w, h, 180.0, 90.0).l(w, h).z();
            simple(b.take(), Rect::new(w * 0.29289, h * 0.29289, w, h), w, h)
        }
        "flowChartOfflineStorage" => {
            let tri = b.poly(&[(0.0, 0.0), (w, 0.0), (w / 2.0, h)]).take();
            let bar = Pb::new().m(w * 0.4, h * 0.8).l(w * 0.6, h * 0.8).take();
            Geometry {
                paths: vec![fill(tri), stroke_only(bar)],
                text_rect: Rect::new(w / 4.0, 0.0, w * 0.75, h * 0.8),
                handles: vec![],
                sites: vec![Point::new(w / 2.0, 0.0), Point::new(w / 4.0, h / 2.0), Point::new(w / 2.0, h), Point::new(w * 0.75, h / 2.0)],
            }
        }
        "chartX" | "chartStar" | "chartPlus" => {
            let body = corner_rect(w, h, [Corner::Square; 4]);
            let mut l = Pb::new();
            if name != "chartPlus" {
                l.m(0.0, 0.0).l(w, h).m(0.0, h).l(w, 0.0);
            }
            if name != "chartX" {
                l.m(w / 2.0, 0.0).l(w / 2.0, h);
            }
            if name == "chartPlus" {
                l.m(0.0, h / 2.0).l(w, h / 2.0);
            }
            Geometry { paths: vec![fill_only(body), stroke_only(l.take())], text_rect: full, handles: vec![], sites: mid_sites(w, h) }
        }
        "cornerTabs" | "squareTabs" | "plaqueTabs" => {
            // Each tab's size is a twentieth of the diagonal.
            let d = (w.hypot(h) / 20.0).min(w / 2.0).min(h / 2.0);
            match name {
                "cornerTabs" => {
                    b.poly(&[(0.0, 0.0), (d, 0.0), (0.0, d)]);
                    b.poly(&[(w - d, 0.0), (w, 0.0), (w, d)]);
                    b.poly(&[(w, h - d), (w, h), (w - d, h)]);
                    b.poly(&[(0.0, h - d), (d, h), (0.0, h)]);
                }
                "squareTabs" => {
                    b.poly(&[(0.0, 0.0), (d, 0.0), (d, d), (0.0, d)]);
                    b.poly(&[(w - d, 0.0), (w, 0.0), (w, d), (w - d, d)]);
                    b.poly(&[(w - d, h - d), (w, h - d), (w, h), (w - d, h)]);
                    b.poly(&[(0.0, h - d), (d, h - d), (d, h), (0.0, h)]);
                }
                _ => {
                    if d > 0.0 {
                        b.m(0.0, 0.0).l(d, 0.0).arc(d, d, 0.0, 90.0).z();
                        b.m(w, 0.0).l(w, d).arc(d, d, 90.0, 90.0).z();
                        b.m(w, h).l(w - d, h).arc(d, d, 180.0, 90.0).z();
                        b.m(0.0, h).l(0.0, h - d).arc(d, d, 270.0, 90.0).z();
                    } else {
                        b.m(0.0, 0.0).z();
                    }
                }
            }
            Geometry { paths: vec![fill(b.take())], text_rect: full.inset(-d), handles: vec![], sites: mid_sites(w, h) }
        }
        "callout1"
        | "callout2"
        | "callout3"
        | "accentCallout1"
        | "accentCallout2"
        | "accentCallout3"
        | "borderCallout2"
        | "borderCallout3"
        | "accentBorderCallout1"
        | "accentBorderCallout2"
        | "accentBorderCallout3" => {
            let n = match name.chars().last() {
                Some('2') => 2,
                Some('3') => 3,
                _ => 1,
            };
            line_callout(w, h, a, n + 1, name.starts_with("accent"), name.contains("order"))
        }
        "leftRightArrowCallout" | "upDownArrowCallout" => {
            let vertical = name == "upDownArrowCallout";
            let (l, t) = if vertical { (h, w) } else { (w, h) };
            let ss = l.min(t);
            let head = (ss * a(1).max(0.0) / 1e5).min(t / 2.0);
            let shaft = (ss * a(0).max(0.0) / 1e5).min(head * 2.0);
            let hl = (ss * a(2).max(0.0) / 1e5).min(l / 2.0);
            let boxl = (l * a(3).max(0.0) / 1e5).min(l - 2.0 * hl);
            let (c, b0, b1) = (t / 2.0, (l - boxl) / 2.0, (l + boxl) / 2.0);
            let pts = [
                (0.0, c),
                (hl, c - head),
                (hl, c - shaft / 2.0),
                (b0, c - shaft / 2.0),
                (b0, 0.0),
                (b1, 0.0),
                (b1, c - shaft / 2.0),
                (l - hl, c - shaft / 2.0),
                (l - hl, c - head),
                (l, c),
                (l - hl, c + head),
                (l - hl, c + shaft / 2.0),
                (b1, c + shaft / 2.0),
                (b1, t),
                (b0, t),
                (b0, c + shaft / 2.0),
                (hl, c + shaft / 2.0),
                (hl, c + head),
            ];
            let d = if vertical { Dir::Down } else { Dir::Right };
            let pts = orient(d, w, h, &pts);
            let tr = orient(d, w, h, &[(b0, 0.0), (b1, t)]);
            simple(b.poly(&pts).take(), Rect::from_points(Point::new(tr[0].0, tr[0].1), Point::new(tr[1].0, tr[1].1)).abs(), w, h)
        }
        "quadArrowCallout" => {
            let head = (ss * a(1).max(0.0) / 1e5).min(ss / 2.0);
            let shaft = (ss * a(0).max(0.0) / 1e5).min(head * 2.0);
            let hl = (ss * a(2).max(0.0) / 1e5).min(ss / 2.0);
            let k = a(3).clamp(0.0, 100000.0) / 1e5;
            let (bw, bh) = ((w * k).min(w - 2.0 * hl).max(shaft), (h * k).min(h - 2.0 * hl).max(shaft));
            let (cx, cy) = (w / 2.0, h / 2.0);
            let (bx0, bx1, by0, by1) = (cx - bw / 2.0, cx + bw / 2.0, cy - bh / 2.0, cy + bh / 2.0);
            let (s2, hd) = (shaft / 2.0, head);
            let pts = [
                (0.0, cy),
                (hl, cy - hd),
                (hl, cy - s2),
                (bx0, cy - s2),
                (bx0, by0),
                (cx - s2, by0),
                (cx - s2, hl),
                (cx - hd, hl),
                (cx, 0.0),
                (cx + hd, hl),
                (cx + s2, hl),
                (cx + s2, by0),
                (bx1, by0),
                (bx1, cy - s2),
                (w - hl, cy - s2),
                (w - hl, cy - hd),
                (w, cy),
                (w - hl, cy + hd),
                (w - hl, cy + s2),
                (bx1, cy + s2),
                (bx1, by1),
                (cx + s2, by1),
                (cx + s2, h - hl),
                (cx + hd, h - hl),
                (cx, h),
                (cx - hd, h - hl),
                (cx - s2, h - hl),
                (cx - s2, by1),
                (bx0, by1),
                (bx0, cy + s2),
                (hl, cy + s2),
                (hl, cy + hd),
            ];
            simple(b.poly(&pts).take(), Rect::new(bx0, by0, bx1, by1), w, h)
        }
        "curvedRightArrow" | "curvedLeftArrow" | "curvedUpArrow" | "curvedDownArrow" => curved_arrow(name, w, h, a(0), a(1), a(2)),
        "swooshArrow" => swoosh(w, h, a(0), a(1)),
        "ellipseRibbon" | "ellipseRibbon2" => ellipse_ribbon(name == "ellipseRibbon2", w, h, a(0), a(1), a(2)),
        "leftRightRibbon" => left_right_ribbon(w, h, a(0), a(1), a(2)),
        "gear6" | "gear9" => gear(if name == "gear6" { 6 } else { 9 }, w, h, a(0), a(1)),
        "funnel" => funnel(w, h),
        "circularArrow" | "leftCircularArrow" | "leftRightCircularArrow" => circular_arrow(name, w, h, [a(0), a(1), a(2), a(3), a(4)]),
        _ => return None,
    };
    Some(g)
}

/// An ellipse-centred frame for the circular arrows: the band's middle runs on the ellipse
/// `(rx, ry)`, its edges `th/2` either side.
struct Ring {
    c: Point,
    rx: f64,
    ry: f64,
    th: f64,
    hw: f64,
}

impl Ring {
    /// The point at visual angle `deg` on the ellipse grown by `d`.
    fn at(&self, deg: f64, d: f64) -> Point {
        let (ax, ay) = ((self.rx + d).max(0.0), (self.ry + d).max(0.0));
        let t = vis(deg, ax, ay).to_radians();
        Point::new(self.c.x + ax * t.cos(), self.c.y + ay * t.sin())
    }
    /// Parametric angle (degrees) of `p` on the ellipse grown by `d`.
    fn param(&self, p: Point, d: f64) -> f64 {
        let (ax, ay) = ((self.rx + d).max(1e-9), (self.ry + d).max(1e-9));
        ((p.y - self.c.y) / ay).atan2((p.x - self.c.x) / ax).to_degrees()
    }
    /// Where the line `b + s·u` meets the ellipse grown by `d`, taking the crossing nearest `b`
    /// on the side `sign` (+1 outwards, −1 inwards).
    fn cross(&self, b: Point, u: Vec2, d: f64, sign: f64) -> Point {
        let (ax, ay) = ((self.rx + d).max(1e-9), (self.ry + d).max(1e-9));
        let (px, py) = ((b.x - self.c.x) / ax, (b.y - self.c.y) / ay);
        let (ux, uy) = (u.x / ax, u.y / ay);
        let qa = ux * ux + uy * uy;
        let qb = 2.0 * (px * ux + py * uy);
        let qc = px * px + py * py - 1.0;
        let disc = qb * qb - 4.0 * qa * qc;
        if qa < 1e-18 || disc < 0.0 {
            // The line misses (a long head on a thin ring): the edge then carries on from the
            // base centre itself, as the reference does.
            return b;
        }
        let r = disc.sqrt();
        let (s0, s1) = ((-qb - r) / (2.0 * qa), (-qb + r) / (2.0 * qa));
        let s = if sign > 0.0 {
            if s0 >= 0.0 { s0 } else { s1 }
        } else if s1 <= 0.0 {
            s1
        } else {
            s0
        };
        b + u * s
    }
    /// An arrow head whose base centre sits on the middle ellipse at `base` degrees and whose tip
    /// sits there at `base + sweep`. The base runs along the line from the centre to the tip.
    /// Returns (band outer edge, outer wing, tip, inner wing, band inner edge).
    fn head(&self, base: f64, sweep: f64) -> [Point; 5] {
        let b = self.at(base, 0.0);
        let tip = self.at(base + sweep, 0.0);
        let d = tip - self.c;
        let u = if d.hypot() > 1e-12 { d / d.hypot() } else { Vec2::new(1.0, 0.0) };
        let half = self.th / 2.0;
        [self.cross(b, u, half, 1.0), b + u * self.hw, tip, b - u * self.hw, self.cross(b, u, -half, -1.0)]
    }
}

/// Sweep (degrees) from `a` to `b` turning one way: positive when `dir > 0`.
fn turn(a: f64, b: f64, dir: f64) -> f64 {
    if dir > 0.0 { (b - a).rem_euclid(360.0) } else { -(a - b).rem_euclid(360.0) }
}

/// The circular arrows: a band along an ellipse with one head (`circularArrow` clockwise,
/// `leftCircularArrow` anticlockwise) or a head at each end (`leftRightCircularArrow`).
/// Adjusts: band thickness, head sweep (degrees), end angle, start angle, head half-width.
fn circular_arrow(name: &str, w: f64, h: f64, a: [f64; 5]) -> Geometry {
    let ss = w.min(h);
    if ss <= 0.0 {
        return simple(corner_rect(w, h, [Corner::Square; 4]), Rect::new(0.0, 0.0, w, h), w, h);
    }
    let a5 = a[4].clamp(0.0, 25000.0);
    let hw = ss * a5 / 1e5;
    let th = ss * a[0].clamp(0.0, 2.0 * a5) / 1e5;
    let ring = Ring { c: Point::new(w / 2.0, h / 2.0), rx: (w / 2.0 - hw).max(th / 2.0), ry: (h / 2.0 - hw).max(th / 2.0), th, hw };
    let deg = |v: f64| (v / 60000.0) % 360.0;
    let (sweep, end, start) = (deg(a[1]), deg(a[2]), deg(a[3]));
    let half = th / 2.0;
    let mut b = Pb::new();
    // Arcs of the ellipse grown by `d`, drawn on from the current point (which is on it except in
    // the degenerate case above, where the arc shifts with it like the reference's).
    let edge_arc = |b: &mut Pb, from: Point, to: Point, d: f64, dir: f64| {
        let (ax, ay) = ((ring.rx + d).max(0.0), (ring.ry + d).max(0.0));
        let (s, e) = (ring.param(from, d), ring.param(to, d));
        b.arc(ax, ay, s, turn(s, e, dir));
    };
    let outer_arc = |b: &mut Pb, from: Point, to: Point, dir: f64| edge_arc(b, from, to, half, dir);
    let inner_arc = |b: &mut Pb, from: Point, to: Point, dir: f64| edge_arc(b, from, to, -half, dir);
    if name == "leftRightCircularArrow" {
        let hs = ring.head(start, -sweep);
        let he = ring.head(end, sweep);
        b.m(hs[0].x, hs[0].y);
        outer_arc(&mut b, hs[0], he[0], 1.0);
        b.l(he[1].x, he[1].y).l(he[2].x, he[2].y).l(he[3].x, he[3].y).l(he[4].x, he[4].y);
        inner_arc(&mut b, he[4], hs[4], -1.0);
        b.l(hs[3].x, hs[3].y).l(hs[2].x, hs[2].y).l(hs[1].x, hs[1].y).z();
    } else {
        let dir = if name == "circularArrow" { 1.0 } else { -1.0 };
        let he = ring.head(end, sweep);
        let (o0, i0) = (ring.at(start, half), ring.at(start, -half));
        b.m(o0.x, o0.y);
        outer_arc(&mut b, o0, he[0], dir);
        b.l(he[1].x, he[1].y).l(he[2].x, he[2].y).l(he[3].x, he[3].y).l(he[4].x, he[4].y);
        inner_arc(&mut b, he[4], i0, -dir);
        b.z();
    }
    simple(b.take(), Rect::new(0.0, 0.0, w, h), w, h)
}

/// A funnel: an elliptical rim (with an open mouth) across the top quarter, and straight sides
/// down to a small rounded spout. The side lines leave the rim and meet the spout at fixed
/// angles, fitted to the reference look.
fn funnel(w: f64, h: f64) -> Geometry {
    const RIM: f64 = 173.0; // where the left side leaves the rim, degrees on the rim ellipse
    const SPOUT: f64 = 163.4; // where it meets the spout, degrees on the spout ellipse
    let ss = w.min(h);
    let (cx, rim_y) = (w / 2.0, h / 4.0);
    let (sx, sy) = (w / 8.0, h / 15.0);
    let spout_y = h - sy;
    let at = |rx: f64, ry: f64, cy: f64, deg: f64| {
        let t = deg.to_radians();
        Point::new(cx + rx * t.cos(), cy + ry * t.sin())
    };
    let p0 = at(cx, rim_y, rim_y, RIM);
    let p1 = at(sx, sy, spout_y, 180.0 - SPOUT);
    let mut b = Pb::new();
    b.m(p0.x, p0.y).arc(cx, rim_y, RIM, 540.0 - 2.0 * RIM);
    b.l(p1.x, p1.y).arc(sx, sy, 180.0 - SPOUT, 2.0 * SPOUT - 180.0).z();
    let d = ss / 20.0;
    b.ellipse(cx, rim_y, (cx - d).max(0.0), (rim_y - d).max(0.0));
    Geometry { paths: vec![fill_eo(b.take())], text_rect: Rect::new(w / 4.0, h / 2.0, w * 0.75, h - sy), handles: vec![], sites: mid_sites(w, h) }
}

/// A gear: an elliptical body with `n` teeth, the first pointing straight up. Each tooth leaves
/// the body at two points an equal angle either side of its centre, and its tip corners sit a
/// tooth height further out along the body's normal and a flank lean further in.
fn gear(n: usize, w: f64, h: f64, a0: f64, a1: f64) -> Geometry {
    let ss = w.min(h);
    let th = ss * a0.clamp(0.0, 20000.0) / 1e5; // tooth height
    let lean_max = if n == 6 { 5380.0 } else { 2730.0 };
    let l = ss * a1.clamp(0.0, a0.clamp(0.0, 20000.0).min(lean_max)) / 1e5; // flank lean
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (rx, ry) = ((cx - th).max(0.0), (cy - th).max(0.0));
    let r = (ss / 2.0 - th).max(1e-9);
    let half = ((th + l) / 2.0 / r).atan();
    let step = std::f64::consts::TAU / n as f64;
    let on_body = |t: f64| Point::new(cx + rx * t.cos(), cy + ry * t.sin());
    let mut b = Pb::new();
    for k in 0..n {
        let t = -std::f64::consts::FRAC_PI_2 + step * k as f64;
        // The tooth stands along the body's normal at its centre.
        let nrm = Vec2::new(t.cos() * ry, t.sin() * rx);
        let u = if nrm.hypot() > 1e-12 { nrm / nrm.hypot() } else { Vec2::new(t.cos(), t.sin()) };
        let v = Vec2::new(-u.y, u.x);
        let (r0, r1) = (on_body(t - half), on_body(t + half));
        let (c0, c1) = (r0 + u * th + v * l, r1 + u * th - v * l);
        if k == 0 {
            b.m(r0.x, r0.y);
        } else {
            b.arc(rx, ry, (t - step + half).to_degrees(), (step - 2.0 * half).to_degrees());
        }
        b.l(c0.x, c0.y).l(c1.x, c1.y).l(r1.x, r1.y);
    }
    let t_last = -std::f64::consts::FRAC_PI_2 + step * (n - 1) as f64;
    b.arc(rx, ry, (t_last + half).to_degrees(), (step - 2.0 * half).to_degrees()).z();
    let k = std::f64::consts::FRAC_1_SQRT_2;
    let text = Rect::new(cx - rx * k, cy - ry * k, cx + rx * k, cy + ry * k);
    simple(b.take(), text, w, h)
}

/// Two arrows on a band that steps down in the middle: the left arrow sits high, the right one low,
/// and the band rolls over between them in a shaded half-ellipse curl.
fn left_right_ribbon(w: f64, h: f64, a0: f64, a1: f64, a2: f64) -> Geometry {
    let ss = w.min(h);
    let sh = h * a2.clamp(0.0, 33333.0) / 1e5; // step between the two halves
    let bt = (h * a0.max(0.0) / 1e5).min(h - sh); // band thickness
    let cx = w / 2.0;
    let (rx, ry) = (w / 32.0, (sh / 4.0).min(w / 24.0));
    let hb = (ss * a1.max(0.0) / 1e5).min(cx - rx); // head length
    let yc = (h - sh) / 2.0;
    let (lt, lb) = (yc - bt / 2.0, yc + bt / 2.0);
    let (rt, rb) = (lt + sh, lb + sh);
    // The left half: filled as a closed outline, stroked only where it can show.
    let mut edge = Pb::new();
    edge.m(cx - rx, lb).l(hb, lb).l(hb, h - sh).l(0.0, yc).l(hb, 0.0).l(hb, lt).l(cx, lt);
    edge.arc(rx, ry, 270.0, 180.0).arc(rx, ry, 270.0, -90.0);
    let edge = edge.take();
    let mut left = Pb::new();
    left.m(0.0, yc).l(hb, 0.0).l(hb, lt).l(cx, lt).arc(rx, ry, 270.0, 180.0).arc(rx, ry, 270.0, -180.0);
    left.l(cx, lb.max(lt + 4.0 * ry)).l(hb, lb).l(hb, h - sh).z();
    let left = left.take();
    let mut curl = Pb::new();
    curl.m(cx + rx, lt + ry).l(cx + rx, rt).l(cx, lt + 4.0 * ry).arc(rx, ry, 90.0, 180.0).arc(rx, ry, 90.0, -90.0).z();
    let mut right = Pb::new();
    right.m(w, yc + sh).l(w - hb, h).l(w - hb, rb).l(cx, rb).arc(rx, ry, 90.0, 90.0).l(cx - rx, rt - ry).arc(rx, ry, 180.0, -90.0);
    right.l(w - hb, rt).l(w - hb, sh).z();
    Geometry {
        paths: vec![fill_only(left), stroke_only(edge), shade(curl.take(), FillMode::DarkenLess), fill(right.take())],
        text_rect: Rect::new(hb.min(cx - rx), rt, (w - hb).max(cx + rx), lb.max(rt)),
        handles: vec![],
        sites: vec![Point::new(0.0, yc), Point::new(cx, rt), Point::new(w, yc + sh), Point::new(cx, lb)],
    }
}

/// Adds a piece of the parabola `y = y0 + d·(1 − ((x − w/2)/(w/2))²)` from the current point (at
/// `xa`) to `xb`, as one quadratic curve.
fn sag(b: &mut Pb, w: f64, d: f64, y0: f64, xa: f64, xb: f64) {
    let hw = (w / 2.0).max(1e-9);
    let f = |x: f64| y0 + d * (1.0 - ((x - hw) / hw).powi(2));
    let slope = -2.0 * d * (xa - hw) / (hw * hw);
    b.q((xa + xb) / 2.0, f(xa) + slope * (xb - xa) / 2.0, xb, f(xb));
}

/// A ribbon whose edges sag along one parabola: a front band in the middle and two notched ends
/// behind it, joined by shaded folds. Built sagging downwards; `up` mirrors it vertically.
fn ellipse_ribbon(up: bool, w: f64, h: f64, a0: f64, a1: f64, a2: f64) -> Geometry {
    let th = h * a0.clamp(0.0, 100000.0) / 1e5; // band drop at the centre
    // Sag of every edge: never more than the drop, never so little that the band would rise
    // above the ends by more than half their height.
    let d = (h * a2.max(0.0) / 1e5).max(th - (h - th) / 2.0).clamp(0.0, th);
    let bw = w * a1.clamp(25000.0, 75000.0) / 1e5;
    let (x0, x1) = ((w - bw) / 2.0, (w + bw) / 2.0);
    let ear = w / 8.0;
    let (xf, xg) = (x0 + ear, x1 - ear);
    let e = h - th; // band height
    let off = th - d; // band top below the ends' top
    let t = |x: f64| {
        let hw = (w / 2.0).max(1e-9);
        d * (1.0 - ((x - hw) / hw).powi(2))
    };
    let mut ends = Pb::new();
    ends.m(0.0, 0.0);
    sag(&mut ends, w, d, 0.0, 0.0, xf);
    ends.l(xf, t(xf) + off).l(x0, t(x0) + off).l(x0, t(x0) + e);
    sag(&mut ends, w, d, e, x0, 0.0);
    ends.l(ear, t(ear) + e / 2.0).z();
    ends.m(w, 0.0).l(w - ear, t(w - ear) + e / 2.0).l(w, e);
    sag(&mut ends, w, d, e, w, x1);
    ends.l(x1, t(x1) + off).l(xg, t(xg) + off).l(xg, t(xg));
    sag(&mut ends, w, d, 0.0, xg, w);
    ends.z();
    let mut folds = Pb::new();
    folds.poly(&[(x0, t(x0) + off), (xf, t(xf)), (xf, t(xf) + off)]);
    folds.poly(&[(x1, t(x1) + off), (xg, t(xg)), (xg, t(xg) + off)]);
    let mut band = Pb::new();
    band.m(x0, t(x0) + off);
    sag(&mut band, w, d, off, x0, x1);
    band.l(x1, t(x1) + off + e);
    sag(&mut band, w, d, off + e, x1, x0);
    band.z();
    let mut paths = vec![fill(ends.take()), shade(folds.take(), FillMode::DarkenLess), fill(band.take())];
    let mut text = Rect::new(x0, th, x1, t(x0) + off + e);
    if up {
        let m = kurbo::Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, h]);
        for p in &mut paths {
            p.path.apply_affine(m);
        }
        text = m.transform_rect_bbox(text);
    }
    Geometry { paths, text_rect: text, handles: vec![], sites: mid_sites(w, h) }
}

/// A sweeping arrow rising from the bottom-left corner. The band's two edges are quadratic curves
/// from the corner; the head's base leans slightly forward and reaches one eighth of the shorter
/// side above and below the band. The tilt and the tip's lift were fitted to the reference look.
fn swoosh(w: f64, h: f64, a0: f64, a1: f64) -> Geometry {
    const TILT: f64 = 0.1126; // tan of the head base's lean from vertical
    const LIFT: f64 = 0.051; // tip height above the band's centre, per unit of height
    let ss = w.min(h);
    let e = ss / 8.0;
    let th = (h * a0.max(0.0) / 1e5).min((h - 2.0 * e).max(0.0));
    // The head is at least long enough to keep its leaning base inside the frame.
    let hl = (ss * a1.max(0.0) / 1e5).max((e + th) * TILT).min((w - e * TILT).max(0.0));
    let qx = w - hl;
    let mut b = Pb::new();
    b.m(0.0, h)
        .q(w / 6.0, h / 3.0, qx, e)
        .l(qx - e * TILT, 0.0)
        .l(w, (e + th / 2.0 - LIFT * h).clamp(0.0, 2.0 * e + th))
        .l((qx + (e + th) * TILT).min(w), 2.0 * e + th)
        .l((qx + th * TILT).min(w), e + th)
        .q(w / 4.0, (th + h / 12.0 + e).min(h), 0.0, h)
        .z();
    simple(b.take(), Rect::new(0.0, 0.0, w, h), w, h)
}

/// A band that sweeps around half an ellipse and ends in an arrow head. Built pointing right
/// (`curvedRightArrow`) in a `cw`×`ch` frame, then turned for the other directions.
///
/// Both halves of the band hang off one ellipse touching the far side: its centre sits on the
/// head side's edge, its horizontal radius is the full length, and the band has a constant
/// vertical thickness. The back half (shaded) starts flat against the head side; the front half
/// runs to the head, centred on the band where it ends.
fn curved_arrow(name: &str, w: f64, h: f64, a0: f64, a1: f64, a2: f64) -> Geometry {
    let along_x = matches!(name, "curvedRightArrow" | "curvedLeftArrow");
    let (cw, ch) = if along_x { (w, h) } else { (h, w) };
    let ss = cw.min(ch);
    let head = (ss * a1.max(0.0) / 1e5).min(ch / 2.0);
    let th = (ss * a0.max(0.0) / 1e5).min(head);
    let hl = (ss * a2.max(0.0) / 1e5).min(cw);
    let hh = head / 2.0;
    // The band's centre line, carried on to the head side, ends at the tip, which sits half a
    // head width above the bottom.
    let r = ((ch - hh - th / 2.0) / 2.0).max(0.0);
    // Parametric angle where the front band reaches the head base.
    let te = if cw > 0.0 { (-(hl / cw)).clamp(-1.0, 1.0).acos().to_degrees() } else { 90.0 };
    let mut back = Pb::new();
    back.m(cw, 0.0).arc(cw, r, 270.0, -90.0).l(0.0, r + th).arc(cw, r, 180.0, 90.0).z();
    let mut front = Pb::new();
    front.m(0.0, r).arc(cw, r, 180.0, te - 180.0);
    let yb = front.cur.y;
    let yc = yb + th / 2.0;
    front.l(cw - hl, yc - hh).l(cw, 2.0 * r + th / 2.0).l(cw - hl, yc + hh).l(cw - hl, yb + th).arc(cw, r, te, 180.0 - te).z();
    let m = match name {
        "curvedLeftArrow" => kurbo::Affine::new([-1.0, 0.0, 0.0, 1.0, w, 0.0]),
        "curvedUpArrow" => kurbo::Affine::new([0.0, -1.0, 1.0, 0.0, 0.0, h]),
        "curvedDownArrow" => kurbo::Affine::new([0.0, 1.0, 1.0, 0.0, 0.0, 0.0]),
        _ => kurbo::Affine::IDENTITY,
    };
    let (mut back, mut front) = (back.take(), front.take());
    back.apply_affine(m);
    front.apply_affine(m);
    Geometry {
        paths: vec![shade(back, FillMode::DarkenLess), fill(front)],
        text_rect: Rect::new(0.0, 0.0, w, h),
        handles: vec![],
        sites: mid_sites(w, h),
    }
}

/// A rectangle with a callout leader through `pts` points (`(y, x)` adjust pairs in 1/100 000 of
/// the height and width), optionally with an accent bar at the first point's x and a border.
fn line_callout(w: f64, h: f64, a: &dyn Fn(usize) -> f64, pts: usize, accent: bool, border: bool) -> Geometry {
    let body = corner_rect(w, h, [Corner::Square; 4]);
    // Leaders may reach well outside the box, but not without bound.
    let pt = |i: usize| (w * a(2 * i + 1).clamp(-1e7, 1e7) / 1e5, h * a(2 * i).clamp(-1e7, 1e7) / 1e5);
    let mut l = Pb::new();
    let (x0, y0) = pt(0);
    l.m(x0, y0);
    for i in 1..pts {
        let (x, y) = pt(i);
        l.l(x, y);
    }
    let mut paths = vec![if border { fill(body) } else { fill_only(body) }];
    if accent {
        paths.push(stroke_only(Pb::new().m(x0, 0.0).l(x0, h).take()));
    }
    paths.push(stroke_only(l.take()));
    Geometry { paths, text_rect: Rect::new(0.0, 0.0, w, h), handles: vec![], sites: mid_sites(w, h) }
}

#[cfg(test)]
mod tests {
    use super::super::{CATALOG, FillMode, build, info};
    use kurbo::{BezPath, PathEl, Point, Shape};

    const NEW: [&str; 38] = [
        "lineInv",
        "nonIsoscelesTrapezoid",
        "pieWedge",
        "leftRightArrowCallout",
        "upDownArrowCallout",
        "quadArrowCallout",
        "circularArrow",
        "leftCircularArrow",
        "leftRightCircularArrow",
        "curvedRightArrow",
        "curvedLeftArrow",
        "curvedUpArrow",
        "curvedDownArrow",
        "swooshArrow",
        "callout1",
        "callout2",
        "callout3",
        "accentCallout1",
        "accentCallout2",
        "accentCallout3",
        "borderCallout2",
        "borderCallout3",
        "accentBorderCallout1",
        "accentBorderCallout2",
        "accentBorderCallout3",
        "ellipseRibbon",
        "ellipseRibbon2",
        "leftRightRibbon",
        "flowChartOfflineStorage",
        "gear6",
        "gear9",
        "funnel",
        "cornerTabs",
        "squareTabs",
        "plaqueTabs",
        "chartX",
        "chartStar",
        "chartPlus",
    ];

    fn outline(name: &str, w: f64, h: f64, adj: &[f64]) -> BezPath {
        build(name, w, h, adj).map(|g| g.outline()).unwrap_or_default()
    }

    /// Distance from `p` to the nearest point of the (flattened) outline.
    fn dist(path: &BezPath, p: Point) -> f64 {
        let mut best = f64::MAX;
        let mut prev = Point::ZERO;
        let mut start = Point::ZERO;
        kurbo::flatten(path.iter(), 0.01, |el| {
            let (a, b) = match el {
                PathEl::MoveTo(q) => {
                    prev = q;
                    start = q;
                    return;
                }
                PathEl::LineTo(q) => (prev, q),
                PathEl::ClosePath => (prev, start),
                _ => return,
            };
            let d = b - a;
            let t = if d.hypot2() > 0.0 { ((p - a).dot(d) / d.hypot2()).clamp(0.0, 1.0) } else { 0.0 };
            best = best.min((a + d * t - p).hypot());
            prev = b;
        });
        best
    }

    /// Asserts every point lies within 1pt of the outline.
    fn on(name: &str, w: f64, h: f64, adj: &[f64], pts: &[(f64, f64)]) {
        let path = outline(name, w, h, adj);
        for &(x, y) in pts {
            let d = dist(&path, Point::new(x, y));
            assert!(d < 1.0, "{name} {w}x{h} {adj:?}: ({x}, {y}) is {d:.2} from the outline");
        }
    }

    #[test]
    fn new_presets_are_in_the_catalog() {
        for n in NEW {
            let i = info(n).unwrap_or_else(|| panic!("{n} missing from the catalog"));
            assert!(!i.label.is_empty());
        }
        assert!(CATALOG.len() >= 183);
    }

    #[test]
    fn new_presets_build_within_bounds_at_many_sizes_and_adjusts() {
        let sizes = [(480.0, 300.0), (300.0, 300.0), (200.0, 460.0), (100.0, 460.0), (1.0, 1.0), (0.0, 0.0), (0.0, 50.0), (60.0, 0.0)];
        let adjusts: [&[f64]; 8] = [
            &[],
            &[0.0; 8],
            &[100000.0; 8],
            &[-50000.0; 8],
            &[1e12, -1e12, 1e12, -1e12, 1e12, -1e12, 1e12, -1e12],
            &[f64::MAX, f64::MIN, f64::MAX, f64::MIN, f64::MAX, f64::MIN, f64::MAX, f64::MIN],
            &[f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, 5.0, 6.0, 7.0, 8.0],
            &[30000.0, 60000.0, 20000.0, 9000000.0, 20000.0, 40000.0, 120000.0, -30000.0],
        ];
        for n in NEW {
            // Line callouts' leaders may reach anywhere, by design.
            let callout = n.contains("allout") && !n.contains("Arrow");
            for (w, h) in sizes {
                for adj in adjusts {
                    let g = build(n, w, h, adj).unwrap_or_else(|| panic!("{n} did not build"));
                    assert!(!g.paths.is_empty(), "{n} has no paths");
                    for s in &g.paths {
                        let bb = s.path.bounding_box();
                        assert!(bb.x0.is_finite() && bb.y0.is_finite() && bb.x1.is_finite() && bb.y1.is_finite(), "{n} {w}x{h} {adj:?}: {bb:?}");
                        if !callout {
                            // Circular arrows with a head longer than their ring can hold carry the
                            // inner edge on from the head, as the reference does, so may spill.
                            let spill = if n.contains("ircular") { 0.5 } else { 0.01 };
                            let tol = 0.5 + w.max(h) * spill;
                            assert!(
                                bb.x0 >= -tol && bb.y0 >= -tol && bb.x1 <= w + tol && bb.y1 <= h + tol,
                                "{n} {w}x{h} {adj:?} out of bounds: {bb:?}"
                            );
                        }
                    }
                    let t = g.text_rect;
                    assert!(t.x0.is_finite() && t.y0.is_finite() && t.x1.is_finite() && t.y1.is_finite(), "{n} text rect {t:?}");
                }
            }
            let g = build(n, 480.0, 300.0, &[]).unwrap_or_else(|| panic!("{n}"));
            assert!(g.outline().elements().len() >= 2, "{n} default outline is empty");
        }
    }

    // The spot checks below are outline points measured on reference renders.

    #[test]
    fn simple_new_presets_match_reference_points() {
        on("lineInv", 480.0, 300.0, &[], &[(0.0, 300.0), (480.0, 0.0)]);
        on("nonIsoscelesTrapezoid", 480.0, 300.0, &[], &[(75.0, 0.0), (405.0, 0.0), (0.0, 300.0)]);
        on("nonIsoscelesTrapezoid", 200.0, 460.0, &[80000.0, 20000.0], &[(100.0, 0.0), (160.0, 0.0)]);
        on("pieWedge", 480.0, 300.0, &[], &[(64.3, 150.0), (0.0, 300.0), (480.0, 0.0)]);
        on("flowChartOfflineStorage", 480.0, 300.0, &[], &[(240.0, 300.0), (192.0, 240.0), (288.0, 240.0)]);
        on("squareTabs", 480.0, 300.0, &[], &[(28.3, 28.3), (451.7, 271.7)]);
        on("cornerTabs", 480.0, 300.0, &[], &[(23.3, 5.0)]);
        on("plaqueTabs", 480.0, 300.0, &[], &[(27.85, 5.0)]);
        on("chartStar", 480.0, 300.0, &[], &[(240.0, 0.0), (240.0, 300.0), (48.0, 30.0)]);
        on("chartPlus", 480.0, 300.0, &[], &[(0.0, 150.0), (240.0, 0.0)]);
        // Chart markers and plain callouts leave the box unstroked.
        for n in ["chartX", "callout1", "accentCallout2"] {
            let g = build(n, 100.0, 100.0, &[]).unwrap_or_else(|| panic!("{n}"));
            assert!(!g.paths[0].stroke, "{n}");
        }
    }

    #[test]
    fn line_callouts_follow_their_adjusts() {
        on("callout3", 480.0, 300.0, &[], &[(-40.0, 56.25), (-80.0, 56.25), (-80.0, 300.0), (-40.0, 338.9)]);
        let adj = [50000.0, 110000.0, 20000.0, 120000.0, -20000.0, 80000.0, 30000.0, 40000.0];
        let g = build("accentBorderCallout3", 480.0, 300.0, &adj).unwrap_or_else(|| panic!("accentBorderCallout3"));
        assert_eq!(g.paths.len(), 3);
        assert!(g.paths[0].stroke);
        let accent = g.paths[1].path.bounding_box();
        assert!((accent.x0 - 528.0).abs() < 1e-6 && accent.y0 == 0.0 && accent.y1 == 300.0, "{accent:?}");
        on("accentBorderCallout3", 480.0, 300.0, &adj, &[(576.0, 60.0), (384.0, -60.0), (192.0, 90.0)]);
    }

    #[test]
    fn arrow_callouts_match_reference_points() {
        on("leftRightArrowCallout", 480.0, 300.0, &[], &[(0.0, 150.0), (75.0, 75.0), (124.5, 0.0), (355.5, 300.0), (480.0, 150.0)]);
        on("leftRightArrowCallout", 200.0, 460.0, &[], &[(50.0, 180.0), (52.0, 0.0), (148.0, 460.0)]);
        on("upDownArrowCallout", 480.0, 300.0, &[], &[(240.0, 0.0), (0.0, 77.8), (480.0, 222.2), (240.0, 300.0)]);
        on("quadArrowCallout", 480.0, 300.0, &[], &[(0.0, 150.0), (124.5, 77.8), (240.0, 0.0), (480.0, 150.0), (240.0, 300.0)]);
    }

    #[test]
    fn curved_arrows_match_reference_points() {
        on(
            "curvedRightArrow",
            480.0,
            300.0,
            &[],
            &[(480.0, 0.0), (480.0, 75.0), (480.0, 225.0), (405.0, 149.0), (405.0, 299.0), (100.0, 151.5), (300.0, 6.7)],
        );
        on("curvedRightArrow", 200.0, 460.0, &[30000.0, 60000.0, 40000.0], &[(200.0, 400.0), (120.0, 324.5), (120.0, 443.5), (200.0, 60.0)]);
        // The band never outgrows the head; the head never outgrows half the frame.
        on("curvedRightArrow", 200.0, 460.0, &[60000.0, 30000.0, 80000.0], &[(200.0, 60.0)]);
        on("curvedLeftArrow", 480.0, 300.0, &[], &[(0.0, 225.0), (0.0, 0.0)]);
        on("curvedUpArrow", 480.0, 300.0, &[], &[(405.0, 0.0), (0.0, 0.0), (75.0, 0.0)]);
        on("curvedDownArrow", 480.0, 300.0, &[], &[(405.0, 300.0), (0.0, 300.0)]);
        let g = build("curvedRightArrow", 480.0, 300.0, &[]).unwrap_or_else(|| panic!("curvedRightArrow"));
        assert_eq!(g.paths[0].fill, FillMode::DarkenLess);
    }

    #[test]
    fn swoosh_matches_reference_points() {
        on("swooshArrow", 480.0, 300.0, &[], &[(0.0, 300.0), (425.8, 0.0), (480.0, 59.7), (442.8, 150.0), (202.0, 105.5), (202.0, 159.0)]);
        on("swooshArrow", 480.0, 300.0, &[40000.0, 16667.0], &[(480.0, 82.2), (447.8, 195.0)]);
        on("swooshArrow", 200.0, 460.0, &[], &[(163.9, 0.0), (200.0, 59.4)]);
    }

    #[test]
    fn ribbons_match_reference_points() {
        on(
            "ellipseRibbon",
            480.0,
            300.0,
            &[],
            &[(0.0, 0.0), (120.0, 28.1), (60.0, 128.9), (240.0, 300.0), (120.0, 66.1), (180.0, 72.2), (180.0, 35.2)],
        );
        on("ellipseRibbon", 480.0, 300.0, &[30000.0, 60000.0, 20000.0], &[(120.0, 45.0), (240.0, 300.0), (96.0, 68.4)]);
        on("ellipseRibbon", 480.0, 300.0, &[80000.0, 20000.0, 60000.0], &[(240.0, 240.0), (1.0, 61.75)]);
        on("ellipseRibbon2", 480.0, 300.0, &[], &[(0.0, 300.0), (240.0, 0.0)]);
        on(
            "leftRightRibbon",
            480.0,
            300.0,
            &[],
            &[(0.0, 125.0), (150.0, 0.0), (150.0, 50.0), (255.0, 62.5), (225.0, 87.5), (480.0, 175.0), (330.0, 300.0)],
        );
        on("leftRightRibbon", 480.0, 300.0, &[90000.0, 10000.0, 60000.0], &[(0.0, 100.0), (480.0, 200.0), (240.0, 300.0)]);
    }

    #[test]
    fn gears_and_funnel_match_reference_points() {
        // Square gear6: body radius 105, top tooth tip 3.5 inside the frame, tip half-width ~16.3.
        on("gear6", 300.0, 300.0, &[], &[(150.0, 3.5), (133.7, 3.5), (166.3, 3.5), (45.0, 150.0)]);
        on("gear9", 300.0, 300.0, &[], &[(150.0, 1.0), (30.0, 150.0)]);
        on("gear6", 480.0, 300.0, &[], &[(240.0, 3.5), (201.0, 3.5)]);
        on("funnel", 480.0, 300.0, &[], &[(240.0, 0.0), (0.0, 75.0), (240.0, 15.0), (15.0, 75.0), (240.0, 300.0), (60.5, 150.0)]);
        let g = build("funnel", 480.0, 300.0, &[]).unwrap_or_else(|| panic!("funnel"));
        assert!(g.paths[0].even_odd);
    }

    #[test]
    fn circular_arrows_match_reference_points() {
        on("circularArrow", 300.0, 300.0, &[], &[(19.0, 150.0), (56.5, 150.0), (150.0, 19.0), (262.4, 150.0), (218.6, 113.4), (293.6, 113.4)]);
        on(
            "circularArrow",
            300.0,
            300.0,
            &[20000.0, 1800000.0, 18000000.0, 5400000.0, 20000.0],
            &[(150.0, 270.0), (150.0, 210.0), (247.0, 42.1), (143.0, 102.1), (227.9, 105.0)],
        );
        on("leftCircularArrow", 300.0, 300.0, &[], &[(37.6, 150.0), (262.4, 150.0), (150.0, 281.0)]);
        on("leftRightCircularArrow", 300.0, 300.0, &[], &[(262.4, 150.0), (37.6, 150.0), (150.0, 19.0)]);
        // The band never grows past twice the head's half-width.
        on("circularArrow", 300.0, 300.0, &[25000.0, 1142319.0, 20457681.0, 10800000.0, 5000.0], &[(0.0, 150.0), (30.0, 150.0)]);
    }
}
