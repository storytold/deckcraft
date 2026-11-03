use super::builder::{Corner, Pb, corner_rect, fill, mid_sites, shade, simple, stroke_only, tip_handle};
use super::catalog::{FillMode, Geometry};
use super::{Point, Rect};

pub(super) fn wedge_rect(w: f64, h: f64, a0: f64, a1: f64, r: f64) -> Geometry {
    let tip = Point::new(w / 2.0 + w * a0 / 1e5, h / 2.0 + h * a1 / 1e5);
    let inside = tip.x > 0.0 && tip.x < w && tip.y > 0.0 && tip.y < h;
    let (dx, dy) = ((tip.x - w / 2.0) / w.max(1e-9), (tip.y - h / 2.0) / h.max(1e-9));
    let mut b = Pb::new();
    let r = r.min(w / 2.0).min(h / 2.0);
    // Walk clockwise from the top-left; insert the wedge on the edge facing the tip.
    let horiz = dx.abs() > dy.abs();
    // The wedge's base: 2..5 twelfths of the edge on the tip's side of the centre when it is
    // before it, 7..10 twelfths when after (presetShapeDefinitions' xg1/xg2, yg1/yg2).
    let (bx0, bx1) = if tip.x < w / 2.0 { (w * 2.0 / 12.0, w * 5.0 / 12.0) } else { (w * 7.0 / 12.0, w * 10.0 / 12.0) };
    let (by0, by1) = if tip.y < h / 2.0 { (h * 2.0 / 12.0, h * 5.0 / 12.0) } else { (h * 7.0 / 12.0, h * 10.0 / 12.0) };
    b.m(r, 0.0);
    if !inside && !horiz && dy < 0.0 {
        b.l(bx0, 0.0).l(tip.x, tip.y).l(bx1, 0.0);
    }
    b.l(w - r, 0.0);
    if r > 0.0 {
        b.arc(r, r, 270.0, 90.0);
    }
    if !inside && horiz && dx > 0.0 {
        b.l(w, by0).l(tip.x, tip.y).l(w, by1);
    }
    b.l(w, h - r);
    if r > 0.0 {
        b.arc(r, r, 0.0, 90.0);
    }
    if !inside && !horiz && dy > 0.0 {
        b.l(bx1, h).l(tip.x, tip.y).l(bx0, h);
    }
    b.l(r, h);
    if r > 0.0 {
        b.arc(r, r, 90.0, 90.0);
    }
    if !inside && horiz && dx < 0.0 {
        b.l(0.0, by1).l(tip.x, tip.y).l(0.0, by0);
    }
    b.l(0.0, r);
    if r > 0.0 {
        b.arc(r, r, 180.0, 90.0);
    }
    b.z();
    let mut g = simple(b.take(), Rect::new(0.0, 0.0, w, h).inset(-r * 0.29289), w, h);
    g.handles.push(tip_handle(w, h, tip));
    g
}

/// ECMA-376 cloud outline arcs (radii in 43200ths of the box, angles in 60000ths of a degree,
/// as DrawingML `arcTo` takes them), from the start point (3900, 14370).
const CLOUD_ARCS: [(f64, f64, f64, f64); 11] = [
    (6753.0, 9190.0, -11429249.0, 7426832.0),
    (5333.0, 7267.0, -8646143.0, 5396714.0),
    (4365.0, 5945.0, -8748475.0, 5983381.0),
    (4857.0, 6595.0, -7859164.0, 7034504.0),
    (5333.0, 7273.0, -4722533.0, 6541615.0),
    (6775.0, 9220.0, -2776035.0, 7816140.0),
    (5785.0, 7867.0, 37501.0, 6842000.0),
    (6752.0, 9215.0, 1347096.0, 6910353.0),
    (7720.0, 10543.0, 3974558.0, 4542661.0),
    (4360.0, 5918.0, -16496525.0, 8804134.0),
    (4345.0, 5945.0, -14809710.0, 9151131.0),
];

