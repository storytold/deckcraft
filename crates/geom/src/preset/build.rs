use super::builder::{
    Corner, Dir, Pb, arrow, corner_rect, double_arrow, ellipse_sites, fill, fill_eo, fill_only, len_thick, mid_sites, orient, polygon_regular, shade,
    simple, spec_pts, star, star_text_rect, stroke_only, tip_handle, vis,
};
use super::catalog::{FillMode, Geometry, Handle, info};
use super::shapes::{action_button, cloud, math, ribbon, scroll, wedge_rect};
use super::{Arc, BezPath, Point, Rect, Vec2, extra};

/// Size and adjust values shared by every preset-shape builder.
struct Ctx<'a> {
    name: &'a str,
    w: f64,
    h: f64,
    adj: &'a [f64],
    defaults: &'a [f64],
}

impl Ctx<'_> {
    /// Adjust value `i`: the caller's value, else the preset default, else 0 (non-finite values become 0).
    fn a(&self, i: usize) -> f64 {
        let v = self.adj.get(i).copied().or_else(|| self.defaults.get(i).copied()).unwrap_or(0.0);
        if v.is_finite() { v } else { 0.0 }
    }
}

/// Builds the geometry for preset `name` at size `w`×`h` with adjust values `adj` (missing
/// values use the preset's defaults). Unknown names return `None`; the caller falls back to `rect`.
pub fn build(name: &str, w: f64, h: f64, adj: &[f64]) -> Option<Geometry> {
    if !(w.is_finite() && h.is_finite()) {
        return None;
    }
    let defaults = info(name).map(|i| i.defaults).unwrap_or(&[]);
    let c = Ctx { name, w: w.max(0.0), h: h.max(0.0), adj, defaults };
    rect_and_triangle_shapes(&c)
        .or_else(|| polygon_and_symbol_shapes(&c))
        .or_else(|| bracket_and_arrow_shapes(&c))
        .or_else(|| flowchart_and_star_shapes(&c))
        .or_else(|| callout_wave_and_connector_shapes(&c))
        .or_else(|| extra::build(name, c.w, c.h, &|i| c.a(i)))
}

fn rect_and_triangle_shapes(c: &Ctx) -> Option<Geometry> {
    let name = c.name;
    let w = c.w;
    let h = c.h;
    let ss = c.w.min(c.h);
    let full = Rect::new(0.0, 0.0, c.w, c.h);
    let a = |i: usize| c.a(i);
    let mut b = Pb::new();
    let g = match c.name {
        "rect" | "textBox" | "flowChartProcess" => simple(corner_rect(w, h, [Corner::Square; 4]), full, w, h),
        "actionButtonBlank"
        | "actionButtonBackPrevious"
        | "actionButtonForwardNext"
        | "actionButtonBeginning"
        | "actionButtonEnd"
        | "actionButtonHome"
        | "actionButtonInformation"
        | "actionButtonReturn"
        | "actionButtonMovie"
        | "actionButtonDocument"
        | "actionButtonSound"
        | "actionButtonHelp" => action_button(name, w, h),
        "roundRect" => {
            let v = a(0).clamp(0.0, 50000.0);
            let r = ss * v / 100000.0;
            let mut g = simple(corner_rect(w, h, [Corner::Round(r); 4]), full.inset(-r * 0.29289), w, h);
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "flowChartAlternateProcess" => simple(corner_rect(w, h, [Corner::Round(ss / 6.0); 4]), full.inset(-ss / 6.0 * 0.29289), w, h),
        "snip1Rect" => {
            let v = a(0).clamp(0.0, 50000.0);
            let d = ss * v / 100000.0;
            let mut g = simple(
                corner_rect(w, h, [Corner::Square, Corner::Snip(d), Corner::Square, Corner::Square]),
                Rect::new(0.0, d / 2.0, w - d / 2.0, h),
                w,
                h,
            );
            g.handles.push(Handle::new(0, Point::new(w, 0.0), Vec2::new(-ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "snip2SameRect" => {
            let (d1, d2) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5);
            simple(corner_rect(w, h, [Corner::Snip(d1), Corner::Snip(d1), Corner::Snip(d2), Corner::Snip(d2)]), full.inset(-d1.max(d2) / 2.0), w, h)
        }
        "snip2DiagRect" => {
            let (d1, d2) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5);
            simple(corner_rect(w, h, [Corner::Snip(d1), Corner::Snip(d2), Corner::Snip(d1), Corner::Snip(d2)]), full.inset(-d1.max(d2) / 2.0), w, h)
        }
        "snipRoundRect" => {
            let (r, d) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5);
            simple(corner_rect(w, h, [Corner::Round(r), Corner::Snip(d), Corner::Square, Corner::Square]), full.inset(-r.max(d) / 2.0), w, h)
        }
        "round1Rect" => {
            let r = ss * a(0).clamp(0.0, 50000.0) / 1e5;
            simple(corner_rect(w, h, [Corner::Square, Corner::Round(r), Corner::Square, Corner::Square]), full.inset(-r * 0.29289), w, h)
        }
        "round2SameRect" => {
            let (r1, r2) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5);
            simple(
                corner_rect(w, h, [Corner::Round(r1), Corner::Round(r1), Corner::Round(r2), Corner::Round(r2)]),
                full.inset(-r1.max(r2) * 0.29289),
                w,
                h,
            )
        }
        "round2DiagRect" => {
            let (r1, r2) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5);
            simple(
                corner_rect(w, h, [Corner::Round(r1), Corner::Round(r2), Corner::Round(r1), Corner::Round(r2)]),
                full.inset(-r1.max(r2) * 0.29289),
                w,
                h,
            )
        }
        "ellipse" | "flowChartConnector" => {
            let mut g =
                simple(b.ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0).take(), Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355), w, h);
            g.sites = ellipse_sites(w, h);
            g
        }
        "triangle" | "flowChartExtract" => {
            let v = if name == "triangle" { a(0).clamp(0.0, 100000.0) } else { 50000.0 };
            let ax = w * v / 100000.0;
            let mut g = simple(b.poly(&[(0.0, h), (ax, 0.0), (w, h)]).take(), Rect::new(ax / 2.0, h / 2.0, (ax + w) / 2.0, h), w, h);
            g.sites = vec![
                Point::new(ax, 0.0),
                Point::new(ax / 2.0, h / 2.0),
                Point::new(0.0, h),
                Point::new(w / 2.0, h),
                Point::new(w, h),
                Point::new((ax + w) / 2.0, h / 2.0),
            ];
            if name == "triangle" {
                g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(w / 100000.0, 0.0), v, 0.0, 100000.0));
            }
            g
        }
        "flowChartMerge" => simple(b.poly(&[(0.0, 0.0), (w, 0.0), (w / 2.0, h)]).take(), Rect::new(w / 4.0, 0.0, w * 0.75, h / 2.0), w, h),
        "rtTriangle" => {
            simple(b.poly(&[(0.0, 0.0), (0.0, h), (w, h)]).take(), Rect::new(w / 12.0, h * 7.0 / 12.0, w * 7.0 / 12.0, h * 11.0 / 12.0), w, h)
        }
        "parallelogram" | "flowChartInputOutput" => {
            let v = if name == "parallelogram" { a(0).clamp(0.0, 100000.0 * w / ss.max(1e-9)) } else { 20000.0 * w / ss.max(1e-9) };
            let off = ss * v / 100000.0;
            let mut g = simple(
                b.poly(&[(0.0, h), (off, 0.0), (w, 0.0), (w - off, h)]).take(),
                Rect::new(off / 2.0 + off * 0.2, h * 0.08, w - off / 2.0 - off * 0.2, h * 0.92),
                w,
                h,
            );
            if name == "parallelogram" {
                g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 100000.0 * w / ss.max(1e-9)));
            }
            g
        }
        "trapezoid" => {
            let v = a(0).clamp(0.0, 50000.0 * w / ss.max(1e-9));
            let off = ss * v / 100000.0;
            let mut g =
                simple(b.poly(&[(0.0, h), (off, 0.0), (w - off, 0.0), (w, h)]).take(), Rect::new(off * 0.66, h * 0.2, w - off * 0.66, h), w, h);
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0 * w / ss.max(1e-9)));
            g
        }
        "flowChartManualOperation" => {
            simple(b.poly(&[(0.0, 0.0), (w, 0.0), (w * 0.8, h), (w * 0.2, h)]).take(), Rect::new(w * 0.2, 0.0, w * 0.8, h), w, h)
        }
        "diamond" | "flowChartDecision" => simple(
            b.poly(&[(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)]).take(),
            Rect::new(w / 4.0, h / 4.0, w * 0.75, h * 0.75),
            w,
            h,
        ),
        "flowChartSort" => {
            let d = b.poly(&[(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)]).take();
            let line = Pb::new().m(0.0, h / 2.0).l(w, h / 2.0).take();
            Geometry {
                paths: vec![fill(d), stroke_only(line)],
                text_rect: Rect::new(w / 4.0, h / 4.0, w * 0.75, h * 0.75),
                handles: vec![],
                sites: mid_sites(w, h),
            }
        }
        _ => return None,
    };
    Some(g)
}

