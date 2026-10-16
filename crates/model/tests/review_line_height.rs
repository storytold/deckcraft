//! Independent fixtures: every editable owner and the complete legacy positional record.
use std::sync::Arc;

use deckcraft_model::notes::{NotesMaster, NotesPage};
use deckcraft_model::text::{Bullet, LevelStyle, LineHeight, ParaProps, Spacing, TextBody};
use deckcraft_model::{Layout, Master, Presentation, Shape, ShapeKind, Table};
use serde_json::json;

fn body(props: &ParaProps, list: bool) -> TextBody {
    let mut body = TextBody::from_text("Original independent fixture");
    if list {
        // An unused level must still be validated, including on a body without paragraphs.
        body.paragraphs.clear();
        body.list_style.set(8, LevelStyle { para: props.clone(), ..Default::default() });
    } else {
        body.paragraphs[0].props = props.clone();
    }
    body
}

fn owner_fixture(owner: usize, props: &ParaProps) -> Presentation {
    let mut p = Presentation::default();
    let shape = Shape { text: Some(body(props, owner.is_multiple_of(2))), ..Default::default() };
    let mut master = Master::default();
    match owner {
        0 => p.default_text_style.set(8, LevelStyle { para: props.clone(), ..Default::default() }),
        1 | 2 => Arc::make_mut(&mut p.slides[0]).shapes = vec![shape],
        3 | 4 => Arc::make_mut(&mut p.slides[0]).notes = body(props, owner == 4),
        5 | 6 => Arc::make_mut(&mut p.slides[0]).notes_page = Some(NotesPage { shapes: vec![shape], ..Default::default() }),
        7 => {
            let mut table = Table::new(1, 1, 100.0, 40.0);
            table.rows[0].cells[0].text = body(props, false);
            Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape { kind: ShapeKind::Table(table), ..Default::default() }];
        }
        8 => {
            Arc::make_mut(&mut p.slides[0]).shapes =
                vec![Shape { kind: ShapeKind::Group { children: vec![shape], child: Default::default() }, ..Default::default() }]
        }
        9..=11 => {
            let style = match owner {
                9 => &mut master.title_style,
                10 => &mut master.body_style,
                _ => &mut master.other_style,
            };
            style.set(8, LevelStyle { para: props.clone(), ..Default::default() });
            p.masters.push(Arc::new(master));
        }
        12 => {
            master.shapes = vec![shape];
            p.masters.push(Arc::new(master));
        }
        13 => {
            master.layouts.push(Layout { shapes: vec![shape], ..Default::default() });
            p.masters.push(Arc::new(master));
        }
        14 => {
            master.shapes = vec![shape];
            p.notes_master = Some(Arc::new(master));
        }
        15 => {
            master.shapes = vec![shape];
            p.handout_master = Some(Arc::new(master));
        }
        16 => {
            master.other_style.set(8, LevelStyle { para: props.clone(), ..Default::default() });
            p.notes_masters.push(NotesMaster { master: Arc::new(master), ..Default::default() });
        }
        17 => {
            master.layouts.push(Layout { shapes: vec![shape], ..Default::default() });
            p.notes_masters.push(NotesMaster { master: Arc::new(master), ..Default::default() });
        }
        _ => unreachable!("fixture owner"),
    }
    p
}

#[test]
fn all_owner_validation_rejects_invalid_and_dual_policies_without_repair() {
    let mut invalid = vec![ParaProps { line_height: Some(LineHeight::Normal), line_spacing: Some(Spacing::Pct(1.0)), ..Default::default() }];
    for points in [-0.01, 1_000_000.01, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        invalid.push(ParaProps { line_height: Some(LineHeight::Minimum { points }), ..Default::default() });
    }
    for owner in 0..18 {
        for props in &invalid {
            assert!(deckcraft_model::line_height::validate(&owner_fixture(owner, props)).is_err(), "owner {owner}: {props:?}");
        }
        for policy in [LineHeight::Normal, LineHeight::Minimum { points: 0.0 }, LineHeight::Minimum { points: 1_000_000.0 }] {
            let props = ParaProps { line_height: Some(policy), ..Default::default() };
            assert!(deckcraft_model::line_height::validate(&owner_fixture(owner, &props)).is_ok(), "owner {owner}: {policy:?}");
        }
    }
}

