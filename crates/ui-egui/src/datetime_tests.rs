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

#[test]
fn insert_opens_the_dialog_and_ok_inserts_one_field() {
    let mut h = harness();
    h.get_by_label("Date & Time").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("dateTime"));
    let _ = h.get_by_label("Update automatically");
    let _ = h.get_by_label("English (United States)");
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
fn choosing_japanese_lists_and_inserts_a_japanese_date() {
    let mut h = harness();
    h.get_by_label("Date & Time").click();
    h.run_steps(2);
    h.get_by_label("English (United States)").click();
    h.run_steps(2);
    h.get_by_label("Japanese").click();
    h.run_steps(2);
    let now = deckcraft_text::datetime::DateTime::now_local().unwrap();
    let sample = deckcraft_text::datetime::field_text("datetime2", Some("ja-JP"), now).unwrap();
    assert!(sample.contains('曜'), "the dialog lists the Japanese weekday: {sample}");
    h.get_by_label(&sample).click();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    let run = date_run(&h).unwrap();
    assert!(matches!(run.kind, RunKind::Field { ref field } if field == "datetime2"), "{run:?}");
    assert_eq!(run.text, sample);
    assert_eq!(run.props.lang.as_deref(), Some("ja-JP"));
}

#[test]
fn japanese_formats_that_read_the_same_name_their_kind() {
    let mut h = harness();
    h.get_by_label("Date & Time").click();
    h.run_steps(2);
    h.get_by_label("English (United States)").click();
    h.run_steps(2);
    h.get_by_label("Japanese").click();
    h.run_steps(2);
    // Japanese has no abbreviated month or day name, so datetime3, datetime4 and datetime5 all
    // read the same text and datetime6/datetime7 both read "2026年10月". The row inserts its
    // kind, so an ambiguous sample also names the kind and the kinds stay distinguishable.
    let now = deckcraft_text::datetime::DateTime::now_local().unwrap();
    let samples: Vec<String> = ["datetime3", "datetime4", "datetime5", "datetime6", "datetime7"]
        .iter()
        .map(|k| deckcraft_text::datetime::field_text(k, Some("ja-JP"), now).unwrap())
        .collect();
    assert_eq!(samples[0], samples[1], "the kinds really do read the same");
    assert_eq!(samples[0], samples[2], "the kinds really do read the same");
    assert_eq!(samples[3], samples[4], "the kinds really do read the same");
    for (kind, sample) in ["datetime3", "datetime4", "datetime5"].iter().zip(&samples) {
        let label = format!("{sample} ({kind})");
        assert!(h.query_all_by_label(&label).next().is_some(), "{label} is listed");
    }
    // A sample no other row shares keeps its bare label.
    let unique = deckcraft_text::datetime::field_text("datetime2", Some("ja-JP"), now).unwrap();
    assert!(h.query_all_by_label(&unique).next().is_some(), "{unique} is listed without a kind");
    // Picking the named kind still inserts that kind.
    h.get_by_label(&format!("{} (datetime4)", samples[1])).click();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    let run = date_run(&h).unwrap();
    assert!(matches!(run.kind, RunKind::Field { ref field } if field == "datetime4"), "{run:?}");
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
