//! Large presentations in the editor: views that list every slide lay out only what is in view.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};
use deckcraft_engine::Session;

fn harness(n: usize) -> egui_kittest::Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    for _ in 1..n {
        s.execute("slide.new", &json!({})).unwrap();
    }
    egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(s, Services::default()),
    )
}

/// Outline view of 2000 slides lays out the slides in view (and the current one), not all 2000
/// title editors every frame (40 ms a frame in a release build, growing with the text).
#[test]
fn the_outline_of_a_huge_deck_lays_out_only_what_is_in_view() {
    let mut h = harness(2000);
    h.state_mut().run("view.outline", json!({})).unwrap();
    h.run_steps(3);
    let editors = h.query_all_by_role(Role::TextInput).count();
    assert!(editors > 5 && editors < 200, "{editors} text editors laid out");
    // The current slide is always laid out, wherever it is.
    h.state_mut().run("slide.go", json!({"index": 1500})).unwrap();
    h.run_steps(3);
    assert!(h.query_all_by_role(Role::TextInput).count() < 200);
    assert_eq!(h.state().session.doc().unwrap().selection.slide, 1500);
}

/// The thumbnail pane of an 800-slide deck renders only the thumbnails in view. When every row
/// asked for one, a deck bigger than the 600-entry thumbnail cache evicted its own thumbnails before
/// the pane was done, so the pane kept rendering and flashing white forever (#969).
#[test]
fn the_thumbnail_pane_of_a_huge_deck_renders_only_the_slides_in_view() {
    let mut h = harness(800);
    h.run_steps(60);
    let settled = h.state().textures.thumb_count();
    assert!(settled > 3 && settled < 60, "{settled} thumbnails rendered");
    // The visible thumbnails are done: more frames render nothing new.
    h.run_steps(10);
    assert_eq!(h.state().textures.thumb_count(), settled);
    // Scrolled far down, the slides now in view get their thumbnails, and only those.
    h.hover_at(egui::pos2(100.0, 450.0));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -40_000.0),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
    });
    h.run_steps(60);
    let scrolled = h.state().textures.thumb_count();
    assert!(scrolled > settled && scrolled < 2 * settled + 10, "{scrolled} thumbnails after scrolling (was {settled})");
}

/// The same in the Slide Sorter: only the cells in view render a thumbnail (#969).
#[test]
fn the_sorter_of_a_huge_deck_renders_only_the_slides_in_view() {
    let mut h = harness(800);
    h.state_mut().ui.view = crate::ViewMode::Sorter;
    h.run_steps(60);
    let settled = h.state().textures.thumb_count();
    assert!(settled > 3 && settled < 80, "{settled} thumbnails rendered");
    h.run_steps(10);
    assert_eq!(h.state().textures.thumb_count(), settled);
}

/// What the count of thumbnails *held* cannot see: a pane that renders the same rows over and
/// over. The cache settles at the rows in view either way, so `thumb_count` is the same whether
/// the pane rendered them once or a thousand times — and the flashing white of #969 was the
/// rendering, not the holding. `thumb_renders` counts the renders, so a settled pane is one that
/// has stopped asking for them.
///
/// A follow-up to #1011, as skylartaylor asked on #1038.
#[test]
fn a_settled_thumbnail_pane_renders_nothing_more() {
    let mut h = harness(800);
    h.run_steps(60);
    let (held, rendered) = (h.state().textures.thumb_count(), h.state().textures.thumb_renders());
    // Every thumbnail held was rendered, and the pane did not render many it then threw away:
    // this is the assertion a count of held thumbnails cannot make, and it comes first so that a
    // thrashing pane is reported as thrashing rather than as holding too few.
    assert!(rendered >= held as u64, "{rendered} renders for {held} thumbnails");
    assert!(rendered < held as u64 * 2, "{rendered} renders for {held} thumbnails: the pane is re-rendering");
    assert!(held > 3 && held < 60, "{held} thumbnails held");

    // Ten more frames of the same view: not one more render.
    h.run_steps(10);
    assert_eq!(h.state().textures.thumb_renders(), rendered, "a settled pane renders nothing more");
    assert_eq!(h.state().textures.thumb_count(), held);

    // Scrolled far down: the slides now in view are rendered, and only those -- the renders go up
    // by about a screen's worth, not by the whole deck.
    h.hover_at(egui::pos2(100.0, 450.0));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -40_000.0),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
    });
    h.run_steps(60);
    let after = h.state().textures.thumb_renders();
    // Measured: 8 held and 8 rendered at rest, 24 rendered after the scroll -- the rows it
    // landed on plus the ones it passed over, not the 800 slides of the deck.
    assert!(after > rendered, "the slides now in view are rendered: {after} vs {rendered}");
    assert!(after < 100, "{after} renders after one scroll of an 800-slide deck (was {rendered})");
    // And it settles again.
    h.run_steps(10);
    assert_eq!(h.state().textures.thumb_renders(), after, "settled again");
}

/// The same for the Slide Sorter: it settles, and stays settled.
#[test]
fn a_settled_sorter_renders_nothing_more() {
    let mut h = harness(800);
    h.state_mut().ui.view = crate::ViewMode::Sorter;
    h.run_steps(60);
    let (held, rendered) = (h.state().textures.thumb_count(), h.state().textures.thumb_renders());
    assert!(rendered >= held as u64 && rendered < held as u64 * 2, "{rendered} renders for {held} thumbnails");
    assert!(held > 3 && held < 80, "{held} thumbnails held");
    h.run_steps(10);
    assert_eq!(h.state().textures.thumb_renders(), rendered, "a settled sorter renders nothing more");
}