/// The cloud's creases: short arcs inside the outline, each from its own start point.
const CLOUD_CREASES: [(f64, f64, f64, f64, f64, f64); 11] = [
    (4693.0, 26177.0, 4345.0, 5945.0, 5204520.0, 1585770.0),
    (6928.0, 34899.0, 4360.0, 5918.0, 4416628.0, 686848.0),
    (16478.0, 39090.0, 6752.0, 9215.0, 8257449.0, 844866.0),
    (28827.0, 34751.0, 6752.0, 9215.0, 387196.0, 959901.0),
    (34129.0, 22954.0, 5785.0, 7867.0, -4217541.0, 4255042.0),
    (41798.0, 15354.0, 5333.0, 7273.0, 1819082.0, 1665090.0),
    (38324.0, 5426.0, 4857.0, 6595.0, -824660.0, 891534.0),
    (29078.0, 3952.0, 4857.0, 6595.0, -8950887.0, 1091722.0),
    (22141.0, 4720.0, 4365.0, 5945.0, -9809656.0, 1061181.0),
    (14000.0, 5192.0, 6753.0, 9190.0, -4002417.0, 739161.0),
    (4127.0, 15789.0, 6753.0, 9190.0, 9459261.0, 711490.0),
];

pub(super) fn cloud(name: &str, w: f64, h: f64, a0: f64, a1: f64) -> Geometry {
    // Drawn in the path's own 43200-unit square, then stretched to the box (as DrawingML lays out
    // a path with its own w and h), so the arcs keep their shape's proportions at any aspect.
    let to_box = kurbo::Affine::scale_non_uniform(w / 43200.0, h / 43200.0);
    let deg = |v: f64| v / 60000.0;
    let mut b = Pb::new();
    b.m(3900.0, 14370.0);
    for (wr, hr, st, sw) in CLOUD_ARCS {
        b.arc_spec(wr, hr, deg(st), deg(sw));
    }
    b.z();
    let mut creases = Pb::new();
    for (x, y, wr, hr, st, sw) in CLOUD_CREASES {
        creases.m(x, y).arc_spec(wr, hr, deg(st), deg(sw));
    }
    let mut outline = b.take();
    outline.apply_affine(to_box);
    let mut paths = vec![fill(outline)];
    if name == "cloud" {
        let mut c = creases.take();
        c.apply_affine(to_box);
        paths.push(stroke_only(c));
    }
    let text_rect = Rect::new(w * 2977.0 / 21600.0, h * 3262.0 / 21600.0, w * 17087.0 / 21600.0, h * 17337.0 / 21600.0);
    let mut handles = vec![];
    if name == "cloudCallout" {
        let tip = Point::new(w / 2.0 + w * a0 / 1e5, h / 2.0 + h * a1 / 1e5);
        let c = Point::new(w / 2.0, h / 2.0);
        let mut bubbles = Pb::new();
        for (k, r) in [(0.55, 0.06), (0.75, 0.045), (0.95, 0.03)] {
            let p = c + (tip - c) * k;
            if !(p.x > w * 0.1 && p.x < w * 0.9 && p.y > h * 0.1 && p.y < h * 0.9) || k > 0.9 {
                bubbles.ellipse(p.x, p.y, w * r, h * r);
            }
        }
        paths.push(fill(bubbles.take()));
        handles.push(tip_handle(w, h, tip));
    }
    Geometry { paths, text_rect, handles, sites: mid_sites(w, h) }
}

