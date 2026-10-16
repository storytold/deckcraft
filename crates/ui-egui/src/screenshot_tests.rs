//! Insert ▸ Screenshot: the ribbon menu inserts a captured window, and Screen Clipping, as pictures.

use std::sync::Arc;

use deckcraft_engine::Session;
use deckcraft_model::ShapeKind;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp, WindowShot};

/// 2×2 opaque red PNG.
const TINY_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0, 0, 0, 253, 212, 154, 115, 0, 0, 0, 16, 73, 68, 65,
    84, 120, 156, 99, 248, 207, 192, 0, 68, 12, 16, 10, 0, 31, 238, 3, 253, 139, 95, 20, 212, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn shot(title: &str) -> WindowShot {
    WindowShot { title: title.into(), png: Arc::from(TINY_PNG.to_vec()) }
}

fn with_capture(err: Option<&'static str>) -> Services {
    Services {
        capture_windows: Some(Arc::new(move || match err {
            Some(e) => Err(e.into()),
            None => Ok(vec![shot("Calculator"), shot("Notes")]),
        })),
        capture_screen_clip: Some(Arc::new(|| Ok(Some(TINY_PNG.to_vec())))),
        ..Default::default()
    }
}

fn harness(services: Services) -> Harness<'static, SlideApp> {
    let mut app = SlideApp::new(Session::with_new(), services);
    app.ui.tab = "Insert".into();
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn picture_names(h: &Harness<'_, SlideApp>) -> Vec<String> {
    h.state()
        .session
        .active()
        .map(|d| d.shapes().iter().filter(|s| matches!(s.kind, ShapeKind::Picture { .. })).map(|s| s.descr.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn screenshot_menu_inserts_the_chosen_window_and_a_clipping() {
    let mut h = harness(with_capture(None));
    h.get_by_label("Screenshot").click();
    for _ in 0..30 {
        h.run_steps(1);
        if h.query_by_label("Calculator").is_some() {
            break;
        }
    }
    assert!(h.query_by_label("Calculator").is_some(), "the open window is listed");
    assert!(h.query_by_label("Notes").is_some());
    h.get_by_label("Calculator").click();
    h.run_steps(3);
    assert_eq!(picture_names(&h), ["Calculator.png"]);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(2);
    assert!(picture_names(&h).is_empty(), "inserting a screenshot is one undo step");

    h.get_by_label("Screenshot").click();
    for _ in 0..30 {
        h.run_steps(1);
        if h.query_by_label("Screen Clipping").is_some() {
            break;
        }
    }
    h.get_by_label("Screen Clipping").click();
    for _ in 0..30 {
        h.run_steps(1);
        if picture_names(&h) == ["Screen Clipping.png"] {
            break;
        }
    }
    assert_eq!(picture_names(&h), ["Screen Clipping.png"]);
}

#[test]
fn a_capture_failure_is_shown_and_inserts_nothing() {
    let mut h = harness(with_capture(Some("Screen Recording is turned off")));
    h.get_by_label("Screenshot").click();
    for _ in 0..30 {
        h.run_steps(1);
        if h.query_by_label_contains("Screen Recording is turned off").is_some() {
            break;
        }
    }
    assert!(h.query_by_label_contains("Screen Recording is turned off").is_some());
    assert!(picture_names(&h).is_empty());
    let status = h.state().status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
    assert!(status.contains("Screen Recording is turned off"), "{status}");
}

#[test]
fn without_a_capture_service_the_button_does_not_insert() {
    let mut h = harness(Services::default());
    h.get_by_label("Screenshot").click();
    h.run_steps(3);
    assert!(h.query_by_label("Calculator").is_none());
    assert!(picture_names(&h).is_empty());
    let err = h.state_mut().run("app.insertScreenshot", json!({})).unwrap_err();
    assert!(err.contains("isn't available"), "{err}");
}

#[test]
fn the_command_inserts_a_window_by_title() {
    let mut app = SlideApp::new(Session::with_new(), with_capture(None));
    app.run("app.insertScreenshot", json!({"title": "notes"})).unwrap();
    let names = app
        .session
        .active()
        .unwrap()
        .shapes()
        .iter()
        .filter(|s| matches!(s.kind, ShapeKind::Picture { .. }))
        .map(|s| s.descr.clone())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Notes.png"]);
    let err = app.run("app.insertScreenshot", json!({"title": "Nope"})).unwrap_err();
    assert!(err.contains("Nope"), "{err}");
}
