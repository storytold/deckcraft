use super::*;

fn source() -> El {
    crate::xml::parse(
        br#"<dataModel><ptLst>
      <pt modelId="root" type="doc"/><pt modelId="a"><t><p><r><t>Alpha</t></r></p></t></pt>
      <pt modelId="b"><t><p><r><t>Beta</t></r></p></t></pt><pt modelId="c"><t><p><r><t>Gamma</t></r></p></t></pt>
      <pt modelId="a1"><t><p><r><t>Detail A</t></r></p></t></pt><pt modelId="a2"><t><p><r><t>Detail B</t></r></p></t></pt>
      </ptLst><cxnLst><cxn srcId="root" destId="a" srcOrd="0"/><cxn srcId="root" destId="b" srcOrd="1"/>
      <cxn srcId="root" destId="c" srcOrd="2"/><cxn srcId="a" destId="a1" srcOrd="0"/><cxn srcId="a" destId="a2" srcOrd="1"/></cxnLst></dataModel>"#,
    )
    .unwrap()
    .root
}
fn layout(body: &str) -> El {
    crate::xml::parse(format!("<layoutDef>{body}</layoutDef>").as_bytes()).unwrap().root
}
fn frame() -> Xfrm {
    Xfrm::new(0.0, 0.0, 600.0, 300.0)
}

#[test]
fn nested_composite_constraints_and_conditional_data_mapping() {
    let layout = layout(
        r#"<layoutNode name="root"><alg type="lin"/><forEach axis="ch" ptType="node">
      <layoutNode name="card"><alg type="composite"/><layoutNode name="title"><presOf axis="self"/><alg type="tx"/><shape type="roundRect"/></layoutNode>
      <choose><if func="cnt" axis="ch" ptType="node" op="gt" val="0"><layoutNode name="detail"><presOf axis="ch" ptType="node"/><alg type="tx"/><shape type="rect"/></layoutNode></if></choose>
      <constrLst><constr type="h" for="ch" forName="title" refType="h" fact="0.25"/>
      <constr type="h" for="ch" forName="detail" refType="h" fact="0.75"/><constr type="t" for="ch" forName="detail" refType="h" fact="0.25"/>
      <constr type="primFontSz" for="ch" val="16"/></constrLst></layoutNode></forEach></layoutNode>"#,
    );
    let data = source();
    let placed = evaluate(&layout, &data, frame()).unwrap();
    assert_eq!(placed.len(), 4);
    let detail = placed.iter().find(|p| p.name == "detail").unwrap();
    assert_eq!(detail.points.len(), 2);
    assert!(placed.iter().all(|p| p.font == Some(16.0)));
    let titles: Vec<_> = placed.iter().filter(|p| p.name == "title").collect();
    // Mapping and fractional child dimensions are independent of the flow's
    // intrinsic size and finite fit; do not assume that it fills the whole frame.
    assert!((detail.rect.h - 75. * 72. / 25.4).abs() < 1e-6);
    assert!((detail.rect.y - titles[0].rect.y - 25. * 72. / 25.4).abs() < 1e-6);
    assert_eq!(detail.rect.w, titles[0].rect.w);
    for pair in titles.windows(2) {
        assert!((pair[1].rect.x - pair[0].rect.x - pair[0].rect.w).abs() < 1e-6);
    }
    assert!((titles[0].rect.x + titles[2].rect.x + titles[2].rect.w - 600.).abs() < 1e-6);
    assert_eq!(titles[2].points[0].attr("modelId"), Some("c"));
}

#[test]
fn recursive_iterator_stops_at_leaf_data_and_preserves_each_node() {
    let layout = layout(
        r#"<layoutNode name="root"><alg type="hierRoot"/><forEach name="walk" axis="ch" ptType="node">
      <layoutNode name="branch"><alg type="hierRoot"/><layoutNode name="label"><alg type="tx"/><presOf axis="self"/><shape type="rect"/></layoutNode>
      <layoutNode name="children"><alg type="hierChild"/><forEach ref="walk"/></layoutNode></layoutNode></forEach></layoutNode>"#,
    );
    let data = source();
    let placed = evaluate(&layout, &data, frame()).unwrap();
    assert_eq!(placed.len(), 5);
    let mut ids: Vec<_> = placed.iter().flat_map(|p| &p.points).filter_map(|p| p.attr("modelId")).collect();
    ids.sort();
    assert_eq!(ids, ["a", "a1", "a2", "b", "c"]);
    assert!(placed.iter().all(|p| p.rect.is_finite()));
}

#[test]
fn all_algorithm_families_generate_bounded_geometry_without_cache() {
    let data = source();
    for alg in ["lin", "cycle", "snake", "hierRoot", "hierChild", "pyra", "composite", "sp", "tx", "conn"] {
        let layout = layout(&format!(
            r#"<layoutNode><alg type="{alg}"/><forEach axis="ch" ptType="node"><layoutNode><presOf axis="self"/><alg type="tx"/><shape type="rect"/></layoutNode></forEach></layoutNode>"#
        ));
        let placed = evaluate(&layout, &data, frame()).unwrap();
        assert_eq!(placed.len(), 3, "{alg}");
        for p in placed {
            assert!(p.rect.is_finite() && p.rect.w > 0.0 && p.rect.h > 0.0, "{alg}");
        }
    }
}

#[test]
fn recursive_self_reference_and_unknown_algorithm_fail_safely() {
    let data = source();
    let recursive = layout(r#"<layoutNode><forEach name="loop" axis="self"><forEach ref="loop"/></forEach></layoutNode>"#);
    assert!(evaluate(&recursive, &data, frame()).is_err());
    let unknown = layout(r#"<layoutNode><alg type="invented"/></layoutNode>"#);
    assert!(evaluate(&unknown, &data, frame()).is_err());
}

#[test]
fn data_selection_is_ordered_bounded_and_supports_reverse_ranges() {
    let source = source();
    let data = Data::new(&source).unwrap();
    for (attrs, expected) in [
        ("axis='ch' ptType='node' st='2' cnt='1'", vec![2]),
        ("axis='ch' ptType='node' step='-1'", vec![3, 2, 1]),
        ("axis='des' ptType='node'", vec![1, 4, 5, 2, 3]),
    ] {
        let selector = crate::xml::parse(format!("<presOf {attrs}/>").as_bytes()).unwrap().root;
        assert_eq!(data.select(&selector, data.root), expected);
    }
}

#[test]
fn absolute_units_rotation_and_descendant_constraints() {
    let l = layout(
        r#"<layoutNode><alg type="composite"/><constrLst><constr type="primFontSz" for="des" ptType="node" val="22"/></constrLst>
    <layoutNode><alg type="composite"/><layoutNode name="label"><presOf axis="ch" ptType="node" cnt="1"/>
    <alg type="tx"/><shape type="rect" rot="90"/><constrLst><constr type="w" val="25.4"/><constr type="h" val="12.7"/></constrLst>
    </layoutNode></layoutNode></layoutNode>"#,
    );
    let data = source();
    let out = evaluate(&l, &data, frame()).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].font, Some(22.0));
    assert_eq!((out[0].rect.w, out[0].rect.h, out[0].rect.rot), (36.0, 72.0, 90.0));
    assert!((out[0].rect.bounds().width() - 72.).abs() < 1e-6);
    assert!((out[0].rect.bounds().height() - 36.).abs() < 1e-6);
}

#[test]
fn transition_connectors_attach_to_the_correct_nodes() {
    let d = crate::xml::parse(
        br#"<dataModel><ptLst><pt modelId="root" type="doc"/>
    <pt modelId="a"/><pt modelId="b"/><pt modelId="ab" type="sibTrans"/></ptLst>
    <cxnLst><cxn srcId="root" destId="a" srcOrd="0" sibTransId="ab"/><cxn srcId="root" destId="b" srcOrd="1"/></cxnLst></dataModel>"#,
    )
    .unwrap()
    .root;
    let l = layout(
        r#"<layoutNode><alg type="lin"/><constrLst><constr type="w" for="ch" forName="item" refType="w" fact="0.4"/>
    <constr type="w" for="ch" forName="link" refType="w" fact="0.2"/></constrLst><forEach axis="ch" ptType="node">
    <layoutNode name="item"><presOf axis="self"/><alg type="tx"/><shape type="rect"/></layoutNode>
    <forEach axis="followSib" ptType="sibTrans"><layoutNode name="link"><presOf axis="self"/><shape type="conn"/>
    <alg type="conn"><param type="endSty" val="arr"/></alg></layoutNode></forEach></forEach></layoutNode>"#,
    );
    let out = evaluate(&l, &d, frame()).unwrap();
    assert_eq!(out.len(), 3);
    assert!(out[0].connector);
    let a = out.iter().find(|p| p.points.iter().any(|pt| pt.attr("modelId") == Some("a"))).unwrap();
    let b = out.iter().find(|p| p.points.iter().any(|pt| pt.attr("modelId") == Some("b"))).unwrap();
    assert_eq!(out[0].rect, Xfrm::new(a.rect.x + a.rect.w, a.rect.y + a.rect.h / 2., b.rect.x - a.rect.x - a.rect.w, 0.));
}

#[test]
fn expansive_selectors_and_constraints_obey_global_work_budget() {
    let data = source();
    let l = layout(&format!(
        r#"<layoutNode><forEach axis="{}"><layoutNode><alg type="tx"/><presOf axis="self"/><shape type="rect"/></layoutNode></forEach></layoutNode>"#,
        ["root desOrSelf"; 8].join(" ")
    ));
    assert!(evaluate(&l, &data, frame()).is_err());
    let constraints = "<constr type='w' for='ch' refType='w' fact='0.1'/>".repeat(512);
    let nodes = "<layoutNode><presOf axis='self'/><alg type='tx'/><shape type='rect'/></layoutNode>".repeat(500);
    let l = layout(&format!("<layoutNode><constrLst>{constraints}</constrLst>{nodes}</layoutNode>"));
    assert!(evaluate(&l, &data, frame()).is_err());
}

#[test]
fn snake_respects_breakpoint_flow_growth_and_alternation() {
    let d = source();
    let l = layout(
        r#"<layoutNode><alg type="snake"><param type="flowDir" val="col"/><param type="bkpt" val="fixed"/>
    <param type="bkPtFixedVal" val="2"/><param type="grDir" val="bR"/><param type="contDir" val="revDir"/></alg>
    <forEach axis="ch" ptType="node"><layoutNode><presOf axis="self"/><alg type="tx"/><shape type="rect"/></layoutNode></forEach></layoutNode>"#,
    );
    let p = evaluate(&l, &d, frame()).unwrap();
    // Unconstrained children begin at 100mm square. The column pair is fitted
    // with the same finite search as explicit sizes, rather than filling cells.
    let side = 100. * 72. / 25.4 * 541. / 1024.;
    let expected = [(300., 150.), (300., 150. - side), (300. - side, 150. - side)];
    for (p, (x, y)) in p.iter().zip(expected) {
        assert!((p.rect.x - x).abs() < 1e-7 && (p.rect.y - y).abs() < 1e-7);
        assert!((p.rect.w - side).abs() < 1e-7 && (p.rect.h - side).abs() < 1e-7);
    }
}

