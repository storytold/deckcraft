//! The Font dialog: Home › Font › Font… and Cmd+T open it, OK applies one undo step, Cancel does not.

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 320.0, 80.0], "text": "Hello"})).unwrap();
    // A selected shape, not a caret: the dialog then formats the text that is already there.
    let _ = s.execute("text.exit", &json!({}));
    let app = SlideApp::new(s, Services::default());
    let mut h = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn bold_of(h: &Harness<'_, SlideApp>) -> Option<bool> {
    let st = h.state().session.active().unwrap();
    st.selected_shapes()
        .first()
        .and_then(|sh| sh.text.as_ref())
        .and_then(|t| t.paragraphs.first())
        .and_then(|p| p.runs.first())
        .and_then(|r| r.props.bold)
}

#[test]
fn font_button_applies_bold_from_the_dialog() {
    let mut h = harness();
    h.get_by_label("Font…").click();
    // The opening frame ignores the click that summoned the window; the next one is live.
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("font"));
    // A pointer click on this checkbox does not register through kittest's per-event frames;
    // the accessibility click does, and it is the same toggle a pointer click runs.
    h.get_by_label("Bold").click_accesskit();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(1);
    assert!(h.state().dialog.is_none(), "status {:?}", h.state().status);
    assert_eq!(bold_of(&h), Some(true));
    assert_eq!(h.state().session.active().unwrap().history.undo.last().unwrap().label, "Font…");
}

#[test]
fn command_t_opens_the_font_dialog_and_cancel_keeps_the_text() {
    let mut h = harness();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
    h.run_steps(1);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("font"), "status {:?}", h.state().status);
    h.get_by_label("Cancel").click();
    h.run_steps(1);
    assert!(h.state().dialog.is_none());
    assert_eq!(bold_of(&h), None);
}
