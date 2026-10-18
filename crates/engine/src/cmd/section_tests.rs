//! Tests for the section commands that had no test: renaming, removing (all three ways its slides
//! can be handed on), removing every section and moving one. `section.add` is covered in
//! `slide.rs`'s own tests.

use deckcraft_model::{Section, SlideId};
use serde_json::json;

use crate::Session;

fn deck(n: usize) -> Session {
    let mut s = Session::with_new();
    while s.doc().expect("a document").doc.slides.len() < n {
        s.execute("slide.new", &json!({})).expect("a slide");
    }
    s
}

fn sections(s: &Session) -> Vec<Section> {
    s.doc().expect("a document").doc.sections.clone()
}

fn names(s: &Session) -> Vec<String> {
    sections(s).iter().map(|x| x.name.clone()).collect()
}

fn slide_ids(s: &Session) -> Vec<SlideId> {
    s.doc().expect("a document").doc.slides.iter().map(|x| x.id).collect()
}

/// Three sections over four slides: A holds slide 0, B holds 1 and 2, C holds 3.
fn three_sections(s: &mut Session) -> Vec<SlideId> {
    s.execute("section.add", &json!({"at": 0, "name": "A"})).expect("A");
    s.execute("section.add", &json!({"at": 1, "name": "B"})).expect("B");
    s.execute("section.add", &json!({"at": 3, "name": "C"})).expect("C");
    let ids = slide_ids(s);
    assert_eq!(names(s).as_slice(), ["A", "B", "C"]);
    assert_eq!(sections(s)[1].slides.as_slice(), &ids[1..3], "B holds the two slides in the middle");
    ids
}

#[test]
fn renaming_a_section_changes_only_its_name() {
    let mut s = deck(3);
    s.execute("section.add", &json!({"at": 0, "name": "Intro"})).expect("a section");
    let ids = slide_ids(&s);

    s.execute("section.rename", &json!({"index": 0, "name": "Opening"})).expect("rename it");
    assert_eq!(names(&s).as_slice(), ["Opening"]);
    assert_eq!(slide_ids(&s), ids, "the slides did not move");

    // Renaming without a name empties it, which is how the UI clears a name it can no longer show.
    s.execute("section.rename", &json!({"index": 0})).expect("rename it to nothing");
    assert_eq!(names(&s).as_slice(), [""]);

    let err = s.execute("section.rename", &json!({"index": 4, "name": "x"})).expect_err("no such section");
    assert!(err.to_string().contains("no such section"), "{err}");
    let err = s.execute("section.rename", &json!({"name": "x"})).expect_err("no index");
    assert!(err.to_string().contains("missing `index`"), "{err}");
    assert_eq!(names(&s).as_slice(), [""], "and nothing was renamed on the way to those errors");
}

/// Removing a section without its slides leaves them to the neighbour — the one before it, or the
/// one after it when it is the first. With `slides`, they go with it.
#[test]
fn removing_a_section_hands_its_slides_to_the_neighbour() {
    let mut s = deck(4);
    let ids = three_sections(&mut s);

    s.execute("section.remove", &json!({"index": 1})).expect("remove B");
    assert_eq!(names(&s).as_slice(), ["A", "C"]);
    assert_eq!(sections(&s)[0].slides.as_slice(), &ids[..3], "B's slides went back to A");

    // The first section has no section before it, so its slides join the one after.
    s.execute("section.remove", &json!({"index": 0})).expect("remove A");
    assert_eq!(names(&s).as_slice(), ["C"]);
    assert_eq!(sections(&s)[0].slides.as_slice(), &ids[..4], "and everything belongs to C now");

    // The destructive half of the dialog: the slides go too.
    s.execute("section.remove", &json!({"index": 0, "slides": true})).expect("remove C and its slides");
    assert!(sections(&s).is_empty());
    assert!(slide_ids(&s).is_empty(), "the slides went with their section");
    // With no slide left there is nothing for the command to act on, and the ribbon greys it out.
    let err = s.execute("section.remove", &json!({"index": 0})).expect_err("no slides left");
    assert!(err.to_string().contains("no slides"), "{err}");

    s.execute("edit.undo", &json!({})).expect("undo");
    assert_eq!(slide_ids(&s), ids, "the slides come back with the section");
    assert_eq!(names(&s).as_slice(), ["C"]);

    s.execute("section.removeAll", &json!({})).expect("no sections left");
    let err = s.execute("section.remove", &json!({"index": 0})).expect_err("there is no section left");
    assert!(err.to_string().contains("no such section"), "{err}");
}

#[test]
fn removing_every_section_keeps_every_slide() {
    let mut s = deck(4);
    let ids = three_sections(&mut s);

    s.execute("section.removeAll", &json!({})).expect("remove them all");
    assert!(sections(&s).is_empty(), "the deck is one list of slides again");
    assert_eq!(slide_ids(&s), ids, "and every slide is still there");

    s.execute("edit.undo", &json!({})).expect("undo");
    assert_eq!(names(&s).as_slice(), ["A", "B", "C"], "one step puts them all back");
}

/// A section owns its slides, so moving it moves them: the slide list follows the section order,
/// which is what the thumbnails show.
#[test]
fn moving_a_section_carries_its_slides_with_it() {
    let mut s = deck(4);
    let ids = three_sections(&mut s);

    s.execute("section.move", &json!({"index": 0, "to": 2})).expect("A to the end");
    assert_eq!(names(&s).as_slice(), ["B", "C", "A"]);
    assert_eq!(slide_ids(&s).as_slice(), [ids[1], ids[2], ids[3], ids[0]], "the slides follow their section");

    // A destination past the end lands at the end.
    s.execute("section.move", &json!({"index": 0, "to": 99})).expect("B to the end");
    assert_eq!(names(&s).as_slice(), ["C", "A", "B"]);
    assert_eq!(slide_ids(&s).as_slice(), [ids[3], ids[0], ids[1], ids[2]]);

    let order = slide_ids(&s);
    s.execute("section.move", &json!({"index": 1, "to": 1})).expect("A nowhere");
    assert_eq!(slide_ids(&s), order, "moving a section to where it already is moves nothing");

    let err = s.execute("section.move", &json!({"index": 7, "to": 0})).expect_err("no such section");
    assert!(err.to_string().contains("no such section"), "{err}");
    let err = s.execute("section.move", &json!({"index": 0})).expect_err("no destination");
    assert!(err.to_string().contains("missing `to`"), "{err}");

    // Every move was one step back, the one that changed nothing included; an error is not a step.
    s.execute("edit.undo", &json!({})).expect("undo the no-op move");
    s.execute("edit.undo", &json!({})).expect("undo the move to the end");
    assert_eq!(names(&s).as_slice(), ["B", "C", "A"]);
    assert_eq!(slide_ids(&s).as_slice(), [ids[1], ids[2], ids[3], ids[0]]);
}
