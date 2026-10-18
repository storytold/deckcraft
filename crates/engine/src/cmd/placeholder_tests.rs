//! Placeholders across the deck: finding the moved ones, putting them back on their layout's
//! position, and making one slide's position every slide's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::json;

use crate::Session;
use crate::cmd::xfrm_of;

/// A title slide and three Title and Content slides.
fn deck() -> Session {
    let mut s = Session::with_new();
    for _ in 0..3 {
        s.execute("slide.new", &json!({})).unwrap();
    }
    s
}

/// The title placeholder's id on slide `i` (a title or a title slide's title).
fn title(s: &Session, i: usize) -> u32 {
    s.doc().unwrap().doc.slides[i].shapes.iter().find(|sh| sh.ph_type().is_some_and(|t| t.is_title())).unwrap().id.0
}

/// Where slide `i`'s title is drawn, and whether it has a position of its own.
fn title_at(s: &mut Session, i: usize) -> (f64, f64, bool) {
    let id = title(s, i);
    s.execute("slide.go", &json!({"index": i})).unwrap();
    let st = s.doc().unwrap();
    let sh = st.shape(deckcraft_model::ShapeId(id)).unwrap();
    let x = xfrm_of(&st.doc, &st.selection, sh);
    (x.x, x.y, sh.xfrm.is_some())
}

fn move_title(s: &mut Session, i: usize, x: f64) {
    let id = title(s, i);
    s.execute("slide.go", &json!({"index": i})).unwrap();
    s.execute("selection.select", &json!({"ids": [id]})).unwrap();
    s.execute("shape.move", &json!({"x": x})).unwrap();
}

#[test]
fn moved_lists_the_placeholders_off_their_layout() {
    let mut s = deck();
    assert_eq!(s.execute("placeholder.moved", &json!({})).unwrap(), json!([]), "a new deck follows its layouts");
    move_title(&mut s, 2, 40.0);
    let v = s.execute("placeholder.moved", &json!({})).unwrap();
    assert_eq!(v, json!([{"slide": 2, "id": title(&s, 2), "kind": "title"}]));
    assert_eq!(s.execute("placeholder.moved", &json!({"kind": "ftr"})).unwrap(), json!([]), "filtered by kind");
}

#[test]
fn reset_puts_every_title_back_on_its_layout() {
    let mut s = deck();
    let home = title_at(&mut s, 1);
    move_title(&mut s, 1, 40.0);
    move_title(&mut s, 3, 70.0);
    move_title(&mut s, 0, 10.0);
    let r = s.execute("placeholder.reset", &json!({"kind": "title"})).unwrap();
    assert_eq!(r["reset"], 3, "a title slide's title counts as a title");
    for i in 0..4 {
        assert!(!title_at(&mut s, i).2, "slide {i}'s title follows its layout");
    }
    assert_eq!(title_at(&mut s, 1), home);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(title_at(&mut s, 3).0, 70.0, "one undo step brings them all back");
    assert!(s.execute("placeholder.reset", &json!({"kind": "nope"})).is_err());
}

#[test]
fn reset_can_keep_to_some_slides_and_takes_the_selected_kind() {
    let mut s = deck();
    move_title(&mut s, 1, 40.0);
    move_title(&mut s, 2, 40.0);
    // The moved title on slide 2 is selected.
    let r = s.execute("placeholder.reset", &json!({"slides": [1]})).unwrap();
    assert_eq!(r["reset"], 1);
    assert!(!title_at(&mut s, 1).2);
    assert!(title_at(&mut s, 2).2, "slide 3 wasn't asked for");
}

#[test]
fn to_layout_makes_one_slides_position_every_slides() {
    let mut s = deck();
    move_title(&mut s, 3, 70.0);
    move_title(&mut s, 2, 40.0);
    let r = s.execute("placeholder.toLayout", &json!({})).unwrap();
    assert_eq!(r["reset"], 2, "slides 3 and 4 had their own");
    for i in 1..4 {
        let (x, _, own) = title_at(&mut s, i);
        assert_eq!(x, 40.0, "slide {i}'s title at the new place");
        assert!(!own, "following the layout");
    }
    assert_ne!(title_at(&mut s, 0).0, 40.0, "the title slide has another layout");
    // New slides with that layout get it too.
    s.execute("slide.new", &json!({})).unwrap();
    assert_eq!(title_at(&mut s, 4).0, 40.0);
}

