//! Text styles: read from the deck, applied as a link where PPTX has one (a copy elsewhere),
//! edited for the whole deck in one undo step.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use serde_json::json;

use super::*;
use crate::{Services, SlideApp};

fn app() -> SlideApp {
    SlideApp::new(Session::with_new(), Services::default())
}

fn style(app: &SlideApp, name: &str) -> TextStyle {
    styles(&app.session.active().unwrap().doc).into_iter().find(|s| s.name == name).unwrap()
}

fn state(app: &mut SlideApp, id: u64) -> Value {
    app.session.execute("selection.select", &json!({"ids": [id]})).unwrap();
    app.session.execute("format.state", &json!({})).unwrap()
}

/// The title placeholder of the first slide, with text in it.
fn title(app: &mut SlideApp) -> u64 {
    let id = app.session.active().unwrap().current_slide().unwrap().shapes[0].id.0 as u64;
    app.session.execute("text.set", &json!({"id": id, "text": "Hello"})).unwrap();
    id
}

fn text_box(app: &mut SlideApp) -> u64 {
    app.session.execute("insert.textBox", &json!({"rect": [100, 300, 300, 80], "text": "Box"})).unwrap()["id"].as_u64().unwrap()
}

#[test]
fn the_deck_has_title_body_levels_and_plain_text() {
    let a = app();
    let st = styles(&a.session.active().unwrap().doc);
    let names: Vec<_> = st.iter().map(|s| (s.name, s.group, s.level)).collect();
    assert_eq!(names, [("Title", "title", 0), ("Body", "body", 0), ("Body 2", "body", 1), ("Body 3", "body", 2), ("Plain text", "default", 0)]);
    assert!(st[0].size() > st[1].size() && st[1].size() > st[2].size());
    assert_eq!(st[0].theme_font(), Some(true), "titles use the theme's heading font");
}

#[test]
fn applying_title_to_a_title_links_it_and_style_edits_reach_it() {
    let mut a = app();
    let id = title(&mut a);
    a.session.execute("selection.select", &json!({"ids": [id]})).unwrap();
    a.run("format.font", json!({"family": "Courier New"})).unwrap();
    let t = style(&a, "Title");
    assert!(links(&a, &t));
    apply(&mut a, &t);
    assert_eq!(state(&mut a, id)["fontRef"], "+mj-lt", "its own formatting is cleared");
    let st = state(&mut a, id);
    assert_eq!(current(&a, &styles(&a.session.active().unwrap().doc), &st).as_deref(), Some("Title"));
    // Edit the style: bold, and the theme heading font.
    edit_style(&mut a, &t, &Edit::Bold(true));
    let s = style(&a, "Title");
    edit_style(&mut a, &s, &Edit::Font("Georgia".into()));
    let st = state(&mut a, id);
    assert_eq!((st["bold"].as_bool(), st["font"].as_str()), (Some(true), Some("Georgia")));
}

#[test]
fn applying_title_to_a_text_box_copies_it_and_style_edits_update_the_copy() {
    let mut a = app();
    let boxed = text_box(&mut a);
    a.session.execute("selection.select", &json!({"ids": [boxed]})).unwrap();
    let t = style(&a, "Title");
    assert!(!links(&a, &t), "PPTX can't link a text box to the title style");
    apply(&mut a, &t);
    let st = state(&mut a, boxed);
    assert_eq!(st["size"], t.size());
    assert_eq!(current(&a, &styles(&a.session.active().unwrap().doc), &st).as_deref(), Some("Title (copy)"));
    // Editing the style's size updates the copy too, in one undo step, and keeps the selection.
    a.run("edit.deselect", json!({})).unwrap();
    let undo0 = a.session.active().unwrap().history.undo.len();
    let s = style(&a, "Title");
    edit_style(&mut a, &s, &Edit::Size(50.0));
    let d = a.session.active().unwrap();
    assert_eq!(d.history.undo.len(), undo0 + 1);
    assert!(d.selection.shapes.is_empty(), "the selection is put back");
    assert_eq!(state(&mut a, boxed)["size"], 50.0, "the copy follows");
    a.run("edit.undo", json!({})).unwrap();
    assert_eq!(state(&mut a, boxed)["size"], t.size(), "one undo takes back the style and its copies");
}

#[test]
fn plain_text_restyles_text_boxes() {
    let mut a = app();
    let boxed = text_box(&mut a);
    let s = style(&a, "Plain text");
    edit_style(&mut a, &s, &Edit::Size(23.0));
    assert_eq!(state(&mut a, boxed)["size"], 23.0);
}

#[test]
fn body_levels_set_the_list_level() {
    let mut a = app();
    a.run("slide.new", json!({})).unwrap();
    let body = a.session.active().unwrap().current_slide().unwrap().shapes.iter().find(|s| group_of(s) == Some("body")).unwrap().id.0 as u64;
    a.session.execute("text.set", &json!({"id": body, "text": "Point"})).unwrap();
    a.session.execute("selection.select", &json!({"ids": [body]})).unwrap();
    let s = style(&a, "Body 2");
    apply(&mut a, &s);
    let d = a.session.active().unwrap();
    let level = d.shape(ShapeId(body as u32)).unwrap().text.as_ref().unwrap().paragraphs[0].level;
    assert_eq!(level, 1);
    let st = state(&mut a, body);
    assert_eq!(current(&a, &styles(&a.session.active().unwrap().doc), &st).as_deref(), Some("Body 2"));
}

#[test]
fn custom_formatting_matches_no_style() {
    let mut a = app();
    let boxed = text_box(&mut a);
    a.session.execute("selection.select", &json!({"ids": [boxed]})).unwrap();
    a.run("format.size", json!({"size": 37.0})).unwrap();
    a.run("format.font", json!({"family": "Courier New"})).unwrap();
    let st = state(&mut a, boxed);
    assert_eq!(current(&a, &styles(&a.session.active().unwrap().doc), &st), None);
}
