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
    place_solved(node, rect, data, out, depth, &solution, None, &mut work)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn place_solved<'a>(
    node: &Instance<'a>,
    rect: Xfrm,
    data: &Data<'a>,
    out: &mut Vec<Placed<'a>>,
    depth: usize,
    solution: &super::constraints::Solution,
    text_refits: Option<&std::collections::HashSet<usize>>,
    work: &mut usize,
) -> Result<(), &'static str> {
    let style = super::constraints::lookup(solution, node).unwrap_or_else(|| BoxStyle::new(rect));
    let start = out.len();
    place_inner(node, style, Some(rect), data, out, depth, solution, text_refits, work)?;
    if depth == 0 {
        super::orientation::apply(out.get_mut(start..).unwrap_or(&mut []), work)?;
        // Allocation-only nodes contribute to ancestor bounds, but are not drawing
        // shapes. Keep text helpers with real data for mapped text and rule fitting.
        out.retain(|p| p.shape.is_some() || (p.text_enabled && p.points.iter().any(|pt| pt.attr("type") != Some("doc"))));
    }
    Ok(())
}

struct CompositeFit {
    frame: Xfrm,
    bounds: Xfrm,
    sx: f64,
    sy: f64,
}

fn fit_composite_frame(out: &mut [Placed<'_>], frame: Xfrm, work: &mut usize) -> Result<Option<CompositeFit>, &'static str> {
    charge(work, out.len() * 4)?;
    let Some(bounds) = occupied_bounds(out) else { return Ok(None) };
    let (width, height) = (bounds.w, bounds.h);
    if width < 0.0 || height < 0.0 || !width.is_finite() || !height.is_finite() {
        return Ok(None);
    }
    let scale = |limit: f64, occupied: f64| {
        if occupied - limit <= 16. * f64::EPSILON * occupied.abs().max(limit.abs()).max(1.) { 1. } else { limit / occupied }
    };
    let (sx, sy) = (scale(frame.w, width), scale(frame.h, height));
    if sx >= 1. && sy >= 1. {
        return Ok(None);
    }
    let fit = CompositeFit { frame, bounds, sx, sy };
    for p in out.iter_mut() {
        fit.project(&mut p.rect);
        if let Some(t) = &mut p.text_rect {
            fit.project(t);
        }
    }
    Ok(Some(fit))
}

impl CompositeFit {
    // In an overflowing axis, scale from the frame origin (or a negative occupied
    // origin), clip the end edge, then centre the resulting occupied bounds. Moving
    // positive leading space away before this clip incorrectly preserves that width.
    fn project(&self, r: &mut Xfrm) {
        let axis = |start: f64, size: f64, low: f64, origin: f64, limit: f64, scale: f64| {
            let base = if scale < 1. { origin.min(low) } else { low };
            let a = (start - base) * scale;
            let a = if (a - limit).abs() <= 16. * f64::EPSILON * a.abs().max(limit.abs()).max(1.) { limit } else { a };
            let b = a + size * scale;
            // Clipping is only meaningful when the start is inside the frame.
            // A fully disjoint shape keeps its scaled extent and aligns its end
            // with the frame, before the occupied group is recentered.
            if scale >= 1. {
                (a, size)
            } else if a > limit {
                (limit - size * scale, size * scale)
            } else {
                (a, (b.min(limit) - a).max(0.))
            }
        };
        (r.x, r.w) = axis(r.x, r.w, self.bounds.x, self.frame.x, self.frame.w, self.sx);
        (r.y, r.h) = axis(r.y, r.h, self.bounds.y, self.frame.y, self.frame.h, self.sy);
    }
}

pub(super) fn occupied_bounds(out: &[Placed<'_>]) -> Option<Xfrm> {
    out.iter().filter(|p| !p.ignore_bounds && (p.rect.w > 0. || p.rect.h > 0.)).fold(None, |bounds, p| {
        Some(match bounds {
            None => p.rect,
            Some(b) => {
                let (x, y) = (b.x.min(p.rect.x), b.y.min(p.rect.y));
                Xfrm::new(x, y, (b.x + b.w).max(p.rect.x + p.rect.w) - x, (b.y + b.h).max(p.rect.y + p.rect.h) - y)
            }
        })
    })
}

fn charge(work: &mut usize, amount: usize) -> Result<(), &'static str> {
    *work = work.saturating_add(amount);
    if *work > MAX_WORK { Err("diagram constraint budget exceeded") } else { Ok(()) }
}

/// Place constrained footprints before shape and text orientation. Rule search can
/// restore a local allocation here, then orient it once for measurement.
#[allow(clippy::too_many_arguments)]
pub(super) fn place_inner<'a>(
    node: &Instance<'a>,
    style: BoxStyle,
    capacity: Option<Xfrm>,
    data: &Data<'a>,
    out: &mut Vec<Placed<'a>>,
    depth: usize,
    solution: &super::constraints::Solution,
    text_refits: Option<&std::collections::HashSet<usize>>,
    work: &mut usize,
) -> Result<(), &'static str> {
    if depth > 64 || out.len() >= MAX_INSTANCES {
        return Err("diagram geometry budget exceeded");
    }
    charge(work, 1)?;
    let own = out.len();
    // Numeric rule endpoints constrain their own search. They are not an inherited
    // minimum font for every descendant, especially after a child has already fitted.
    if let Some(shape) = node.shape.filter(|s| s.attr("type").is_some_and(|t| !t.is_empty() && t != "none")) {
        let mut rect = style.rect;
        rect.rot += shape.f64("rot").unwrap_or(0.0).clamp(-36000.0, 36000.0);
        out.push(Placed {
            node_key: super::constraints::id(node),
            shape: Some(shape),
            recovered: false,
            points: node.points.iter().filter_map(|i| data.points.get(*i).copied()).collect(),
            point_levels: node.point_levels.clone(),
            name: node.name.clone(),
            style: node.style.clone(),
            color_index: 0,
            rect,
            ignore_bounds: false,
            path_rotation: 0.,
            text_rect: None,
            text_source: None,
            text_enabled: node.alg == "tx",
            font: style.font,
            secondary_font: style.secondary_font,
            min_font: style.min_font,
            margins: style.margins.map(|m| m.unwrap_or(style.font.unwrap_or(18.0) * 0.56)),
            text_rotation: node.number("txRot", 0.0),
            connector: node.alg == "conn" || shape.attr("type") == Some("conn"),
            params: node.params.clone(),
            endpoints: node.points.iter().find_map(|i| data.endpoints(*i)),
        });
    } else if matches!(node.alg.as_str(), "tx" | "sp" | "conn") {
        out.push(Placed {
            node_key: super::constraints::id(node),
            shape: None,
            recovered: false,
            points: node.points.iter().filter_map(|i| data.points.get(*i).copied()).collect(),
            point_levels: node.point_levels.clone(),
            name: node.name.clone(),
            style: node.style.clone(),
            color_index: 0,
            rect: style.rect,
            ignore_bounds: false,
            path_rotation: 0.,
            text_rect: None,
            text_source: None,
            text_enabled: node.alg == "tx",
            font: style.font,
            secondary_font: style.secondary_font,
            min_font: style.min_font,
            margins: style.margins.map(|m| m.unwrap_or(style.font.unwrap_or(18.0) * 0.56)),
            text_rotation: node.number("txRot", 0.0),
            connector: node.alg == "conn",
            params: node.params.clone(),
            endpoints: None,
        });
    }
    if node.children.is_empty() {
        if node.alg == "composite" {
            fit_composite_shape(out.get_mut(own..).unwrap_or(&mut []), work)?;
        }
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
            resolved.min_font = resolved.min_font.or(local.min_font);
            resolved
        })
        .collect();
    let mut text_refit = false;
    let root_linear = node.alg == "lin" && depth == 0 && text_refits.is_some();
    // Linear placement uses the constrained sizes, including invisible spacing nodes.
    if node.alg == "hierRoot" {
        for child in &mut children {
            child.rect.x = 0.;
            child.rect.y = 0.;
        }
    } else if node.alg == "hierChild" {
        let refit = text_refits.map_or(depth == 0, |refits| refits.contains(&super::constraints::id(node)));
        super::hierarchy::pack(node, local, &mut children, refit, work)?;
    } else if node.alg == "lin" {
        // With real text measurement, a fitting root retains its exact size.
        // Forced word breaks or overflow request the finite fit through this scope.
        // The geometry-only evaluator retains its conservative root-fit fallback.
        let refit = text_refits.map_or(depth == 0, |refits| refits.contains(&super::constraints::id(node)));
        text_refit = super::linear_root::prepare(node, local, capacity, &mut children, root_linear, refit, work)?;
    } else if node.alg == "cycle" {
        text_refit = match text_refits {
            Some(refits) => refits.contains(&super::constraints::id(node)),
            None => cycle_has_text(node, data, depth, work)?,
        };
        pack_cycle(node, local, &mut children, text_refit, work)?;
    } else if node.alg == "snake" {
        text_refit = text_refits.is_some_and(|refits| refits.contains(&super::constraints::id(node)));
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
            // A composite establishes the available cross-axis extent for nested
            // flows. Linear and space containers can expose their ancestor's
            // capacity beyond their own allocation; they do not clip it early.
            // Snake fits the occupied children after their algorithms resolve.
            // Its linear children therefore expose their intrinsic cross extent,
            // including content larger than both their allocation and the canvas.
            let capacity = match node.alg.as_str() {
                "composite" => Some(style.rect),
                "snake" => None,
                _ => capacity,
            };
            place_inner(child, global, capacity, data, out, depth + 1, solution, text_refits, work)?;
            if node.alg == "hierChild" && (bounds.rect.w <= 0. || bounds.rect.h <= 0.) {
                let descendants = out.get_mut(start..).ok_or("invalid hierarchy child range")?;
                charge(work, descendants.len())?;
                for p in descendants {
                    p.ignore_bounds |= p.rect.w <= 0. || p.rect.h <= 0.;
                }
            }
            if let Some(range) = ranges.get_mut(i) {
                *range = start..out.len();
            }
        }
    }
    rotate_cycle_leaves(node, &ranges, out, work)?;
    if root_linear {
        RootLinear { node, style, data, depth, solution, text_refits, text_refit }.place(&mut children, &ranges, out, work)?;
    } else {
        pack_composite_extents(node, style, local, &children, &ranges, out, text_refit, work)?;
    }
    if node.alg == "lin" && out.get(own).is_some_and(|p| p.node_key == super::constraints::id(node)) {
        fit_composite_shape(out.get_mut(own..).unwrap_or(&mut []), work)?;
    }
    if matches!(node.alg.as_str(), "composite" | "sp") {
        if out.get(own).is_some_and(|p| p.node_key == super::constraints::id(node)) {
            fit_composite_shape(out.get_mut(own..).unwrap_or(&mut []), work)?;
        }
        // A child-bearing space encloses and centres its occupied children without
        // shrinking them. A leaf space still reserves its entire allocation.
        // Composites additionally fit overflowing children before parent placement.
        if node.alg == "sp" {
            center_frame(out.get_mut(own..).unwrap_or(&mut []), style.rect, work)?;
        } else {
            if let Some(fit) = fit_composite_frame(out.get_mut(own..).unwrap_or(&mut []), style.rect, work)? {
                // A fitted parent changes its child allocations. A linear child
                // lays out again inside that allocation; scaling its old painted
                // rectangles would incorrectly shrink intrinsic cross-axis sizes.
                for ((child, bounds), range) in node.children.iter().zip(&children).zip(&ranges) {
                    if child.alg != "lin" {
                        continue;
                    }
                    let mut fitted = *bounds;
                    fitted.rect.x += style.rect.x;
                    fitted.rect.y += style.rect.y;
                    fit.project(&mut fitted.rect);
                    let mut replacement = Vec::new();
                    place_inner(child, fitted, Some(style.rect), data, &mut replacement, depth + 1, solution, text_refits, work)?;
                    let target = out.get_mut(range.clone()).ok_or("invalid fitted diagram range")?;
                    if target.len() != replacement.len() {
                        return Err("fitted diagram changed its node count");
                    }
                    for (target, value) in target.iter_mut().zip(replacement) {
                        *target = value;
                    }
                }
                if node.children.iter().any(|child| child.alg == "lin") && out.get(own).is_some_and(|p| p.node_key == super::constraints::id(node)) {
                    fit_composite_shape(out.get_mut(own..).unwrap_or(&mut []), work)?;
                }
            }
            center_frame(out.get_mut(own..).unwrap_or(&mut []), style.rect, work)?;
        }
    }
    Ok(())
}

