//! `edit.batch`: several commands as one step. An agent that builds a slide in several calls
//! leaves it half-built when a later call fails; a batch makes every change to the presentation or
//! none, and one undo takes all of them back. `document.inspect` reports how many undo and redo
//! steps there are, so an agent can check that.

use serde_json::{Value, json};

use crate::Session;
use crate::cmd::base64_encode;

fn depth(s: &mut Session) -> (u64, u64) {
    let d = s.execute("document.inspect", &json!({})).unwrap();
    (d["undoDepth"].as_u64().unwrap(), d["redoDepth"].as_u64().unwrap())
}

fn names(s: &Session) -> Vec<String> {
    s.doc().unwrap().shapes().iter().map(|x| x.name.clone()).collect()
}

fn rect(x: f64) -> Value {
    json!({"command": "shape.insert", "params": {"preset": "rect", "rect": [x, 50, 100, 80]}})
}

/// A step that fails after earlier steps went through: no shape has this id.
fn bad_rename() -> Value {
    json!({"command": "shape.rename", "params": {"id": 4_000_000, "name": "Nobody"}})
}

/// The whole presentation and the selection, to compare before and after.
fn snapshot(s: &Session) -> (Value, Value) {
    let st = s.doc().unwrap();
    (serde_json::to_value(&*st.doc).unwrap(), serde_json::to_value(&st.selection).unwrap())
}

fn blank() -> Session {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s
}

#[test]
fn a_batch_makes_every_change_in_one_undo_step() {
    let mut s = blank();
    let shapes = names(&s).len();
    let (undo, _) = depth(&mut s);
    let r = s
        .execute(
            "edit.batch",
            &json!({"commands": [
                rect(20.0),
                {"command": "shape.rename", "params": {"name": "First"}},
                rect(140.0),
                {"command": "shape.fill", "params": {"color": "#C00000"}}
            ]}),
        )
        .unwrap();
    let results = r["results"].as_array().unwrap();
    assert_eq!(results.len(), 4, "{r}");
    assert_eq!(results[0]["command"], "shape.insert", "{r}");
    assert!(results[0]["result"]["id"].is_u64(), "{r}");
    assert_eq!(names(&s).len(), shapes + 2);
    assert!(names(&s).contains(&"First".to_string()), "{:?}", names(&s));
    assert_eq!(depth(&mut s), (undo + 1, 0));
    assert_eq!(s.execute("document.inspect", &json!({})).unwrap()["undo"], "Batch Edit");
    // One undo takes back both shapes, the name and the fill.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(names(&s).len(), shapes);
    assert_eq!(depth(&mut s), (undo, 1));
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(names(&s).len(), shapes + 2);
    // The journal records the batch once, so a replay runs it as one step too.
    assert_eq!(s.journal.iter().filter(|(id, _)| id == "edit.batch").count(), 1);
    assert!(!s.journal.iter().any(|(id, _)| id == "shape.rename"), "{:?}", s.journal);
}

#[test]
fn a_failing_step_leaves_the_presentation_and_selection_as_they_were() {
    let mut s = blank();
    s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [300, 200, 80, 80]})).unwrap();
    let before = snapshot(&s);
    let steps = depth(&mut s);
    let journal = s.journal.len();
    let e = s.execute("edit.batch", &json!({"commands": [rect(20.0), rect(140.0), bad_rename()]})).unwrap_err().to_string();
    assert!(e.contains("step 2 (`shape.rename`)"), "{e}");
    assert!(e.contains("failed, so the presentation was left as it was"), "{e}");
    assert_eq!(snapshot(&s), before);
    assert_eq!(depth(&mut s), steps);
    assert_eq!(s.journal.len(), journal);
}

#[test]
fn a_failing_batch_of_selection_changes_puts_the_selection_back() {
    let mut s = blank();
    let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap()["id"].clone();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [140, 50, 100, 80]})).unwrap();
    let before = snapshot(&s);
    let steps = json!([{"command": "edit.select", "params": {"ids": [a]}}, {"command": "edit.selectAll"}, bad_rename()]);
    assert!(s.execute("edit.batch", &json!({"commands": steps})).is_err());
    assert_eq!(snapshot(&s), before);
    // Without the failing step the same selection changes go through.
    let steps = json!([{"command": "edit.select", "params": {"ids": [a]}}]);
    s.execute("edit.batch", &json!({"commands": steps})).unwrap();
    assert_eq!(serde_json::to_value(&s.doc().unwrap().selection.shapes).unwrap(), json!([a]));
}

