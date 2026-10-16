//! States: Morph chains, adding a state, what changes between states, and the Animate tab.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use super::*;
use crate::{Services, SlideApp};

/// A one-slide deck with a named box on it, selected.
fn app_with_box() -> (SlideApp, u32) {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    let id = app.session.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 120, 80]})).unwrap()["id"].as_u64().unwrap() as u32;
    app.session.execute("selection.select", &json!({"ids": [id]})).unwrap();
    app.run("shape.rename", json!({"name": "Card"})).unwrap();
    (app, id)
}

fn doc(app: &SlideApp) -> std::sync::Arc<Presentation> {
    app.session.active().unwrap().doc.clone()
}

/// The shape named `name` on slide `i`.
fn named(app: &SlideApp, i: usize, name: &str) -> u32 {
    doc(app).slides[i].shapes.iter().find(|s| s.name == name).unwrap().id.0
}

#[test]
fn adding_a_state_duplicates_the_slide_with_morph_in_one_undo_step() {
    let (mut app, _) = app_with_box();
    let undo0 = app.session.active().unwrap().history.undo.len();
    add_state(&mut app);
    let d = doc(&app);
    assert_eq!(d.slides.len(), 2);
    assert_eq!(app.session.active().unwrap().selection.slide, 1, "the new state is current");
    assert!(is_state_step(&d, 1));
    assert_eq!(run_of(&d, 0), 0..2);
    assert_eq!(run_of(&d, 1), 0..2);
    let st = app.session.active().unwrap();
    assert_eq!(st.history.undo.len(), undo0 + 1);
    assert_eq!(st.history.undo.last().unwrap().label, "Add State");
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(doc(&app).slides.len(), 1, "one undo removes the state");
}

#[test]
fn runs_stop_at_slides_without_morph() {
    let (mut app, _) = app_with_box();
    add_state(&mut app);
    add_state(&mut app);
    app.run("slide.new", json!({})).unwrap();
    let d = doc(&app);
    assert_eq!(run_of(&d, 1), 0..3);
    assert_eq!(run_of(&d, 3), 3..4, "a plain slide is a run of one");
    assert!(!is_state_step(&d, 0) && !is_state_step(&d, 3));
}

#[test]
fn changes_name_what_morph_will_animate() {
    let (mut app, _) = app_with_box();
    app.session.execute("shape.insert", &json!({"preset": "ellipse", "rect": [400, 100, 50, 50]})).unwrap();
    app.run("shape.rename", json!({"name": "Dot"})).unwrap();
    add_state(&mut app);
    let card = named(&app, 1, "Card");
    let dot = named(&app, 1, "Dot");
    app.session.execute("shape.move", &json!({"ids": [card], "x": 300, "y": 200})).unwrap();
    app.session.execute("shape.fill", &json!({"ids": [card], "color": "#FF0000"})).unwrap();
    app.session.execute("selection.select", &json!({"ids": [dot]})).unwrap();
    app.run("edit.delete", json!({})).unwrap();
    app.session.execute("shape.insert", &json!({"preset": "star5", "rect": [50, 300, 60, 60]})).unwrap();
    let d = doc(&app);
    let list = changes(&d.slides[0], &d.slides[1]);
    let of = |name: &str| list.iter().find(|c| c.name == name).map(|c| c.changes.clone());
    assert_eq!(of("Card"), Some(vec![Change::Moved, Change::Restyled]));
    assert_eq!(of("Dot"), Some(vec![Change::Leaves]));
    assert!(list.iter().any(|c| c.changes == [Change::Enters]), "the star fades in");
    // The Layers list marks the card (and the star), not the layer that left.
    app.run("slide.go", json!({"index": 1})).unwrap();
    let marked = changed_layers(&app);
    assert!(marked.contains(&card));
    assert_eq!(marked.len(), 2);
}

#[test]
fn hiding_with_the_eye_comes_with_a_warning() {
    let (mut app, _) = app_with_box();
    add_state(&mut app);
    let card = named(&app, 1, "Card");
    app.session.execute("shape.visible", &json!({"ids": [card], "visible": false})).unwrap();
    let d = doc(&app);
    let list = changes(&d.slides[0], &d.slides[1]);
    assert_eq!(list[0].changes, [Change::Hidden]);
    assert!(list[0].warning().is_some());
}

#[test]
fn states_survive_a_pptx_round_trip() {
    let (mut app, _) = app_with_box();
    add_state(&mut app);
    let card = named(&app, 1, "Card");
    app.session.execute("shape.move", &json!({"ids": [card], "x": 300, "y": 200})).unwrap();
    let data = app.session.execute("file.saveBytes", &json!({"format": "pptx"})).unwrap()["data"].clone();
    app.session.execute("file.openBytes", &json!({"name": "states.pptx", "data": data})).unwrap();
    let d = doc(&app);
    assert_eq!(run_of(&d, 0), 0..2, "the Morph link is kept");
    let list = changes(&d.slides[0], &d.slides[1]);
    assert_eq!(list.iter().find(|c| c.name == "Card").map(|c| c.changes.clone()), Some(vec![Change::Moved]));
}

#[test]
fn the_animate_tab_adds_and_walks_states() {
    let (mut app, _) = app_with_box();
    app.run("view.shell", json!({"shell": "canvas"})).unwrap();
    app.ui.inspector_tab = "animate".into();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("+ Add state").click();
    h.run_steps(4);
    assert_eq!(doc(h.state()).slides.len(), 2);
    assert!(h.query_by_label("State 2").is_some());
    assert!(h.query_by_label("Transition from the previous state").is_some());
    h.get_by_label("Words").click();
    h.run_steps(3);
    assert_eq!(doc(h.state()).slides[1].transition.as_ref().unwrap().option, "words");
    h.get_by_label("State 1").click();
    h.run_steps(3);
    assert_eq!(h.state().session.active().unwrap().selection.slide, 0);
}