struct RootLinear<'s, 'a> {
    node: &'s Instance<'a>,
    style: BoxStyle,
    data: &'s Data<'a>,
    depth: usize,
    solution: &'s super::constraints::Solution,
    text_refits: Option<&'s std::collections::HashSet<usize>>,
    text_refit: bool,
}

impl<'a> RootLinear<'_, 'a> {
    fn place(
        self,
        children: &mut [BoxStyle],
        ranges: &[std::ops::Range<usize>],
        out: &mut [Placed<'a>],
        work: &mut usize,
    ) -> Result<(), &'static str> {
        let Self { node, style, data, depth, solution, text_refits, text_refit } = self;
        let clipped = super::linear_root::constrain_cross(node, style, children, ranges, out, work)?;
        let text_refit = text_refit || !clipped.is_empty();
        for i in clipped {
            let child = node.children.get(i).ok_or("invalid root linear child")?;
            let mut fitted = *children.get(i).ok_or("invalid root linear allocation")?;
            fitted.rect.x += style.rect.x;
            fitted.rect.y += style.rect.y;
            let mut replacement = Vec::new();
            place_inner(child, fitted, Some(style.rect), data, &mut replacement, depth + 1, solution, text_refits, work)?;
            let range = ranges.get(i).ok_or("invalid root linear range")?;
            let target = out.get_mut(range.clone()).ok_or("invalid root linear child range")?;
            if target.len() != replacement.len() {
                return Err("root linear fitting changed its node count");
            }
            for (target, value) in target.iter_mut().zip(replacement) {
                *target = value;
            }
        }
        super::linear_root::place(node, style, children, ranges, out, text_refit, work)?;
        Ok(())
    }
}

