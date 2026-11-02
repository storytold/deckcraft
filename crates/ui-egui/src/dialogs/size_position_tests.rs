//! Size and Position dialog: the Shape Format launcher opens it, and OK applies one undo step.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(deckcraft_engine::Session::with_new(), Services::default()),
    );
    h.run_steps(2);
    h
}

fn box_of(app: &SlideApp, id: u64) -> (f64, f64, f64, f64, f64) {
    let st = app.session.active().unwrap();
    let x = st.shape(deckcraft_model::ShapeId(id as u32)).and_then(|s| s.xfrm).unwrap();
    (x.x, x.y, x.w, x.h, x.rot)
}

#[test]
fn size_button_sets_width_in_one_undo() {
    let mut h = harness();
    let id = h.state_mut().session.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 50.0, 200.0, 100.0]})).unwrap()["id"]
        .as_u64()
        .unwrap();
    h.state_mut().ui.tab = "Shape Format".into();
    h.run_steps(2);
    h.get_by_label("Size and Position…").click();
    h.run_steps(2);
    let dlg = h.state().dialog.as_ref().unwrap();
    assert_eq!(dlg.id, "sizePosition");
    assert_eq!(dlg.fields.get("w").map(String::as_str), Some("2.78"));
    assert_eq!(dlg.fields.get("h").map(String::as_str), Some("1.39"));
    assert_eq!(dlg.fields.get("lock").map(String::as_str), Some("0"));
    h.state_mut().dialog.as_mut().unwrap().fields.insert("w".into(), "4".into());
    h.run_steps(1);
    h.get_by_label("OK").click_accesskit();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    assert_eq!(box_of(h.state(), id), (40.0, 50.0, 288.0, 100.0, 0.0));
    let undo = &h.state().session.active().unwrap().history.undo;
    assert_eq!(undo.last().unwrap().label, "Size and Position…");
    let n = undo.len();
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(box_of(h.state(), id), (40.0, 50.0, 200.0, 100.0, 0.0));
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), n - 1);
}

#[test]
fn lock_aspect_from_the_dialog_scales_height() {
    let mut h = harness();
    let id = h.state_mut().session.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 50.0, 200.0, 100.0]})).unwrap()["id"]
        .as_u64()
        .unwrap();
    h.state_mut().run("shape.sizePosition", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().unwrap().id, "sizePosition");
    {
        let fields = &mut h.state_mut().dialog.as_mut().unwrap().fields;
        fields.insert("lock".into(), "1".into());
        fields.insert("w".into(), "4".into());
    }
    h.get_by_label("OK").click_accesskit();
    h.run_steps(2);
    assert_eq!(box_of(h.state(), id), (40.0, 50.0, 288.0, 144.0, 0.0));
}