/// font.replace lists the fonts the presentation uses that aren't installed (the app tells the user
/// about them). A batch whose replace is undone by a later failing step leaves that list as it was.
#[test]
fn a_failing_batch_puts_the_list_of_missing_fonts_back() {
    let mut s = blank();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 300, 80], "text": "Hello"})).unwrap();
    let used = s.execute("font.used", &json!({})).unwrap();
    let family = used[0].as_str().unwrap().to_string();
    s.execute("font.replace", &json!({"from": family, "to": "Qqq Nonexistent Sans"})).unwrap();
    let missing = vec!["Qqq Nonexistent Sans".to_string()];
    assert_eq!(s.doc().unwrap().missing_fonts, missing);
    // Back to a bundled family, which empties the list, then a step that fails.
    let have = deckcraft_fonts::FontDb::global().face(deckcraft_fonts::FALLBACK_FAMILY, "Regular").family.clone();
    let fix = json!({"command": "font.replace", "params": {"from": "Qqq Nonexistent Sans", "to": have}});
    let e = s.execute("edit.batch", &json!({"commands": [fix, bad_rename()]})).unwrap_err().to_string();
    assert!(e.contains("step 1 (`shape.rename`)"), "{e}");
    assert_eq!(s.execute("font.used", &json!({})).unwrap(), json!(["Qqq Nonexistent Sans"]));
    assert_eq!(s.doc().unwrap().missing_fonts, missing);
    // Without the failing step the replace goes through and the list empties.
    let fix = json!({"command": "font.replace", "params": {"from": "Qqq Nonexistent Sans", "to": have}});
    s.execute("edit.batch", &json!({"commands": [fix]})).unwrap();
    assert!(s.doc().unwrap().missing_fonts.is_empty());
}

/// Duplicating the copies of the last Duplicate again repeats how far they were moved (#966). A
/// batch that duplicates the latest copy and then fails leaves that record as it was, so the next
/// Duplicate repeats the step as it would have without the batch.
#[test]
fn a_failing_batch_leaves_the_offset_duplicate_repeats() {
    let mut s = blank();
    let pos = |s: &mut Session, id: &Value| {
        let b = s.execute("shape.inspect", &json!({"id": id})).unwrap()["box"].clone();
        (b["x"].as_f64().unwrap(), b["y"].as_f64().unwrap())
    };
    let duplicate = |s: &mut Session, id: &Value| s.execute("edit.duplicate", &json!({"ids": [id]})).unwrap()["ids"][0].clone();
    let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 80, 80]})).unwrap()["id"].clone();
    let b = duplicate(&mut s, &a);
    // The copy moved to 60 pt right of and 40 pt below the original: Duplicate repeats that step.
    let ((ax, ay), (bx, by)) = (pos(&mut s, &a), pos(&mut s, &b));
    s.execute("shape.move", &json!({"ids": [b], "dx": ax + 60.0 - bx, "dy": ay + 40.0 - by})).unwrap();
    let c = duplicate(&mut s, &b);
    let (cx, cy) = pos(&mut s, &c);
    assert!((cx - ax - 120.0).abs() < 1e-6 && (cy - ay - 80.0).abs() < 1e-6, "{:?}", (cx - ax, cy - ay));
    let steps = json!([{"command": "edit.duplicate", "params": {"ids": [c]}}, bad_rename()]);
    let e = s.execute("edit.batch", &json!({"commands": steps})).unwrap_err().to_string();
    assert!(e.contains("step 1 (`shape.rename`)"), "{e}");
    let d = duplicate(&mut s, &c);
    let (dx, dy) = pos(&mut s, &d);
    assert!((dx - ax - 180.0).abs() < 1e-6 && (dy - ay - 120.0).abs() < 1e-6, "copy at {:?} from the original", (dx - ax, dy - ay));
}

/// The open deck saved as base64 `.deckcraft` bytes, to compare it with later (Review ▸ Compare).
fn saved_copy(s: &mut Session) -> Value {
    s.execute("file.saveBytes", &json!({})).unwrap()["data"].clone()
}

/// Starting a comparison is not an undo step. A batch that starts one and then fails leaves no
/// comparison open: `review.revisions` refuses, as before the batch.
#[test]
fn a_failing_batch_that_starts_a_comparison_leaves_none_open() {
    let mut s = blank();
    let saved = saved_copy(&mut s);
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80], "text": "Added"})).unwrap();
    assert!(s.execute("review.revisions", &json!({})).is_err());
    let steps = json!([{"command": "review.compare", "params": {"name": "saved.deckcraft", "data": saved}}, bad_rename()]);
    let e = s.execute("edit.batch", &json!({"commands": steps})).unwrap_err().to_string();
    assert!(e.contains("step 1 (`shape.rename`)"), "{e}");
    let open = s.execute("review.revisions", &json!({})).ok();
    assert_eq!(open, None, "a comparison is open after the failed batch");
}

/// Ending a comparison is not an undo step either. A batch that ends one and then fails leaves the
/// same comparison open.
#[test]
fn a_failing_batch_that_ends_a_comparison_leaves_it_open() {
    let mut s = blank();
    let saved = saved_copy(&mut s);
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80], "text": "Added"})).unwrap();
    let opened = s.execute("review.compare", &json!({"name": "saved.deckcraft", "data": saved})).unwrap();
    let steps = json!([{"command": "review.endCompare"}, bad_rename()]);
    let e = s.execute("edit.batch", &json!({"commands": steps})).unwrap_err().to_string();
    assert!(e.contains("step 1 (`shape.rename`)"), "{e}");
    assert_eq!(s.execute("review.revisions", &json!({})).ok(), Some(opened));
}

