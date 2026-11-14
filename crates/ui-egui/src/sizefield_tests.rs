//! Home › Font Size: the size is a field the person can type in, as well as a list to pick from.

use deckcraft_engine::Session;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::Node;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("insert.textBox", &json!({"rect": [40, 40, 400, 80], "text": "Hello size"})).unwrap();
    s.execute("text.exit", &json!({})).unwrap();
    s.execute("format.size", &json!({"size": 40})).unwrap();
    let app = SlideApp::new(s, Services::default());
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

/// The size field: the text input showing the selection's size (the list's buttons and the
/// field's own text run carry the same text, so the role picks it out).
fn size_field<'a>(h: &'a Harness<'_, SlideApp>, shown: &str) -> Node<'a> {
    h.query_all_by_role(Role::TextInput).find(|n| n.value().as_deref() == Some(shown)).expect("the size field")
}

fn run_size(h: &Harness<'_, SlideApp>) -> Option<f64> {
    let st = h.state().session.active().unwrap();
    st.current_slide()
        .unwrap()
        .shapes
        .iter()
        .find_map(|s| s.text.as_ref())
        .and_then(|b| b.paragraphs.first())
        .and_then(|p| p.runs.first())
        .and_then(|r| r.props.size)
}

#[test]
fn a_typed_size_is_applied_on_enter() {
    let mut h = harness();
    assert_eq!(run_size(&h), Some(40.0));
    size_field(&h, "40").click();
    h.run_steps(2);
    size_field(&h, "40").type_text("37");
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(run_size(&h), Some(37.0), "the size typed in the field is applied");
}

/// Tab moves on and keeps what was typed, as Enter does.
#[test]
fn a_typed_size_is_applied_when_tab_moves_on() {
    let mut h = harness();
    size_field(&h, "40").click();
    h.run_steps(2);
    size_field(&h, "40").type_text("29");
    h.run_steps(2);
    h.key_press(egui::Key::Tab);
    h.run_steps(3);
    assert_eq!(run_size(&h), Some(29.0), "the size typed in the field is applied on Tab");
}

#[test]
fn text_that_is_not_a_size_changes_nothing_and_says_so() {
    let mut h = harness();
    size_field(&h, "40").click();
    h.run_steps(2);
    size_field(&h, "40").type_text("big");
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(run_size(&h), Some(40.0));
    assert!(h.state().status.as_ref().is_some_and(|(m, _)| m.contains("isn't a font size")), "{:?}", h.state().status);
}

#[test]
fn escape_drops_what_was_typed() {
    let mut h = harness();
    size_field(&h, "40").click();
    h.run_steps(2);
    size_field(&h, "40").type_text("99");
    h.run_steps(2);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert_eq!(run_size(&h), Some(40.0));
}
