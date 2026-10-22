//! The canvas's right-click menu, driven like a person would.

use deckcraft_engine::Session;
use egui::{Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// A cube as a glTF binary.
fn cube() -> Vec<u8> {
    let mut p = deckcraft_mesh::Primitive { material: None, ..Default::default() };
    for (n, u, v) in [
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ] {
        let b = p.positions.len() as u32;
        for (su, sv) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            p.positions.push([0, 1, 2].map(|k| 0.5 * (n[k] + su * u[k] + sv * v[k])));
        }
        p.indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    deckcraft_mesh::to_glb(&deckcraft_mesh::Scene { primitives: vec![p], ..Default::default() })
}

/// A blank slide holding a selected 3-D model (and a rectangle when `rect`).
fn harness(rect: bool) -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    if rect {
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [380, 220, 200, 100]})).unwrap();
    } else {
        s.execute("insert.model3d", &json!({"name": "cube.glb", "data": deckcraft_engine::cmd::base64_encode(&cube())})).unwrap();
    }
    let app = SlideApp::new(s, Services::default());
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

/// Right-click the middle of the slide (where the object is).
fn right_click_middle(h: &mut Harness<'_, SlideApp>) {
    let r = h.state().slide_rect.unwrap();
    // Off the 3-D rotation handle, still on the object.
    let pos = Pos2::new(r.center().x + 30.0, r.center().y + 25.0);
    h.event(egui::Event::PointerMoved(pos));
    h.event(egui::Event::PointerButton { pos, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE });
    h.event(egui::Event::PointerButton { pos, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

#[test]
fn a_3d_model_gets_its_own_menu() {
    let mut h = harness(false);
    right_click_middle(&mut h);
    for item in ["Save as Picture…", "View Alt Text…", "Size and Position…", "Format 3D Model…", "New Comment", "Lock", "Link…"] {
        assert!(h.query_by_label(item).is_some(), "{item} missing");
    }
    // Nothing a model can't do.
    for item in ["Edit Text", "Set as Default Shape", "Format Shape…"] {
        assert!(h.query_by_label(item).is_none(), "{item} offered");
    }
    // Format 3D Model… opens the Format pane at the model's page.
    h.get_by_label("Format 3D Model…").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.pane.as_deref(), Some("format"));
    assert_eq!(h.state().ui.format_tab, "model3d");
}

#[test]
fn other_shapes_keep_edit_text() {
    let mut h = harness(true);
    right_click_middle(&mut h);
    assert!(h.query_by_label("Edit Text").is_some());
    assert!(h.query_by_label("Format Shape…").is_some());
}
