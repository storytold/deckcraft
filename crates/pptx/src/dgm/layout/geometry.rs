//! Algorithm geometry and reference constraints. All dimensions are in presentation points.
use super::{Data, Instance, MAX_INSTANCES, MAX_WORK, Placed};
use crate::xml::El;
use deckcraft_geom::Xfrm;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct BoxStyle {
    pub(super) rect: Xfrm,
    pub(super) font: Option<f64>,
    pub(super) secondary_font: Option<f64>,
    pub(super) min_font: Option<f64>,
    pub(super) margins: [Option<f64>; 4],
    pub(super) extra: [Option<f64>; 64],
}
impl BoxStyle {
    pub(super) fn new(rect: Xfrm) -> Self {
        Self { rect, font: Some(36.0), secondary_font: None, min_font: None, margins: [None; 4], extra: [None; 64] }
    }
    pub(super) fn get(&self, property: &str) -> Option<f64> {
        let r = self.rect;
        Some(match property {
            "diam" => r.w.min(r.h),
            "w" => r.w,
            "h" => r.h,
            "l" => r.x,
            "t" => r.y,
            "r" => r.x + r.w,
            "b" => r.y + r.h,
            "ctrX" => r.x + r.w / 2.0,
            "ctrY" => r.y + r.h / 2.0,
            "primFontSz" => self.font.unwrap_or(18.0),
            "secFontSz" => self.secondary_font.unwrap_or(self.font.unwrap_or(18.0) * 0.78),
            "lMarg" => self.margins[0].unwrap_or(self.font.unwrap_or(18.0) * 0.56),
            "tMarg" => self.margins[1].unwrap_or(self.font.unwrap_or(18.0) * 0.56),
            "rMarg" => self.margins[2].unwrap_or(self.font.unwrap_or(18.0) * 0.56),
            "bMarg" => self.margins[3].unwrap_or(self.font.unwrap_or(18.0) * 0.56),
            _ => return extra_index(property).and_then(|i| self.extra.get(i)).copied().flatten(),
        })
    }
    pub(super) fn set(&mut self, property: &str, value: f64, op: &str) {
        if !value.is_finite() || value.abs() > 1e6 {
            return;
        }
        let old = self.get(property).unwrap_or(value);
        let v = match op {
            "gte" => old.max(value),
            "lte" => old.min(value),
            _ => value,
        };
        match property {
            "diam" => {
                self.rect.w = v.max(0.0);
                self.rect.h = v.max(0.0);
            }
            "w" => self.rect.w = v.max(0.0),
            "h" => self.rect.h = v.max(0.0),
            "l" => self.rect.x = v,
            "t" => self.rect.y = v,
            "r" => self.rect.x = v - self.rect.w,
            "b" => self.rect.y = v - self.rect.h,
            "ctrX" => self.rect.x = v - self.rect.w / 2.0,
            "ctrY" => self.rect.y = v - self.rect.h / 2.0,
            "primFontSz" => self.font = Some(v.clamp(1.0, 400.0)),
            "secFontSz" => self.secondary_font = Some(v.clamp(1.0, 400.0)),
            "lMarg" => self.margins[0] = Some(v.max(0.0)),
            "tMarg" => self.margins[1] = Some(v.max(0.0)),
            "rMarg" => self.margins[2] = Some(v.max(0.0)),
            "bMarg" => self.margins[3] = Some(v.max(0.0)),
            _ => {
                if let Some(i) = extra_index(property)
                    && let Some(extra) = self.extra.get_mut(i)
                {
                    *extra = Some(v);
                }
            }
        }
    }
}

fn extra_index(property: &str) -> Option<usize> {
    const EXTRA: &[&str] = &[
        "sp",
        "sibSp",
        "secSibSp",
        "begPad",
        "endPad",
        "alignOff",
        "begMarg",
        "endMarg",
        "bendDist",
        "connDist",
        "pyraAcctRatio",
        "stemThick",
        "wArH",
        "hArH",
    ];
    EXTRA.iter().position(|p| *p == property).or_else(|| {
        property.strip_prefix("user").and_then(|v| {
            let b = v.as_bytes();
            if b.len() == 1 && b[0].is_ascii_uppercase() { Some(20 + (b[0] - b'A') as usize) } else { None }
        })
    })
}

