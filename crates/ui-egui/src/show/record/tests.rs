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

/// Stop freezes the slide's captured length: the interval between Stop and leaving the view (or
/// the rest of a pause) must not reach the saved timing.
#[test]
fn stop_freezes_the_slide_timing() {
    let doc = doc_with(1);
    let mut show = Show::new(&doc, 0, false, false);
    let mut rec = RecordView::default();
    rec.start(&mut show, 100.0);
    assert_eq!(rec.frozen_slide_seconds(), None, "a running take has no frozen timing");
    rec.stop(&mut show, 105.0);
    assert!((rec.frozen_slide_seconds().expect("frozen at Stop") - 5.0).abs() < 1e-9);
    // Ten seconds of browsing after Stop are not part of the take.
    assert!((show.captured_slide_seconds(115.0) - 15.0).abs() < 1e-9, "the running clock keeps growing");
    // Stopping while paused freezes at the pause, not at the Stop that came later.
    rec.start(&mut show, 200.0);
    rec.pause(&mut show, 203.0);
    rec.stop(&mut show, 260.0);
    assert!((rec.frozen_slide_seconds().expect("frozen while paused") - 3.0).abs() < 1e-9);
}

/// The keyboard path: `ShowState` enters the destination before `apply` runs, and the frame's
/// trigger tick has already rebuilt the timeline cache for it. The departing slide must still keep
/// its timing.
#[test]
fn a_slide_change_files_the_departing_slide_after_the_state_moved() {
    let doc = doc_with(2);
    let mut show = Show::new(&doc, 0, false, false);
    show.record = true;
    let mut rec = RecordView::default();
    rec.start(&mut show, 100.0);
    show.record_view = rec;
    let action = show.state.next(&doc);
    let _ = show.timeline(&doc);
    let _ = show.apply(action, &doc, 105.0);
    assert_eq!(show.applied_slide, 1, "the show applied the new slide");
    assert_eq!(show.timings, vec![(0, 5.0)], "the departing slide keeps its timing");
}

/// A stopped take keeps its slide's timing when the user navigates before leaving the view, and
/// the slide shown afterwards gets no timing it never captured.
#[test]
fn a_stopped_take_keeps_its_slide_timing_across_navigation() {
    let doc = doc_with(2);
    let mut show = Show::new(&doc, 0, false, false);
    show.record = true;
    let mut rec = RecordView::default();
    rec.start(&mut show, 100.0);
    show.record_view = rec;
    let _ = show.timeline(&doc); // the frame draws the current slide before anything navigates
    let _ = show.record_command("record.stop", 105.0).expect("stop");
    assert_eq!(show.timings, vec![(0, 5.0)], "Stop files the slide it froze");
    let action = show.state.next(&doc);
    let _ = show.apply(action, &doc, 106.0);
    // What `finish` does for the slide on screen when the show closes.
    show.finalize_take_slide(120.0);
    assert_eq!(show.timings, vec![(0, 5.0)], "slide 1 was never captured");
}

/// A paused take that navigates keeps both slides: the departing one up to the pause, the new one
/// from its own start, without the old pause folded into it.
#[test]
fn a_paused_take_files_both_slides() {
    let doc = doc_with(2);
    let mut show = Show::new(&doc, 0, false, false);
    show.record = true;
    let mut rec = RecordView::default();
    rec.start(&mut show, 100.0);
    rec.pause(&mut show, 103.0);
    show.record_view = rec;
    let _ = show.timeline(&doc);
    let action = show.state.next(&doc);
    let _ = show.apply(action, &doc, 110.0);
    let _ = show.record_command("record.resume", 112.0).expect("resume");
    let _ = show.record_command("record.stop", 116.0).expect("stop");
    show.finalize_take_slide(120.0);
    assert_eq!(show.timings, vec![(0, 3.0), (1, 4.0)]);
}

/// Erasing ink (E) keeps the laser samples a paused or stopped take still has to save: only a
/// show with no take clears them.
#[test]
fn erasing_ink_keeps_a_paused_or_stopped_takes_laser_samples() {
    let doc = doc_with(1);
    let mut show = Show::new(&doc, 0, false, false);
    show.record = true;
    show.laser_live = vec![(1.0, 2.0, 0)];
    // A paused or stopped take owns its samples, so erasing keeps them.
    show.record_view.captured = true;
    let mut actions = vec![];
    let (_, consumed) = crate::show::keys::handle_keys(&mut show, &doc, 0.0, vec![(egui::Key::E, egui::Modifiers::NONE)], false, &mut actions);
    assert_eq!(consumed.len(), 1, "E is handled");
    assert_eq!(show.laser_live.len(), 1, "a paused or stopped take keeps its samples");
    // No take: erase clears the buffer.
    show.record_view.captured = false;
    let (_, consumed) = crate::show::keys::handle_keys(&mut show, &doc, 0.0, vec![(egui::Key::E, egui::Modifiers::NONE)], false, &mut actions);
    assert_eq!(consumed.len(), 1, "E is handled");
    assert!(show.laser_live.is_empty(), "no take: erase clears the buffer");
}