/// Set as Default Shape gives the shapes inserted after it the selected shape's look; it is not an
/// undo step. A batch that sets the default and then fails leaves the default as it was.
#[test]
fn a_failing_batch_leaves_the_default_shape_look_as_it_was() {
    let mut s = blank();
    let insert = |s: &mut Session| s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap()["id"].clone();
    let fill = |s: &mut Session, id: &Value| s.execute("shape.inspect", &json!({"id": id})).unwrap()["shape"]["fill"].clone();
    let plain = insert(&mut s);
    let red = insert(&mut s);
    s.execute("shape.fill", &json!({"ids": [red], "color": "#C00000"})).unwrap();
    let set_default = |id: &Value| json!([{"command": "edit.select", "params": {"ids": [id]}}, {"command": "shape.setDefault"}, bad_rename()]);
    let e = s.execute("edit.batch", &json!({"commands": set_default(&red)})).unwrap_err().to_string();
    assert!(e.contains("step 2 (`shape.rename`)"), "{e}");
    let after = insert(&mut s);
    assert_eq!(fill(&mut s, &after), fill(&mut s, &plain), "the failed batch set the default look");
    // A default set before the batch stays when the failing batch sets another.
    s.execute("edit.select", &json!({"ids": [red]})).unwrap();
    s.execute("shape.setDefault", &json!({})).unwrap();
    assert!(s.execute("edit.batch", &json!({"commands": set_default(&plain)})).is_err());
    let after = insert(&mut s);
    assert_eq!(fill(&mut s, &after), fill(&mut s, &red));
}

/// The Format Painter holds the formatting it picked up until it is applied; picking it up is not
/// an undo step. A failing batch leaves the painter armed or not, as it was before the batch.
#[test]
fn a_failing_batch_leaves_the_format_painter_as_it_was() {
    let mut s = blank();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap();
    let armed = |s: &mut Session| s.execute("format.state", &json!({})).unwrap()["painter"].clone();
    assert_eq!(armed(&mut s), false);
    let e = s.execute("edit.batch", &json!({"commands": [{"command": "format.painter"}, bad_rename()]})).unwrap_err().to_string();
    assert!(e.contains("step 1 (`shape.rename`)"), "{e}");
    assert_eq!(armed(&mut s), false, "the failed batch armed the Format Painter");
    // Armed before the batch, it stays armed when a step of the failing batch clears it.
    s.execute("format.painter", &json!({})).unwrap();
    assert!(s.execute("edit.batch", &json!({"commands": [{"command": "edit.deselect"}, bad_rename()]})).is_err());
    assert_eq!(armed(&mut s), true);
}

/// The Animation Painter does the same with a shape's animations.
#[test]
fn a_failing_batch_leaves_the_animation_painter_as_it_was() {
    let mut s = blank();
    let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap()["id"].clone();
    let b = s.execute("shape.insert", &json!({"preset": "rect", "rect": [140, 50, 100, 80]})).unwrap()["id"].clone();
    s.execute("animation.add", &json!({"effect": "fade", "class": "entrance", "ids": [a]})).unwrap();
    let pick = json!({"command": "animation.painter", "params": {"id": a}});
    let e = s.execute("edit.batch", &json!({"commands": [pick, bad_rename()]})).unwrap_err().to_string();
    assert!(e.contains("step 1 (`shape.rename`)"), "{e}");
    let e = s.execute("animation.painterApply", &json!({"ids": [b]})).unwrap_err().to_string();
    assert!(e.contains("pick animations with the Animation Painter first"), "{e}");
    // Armed before the batch, it stays armed when a step of the failing batch clears it.
    s.execute("animation.painter", &json!({"id": a})).unwrap();
    assert!(s.execute("edit.batch", &json!({"commands": [{"command": "edit.deselect"}, bad_rename()]})).is_err());
    assert_eq!(s.execute("animation.painterApply", &json!({"ids": [b]})).unwrap()["applied"], 1);
}

/// A twist on a trackpad is one undo step, from `shape.rotate` with `gesture: move` to
/// `gesture: end`. A batch that starts one and then fails leaves no gesture open: the next edit is
/// an undo step of its own, and undoing it does not bring back what the batch added.
#[test]
fn a_failing_batch_that_starts_a_gesture_leaves_none_open() {
    let mut s = blank();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap()["id"].clone();
    let shapes = names(&s).len();
    let (undo, _) = depth(&mut s);
    let steps = json!([
        {"command": "shape.insert", "params": {"preset": "ellipse", "rect": [300, 200, 80, 80]}},
        {"command": "shape.rotate", "params": {"by": 10.0, "gesture": "move"}},
        bad_rename(),
    ]);
    let e = s.execute("edit.batch", &json!({"commands": steps})).unwrap_err().to_string();
    assert!(e.contains("step 2 (`shape.rename`)"), "{e}");
    assert_eq!(names(&s).len(), shapes);
    s.execute("shape.move", &json!({"ids": [id], "dx": 10.0, "dy": 0.0})).unwrap();
    assert_eq!(depth(&mut s), (undo + 1, 0), "the move made no undo step of its own");
    // Ending a gesture finds none to end, and one undo takes back the move alone.
    s.execute("shape.rotate", &json!({"gesture": "end"})).unwrap();
    assert_eq!(depth(&mut s), (undo + 1, 0));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(names(&s).len(), shapes, "undo brought back a shape from the failed batch");
    assert_eq!(depth(&mut s), (undo, 1));
}

