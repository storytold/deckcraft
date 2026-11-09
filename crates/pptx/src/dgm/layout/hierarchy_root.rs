//! A hierarchy root attaches its first child to its second child's hierarchy.
use super::{Instance, MAX_WORK, Placed, geometry::BoxStyle};
use deckcraft_geom::Xfrm;

fn charge(work: &mut usize, amount: usize) -> Result<(), &'static str> {
    *work = work.saturating_add(amount);
    if *work > MAX_WORK { Err("diagram hierarchy root budget exceeded") } else { Ok(()) }
}

fn exceeds(value: f64, limit: f64) -> bool {
    value - limit > 16. * f64::EPSILON * value.abs().max(limit.abs()).max(1.)
}

pub(super) fn place(
    node: &Instance<'_>,
    style: BoxStyle,
    children: &[BoxStyle],
    ranges: &[std::ops::Range<usize>],
    out: &mut [Placed<'_>],
    work: &mut usize,
) -> Result<(), &'static str> {
    charge(work, children.len())?;
    let mut occupied = children.to_vec();
    for ((child, bounds), range) in node.children.iter().zip(&mut occupied).zip(ranges) {
        if child.alg != "hierChild" {
            continue;
        }
        let shapes = out.get(range.clone()).ok_or("invalid hierarchy root child range")?;
        charge(work, shapes.len())?;
        bounds.rect = match super::geometry::occupied_bounds(shapes) {
            Some(mut rect) => {
                rect.x -= style.rect.x;
                rect.y -= style.rect.y;
                rect
            }
            None => Xfrm::new(0., 0., 0., 0.),
        };
    }
    let mut packed = occupied.clone();
    let local = BoxStyle { rect: Xfrm::new(0., 0., style.rect.w, style.rect.h), ..style };
    pack(node, local, &mut packed, work)?;
    for ((before, after), range) in occupied.iter().zip(&packed).zip(ranges) {
        let shapes = out.get_mut(range.clone()).ok_or("invalid hierarchy root child range")?;
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

/// Child algorithms have already run. A hierarchy-child container supplies its
/// occupied rectangle; a flat text child retains its own dimensions. The caller
/// applies each resulting translation and uniform scale to that child's shapes.
pub(super) fn pack(node: &Instance<'_>, style: BoxStyle, children: &mut [BoxStyle], work: &mut usize) -> Result<(), &'static str> {
    charge(work, children.len().saturating_mul(4))?;
    let frame = style.rect;
    if !frame.is_finite() || frame.w < 0. || frame.h < 0. {
        return Err("invalid diagram hierarchy root frame");
    }
    for child in children.iter_mut() {
        let r = &mut child.rect;
        if !r.is_finite() || r.w < 0. || r.h < 0. {
            return Err("invalid diagram hierarchy root child");
        }
        // Authored positions do not take part in the hierarchy's attachment.
        r.x = 0.;
        r.y = 0.;
    }
    let Some(parent) = children.first().map(|child| child.rect) else { return Ok(()) };
    let alignment = node.param("hierAlign", "tCtrCh");
    let horizontal = alignment.starts_with(['l', 'r']);
    let reverse = alignment.starts_with(['r', 'b']);
    let high_edge = matches!(alignment.as_str(), "tR" | "bR" | "lB" | "rB");
    let centered = alignment.contains("Ctr");
    let child_hierarchy = node.children.get(1).is_some_and(|child| child.alg == "hierChild");
    if let Some(child) = children.get_mut(1) {
        let r = &mut child.rect;
        let (parent_flow, parent_cross, child_flow, child_cross) =
            if horizontal { (parent.w, parent.h, r.w, r.h) } else { (parent.h, parent.w, r.h, r.w) };
        let along = if reverse { -child_flow } else { parent_flow };
        let across = if high_edge {
            parent_cross - child_cross
        } else if centered {
            parent_cross / 2. - if child_hierarchy { child_cross / 2. } else { 0. }
        } else {
            0.
        };
        (r.x, r.y) = if horizontal { (along, across) } else { (across, along) };
    }
    // Additional direct children remain at the hierarchy origin. They still
    // contribute to the final occupied group and its capacity fit.
    let bounds = children.iter().map(|child| child.rect).filter(|r| r.w > 0. || r.h > 0.).fold(None::<Xfrm>, |bounds, r| {
        Some(match bounds {
            None => r,
            Some(b) => {
                let (x, y) = (b.x.min(r.x), b.y.min(r.y));
                Xfrm::new(x, y, (b.x + b.w).max(r.x + r.w) - x, (b.y + b.h).max(r.y + r.h) - y)
            }
        })
    });
    let Some(bounds) = bounds else { return Ok(()) };
    if !bounds.is_finite() {
        return Err("invalid diagram hierarchy root extent");
    }
    let fits = |scale: f64| !exceeds(bounds.w * scale, frame.w) && !exceeds(bounds.h * scale, frame.h);
    let scale = if frame.w <= 0. || frame.h <= 0. {
        0.
    } else if fits(1.) {
        1.
    } else {
        let (mut low, mut high) = (0., 1.);
        while !fits(high * 0.5) {
            charge(work, 1)?;
            high *= 0.5;
        }
        for _ in 0..10 {
            charge(work, 1)?;
            let mid = (low + high) / 2.;
            if fits(mid) {
                low = mid;
            } else {
                high = mid;
            }
        }
        low
    };
    let x = frame.x + (frame.w - bounds.w * scale) / 2. - bounds.x * scale;
    let y = frame.y + (frame.h - bounds.h * scale) / 2. - bounds.y * scale;
    for child in children {
        let r = &mut child.rect;
        r.x = x + r.x * scale;
        r.y = y + r.y * scale;
        r.w *= scale;
        r.h *= scale;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dgm::layout::{MeasureKind, Placed, evaluate_measured};

    fn data() -> crate::xml::Doc {
        crate::xml::parse(
            br#"<dataModel><ptLst><pt modelId="root" type="doc"/><pt modelId="a"/><pt modelId="b"/><pt modelId="c"/></ptLst>
            <cxnLst><cxn srcId="root" destId="a" srcOrd="0"/><cxn srcId="root" destId="b" srcOrd="1"/><cxn srcId="root" destId="c" srcOrd="2"/></cxnLst></dataModel>"#,
        )
        .unwrap()
    }

    fn packed(sizes: &[(f64, f64)], frame: (f64, f64), alignment: &str, child_hierarchy: bool) -> Vec<Xfrm> {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let mut node = Instance::new(&xml.root);
        node.alg = "hierRoot".into();
        node.params.insert("hierAlign".into(), alignment.into());
        for i in 0..sizes.len() {
            let mut child = Instance::new(&xml.root);
            child.alg = if i == 1 && child_hierarchy { "hierChild" } else { "tx" }.into();
            node.children.push(child);
        }
        let mut children: Vec<_> = sizes.iter().map(|&(w, h)| BoxStyle::new(Xfrm::new(19., -31., w, h))).collect();
        pack(&node, BoxStyle::new(Xfrm::new(0., 0., frame.0, frame.1)), &mut children, &mut 0).unwrap();
        children.into_iter().map(|child| child.rect).collect()
    }

    #[test]
    fn only_the_first_two_children_have_parent_and_child_roles() {
        let r = packed(&[(22., 13.), (18., 11.), (31., 17.)], (140., 95.), "tCtrCh", false);
        assert_eq!(r[0], Xfrm::new(54.5, 35.5, 22., 13.));
        assert_eq!(r[1], Xfrm::new(65.5, 48.5, 18., 11.));
        assert_eq!(r[2], Xfrm::new(54.5, 35.5, 31., 17.));
    }

    #[test]
    fn a_hierarchy_child_exposes_its_occupied_center() {
        let r = packed(&[(22., 13.), (49., 17.)], (140., 95.), "tCtrCh", true);
        assert_eq!(r[0].x + r[0].w / 2., r[1].x + r[1].w / 2.);
        assert_eq!(r[1].y, r[0].y + r[0].h);
        assert_eq!(r[1].w, 49.);
    }

    #[test]
    fn high_edges_use_child_extent_in_all_four_directions() {
        for alignment in ["tR", "bR", "lB", "rB"] {
            let r = packed(&[(22., 13.), (18., 11.), (31., 17.)], (140., 95.), alignment, false);
            if alignment.starts_with(['l', 'r']) {
                assert_eq!(r[0].y + r[0].h, r[1].y + r[1].h);
                if alignment.starts_with('r') {
                    assert_eq!(r[1].x + r[1].w, r[0].x);
                } else {
                    assert_eq!(r[1].x, r[0].x + r[0].w);
                }
            } else {
                assert_eq!(r[0].x + r[0].w, r[1].x + r[1].w);
                if alignment.starts_with('b') {
                    assert_eq!(r[1].y + r[1].h, r[0].y);
                } else {
                    assert_eq!(r[1].y, r[0].y + r[0].h);
                }
            }
        }
    }

    #[test]
    fn root_capacity_scales_the_whole_group_uniformly() {
        let r = packed(&[(22., 13.), (18., 11.), (31., 17.)], (23., 19.), "tCtrCh", false);
        let scale = 759. / 1024.;
        for (rect, (w, h)) in r.iter().zip([(22., 13.), (18., 11.), (31., 17.)]) {
            assert_eq!(rect.w, w * scale);
            assert_eq!(rect.h, h * scale);
        }
        assert_eq!(r[0].x * 2. + r[2].w, 23.);
    }

    #[test]
    fn hierarchy_child_attachment_uses_occupied_geometry_after_its_own_layout() {
        let source = data();
        let layout = crate::xml::parse(
            br#"<layoutDef><layoutNode><alg type="hierRoot"/><constrLst>
            <constr type="w" for="ch" forName="parent" val="22"/><constr type="h" for="ch" forName="parent" val="13"/>
            <constr type="w" for="ch" forName="children" val="80"/><constr type="h" for="ch" forName="children" val="40"/>
            </constrLst><layoutNode name="parent"><alg type="tx"/><presOf axis="ch" st="1" cnt="1"/><shape type="rect"/></layoutNode>
            <layoutNode name="children"><alg type="hierChild"/><constrLst>
            <constr type="w" for="ch" forName="first" val="18"/><constr type="h" for="ch" forName="first" val="11"/>
            <constr type="w" for="ch" forName="second" val="31"/><constr type="h" for="ch" forName="second" val="17"/>
            </constrLst><layoutNode name="first"><alg type="tx"/><presOf axis="ch" st="2" cnt="1"/><shape type="rect"/></layoutNode>
            <layoutNode name="second"><alg type="tx"/><presOf axis="ch" st="3" cnt="1"/><shape type="rect"/></layoutNode>
            </layoutNode></layoutNode></layoutDef>"#,
        )
        .unwrap();
        let mut measure = |placed: &[Placed<'_>], _: MeasureKind| Ok(vec![0.; placed.len()]);
        let out = evaluate_measured(&layout.root, &source.root, Xfrm::new(0., 0., 600., 300.), Some(&mut measure)).unwrap();
        let parent = out.iter().find(|p| p.name == "parent").unwrap().rect;
        let first = out.iter().find(|p| p.name == "first").unwrap().rect;
        let second = out.iter().find(|p| p.name == "second").unwrap().rect;
        assert!((parent.x + parent.w / 2. - (first.x + second.x + second.w) / 2.).abs() < 1e-6);
        assert!((first.y - parent.y - parent.h).abs() < 1e-6);
        assert!((second.y - first.y).abs() < 1e-6);
        assert!((second.x + second.w - first.x - 49. * 72. / 25.4).abs() < 1e-6);
    }

    #[test]
    fn missing_dimensions_are_intrinsic_and_root_does_not_force_a_text_refit() {
        let source = data();
        let layout = crate::xml::parse(
            br#"<layoutDef><layoutNode><alg type="hierRoot"/><forEach axis="ch">
            <layoutNode><alg type="tx"/><presOf axis="self"/><shape type="rect"/></layoutNode>
            </forEach></layoutNode></layoutDef>"#,
        )
        .unwrap();
        let mut measure = |placed: &[Placed<'_>], _: MeasureKind| Ok(vec![0.; placed.len()]);
        let out = evaluate_measured(&layout.root, &source.root, Xfrm::new(0., 0., 1000., 1000.), Some(&mut measure)).unwrap();
        assert_eq!(out.len(), 3);
        for placed in out {
            assert!((placed.rect.w - 100. * 72. / 25.4).abs() < 1e-6);
            assert!((placed.rect.h - 100. * 72. / 25.4).abs() < 1e-6);
        }
    }

    #[test]
    fn one_axis_zero_root_child_remains_in_occupied_bounds() {
        let source = data();
        let layout = crate::xml::parse(
            br#"<layoutDef><layoutNode><alg type="hierRoot"/><constrLst>
            <constr type="w" for="ch" forName="parent" val="17"/><constr type="h" for="ch" forName="parent" val="23"/>
            <constr type="w" for="ch" forName="child" val="9"/><constr type="h" for="ch" forName="child" val="27"/>
            <constr type="w" for="ch" forName="extra" val="0"/><constr type="h" for="ch" forName="extra" val="90"/>
            </constrLst><layoutNode name="parent"><alg type="tx"/><presOf axis="ch" st="1" cnt="1"/><shape type="rect"/></layoutNode>
            <layoutNode name="child"><alg type="tx"/><presOf axis="ch" st="2" cnt="1"/><shape type="rect"/></layoutNode>
            <layoutNode name="extra"><alg type="tx"/><presOf axis="ch" st="3" cnt="1"/><shape type="rect"/></layoutNode>
            </layoutNode></layoutDef>"#,
        )
        .unwrap();
        let mut measure = |placed: &[Placed<'_>], _: MeasureKind| Ok(vec![0.; placed.len()]);
        let out = evaluate_measured(&layout.root, &source.root, Xfrm::new(0., 0., 600., 300.), Some(&mut measure)).unwrap();
        let parent = out.iter().find(|p| p.name == "parent").unwrap();
        let extra = out.iter().find(|p| p.name == "extra").unwrap();
        assert!(!extra.ignore_bounds);
        assert_eq!(extra.rect.w, 0.);
        assert!((extra.rect.h - 90. * 72. / 25.4).abs() < 1e-6);
        assert_eq!(extra.rect.x, parent.rect.x);
        assert!((extra.rect.y + extra.rect.h / 2. - 150.).abs() < 1e-6);
    }

    #[test]
    fn empty_degenerate_and_tiny_inputs_remain_finite_and_budgeted() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let node = Instance::new(&xml.root);
        for size in [0., 1e-300, 1.] {
            let style = BoxStyle::new(Xfrm::new(0., 0., size, size));
            let mut children = [BoxStyle::new(Xfrm::new(1., -1., 100., 100.))];
            pack(&node, style, &mut children, &mut 0).unwrap();
            assert!(children[0].rect.is_finite());
            let mut work = MAX_WORK;
            assert!(pack(&node, style, &mut children, &mut work).is_err());
            pack(&node, style, &mut [], &mut 0).unwrap();
        }
        let style = BoxStyle::new(Xfrm::new(0., 0., 100., 100.));
        for value in [f64::NAN, f64::INFINITY, -1.] {
            let mut children = [BoxStyle::new(Xfrm::new(0., 0., value, 1.))];
            assert!(pack(&node, style, &mut children, &mut 0).is_err());
        }
        let children = [BoxStyle::new(Xfrm::new(0., 0., 20., 10.))];
        assert!(place(&node, style, &children, std::slice::from_ref(&(0..1)), &mut [], &mut 0).is_err());
    }
}