/// The equation shapes, by ECMA-376's formulas (checked against PowerPoint's outlines at several
/// sizes and adjust values). Their bars span 73.49% of the width; the plus and multiply bars are
/// `adj1` of the shorter side thick, the others' `adj1` of the height.
pub(super) fn math(name: &str, w: f64, h: f64, a0: f64, a1: f64, a2: f64) -> Geometry {
    let pin = |lo: f64, v: f64, hi: f64| v.max(lo).min(hi.max(lo));
    let ss = w.min(h);
    let (hc, vc) = (w / 2.0, h / 2.0);
    let dx1 = w * 73490.0 / 200000.0;
    let (x1, x2) = (hc - dx1, hc + dx1);
    let mut b = Pb::new();
    let text = match name {
        "mathPlus" => {
            let a = pin(0.0, a0, 73490.0);
            let dy1 = h * 73490.0 / 200000.0;
            let dx2 = ss * a / 200000.0;
            let (y1, y2, y3, y4) = (vc - dy1, vc - dx2, vc + dx2, vc + dy1);
            let (xa, xb) = (hc - dx2, hc + dx2);
            b.poly(&[(x1, y2), (xa, y2), (xa, y1), (xb, y1), (xb, y2), (x2, y2), (x2, y3), (xb, y3), (xb, y4), (xa, y4), (xa, y3), (x1, y3)]);
            Rect::new(x1, y2, x2, y3)
        }
        "mathMinus" => {
            let dy1 = h * pin(0.0, a0, 100000.0) / 200000.0;
            b.poly(&[(x1, vc - dy1), (x2, vc - dy1), (x2, vc + dy1), (x1, vc + dy1)]);
            Rect::new(x1, vc - dy1, x2, vc + dy1)
        }
        "mathEqual" => {
            let a = pin(0.0, a0, 36745.0);
            let g = pin(0.0, a1, 100000.0 - 2.0 * a);
            let (dy1, dy2) = (h * a / 100000.0, h * g / 200000.0);
            let (y2, y3) = (vc - dy2, vc + dy2);
            let (y1, y4) = (y2 - dy1, y3 + dy1);
            b.poly(&[(x1, y1), (x2, y1), (x2, y2), (x1, y2)]);
            b.poly(&[(x1, y3), (x2, y3), (x2, y4), (x1, y4)]);
            Rect::new(x1, y1, x2, y4)
        }
        "mathNotEqual" => {
            let a = pin(0.0, a0, 50000.0);
            let cr_ang = pin(4200000.0, a1, 6600000.0);
            let g = pin(0.0, a2, 100000.0 - 2.0 * a);
            let (dy1, dy2) = (h * a / 100000.0, h * g / 200000.0);
            let hd2 = (h / 2.0).max(1e-9);
            let (y2, y3) = (vc - dy2, vc + dy2);
            let (y1, y4) = (y2 - dy1, y3 + dy1);
            let cadj2 = cr_ang - 5400000.0;
            let xadj2 = hd2 * (cadj2 / 60000.0).to_radians().tan();
            let len = xadj2.hypot(hd2);
            let bhw = len * dy1 / hd2;
            let x7 = hc + xadj2 - bhw / 2.0;
            let at = |y: f64| x7 - xadj2 * y / hd2;
            let (x6, x5, x4, x3) = (at(y1), at(y2), at(y3), at(y4));
            let rx7 = x7 + bhw;
            let dx7 = dy1 * hd2 / len;
            let dy3 = dy1 * xadj2 / len;
            let (rx, lx, ry, ly) = if cadj2 > 0.0 { (x7 + dx7, x7, dy3, 0.0) } else { (rx7, rx7 - dx7, 0.0, -dy3) };
            b.poly(&[
                (x1, y1),
                (x6, y1),
                (lx, ly),
                (rx, ry),
                (x6 + bhw, y1),
                (x2, y1),
                (x2, y2),
                (x5 + bhw, y2),
                (x4 + bhw, y3),
                (x2, y3),
                (x2, y4),
                (x3 + bhw, y4),
                (w - lx, h - ly),
                (w - rx, h - ry),
                (x3, y4),
                (x1, y4),
                (x1, y3),
                (x4, y3),
                (x5, y2),
                (x1, y2),
            ]);
            Rect::new(x1, y1, x2, y4)
        }
        "mathDivide" => {
            let a = pin(1000.0, a0, 36745.0);
            let max3 = ((73490.0 - a) / 4.0).min(36745.0 * w / h.max(1e-9));
            let r3 = pin(1000.0, a2, max3);
            let g = pin(0.0, a1, 73490.0 - 4.0 * r3 - a);
            let dy1 = h * a / 200000.0;
            let yg = h * g / 100000.0;
            let rad = h * r3 / 100000.0;
            let (y3, y4) = (vc - dy1, vc + dy1);
            let y2 = y3 - (yg + rad);
            let y1 = y2 - rad;
            b.ellipse(hc, y1 + rad, rad, rad);
            b.ellipse(hc, h - y1 - rad, rad, rad);
            b.poly(&[(x1, y3), (x2, y3), (x2, y4), (x1, y4)]);
            Rect::new(x1, y3, x2, y4)
        }
        _ => {
            // mathMultiply: an X whose arms run along the box's diagonals.
            let th = ss * pin(0.0, a0, 51965.0) / 100000.0;
            let ang = h.atan2(w);
            let (sa, ca, ta) = (ang.sin(), ang.cos(), ang.tan().max(1e-9));
            let lm = w.hypot(h) * (1.0 - 51965.0 / 100000.0);
            let (xm, ym) = (ca * lm / 2.0, sa * lm / 2.0);
            let (dxam, dyam) = (sa * th / 2.0, ca * th / 2.0);
            let (xa, ya, xb, yb) = (xm - dxam, ym + dyam, xm + dxam, ym - dyam);
            let yc = (hc - xb) * ta + yb;
            let (xd, xe) = (w - xb, w - xa);
            let xfe = (vc - ya) / ta;
            let (xf, xl) = (xe - xfe, xa + xfe);
            let (yg, yh, yi) = (h - ya, h - yb, h - yc);
            b.poly(&[(xa, ya), (xb, yb), (hc, yc), (xd, yb), (xe, ya), (xf, vc), (xe, yg), (xd, yh), (hc, yi), (xb, yh), (xa, yg), (xl, vc)]);
            Rect::new(xa, yb, xe, yh)
        }
    };
    Geometry { paths: vec![fill(b.take())], text_rect: text, handles: vec![], sites: mid_sites(w, h) }
}

