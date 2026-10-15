//! Picture Format ▸ Corrections: Sharpen/Soften from the ribbon menu and the Format Picture pane.

use deckcraft_engine::Session;
use deckcraft_model::ShapeKind;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

use crate::{Services, SlideApp};

/// A selected picture with fine stripes, on the Picture Format tab.
fn harness() -> Harness<'static, SlideApp> {
    let (w, h) = (240u32, 160u32);
    let pixels = (0..w * h)
        .flat_map(|i| {
            let v: u8 = if (i % w / 6 + i / w / 6) % 2 == 0 { 70 } else { 190 };
            [v, v / 2 + 60, 255 - v, 255]
        })
        .collect();
    let png = deckcraft_render::Image { width: w, height: h, pixels }.to_png();
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("insert.picture", &json!({"name": "stripes.png", "data": deckcraft_engine::cmd::base64_encode(&png), "rect": [200, 100, 480, 320]}))
        .unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "Picture Format".into();
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn sharpen(h: &Harness<'_, SlideApp>) -> f64 {
    match &h.state().session.active().unwrap().selected_shapes()[0].kind {
        ShapeKind::Picture { fill } => fill.adjust.sharpen,
        k => panic!("{k:?}"),
    }
}

#[test]
fn corrections_menu_sharpens_and_softens_in_one_step() {
    let mut h = harness();
    let n = h.state().session.active().unwrap().history.undo.len();
    h.get_by_label_contains("Corrections").click();
    h.run_steps(2);
    assert!(!h.get_by_label("Sharpen 25%").accesskit_node().is_disabled());
    h.get_by_label("Sharpen 25%").click();
    h.run_steps(2);
    assert_eq!(sharpen(&h), 0.25);
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), n + 1);
    h.get_by_label_contains("Corrections").click();
    h.run_steps(2);
    h.get_by_label("Soften 50%").click();
    h.run_steps(2);
    assert_eq!(sharpen(&h), -0.5);
}

#[test]
fn corrections_menu_leaves_a_picture_with_an_artistic_effect_alone() {
    let mut h = harness();
    h.state_mut().session.execute("picture.artisticEffect", &json!({"effect": "artisticBlur"})).unwrap();
    h.run_steps(2);
    let n = h.state().session.active().unwrap().history.undo.len();
    h.get_by_label_contains("Corrections").click();
    h.run_steps(2);
    for label in ["Soften 50%", "Sharpen/Soften 0%", "Sharpen 25%"] {
        assert!(h.get_by_label(label).accesskit_node().is_disabled(), "{label}");
    }
    h.get_by_label("Sharpen 25%").click();
    h.run_steps(2);
    assert_eq!(sharpen(&h), 0.0);
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), n);
}

#[test]
fn format_picture_pane_has_a_sharpness_slider() {
    let mut h = harness();
    h.state_mut().ui.pane = Some("format".into());
    h.state_mut().ui.format_tab = "picture".into();
    h.run_steps(3);
    assert!(h.query_all_by_label_contains("Sharpness").next().is_some());
    assert!(h.query_all_by_label_contains("Brightness").next().is_some(), "beside the other corrections");
}

#[test]
#[ignore = "writes a screenshot for review"]
fn screenshot() {
    let mut h = harness();
    h.state_mut().session.execute("picture.adjust", &json!({"sharpen": 0.5})).unwrap();
    h.state_mut().ui.pane = Some("format".into());
    h.state_mut().ui.format_tab = "picture".into();
    h.run_steps(4);
    h.get_by_label_contains("Corrections").click();
    h.run_steps(4);
    h.render().unwrap().save("/tmp/deckcraft-sharpen.png").unwrap();
}
