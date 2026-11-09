//! Insert › Date and Time, driven like a person: the ribbon opens the dialog, a format is
//! chosen, OK inserts it in one undo, Cancel leaves the slide alone.

use deckcraft_engine::Session;
use deckcraft_model::text::RunKind;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.ui.tab = "Insert".into();
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 1100.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn date_run(h: &Harness<'_, SlideApp>) -> Option<deckcraft_model::Run> {
    h.state()
        .session
        .active()
        .unwrap()
        .current_slide()
        .unwrap()
        .shapes
        .iter()
        .find(|x| x.ph_type() == Some(deckcraft_model::PhType::Date))
        .and_then(|x| x.text.as_ref())
        .and_then(|t| t.paragraphs.first())
        .and_then(|p| p.runs.first())
        .cloned()
}

/// Insert opens date/time settings, and OK inserts exactly one field.
#[test]
fn insert_opens_the_dialog_and_ok_inserts_one_field() {
    let mut h = harness();
    h.get_by_label("Date & Time").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("dateTime"));
    let _ = h.get_by_label("Update automatically");
    assert_eq!(h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Language").value().as_deref(), Some("English (United States)"));
    let now = deckcraft_text::datetime::DateTime::now_local().unwrap();
    let sample = deckcraft_text::datetime::field_text("datetime2", Some("en-US"), now).unwrap();
    h.get_by_label(&sample).click();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    let run = date_run(&h).unwrap();
    assert!(matches!(run.kind, RunKind::Field { ref field } if field == "datetime2"), "{run:?}");
    assert_eq!(run.text, sample);
    let undo = &h.state().session.active().unwrap().history.undo;
    assert_eq!(undo.len(), 1);
    assert_eq!(undo[0].label, "Date & Time");
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    h.run_steps(1);
    assert!(date_run(&h).is_none());
}

#[test]
fn cancel_closes_the_dialog_without_a_date() {
    let mut h = harness();
    h.get_by_label("Date & Time").click();
    h.run_steps(2);
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    assert!(date_run(&h).is_none());
    assert!(h.state().session.active().unwrap().history.undo.is_empty());
}
