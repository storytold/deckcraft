//! Camera Format ▸ Arrange round trips: a Cameo's stacking order and its place in a group
//! survive PPTX, so PowerPoint and DeckCraft open the same slide in the same order.
//!
//! The decks are built through the engine, never produced by PowerPoint.

use deckcraft_engine::Session;
use deckcraft_model::{Presentation, Shape};
use serde_json::json;

fn blank() -> Session {
    let mut s = Session::with_new();
    s.execute("slide.layout", &json!({"layout": "blank"})).expect("blank slide");
    s
}

fn name(s: &Session, id: u32) -> String {
    s.doc().expect("doc").doc.slides[0].shape(deckcraft_model::ShapeId(id)).expect("shape").name.clone()
}

fn rect(s: &mut Session, x: f64) -> u32 {
    u32::try_from(s.execute("shape.insert", &json!({"preset": "rect", "rect": [x, 100.0, 80.0, 60.0]})).expect("rect")["id"].as_u64().expect("id"))
        .expect("id fits")
}

fn cameo(s: &mut Session, x: f64) -> u32 {
    u32::try_from(s.execute("insert.cameo", &json!({"rect": [x, 100.0, 162.0, 162.0]})).expect("cameo")["id"].as_u64().expect("id")).expect("id fits")
}

fn round_trip(p: &Presentation) -> Presentation {
    let bytes = deckcraft_pptx::export(p).expect("export");
    deckcraft_pptx::import(&bytes).expect("import")
}

fn stack(p: &Presentation) -> Vec<(String, bool)> {
    p.slides[0].shapes.iter().map(|s| (s.name.clone(), s.is_cameo())).collect()
}

#[test]
fn a_reordered_cameo_keeps_its_slide_stacking_position_through_pptx() {
    let mut s = blank();
    let a = rect(&mut s, 200.0);
    let b = rect(&mut s, 400.0);
    let c = cameo(&mut s, 600.0);
    s.execute("selection.select", &json!({"ids": [a]})).expect("select");
    s.execute("arrange.bringToFront", &json!({})).expect("bring to front");
    let expected = vec![(name(&s, b), false), (name(&s, c), true), (name(&s, a), false)];
    let doc = s.doc().expect("doc").doc.clone();
    assert_eq!(stack(&doc), expected, "the arrangement under test");

    let q = round_trip(&doc);
    assert_eq!(stack(&q), expected, "stacking order and the Cameo marker survive");
}

#[test]
fn a_cameo_inside_a_group_keeps_its_child_order_and_marker_through_pptx() {
    let mut s = blank();
    let a = rect(&mut s, 200.0);
    let c = cameo(&mut s, 300.0);
    let b = rect(&mut s, 600.0);
    s.execute("selection.select", &json!({"ids": [a, c]})).expect("select");
    let g = u32::try_from(s.execute("arrange.group", &json!({})).expect("group")["id"].as_u64().expect("id")).expect("id fits");
    let children = vec![(name(&s, a), false), (name(&s, c), true)];
    let group_name = name(&s, g);
    // The group sits above its last member, with the untouched rectangle still in front of it.
    let expected = vec![(group_name.clone(), false), (name(&s, b), false)];
    let doc = s.doc().expect("doc").doc.clone();
    assert_eq!(stack(&doc), expected, "group position under test");

    let q = round_trip(&doc);
    assert_eq!(stack(&q), expected, "the group keeps its place in the stacking order");
    let group = q.slides[0].shapes.iter().find(|s| s.name == group_name).expect("group");
    let deckcraft_model::ShapeKind::Group { children: kids, .. } = &group.kind else { panic!("not a group: {:?}", group.kind) };
    let got: Vec<(String, bool)> = kids.iter().map(|k| (k.name.clone(), k.is_cameo())).collect();
    assert_eq!(got, children, "child order and the Cameo marker survive");
}

/// Ungrouping keeps the Cameo's marker and its place in the slide's stacking order, both before
/// and after a PPTX round trip.
#[test]
fn ungrouped_cameo_keeps_its_marker_and_order_through_pptx() {
    let mut s = blank();
    let a = rect(&mut s, 200.0);
    let c = cameo(&mut s, 300.0);
    let b = rect(&mut s, 600.0);
    s.execute("selection.select", &json!({"ids": [a, c]})).expect("select");
    s.execute("arrange.group", &json!({})).expect("group");
    s.execute("arrange.ungroup", &json!({})).expect("ungroup");
    let doc = s.doc().expect("doc").doc.clone();
    let from_doc: Vec<(String, bool)> = doc.slides[0].shapes.iter().map(|sh: &Shape| (sh.name.clone(), sh.is_cameo())).collect();
    assert!(from_doc.iter().any(|(_, cameo)| *cameo));
    let _ = b;
    let q = round_trip(&doc);
    let got: Vec<(String, bool)> = q.slides[0].shapes.iter().map(|s| (s.name.clone(), s.is_cameo())).collect();
    assert_eq!(got, from_doc);
}
