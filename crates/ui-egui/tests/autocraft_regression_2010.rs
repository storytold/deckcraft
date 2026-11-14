//! #2010: the Size and Position dialog follows the measurement unit chosen in Preferences
//! instead of always showing and taking inches.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use deckcraft_engine::Session;
use deckcraft_geom::Unit;
use deckcraft_ui_egui::dialogs::Dialog;
use deckcraft_ui_egui::{Services, SlideApp};

/// A blank slide with one 2" x 1.5" rectangle at (1", 0.5"), selected.
fn dialog_harness(units: Unit) -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [72.0, 36.0, 144.0, 108.0]})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    s.prefs.units = units;
    let mut app = SlideApp::new(s, Services::default());
    app.dialog = Some(Dialog::new("sizePosition"));
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn selected_xfrm(app: &SlideApp) -> deckcraft_geom::Xfrm {
    let st = app.session.active().unwrap();
    let sh = st.shape(*st.selection.shapes.first().unwrap()).unwrap();
    deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh)
}

#[test]
fn size_and_position_dialog_shows_and_takes_centimetres() {
    let mut h = dialog_harness(Unit::Centimeters);
    assert!(h.query_by_label("Height (cm):").is_some() && h.query_by_label("Width (cm):").is_some());
    assert!(h.query_by_label("Horizontal (cm):").is_some() && h.query_by_label("Vertical (cm):").is_some());
    assert!(h.query_by_label("Height (in):").is_none());
    // 108 pt = 3.81 cm, 144 pt = 5.08 cm, 72 pt = 2.54 cm, 36 pt = 1.27 cm.
    let fields = &h.state().dialog.as_ref().unwrap().fields;
    let shown = |k: &str| fields.get(k).cloned().unwrap_or_default();
    assert_eq!((shown("h"), shown("w"), shown("x"), shown("y")), ("3.81".into(), "5.08".into(), "2.54".into(), "1.27".into()));
    // Typing 2.54 into Width makes the shape one inch wide, not 2.54 inches.
    h.state_mut().dialog.as_mut().unwrap().fields.insert("w".into(), "2.54".into());
    h.run_steps(1);
    h.get_by_label("OK").click_accesskit();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    let x = selected_xfrm(h.state());
    assert!((x.w - 72.0).abs() < 1e-6, "{x:?}");
    assert!((x.h - 108.0).abs() < 1e-6 && (x.x - 72.0).abs() < 1e-6 && (x.y - 36.0).abs() < 1e-6, "{x:?}");
}

#[test]
fn size_and_position_dialog_keeps_inches_by_default() {
    let h = dialog_harness(Unit::Inches);
    assert!(h.query_by_label("Height (in):").is_some());
    let fields = &h.state().dialog.as_ref().unwrap().fields;
    assert_eq!((fields.get("w").map(String::as_str), fields.get("h").map(String::as_str)), (Some("2"), Some("1.5")));
}
