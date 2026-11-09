use super::super::evaluate_measured;
use super::*;

fn layout(width: f64, height: f64, rule: &str) -> El {
    crate::xml::parse(
        format!(
            "<layoutDef><layoutNode><alg type='tx'/><shape type='rect' rot='25'/>
             <constrLst><constr type='w' val='{width}'/><constr type='h' val='{height}'/></constrLst>
             <ruleLst>{rule}</ruleLst></layoutNode></layoutDef>"
        )
        .as_bytes(),
    )
    .unwrap()
    .root
}

#[test]
fn growing_footprints_survive_a_rotated_text_rectangle_plateau() {
    let data = crate::xml::parse(b"<dataModel><ptLst><pt modelId='root' type='doc'/></ptLst><cxnLst/></dataModel>").unwrap().root;
    let mm = unit("w");
    for (width, height, property, limit) in [(21., 12., "w", 97.), (12., 65., "h", 90.)] {
        let seed = layout(width, height, "");
        let changed = layout(width, height, &format!("<rule type='{property}' val='{limit}'/>"));
        let frame = Xfrm::new(0., 0., 400., 300.);
        // One active edge fixes the maximum-area inscribed text rectangle. Growing
        // the inactive footprint dimension cannot improve this positive overflow.
        let mut measure = |items: &[Placed<'_>], _: MeasureKind| {
            Ok(items.iter().map(|p| (100. * mm - p.text_rect.unwrap().w - p.text_rect.unwrap().h).max(0.)).collect())
        };
        let before = evaluate_measured(&seed, &data, frame, Some(&mut measure)).unwrap();
        let after = evaluate_measured(&changed, &data, frame, Some(&mut measure)).unwrap();
        assert_eq!(after.len(), 1);
        assert!((before[0].rect.w - after[0].rect.w).abs() < 1e-8);
        assert!((before[0].rect.h - after[0].rect.h).abs() < 1e-8);
        let centre = after[0].rect.center() - before[0].rect.center();
        let (actual, expected) = if property == "w" { (centre.x, (limit - width) * mm / 2.) } else { (centre.y, (limit - height) * mm / 2.) };
        assert!((actual - expected).abs() < 1e-8);
    }
}

#[test]
fn resolved_or_non_growing_rules_keep_their_footprint() {
    let data = crate::xml::parse(b"<dataModel><ptLst><pt modelId='root' type='doc'/></ptLst><cxnLst/></dataModel>").unwrap().root;
    let frame = Xfrm::new(0., 0., 400., 300.);
    for (width, height, limit) in [(21., 12., 18.), (21., 12., 21.)] {
        let seed = layout(width, height, "");
        let changed = layout(width, height, &format!("<rule type='w' val='{limit}'/>"));
        let mut measure = |items: &[Placed<'_>], _: MeasureKind| Ok(items.iter().map(|p| (100. - p.text_rect.unwrap().h).max(0.)).collect());
        let before = evaluate_measured(&seed, &data, frame, Some(&mut measure)).unwrap();
        let after = evaluate_measured(&changed, &data, frame, Some(&mut measure)).unwrap();
        assert_eq!(before[0].rect, after[0].rect);
    }
    let seed = layout(21., 12., "");
    let changed = layout(21., 12., "<rule type='w' val='97'/>");
    let mut fits = |items: &[Placed<'_>], _: MeasureKind| Ok(vec![0.; items.len()]);
    let before = evaluate_measured(&seed, &data, frame, Some(&mut fits)).unwrap();
    let after = evaluate_measured(&changed, &data, frame, Some(&mut fits)).unwrap();
    assert_eq!(before[0].rect, after[0].rect);
}

#[test]
fn unresolved_dimensions_grow_even_when_rotated_text_height_worsens() {
    let data = crate::xml::parse(b"<dataModel><ptLst><pt modelId='root' type='doc'/></ptLst><cxnLst/></dataModel>").unwrap().root;
    let frame = Xfrm::new(0., 0., 400., 300.);
    let seed = layout(20., 20., "");
    let changed = layout(20., 20., "<rule type='w' val='25'/>");
    // Independent native seed/locked/rule controls contradict the former mock
    // expectation that any larger positive height deficit rejects box growth.
    let mut measure = |items: &[Placed<'_>], _: MeasureKind| Ok(items.iter().map(|p| (100. - p.text_rect.unwrap().h).max(0.)).collect());
    let before = evaluate_measured(&seed, &data, frame, Some(&mut measure)).unwrap();
    let after = evaluate_measured(&changed, &data, frame, Some(&mut measure)).unwrap();
    assert!(after[0].text_rect.unwrap().h < before[0].text_rect.unwrap().h);
    assert!((after[0].rect.center().x - before[0].rect.center().x - 2.5 * unit("w")).abs() < 1e-8);
}
