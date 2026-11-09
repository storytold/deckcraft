//! Format Shape ▸ Effects (#1356): Reflection "None" clears the effect after another preset.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};
use deckcraft_engine::Session;

fn has_reflection(h: &Harness<'_, SlideApp>) -> bool {
    let st = h.state().session.active().unwrap();
    let id = *st.selection.shapes.first().unwrap();
    st.shape(id).unwrap().effects.as_ref().is_some_and(|e| e.reflection.is_some())
}

#[test]
fn reflection_none_clears_after_a_second_preset() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [72.0, 36.0, 144.0, 108.0]})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.pane = Some("format".into());
    app.ui.format_tab = "effects".into();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    // The Shadow row has a "None" too; Reflection's is the later one.
    for preset in ["Tight", "Half", "Full"] {
        h.get_by_label(preset).click();
        h.run_steps(4);
        assert!(has_reflection(&h), "{preset} adds a reflection");
        h.get_all_by_label("None").last().unwrap().click();
        h.run_steps(4);
        assert!(!has_reflection(&h), "None clears the {preset} reflection");
    }
}

#[test]
fn reflection_none_stays_cleared_when_a_slider_only_had_focus() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [72.0, 36.0, 144.0, 108.0]})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.pane = Some("format".into());
    app.ui.format_tab = "effects".into();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("Tight").click();
    h.run_steps(4);
    // Focus a reflection slider without editing it, then click "None": the slider's value must not
    // be written back over the cleared reflection.
    // The reflection's Size comes before the glow's (the same label).
    h.get_all_by_role_and_label(egui::accesskit::Role::Slider, "Size").next().unwrap().focus();
    h.run_steps(4);
    assert!(h.get_all_by_role_and_label(egui::accesskit::Role::Slider, "Size").next().unwrap().is_focused(), "the reflection size slider has focus");
    h.get_all_by_label("None").last().unwrap().click();
    h.run_steps(4);
    assert!(!has_reflection(&h), "a slider that only had focus does not re-add the reflection");
}
