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

#[test]
fn coupled_cycles_edges_and_descendant_references_solve_together() {
    let data = source();
    let layout = layout(
        r#"<layoutNode name="root"><alg type="composite"/>
      <layoutNode name="box"><alg type="composite"/><layoutNode name="leaf"><alg type="tx"/><shape type="rect"/></layoutNode></layoutNode>
      <layoutNode name="other"><alg type="tx"/><shape type="rect"/></layoutNode>
      <constrLst>
        <constr type="w" for="ch" forName="box" refFor="ch" refForName="other" refType="w" fact="0.5"/>
        <constr type="w" for="ch" forName="other" refFor="ch" refForName="box" refType="w" fact="2"/>
        <constr type="w" for="ch" forName="other" op="gte" val="40"/>
        <constr type="w" for="ch" forName="other" op="lte" val="40"/>
        <constr type="l" for="des" forName="leaf" val="5"/>
        <constr type="r" for="des" forName="leaf" refFor="ch" refForName="other" refType="w"/>
      </constrLst></layoutNode>"#,
    );
    let shapes = evaluate(&layout, &data, frame()).unwrap();
    let other = shapes.iter().find(|s| s.name == "other").unwrap();
    let leaf = shapes.iter().find(|s| s.name == "leaf").unwrap();
    assert!((other.rect.w - 40. * 72. / 25.4).abs() < 1e-6);
    assert!((leaf.rect.x - other.rect.x - 5. * 72. / 25.4).abs() < 1e-6);
    assert!((leaf.rect.x + leaf.rect.w - other.rect.x - other.rect.w).abs() < 1e-6);
}

#[test]
fn longer_than_eight_reference_chain_and_tightening_bounds() {
    let data = source();
    let mut xml = String::from("<layoutNode><alg type='composite'/>");
    for i in 0..24 {
        xml += &format!("<layoutNode name='n{i}'><alg type='tx'/><shape type='rect'/></layoutNode>");
    }
    xml += "<constrLst>";
    for i in 0..23 {
        xml += &format!("<constr type='w' for='ch' forName='n{i}' refFor='ch' refForName='n{}' refType='w'/>", i + 1);
    }
    xml += "<constr type='w' for='ch' forName='n23' op='gte' val='30'/><constr type='w' for='ch' forName='n23' op='gte' val='40'/><constr type='w' for='ch' forName='n23' op='lte' val='50'/></constrLst></layoutNode>";
    let layout = layout(&xml);
    let shapes = evaluate(&layout, &data, frame()).unwrap();
    assert_eq!(shapes.len(), 24);
    assert!(shapes.iter().all(|s| (s.rect.w - 50. * 72. / 25.4).abs() < 1e-6));
}