pub(super) fn ribbon(up: bool, w: f64, h: f64, a0: f64, a1: f64) -> Geometry {
    let fold = h * a0.clamp(0.0, 33333.0) / 1e5;
    let cw = w * a1.clamp(25000.0, 75000.0) / 1e5; // centre band width
    let x0 = (w - cw) / 2.0;
    let x1 = w - x0;
    let ear = w / 8.0;
    let (top, bot) = (0.0, h);
    let mut b = Pb::new();
    let mut shade_p = Pb::new();
    if up {
        // centre band sits high; ends hang low.
        b.poly(&[(x0, top), (x1, top), (x1, bot - fold), (x0, bot - fold)]);
        let mut ends = Pb::new();
        ends.poly(&[(0.0, fold), (x0 + ear * 0.5, fold), (x0 + ear * 0.5, bot), (0.0, bot), (ear, (fold + bot) / 2.0)]);
        ends.poly(&[(w, fold), (x1 - ear * 0.5, fold), (x1 - ear * 0.5, bot), (w, bot), (w - ear, (fold + bot) / 2.0)]);
        shade_p.poly(&[(x0, bot - fold), (x0 + ear * 0.5, bot - fold), (x0 + ear * 0.5, bot)]);
        shade_p.poly(&[(x1, bot - fold), (x1 - ear * 0.5, bot - fold), (x1 - ear * 0.5, bot)]);
        let text = Rect::new(x0, top, x1, bot - fold);
        return Geometry {
            paths: vec![fill(ends.take()), shade(shade_p.take(), FillMode::DarkenLess), fill(b.take())],
            text_rect: text,
            handles: vec![],
            sites: mid_sites(w, h),
        };
    }
    b.poly(&[(x0, top + fold), (x1, top + fold), (x1, bot), (x0, bot)]);
    let mut ends = Pb::new();
    ends.poly(&[(0.0, top), (x0 + ear * 0.5, top), (x0 + ear * 0.5, bot - fold), (0.0, bot - fold), (ear, (bot - fold) / 2.0)]);
    ends.poly(&[(w, top), (x1 - ear * 0.5, top), (x1 - ear * 0.5, bot - fold), (w, bot - fold), (w - ear, (bot - fold) / 2.0)]);
    shade_p.poly(&[(x0, top + fold), (x0 + ear * 0.5, top + fold), (x0 + ear * 0.5, top)]);
    shade_p.poly(&[(x1, top + fold), (x1 - ear * 0.5, top + fold), (x1 - ear * 0.5, top)]);
    let text = Rect::new(x0, top + fold, x1, bot);
    Geometry {
        paths: vec![fill(ends.take()), shade(shade_p.take(), FillMode::DarkenLess), fill(b.take())],
        text_rect: text,
        handles: vec![],
        sites: mid_sites(w, h),
    }
}