/// A twist that was open before the batch stays open when a step of the failing batch ends it, so
/// ending it afterwards still makes the whole twist one undo step.
#[test]
fn a_failing_batch_that_ends_a_gesture_leaves_it_open() {
    let mut s = blank();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap()["id"].clone();
    let rotation = |s: &mut Session| s.execute("shape.inspect", &json!({"id": id})).unwrap()["box"]["rot"].as_f64().unwrap_or(0.0);
    let (undo, _) = depth(&mut s);
    s.execute("shape.rotate", &json!({"by": 10.0, "gesture": "move"})).unwrap();
    let steps = json!([{"command": "shape.rotate", "params": {"gesture": "end"}}, bad_rename()]);
    let e = s.execute("edit.batch", &json!({"commands": steps})).unwrap_err().to_string();
    assert!(e.contains("step 1 (`shape.rename`)"), "{e}");
    s.execute("shape.rotate", &json!({"by": 10.0, "gesture": "move"})).unwrap();
    s.execute("shape.rotate", &json!({"gesture": "end"})).unwrap();
    assert!((rotation(&mut s) - 20.0).abs() < 1e-9, "rotation {}", rotation(&mut s));
    assert_eq!(depth(&mut s), (undo + 1, 0));
    // One undo takes back the whole twist.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(rotation(&mut s).abs() < 1e-9, "rotation {} after one undo", rotation(&mut s));
}

/// A batch that leaves open a twist it started fails and leaves the presentation as it was. The
/// twist's undo step would begin partway through the batch: undoing it would leave the batch's
/// earlier edits behind, to go with the undo step before the batch.
#[test]
fn a_batch_that_leaves_open_a_gesture_it_started_fails_and_leaves_the_presentation_as_it_was() {
    let mut s = blank();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap()["id"].clone();
    let before = snapshot(&s);
    let steps = depth(&mut s);
    let open = json!([
        {"command": "shape.insert", "params": {"preset": "ellipse", "rect": [300, 200, 80, 80]}},
        {"command": "shape.rotate", "params": {"by": 10.0, "gesture": "move"}},
    ]);
    let r = s.execute("edit.batch", &json!({"commands": open}));
    let e = r.as_ref().err().map(ToString::to_string).unwrap_or_default();
    assert!(e.contains("step 1 (`shape.rotate`) started a gesture"), "{r:?}");
    assert!(e.contains("`gesture: \"end\"`"), "{e}");
    assert_eq!(snapshot(&s), before, "the ellipse or the twist stayed");
    assert_eq!(depth(&mut s), steps);
    assert!(s.doc().unwrap().interaction.is_none(), "a gesture is still open");
    // The next edit is an undo step of its own.
    s.execute("shape.move", &json!({"ids": [id], "dx": 10.0, "dy": 0.0})).unwrap();
    assert_eq!(depth(&mut s), (steps.0 + 1, 0));
}

/// The same holds for a batch that ends a twist open before it and starts another.
#[test]
fn a_batch_that_ends_a_gesture_and_leaves_its_own_open_fails_and_leaves_the_presentation_as_it_was() {
    let mut s = blank();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap()["id"].clone();
    let rotation = |s: &mut Session| s.execute("shape.inspect", &json!({"id": id})).unwrap()["box"]["rot"].as_f64().unwrap_or(0.0);
    let (undo, _) = depth(&mut s);
    s.execute("shape.rotate", &json!({"by": 10.0, "gesture": "move"})).unwrap();
    let before = snapshot(&s);
    let swap = json!([
        {"command": "shape.rotate", "params": {"gesture": "end"}},
        {"command": "shape.insert", "params": {"preset": "ellipse", "rect": [300, 200, 80, 80]}},
        {"command": "shape.rotate", "params": {"by": 10.0, "gesture": "move"}},
    ]);
    let r = s.execute("edit.batch", &json!({"commands": swap}));
    let e = r.as_ref().err().map(ToString::to_string).unwrap_or_default();
    assert!(e.contains("step 2 (`shape.rotate`) started a gesture"), "{r:?}");
    assert_eq!(snapshot(&s), before);
    assert_eq!(depth(&mut s), (undo, 0));
    // The twist open before the batch is still open, and ending it makes it one undo step.
    s.execute("shape.rotate", &json!({"gesture": "end"})).unwrap();
    assert_eq!(depth(&mut s), (undo + 1, 0));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(rotation(&mut s).abs() < 1e-9, "rotation {} after one undo", rotation(&mut s));
}

