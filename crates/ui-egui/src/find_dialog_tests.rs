//! Home › Find (Ctrl+F): "Find what:" has the keyboard as soon as the dialog opens (#951).

use deckcraft_engine::Session;
use egui_kittest::Harness;
use serde_json::json;

use crate::{Services, SlideApp};

#[test]
fn ctrl_f_puts_the_caret_in_find_what() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("insert.textBox", &json!({"rect": [40, 40, 400, 80], "text": "Hello there"})).unwrap();
    s.execute("text.exit", &json!({})).unwrap();
    s.doc_mut().unwrap().selection.shapes.clear();
    let app = SlideApp::new(s, Services::default());
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("find"));
    // Typing right away goes into the field, without clicking it first.
    h.event(egui::Event::Text("there".into()));
    h.run_steps(3);
    let typed = h.state().dialog.as_ref().and_then(|d| d.fields.get("text").cloned());
    assert_eq!(typed.as_deref(), Some("there"));
    // Enter finds it.
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    let status = h.state().dialog.as_ref().and_then(|d| d.fields.get("status").cloned());
    assert!(status.as_deref().is_some_and(|m| m.starts_with("Match 1 of 1")), "Enter didn't find the text: {status:?}");
    // The keyboard stays in the field afterwards: more typing adds to it.
    h.event(egui::Event::Text("!".into()));
    h.run_steps(3);
    let typed = h.state().dialog.as_ref().and_then(|d| d.fields.get("text").cloned());
    assert_eq!(typed.as_deref(), Some("there!"));
}
