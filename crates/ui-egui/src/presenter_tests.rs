//! JJ3: `show.presenter` answers the setting and where presenter view appears.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::json;

use crate::{Services, SlideApp};
use deckcraft_engine::Session;

#[test]
fn show_presenter_answers_on_and_window() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    let v = app.run("show.presenter", json!({"on": true})).unwrap();
    assert_eq!(v, json!({"on": true, "window": true}));
    let v = app.run("show.presenter", json!({})).unwrap();
    assert_eq!(v["on"], false, "no `on` toggles: {v}");
    let doc = crate::UI_COMMANDS.iter().find(|c| c.0 == "show.presenter").unwrap().3;
    assert!(doc.contains("separate window"), "{doc}");
}

fn started_presenter(displays: Option<Option<usize>>) -> bool {
    let mut services = Services::default();
    if let Some(n) = displays {
        services.display_count = Some(Box::new(move || n));
    }
    let mut app = SlideApp::new(Session::with_new(), services);
    app.run("show.presenter", json!({"on": true})).unwrap();
    app.run("show.start", json!({})).unwrap();
    app.show.as_ref().expect("show started").presenter
}

/// #1567: on one display the presenter window sat under the full-screen show, and its blocked
/// swapchain held every show frame for wgpu's one-second acquire timeout (video at ~1 fps).
#[test]
fn presenter_view_opens_only_beside_a_second_display() {
    assert!(!started_presenter(Some(Some(1))), "one display: the show alone");
    assert!(started_presenter(Some(Some(2))), "two displays: presenter view beside the show");
    assert!(started_presenter(Some(None)), "count unknown: presenter view as before");
    assert!(started_presenter(None), "host cannot tell (web, tests): presenter view as before");
}