fn polygon_and_symbol_shapes(c: &Ctx) -> Option<Geometry> {
    let name = c.name;
    let w = c.w;
    let h = c.h;
    let ss = c.w.min(c.h);
    let full = Rect::new(0.0, 0.0, c.w, c.h);
    let a = |i: usize| c.a(i);
    let mut b = Pb::new();
    let g = match c.name {
        "pentagon" => simple(b.poly(&polygon_regular(5, w, h, -90.0)).take(), Rect::new(w * 0.2, h * 0.27, w * 0.8, h * 0.9), w, h),
        "heptagon" => simple(b.poly(&polygon_regular(7, w, h, -90.0)).take(), Rect::new(w * 0.16, h * 0.2, w * 0.84, h * 0.86), w, h),
        "decagon" => simple(b.poly(&polygon_regular(10, w, h, 0.0)).take(), Rect::new(w * 0.1, h * 0.17, w * 0.9, h * 0.83), w, h),
        "dodecagon" => simple(b.poly(&polygon_regular(12, w, h, 15.0)).take(), Rect::new(w * 0.13, h * 0.13, w * 0.87, h * 0.87), w, h),
        "hexagon" | "flowChartPreparation" => {
            let v = if name == "hexagon" { a(0).clamp(0.0, 50000.0 * w / ss.max(1e-9)) } else { 20000.0 * w / ss.max(1e-9) };
            let off = ss * v / 100000.0;
            let mut g = simple(
                b.poly(&[(0.0, h / 2.0), (off, 0.0), (w - off, 0.0), (w, h / 2.0), (w - off, h), (off, h)]).take(),
                Rect::new(off * 0.6, h * 0.14, w - off * 0.6, h * 0.86),
                w,
                h,
            );
            if name == "hexagon" {
                g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0 * w / ss.max(1e-9)));
            }
            g
        }
        "octagon" => {
            let v = a(0).clamp(0.0, 50000.0);
            let d = ss * v / 100000.0;
            let mut g = simple(
                b.poly(&[(d, 0.0), (w - d, 0.0), (w, d), (w, h - d), (w - d, h), (d, h), (0.0, h - d), (0.0, d)]).take(),
                full.inset(-d / 2.0),
                w,
                h,
            );
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "plus" => {
            let v = a(0).clamp(0.0, 50000.0);
            let d = ss * v / 100000.0;
            let mut g = simple(
                b.poly(&[
                    (d, 0.0),
                    (w - d, 0.0),
                    (w - d, d),
                    (w, d),
                    (w, h - d),
                    (w - d, h - d),
                    (w - d, h),
                    (d, h),
                    (d, h - d),
                    (0.0, h - d),
                    (0.0, d),
                    (d, d),
                ])
                .take(),
                Rect::new(0.0, d, w, h - d),
                w,
                h,
            );
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "frame" => {
            let v = a(0).clamp(0.0, 50000.0);
            let t = ss * v / 100000.0;
            b.poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]);
            b.poly(&[(t, t), (w - t, t), (w - t, h - t), (t, h - t)]);
            let mut g = Geometry { paths: vec![fill_eo(b.take())], text_rect: full.inset(-t), handles: vec![], sites: mid_sites(w, h) };
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "halfFrame" => {
            let (t1, t2) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5);
            simple(b.poly(&[(0.0, 0.0), (w, 0.0), (w - t1, t1), (t2, t1), (t2, h - t2), (0.0, h)]).take(), full, w, h)
        }
        "corner" => {
            let (t1, t2) = (ss * a(0).clamp(0.0, 100000.0) / 1e5, ss * a(1).clamp(0.0, 100000.0) / 1e5);
            simple(b.poly(&[(0.0, 0.0), (t2, 0.0), (t2, h - t1), (w, h - t1), (w, h), (0.0, h)]).take(), Rect::new(0.0, h - t1, w, h), w, h)
        }
        "diagStripe" => {
            let k = a(0).clamp(0.0, 100000.0) / 1e5;
            simple(b.poly(&[(0.0, h * k), (w * k, 0.0), (w, 0.0), (0.0, h)]).take(), Rect::new(0.0, 0.0, w / 2.0, h / 2.0), w, h)
        }
        "plaque" => {
            let v = a(0).clamp(0.0, 50000.0);
            let r = ss * v / 100000.0;
            b.m(0.0, r)
                .arc(r, r, 90.0, -90.0)
                .l(w - r, 0.0)
                .arc(r, r, 180.0, -90.0)
                .l(w, h - r)
                .arc(r, r, 270.0, -90.0)
                .l(r, h)
                .arc(r, r, 0.0, -90.0)
                .z();
            let mut g = simple(b.take(), full.inset(-r * 0.7), w, h);
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "can" | "flowChartMagneticDisk" => {
            // The disk's ends are a third of its height (ECMA-376: ellipses of radius h/6).
            let v = if name == "can" { a(0).clamp(0.0, 50000.0 * h / ss.max(1e-9)) } else { 100000.0 / 3.0 * h / ss.max(1e-9) };
            let eh = ss * v / 100000.0;
            let r = eh / 2.0;
            let body = Pb::new().m(0.0, r).l(0.0, h - r).arc(w / 2.0, r, 180.0, -180.0).l(w, r).arc(w / 2.0, r, 0.0, -180.0).z().take();
            let top = Pb::new().ellipse(w / 2.0, r, w / 2.0, r).take();
            let mut g = Geometry {
                paths: vec![fill(body), shade(top, if name == "can" { FillMode::Lighten } else { FillMode::Norm })],
                text_rect: Rect::new(0.0, eh, w, h - r),
                handles: vec![],
                sites: mid_sites(w, h),
            };
            if name == "can" {
                g.handles.push(Handle::new(0, Point::new(w / 2.0, 0.0), Vec2::new(0.0, ss / 100000.0), v, 0.0, 50000.0 * h / ss.max(1e-9)));
            }
            g
        }
        "flowChartMagneticDrum" => {
            // ECMA-376: end ellipses of radius w/6 across.
            let r = w / 6.0;
            let body = Pb::new().m(r, 0.0).l(w - r, 0.0).arc(r, h / 2.0, 270.0, 180.0).l(r, h).arc(r, h / 2.0, 90.0, 180.0).z().take();
            let side = Pb::new().m(w - r, 0.0).arc(r, h / 2.0, 270.0, -180.0).take();
            Geometry {
                paths: vec![fill(body), stroke_only(side)],
                text_rect: Rect::new(r, 0.0, w - 2.0 * r, h),
                handles: vec![],
                sites: mid_sites(w, h),
            }
        }
        "cube" => {
            let v = a(0).clamp(0.0, 100000.0);
            let d = ss * v / 100000.0;
            let front = Pb::new().poly(&[(0.0, d), (w - d, d), (w - d, h), (0.0, h)]).take();
            let top = Pb::new().poly(&[(0.0, d), (d, 0.0), (w, 0.0), (w - d, d)]).take();
            let side = Pb::new().poly(&[(w - d, d), (w, 0.0), (w, h - d), (w - d, h)]).take();
            let mut g = Geometry {
                paths: vec![fill(front), shade(top, FillMode::LightenLess), shade(side, FillMode::DarkenLess)],
                text_rect: Rect::new(0.0, d, w - d, h),
                handles: vec![],
                sites: mid_sites(w, h),
            };
            g.handles.push(Handle::new(0, Point::new(0.0, 0.0), Vec2::new(0.0, ss / 100000.0), v, 0.0, 100000.0));
            g
        }
        "bevel" => {
            let v = a(0).clamp(0.0, 50000.0);
            let d = ss * v / 100000.0;
            let inner = Pb::new().poly(&[(d, d), (w - d, d), (w - d, h - d), (d, h - d)]).take();
            let top = Pb::new().poly(&[(0.0, 0.0), (w, 0.0), (w - d, d), (d, d)]).take();
            let left = Pb::new().poly(&[(0.0, 0.0), (d, d), (d, h - d), (0.0, h)]).take();
            let right = Pb::new().poly(&[(w, 0.0), (w, h), (w - d, h - d), (w - d, d)]).take();
            let bottom = Pb::new().poly(&[(0.0, h), (d, h - d), (w - d, h - d), (w, h)]).take();
            let mut g = Geometry {
                paths: vec![
                    fill(inner),
                    shade(top, FillMode::LightenLess),
                    shade(left, FillMode::Lighten),
                    shade(right, FillMode::DarkenLess),
                    shade(bottom, FillMode::Darken),
                ],
                text_rect: full.inset(-d),
                handles: vec![],
                sites: mid_sites(w, h),
            };
            g.handles.push(Handle::new(0, Point::ZERO, Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "donut" => {
            let v = a(0).clamp(0.0, 50000.0);
            let t = ss * v / 100000.0;
            b.ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0);
            b.ellipse(w / 2.0, h / 2.0, (w / 2.0 - t).max(0.0), (h / 2.0 - t).max(0.0));
            let mut g = Geometry {
                paths: vec![fill_eo(b.take())],
                text_rect: Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355),
                handles: vec![],
                sites: ellipse_sites(w, h),
            };
            g.handles.push(Handle::new(0, Point::new(0.0, h / 2.0), Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0));
            g
        }
        "noSmoking" => {
            let v = a(0).clamp(0.0, 50000.0);
            let t = ss * v / 100000.0;
            let (rx, ry) = ((w / 2.0 - t).max(0.0), (h / 2.0 - t).max(0.0));
            b.ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0);
            b.ellipse(w / 2.0, h / 2.0, rx, ry);
            let ring = b.take();
            let th = (h).atan2(w);
            let dir = Vec2::new(th.cos(), th.sin());
            let r = if rx > 0.0 && ry > 0.0 { rx * ry / ((ry * th.cos()).powi(2) + (rx * th.sin()).powi(2)).sqrt() } else { 0.0 };
            let perp = Vec2::new(-dir.y, dir.x) * (t / 2.0);
            let c = Point::new(w / 2.0, h / 2.0);
            let p = [c - dir * r - perp, c + dir * r - perp, c + dir * r + perp, c - dir * r + perp];
            let bar = Pb::new().poly(&p.map(|q| (q.x, q.y))).take();
            Geometry {
                paths: vec![fill_eo(ring), fill(bar)],
                text_rect: Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355),
                handles: vec![Handle::new(0, Point::new(0.0, h / 2.0), Vec2::new(ss / 100000.0, 0.0), v, 0.0, 50000.0)],
                sites: ellipse_sites(w, h),
            }
        }
        "blockArc" => {
            let st = a(0) / 60000.0;
            let en = a(1) / 60000.0;
            let t = ss * a(2).clamp(0.0, 50000.0) / 100000.0;
            let mut sw = en - st;
            if sw <= 0.0 {
                sw += 360.0;
            }
            let (rx, ry) = (w / 2.0, h / 2.0);
            let (irx, iry) = ((rx - t).max(0.0), (ry - t).max(0.0));
            let s0 = vis(st, rx, ry);
            let e0 = vis(st + sw, rx, ry);
            let mut sweep = e0 - s0;
            if sweep <= 0.0 {
                sweep += 360.0;
            }
            let si = vis(st + sw, irx, iry);
            let ei = vis(st, irx, iry);
            let mut isweep = ei - si;
            if isweep >= 0.0 {
                isweep -= 360.0;
            }
            let p0 = Point::new(w / 2.0 + rx * s0.to_radians().cos(), h / 2.0 + ry * s0.to_radians().sin());
            b.m(p0.x, p0.y).arc(rx, ry, s0, sweep);
            let pi = Point::new(w / 2.0 + irx * si.to_radians().cos(), h / 2.0 + iry * si.to_radians().sin());
            b.l(pi.x, pi.y).arc(irx, iry, si, isweep).z();
            let mut g = simple(b.take(), full, w, h);
            let c = Point::new(rx, ry);
            g.handles.push(Handle::angle(0, c, rx - t / 2.0, ry - t / 2.0, a(0)));
            // Thickness: slides inward from the outer edge along the end angle's ray.
            let pe = Point::new(rx + rx * e0.to_radians().cos(), ry + ry * e0.to_radians().sin());
            let u = (c - pe).normalize();
            let u = if u.x.is_finite() && u.y.is_finite() { u } else { Vec2::ZERO };
            g.handles.push(Handle::new(2, pe, u * (ss / 100000.0), a(2).clamp(0.0, 50000.0), 0.0, 50000.0));
            g
        }
        "pie" | "chord" | "arc" => {
            let (rx, ry) = (w / 2.0, h / 2.0);
            let s = vis(a(0) / 60000.0, rx, ry);
            let e = vis(a(1) / 60000.0, rx, ry);
            let mut sw = e - s;
            if sw <= 0.0 {
                sw += 360.0;
            }
            let p0 = Point::new(rx + rx * s.to_radians().cos(), ry + ry * s.to_radians().sin());
            b.m(p0.x, p0.y).arc(rx, ry, s, sw);
            let text = Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355);
            let mut g = match name {
                "pie" => {
                    b.l(rx, ry).z();
                    simple(b.take(), text, w, h)
                }
                "chord" => {
                    b.z();
                    simple(b.take(), text, w, h)
                }
                _ => {
                    let open = b.take();
                    let mut f = BezPath::from_vec(open.elements().to_vec());
                    f.line_to(Point::new(rx, ry));
                    f.close_path();
                    Geometry { paths: vec![fill_only(f), stroke_only(open)], text_rect: text, handles: vec![], sites: mid_sites(w, h) }
                }
            };
            let c = Point::new(rx, ry);
            g.handles.push(Handle::angle(0, c, rx, ry, a(0)));
            g.handles.push(Handle::angle(1, c, rx, ry, a(1)));
            g
        }
        "teardrop" => {
            let k = a(0).clamp(0.0, 200000.0) / 100000.0;
            let tip = (w / 2.0 + w / 2.0 * k, h / 2.0 - h / 2.0 * k);
            b.m(0.0, h / 2.0).arc(w / 2.0, h / 2.0, 180.0, 90.0);
            b.q(w / 2.0 + w / 4.0 * k, 0.0, tip.0, tip.1).q(w, h / 2.0 - h / 4.0 * k, w, h / 2.0);
            b.arc(w / 2.0, h / 2.0, 0.0, 180.0).z();
            simple(b.take(), Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355), w, h)
        }
        "foldedCorner" => {
            let v = a(0).clamp(0.0, 50000.0);
            let d = ss * v / 100000.0;
            let body = Pb::new().poly(&[(0.0, 0.0), (w, 0.0), (w, h - d), (w - d, h), (0.0, h)]).take();
            let fold = Pb::new().m(w - d, h).l(w - d + d * 0.2, h - d + d * 0.2).l(w, h - d).z().take();
            Geometry {
                paths: vec![fill(body), shade(fold, FillMode::DarkenLess)],
                text_rect: Rect::new(0.0, 0.0, w, h - d),
                handles: vec![],
                sites: mid_sites(w, h),
            }
        }
        "smileyFace" => {
            let k = a(0).clamp(-4653.0, 4653.0) / 100000.0;
            let face = Pb::new().ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0).take();
            let mut eyes = Pb::new();
            eyes.ellipse(w * 0.35, h * 0.37, w * 0.05, h * 0.05);
            eyes.ellipse(w * 0.65, h * 0.37, w * 0.05, h * 0.05);
            let ys = h * (0.72 - k);
            let smile = Pb::new().m(w * 0.22, ys).q(w / 2.0, ys + h * k * 4.0 + h * 0.0, w * 0.78, ys).take();
            Geometry {
                paths: vec![fill(face), shade(eyes.take(), FillMode::DarkenLess), stroke_only(smile)],
                text_rect: Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355),
                handles: vec![],
                sites: ellipse_sites(w, h),
            }
        }
        "heart" => {
            // ECMA-376 heart: two cubic curves from the notch at the top to the point.
            let (dx1, dx2) = (w * 49.0 / 48.0, w * 10.0 / 48.0);
            let y1 = -h / 3.0;
            b.m(w / 2.0, h / 4.0)
                .c(w / 2.0 + dx2, y1, w / 2.0 + dx1, h / 4.0, w / 2.0, h)
                .c(w / 2.0 - dx1, h / 4.0, w / 2.0 - dx2, y1, w / 2.0, h / 4.0)
                .z();
            simple(b.take(), Rect::new(w * 0.2, h * 0.25, w * 0.8, h * 0.7), w, h)
        }
        "lightningBolt" => {
            // ECMA-376 lightningBolt, in 21600ths of the box.
            let pts = spec_pts(
                &[
                    (8472, 0),
                    (12860, 6080),
                    (11050, 6797),
                    (16577, 12007),
                    (14767, 12877),
                    (21600, 21600),
                    (10012, 14915),
                    (12222, 13987),
                    (5022, 9705),
                    (7602, 8382),
                    (0, 3890),
                ],
                w,
                h,
            );
            simple(b.poly(&pts).take(), Rect::new(w * 8757.0 / 21600.0, h * 7637.0 / 21600.0, w * 13917.0 / 21600.0, h * 12963.0 / 21600.0), w, h)
        }
        "sun" => {
            // ECMA-376 sun: eight triangular rays (their bases at fixed fractions of the core's
            // margin) around a circle of radius 50% - adj.
            let a = a(0).clamp(12500.0, 46875.0);
            let g0 = 50000.0 - a;
            let (g1, g2) = (g0 * 30274.0 / 32768.0, g0 * 12540.0 / 32768.0);
            let (g10, g11) = ((50000.0 - g1) * 0.75, (50000.0 - g2) * 0.75);
            let (g12, g13, g14) = (g10 + 3662.0, g11 + 3662.0, g11 + 12500.0);
            let (g15, g16, g17, g18) = (100000.0 - g10, 100000.0 - g12, 100000.0 - g13, 100000.0 - g14);
            let x = |g: f64| w * g / 100000.0;
            let y = |g: f64| h * g / 100000.0;
            let (ox1, oy1, ox2, oy2) = (w * 18436.0 / 21600.0, h * 3163.0 / 21600.0, w * 3163.0 / 21600.0, h * 18436.0 / 21600.0);
            let (hc, vc) = (w / 2.0, h / 2.0);
            b.poly(&[(w, vc), (x(g15), y(g18)), (x(g15), y(g14))]);
            b.poly(&[(ox1, oy1), (x(g16), y(g13)), (x(g17), y(g12))]);
            b.poly(&[(hc, 0.0), (x(g18), y(g10)), (x(g14), y(g10))]);
            b.poly(&[(ox2, oy1), (x(g13), y(g12)), (x(g12), y(g13))]);
            b.poly(&[(0.0, vc), (x(g10), y(g14)), (x(g10), y(g18))]);
            b.poly(&[(ox2, oy2), (x(g12), y(g17)), (x(g13), y(g16))]);
            b.poly(&[(hc, h), (x(g14), y(g15)), (x(g18), y(g15))]);
            b.poly(&[(ox1, oy2), (x(g17), y(g16)), (x(g16), y(g17))]);
            b.ellipse(hc, vc, x(g0), y(g0));
            let g7 = g0 * 23170.0 / 32768.0;
            let mut g = simple(b.take(), Rect::new(x(50000.0 - g7), y(50000.0 - g7), x(50000.0 + g7), y(50000.0 + g7)), w, h);
            g.handles.push(Handle::new(0, Point::new(0.0, vc), Vec2::new(w / 100000.0, 0.0), a, 12500.0, 46875.0));
            g
        }
        "moon" => {
            // ECMA-376 moon: the outer edge is the left half of an ellipse w × h/2 centred on the
            // right edge; the inner edge an arc through the corners and (adj · w, middle) of a
            // larger ellipse (rx = g18, ry = dy1) whose centre lies right of that point.
            let k = a(0).clamp(0.0, 87500.0) / 100000.0;
            let ssf = ss.max(1e-9);
            let g0 = ss * k;
            let g1 = (ss - g0).max(1e-9);
            let g2 = g0 * g0 / g1;
            let g3 = ss * ss / g1;
            let g5 = 2.0 * g3 - g2;
            let g6 = g5 - g0;
            let dy1 = (g5 / 2.0 - g0) * (h / 2.0) / ssf;
            let (g0w, g6w) = (g0 * w / ssf, g6 * w / ssf);
            let rx = (g6w - g0w) / 2.0;
            let cx = g0w + rx;
            b.m(w, 0.0).arc(w, h / 2.0, 270.0, -180.0);
            if rx > 1e-9 && dy1 > 1e-9 {
                // From the bottom-right corner back up to the top-right through (g0w, h/2).
                let t0 = ((h / 2.0) / dy1).clamp(-1.0, 1.0).atan2((w - cx) / rx);
                let t1 = (-(h / 2.0) / dy1).clamp(-1.0, 1.0).atan2((w - cx) / rx) + std::f64::consts::TAU;
                let arc = Arc::new(Point::new(cx, h / 2.0), Vec2::new(rx, dy1), t0, t1 - t0, 0.0);
                arc.to_cubic_beziers(0.05, |p1, p2, p| {
                    b.p.curve_to(p1, p2, p);
                });
            }
            b.z();
            let g12 = g0 * 9598.0 / 32768.0;
            let g13 = ss - g12;
            let dy4 = (ss * ss - g13 * g13).max(0.0).sqrt() * (h / 2.0) / ssf;
            simple(b.take(), Rect::new(g12 * w / ssf, h / 2.0 - dy4, g0w, h / 2.0 + dy4), w, h)
        }
        _ => return None,
    };
    Some(g)
}