/// A batch run while a twist is open, which leaves that twist open, goes through: its edits join
/// the twist's undo step, as any command's do while a twist is open.
#[test]
fn a_batch_run_inside_an_open_gesture_joins_its_undo_step() {
    let mut s = blank();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap()["id"].clone();
    let rotation = |s: &mut Session| s.execute("shape.inspect", &json!({"id": id})).unwrap()["box"]["rot"].as_f64().unwrap_or(0.0);
    let shapes = names(&s).len();
    let (undo, _) = depth(&mut s);
    s.execute("shape.rotate", &json!({"by": 10.0, "gesture": "move"})).unwrap();
    let steps = json!([
        {"command": "shape.insert", "params": {"preset": "ellipse", "rect": [300, 200, 80, 80]}},
        {"command": "shape.rotate", "params": {"ids": [id], "by": 10.0, "gesture": "move"}},
    ]);
    s.execute("edit.batch", &json!({"commands": steps})).unwrap();
    assert_eq!(names(&s).len(), shapes + 1);
    assert_eq!(depth(&mut s), (undo, 0), "an undo step while the twist is open");
    s.execute("shape.rotate", &json!({"gesture": "end"})).unwrap();
    assert!((rotation(&mut s) - 20.0).abs() < 1e-9, "rotation {}", rotation(&mut s));
    assert_eq!(depth(&mut s), (undo + 1, 0));
    // One undo takes back the twist and the batch.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(rotation(&mut s).abs() < 1e-9, "rotation {} after one undo", rotation(&mut s));
    assert_eq!(names(&s).len(), shapes);
}

/// A batch that starts a twist and ends it goes through as one undo step.
#[test]
fn a_batch_that_starts_and_ends_a_gesture_is_one_undo_step() {
    let mut s = blank();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap();
    let shapes = names(&s).len();
    let (undo, _) = depth(&mut s);
    let steps = json!([
        {"command": "shape.insert", "params": {"preset": "ellipse", "rect": [300, 200, 80, 80]}},
        {"command": "shape.rotate", "params": {"by": 10.0, "gesture": "move"}},
        {"command": "shape.rotate", "params": {"by": 10.0, "gesture": "move"}},
        {"command": "shape.rotate", "params": {"gesture": "end"}},
    ]);
    s.execute("edit.batch", &json!({"commands": steps})).unwrap();
    assert!(s.doc().unwrap().interaction.is_none(), "a gesture is still open");
    assert_eq!(depth(&mut s), (undo + 1, 0));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(names(&s).len(), shapes);
    assert_eq!(depth(&mut s), (undo, 1));
}

/// The label of the step the next undo takes back.
fn next_undo(s: &mut Session) -> Value {
    s.execute("document.inspect", &json!({})).unwrap()["undo"].clone()
}

/// A batch that ends a twist opened before it keeps the twist's own undo step, ahead of the
/// batch's: one undo takes back the batch and the next the twist, as if the twist had ended just
/// before the batch.
#[test]
fn a_batch_that_ends_an_earlier_gesture_leaves_the_gesture_its_own_undo_step() {
    let mut s = blank();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap()["id"].clone();
    let rotation = |s: &mut Session| s.execute("shape.inspect", &json!({"id": id})).unwrap()["box"]["rot"].as_f64().unwrap_or(0.0);
    let shapes = names(&s).len();
    let (undo, _) = depth(&mut s);
    s.execute("shape.rotate", &json!({"by": 10.0, "gesture": "move"})).unwrap();
    let steps = json!([
        {"command": "shape.rotate", "params": {"gesture": "end"}},
        {"command": "shape.insert", "params": {"preset": "ellipse", "rect": [300, 200, 80, 80]}},
    ]);
    s.execute("edit.batch", &json!({"commands": steps})).unwrap();
    assert!(s.doc().unwrap().interaction.is_none(), "a gesture is still open");
    assert_eq!(names(&s).len(), shapes + 1);
    assert_eq!(depth(&mut s), (undo + 2, 0), "the twist and the batch are not two undo steps");
    assert_eq!(next_undo(&mut s), "Batch Edit");
    // One undo takes back the ellipse and leaves the twist.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(names(&s).len(), shapes);
    assert!((rotation(&mut s) - 10.0).abs() < 1e-9, "rotation {} after one undo", rotation(&mut s));
    assert_eq!(next_undo(&mut s), "Rotation");
    // The next takes back the twist, and the shape is still there.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(names(&s).len(), shapes);
    assert!(rotation(&mut s).abs() < 1e-9, "rotation {} after two undos", rotation(&mut s));
    assert_eq!(depth(&mut s), (undo, 2));
    // The third takes back the shape's insertion, the step before the twist.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(names(&s).len(), shapes - 1);
}

/// A batch whose only step ends a twist opened before it adds the twist's undo step and no step of
/// its own: nothing else changed.
#[test]
fn a_batch_that_only_ends_an_earlier_gesture_adds_just_the_gestures_undo_step() {
    let mut s = blank();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap()["id"].clone();
    let rotation = |s: &mut Session| s.execute("shape.inspect", &json!({"id": id})).unwrap()["box"]["rot"].as_f64().unwrap_or(0.0);
    let (undo, _) = depth(&mut s);
    s.execute("shape.rotate", &json!({"by": 10.0, "gesture": "move"})).unwrap();
    s.execute("edit.batch", &json!({"commands": [{"command": "shape.rotate", "params": {"gesture": "end"}}]})).unwrap();
    assert!(s.doc().unwrap().interaction.is_none(), "a gesture is still open");
    assert_eq!(depth(&mut s), (undo + 1, 0), "the twist has no undo step, or the batch added one");
    assert_eq!(next_undo(&mut s), "Rotation");
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(rotation(&mut s).abs() < 1e-9, "rotation {} after one undo", rotation(&mut s));
    assert_eq!(depth(&mut s), (undo, 1));
}

