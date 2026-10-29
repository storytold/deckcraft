//! Photo Album dialog: the Insert ribbon opens it, and Create inserts the pictures in one step.

use deckcraft_engine::Session;
use deckcraft_model::ShapeKind;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// 8×8 red PNG.
fn png() -> Vec<u8> {
    vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0, 0, 0, 75, 109, 41, 220, 0, 0, 0, 17, 73, 68,
        65, 84, 120, 156, 99, 184, 163, 161, 129, 21, 49, 12, 45, 9, 0, 123, 123, 75, 1, 251, 162, 81, 240, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
        130,
    ]
}

fn harness(app: SlideApp) -> Harness<'static, SlideApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 1200.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn slides(h: &Harness<'_, SlideApp>) -> usize {
    h.state().session.active().map(|d| d.doc.slides.len()).unwrap_or(0)
}

#[test]
fn insert_ribbon_opens_the_dialog_without_editing() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.ui.tab = "Insert".into();
    let mut h = harness(app);
    h.get_by_label("Photo Album…").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("photoAlbum"));
    assert_eq!(slides(&h), 1);
    assert!(h.state().session.active().unwrap().history.undo.is_empty());
}

#[test]
fn create_inserts_the_album_in_one_step() {
    let bytes = png();
    let services = Services {
        pick_open: Some(Box::new(|_| Some("/tmp/cat.png".into()))),
        read: Some(Box::new(move |_| Ok(bytes.clone()))),
        ..Default::default()
    };
    let mut app = SlideApp::new(Session::with_new(), services);
    app.run("insert.photoAlbum", json!({})).expect("open");
    let mut h = harness(app);
    h.get_by_label("Add Pictures…").click();
    h.run_steps(2);
    assert!(h.query_by_label("cat.png").is_some(), "the chosen file is listed");
    h.get_by_label("New Text Box").click();
    h.run_steps(2);
    assert!(h.query_by_label("Text: Text Box").is_some());
    h.get_by_label("Create").click();
    h.run_steps(4);
    assert!(h.state().dialog.is_none(), "Create closes the dialog");
    assert_eq!(slides(&h), 3);
    let st = h.state().session.active().unwrap();
    assert_eq!(st.history.undo.len(), 1);
    assert_eq!(st.history.undo[0].label, "Photo Album…");
    let pics: Vec<_> = st.doc.slides[1].shapes.iter().filter(|sh| matches!(sh.kind, ShapeKind::Picture { .. })).collect();
    assert_eq!(pics.len(), 1);
    assert_eq!(pics[0].line.as_ref().and_then(|l| l.width), Some(1.5));
    let text: Vec<_> = st.doc.slides[2].shapes.iter().filter(|sh| sh.text_box).filter_map(|sh| sh.text.as_ref().map(|t| t.text())).collect();
    assert_eq!(text, ["Text Box"]);
}

/// Updating photo-album data reflows the already-open album without requiring it to be
/// reopened.
#[test]
fn update_reflows_the_open_album() {
    let data = deckcraft_engine::cmd::base64_encode(&png());
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.run("insert.photoAlbum", json!({"pictures": [{"name": "hill.png", "data": data}, {"name": "lake.png", "data": data}], "layout": "1"}))
        .expect("create");
    assert_eq!(slides_of(&app), 3);
    app.run("insert.photoAlbumEdit", json!({})).expect("edit");
    let mut h = harness(app);
    assert!(h.query_by_label("hill.png").is_some(), "the album's pictures are listed");
    assert!(h.query_by_label("Update").is_some());
    h.get_by_label("ALL pictures black and white").click();
    h.run_steps(2);
    let layout = h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Picture layout");
    assert_eq!(layout.value().as_deref(), Some("1 picture"));
    layout.click();
    h.run_steps(2);
    h.get_by_label("2 pictures").click();
    h.run_steps(2);
    h.get_by_label("Update").click();
    h.run_steps(4);
    assert!(h.state().dialog.is_none(), "Update closes the dialog");
    assert_eq!(slides(&h), 2);
    let st = h.state().session.active().unwrap();
    assert_eq!(st.history.undo.last().map(|u| u.label.as_str()), Some("Edit Photo Album…"));
    let pics: Vec<_> = st.doc.slides[1].shapes.iter().filter(|sh| matches!(sh.kind, ShapeKind::Picture { .. })).collect();
    assert_eq!(pics.len(), 2);
    match &pics[0].kind {
        ShapeKind::Picture { fill } => assert!(fill.adjust.grayscale),
        _ => panic!("picture"),
    }
}

fn slides_of(app: &SlideApp) -> usize {
    app.session.active().map(|d| d.doc.slides.len()).unwrap_or(0)
}

#[test]
fn create_with_nothing_keeps_the_dialog() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.run("insert.photoAlbum", json!({})).expect("open");
    let mut h = harness(app);
    h.get_by_label("Create").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("photoAlbum"));
    assert_eq!(slides(&h), 1);
    let status = h.state().status.as_ref().map(|s| s.0.as_str()).unwrap_or("");
    assert!(status.contains("picture") || status.contains("text"), "{status}");
}
