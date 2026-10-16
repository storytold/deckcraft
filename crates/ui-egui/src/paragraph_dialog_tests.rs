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

#[test]
fn tabs_from_paragraph_applies_a_stop_and_returns() {
    let mut h = harness();
    h.get_by_label("Paragraph…").click();
    h.run_steps(2);
    h.state_mut().dialog.as_mut().unwrap().fields.insert("align".into(), "right".into());
    h.get_by_label("Tabs…").click_accesskit();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("tabs"), "status {:?}", h.state().status);
    {
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.fields.insert("stops".into(), "2:ctr".into());
    }
    h.get_by_label("OK").click_accesskit();
    // The Tabs OK frame restores Paragraph; the next frame fills its fields.
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("paragraph"), "status {:?}", h.state().status);
    assert_eq!(h.state().dialog.as_ref().unwrap().fields.get("align").map(String::as_str), Some("right"));
    assert_eq!(tabs_of(&h), vec![(144.0, "ctr".into())]);
    assert_eq!(h.state().session.active().unwrap().history.undo.last().unwrap().label, "Tabs…");
    h.get_by_label("Cancel").click_accesskit();
    h.run_steps(1);
    assert!(h.state().dialog.is_none());
    assert_eq!(align_of(&h).as_deref(), Some("ctr"), "cancelling Paragraph leaves the alignment shape.insert wrote");
}

#[test]
fn unchanged_minimum_survives_alignment_and_tabs_without_rounding() {
    use deckcraft_model::text::LineHeight;
    let mut h = harness();
    h.state_mut().session.execute("format.lineSpacing", &json!({"mode":"minimum", "pt":29.123456789})).unwrap();
    h.get_by_label("Paragraph…").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().unwrap().fields.get("lineKind").map(String::as_str), Some("minimum"));
    if let Ok(path) = std::env::var("DECKCRAFT_MINIMUM_DIALOG_QA_PNG") {
        h.render().unwrap().save(path).unwrap();
    }
    h.state_mut().dialog.as_mut().unwrap().fields.insert("align".into(), "right".into());
    h.get_by_label("Tabs…").click_accesskit();
    h.run_steps(2);
    h.get_by_label("OK").click_accesskit();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("paragraph"));
    h.get_by_label("OK").click_accesskit();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "{:?}", h.state().status);
    let st = h.state().session.active().unwrap();
    let props = &st.selected_shapes()[0].text.as_ref().unwrap().paragraphs[0].props;
    assert_eq!(props.line_height, Some(LineHeight::Minimum { points: 29.123456789 }));
    assert_eq!(props.line_spacing, None);
    assert_eq!(align_of(&h).as_deref(), Some("r"));
}

#[test]
fn paragraph_dialog_applies_minimum_and_normal_as_real_engine_settings() {
    use deckcraft_model::text::LineHeight;
    let mut h = harness();
    for (kind, value, expected) in [("minimum", "37.25", LineHeight::Minimum { points: 37.25 }), ("normal", "1", LineHeight::Normal)] {
        h.get_by_label("Paragraph…").click();
        h.run_steps(2);
        let dialog = h.state_mut().dialog.as_mut().unwrap();
        dialog.fields.insert("lineKind".into(), kind.into());
        dialog.fields.insert("lineValue".into(), value.into());
        h.get_by_label("OK").click_accesskit();
        h.run_steps(2);
        assert!(h.state().dialog.is_none(), "{:?}", h.state().status);
        let st = h.state().session.active().unwrap();
        let props = &st.selected_shapes()[0].text.as_ref().unwrap().paragraphs[0].props;
        assert_eq!(props.line_height, Some(expected));
        assert_eq!(props.line_spacing, None);
    }
}

#[test]
fn pending_line_changes_survive_repeated_tabs_visits_until_paragraph_ok() {
    use deckcraft_model::text::{LineHeight, Spacing};
    for (kind, value, policy, legacy) in [
        ("minimum", "37.25123456789", Some(LineHeight::Minimum { points: 37.25123456789 }), None),
        ("normal", "1", Some(LineHeight::Normal), None),
        ("exactly", "19.5", None, Some(Spacing::Pts(19.5))),
    ] {
        let mut h = harness();
        h.state_mut().session.execute("format.lineSpacing", &json!({"mode":"minimum", "pt":29.123456789})).unwrap();
        h.get_by_label("Paragraph…").click();
        h.run_steps(2);
        {
            let dialog = h.state_mut().dialog.as_mut().unwrap();
            dialog.fields.insert("lineKind".into(), kind.into());
            dialog.fields.insert("lineValue".into(), value.into());
        }
        for _ in 0..2 {
            h.get_by_label("Tabs…").click_accesskit();
            h.run_steps(2);
            h.get_by_label("OK").click_accesskit();
            h.run_steps(2);
            assert_eq!(h.state().dialog.as_ref().unwrap().fields.get("lineKind").map(String::as_str), Some(kind));
        }
        h.get_by_label("OK").click_accesskit();
        h.run_steps(2);
        assert!(h.state().dialog.is_none(), "{:?}", h.state().status);
        let st = h.state().session.active().unwrap();
        let props = &st.selected_shapes()[0].text.as_ref().unwrap().paragraphs[0].props;
        assert_eq!(props.line_height, policy);
        assert_eq!(props.line_spacing, legacy);
    }
}

#[test]
fn reverting_pending_line_value_after_tabs_keeps_the_original_exact_minimum() {
    use deckcraft_model::text::LineHeight;
    let mut h = harness();
    h.state_mut().session.execute("format.lineSpacing", &json!({"mode":"minimum", "pt":29.123456789})).unwrap();
    h.get_by_label("Paragraph…").click();
    h.run_steps(2);
    for value in ["37.25", "29.12"] {
        h.state_mut().dialog.as_mut().unwrap().fields.insert("lineValue".into(), value.into());
        h.get_by_label("Tabs…").click_accesskit();
        h.run_steps(2);
        h.get_by_label("OK").click_accesskit();
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().unwrap().fields.get("lineValue").map(String::as_str), Some(value));
    }
    h.get_by_label("OK").click_accesskit();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    let st = h.state().session.active().unwrap();
    assert_eq!(st.selected_shapes()[0].text.as_ref().unwrap().paragraphs[0].props.line_height, Some(LineHeight::Minimum { points: 29.123456789 }));
}