/// The steps before and after the one that ends a twist opened before the batch undo together, as
/// the batch's one step, after the twist's own: one undo takes back the ellipse, the rectangle and
/// the 5 degrees the batch added to the twist, and the next takes back the twist's first 10.
#[test]
fn a_batch_that_ends_an_earlier_gesture_between_its_edits_undoes_them_together() {
    let mut s = blank();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap()["id"].clone();
    let rotation = |s: &mut Session| s.execute("shape.inspect", &json!({"id": id})).unwrap()["box"]["rot"].as_f64().unwrap_or(0.0);
    let shapes = names(&s).len();
    let (undo, _) = depth(&mut s);
    s.execute("shape.rotate", &json!({"by": 10.0, "gesture": "move"})).unwrap();
    let steps = json!([
        {"command": "shape.insert", "params": {"preset": "ellipse", "rect": [300, 200, 80, 80]}},
        {"command": "shape.rotate", "params": {"ids": [id], "by": 5.0, "gesture": "move"}},
        {"command": "shape.rotate", "params": {"gesture": "end"}},
        rect(140.0),
    ]);
    s.execute("edit.batch", &json!({"commands": steps})).unwrap();
    assert!((rotation(&mut s) - 15.0).abs() < 1e-9, "rotation {}", rotation(&mut s));
    assert_eq!(names(&s).len(), shapes + 2);
    assert_eq!(depth(&mut s), (undo + 2, 0));
    // One undo takes back all the batch did, before the twist ended and after.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(names(&s).len(), shapes);
    assert!((rotation(&mut s) - 10.0).abs() < 1e-9, "rotation {} after one undo", rotation(&mut s));
    // The next takes back the twist from before the batch.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(names(&s).len(), shapes);
    assert!(rotation(&mut s).abs() < 1e-9, "rotation {} after two undos", rotation(&mut s));
}

/// A batch that ends a twist opened before it, edits and then fails leaves the presentation as it
/// was and the twist open, so ending the twist afterwards makes it one undo step.
#[test]
fn a_batch_that_fails_after_ending_an_earlier_gesture_and_editing_leaves_the_gesture_open() {
    let mut s = blank();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap()["id"].clone();
    let rotation = |s: &mut Session| s.execute("shape.inspect", &json!({"id": id})).unwrap()["box"]["rot"].as_f64().unwrap_or(0.0);
    let (undo, _) = depth(&mut s);
    s.execute("shape.rotate", &json!({"by": 10.0, "gesture": "move"})).unwrap();
    let before = snapshot(&s);
    let steps = json!([
        {"command": "shape.rotate", "params": {"gesture": "end"}},
        {"command": "shape.insert", "params": {"preset": "ellipse", "rect": [300, 200, 80, 80]}},
        bad_rename(),
    ]);
    let e = s.execute("edit.batch", &json!({"commands": steps})).unwrap_err().to_string();
    assert!(e.contains("step 2 (`shape.rename`)"), "{e}");
    assert_eq!(snapshot(&s), before, "the ellipse stayed or the twist moved");
    assert_eq!(depth(&mut s), (undo, 0));
    assert!(s.doc().unwrap().interaction.is_some(), "the twist is no longer open");
    s.execute("shape.rotate", &json!({"gesture": "end"})).unwrap();
    assert_eq!(depth(&mut s), (undo + 1, 0));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(rotation(&mut s).abs() < 1e-9, "rotation {} after one undo", rotation(&mut s));
}

/// A batch that ends a twist opened before it and then starts and ends one of its own keeps the
/// earlier twist's undo step; its own twist is part of the batch's step.
#[test]
fn a_batch_that_ends_an_earlier_gesture_and_runs_its_own_keeps_only_the_earlier_ones_step() {
    let mut s = blank();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap()["id"].clone();
    let rotation = |s: &mut Session| s.execute("shape.inspect", &json!({"id": id})).unwrap()["box"]["rot"].as_f64().unwrap_or(0.0);
    let shapes = names(&s).len();
    let (undo, _) = depth(&mut s);
    s.execute("shape.rotate", &json!({"by": 10.0, "gesture": "move"})).unwrap();
    let steps = json!([
        {"command": "shape.rotate", "params": {"gesture": "end"}},
        {"command": "shape.rotate", "params": {"by": 10.0, "gesture": "move"}},
        {"command": "shape.rotate", "params": {"gesture": "end"}},
        {"command": "shape.insert", "params": {"preset": "ellipse", "rect": [300, 200, 80, 80]}},
    ]);
    s.execute("edit.batch", &json!({"commands": steps})).unwrap();
    assert!((rotation(&mut s) - 20.0).abs() < 1e-9, "rotation {}", rotation(&mut s));
    assert_eq!(depth(&mut s), (undo + 2, 0));
    // One undo takes back the batch's twist and the ellipse.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(names(&s).len(), shapes);
    assert!((rotation(&mut s) - 10.0).abs() < 1e-9, "rotation {} after one undo", rotation(&mut s));
    // The next takes back the twist from before the batch.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(names(&s).len(), shapes);
    assert!(rotation(&mut s).abs() < 1e-9, "rotation {} after two undos", rotation(&mut s));
}

