//! Format Background pane: picture transparency, tiling, and Apply to All.

use deckcraft_engine::Session;
use deckcraft_model::style::PictureMode;
use deckcraft_model::{Background, Fill};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// 1×1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

fn harness(mut app: SlideApp) -> Harness<'static, SlideApp> {
    app.ui.tab = "Design".into();
    app.ui.pane = Some("background".into());
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 1100.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn picture_mode(h: &Harness<'_, SlideApp>) -> PictureMode {
    match &h.state().session.active().unwrap().doc.slides[0].background {
        Some(Background::Fill { fill: Fill::Picture(pf) }) => pf.mode.clone(),
        other => panic!("expected a picture background, got {other:?}"),
    }
}

#[test]
fn picture_fill_shows_transparency_and_tiling() {
    let mut s = Session::with_new();
    s.execute("design.background", &json!({"picture": PNG, "name": "dot.png"})).unwrap();
    let mut h = harness(SlideApp::new(s, Services::default()));
    assert!(h.query_all_by_label("Format Background").next().is_some(), "the pane is open");
    let _ = h.get_by_label("Picture or texture fill");
    assert!(h.query_all_by_label("Transparency").next().is_some());
    h.get_by_label("Tile picture as texture").click();
    h.run_steps(2);
    let _ = h.get_by_label("Offset X");
    let _ = h.get_by_label("Scale X");
    let _ = h.get_by_label("Alignment");
    let _ = h.get_by_label("Mirror");
    assert!(matches!(picture_mode(&h), PictureMode::Tile { .. }));
}

#[test]
fn solid_fill_shows_transparency_without_tile_options() {
    let mut s = Session::with_new();
    s.execute("design.background", &json!({"color": "accent1"})).unwrap();
    let h = harness(SlideApp::new(s, Services::default()));
    assert!(h.query_all_by_label("Transparency").next().is_some());
    let _ = h.get_by_label("Solid fill");
    assert!(h.query_by_label("Tile picture as texture").is_none());
}

#[test]
fn apply_to_all_copies_the_picture() {
    let mut s = Session::with_new();
    s.execute("design.background", &json!({"picture": PNG, "name": "dot.png"})).unwrap();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("slide.go", &json!({"index": 0})).unwrap();
    let mut h = harness(SlideApp::new(s, Services::default()));
    h.get_by_label("Apply to All").click();
    h.run_steps(2);
    let doc = &h.state().session.active().unwrap().doc;
    assert!(matches!(doc.slides[1].background, Some(Background::Fill { fill: Fill::Picture(_) })));
    assert_eq!(doc.slides[1].background, doc.slides[0].background);
    assert_eq!(doc.masters[0].background, doc.slides[0].background);
}
