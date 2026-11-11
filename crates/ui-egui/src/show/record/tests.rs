//! Record view shell: state machine, layout, and the kittest behaviour of its sections.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use deckcraft_model::Presentation;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::*;
use serde_json::json;

fn doc_with(slides: usize) -> Arc<Presentation> {
    let mut session = deckcraft_engine::Session::with_new();
    for _ in 1..slides {
        session.execute("slide.new", &json!({})).unwrap();
    }
    session.active().unwrap().doc.clone()
}

#[test]
fn the_capture_clock_starts_pauses_resumes_and_stops() {
    let doc = doc_with(1);
    let mut show = Show::new(&doc, 0, false, false);
    let mut rec = RecordView::default();
    assert_eq!(rec.status, RecordStatus::Idle);
    rec.start(&mut show, 10.0);
    assert_eq!(rec.status, RecordStatus::Recording);
    assert!(rec.captured, "a take that started is saved when the view closes");
    assert!((rec.elapsed(13.0) - 3.0).abs() < 1e-9);
    rec.pause(&mut show, 13.0);
    assert_eq!(rec.status, RecordStatus::Paused);
    assert!((rec.elapsed(30.0) - 3.0).abs() < 1e-9, "a paused clock does not move");
    rec.resume(&mut show, 30.0);
    assert_eq!(rec.status, RecordStatus::Recording);
    assert!((rec.elapsed(32.0) - 5.0).abs() < 1e-9, "the pause is not counted");
    rec.stop(&mut show, 32.0);
    assert_eq!(rec.status, RecordStatus::Idle);
    assert!((rec.elapsed(99.0) - 5.0).abs() < 1e-9, "a stopped clock keeps its value");
}

#[test]
fn record_toggles_from_idle_and_pause_needs_a_take() {
    let doc = doc_with(1);
    let mut show = Show::new(&doc, 0, false, false);
    let mut rec = RecordView::default();
    // Pause does nothing before the first take.
    rec.pause(&mut show, 1.0);
    assert_eq!(rec.status, RecordStatus::Idle);
    rec.toggle_record(&mut show, 2.0);
    assert_eq!(rec.status, RecordStatus::Recording);
    rec.toggle_record(&mut show, 3.0);
    assert_eq!(rec.status, RecordStatus::Idle);
}

#[test]
fn reset_and_undo_move_the_clock() {
    let doc = doc_with(1);
    let mut show = Show::new(&doc, 0, false, false);
    let mut rec = RecordView::default();
    rec.start(&mut show, 100.0);
    assert!((rec.elapsed(105.0) - 5.0).abs() < 1e-9);
    rec.reset(105.0);
    assert!(rec.elapsed(105.0) < 1e-9, "Reset zeroes the clock");
    assert!(rec.undo(106.0));
    // The clock kept running for the second between Reset and Undo, so the restored run is 5 + 1
    // at 106 and 7 a second later.
    assert!((rec.elapsed(106.0) - 6.0).abs() < 1e-9, "Undo restores the elapsed time plus the time since Reset");
    assert!((rec.elapsed(107.0) - 7.0).abs() < 1e-9, "and the clock runs on from there");
    assert!(!rec.undo(108.0), "one Undo per Reset");
}

#[test]
fn the_shell_geometry_matches_the_powerpoint_layout() {
    let doc = doc_with(3);
    let rec = RecordView::default();
    let full = Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(1920.0, 1200.0));
    let layout = layout(full, &doc, &rec);
    assert!(layout.top.min.y < 80.0 && layout.top.width() > 800.0, "floating top bar");
    assert!(layout.notes.min.y >= layout.top.max.y, "notes below the top bar");
    assert!(layout.slide.min.y >= layout.notes.max.y, "slide below the notes");
    assert!((layout.bottom.max.y - full.max.y).abs() < 1e-6, "bottom strip reaches the window bottom");
    assert!(layout.slide.width() > 0.0 && layout.slide.height() > 0.0);
    assert!(layout.slide.contains_rect(layout.camera), "cameo bubble sits on the slide");
    assert!(layout.camera.center().x > layout.slide.center().x && layout.camera.center().y > layout.slide.center().y, "bottom-right");
    assert!(layout.views.max.x < layout.bottom.max.x, "Views menu inside the bottom strip");
    assert!(layout.camera_preview.max.x <= layout.views.min.x, "preview icon left of Views");
    assert!(layout.next.is_none(), "only Presenter view reserves a next-slide strip");
}

#[test]
fn view_kind_resizes_the_notes_and_reserves_the_next_strip() {
    let doc = doc_with(3);
    let full = Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(1920.0, 1200.0));
    let mut rec = RecordView::default();
    let tele = layout(full, &doc, &rec);
    rec.view = views::ViewKind::Presenter;
    let presenter = layout(full, &doc, &rec);
    rec.view = views::ViewKind::Slide;
    let slide_only = layout(full, &doc, &rec);
    assert!(tele.notes.height() > presenter.notes.height());
    assert_eq!(slide_only.notes.height(), 0.0, "Slide view hides the script");
    assert!(presenter.next.is_some(), "Presenter view has a next-slide strip");
    assert!(presenter.next.unwrap().max.y <= presenter.notes.max.y);
}

fn app_with(slides: usize) -> SlideApp {
    let mut session = deckcraft_engine::Session::with_new();
    for _ in 1..slides {
        session.execute("slide.new", &json!({})).unwrap();
    }
    let mut app = SlideApp::new(session, crate::Services::default());
    app.ui.presenter_view = false;
    app
}

