//! Selection pane: reorder, groups, hide/show, lock and undo.

use serde_json::{Value, json};

use crate::Session;
use crate::tools::{Mods, PointerEvent, PointerKind};

/// A blank slide with A, B, C (back to front) and G = group [D, H = group [E, F]] in front.
fn deck() -> (Session, Vec<u32>) {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let mut ids = vec![];
    for (i, n) in ["A", "B", "C", "D", "E", "F"].iter().enumerate() {
        let x = 20.0 + 110.0 * i as f64;
        let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [x, 50, 100, 80]})).unwrap();
        let id = r["id"].as_u64().unwrap() as u32;
        s.execute("shape.rename", &json!({"id": id, "name": n})).unwrap();
        ids.push(id);
    }
    let h = s.execute("arrange.group", &json!({"ids": [ids[4], ids[5]]})).unwrap()["id"].as_u64().unwrap() as u32;
    s.execute("shape.rename", &json!({"id": h, "name": "H"})).unwrap();
    let g = s.execute("arrange.group", &json!({"ids": [ids[3], h]})).unwrap()["id"].as_u64().unwrap() as u32;
    s.execute("shape.rename", &json!({"id": g, "name": "G"})).unwrap();
    ids.extend([h, g]);
    (s, ids)
}

/// The tree as text, back to front: `A B C G[D H[E F]]`.
fn tree(s: &Session) -> String {
    fn rec(v: &[deckcraft_model::Shape]) -> String {
        v.iter().map(|x| if x.is_group() { format!("{}[{}]", x.name, rec(x.children())) } else { x.name.clone() }).collect::<Vec<_>>().join(" ")
    }
    rec(s.doc().unwrap().shapes())
}

/// (hidden, locked)
fn flags(s: &Session, id: u32) -> (bool, bool) {
    let sh = s.doc().unwrap().shape(deckcraft_model::ShapeId(id)).unwrap();
    (sh.hidden, sh.locked)
}

fn selected(s: &Session) -> Vec<u32> {
    s.doc().unwrap().selection.shapes.iter().map(|i| i.0).collect()
}

fn undo(s: &mut Session) {
    s.execute("edit.undo", &json!({})).unwrap();
}

fn bx(s: &mut Session, id: u32) -> Value {
    s.execute("shape.inspect", &json!({"id": id})).unwrap()["box"].clone()
}

#[test]
fn bring_forward_and_send_backward_stay_inside_the_group() {
    let (mut s, ids) = deck();
    let (a, d, e, h) = (ids[0], ids[3], ids[4], ids[6]);
    assert_eq!(tree(&s), "A B C G[D H[E F]]");
    s.execute("arrange.bringForward", &json!({"ids": [a]})).unwrap();
    assert_eq!(tree(&s), "B A C G[D H[E F]]");
    // A child moves among its siblings only and stops at the group's front.
    s.execute("arrange.bringForward", &json!({"ids": [d]})).unwrap();
    assert_eq!(tree(&s), "B A C G[H[E F] D]");
    s.execute("arrange.bringForward", &json!({"ids": [d]})).unwrap();
    assert_eq!(tree(&s), "B A C G[H[E F] D]");
    s.execute("arrange.sendBackward", &json!({"ids": [e]})).unwrap();
    assert_eq!(tree(&s), "B A C G[H[E F] D]", "E is already at the back of H");
    s.execute("arrange.bringToFront", &json!({"ids": [e]})).unwrap();
    assert_eq!(tree(&s), "B A C G[H[F E] D]");
    s.execute("arrange.sendToBack", &json!({"ids": [d]})).unwrap();
    assert_eq!(tree(&s), "B A C G[D H[F E]]");
    s.execute("arrange.bringForward", &json!({"ids": [h]})).unwrap();
    assert_eq!(tree(&s), "B A C G[D H[F E]]", "H is already in front inside G");
    // Steps that changed nothing left no undo step.
    undo(&mut s);
    assert_eq!(tree(&s), "B A C G[H[F E] D]");
    undo(&mut s);
    undo(&mut s);
    assert_eq!(tree(&s), "B A C G[D H[E F]]");
}

