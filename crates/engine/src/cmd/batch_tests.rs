//! `edit.batch`: several commands as one step. An agent that builds a slide in several calls
//! leaves it half-built when a later call fails; a batch makes every change or none, and one undo
//! takes all of them back. `document.inspect` reports how many undo and redo steps there are, so
//! an agent can check that.

use serde_json::{Value, json};

use crate::Session;

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
    assert!(e.contains("nothing changed"), "{e}");
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
