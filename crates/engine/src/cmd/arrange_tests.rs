//! Tests for the Arrange commands that had none: the rotate/flip/nudge wrappers and `regroup`.
//! Guides live in `guides_tests.rs`; grouping, ordering, aligning and distributing are covered by
//! `tests.rs`.

use serde_json::{Value, json};

use crate::Session;

/// One rectangle, inserted and selected.
fn rect() -> (Session, u32) {
    let mut s = Session::with_new();
    let id = insert(&mut s, [100.0, 100.0, 200.0, 100.0]);
    (s, id)
}

fn insert(s: &mut Session, rect: [f64; 4]) -> u32 {
    s.execute("shape.insert", &json!({"preset": "rect", "rect": rect})).expect("insert")["id"].as_u64().expect("shape id") as u32
}

/// The shape's own transform. `Xfrm` serializes camelCase and leaves `rot` and the flips out when
/// they are zero or false, so `rot` reads back as 0.0 and a flip is simply absent.
fn xfrm(s: &mut Session, id: u32) -> Value {
    s.execute("shape.inspect", &json!({"id": id})).expect("inspect")["shape"]["xfrm"].clone()
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_default()
}

fn flipped(s: &mut Session, id: u32, axis: &str) -> bool {
    xfrm(s, id).get(axis).and_then(Value::as_bool).unwrap_or(false)
}

/// Rotation, mirroring and nudging, read back from the document rather than from a reply.
#[test]
fn rotate_flip_and_nudge_change_the_shape_transform() {
    let (mut s, id) = rect();

    s.execute("arrange.rotateRight", &json!({})).expect("rotate right");
    assert_eq!(num(&xfrm(&mut s, id)["rot"]), 90.0);
    s.execute("arrange.rotateRight", &json!({})).expect("rotate right again");
    assert_eq!(num(&xfrm(&mut s, id)["rot"]), 180.0);
    s.execute("arrange.rotateLeft", &json!({})).expect("rotate left");
    assert_eq!(num(&xfrm(&mut s, id)["rot"]), 90.0);

    // Three more quarter turns make a whole turn: the angle wraps instead of growing past 360.
    for _ in 0..3 {
        s.execute("arrange.rotateRight", &json!({})).expect("rotate");
    }
    assert_eq!(num(&xfrm(&mut s, id)["rot"]), 0.0);

    // Each mirror is its own flag, so flipping twice is a no-op and the two never interfere.
    s.execute("arrange.flipHorizontal", &json!({})).expect("flip h");
    assert!(flipped(&mut s, id, "flipH") && !flipped(&mut s, id, "flipV"));
    s.execute("arrange.flipHorizontal", &json!({})).expect("flip h back");
    assert!(!flipped(&mut s, id, "flipH"));
    s.execute("arrange.flipVertical", &json!({})).expect("flip v");
    assert!(flipped(&mut s, id, "flipV") && !flipped(&mut s, id, "flipH"));

    // A nudge is a delta, not a position: the shape moves by it and stays where it landed.
    s.execute("arrange.nudge", &json!({"dx": 10.0, "dy": -5.0})).expect("nudge");
    let x = xfrm(&mut s, id);
    assert_eq!((num(&x["x"]), num(&x["y"])), (110.0, 95.0));
    s.execute("arrange.nudge", &json!({})).expect("nudge without a delta");
    let after = xfrm(&mut s, id);
    assert_eq!((num(&after["x"]), num(&after["y"])), (110.0, 95.0), "a nudge of nothing moves nothing");
}

/// Each gesture is one undo step, so one Ctrl+Z puts the shape back as it was.
#[test]
fn rotate_and_nudge_are_one_undo_step_each() {
    let (mut s, id) = rect();
    s.execute("arrange.rotateRight", &json!({})).expect("rotate");
    assert_eq!(num(&xfrm(&mut s, id)["rot"]), 90.0);
    s.execute("edit.undo", &json!({})).expect("undo");
    assert_eq!(num(&xfrm(&mut s, id)["rot"]), 0.0, "undo takes the whole rotation back");

    s.execute("arrange.nudge", &json!({"dx": 12.0})).expect("nudge");
    assert_eq!(num(&xfrm(&mut s, id)["x"]), 112.0);
    s.execute("edit.undo", &json!({})).expect("undo");
    assert_eq!(num(&xfrm(&mut s, id)["x"]), 100.0);
}

/// `arrange.regroup` puts back what the last ungroup took apart, and says so when there is nothing.
#[test]
fn regroup_regroups_the_last_ungroup() {
    let mut s = Session::with_new();
    let a = insert(&mut s, [10.0, 10.0, 50.0, 50.0]);
    let b = insert(&mut s, [80.0, 80.0, 50.0, 50.0]);

    assert!(s.execute("arrange.regroup", &json!({})).is_err(), "nothing was ungrouped yet");

    s.execute("selection.select", &json!({"ids": [a, b]})).expect("select both");
    s.execute("arrange.group", &json!({})).expect("group");
    let grouped = plain(&s);
    assert_eq!(grouped.len(), 1, "the two shapes are one now");
    assert_eq!(children(&s, grouped[0]), vec![a, b], "a group holding both of them");

    s.execute("arrange.ungroup", &json!({})).expect("ungroup");
    assert_eq!(plain(&s), vec![a, b], "ungroup leaves the two shapes in their order");
    assert_eq!(selected(&s), vec![a, b], "ungroup leaves the children selected");

    s.execute("arrange.regroup", &json!({})).expect("regroup");
    assert_eq!(plain(&s).len(), 1, "regroup puts them back in one shape");
    let group = plain(&s)[0];
    assert_eq!(children(&s, group), vec![a, b], "holding the same two shapes");
    assert_eq!(selected(&s), vec![group], "and the new group is selected");
}

/// The slide's own shapes — the placeholders a layout brings in are not part of this test.
fn plain(s: &Session) -> Vec<u32> {
    let st = s.doc().expect("doc");
    st.doc.slides[st.selection.slide].shapes.iter().filter(|x| x.ph.is_none()).map(|x| x.id.0).collect()
}

fn selected(s: &Session) -> Vec<u32> {
    s.doc().expect("doc").selection.shapes.iter().map(|x| x.0).collect()
}

fn children(s: &Session, group: u32) -> Vec<u32> {
    let st = s.doc().expect("doc");
    let sh = st.shape(deckcraft_model::ShapeId(group)).expect("group shape");
    match &sh.kind {
        deckcraft_model::ShapeKind::Group { children, .. } => children.iter().map(|c| c.id.0).collect(),
        _ => Vec::new(),
    }
}
