//! Add Animation (#1370): the menu scrolls, so the last effect can be reached and added.

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn harness(size: egui::Vec2) -> Harness<'static, SlideApp> {
    let mut session = Session::with_new();
    let id = session.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 200.0, 120.0]})).unwrap()["id"].as_u64().unwrap();
    session.execute("edit.select", &json!({"ids": [id]})).unwrap();
    let mut app = SlideApp::new(session, Services::default());
    app.ui.tab = "Animations".into();
    let mut h = Harness::builder().with_size(size).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn open_add_animation(h: &mut Harness<'static, SlideApp>, window: egui::Vec2) {
    // The Animations ribbon is wider than a short window. The edge chevron is what a person uses
    // to reach Add Animation; scroll-into-view on the button does not move that bar.
    for _ in 0..12 {
        let rect = h.get_by_label("Add Animation").rect();
        if rect.min.x >= 8.0 && rect.max.x <= window.x - 8.0 {
            break;
        }
        let Some(cue) = h.query_by_label("Scroll ribbon right") else { break };
        cue.click();
        h.run_steps(2);
    }
    h.get_by_label("Add Animation").click();
    h.run_steps(3);
}

fn added_effect(h: &Harness<'_, SlideApp>) -> Option<(String, deckcraft_model::anim::AnimClass)> {
    h.state().session.active().and_then(|d| d.current_slide()).and_then(|s| s.animations.first()).map(|a| (a.effect.clone(), a.class))
}

#[test]
fn the_last_effect_is_reachable_by_scrolling() {
    let size = egui::vec2(800.0, 700.0);
    let mut h = harness(size);
    open_add_animation(&mut h, size);
    let before = h.get_by_label("Neutron — Motion").rect();
    assert!(before.min.y > 700.0, "the last effect starts below the window: {before:?}");
    h.get_by_label("Neutron — Motion").scroll_to_me();
    h.run_steps(4);
    let row = h.get_by_label("Neutron — Motion").rect();
    assert!(row.min.y >= 0.0 && row.max.y <= 700.0, "a scrolled effect stays inside the window: {row:?}");
    h.get_by_label("Neutron — Motion").click();
    h.run_steps(3);
    assert_eq!(added_effect(&h), Some(("neutron".into(), deckcraft_model::anim::AnimClass::Path)));
}
