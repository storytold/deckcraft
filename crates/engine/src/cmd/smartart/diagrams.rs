use super::nodes::Item;
use super::paint::{accent, deco, line_shape, paint, text_shape};
use super::*;

/// Organization Chart: [`hierarchy`] with elbow connectors, and assistants beside their parent on
/// a row of their own between the parent and its other children. `half_circle` draws each node as
/// its text in an open half ring (Half Circle Organization Chart).
pub(super) fn org_chart(d: &Diagram, area: Xfrm) -> Vec<Item> {
    let half_circle = d.kind == "halfCircleOrgChart";
    let name_title = d.kind == "nameTitleOrgChart";
    let nodes = &d.nodes[..d.nodes.len().min(MAX_SHAPES)];
    if nodes.is_empty() {
        return vec![];
    }
    let mut parent: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut stack: Vec<usize> = vec![];
    for (i, n) in nodes.iter().enumerate() {
        while stack.last().is_some_and(|&p| nodes[p].level >= n.level) {
            stack.pop();
        }
        parent[i] = stack.last().copied();
        stack.push(i);
    }
    let asst = |i: usize| nodes[i].assistant && parent[i].is_some();
    let kids = |i: Option<usize>| -> Vec<usize> { (0..nodes.len()).filter(|&k| parent[k] == i && !asst(k)).collect() };
    let mut leaves = vec![0usize; nodes.len()];
    for i in (0..nodes.len()).rev() {
        let k = kids(Some(i));
        leaves[i] = if k.is_empty() { 1 } else { k.iter().map(|&c| leaves[c]).sum() };
    }
    // Row slots: a node's level is 2 per depth; assistants take the odd slot under their parent.
    let mut slot = vec![0usize; nodes.len()];
    for i in 0..nodes.len() {
        slot[i] = match parent[i] {
            None => 0,
            Some(p) if asst(i) => slot[p] + 1,
            Some(p) => slot[p] + 2,
        };
    }
    let mut used: Vec<usize> = slot.clone();
    used.sort_unstable();
    used.dedup();
    let row = |i: usize| used.iter().position(|&s| s == slot[i]).unwrap_or(0);
    let rows = used.len().max(1);
    let total: usize = kids(None).iter().map(|&r| leaves[r]).sum::<usize>().max(1);
    let cell_w = area.w / total as f64;
    let row_h = area.h / rows as f64;
    let (bw, bh) = ((cell_w * 0.85).min(area.w * 0.3), row_h * 0.62);
    let mut left = vec![0usize; nodes.len()];
    let mut next_root = 0;
    for i in 0..nodes.len() {
        if asst(i) {
            continue;
        }
        let mut start = match parent[i] {
            None => {
                let s = next_root;
                next_root += leaves[i];
                s
            }
            Some(p) => left[p],
        };
        if let Some(p) = parent[i] {
            for s in kids(Some(p)) {
                if s == i {
                    break;
                }
                start += leaves[s];
            }
        }
        left[i] = start;
    }
    let mut side = vec![0usize; nodes.len()];
    let boxes: Vec<Xfrm> = (0..nodes.len())
        .map(|i| {
            let y = area.y + row(i) as f64 * row_h + (row_h - bh) / 2.0;
            match parent[i] {
                Some(p) if asst(i) => {
                    // Assistants alternate right and left of the parent's trunk.
                    let k = (0..i).filter(|&j| parent[j] == Some(p) && asst(j)).count();
                    side[i] = k;
                    let pc = area.x + (left[p] as f64 + leaves[p] as f64 / 2.0) * cell_w;
                    let off = bw * 0.15 + (k / 2) as f64 * bw * 1.1;
                    let x = if k % 2 == 0 { pc + off } else { pc - off - bw };
                    Xfrm::new(x.clamp(area.x, area.x + area.w - bw), y, bw, bh)
                }
                _ => {
                    let cx = area.x + (left[i] as f64 + leaves[i] as f64 / 2.0) * cell_w;
                    Xfrm::new(cx - bw / 2.0, y, bw, bh)
                }
            }
        })
        .collect();
    let mut out: Vec<Item> = vec![];
    for i in 0..nodes.len() {
        let Some(p) = parent[i] else { continue };
        let (a, b) = (boxes[p], boxes[i]);
        let pc = a.x + a.w / 2.0;
        if asst(i) {
            let my = b.y + b.h / 2.0;
            out.push((line_shape((pc, a.y + a.h), (pc, my)), vec![]));
            let ex = if b.x > pc { b.x } else { b.x + b.w };
            out.push((line_shape((pc, my), (ex, my)), vec![]));
        } else {
            let mid = b.y - (row_h - bh) / 2.0;
            let cc = b.x + b.w / 2.0;
            out.push((line_shape((pc, a.y + a.h), (pc, mid)), vec![]));
            out.push((line_shape((pc.min(cc), mid), (pc.max(cc), mid)), vec![]));
            out.push((line_shape((cc, mid), (cc, b.y)), vec![]));
        }
    }
    let _ = side;
    for (i, n) in nodes.iter().enumerate() {
        let depth = slot[i] / 2;
        if half_circle {
            // Two half rings round the text: the upper left one in the accent, the lower right one
            // in its tint.
            let x = boxes[i];
            for (k, (a0, a1)) in [(140.0, 310.0), (320.0, 130.0)].into_iter().enumerate() {
                let mut arc = deco("blockArc", x, 0.0);
                arc.geom = Geom::Preset { name: "blockArc".into(), adj: vec![a0 * 60_000.0, a1 * 60_000.0, 4_000.0] };
                paint(d, &mut arc, depth, false);
                if k == 1 {
                    let c = ColorRef::scheme(accent(d, depth)).with(ColorTransform::LumMod(60_000)).with(ColorTransform::LumOff(40_000));
                    arc.fill = Some(Fill::solid(c));
                }
                arc.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                arc.effects = None;
                out.push((arc, vec![]));
            }
            let mut t = text_shape("rect", x, &[n], true);
            paint(d, &mut t, depth, true);
            t.fill = Some(Fill::None);
            t.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
            out.push((t, vec![i]));
        } else {
            let mut x = boxes[i];
            if name_title {
                // The name in the upper part; the title box overlaps the lower edge.
                x.h *= 0.85;
            }
            let mut sh = text_shape("rect", x, &[n], true);
            paint(d, &mut sh, depth, false);
            out.push((sh, vec![i]));
            if name_title {
                let tb = Xfrm::new(x.x + x.w * 0.12, x.y + x.h * 0.72, x.w * 0.8, x.h * 0.42);
                let title = DiagramNode { text: n.title.clone(), ..Default::default() };
                let mut t = text_shape("rect", tb, &[&title], true);
                paint(d, &mut t, depth, true);
                t.fill = Some(Fill::solid(ColorRef::scheme(SchemeSlot::Lt1)));
                if let Some(b) = &mut t.text {
                    for p in &mut b.paragraphs {
                        for r in &mut p.runs {
                            r.props.size = Some(11.0);
                        }
                        p.end_props.size = Some(11.0);
                    }
                }
                out.push((t, vec![]));
            }
        }
    }
    out
}