/// A text box with `text` on slide `i`.
fn text_box(s: &mut Session, i: usize, text: &str) -> u32 {
    s.execute("slide.go", &json!({"index": i})).unwrap();
    let v = s.execute("insert.textBox", &json!({"rect": [30.0, 20.0, 400.0, 50.0], "text": text})).unwrap();
    v["id"].as_u64().or_else(|| v["ids"][0].as_u64()).unwrap() as u32
}

fn shape(s: &Session, i: usize, id: u32) -> Option<deckcraft_model::Shape> {
    s.doc().unwrap().doc.slides[i].shapes.iter().find(|sh| sh.id.0 == id).cloned()
}

#[test]
fn assign_makes_a_text_box_the_slides_title() {
    let mut s = deck();
    let old = title(&s, 1);
    let id = text_box(&mut s, 1, "Imported title");
    let r = s.execute("placeholder.assign", &json!({"id": id, "kind": "title"})).unwrap();
    assert_eq!(r["removed"], json!(old), "the empty title is deleted, not kept as a box");
    let sh = shape(&s, 1, id).unwrap();
    assert!(sh.ph_type().is_some_and(|t| t.is_title()));
    assert!(!sh.text_box);
    assert_eq!(sh.xfrm.unwrap().x, 30.0, "it stays where it was");
    assert_eq!(sh.text.as_ref().unwrap().body.autofit, Some(deckcraft_model::text::AutoFit::Shape), "it still grows with its text");
    assert!(shape(&s, 1, old).is_none(), "the empty layout title is gone");
    assert_eq!(title(&s, 1), id);
    // It now counts with the deck's titles: resetting puts it on the layout's place.
    s.execute("placeholder.reset", &json!({"kind": "title", "slides": [1]})).unwrap();
    assert!(!title_at(&mut s, 1).2);
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(shape(&s, 1, id).unwrap().ph.is_none(), "one undo step");
    assert!(shape(&s, 1, old).is_some());
}

#[test]
fn assign_untags_a_title_that_has_text() {
    let mut s = deck();
    let old = title(&s, 1);
    s.execute("slide.go", &json!({"index": 1})).unwrap();
    s.execute("text.set", &json!({"id": old, "text": "Keep me"})).unwrap();
    let before = title_at(&mut s, 1);
    let id = text_box(&mut s, 1, "New title");
    let r = s.execute("placeholder.assign", &json!({"id": id, "kind": "title"})).unwrap();
    assert_eq!(r["untagged"], json!(old));
    let o = shape(&s, 1, old).unwrap();
    assert!(o.ph.is_none());
    let x = o.xfrm.unwrap();
    assert_eq!((x.x, x.y), (before.0, before.1), "the old title keeps its place as a plain box");
}

#[test]
fn none_makes_a_placeholder_a_plain_shape_that_looks_the_same() {
    let mut s = deck();
    let id = title(&s, 1);
    let at = title_at(&mut s, 1);
    s.execute("placeholder.assign", &json!({"id": id, "kind": "none"})).unwrap();
    let sh = shape(&s, 1, id).unwrap();
    assert!(sh.ph.is_none());
    let x = sh.xfrm.unwrap();
    assert_eq!((x.x, x.y), (at.0, at.1));
    let body = &sh.text.unwrap().body;
    assert!(body.anchor.is_some() && body.inset_l.is_some(), "text box settings written down");
}