#[test]
fn reorder_is_the_drag_and_drop_among_siblings() {
    let (mut s, ids) = deck();
    let (a, c, d, f, g) = (ids[0], ids[2], ids[3], ids[5], ids[7]);
    // Drag A to the front of the slide (an index past the end clamps).
    assert_eq!(s.execute("arrange.reorder", &json!({"id": a, "index": 99})).unwrap()["index"], 3);
    assert_eq!(tree(&s), "B C G[D H[E F]] A");
    s.execute("arrange.reorder", &json!({"id": g, "index": 0})).unwrap();
    assert_eq!(tree(&s), "G[D H[E F]] B C A");
    // A child is dragged within its group: it never leaves it.
    s.execute("arrange.reorder", &json!({"id": d, "index": 5})).unwrap();
    assert_eq!(tree(&s), "G[H[E F] D] B C A");
    s.execute("arrange.reorder", &json!({"id": f, "index": 0})).unwrap();
    assert_eq!(tree(&s), "G[H[F E] D] B C A");
    // Dropping where it already is changes nothing (no undo step).
    let n = s.doc().unwrap().history.undo.len();
    s.execute("arrange.reorder", &json!({"id": c, "index": 2})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), n);
    // One undo step per drop.
    undo(&mut s);
    assert_eq!(tree(&s), "G[H[E F] D] B C A");
    undo(&mut s);
    undo(&mut s);
    undo(&mut s);
    assert_eq!(tree(&s), "A B C G[D H[E F]]");
    assert!(s.execute("arrange.reorder", &json!({"id": 4242, "index": 0})).is_err());
    assert!(s.execute("arrange.reorder", &json!({"id": a})).is_err());
}

#[test]
fn eye_show_all_and_hide_all() {
    let (mut s, ids) = deck();
    let (a, b, d, e, g) = (ids[0], ids[1], ids[3], ids[4], ids[7]);
    s.execute("shape.visible", &json!({"id": b, "visible": false})).unwrap();
    assert!(flags(&s, b).0);
    // Hiding a group hides only the group; its children keep their own flags.
    s.execute("shape.visible", &json!({"id": g})).unwrap();
    assert!(flags(&s, g).0 && !flags(&s, d).0);
    s.execute("shape.visible", &json!({"id": e, "visible": false})).unwrap();
    // Hide All hides the top level only.
    s.execute("selection.hideAll", &json!({})).unwrap();
    assert!(flags(&s, a).0 && flags(&s, g).0 && !flags(&s, d).0 && flags(&s, e).0);
    // Show All shows everything, children included.
    s.execute("selection.showAll", &json!({})).unwrap();
    for id in &ids {
        assert!(!flags(&s, *id).0, "{id} still hidden");
    }
    let n = s.doc().unwrap().history.undo.len();
    s.execute("selection.showAll", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), n, "nothing to show: no undo step");
    undo(&mut s);
    assert!(flags(&s, a).0 && flags(&s, e).0);
    undo(&mut s);
    assert!(!flags(&s, a).0 && flags(&s, g).0 && flags(&s, e).0);
}

#[test]
fn lock_cascades_through_groups_and_undoes() {
    let (mut s, ids) = deck();
    let (a, d, e, h, g) = (ids[0], ids[3], ids[4], ids[6], ids[7]);
    s.execute("shape.lock", &json!({"id": a})).unwrap();
    assert!(flags(&s, a).1);
    s.execute("shape.lock", &json!({"id": a})).unwrap();
    assert!(!flags(&s, a).1, "toggles");
    // Locking a group locks everything in it.
    s.execute("shape.lock", &json!({"id": g, "locked": true})).unwrap();
    assert!(flags(&s, g).1 && flags(&s, d).1 && flags(&s, h).1 && flags(&s, e).1);
    // A child can be unlocked on its own.
    s.execute("shape.lock", &json!({"id": d, "locked": false})).unwrap();
    assert!(flags(&s, g).1 && !flags(&s, d).1);
    // Unlocking the group unlocks everything in it.
    s.execute("shape.lock", &json!({"id": g})).unwrap();
    assert!(!flags(&s, g).1 && !flags(&s, h).1 && !flags(&s, e).1);
    undo(&mut s);
    assert!(flags(&s, g).1 && !flags(&s, d).1 && flags(&s, e).1);
    undo(&mut s);
    undo(&mut s);
    assert!(!flags(&s, g).1 && !flags(&s, e).1);
}

#[test]
fn locked_objects_refuse_moving_sizing_rotating_but_take_formatting() {
    let (mut s, ids) = deck();
    let (a, b) = (ids[0], ids[1]);
    s.execute("shape.lock", &json!({"id": a, "locked": true})).unwrap();
    let before = bx(&mut s, a);
    for (cmd, p) in [
        ("shape.move", json!({"ids": [a], "dx": 10})),
        ("shape.resize", json!({"ids": [a], "w": 300})),
        ("shape.setBounds", json!({"ids": [a], "x": 1, "y": 1, "w": 2, "h": 2})),
        ("shape.rotate", json!({"ids": [a], "deg": 45})),
        ("shape.flip", json!({"ids": [a], "axis": "vertical"})),
        ("arrange.align", json!({"ids": [a], "edge": "right"})),
        ("shape.change", json!({"ids": [a], "preset": "ellipse"})),
    ] {
        assert!(s.execute(cmd, &p).is_err(), "{cmd} changed a locked shape");
    }
    assert_eq!(bx(&mut s, a), before);
    // Mixed selections: the unlocked ones move, the locked one stays.
    let bb = bx(&mut s, b);
    s.execute("shape.move", &json!({"ids": [a, b], "dx": 10})).unwrap();
    assert_eq!(bx(&mut s, a), before);
    assert_eq!(bx(&mut s, b)["x"].as_f64().unwrap(), bb["x"].as_f64().unwrap() + 10.0);
    // Formatting and the name still change.
    s.execute("shape.fill", &json!({"ids": [a], "color": "#00ff00"})).unwrap();
    s.execute("shape.rename", &json!({"id": a, "name": "Anchor"})).unwrap();
    assert_eq!(s.doc().unwrap().shape(deckcraft_model::ShapeId(a)).unwrap().name, "Anchor");
    // The arrow keys don't nudge it.
    s.execute("edit.select", &json!({"ids": [a]})).unwrap();
    let _ = s.key("ArrowRight", Mods::default());
    assert_eq!(bx(&mut s, a), before);
}

