//! Root linear flows fit the occupied output of their child algorithms.
use super::{Instance, MAX_WORK, Placed, geometry::BoxStyle};
use deckcraft_geom::Xfrm;

fn charge(work: &mut usize, amount: usize) -> Result<(), &'static str> {
    *work = work.saturating_add(amount);
    if *work > MAX_WORK { Err("diagram root linear budget exceeded") } else { Ok(()) }
}

/// Resolve child algorithms before assigning root flow positions. Nested flows
/// retain their allocation pass, including the ancestor's cross-axis capacity.
pub(super) fn prepare(
    node: &Instance<'_>,
    local: BoxStyle,
    capacity: Option<Xfrm>,
    children: &mut [BoxStyle],
    root: bool,
    refit: bool,
    work: &mut usize,
) -> Result<bool, &'static str> {
    if root {
        charge(work, children.len())?;
        for child in children {
            child.rect.x = 0.;
            child.rect.y = 0.;
        }
        Ok(refit)
    } else {
        super::geometry::pack_linear(node, local, capacity, children, refit);
        Ok(false)
    }
}

/// Cross overflow changes a child's allocation before its algorithm runs again.
/// Unused allocation space alone does not request a fit. The caller retains the
/// overflow signal for the subsequent finite flow fit, after these children fit.
pub(super) fn constrain_cross(
    node: &Instance<'_>,
    style: BoxStyle,
    children: &mut [BoxStyle],
    ranges: &[std::ops::Range<usize>],
    out: &[Placed<'_>],
    work: &mut usize,
) -> Result<Vec<usize>, &'static str> {
    charge(work, children.len())?;
    if children.len() != ranges.len() {
        return Err("invalid root linear child count");
    }
    let vertical = matches!(node.param("linDir", "fromL").as_str(), "fromT" | "fromB");
    let limit = if vertical { style.rect.w } else { style.rect.h };
    let mut clipped = Vec::new();
    for (i, (child, range)) in children.iter_mut().zip(ranges).enumerate() {
        let shapes = out.get(range.clone()).ok_or("invalid root linear child range")?;
        charge(work, shapes.len())?;
        let Some(bounds) = super::geometry::occupied_bounds(shapes) else { continue };
        let cross = if vertical { bounds.w } else { bounds.h };
        if cross - limit > 16. * f64::EPSILON * cross.abs().max(limit.abs()).max(1.) {
            if vertical {
                child.rect.w = child.rect.w.min(limit);
            } else {
                child.rect.h = child.rect.h.min(limit);
            }
            clipped.push(i);
        }
    }
    Ok(clipped)
}