#[test]
fn legacy_full_sequence_and_new_trailing_policy_preserve_every_old_field() {
    let old = json!([
        "center", {"kind":"none"}, 11.0, 12.0, -13.0,
        {"kind":"pct","v":1.25}, {"kind":"pts","v":14.0}, {"kind":"pct","v":0.75},
        {"kind":"autoNum","scheme":"romanLcPeriod","start_at":7}, 9,
        "Independent fixture font", null, 0.625, [{"pos":23.0,"align":"dec"}], 41.0,
        false, "base", true, false, true
    ]);
    let props: ParaProps = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(props.line_spacing, Some(Spacing::Pct(1.25)));
    assert_eq!(props.space_before, Some(Spacing::Pts(14.0)));
    assert_eq!(props.space_after, Some(Spacing::Pct(0.75)));
    assert_eq!(props.bullet, Some(Bullet::AutoNum { scheme: "romanLcPeriod".into(), start_at: 7 }));
    assert_eq!(props.number_restart, Some(9));
    assert_eq!(props.bullet_size, Some(0.625));
    assert_eq!(props.tabs.as_ref().unwrap()[0].pos, 23.0);
    assert_eq!(props.rtl, Some(false));
    assert_eq!(props.hanging_punct, Some(true));
    assert_eq!(props.line_height, None);
    let mut sequence = old.as_array().unwrap().clone();
    sequence.push(json!({"kind":"minimum","points":0.0}));
    let appended: ParaProps = serde_json::from_value(json!(sequence)).unwrap();
    let mut expected = props.clone();
    expected.line_height = Some(LineHeight::Minimum { points: 0.0 });
    assert_eq!(appended, expected);
    // Deserialization preserves malformed dual records for the validator; inheritance must not hide them.
    expected.inherit(&ParaProps { line_height: Some(LineHeight::Normal), margin_left: Some(999.0), ..Default::default() });
    assert_eq!(expected.line_height, appended.line_height);
    assert_eq!(expected.line_spacing, appended.line_spacing);
    assert_eq!(expected.margin_left, Some(11.0));
}

#[test]
fn zero_minimum_and_normal_block_a_three_generation_legacy_ancestor() {
    for policy in [LineHeight::Normal, LineHeight::Minimum { points: 0.0 }] {
        let ancestor = ParaProps { line_spacing: Some(Spacing::Pts(72.0)), space_before: Some(Spacing::Pts(17.0)), ..Default::default() };
        let mut middle = ParaProps { line_height: Some(policy), ..Default::default() };
        middle.inherit(&ancestor);
        let mut child = ParaProps::default();
        child.inherit(&middle);
        child.inherit(&ancestor);
        assert_eq!(child.line_height, Some(policy));
        assert_eq!(child.line_spacing, None);
        assert_eq!(child.space_before, ancestor.space_before);
    }
}

#[test]
fn strict_policy_decode_rejects_missing_null_malformed_and_unknown_values() {
    for policy in [
        json!({"kind":"minimum"}),
        json!({"kind":"minimum","points":null}),
        json!({"kind":"minimum","points":"0"}),
        json!({"kind":"minimum","points":false}),
        json!({"kind":"minimum","points":-0.1}),
        json!({"kind":"minimum","points":1_000_000.1}),
        json!({"kind":"minimum","points":0,"additional":3}),
        json!({"kind":"normal","points":0}),
        json!({"kind":"normal","unexpected":null}),
        json!({"kind":"additional","points":2}),
    ] {
        assert!(serde_json::from_value::<LineHeight>(policy.clone()).is_err(), "{policy}");
        assert!(serde_json::from_value::<ParaProps>(json!({"lineHeight":policy})).is_err());
        let mut sequence = vec![serde_json::Value::Null; 20];
        sequence.push(policy);
        assert!(serde_json::from_value::<ParaProps>(json!(sequence)).is_err());
    }
}