#[test]
fn snake_repacking_uses_its_own_text_measurement_mode() {
    let data = source();
    let layout = layout(
        r#"<layoutNode><alg type="snake"/><constrLst>
        <constr type="w" for="ch" val="20"/><constr type="h" for="ch" val="10"/></constrLst>
        <forEach axis="ch" ptType="node"><layoutNode><presOf axis="self"/><alg type="tx"/><shape type="rect"/></layoutNode></forEach></layoutNode>"#,
    );
    for fits in [true, false] {
        let mut modes = Vec::new();
        let mut measure = |placed: &[Placed<'_>], mode: MeasureKind| {
            modes.push(mode);
            Ok(vec![f64::from(mode != MeasureKind::SnakeFit || !fits); placed.len()])
        };
        let p = evaluate_measured(&layout, &data, frame(), Some(&mut measure)).unwrap();
        let width = 20. * 72. / 25.4 * if fits { 1. } else { 1023. / 1024. };
        assert!(p.iter().all(|p| (p.rect.w - width).abs() < 1e-6));
        assert!(modes.contains(&MeasureKind::SnakeFit));
        assert!(!modes.contains(&MeasureKind::WordWrap));
    }
    for bad in [f64::NAN, f64::INFINITY, -1.] {
        let mut measure = |placed: &[Placed<'_>], _: MeasureKind| Ok(vec![bad; placed.len()]);
        assert!(evaluate_measured(&layout, &data, frame(), Some(&mut measure)).is_err());
    }
}

#[test]
fn measured_linear_fit_changes_the_root_but_not_an_allocated_nested_flow() {
    let data = source();
    let mm = 72. / 25.4;
    let flow = r#"<layoutNode name="flow"><alg type="lin"/><constrLst>
        <constr type="w" for="ch" val="47"/><constr type="h" for="ch" val="19"/></constrLst>
        <forEach axis="ch" cnt="2"><layoutNode><presOf axis="self"/><alg type="tx"/><shape type="rect"/></layoutNode></forEach>
        </layoutNode>"#;
    for nested in [false, true] {
        let xml = if nested {
            format!(
                "<layoutNode><alg type='composite'/><constrLst><constr type='w' for='ch' val='200'/><constr type='h' for='ch' val='90'/></constrLst>{flow}</layoutNode>"
            )
        } else {
            flow.to_owned()
        };
        let l = layout(&xml);
        for overflow in [false, true] {
            let mut measure = |placed: &[Placed<'_>], mode: MeasureKind| Ok(vec![f64::from(overflow && mode == MeasureKind::WordWrap); placed.len()]);
            let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
            assert_eq!(out.len(), 2);
            let expected = 47. * mm * if overflow && !nested { 1023. / 1024. } else { 1. };
            for p in out {
                assert!((p.rect.w - expected).abs() < 1e-6, "{nested}/{overflow}: {:?}", p.rect);
                assert!((p.rect.h - 19. * mm).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn snake_fitting_restores_linear_cross_extents_and_retains_fitted_flow() {
    let data = source();
    let mm = 72. / 25.4;
    for direction in ["fromL", "fromR", "fromT", "fromB"] {
        let vertical = matches!(direction, "fromT" | "fromB");
        let (flow, cross) = if vertical { ("h", "w") } else { ("w", "h") };
        for (own_cross, total) in [(100., 14.9853515625), (20., 14.970717430114746)] {
            let l = layout(&format!(
                "<layoutNode><alg type='snake'/><constrLst>
                 <constr type='{flow}' for='ch' forName='row' val='50'/><constr type='{cross}' for='ch' forName='row' val='{own_cross}'/>
                 </constrLst><layoutNode name='row'><alg type='lin'><param type='linDir' val='{direction}'/></alg><constrLst>
                 <constr type='{flow}' for='ch' forName='a' val='10'/><constr type='{cross}' for='ch' forName='a' val='80'/>
                 <constr type='{flow}' for='ch' forName='b' val='30'/><constr type='{cross}' for='ch' forName='b' val='40'/>
                 </constrLst><layoutNode name='a'><alg type='tx'/><shape type='rect'/></layoutNode>
                 <layoutNode name='b'><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode></layoutNode>"
            ));
            let (w, h) = if vertical { (30., 25.) } else { (25., 30.) };
            let out = evaluate(&l, &data, Xfrm::new(13. * mm, 17. * mm, w * mm, h * mm)).unwrap();
            assert_eq!(out.len(), 2);
            let start = (25. - total) / 2.;
            for (i, p) in out.iter().enumerate() {
                let length = total * if i == 0 { 0.25 } else { 0.75 };
                let mut position = start + if i == 0 { 0. } else { total * 0.25 };
                if matches!(direction, "fromR" | "fromB") {
                    position = 25. - position - length;
                }
                let height = if i == 0 { 80. } else { 40. };
                let expected =
                    if vertical { [(30. - height) / 2., position, height, length] } else { [position, (30. - height) / 2., length, height] };
                let actual = [p.rect.x / mm - 13., p.rect.y / mm - 17., p.rect.w / mm, p.rect.h / mm];
                for (a, e) in actual.into_iter().zip(expected) {
                    assert!((a - e).abs() < 1e-6, "{direction}/{own_cross}: {actual:?} != {expected:?}");
                }
            }
        }
    }
}

#[test]
fn a_self_spacing_property_does_not_insert_linear_spacers() {
    let d = source();
    for property in ["sp", "sibSp"] {
        let l = layout(&format!(
            r#"<layoutNode><alg type="lin"/><constrLst><constr type="{property}" val="25.4"/></constrLst>
            <forEach axis="ch" ptType="node"><layoutNode><presOf axis="self"/><alg type="tx"/><shape type="rect"/></layoutNode></forEach></layoutNode>"#,
        ));
        let p = evaluate(&l, &d, frame()).unwrap();
        assert!((p[1].rect.x - p[0].rect.x - p[0].rect.w).abs() < 1e-6);
        assert!((p[2].rect.x - p[1].rect.x - p[1].rect.w).abs() < 1e-6);
        assert!((p[0].rect.x + p[2].rect.x + p[2].rect.w - 600.).abs() < 1e-6);
    }
}

#[test]
fn linear_pack_uses_the_visible_composite_extent() {
    let d = source();
    let l = layout(
        r#"<layoutNode><alg type="lin"/><constrLst>
      <constr type="w" for="ch" forName="card" refType="w" fact="0.25"/>
      <constr type="h" for="ch" forName="card" refType="h" fact="0.8"/></constrLst>
      <forEach axis="ch" ptType="node"><layoutNode name="card"><alg type="composite"/>
      <constrLst><constr type="w" for="ch" forName="label" refType="w" fact="0.9"/>
      <constr type="h" for="ch" forName="label" refType="h"/>
      <constr type="l" for="ch" forName="label" refType="w" fact="0.05"/></constrLst>
      <layoutNode name="label"><alg type="tx"/><presOf axis="self"/><shape type="rect"/></layoutNode>
      </layoutNode></forEach></layoutNode>"#,
    );
    let p = evaluate(&l, &d, frame()).unwrap();
    assert_eq!(p.len(), 3);
    assert_eq!(p[0].rect, Xfrm::new(97.5, 30.0, 135.0, 240.0));
    assert_eq!(p[1].rect.x, p[0].rect.x + p[0].rect.w);
    assert_eq!(p[2].rect.x, p[1].rect.x + p[1].rect.w);
}

#[test]
fn over_limit_data_and_selectors_refuse_instead_of_truncating() {
    let source = source();
    let data = Data::new(&source).unwrap();
    let stages = ["self"; 17].join(" ");
    let selector = crate::xml::parse(format!("<presOf axis='{stages}'/>").as_bytes()).unwrap().root;
    assert!(data.select_bounded(&selector, data.root, &mut 0).is_err());
    let edges = "<cxn srcId='root' destId='a'/>".repeat(32001);
    let source = crate::xml::parse(
        format!("<dataModel><ptLst><pt modelId='root' type='doc'/><pt modelId='a'/></ptLst><cxnLst>{edges}</cxnLst></dataModel>").as_bytes(),
    )
    .unwrap()
    .root;
    assert!(Data::new(&source).is_none());
}

#[test]
fn forward_user_variable_dependencies_are_not_mistaken_for_convergence() {
    let d = source();
    let l = layout(
        r#"<layoutNode><alg type="composite"/><layoutNode name="first"><presOf axis="ch" cnt="1"/><alg type="tx"/><shape type="rect"/></layoutNode>
        <layoutNode name="second"><presOf axis="ch" st="2" cnt="1"/><alg type="tx"/><shape type="rect"/></layoutNode>
        <constrLst><constr type="w" for="ch" forName="first" refFor="ch" refForName="second" refType="userA"/>
        <constr type="userA" for="ch" forName="second" val="84"/></constrLst></layoutNode>"#,
    );
    let p = evaluate(&l, &d, frame()).unwrap();
    assert_eq!(p[0].rect.w, 84.0);
}

#[test]
fn coupled_cycles_respect_local_descendant_geometry() {
    let data = source();
    let layout = layout(
        r#"<layoutNode name="root"><alg type="composite"/>
      <layoutNode name="box"><alg type="composite"/><layoutNode name="leaf"><alg type="tx"/><shape type="rect"/></layoutNode></layoutNode>
      <layoutNode name="other"><alg type="tx"/><shape type="rect"/></layoutNode>
      <constrLst>
        <constr type="w" for="ch" forName="box" refFor="ch" refForName="other" refType="w" fact="0.5"/>
        <constr type="w" for="ch" forName="other" refFor="ch" refForName="box" refType="w" fact="2"/>
        <constr type="userA" for="ch" forName="other" refFor="ch" refForName="other" refType="w"/>
        <constr type="userA" for="ch" forName="other" op="gte" val="113.38582677165354"/>
        <constr type="userA" for="ch" forName="other" op="lte" val="113.38582677165354"/>
        <constr type="l" for="des" forName="leaf" val="5"/>
        <constr type="r" for="des" forName="leaf" refFor="ch" refForName="other" refType="w"/>
      </constrLst></layoutNode>"#,
    );
    // Exercise the general affine inequality solver via an auxiliary quantity.
    // Native width/height inequalities do not allocate initial geometry. Descendant
    // positions do not cross the composite; its leaf inherits the local box.
    let data = Data::new(&data).unwrap();
    let context = Context { point: data.root, position: 1, count: 1, vars: HashMap::new() };
    let mut compiler = Compiler { data, refs: HashMap::new(), instances: 0, work: 0 };
    let tree = compiler.node(layout.child("layoutNode").unwrap(), context, 0).unwrap();
    let solution = constraints::resolve(&tree, frame(), &compiler.data, &mut 0).unwrap();
    let leaf = constraints::lookup(&solution, &tree.children[0].children[0]).unwrap().rect;
    let other = constraints::lookup(&solution, &tree.children[1]).unwrap().rect;
    assert!((other.w - 40. * 72. / 25.4).abs() < 1e-6);
    assert!(leaf.x.abs() < 1e-6);
    assert!((leaf.w - other.w * 0.5).abs() < 1e-6);
}

#[test]
fn descendant_geometry_respects_local_algorithms_but_fonts_cross_them() {
    let data = source();
    let mm = 72. / 25.4;
    for alg in ["composite", "sp", "lin"] {
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><presOf/><constrLst>
            <constr type='w' for='ch' forName='wrapper' val='40'/>
            <constr type='h' for='ch' forName='wrapper' val='60'/>
            <constr type='w' for='des' forName='label' val='15'/>
            <constr type='h' for='des' forName='label' val='20'/>
            <constr type='primFontSz' for='des' forName='label' val='18'/>
            </constrLst><layoutNode name='wrapper'><alg type='{alg}'/><presOf/>
            <layoutNode name='label'><alg type='tx'/><shape type='rect'/><presOf axis='ch' cnt='1'/></layoutNode>
            </layoutNode></layoutNode>"
        ));
        let out = evaluate(&l, &data, frame()).unwrap();
        let label = out.iter().find(|p| p.name == "label").unwrap();
        let (w, h) = if alg == "lin" { (15., 20.) } else { (40., 60.) };
        assert!((label.rect.w - w * mm).abs() < 1e-6, "{alg}: {:?}", label.rect);
        assert!((label.rect.h - h * mm).abs() < 1e-6, "{alg}: {:?}", label.rect);
        assert_eq!(label.font, Some(18.));
    }
}

#[test]
fn descendant_position_constraints_leave_composite_placement_to_children() {
    let data = source();
    let mm = 72. / 25.4;
    for property in ["l", "r", "ctrX", "t", "b", "ctrY"] {
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><presOf/><constrLst>
            <constr type='w' for='ch' val='20'/><constr type='h' for='ch' val='20'/>
            <constr type='l' for='ch' forName='a' val='0'/><constr type='t' for='ch' forName='a' val='0'/>
            <constr type='l' for='ch' forName='b' val='40'/><constr type='t' for='ch' forName='b' val='40'/>
            <constr type='{property}' for='des' forName='b' val='5'/></constrLst>
            <layoutNode name='a'><alg type='tx'/><shape type='rect'/><presOf axis='ch' st='1' cnt='1'/></layoutNode>
            <layoutNode name='b'><alg type='tx'/><shape type='rect'/><presOf axis='ch' st='2' cnt='1'/></layoutNode>
            </layoutNode>"
        ));
        let out = evaluate(&l, &data, frame()).unwrap();
        assert!((out[1].rect.x - out[0].rect.x - 40. * mm).abs() < 1e-6, "{property}");
        assert!((out[1].rect.y - out[0].rect.y - 40. * mm).abs() < 1e-6, "{property}");
    }
}

#[test]
fn local_size_constraints_override_ancestor_defaults_and_composites_require_layout_selectors() {
    let data = source();
    let mm = 72. / 25.4;
    for alg in ["composite", "sp", "lin"] {
        for point in ["", "ptType='node'"] {
            let l = layout(&format!(
                "<layoutNode><alg type='composite'/><presOf/><constrLst>
                <constr type='w' for='ch' forName='wrapper' val='40'/>
                <constr type='h' for='ch' forName='wrapper' val='60'/>
                <constr type='h' for='des' forName='label' val='20'/></constrLst>
                <layoutNode name='wrapper'><alg type='{alg}'/><presOf/><constrLst>
                <constr type='h' for='ch' forName='label' {point} val='50'/></constrLst>
                <layoutNode name='label'><alg type='tx'/><shape type='rect'/><presOf axis='ch' cnt='1'/></layoutNode>
                </layoutNode></layoutNode>"
            ));
            let out = evaluate(&l, &data, frame()).unwrap();
            let label = out.iter().find(|p| p.name == "label").unwrap();
            let expected = if alg == "composite" && !point.is_empty() { 60. } else { 50. };
            assert!((label.rect.h - expected * mm).abs() < 1e-6, "{alg}/{point}: {:?}", label.rect);
        }
    }
}

#[test]
fn longer_than_eight_reference_chain_and_tightening_auxiliary_bounds() {
    let data = source();
    let mut xml = String::from("<layoutNode><alg type='composite'/>");
    for i in 0..24 {
        xml += &format!("<layoutNode name='n{i}'><alg type='tx'/><shape type='rect'/></layoutNode>");
    }
    xml += "<constrLst>";
    for i in 0..23 {
        xml += &format!("<constr type='w' for='ch' forName='n{i}' refFor='ch' refForName='n{}' refType='w'/>", i + 1);
    }
    xml += "<constr type='w' for='ch' forName='n23' refFor='ch' refForName='n23' refType='userA'/><constr type='userA' for='ch' forName='n23' op='gte' val='30'/><constr type='userA' for='ch' forName='n23' op='gte' val='40'/><constr type='userA' for='ch' forName='n23' op='lte' val='50'/></constrLst></layoutNode>";
    let layout = layout(&xml);
    let shapes = evaluate(&layout, &data, frame()).unwrap();
    assert_eq!(shapes.len(), 24);
    assert!(shapes.iter().all(|s| (s.rect.w - 50.).abs() < 1e-6), "{:?}", shapes.iter().map(|s| s.rect.w).collect::<Vec<_>>());
}

#[test]
fn inconsistent_cycles_and_missing_named_references_fail_instead_of_guessing() {
    let data = source();
    for reference in ["a", "missing"] {
        let layout = layout(&format!(
            "<layoutNode><alg type='composite'/><layoutNode name='a'><shape type='rect'/></layoutNode><constrLst><constr type='w' for='ch' forName='a' refFor='ch' refForName='{reference}' refType='w' fact='2'/><constr type='userA' for='ch' forName='a' refFor='ch' refForName='a' refType='w'/><constr type='userA' for='ch' forName='a' op='gte' val='1'/></constrLst></layoutNode>"
        ));
        assert!(evaluate(&layout, &data, frame()).is_err());
    }
}

#[test]
fn measured_rules_are_ordered_stop_at_fit_and_recompute_dependencies() {
    let data = source();
    let run = |rules: &str| {
        let rules = rules.replace(r#" for="ch" forName="text""#, "");
        let l = layout(&format!(
            r#"<layoutNode><alg type="composite"/><constrLst>
          <constr type="w" for="ch" forName="text" val="50"/>
          <constr type="h" for="ch" forName="text" val="10"/>
          <constr type="h" for="ch" forName="mirror" refType="h" refFor="ch" refForName="text"/>
          <constr type="primFontSz" for="ch" val="30"/></constrLst>
          <layoutNode name="text"><alg type="tx"/><presOf axis="ch" cnt="1"/><shape type="rect"/><ruleLst>{rules}</ruleLst></layoutNode>
          <layoutNode name="mirror"><alg type="sp"/><shape type="rect"/></layoutNode></layoutNode>"#
        ));
        let mut measure = |items: &[Placed<'_>], _: MeasureKind| {
            Ok(items.iter().map(|p| if p.name == "text" { (p.font.unwrap_or(18.0) * 3.0 - p.rect.h).max(0.0) } else { 0.0 }).collect())
        };
        evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap().iter().map(|p| (p.rect.h, p.font)).collect::<Vec<_>>()
    };
    let grow = r#"<rule type="h" for="ch" forName="text" val="50"/>"#;
    let shrink = r#"<rule type="primFontSz" for="ch" forName="text" val="5"/>"#;
    let a = run(&format!("{grow}{shrink}"));
    let b = run(&format!("{shrink}{grow}"));
    assert!((a[0].0 - 32.5 * 72. / 25.4).abs() < 0.02 && a[0].1 == Some(30.0));
    assert!((a[1].0 - a[0].0).abs() < 1e-6, "dependent height must be re-solved");
    assert!((b[0].0 - 10.0 * 72.0 / 25.4).abs() < 1e-6 && b[0].1.unwrap() < 10.0);
}

#[test]
fn measured_rules_resolve_factors_and_maximum_bounds_and_reject_bad_measurements() {
    let data = source();
    let l = layout(
        r#"<layoutNode><alg type="composite"/><layoutNode name="text"><alg type="tx"/>
      <presOf axis="ch" cnt="1"/><shape type="rect"/><constrLst><constr type="w" val="40"/>
      <constr type="h" refType="w" fact="0.25"/></constrLst><ruleLst><rule type="h" fact="2" max="30"/>
      </ruleLst></layoutNode></layoutNode>"#,
    );
    let mut measure = |items: &[Placed<'_>], _: MeasureKind| Ok(items.iter().map(|p| (150.0 - p.rect.h).max(0.0)).collect());
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert!((out[0].rect.h - 30.0 * 72.0 / 25.4).abs() < 1e-6);
    let mut bad = |_: &[Placed<'_>], _: MeasureKind| Ok(vec![f64::NAN]);
    assert!(evaluate_measured(&l, &data, frame(), Some(&mut bad)).is_err());
}

#[test]
fn text_rules_measure_reference_linked_font_groups_and_keep_secondary_sizes_distinct() {
    let data = source();
    let layout = layout(
        r#"<layoutNode name="root"><alg type="composite"/>
      <layoutNode name="a"><alg type="tx"/><shape type="rect"/></layoutNode>
      <layoutNode name="b"><alg type="tx"/><shape type="rect"/></layoutNode>
      <constrLst><constr type="primFontSz" for="ch" forName="a" val="36"/>
      <constr type="primFontSz" for="ch" forName="b" refFor="ch" refForName="a" refType="primFontSz"/>
      <constr type="secFontSz" for="ch" forName="a" refType="primFontSz" refFor="ch" refForName="a" fact="0.75"/>
      <constr type="secFontSz" for="ch" forName="b" refType="primFontSz" refFor="ch" refForName="b" fact="0.75"/></constrLst>
      <ruleLst><rule type="primFontSz" for="ch" forName="a" val="8"/></ruleLst></layoutNode>"#,
    );
    let mut measure =
        |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| if p.name == "b" { (p.font.unwrap() - 18.).max(0.) } else { 0. }).collect());
    let shapes = evaluate_measured(&layout, &data, frame(), Some(&mut measure)).unwrap();
    assert_eq!(shapes[0].font, shapes[1].font);
    assert!((shapes[0].font.unwrap() - 18.).abs() <= 1.0);
    for p in &shapes {
        assert_eq!(p.secondary_font.unwrap(), (p.font.unwrap() * 0.75).round());
    }
}
#[test]
fn reference_constraint_ignores_absolute_value_instead_of_adding_an_offset() {
    let data = source();
    let layout = layout(
        r#"<layoutNode><alg type="tx"/><shape type="rect"/><constrLst>
      <constr type="w" val="50"/><constr type="h" refType="w" fact="0.5" val="10"/>
      </constrLst></layoutNode>"#,
    );
    let p = evaluate(&layout, &data, frame()).unwrap();
    assert!((p[0].rect.h - 25. * 72. / 25.4).abs() < 1e-6);
}

#[test]
fn font_rule_does_not_shrink_to_floor_because_of_an_unrelated_geometry_dependency() {
    let data = source();
    let layout = layout(
        r#"<layoutNode><alg type="composite"/>
      <layoutNode name="a"><alg type="tx"/><shape type="rect"/><ruleLst><rule type="primFontSz" val="8"/></ruleLst></layoutNode>
      <layoutNode name="b"><alg type="tx"/><shape type="rect"/></layoutNode>
      <constrLst><constr type="primFontSz" for="ch" val="36"/>
      <constr type="w" for="ch" forName="b" refFor="ch" refForName="a" refType="w"/></constrLst>
      </layoutNode>"#,
    );
    let mut measure =
        |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| if p.name == "a" { (p.font.unwrap() - 24.).max(0.) } else { 50. }).collect());
    let shapes = evaluate_measured(&layout, &data, frame(), Some(&mut measure)).unwrap();
    assert!((shapes[0].font.unwrap() - 24.).abs() <= 1.);
    assert_eq!(shapes[1].font, Some(36.));
}

#[test]
fn text_search_finds_interior_fit_when_endpoints_are_worse_or_infeasible() {
    let data = source();
    // Two labels share a fixed horizontal span. Growing the first too far starves the
    // second; the endpoint is either worse or has a negative dependent extent.
    for limit in [100, 120] {
        let layout = layout(&format!(
            "<layoutNode><alg type='composite'/><layoutNode name='a'><alg type='tx'/><shape type='rect'/><ruleLst><rule type='w' val='{limit}'/></ruleLst></layoutNode>
            <layoutNode name='b'><alg type='tx'/><shape type='rect'/></layoutNode><constrLst>
            <constr type='w' for='ch' forName='a' val='20'/>
            <constr type='l' for='ch' forName='b' refFor='ch' refForName='a' refType='w'/>
            <constr type='r' for='ch' forName='b' val='100'/></constrLst>
            </layoutNode>"
        ));
        let mut measure = |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| (40. * 72. / 25.4 - p.rect.w).max(0.)).collect());
        let out = evaluate_measured(&layout, &data, frame(), Some(&mut measure)).unwrap();
        assert!((out[0].rect.w - 40. * 72. / 25.4).abs() < 0.02, "{limit}");
        assert!(out[1].rect.w >= 40. * 72. / 25.4);
    }
}

