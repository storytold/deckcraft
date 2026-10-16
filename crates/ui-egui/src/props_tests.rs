//! The property editor: its sections, and that its controls run the commands they name.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use deckcraft_model::{Fill, ShapeId};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use super::*;
use crate::{Services, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.run("view.shell", json!({"shell": "canvas"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1600.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

/// A blue rectangle with text, selected.
fn select_box(h: &mut Harness<'static, SlideApp>, preset: &str) -> u32 {
    let app = h.state_mut();
    let id = app.session.execute("shape.insert", &json!({"preset": preset, "rect": [100, 100, 200, 100], "text": "Hi"})).unwrap()["id"]
        .as_u64()
        .unwrap() as u32;
    app.session.execute("selection.select", &json!({"ids": [id]})).unwrap();
    app.session.execute("shape.fill", &json!({"color": "#3366CC"})).unwrap();
    h.run_steps(3);
    id
}

fn shape(app: &SlideApp, id: u32) -> Shape {
    app.session.active().unwrap().shape(ShapeId(id)).cloned().unwrap()
}

fn click(h: &mut Harness<'static, SlideApp>, label: &str) {
    h.get_by_label(label).click();
    h.run_steps(3);
}

#[test]
fn a_shape_shows_every_section() {
    let mut h = harness();
    select_box(&mut h, "rect");
    for label in [
        "Rectangle",
        "Position",
        "Layout",
        "Appearance",
        "Typography",
        "Fill",
        "Stroke",
        "Effects",
        "Export",
        "Link",
        "Align left",
        "Constrain proportions",
    ] {
        assert!(h.query_all_by_label(label).next().is_some(), "`{label}` is shown");
    }
    // Numbers in pixels, without a unit: x 100 pt is 200, the fill is 3366CC at 100%.
    for value in ["200", "400", "3366CC", "100%"] {
        assert!(h.query_all_by_value(value).next().is_some(), "a field reads {value}");
    }
}

#[test]
fn position_buttons_align_rotate_and_flip() {
    let mut h = harness();
    let id = select_box(&mut h, "rect");
    click(&mut h, "Align left");
    assert_eq!(shape(h.state(), id).xfrm.unwrap().x, 0.0);
    click(&mut h, "Flip horizontal");
    assert!(shape(h.state(), id).xfrm.unwrap().flip_h);
    click(&mut h, "Rotate 90° right");
    assert_eq!(shape(h.state(), id).xfrm.unwrap().rot, 90.0);
}

#[test]
fn resizing_buttons_set_wrap_and_autofit() {
    let mut h = harness();
    let id = select_box(&mut h, "rect");
    click(&mut h, "Auto width");
    assert_eq!(resizing_of(&shape(h.state(), id)), Resizing::AutoWidth);
    click(&mut h, "Auto height");
    assert_eq!(resizing_of(&shape(h.state(), id)), Resizing::AutoHeight);
    click(&mut h, "Fixed size");
    assert_eq!(resizing_of(&shape(h.state(), id)), Resizing::Fixed);
}

#[test]
fn stroke_and_effects_are_added_and_removed() {
    let mut h = harness();
    let id = select_box(&mut h, "rect");
    click(&mut h, "Remove stroke");
    assert!(matches!(shape(h.state(), id).line.and_then(|l| l.fill), Some(Fill::None)));
    click(&mut h, "Add stroke");
    assert!(matches!(shape(h.state(), id).line.and_then(|l| l.fill), Some(Fill::Solid { .. })));
    click(&mut h, "Add effect");
    click(&mut h, "Drop shadow");
    assert!(shape(h.state(), id).effects.and_then(|e| e.outer_shadow).is_some());
    click(&mut h, "Remove drop shadow");
    assert!(shape(h.state(), id).effects.and_then(|e| e.outer_shadow).is_none());
}

#[test]
fn a_text_box_fill_is_its_text_colour() {
    let mut h = harness();
    let app = h.state_mut();
    let id = app.session.execute("insert.textBox", &json!({"rect": [100, 100, 300, 80], "text": "Words"})).unwrap()["id"].as_u64().unwrap();
    app.session.execute("selection.select", &json!({"ids": [id]})).unwrap();
    app.run("format.color", json!({"color": "#AA2200"})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Text").is_some(), "the header names a text layer");
    assert!(h.query_all_by_value("AA2200").next().is_some(), "Fill shows the text colour");
}

#[test]
fn type_details_open_from_the_tune_button() {
    let mut h = harness();
    select_box(&mut h, "rect");
    assert!(h.query_by_label("Bold (⌘B)").is_none(), "details start folded");
    click(&mut h, "Type details: font, weight, size, spacing");
    click(&mut h, "Bold (⌘B)");
    let st = h.state_mut().session.execute("format.state", &json!({})).unwrap();
    assert_eq!(st["bold"], true);
}

#[test]
fn model_helpers() {
    assert_eq!(parse_hex("fff").as_deref(), Some("#FFFFFF"));
    assert_eq!(parse_hex("#3366cc").as_deref(), Some("#3366CC"));
    assert_eq!(parse_hex("33x"), None);
    assert_eq!(parse_hex("12345"), None);
    let mut s = Session::with_new();
    let id = s.execute("shape.insert", &json!({"preset": "roundRect", "rect": [0, 0, 200, 100]})).unwrap()["id"].as_u64().unwrap();
    let st = s.doc().unwrap();
    let sh = st.shape(ShapeId(id as u32)).unwrap().clone();
    let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, &sh);
    let r = corner_radius(&sh, &x).unwrap();
    assert!((r - 16.667).abs() < 0.01, "{r}: 16.667% of the short side");
    assert_eq!(kind_label(&sh), "Rectangle: Rounded Corners", "the shape catalog's name");
    let sc = deckcraft_model::Theme::default().colors;
    assert_eq!(opacity_of(&sh, &sc), 1.0);
}
