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
    assert_eq!(detail.rect, Xfrm::new(0.0, 75.0, 200.0, 225.0));
    assert!(placed.iter().all(|p| p.font == Some(16.0)));
    let titles: Vec<_> = placed.iter().filter(|p| p.name == "title").collect();
    assert_eq!(titles.iter().map(|p| p.rect.x).collect::<Vec<_>>(), [0.0, 200.0, 400.0]);
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
    assert_eq!((out[0].rect.w, out[0].rect.h, out[0].rect.rot), (72.0, 36.0, 90.0));
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
    assert_eq!(out[0].rect, Xfrm::new(240.0, 150.0, 120.0, 0.0));
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
    assert_eq!(p.iter().map(|p| (p.rect.x, p.rect.y)).collect::<Vec<_>>(), [(300.0, 150.0), (300.0, 0.0), (0.0, 0.0)]);
}

#[test]
fn absolute_spacing_survives_linear_pack() {
    let d = source();
    let l = layout(
        r#"<layoutNode><alg type="lin"/><constrLst><constr type="sp" val="25.4"/></constrLst>
    <forEach axis="ch" ptType="node"><layoutNode><presOf axis="self"/><alg type="tx"/><shape type="rect"/></layoutNode></forEach></layoutNode>"#,
    );
    let p = evaluate(&l, &d, frame()).unwrap();
    assert_eq!(p[1].rect.x - p[0].rect.x - p[0].rect.w, 72.0);
    assert_eq!(p[2].rect.x + p[2].rect.w, 600.0);
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
