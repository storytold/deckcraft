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

/// #1661: text with an inherited shadow displays the shadow in the Format Shape Text Options
/// rather than showing "Shadow: None", while Shape Effects shows only the shape's own effects.
#[test]
fn inherited_text_shadow_is_shown_in_text_options() {
    let mut s = Session::with_new();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 200.0, 120.0], "text": "Quote"})).unwrap();
    let id = r.get("id").and_then(|v| v.as_u64()).unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();

    // Set an inherited shadow on the master's body style (simulating layout/master inheritance).
    s.edit(|doc, _| {
        let slide = std::sync::Arc::make_mut(&mut doc.slides[0]);
        let sh = slide.shapes.iter_mut().find(|s| s.id.0 as u64 == id).unwrap();
        sh.ph =
            Some(deckcraft_model::Placeholder { kind: deckcraft_model::PhType::Body, idx: 1, vertical: false, size: None, has_custom_prompt: false });
        let master = std::sync::Arc::make_mut(&mut doc.masters[0]);
        let mut lvl = master.body_style.level(0).cloned().unwrap_or_default();
        lvl.run.shadow = Some(deckcraft_model::style::Shadow {
            color: deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Dk1),
            blur: 4.0,
            dist: 3.0,
            dir: 90.0,
            inner: false,
            sx: 1.0,
            sy: 1.0,
            kx: 0.0,
            ky: 0.0,
            align: String::new(),
            rotate_with_shape: false,
        });
        master.body_style.set(0, lvl);
        Ok(())
    })
    .unwrap();

    let mut app = SlideApp::new(s, Services::default());
    app.ui.pane = Some("format".into());
    app.ui.format_tab = "text".into();
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 1100.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);

    // Under Text Options, the text shadow checkbox reflects the inherited shadow and is read-only.
    assert_eq!(h.get_by_label("Text shadow").accesskit_node().toggled(), Some(Toggled::True));
    assert!(h.get_by_label("Text shadow").accesskit_node().is_disabled());
    assert!(h.query_all_by_label("Size").count() >= 1);
    assert!(h.query_all_by_label("Distance").count() >= 1);
    // Switch to Shape Effects tab: the shape has no shape-level shadow, so it shows "No shadow" toggled.
    h.state_mut().ui.format_tab = "effects".into();
    h.run_steps(2);
    assert_eq!(h.get_by_label("No shadow").accesskit_node().toggled(), Some(Toggled::True));
}
