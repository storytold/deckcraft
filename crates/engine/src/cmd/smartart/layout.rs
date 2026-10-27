use super::diagrams::{hierarchy, org_chart};
use super::nodes::{Item, groups};
use super::paint::{accent, deco, line_shape, paint, picture_shape, text_shape};
use super::*;

pub(super) fn layout_raw(d: &Diagram, area: Xfrm) -> Vec<Item> {
    let gs = groups(d);
    let nodes = |g: &[usize]| -> Vec<&DiagramNode> { g.iter().filter_map(|i| d.nodes.get(*i)).collect() };
    let n = gs.len().max(1);
    let nf = n as f64;
    let mut out: Vec<Item> = vec![];
    // A titled shape for top-level group `i`, painted in its accent.
    let titled = |out: &mut Vec<Item>, preset: &str, x: Xfrm, i: usize, g: &[usize]| {
        let mut sh = text_shape(preset, x, &nodes(g), true);
        paint(d, &mut sh, i, false);
        out.push((sh, g.to_vec()));
    };
    let arrow = |out: &mut Vec<Item>, x: Xfrm, rot: f64, i: usize| {
        let mut sh = deco("rightArrow", x, rot);
        paint(d, &mut sh, i, true);
        out.push((sh, vec![]));
    };
    match d.kind.as_str() {
        "hierarchy" => return hierarchy(d, area, false),
        "horizontalHierarchy" => return hierarchy(d, area, true),
        "orgChart" | "halfCircleOrgChart" | "nameTitleOrgChart" => return org_chart(d, area),
        "list" | "hBulletList" => {
            // Vertical / Horizontal Bullet List: a coloured title bar, sub-points in a pale box below.
            let vertical = d.kind == "list";
            for (i, g) in gs.iter().enumerate() {
                let (cell, bar_h) = if vertical {
                    (
                        Xfrm::new(area.x, area.y + i as f64 * area.h / nf, area.w, area.h / nf * 0.92),
                        (area.h / nf * 0.92).min(area.h * 0.12).max(18.0),
                    )
                } else {
                    let w = area.w / nf;
                    (Xfrm::new(area.x + i as f64 * w + w * 0.03, area.y, w * 0.94, area.h), area.h * 0.22)
                };
                let has_sub = g.len() > 1;
                let bar_h = bar_h.min(cell.h);
                titled(&mut out, "roundRect", Xfrm::new(cell.x, cell.y, cell.w, bar_h), i, &g[..1]);
                if has_sub {
                    let pad = cell.w * 0.03;
                    let x = Xfrm::new(cell.x + pad, cell.y + bar_h, cell.w - 2.0 * pad, (cell.h - bar_h).max(1.0));
                    let mut sh = text_shape("rect", x, &nodes(&g[1..]), false);
                    paint(d, &mut sh, i, true);
                    out.push((sh, g[1..].to_vec()));
                }
            }
        }
        "blockList" => {
            // Basic Block List: equal blocks in rows, as square as the area allows.
            let cols = ((nf * area.w / area.h.max(1.0) / 1.6).sqrt().ceil() as usize).clamp(1, n);
            let rows = n.div_ceil(cols);
            let (w, h) = (area.w / cols as f64, area.h / rows as f64);
            let (bw, bh) = (w * 0.9, (h * 0.85).min(w * 0.9 * 0.6));
            for (i, g) in gs.iter().enumerate() {
                let (c, r) = (i % cols, i / cols);
                let in_row = if r + 1 == rows { n - r * cols } else { cols };
                let x0 = area.x + (area.w - in_row as f64 * w) / 2.0;
                titled(&mut out, "rect", Xfrm::new(x0 + c as f64 * w + (w - bw) / 2.0, area.y + r as f64 * h + (h - bh) / 2.0, bw, bh), i, g);
            }
        }
        "process" | "chevron" => {
            let chevron = d.kind == "chevron";
            let gap = if chevron { -area.w * 0.02 } else { area.w * 0.06 };
            let w = ((area.w - gap * (nf - 1.0)) / nf).max(1.0);
            let h = (area.h * 0.5).min(w * if chevron { 0.45 } else { 0.65 });
            let y = area.y + (area.h - h) / 2.0;
            for (i, g) in gs.iter().enumerate() {
                let x = area.x + i as f64 * (w + gap);
                titled(&mut out, if chevron { "chevron" } else { "roundRect" }, Xfrm::new(x, y, w, h), i, g);
                if !chevron && i + 1 < n {
                    let aw = gap * 0.7;
                    arrow(&mut out, Xfrm::new(x + w + gap * 0.15, y + h / 2.0 - aw * 0.45, aw, aw * 0.9), 0.0, i);
                }
            }
        }
        "verticalProcess" => {
            let gap = area.h * 0.06;
            let h = ((area.h - gap * (nf - 1.0)) / nf).max(1.0);
            let w = area.w * 0.6;
            let x = area.x + (area.w - w) / 2.0;
            for (i, g) in gs.iter().enumerate() {
                let y = area.y + i as f64 * (h + gap);
                titled(&mut out, "roundRect", Xfrm::new(x, y, w, h), i, g);
                if i + 1 < n {
                    let s = gap * 0.8;
                    // A right arrow turned to point down.
                    arrow(&mut out, Xfrm::new(area.x + area.w / 2.0 - s / 2.0, y + h + gap * 0.1, s, s), 90.0, i);
                }
            }
        }
        "stepUp" => {
            // Step Up Process: each step higher than the one before.
            let w = area.w / nf;
            let h = area.h / (nf + 1.0) * 1.6;
            for (i, g) in gs.iter().enumerate() {
                let y = area.y + area.h - h - i as f64 * (area.h - h) / (nf - 1.0).max(1.0);
                titled(&mut out, "rect", Xfrm::new(area.x + i as f64 * w + w * 0.04, y, w * 0.92, h), i, g);
            }
        }
        "cycle" | "blockCycle" => {
            let block = d.kind == "blockCycle";
            let r = area.h.min(area.w) * 0.36;
            let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
            let dd = (r * if block { 0.75 } else { 0.8 }).min(2.0 * std::f64::consts::PI * r / nf * 0.62);
            for (i, g) in gs.iter().enumerate() {
                let a = (i as f64 / nf * 360.0 - 90.0).to_radians();
                let (w, h) = if block { (dd * 1.3, dd * 0.75) } else { (dd, dd) };
                titled(
                    &mut out,
                    if block { "roundRect" } else { "ellipse" },
                    Xfrm::new(cx + r * a.cos() - w / 2.0, cy + r * a.sin() - h / 2.0, w, h),
                    i,
                    g,
                );
                if n > 1 {
                    // An arrow halfway to the next node, along the circle.
                    let m = ((i as f64 + 0.5) / nf * 360.0 - 90.0).to_radians();
                    let s = (dd * 0.35).max(6.0);
                    let rot = (i as f64 + 0.5) / nf * 360.0;
                    arrow(&mut out, Xfrm::new(cx + r * m.cos() - s / 2.0, cy + r * m.sin() - s * 0.4, s, s * 0.8), rot, i);
                }
            }
        }
        "radial" => {
            // Basic Radial: the first node in the middle, the others around it with spokes.
            let Some((centre, rest)) = gs.split_first() else { return out };
            let m = rest.len().max(1) as f64;
            let r = area.h.min(area.w) * 0.36;
            let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
            let dc = r * 0.75;
            let dd = (r * 0.75).min(2.0 * std::f64::consts::PI * r / m * 0.7);
            for (k, _) in rest.iter().enumerate() {
                let a = (k as f64 / m * 360.0 - 90.0).to_radians();
                let (ex, ey) = (cx + (r - dd / 2.0) * a.cos(), cy + (r - dd / 2.0) * a.sin());
                out.push((line_shape((cx + dc / 2.0 * a.cos(), cy + dc / 2.0 * a.sin()), (ex, ey)), vec![]));
            }
            titled(&mut out, "ellipse", Xfrm::new(cx - dc / 2.0, cy - dc / 2.0, dc, dc), 0, centre);
            for (k, g) in rest.iter().enumerate() {
                let a = (k as f64 / m * 360.0 - 90.0).to_radians();
                titled(&mut out, "ellipse", Xfrm::new(cx + r * a.cos() - dd / 2.0, cy + r * a.sin() - dd / 2.0, dd, dd), k + 1, g);
            }
        }
        "pyramid" | "invertedPyramid" => {
            // One triangle cut into bands; inverted, the wide band is on top. Each band is drawn as
            // its own polygon so the sides keep the triangle's slope and the text stays upright.
            let inverted = d.kind == "invertedPyramid";
            let h = area.h / nf;
            let pw = area.h.min(area.w) * 1.15;
            let px = area.x + (area.w - pw) / 2.0;
            for (i, g) in gs.iter().enumerate() {
                let (a, b) = (i as f64 / nf, (i as f64 + 1.0) / nf);
                let (t, bt) = if inverted { (1.0 - a, 1.0 - b) } else { (a, b) };
                let (wt, wb) = (pw * t, pw * bt);
                let w = wt.max(wb).max(1.0);
                let bh = h * 0.96;
                let x = px + (pw - w) / 2.0;
                let (it, ib) = ((w - wt) / 2.0, (w - wb) / 2.0);
                let path = format!("M {it:.2} 0 L {:.2} 0 L {:.2} {bh:.2} L {ib:.2} {bh:.2} Z", w - it, w - ib);
                let mut sh = text_shape("rect", Xfrm::new(x, area.y + i as f64 * h, w, bh), &nodes(g), true);
                sh.geom = Geom::Custom {
                    paths: vec![deckcraft_model::CustomPath { w, h: bh, d: path, fill: deckcraft_geom::preset::FillMode::Norm, stroke: true }],
                };
                // Keep the text inside the sloped sides: inset it to a bit past the band's mid width.
                // The tip band is a triangle: its text sits at the wide end, inset to the width there.
                let tip = wt.min(wb) < 1e-6;
                let inset = if tip { w * 0.22 } else { (it + ib) / 2.0 + (it - ib).abs() / 4.0 + 4.0 };
                if let Some(b) = &mut sh.text {
                    b.body.inset_l = Some(inset);
                    b.body.inset_r = Some(inset);
                    if tip {
                        b.body.anchor = Some(if inverted { deckcraft_model::text::Anchor::Top } else { deckcraft_model::text::Anchor::Bottom });
                    }
                }
                paint(d, &mut sh, i, false);
                out.push((sh, g.clone()));
            }
        }
        "matrix" | "titledMatrix" => {
            let titled_m = d.kind == "titledMatrix";
            let s = area.h.min(area.w);
            let (x0, y0) = (area.x + (area.w - s) / 2.0, area.y + (area.h - s) / 2.0);
            let (quads, title): (&[Vec<usize>], Option<&Vec<usize>>) =
                if titled_m { gs.split_first().map(|(t, r)| (r, Some(t))).unwrap_or((&[], None)) } else { (&gs, None) };
            for (i, g) in quads.iter().take(4).enumerate() {
                let x = Xfrm::new(x0 + (i % 2) as f64 * s / 2.0 + 3.0, y0 + (i / 2) as f64 * s / 2.0 + 3.0, s / 2.0 - 6.0, s / 2.0 - 6.0);
                titled(&mut out, "roundRect", x, i + usize::from(titled_m), g);
            }
            if let Some(t) = title {
                let c = s * 0.42;
                let mut sh = text_shape("roundRect", Xfrm::new(x0 + (s - c) / 2.0, y0 + (s - c * 0.6) / 2.0, c, c * 0.6), &nodes(t), true);
                paint(d, &mut sh, 0, true);
                out.push((sh, t.clone()));
            }
        }
        "pictureList" => {
            // Vertical Picture List: a picture square at the left of each row, the text beside it.
            let h = area.h / nf;
            let bh = h * 0.88;
            for (i, g) in gs.iter().enumerate() {
                let y = area.y + i as f64 * h;
                let s = bh.min(area.w * 0.3);
                out.push((picture_shape(d, "rect", Xfrm::new(area.x, y, s, bh), i, g[0]), vec![]));
                titled(&mut out, "roundRect", Xfrm::new(area.x + s + bh * 0.08, y, (area.w - s - bh * 0.08).max(1.0), bh), i, g);
            }
        }
        "pictureCaption" | "pictureProcess" => {
            // Picture Caption List: a picture over each caption, in rows. Picture Accent Process: one
            // row, arrows between the steps.
            let process = d.kind == "pictureProcess";
            let cols = if process { n } else { ((nf * area.w / area.h.max(1.0) / 1.2).sqrt().ceil() as usize).clamp(1, n) };
            let rows = n.div_ceil(cols);
            let gap = if process { area.w * 0.05 } else { 0.0 };
            let w = (area.w - gap * (cols as f64 - 1.0)) / cols as f64;
            let h = area.h / rows as f64;
            let bw = if process { w } else { w * 0.9 };
            let ch = (h * 0.9).min(bw * 1.1);
            for (i, g) in gs.iter().enumerate() {
                let (c, r) = (i % cols, i / cols);
                let in_row = if r + 1 == rows { n - r * cols } else { cols };
                let x0 = area.x + (area.w - in_row as f64 * w - gap * (in_row as f64 - 1.0)) / 2.0;
                let x = x0 + c as f64 * (w + gap) + (w - bw) / 2.0;
                let y = area.y + r as f64 * h + (h - ch) / 2.0;
                let ph = ch * 0.62;
                out.push((
                    picture_shape(d, if process { "ellipse" } else { "rect" }, Xfrm::new(x + (bw - ph.min(bw)) / 2.0, y, ph.min(bw), ph), i, g[0]),
                    vec![],
                ));
                titled(&mut out, if process { "roundRect" } else { "rect" }, Xfrm::new(x, y + ph + ch * 0.03, bw, ch * 0.35), i, g);
                if process && i + 1 < n {
                    let a = (gap * 0.7).min(ch * 0.2);
                    arrow(&mut out, Xfrm::new(x + bw + (gap - a) / 2.0, y + ph / 2.0 - a / 2.0, a, a), 0.0, i);
                }
            }
        }
        "textCycle" | "continuousCycle" => {
            // Text Cycle: the text alone around a circle, arrows between. Continuous Cycle: boxes on
            // a ring.
            let text_only = d.kind == "textCycle";
            let r = area.h.min(area.w) * 0.36;
            let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
            let dd = (r * 0.8).min(2.0 * std::f64::consts::PI * r / nf * 0.62);
            if !text_only {
                let ring = r * 2.0 + dd * 0.3;
                let mut sh = deco("donut", Xfrm::new(cx - ring / 2.0, cy - ring / 2.0, ring, ring), 0.0);
                sh.geom = Geom::Preset { name: "donut".into(), adj: vec![7000.0] };
                paint(d, &mut sh, 0, true);
                out.push((sh, vec![]));
            }
            for (i, g) in gs.iter().enumerate() {
                let a = (i as f64 / nf * 360.0 - 90.0).to_radians();
                let (w, h) = (dd * 1.35, dd * 0.7);
                let mut sh = text_shape("roundRect", Xfrm::new(cx + r * a.cos() - w / 2.0, cy + r * a.sin() - h / 2.0, w, h), &nodes(g), true);
                paint(d, &mut sh, i, false);
                if text_only {
                    sh.fill = Some(Fill::None);
                    sh.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                    sh.effects = None;
                    let c = ColorRef::scheme(accent(d, i)).with(ColorTransform::LumMod(75_000));
                    if let Some(b) = &mut sh.text {
                        for p in &mut b.paragraphs {
                            for r in &mut p.runs {
                                r.props.fill = Some(Fill::solid(c.clone()));
                            }
                        }
                    }
                    if n > 1 {
                        let m = ((i as f64 + 0.5) / nf * 360.0 - 90.0).to_radians();
                        let s = (dd * 0.3).max(6.0);
                        arrow(
                            &mut out,
                            Xfrm::new(cx + r * m.cos() - s / 2.0, cy + r * m.sin() - s * 0.4, s, s * 0.8),
                            (i as f64 + 0.5) / nf * 360.0,
                            i,
                        );
                    }
                }
                out.push((sh, g.clone()));
            }
        }
        "segmentedCycle" | "pie" => {
            // Segmented Cycle: a ring cut into one arc per node. Basic Pie: a disc cut into wedges.
            // The text sits in a clear box over each segment's middle.
            let ring = d.kind == "segmentedCycle";
            let s = area.h.min(area.w);
            let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
            let gap = if ring { 2.0 } else { 0.0 };
            for (i, g) in gs.iter().enumerate() {
                // Angles clockwise from 3 o'clock, starting at the top.
                let a0 = i as f64 / nf * 360.0 - 90.0 + gap / 2.0;
                let a1 = (i as f64 + 1.0) / nf * 360.0 - 90.0 - gap / 2.0;
                let mut sh = deco(if ring { "blockArc" } else { "pie" }, Xfrm::new(cx - s / 2.0, cy - s / 2.0, s, s), 0.0);
                let ang = |a: f64| (a.rem_euclid(360.0) * 60_000.0).round();
                // blockArc runs from its first angle to its second clockwise; so does pie.
                sh.geom = Geom::Preset {
                    name: if ring { "blockArc" } else { "pie" }.into(),
                    adj: if ring { vec![ang(a0), ang(a1), 25_000.0] } else { vec![ang(a0), ang(a1)] },
                };
                paint(d, &mut sh, i, false);
                out.push((sh, vec![]));
                let m = ((a0 + a1) / 2.0).to_radians();
                let rr = if ring { s * 0.4375 } else { s * 0.3 };
                let tw = (s * if ring { 0.22 } else { 0.3 }).min(std::f64::consts::PI * 2.0 * rr / nf * 0.9);
                let th = s * 0.16;
                let mut t = text_shape("rect", Xfrm::new(cx + rr * m.cos() - tw / 2.0, cy + rr * m.sin() - th / 2.0, tw, th), &nodes(g), true);
                paint(d, &mut t, i, false);
                t.fill = Some(Fill::None);
                t.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                t.effects = None;
                out.push((t, g.clone()));
            }
        }
        "multidirectionalCycle" => {
            // Multidirectional Cycle: nodes around a circle, double-headed arrows between them.
            let r = area.h.min(area.w) * 0.36;
            let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
            let dd = (r * 0.75).min(2.0 * std::f64::consts::PI * r / nf * 0.55);
            for (i, g) in gs.iter().enumerate() {
                let a = (i as f64 / nf * 360.0 - 90.0).to_radians();
                titled(&mut out, "roundRect", Xfrm::new(cx + r * a.cos() - dd * 0.65, cy + r * a.sin() - dd * 0.4, dd * 1.3, dd * 0.8), i, g);
                if n > 1 {
                    let m = ((i as f64 + 0.5) / nf * 360.0 - 90.0).to_radians();
                    let s = (dd * 0.5).max(8.0);
                    let mut sh = deco(
                        "leftRightArrow",
                        Xfrm::new(cx + r * m.cos() - s / 2.0, cy + r * m.sin() - s * 0.2, s, s * 0.4),
                        (i as f64 + 0.5) / nf * 360.0,
                    );
                    paint(d, &mut sh, i, true);
                    out.push((sh, vec![]));
                }
            }
        }
        "cycleMatrix" => {
            // Cycle Matrix: four quarters of a circle in the middle with the top-level text, each
            // quarter's sub-points in a box at its corner.
            let s = area.h.min(area.w);
            let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
            let c = s * 0.56;
            for (i, g) in gs.iter().take(4).enumerate() {
                let (sx, sy) = ([-1.0, 1.0, 1.0, -1.0][i], [-1.0, -1.0, 1.0, 1.0][i]);
                let b = s * 0.4;
                let bx = Xfrm::new(cx + if sx < 0.0 { -s / 2.0 } else { s / 2.0 - b }, cy + if sy < 0.0 { -s / 2.0 } else { s / 2.0 - b }, b, b);
                if g.len() > 1 {
                    let mut sh = text_shape("roundRect", bx, &nodes(&g[1..]), false);
                    paint(d, &mut sh, i, true);
                    if let Some(body) = &mut sh.text {
                        // Text in the box's outer part, clear of the circle.
                        body.body.inset_l = Some(if sx > 0.0 { b * 0.35 } else { 6.0 });
                        body.body.inset_r = Some(if sx < 0.0 { b * 0.35 } else { 6.0 });
                        body.body.anchor = Some(if sy < 0.0 { deckcraft_model::text::Anchor::Top } else { deckcraft_model::text::Anchor::Bottom });
                    }
                    out.push((sh, g[1..].to_vec()));
                }
            }
            for (i, g) in gs.iter().take(4).enumerate() {
                let (sx, sy) = ([-1.0, 1.0, 1.0, -1.0][i], [-1.0, -1.0, 1.0, 1.0][i]);
                let a0 = [180.0, 270.0, 0.0, 90.0][i] + 1.5;
                let mut sh = deco("pie", Xfrm::new(cx - c / 2.0, cy - c / 2.0, c, c), 0.0);
                sh.geom = Geom::Preset { name: "pie".into(), adj: vec![a0 * 60_000.0, (a0 + 87.0) * 60_000.0] };
                paint(d, &mut sh, i, false);
                out.push((sh, vec![]));
                let q = c * 0.3;
                let mut t = text_shape("rect", Xfrm::new(cx + sx * c * 0.2 - q / 2.0, cy + sy * c * 0.2 - q / 2.0, q, q), &nodes(&g[..1]), true);
                paint(d, &mut t, i, false);
                t.fill = Some(Fill::None);
                t.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                t.effects = None;
                out.push((t, g[..1].to_vec()));
            }
        }
        "segmentedPyramid" => {
            // Segmented Pyramid: a triangle cut into rows of triangles (1, 3, 5, …), one per node,
            // every other one upside down.
            let rows = (nf.sqrt().ceil() as usize).max(1);
            let s = area.h.min(area.w / 1.15);
            let pw = s * 1.15;
            let (x0, y0) = (area.x + (area.w - pw) / 2.0, area.y + (area.h - s) / 2.0);
            let (tw, th) = (pw / rows as f64, s / rows as f64);
            let mut k = 0;
            'rows: for r in 0..rows {
                let left = x0 + pw / 2.0 - (r as f64 + 1.0) * tw / 2.0;
                for c in 0..(2 * r + 1) {
                    let Some(g) = gs.get(k) else { break 'rows };
                    let x = Xfrm::new(left + c as f64 * tw / 2.0, y0 + r as f64 * th, tw, th);
                    let mut sh = text_shape("rect", x, &nodes(g), true);
                    // Drawn as a polygon (not a flipped preset) so upside-down text stays upright.
                    let path = if c % 2 == 1 {
                        format!("M 0 0 L {tw:.2} 0 L {:.2} {th:.2} Z", tw / 2.0)
                    } else {
                        format!("M {:.2} 0 L {tw:.2} {th:.2} L 0 {th:.2} Z", tw / 2.0)
                    };
                    sh.geom = Geom::Custom {
                        paths: vec![deckcraft_model::CustomPath {
                            w: tw,
                            h: th,
                            d: path,
                            fill: deckcraft_geom::preset::FillMode::Norm,
                            stroke: true,
                        }],
                    };
                    if let Some(b) = &mut sh.text {
                        // The text in the triangle's wide half.
                        b.body.inset_l = Some(tw * 0.22);
                        b.body.inset_r = Some(tw * 0.22);
                        if c % 2 == 1 {
                            b.body.anchor = Some(deckcraft_model::text::Anchor::Top);
                            b.body.inset_b = Some(th * 0.35);
                        } else {
                            b.body.anchor = Some(deckcraft_model::text::Anchor::Bottom);
                            b.body.inset_t = Some(th * 0.35);
                        }
                    }
                    paint(d, &mut sh, k, false);
                    out.push((sh, g.clone()));
                    k += 1;
                }
            }
        }
        "verticalBoxList" => {
            // Vertical Box List: a pale box per node with its sub-points, its title on a tab
            // overlapping the box's top left.
            let h = area.h / nf;
            for (i, g) in gs.iter().enumerate() {
                let y = area.y + i as f64 * h;
                let tab_h = (h * 0.3).min(40.0);
                let mut bx = text_shape("rect", Xfrm::new(area.x, y + tab_h / 2.0, area.w, h * 0.9 - tab_h / 2.0), &nodes(&g[1..]), false);
                paint(d, &mut bx, i, true);
                if let Some(b) = &mut bx.text {
                    b.body.inset_t = Some(tab_h * 0.6);
                }
                out.push((bx, g[1..].to_vec()));
                titled(&mut out, "roundRect", Xfrm::new(area.x + area.w * 0.05, y, area.w * 0.6, tab_h), i, &g[..1]);
            }
        }
        "trapezoidList" | "tableList" => {
            // Trapezoid List: a column per node, each a trapezoid with its sub-points. Table List: the
            // first node as a header bar over a column per other node.
            let table = d.kind == "tableList";
            let (head, cols): (Option<&Vec<usize>>, &[Vec<usize>]) =
                if table { gs.split_first().map(|(h, r)| (Some(h), r)).unwrap_or((None, &[])) } else { (None, &gs) };
            let top = if head.is_some() { area.h * 0.28 } else { 0.0 };
            if let Some(h) = head {
                titled(&mut out, "rect", Xfrm::new(area.x, area.y, area.w, top * 0.9), 0, h);
            }
            let m = cols.len().max(1) as f64;
            let w = area.w / m;
            for (k, g) in cols.iter().enumerate() {
                let i = k + usize::from(table);
                let x = Xfrm::new(area.x + k as f64 * w + w * 0.02, area.y + top, w * 0.96, area.h - top);
                let mut sh = text_shape(if table { "rect" } else { "trapezoid" }, x, &nodes(g), true);
                if let Some(b) = &mut sh.text {
                    b.body.anchor = Some(deckcraft_model::text::Anchor::Top);
                    if !table {
                        b.body.inset_l = Some(x.w * 0.2);
                        b.body.inset_r = Some(x.w * 0.2);
                        b.body.inset_t = Some(x.h * 0.08);
                    }
                }
                paint(d, &mut sh, i, table);
                out.push((sh, g.clone()));
            }
        }
        "accentProcess" => {
            // Accent Process: a title box per step with its sub-points in a pale box below and to the
            // right, arrows between the titles.
            let gap = area.w * 0.05;
            let w = (area.w - gap * (nf - 1.0)) / nf;
            for (i, g) in gs.iter().enumerate() {
                let x = area.x + i as f64 * (w + gap);
                let th = area.h * 0.22;
                if g.len() > 1 {
                    let mut sh = text_shape("rect", Xfrm::new(x + w * 0.12, area.y + th * 0.7, w * 0.88, area.h - th * 0.7), &nodes(&g[1..]), false);
                    paint(d, &mut sh, i, true);
                    if let Some(b) = &mut sh.text {
                        b.body.inset_t = Some(th * 0.45);
                    }
                    out.push((sh, g[1..].to_vec()));
                }
                titled(&mut out, "roundRect", Xfrm::new(x, area.y, w * 0.85, th), i, &g[..1]);
                if i + 1 < n {
                    let a = gap * 0.8;
                    arrow(&mut out, Xfrm::new(x + w + (gap - a) / 2.0 - w * 0.07, area.y + th / 2.0 - a / 2.0, a, a), 0.0, i);
                }
            }
        }
        "continuousBlockProcess" => {
            // Continuous Block Process: one big pale arrow behind a row of boxes.
            let mut big = deco("rightArrow", Xfrm::new(area.x, area.y + area.h * 0.15, area.w, area.h * 0.7), 0.0);
            paint(d, &mut big, 0, true);
            out.push((big, vec![]));
            let w = area.w * 0.85 / nf;
            let h = area.h * 0.32;
            for (i, g) in gs.iter().enumerate() {
                let x = area.x + area.w * 0.02 + i as f64 * w;
                titled(&mut out, "roundRect", Xfrm::new(x + w * 0.05, area.y + (area.h - h) / 2.0, w * 0.9, h), i, g);
            }
        }
        "bendingProcess" => {
            // Basic Bending Process: rows of boxes read left to right, wrapping, arrows between.
            let cols = ((nf * area.w / area.h.max(1.0) / 1.5).sqrt().ceil() as usize).clamp(1, n);
            let rows = n.div_ceil(cols);
            let (cw, rh) = (area.w / cols as f64, area.h / rows as f64);
            let (bw, bh) = (cw * 0.72, (rh * 0.75).min(cw * 0.72 * 0.6));
            for (i, g) in gs.iter().enumerate() {
                let (c, r) = (i % cols, i / cols);
                let x = area.x + c as f64 * cw;
                let y = area.y + r as f64 * rh + (rh - bh) / 2.0;
                titled(&mut out, "rect", Xfrm::new(x, y, bw, bh), i, g);
                if i + 1 < n {
                    let a = (cw - bw) * 0.7;
                    if c + 1 < cols {
                        arrow(&mut out, Xfrm::new(x + bw + (cw - bw - a) / 2.0, y + bh / 2.0 - a / 2.0, a, a), 0.0, i);
                    } else {
                        // The bend: down from the row's last box towards the next row's first.
                        let a = (rh - bh).min(a * 1.5).max(6.0);
                        arrow(&mut out, Xfrm::new(x + bw / 2.0 - a / 2.0, y + bh + (rh - bh - a) / 2.0, a, a), 90.0, i);
                    }
                }
            }
        }
        "linearVenn" | "stackedVenn" => {
            // Linear Venn: overlapping circles in a row. Stacked Venn: circles nested from the bottom,
            // the text near each one's top.
            let linear = d.kind == "linearVenn";
            if linear {
                let dc = (area.w / (nf * 0.75 + 0.25)).min(area.h);
                let step = dc * 0.75;
                let x0 = area.x + (area.w - (step * (nf - 1.0) + dc)) / 2.0;
                for (i, g) in gs.iter().enumerate() {
                    let mut sh = text_shape("ellipse", Xfrm::new(x0 + i as f64 * step, area.y + (area.h - dc) / 2.0, dc, dc), &nodes(g), true);
                    paint(d, &mut sh, i, false);
                    if let Some(Fill::Solid { color: c }) = &mut sh.fill {
                        *c = c.clone().with(ColorTransform::Alpha(55_000));
                    }
                    out.push((sh, g.clone()));
                }
            } else {
                let s = area.h.min(area.w);
                let (cx, bottom) = (area.x + area.w / 2.0, area.y + (area.h + s) / 2.0);
                for (i, _) in gs.iter().enumerate() {
                    let dc = s * (1.0 - i as f64 / nf);
                    let mut sh = deco("ellipse", Xfrm::new(cx - dc / 2.0, bottom - dc, dc, dc), 0.0);
                    paint(d, &mut sh, i, false);
                    out.push((sh, vec![]));
                }
                // Each circle's text in the band between its top and the next circle's.
                let band = s / nf;
                for (i, g) in gs.iter().enumerate() {
                    let dc = s * (1.0 - i as f64 / nf);
                    let top = bottom - dc;
                    let h = if i + 1 < n { band } else { dc };
                    let w = dc * if i + 1 < n { 0.6 } else { 0.7 };
                    let mut t =
                        text_shape("rect", Xfrm::new(cx - w / 2.0, top + if i + 1 < n { band * 0.05 } else { 0.0 }, w, h * 0.9), &nodes(g), true);
                    paint(d, &mut t, i, false);
                    t.fill = Some(Fill::None);
                    t.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                    t.effects = None;
                    out.push((t, g.clone()));
                }
            }
        }
        "nestedTarget" => {
            // Nested Target: rounded boxes nested inside each other, the text on each one's top band.
            let band_x = area.w / 2.0 / nf * 0.6;
            let band_y = area.h / nf * 0.9 / 2.0;
            for (i, _) in gs.iter().enumerate() {
                let (dx, dy) = (band_x * i as f64, band_y * i as f64);
                let mut sh = deco("roundRect", Xfrm::new(area.x + dx, area.y + dy, area.w - 2.0 * dx, area.h - 2.0 * dy), 0.0);
                paint(d, &mut sh, i, false);
                out.push((sh, vec![]));
            }
            for (i, g) in gs.iter().enumerate() {
                let (dx, dy) = (band_x * i as f64, band_y * i as f64);
                let h = if i + 1 < n { band_y } else { area.h - 2.0 * dy };
                let mut t = text_shape(
                    "rect",
                    Xfrm::new(area.x + dx + band_x * 0.3, area.y + dy, (area.w - 2.0 * dx - band_x * 0.6).max(1.0), h),
                    &nodes(g),
                    true,
                );
                paint(d, &mut t, i, false);
                t.fill = Some(Fill::None);
                t.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                t.effects = None;
                out.push((t, g.clone()));
            }
        }
        "gridMatrix" => {
            // Grid Matrix: four quadrants on crossed axes.
            let s = area.h.min(area.w);
            let (x0, y0) = (area.x + (area.w - s) / 2.0, area.y + (area.h - s) / 2.0);
            let t = s * 0.06;
            for (preset, x) in [("leftRightArrow", Xfrm::new(x0, y0 + (s - t) / 2.0, s, t)), ("upDownArrow", Xfrm::new(x0 + (s - t) / 2.0, y0, t, s))]
            {
                let mut sh = deco(preset, x, 0.0);
                paint(d, &mut sh, 0, true);
                out.push((sh, vec![]));
            }
            let q = s / 2.0 - t * 1.2;
            for (i, g) in gs.iter().take(4).enumerate() {
                let (c, r) = ((i % 2) as f64, (i / 2) as f64);
                let x = Xfrm::new(x0 + c * (s / 2.0 + t * 0.7) + t * 0.5 * (1.0 - c), y0 + r * (s / 2.0 + t * 0.7) + t * 0.5 * (1.0 - r), q, q);
                titled(&mut out, "rect", x, i, g);
            }
        }
        "gear" => {
            // Gear: up to three interlocking gears, the first the biggest.
            let s = area.h.min(area.w * 0.62);
            let (x0, y0) = (area.x + (area.w - s / 0.62) / 2.0, area.y + (area.h - s) / 2.0);
            let spots = [(0.0, 0.18, 0.66, "gear9"), (0.5, 0.0, 0.48, "gear6"), (0.56, 0.48, 0.42, "gear6")];
            for (i, g) in gs.iter().take(3).enumerate() {
                let (fx, fy, fs, preset) = spots[i];
                let z = s * fs;
                titled(&mut out, preset, Xfrm::new(x0 + fx * s * 1.3, y0 + fy * s, z, z), i, g);
            }
        }
        "funnel" => {
            // Funnel: the ideas poured in at the top (up to three), the last one coming out below.
            let s = area.h.min(area.w);
            let (cx, y0) = (area.x + area.w / 2.0, area.y);
            let fw = s * 0.8;
            let fh = s * 0.62;
            let mut f = deco("funnel", Xfrm::new(cx - fw / 2.0, y0, fw, fh), 0.0);
            paint(d, &mut f, 0, true);
            out.push((f, vec![]));
            let (inside, last) = match gs.split_last() {
                Some((l, r)) if !r.is_empty() => (r, Some(l)),
                _ => (&gs[..], None),
            };
            let k = inside.len().min(3);
            let dc = fw * 0.27;
            let spots = [(0.0, 0.04), (-0.2, 0.3), (0.2, 0.3)];
            for (i, g) in inside.iter().take(3).enumerate() {
                let (dx, dy) = if k == 1 { (0.0, 0.15) } else { spots[i] };
                titled(&mut out, "ellipse", Xfrm::new(cx + dx * fw - dc / 2.0, y0 + dy * fh + fh * 0.05, dc, dc), i, g);
            }
            if let Some(g) = last {
                let a = s * 0.12;
                let mut ar = deco("downArrow", Xfrm::new(cx - a / 2.0, y0 + fh, a, a * 1.2), 0.0);
                paint(d, &mut ar, k, true);
                out.push((ar, vec![]));
                titled(&mut out, "roundRect", Xfrm::new(cx - fw * 0.35, y0 + fh + a * 1.3, fw * 0.7, (s - fh - a * 1.3).max(10.0)), k, g);
            }
        }
        "pictureHList" => {
            // Horizontal Picture List: a column per node, a big picture over its text.
            let w = area.w / nf;
            for (i, g) in gs.iter().enumerate() {
                let x = area.x + i as f64 * w + w * 0.04;
                let bw = w * 0.92;
                let ph = area.h * 0.55;
                out.push((picture_shape(d, "rect", Xfrm::new(x, area.y, bw, ph), i, g[0]), vec![]));
                let mut sh = text_shape("rect", Xfrm::new(x, area.y + ph, bw, area.h - ph), &nodes(g), true);
                if let Some(b) = &mut sh.text {
                    b.body.anchor = Some(deckcraft_model::text::Anchor::Top);
                }
                paint(d, &mut sh, i, false);
                out.push((sh, g.clone()));
            }
        }
        "timeline" => {
            // Basic Timeline: an arrow across the middle, a dot per node on it, the text above and
            // below in turn.
            let mid = area.y + area.h / 2.0;
            let bar = (area.h * 0.08).max(6.0);
            let mut sh = deco("rightArrow", Xfrm::new(area.x, mid - bar, area.w, bar * 2.0), 0.0);
            paint(d, &mut sh, 0, true);
            out.push((sh, vec![]));
            let w = area.w * 0.92 / nf;
            for (i, g) in gs.iter().enumerate() {
                let cx = area.x + w * (i as f64 + 0.5);
                let dot = bar * 1.4;
                let mut dsh = deco("ellipse", Xfrm::new(cx - dot / 2.0, mid - dot / 2.0, dot, dot), 0.0);
                paint(d, &mut dsh, i, false);
                out.push((dsh, vec![]));
                let th = area.h / 2.0 - bar * 1.6;
                let y = if i % 2 == 0 { area.y } else { mid + bar * 1.6 };
                let mut t = text_shape("rect", Xfrm::new(cx - w * 0.48, y, w * 0.96, th.max(1.0)), &nodes(g), true);
                if let Some(b) = &mut t.text {
                    b.body.anchor = Some(if i % 2 == 0 { deckcraft_model::text::Anchor::Bottom } else { deckcraft_model::text::Anchor::Top });
                }
                paint(d, &mut t, i, true);
                t.fill = Some(Fill::None);
                t.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                out.push((t, g.clone()));
            }
        }
        "venn" => {
            // Basic Venn: see-through circles overlapping in a ring (two or three in a row/triangle).
            let s = area.h.min(area.w);
            let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
            let dc = if n == 1 { s } else { s * 0.62 };
            let r = if n == 1 { 0.0 } else { (s - dc) / 2.0 };
            for (i, g) in gs.iter().enumerate() {
                let a = (i as f64 / nf * 360.0 + if n == 2 { 180.0 } else { -90.0 }).to_radians();
                let mut sh = text_shape("ellipse", Xfrm::new(cx + r * a.cos() - dc / 2.0, cy + r * a.sin() - dc / 2.0, dc, dc), &nodes(g), true);
                paint(d, &mut sh, i, false);
                if let Some(Fill::Solid { color: c }) = &mut sh.fill {
                    *c = c.clone().with(ColorTransform::Alpha(50_000));
                }
                if let Some(b) = &mut sh.text {
                    // Text towards the circle's outer side, clear of the overlap.
                    let p = dc * 0.36;
                    let (dx, dy) = (a.cos() * p, a.sin() * p);
                    b.body.inset_l = Some((7.2 + dx).max(2.0));
                    b.body.inset_r = Some((7.2 - dx).max(2.0));
                    b.body.inset_t = Some((3.6 + dy).max(2.0));
                    b.body.inset_b = Some((3.6 - dy).max(2.0));
                }
                out.push((sh, g.clone()));
            }
        }
        "target" => {
            // Basic Target: nested rings, the outermost first, each ring's text in a box beside it
            // with a line from the ring.
            let s = area.h.min(area.w * 0.5);
            let (cx, cy) = (area.x + s / 2.0, area.y + area.h / 2.0);
            let band = s / 2.0 / nf;
            for (i, _) in gs.iter().enumerate() {
                let dc = s - 2.0 * band * i as f64;
                let mut sh = deco("ellipse", Xfrm::new(cx - dc / 2.0, cy - dc / 2.0, dc, dc), 0.0);
                paint(d, &mut sh, i, false);
                out.push((sh, vec![]));
            }
            let tx = area.x + s + area.w * 0.06;
            let rh = area.h / nf;
            for (i, g) in gs.iter().enumerate() {
                let ty = area.y + rh * (i as f64 + 0.5);
                // The point on ring i's band level with the text, else its top.
                let ry = cy - s / 2.0 + band * (i as f64 + 0.5);
                out.push((line_shape((cx, ry.min(ty)), (tx, ty)), vec![]));
                let mut t = text_shape("rect", Xfrm::new(tx, ty - rh * 0.45, (area.x + area.w - tx).max(1.0), rh * 0.9), &nodes(g), true);
                paint(d, &mut t, i, true);
                t.fill = Some(Fill::None);
                t.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                if let Some(b) = &mut t.text {
                    for p in &mut b.paragraphs {
                        p.props.align = Some(Align::Left);
                    }
                }
                out.push((t, g.clone()));
            }
        }
        _ => {}
    }
    out
}