pub(super) fn place<'a>(node: &Instance<'a>, rect: Xfrm, data: &Data<'a>, out: &mut Vec<Placed<'a>>, depth: usize) -> Result<(), &'static str> {
    let mut work = 0;
    let solution = super::constraints::resolve(node, rect, data, &mut work)?;
    place_solved(node, rect, data, out, depth, &solution, &mut work)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn place_solved<'a>(
    node: &Instance<'a>,
    rect: Xfrm,
    data: &Data<'a>,
    out: &mut Vec<Placed<'a>>,
    depth: usize,
    solution: &super::constraints::Solution,
    work: &mut usize,
) -> Result<(), &'static str> {
    let style = super::constraints::lookup(solution, node).unwrap_or_else(|| BoxStyle::new(rect));
    let start = out.len();
    place_inner(node, style, data, out, depth, solution, work)?;
    if depth == 0 && node.alg == "composite" {
        fit_composite_frame(out.get_mut(start..).unwrap_or(&mut []), rect, work)?;
    }
    Ok(())
}

fn fit_composite_frame(out: &mut [Placed<'_>], frame: Xfrm, work: &mut usize) -> Result<(), &'static str> {
    charge(work, out.len() * 2)?;
    let shapes: Vec<_> = out.iter().filter(|p| !p.connector).collect();
    let left = shapes.iter().map(|p| p.rect.x).fold(f64::INFINITY, f64::min);
    let top = shapes.iter().map(|p| p.rect.y).fold(f64::INFINITY, f64::min);
    let right = shapes.iter().map(|p| p.rect.x + p.rect.w).fold(f64::NEG_INFINITY, f64::max);
    let bottom = shapes.iter().map(|p| p.rect.y + p.rect.h).fold(f64::NEG_INFINITY, f64::max);
    let (width, height) = (right - left, bottom - top);
    if width <= 0.0 || height <= 0.0 || !width.is_finite() || !height.is_finite() {
        return Ok(());
    }
    let (sx, sy) = ((frame.w / width).min(1.0), (frame.h / height).min(1.0));
    let (x, y) = (frame.x + (frame.w - width * sx) / 2.0, frame.y + (frame.h - height * sy) / 2.0);
    for p in out {
        p.rect.x = x + (p.rect.x - left) * sx;
        p.rect.y = y + (p.rect.y - top) * sy;
        p.rect.w *= sx;
        p.rect.h *= sy;
    }
    Ok(())
}

fn charge(work: &mut usize, amount: usize) -> Result<(), &'static str> {
    *work = work.saturating_add(amount);
    if *work > MAX_WORK { Err("diagram constraint budget exceeded") } else { Ok(()) }
}

#[allow(clippy::too_many_arguments)]
fn place_inner<'a>(
    node: &Instance<'a>,
    style: BoxStyle,
    data: &Data<'a>,
    out: &mut Vec<Placed<'a>>,
    depth: usize,
    solution: &super::constraints::Solution,
    work: &mut usize,
) -> Result<(), &'static str> {
    if depth > 64 || out.len() >= MAX_INSTANCES {
        return Err("diagram geometry budget exceeded");
    }
    charge(work, 1)?;
    // Numeric rule endpoints constrain their own search. They are not an inherited
    // minimum font for every descendant, especially after a child has already fitted.
    if let Some(shape) = node.shape.filter(|s| s.attr("type").is_some_and(|t| !t.is_empty() && t != "none")) {
        let mut rect = style.rect;
        rect.rot += shape.f64("rot").unwrap_or(0.0).clamp(-36000.0, 36000.0);
        out.push(Placed {
            node_key: super::constraints::id(node),
            shape: Some(shape),
            points: node.points.iter().filter_map(|i| data.points.get(*i).copied()).collect(),
            point_levels: node.point_levels.clone(),
            name: node.name.clone(),
            style: node.style.clone(),
            rect,
            font: style.font,
            secondary_font: style.secondary_font,
            min_font: style.min_font,
            margins: style.margins.map(|m| m.unwrap_or(style.font.unwrap_or(18.0) * 0.56)),
            text_rotation: node.number("txRot", 0.0),
            connector: node.alg == "conn" || shape.attr("type") == Some("conn"),
            params: node.params.clone(),
            endpoints: node.points.iter().find_map(|i| data.endpoints(*i)),
        });
    } else if node.alg == "tx" && !node.points.is_empty() {
        out.push(Placed {
            node_key: super::constraints::id(node),
            shape: None,
            points: node.points.iter().filter_map(|i| data.points.get(*i).copied()).collect(),
            point_levels: node.point_levels.clone(),
            name: node.name.clone(),
            style: node.style.clone(),
            rect: style.rect,
            font: style.font,
            secondary_font: style.secondary_font,
            min_font: style.min_font,
            margins: style.margins.map(|m| m.unwrap_or(style.font.unwrap_or(18.0) * 0.56)),
            text_rotation: node.number("txRot", 0.0),
            connector: false,
            params: node.params.clone(),
            endpoints: None,
        });
    }
    if node.children.is_empty() {
        return Ok(());
    }
    let local = BoxStyle { rect: Xfrm::new(0.0, 0.0, style.rect.w, style.rect.h), ..style };
    let mut children: Vec<_> = node
        .children
        .iter()
        .zip(initial(node, local))
        .map(|(child, rect)| {
            let mut resolved = super::constraints::lookup(solution, child).unwrap_or(BoxStyle { rect, ..local });
            resolved.font = resolved.font.or(local.font);
            resolved.secondary_font = resolved.secondary_font.or(local.secondary_font);
            resolved.min_font = resolved.min_font.or(local.min_font);
            resolved
        })
        .collect();
    // Linear placement uses the constrained sizes, including invisible spacing nodes.
    if matches!(node.alg.as_str(), "lin" | "hierChild") {
        pack_linear(node, local, &mut children);
    } else if node.alg == "cycle" {
        pack_cycle(node, local, &mut children, work)?;
    }
    let iter: Box<dyn Iterator<Item = usize>> =
        if node.reverse { Box::new((0..node.children.len()).rev()) } else { Box::new(0..node.children.len()) };
    let mut ranges = vec![0..0; children.len()];
    for i in iter {
        if let (Some(child), Some(bounds)) = (node.children.get(i), children.get(i)) {
            let mut global = *bounds;
            global.rect.x += style.rect.x;
            global.rect.y += style.rect.y;
            let start = out.len();
            place_inner(child, global, data, out, depth + 1, solution, work)?;
            if let Some(range) = ranges.get_mut(i) {
                *range = start..out.len();
            }
        }
    }
    pack_composite_extents(node, style, local, &children, &ranges, out, work)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn pack_composite_extents(
    node: &Instance<'_>,
    style: BoxStyle,
    local: BoxStyle,
    children: &[BoxStyle],
    ranges: &[std::ops::Range<usize>],
    out: &mut [Placed<'_>],
    work: &mut usize,
) -> Result<(), &'static str> {
    // A composite is a container, not an invisible spacing shape. Linear layout packs
    // its actual children, so unused margins inside that container do not become gaps.
    // Retain the existing constrained-size path for rotated/overflowing containers.
    if node.alg == "lin" {
        let mut occupied = children.to_vec();
        let mut changed = false;
        for ((child, bounds), range) in node.children.iter().zip(&mut occupied).zip(ranges) {
            if child.alg != "composite" || range.is_empty() {
                continue;
            }
            let shapes = out.get(range.clone()).unwrap_or(&[]);
            charge(work, shapes.len())?;
            if shapes.iter().any(|p| p.connector || p.rect.rot != 0.0) {
                continue;
            }
            let mut box_rect: Option<Xfrm> = None;
            for p in shapes {
                box_rect = Some(match box_rect {
                    None => p.rect,
                    Some(b) => {
                        let (x, y) = (b.x.min(p.rect.x), b.y.min(p.rect.y));
                        Xfrm::new(x, y, (b.x + b.w).max(p.rect.x + p.rect.w) - x, (b.y + b.h).max(p.rect.y + p.rect.h) - y)
                    }
                });
            }
            if let Some(mut b) = box_rect.filter(|b| b.w > 0.0 && b.h > 0.0 && b.w <= bounds.rect.w && b.h <= bounds.rect.h) {
                b.x -= style.rect.x;
                b.y -= style.rect.y;
                changed |= b != bounds.rect;
                bounds.rect = b;
            }
        }
        if changed {
            let mut packed = occupied.clone();
            pack_linear(node, local, &mut packed);
            for ((before, after), range) in occupied.iter().zip(&packed).zip(ranges.iter().cloned()) {
                for p in out.get_mut(range).unwrap_or(&mut []) {
                    p.rect.x += after.rect.x - before.rect.x;
                    p.rect.y += after.rect.y - before.rect.y;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn self_constraints(node: &Instance<'_>, style: &mut BoxStyle) {
    for constraint in
        node.constraints.iter().filter(|e| e.attr("for").unwrap_or("self") == "self" && !super::constraints::position(e.attr("type").unwrap_or("")))
    {
        if super::constraints::font_bound(constraint) {
            continue;
        }
        if let Some(value) = constraint_value(constraint, style) {
            style.set(constraint.attr("type").unwrap_or(""), value, constraint.attr("op").unwrap_or("equ"));
        }
    }
}

fn constraint_value(c: &El, reference: &BoxStyle) -> Option<f64> {
    let ty = c.attr("refType").filter(|s| *s != "none").unwrap_or("");
    // Diagram absolute lengths are millimetres; only font sizes use points.
    let scale =
        if matches!(c.attr("type"), Some("primFontSz" | "secFontSz" | "pyraAcctRatio")) || c.attr("type").is_some_and(|s| s.starts_with("user")) {
            1.0
        } else {
            72.0 / 25.4
        };
    if ty.is_empty() { c.f64("val").map(|v| v * scale) } else { reference.get(ty).map(|v| v * c.f64("fact").unwrap_or(1.0)) }
}

pub(super) fn initial(node: &Instance<'_>, style: BoxStyle) -> Vec<Xfrm> {
    let r = style.rect;
    let count = node.children.len();
    let n = count.max(1) as f64;
    let default_dir = if node.alg == "hierChild" && node.param("chDir", "horz") == "vert" { "fromT" } else { "fromL" };
    let dir = node.param("linDir", default_dir);
    let vertical = matches!(dir.as_str(), "fromT" | "fromB");
    let gap = if node.alg == "composite" { 0.0 } else { style.get("sp").or(style.get("sibSp")).unwrap_or(0.0).max(0.0) };
    let (w, h) = if vertical { (r.w, ((r.h - gap * (n - 1.0)) / n).max(0.0)) } else { (((r.w - gap * (n - 1.0)) / n).max(0.0), r.h) };
    match node.alg.as_str() {
        "lin" | "hierChild" => (0..count)
            .map(|i| {
                Xfrm::new(r.x + if vertical { 0.0 } else { i as f64 * (w + gap) }, r.y + if vertical { i as f64 * (h + gap) } else { 0.0 }, w, h)
            })
            .collect(),
        "cycle" => {
            let start = (node.number("stAng", 0.0) - 90.0).to_radians();
            let span = node.number("spanAng", 360.0).to_radians();
            let center = node.param("ctrShpMap", "none") != "none";
            let orbit = (count - usize::from(center)).max(1) as f64;
            let diameter = (r.w.min(r.h) / (1.0 + orbit / std::f64::consts::PI)).min(r.w.min(r.h) * 0.42);
            (0..count)
                .map(|i| {
                    if center && i == 0 {
                        return Xfrm::new(r.x + r.w / 2.0 - diameter / 2.0, r.y + r.h / 2.0 - diameter / 2.0, diameter, diameter);
                    }
                    let angle = start
                        + (i - usize::from(center)) as f64 * span
                            / if (span.abs() - std::f64::consts::TAU).abs() < 1e-6 { orbit } else { (orbit - 1.0).max(1.0) };
                    Xfrm::new(
                        r.x + r.w / 2.0 + (r.w - diameter) * 0.5 * angle.cos() - diameter / 2.0,
                        r.y + r.h / 2.0 + (r.h - diameter) * 0.5 * angle.sin() - diameter / 2.0,
                        diameter,
                        diameter,
                    )
                })
                .collect()
        }
        "snake" => {
            let aspect = node.number("ar", 1.6).max(0.05);
            let column = node.param("flowDir", "row") == "col";
            let primary = if node.param("bkpt", "bal") == "fixed" {
                (node.number("bkPtFixedVal", 1.0) as usize).clamp(1, count.max(1))
            } else {
                let ratio = if column { r.h / r.w.max(0.01) * aspect } else { r.w / r.h.max(0.01) / aspect };
                ((n * ratio).sqrt().ceil() as usize).clamp(1, count.max(1))
            };
            let secondary = count.div_ceil(primary).max(1);
            let (cols, rows) = if column { (secondary, primary) } else { (primary, secondary) };
            let (w, h) = (r.w / cols as f64, r.h / rows as f64);
            let reversed = node.param("contDir", "sameDir") == "revDir";
            let grow = node.param("grDir", "tL");
            (0..count)
                .map(|i| {
                    let minor = i / primary;
                    let major = if reversed && minor % 2 == 1 { primary - 1 - i % primary } else { i % primary };
                    let (mut col, mut row) = if column { (minor, major) } else { (major, minor) };
                    if grow.ends_with('R') {
                        col = cols - 1 - col;
                    }
                    if grow.starts_with('b') {
                        row = rows - 1 - row;
                    }
                    Xfrm::new(r.x + col as f64 * w, r.y + row as f64 * h, w, h)
                })
                .collect()
        }
        "pyra" => (0..count)
            .map(|i| {
                let width = r.w * (i + 1) as f64 / n;
                Xfrm::new(r.x + (r.w - width) / 2.0, r.y + i as f64 * r.h / n, width, r.h / n)
            })
            .collect(),
        "hierRoot" => hierarchy(node, style),
        _ => vec![r; count],
    }
}

// Cycle angles start at twelve o'clock and turn clockwise (ECMA-376 ST_ParameterId).
// Fit a circle against the actual child extents; an incomplete arc is centred by its
// occupied bounds, not the centre of the unoccupied remainder of the circle.
fn pack_cycle(node: &Instance<'_>, style: BoxStyle, children: &mut [BoxStyle], work: &mut usize) -> Result<(), &'static str> {
    if children.is_empty() {
        return Ok(());
    }
    let center = node.param("ctrShpMap", "none") != "none";
    let orbit = children.len().saturating_sub(usize::from(center)).max(1);
    let start = node.number("stAng", 0.0).to_radians();
    let span = node.number("spanAng", 360.0).to_radians();
    let divisor = if (span.abs() - std::f64::consts::TAU).abs() < 1e-6 { orbit } else { orbit.saturating_sub(1).max(1) } as f64;
    let units: Vec<_> = (0..children.len())
        .map(|i| {
            if center && i == 0 {
                (0., 0.)
            } else {
                let angle = start + i.saturating_sub(usize::from(center)) as f64 * span / divisor;
                (angle.sin(), -angle.cos())
            }
        })
        .collect();
    // Oversized constrained nodes must shrink together before choosing the orbit. Find
    // the smallest radius that separates every pair of axis-aligned child rectangles.
    // Scaling both rectangles and that orbit by the same factor preserves aspect ratios.
    let mut minimum_radius: f64 = 0.;
    for (i, (a, (ax, ay))) in children.iter().zip(&units).enumerate() {
        for (b, (bx, by)) in children.iter().zip(&units).take(i) {
            charge(work, 1)?;
            let (dx, dy) = ((ax - bx).abs(), (ay - by).abs());
            if dx < 1e-9 && dy < 1e-9 {
                continue;
            }
            let x = if dx < 1e-9 { f64::INFINITY } else { (a.rect.w + b.rect.w) / (2. * dx) };
            let y = if dy < 1e-9 { f64::INFINITY } else { (a.rect.h + b.rect.h) / (2. * dy) };
            minimum_radius = minimum_radius.max(x.min(y));
        }
    }
    let bounds = |children: &[BoxStyle], radius: f64| {
        let mut extents = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (child, (x, y)) in children.iter().zip(&units) {
            extents.0 = extents.0.min(x * radius - child.rect.w / 2.);
            extents.1 = extents.1.min(y * radius - child.rect.h / 2.);
            extents.2 = extents.2.max(x * radius + child.rect.w / 2.);
            extents.3 = extents.3.max(y * radius + child.rect.h / 2.);
        }
        extents
    };
    let b = bounds(children, minimum_radius);
    let scale = (style.rect.w / (b.2 - b.0).max(1e-9)).min(style.rect.h / (b.3 - b.1).max(1e-9)).clamp(0., 1.);
    for child in children.iter_mut() {
        child.rect.w *= scale;
        child.rect.h *= scale;
    }
    let (mut low, mut high) = (0., (style.rect.w.min(style.rect.h) / 2.).max(minimum_radius * scale));
    for _ in 0..24 {
        let r = (low + high) / 2.;
        let b = bounds(children, r);
        if b.2 - b.0 <= style.rect.w && b.3 - b.1 <= style.rect.h {
            low = r;
        } else {
            high = r;
        }
    }
    let b = bounds(children, low);
    let cx = style.rect.x + (style.rect.w - b.0 - b.2) / 2.;
    let cy = style.rect.y + (style.rect.h - b.1 - b.3) / 2.;
    for (child, (x, y)) in children.iter_mut().zip(units) {
        child.rect.x = cx + x * low - child.rect.w / 2.;
        child.rect.y = cy + y * low - child.rect.h / 2.;
    }
    Ok(())
}

fn pack_linear(node: &Instance<'_>, style: BoxStyle, children: &mut [BoxStyle]) {
    let r = style.rect;
    let default_dir = if node.alg == "hierChild" && node.param("chDir", "horz") == "vert" { "fromT" } else { "fromL" };
    let dir = node.param("linDir", default_dir);
    let vertical = matches!(dir.as_str(), "fromT" | "fromB");
    let backwards = matches!(dir.as_str(), "fromR" | "fromB");
    let gap = style.get("sp").or(style.get("sibSp")).unwrap_or(0.0).max(0.0);
    let gaps = gap * children.len().saturating_sub(1) as f64;
    let sum: f64 = children.iter().map(|b| if vertical { b.rect.h } else { b.rect.w }).sum();
    let available = if vertical { r.h } else { r.w };
    let scale = if sum + gaps > available { available / (sum + gaps).max(0.01) } else { 1.0 };
    let align = node.param(if vertical { "vertAlign" } else { "horzAlign" }, "ctr");
    let mut pos = if align == "ctr" {
        (available - (sum + gaps) * scale) / 2.0
    } else if align == "r" || align == "b" {
        available - (sum + gaps) * scale
    } else {
        0.0
    };
    for child in children {
        let b = &mut child.rect;
        if vertical {
            b.w = b.w.min(r.w);
            b.h *= scale;
            b.y = r.y + if backwards { r.h - pos - b.h } else { pos };
            b.x = r.x + (r.w - b.w) / 2.0;
            pos += b.h + gap * scale;
        } else {
            b.h = b.h.min(r.h);
            b.w *= scale;
            b.x = r.x + if backwards { r.w - pos - b.w } else { pos };
            b.y = r.y + (r.h - b.h) / 2.0;
            pos += b.w + gap * scale;
        }
    }
}

/// Resolve transition identities after all node bounds are known. Connectors are kept behind
/// nodes, and use actual boundary attachment points rather than an arbitrary canvas diagonal.
pub(super) fn connect(out: &mut [Placed<'_>], data: &Data<'_>) {
    use std::collections::HashMap;
    let mut boxes = HashMap::new();
    for p in out.iter().filter(|p| !p.connector) {
        for point in &p.points {
            if let Some(id) = point.attr("modelId") {
                boxes.entry(id).or_insert(p.rect);
            }
        }
    }
    for p in out.iter_mut().filter(|p| p.connector) {
        let Some((a, b)) = p.endpoints else { continue };
        let lookup = |i: usize| data.points.get(i).and_then(|point| point.attr("modelId")).and_then(|id| boxes.get(id)).copied();
        let (Some(a), Some(b)) = (lookup(a), lookup(b)) else { continue };
        let attach = |r: Xfrm, target: Xfrm, key: &str| {
            let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            let (dx, dy) = (target.x + target.w / 2.0 - cx, target.y + target.h / 2.0 - cy);
            match p.params.get(key).map(String::as_str).unwrap_or("auto") {
                "tCtr" | "t" => (cx, r.y),
                "bCtr" | "b" => (cx, r.y + r.h),
                "midL" | "lCtr" | "l" => (r.x, cy),
                "midR" | "rCtr" | "r" => (r.x + r.w, cy),
                "tL" => (r.x, r.y),
                "tR" => (r.x + r.w, r.y),
                "bL" => (r.x, r.y + r.h),
                "bR" => (r.x + r.w, r.y + r.h),
                "ctr" => (cx, cy),
                _ => {
                    let t = (r.w / 2.0 / dx.abs().max(1e-9)).min(r.h / 2.0 / dy.abs().max(1e-9));
                    (cx + dx * t, cy + dy * t)
                }
            }
        };
        let (x1, y1) = attach(a, b, "begPts");
        let (x2, y2) = attach(b, a, "endPts");
        p.rect = Xfrm { flip_h: x2 < x1, flip_v: y2 < y1, ..Xfrm::new(x1.min(x2), y1.min(y2), (x2 - x1).abs(), (y2 - y1).abs()) };
    }
    out.sort_by_key(|p| !p.connector);
}

/// Allocate each subtree by its leaf demand. Orientation comes from hierAlign; a forest has
/// no distinguished first parent. This avoids shrinking deeper branches into equal-width slots.
fn hierarchy(node: &Instance<'_>, style: BoxStyle) -> Vec<Xfrm> {
    fn weight(n: &Instance<'_>, depth: usize) -> f64 {
        if depth > 64 || n.children.is_empty() {
            return 1.0;
        }
        n.children.iter().filter(|c| c.alg != "conn").map(|c| weight(c, depth + 1)).sum::<f64>().max(1.0)
    }
    fn levels(n: &Instance<'_>, depth: usize) -> usize {
        if depth > 64 {
            return 1;
        }
        usize::from(!n.points.is_empty()) + n.children.iter().map(|c| levels(c, depth + 1)).max().unwrap_or(0)
    }
    let r = style.rect;
    let count = node.children.len();
    let has_parent = node
        .children
        .first()
        .is_some_and(|c| !c.points.is_empty() || c.shape.is_some_and(|s| s.attr("type").is_some_and(|t| t != "none" && !t.is_empty())));
    let alignment = node.param("hierAlign", "tCtrCh");
    let horizontal = alignment.starts_with(['l', 'r']);
    let reverse = alignment.starts_with(['r', 'b']);
    let gap = style.get("sp").unwrap_or(0.0).max(0.0);
    let depth = levels(node, 0).max(if has_parent && count > 1 { 2 } else { 1 }) as f64;
    let parent_len = if has_parent { (if horizontal { r.w } else { r.h }) / depth } else { 0.0 };
    let start = usize::from(has_parent);
    let total = node.children.iter().skip(start).map(|c| weight(c, 0)).sum::<f64>().max(1.0);
    let mut offset = 0.0;
    let mut result = Vec::with_capacity(count);
    for (i, child) in node.children.iter().enumerate() {
        let mut b = if has_parent && i == 0 {
            if horizontal { Xfrm::new(0.0, 0.0, parent_len, r.h) } else { Xfrm::new(0.0, 0.0, r.w, parent_len) }
        } else {
            let fraction = weight(child, 0) / total;
            let b = if horizontal {
                Xfrm::new(parent_len + gap, offset * r.h, (r.w - parent_len - gap).max(0.0), r.h * fraction)
            } else {
                Xfrm::new(offset * r.w, parent_len + gap, r.w * fraction, (r.h - parent_len - gap).max(0.0))
            };
            offset += fraction;
            b
        };
        if reverse {
            if horizontal {
                b.x = r.w - b.x - b.w;
            } else {
                b.y = r.h - b.y - b.h;
            }
        }
        b.x += r.x;
        b.y += r.y;
        result.push(b);
    }
    result
}

#[cfg(test)]
mod cycle_tests {
    use super::*;

    #[test]
    fn dense_cycle_pair_search_obeys_the_shared_work_budget() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let node = Instance::new(&xml.root);
        let style = BoxStyle::new(Xfrm::new(0., 0., 800., 400.));
        let mut children = vec![BoxStyle::new(Xfrm::new(0., 0., 100., 80.)); 1000];
        let mut work = 0;
        assert_eq!(pack_cycle(&node, style, &mut children, &mut work), Err("diagram constraint budget exceeded"));
        assert_eq!(work, MAX_WORK + 1);
    }
}