pub(super) fn place(
    node: &Instance<'_>,
    style: BoxStyle,
    children: &[BoxStyle],
    ranges: &[std::ops::Range<usize>],
    out: &mut [Placed<'_>],
    text_refit: bool,
    work: &mut usize,
) -> Result<(), &'static str> {
    charge(work, children.len().saturating_mul(2))?;
    if children.len() != ranges.len() || !style.rect.is_finite() || style.rect.w < 0. || style.rect.h < 0. {
        return Err("invalid root linear frame or child count");
    }
    let mut occupied = children.to_vec();
    for (bounds, range) in occupied.iter_mut().zip(ranges) {
        let shapes = out.get(range.clone()).ok_or("invalid root linear child range")?;
        charge(work, shapes.len())?;
        if let Some(mut rect) = super::geometry::occupied_bounds(shapes) {
            rect.x -= style.rect.x;
            rect.y -= style.rect.y;
            bounds.rect = rect;
        }
        if !bounds.rect.is_finite() || bounds.rect.w < 0. || bounds.rect.h < 0. {
            return Err("invalid root linear occupied extent");
        }
    }
    let mut packed = occupied.clone();
    let local = BoxStyle { rect: Xfrm::new(0., 0., style.rect.w, style.rect.h), ..style };
    super::geometry::pack_linear(node, local, Some(local.rect), &mut packed, text_refit);
    for ((before, after), range) in occupied.iter().zip(&packed).zip(ranges) {
        let shapes = out.get_mut(range.clone()).ok_or("invalid root linear child range")?;
        charge(work, shapes.len().saturating_mul(2))?;
        let sx = if before.rect.w > 0. { after.rect.w / before.rect.w } else { 1. };
        let sy = if before.rect.h > 0. { after.rect.h / before.rect.h } else { 1. };
        let project = |rect: &mut Xfrm| {
            rect.x = style.rect.x + after.rect.x + (rect.x - style.rect.x - before.rect.x) * sx;
            rect.y = style.rect.y + after.rect.y + (rect.y - style.rect.y - before.rect.y) * sy;
            rect.w *= sx;
            rect.h *= sy;
        };
        for shape in shapes {
            project(&mut shape.rect);
            if let Some(rect) = &mut shape.text_rect {
                project(rect);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dgm::layout::{MeasureKind, evaluate_measured};

    fn layout(frame: (f64, f64), direction: &str, wrap: bool, zero: bool) -> Vec<(String, Xfrm)> {
        let source = crate::xml::parse(
            br#"<dataModel><ptLst><pt modelId="root" type="doc"/><pt modelId="a"/><pt modelId="b"/><pt modelId="c"><t><p><r><t>Cedarbranches</t></r></p></t></pt><pt modelId="d"/></ptLst>
            <cxnLst><cxn srcId="root" destId="a" srcOrd="0"/><cxn srcId="root" destId="b" srcOrd="1"/><cxn srcId="root" destId="c" srcOrd="2"/><cxn srcId="root" destId="d" srcOrd="3"/></cxnLst></dataModel>"#,
        )
        .unwrap();
        let width = if zero { 0. } else { 14. };
        let xml = format!(
            r#"<layoutDef><layoutNode><alg type="lin"><param type="linDir" val="{direction}"/></alg><constrLst>
            <constr type="w" for="ch" forName="group" val="110"/><constr type="h" for="ch" forName="group" val="90"/>
            <constr type="w" for="ch" forName="flat" val="21"/><constr type="h" for="ch" forName="flat" val="18"/>
            </constrLst><layoutNode name="group"><alg type="hierChild"/><constrLst>
            <constr type="w" for="ch" forName="first" val="{width}"/><constr type="h" for="ch" forName="first" val="26"/>
            <constr type="w" for="ch" forName="second" val="39"/><constr type="h" for="ch" forName="second" val="13"/>
            <constr type="w" for="ch" forName="third" val="23"/><constr type="h" for="ch" forName="third" val="41"/>
            </constrLst><layoutNode name="first"><alg type="tx"/><presOf axis="ch" st="1" cnt="1"/><shape type="rect"/></layoutNode>
            <layoutNode name="second"><alg type="tx"/><presOf axis="ch" st="2" cnt="1"/><shape type="rect"/></layoutNode>
            <layoutNode name="third"><alg type="tx"/><presOf axis="ch" st="3" cnt="1"/><shape type="rect"/></layoutNode></layoutNode>
            <layoutNode name="flat"><alg type="tx"/><presOf axis="ch" st="4" cnt="1"/><shape type="rect"/></layoutNode>
            </layoutNode></layoutDef>"#
        );
        let definition = crate::xml::parse(xml.as_bytes()).unwrap();
        let mut measure = |placed: &[Placed<'_>], kind: MeasureKind| {
            Ok(placed.iter().map(|p| if wrap && kind == MeasureKind::WordWrap && p.name == "third" { 1. } else { 0. }).collect())
        };
        let unit = 72. / 25.4;
        evaluate_measured(&definition.root, &source.root, Xfrm::new(0., 0., frame.0 * unit, frame.1 * unit), Some(&mut measure))
            .unwrap()
            .into_iter()
            .map(|p| (p.name, Xfrm::new(p.rect.x / unit, p.rect.y / unit, p.rect.w / unit, p.rect.h / unit)))
            .collect()
    }

    fn rect(shapes: &[(String, Xfrm)], name: &str) -> Xfrm {
        shapes.iter().find(|(n, _)| n == name).unwrap().1
    }

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }

    #[test]
    fn unused_flow_and_cross_allocation_do_not_request_fitting() {
        let shapes = layout((100., 60.), "fromL", false, false);
        let first = rect(&shapes, "first");
        let third = rect(&shapes, "third");
        let flat = rect(&shapes, "flat");
        near(first.w, 14.);
        near(third.h, 41.);
        near(flat.x, third.x + third.w);
        near(first.x, 1.5);
    }

    #[test]
    fn text_fit_scales_resolved_child_flow_and_preserves_cross_extent() {
        let shapes = layout((160., 130.), "fromL", true, false);
        let scale = 1023. / 1024.;
        near(rect(&shapes, "first").w, 14. * scale);
        near(rect(&shapes, "third").h, 41.);
        near(rect(&shapes, "flat").w, 21. * scale);
        let vertical = layout((160., 130.), "fromT", true, false);
        near(rect(&vertical, "first").w, 14.);
        near(rect(&vertical, "third").h, 41. * scale);
    }

    #[test]
    fn actual_parent_flow_overflow_fits_occupied_sibling_sum() {
        let shapes = layout((84., 140.), "fromL", false, false);
        let scale = 886. / 1024.;
        near(rect(&shapes, "first").w, 14. * scale);
        near(rect(&shapes, "third").h, 41.);
        near(rect(&shapes, "flat").w, 21. * scale);
    }

    #[test]
    fn cross_overflow_relayouts_the_child_before_parent_flow_fitting() {
        let shapes = layout((160., 31.), "fromL", false, false);
        let child_scale = 774. / 1024.;
        near(rect(&shapes, "third").h, 41. * child_scale);
        near(rect(&shapes, "first").w, 14. * child_scale * 1023. / 1024.);
        near(rect(&shapes, "flat").h, 18.);
    }

    #[test]
    fn reverse_flow_moves_groups_without_reversing_their_internal_order() {
        let forward = layout((84., 140.), "fromL", false, false);
        let reverse = layout((84., 140.), "fromR", false, false);
        let first = rect(&reverse, "first");
        let third = rect(&reverse, "third");
        let flat = rect(&reverse, "flat");
        assert!(first.x < third.x);
        near(flat.x + flat.w, first.x);
        near(rect(&forward, "first").w, first.w);
    }

    #[test]
    fn degenerate_editable_children_remain_present() {
        let shapes = layout((160., 130.), "fromL", false, true);
        assert_eq!(shapes.len(), 4);
        near(rect(&shapes, "first").w, 0.);
    }

    #[test]
    fn exact_finite_fit_endpoints_survive_point_conversion_rounding() {
        let unit: f64 = 72. / 25.4;
        let occupied = (80. * unit).next_up();
        near(super::super::geometry::flow_fit_scale(occupied, 70. * unit), 0.875);
        near(super::super::geometry::flow_fit_scale(80e-200, 70e-200), 0.875);
        near(super::super::geometry::flow_fit_scale(occupied, 69.999 * unit), 895. / 1024.);
    }

    #[test]
    fn affine_cycle_solves_exact_widths_before_root_text_fitting() {
        use super::super::{Compiler, Context, Data, constraints};
        use std::collections::HashMap;
        let source = crate::xml::parse(br#"<dataModel><ptLst><pt modelId="root" type="doc"/><pt modelId="a"/></ptLst><cxnLst><cxn srcId="root" destId="a"/></cxnLst></dataModel>"#).unwrap();
        let layout = crate::xml::parse(
            br#"<layoutNode><alg type="lin"/><constrLst>
        <constr type="w" for="ch" refType="w" fact="0.25"/><constr type="h" for="ch" refType="h" fact="0.8"/>
        </constrLst><forEach axis="ch" ptType="node"><layoutNode name="card"><alg type="composite"/><constrLst>
        <constr type="w" for="ch" forName="header" refFor="ch" refForName="details" refType="w" fact="1"/>
        <constr type="w" for="ch" forName="details" refFor="ch" refForName="header" refType="w" fact="1"/>
        <constr type="userA" for="ch" forName="details" refFor="ch" refForName="details" refType="w"/>
        <constr type="userA" for="ch" forName="details" op="gte" val="85.03937007874016"/>
        <constr type="userA" for="ch" forName="details" op="lte" val="85.03937007874016"/>
        </constrLst><layoutNode name="header"><alg type="tx"/><shape type="roundRect"/></layoutNode>
        <layoutNode name="details"><alg type="tx"/><shape type="rect"/></layoutNode></layoutNode></forEach></layoutNode>"#,
        )
        .unwrap();
        let data = Data::new(&source.root).unwrap();
        let context = Context { point: data.root, position: 1, count: 1, vars: HashMap::new() };
        let mut compiler = Compiler { data, refs: HashMap::new(), instances: 0, work: 0 };
        let tree = compiler.node(&layout.root, context, 0).unwrap();
        let solution = constraints::resolve(&tree, Xfrm::new(0., 0., 600., 300.), &compiler.data, &mut 0).unwrap();
        let children = &tree.children[0].children;
        assert_eq!(children.len(), 2);
        for child in children {
            near(constraints::lookup(&solution, child).unwrap().rect.w, 30. * 72. / 25.4);
        }
    }

    #[test]
    fn malformed_ranges_and_exhausted_work_are_errors() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let node = Instance::new(&xml.root);
        let style = BoxStyle::new(Xfrm::new(0., 0., 100., 100.));
        let mut children = [BoxStyle::new(Xfrm::new(0., 0., 20., 10.))];
        let range = 0..1;
        assert!(place(&node, style, &children, std::slice::from_ref(&range), &mut [], false, &mut 0).is_err());
        assert!(constrain_cross(&node, style, &mut children, std::slice::from_ref(&range), &[], &mut 0).is_err());
        assert!(place(&node, style, &children, &[], &mut [], false, &mut 0).is_err());
        let mut work = MAX_WORK;
        assert!(place(&node, style, &children, std::slice::from_ref(&(0..0)), &mut [], false, &mut work).is_err());
        for value in [f64::NAN, f64::INFINITY, -1.] {
            let invalid = BoxStyle::new(Xfrm::new(0., 0., value, 1.));
            assert!(place(&node, invalid, &[], &[], &mut [], false, &mut 0).is_err());
        }
    }
}
