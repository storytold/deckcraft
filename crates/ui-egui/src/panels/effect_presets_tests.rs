//! Format Shape ▸ Effects: the Shadow, Glow and Soft Edges preset tiles.

use deckcraft_engine::Session;
use deckcraft_model::style::Effects;
use egui::accesskit::Toggled;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

use crate::{Services, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 200.0, 120.0]})).unwrap();
    let id = r.get("id").and_then(|v| v.as_u64()).unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.pane = Some("format".into());
    app.ui.format_tab = "effects".into();
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

fn effects(h: &Harness<'_, SlideApp>) -> Effects {
    h.state().session.active().unwrap().selected_shapes()[0].effects.clone().unwrap_or_default()
}

fn click(h: &mut Harness<'_, SlideApp>, label: &str) {
    h.get_by_label(label).click();
    h.run_steps(3);
}

#[test]
fn preset_tiles_set_shadow_glow_and_soft_edges() {
    let mut h = harness();
    click(&mut h, "Bottom shadow");
    let sw = effects(&h).outer_shadow.unwrap();
    assert_eq!((sw.dir, sw.dist), (90.0, 3.0));
    // The Size slider appears once there is a shadow.
    assert!(h.query_all_by_label("Size").count() >= 2);

    click(&mut h, "Inner shadow");
    let fx = effects(&h);
    assert!(fx.outer_shadow.is_none() && fx.inner_shadow.is_some());
    click(&mut h, "No shadow");
    let fx = effects(&h);
    assert!(fx.outer_shadow.is_none() && fx.inner_shadow.is_none());

    // A radius tile keeps the glow's colour and transparency.
    h.state_mut().session.execute("shape.effects", &json!({"glowProps": {"color": "accent2", "transparency": 0.35, "radius": 8.0}})).unwrap();
    h.run_steps(3);
    let colour = effects(&h).glow.unwrap().color;
    click(&mut h, "18 pt glow");
    let glow = effects(&h).glow.unwrap();
    assert_eq!(glow.radius, 18.0);
    assert_eq!(glow.color, colour);
    click(&mut h, "No glow");
    assert!(effects(&h).glow.is_none());

    click(&mut h, "10 pt soft edges");
    assert_eq!(effects(&h).soft_edge, Some(10.0));
    click(&mut h, "No soft edges");
    assert_eq!(effects(&h).soft_edge, None);
}

#[test]
fn the_current_preset_tile_is_marked_selected() {
    let mut h = harness();
    click(&mut h, "5 pt glow");
    assert_eq!(h.get_by_label("5 pt glow").accesskit_node().toggled(), Some(Toggled::True));
    assert_eq!(h.get_by_label("11 pt glow").accesskit_node().toggled(), Some(Toggled::False));
}
