//! Drawing guides: commands, dragging a guide with the pointer, snapping shapes to guides, and
//! the PPTX round trip.

use serde_json::json;

use crate::Session;
use crate::tools::{Mods, PointerEvent, PointerKind};

fn ev(s: &mut Session, kind: PointerKind, x: f64, y: f64) {
    s.pointer(PointerEvent { kind, x, y, mods: Mods::default(), tol: 4.0, pressure: None }).unwrap();
}

#[test]
fn add_move_delete_and_undo() {
    let mut s = Session::with_new();
    assert_eq!(s.execute("guide.add", &json!({"defaults": true})).unwrap()["added"], 2);
    assert_eq!(s.execute("guide.add", &json!({"defaults": true})).unwrap()["added"], 0, "only when there are none");
    let i = s.execute("guide.add", &json!({"vertical": false, "pos": 100})).unwrap()["index"].as_u64().unwrap();
    assert_eq!(i, 2);
    s.execute("guide.move", &json!({"index": 2, "pos": 5000})).unwrap();
    assert_eq!(s.doc().unwrap().doc.guides[2].pos, 540.0, "kept on the slide");
    assert!(s.execute("guide.move", &json!({"index": 9, "pos": 1})).is_err());
    assert!(s.execute("guide.move", &json!({"index": 0, "pos": null})).is_err());
    s.execute("guide.delete", &json!({"index": 0})).unwrap();
    assert_eq!(s.execute("guide.list", &json!({})).unwrap().as_array().unwrap().len(), 2);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.guides.len(), 3);
}

#[test]
fn drag_a_guide_drop_it_off_the_slide_and_snap_shapes_to_it() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("guide.add", &json!({"vertical": true, "pos": 300})).unwrap();
    // Hidden guides can't be grabbed: the press starts a marquee instead.
    ev(&mut s, PointerKind::Down, 300.0, 50.0);
    ev(&mut s, PointerKind::Up, 300.0, 50.0);
    assert_eq!(s.doc().unwrap().doc.guides[0].pos, 300.0);
    s.tool.guides_visible = true;
    ev(&mut s, PointerKind::Down, 301.0, 50.0);
    ev(&mut s, PointerKind::Drag, 350.4, 60.0);
    assert_eq!(s.tool.guide_drag, Some((0, 350.0)), "whole points while dragging");
    ev(&mut s, PointerKind::Up, 350.4, 60.0);
    assert_eq!(s.doc().unwrap().doc.guides[0].pos, 350.0);
    assert_eq!(s.tool.guide_drag, None);

    // A shape dragged near the guide snaps its left edge onto it.
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 200, 80, 60]})).unwrap()["id"].as_u64().unwrap();
    s.prefs.snap_to_grid = false;
    ev(&mut s, PointerKind::Down, 140.0, 230.0);
    ev(&mut s, PointerKind::Drag, 200.0, 230.0);
    ev(&mut s, PointerKind::Drag, 388.0, 231.0);
    ev(&mut s, PointerKind::Up, 388.0, 231.0);
    let x = s.doc().unwrap().shapes().iter().find(|sh| sh.id.0 as u64 == id).and_then(|sh| sh.xfrm).unwrap();
    assert!((x.x - 350.0).abs() < 0.01, "{x:?}");

    // Dropped off the slide: gone, and undo brings it back.
    ev(&mut s, PointerKind::Down, 350.0, 10.0);
    ev(&mut s, PointerKind::Drag, -40.0, 10.0);
    ev(&mut s, PointerKind::Up, -40.0, 10.0);
    assert!(s.doc().unwrap().doc.guides.is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.guides.len(), 1);
}

#[test]
fn guides_round_trip_through_pptx() {
    let mut s = Session::with_new();
    s.execute("guide.add", &json!({"vertical": true, "pos": 123.5})).unwrap();
    s.execute("guide.add", &json!({"vertical": false, "pos": 400})).unwrap();
    let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    assert_eq!(back.guides, s.doc().unwrap().doc.guides);
    let none = deckcraft_pptx::import(&deckcraft_pptx::export(&deckcraft_model::Presentation::default()).unwrap()).unwrap();
    assert!(none.guides.is_empty());
}