#[test]
fn locked_objects_are_picked_on_the_canvas_but_not_dragged() {
    let (mut s, ids) = deck();
    let a = ids[0];
    s.prefs.snap_to_grid = false;
    s.prefs.smart_guides = false;
    s.execute("shape.lock", &json!({"id": a, "locked": true})).unwrap();
    s.execute("edit.select", &json!({"ids": []})).unwrap();
    let n = s.doc().unwrap().history.undo.len();
    let ev = |kind, x, y| PointerEvent { kind, x, y, mods: Mods::default(), tol: 3.0, pressure: None };
    // A: x 20..120, y 50..130.
    s.pointer(ev(PointerKind::Down, 60.0, 90.0)).unwrap();
    assert_eq!(selected(&s), vec![a], "a click selects a locked object");
    s.pointer(ev(PointerKind::Drag, 160.0, 190.0)).unwrap();
    s.pointer(ev(PointerKind::Up, 160.0, 190.0)).unwrap();
    assert_eq!(bx(&mut s, a)["x"], 20.0);
    // No resize handle: pressing on the SE corner and dragging doesn't size it.
    s.pointer(ev(PointerKind::Down, 120.0, 130.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 200.0, 200.0)).unwrap();
    s.pointer(ev(PointerKind::Up, 200.0, 200.0)).unwrap();
    assert_eq!(bx(&mut s, a)["w"], 100.0);
    assert_eq!(s.doc().unwrap().history.undo.len(), n, "no undo steps for gestures that changed nothing");
}

#[test]
fn select_from_the_pane() {
    let (mut s, ids) = deck();
    let (a, b, c, d, e, g) = (ids[0], ids[1], ids[2], ids[3], ids[4], ids[7]);
    s.execute("selection.select", &json!({"id": b})).unwrap();
    assert_eq!(selected(&s), vec![b]);
    s.execute("selection.select", &json!({"id": c, "toggle": true})).unwrap();
    assert_eq!(selected(&s), vec![b, c]);
    s.execute("selection.select", &json!({"id": b, "toggle": true})).unwrap();
    assert_eq!(selected(&s), vec![c]);
    s.execute("selection.select", &json!({"ids": [a, b], "add": true})).unwrap();
    assert_eq!(selected(&s), vec![c, a, b]);
    // A child inside a group; children across nesting levels go together.
    s.execute("selection.select", &json!({"id": d})).unwrap();
    assert_eq!(selected(&s), vec![d]);
    s.execute("selection.select", &json!({"id": e, "toggle": true})).unwrap();
    assert_eq!(selected(&s), vec![d, e]);
    // ...but not with top-level objects: Ctrl+click on one is ignored.
    s.execute("selection.select", &json!({"id": g, "toggle": true})).unwrap();
    assert_eq!(selected(&s), vec![d, e]);
    s.execute("selection.select", &json!({"id": a})).unwrap();
    s.execute("selection.select", &json!({"id": d, "toggle": true})).unwrap();
    assert_eq!(selected(&s), vec![a]);
    // Picking makes no undo step...
    let n = s.doc().unwrap().history.undo.len();
    s.execute("selection.select", &json!({"id": b})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), n);
    // ...unless it shows a hidden object again.
    s.execute("shape.visible", &json!({"id": c, "visible": false})).unwrap();
    s.execute("selection.select", &json!({"id": c})).unwrap();
    assert_eq!(selected(&s), vec![c]);
    assert!(!flags(&s, c).0, "picking a hidden object shows it");
    undo(&mut s);
    assert!(flags(&s, c).0);
    assert!(s.execute("selection.select", &json!({"id": 4242})).is_err());
}

#[test]
fn rename_from_the_pane_undoes() {
    let (mut s, ids) = deck();
    let e = ids[4];
    s.execute("shape.rename", &json!({"id": e, "name": "Eagle"})).unwrap();
    assert_eq!(s.doc().unwrap().shape(deckcraft_model::ShapeId(e)).unwrap().name, "Eagle");
    undo(&mut s);
    assert_eq!(s.doc().unwrap().shape(deckcraft_model::ShapeId(e)).unwrap().name, "E");
}
