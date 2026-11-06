//! Format Background pane: picture transparency, tiling, and Apply to All.

use deckcraft_engine::Session;
use deckcraft_model::style::PictureMode;
use deckcraft_model::{Background, Fill, ShapeId};
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

/// Selecting a shape while Format Background is open switches the pane to Format Shape,
/// and deselecting switches back to Format Background.
#[test]
fn format_background_switches_to_format_shape_on_selection_and_back_on_deselect() {
    let mut s = Session::with_new();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 200.0, 120.0]})).unwrap();
    let id = r.get("id").and_then(|v| v.as_u64()).unwrap();
    // Start with deselect so no shape is selected.
    s.execute("edit.select", &json!({"ids": []})).unwrap();

    let mut h = harness(SlideApp::new(s, Services::default()));
    assert_eq!(h.state().ui.pane.as_deref(), Some("background"));
    assert!(h.query_all_by_label("Format Background").next().is_some());
    assert!(h.query_by_label("Follow the layout").is_some());

    // Selecting the shape switches the format pane to "format" (Format Shape).
    h.state_mut().session.execute("edit.select", &json!({"ids": [id]})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().ui.pane.as_deref(), Some("format"));
    assert!(h.query_all_by_label("Format Shape").next().is_some());
    assert!(h.query_by_label("Follow the layout").is_none());

    // Deselecting switches back to "background" (Format Background).
    h.state_mut().session.execute("edit.select", &json!({"ids": []})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().ui.pane.as_deref(), Some("background"));
    assert!(h.query_all_by_label("Format Shape").next().is_none());
    assert!(h.query_by_label("Follow the layout").is_some());
}

/// Explicitly opening the background pane via view.pane clears the active shape selection.
#[test]
fn format_background_command_clears_shape_selection() {
    let mut s = Session::with_new();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 200.0, 120.0]})).unwrap();
    let id = r.get("id").and_then(|v| v.as_u64()).unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();

    let mut app = SlideApp::new(s, Services::default());
    app.ui.pane = Some("format".into());
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 1100.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    assert_eq!(h.state().ui.pane.as_deref(), Some("format"));
    assert_eq!(h.state().session.active().unwrap().selection.shapes.len(), 1);

    // Opening background pane clears shape selection and opens Format Background.
    h.state_mut().run("view.pane", json!({"pane": "background"})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().ui.pane.as_deref(), Some("background"));
    assert!(h.state().session.active().unwrap().selection.shapes.is_empty());
    assert!(h.query_all_by_label("Format Background").next().is_some());
}

/// Panes unrelated to shape formatting (such as the Animation Pane) keep their state
/// when selection changes.
#[test]
fn unrelated_pane_is_not_affected_by_selection_changes() {
    let mut s = Session::with_new();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 200.0, 120.0]})).unwrap();
    let id = r.get("id").and_then(|v| v.as_u64()).unwrap();
    s.execute("edit.select", &json!({"ids": []})).unwrap();

    let mut app = SlideApp::new(s, Services::default());
    app.ui.pane = Some("animation".into());
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 1100.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    assert_eq!(h.state().ui.pane.as_deref(), Some("animation"));

    // Selecting a shape does not change the animation pane.
    h.state_mut().session.execute("edit.select", &json!({"ids": [id]})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().ui.pane.as_deref(), Some("animation"));
}

/// Editing text outside of slide shapes (e.g. speaker notes where selection.text has notes: true)
/// does not trigger Format Shape and keeps Format Background.
#[test]
fn speaker_notes_text_does_not_switch_to_format_shape() {
    let s = Session::with_new();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.pane = Some("background".into());
    if let Some(d) = app.session.active_mut() {
        d.selection.shapes.clear();
        d.selection.text =
            Some(deckcraft_engine::TextSel { shape: ShapeId(0), anchor: (0, 0), caret: (0, 0), cell: None, notes: true, line_end: None });
    }
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 1100.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    assert_eq!(h.state().ui.pane.as_deref(), Some("background"));
    assert!(h.query_all_by_label("Format Background").next().is_some());
    assert!(h.query_by_label("Format Shape").is_none());
}
