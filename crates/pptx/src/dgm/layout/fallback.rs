//! Native recovery for invalid font-reference graphs. This is separate from a failed
//! numeric search: malformed data and exhausted work budgets still return errors.
use super::{Data, Instance, MAX_INSTANCES, MAX_WORK, Placed, rules};
use crate::xml::El;
use deckcraft_geom::Xfrm;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FontKey<'a> {
    axis: &'a str,
    name: &'a str,
    point: &'a str,
    property: &'a str,
}
impl<'a> FontKey<'a> {
    fn of(c: &'a El, reference: bool) -> Self {
        let (axis, name, point, property) =
            if reference { ("refFor", "refForName", "refPtType", "refType") } else { ("for", "forName", "ptType", "type") };
        Self {
            axis: c.attr(axis).unwrap_or("self"),
            name: c.attr(name).unwrap_or(""),
            point: c.attr(point).unwrap_or("all"),
            property: c.attr(property).unwrap_or(""),
        }
    }
    fn own(property: &'a str) -> Self {
        Self { axis: "self", name: "", point: "all", property }
    }
}
fn font(property: &str) -> bool {
    matches!(property, "primFontSz" | "secFontSz")
}
fn charge(work: &mut usize, amount: usize) -> Result<(), &'static str> {
    *work = work.saturating_add(amount);
    if *work > MAX_WORK { Err("diagram fallback work budget exceeded") } else { Ok(()) }
}

fn overlaps(node: &Instance<'_>, a: FontKey<'_>, b: FontKey<'_>, data: &Data<'_>, work: &mut usize) -> Result<bool, &'static str> {
    let matches = |key: FontKey<'_>, n: &Instance<'_>, depth: usize| {
        (match key.axis {
            "ch" => depth == 1,
            "des" => depth > 0,
            _ => depth == 0,
        }) && (key.name.is_empty() || key.name == n.name)
            && (key.point == "all" || n.points.iter().any(|p| data.matches(*p, key.point)))
    };
    let mut pending = vec![(node, 0usize)];
    while let Some((current, depth)) = pending.pop() {
        charge(work, 1 + current.children.len() + current.points.len())?;
        if matches(a, current, depth) && matches(b, current, depth) {
            return Ok(true);
        }
        pending.extend(current.children.iter().map(|child| (child, depth + 1)));
    }
    Ok(false)
}

pub(super) fn required(tree: &Instance<'_>, data: &Data<'_>, work: &mut usize) -> Result<bool, &'static str> {
    let mut pending = vec![tree];
    while let Some(node) = pending.pop() {
        charge(work, node.constraints.len() + node.children.len() + 1)?;
        pending.extend(&node.children);
        let mut declarations = HashMap::new();
        for c in &node.constraints {
            if !font(c.attr("type").unwrap_or("")) || matches!(c.attr("op"), Some("gte" | "lte")) {
                continue;
            }
            // References resolve declarations at this owner, not the effective
            // values eventually allocated to descendant objects by other scopes.
            if declarations.insert(FontKey::of(c, false), *c).is_some() {
                return Ok(true);
            }
        }
        let mut dependencies = HashMap::new();
        for (&key, c) in &declarations {
            if !font(c.attr("refType").unwrap_or("")) || c.attr("op") == Some("equ") && c.f64("fact") == Some(0.) {
                continue;
            }
            let reference = FontKey::of(c, true);
            if key.axis != reference.axis {
                return Ok(true);
            }
            if key.property == reference.property && overlaps(node, key, reference, data, work)? {
                return Ok(true);
            }
            if !declarations.contains_key(&reference) {
                if reference == FontKey::own("primFontSz") {
                    // A local text algorithm has a primary default. A remote
                    // child/descendant declaration does not manufacture one.
                    continue;
                }
                if reference == FontKey::own("secFontSz") {
                    dependencies.insert(reference, FontKey::own("primFontSz"));
                } else {
                    return Ok(true);
                }
            }
            dependencies.insert(key, reference);
        }
        let mut complete = HashSet::new();
        for key in dependencies.keys() {
            let mut path = HashSet::new();
            let mut next = Some(*key);
            while let Some(current) = next.filter(|k| !complete.contains(k)) {
                charge(work, 1)?;
                if !path.insert(current) {
                    return Ok(true);
                }
                next = dependencies.get(&current).copied();
            }
            complete.extend(path);
        }
    }
    Ok(false)
}

static RECTANGLE: LazyLock<El> = LazyLock::new(|| {
    let mut shape = El { name: "shape".into(), ..Default::default() };
    shape.set_attr("type", "rect");
    shape
});