fn bracket_and_arrow_shapes(c: &Ctx) -> Option<Geometry> {
    let name = c.name;
    let w = c.w;
    let h = c.h;
    let ss = c.w.min(c.h);
    let full = Rect::new(0.0, 0.0, c.w, c.h);
    let a = |i: usize| c.a(i);
    let mut b = Pb::new();
    let g = match c.name {
        "cloud" | "cloudCallout" => cloud(name, w, h, a(0), a(1)),
        "bracketPair" => {
            let r = ss * a(0).clamp(0.0, 50000.0) / 1e5;
            let mut l = Pb::new();
            l.m(r, h).arc(r, r, 90.0, 90.0).l(0.0, r).arc(r, r, 180.0, 90.0);
            l.m(w - r, 0.0).arc(r, r, 270.0, 90.0).l(w, h - r).arc(r, r, 0.0, 90.0);
            let f = corner_rect(w, h, [Corner::Round(r); 4]);
            Geometry {
                paths: vec![fill_only(f), stroke_only(l.take())],
                text_rect: full.inset(-r * 0.29289),
                handles: vec![],
                sites: mid_sites(w, h),
            }
        }
        "bracePair" => {
            let r = ss * a(0).clamp(0.0, 25000.0) / 1e5;
            let mut l = Pb::new();
            l.m(2.0 * r, h).arc(r, r, 90.0, 90.0).l(r, h / 2.0 + r).arc(r, r, 0.0, -90.0).arc(r, r, 90.0, -90.0).l(r, r).arc(r, r, 180.0, 90.0);
            l.m(w - 2.0 * r, 0.0)
                .arc(r, r, 270.0, 90.0)
                .l(w - r, h / 2.0 - r)
                .arc(r, r, 180.0, -90.0)
                .arc(r, r, 270.0, -90.0)
                .l(w - r, h - r)
                .arc(r, r, 0.0, 90.0);
            let f = corner_rect(w, h, [Corner::Round(r); 4]);
            Geometry { paths: vec![fill_only(f), stroke_only(l.take())], text_rect: full.inset(-r * 1.2), handles: vec![], sites: mid_sites(w, h) }
        }
        "leftBracket" | "rightBracket" => {
            let r = (ss * a(0).clamp(0.0, 50000.0 * h / ss.max(1e-9)) / 1e5).min(h / 2.0);
            let mut l = Pb::new();
            l.m(w, h).arc(w, r, 90.0, 90.0).l(0.0, r).arc(w, r, 180.0, 90.0);
            let mut path = l.take();
            if name == "rightBracket" {
                path.apply_affine(kurbo::Affine::new([-1.0, 0.0, 0.0, 1.0, w, 0.0]));
            }
            // Filled (when the shape has a fill) out to the straight side, as PowerPoint does.
            let mut area = path.clone();
            area.close_path();
            Geometry { paths: vec![fill_only(area), stroke_only(path)], text_rect: full, handles: vec![], sites: mid_sites(w, h) }
        }
        "leftBrace" | "rightBrace" => {
            let r = (ss * a(0).clamp(0.0, 25000.0 * h / ss.max(1e-9)) / 1e5).min(h / 4.0);
            let my = h * a(1).clamp(0.0, 100000.0) / 1e5;
            let hw = w / 2.0;
            let mut l = Pb::new();
            l.m(w, h).arc(hw, r, 90.0, 90.0).l(hw, my + r).arc(hw, r, 0.0, -90.0).arc(hw, r, 90.0, -90.0).l(hw, r).arc(hw, r, 180.0, 90.0);
            let mut path = l.take();
            if name == "rightBrace" {
                path.apply_affine(kurbo::Affine::new([-1.0, 0.0, 0.0, 1.0, w, 0.0]));
            }
            // Filled (when the shape has a fill) out to the straight side, as PowerPoint does.
            let mut area = path.clone();
            area.close_path();
            Geometry { paths: vec![fill_only(area), stroke_only(path)], text_rect: full, handles: vec![], sites: mid_sites(w, h) }
        }
        "rightArrow" => arrow(Dir::Right, w, h, a(0), a(1)),
        "leftArrow" => arrow(Dir::Left, w, h, a(0), a(1)),
        "upArrow" => arrow(Dir::Up, w, h, a(0), a(1)),
        "downArrow" => arrow(Dir::Down, w, h, a(0), a(1)),
        "leftRightArrow" => double_arrow(false, w, h, a(0), a(1)),
        "upDownArrow" => double_arrow(true, w, h, a(0), a(1)),
        "notchedRightArrow" | "stripedRightArrow" => {
            let mut g = arrow(Dir::Right, w, h, a(0), a(1));
            let k = a(0).clamp(0.0, 100000.0) / 1e5;
            let (y1, y2) = (h / 2.0 - h * k / 2.0, h / 2.0 + h * k / 2.0);
            let dx = (ss * a(1).max(0.0) / 1e5).min(w);
            if name == "notchedRightArrow" {
                // x1 = dy1 · dx2 / hd2 in presetShapeDefinitions, with dy1 = h·k/2: the notch's
                // edges run parallel to the head's
                let n = dx * k;
                let pts = [(0.0, y1), (w - dx, y1), (w - dx, 0.0), (w, h / 2.0), (w - dx, h), (w - dx, y2), (0.0, y2), (n, h / 2.0)];
                g.paths = vec![fill(Pb::new().poly(&pts).take())];
            } else {
                let s = ss / 32.0;
                let start = 5.0 * s;
                let pts = [(start, y1), (w - dx, y1), (w - dx, 0.0), (w, h / 2.0), (w - dx, h), (w - dx, y2), (start, y2)];
                let mut stripes = Pb::new();
                stripes.poly(&[(0.0, y1), (s, y1), (s, y2), (0.0, y2)]);
                stripes.poly(&[(2.0 * s, y1), (4.0 * s, y1), (4.0 * s, y2), (2.0 * s, y2)]);
                g.paths = vec![fill(stripes.take()), fill(Pb::new().poly(&pts).take())];
            }
            g
        }
        "homePlate" | "chevron" => {
            let v = a(0).clamp(0.0, 100000.0 * w / ss.max(1e-9));
            let d = (ss * v / 1e5).min(w);
            let pts: Vec<(f64, f64)> = if name == "homePlate" {
                vec![(0.0, 0.0), (w - d, 0.0), (w, h / 2.0), (w - d, h), (0.0, h)]
            } else {
                vec![(0.0, 0.0), (w - d, 0.0), (w, h / 2.0), (w - d, h), (0.0, h), (d, h / 2.0)]
            };
            // Text rectangles as presetShapeDefinitions give them: homePlate's runs to halfway
            // along the point; chevron's lies between the notch and the point's base (x1..x2)
            // and covers the whole width once those meet or cross (`?: dx x1 l`).
            let text = if name == "homePlate" {
                Rect::new(0.0, 0.0, w - d / 2.0, h)
            } else if w - d > d {
                Rect::new(d, 0.0, w - d, h)
            } else {
                Rect::new(0.0, 0.0, w, h)
            };
            let mut g = simple(b.poly(&pts).take(), text, w, h);
            g.handles.push(Handle::new(0, Point::new(w, 0.0), Vec2::new(-ss / 1e5, 0.0), v, 0.0, 100000.0 * w / ss.max(1e-9)));
            g
        }
        "quadArrow" => {
            let (t, hw, hl) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5, ss * a(2).clamp(0.0, 50000.0) / 1e5);
            let (cx, cy) = (w / 2.0, h / 2.0);
            let (t2, hw2) = (t / 2.0, hw);
            let pts = [
                (0.0, cy),
                (hl, cy - hw2),
                (hl, cy - t2),
                (cx - t2, cy - t2),
                (cx - t2, hl),
                (cx - hw2, hl),
                (cx, 0.0),
                (cx + hw2, hl),
                (cx + t2, hl),
                (cx + t2, cy - t2),
                (w - hl, cy - t2),
                (w - hl, cy - hw2),
                (w, cy),
                (w - hl, cy + hw2),
                (w - hl, cy + t2),
                (cx + t2, cy + t2),
                (cx + t2, h - hl),
                (cx + hw2, h - hl),
                (cx, h),
                (cx - hw2, h - hl),
                (cx - t2, h - hl),
                (cx - t2, cy + t2),
                (hl, cy + t2),
                (hl, cy + hw2),
            ];
            simple(b.poly(&pts).take(), Rect::new(cx - t2, cy - t2, cx + t2, cy + t2), w, h)
        }
        "leftRightUpArrow" => {
            let (t, hw, hl) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5, ss * a(2).clamp(0.0, 50000.0) / 1e5);
            let cx = w / 2.0;
            let by = h - hw; // horizontal shaft centre
            let pts = [
                (0.0, by),
                (hl, by - hw),
                (hl, by - t / 2.0),
                (cx - t / 2.0, by - t / 2.0),
                (cx - t / 2.0, hl),
                (cx - hw, hl),
                (cx, 0.0),
                (cx + hw, hl),
                (cx + t / 2.0, hl),
                (cx + t / 2.0, by - t / 2.0),
                (w - hl, by - t / 2.0),
                (w - hl, by - hw),
                (w, by),
                (w - hl, h),
                (w - hl, by + t / 2.0),
                (hl, by + t / 2.0),
                (hl, h),
            ];
            simple(b.poly(&pts).take(), Rect::new(hl, by - t / 2.0, w - hl, by + t / 2.0), w, h)
        }
        "leftUpArrow" | "bentUpArrow" => {
            let (t, hw, hl) = (ss * a(0).clamp(0.0, 50000.0) / 1e5, ss * a(1).clamp(0.0, 50000.0) / 1e5, ss * a(2).clamp(0.0, 50000.0) / 1e5);
            let pts: Vec<(f64, f64)> = if name == "leftUpArrow" {
                let cx = w - hw;
                let cy = h - hw;
                vec![
                    (0.0, cy),
                    (hl, cy - hw),
                    (hl, cy - t / 2.0),
                    (cx - t / 2.0, cy - t / 2.0),
                    (cx - t / 2.0, hl),
                    (cx - hw, hl),
                    (cx, 0.0),
                    (w, hl),
                    (cx + t / 2.0, hl),
                    (cx + t / 2.0, cy + t / 2.0),
                    (hl, cy + t / 2.0),
                    (hl, h),
                ]
            } else {
                let cx = w - hw;
                vec![
                    (0.0, h - t),
                    (cx - t / 2.0, h - t),
                    (cx - t / 2.0, hl),
                    (cx - hw, hl),
                    (cx, 0.0),
                    (w, hl),
                    (cx + t / 2.0, hl),
                    (cx + t / 2.0, h),
                    (0.0, h),
                ]
            };
            simple(b.poly(&pts).take(), full, w, h)
        }
        "bentArrow" => {
            let (t, hw, hl, r) = (
                ss * a(0).clamp(0.0, 50000.0) / 1e5,
                ss * a(1).clamp(0.0, 50000.0) / 1e5,
                ss * a(2).clamp(0.0, 50000.0) / 1e5,
                ss * a(3).clamp(0.0, 100000.0) / 1e5,
            );
            let cy = hw; // head centre line
            let ro = r.max(t);
            let ri = (ro - t).max(0.0);
            b.m(0.0, h)
                .l(0.0, cy - t / 2.0 + ro)
                .arc(ro, ro, 180.0, 90.0)
                .l(w - hl, cy - t / 2.0)
                .l(w - hl, 0.0)
                .l(w, cy)
                .l(w - hl, 2.0 * hw)
                .l(w - hl, cy + t / 2.0);
            b.l(t + ri, cy + t / 2.0);
            if ri > 0.0 {
                b.arc(ri, ri, 270.0, -90.0);
            }
            b.l(t, h).z();
            simple(b.take(), full, w, h)
        }
        "uturnArrow" => {
            let (t, hw, hl, r) = (
                ss * a(0).clamp(0.0, 25000.0) / 1e5,
                ss * a(1).clamp(0.0, 25000.0) / 1e5,
                ss * a(2).clamp(0.0, 25000.0) / 1e5,
                ss * a(3).clamp(0.0, 100000.0) / 1e5,
            );
            let hy = (h * a(4).clamp(0.0, 100000.0) / 1e5).max(hl + t);
            let ax = w - hw; // arrow shaft centre x
            let ro = r.max(t).min((ax + t / 2.0) / 2.0);
            let ri = (ro - t).max(0.0);
            b.m(0.0, h).l(0.0, ro).arc(ro, ro, 180.0, 90.0).l(ax + t / 2.0 - ro, 0.0).arc(ro, ro, 270.0, 90.0);
            b.l(ax + t / 2.0, hy - hl).l(w, hy - hl).l(ax, hy).l(ax - hw, hy - hl).l(ax - t / 2.0, hy - hl).l(ax - t / 2.0, ro);
            if ri > 0.0 {
                b.arc(ri, ri, 0.0, -90.0);
            }
            b.l(t + ri, t);
            if ri > 0.0 {
                b.arc(ri, ri, 270.0, -90.0);
            }
            b.l(t, h).z();
            simple(b.take(), full, w, h)
        }
        "rightArrowCallout" | "leftArrowCallout" | "upArrowCallout" | "downArrowCallout" => {
            let d = match name {
                "rightArrowCallout" => Dir::Right,
                "leftArrowCallout" => Dir::Left,
                "upArrowCallout" => Dir::Up,
                _ => Dir::Down,
            };
            let (l, t) = len_thick(d, w, h);
            let ss2 = l.min(t);
            let shaft = ss2 * a(0).clamp(0.0, 100000.0) / 1e5;
            let head = ss2 * a(1).clamp(0.0, 100000.0) / 1e5;
            let hl = ss2 * a(2).clamp(0.0, 100000.0) / 1e5;
            let boxl = (l * a(3).clamp(0.0, 100000.0) / 1e5).min(l - hl);
            let c = t / 2.0;
            let pts = [
                (0.0, 0.0),
                (boxl, 0.0),
                (boxl, c - shaft / 2.0),
                (l - hl, c - shaft / 2.0),
                (l - hl, c - head),
                (l, c),
                (l - hl, c + head),
                (l - hl, c + shaft / 2.0),
                (boxl, c + shaft / 2.0),
                (boxl, t),
                (0.0, t),
            ];
            let pts = orient(d, w, h, &pts);
            let tr = orient(d, w, h, &[(0.0, 0.0), (boxl, t)]);
            simple(b.poly(&pts).take(), Rect::from_points(Point::new(tr[0].0, tr[0].1), Point::new(tr[1].0, tr[1].1)).abs(), w, h)
        }
        "mathPlus" | "mathMinus" | "mathEqual" | "mathDivide" | "mathMultiply" | "mathNotEqual" => math(name, w, h, a(0), a(1), a(2)),
        _ => return None,
    };
    Some(g)
}