/// AutoCorrect's options and replacement list are preferences, which the app saves when it quits;
/// changing them is not an undo step. A failing batch leaves them as they were.
#[test]
fn a_failing_batch_leaves_the_autocorrect_options_as_they_were() {
    let mut s = blank();
    let options = |s: &mut Session| s.execute("text.autocorrect", &json!({})).unwrap();
    let before = options(&mut s);
    let change = json!({"command": "text.autocorrect", "params": {"on": false, "add": ["dkc", "DeckCraft"]}});
    let e = s.execute("edit.batch", &json!({"commands": [change, bad_rename()]})).unwrap_err().to_string();
    assert!(e.contains("step 1 (`shape.rename`)"), "{e}");
    assert_eq!(options(&mut s), before, "the failed batch changed the AutoCorrect options");
    // Without the failing step the change goes through.
    s.execute("edit.batch", &json!({"commands": [change]})).unwrap();
    let after = options(&mut s);
    assert_eq!(after["on"], false);
    assert!(after["entries"].as_array().unwrap().iter().any(|e| e[0] == "dkc"), "{after}");
}

/// An animated GIF of two frames.
fn gif() -> Vec<u8> {
    use image::{Delay, Frame, Rgba, RgbaImage};
    let frame = |c: [u8; 4]| Frame::from_parts(RgbaImage::from_pixel(8, 8, Rgba(c)), 0, 0, Delay::from_numer_denom_ms(100, 1));
    let mut gif = Vec::new();
    image::codecs::gif::GifEncoder::new(&mut gif).encode_frames([frame([255, 0, 0, 255]), frame([0, 0, 255, 255])]).unwrap();
    gif
}

/// Pausing an animated GIF (media.gifPlay) is not an undo step. A failing batch leaves each GIF
/// playing or paused, as it was before the batch.
#[test]
fn a_failing_batch_leaves_gifs_playing_or_paused_as_they_were() {
    let mut s = blank();
    let id = s.execute("insert.picture", &json!({"name": "a.gif", "data": base64_encode(&gif())})).unwrap()["id"].clone();
    let pause = json!({"command": "media.gifPlay", "params": {"id": id, "play": false}});
    let e = s.execute("edit.batch", &json!({"commands": [pause, bad_rename()]})).unwrap_err().to_string();
    assert!(e.contains("step 1 (`shape.rename`)"), "{e}");
    assert!(s.gif_paused.is_empty(), "the failed batch paused the GIF");
    // Paused before the batch, it stays paused when a step of the failing batch plays it.
    s.execute("media.gifPlay", &json!({"id": id, "play": false})).unwrap();
    let play = json!({"command": "media.gifPlay", "params": {"id": id, "play": true}});
    assert!(s.execute("edit.batch", &json!({"commands": [play, bad_rename()]})).is_err());
    assert_eq!(s.gif_paused.len(), 1);
}

/// Edit Points is a mode, not an undo step. A failing batch leaves it on or off, as it was before
/// the batch, with the same point picked.
#[test]
fn a_failing_batch_leaves_edit_points_as_it_was() {
    let mut s = blank();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap();
    // The first Edit Points makes the rectangle a freeform; after that, turning it on changes
    // nothing in the presentation.
    let id = s.execute("shape.editPoints", &json!({"on": true})).unwrap()["editing"].as_u64().unwrap();
    s.execute("shape.editPoints", &json!({"on": false})).unwrap();
    let editing = |s: &Session| s.tool.editing_points(s.doc().unwrap()).map(|x| u64::from(x.0));
    let on = json!({"command": "shape.editPoints", "params": {"on": true}});
    let e = s.execute("edit.batch", &json!({"commands": [on, bad_rename()]})).unwrap_err().to_string();
    assert!(e.contains("step 1 (`shape.rename`)"), "{e}");
    assert_eq!(editing(&s), None, "the failed batch turned Edit Points on");
    // On before the batch with a point picked (a click on it in the app), it stays on with that
    // point when a step of the failing batch turns it off.
    s.execute("shape.editPoints", &json!({"on": true})).unwrap();
    s.tool.edit_vertex = Some((0, 2));
    let off = json!({"command": "shape.editPoints", "params": {"on": false}});
    assert!(s.execute("edit.batch", &json!({"commands": [off, bad_rename()]})).is_err());
    assert_eq!(editing(&s), Some(id));
    assert_eq!(s.tool.edit_vertex, Some((0, 2)));
}