fn rectangles(frame: Xfrm, count: usize, work: &mut usize) -> Result<Vec<Xfrm>, &'static str> {
    if count == 0 || count > MAX_INSTANCES {
        return Err("invalid diagram fallback node count");
    }
    if !frame.is_finite() || frame.w <= 0. || frame.h <= 0. || !(frame.x + frame.w).is_finite() || !(frame.y + frame.h).is_finite() {
        return Err("invalid diagram fallback frame");
    }
    let (mut columns, mut capacity) = (1, 0.0_f64);
    for candidate in 1..=count {
        charge(work, 1)?;
        let rows = count.div_ceil(candidate);
        // Search in units of the frame width. Integer ratios avoid losing an
        // exactly fitting trial through a point/mm conversion (e.g. 5/16).
        let width = (10. / (11 * candidate - 1) as f64).min((frame.h / frame.w) * (10. / (7 * rows - 1) as f64));
        // Equal-height candidates fill the row before starting another one.
        if width >= capacity - 16. * f64::EPSILON * width.abs().max(1.) {
            columns = candidate;
            capacity = width;
        }
    }
    let rows = count.div_ceil(columns);
    let scale = if capacity >= 1. { 1. } else { super::geometry::flow_fit_scale(1., capacity) };
    let width = frame.w * scale;
    let height = width * 0.6;
    let gap = width * 0.1;
    let top = frame.y + (frame.h - rows as f64 * height - rows.saturating_sub(1) as f64 * gap) / 2.;
    Ok((0..count)
        .map(|index| {
            let row = index / columns;
            let in_row = columns.min(count - row * columns);
            let left = frame.x + (frame.w - in_row as f64 * width - in_row.saturating_sub(1) as f64 * gap) / 2.;
            Xfrm::new(left + (index % columns) as f64 * (width + gap), top + row as f64 * (height + gap), width, height)
        })
        .collect())
}

fn set_font(placed: &mut [Placed<'_>], value: f64) {
    for p in placed {
        p.font = Some(value);
        p.secondary_font = Some((value * 0.78).round());
        p.margins = [value * 0.3; 4];
    }
}

pub(super) fn place<'a>(
    data: &Data<'a>,
    frame: Xfrm,
    measure: Option<&mut rules::Measure<'_>>,
    work: &mut usize,
) -> Result<Vec<Placed<'a>>, &'static str> {
    let selector = |axis: &str| {
        let mut el = El::default();
        el.set_attr("axis", axis);
        el.set_attr("ptType", "node");
        el
    };
    let roots = data.select_bounded(&selector("ch"), data.root, work)?;
    let boxes = rectangles(frame, roots.len(), work)?;
    let mut placed = Vec::with_capacity(roots.len());
    for (index, (point, rect)) in roots.into_iter().zip(boxes).enumerate() {
        let points = data.select_bounded(&selector("desOrSelf"), point, work)?;
        let base = data.depth(point);
        let mut levels = Vec::with_capacity(points.len());
        for &p in &points {
            let depth = data.depth(p);
            charge(work, depth + 1)?;
            levels.push(depth.saturating_sub(base));
        }
        placed.push(Placed {
            node_key: point,
            shape: Some(&RECTANGLE),
            recovered: true,
            points: points.iter().filter_map(|i| data.points.get(*i).copied()).collect(),
            point_levels: levels,
            name: format!("Recovered diagram node {}", index + 1),
            style: "node1".into(),
            color_index: index,
            rect,
            ignore_bounds: false,
            path_rotation: 0.,
            text_rect: None,
            text_source: None,
            text_enabled: true,
            font: Some(65.),
            secondary_font: None,
            min_font: None,
            margins: [0.; 4],
            text_rotation: 0.,
            connector: false,
            params: HashMap::new(),
            endpoints: None,
        });
    }
    set_font(&mut placed, 65.);
    if let Some(measure) = measure {
        let mut fits = |placed: &[Placed<'_>]| -> Result<bool, &'static str> {
            charge(work, placed.len())?;
            let scores = measure(placed, super::rules::MeasureKind::WordWrap)?;
            if scores.len() != placed.len() || scores.iter().any(|s| !s.is_finite() || *s < 0.) {
                return Err("invalid diagram text measurement");
            }
            Ok(scores.iter().all(|s| *s <= 1e-6))
        };
        if !fits(&placed)? {
            let (mut low, mut high) = (5., 65.);
            while high - low > 1. {
                let mid = ((low + high) / 2.0_f64).floor();
                set_font(&mut placed, mid);
                if fits(&placed)? {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            set_font(&mut placed, low);
        }
    }
    Ok(placed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_grids_match_independent_native_boxes_and_translation() {
        for (count, size, first) in [
            (3, [200., 120.], [3.10058, 0.01465, 92.28516, 55.37109]),
            (4, [150., 60.], [1.17188, 0.00732, 46.14258, 27.68555]),
            (5, [200., 120.], [0., 19.375, 62.5, 37.5]),
            (8, [60., 150.], [0.00732, 36.43799, 28.56445, 17.13867]),
        ] {
            let mm = 72. / 25.4;
            for origin in [[0., 0.], [-13., 27.]] {
                let out = rectangles(Xfrm::new(origin[0], origin[1], size[0] * mm, size[1] * mm), count, &mut 0).unwrap();
                assert_eq!(out.len(), count);
                for (actual, expected) in [out[0].x - origin[0], out[0].y - origin[1], out[0].w, out[0].h].into_iter().zip(first) {
                    assert!((actual - expected * mm).abs() < 0.01, "{count}/{size:?}: {actual} vs {}", expected * mm);
                }
            }
        }
        assert!(rectangles(Xfrm::new(0., 0., 100., 100.), 0, &mut 0).is_err());
        assert!(rectangles(Xfrm::new(0., 0., 100., 100.), MAX_INSTANCES + 1, &mut 0).is_err());
        let mut exhausted = MAX_WORK;
        assert!(rectangles(Xfrm::new(0., 0., 100., 100.), 3, &mut exhausted).is_err());
        assert!(rectangles(Xfrm::new(f64::MAX, 0., f64::MAX, 100.), 3, &mut 0).is_err());
    }
}