#[test]
fn native_text_rule_can_override_a_same_property_bound() {
    let data = source();
    let layout = layout(
        "<layoutNode><alg type='tx'/><shape type='rect'/><constrLst>
      <constr type='primFontSz' val='36'/><constr type='primFontSz' op='gte' val='20'/></constrLst>
      <ruleLst><rule type='primFontSz' val='5'/></ruleLst></layoutNode>",
    );
    let mut measure = |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| (p.font.unwrap() - 12.).max(0.)).collect());
    let out = evaluate_measured(&layout, &data, frame(), Some(&mut measure)).unwrap();
    assert!((11. ..=12.).contains(&out[0].font.unwrap()));
}

#[test]
fn font_rule_binding_retains_selector_identity_and_schema_defaults() {
    let data = source();
    for (constraint, rule, fits) in [
        ("for='ch' forName='a'", "for='ch' forName='a'", true),
        ("for='ch' forName='a'", "for='ch'", false),
        ("for='ch'", "for='ch' forName='a'", false),
        ("for='des' forName='a'", "for='ch' forName='a'", false),
        ("for='ch' forName='a'", "for='ch' forName='a' ptType='all'", true),
        ("for='ch' forName='a'", "for='ch' forName='a' ptType='node'", false),
        ("for='ch' forName='a' ptType='node'", "for='ch' forName='a' ptType='node'", true),
    ] {
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><constrLst><constr type='primFontSz' {constraint} val='36'/></constrLst><ruleLst><rule type='primFontSz' {rule} val='5'/></ruleLst><layoutNode name='a'><presOf axis='ch' cnt='1'/><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>"
        ));
        let mut measure = |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| (p.font.unwrap() - 14.).max(0.)).collect());
        let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
        assert!((out[0].font.unwrap() - if fits { 14. } else { 36. }).abs() <= 1., "{constraint} / {rule}");
    }
}

#[test]
fn shared_font_rules_fit_the_whole_group_but_unrelated_defaults_stay_independent() {
    let data = source();
    for (op, parent, expected_second) in [("none", false, 36.), ("equ", false, 14.), ("gte", false, 14.), ("none", true, 14.), ("equ", true, 14.)] {
        let rule = "<rule type='primFontSz' val='5'/>";
        let parent_rule = if parent { "<rule type='primFontSz' for='ch' val='5'/>" } else { "" };
        let local_rule = if parent { "" } else { rule };
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><constrLst><constr type='primFontSz' for='ch' op='{op}' val='36'/></constrLst><ruleLst>{parent_rule}</ruleLst><layoutNode name='a'><alg type='tx'/><shape type='rect'/><ruleLst>{local_rule}</ruleLst></layoutNode><layoutNode name='b'><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>"
        ));
        let mut measure =
            |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| if p.name == "a" { (p.font.unwrap() - 14.).max(0.) } else { 0. }).collect());
        let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
        assert!((out[0].font.unwrap() - 14.).abs() <= 1., "{op}/{parent}");
        assert!((out[1].font.unwrap() - expected_second).abs() <= 1., "{op}/{parent}");
    }
}

#[test]
fn local_font_references_distinguish_defaults_from_bidirectional_equalities() {
    let data = source();
    for (op, factor, target, expected) in [
        ("none", 1., "a", [21., 36.]),
        ("none", 0.5, "a", [21., 18.]),
        ("none", 0.5, "b", [36., 14.]),
        ("equ", 1., "a", [21., 21.]),
        ("equ", 0.5, "a", [21., 11.]),
        ("equ", 0.5, "b", [28., 14.]),
    ] {
        let rule = "<rule type='primFontSz' val='5'/>";
        let a = if target == "a" { rule } else { "" };
        let b = if target == "b" { rule } else { "" };
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><constrLst><constr type='primFontSz' for='ch' forName='a' val='36'/><constr type='primFontSz' for='ch' forName='b' refFor='ch' refForName='a' refType='primFontSz' op='{op}' fact='{factor}'/></constrLst><layoutNode name='a'><alg type='tx'/><shape type='rect'/><ruleLst>{a}</ruleLst></layoutNode><layoutNode name='b'><alg type='tx'/><shape type='rect'/><ruleLst>{b}</ruleLst></layoutNode></layoutNode>"
        ));
        let mut measure =
            |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| (p.font.unwrap() - if p.name == "a" { 21. } else { 14. }).max(0.)).collect());
        let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
        assert_eq!(out.iter().map(|p| p.font.unwrap()).collect::<Vec<_>>(), expected, "{op}/{factor}/{target}");
    }
}

#[test]
fn local_font_factor_rules_use_the_reference_size_and_keep_defaults_separate() {
    let data = source();
    for (initial, expected) in [(1., 12.), (0., 1.)] {
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><constrLst><constr type='primFontSz' for='ch' forName='reference' val='36'/><constr type='primFontSz' for='ch' forName='a' refFor='ch' refForName='reference' refType='primFontSz' fact='{initial}'/></constrLst><layoutNode name='reference'><alg type='sp'/></layoutNode><layoutNode name='a'><alg type='tx'/><shape type='rect'/><ruleLst><rule type='primFontSz' fact='0.25' max='12'/></ruleLst></layoutNode></layoutNode>"
        ));
        let mut measure =
            |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| if initial == 0. { 1. } else { (p.font.unwrap() - 12.).max(0.) }).collect());
        let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
        assert_eq!(out[0].font, Some(expected));
    }
}

#[test]
fn local_font_fit_does_not_activate_disconnected_bounds() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='composite'/><constrLst><constr type='primFontSz' for='ch' forName='b' refFor='ch' refForName='a' refType='primFontSz' op='lte' fact='0.5'/></constrLst><layoutNode name='a'><alg type='tx'/><shape type='rect'/></layoutNode><layoutNode name='b'><alg type='tx'/><shape type='rect'/></layoutNode><layoutNode name='c'><alg type='tx'/><shape type='rect'/><ruleLst><rule type='primFontSz' val='5'/></ruleLst></layoutNode></layoutNode>",
    );
    let mut measure =
        |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| if p.name == "c" { (p.font.unwrap() - 14.).max(0.) } else { 0. }).collect());
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert_eq!(out.iter().map(|p| p.font.unwrap()).collect::<Vec<_>>(), [36., 36., 14.]);
}

#[test]
fn child_font_rules_run_before_parent_rules_and_font_bounds_do_not_set_defaults() {
    let data = source();
    for op in ["none", "equ"] {
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><constrLst><constr type='primFontSz' for='ch' forName='a' val='36'/><constr type='primFontSz' for='ch' forName='b' refFor='ch' refForName='a' refType='primFontSz' op='{op}'/></constrLst><ruleLst><rule type='primFontSz' for='ch' forName='a' val='20'/></ruleLst><layoutNode name='a'><alg type='tx'/><shape type='rect'/><ruleLst><rule type='primFontSz' val='5'/></ruleLst></layoutNode><layoutNode name='b'><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>"
        ));
        let mut measure =
            |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| if p.name == "a" { (p.font.unwrap() - 14.).max(0.) } else { 0. }).collect());
        let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
        assert_eq!(out[0].font, Some(14.));
        assert_eq!(out[1].font, Some(if op == "none" { 36. } else { 14. }));
    }
    for op in ["gte", "lte"] {
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><constrLst><constr type='primFontSz' for='ch' forName='a' val='24'/><constr type='primFontSz' for='ch' forName='b' refFor='ch' refForName='a' refType='primFontSz' op='{op}' fact='0.5'/></constrLst><layoutNode name='a'><alg type='tx'/><shape type='rect'/></layoutNode><layoutNode name='b'><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>"
        ));
        let out = evaluate(&l, &data, frame()).unwrap();
        assert_eq!(out[0].font, Some(24.));
        assert_eq!(out[1].font, Some(36.));
    }
}

#[test]
fn parent_size_rules_and_unmapped_inherited_font_rules_do_not_fit_child_text() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='composite'/><constrLst><constr type='w' for='ch' forName='a' val='20'/><constr type='primFontSz' val='36'/></constrLst><ruleLst><rule type='w' for='ch' forName='a' val='80'/><rule type='primFontSz' val='5'/></ruleLst><layoutNode name='a'><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>",
    );
    let mut measure = |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| (100. - p.rect.w).max(0.) + (p.font.unwrap() - 14.).max(0.)).collect());
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert!((out[0].rect.w - 20. * 72. / 25.4).abs() < 1e-6);
    assert_eq!(out[0].font, Some(36.));
}

#[test]
fn ordered_dimensions_keep_earlier_growth_without_changing_an_unrelated_label() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='composite'/>
        <layoutNode name='a'><alg type='tx'/><shape type='rect'/><constrLst>
        <constr type='w' val='20'/><constr type='h' val='20'/></constrLst>
        <ruleLst><rule type='w' val='60'/><rule type='h' val='60'/></ruleLst></layoutNode>
        <layoutNode name='unrelated'><alg type='tx'/><shape type='rect'/><constrLst>
        <constr type='w' val='20'/><constr type='primFontSz' val='30'/></constrLst>
        <ruleLst><rule type='primFontSz' val='8'/></ruleLst></layoutNode></layoutNode>",
    );
    let mut measure = |p: &[Placed<'_>], _: MeasureKind| {
        Ok(p.iter().map(|p| if p.name == "a" { (40. * 72. / 25.4 - p.rect.w.min(p.rect.h)).max(0.) } else { 0. }).collect())
    };
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    // Native paired controls retain the first unresolved width endpoint. The
    // later successful height rule does not minimize both dimensions together.
    assert!((out[0].rect.w - 60. * 72. / 25.4).abs() < 0.02);
    assert!((out[0].rect.h - 40. * 72. / 25.4).abs() < 0.02);
    assert_eq!(out[1].font, Some(30.));
}

#[test]
fn coupled_user_rules_cross_a_plateau_without_changing_an_unrelated_label() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='composite'/>
        <layoutNode name='a'><alg type='tx'/><shape type='rect'/><constrLst>
        <constr type='userA' val='20'/><constr type='userB' val='20'/>
        <constr type='w' refType='userA'/><constr type='h' refType='userB'/></constrLst>
        <ruleLst><rule type='userA' val='60'/><rule type='userB' val='60'/></ruleLst></layoutNode>
        <layoutNode name='unrelated'><alg type='tx'/><shape type='rect'/><constrLst>
        <constr type='w' val='20'/><constr type='primFontSz' val='30'/></constrLst>
        <ruleLst><rule type='primFontSz' val='8'/></ruleLst></layoutNode></layoutNode>",
    );
    let mut measure =
        |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| if p.name == "a" { (40. - p.rect.w.min(p.rect.h)).max(0.) } else { 0. }).collect());
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert!((out[0].rect.w - 40.).abs() < 0.02);
    assert!((out[0].rect.h - 40.).abs() < 0.02);
    assert_eq!(out[1].font, Some(30.));
}

#[test]
fn local_text_edges_do_not_constrain_independent_size_rules() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='composite'/><layoutNode name='a'><alg type='tx'/><shape type='rect'/>
        <constrLst><constr type='w' val='20'/><constr type='h' val='40'/>
        <constr type='l' refType='w'/><constr type='r' refType='h'/></constrLst>
        <ruleLst><rule type='w' val='40'/><rule type='h' val='80'/></ruleLst>
        </layoutNode></layoutNode>",
    );
    let mut measure = |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| (90. - p.rect.w).max(0.)).collect());
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert!((out[0].rect.w - 31.875 * 72. / 25.4).abs() < 0.02);
    assert!((out[0].rect.h - 40. * 72. / 25.4).abs() < 1e-6);
}

#[test]
fn composite_coordinates_belong_to_the_parent_and_flow_algorithms_replace_them() {
    let data = source();
    let unit = 72. / 25.4;
    for (parent, local, separation) in [("composite", true, 0.), ("composite", false, 110.), ("lin", false, 39.9609375)] {
        let mut nodes = String::new();
        let mut constraints = String::new();
        for (name, left) in [("first", 10), ("second", 120)] {
            let coord = format!("<constr type='l' val='{left}' {}/>", if local { String::new() } else { format!("for='ch' forName='{name}'") });
            if !local {
                constraints += &coord;
            }
            nodes += &format!(
                "<layoutNode name='{name}'><alg type='tx'/><shape type='rect'/><constrLst>
                <constr type='w' val='40'/><constr type='h' val='40'/>{}</constrLst></layoutNode>",
                if local { coord.as_str() } else { "" }
            );
        }
        let l = layout(&format!("<layoutNode><alg type='{parent}'/><constrLst>{constraints}</constrLst>{nodes}</layoutNode>"));
        let out = evaluate(&l, &data, Xfrm::new(96., 151.2, 768., 324.)).unwrap();
        assert!((out[1].rect.x - out[0].rect.x - separation * unit).abs() < 1e-6);
        assert!((out[0].rect.x + out[1].rect.x + out[1].rect.w - 960.).abs() < 1e-6);
        assert!(out.iter().all(|p| (p.rect.y - (313.2 - 20. * unit)).abs() < 1e-6));
    }
}

#[test]
fn redundant_composite_coordinates_have_stable_axis_priority() {
    let data = source();
    for (properties, width, separation) in [
        (vec![("r", 80), ("l", 10), ("w", 40)], 40., 110.),
        (vec![("w", 40), ("r", 80), ("ctrX", 40)], 40., 80.),
        (vec![("ctrX", 40), ("r", 80), ("l", 10)], 70., 110.),
        (vec![("ctrX", 40), ("r", 80)], 80., 120.),
        (vec![("w", 40), ("ctrX", 40)], 40., 100.),
    ] {
        // The second node fixes a reference position, so global centring cannot hide a
        // wrong start/end/centre priority. Check both axes with transposed constraints.
        for vertical in [false, true] {
            let prop = |p| {
                if vertical {
                    match p {
                        "w" => "h",
                        "l" => "t",
                        "r" => "b",
                        _ => "ctrY",
                    }
                } else {
                    p
                }
            };
            let mut constraints = String::new();
            for (p, v) in &properties {
                constraints += &format!("<constr type='{}' val='{v}' for='ch' forName='a'/>", prop(p));
            }
            constraints +=
                &format!("<constr type='{}' val='120' for='ch' forName='b'/><constr type='{}' val='40' for='ch' forName='b'/>", prop("l"), prop("w"));
            let other_size = if vertical { "w" } else { "h" };
            constraints += &format!("<constr type='{other_size}' val='40' for='ch'/>");
            let l = layout(&format!(
                "<layoutNode><alg type='composite'/><constrLst>{constraints}</constrLst><layoutNode name='a'><alg type='tx'/><shape type='rect'/></layoutNode><layoutNode name='b'><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>"
            ));
            let out = evaluate(&l, &data, Xfrm::new(0., 0., 600., 600.)).unwrap();
            let size = if vertical { out[0].rect.h } else { out[0].rect.w };
            let distance = if vertical { out[1].rect.y - out[0].rect.y } else { out[1].rect.x - out[0].rect.x };
            assert!((size - width * 72. / 25.4).abs() < 1e-6, "{properties:?}");
            assert!((distance - separation * 72. / 25.4).abs() < 1e-6, "{properties:?}");
        }
    }
}