fn harness(app: SlideApp) -> Harness<'static, SlideApp> {
    Harness::builder().with_size(vec2(1600.0, 1000.0)).with_step_dt(0.05).build_ui_state(
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

#[test]
fn record_and_stop_are_wired_to_show_record() {
    let mut h = harness(app_with(2));
    open_record(&mut h, 0);
    assert_eq!(h.state().show.as_ref().unwrap().record_view.status, RecordStatus::Idle);
    h.get_by_label("Record").click();
    h.run_steps(3);
    assert_eq!(h.state().show.as_ref().unwrap().record_view.status, RecordStatus::Recording);
    assert!(h.state().show.as_ref().unwrap().record_view.captured, "the take is marked");
    // The button is named after what it does: a running take offers Stop.
    h.get_by_label("Stop").click();
    h.run_steps(3);
    assert_eq!(h.state().show.as_ref().unwrap().record_view.status, RecordStatus::Idle);
}

#[test]
fn pause_and_resume_are_buttons_on_the_top_bar() {
    let mut h = harness(app_with(1));
    open_record(&mut h, 0);
    h.get_by_label("Record").click();
    h.run_steps(3);
    h.get_by_label("Pause").click();
    h.run_steps(3);
    assert_eq!(h.state().show.as_ref().unwrap().record_view.status, RecordStatus::Paused);
    h.get_by_label("Resume").click();
    h.run_steps(3);
    assert_eq!(h.state().show.as_ref().unwrap().record_view.status, RecordStatus::Recording);
}

#[test]
fn the_bottom_chevrons_navigate_the_show() {
    let mut h = harness(app_with(3));
    open_record(&mut h, 0);
    h.get_by_label("Next slide").click();
    h.run_steps(3);
    assert_eq!(h.state().show.as_ref().unwrap().state.slide, 1);
    h.get_by_label("Previous slide").click();
    h.run_steps(3);
    assert_eq!(h.state().show.as_ref().unwrap().state.slide, 0);
}

#[test]
fn the_notes_pane_shows_the_current_slides_script_and_its_size_controls() {
    let mut session = deckcraft_engine::Session::with_new();
    session.execute("slide.notes", &json!({"index": 0, "text": "Open with the market chart"})).unwrap();
    session.execute("slide.new", &json!({})).unwrap();
    session.execute("slide.notes", &json!({"index": 1, "text": "Then the roadmap"})).unwrap();
    let mut app = SlideApp::new(session, crate::Services::default());
    app.ui.presenter_view = false;
    let mut h = harness(app);
    open_record(&mut h, 0);
    // The script is the editor's value; its accessible label is only the empty-state hint.
    let script = |h: &Harness<'static, SlideApp>| h.get_by_role(egui::accesskit::Role::MultilineTextInput).value().unwrap_or_default();
    assert!(script(&h).contains("Open with the market chart"), "slide 1 notes: {}", script(&h));
    assert!(!script(&h).contains("Then the roadmap"), "not the next slide's notes: {}", script(&h));
    let before = h.state().show.as_ref().unwrap().record_view.notes_px;
    h.get_by_label("A+").click();
    h.run_steps(2);
    assert!(h.state().show.as_ref().unwrap().record_view.notes_px > before);
    h.get_by_label("A−").click();
    h.run_steps(2);
    assert_eq!(h.state().show.as_ref().unwrap().record_view.notes_px, before);
}

#[test]
fn the_ink_bar_selects_tools_and_colours() {
    let mut h = harness(app_with(1));
    open_record(&mut h, 0);
    h.get_by_label("Highlighter").click();
    h.run_steps(2);
    let show = h.state().show.as_ref().unwrap();
    assert_eq!(show.record_view.ink.tool, InkTool::Highlighter);
    assert!(show.pen, "the shell turns the show's pen on for an ink tool");
    h.get_by_label("Ink colour 4").click();
    h.run_steps(2);
    assert_eq!(h.state().show.as_ref().unwrap().record_view.ink.color, 3);
    h.get_by_label("Pen").click();
    h.run_steps(2);
    assert_eq!(h.state().show.as_ref().unwrap().record_view.ink.tool, InkTool::Pen);
    h.get_by_label("Eraser").click();
    h.run_steps(2);
    assert_eq!(h.state().show.as_ref().unwrap().record_view.ink.tool, InkTool::Eraser);
    assert!(!h.state().show.as_ref().unwrap().pen, "Eraser leaves the pen capture off");
}

#[test]
fn the_views_menu_switches_layouts() {
    let mut h = harness(app_with(2));
    open_record(&mut h, 0);
    assert_eq!(h.state().show.as_ref().unwrap().record_view.view, views::ViewKind::Teleprompter);
    h.get_by_label("Views").click();
    h.run_steps(2);
    h.get_by_label("Slide").click();
    h.run_steps(2);
    let rec = &h.state().show.as_ref().unwrap().record_view;
    assert_eq!(rec.view, views::ViewKind::Slide);
    assert!(!rec.notes_open, "Slide view hides the notes");
}

#[test]
fn devices_report_unavailable_instead_of_faking_success() {
    let mut h = harness(app_with(1));
    open_record(&mut h, 0);
    assert_eq!(h.state().show.as_ref().unwrap().record_view.devices.camera.status, devices::DeviceStatus::Unavailable);
    // The buttons are disabled without a host, so a click cannot turn a missing camera on.
    h.get_by_label("Camera").click();
    h.run_steps(2);
    assert_eq!(h.state().show.as_ref().unwrap().record_view.devices.camera.status, devices::DeviceStatus::Unavailable);
}

#[test]
fn a_shell_snapshot() {
    let mut h = harness(app_with(3));
    open_record(&mut h, 0);
    h.snapshot("record_window_shell");
}