#[test]
fn record_commands_share_the_shell_state_machine() {
    let doc = doc_with(1);
    let mut show = Show::new(&doc, 0, false, false);
    let mut rec = RecordView::default();
    assert_eq!(command(&mut rec, &mut show, "record.start", 10.0).expect("start"), "recording");
    assert!(rec.captured);
    assert_eq!(command(&mut rec, &mut show, "record.pause", 12.0).expect("pause"), "paused");
    assert_eq!(command(&mut rec, &mut show, "record.resume", 14.0).expect("resume"), "recording");
    assert_eq!(command(&mut rec, &mut show, "record.toggle", 16.0).expect("stop"), "idle");
    assert!(rec.frozen_slide_seconds().is_some(), "the command path freezes the timing too");
    assert_eq!(command(&mut rec, &mut show, "record.reset", 17.0).expect("reset"), "idle");
    assert_eq!(command(&mut rec, &mut show, "record.undo", 18.0).expect("undo"), "idle");
    assert!(command(&mut rec, &mut show, "record.jump", 19.0).is_err(), "an unknown command is refused");
    // Start over a running take would restart its clocks and clear the microphone buffer.
    command(&mut rec, &mut show, "record.start", 20.0).expect("start again");
    assert!(command(&mut rec, &mut show, "record.start", 21.0).is_err(), "a take is already recording");
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
    assert!((rec.elapsed(106.0) - 1.0).abs() < 1e-9, "and it counts on from zero");
    assert!(rec.undo(106.0));
    assert!((rec.elapsed(107.0) - 6.0).abs() < 1e-9, "Undo restores the elapsed time");
    assert!(!rec.undo(108.0), "one Undo per Reset");
}

/// A stopped clock is frozen, and Undo after a Reset must not start it: the record status is
/// `Idle` after Stop, so the timer's own paused state has to decide.
#[test]
fn undo_after_a_stop_keeps_the_clock_frozen() {
    let doc = doc_with(1);
    let mut show = Show::new(&doc, 0, false, false);
    let mut rec = RecordView::default();
    rec.start(&mut show, 100.0);
    rec.stop(&mut show, 105.0);
    assert_eq!(rec.status, RecordStatus::Idle);
    assert!((rec.elapsed(105.0) - 5.0).abs() < 1e-9);
    rec.reset(105.0);
    assert!(rec.undo(106.0));
    assert!((rec.elapsed(200.0) - 5.0).abs() < 1e-9, "the stopped clock does not count up again");
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
}