fn center_frame(out: &mut [Placed<'_>], frame: Xfrm, work: &mut usize) -> Result<(), &'static str> {
    charge(work, out.len() * 2)?;
    let Some(bounds) = occupied_bounds(out) else { return Ok(()) };
    let dx = frame.x + (frame.w - bounds.w) / 2. - bounds.x;
    let dy = frame.y + (frame.h - bounds.h) / 2. - bounds.y;
    for p in out {
        p.rect.x += dx;
        p.rect.y += dy;
        if let Some(t) = &mut p.text_rect {
            t.x += dx;
            t.y += dy;
        }
    }
    Ok(())
}

fn fit_composite_shape(out: &mut [Placed<'_>], work: &mut usize) -> Result<(), &'static str> {
    let Some((own, descendants)) = out.split_first_mut() else { return Ok(()) };
    charge(work, descendants.len())?;
    let mut bounds: Option<Xfrm> = None;
    for p in descendants.iter().filter(|p| !p.ignore_bounds && (p.rect.w > 0. || p.rect.h > 0.)) {
        bounds = Some(match bounds {
            None => p.rect,
            Some(b) => {
                let (x, y) = (b.x.min(p.rect.x), b.y.min(p.rect.y));
                Xfrm::new(x, y, (b.x + b.w).max(p.rect.x + p.rect.w) - x, (b.y + b.h).max(p.rect.y + p.rect.h) - y)
            }
        });
    }
    if let Some(bounds) = bounds {
        own.rect = Xfrm { rot: own.rect.rot, ..bounds };
    } else {
        own.rect.w = 0.;
        own.rect.h = 0.;
    }
    // The text algorithm for the same data point owns typography and the text box,
    // even when the composite's visible shape encloses several different points.
    for p in descendants
        .iter()
        .filter(|p| p.text_enabled && p.shape.is_some_and(|s| s.bool("hideGeom") != Some(true)) && !p.connector && !p.points.is_empty())
    {
        charge(work, own.points.len().saturating_mul(p.points.len()))?;
        if own.points.len() != p.points.len() || !own.points.iter().zip(&p.points).all(|(a, b)| std::ptr::eq(*a, *b)) {
            continue;
        }
        own.text_source = Some(p.text_source.unwrap_or(p.node_key));
        own.text_enabled = true;
        own.font = p.font;
        own.secondary_font = p.secondary_font;
        own.min_font = p.min_font;
        own.margins = p.margins;
        own.text_rotation = p.text_rotation;
        own.params = p.params.clone();
        break;
    }
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
    text_refit: bool,
    work: &mut usize,
) -> Result<(), &'static str> {
    if node.alg == "hierRoot" {
        return super::hierarchy_root::place(node, style, children, ranges, out, work);
    }
    if node.alg == "snake" {
        // Each nested algorithm first resolves its own occupied size. Packing
        // the enclosing allocations prematurely would wrap and shrink children
        // before their unused space has been removed (or their overflow exposed).
        let mut occupied = children.to_vec();
        for (bounds, range) in occupied.iter_mut().zip(ranges) {
            let shapes = out.get(range.clone()).unwrap_or_default();
            charge(work, shapes.len())?;
            if let Some(mut rect) = occupied_bounds(shapes) {
                rect.x -= style.rect.x;
                rect.y -= style.rect.y;
                bounds.rect = rect;
            }
        }
        let mut packed = occupied.clone();
        super::snake::pack(node, local, &mut packed, text_refit, work)?;
        let allocations = packed.clone();
        if restore_linear_cross_extents(node, &occupied, &mut packed, work)? {
            super::snake::repack(node, local, &mut packed, &allocations, work)?;
        }
        for ((before, after), range) in occupied.iter().zip(&packed).zip(ranges.iter().cloned()) {
            let (sx, sy) = (
                if before.rect.w > 0. { after.rect.w / before.rect.w } else { 1. },
                if before.rect.h > 0. { after.rect.h / before.rect.h } else { 1. },
            );
            for p in out.get_mut(range).unwrap_or_default() {
                let project = |rect: &mut Xfrm| {
                    rect.x = style.rect.x + after.rect.x + (rect.x - style.rect.x - before.rect.x) * sx;
                    rect.y = style.rect.y + after.rect.y + (rect.y - style.rect.y - before.rect.y) * sy;
                    rect.w *= sx;
                    rect.h *= sy;
                };
                project(&mut p.rect);
                if let Some(rect) = &mut p.text_rect {
                    project(rect);
                }
            }
        }
        return Ok(());
    }
    // A composite is a container, not an invisible spacing shape. Linear layout packs
    // its actual children, so unused margins inside that container do not become gaps.
    // Retain the existing constrained-size path for rotated/overflowing containers.
    if matches!(node.alg.as_str(), "lin" | "cycle") {
        let mut occupied = children.to_vec();
        let mut changed = false;
        for ((child, bounds), range) in node.children.iter().zip(&mut occupied).zip(ranges) {
            if !(child.alg == "composite" || node.alg == "cycle" && matches!(child.alg.as_str(), "sp" | "lin")) || range.is_empty() {
                continue;
            }
            let shapes = out.get(range.clone()).unwrap_or(&[]);
            charge(work, shapes.len())?;
            if shapes.iter().any(|p| p.connector || node.alg == "lin" && p.rect.rot != 0.0) {
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
            let fits = |size: f64, limit: f64| size - limit <= 16. * f64::EPSILON * size.abs().max(limit.abs()).max(1.);
            if let Some(mut b) = box_rect.filter(|b| b.w > 0.0 && b.h > 0.0 && fits(b.w, bounds.rect.w) && fits(b.h, bounds.rect.h)) {
                b.x -= style.rect.x;
                b.y -= style.rect.y;
                changed |= b != bounds.rect;
                bounds.rect = b;
            }
        }
        if changed {
            let mut packed = occupied.clone();
            // These are already painted extents, not another allocation trial.
            if node.alg == "cycle" {
                pack_cycle(node, local, &mut packed, text_refit, work)?;
            } else {
                pack_linear(node, local, Some(local.rect), &mut packed, false);
            }
            for ((before, after), range) in occupied.iter().zip(&packed).zip(ranges.iter().cloned()) {
                for p in out.get_mut(range).unwrap_or(&mut []) {
                    if node.alg != "cycle" {
                        p.rect.x += after.rect.x - before.rect.x;
                        p.rect.y += after.rect.y - before.rect.y;
                        if let Some(t) = &mut p.text_rect {
                            t.x += after.rect.x - before.rect.x;
                            t.y += after.rect.y - before.rect.y;
                        }
                        continue;
                    }
                    let (sx, sy) = (
                        if before.rect.w > 0. { after.rect.w / before.rect.w } else { 1. },
                        if before.rect.h > 0. { after.rect.h / before.rect.h } else { 1. },
                    );
                    let move_frame = |r: &mut Xfrm| {
                        r.x = style.rect.x + after.rect.x + (r.x - style.rect.x - before.rect.x) * sx;
                        r.y = style.rect.y + after.rect.y + (r.y - style.rect.y - before.rect.y) * sy;
                        r.w *= sx;
                        r.h *= sy;
                    };
                    move_frame(&mut p.rect);
                    if let Some(t) = &mut p.text_rect {
                        move_frame(t);
                    }
                }
            }
        }
    }
    Ok(())
}

fn restore_linear_cross_extents(node: &Instance<'_>, occupied: &[BoxStyle], packed: &mut [BoxStyle], work: &mut usize) -> Result<bool, &'static str> {
    charge(work, node.children.len())?;
    let mut restored = false;
    for ((child, before), after) in node.children.iter().zip(occupied).zip(packed) {
        if child.alg != "lin" {
            continue;
        }
        let vertical = matches!(child.param("linDir", "fromL").as_str(), "fromT" | "fromB");
        let (cross, flow) = if vertical { (&mut after.rect.w, &mut after.rect.h) } else { (&mut after.rect.h, &mut after.rect.w) };
        let intrinsic = if vertical { before.rect.w } else { before.rect.h };
        if *cross < intrinsic {
            // The next linear pass keeps the already fitted flow geometry.
            // Restoring the cross extent triggers its finite flow search at
            // that allocation, rather than reloading the pre-fit constraints.
            *flow *= flow_fit_scale(*flow, *flow);
            *cross = intrinsic;
            restored = true;
        }
    }
    Ok(restored)
}

pub(super) fn self_constraints(node: &Instance<'_>, style: &mut BoxStyle, sizes: bool) {
    for constraint in
        node.constraints.iter().filter(|e| e.attr("for").unwrap_or("self") == "self" && !super::constraints::position(e.attr("type").unwrap_or("")))
    {
        if super::constraints::font_bound(constraint)
            || super::constraints::size_bound(constraint)
            || (!sizes && matches!(constraint.attr("type"), Some("w" | "h" | "diam")))
        {
            continue;
        }
        if let Some(value) = constraint_value(constraint, style) {
            let property = constraint.attr("type").unwrap_or("");
            let op = constraint.attr("op").unwrap_or("equ");
            // Initial child proportions must use the same positive size as the
            // final solver, including a negative authored local dimension.
            let value = if matches!(property, "w" | "h" | "diam") && !matches!(op, "gte" | "lte") { value.abs() } else { value };
            style.set(property, value, op);
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
    let gap = if matches!(node.alg.as_str(), "composite" | "lin") { 0.0 } else { style.get("sp").or(style.get("sibSp")).unwrap_or(0.0).max(0.0) };
    let (w, h) = if vertical { (r.w, ((r.h - gap * (n - 1.0)) / n).max(0.0)) } else { (((r.w - gap * (n - 1.0)) / n).max(0.0), r.h) };
    let mut rectangles: Vec<Xfrm> = match node.alg.as_str() {
        "lin" | "hierChild" => (0..count)
            .map(|i| {
                Xfrm::new(r.x + if vertical { 0.0 } else { i as f64 * (w + gap) }, r.y + if vertical { i as f64 * (h + gap) } else { 0.0 }, w, h)
            })
            .collect(),
        "cycle" => {
            let start = (node.number("stAng", 0.0) - 90.0).to_radians();
            let span = node.number("spanAng", 360.0).to_radians();
            let center = node.param("ctrShpMap", "none") != "none";
            let orbit = count.saturating_sub(usize::from(center)).max(1) as f64;
            let diameter = (r.w.min(r.h) / (1.0 + orbit / std::f64::consts::PI)).min(r.w.min(r.h) * 0.42);
            (0..count)
                .map(|i| {
                    if center && i == 0 {
                        return Xfrm::new(r.x + r.w / 2.0 - diameter / 2.0, r.y + r.h / 2.0 - diameter / 2.0, diameter, diameter);
                    }
                    let intervals = if (span.abs() - std::f64::consts::TAU).abs() < 1e-6 { orbit } else { (orbit - 1.0).max(1.0) };
                    let angle = start + ((i - usize::from(center)) as f64 / intervals) * span;
                    Xfrm::new(
                        r.x + r.w / 2.0 + (r.w - diameter) * 0.5 * angle.cos() - diameter / 2.0,
                        r.y + r.h / 2.0 + (r.h - diameter) * 0.5 * angle.sin() - diameter / 2.0,
                        diameter,
                        diameter,
                    )
                })
                .collect()
        }
        "snake" => vec![r; count],
        "pyra" => (0..count)
            .map(|i| {
                let width = r.w * (i + 1) as f64 / n;
                Xfrm::new(r.x + (r.w - width) / 2.0, r.y + i as f64 * r.h / n, width, r.h / n)
            })
            .collect(),
        "hierRoot" => vec![r; count],
        _ => vec![r; count],
    };
    if matches!(node.alg.as_str(), "lin" | "cycle" | "snake" | "hierChild" | "hierRoot") {
        for rect in &mut rectangles {
            rect.w = 100. * 72. / 25.4;
            rect.h = 100. * 72. / 25.4;
        }
    }
    rectangles
}

// The cycle's path direction rotates leaf shapes after allocation/shape fitting.
// Containers run their own algorithms; rotating their subtree here would also
// rotate independent text frames and change occupied bounds a second time.
fn rotate_cycle_leaves(node: &Instance<'_>, ranges: &[std::ops::Range<usize>], out: &mut [Placed<'_>], work: &mut usize) -> Result<(), &'static str> {
    if node.alg != "cycle" || node.param("rotPath", "none") != "alongPath" {
        return Ok(());
    }
    let center = node.param("ctrShpMap", "none") != "none";
    let orbit = node.children.len().saturating_sub(usize::from(center)).max(1);
    let span = node.number("spanAng", 360.).to_radians();
    let start = node.number("stAng", 0.).to_radians();
    let divisor = if (span.abs() - std::f64::consts::TAU).abs() < 1e-6 { orbit } else { orbit.saturating_sub(1).max(1) } as f64;
    for (i, (child, range)) in node.children.iter().zip(ranges).enumerate() {
        charge(work, 1)?;
        if !child.children.is_empty() || center && i == 0 || range.is_empty() {
            continue;
        }
        if let Some(p) = out.get_mut(range.start).filter(|p| p.node_key == super::constraints::id(child) && !p.connector) {
            // Convert bounded radians back to degrees so huge authored angles
            // cannot overflow an intermediate sum in degrees.
            let angle = start + (i.saturating_sub(usize::from(center)) as f64 / divisor) * span;
            p.path_rotation = angle.rem_euclid(std::f64::consts::TAU).to_degrees();
        }
    }
    Ok(())
}

// Only text algorithms map labels into the cycle. The data tree may also contain
// unconnected labels, document text or a composite's non-text mappings.
pub(super) fn cycle_has_text(node: &Instance<'_>, data: &Data<'_>, depth: usize, work: &mut usize) -> Result<bool, &'static str> {
    charge(work, 1)?;
    if depth > 64 {
        return Err("diagram layout depth exceeded");
    }
    if node.alg == "tx" {
        for point in &node.points {
            charge(work, 1)?;
            if let Some(point) = data.points.get(*point) {
                charge(work, point.children.len())?;
                let Some(body) = point.child("t") else { continue };
                charge(work, body.children.len())?;
                for paragraph in body.children_named("p") {
                    charge(work, paragraph.children.len())?;
                    for run in paragraph.elements().filter(|r| matches!(r.local(), "r" | "fld")) {
                        charge(work, run.children.len())?;
                        if let Some(text) = run.child("t") {
                            charge(work, text.children.len())?;
                            for child in &text.children {
                                if let crate::xml::Node::Text(value) = child {
                                    charge(work, value.len())?;
                                    if value.chars().any(cycle_text_character) {
                                        return Ok(true);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    for child in &node.children {
        if cycle_has_text(child, data, depth + 1, work)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn cycle_text_character(c: char) -> bool {
    use unicode_general_category::{GeneralCategory as G, get_general_category};
    // Breakable spaces and standalone zero-advance marks do not start the text
    // fitting pass. Nonbreaking spaces and run tabs/newlines do. An a:br without
    // any run content is distinct from a literal newline inside a:t.
    if matches!(c, '\t' | '\r' | '\n' | '\u{a0}' | '\u{2007}' | '\u{202f}') {
        return true;
    }
    !matches!(
        get_general_category(c),
        G::SpaceSeparator | G::LineSeparator | G::ParagraphSeparator | G::NonspacingMark | G::EnclosingMark | G::Format | G::Control
    )
}

// Cycle angles start at twelve o'clock and turn clockwise (ECMA-376 ST_ParameterId).
// Fit a circle against the actual child extents; an incomplete arc is centred by its
// occupied bounds, not the centre of the unoccupied remainder of the circle.
fn pack_cycle(node: &Instance<'_>, style: BoxStyle, children: &mut [BoxStyle], has_text: bool, work: &mut usize) -> Result<(), &'static str> {
    if children.is_empty() {
        return Ok(());
    }
    let center = node.param("ctrShpMap", "none") != "none";
    let orbit = children.len().saturating_sub(usize::from(center)).max(1);
    let start = node.number("stAng", 0.0).to_radians();
    let span = node.number("spanAng", 360.0).to_radians();
    let full_circle = (span.abs() - std::f64::consts::TAU).abs() < 1e-6;
    let divisor = if full_circle { orbit } else { orbit.saturating_sub(1).max(1) } as f64;
    let units: Vec<_> = (0..children.len())
        .map(|i| {
            if center && i == 0 {
                (0., 0.)
            } else {
                let angle = start + (i.saturating_sub(usize::from(center)) as f64 / divisor) * span;
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
    let limit = cycle_radius_limit(style.rect, start, span, divisor);
    let trial = |scale: f64, upper: f64, work: &mut usize| -> Result<(bool, f64), &'static str> {
        let radius = cycle_fit_radius(children, &units, style.rect, scale, upper, work)?;
        let fits = children.iter().all(|c| c.rect.w * scale <= style.rect.w && c.rect.h * scale <= style.rect.h);
        // Tangency is not a successful size trial: native controls reduce exactly
        // touching nodes. Coincident/zero-size nodes have no radial separation.
        Ok((fits && (minimum_radius == 0. || radius > minimum_radius * scale), radius))
    };
    let (fits, first_radius) = trial(1., limit, work)?;
    let (scale, radius) = if fits {
        // Text fitting revisits successful geometry once. Empty mapped text does
        // not trigger that pass; unrelated or unmapped data must not trigger it.
        (1., if has_text { cycle_fit_radius(children, &units, style.rect, 1., first_radius, work)? } else { first_radius })
    } else {
        let (mut low, mut high) = (0., 1.);
        // Bracket probes do not commit geometry. Accepted size trials below do:
        // retain their radius as the upper bound of the next trial, and roll it
        // back when a trial fails. Resetting it each time changes both node size
        // and arc extent, particularly for short arcs and oversized nodes.
        for _ in 0..64 {
            if trial(high * 0.5, limit, work)?.0 {
                break;
            }
            high *= 0.5;
        }
        let (mut upper, mut best_radius) = (limit, 0.);
        for _ in 0..10 {
            let mid = (low + high) / 2.;
            let (fits, radius) = trial(mid, upper, work)?;
            if fits {
                low = mid;
                upper = radius;
                best_radius = radius;
            } else {
                high = mid;
            }
        }
        (low, best_radius)
    };
    let low = radius.max(minimum_radius * scale);
    for child in children.iter_mut() {
        child.rect.w *= scale;
        child.rect.h *= scale;
    }
    let b = cycle_bounds(children, &units, 1., low);
    let cx = style.rect.x + (style.rect.w - b.0 - b.2) / 2.;
    let cy = style.rect.y + (style.rect.h - b.1 - b.3) / 2.;
    for (child, (x, y)) in children.iter_mut().zip(units) {
        child.rect.x = cx + x * low - child.rect.w / 2.;
        child.rect.y = cy + y * low - child.rect.h / 2.;
    }
    Ok(())
}

/// The circular path includes its extrema between nodes, even where no node is
/// painted. A closed path adds the closing interval. With fewer than three
/// intervals the native search uses a perimeter-based initial radius instead.
fn cycle_radius_limit(frame: Xfrm, start: f64, span: f64, intervals: f64) -> f64 {
    if intervals < 3. {
        return (frame.w + frame.h) / intervals.max(1.);
    }
    if span.abs() >= std::f64::consts::TAU {
        return frame.w.min(frame.h) / 2.;
    }
    // Normalize before checking a bounded set of cardinal extrema. Authored
    // angles may be huge or reversed; no loop count depends on their magnitude.
    let start = start.rem_euclid(std::f64::consts::TAU);
    // Radius capacity uses the forward arc magnitude; placement above keeps
    // the authored winding. Reversed asymmetric native arcs distinguish these.
    let end = start + span.abs();
    let (lo, hi) = (start.min(end), start.max(end));
    let (mut left, mut right) = (start.sin().min(end.sin()), start.sin().max(end.sin()));
    let (mut top, mut bottom) = ((-start.cos()).min(-end.cos()), (-start.cos()).max(-end.cos()));
    for quadrant in -4..=8 {
        let angle = f64::from(quadrant) * std::f64::consts::FRAC_PI_2;
        if angle >= lo && angle <= hi {
            left = left.min(angle.sin());
            right = right.max(angle.sin());
            top = top.min(-angle.cos());
            bottom = bottom.max(-angle.cos());
        }
    }
    let x = if right - left > 1e-9 { frame.w / (right - left) } else { f64::INFINITY };
    let y = if bottom - top > 1e-9 { frame.h / (bottom - top) } else { f64::INFINITY };
    let radius = x.min(y);
    if radius.is_finite() { radius.max(0.) } else { 0. }
}

fn cycle_bounds(children: &[BoxStyle], units: &[(f64, f64)], scale: f64, radius: f64) -> (f64, f64, f64, f64) {
    let mut extents = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (child, (x, y)) in children.iter().zip(units) {
        extents.0 = extents.0.min(x * radius - child.rect.w * scale / 2.);
        extents.1 = extents.1.min(y * radius - child.rect.h * scale / 2.);
        extents.2 = extents.2.max(x * radius + child.rect.w * scale / 2.);
        extents.3 = extents.3.max(y * radius + child.rect.h * scale / 2.);
    }
    extents
}

fn cycle_fit_radius(
    children: &[BoxStyle],
    units: &[(f64, f64)],
    frame: Xfrm,
    scale: f64,
    mut high: f64,
    work: &mut usize,
) -> Result<f64, &'static str> {
    let mut low = 0.;
    for _ in 0..10 {
        charge(work, children.len())?;
        let radius = (low + high) / 2.;
        let b = cycle_bounds(children, units, scale, radius);
        if b.2 - b.0 <= frame.w && b.3 - b.1 <= frame.h {
            low = radius;
        } else {
            high = radius;
        }
    }
    Ok(low)
}

pub(super) fn pack_linear(node: &Instance<'_>, style: BoxStyle, capacity: Option<Xfrm>, children: &mut [BoxStyle], force_fit: bool) {
    let r = style.rect;
    let default_dir = if node.alg == "hierChild" && node.param("chDir", "horz") == "vert" { "fromT" } else { "fromL" };
    let dir = node.param("linDir", default_dir);
    let vertical = matches!(dir.as_str(), "fromT" | "fromB");
    let backwards = matches!(dir.as_str(), "fromR" | "fromB");
    // A linear spacer participates as a child allocation. A bare self `sp`
    // property does not insert additional gaps between the children.
    let gap = if node.alg == "lin" { 0.0 } else { style.get("sp").or(style.get("sibSp")).unwrap_or(0.0).max(0.0) };
    let gaps = gap * children.len().saturating_sub(1) as f64;
    let sum: f64 = children.iter().map(|b| if vertical { b.rect.h } else { b.rect.w }).sum();
    let available = if vertical { r.h } else { r.w };
    let exceeds = |size: f64, limit: f64| size - limit > 16. * f64::EPSILON * size.abs().max(limit.abs()).max(1.);
    let cross_overflow = children.iter().any(|b| if vertical { exceeds(b.rect.w, r.w) } else { exceeds(b.rect.h, r.h) });
    let scale = if node.alg == "lin" && (force_fit || exceeds(sum + gaps, available) || cross_overflow) {
        flow_fit_scale(sum + gaps, available)
    } else if sum + gaps > available {
        available / (sum + gaps).max(0.01)
    } else {
        1.0
    };
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
            b.w = b.w.min(if node.alg == "lin" { capacity.map_or(b.w, |c| c.w) } else { r.w });
            b.h *= scale;
            b.y = r.y + if backwards { r.h - pos - b.h } else { pos };
            b.x = r.x + (r.w - b.w) / 2.0;
            pos += b.h + gap * scale;
        } else {
            b.h = b.h.min(if node.alg == "lin" { capacity.map_or(b.h, |c| c.h) } else { r.h });
            b.w *= scale;
            b.x = r.x + if backwards { r.w - pos - b.w } else { pos };
            b.y = r.y + (r.h - b.h) / 2.0;
            pos += b.w + gap * scale;
        }
    }
}

/// Reproduce the observed finite linear fit with a normalized ten-trial search.
/// Normalizing the bracket preserves relative precision for very small scales.
pub(super) fn flow_fit_scale(occupied: f64, available: f64) -> f64 {
    if occupied <= 0. || available <= 0. {
        return if available > 0. { 1. } else { 0. };
    }
    // A mathematically exact endpoint remains feasible after point conversion
    // and occupied-bound subtraction introduce a few rounding bits.
    let fits = |scale: f64| {
        let extent = occupied * scale;
        extent - available <= 16. * f64::EPSILON * extent.abs().max(available.abs())
    };
    let (mut low, mut high) = (0., 1.);
    for _ in 0..64 {
        if fits(high * 0.5) {
            break;
        }
        high *= 0.5;
    }
    for _ in 0..10 {
        let mid = (low + high) / 2.;
        if fits(mid) {
            low = mid;
        } else {
            high = mid;
        }
    }
    low
}

/// Resolve transition identities after all node bounds are known. Connectors are kept behind
/// nodes, and use actual boundary attachment points rather than an arbitrary canvas diagonal.
pub(super) fn connect(out: &mut [Placed<'_>], data: &Data<'_>) {
    use std::collections::HashMap;
    let mut counts = HashMap::<&str, usize>::new();
    for p in out.iter_mut() {
        // Geometry-free text uses the first colour without consuming a slot.
        // Connectors do consume one, in layout order rather than paint order.
        p.color_index = 0;
        if p.shape.is_some_and(|s| s.bool("hideGeom") != Some(true)) {
            let next = counts.entry(&p.style).or_default();
            p.color_index = *next;
            *next += 1;
        }
    }
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

#[cfg(test)]
mod cycle_tests {
    use super::*;

    #[test]
    fn cycle_text_mapping_walks_nested_algorithms_and_limits_empty_text_work() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let source = crate::xml::parse(b"<dataModel><ptLst><pt type='doc' modelId='root'/><pt modelId='a'><t><p><fld><t>Label</t></fld></p></t></pt></ptLst><cxnLst/></dataModel>").unwrap();
        let data = Data::new(&source.root).unwrap();
        let mut node = Instance::new(&xml.root);
        node.points = vec![1];
        assert!(!cycle_has_text(&node, &data, 0, &mut 0).unwrap());
        let mut child = Instance::new(&xml.root);
        child.alg = "tx".into();
        child.points = vec![1];
        node.children.push(child);
        assert!(cycle_has_text(&node, &data, 0, &mut 0).unwrap());
        assert!(cycle_has_text(&node, &data, 64, &mut 0).is_err());
        let mut work = MAX_WORK;
        assert!(cycle_has_text(&node, &data, 0, &mut work).is_err());
        let empty = " ".repeat(MAX_WORK + 1);
        for body in [format!("<t><p><r><t>{empty}</t></r></p></t>"), "<spPr/>".repeat(MAX_WORK + 1)] {
            let source = crate::xml::parse(
                format!("<dataModel><ptLst><pt type='doc' modelId='root'/><pt modelId='a'>{body}</pt></ptLst><cxnLst/></dataModel>").as_bytes(),
            )
            .unwrap();
            let data = Data::new(&source.root).unwrap();
            assert!(cycle_has_text(&node, &data, 0, &mut 0).is_err());
        }
    }

    #[test]
    fn an_empty_cycle_with_a_center_mapping_has_no_allocations() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let mut node = Instance::new(&xml.root);
        node.alg = "cycle".into();
        node.params.insert("ctrShpMap".into(), "fNode".into());
        let style = BoxStyle::new(Xfrm::new(0., 0., 100., 80.));
        assert!(initial(&node, style).is_empty());
        pack_cycle(&node, style, &mut [], false, &mut 0).unwrap();
    }

    #[test]
    fn partial_cycles_preserve_successful_radius_trials_across_size_search() {
        // Independent native observations: several of these layouts retain empty
        // space instead of maximizing the final radius after the size search.
        let controls = [
            (3, false, [77.3, 43.7], [18.838445, 10.148512, 10.194724, 6.920519], [42.164861, 26.630925, 16.296674, 6.920519]),
            (3, false, [143.7, 91.3], [37.210135, 24.915935, 13.7, 9.3], [84.589844, 57.084008, 21.9, 9.3]),
            (4, false, [143.7, 91.3], [2.753418, 0.053994, 13.7, 9.3], [127.246575, 73.745986, 13.7, 17.5]),
            (6, false, [143.7, 91.3], [0.703415, 0.053994, 13.7, 9.3], [121.096563, 73.745986, 21.9, 17.5]),
            (8, false, [143.7, 91.3], [1.547665, 0.038362, 12.950780, 8.791399], [125.325730, 74.718634, 16.826546, 16.542950]),
            (8, true, [77.3, 43.7], [21.502058, 42.206740, 2.194136, 1.489455], [52.947127, 11.920906, 2.850795, 2.802747]),
        ];
        let mm = 72. / 25.4;
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        for (count, center, frame, first, last) in controls {
            for (x, y) in [(0., 0.), (31., 17.)] {
                let mut node = Instance::new(&xml.root);
                node.params.insert("ctrShpMap".into(), if center { "fNode" } else { "none" }.into());
                node.params.insert("stAng".into(), "17".into());
                node.params.insert("spanAng".into(), "30".into());
                let style = BoxStyle::new(Xfrm::new(x, y, frame[0] * mm, frame[1] * mm));
                let mut children: Vec<_> = (0..count)
                    .map(|i| BoxStyle::new(Xfrm::new(0., 0., (13.7 + (i % 3) as f64 * 4.1) * mm, (9.3 + (i % 2) as f64 * 8.2) * mm)))
                    .collect();
                pack_cycle(&node, style, &mut children, true, &mut 0).unwrap();
                for (index, expected) in [(0, first), (count - 1, last)] {
                    let r = children[index].rect;
                    for (actual, native) in [r.x - x, r.y - y, r.w, r.h].into_iter().zip(expected) {
                        assert!((actual - native * mm).abs() < 0.025, "{count}/{center}: {actual}, {native}");
                    }
                }
            }
        }
    }

    #[test]
    fn reversed_cycle_capacity_uses_magnitude_but_nodes_keep_their_winding() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let mut node = Instance::new(&xml.root);
        for (k, v) in [("ctrShpMap", "fNode"), ("stAng", "17"), ("spanAng", "-137")] {
            node.params.insert(k.into(), v.into());
        }
        let mm = 72. / 25.4;
        let sizes = [[57.166, 60.170], [83.982, 19.806], [55.932, 39.923], [57.984, 54.575], [12.621, 26.657], [96.400, 37.264]];
        let mut children: Vec<_> = sizes.iter().map(|s| BoxStyle::new(Xfrm::new(0., 0., s[0] * mm, s[1] * mm))).collect();
        pack_cycle(&node, BoxStyle::new(Xfrm::new(0., 0., 35.7 * mm, 21.9 * mm)), &mut children, true, &mut 0).unwrap();
        for (index, expected) in [(0, [18.700332, 10.713734, 5.652390, 5.949422]), (5, [6.586778, 17.720046, 9.531721, 3.684551])] {
            let r = children[index].rect;
            for (actual, native) in [r.x, r.y, r.w, r.h].into_iter().zip(expected) {
                assert!((actual - native * mm).abs() < 0.025);
            }
        }
    }

    #[test]
    fn exactly_tangent_cycle_nodes_trigger_size_search() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let mm = 72. / 25.4;
        for span in ["180", "360"] {
            let mut node = Instance::new(&xml.root);
            node.params.insert("spanAng".into(), span.into());
            let mut children = vec![BoxStyle::new(Xfrm::new(0., 0., 60. * mm, 60. * mm)); 2];
            pack_cycle(&node, BoxStyle::new(Xfrm::new(0., 0., 200. * mm, 120. * mm)), &mut children, true, &mut 0).unwrap();
            assert!((children[0].rect.w / mm - 59.94140625).abs() < 1e-6);
            let radius = (children[1].rect.y - children[0].rect.y) / 2.;
            assert!((radius - 30.0098715 * mm).abs() < 0.025);
        }
    }

    #[test]
    fn degenerate_and_huge_cycle_angles_keep_finite_bounded_geometry() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        for (start, span) in [(0., 0.), (1e308, 1e308), (-1e308, -1e308), (720., -360.)] {
            for (w, h) in [(100., 80.), (0., 0.)] {
                let mut node = Instance::new(&xml.root);
                node.params.insert("stAng".into(), start.to_string());
                node.params.insert("spanAng".into(), span.to_string());
                let mut children = vec![BoxStyle::new(Xfrm::new(0., 0., 15., 12.)); 4];
                let mut work = 0;
                pack_cycle(&node, BoxStyle::new(Xfrm::new(0., 0., w, h)), &mut children, true, &mut work).unwrap();
                assert!(work <= MAX_WORK);
                for child in children {
                    let r = child.rect;
                    assert!([r.x, r.y, r.w, r.h].into_iter().all(f64::is_finite));
                    assert!(r.x >= -1e-6 && r.y >= -1e-6 && r.x + r.w <= w + 1e-6 && r.y + r.h <= h + 1e-6);
                }
            }
        }
    }

    #[test]
    fn nonshrinking_circle_positions_match_independent_native_controls() {
        // Mixed rectangular nodes, fractional frames, several rotations and
        // centre mappings. Expected first/last rectangles are native observations
        // in millimetres, not the output of the fit under test.
        let controls = [
            (2, true, -37., [95.34509, 81.95907, 13.7, 9.3], [34.65489, 0.04090, 17.8, 17.5]),
            (3, false, -37., [27.65472, 1.83721, 13.7, 9.3], [33.17192, 80.16277, 21.9, 9.3]),
            (4, false, 0., [63.97496, 0.07092, 13.7, 9.3], [23.04592, 36.90000, 13.7, 17.5]),
            (4, true, 63., [62.78275, 32.70788, 13.7, 9.3], [24.57217, 3.79365, 13.7, 17.5]),
            (6, false, 0., [64.99995, 0.06958, 13.7, 9.3], [27.22857, 15.40978, 21.9, 17.5]),
            (6, true, 63., [66.89674, 42.89674, 13.7, 9.3], [56.65874, 0.04312, 21.9, 17.5]),
            (8, false, 0., [62.94996, 0.07092, 13.7, 9.3], [31.95875, 7.95876, 17.8, 17.5]),
            (8, true, 63., [66.68772, 41.18876, 13.7, 9.3], [72.22231, 0.04574, 17.8, 17.5]),
        ];
        let mm = 72. / 25.4;
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        for (count, center, angle, first, last) in controls {
            for (x, y) in [(0., 0.), (11.3, 27.9)] {
                let mut node = Instance::new(&xml.root);
                node.params.insert("ctrShpMap".into(), if center { "fNode" } else { "none" }.into());
                node.params.insert("stAng".into(), angle.to_string());
                let style = BoxStyle::new(Xfrm::new(x, y, 143.7 * mm, 91.3 * mm));
                let mut children: Vec<_> = (0..count)
                    .map(|i| BoxStyle::new(Xfrm::new(0., 0., (13.7 + (i % 3) as f64 * 4.1) * mm, (9.3 + (i % 2) as f64 * 8.2) * mm)))
                    .collect();
                pack_cycle(&node, style, &mut children, true, &mut 0).unwrap();
                for (index, expected) in [(0, first), (count - 1, last)] {
                    let r = children[index].rect;
                    for (actual, native) in [r.x - x, r.y - y, r.w, r.h].into_iter().zip(expected) {
                        assert!((actual - native * mm).abs() < 0.025, "{count}/{center}/{angle}: {actual}, {native}");
                    }
                }
                for (i, child) in children.iter().enumerate() {
                    let a = child.rect;
                    assert!(a.x >= x && a.y >= y && a.x + a.w <= x + style.rect.w && a.y + a.h <= y + style.rect.h);
                    for other in &children[..i] {
                        let b = other.rect;
                        assert!(a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y);
                    }
                }
            }
        }
    }

    #[test]
    fn a_single_orbiting_node_uses_the_same_extent_for_every_span() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let mm = 72. / 25.4;
        for span in [-360., -30., 0., 90., 180., 360.] {
            let mut node = Instance::new(&xml.root);
            node.params.insert("ctrShpMap".into(), "fNode".into());
            node.params.insert("spanAng".into(), span.to_string());
            let style = BoxStyle::new(Xfrm::new(0., 0., 200. * mm, 120. * mm));
            let mut children = vec![BoxStyle::new(Xfrm::new(0., 0., 25. * mm, 25. * mm)); 2];
            pack_cycle(&node, style, &mut children, true, &mut 0).unwrap();
            // Two independent native controls (full and half span): 94.90724 mm.
            let radius = children[0].rect.y - children[1].rect.y;
            assert!((radius - 94.90724 * mm).abs() < 0.025);
        }
    }

    #[test]
    fn cycle_radius_trials_share_the_constraint_work_budget() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let node = Instance::new(&xml.root);
        let style = BoxStyle::new(Xfrm::new(0., 0., 800., 400.));
        let mut children = vec![BoxStyle::new(Xfrm::new(0., 0., 10., 8.)); 3];
        let mut work = MAX_WORK - 4;
        assert_eq!(pack_cycle(&node, style, &mut children, true, &mut work), Err("diagram constraint budget exceeded"));
        assert_eq!(work, MAX_WORK + 2);
    }

    #[test]
    fn dense_cycle_pair_search_obeys_the_shared_work_budget() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let node = Instance::new(&xml.root);
        let style = BoxStyle::new(Xfrm::new(0., 0., 800., 400.));
        let mut children = vec![BoxStyle::new(Xfrm::new(0., 0., 100., 80.)); 1000];
        let mut work = 0;
        assert_eq!(pack_cycle(&node, style, &mut children, true, &mut work), Err("diagram constraint budget exceeded"));
        assert_eq!(work, MAX_WORK + 1);
    }
}
