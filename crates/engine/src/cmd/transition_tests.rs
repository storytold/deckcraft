//! Tests for the transition commands that had no test: the gallery query, the effect options, the
//! timing, the sound and Apply To All. `transition.set` itself is covered in `tests.rs`.

use deckcraft_model::MediaItem;
use deckcraft_model::Transition;
use deckcraft_model::anim::TRANSITIONS;
use serde_json::{Value, json};

use crate::Session;
use crate::cmd::base64_encode;

fn deck(n: usize) -> Session {
    let mut s = Session::with_new();
    while s.doc().expect("a document").doc.slides.len() < n {
        s.execute("slide.new", &json!({})).expect("a slide");
    }
    s
}

fn transition(s: &Session, i: usize) -> Option<Transition> {
    s.doc().expect("a document").doc.slides.get(i).expect("a slide").transition.clone()
}

fn media(s: &Session) -> Vec<MediaItem> {
    s.doc().expect("a document").doc.media.clone()
}

/// Effect Options only reaches a slide that already has a transition: it changes how the effect
/// plays, it does not choose one, so asking for an option on a plain slide quietly does nothing.
#[test]
fn an_effect_option_only_lands_on_a_slide_that_has_a_transition() {
    let mut s = Session::with_new();
    assert!(transition(&s, 0).is_none(), "a new deck's first slide has no effect");

    let err = s.execute("transition.options", &json!({})).expect_err("no option given");
    assert!(err.to_string().contains("missing `option`"), "{err}");

    s.execute("transition.options", &json!({"option": "r"})).expect("nothing to set it on");
    assert!(transition(&s, 0).is_none(), "an option does not invent a transition");

    s.execute("transition.set", &json!({"kind": "push"})).expect("push");
    assert_eq!(transition(&s, 0).expect("a transition").option, "u", "the first direction of the kind");
    s.execute("transition.options", &json!({"option": "r"})).expect("turn it");
    assert_eq!(transition(&s, 0).expect("a transition").option, "r");
    assert_eq!(transition(&s, 0).expect("a transition").kind, "push", "and only how it plays changed");
}

/// The thumbnail pane can hold several slides at once, so a transition command acts on all of them
/// — unless `index` names one, which is what the UI does for a single thumbnail.
#[test]
fn a_transition_command_reaches_every_selected_slide_or_just_the_indexed_one() {
    let mut s = deck(3);
    s.execute("slide.selectSlides", &json!({"indices": [0, 2]})).expect("select two");

    s.execute("transition.set", &json!({"kind": "fade"})).expect("fade both");
    assert_eq!(transition(&s, 0).expect("slide 0").kind, "fade");
    assert!(transition(&s, 1).is_none(), "the slide between them is not selected");
    assert_eq!(transition(&s, 2).expect("slide 2").kind, "fade");

    s.execute("transition.set", &json!({"kind": "wipe", "index": 1})).expect("wipe one");
    assert_eq!(transition(&s, 1).expect("slide 1").kind, "wipe");
    assert_eq!(transition(&s, 0).expect("slide 0").kind, "fade", "and only that one was touched");
}

/// Timing is where a slide is told to move on by itself; it creates the default transition when a
/// slide has none, and every number it takes from a caller is bounded before it reaches the file.
#[test]
fn timing_sets_when_the_slide_advances_and_bounds_the_numbers() {
    let mut s = Session::with_new();
    s.execute("transition.timing", &json!({"duration": 300})).expect("a timing");
    let t = transition(&s, 0).expect("a slide with no effect gets the default one for it");
    assert_eq!((t.kind.as_str(), t.duration_ms), ("none", 300));
    assert!(t.advance_on_click && t.advance_after_ms.is_none(), "which still waits for a click");

    s.execute("transition.timing", &json!({"after": 2000, "onClick": false})).expect("after two seconds");
    let t = transition(&s, 0).expect("a transition");
    assert_eq!(t.advance_after_ms, Some(2000));
    assert!(!t.advance_on_click, "this slide moves on by itself, a click does not wait for it");
    assert_eq!(t.duration_ms, 300, "the duration is not mentioned, so it is left alone");

    s.execute("transition.timing", &json!({"after": Value::Null})).expect("back to waiting for a click");
    assert_eq!(transition(&s, 0).expect("a transition").advance_after_ms, None);

    // The file format takes a u32; PowerPoint's own ceiling is lower than that.
    s.execute("transition.timing", &json!({"duration": 999_999, "after": 999_999_999})).expect("absurd numbers");
    let t = transition(&s, 0).expect("a transition");
    assert_eq!((t.duration_ms, t.advance_after_ms), (59_000, Some(86_400_000)));
}

