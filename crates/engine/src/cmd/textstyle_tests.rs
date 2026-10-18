//! master.textStyle: editing the master's text styles restyles the text that inherits them, as
//! one undo step, and survives PPTX.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::json;

use crate::Session;

/// A new deck with text in its title placeholder, which inherits the master's title style.
fn deck() -> (Session, u32) {
    let mut s = Session::with_new();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.set", &json!({"id": title, "text": "Hello"})).unwrap();
    (s, title)
}

fn title_state(s: &mut Session, id: u32) -> serde_json::Value {
    s.execute("selection.select", &json!({"ids": [id]})).unwrap();
    s.execute("format.state", &json!({})).unwrap()
}

#[test]
fn editing_the_title_style_restyles_inheriting_titles() {
    let (mut s, title) = deck();
    s.execute("master.textStyle", &json!({"style": "title", "font": "Georgia", "bold": true, "color": "#336699"})).unwrap();
    let st = title_state(&mut s, title);
    assert_eq!(st["font"], "Georgia");
    assert_eq!(st["bold"], true);
    let styles = s.execute("master.textStyles", &json!({})).unwrap();
    assert_eq!(styles["title"][0]["run"]["font"], "Georgia");
}

#[test]
fn layouts_keep_their_overrides_unless_asked() {
    let (mut s, title) = deck();
    // The Title Slide layout makes its title 60 pt; the master's title style is 44.
    s.execute("master.textStyle", &json!({"style": "title", "size": 50})).unwrap();
    assert_eq!(title_state(&mut s, title)["size"], 60.0, "the layout's size wins, as in PowerPoint");
    s.execute("master.textStyle", &json!({"style": "title", "size": 50, "layouts": true})).unwrap();
    assert_eq!(title_state(&mut s, title)["size"], 50.0, "with layouts: true the override is cleared");
}

#[test]
fn text_with_its_own_formatting_keeps_it() {
    let (mut s, title) = deck();
    s.execute("selection.select", &json!({"ids": [title]})).unwrap();
    s.execute("format.font", &json!({"family": "Courier New"})).unwrap();
    s.execute("master.textStyle", &json!({"style": "title", "font": "Georgia"})).unwrap();
    assert_eq!(title_state(&mut s, title)["font"], "Courier New");
}

#[test]
fn body_levels_and_line_spacing() {
    let (mut s, _) = deck();
    s.execute("master.textStyle", &json!({"style": "body", "level": 2, "size": 21, "lineSpacing": {"lines": 1.2}})).unwrap();
    let styles = s.execute("master.textStyles", &json!({})).unwrap();
    assert_eq!(styles["body"][1]["run"]["size"], 21.0);
    assert_eq!(styles["body"][1]["para"]["lineSpacing"]["v"], 1.2);
    assert_ne!(styles["body"][0]["run"]["size"], 21.0, "level 1 is untouched");
}

#[test]
fn one_undo_step_and_bad_parameters() {
    let (mut s, title) = deck();
    let before = title_state(&mut s, title)["font"].clone();
    let n = s.doc().unwrap().history.undo.len();
    s.execute("master.textStyle", &json!({"style": "title", "font": "Georgia", "size": 40})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), n + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(title_state(&mut s, title)["font"], before);
    for bad in [
        json!({"style": "heading", "size": 20}),
        json!({"style": "title"}),
        json!({"style": "body", "level": 10, "size": 20}),
        json!({"style": "title", "lineSpacing": {"lines": 0}}),
        json!({"style": "title", "color": "not a colour"}),
    ] {
        assert!(s.execute("master.textStyle", &bad).is_err(), "{bad}");
    }
}

#[test]
fn edited_styles_survive_pptx() {
    let (mut s, _) = deck();
    s.execute("master.textStyle", &json!({"style": "title", "font": "Georgia", "size": 48, "bold": true})).unwrap();
    let data = s.execute("file.saveBytes", &json!({"format": "pptx"})).unwrap()["data"].clone();
    s.execute("file.openBytes", &json!({"name": "styles.pptx", "data": data})).unwrap();
    let styles = s.execute("master.textStyles", &json!({})).unwrap();
    assert_eq!(styles["title"][0]["run"]["font"], "Georgia");
    assert_eq!(styles["title"][0]["run"]["size"], 48.0);
    assert_eq!(styles["title"][0]["run"]["bold"], true);
}

#[test]
fn the_default_style_restyles_text_boxes() {
    let mut s = Session::with_new();
    // A text box (a shape with a style keeps its theme font reference, which wins in PPTX).
    let id = s.execute("insert.textBox", &json!({"rect": [100, 100, 300, 100], "text": "Box"})).unwrap()["id"].as_u64().unwrap();
    s.execute("master.textStyle", &json!({"style": "default", "font": "Georgia", "size": 22})).unwrap();
    s.execute("selection.select", &json!({"ids": [id]})).unwrap();
    let st = s.execute("format.state", &json!({})).unwrap();
    assert_eq!((st["font"].as_str(), st["size"].as_f64()), (Some("Georgia"), Some(22.0)));
    let styles = s.execute("master.textStyles", &json!({})).unwrap();
    assert_eq!(styles["default"][0]["run"]["size"], 22.0);
}