fn flowchart_and_star_shapes(c: &Ctx) -> Option<Geometry> {
    let name = c.name;
    let w = c.w;
    let h = c.h;
    let ss = c.w.min(c.h);
    let full = Rect::new(0.0, 0.0, c.w, c.h);
    let a = |i: usize| c.a(i);
    let mut b = Pb::new();
    let g = match c.name {
        "flowChartPredefinedProcess" => {
            let r = Pb::new().poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]).take();
            let mut l = Pb::new();
            l.m(w / 8.0, 0.0).l(w / 8.0, h).m(w * 7.0 / 8.0, 0.0).l(w * 7.0 / 8.0, h);
            Geometry {
                paths: vec![fill(r), stroke_only(l.take())],
                text_rect: Rect::new(w / 8.0, 0.0, w * 7.0 / 8.0, h),
                handles: vec![],
                sites: mid_sites(w, h),
            }
        }
        "flowChartInternalStorage" => {
            let r = Pb::new().poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]).take();
            let mut l = Pb::new();
            l.m(w / 8.0, 0.0).l(w / 8.0, h).m(0.0, h / 8.0).l(w, h / 8.0);
            Geometry {
                paths: vec![fill(r), stroke_only(l.take())],
                text_rect: Rect::new(w / 8.0, h / 8.0, w, h),
                handles: vec![],
                sites: mid_sites(w, h),
            }
        }
        "flowChartDocument" => {
            let u = |x: f64, y: f64| (w * x / 21600.0, h * y / 21600.0);
            let (p1, p2, p3, p4) = (u(21600.0, 17322.0), u(10800.0, 17322.0), u(10800.0, 23922.0), u(0.0, 20172.0));
            b.m(0.0, 0.0).l(w, 0.0).l(p1.0, p1.1).c(p2.0, p2.1, p3.0, p3.1, p4.0, p4.1).z();
            simple(b.take(), Rect::new(0.0, 0.0, w, h * 17322.0 / 21600.0), w, h)
        }
        "flowChartMultidocument" => {
            // ECMA-376 multidocument (21600ths): two back pages peeking out above and right of
            // the front one; the back pages' edges are outlined only where they show.
            let x = |v: f64| w * v / 21600.0;
            let y = |v: f64| h * v / 21600.0;
            let front = |b: &mut Pb| {
                b.m(0.0, y(20782.0))
                    .c(x(9298.0), y(23542.0), x(9298.0), y(18022.0), x(18595.0), y(18022.0))
                    .l(x(18595.0), y(3675.0))
                    .l(0.0, y(3675.0))
                    .z();
            };
            let mid = |b: &mut Pb| {
                b.m(x(1532.0), y(3675.0)).l(x(1532.0), y(1815.0)).l(x(20000.0), y(1815.0)).l(x(20000.0), y(16252.0));
                b.c(x(19298.0), y(16252.0), x(18595.0), y(16352.0), x(18595.0), y(16352.0));
            };
            let back = |b: &mut Pb| {
                b.m(x(2972.0), y(1815.0)).l(x(2972.0), 0.0).l(w, 0.0).l(w, y(14392.0));
                b.c(x(20800.0), y(14392.0), x(20000.0), y(14467.0), x(20000.0), y(14467.0));
            };
            let mut area = Pb::new();
            front(&mut area);
            mid(&mut area);
            area.l(x(18595.0), y(3675.0)).z();
            back(&mut area);
            area.l(x(20000.0), y(1815.0)).z();
            let mut outline = Pb::new();
            outline.m(0.0, y(3675.0)).l(x(18595.0), y(3675.0)).l(x(18595.0), y(18022.0));
            outline.c(x(9298.0), y(18022.0), x(9298.0), y(23542.0), 0.0, y(20782.0)).z();
            mid(&mut outline);
            back(&mut outline);
            Geometry {
                paths: vec![fill_only(area.take()), stroke_only(outline.take())],
                text_rect: Rect::new(0.0, y(3675.0), x(18595.0), y(17322.0)),
                handles: vec![],
                sites: mid_sites(w, h),
            }
        }
        "flowChartTerminator" => {
            let rx = (w * 0.16).min(w / 2.0);
            b.m(rx, 0.0).l(w - rx, 0.0).arc(rx, h / 2.0, 270.0, 180.0).l(rx, h).arc(rx, h / 2.0, 90.0, 180.0).z();
            simple(b.take(), Rect::new(rx * 0.3, h * 0.15, w - rx * 0.3, h * 0.85), w, h)
        }
        "flowChartManualInput" => simple(b.poly(&[(0.0, h / 5.0), (w, 0.0), (w, h), (0.0, h)]).take(), Rect::new(0.0, h / 5.0, w, h), w, h),
        "flowChartOffpageConnector" => {
            simple(b.poly(&[(0.0, 0.0), (w, 0.0), (w, h * 0.8), (w / 2.0, h), (0.0, h * 0.8)]).take(), Rect::new(0.0, 0.0, w, h * 0.8), w, h)
        }
        "flowChartPunchedCard" => {
            simple(b.poly(&[(w * 0.2, 0.0), (w, 0.0), (w, h), (0.0, h), (0.0, h * 0.2)]).take(), Rect::new(0.0, h * 0.2, w, h), w, h)
        }
        "flowChartPunchedTape" => {
            // ECMA-376: each long edge is two half-ellipses a quarter of the width wide and a
            // tenth of the height deep, one bulging in and one out.
            let (rx, ry) = (w / 4.0, h / 10.0);
            b.m(0.0, ry).arc(rx, ry, 180.0, -180.0).arc(rx, ry, 180.0, 180.0).l(w, h - ry).arc(rx, ry, 0.0, -180.0).arc(rx, ry, 0.0, 180.0).z();
            simple(b.take(), Rect::new(0.0, h / 5.0, w, h * 0.8), w, h)
        }
        "flowChartSummingJunction" | "flowChartOr" => {
            let e = Pb::new().ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0).take();
            let mut l = Pb::new();
            if name == "flowChartOr" {
                l.m(w / 2.0, 0.0).l(w / 2.0, h).m(0.0, h / 2.0).l(w, h / 2.0);
            } else {
                let (dx, dy) = (w / 2.0 * std::f64::consts::FRAC_1_SQRT_2, h / 2.0 * std::f64::consts::FRAC_1_SQRT_2);
                l.m(w / 2.0 - dx, h / 2.0 - dy).l(w / 2.0 + dx, h / 2.0 + dy).m(w / 2.0 + dx, h / 2.0 - dy).l(w / 2.0 - dx, h / 2.0 + dy);
            }
            Geometry {
                paths: vec![fill(e), stroke_only(l.take())],
                text_rect: Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355),
                handles: vec![],
                sites: ellipse_sites(w, h),
            }
        }
        "flowChartCollate" => simple(b.poly(&[(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)]).take(), Rect::new(w / 4.0, h / 4.0, w * 0.75, h * 0.75), w, h),
        "flowChartOnlineStorage" => {
            let rx = w / 6.0;
            b.m(rx, 0.0).l(w, 0.0).arc(rx, h / 2.0, 270.0, -180.0).l(rx, h).arc(rx, h / 2.0, 90.0, 180.0).z();
            simple(b.take(), Rect::new(rx, 0.0, w - rx, h), w, h)
        }
        "flowChartDelay" => {
            b.m(0.0, 0.0).l(w / 2.0, 0.0).arc(w / 2.0, h / 2.0, 270.0, 180.0).l(0.0, h).z();
            simple(b.take(), Rect::new(0.0, h * 0.15, w * 0.85, h * 0.85), w, h)
        }
        "flowChartMagneticTape" => {
            b.m(w / 2.0, h).arc(w / 2.0, h / 2.0, 90.0, 90.0).arc(w / 2.0, h / 2.0, 180.0, 90.0).arc(w / 2.0, h / 2.0, 270.0, 90.0).arc(
                w / 2.0,
                h / 2.0,
                0.0,
                45.0,
            );
            let p = b.cur;
            b.l(w, p.y).l(w, h).z();
            simple(b.take(), Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355), w, h)
        }
        "flowChartDisplay" => {
            let rx = w / 6.0;
            b.m(0.0, h / 2.0).l(rx, 0.0).l(w - rx, 0.0).arc(rx, h / 2.0, 270.0, 180.0).l(rx, h).z();
            simple(b.take(), Rect::new(rx, 0.0, w - rx, h), w, h)
        }
        "star4" | "star5" | "star6" | "star7" | "star8" | "star10" | "star12" | "star16" | "star24" | "star32" => {
            let n: usize = name.trim_start_matches("star").parse().unwrap_or(5);
            let v = a(0).clamp(0.0, 50000.0);
            let inner = v / 50000.0;
            let text = star_text_rect(n, w, h, inner).unwrap_or_else(|| full.inset(-ss * (0.5 - inner * 0.5) * 0.5));
            let mut g = simple(b.poly(&star(n, w, h, inner)).take(), text, w, h);
            g.handles.push(Handle::new(0, Point::new(w / 2.0, h / 2.0), Vec2::new(0.0, -h / 2.0 / 50000.0), v, 0.0, 50000.0));
            g
        }
        "irregularSeal1" => {
            // ECMA-376 irregularSeal1 ("Explosion: 8 Points"), in 21600ths of the box.
            let pts = spec_pts(
                &[
                    (10800, 5800),
                    (14522, 0),
                    (14155, 5325),
                    (18380, 4457),
                    (16702, 7315),
                    (21097, 8137),
                    (17607, 10475),
                    (21600, 13290),
                    (16837, 12942),
                    (18145, 18095),
                    (14020, 14457),
                    (13247, 19737),
                    (10532, 14935),
                    (8485, 21600),
                    (7715, 15627),
                    (4762, 17617),
                    (5667, 13937),
                    (135, 14587),
                    (3722, 11775),
                    (0, 8615),
                    (4627, 7617),
                    (370, 2295),
                    (7312, 6320),
                    (8352, 2295),
                ],
                w,
                h,
            );
            simple(b.poly(&pts).take(), Rect::new(w * 4627.0 / 21600.0, h * 6320.0 / 21600.0, w * 16702.0 / 21600.0, h * 13937.0 / 21600.0), w, h)
        }
        "irregularSeal2" => {
            // ECMA-376 irregularSeal2 ("Explosion: 14 Points"), in 21600ths of the box.
            let pts = spec_pts(
                &[
                    (11462, 4342),
                    (14790, 0),
                    (14525, 5777),
                    (18007, 3172),
                    (16380, 6532),
                    (21600, 6645),
                    (16985, 9402),
                    (18270, 11290),
                    (16380, 12310),
                    (18877, 15632),
                    (14640, 14350),
                    (14942, 17370),
                    (12180, 15935),
                    (11612, 18842),
                    (9872, 17370),
                    (8700, 19712),
                    (7527, 18125),
                    (4917, 21600),
                    (4805, 18240),
                    (1285, 17825),
                    (3330, 15370),
                    (0, 12877),
                    (3935, 11592),
                    (1172, 8270),
                    (5372, 7817),
                    (4502, 3625),
                    (8550, 6382),
                    (9722, 1887),
                ],
                w,
                h,
            );
            simple(b.poly(&pts).take(), Rect::new(w * 5372.0 / 21600.0, h * 6382.0 / 21600.0, w * 14640.0 / 21600.0, h * 15935.0 / 21600.0), w, h)
        }
        _ => return None,
    };
    Some(g)
}

