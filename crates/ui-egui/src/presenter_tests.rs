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