/// A built-in sound is embedded once however many slides use it, `none` takes it off the slide
/// without throwing the bytes away, and the two flags it can set are checked before they stick.
#[test]
fn a_built_in_sound_is_embedded_once_and_none_takes_it_off_the_slide() {
    let mut s = Session::with_new();
    let r = s.execute("transition.sound", &json!({"builtin": "chime"})).expect("a built-in sound");
    assert_eq!(r["sound"], "Chime.wav");
    assert_eq!(r["playable"], true);
    let items = media(&s);
    assert_eq!(items.len(), 1, "the sound is in the file now");
    assert_eq!((items[0].name.as_str(), items[0].content_type.as_str()), ("Chime.wav", "audio/wav"));
    let sound = items[0].id;
    assert_eq!(transition(&s, 0).expect("a transition").sound, Some(sound));
    assert!(!transition(&s, 0).expect("a transition").sound_loop, "a sound does not loop by itself");

    // The same sound on another slide is the bytes already in the file, not a second copy.
    s.execute("slide.new", &json!({})).expect("a second slide");
    s.execute("transition.sound", &json!({"builtin": "CHIME"})).expect("the same sound, spelled differently");
    assert_eq!(media(&s).len(), 1, "embedded once");
    assert_eq!(transition(&s, 1).expect("a transition").sound, Some(sound));
    assert_eq!(transition(&s, 0).expect("a transition").sound, Some(sound), "the first slide still has it");

    // Loop Until Next Sound means nothing without a sound to loop.
    s.execute("transition.sound", &json!({"loop": true})).expect("loop");
    assert!(transition(&s, 1).expect("a transition").sound_loop);
    s.execute("transition.sound", &json!({"index": 0, "loop": false})).expect("and not loop");
    assert!(!transition(&s, 0).expect("a transition").sound_loop);

    s.execute("transition.sound", &json!({"none": true})).expect("take the sound away");
    let t = transition(&s, 1).expect("a transition");
    assert!(t.sound.is_none() && !t.sound_loop, "taking the sound away takes the loop with it");
    assert_eq!(media(&s).len(), 1, "but the bytes stay in the document, they may be used again");

    // Stop Previous Sound is a marker on the slide, not a sound of its own.
    s.execute("transition.sound", &json!({"stopPrevious": true})).expect("stop the previous one");
    let t = transition(&s, 1).expect("a transition");
    assert!(t.stop_sound && t.sound.is_none());

    // A slide with no sound cannot be told to loop one.
    s.execute("slide.new", &json!({})).expect("a third slide");
    s.execute("transition.sound", &json!({"loop": true})).expect("loop with nothing to loop");
    assert!(!transition(&s, 2).expect("a transition").sound_loop, "there is no sound to loop, so the flag stays off");
}

#[test]
fn a_sound_that_is_not_a_sound_is_refused_with_the_reason() {
    let mut s = Session::with_new();
    let err = s.execute("transition.sound", &json!({"builtin": "nope"})).expect_err("no such sound");
    assert!(err.to_string().contains("unknown built-in sound"), "{err}");
    assert!(err.to_string().contains("chime"), "and it lists what there is: {err}");

    let err = s.execute("transition.sound", &json!({"name": "noise.txt", "data": base64_encode(b"hello")})).expect_err("not a sound");
    assert!(err.to_string().contains("is not a sound file"), "{err}");

    let err = s.execute("transition.sound", &json!({})).expect_err("nothing asked for");
    assert!(err.to_string().contains("give `path`"), "{err}");
    assert!(media(&s).is_empty(), "and nothing was embedded on the way to the error");
}

/// Apply To All copies the *current* slide onto every other one — including the effect options,
/// the timing and the sound.
#[test]
fn apply_to_all_copies_the_current_slide_onto_every_other_slide() {
    let mut s = deck(3);
    // Slide 1 and not slide 0, so an implementation that reads the first slide is caught.
    s.execute("slide.go", &json!({"index": 1})).expect("go to the second slide");
    s.execute("transition.set", &json!({"kind": "wipe", "option": "ld", "duration": 2500})).expect("wipe");
    s.execute("transition.timing", &json!({"after": 500})).expect("and move on by itself");
    s.execute("transition.sound", &json!({"builtin": "click"})).expect("with a sound");

    s.execute("transition.applyAll", &json!({})).expect("apply to all");
    let first = transition(&s, 0).expect("slide 0");
    for i in 0..3 {
        let t = transition(&s, i).unwrap_or_else(|| panic!("slide {i} has no transition"));
        assert_eq!((t.kind.as_str(), t.option.as_str(), t.duration_ms), ("wipe", "ld", 2500), "slide {i}");
        assert_eq!(t.advance_after_ms, Some(500), "slide {i}");
        assert_eq!(t.sound, first.sound, "slide {i}");
        assert!(t.sound.is_some(), "slide {i} has the sound");
    }

    // `none` with nothing else on the slide is no transition at all, and Apply To All copies that
    // too: a deck can be put back to plain with one command.
    s.execute("slide.go", &json!({"index": 0})).expect("back to the first slide");
    s.execute("transition.timing", &json!({"after": Value::Null})).expect("waits for a click again");
    s.execute("transition.sound", &json!({"none": true})).expect("and no sound");
    s.execute("transition.set", &json!({"kind": "none"})).expect("no effect");
    assert!(transition(&s, 0).is_none(), "none with nothing else left on it is no transition at all");
    s.execute("transition.applyAll", &json!({})).expect("apply that to all");
    for i in 0..3 {
        assert!(transition(&s, i).is_none(), "slide {i} is back to no effect");
    }
}

/// The gallery is a query: it reports the table the rest of the app uses and touches nothing.
#[test]
fn the_gallery_lists_every_transition() {
    let mut s = Session::with_new();
    let before = s.doc().expect("a document").history.undo.len();

    let v = s.execute("transition.list", &json!({})).expect("the gallery");
    let all = v.as_array().expect("an array");
    assert_eq!(all.len(), TRANSITIONS.len());
    assert_eq!(all[0]["id"], "none");
    assert_eq!((all[0]["label"].as_str(), all[0]["category"].as_str()), (Some("None"), Some("Subtle")));
    assert_eq!(all[0]["duration"], 0, "no effect takes no time");
    assert_eq!(all[0]["options"], json!([]), "and has nothing to configure");

    let fade = all.iter().find(|t| t["id"] == "fade").expect("fade is in the gallery");
    assert_eq!(fade["label"], "Fade");
    assert_eq!(fade["duration"], 700);
    assert_eq!(fade["options"], json!(["smoothly", "throughBlack"]));

    assert_eq!(s.doc().expect("a document").history.undo.len(), before, "asking for the list is not an edit");
}
