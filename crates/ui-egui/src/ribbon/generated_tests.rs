//! Insert ▸ Harvey Ball and Checkbox, and the Shape Format controls shown while one is selected
//! (deckcraft#2127).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use deckcraft_model::{CheckState, Generated};
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn harness(s: Session, tab: &str) -> Harness<'static, SlideApp> {
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = tab.into();
    let mut h = Harness::builder().with_size(egui::vec2(2200.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn blank() -> Session {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s
}

/// The element the app has selected.
fn selected(h: &Harness<'static, SlideApp>) -> Generated {
    let st = h.state().session.active().unwrap();
    deckcraft_engine::cmd::generated::selected(st).expect("an element is selected").1
}

fn click(h: &mut Harness<'static, SlideApp>, role: Role, name: &str) {
    h.get_by_role_and_label(role, name).click();
    h.run_steps(3);
}

#[test]
fn insert_menus_add_a_ball_and_a_checkbox() {
    let mut h = harness(blank(), "Insert");
    click(&mut h, Role::Button, "Harvey Ball options");
    click(&mut h, Role::Button, "3/4");
    assert!(matches!(selected(&h), Generated::HarveyBall(b) if (b.value, b.of) == (3, 4)));
    click(&mut h, Role::Button, "Checkbox options");
    click(&mut h, Role::Button, "Crossed");
    assert!(matches!(selected(&h), Generated::Checkbox(c) if c.state == CheckState::Crossed));
    // The main halves insert the defaults.
    click(&mut h, Role::Button, "Harvey Ball");
    assert!(matches!(selected(&h), Generated::HarveyBall(b) if (b.value, b.of) == (1, 4)));
}

#[test]
fn shape_format_edits_the_selected_ball() {
    let mut s = blank();
    s.execute("insert.harveyBall", &json!({"value": 1, "of": 4})).unwrap();
    let mut h = harness(s, "Shape Format");
    click(&mut h, Role::Button, "Fill More");
    assert!(matches!(selected(&h), Generated::HarveyBall(b) if b.value == 2));
    click(&mut h, Role::Button, "Fill Less");
    click(&mut h, Role::Button, "Fill Less");
    assert!(matches!(selected(&h), Generated::HarveyBall(b) if b.value == 0));
    // Parts: the value is kept within the new number of parts.
    h.state_mut().run("harveyBall.set", json!({"value": 4})).unwrap();
    h.run_steps(3);
    click(&mut h, Role::ComboBox, "Parts");
    click(&mut h, Role::Button, "3/3");
    assert!(matches!(selected(&h), Generated::HarveyBall(b) if (b.value, b.of) == (3, 3)));
    // No outline from the outline menu.
    click(&mut h, Role::Button, "Ball Outline");
    click(&mut h, Role::Button, "No Outline");
    assert!(matches!(selected(&h), Generated::HarveyBall(b) if b.outline.is_none()));
}

#[test]
fn shape_format_sets_the_checkbox_state() {
    let mut s = blank();
    s.execute("insert.checkbox", &json!({})).unwrap();
    let mut h = harness(s, "Shape Format");
    click(&mut h, Role::ComboBox, "Checkbox State");
    click(&mut h, Role::Button, "Unchecked");
    assert!(matches!(selected(&h), Generated::Checkbox(c) if c.state == CheckState::Unchecked));
    // Nothing of the sort while a plain shape is selected.
    h.state_mut().run("shape.insert", json!({"preset": "rect", "rect": [10, 10, 50, 50]})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::ComboBox, "Checkbox State").is_none());
}
