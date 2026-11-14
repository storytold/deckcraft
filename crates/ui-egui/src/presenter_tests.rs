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

#[test]
fn presenter_view_needs_a_second_display() {
    // One display: the presenter window covered the show (or the show covered it) and the
    // hidden window stalled every frame.
    let presenter = |displays: Option<usize>, setting: bool| {
        let screens = move || displays.map(|n| crate::Screens { list: (0..n).collect(), current: Some(0), primary: None });
        let services = Services { screens: Some(Box::new(screens)), ..Default::default() };
        let mut app = SlideApp::new(Session::with_new(), services);
        app.ui.presenter_view = setting;
        app.start_show(0, false);
        app.show.as_ref().map(|s| s.presenter)
    };
    assert_eq!(presenter(Some(1), true), Some(false));
    assert_eq!(presenter(Some(2), true), Some(true));
    assert_eq!(presenter(None, true), Some(true), "a host that can't count displays keeps Presenter View");
    assert_eq!(presenter(Some(2), false), Some(false), "the setting still turns it off");
}
