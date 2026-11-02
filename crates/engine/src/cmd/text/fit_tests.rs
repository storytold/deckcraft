//! `text.fit`: how text bodies fit their shapes as the slide draws them. An agent filling a
//! placeholder sees `autofit: null` in `shape.inspect` while the layout's shrink-on-overflow draws
//! the text at a fraction of its size; these tests pin what the query reports for that case, for
//! text that fits, for text that overflows a shape that does not shrink, and for bad params.

use serde_json::{Value, json};

use crate::Session;

/// Twenty-five bullets, each long enough to wrap, as an agent pasting an outline writes them.
fn long_outline() -> String {
    (1..=25)
        .map(|i| format!("Point {i}: a sentence long enough to wrap across the full width of the content placeholder"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn fit(s: &mut Session, p: Value) -> Value {
    s.execute("text.fit", &p).unwrap()
}

fn body_named<'a>(r: &'a Value, name: &str) -> &'a Value {
    r["bodies"].as_array().unwrap().iter().find(|b| b["name"] == name).unwrap_or_else(|| panic!("no body {name} in {r}"))
}

fn num(b: &Value, key: &str) -> f64 {
    b[key].as_f64().unwrap_or_else(|| panic!("{key} in {b}"))
}

#[test]
fn an_overfull_placeholder_reports_the_shrink_its_layout_applies() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "Agenda", "body": long_outline()})).unwrap();
    let r = fit(&mut s, json!({}));
    assert_eq!(r["index"], 1);
    let b = body_named(&r, "Content Placeholder 2");
    // The shape sets no autofit of its own: the layout's placeholder shrinks the text.
    assert_eq!(b["autofit"], "shrink", "{b}");
    assert_eq!(b["autofitFrom"], "layout", "{b}");
    assert!(num(b, "fontScale") < 0.7, "{b}");
    assert!(num(b, "lineReduction") > 0.0, "{b}");
    assert!(num(b, "minSize") < 20.0, "{b}");
    assert!(num(b, "minSize") <= num(b, "maxSize"), "{b}");
    assert!(b["lines"].as_u64().unwrap() >= 25, "{b}");
    // Shrunk until the drawing's own overflow test passes, which allows half a point.
    assert_eq!(b["overflows"], false, "{b}");
    assert!(num(b, "overflowPt") <= 0.5, "{b}");
    // The default layout's content placeholder is 343 pt tall, less its top and bottom insets.
    assert!((330.0..=343.0).contains(&num(b, "frameHeight")), "{b}");
    // shape.inspect shows no autofit on the shape itself.
    let own = s.execute("shape.inspect", &json!({"id": b["id"]})).unwrap()["shape"]["text"]["body"].clone();
    assert!(own["autofit"].is_null(), "{own}");
}

#[test]
fn text_that_fits_reports_full_size_and_no_overflow() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "Agenda", "body": "One\nTwo"})).unwrap();
    let r = fit(&mut s, json!({}));
    let b = body_named(&r, "Content Placeholder 2");
    assert_eq!(b["autofit"], "shrink", "{b}");
    assert_eq!(b["fontScale"], 1.0, "{b}");
    assert_eq!(b["lineReduction"], 0.0, "{b}");
    assert_eq!(b["lines"], 2, "{b}");
    assert_eq!(b["overflows"], false, "{b}");
    assert_eq!(b["overflowPt"], 0.0, "{b}");
    assert!(num(b, "minSize") >= 20.0, "{b}");
    assert!(num(b, "contentHeight") < num(b, "frameHeight"), "{b}");
    assert_eq!(body_named(&r, "Title 1")["autofitFrom"], "layout");
}

#[test]
fn text_that_cannot_shrink_reports_how_far_it_overflows() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let long = "A sentence that will not fit in a short box. ".repeat(12);
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 40], "text": long})).unwrap()["id"].clone();
    let r = fit(&mut s, json!({"ids": [id]}));
    let bodies = r["bodies"].as_array().unwrap();
    assert_eq!(bodies.len(), 1, "{r}");
    let b = &bodies[0];
    assert_eq!(b["id"], id);
    assert_eq!(b["autofit"], "none", "{b}");
    assert_eq!(b["autofitFrom"], "default", "{b}");
    assert_eq!(b["fontScale"], 1.0, "{b}");
    assert_eq!(b["overflows"], true, "{b}");
    let (content, frame, over) = (num(b, "contentHeight"), num(b, "frameHeight"), num(b, "overflowPt"));
    assert!(content > frame + 20.0, "{b}");
    assert!(over > 20.0 && over <= content, "{b}");
}

#[test]
fn group_members_are_measured_and_groups_and_tables_left_out() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 80], "text": "A"})).unwrap()["id"].clone();
    let b = s.execute("shape.insert", &json!({"preset": "rect", "rect": [400, 100, 200, 80], "text": "B"})).unwrap()["id"].clone();
    s.execute("edit.select", &json!({"ids": [a, b]})).unwrap();
    let g = s.execute("arrange.group", &json!({})).unwrap()["id"].clone();
    let t = s.execute("insert.table", &json!({"rows": 2, "cols": 2, "rect": [100, 300, 400, 100]})).unwrap()["id"].clone();
    let r = fit(&mut s, json!({}));
    let ids: Vec<Value> = r["bodies"].as_array().unwrap().iter().map(|x| x["id"].clone()).collect();
    assert_eq!(ids, vec![a, b], "{r}");
    for (id, why) in [(g, "no text body"), (t, "no text body")] {
        let e = s.execute("text.fit", &json!({"ids": [id]})).unwrap_err().to_string();
        assert!(e.contains(why), "{e}");
    }
}

#[test]
fn fitting_reads_any_slide_and_changes_nothing() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "One", "body": long_outline()})).unwrap();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let before = {
        let d = s.doc().unwrap();
        (d.history.undo.len(), d.revision, d.selection.slide)
    };
    let r = fit(&mut s, json!({"index": 1}));
    assert_eq!(r["index"], 1);
    assert!(num(body_named(&r, "Content Placeholder 2"), "fontScale") < 0.7, "{r}");
    // The current slide is the blank one: no text bodies.
    assert_eq!(fit(&mut s, json!({}))["bodies"], json!([]));
    let d = s.doc().unwrap();
    assert_eq!((d.history.undo.len(), d.revision, d.selection.slide), before);
}

#[test]
fn bad_slide_indexes_and_shape_ids_are_refused() {
    let mut s = Session::with_new();
    assert_eq!(fit(&mut s, json!({"index": 0}))["index"], 0);
    for p in [
        json!({"index": 1}),
        json!({"index": -1}),
        json!({"index": 0.5}),
        json!({"index": "0"}),
        json!({"ids": "2"}),
        json!({"ids": [-1]}),
        json!({"ids": [1e300]}),
        json!({"ids": [4_294_967_296_u64]}),
        json!({"ids": [99_999]}),
    ] {
        assert!(s.execute("text.fit", &p).is_err(), "{p}");
    }
    assert!(s.execute("text.fit", &json!({"ids": [99_999]})).unwrap_err().to_string().contains("no shape"));
}
