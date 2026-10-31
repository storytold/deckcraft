//! The Paragraph dialog: Home › Paragraph › Paragraph… opens it, OK applies one undo step,
//! and Tabs… edits stops then returns to Paragraph.

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 320.0, 80.0], "text": "Hello"})).unwrap();
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

fn align_of(h: &Harness<'_, SlideApp>) -> Option<String> {
    let st = h.state().session.active().unwrap();
    st.selected_shapes()
        .first()
        .and_then(|sh| sh.text.as_ref())
        .and_then(|t| t.paragraphs.first())
        .and_then(|p| p.props.align.map(|a| a.xml().to_string()))
}

fn tabs_of(h: &Harness<'_, SlideApp>) -> Vec<(f64, String)> {
    let st = h.state().session.active().unwrap();
    st.selected_shapes()
        .first()
        .and_then(|sh| sh.text.as_ref())
        .and_then(|t| t.paragraphs.first())
        .and_then(|p| p.props.tabs.clone())
        .unwrap_or_default()
        .into_iter()
        .map(|t| (t.pos, t.align))
        .collect()
}

#[test]
fn paragraph_button_aligns_text_in_one_undo() {
    let mut h = harness();
    h.get_by_label("Paragraph…").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("paragraph"));
    h.state_mut().dialog.as_mut().unwrap().fields.insert("align".into(), "right".into());
    h.get_by_label("OK").click_accesskit();
    h.run_steps(1);
    assert!(h.state().dialog.is_none(), "status {:?}", h.state().status);
    assert_eq!(align_of(&h).as_deref(), Some("r"));
    assert_eq!(h.state().session.active().unwrap().history.undo.last().unwrap().label, "Paragraph…");
}

/// Paragraph-dialog tab settings apply the chosen stop and return to the paragraph controls.
#[test]
fn tabs_from_paragraph_applies_a_stop_and_returns() {
    let mut h = harness();
    h.get_by_label("Paragraph…").click();
    h.run_steps(2);
    h.state_mut().dialog.as_mut().unwrap().fields.insert("align".into(), "right".into());
    // Settle the anchored window's geometry before hit-testing the button with the real pointer
    // path. egui 0.37 keeps a window invisible for its first sizing frame; AccessKit can activate
    // such a node, but a pointer must actually hit the visible button inside the window.
    h.run_steps(3);
    let tabs = h.get_by_label("Tabs…");
    let rect = tabs.rect();
    assert!(rect.min.x >= 0.0 && rect.max.x <= 1440.0 && rect.min.y >= 0.0 && rect.max.y <= 900.0, "Tabs… has settled on screen: {rect:?}");
    tabs.click();
    // The click switches the dialog at the end of one pass; wait for that and the first visible
    // frame before querying the Tabs window's buttons.
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("tabs"), "status {:?}", h.state().status);
    {
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.fields.insert("stops".into(), "2:ctr".into());
    }
    h.get_by_role_and_label(egui::accesskit::Role::Button, "OK").click_accesskit();
    // The Tabs OK frame restores Paragraph; the next frame fills its fields.
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("paragraph"), "status {:?}", h.state().status);
    assert_eq!(h.state().dialog.as_ref().unwrap().fields.get("align").map(String::as_str), Some("right"));
    assert_eq!(tabs_of(&h), vec![(144.0, "ctr".into())]);
    assert_eq!(h.state().session.active().unwrap().history.undo.last().unwrap().label, "Tabs…");
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Cancel").click_accesskit();
    h.run_steps(1);
    assert!(h.state().dialog.is_none());
    assert_eq!(align_of(&h).as_deref(), Some("ctr"), "cancelling Paragraph leaves the alignment shape.insert wrote");
}