#[test]
fn inconsistent_cycles_and_missing_named_references_fail_instead_of_guessing() {
    let data = source();
    for reference in ["a", "missing"] {
        let layout = layout(&format!(
            "<layoutNode><alg type='composite'/><layoutNode name='a'><shape type='rect'/></layoutNode><constrLst><constr type='w' for='ch' forName='a' refFor='ch' refForName='{reference}' refType='w' fact='2'/><constr type='w' for='ch' forName='a' op='gte' val='1'/></constrLst></layoutNode>"
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
        let mut measure = |items: &[Placed<'_>], _: bool| {
            Ok(items.iter().map(|p| if p.name == "text" { (p.font.unwrap_or(18.0) * 3.0 - p.rect.h).max(0.0) } else { 0.0 }).collect())
        };
        evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap().iter().map(|p| (p.rect.h, p.font)).collect::<Vec<_>>()
    };
    let grow = r#"<rule type="h" for="ch" forName="text" val="50"/>"#;
    let shrink = r#"<rule type="primFontSz" for="ch" forName="text" val="5"/>"#;
    let a = run(&format!("{grow}{shrink}"));
    let b = run(&format!("{shrink}{grow}"));
    assert!((a[0].0 - 90.0).abs() < 0.02 && a[0].1 == Some(30.0));
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
    let mut measure = |items: &[Placed<'_>], _: bool| Ok(items.iter().map(|p| (150.0 - p.rect.h).max(0.0)).collect());
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert!((out[0].rect.h - 30.0 * 72.0 / 25.4).abs() < 1e-6);
    let mut bad = |_: &[Placed<'_>], _: bool| Ok(vec![f64::NAN]);
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
      <constr type="secFontSz" for="ch" refType="primFontSz" refFor="ch" fact="0.75"/></constrLst>
      <ruleLst><rule type="primFontSz" for="ch" forName="a" val="8"/></ruleLst></layoutNode>"#,
    );
    let mut measure = |p: &[Placed<'_>], _: bool| Ok(p.iter().map(|p| if p.name == "b" { (p.font.unwrap() - 18.).max(0.) } else { 0. }).collect());
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
    let mut measure = |p: &[Placed<'_>], _: bool| Ok(p.iter().map(|p| if p.name == "a" { (p.font.unwrap() - 24.).max(0.) } else { 50. }).collect());
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
        let mut measure = |p: &[Placed<'_>], _: bool| Ok(p.iter().map(|p| (40. * 72. / 25.4 - p.rect.w).max(0.)).collect());
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
    let mut measure = |p: &[Placed<'_>], _: bool| Ok(p.iter().map(|p| (p.font.unwrap() - 12.).max(0.)).collect());
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
        let mut measure = |p: &[Placed<'_>], _: bool| Ok(p.iter().map(|p| (p.font.unwrap() - 14.).max(0.)).collect());
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
            |p: &[Placed<'_>], _: bool| Ok(p.iter().map(|p| if p.name == "a" { (p.font.unwrap() - 14.).max(0.) } else { 0. }).collect());
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
            |p: &[Placed<'_>], _: bool| Ok(p.iter().map(|p| (p.font.unwrap() - if p.name == "a" { 21. } else { 14. }).max(0.)).collect());
        let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
        assert_eq!(out.iter().map(|p| p.font.unwrap()).collect::<Vec<_>>(), expected, "{op}/{factor}/{target}");
    }
}

#[test]
fn local_font_factor_rules_use_the_reference_size_and_keep_defaults_separate() {
    let data = source();
    for (initial, expected) in [(1., 12.), (0., 1.)] {
        let l = layout(&format!(
            "<layoutNode><alg type='composite'/><constrLst><constr type='primFontSz' val='36'/><constr type='primFontSz' for='ch' refType='primFontSz' fact='{initial}'/></constrLst><layoutNode name='a'><alg type='tx'/><shape type='rect'/><ruleLst><rule type='primFontSz' fact='0.25' max='12'/></ruleLst></layoutNode></layoutNode>"
        ));
        let mut measure =
            |p: &[Placed<'_>], _: bool| Ok(p.iter().map(|p| if initial == 0. { 1. } else { (p.font.unwrap() - 12.).max(0.) }).collect());
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
    let mut measure = |p: &[Placed<'_>], _: bool| Ok(p.iter().map(|p| if p.name == "c" { (p.font.unwrap() - 14.).max(0.) } else { 0. }).collect());
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
            |p: &[Placed<'_>], _: bool| Ok(p.iter().map(|p| if p.name == "a" { (p.font.unwrap() - 14.).max(0.) } else { 0. }).collect());
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
    let mut measure = |p: &[Placed<'_>], _: bool| Ok(p.iter().map(|p| (100. - p.rect.w).max(0.) + (p.font.unwrap() - 14.).max(0.)).collect());
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert!((out[0].rect.w - 20. * 72. / 25.4).abs() < 1e-6);
    assert_eq!(out[0].font, Some(36.));
}

#[test]
fn coupled_rules_cross_a_plateau_without_changing_an_unrelated_label() {
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
    let mut measure = |p: &[Placed<'_>], _: bool| {
        Ok(p.iter().map(|p| if p.name == "a" { (40. * 72. / 25.4 - p.rect.w.min(p.rect.h)).max(0.) } else { 0. }).collect())
    };
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert!((out[0].rect.w - 40. * 72. / 25.4).abs() < 0.02);
    assert!((out[0].rect.h - 40. * 72. / 25.4).abs() < 0.02);
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
    let mut measure = |p: &[Placed<'_>], _: bool| Ok(p.iter().map(|p| (90. - p.rect.w).max(0.)).collect());
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert!((out[0].rect.w - 90.).abs() < 0.02);
    assert!((out[0].rect.h - 40. * 72. / 25.4).abs() < 1e-6);
}

#[test]
fn composite_coordinates_belong_to_the_parent_and_flow_algorithms_replace_them() {
    let data = source();
    let unit = 72. / 25.4;
    for (parent, local, separation) in [("composite", true, 0.), ("composite", false, 110.), ("lin", false, 40.)] {
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
fn joint_rules_do_not_reintroduce_a_redundant_edge_equation() {
    let data = source();
    let l = layout(
        "<layoutNode><alg type='composite'/><constrLst>
        <constr type='w' for='ch' val='20'/><constr type='h' for='ch' val='20'/>
        <constr type='l' for='ch' val='10'/><constr type='r' for='ch' val='100'/></constrLst>
        <ruleLst><rule type='r' for='ch' val='200'/></ruleLst>
        <layoutNode><alg type='tx'/><shape type='rect'/><ruleLst>
        <rule type='w' val='60'/><rule type='h' val='60'/></ruleLst></layoutNode></layoutNode>",
    );
    let mut measure = |p: &[Placed<'_>], _: bool| Ok(p.iter().map(|p| (90. - p.rect.w.min(p.rect.h)).max(0.)).collect());
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert!((out[0].rect.w - 90.).abs() < 0.02);
    assert!((out[0].rect.h - 90.).abs() < 0.02);
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
    for (dir, count, w, h, expected_w, expected_h) in [
        ("fromL", 1, 80., 160., 80. * 72. / 25.4, 324.),
        ("fromR", 1, 500., 40., 768., 40. * 72. / 25.4),
        ("fromL", 3, 150., 160., 256., 324.),
        ("fromT", 3, 500., 60., 768., 108.),
        ("fromB", 3, 500., 60., 768., 108.),
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
            |p: &[Placed<'_>], _: bool| Ok(p.iter().map(|p| if p.name == "b" { (p.font.unwrap() - threshold).max(0.) } else { 0. }).collect());
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
    let mut measure = |p: &[Placed<'_>], _: bool| Ok(vec![0.; p.len()]);
    let out = evaluate_measured(&l, &data, frame(), Some(&mut measure)).unwrap();
    assert_eq!(out[0].font, Some(25.));
    assert_eq!(out[1].font, Some(19.));
    assert_eq!(out[1].secondary_font, Some(10.));
}