#[test]
fn assign_refuses_kinds_the_layout_lacks() {
    let mut s = deck();
    let id = text_box(&mut s, 1, "x");
    assert!(s.execute("placeholder.assign", &json!({"id": id, "kind": "nope"})).is_err());
    assert!(s.execute("placeholder.assign", &json!({"id": id})).is_err());
    // Title and Content has no subtitle.
    assert!(s.execute("placeholder.assign", &json!({"id": id, "kind": "subTitle"})).is_err());
    assert!(s.execute("placeholder.assign", &json!({"id": id, "kind": "body"})).is_ok(), "the content placeholder takes body");
}

/// A text box on the current master or layout.
fn template_box(s: &mut Session, text: &str) -> u32 {
    let v = s.execute("insert.textBox", &json!({"rect": [40.0, 500.0, 300.0, 30.0], "text": text})).unwrap();
    v["id"].as_u64().unwrap() as u32
}

fn template_shape(s: &Session, id: u32) -> Option<deckcraft_model::Shape> {
    s.doc().unwrap().shapes().iter().find(|sh| sh.id.0 == id).cloned()
}

#[test]
fn a_layout_box_becomes_a_new_placeholder() {
    let mut s = deck();
    s.execute("view.slideMaster", &json!({"master": 0, "layout": 1})).unwrap();
    let before = s.doc().unwrap().shapes().iter().filter(|x| x.ph.is_some()).count();
    let id = template_box(&mut s, "Notes for the presenter");
    s.execute("placeholder.assign", &json!({"id": id, "kind": "body"})).unwrap();
    let sh = template_shape(&s, id).unwrap();
    assert_eq!(sh.ph_type(), Some(deckcraft_model::PhType::Body));
    let idx = sh.ph.unwrap().idx;
    let shapes = s.doc().unwrap().shapes().to_vec();
    assert_eq!(shapes.iter().filter(|x| x.ph.is_some()).count(), before + 1, "a second body placeholder, nothing given up");
    assert_eq!(shapes.iter().filter(|x| x.ph.as_ref().is_some_and(|p| p.idx == idx)).count(), 1, "its own index");
    // A footer replaces the layout's footer.
    let f = template_box(&mut s, "Company confidential");
    let r = s.execute("placeholder.assign", &json!({"id": f, "kind": "ftr"})).unwrap();
    assert!(r.get("removed").is_some() || r.get("untagged").is_some(), "{r}");
    assert_eq!(s.doc().unwrap().shapes().iter().filter(|x| x.ph_type() == Some(deckcraft_model::PhType::Footer)).count(), 1);
    // Headers belong to notes and handouts.
    let h = template_box(&mut s, "Header");
    assert!(s.execute("placeholder.assign", &json!({"id": h, "kind": "hdr"})).is_err());
}

#[test]
fn the_master_takes_its_five_kinds_only() {
    let mut s = deck();
    s.execute("view.slideMaster", &json!({"master": 0, "layout": null})).unwrap();
    assert_eq!(s.doc().unwrap().selection.target, crate::Target::Master { master: 0 }, "layout: null opens the master itself");
    let id = template_box(&mut s, "Deck title");
    assert!(s.execute("placeholder.assign", &json!({"id": id, "kind": "pic"})).is_err());
    let r = s.execute("placeholder.assign", &json!({"id": id, "kind": "title"})).unwrap();
    assert!(r.get("untagged").is_some() || r.get("removed").is_some(), "the old master title gives up its place");
    let titles = s.doc().unwrap().shapes().iter().filter(|x| x.ph_type().is_some_and(|t| t.is_title())).count();
    assert_eq!(titles, 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(template_shape(&s, id).unwrap().ph.is_none(), "one undo step");
}

#[test]
fn reset_by_id_puts_back_just_that_box() {
    let mut s = deck();
    move_title(&mut s, 2, 40.0);
    move_title(&mut s, 1, 40.0);
    let id = title(&s, 1);
    let r = s.execute("placeholder.reset", &json!({"ids": [id]})).unwrap();
    assert_eq!(r["reset"], 1);
    assert!(!title_at(&mut s, 1).2);
    assert!(title_at(&mut s, 2).2, "the other slide's title stays where it was moved");
}