#[test]
fn view_kind_resizes_the_notes() {
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
    // The button says what it will do: while a take runs it offers Stop.
    h.get_by_label("Stop recording").click();
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

/// The right-hand control group must not run into the clock: on a narrow bar the controls closest
/// to the middle (Undo, Reset) are the first to reach it, especially with a device that has no
/// driver (its label used to spell the whole status out).
#[test]
fn the_right_control_group_stays_clear_of_the_clock() {
    // A narrow bar drops the optional controls; a wide one shows them all. Neither may run into
    // the clock, which owns the middle of the bar.
    let wide = [
        "Reset",
        "Undo",
        "Record",
        "Pause",
        "Export ›",
        "More",
        "Camera — unavailable",
        "Microphone — unavailable",
        "Screen recording — unavailable",
    ];
    let narrow = ["Reset", "Undo", "Record", "Pause"];
    for (width, expected) in [(900.0, narrow.as_slice()), (1600.0, wide.as_slice())] {
        let mut app = app_with(1);
        app.ui.presenter_view = false;
        let mut h = egui_kittest::Harness::builder().with_size(vec2(width, 620.0)).with_step_dt(0.05).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        open_record(&mut h, 0);
        h.run_steps(3);
        let top = {
            let app = h.state();
            let doc = app.session.active().expect("a document").doc.clone();
            super::layout(h.ctx.content_rect(), &doc, &app.show.as_ref().expect("a show").record_view).top
        };
        let limit = top.center().x + 40.0;
        for label in expected {
            // `get_by_label` fails the test when the control is missing, so each width checks the
            // controls it must show.
            let b = h.get_by_label(label).rect();
            assert!(b.min.x >= limit - 0.5, "{label} at {width}: starts at {} but the clock's side ends at {limit}", b.min.x);
        }
    }
}

/// The Record view's backdrop is painted before the slide: the bars are drawn over the frame, so a
/// full-frame fill there would hide the slide it was drawn for.
#[test]
fn the_record_backdrop_is_behind_the_slide() {
    let mut session = deckcraft_engine::Session::with_new();
    let size = session.active().unwrap().doc.slide_size;
    session.execute("shape.insert", &json!({"preset": "rect", "rect": [0.0, 0.0, size.width, size.height]})).unwrap();
    let mut app = SlideApp::new(session, crate::Services::default());
    app.ui.presenter_view = false;
    let mut h = harness(app);
    open_record(&mut h, 0);
    h.run_steps(3);
    let layout = {
        let app = h.state();
        let doc = app.session.active().expect("a document").doc.clone();
        super::layout(h.ctx.content_rect(), &doc, &app.show.as_ref().expect("a show").record_view)
    };
    // Walk the frame's paint list: the backdrop must come before everything else, so nothing
    // fills the slide rect after the slide is drawn.
    let mut backdrop = None;
    let mut covering = None;
    for (i, clipped) in h.output().shapes.iter().enumerate() {
        let egui::Shape::Rect(r) = &clipped.shape else { continue };
        if !layout.slide.intersects(r.rect) || r.fill.a() < 250 {
            continue;
        }
        if r.rect.contains_rect(layout.slide) {
            if backdrop.is_none() {
                backdrop = Some(i);
            } else {
                covering = Some(i);
            }
        }
    }
    assert!(backdrop.is_some(), "the frame has a backdrop");
    assert_eq!(covering, None, "no later fill covers the slide (backdrop at {backdrop:?})");
}

/// A focused button must not take the keyboard away from the show: nothing in the Record view
/// edits text, so the navigation keys keep working while a take records.
#[test]
fn a_focused_button_keeps_the_show_navigation_keys_and_the_take() {
    let mut h = harness(app_with(3));
    open_record(&mut h, 0);
    h.get_by_label("Record").click();
    h.run_steps(3);
    assert_eq!(h.state().show.as_ref().unwrap().record_view.status, RecordStatus::Recording);
    // Tab hands the keyboard to a widget (the shell draws only buttons and the read-only notes),
    // which must not take the show's keys away.
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    h.key_press(egui::Key::ArrowRight);
    h.run_steps(3);
    assert_eq!(h.state().show.as_ref().unwrap().state.slide, 1, "ArrowRight turns the page with a button focused");
    assert_eq!(h.state().show.as_ref().unwrap().record_view.status, RecordStatus::Recording, "the take keeps recording across the navigation");
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

/// A wheel over the notes pane scrolls the notes, it does not turn the page: a page turn during a
/// take closes the departing slide's timing segment.
#[test]
fn a_wheel_over_the_notes_does_not_turn_the_page() {
    let mut h = harness(app_with(3));
    open_record(&mut h, 0);
    h.get_by_label("Record").click();
    h.run_steps(3);
    let notes = {
        let app = h.state();
        let full = h.ctx.content_rect();
        let doc = app.session.active().expect("a document").doc.clone();
        super::layout(full, &doc, &app.show.as_ref().expect("a show").record_view).notes
    };
    h.hover_at(notes.center());
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -40.0),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
    });
    h.run_steps(3);
    assert_eq!(h.state().show.as_ref().unwrap().state.slide, 0, "a wheel over the notes is not a page turn");
    assert_eq!(h.state().show.as_ref().unwrap().record_view.status, RecordStatus::Recording, "the take goes on");
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
    assert!(h.query_by_label_contains("Open with the market chart").is_some(), "slide 1 notes");
    assert!(h.query_by_label_contains("Then the roadmap").is_none(), "not the next slide's notes");
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
    let rec = &h.state().show.as_ref().unwrap().record_view.devices;
    assert_eq!(rec.camera.status, devices::DeviceStatus::Unavailable);
    assert_eq!(rec.mic.status, devices::DeviceStatus::Unavailable, "no microphone host");
    assert_eq!(rec.screen.status, devices::DeviceStatus::Unavailable, "no capture host");
    // The buttons cannot turn a missing device on, and they say why they can't.
    let camera = h.get_by_label("Camera — unavailable");
    let node = egui_kittest::kittest::NodeT::accesskit_node(&camera);
    assert!(node.is_disabled(), "an unavailable device is a disabled control");
    assert_ne!(node.toggled(), Some(egui::accesskit::Toggled::True), "and never reports itself on");
    h.get_by_label("Camera — unavailable").click();
    h.run_steps(2);
    let rec = &h.state().show.as_ref().unwrap().record_view.devices;
    assert_eq!(rec.camera.status, devices::DeviceStatus::Unavailable);
    assert_eq!(rec.mic.status, devices::DeviceStatus::Unavailable);
}

#[test]
#[ignore = "writes a screenshot for review"]
fn a_shell_snapshot() {
    let mut h = harness(app_with(3));
    open_record(&mut h, 0);
    let img = h.render().unwrap();
    img.save(std::env::temp_dir().join("record_window_shell.png")).unwrap();
}