#[test]
fn root_composite_fits_the_occupied_extent_without_scaling_fonts() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='composite'/><layoutNode><alg type='tx'/><shape type='rect'/><constrLst><constr type='w' val='80'/><constr type='h' val='160'/><constr type='primFontSz' val='36'/></constrLst></layoutNode></layoutNode>",
    );
    let out = evaluate(&l, &data, Xfrm::new(96., 151.2, 768., 324.)).unwrap();
    assert!((out[0].rect.w - 80. * 72. / 25.4).abs() < 1e-6);
    assert!((out[0].rect.h - 324.).abs() < 1e-6);
    assert!((out[0].rect.x - (480. - 40. * 72. / 25.4)).abs() < 1e-6);
    assert!((out[0].rect.y - 151.2).abs() < 1e-6);
    assert_eq!(out[0].font, Some(36.));
}

#[test]
fn composite_shape_wraps_children_and_mapped_text_follows_its_data_point() {
    let data = source();
    let mm = 72. / 25.4;
    for mapped in [1, 2] {
        for sized in [false, true] {
            let size = if sized { "<constr type='w' val='100'/><constr type='h' val='60'/>" } else { "" };
            let l = layout(&format!(
                "<layoutNode name='box'><alg type='composite'/><presOf axis='ch' cnt='1' st='{mapped}'/><shape type='rect'/>
                <constrLst>{size}<constr type='primFontSz' val='48'/>
                <constr type='w' for='ch' forName='first' val='20'/><constr type='h' for='ch' forName='first' val='40'/>
                <constr type='l' for='ch' forName='first' val='20'/>
                <constr type='w' for='ch' forName='second' val='30'/><constr type='h' for='ch' forName='second' val='20'/>
                <constr type='l' for='ch' forName='second' val='70'/><constr type='t' for='ch' forName='second' val='50'/></constrLst>
                <forEach axis='ch' cnt='1'><layoutNode name='first'><alg type='tx'/><shape type='roundRect'/><constrLst><constr type='primFontSz' val='24'/></constrLst></layoutNode></forEach>
                <forEach axis='ch' st='2' cnt='1'><layoutNode name='second'><alg type='tx'/><shape type='roundRect'/><constrLst><constr type='primFontSz' val='36'/></constrLst></layoutNode></forEach>
                </layoutNode>"
            ));
            let out = evaluate(&l, &data, Xfrm::new(96., 151.2, 768., 324.)).unwrap();
            assert_eq!(out.len(), 3);
            let (width, height) = if sized { (100. * mm, 60. * mm) } else { (768., 324.) };
            let root = &out[0];
            assert!((root.rect.w - 80. * mm).abs() < 1e-6);
            assert!((root.rect.h - (70. * mm).min(height)).abs() < 1e-6);
            assert!((root.rect.x - (96. + (width - root.rect.w) / 2.)).abs() < 1e-6);
            assert!((root.rect.y - (151.2 + (height - root.rect.h) / 2.)).abs() < 1e-6);
            assert_eq!(root.font, out[mapped].font);
            assert_eq!(root.points[0].attr("modelId"), out[mapped].points[0].attr("modelId"));
            let text = root.text_rect.unwrap();
            assert!(text.x >= out[mapped].rect.x && text.y >= out[mapped].rect.y);
            assert!(text.w < out[mapped].rect.w && text.h < out[mapped].rect.h);
        }
    }
}

#[test]
fn explicit_empty_mapping_overrides_default_current_point() {
    let data = source();
    for mapping in ["", "<presOf/>", "<presOf axis='none'/>"] {
        let l = layout(&format!(
            "<layoutNode><alg type='lin'/><forEach axis='ch' cnt='1'><layoutNode>{mapping}<alg type='tx'/><shape type='rect'/></layoutNode></forEach></layoutNode>"
        ));
        let out = evaluate(&l, &data, frame()).unwrap();
        assert_eq!(out[0].points.len(), usize::from(mapping.is_empty()));
    }
}

#[test]
fn empty_composites_have_no_occupied_geometry_or_text_algorithm() {
    let data = source();
    for alg in ["composite", "sp"] {
        let l = layout(&format!(
            "<layoutNode><alg type='{alg}'/><presOf axis='ch' cnt='1'/><shape type='rect'/><constrLst><constr type='w' val='100'/><constr type='h' val='60'/></constrLst></layoutNode>"
        ));
        let out = evaluate(&l, &data, frame()).unwrap();
        assert_eq!(out.len(), 1);
        assert!(!out[0].text_enabled);
        assert_eq!(out[0].rect.w > 0., alg == "sp");
        assert_eq!(out[0].rect.h > 0., alg == "sp");
    }
    let l = layout(
        "<layoutNode><alg type='composite'/><shape type='rect'/><constrLst><constr type='l' for='ch' forName='visible' val='100'/></constrLst><layoutNode><alg type='composite'/><shape type='rect'/></layoutNode><layoutNode name='visible'><alg type='tx'/><shape type='rect'/><constrLst><constr type='w' val='20'/><constr type='h' val='40'/></constrLst></layoutNode></layoutNode>",
    );
    let out = evaluate(&l, &data, frame()).unwrap();
    assert_eq!(out[0].rect, out[2].rect, "an empty child cannot enlarge the occupied extent");
}

#[test]
fn nonpainting_allocations_and_connectors_reserve_composite_space() {
    let data = source();
    let mm = 72. / 25.4;
    for alg in ["sp", "tx", "conn", "composite"] {
        for shape in ["", "<shape type='none'/>", "<shape type='rect'/>", "<shape type='rect' hideGeom='1'/>"] {
            for (w, h) in [(10.0_f64, 20.), (0., 20.), (10., 0.), (0., 0.), (400., 20.)] {
                let l = layout(&format!(
                    "<layoutNode><alg type='composite'/><shape type='none'/><presOf/>
                    <constrLst><constr type='l' for='ch' forName='visible' val='50'/>
                    <constr type='w' for='ch' forName='visible' val='40'/><constr type='h' for='ch' forName='visible' val='20'/>
                    <constr type='w' for='ch' forName='helper' val='{w}'/><constr type='h' for='ch' forName='helper' val='{h}'/>
                    </constrLst><layoutNode name='visible'><alg type='tx'/><presOf axis='ch' cnt='1'/><shape type='rect'/></layoutNode>
                    <layoutNode name='helper'><alg type='{alg}'/><presOf/>{shape}</layoutNode></layoutNode>"
                ));
                let out = evaluate(&l, &data, frame()).unwrap();
                let visible = out.iter().find(|p| p.name == "visible").unwrap();
                let occupied = alg != "composite" && (w > 0. || h > 0.);
                let width = if occupied { w.max(90.) } else { 40. } * mm;
                let scale = (frame().w / width).min(1.);
                let x = (frame().w - width * scale) / 2. + if occupied { 50. * mm * scale } else { 0. };
                assert!((visible.rect.x - x).abs() < 1e-6, "{alg}/{shape}/{w}/{h}: {:?} expected {x}", visible.rect);
                assert!((visible.rect.w - 40. * mm * scale).abs() < 1e-6);
                // Pure allocation helpers must not become exported/drawn shapes.
                assert!(out.iter().all(|p| p.shape.is_some() || !p.points.is_empty()));
            }
        }
    }
}

#[test]
fn nested_composites_keep_allocated_sizes_when_space_parent_centres_them() {
    let data = source();
    let mm = 72. / 25.4;
    for alg in ["sp", "tx", "conn"] {
        let l = layout(&format!(
            "<layoutNode><alg type='sp'/><presOf/><constrLst>
            <constr type='w' for='ch' val='100'/><constr type='h' for='ch' val='60'/></constrLst>
            <layoutNode name='box'><alg type='composite'/><shape type='rect'/><presOf/>
            <constrLst><constr type='l' for='ch' val='10'/><constr type='t' for='ch' val='20'/>
            <constr type='w' for='ch' val='20'/><constr type='h' for='ch' val='10'/></constrLst>
            <layoutNode name='helper'><alg type='{alg}'/><shape type='none'/><presOf/></layoutNode>
            </layoutNode></layoutNode>"
        ));
        let out = evaluate(&l, &data, frame()).unwrap();
        let box_shape = out.iter().find(|p| p.name == "box").unwrap();
        assert!((box_shape.rect.x - (frame().w - 20. * mm) / 2.).abs() < 1e-6, "{alg}: {:?}", box_shape.rect);
        assert!((box_shape.rect.y - (frame().h - 10. * mm) / 2.).abs() < 1e-6);
        assert!((box_shape.rect.w - 20. * mm).abs() < 1e-6);
        assert!((box_shape.rect.h - 10. * mm).abs() < 1e-6);
        assert_eq!(out.len(), 1, "allocation-only nodes do not escape the layout stage");
    }
}

#[test]
fn overflowing_composites_clip_scaled_end_edges_before_recentering() {
    let data = source();
    let mm = 72. / 25.4;
    for vertical in [false, true] {
        for (frame_size, start, size, expected) in [
            (20., 5., 35., 120. / 7.),
            (35., 5., 35., 35.),
            (20., -5., 45., 20.),
            (20., 15., 25., 8.),
            (20., 35., 35., 0.),
            (20., 40., 35., 20.),
            (20., 100., 35., 20.),
            (20., 100., 20., 20.),
            (20., 15., 20., 20.),
            (20., 25., 20., 20.),
        ] {
            let (axis, edge, other) = if vertical { ("h", "t", "w") } else { ("w", "l", "h") };
            let l = layout(&format!(
                "<layoutNode><alg type='sp'/><presOf/><constrLst>
                <constr type='{axis}' for='ch' val='{frame_size}'/><constr type='{other}' for='ch' val='60'/></constrLst>
                <layoutNode name='box'><alg type='composite'/><shape type='none'/><presOf/>
                <constrLst><constr type='{edge}' for='ch' val='{start}'/><constr type='{axis}' for='ch' val='{size}'/>
                <constr type='{other}' for='ch' val='20'/></constrLst>
                <layoutNode name='child'><alg type='tx'/><shape type='rect'/><presOf axis='ch' cnt='1'/></layoutNode>
                </layoutNode></layoutNode>"
            ));
            for offset in [0., 180. * mm, 20.5] {
                let out = evaluate(&l, &data, Xfrm::new(offset, offset, 600., 300.)).unwrap();
                let r = out.iter().find(|p| p.name == "child").unwrap().rect;
                let (position, extent) = if vertical { (r.y, r.h) } else { (r.x, r.w) };
                assert!((extent - expected * mm).abs() < 1e-6, "{vertical}/{frame_size}/{start}/{size}/{offset}: {r:?}");
                let outer_size = if vertical { 300. } else { 600. };
                assert!((position - offset - (outer_size - expected * mm) / 2.).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn composite_overflow_uses_shared_bounds_with_individual_end_edges() {
    let data = source();
    let mm = 72. / 25.4;
    for vertical in [false, true] {
        // Independent two-shape native controls: positive leading space clips the
        // trailing shape, whereas a negative occupied origin preserves its width.
        for (left, second, expected, starts) in [
            (5., 30., [40. / 9., 20. / 3.], [10. / 9., 110. / 9.]),
            (-5., 30., [40. / 11., 80. / 11.], [0., 140. / 11.]),
            (100., 130., [4., 8.], [10., 6.]),
            (30., 55., [40. / 9., 80. / 9.], [70. / 9., 50. / 9.]),
            (45., 55., [20. / 3., 40. / 3.], [10., 10. / 3.]),
        ] {
            let (edge, axis, other) = if vertical { ("t", "h", "w") } else { ("l", "w", "h") };
            let l = layout(&format!(
                "<layoutNode><alg type='sp'/><presOf/><constrLst>
                <constr type='w' for='ch' val='20'/><constr type='h' for='ch' val='20'/></constrLst>
                <layoutNode><alg type='composite'/><presOf/><constrLst>
                <constr type='{edge}' for='ch' forName='a' val='{left}'/>
                <constr type='{axis}' for='ch' forName='a' val='10'/>
                <constr type='{edge}' for='ch' forName='b' val='{second}'/>
                <constr type='{axis}' for='ch' forName='b' val='20'/>
                <constr type='{other}' for='ch' val='20'/></constrLst>
                <layoutNode name='a'><alg type='tx'/><shape type='rect'/><presOf axis='ch' st='1' cnt='1'/></layoutNode>
                <layoutNode name='b'><alg type='tx'/><shape type='rect'/><presOf axis='ch' st='2' cnt='1'/></layoutNode>
                </layoutNode></layoutNode>"
            ));
            let out = evaluate(&l, &data, frame()).unwrap();
            // The enclosing space also centres the already-fitted child group.
            let base = ((if vertical { frame().h } else { frame().w }) - 20. * mm) / 2.;
            for ((name, extent), position) in ["a", "b"].into_iter().zip(expected).zip(starts) {
                let p = out.iter().find(|p| p.name == name).unwrap();
                let r = p.rect;
                let (start, size, unscaled) = if vertical { (r.y, r.h, r.w) } else { (r.x, r.w, r.h) };
                assert!((size - extent * mm).abs() < 1e-6, "{vertical}/{left}/{name}: {r:?}");
                assert!((start - base - position * mm).abs() < 1e-6, "{vertical}/{left}/{name}: {r:?}");
                assert!((unscaled - 20. * mm).abs() < 1e-6);
                assert!(start >= base && start + size <= base + 20. * mm + 1e-6);
                assert_eq!(p.font, Some(36.));
            }
        }
    }
}

#[test]
fn ordered_rules_do_not_reintroduce_a_redundant_edge_equation() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='composite'/><constrLst>
        <constr type='w' for='ch' val='20'/><constr type='h' for='ch' val='20'/>
        <constr type='l' for='ch' val='10'/><constr type='r' for='ch' val='100'/></constrLst>
        <ruleLst><rule type='r' for='ch' val='200'/></ruleLst>
        <layoutNode><alg type='tx'/><shape type='rect'/><ruleLst>
        <rule type='w' val='60'/><rule type='h' val='60'/></ruleLst></layoutNode></layoutNode>",
    );
    let mut measure = |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| (90. - p.rect.w.min(p.rect.h)).max(0.)).collect());
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert!((out[0].rect.w - 60. * 72. / 25.4).abs() < 0.02);
    // Five native size bisections reach 32.5mm for a 90pt height requirement.
    assert!((out[0].rect.h - 32.5 * 72. / 25.4).abs() < 0.02);
}

#[test]
fn constrained_cycle_starts_at_twelve_oclock_and_centres_partial_arc() {
    let data = source();
    for span in [180., 360.] {
        let l = layout(&format!(
            r#"<layoutNode><alg type="cycle"><param type="spanAng" val="{span}"/></alg>
          <forEach axis="ch"><layoutNode name="card"><alg type="tx"/><presOf/><shape type="rect"/>
          <constrLst><constr type="w" val="25"/><constr type="h" val="25"/></constrLst>
          </layoutNode></forEach></layoutNode>"#
        ));
        let shapes = evaluate(&l, &data, Xfrm::new(0., 0., 768., 324.)).unwrap();
        assert_eq!(shapes.len(), 3);
        assert!(shapes[0].rect.y < shapes[1].rect.y && shapes[1].rect.x > shapes[0].rect.x);
        let left = shapes.iter().map(|p| p.rect.x).fold(f64::INFINITY, f64::min);
        let right = shapes.iter().map(|p| p.rect.x + p.rect.w).fold(f64::NEG_INFINITY, f64::max);
        let top = shapes.iter().map(|p| p.rect.y).fold(f64::INFINITY, f64::min);
        let bottom = shapes.iter().map(|p| p.rect.y + p.rect.h).fold(f64::NEG_INFINITY, f64::max);
        assert!((left + right - 768.).abs() < 1e-6 && (top + bottom - 324.).abs() < 1e-6);
        assert!(top >= -1e-6 && bottom <= 324. + 1e-6);
    }
}

#[test]
fn linear_container_fits_both_axes_without_scaling_the_font() {
    let data = source();
    let mm = 72. / 25.4;
    // Independent native controls measure the finite fit in each direction.
    for (dir, count, w, h, expected_w, expected_h) in [
        ("fromL", 1, 80., 160., 79.921875 * mm, 324.),
        ("fromR", 1, 500., 40., 270.5078125 * mm, 40. * mm),
        ("fromL", 3, 150., 160., 90.234375 * mm, 324.),
        ("fromT", 3, 500., 60., 768., 38.0859375 * mm),
        ("fromB", 3, 500., 60., 768., 38.0859375 * mm),
    ] {
        let l = layout(&format!(
            r#"<layoutNode><alg type="lin"><param type="linDir" val="{dir}"/></alg>
            <forEach axis="ch" cnt="{count}"><layoutNode><presOf/><alg type="tx"/><shape type="rect"/>
            <constrLst><constr type="w" val="{w}"/><constr type="h" val="{h}"/>
            <constr type="primFontSz" val="18"/></constrLst></layoutNode></forEach></layoutNode>"#
        ));
        let shapes = evaluate(&l, &data, Xfrm::new(11., 23., 768., 324.)).unwrap();
        assert_eq!(shapes.len(), count);
        for p in shapes {
            assert!((p.rect.w - expected_w).abs() < 1e-6, "{dir}: {:?}", p.rect);
            assert!((p.rect.h - expected_h).abs() < 1e-6, "{dir}: {:?}", p.rect);
            assert!(p.rect.x >= 11. - 1e-6 && p.rect.x + p.rect.w <= 779. + 1e-6);
            assert!(p.rect.y >= 23. - 1e-6 && p.rect.y + p.rect.h <= 347. + 1e-6);
            assert_eq!(p.font, Some(18.));
        }
    }
}

#[test]
fn oversized_cycle_nodes_fit_without_overlap_and_keep_their_aspect_ratio() {
    let data = source();
    for span in [30., 180., 360., -180.] {
        for center in ["none", "fNode"] {
            let l = layout(&format!(
                r#"<layoutNode><alg type="cycle"><param type="spanAng" val="{span}"/>
                <param type="ctrShpMap" val="{center}"/></alg>
                <forEach axis="ch"><layoutNode><presOf/><alg type="tx"/><shape type="rect"/>
                <constrLst><constr type="w" val="150"/><constr type="h" val="160"/>
                <constr type="primFontSz" val="18"/></constrLst></layoutNode></forEach></layoutNode>"#
            ));
            let shapes = evaluate(&l, &data, Xfrm::new(0., 0., 768., 324.)).unwrap();
            assert_eq!(shapes.len(), 3);
            for (i, a) in shapes.iter().enumerate() {
                assert!(a.rect.x >= -1e-5 && a.rect.x + a.rect.w <= 768. + 1e-5, "{span}/{center}");
                assert!(a.rect.y >= -1e-5 && a.rect.y + a.rect.h <= 324. + 1e-5, "{span}/{center}");
                assert!((a.rect.w / a.rect.h - 150. / 160.).abs() < 1e-6);
                assert_eq!(a.font, Some(18.));
                for b in &shapes[..i] {
                    let overlap_w = (a.rect.x + a.rect.w).min(b.rect.x + b.rect.w) - a.rect.x.max(b.rect.x);
                    let overlap_h = (a.rect.y + a.rect.h).min(b.rect.y + b.rect.h) - a.rect.y.max(b.rect.y);
                    assert!(overlap_w <= 1e-4 || overlap_h <= 1e-4, "{span}/{center}: {:?}, {:?}", a.rect, b.rect);
                }
            }
        }
    }
}

#[test]
fn fractional_font_trials_propagate_before_final_point_quantization() {
    let data = source();
    for (op, factor, floor, threshold, expected) in [
        ("equ", 0.7, 5., 14.6, [21., 14.]),
        ("equ", 1.25, 5., 14.6, [12., 14.]),
        ("gte", 0.5, 5., 14.6, [27., 14.]),
        ("gte", 0.75, 5., 14.6, [18., 14.]),
        ("gte", 0.5, 5., 18.4, [36., 18.]),
        ("equ", 0.7, 14., 0., [20., 14.]),
        ("gte", 0.5, 14., 0., [28., 14.]),
    ] {
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><constrLst><constr type='primFontSz' for='ch' forName='a' val='36'/><constr type='primFontSz' for='ch' forName='b' refFor='ch' refForName='a' refType='primFontSz' op='{op}' fact='{factor}'/></constrLst><layoutNode name='a'><alg type='tx'/><shape type='rect'/></layoutNode><layoutNode name='b'><alg type='tx'/><shape type='rect'/><ruleLst><rule type='primFontSz' val='{floor}'/></ruleLst></layoutNode></layoutNode>"
        ));
        let mut measure =
            |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| if p.name == "b" { (p.font.unwrap() - threshold).max(0.) } else { 0. }).collect());
        let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
        assert_eq!(out.iter().map(|p| p.font.unwrap()).collect::<Vec<_>>(), expected, "{op}/{factor}/{floor}");
        for p in out {
            assert_eq!(p.margins, [p.font.unwrap() * 0.56; 4]);
        }
    }
}

