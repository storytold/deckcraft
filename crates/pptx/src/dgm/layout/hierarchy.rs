//! Child rows in a hierarchy retain a common edge and fit both dimensions.
use super::{Instance, MAX_WORK, geometry::BoxStyle};

fn charge(work: &mut usize, amount: usize) -> Result<(), &'static str> {
    *work = work.saturating_add(amount);
    if *work > MAX_WORK { Err("diagram hierarchy budget exceeded") } else { Ok(()) }
}

fn exceeds(value: f64, limit: f64) -> bool {
    value - limit > 16. * f64::EPSILON * value.abs().max(limit.abs()).max(1.)
}

fn aligned(available: f64, occupied: f64, alignment: &str) -> f64 {
    match alignment {
        "l" | "t" => 0.,
        "r" | "b" => available - occupied,
        _ => (available - occupied) / 2.,
    }
}

pub(super) fn pack(node: &Instance<'_>, style: BoxStyle, children: &mut [BoxStyle], refit: bool, work: &mut usize) -> Result<(), &'static str> {
    charge(work, children.len().saturating_mul(3))?;
    let frame = style.rect;
    let direction = node.param("linDir", "fromL");
    let vertical = matches!(direction.as_str(), "fromT" | "fromB");
    let backwards = matches!(direction.as_str(), "fromR" | "fromB");
    let gap = style.get("sibSp").unwrap_or(0.).max(0.);
    let (mut flow, mut cross, mut count) = (0., 0_f64, 0_usize);
    for child in children.iter() {
        let r = child.rect;
        if !r.is_finite() {
            return Err("invalid diagram hierarchy child");
        }
        // A degenerate child contributes neither an edge nor a sibling gap.
        if r.w > 0. && r.h > 0. {
            flow += if vertical { r.h } else { r.w };
            cross = cross.max(if vertical { r.w } else { r.h });
            count += 1;
        }
    }
    flow += gap * count.saturating_sub(1) as f64;
    let (width, height) = if vertical { (cross, flow) } else { (flow, cross) };
    if !frame.is_finite() || !width.is_finite() || !height.is_finite() {
        return Err("invalid diagram hierarchy extent");
    }
    let fits = |scale: f64| !exceeds(width * scale, frame.w) && !exceeds(height * scale, frame.h);
    let scale = if frame.w <= 0. || frame.h <= 0. {
        0.
    } else if refit || !fits(1.) {
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
    } else {
        1.
    };
    let x = frame.x + aligned(frame.w, width * scale, &node.param("horzAlign", "ctr"));
    let y = frame.y + aligned(frame.h, height * scale, &node.param("vertAlign", "ctr"));
    let child_alignment = node.param("chAlign", if vertical { "l" } else { "t" });
    let mut pos = 0.;
    for child in children {
        let r = &mut child.rect;
        let active = r.w > 0. && r.h > 0.;
        r.w *= scale;
        r.h *= scale;
        r.x = x;
        r.y = y;
        if !active {
            continue;
        }
        let (length, thickness) = if vertical { (r.h, r.w) } else { (r.w, r.h) };
        let along = if backwards { flow * scale - pos - length } else { pos };
        let across = if child_alignment == if vertical { "r" } else { "b" } { cross * scale - thickness } else { 0. };
        if vertical {
            r.x += across;
            r.y += along;
        } else {
            r.x += along;
            r.y += across;
        }
        pos += length + gap * scale;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dgm::layout::{MeasureKind, Placed, evaluate_measured};
    use deckcraft_geom::Xfrm;

    fn packed(sizes: &[(f64, f64)], frame: (f64, f64), params: &[(&str, &str)], sibling_gap: f64) -> Vec<Xfrm> {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let mut node = Instance::new(&xml.root);
        node.alg = "hierChild".into();
        node.params = params.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect();
        let mut style = BoxStyle::new(Xfrm::new(0., 0., frame.0, frame.1));
        style.set("sibSp", sibling_gap, "equ");
        style.set("sp", 99., "equ");
        let mut children: Vec<_> = sizes.iter().map(|&(w, h)| BoxStyle::new(Xfrm::new(0., 0., w, h))).collect();
        pack(&node, style, &mut children, false, &mut 0).unwrap();
        children.into_iter().map(|c| c.rect).collect()
    }

    #[test]
    fn cross_capacity_scales_both_dimensions_and_keeps_a_common_edge() {
        let sizes = [(17., 31.), (36., 9.), (22., 18.), (11., 24.)];
        let actual = packed(&sizes, (147., 14.), &[], 0.);
        let scale = 462. / 1024.;
        for (rect, (w, h)) in actual.iter().zip(sizes) {
            assert!((rect.w - w * scale).abs() < 1e-9);
            assert!((rect.h - h * scale).abs() < 1e-9);
            assert!((rect.y - (14. - 31. * scale) / 2.).abs() < 1e-9);
        }
        assert!((actual[0].x + actual[3].x + actual[3].w - 147.).abs() < 1e-9);
        let exact = packed(&sizes, (147., 93.), &[], 0.);
        assert_eq!(exact[0].w, 17.);
    }

    #[test]
    fn reverse_columns_align_children_to_the_right_edge_and_keep_sibling_gaps() {
        let sizes = [(17., 31.), (36., 9.), (22., 18.), (11., 24.)];
        let actual = packed(&sizes, (147., 93.), &[("linDir", "fromB"), ("chAlign", "r")], 2.7);
        for rect in &actual {
            assert!((rect.x + rect.w - 91.5).abs() < 1e-9);
        }
        assert!((actual[0].y - 60.55).abs() < 1e-9);
        for pair in actual.windows(2) {
            assert!((pair[0].y - pair[1].y - pair[1].h - 2.7).abs() < 1e-9);
        }
    }

    #[test]
    fn zero_area_children_do_not_consume_flow_or_sibling_spacing() {
        let sizes = [(17., 31.), (36., 0.), (22., 18.), (11., 24.)];
        let actual = packed(&sizes, (147., 93.), &[], 3.);
        assert!((actual[0].x - 45.5).abs() < 1e-9);
        assert!((actual[2].x - 65.5).abs() < 1e-9);
        assert!((actual[3].x - 90.5).abs() < 1e-9);
        assert_eq!(actual[1].h, 0.);
    }

    fn data() -> crate::xml::El {
        crate::xml::parse(
            br#"<dataModel><ptLst><pt modelId="root" type="doc"/>
            <pt modelId="a"><t><p><r><t>Elm</t></r></p></t></pt>
            <pt modelId="b"><t><p><r><t>Oak</t></r></p></t></pt></ptLst>
            <cxnLst><cxn srcId="root" destId="a" srcOrd="0"/><cxn srcId="root" destId="b" srcOrd="1"/></cxnLst></dataModel>"#,
        )
        .unwrap()
        .root
    }

    #[test]
    fn word_wrap_refits_only_the_hierarchy_root_and_both_dimensions() {
        let data = data();
        let flow = r#"<layoutNode name="flow"><alg type="hierChild"/><constrLst>
            <constr type="w" for="ch" val="47"/><constr type="h" for="ch" val="19"/></constrLst>
            <forEach axis="ch" cnt="2"><layoutNode><presOf axis="self"/><alg type="tx"/><shape type="rect"/></layoutNode></forEach></layoutNode>"#;
        for nested in [false, true] {
            let xml = if nested {
                format!(
                    "<layoutNode><alg type='composite'/><constrLst><constr type='w' for='ch' val='200'/><constr type='h' for='ch' val='90'/></constrLst>{flow}</layoutNode>"
                )
            } else {
                flow.to_owned()
            };
            let layout = crate::xml::parse(format!("<layoutDef>{xml}</layoutDef>").as_bytes()).unwrap();
            for overflow in [false, true] {
                let mut measure =
                    |placed: &[Placed<'_>], mode: MeasureKind| Ok(vec![f64::from(overflow && mode == MeasureKind::WordWrap); placed.len()]);
                let out = evaluate_measured(&layout.root, &data, Xfrm::new(0., 0., 600., 300.), Some(&mut measure)).unwrap();
                let scale = if overflow && !nested { 1023. / 1024. } else { 1. };
                assert_eq!(out.len(), 2);
                for p in out {
                    assert!((p.rect.w - 47. * 72. / 25.4 * scale).abs() < 1e-6);
                    assert!((p.rect.h - 19. * 72. / 25.4 * scale).abs() < 1e-6);
                }
            }
        }
    }

    #[test]
    fn measurement_of_zero_area_text_does_not_refit_visible_siblings() {
        let data = data();
        let layout = crate::xml::parse(
            br#"<layoutDef><layoutNode><alg type="hierChild"/><constrLst>
            <constr type="w" for="ch" val="47"/><constr type="h" for="ch" val="19"/>
            <constr type="w" for="ch" forName="zero" val="0"/></constrLst>
            <layoutNode name="zero"><alg type="tx"/><presOf axis="ch" st="1" cnt="1"/><shape type="rect"/></layoutNode>
            <layoutNode name="visible"><alg type="tx"/><presOf axis="ch" st="2" cnt="1"/><shape type="rect"/></layoutNode>
            </layoutNode></layoutDef>"#,
        )
        .unwrap();
        let mut measure = |placed: &[Placed<'_>], _: MeasureKind| Ok(placed.iter().map(|p| f64::from(p.name == "zero")).collect());
        let out = evaluate_measured(&layout.root, &data, Xfrm::new(0., 0., 600., 300.), Some(&mut measure)).unwrap();
        let visible = out.iter().find(|p| p.name == "visible").unwrap();
        assert!((visible.rect.w - 47. * 72. / 25.4).abs() < 1e-6);
        assert!((visible.rect.h - 19. * 72. / 25.4).abs() < 1e-6);
    }

    #[test]
    fn nested_zero_area_hierarchy_children_do_not_enlarge_composite_bounds() {
        let data = data();
        for alg in ["hierChild", "lin", "composite"] {
            let xml = format!(
                r#"<layoutDef><layoutNode name="canvas"><alg type="composite"/><shape type="rect"/>
                <constrLst><constr type="w" for="ch" val="100"/><constr type="h" for="ch" val="80"/></constrLst>
                <layoutNode name="branch"><alg type="{alg}"/><constrLst>
                <constr type="w" for="ch" forName="zero" val="0"/><constr type="h" for="ch" forName="zero" val="100"/>
                <constr type="w" for="ch" forName="visible" val="30"/><constr type="h" for="ch" forName="visible" val="20"/>
                </constrLst>
                <layoutNode name="zero"><alg type="tx"/><presOf axis="ch" st="1" cnt="1"/><shape type="rect"/></layoutNode>
                <layoutNode name="visible"><alg type="tx"/><presOf axis="ch" st="2" cnt="1"/><shape type="rect"/></layoutNode>
                </layoutNode></layoutNode></layoutDef>"#
            );
            let layout = crate::xml::parse(xml.as_bytes()).unwrap();
            let mut measure = |placed: &[Placed<'_>], _: MeasureKind| Ok(vec![0.; placed.len()]);
            let out = evaluate_measured(&layout.root, &data, Xfrm::new(0., 0., 600., 300.), Some(&mut measure)).unwrap();
            let zero = out.iter().find(|p| p.name == "zero").unwrap();
            let visible = out.iter().find(|p| p.name == "visible").unwrap();
            let canvas = out.iter().find(|p| p.name == "canvas").unwrap();
            assert_eq!(zero.ignore_bounds, alg == "hierChild");
            assert_eq!(zero.rect.w, 0.);
            assert!(!visible.ignore_bounds);
            assert!(!canvas.ignore_bounds);
            if alg == "hierChild" {
                assert!((zero.rect.h - 100. * 72. / 25.4).abs() < 1e-6);
                assert_eq!(canvas.rect, visible.rect);
                assert!((visible.rect.x + visible.rect.w / 2. - 300.).abs() < 1e-6);
                assert!((visible.rect.y + visible.rect.h / 2. - 150.).abs() < 1e-6);
            } else {
                assert!(canvas.rect.h > visible.rect.h * 2.);
            }
        }
    }

    #[test]
    fn excluding_a_connector_allocation_keeps_its_endpoint_geometry() {
        let data = crate::xml::parse(
            br#"<dataModel><ptLst><pt modelId="root" type="doc"/><pt modelId="a"/><pt modelId="b"/>
            <pt modelId="ab" type="sibTrans"/></ptLst><cxnLst>
            <cxn srcId="root" destId="a" srcOrd="0" sibTransId="ab"/><cxn srcId="root" destId="b" srcOrd="1"/>
            </cxnLst></dataModel>"#,
        )
        .unwrap();
        let layout = crate::xml::parse(
            br#"<layoutDef><layoutNode><alg type="composite"/><layoutNode><alg type="hierChild"/><constrLst>
            <constr type="w" for="ch" forName="item" val="30"/><constr type="h" for="ch" forName="item" val="20"/>
            <constr type="w" for="ch" forName="link" val="0"/><constr type="h" for="ch" forName="link" val="100"/>
            <constr type="sibSp" for="self" val="5"/></constrLst><forEach axis="ch" ptType="node">
            <layoutNode name="item"><presOf axis="self"/><alg type="tx"/><shape type="rect"/></layoutNode>
            <forEach axis="followSib" ptType="sibTrans"><layoutNode name="link"><presOf axis="self"/>
            <alg type="conn"/><shape type="conn"/></layoutNode></forEach></forEach></layoutNode></layoutNode></layoutDef>"#,
        )
        .unwrap();
        let mut measure = |placed: &[Placed<'_>], _: MeasureKind| Ok(vec![0.; placed.len()]);
        let out = evaluate_measured(&layout.root, &data.root, Xfrm::new(0., 0., 600., 300.), Some(&mut measure)).unwrap();
        let connector = out.iter().find(|p| p.connector).unwrap();
        let nodes: Vec<_> = out.iter().filter(|p| !p.connector).collect();
        assert_eq!(nodes.len(), 2);
        assert!(connector.ignore_bounds);
        assert!((connector.rect.w - 5. * 72. / 25.4).abs() < 1e-6);
        assert_eq!(connector.rect.h, 0.);
        assert!((connector.rect.x - nodes[0].rect.x - nodes[0].rect.w).abs() < 1e-6);
        assert!((connector.rect.x + connector.rect.w - nodes[1].rect.x).abs() < 1e-6);
        assert!((connector.rect.y - 150.).abs() < 1e-6);
    }

    #[test]
    fn missing_dimensions_use_intrinsic_defaults_and_child_direction_does_not_rotate_flow() {
        let data = data();
        let layout = crate::xml::parse(
            br#"<layoutDef><layoutNode><alg type="hierChild"><param type="chDir" val="vert"/></alg>
            <forEach axis="ch"><layoutNode><alg type="tx"/><presOf axis="self"/><shape type="rect"/></layoutNode></forEach>
            </layoutNode></layoutDef>"#,
        )
        .unwrap();
        let mut measure = |placed: &[Placed<'_>], _: MeasureKind| Ok(vec![0.; placed.len()]);
        let out = evaluate_measured(&layout.root, &data, Xfrm::new(0., 0., 700., 400.), Some(&mut measure)).unwrap();
        assert_eq!(out.len(), 2);
        for p in &out {
            assert!((p.rect.w - 100. * 72. / 25.4).abs() < 1e-6);
            assert!((p.rect.h - 100. * 72. / 25.4).abs() < 1e-6);
        }
        assert!((out[1].rect.x - out[0].rect.x - out[0].rect.w).abs() < 1e-6);
        assert_eq!(out[0].rect.y, out[1].rect.y);
    }

    #[test]
    fn zero_extreme_and_invalid_geometry_obey_the_shared_work_budget() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let node = Instance::new(&xml.root);
        let mut children = [BoxStyle::new(Xfrm::new(0., 0., 100., 100.))];
        for extent in [0., 1e-300] {
            let mut current = children;
            pack(&node, BoxStyle::new(Xfrm::new(0., 0., extent, extent)), &mut current, false, &mut 0).unwrap();
            assert!(current[0].rect.is_finite());
        }
        let mut work = MAX_WORK;
        assert!(pack(&node, BoxStyle::new(Xfrm::new(0., 0., 20., 20.)), &mut children, false, &mut work).is_err());
        children[0].rect.w = f64::NAN;
        assert!(pack(&node, BoxStyle::new(Xfrm::new(0., 0., 20., 20.)), &mut children, false, &mut 0).is_err());
    }
}
