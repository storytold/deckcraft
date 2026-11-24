//! The tab strip under the title bar: one tab per open presentation, shown from two upward.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use egui_kittest::kittest::Queryable;
use serde_json::json;

use super::{Services, SlideApp};
use crate::Session;

fn harness(decks: usize) -> egui_kittest::Harness<'static, SlideApp> {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    for _ in 1..decks {
        app.session.execute("file.new", &json!({})).unwrap();
    }
    egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    )
}

fn titles(h: &egui_kittest::Harness<'static, SlideApp>) -> Vec<String> {
    h.state().session.documents().iter().map(|d| d.title()).collect()
}

#[test]
fn one_presentation_has_no_tab_strip() {
    let mut h = harness(1);
    h.run();
    let name = titles(&h)[0].clone();
    assert!(h.query_by_label(&format!("Close {name}")).is_none());
}

#[test]
fn two_presentations_show_a_tab_each_and_clicking_switches() {
    let mut h = harness(2);
    h.run();
    let names = titles(&h);
    assert_eq!(h.state().session.active_index(), Some(1));
    h.get_by_label(&names[0]).click();
    h.run();
    assert_eq!(h.state().session.active_index(), Some(0));
    h.get_by_label(&names[1]).click();
    h.run();
    assert_eq!(h.state().session.active_index(), Some(1));
}

#[test]
fn the_x_closes_that_tab_and_the_strip_goes_with_the_last_but_one() {
    let mut h = harness(2);
    h.run();
    let names = titles(&h);
    h.get_by_label(&format!("Close {}", names[0])).click();
    h.run();
    assert_eq!(titles(&h), vec![names[1].clone()]);
    assert!(h.query_by_label(&format!("Close {}", names[1])).is_none());
}

#[test]
fn closing_an_unsaved_tab_asks_first() {
    let mut h = harness(2);
    h.run();
    let names = titles(&h);
    h.state_mut().session.set_active(0);
    h.state_mut().session.execute("slide.duplicate", &json!({})).unwrap();
    assert!(h.state().session.documents()[0].is_dirty());
    h.run();
    h.get_by_label(&format!("Close {}", names[0])).click();
    h.run();
    assert_eq!(h.state().session.documents().len(), 2, "nothing closed yet");
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("close"));
}
