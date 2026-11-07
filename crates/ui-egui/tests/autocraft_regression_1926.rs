//! #1926: Ctrl+= (Subscript) and Ctrl+Shift+= (Superscript) zoomed or twitched the screen instead.

use deckcraft_engine::Session;
use deckcraft_ui_egui::{Services, SlideApp};
use egui::{Key, Modifiers};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 320.0, 80.0], "text": "Hello"})).unwrap();
    // A selected shape, not a caret: the shortcuts then format the text that is already there.
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

fn baseline_of(h: &Harness<'_, SlideApp>) -> Option<f64> {
    let st = h.state().session.active().unwrap();
    st.selected_shapes()
        .first()
        .and_then(|sh| sh.text.as_ref())
        .and_then(|t| t.paragraphs.first())
        .and_then(|p| p.runs.first())
        .and_then(|r| r.props.baseline)
}

#[test]
fn ctrl_equals_subscripts_without_zooming() {
    let mut h = harness();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Equals);
    h.run_steps(1);
    assert_eq!(baseline_of(&h), Some(-0.25), "status {:?}", h.state().status);
    assert!(!h.ctx.options(|o| o.zoom_with_keyboard), "egui's own keyboard zoom must not act on Ctrl+=");
    assert_eq!(h.ctx.zoom_factor(), 1.0);
}

#[test]
fn ctrl_shift_equals_superscripts_without_zooming() {
    let mut h = harness();
    // Ctrl+Shift+= is the shifted character, so egui reports it as `Plus`.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Plus);
    h.run_steps(1);
    assert_eq!(baseline_of(&h), Some(0.3), "status {:?}", h.state().status);
    assert!(!h.ctx.options(|o| o.zoom_with_keyboard), "egui's own keyboard zoom must not act on Ctrl+Shift+=");
    assert_eq!(h.ctx.zoom_factor(), 1.0);
}
