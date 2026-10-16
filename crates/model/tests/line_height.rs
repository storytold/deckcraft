//! Original model fixtures for coupled line-height inheritance and old positional records.
use deckcraft_model::text::{LineHeight, ParaProps, Spacing};
use serde_json::json;

#[test]
fn line_height_inherits_as_one_semantic_setting() {
    let settings = [
        ParaProps { line_spacing: Some(Spacing::Pts(18.0)), ..Default::default() },
        ParaProps { line_height: Some(LineHeight::Normal), ..Default::default() },
        ParaProps { line_height: Some(LineHeight::Minimum { points: 32.0 }), ..Default::default() },
    ];
    for parent in &settings {
        let mut inherited = ParaProps::default();
        inherited.inherit(parent);
        assert_eq!(inherited.line_spacing, parent.line_spacing);
        assert_eq!(inherited.line_height, parent.line_height);
        for child in &settings {
            let mut inherited = child.clone();
            inherited.inherit(parent);
            assert_eq!(inherited.line_spacing, child.line_spacing);
            assert_eq!(inherited.line_height, child.line_height);
        }
    }
}

#[test]
fn trailing_line_height_preserves_old_named_and_positional_records() {
    let old = json!([
        "right", null, 13.0, 17.0, -3.0, {"kind":"pts","v":19.0},
        {"kind":"pts","v":7.0}, {"kind":"pct","v":0.3}, null, null,
        "BIZ UDPGothic", null, 0.8, [], 42.0, true, "base", true, false, true
    ]);
    let props: ParaProps = serde_json::from_value(old).unwrap();
    assert_eq!(props.line_height, None);
    assert_eq!(props.line_spacing, Some(Spacing::Pts(19.0)));
    assert_eq!(props.space_after, Some(Spacing::Pct(0.3)));
    assert_eq!(props.hanging_punct, Some(true));
    assert_eq!(props.rtl, Some(true));
    assert_eq!(props.margin_left, Some(13.0));
    let named = serde_json::to_value(&props).unwrap();
    assert!(named.get("lineHeight").is_none());
    assert_eq!(serde_json::from_value::<ParaProps>(named).unwrap(), props);
    for policy in [LineHeight::Normal, LineHeight::Minimum { points: 0.0 }, LineHeight::Minimum { points: 29.123456789 }] {
        let props = ParaProps { line_height: Some(policy), ..Default::default() };
        assert_eq!(serde_json::from_str::<ParaProps>(&serde_json::to_string(&props).unwrap()).unwrap(), props);
    }
}

#[test]
fn minimum_domain_is_finite_bounded_and_nonnegative() {
    for points in [0.0, 1.0, 1_000_000.0] {
        assert!(LineHeight::Minimum { points }.valid());
    }
    for points in [-0.1, 1_000_000.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(!LineHeight::Minimum { points }.valid());
    }
}
