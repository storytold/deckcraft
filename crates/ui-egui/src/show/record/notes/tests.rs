//! The notes/script pane: formatted display, size limits, scrolling, editing during a take,
//! keyboard focus, and the Teleprompter view.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use super::super::RecordStatus;
use super::super::views;
use super::*;

fn app_with(notes: &[&str]) -> SlideApp {
    let mut session = deckcraft_engine::Session::with_new();
    for (i, text) in notes.iter().enumerate() {
        if i > 0 {
            session.execute("slide.new", &json!({})).unwrap();
        }
        session.execute("slide.notes", &json!({"index": i, "text": text})).unwrap();
    }
    let mut app = SlideApp::new(session, crate::Services::default());
    app.ui.presenter_view = false;
    app
}

fn harness(app: SlideApp) -> Harness<'static, SlideApp> {
    Harness::builder().with_size(egui::vec2(1600.0, 1000.0)).with_step_dt(0.05).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    )
}

fn open_record(h: &mut Harness<'static, SlideApp>, slide: usize) {
    h.state_mut().start_show(slide, false);
    h.state_mut().show.as_mut().unwrap().record = true;
    h.run_steps(3);
}

fn notes_editor<'a>(h: &'a Harness<'static, SlideApp>) -> egui_kittest::Node<'a> {
    // The script field is the shell's only multiline input; its hint is not a stable name.
    h.get_by_role(Role::MultilineTextInput)
}

fn view<'a>(h: &'a Harness<'static, SlideApp>) -> &'a RecordView {
    &h.state().show.as_ref().unwrap().record_view
}

fn press(h: &mut Harness<'static, SlideApp>, key: egui::Key) {
    h.input_mut().events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
    // A real key press also produces its text, which is what a focused field inserts.
    if key == egui::Key::Space {
        h.input_mut().events.push(egui::Event::Text(" ".into()));
    }
    h.input_mut().events.push(egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: egui::Modifiers::NONE });
}

#[test]
fn the_pane_shows_the_current_slides_script() {
    let mut h = harness(app_with(&["Open with the market chart", "Then the roadmap"]));
    open_record(&mut h, 0);
    assert_eq!(notes_editor(&h).value().as_deref(), Some("Open with the market chart"));
    h.get_by_label("Next slide").click();
    h.run_steps(3);
    assert_eq!(notes_editor(&h).value().as_deref(), Some("Then the roadmap"), "the pane follows the slide");
}

#[test]
fn a_long_script_scrolls_inside_the_pane() {
    let long: Vec<String> = (1..=80).map(|i| format!("Line {i} of a long teleprompter script")).collect();
    let mut h = harness(app_with(&[&long.join("\n")]));
    open_record(&mut h, 0);
    h.state_mut().show.as_mut().unwrap().record_view.notes_scroll = 30.0;
    h.run_steps(3);
    let offset = view(&h).notes_scroll;
    assert!(offset.is_finite() && offset >= 0.0);
    assert!(offset > 0.0, "long notes keep the requested scroll offset");
}

#[test]
fn size_controls_stop_at_the_limits() {
    let mut h = harness(app_with(&["Script"]));
    open_record(&mut h, 0);
    for _ in 0..30 {
        h.get_by_label("A+").click();
        h.run_steps(1);
    }
    assert_eq!(view(&h).notes_px, super::NOTES_MAX);
    for _ in 0..30 {
        h.get_by_label("A−").click();
        h.run_steps(1);
    }
    assert_eq!(view(&h).notes_px, super::NOTES_MIN);
}

#[test]
fn an_edit_while_recording_is_saved_when_the_field_loses_focus() {
    let mut h = harness(app_with(&["Meet the team"]));
    open_record(&mut h, 0);
    h.get_by_label("Record").click();
    h.run_steps(2);
    notes_editor(&h).click();
    h.run_steps(2);
    notes_editor(&h).type_text(" and the plan");
    h.run_steps(2);
    assert_eq!(view(&h).status, RecordStatus::Recording, "editing does not stop the take");
    // A+ takes the focus away; the draft is written to the slide.
    h.get_by_label("A+").click();
    h.run_steps(3);
    let doc = h.state().session.active().unwrap().doc.clone();
    assert_eq!(doc.slides[0].notes_text(), "Meet the team and the plan");
    assert_eq!(view(&h).status, RecordStatus::Recording, "the take is still running");
}

#[test]
fn the_editor_keeps_navigation_keys_out_of_the_show() {
    let mut h = harness(app_with(&["First", "Second"]));
    open_record(&mut h, 0);
    notes_editor(&h).click();
    h.run_steps(2);
    press(&mut h, egui::Key::Space);
    h.run_steps(2);
    assert_eq!(h.state().show.as_ref().unwrap().state.slide, 0, "Space typed, not advanced");
    assert!(notes_editor(&h).value().as_deref().unwrap_or_default().contains("First "), "Space reached the script");
    press(&mut h, egui::Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(h.state().show.as_ref().unwrap().state.slide, 0, "an arrow moves the caret, not the slide");
}

#[test]
fn the_teleprompter_scrolls_only_while_recording() {
    let mut h = harness(app_with(&["A very long script\nline after line\nfor the teleprompter"]));
    open_record(&mut h, 0);
    assert_eq!(view(&h).view, views::ViewKind::Teleprompter);
    assert_eq!(view(&h).teleprompter.offset, 0.0);
    h.get_by_label("Record").click();
    h.run_steps(10);
    assert!(view(&h).teleprompter.offset > 0.0, "the script advances with the take");
    h.get_by_label("Pause").click();
    h.run_steps(3);
    // The press and release land on separate frames; measure from the settled paused state.
    assert_eq!(view(&h).status, RecordStatus::Paused);
    let before = view(&h).teleprompter.offset;
    h.run_steps(10);
    assert!((view(&h).teleprompter.offset - before).abs() < 1e-6, "pausing stops the teleprompter");
}

#[test]
fn teleprompter_speed_and_overlay_are_controls() {
    let mut h = harness(app_with(&["Script"]));
    open_record(&mut h, 0);
    let speed = view(&h).teleprompter.speed;
    h.get_by_label("Teleprompter faster").click();
    h.run_steps(2);
    assert!(view(&h).teleprompter.speed > speed);
    h.get_by_label("Teleprompter slower").click();
    h.run_steps(2);
    assert_eq!(view(&h).teleprompter.speed, speed);
    h.get_by_label("Teleprompter overlay").click();
    h.run_steps(2);
    assert!(!view(&h).teleprompter.overlay);
}

#[test]
fn leaving_the_pane_saves_an_edit() {
    let mut h = harness(app_with(&["Keep this"]));
    open_record(&mut h, 0);
    notes_editor(&h).click();
    h.run_steps(2);
    notes_editor(&h).type_text(" edit");
    h.run_steps(2);
    // Slide view hides the pane; the draft must be flushed before it disappears.
    h.get_by_role_and_label(Role::ComboBox, "Views").click();
    h.run_steps(2);
    h.get_by_label("Slide").click();
    h.run_steps(3);
    let doc = h.state().session.active().unwrap().doc.clone();
    assert_eq!(doc.slides[0].notes_text(), "Keep this edit");
}