/// An organisation chart: one box per node, rows by level, each subtree as wide as its leaves.
pub(super) fn hierarchy(d: &Diagram, area: Xfrm, horizontal: bool) -> Vec<Item> {
    let nodes = &d.nodes[..d.nodes.len().min(MAX_SHAPES)];
    if nodes.is_empty() {
        return vec![];
    }
    let mut parent: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut stack: Vec<usize> = vec![];
    for (i, n) in nodes.iter().enumerate() {
        while stack.last().is_some_and(|&p| nodes[p].level >= n.level) {
            stack.pop();
        }
        parent[i] = stack.last().copied();
        stack.push(i);
    }
    let kids = |i: Option<usize>| -> Vec<usize> { (0..nodes.len()).filter(|&k| parent[k] == i).collect() };
    let mut leaves = vec![0usize; nodes.len()];
    for i in (0..nodes.len()).rev() {
        let k = kids(Some(i));
        leaves[i] = if k.is_empty() { 1 } else { k.iter().map(|&c| leaves[c]).sum() };
    }
    let depth: Vec<usize> = (0..nodes.len())
        .map(|i| {
            let (mut d, mut p) = (0, parent[i]);
            while let Some(q) = p {
                d += 1;
                p = parent[q];
            }
            d
        })
        .collect();
    let rows = depth.iter().max().copied().unwrap_or(0) + 1;
    let total: usize = kids(None).iter().map(|&r| leaves[r]).sum::<usize>().max(1);
    // Horizontal Hierarchy: the same tree turned a quarter, levels from left to right.
    let (span, across) = if horizontal { (area.h, area.w) } else { (area.w, area.h) };
    let cell_w = span / total as f64;
    let row_h = across / rows as f64;
    let (bw, bh) = if horizontal { (cell_w * 0.8, row_h * 0.7) } else { (cell_w * 0.85, row_h * 0.6) };
    // Left edge (in leaves) of each node's span.
    let mut left = vec![0usize; nodes.len()];
    let mut next_root = 0;
    for i in 0..nodes.len() {
        let mut start = match parent[i] {
            None => {
                let s = next_root;
                next_root += leaves[i];
                s
            }
            Some(p) => left[p],
        };
        if let Some(p) = parent[i] {
            for s in kids(Some(p)) {
                if s == i {
                    break;
                }
                start += leaves[s];
            }
        }
        left[i] = start;
    }
    let boxes: Vec<Xfrm> = (0..nodes.len())
        .map(|i| {
            let c = (left[i] as f64 + leaves[i] as f64 / 2.0) * cell_w;
            let r = depth[i] as f64 * row_h + (row_h - bh) / 2.0;
            if horizontal { Xfrm::new(area.x + r, area.y + c - bw / 2.0, bh, bw) } else { Xfrm::new(area.x + c - bw / 2.0, area.y + r, bw, bh) }
        })
        .collect();
    let mut out: Vec<Item> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let mut sh = text_shape("roundRect", boxes[i], &[n], true);
            paint(d, &mut sh, depth[i], false);
            (sh, vec![i])
        })
        .collect();
    for i in 0..nodes.len() {
        if let Some(p) = parent[i] {
            let (a, b) = (boxes[p], boxes[i]);
            let (p, q) = if horizontal {
                ((a.x + a.w, a.y + a.h / 2.0), (b.x, b.y + b.h / 2.0))
            } else {
                ((a.x + a.w / 2.0, a.y + a.h), (b.x + b.w / 2.0, b.y))
            };
            out.push((line_shape(p, q), vec![]));
        }
    }
    out
}
