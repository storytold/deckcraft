//! Textures stay within the renderer's `max_texture_side` (2048 in the test harness, as on many
//! GPUs and software fallbacks): bigger ones panicked in egui. Slide show on a big screen,
//! thumbnails, master views and presenter view of a very tall slide.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::json;

use crate::{Services, SlideApp};
use deckcraft_engine::Session;

fn harness(session: Session, size: egui::Vec2) -> egui_kittest::Harness<'static, SlideApp> {
    egui_kittest::Harness::builder().with_size(size).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(session, Services::default()),
    )
}

/// The tallest slide allowed (1 × 56): its thumbnails and presenter-view preview are tens of
/// thousands of pixels high at their usual widths.
fn tall_deck() -> Session {
    let mut s = Session::with_new();
    s.execute("design.slideSize", &json!({"w": 72, "h": 4032})).unwrap();
    s.execute("slide.new", &json!({})).unwrap();
    s
}

#[test]
fn a_slide_show_on_a_screen_wider_than_the_texture_limit() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let mut h = harness(s, egui::vec2(2600.0, 1500.0));
    h.run_steps(2);
    h.state_mut().run("show.start", json!({})).unwrap();
    h.run_steps(4);
    assert!(h.state().show.is_some());
}

#[test]
fn a_very_tall_slide_in_the_editor_views_and_presenter_view() {
    let mut h = harness(tall_deck(), egui::vec2(1280.0, 800.0));
    h.run_steps(3);
    for (cmd, p) in [("view.sorter", json!({})), ("view.normal", json!({})), ("view.slideMaster", json!({})), ("view.closeMaster", json!({}))] {
        let _ = h.state_mut().run(cmd, p);
        h.run_steps(3);
    }
    h.state_mut().run("show.presenter", json!({"on": true})).unwrap();
    h.state_mut().run("show.start", json!({"from": 0})).unwrap();
    h.run_steps(4);
    assert!(h.state().show.is_some());
}

/// Presenter view of a portrait slide: the "Next" preview keeps the slide's shape inside its
/// column. It took the column's full width and 56 times that in height, a white slab over the
/// notes and past the window.
#[test]
fn presenter_view_keeps_a_portrait_next_slide_in_its_column() {
    let _gpu = crate::gpu_lock::gpu_lock();
    let mut h = harness(tall_deck(), egui::vec2(1280.0, 800.0));
    h.run_steps(3);
    h.state_mut().run("show.presenter", json!({"on": true})).unwrap();
    h.state_mut().run("show.start", json!({"from": 0})).unwrap();
    h.run_steps(4);
    let img = h.render().unwrap();
    // Right of the Next column's narrow preview, below where a landscape one would end.
    let px = img.get_pixel(1000, 600).0;
    assert!(px[0] < 128 && px[1] < 128 && px[2] < 128, "the Next preview spills over the column: {px:?}");
}
