//! The Font dialog: Home › Font › Font… and Cmd+T open it, OK applies one undo step, Cancel does not.

use deckcraft_engine::Session;
use egui::accesskit::Toggled;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
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

fn shadow_of(h: &Harness<'_, SlideApp>) -> bool {
    let st = h.state().session.active().unwrap();
    st.selected_shapes()
        .first()
        .and_then(|sh| sh.text.as_ref())
        .and_then(|t| t.paragraphs.first())
        .and_then(|p| p.runs.first())
        .and_then(|r| r.props.shadow.as_ref())
        .is_some()
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

#[test]
fn font_button_applies_shadow_from_the_dialog() {
    let mut h = harness();
    assert!(!shadow_of(&h));
    h.get_by_label("Font…").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("font"));
    h.get_by_label("Shadow").click_accesskit();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(1);
    assert!(h.state().dialog.is_none());
    assert!(shadow_of(&h));
    assert_eq!(h.state().session.active().unwrap().history.undo.last().unwrap().label, "Font…");
}

#[test]
fn text_shadow_button_on_ribbon_toggles_shadow() {
    let mut h = harness();
    assert!(!shadow_of(&h));
    // The "Text Shadow" toggle button in Home › Font.
    h.get_by_label("Text Shadow").click();
    h.run_steps(1);
    assert!(shadow_of(&h));
    // Clicking again toggles it off.
    h.get_by_label("Text Shadow").click();
    h.run_steps(1);
    assert!(!shadow_of(&h));
}

#[test]
fn inherited_shadow_is_reflected_in_ribbon_and_dialog() {
    let mut s = Session::with_new();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 320.0, 80.0], "text": "Hello"})).unwrap();
    // Set a shadow on the master's body style so this shape inherits it.
    s.edit(|doc, _| {
        doc.default_text_style.set(
            0,
            deckcraft_model::text::LevelStyle {
                run: deckcraft_model::text::RunProps { shadow: Some(deckcraft_engine::cmd::format::default_text_shadow()), ..Default::default() },
                ..Default::default()
            },
        );
        Ok(())
    })
    .unwrap();
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
    // The ribbon button reflects the inherited shadow.
    assert_eq!(h.get_by_label("Text Shadow").accesskit_node().toggled(), Some(Toggled::True));
    // The font dialog reflects the inherited shadow.
    h.get_by_label("Font…").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("font"));
    assert_eq!(h.get_by_label("Shadow").accesskit_node().toggled(), Some(Toggled::True));
}

#[test]
fn font_dialog_preserves_custom_shadow_when_shadow_checkbox_not_touched() {
    let mut h = harness();
    // Turn shadow on and give it custom blur (10.0 pt).
    h.state_mut().session.execute("format.textShadow", &json!({"on": true})).unwrap();
    let sid = h.state().session.active().unwrap().selection.shapes[0];
    h.state_mut()
        .session
        .edit(|doc, _| {
            let slide = std::sync::Arc::make_mut(&mut doc.slides[0]);
            let sh = deckcraft_model::find_shape_mut(&mut slide.shapes, sid).unwrap();
            let r = &mut sh.text.as_mut().unwrap().paragraphs[0].runs[0].props;
            if let Some(s) = &mut r.shadow {
                s.blur = 10.0;
            }
            Ok(())
        })
        .unwrap();
    // Open font dialog and change bold, without touching shadow.
    h.get_by_label("Font…").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("font"));
    h.get_by_label("Bold").click_accesskit();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(1);
    assert!(h.state().dialog.is_none());
    assert_eq!(bold_of(&h), Some(true));
    // The custom shadow blur must remain 10.0 pt, not replaced by the default 3.0 pt.
    let st = h.state().session.active().unwrap();
    let blur = st.selected_shapes()[0].text.as_ref().unwrap().paragraphs[0].runs[0].props.shadow.as_ref().unwrap().blur;
    assert_eq!(blur, 10.0);
}