pub(super) fn scroll(vertical: bool, w: f64, h: f64, a0: f64) -> Geometry {
    let ss = w.min(h);
    let r = ss * a0.clamp(0.0, 25000.0) / 1e5 / 2.0;
    let mut b = Pb::new();
    let mut curl = Pb::new();
    if vertical {
        b.poly(&[(r, r * 2.0), (w - r, r * 2.0), (w - r, h - r), (r, h - r)]);
        b.ellipse(w / 2.0, r, w / 2.0 - r * 0.0, r);
        curl.ellipse(r, h - r, r, r);
        curl.ellipse(w - r * 2.0, r, r * 0.5, r * 0.5);
        let text = Rect::new(r, r * 2.0, w - r, h - r);
        return Geometry {
            paths: vec![fill(b.take()), shade(curl.take(), FillMode::DarkenLess)],
            text_rect: text,
            handles: vec![],
            sites: mid_sites(w, h),
        };
    }
    b.poly(&[(r * 2.0, r), (w - r, r), (w - r, h - r), (r * 2.0, h - r)]);
    b.ellipse(r, h / 2.0, r, h / 2.0);
    curl.ellipse(w - r, r, r, r);
    curl.ellipse(r, h - r * 2.0, r * 0.5, r * 0.5);
    let text = Rect::new(r * 2.0, r, w - r, h - r);
    Geometry { paths: vec![fill(b.take()), shade(curl.take(), FillMode::DarkenLess)], text_rect: text, handles: vec![], sites: mid_sites(w, h) }
}

pub(super) fn action_button(name: &str, w: f64, h: f64) -> Geometry {
    let body = corner_rect(w, h, [Corner::Square; 4]);
    let ss = w.min(h);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let s = ss * 0.3;
    let mut g = Pb::new();
    match name {
        "actionButtonBackPrevious" => {
            g.poly(&[(cx + s, cy - s), (cx + s, cy + s), (cx - s, cy)]);
        }
        "actionButtonForwardNext" => {
            g.poly(&[(cx - s, cy - s), (cx - s, cy + s), (cx + s, cy)]);
        }
        "actionButtonBeginning" => {
            g.poly(&[(cx + s, cy - s), (cx + s, cy + s), (cx - s * 0.6, cy)]);
            g.poly(&[(cx - s, cy - s), (cx - s * 0.7, cy - s), (cx - s * 0.7, cy + s), (cx - s, cy + s)]);
        }
        "actionButtonEnd" => {
            g.poly(&[(cx - s, cy - s), (cx - s, cy + s), (cx + s * 0.6, cy)]);
            g.poly(&[(cx + s * 0.7, cy - s), (cx + s, cy - s), (cx + s, cy + s), (cx + s * 0.7, cy + s)]);
        }
        "actionButtonHome" => {
            g.poly(&[
                (cx, cy - s),
                (cx + s, cy),
                (cx + s * 0.7, cy),
                (cx + s * 0.7, cy + s),
                (cx - s * 0.7, cy + s),
                (cx - s * 0.7, cy),
                (cx - s, cy),
            ]);
        }
        "actionButtonInformation" | "actionButtonHelp" => {
            g.ellipse(cx, cy, s, s);
        }
        "actionButtonReturn" => {
            g.m(cx + s, cy - s * 0.6).l(cx + s, cy + s * 0.2).q(cx + s, cy + s * 0.8, cx + s * 0.4, cy + s * 0.8).l(cx - s * 0.5, cy + s * 0.8);
            g.l(cx - s * 0.5, cy + s * 0.4)
                .l(cx - s, cy + s * 0.9)
                .l(cx - s * 0.5, cy + s * 1.4)
                .l(cx - s * 0.5, cy + s)
                .l(cx + s * 0.6, cy + s)
                .l(cx + s * 0.6, cy - s * 0.6)
                .z();
        }
        "actionButtonMovie" => {
            g.poly(&[(cx - s, cy - s * 0.6), (cx + s * 0.4, cy - s * 0.6), (cx + s * 0.4, cy + s * 0.6), (cx - s, cy + s * 0.6)]);
            g.poly(&[(cx + s * 0.4, cy), (cx + s, cy - s * 0.5), (cx + s, cy + s * 0.5)]);
        }
        "actionButtonDocument" => {
            g.poly(&[(cx - s * 0.7, cy - s), (cx + s * 0.3, cy - s), (cx + s * 0.7, cy - s * 0.6), (cx + s * 0.7, cy + s), (cx - s * 0.7, cy + s)]);
        }
        "actionButtonSound" => {
            g.poly(&[
                (cx - s, cy - s * 0.35),
                (cx - s * 0.5, cy - s * 0.35),
                (cx, cy - s),
                (cx, cy + s),
                (cx - s * 0.5, cy + s * 0.35),
                (cx - s, cy + s * 0.35),
            ]);
        }
        _ => {}
    }
    Geometry {
        paths: vec![fill(body), shade(g.take(), FillMode::DarkenLess)],
        text_rect: Rect::new(0.0, 0.0, w, h),
        handles: vec![],
        sites: mid_sites(w, h),
    }
}