#[test]
fn authored_font_defaults_quantize_each_reference_before_fitting() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='composite'/><constrLst><constr type='primFontSz' for='ch' forName='a' val='24.5'/><constr type='primFontSz' for='ch' forName='b' refFor='ch' refForName='a' refType='primFontSz' fact='0.75'/><constr type='secFontSz' for='ch' forName='b' refFor='ch' refForName='b' refType='primFontSz' fact='0.5'/></constrLst><layoutNode name='a'><alg type='tx'/><shape type='rect'/></layoutNode><layoutNode name='b'><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>",
    );
    let mut measure = |p: &[Placed<'_>], _: MeasureKind| Ok(vec![0.; p.len()]);
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert_eq!(out[0].font, Some(25.));
    assert_eq!(out[1].font, Some(19.));
    assert_eq!(out[1].secondary_font, Some(10.));
}

#[test]
fn implicit_edges_keep_reference_values_separate_from_positive_display_sizes() {
    let data_source = source();
    let mm = 72. / 25.4;
    for vertical in [false, true] {
        let (start, end, center, size) = if vertical { ("t", "b", "ctrY", "h") } else { ("l", "r", "ctrX", "w") };
        for (edge, value, explicit, expected) in [
            (start, -5., None, [-5., 20., 7.5, 25.]),
            (start, 25., None, [25., 20., 22.5, 5.]),
            (end, -5., None, [0., -5., -2.5, 5.]),
            (center, 5., None, [0., 0., 5., 0.]),
            (start, 5., Some(-5.), [5., 10., 7.5, 5.]),
            (end, 5., Some(-5.), [0., 5., 2.5, 5.]),
            (center, 5., Some(-5.), [2.5, 7.5, 5., 5.]),
        ] {
            for (reference, reference_value) in [start, end, center, size].into_iter().zip(expected) {
                let explicit = explicit.map(|v| format!("<constr type='{size}' for='ch' forName='subject' val='{v}'/>")).unwrap_or_default();
                let l = layout(&format!(
                    "<layoutNode><alg type='composite'/><constrLst>{explicit}
                    <constr type='{edge}' for='ch' forName='subject' val='{value}'/>
                    <constr type='{start}' for='ch' forName='witness' refFor='ch' refForName='subject' refType='{reference}'/>
                    <constr type='w' for='ch' forName='witness' val='4'/><constr type='h' for='ch' forName='witness' val='4'/>
                    </constrLst><layoutNode name='subject'><alg type='tx'/><shape type='rtTriangle'/></layoutNode>
                    <layoutNode name='witness'><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>"
                ));
                let data = Data::new(&data_source).unwrap();
                let context = Context { point: data.root, position: 1, count: 1, vars: HashMap::new() };
                let mut compiler = Compiler { data, refs: HashMap::new(), instances: 0, work: 0 };
                let tree = compiler.node(l.child("layoutNode").unwrap(), context, 0).unwrap();
                let solved = constraints::resolve(&tree, Xfrm::new(0., 0., 20. * mm, 20. * mm), &compiler.data, &mut 0).unwrap();
                let subject = constraints::lookup(&solved, &tree.children[0]).unwrap();
                let witness = constraints::lookup(&solved, &tree.children[1]).unwrap();
                assert!((subject.get(start).unwrap() / mm - expected[0]).abs() < 1e-6, "{edge}/{value}/{reference}");
                assert!((subject.get(size).unwrap() / mm - expected[3]).abs() < 1e-6, "{edge}/{value}/{reference}");
                assert!(
                    (witness.get(start).unwrap() / mm - reference_value).abs() < 1e-6,
                    "{edge}/{value}/{reference}: {}",
                    witness.get(start).unwrap() / mm
                );
                assert!(!subject.rect.flip_h && !subject.rect.flip_v);
            }
        }
    }
}

#[test]
fn negative_size_references_normalize_before_positioning_and_preserve_translation() {
    let d = source();
    let mm = 72. / 25.4;
    for (edge, expected) in [("l", 5.), ("r", 0.), ("ctrX", 2.5)] {
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><constrLst>
            <constr type='w' for='ch' forName='a' refType='w' fact='-0.25'/>
            <constr type='{edge}' for='ch' forName='a' val='5'/>
            <constr type='l' for='ch' forName='b' refFor='ch' refForName='a' refType='w'/>
            <constr type='w' for='ch' forName='b' val='4'/><constr type='h' for='ch' val='4'/>
            </constrLst><layoutNode name='a'><alg type='tx'/><shape type='rtTriangle'/></layoutNode>
            <layoutNode name='b'><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>"
        ));
        for offset in [0., 100.3] {
            let out = evaluate(&l, &d, Xfrm::new(offset, offset, 20. * mm, 20. * mm)).unwrap();
            let a = &out[0].rect;
            let b = &out[1].rect;
            assert!((a.w / mm - 5.).abs() < 1e-6);
            assert!((b.x - a.x - (5. - expected) * mm).abs() < 1e-6);
            assert!(!a.flip_h && !a.flip_v);
        }
    }
}

#[test]
fn one_dimensional_composites_center_their_zero_axis_without_dividing_by_zero() {
    let d = source();
    let mm = 72. / 25.4;
    for edge in ["r", "ctrX", "b", "ctrY"] {
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><constrLst><constr type='{edge}' for='ch' val='0'/></constrLst>
            <layoutNode><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>"
        ));
        for offset in [0., 103.7] {
            let out = evaluate(&l, &d, Xfrm::new(offset, offset, 20. * mm, 20. * mm)).unwrap();
            let r = out[0].rect;
            let (position, extent) = if matches!(edge, "r" | "ctrX") { (r.x, r.w) } else { (r.y, r.h) };
            assert!(extent.abs() < 1e-6);
            assert!((position - offset - 10. * mm).abs() < 1e-6, "{edge}: {r:?}");
        }
    }
}

#[test]
fn children_inherit_the_positive_size_of_a_negative_local_dimension() {
    let d = source();
    let mm = 72. / 25.4;
    for size in ["w", "h"] {
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><constrLst><constr type='{size}' val='-5'/></constrLst>
            <layoutNode><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>"
        ));
        let p = evaluate(&l, &d, Xfrm::new(0., 0., 20. * mm, 20. * mm)).unwrap();
        let extent = if size == "w" { p[0].rect.w } else { p[0].rect.h };
        assert!((extent - 5. * mm).abs() < 1e-6);
    }
}

fn resolved_children(l: &El, source: &El, rect: Xfrm) -> Vec<geometry::BoxStyle> {
    let data = Data::new(source).unwrap();
    let context = Context { point: data.root, position: 1, count: 1, vars: HashMap::new() };
    let mut compiler = Compiler { data, refs: HashMap::new(), instances: 0, work: 0 };
    let tree = compiler.node(l.child("layoutNode").unwrap(), context, 0).unwrap();
    let solution = constraints::resolve(&tree, rect, &compiler.data, &mut 0).unwrap();
    tree.children.iter().map(|child| constraints::lookup(&solution, child).unwrap()).collect()
}

