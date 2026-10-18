//! Tests for the animation commands that had no test: the gallery query, taking one effect out of
//! the sequence, reordering it and its timing. `animation.set`, `animation.add`,
//! `animation.options`, `animation.clear`, `animation.get` and the painter are covered in
//! `animation.rs`'s own tests.

use deckcraft_model::anim::{ANIMATIONS, AnimStart, Animation};
use serde_json::json;

use crate::Session;

fn shape(s: &mut Session, preset: &str, x: f64) -> u32 {
    s.execute("shape.insert", &json!({"preset": preset, "rect": [x, 10.0, 100.0, 100.0]})).expect("insert a shape")["id"]
        .as_u64()
        .expect("a shape id") as u32
}

/// The current slide's sequence, in order.
fn sequence(s: &Session) -> Vec<Animation> {
    let st = s.doc().expect("a document");
    st.doc.slides.get(st.selection.slide).expect("the current slide").animations.clone()
}

fn effects(s: &Session) -> Vec<String> {
    sequence(s).iter().map(|a| a.effect.clone()).collect()
}

/// A slide with three effects on one shape: an entrance, an emphasis and another entrance.
fn three_effects(s: &mut Session) -> u32 {
    let id = shape(s, "rect", 10.0);
    s.execute("animation.add", &json!({"effect": "fade", "ids": [id]})).expect("fade");
    s.execute("animation.add", &json!({"effect": "spin", "class": "emphasis", "ids": [id]})).expect("spin");
    s.execute("animation.add", &json!({"effect": "fly", "ids": [id]})).expect("fly");
    assert_eq!(effects(s), ["fade", "spin", "fly"]);
    id
}

/// The gallery is a query: it reports the table the rest of the app uses, and it touches nothing.
#[test]
fn the_gallery_lists_every_animation() {
    let mut s = Session::with_new();
    let before = s.doc().expect("a document").history.undo.len();

    let v = s.execute("animation.list", &json!({})).expect("the gallery");
    let all = v.as_array().expect("an array");
    assert_eq!(all.len(), ANIMATIONS.len());

    let fade = all.iter().find(|a| a["id"] == "fade").expect("fade is in the gallery");
    assert_eq!(fade["label"], "Fade");
    assert_eq!(fade["class"], "entr", "the class is the token the file format uses");
    assert_eq!(fade["options"], json!([]), "fade has nothing to configure");
    let fly = all.iter().find(|a| a["id"] == "fly").expect("fly is in the gallery");
    assert_eq!(fly["options"], json!(["b", "lb", "l", "lt", "t", "rt", "r", "rb"]));
    let pulse = all.iter().find(|a| a["id"] == "pulse").expect("pulse is in the gallery");
    assert_eq!(pulse["class"], "emph");

    assert_eq!(s.doc().expect("a document").history.undo.len(), before, "asking for the list is not an edit");
}

/// Removing by index takes out that one effect and leaves the rest of the sequence in order.
#[test]
fn removing_an_animation_by_index_keeps_the_others_in_order() {
    let mut s = Session::with_new();
    let id = three_effects(&mut s);

    s.execute("animation.remove", &json!({"index": 1})).expect("remove the emphasis");
    assert_eq!(effects(&s), ["fade", "fly"], "the one that was taken out, and nothing else");

    let err = s.execute("animation.remove", &json!({"index": 9})).expect_err("there is no such animation");
    assert!(err.to_string().contains("no such animation"), "{err}");
    assert_eq!(effects(&s), ["fade", "fly"], "and nothing was removed on the way to the error");

    // Without an index it is the shapes that decide: every effect on them goes.
    s.execute("animation.remove", &json!({"ids": [id]})).expect("remove them all");
    assert!(sequence(&s).is_empty());

    s.execute("edit.undo", &json!({})).expect("undo");
    assert_eq!(effects(&s), ["fade", "fly"], "one step puts the shape's effects back");
}

