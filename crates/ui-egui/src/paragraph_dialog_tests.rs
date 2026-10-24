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

fn para_props(h: &Harness<'_, SlideApp>) -> deckcraft_model::text::ParaProps {
    h.state()
        .session
        .active()
        .unwrap()
        .selected_shapes()
        .first()
        .and_then(|sh| sh.text.as_ref())
        .and_then(|t| t.paragraphs.first())
        .map(|p| p.props.clone())
        .unwrap()
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
fn paragraph_dialog_sets_asian_typography_in_one_undo() {
    let mut h = harness();
    assert_eq!(para_props(&h).hanging_punct, None);
    let before = h.state().session.active().unwrap().history.undo.len();
    h.get_by_label("Paragraph…").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("paragraph"));
    assert!(h.query_all_by_label("Asian Typography").next().is_some(), "the Asian Typography section is shown");
    h.get_by_label("Hang Punctuation").click_accesskit();
    h.run_steps(1);
    h.get_by_label("Allow Latin Text to Wrap Mid-word").click_accesskit();
    h.run_steps(1);
    h.get_by_label("OK").click_accesskit();
    h.run_steps(1);
    assert!(h.state().dialog.is_none(), "status {:?}", h.state().status);
    let p = para_props(&h);
    assert_eq!(p.hanging_punct, Some(true));
    assert_eq!(p.latin_line_break, Some(true));
    // The box opens checked: an absent `eaLnBrk` means on in the layout, so OK writes the
    // effective value rather than turning Japanese wrapping off as a side effect.
    assert_eq!(p.east_asian_line_break, Some(true), "the effective default is written");
    let undo = &h.state().session.active().unwrap().history.undo;
    assert_eq!(undo.len(), before + 1, "the whole dialog is one undo step");
    assert_eq!(undo.last().unwrap().label, "Paragraph…");
}

#[test]
fn cancelling_the_paragraph_dialog_leaves_asian_typography_alone() {
    let mut h = harness();
    let before = h.state().session.active().unwrap().history.undo.len();
    h.get_by_label("Paragraph…").click();
    h.run_steps(2);
    {
        // The East Asian box opens checked: an absent `eaLnBrk` means on in the layout.
        let fields = &h.state().dialog.as_ref().unwrap().fields;
        assert_eq!(fields.get("eastAsianLineBreak").map(String::as_str), Some("1"), "the box opens at its effective value");
    }
    h.get_by_label("East Asian Line Breaking").click_accesskit();
    h.run_steps(1);
    h.get_by_label("Hang Punctuation").click_accesskit();
    h.run_steps(1);
    let fields = &h.state().dialog.as_ref().unwrap().fields;
    assert_eq!(fields.get("eastAsianLineBreak").map(String::as_str), Some("0"), "the click reached the box");
    assert_eq!(fields.get("hangingPunctuation").map(String::as_str), Some("1"));
    h.get_by_label("Cancel").click_accesskit();
    h.run_steps(1);
    assert!(h.state().dialog.is_none());
    let p = para_props(&h);
    assert_eq!(p.east_asian_line_break, None);
    assert_eq!(p.hanging_punct, None);
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), before, "nothing was applied");
}