/// Commands that open a dialog or a pane ask the app to (`Session::ui_requests`), and the app acts
/// on the requests after every command, failed or not. A failing batch drops the requests its
/// steps made, so no dialog opens over the presentation it put back.
#[test]
fn a_failing_batch_drops_the_dialogs_its_steps_asked_for() {
    let mut s = blank();
    s.ui_requests.clear();
    let colors = json!({"command": "design.colors"});
    let e = s.execute("edit.batch", &json!({"commands": [colors, bad_rename()]})).unwrap_err().to_string();
    assert!(e.contains("step 1 (`shape.rename`)"), "{e}");
    assert!(s.ui_requests.is_empty(), "the failed batch asked for the Theme Colors dialog: {:?}", s.ui_requests);
    // Requests made before the batch stay, and a batch that goes through keeps its own.
    s.execute("design.colors", &json!({})).unwrap();
    let queued = s.ui_requests.clone();
    assert_eq!(queued.len(), 1);
    assert!(s.execute("edit.batch", &json!({"commands": [colors, bad_rename()]})).is_err());
    assert_eq!(s.ui_requests, queued);
    s.execute("edit.batch", &json!({"commands": [colors]})).unwrap();
    assert_eq!(s.ui_requests.len(), 2);
}

#[test]
fn undo_steps_a_command_pushes_itself_stay_inside_the_batch() {
    let mut s = blank();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 300, 80], "text": "Hello world"})).unwrap()["id"].clone();
    let text = |s: &mut Session| s.execute("shape.inspect", &json!({"id": id})).unwrap()["shape"]["text"]["paragraphs"][0]["runs"][0]["text"].clone();
    assert_eq!(text(&mut s), "Hello world");
    let (undo, _) = depth(&mut s);
    // text.deleteSurrounding (an input method's deletion) pushes its own "Delete Text" step.
    let delete = json!([
        {"command": "text.edit", "params": {"id": id, "end": true}},
        {"command": "text.deleteSurrounding", "params": {"before": 6, "after": 0}},
        {"command": "text.exit"},
    ]);
    s.execute("edit.batch", &json!({"commands": delete})).unwrap();
    assert_eq!(text(&mut s), "Hello");
    assert_eq!(depth(&mut s).0, undo + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(text(&mut s), "Hello world");
    assert_eq!(depth(&mut s).0, undo);
    // When a later step fails, the step it pushed goes too.
    let mut failing = delete.as_array().unwrap().clone();
    failing.push(bad_rename());
    let before = snapshot(&s);
    let steps = depth(&mut s);
    assert!(s.execute("edit.batch", &json!({"commands": failing})).is_err());
    assert_eq!(text(&mut s), "Hello world");
    assert_eq!(snapshot(&s), before);
    assert_eq!(depth(&mut s), steps);
}

#[test]
fn refused_and_malformed_batches_run_nothing() {
    let mut s = blank();
    let revision = s.doc().unwrap().revision;
    let shapes = names(&s).len();
    for id in [
        "file.save",
        "file.new",
        "window.next",
        "edit.undo",
        "edit.redo",
        "edit.batch",
        "show.fromStart",
        "show.fromCurrent",
        "text.compose",
        "text.commit",
    ] {
        let e = s.execute("edit.batch", &json!({"commands": [rect(20.0), {"command": id}]})).unwrap_err().to_string();
        assert!(e.contains(&format!("step 1: `{id}` cannot run in a batch")), "{id}: {e}");
    }
    let many = vec![json!({"command": "edit.deselect"}); 1001];
    let cases = [
        (json!({}), "missing `commands`"),
        (json!({"commands": "shape.insert"}), "missing `commands`"),
        (json!({"commands": [rect(20.0), {"command": "nope.nothing"}]}), "step 1: unknown command `nope.nothing`"),
        (json!({"commands": [rect(20.0), {"params": {}}]}), "step 1: missing `command`"),
        (json!({"commands": [rect(20.0), {"command": 7}]}), "step 1: missing `command`"),
        (json!({"commands": many}), "1001 commands; one batch runs at most 1000"),
    ];
    for (p, want) in cases {
        let e = s.execute("edit.batch", &p).unwrap_err().to_string();
        assert!(e.contains(want), "{p}: {e}");
    }
    // Refused before the first step ran: not even a rollback happened.
    assert_eq!(s.doc().unwrap().revision, revision);
    assert_eq!(names(&s).len(), shapes);
    // An empty batch is a no-op that adds no undo step.
    let (undo, _) = depth(&mut s);
    assert_eq!(s.execute("edit.batch", &json!({"commands": []})).unwrap()["results"], json!([]));
    assert_eq!(depth(&mut s).0, undo);
}

#[test]
fn document_inspect_counts_undo_and_redo_steps() {
    let mut s = blank();
    let (undo, redo) = depth(&mut s);
    assert_eq!(redo, 0);
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 50, 100, 80]})).unwrap();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [140, 50, 100, 80]})).unwrap();
    assert_eq!(depth(&mut s), (undo + 2, 0));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(depth(&mut s), (undo + 1, 1));
}