#[test]
fn parent_allocation_precedes_self_sizes_and_self_bounds_do_not_allocate() {
    let d = source();
    let mm = 72. / 25.4;
    for vertical in [false, true] {
        let (start, end, center, size) = if vertical { ("t", "b", "ctrY", "h") } else { ("l", "r", "ctrX", "w") };
        for (edge, position) in [(start, 5.), (end, 15.), (center, 15.)] {
            for local in [2., 10., 30.] {
                for op in ["equ", "gte", "lte"] {
                    for parent in [None, Some(12.)] {
                        let allocation = parent.map(|v| format!("<constr type='{size}' for='ch' forName='subject' val='{v}'/>")).unwrap_or_default();
                        let l = layout(&format!(
                            "<layoutNode><alg type='composite'/><constrLst>{allocation}<constr type='{edge}' for='ch' forName='subject' val='{position}'/></constrLst>
                            <layoutNode name='subject'><alg type='tx'/><shape type='rect'/><constrLst><constr type='{size}' val='{local}' op='{op}'/></constrLst></layoutNode></layoutNode>"
                        ));
                        let child = resolved_children(&l, &d, Xfrm::new(0., 0., 20. * mm, 20. * mm))[0];
                        let (expected_start, expected_size) = if let Some(p) = parent {
                            (
                                if edge == start {
                                    position
                                } else if edge == end {
                                    position - p
                                } else {
                                    position - p / 2.
                                },
                                p,
                            )
                        } else if op == "equ" {
                            if edge == start { (position, local) } else { (0., 0.) }
                        } else if edge == start {
                            (position, 20. - position)
                        } else if edge == end {
                            (0., position)
                        } else {
                            (0., 0.)
                        };
                        assert!((child.get(size).unwrap() / mm - expected_size).abs() < 1e-6, "{edge}/{local}/{op}/{parent:?}");
                        assert!((child.get(start).unwrap() / mm - expected_start).abs() < 1e-6, "{edge}/{local}/{op}/{parent:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn unallocated_self_size_retains_the_parent_end_or_center_reference() {
    let d = source();
    let mm = 72. / 25.4;
    for vertical in [false, true] {
        let (start, end, center, size) = if vertical { ("t", "b", "ctrY", "h") } else { ("l", "r", "ctrX", "w") };
        for edge in [end, center] {
            for reference in [start, end, center, size] {
                let l = layout(&format!(
                    "<layoutNode><alg type='composite'/><constrLst><constr type='{edge}' for='ch' forName='subject' val='15'/>
                    <constr type='{start}' for='ch' forName='witness' refFor='ch' refForName='subject' refType='{reference}'/>
                    <constr type='w' for='ch' forName='witness' val='4'/><constr type='h' for='ch' forName='witness' val='4'/>
                    </constrLst><layoutNode name='subject'><alg type='tx'/><shape type='rect'/><constrLst><constr type='{size}' val='5'/></constrLst></layoutNode>
                    <layoutNode name='witness'><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>"
                ));
                let children = resolved_children(&l, &d, Xfrm::new(0., 0., 20. * mm, 20. * mm));
                assert!(children[0].get(size).unwrap().abs() < 1e-6);
                assert!(children[0].get(start).unwrap().abs() < 1e-6);
                let expected = if reference == edge { 15. } else { 0. };
                assert!((children[1].get(start).unwrap() / mm - expected).abs() < 1e-6, "{edge}/{reference}");
            }
        }
    }
}

#[test]
fn two_parent_coordinates_override_a_self_size_before_axis_priority() {
    let d = source();
    let mm = 72. / 25.4;
    for vertical in [false, true] {
        let (start, end, center, size) = if vertical { ("t", "b", "ctrY", "h") } else { ("l", "r", "ctrX", "w") };
        for (a, b, expected_start, expected_size) in [(start, end, 5., 13.), (start, center, 5., 26.), (end, center, 31., 26.)] {
            for local in [5., 20.] {
                let l = layout(&format!(
                    "<layoutNode><alg type='composite'/><constrLst><constr type='{a}' for='ch' val='5'/><constr type='{b}' for='ch' val='18'/></constrLst>
                    <layoutNode><alg type='tx'/><shape type='rect'/><constrLst><constr type='{size}' val='{local}'/></constrLst></layoutNode></layoutNode>"
                ));
                let child = resolved_children(&l, &d, Xfrm::new(0., 0., 20. * mm, 20. * mm))[0];
                assert!((child.get(size).unwrap() / mm - expected_size).abs() < 1e-6);
                assert!((child.get(start).unwrap() / mm - expected_start).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn spaces_with_children_enclose_and_centre_without_using_child_self_sizes() {
    let data = source();
    let mm = 72. / 25.4;
    for shape in ["none", "rect"] {
        for second in [false, true] {
            for self_size in [false, true] {
                for allocation in [None, Some(12.), Some(30.)] {
                    let size = allocation.map(|v| format!("<constr type='w' for='ch' forName='a' val='{v}'/>")).unwrap_or_default();
                    let own_size = if self_size { "<constr type='w' val='5'/>" } else { "" };
                    let other = if second { "<layoutNode name='b'><alg type='tx'/><presOf/><shape type='rect'/></layoutNode>" } else { "" };
                    let l = layout(&format!(
                        "<layoutNode name='space'><alg type='sp'/><presOf/><shape type='{shape}'/><constrLst>{size}
                        <constr type='h' for='ch' forName='a' val='6'/><constr type='l' for='ch' forName='a' val='5'/>
                        <constr type='t' for='ch' forName='a' val='10'/><constr type='w' for='ch' forName='b' val='4'/>
                        <constr type='h' for='ch' forName='b' val='4'/></constrLst>
                        <layoutNode name='a'><alg type='tx'/><presOf/><shape type='rect'/><constrLst>{own_size}
                        <constr type='primFontSz' val='24'/></constrLst></layoutNode>{other}</layoutNode>"
                    ));
                    for offset in [0., 123.5] {
                        let out = evaluate(&l, &data, Xfrm::new(offset, offset, 20. * mm, 20. * mm)).unwrap();
                        let a = out.iter().find(|p| p.name == "a").unwrap();
                        let w = allocation.unwrap_or(20.) * mm;
                        let expected = Xfrm::new(offset + (20. * mm - w) / 2., offset + 7. * mm, w, 6. * mm);
                        for (actual, expected) in [(a.rect.x, expected.x), (a.rect.y, expected.y), (a.rect.w, w), (a.rect.h, expected.h)] {
                            assert!((actual - expected).abs() < 1e-6, "{shape}/{second}/{self_size}/{allocation:?}: {:?}", a.rect);
                        }
                        assert_eq!(a.font, Some(24.), "space allocation does not replace text defaults");
                        if let Some(b) = out.iter().find(|p| p.name == "b") {
                            assert!((b.rect.x - a.rect.x).abs() < 1e-6);
                            assert!((b.rect.y - a.rect.y).abs() < 1e-6);
                        }
                        if let Some(own) = out.iter().find(|p| p.name == "space") {
                            for (outer, inner) in [(own.rect.x, a.rect.x), (own.rect.y, a.rect.y), (own.rect.w, a.rect.w), (own.rect.h, a.rect.h)] {
                                assert!((outer - inner).abs() < 1e-6);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn ancestor_self_size_defaults_keep_their_local_allocation_role() {
    let data = source();
    let mm = 72. / 25.4;
    for alg in ["composite", "sp", "lin"] {
        for edge in ["l", "r", "ctrX"] {
            for local in [false, true] {
                for allocated in [false, true] {
                    let size = if allocated { "<constr type='w' for='ch' forName='a' val='12'/>" } else { "" };
                    let own = if local { "<constrLst><constr type='w' val='5'/></constrLst>" } else { "" };
                    let l = layout(&format!(
                        "<layoutNode><alg type='composite'/><presOf/><constrLst><constr type='w' for='des' forName='a' val='15'/>
                        <constr type='w' for='ch' forName='box' val='20'/><constr type='h' for='ch' forName='box' val='20'/></constrLst>
                        <layoutNode name='box'><alg type='{alg}'/><presOf/><constrLst>{size}<constr type='{edge}' for='ch' forName='a' val='15'/>
                        <constr type='h' for='ch' forName='a' val='6'/></constrLst>
                        <layoutNode name='a'><alg type='tx'/><shape type='rect'/><presOf/>{own}</layoutNode></layoutNode></layoutNode>"
                    ));
                    let out = evaluate(&l, &data, frame()).unwrap();
                    let a = out.iter().find(|p| p.name == "a").unwrap();
                    let expected = if allocated {
                        12.
                    } else if alg == "sp" {
                        20.
                    } else if alg == "lin" || (local && edge == "l") || (!local && edge == "r") {
                        15.
                    } else if !local && edge == "l" {
                        5.
                    } else {
                        0.
                    };
                    assert!((a.rect.w - expected * mm).abs() < 1e-6, "{alg}/{edge}/{local}/{allocated}: {:?}", a.rect);
                }
            }
        }
    }
}

#[test]
fn linear_descendants_accept_ancestor_sizes_through_nested_containers() {
    let data = source();
    let mm = 72. / 25.4;
    for alg in ["composite", "sp", "lin"] {
        for local in [false, true] {
            for allocated in [false, true] {
                let size = if allocated { "<constr type='w' for='ch' forName='a' val='12'/>" } else { "" };
                let own = if local { "<constrLst><constr type='w' val='5'/></constrLst>" } else { "" };
                let l = layout(&format!(
                    "<layoutNode><alg type='composite'/><presOf/><constrLst><constr type='w' for='des' forName='a' val='15'/>
                    <constr type='w' for='ch' forName='box' val='20'/><constr type='h' for='ch' forName='box' val='20'/></constrLst>
                    <layoutNode name='box'><alg type='{alg}'/><presOf/><constrLst><constr type='w' for='ch' forName='flow' val='20'/>
                    <constr type='h' for='ch' forName='flow' val='20'/></constrLst>
                    <layoutNode name='flow'><alg type='lin'/><presOf/><constrLst>{size}<constr type='h' for='ch' val='6'/></constrLst>
                    <layoutNode name='a'><alg type='tx'/><shape type='rect'/><presOf/>{own}</layoutNode>
                    </layoutNode></layoutNode></layoutNode>"
                ));
                let out = evaluate(&l, &data, frame()).unwrap();
                let a = out.iter().find(|p| p.name == "a").unwrap();
                assert!((a.rect.w - (if allocated { 12. } else { 15. }) * mm).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn late_unnamed_geometry_does_not_replace_existing_allocations() {
    let data = source();
    let mm = 72. / 25.4;
    for name in ["", " forName='' "] {
        for op in ["", " op='equ' "] {
            for property in ["w", "h", "l", "t"] {
                let l = layout(&format!(
                    "<layoutNode><alg type='composite'/><presOf/><constrLst>
                    <constr type='w' for='ch' forName='a' val='20'/><constr type='h' for='ch' forName='a' val='20'/>
                    <constr type='w' for='ch' forName='b' val='30'/><constr type='h' for='ch' forName='b' val='10'/>
                    <constr type='l' for='ch' forName='a' val='0'/><constr type='l' for='ch' forName='b' val='40'/>
                    <constr type='t' for='ch' forName='a' val='0'/><constr type='t' for='ch' forName='b' val='10'/>
                    <constr type='{property}' for='ch' {name} {op} val='5'/></constrLst>
                    <layoutNode name='a'><alg type='tx'/><shape type='rect'/></layoutNode>
                    <layoutNode name='b'><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>"
                ));
                let out = evaluate(&l, &data, frame()).unwrap();
                assert!((out[0].rect.w - 20. * mm).abs() < 1e-6);
                assert!((out[1].rect.w - 30. * mm).abs() < 1e-6);
                assert!((out[0].rect.h - 20. * mm).abs() < 1e-6);
                assert!((out[1].rect.h - 10. * mm).abs() < 1e-6);
                assert!((out[1].rect.x - out[0].rect.x - 40. * mm).abs() < 1e-6);
                assert!((out[1].rect.y - out[0].rect.y - 10. * mm).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn size_inequalities_do_not_replace_initial_geometry_across_algorithms() {
    let data = source();
    for alg in ["composite", "lin", "sp", "cycle"] {
        for property in ["w", "h", "diam"] {
            for (op, value) in [("gte", 50.), ("lte", 15.)] {
                let build = |bound: bool| {
                    let extra =
                        if bound { format!("<constr type='{property}' for='ch' forName='a' op='{op}' val='{value}'/>") } else { String::new() };
                    layout(&format!(
                        "<layoutNode><alg type='{alg}'/><presOf/><constrLst>
                        <constr type='w' for='ch' forName='a' val='40'/><constr type='h' for='ch' forName='a' val='40'/>
                        <constr type='w' for='ch' forName='b' val='20'/><constr type='h' for='ch' forName='b' val='20'/>{extra}</constrLst>
                        <layoutNode name='a'><alg type='tx'/><shape type='rect'/></layoutNode>
                        <layoutNode name='b'><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>"
                    ))
                };
                let baseline = build(false);
                let bounded = build(true);
                let a = evaluate(&baseline, &data, frame()).unwrap();
                let b = evaluate(&bounded, &data, frame()).unwrap();
                assert_eq!(a.iter().map(|p| p.rect).collect::<Vec<_>>(), b.iter().map(|p| p.rect).collect::<Vec<_>>(), "{alg}/{property}/{op}");
            }
        }
    }
}

#[test]
fn size_bounds_do_not_cap_text_driven_growth() {
    let data = source();
    let mm = 72. / 25.4;
    for op in ["gte", "lte"] {
        for bound in [5., 20., 80.] {
            let l = layout(&format!(
                "<layoutNode><alg type='composite'/><presOf/><constrLst>
                <constr type='w' for='ch' forName='a' val='12'/><constr type='h' for='ch' forName='a' val='10'/>
                <constr type='w' for='ch' forName='a' op='{op}' val='{bound}'/></constrLst>
                <layoutNode name='a'><alg type='tx'/><shape type='rect'/><presOf axis='ch' cnt='1'/>
                <ruleLst><rule type='w' val='50'/></ruleLst></layoutNode></layoutNode>"
            ));
            let mut measure = |out: &[Placed<'_>], _: MeasureKind| Ok(out.iter().map(|p| (50. * mm - p.rect.w).max(0.)).collect());
            let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
            assert!((out[0].rect.w - 50. * mm).abs() < 1e-5, "{op}/{bound}: {:?}", out[0].rect);
        }
    }
}

#[test]
fn late_unnamed_size_does_not_rebind_a_factor_rule() {
    let data = source();
    let mm = 72. / 25.4;
    for name in ["", " forName='' "] {
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><presOf/><constrLst>
            <constr type='w' for='ch' forName='a' refFor='ch' refForName='b' refType='w' fact='0.5'/>
            <constr type='w' for='ch' forName='b' val='20'/><constr type='h' for='ch' val='10'/>
            <constr type='w' for='ch' {name} val='5'/></constrLst>
            <layoutNode name='a'><alg type='tx'/><shape type='rect'/><presOf axis='ch' cnt='1'/>
            <ruleLst><rule type='w' fact='1'/></ruleLst></layoutNode>
            <layoutNode name='b'><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>"
        ));
        let mut measure =
            |out: &[Placed<'_>], _: MeasureKind| Ok(out.iter().map(|p| if p.name == "a" { (15. * mm - p.rect.w).max(0.) } else { 0. }).collect());
        let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
        assert!((out[0].rect.w - 15. * mm).abs() < 1e-4);
        assert!((out[1].rect.w - 20. * mm).abs() < 1e-6);
    }
}

#[test]
fn intrinsic_flow_sizes_do_not_scale_with_later_parent_allocations() {
    let mm = 72. / 25.4;
    for alg in ["lin", "cycle"] {
        for (width, height) in [(20., 60.), (80., 30.), (200., 120.)] {
            for own in [false, true] {
                let source = source();
                let data = Data::new(&source).unwrap();
                let explicit = if own { "<constrLst><constr type='w' val='5'/><constr type='h' val='10'/></constrLst>" } else { "" };
                let l = layout(&format!(
                    "<layoutNode><alg type='composite'/><constrLst>
                    <constr type='w' for='ch' forName='flow' val='{width}'/>
                    <constr type='h' for='ch' forName='flow' val='{height}'/></constrLst>
                    <layoutNode name='flow'><alg type='{alg}'/>
                    <layoutNode name='card'><alg type='tx'/><shape type='rect'/>{explicit}</layoutNode>
                    </layoutNode></layoutNode>"
                ));
                let context = Context { point: data.root, position: 1, count: 1, vars: HashMap::new() };
                let mut compiler = Compiler { data, refs: HashMap::new(), instances: 0, work: 0 };
                let tree = compiler.node(l.child("layoutNode").unwrap(), context, 0).unwrap();
                let solution = constraints::resolve(&tree, frame(), &compiler.data, &mut 0).unwrap();
                let rect = constraints::lookup(&solution, &tree.children[0].children[0]).unwrap().rect;
                assert!((rect.w - if own { 5. } else { 100. } * mm).abs() < 1e-6, "{alg}/{width}/{height}: {rect:?}");
                assert!((rect.h - if own { 10. } else { 100. } * mm).abs() < 1e-6, "{alg}/{width}/{height}: {rect:?}");
            }
        }
    }
}

#[test]
fn linear_fitting_matches_native_normalized_search_in_both_axes() {
    let data = source();
    let mm = 72. / 25.4;
    for (width, height, horizontal, vertical) in [
        (40., 30., 15.99609375, 8.564453125),
        (80., 60., 31.9921875, 17.12890625),
        (200., 120., 39.9609375, 34.2578125),
        (270.933333333, 114.3, 39.9609375, 32.65625),
    ] {
        for dir in ["fromL", "fromR", "fromT", "fromB"] {
            let l = layout(&format!(
                "<layoutNode><alg type='lin'><param type='linDir' val='{dir}'/></alg><constrLst>
                <constr type='w' for='ch' forName='a' val='40'/><constr type='h' for='ch' forName='a' val='40'/>
                <constr type='w' for='ch' forName='b' val='40'/><constr type='h' for='ch' forName='b' val='40'/>
                <constr type='w' for='ch' forName='c' val='20'/><constr type='h' for='ch' forName='c' val='60'/></constrLst>
                <layoutNode name='a'><alg type='tx'/><shape type='rect'/></layoutNode>
                <layoutNode name='b'><alg type='tx'/><shape type='rect'/></layoutNode>
                <layoutNode name='c'><alg type='tx'/><shape type='rect'/></layoutNode></layoutNode>"
            ));
            let out = evaluate(&l, &data, Xfrm::new(0., 0., width * mm, height * mm)).unwrap();
            let along = if matches!(dir, "fromL" | "fromR") { out[0].rect.w } else { out[0].rect.h };
            let expected = if matches!(dir, "fromL" | "fromR") { horizontal } else { vertical };
            assert!((along - expected * mm).abs() < 1e-6, "{dir}/{width}/{height}: {along}");
        }
    }
}

#[test]
fn nested_linear_cross_sizes_use_the_nearest_composite_capacity() {
    let data = source();
    let mm = 72. / 25.4;
    for outer in ["composite", "sp", "lin"] {
        for vertical in [false, true] {
            let (primary, cross, direction) = if vertical { ("h", "w", "fromT") } else { ("w", "h", "fromL") };
            for available in [60., 120.] {
                for requested in [20_f64, 60., 200.] {
                    let l = layout(&format!(
                        "<layoutNode><alg type='composite'/><constrLst>
                        <constr type='{primary}' for='ch' forName='outer' val='20'/>
                        <constr type='{cross}' for='ch' forName='outer' val='30'/></constrLst>
                        <layoutNode name='outer'><alg type='{outer}'><param type='linDir' val='{direction}'/></alg><constrLst>
                        <constr type='{primary}' for='ch' forName='inner' val='20'/>
                        <constr type='{cross}' for='ch' forName='inner' val='10'/></constrLst>
                        <layoutNode name='inner'><alg type='lin'><param type='linDir' val='{direction}'/></alg>
                        <layoutNode name='card'><alg type='tx'/><shape type='rect'/><constrLst>
                        <constr type='{primary}' val='15'/><constr type='{cross}' val='{requested}'/>
                        </constrLst></layoutNode></layoutNode></layoutNode></layoutNode>"
                    ));
                    let frame = if vertical { Xfrm::new(0., 0., available * mm, 300. * mm) } else { Xfrm::new(0., 0., 300. * mm, available * mm) };
                    let out = evaluate(&l, &data, frame).unwrap();
                    assert_eq!(out.len(), 1);
                    let actual = if vertical { out[0].rect.w } else { out[0].rect.h };
                    let expected = requested.min(if outer == "composite" { 30. } else { available }) * mm;
                    assert!((actual - expected).abs() < 1e-6, "{outer}/{direction}/{available}/{requested}: {actual}");
                }
            }
        }
    }
}

#[test]
fn fitted_composite_reflows_linear_children_before_recentering() {
    let data = source();
    let mm = 72. / 25.4;
    for vertical in [false, true] {
        let (primary, cross, along, across, direction) = if vertical { ("h", "w", "t", "l", "fromT") } else { ("w", "h", "l", "t", "fromL") };
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><constrLst>
            <constr type='{primary}' for='ch' forName='anchor' val='40'/>
            <constr type='{cross}' for='ch' forName='anchor' val='40'/>
            <constr type='{along}' for='ch' forName='anchor' val='10'/>
            <constr type='{across}' for='ch' forName='anchor' val='20'/>
            <constr type='{primary}' for='ch' forName='flow' val='20'/>
            <constr type='{cross}' for='ch' forName='flow' val='10'/>
            <constr type='{along}' for='ch' forName='flow' val='180'/>
            <constr type='{across}' for='ch' forName='flow' val='20'/></constrLst>
            <layoutNode name='anchor'><alg type='tx'/><shape type='rect'/></layoutNode>
            <layoutNode name='flow'><alg type='lin'><param type='linDir' val='{direction}'/></alg>
            <layoutNode name='card'><alg type='tx'/><shape type='rect'/><constrLst>
            <constr type='{primary}' val='15'/><constr type='{cross}' val='60'/>
            </constrLst></layoutNode></layoutNode></layoutNode>"
        ));
        let frame = if vertical { Xfrm::new(0., 0., 60. * mm, 300. * mm) } else { Xfrm::new(0., 0., 300. * mm, 60. * mm) };
        let out = evaluate(&l, &data, frame).unwrap();
        let a = out.iter().find(|p| p.name == "anchor").unwrap().rect;
        let b = out.iter().find(|p| p.name == "card").unwrap().rect;
        let (start_a, size_a, start_b, size_b) = if vertical { (a.x, a.w, b.x, b.w) } else { (a.y, a.h, b.y, b.h) };
        // Independent native controls: the rigid anchor shrinks, but the flow
        // restores its child's intrinsic cross size, then the group is centred.
        assert!((size_a - 36.92307692307692 * mm).abs() < 1e-6);
        assert!((start_a - 24.23076923076923 * mm).abs() < 1e-6);
        assert!((size_b - 60. * mm).abs() < 1e-6);
        assert!((start_b - -1.153846153846154 * mm).abs() < 1e-6);
    }
}

#[test]
fn exactly_fitting_nested_linear_children_do_not_trigger_a_fit_from_roundoff() {
    let data = source();
    let mm = 72. / 25.4;
    for vertical in [false, true] {
        let (primary, cross, direction) = if vertical { ("h", "w", "fromT") } else { ("w", "h", "fromL") };
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><constrLst>
            <constr type='{primary}' for='ch' forName='flow' val='20'/>
            <constr type='{cross}' for='ch' forName='flow' val='10'/></constrLst>
            <layoutNode name='flow'><alg type='lin'><param type='linDir' val='{direction}'/></alg>
            <layoutNode name='a'><alg type='tx'/><shape type='rect'/><constrLst>
            <constr type='{primary}' val='15'/><constr type='{cross}' val='5'/></constrLst></layoutNode>
            <layoutNode name='b'><alg type='tx'/><shape type='rect'/><constrLst>
            <constr type='{primary}' val='5'/><constr type='{cross}' val='2.5'/></constrLst></layoutNode>
            </layoutNode></layoutNode>"
        ));
        let out = evaluate(&l, &data, frame()).unwrap();
        assert_eq!(out.len(), 2);
        for (p, expected) in out.iter().zip([15., 5.]) {
            let actual = if vertical { p.rect.h } else { p.rect.w };
            assert!((actual - expected * mm).abs() < 1e-6);
        }
    }
}

#[test]
fn cycle_refitting_depends_on_mapped_run_content_not_unrelated_data() {
    // Native controls distinguish literal run newlines, nonbreaking spaces,
    // standalone marks, paragraph breaks, and unconnected/document labels.
    let controls = [
        ("", false),
        ("Read", true),
        (" ", false),
        ("&#x200B;", false),
        ("&#10;", true),
        ("&#13;", true),
        ("&#9;", true),
        ("&#xA0;", true),
        ("&#x2003;", false),
        ("&#x202F;", true),
        ("&#x3000;", false),
        ("&#xFEFF;", false),
        ("&#x200C;", false),
        ("&#x200D;", false),
        ("&#x2060;", false),
        ("&#x200E;", false),
        ("&#x200F;", false),
        ("&#xAD;", false),
        ("&#x301;", false),
        ("&#x2028;", false),
        ("&#x2029;", false),
        (" &#10; ", true),
        ("  ", false),
        ("例", true),
        ("😀", true),
        ("…", true),
        ("م", true),
        ("क", true),
        ("ा", true),
        ("e&#x301;", true),
        ("&#x301;&#x302;", false),
        ("&#x2002;", false),
        ("&#x2007;", true),
    ];
    let mm = 72. / 25.4;
    for span in [30., 360.] {
        let l = layout(&format!(
            r#"<layoutNode name="root"><alg type="cycle"><param type="spanAng" val="{span}"/></alg>
            <forEach axis="ch" ptType="node"><layoutNode name="label"><presOf axis="self"/><alg type="tx"/><shape type="rect"/></layoutNode></forEach>
            <constrLst><constr type="w" for="ch" forName="label" val="10"/><constr type="h" for="ch" forName="label" val="10"/></constrLst></layoutNode>"#
        ));
        for (text, refit) in controls {
            let data = crate::xml::parse(format!(r#"<dataModel><ptLst>
                <pt modelId="root" type="doc"><t><p><r><t>Document text</t></r></p></t></pt>
                <pt modelId="a"><t><p><r><t>{text}</t></r><br/></p></t></pt><pt modelId="b"/><pt modelId="c"/>
                <pt modelId="unmapped"><t><p><r><t>Unused</t></r></p></t></pt></ptLst>
                <cxnLst><cxn srcId="root" destId="a" srcOrd="0"/><cxn srcId="root" destId="b" srcOrd="1"/><cxn srcId="root" destId="c" srcOrd="2"/></cxnLst></dataModel>"#).as_bytes()).unwrap().root;
            let out = evaluate(&l, &data, Xfrm::new(0., 0., 200. * mm, 120. * mm)).unwrap();
            assert_eq!(out.len(), 3);
            let (a, b) = (out[0].rect, out[1].rect);
            let angle = if span == 360. { 120_f64 } else { 15_f64 };
            let radius = (a.x - b.x).hypot(a.y - b.y) / (2. * (angle.to_radians() / 2.).sin());
            let native = match (span == 360., refit) {
                (false, false) => 159.843735,
                (false, true) => 159.687605,
                (true, false) => 59.941396,
                (true, true) => 59.882872,
            };
            assert!((radius - native * mm).abs() < 0.025, "{span}/{text}: {radius}");
        }
    }
}

#[test]
fn measured_cycle_refitting_uses_overflow_and_rejects_invalid_measurements() {
    let l = layout(
        r#"<layoutNode name="ring"><alg type="cycle"/><forEach axis="ch" ptType="node">
        <layoutNode name="label"><alg type="tx"/><presOf axis="self"/><shape type="rect"/></layoutNode></forEach>
        <constrLst><constr type="w" for="ch" forName="label" val="10"/><constr type="h" for="ch" forName="label" val="10"/>
        <constr type="primFontSz" for="ch" val="8"/></constrLst></layoutNode>"#,
    );
    let data = source();
    let mm = 72. / 25.4;
    for (overflow, expected_radius) in [(0., 59.941396), (1., 59.882872)] {
        let mut measure = |p: &[Placed<'_>], words: MeasureKind| {
            assert_eq!(words, MeasureKind::WordWrap);
            Ok(vec![overflow; p.len()])
        };
        let out = evaluate_measured(&l, &data, Xfrm::new(0., 0., 200. * mm, 120. * mm), Some(&mut measure)).unwrap();
        let (a, b) = (out[0].rect, out[1].rect);
        let radius = (a.x - b.x).hypot(a.y - b.y) / 3_f64.sqrt();
        assert!((radius - expected_radius * mm).abs() < 0.025);
    }
    for invalid in [f64::NAN, f64::INFINITY, -1.] {
        let mut measure = |p: &[Placed<'_>], _: MeasureKind| Ok(vec![invalid; p.len()]);
        assert!(evaluate_measured(&l, &data, frame(), Some(&mut measure)).is_err());
    }
}

#[test]
fn cycle_path_rotation_follows_shape_fitting_and_preserves_center_nodes() {
    let data = source();
    let mm = 72. / 25.4;
    for center in ["none", "fNode"] {
        for own_angle in [0., 30.] {
            let l = layout(&format!(
                r#"<layoutNode name="ring"><alg type="cycle"><param type="stAng" val="17"/>
                <param type="rotPath" val="alongPath"/><param type="ctrShpMap" val="{center}"/></alg>
                <forEach axis="ch" ptType="node"><layoutNode name="label"><alg type="tx"/><presOf axis="self"/><shape type="rect" rot="{own_angle}"/></layoutNode></forEach>
                <constrLst><constr type="w" for="ch" forName="label" val="25"/><constr type="h" for="ch" forName="label" val="15"/>
                <constr type="primFontSz" for="ch" val="8"/></constrLst></layoutNode>"#
            ));
            let mut fits = |p: &[Placed<'_>], _: MeasureKind| Ok(vec![0.; p.len()]);
            let out = evaluate_measured(&l, &data, Xfrm::new(0., 0., 200. * mm, 120. * mm), Some(&mut fits)).unwrap();
            for (i, p) in out.iter().enumerate() {
                let path = if center == "fNode" && i == 0 {
                    0.
                } else {
                    17. + (i - usize::from(center == "fNode")) as f64 * if center == "fNode" { 180. } else { 120. }
                };
                assert!((p.rect.rot - (own_angle + path).rem_euclid(360.)).abs() < 1e-6);
                let (w, h) = if own_angle == 0. { (25., 15.) } else { (15., 8.660254) };
                assert!((p.rect.w - w * mm).abs() < 0.025 && (p.rect.h - h * mm).abs() < 0.025);
            }
            if center == "none" && own_angle == 30. {
                assert!((out[0].text_rect.unwrap().rot - 317.).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn cycles_pack_container_occupied_frames_without_rotating_the_container() {
    let mm = 72. / 25.4;
    let data = source();
    for alg in ["composite", "sp", "lin"] {
        let l = layout(&format!(
            r#"<layoutNode name="ring"><alg type="cycle"><param type="stAng" val="17"/>
            <param type="spanAng" val="270"/><param type="rotPath" val="alongPath"/></alg>
            <forEach axis="ch" ptType="node"><layoutNode name="card"><alg type="{alg}"/><presOf/><shape type="rect"/>
            <constrLst><constr type="w" for="ch" forName="inner" val="12"/><constr type="h" for="ch" forName="inner" val="8"/>
            <constr type="l" for="ch" forName="inner" val="4"/><constr type="t" for="ch" forName="inner" val="2"/></constrLst>
            <layoutNode name="inner"><alg type="tx"/><presOf/><shape type="none"/></layoutNode></layoutNode></forEach>
            <constrLst><constr type="w" for="ch" forName="card" val="25"/><constr type="h" for="ch" forName="card" val="15"/></constrLst></layoutNode>"#
        ));
        let mut fits = |p: &[Placed<'_>], _: MeasureKind| Ok(vec![0.; p.len()]);
        let out = evaluate_measured(&l, &data, Xfrm::new(10. * mm, 10. * mm, 200. * mm, 120. * mm), Some(&mut fits)).unwrap();
        let card = out.iter().find(|p| p.name == "card").unwrap();
        assert_eq!(card.rect.rot, 0.);
        for (actual, native) in [card.rect.x, card.rect.y, card.rect.w, card.rect.h].into_iter().zip([136.565857, 10.103948, 12., 8.]) {
            assert!((actual - native * mm).abs() < 0.025, "{alg}: {actual} vs {native}");
        }
    }
}

#[test]
fn nested_typography_uses_selectors_instead_of_parent_inheritance() {
    let data = source();
    for alg in ["composite", "sp", "lin"] {
        for scope in ["parent", "child", "self"] {
            let child = if scope == "child" { " for='ch' forName='label'" } else { "" };
            let own = if scope == "self" {
                "<constrLst><constr type='primFontSz' val='18'/><constr type='secFontSz' val='12'/><constr type='lMarg' val='2'/></constrLst>"
            } else {
                ""
            };
            let l = layout(&format!(
                "<layoutNode><alg type='{alg}'/><presOf axis='ch' ptType='node' cnt='1'/><shape type='rect'/>
                <constrLst><constr type='primFontSz'{child} val='8'/><constr type='secFontSz'{child} val='10'/><constr type='lMarg'{child} val='0'/>
                <constr type='w' for='ch' val='40'/><constr type='h' for='ch' val='30'/></constrLst>
                <layoutNode name='label'><alg type='tx'/><presOf axis='ch' ptType='node' cnt='1'/><shape type='rect'/>{own}</layoutNode></layoutNode>"
            ));
            let out = evaluate(&l, &data, frame()).unwrap();
            let label = out.iter().find(|p| p.name == "label").unwrap();
            let (font, secondary, left) = match scope {
                "child" => (8., Some(10.), 0.),
                "self" => (18., Some(12.), 2. * 72. / 25.4),
                _ => (36., None, 36. * 0.56),
            };
            assert_eq!((label.font, label.secondary_font), (Some(font), secondary));
            assert!((label.margins[0] - left).abs() < 1e-6);
        }
    }
}

#[test]
fn helper_visibility_controls_borrowing_and_cycle_refitting() {
    let data = source();
    for (shape, borrowed, painted) in [
        ("", false, false),
        ("<shape type='none'/>", false, false),
        ("<shape type='rect' hideGeom='1'/>", false, true),
        ("<shape type='rect'/>", true, true),
    ] {
        let l = layout(&format!(
            "<layoutNode><alg type='cycle'/><constrLst><constr type='w' for='ch' val='20'/><constr type='h' for='ch' val='20'/></constrLst>
            <forEach axis='ch' ptType='node'><layoutNode name='outer'><alg type='composite'/><presOf axis='self'/><shape type='rect'/>
            <constrLst><constr type='w' for='ch' val='10'/><constr type='h' for='ch' val='10'/></constrLst>
            <layoutNode name='label'><alg type='tx'/><presOf axis='self'/>{shape}</layoutNode></layoutNode></forEach></layoutNode>"
        ));
        let mut fits = |p: &[Placed<'_>], _: MeasureKind| Ok(vec![0.; p.len()]);
        let mut overflowing = |p: &[Placed<'_>], _: MeasureKind| Ok(p.iter().map(|p| f64::from(p.name == "label")).collect());
        let frame = Xfrm::new(0., 0., 200. * 72. / 25.4, 120. * 72. / 25.4);
        let initial = evaluate_measured(&l, &data, frame, Some(&mut fits)).unwrap();
        let fitted = evaluate_measured(&l, &data, frame, Some(&mut overflowing)).unwrap();
        let a = initial.iter().find(|p| p.name == "outer").unwrap();
        let b = fitted.iter().find(|p| p.name == "outer").unwrap();
        assert_eq!(b.text_enabled, borrowed);
        assert_eq!(b.text_source.is_some(), borrowed);
        assert_eq!(a.rect == b.rect, !painted);
    }
}

#[test]
fn font_reference_recovery_distinguishes_declaration_scopes_and_cycles() {
    let data = source();
    let leaf = "<layoutNode name='a'><alg type='tx'/><presOf axis='ch' st='1' cnt='1'/><shape type='ellipse'/></layoutNode><layoutNode name='b'><alg type='tx'/><presOf axis='ch' st='2' cnt='1'/><shape type='ellipse'/></layoutNode>";
    let decl = "<constr type='primFontSz' for='ch' forName='a' val='24'/>";
    let reference = "<constr type='primFontSz' for='ch' forName='b' refFor='ch' refForName='a' refType='primFontSz' fact='0.5'/>";
    for (body, recover) in [
        (format!("{decl}{reference}"), false),
        (format!("{reference}{decl}"), false),
        (reference.to_string(), true),
        (format!("{decl}{decl}"), true),
        (format!("{decl}{}", reference.replace("refFor='ch' refForName='a'", "refFor='self'")), true),
        (format!("{decl}{}", reference.replace("refForName='a'", "refForName='b'")), true),
        (format!("{}{}", decl.replace("for='ch'", "for='des'"), reference), true),
        (
            format!("{}{}", decl.replace("for='ch'", "for='des'"), reference.replace("for='ch'", "for='des'").replace("refFor='ch'", "refFor='des'")),
            false,
        ),
        (format!("{}{}", decl.replace("forName='a'", ""), reference.replace("refForName='a'", "")), true),
    ] {
        let l = layout(&format!("<layoutNode name='Unrelated title'><alg type='composite'/><constrLst>{body}</constrLst>{leaf}</layoutNode>"));
        let out = evaluate(&l, &data, frame()).unwrap();
        assert!(out.iter().all(|p| p.recovered == recover), "{body}");
        assert_eq!(out.len(), if recover { 3 } else { 2 });
        if recover {
            assert!(out.iter().all(|p| p.shape.unwrap().attr("type") == Some("rect")));
            assert_eq!(out[0].points.iter().map(|p| p.attr("modelId").unwrap()).collect::<Vec<_>>(), ["a", "a1", "a2"]);
            assert_eq!(out[0].point_levels, [0, 1, 1]);
        }
    }
}

#[test]
fn recovered_text_uses_a_shared_bounded_fit_and_rejects_bad_measurements() {
    let data = source();
    let l = layout("<layoutNode><constrLst><constr type='primFontSz' refType='primFontSz'/></constrLst></layoutNode>");
    for ceiling in [0., 5., 23., 65., 100.] {
        let mut calls = 0;
        let mut measure = |items: &[Placed<'_>], words: MeasureKind| {
            calls += 1;
            assert_eq!(words, MeasureKind::WordWrap);
            Ok(items.iter().enumerate().map(|(i, p)| if i == 1 { (p.font.unwrap() - ceiling).max(0.) } else { 0. }).collect())
        };
        let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
        assert!(calls <= 7);
        assert!(out.iter().all(|p| p.font == Some(ceiling.clamp(5., 65.))));
    }
    for values in [vec![], vec![f64::NAN; 3], vec![-1.; 3], vec![f64::INFINITY; 3]] {
        let mut measure = |_: &[Placed<'_>], _: MeasureKind| Ok(values.clone());
        assert!(evaluate_measured(&l, &data, frame(), Some(&mut measure)).is_err());
    }
}

#[test]
fn zero_font_equalities_do_not_assign_or_declare_dependencies() {
    let data = source();
    let leaf = "<layoutNode name='a'><alg type='tx'/><presOf axis='ch' st='1' cnt='1'/><shape type='ellipse'/></layoutNode><layoutNode name='b'><alg type='tx'/><presOf axis='ch' st='2' cnt='1'/><shape type='ellipse'/></layoutNode>";
    for property in ["primFontSz", "secFontSz"] {
        let shown = |p: &Placed<'_>| {
            if property == "primFontSz" { p.font.unwrap() } else { p.secondary_font.unwrap_or_else(|| (p.font.unwrap() * 0.78).round()) }
        };
        let default = if property == "primFontSz" { 36. } else { 28. };
        for (op, value, expected) in [("equ", "0", default), ("none", "0", 1.), ("equ", "0.1", 1.), ("equ", "-1", 1.)] {
            for selector in ["", "for='ch' forName='a'"] {
                let declaration = format!("<constr type='{property}' {selector} op='{op}' val='{value}'/>");
                let body = if selector.is_empty() {
                    format!(
                        "<layoutNode><alg type='tx'/><presOf axis='ch' st='1' cnt='1'/><shape type='ellipse'/><constrLst>{declaration}</constrLst></layoutNode>"
                    )
                } else {
                    format!("<layoutNode><alg type='composite'/><constrLst>{declaration}</constrLst>{leaf}</layoutNode>")
                };
                let xml = layout(&body);
                let out = evaluate(&xml, &data, frame()).unwrap();
                assert!(!out[0].recovered, "{declaration}");
                assert_eq!(shown(&out[0]), expected, "{declaration}");
            }
        }
        let zero = format!("<constr type='{property}' for='ch' forName='a' op='equ' val='0'/>");
        let prior = format!("<constr type='{property}' for='ch' forName='a' val='24'/>");
        let reference = format!("<constr type='{property}' for='ch' forName='b' refFor='ch' refForName='a' refType='{property}' fact='0.5'/>");
        for (body, recovered, sizes) in [
            (format!("{zero}{zero}"), false, [default, default]),
            (format!("{prior}{zero}{reference}"), false, [24., 12.]),
            (format!("{zero}{reference}"), true, [65., 65.]),
            (reference.replace("fact='0.5'", "fact='0' op='equ'"), false, [default, default]),
        ] {
            let xml = layout(&format!("<layoutNode><alg type='composite'/><constrLst>{body}</constrLst>{leaf}</layoutNode>"));
            let out = evaluate(&xml, &data, frame()).unwrap();
            assert!(out.iter().all(|p| p.recovered == recovered), "{body}");
            if !recovered {
                assert_eq!(out.iter().map(shown).collect::<Vec<_>>(), sizes, "{body}");
            }
        }
    }
}

#[test]
fn width_rules_keep_unbroken_words_but_height_rules_only_fit_lines() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='tx'/><shape type='rect'/><constrLst>
         <constr type='w' val='20'/><constr type='h' val='20'/></constrLst>
         <ruleLst><rule type='w' val='60'/><rule type='h' val='60'/></ruleLst></layoutNode>",
    );
    let mm = 72. / 25.4;
    let mut measure = |items: &[Placed<'_>], kind: MeasureKind| {
        Ok(items.iter().map(|p| if kind == MeasureKind::WordWrap && p.rect.w < 42. * mm { 1. } else { 0. }).collect())
    };
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert!((out[0].rect.w / mm - 42.5).abs() < 1e-6);
    assert!((out[0].rect.h / mm - 20.).abs() < 1e-6);
}

#[test]
fn font_before_growth_does_not_retroactively_reclaim_later_space() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='tx'/><shape type='rect'/><constrLst>
         <constr type='w' val='20'/><constr type='h' val='15'/><constr type='primFontSz' val='28'/></constrLst>
         <ruleLst><rule type='primFontSz' val='12'/><rule type='h' val='70'/><rule type='w' val='50'/></ruleLst></layoutNode>",
    );
    let mut measure = |items: &[Placed<'_>], kind: MeasureKind| {
        Ok(items
            .iter()
            .map(|p| {
                let narrow = p.rect.w < 90.;
                let height = (if narrow { 60. } else { 30. } - p.rect.h).max(0.);
                height + if kind == MeasureKind::WordWrap { (p.font.unwrap_or(28.) - 12.).max(0.) + f64::from(narrow) } else { 0. }
            })
            .collect())
    };
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert_eq!(out[0].font, Some(12.));
    assert!((out[0].rect.h * 25.4 / 72. - 21.875).abs() < 1e-6);
    assert!(out[0].rect.w >= 90.);
}

#[test]
fn later_font_shrink_can_restore_the_exact_original_size() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='tx'/><shape type='rect'/><constrLst>
         <constr type='h' val='10'/><constr type='primFontSz' val='28'/></constrLst>
         <ruleLst><rule type='h' val='50'/><rule type='primFontSz' val='8'/></ruleLst></layoutNode>",
    );
    let mut measure = |items: &[Placed<'_>], kind: MeasureKind| {
        Ok(items
            .iter()
            .map(|p| {
                let font = p.font.unwrap_or(28.);
                (font * 3. - p.rect.h).max(0.) + if kind == MeasureKind::WordWrap { (font - 8.).max(0.) } else { 0. }
            })
            .collect())
    };
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert_eq!(out[0].font, Some(8.));
    assert!((out[0].rect.h - 10. * 72. / 25.4).abs() < 1e-6);
}

#[test]
fn linear_allocation_remeasures_wrapped_height_before_later_rules() {
    let data = source();
    let leaf = "<layoutNode><alg type='tx'/><shape type='rect'/><constrLst>
        <constr type='w' val='20'/><constr type='h' val='10'/><constr type='primFontSz' val='20'/></constrLst>
        <ruleLst><rule type='w' val='90'/><rule type='h' val='60'/></ruleLst></layoutNode>";
    let l = layout(&format!("<layoutNode><alg type='lin'/>{leaf}{leaf}</layoutNode>"));
    let mm = 72. / 25.4;
    let mut measure = |items: &[Placed<'_>], kind: MeasureKind| {
        Ok(items
            .iter()
            .map(|p| {
                let narrow = p.rect.w < 60. * mm;
                let height = (if narrow { 40. * mm } else { 8. * mm } - p.rect.h).max(0.);
                height + if kind == MeasureKind::WordWrap { f64::from(narrow) } else { 0. }
            })
            .collect())
    };
    let out = evaluate_measured(&l, &data, Xfrm::new(0., 0., 100. * mm, 100. * mm), Some(&mut measure)).unwrap();
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|p| p.font == Some(20.)));
    assert!(out.iter().all(|p| (40. ..=42.).contains(&(p.rect.h / mm))), "heights: {:?}", out.iter().map(|p| p.rect.h / mm).collect::<Vec<_>>());
    assert!(out.iter().map(|p| p.rect.w).sum::<f64>() <= 100. * mm);
}

#[test]
fn linear_font_search_respects_the_enclosing_cross_axis_capacity() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='lin'/><layoutNode><alg type='tx'/><shape type='rect'/><constrLst>
        <constr type='w' val='20'/><constr type='h' val='10'/><constr type='primFontSz' val='28'/></constrLst>
        <ruleLst><rule type='w' val='90'/><rule type='h' val='70'/><rule type='primFontSz' val='10'/></ruleLst>
        </layoutNode></layoutNode>",
    );
    let mm = 72. / 25.4;
    let mut measure = |items: &[Placed<'_>], kind: MeasureKind| {
        Ok(items
            .iter()
            .map(|p| {
                (p.font.unwrap_or(28.) * 3. - p.rect.h).max(0.) + if kind == MeasureKind::WordWrap { f64::from(p.rect.w < 60. * mm) } else { 0. }
            })
            .collect())
    };
    let out = evaluate_measured(&l, &data, Xfrm::new(0., 0., 100. * mm, 25. * mm), Some(&mut measure)).unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].font.is_some_and(|size| (10. ..28.).contains(&size)));
    assert!(out[0].rect.h <= 25. * mm);
}

#[test]
fn linear_height_reclaim_after_font_shrink_does_not_require_unbroken_words() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='lin'/><layoutNode><alg type='tx'/><shape type='rect'/><constrLst>
        <constr type='w' val='20'/><constr type='h' val='10'/><constr type='primFontSz' val='28'/></constrLst>
        <ruleLst><rule type='h' val='70'/><rule type='primFontSz' val='8'/></ruleLst>
        </layoutNode></layoutNode>",
    );
    let mut measure = |items: &[Placed<'_>], kind: MeasureKind| {
        Ok(items
            .iter()
            .map(|p| {
                let font = p.font.unwrap_or(28.);
                (font * 3. - p.rect.h).max(0.) + if kind == MeasureKind::WordWrap { (font - 8.).max(0.) + 1. } else { 0. }
            })
            .collect())
    };
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].font, Some(8.));
    assert!((out[0].rect.h - 10. * 72. / 25.4).abs() < 1e-6);
    assert_eq!(measure(&out, MeasureKind::Overflow).unwrap(), [0.]);
    assert_eq!(measure(&out, MeasureKind::WordWrap).unwrap(), [1.]);
}

#[test]
fn linear_font_reclaims_height_before_a_later_width_rule_changes_wrapping() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='lin'/><layoutNode><alg type='tx'/><shape type='rect'/><constrLst>
        <constr type='w' val='20'/><constr type='h' val='10'/><constr type='primFontSz' val='28'/></constrLst>
        <ruleLst><rule type='h' val='70'/><rule type='primFontSz' val='14'/><rule type='w' val='90'/></ruleLst>
        </layoutNode></layoutNode>",
    );
    let mm = 72. / 25.4;
    let mut measure = |items: &[Placed<'_>], kind: MeasureKind| {
        Ok(items
            .iter()
            .map(|p| {
                let font = p.font.unwrap_or(28.);
                let narrow = p.rect.w < 60. * mm;
                let height = (font * 1.2 * if narrow { 3. } else { 1. } - p.rect.h).max(0.);
                height + if kind == MeasureKind::WordWrap { (font - 14.).max(0.) + f64::from(narrow) } else { 0. }
            })
            .collect())
    };
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert_eq!(out[0].font, Some(14.));
    assert!(out[0].rect.w >= 60. * mm);
    // The font stage still has three wrapped lines. Later width growth must not
    // retroactively reclaim that height as if it had already been one line.
    assert!((18. ..20.).contains(&(out[0].rect.h / mm)), "height = {}", out[0].rect.h / mm);
}

#[test]
fn partial_height_reclaim_uses_the_authored_interval_after_font_fitting() {
    let data = source();
    let mm = 72. / 25.4;
    for (limit, expected) in [(50, 18.75), (70, 19.375), (90, 20.)] {
        let l = layout(&format!(
            "<layoutNode><alg type='lin'/><layoutNode><alg type='tx'/><shape type='rect'/><constrLst>
            <constr type='w' val='20'/><constr type='h' val='10'/><constr type='primFontSz' val='28'/></constrLst>
            <ruleLst><rule type='h' val='{limit}'/><rule type='primFontSz' val='14'/></ruleLst>
            </layoutNode></layoutNode>"
        ));
        let mut measure = |items: &[Placed<'_>], kind: MeasureKind| {
            Ok(items
                .iter()
                .map(|p| {
                    let font = p.font.unwrap_or(28.);
                    (font * 3.6 - p.rect.h).max(0.) + if kind == MeasureKind::WordWrap { (font - 14.).max(0.) } else { 0. }
                })
                .collect())
        };
        let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].font, Some(14.));
        assert!((out[0].rect.h / mm - expected).abs() < 1e-9, "limit {limit}: {}", out[0].rect.h / mm);
        assert_eq!(measure(&out, MeasureKind::Overflow).unwrap(), [0.]);
    }
}

#[test]
fn shrinking_font_limits_survive_an_unchanged_forced_break_count() {
    let data = source();
    for limit in [26, 28] {
        let l = layout(&format!(
            "<layoutNode><alg type='lin'/><layoutNode><alg type='tx'/><shape type='rect'/><constrLst>
            <constr type='w' val='101'/><constr type='h' val='65'/><constr type='primFontSz' val='29'/></constrLst>
            <ruleLst><rule type='primFontSz' val='{limit}'/></ruleLst></layoutNode></layoutNode>"
        ));
        let mut measure = |items: &[Placed<'_>], kind: MeasureKind| Ok(vec![if kind == MeasureKind::WordWrap { 2. } else { 0. }; items.len()]);
        let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].font, Some(f64::from(limit)));
    }
}

#[test]
fn shrinking_font_does_not_accept_a_worse_endpoint_score() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='lin'/><layoutNode><alg type='tx'/><shape type='rect'/><constrLst>
        <constr type='w' val='101'/><constr type='h' val='65'/><constr type='primFontSz' val='29'/></constrLst>
        <ruleLst><rule type='primFontSz' val='26'/></ruleLst></layoutNode></layoutNode>",
    );
    let mut measure = |items: &[Placed<'_>], kind: MeasureKind| {
        Ok(items.iter().map(|item| if kind == MeasureKind::WordWrap { if item.font.unwrap_or(29.) < 29. { 3. } else { 2. } } else { 0. }).collect())
    };
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].font, Some(29.));
}

#[test]
fn allocated_linear_rules_revisit_cross_dimensions_in_authored_order() {
    let data = source();
    let mm = 72. / 25.4;
    for nested in [false, true] {
        for repeated in [false, true] {
            for initially_fits in [false, true] {
                let (width, initial_height, limit) = if initially_fits { (110., 70., 100.) } else { (163., 16., 59.) };
                let extra = if repeated { "<rule type='h' val='71'/>" } else { "" };
                let mut flow = String::from("<layoutNode name='flow'><alg type='lin'/>");
                for (name, w, h, rules) in [
                    ("first", 17., 12., "<rule type='w' val='100'/><rule type='h' val='79'/>".to_string()),
                    ("second", 31., 18., "<rule type='w' val='83'/><rule type='h' val='67'/>".to_string()),
                    ("last", 23., initial_height, format!("<rule type='h' val='{limit}'/>{extra}<rule type='w' val='71'/>")),
                ] {
                    flow.push_str(&format!(
                        "<layoutNode name='{name}'><alg type='tx'/><presOf axis='ch'/><shape type='rect'/>
                        <constrLst><constr type='w' val='{w}'/><constr type='h' val='{h}'/></constrLst>
                        <ruleLst>{rules}</ruleLst></layoutNode>"
                    ));
                }
                flow.push_str("</layoutNode>");
                let body = if nested {
                    format!(
                        "<layoutNode><alg type='composite'/><constrLst>
                        <constr type='w' for='ch' forName='flow' val='{width}'/>
                        <constr type='h' for='ch' forName='flow' val='120'/>
                        </constrLst>{flow}</layoutNode>"
                    )
                } else {
                    flow
                };
                let mut measure = |items: &[Placed<'_>], kind: MeasureKind| {
                    Ok(items
                        .iter()
                        .map(|p| {
                            let width = p.rect.w / mm;
                            let height = match p.name.as_str() {
                                "first" => 18.,
                                "second" => 100.,
                                _ if width < 20. => 78.,
                                _ if width < 25. => 67.,
                                _ if width < 33. => 55.,
                                _ => 30.,
                            };
                            let words = if p.name == "last" { 33. } else { 40. };
                            (height * mm - p.rect.h).max(0.) + if kind == MeasureKind::WordWrap { (words - width).max(0.) } else { 0. }
                        })
                        .collect())
                };
                let definition = layout(&body);
                let out = evaluate_measured(&definition, &data, Xfrm::new(0., 0., width * mm, 120. * mm), Some(&mut measure)).unwrap();
                let last = out.iter().find(|p| p.name == "last").unwrap();
                if initially_fits {
                    // A previously fitting height must grow when allocation adds lines,
                    // including at the root. Later smaller limits cannot erase that fit.
                    assert!((last.rect.h / mm - 78.4375).abs() < 1e-7, "nested={nested}, repeated={repeated}: {}", last.rect.h / mm);
                } else if nested {
                    // The second height rule sees the first one's new fit and is skipped.
                    assert!((last.rect.h / mm - 56.3125).abs() < 1e-7, "repeated={repeated}: {}", last.rect.h / mm);
                } else {
                    assert!(last.rect.h / mm >= 59. - 1e-7);
                }
            }
        }
    }
}
