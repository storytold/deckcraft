//! Camera Format: the contextual tab of a Cameo, its sections and the Camera Styles gallery.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use deckcraft_model::ShapeId;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// A blank deck with one selected Cameo, on the Camera Format tab, in a `width` px window.
fn cameo_app(width: f32) -> (Harness<'static, SlideApp>, ShapeId) {
    let mut s = Session::with_new();
    s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("insert.cameo", &json!({"rect": [200, 100, 162, 162]})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "Camera Format".into();
    let mut h = Harness::builder().with_size(egui::vec2(width, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    (h, ShapeId(u32::try_from(id).unwrap()))
}

fn cameos(app: &SlideApp) -> Vec<deckcraft_model::Shape> {
    app.session
        .active()
        .map(|st| {
            st.doc
                .slides
                .iter()
                .flat_map(|sl| {
                    let mut v = vec![];
                    deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
                        if sh.is_cameo() {
                            v.push(sh.clone());
                        }
                    });
                    v
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_cameo_shows_camera_format_and_not_picture_format() {
    let (h, _) = cameo_app(1440.0);
    let tabs = super::contextual(h.state());
    assert!(tabs.contains(&"Camera Format"), "{tabs:?}");
    assert!(!tabs.contains(&"Picture Format"), "{tabs:?}");
    assert!(h.query_by_label("Camera Format").is_some(), "the tab is drawn");
}

#[test]
fn a_plain_picture_keeps_picture_format_and_has_no_camera_tab() {
    let mut s = Session::with_new();
    s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
    let pixels = (0..4).flat_map(|_| [40, 120, 200, 255]).collect();
    let png = deckcraft_render::Image { width: 2, height: 2, pixels }.to_png();
    let data = deckcraft_engine::cmd::base64_encode(&png);
    s.execute("insert.picture", &json!({"data": data, "name": "a.png", "rect": [100, 100, 200, 150]})).unwrap();
    let app = SlideApp::new(s, Services::default());
    let tabs = super::contextual(&app);
    assert!(tabs.contains(&"Picture Format"), "{tabs:?}");
    assert!(!tabs.contains(&"Camera Format"), "{tabs:?}");
}

#[test]
fn the_tab_wires_the_camera_and_picture_commands() {
    for width in [1600.0, 1920.0] {
        let (h, _) = cameo_app(width);
        for label in ["Apply to All Slides", "Camera Shape", "Camera Border", "Camera Effects", "Alt Text", "Crop", "Height value", "Width value"] {
            assert!(h.query_by_label(label).is_some(), "{label} missing at {width} px");
        }
        for style in deckcraft_engine::cmd::camera::STYLES.iter() {
            assert!(h.query_by_label(style.label).is_some(), "{} missing at {width} px", style.label);
        }
    }
}

#[test]
fn the_styles_gallery_applies_a_preset_once_and_undoably() {
    let (mut h, id) = cameo_app(1920.0);
    let undo = h.state().session.active().unwrap().history.undo.len();
    h.get_by_label("Center Shadow Circle").click();
    h.run_steps(2);
    let st = h.state().session.active().unwrap();
    let sh = st.doc.slides[0].shape(id).expect("cameo");
    assert_eq!(sh.geom.preset_name(), Some("ellipse"));
    assert!(sh.effects.as_ref().and_then(|e| e.outer_shadow.as_ref()).is_some());
    assert_eq!(st.history.undo.len(), undo + 1);
}

#[test]
fn apply_to_all_slides_styles_every_cameo() {
    let mut s = Session::with_new();
    s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("insert.cameo", &json!({"all": true})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "Camera Format".into();
    let mut h = Harness::builder().with_size(egui::vec2(1920.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("Center Shadow Rectangle").click();
    h.run_steps(2);
    h.get_by_label("Apply to All Slides").click();
    h.run_steps(2);
    let shapes = cameos(h.state());
    assert_eq!(shapes.len(), 2);
    for sh in shapes {
        assert_eq!(sh.geom.preset_name(), Some("rect"));
        assert!(sh.effects.as_ref().and_then(|e| e.outer_shadow.as_ref()).is_some());
    }
}