/// Reordering is how the pane's up and down buttons work: the effect keeps its settings and moves
/// in the sequence.
#[test]
fn moving_an_animation_reorders_the_sequence() {
    let mut s = Session::with_new();
    let id = three_effects(&mut s);
    let spin = sequence(&s)[1].clone();

    s.execute("animation.move", &json!({"index": 1, "to": 0})).expect("spin to the front");
    assert_eq!(effects(&s), ["spin", "fade", "fly"]);
    assert_eq!(sequence(&s)[0], spin, "the effect itself travelled, it was not rebuilt");

    s.execute("animation.move", &json!({"index": 0, "to": 99})).expect("and to the back");
    assert_eq!(effects(&s), ["fade", "fly", "spin"], "a destination past the end lands at the end");

    let err = s.execute("animation.move", &json!({"index": 9, "to": 0})).expect_err("no such animation");
    assert!(err.to_string().contains("no such animation"), "{err}");
    let err = s.execute("animation.move", &json!({"to": 0})).expect_err("no index");
    assert!(err.to_string().contains("missing `index`"), "{err}");
    let err = s.execute("animation.move", &json!({"index": 0})).expect_err("no destination");
    assert!(err.to_string().contains("missing `to`"), "{err}");
    assert_eq!(effects(&s), ["fade", "fly", "spin"], "an error moves nothing");

    s.execute("edit.undo", &json!({})).expect("undo");
    assert_eq!(effects(&s), ["spin", "fade", "fly"], "the last move is undone");

    // The shape is only along for the ride while the sequence is reordered.
    assert!(sequence(&s).iter().all(|a| a.shape.0 == id));
}

/// Timing is everything about an animation except the effect itself: when it starts, how long it
/// runs, how long it waits, how often it repeats, and what sets it going.
#[test]
fn timing_sets_how_an_animation_starts_and_runs() {
    let mut s = Session::with_new();
    let a = shape(&mut s, "rect", 10.0);
    let b = shape(&mut s, "ellipse", 150.0);
    s.execute("animation.add", &json!({"effect": "fade", "ids": [a]})).expect("fade");
    s.execute("animation.add", &json!({"effect": "spin", "class": "emphasis", "ids": [b]})).expect("spin");
    let untouched = sequence(&s)[1].duration_ms;

    s.execute("animation.timing", &json!({"index": 0, "start": "withPrevious", "duration": 250, "delay": 100, "repeat": 3, "rewind": true}))
        .expect("timing");
    let x = &sequence(&s)[0];
    assert_eq!(x.start, AnimStart::WithPrevious);
    assert_eq!((x.duration_ms, x.delay_ms, x.repeat), (250, 100, 3));
    assert!(x.rewind, "rewind is how the effect gets back to where it started");
    assert_eq!(sequence(&s)[1].duration_ms, untouched, "the other animation was not mentioned, so it was left alone");

    // The words the pane shows, and the click on another object that starts this one.
    for (word, want) in [("with", AnimStart::WithPrevious), ("after", AnimStart::AfterPrevious), ("onClick", AnimStart::OnClick)] {
        s.execute("animation.timing", &json!({"index": 0, "start": word})).expect("a start");
        assert_eq!(sequence(&s)[0].start, want, "start `{word}`");
    }
    s.execute("animation.timing", &json!({"index": 0, "trigger": b})).expect("trigger on another object");
    assert_eq!(sequence(&s)[0].trigger.map(|t| t.0), Some(b));

    // The file format takes a u32 for each of these; the pane's own ceilings are lower.
    s.execute("animation.timing", &json!({"index": 0, "duration": 9_000_000, "delay": 9_000_000})).expect("absurd numbers");
    let x = &sequence(&s)[0];
    assert_eq!((x.duration_ms, x.delay_ms), (600_000, 600_000), "ten minutes, the longest PowerPoint plays");
    assert_eq!(x.trigger.map(|t| t.0), Some(b), "and what was not mentioned is still there");

    // Nothing to time is an error, not a silent no-op.
    let err = s.execute("animation.timing", &json!({"ids": [999], "duration": 100})).expect_err("no such animation");
    assert!(err.to_string().contains("no animation to change"), "{err}");
}