fn callout_wave_and_connector_shapes(c: &Ctx) -> Option<Geometry> {
    let name = c.name;
    let w = c.w;
    let h = c.h;
    let ss = c.w.min(c.h);
    let full = Rect::new(0.0, 0.0, c.w, c.h);
    let a = |i: usize| c.a(i);
    let mut b = Pb::new();
    let g = match c.name {
        "ribbon" | "ribbon2" => ribbon(name == "ribbon2", w, h, a(0), a(1)),
        "verticalScroll" | "horizontalScroll" => scroll(name == "verticalScroll", w, h, a(0)),
        "wave" | "doubleWave" => {
            // ECMA-376 wave / doubleWave: the top and bottom edges are cubic waves (one or two
            // periods) of amplitude adj1, shifted sideways by adj2.
            let a1 = a(0).clamp(0.0, if name == "wave" { 20000.0 } else { 12500.0 });
            let a2 = a(1).clamp(-10000.0, 10000.0);
            let y1 = h * a1 / 100000.0;
            let dy2 = y1 * 10.0 / 3.0;
            let (y2, y3, y4) = (y1 - dy2, y1 + dy2, h - y1);
            let (y5, y6) = (y4 - dy2, y4 + dy2);
            let of2 = w * a2 / 50000.0;
            let dx2 = of2.min(0.0);
            let dxr = of2.max(0.0);
            let (x2, xr, xl, x10) = (-dx2, w - dxr, dxr, w + dx2);
            let text_y = h * a1 / 50000.0;
            if name == "wave" {
                let dx3 = (dx2 + xr) / 3.0;
                let (x3, x7) = (x2 + dx3, xl + dx3);
                let (x4, x8) = ((x3 + xr) / 2.0, (x7 + x10) / 2.0);
                b.m(x2, y1).c(x3, y2, x4, y3, xr, y1).l(x10, y4).c(x8, y6, x7, y5, xl, y4).z();
            } else {
                let (dx3, dx4) = ((dx2 + xr) / 6.0, (dx2 + xr) / 3.0);
                let (x3, x4, x5) = (x2 + dx3, x2 + dx4, (x2 + xr) / 2.0);
                let x6 = x5 + dx3;
                let x7 = (x6 + xr) / 2.0;
                let (x10b, x11, x12) = (xl + dx3, xl + dx4, (xl + x10) / 2.0);
                let x13 = x12 + dx3;
                let x14 = (x13 + x10) / 2.0;
                b.m(x2, y1).c(x3, y2, x4, y3, x5, y1).c(x6, y2, x7, y3, xr, y1).l(x10, y4);
                b.c(x14, y6, x13, y5, x12, y4).c(x11, y6, x10b, y5, xl, y4).z();
            }
            simple(b.take(), Rect::new(x2.max(xl), text_y, xr.min(x10), h - text_y), w, h)
        }
        "wedgeRectCallout" | "wedgeRoundRectCallout" => {
            let r = if name == "wedgeRoundRectCallout" { ss * a(2).clamp(0.0, 50000.0) / 1e5 } else { 0.0 };
            wedge_rect(w, h, a(0), a(1), r)
        }
        "wedgeEllipseCallout" => {
            let tip = Point::new(w / 2.0 + w * a(0) / 1e5, h / 2.0 + h * a(1) / 1e5);
            let ang = (tip.y - h / 2.0).atan2(tip.x - w / 2.0).to_degrees();
            let s = vis(ang + 10.0, w / 2.0, h / 2.0);
            let p0 = Point::new(w / 2.0 + w / 2.0 * s.to_radians().cos(), h / 2.0 + h / 2.0 * s.to_radians().sin());
            let e = vis(ang - 10.0, w / 2.0, h / 2.0);
            let mut sw = e - s;
            if sw <= 0.0 {
                sw += 360.0;
            }
            b.m(p0.x, p0.y).arc(w / 2.0, h / 2.0, s, sw).l(tip.x, tip.y).z();
            let mut g = simple(b.take(), Rect::new(w * 0.14645, h * 0.14645, w * 0.85355, h * 0.85355), w, h);
            g.handles.push(Handle::new(0, Point::new(w / 2.0, h / 2.0), Vec2::new(w / 1e5, 0.0), a(0), -1e6, 1e6));
            g.handles.push(Handle::new(1, Point::new(w / 2.0, h / 2.0), Vec2::new(0.0, h / 1e5), a(1), -1e6, 1e6));
            g.handles.retain(|_| false);
            g.handles.push(tip_handle(w, h, tip));
            g
        }
        "borderCallout1" => {
            let r = Pb::new().poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]).take();
            let p1 = (w * a(1) / 1e5, h * a(0) / 1e5);
            let p2 = (w * a(3) / 1e5, h * a(2) / 1e5);
            let l = Pb::new().m(p1.0, p1.1).l(p2.0, p2.1).take();
            Geometry { paths: vec![fill(r), stroke_only(l)], text_rect: full, handles: vec![], sites: mid_sites(w, h) }
        }
        "line" | "straightConnector1" => {
            let l = Pb::new().m(0.0, 0.0).l(w, h).take();
            Geometry { paths: vec![stroke_only(l)], text_rect: full, handles: vec![], sites: vec![Point::ZERO, Point::new(w, h)] }
        }
        "bentConnector2" => {
            let l = Pb::new().m(0.0, 0.0).l(w, 0.0).l(w, h).take();
            Geometry { paths: vec![stroke_only(l)], text_rect: full, handles: vec![], sites: vec![Point::ZERO, Point::new(w, h)] }
        }
        "bentConnector3" | "bentConnector4" | "bentConnector5" => {
            let x = w * a(0) / 1e5;
            let l = Pb::new().m(0.0, 0.0).l(x, 0.0).l(x, h).l(w, h).take();
            let mut g = Geometry { paths: vec![stroke_only(l)], text_rect: full, handles: vec![], sites: vec![Point::ZERO, Point::new(w, h)] };
            g.handles.push(Handle::new(0, Point::new(0.0, h / 2.0), Vec2::new(w / 1e5, 0.0), a(0), -1e6, 1e6));
            g
        }
        "curvedConnector2" | "curvedConnector3" | "curvedConnector4" | "curvedConnector5" => {
            let x = w * if name == "curvedConnector2" { 100000.0 } else { a(0) } / 1e5;
            let l = Pb::new().m(0.0, 0.0).c(x / 2.0, 0.0, x, h / 4.0, x, h / 2.0).c(x, h * 0.75, (x + w) / 2.0, h, w, h).take();
            Geometry { paths: vec![stroke_only(l)], text_rect: full, handles: vec![], sites: vec![Point::ZERO, Point::new(w, h)] }
        }
        _ => return None,
    };
    Some(g)
}
