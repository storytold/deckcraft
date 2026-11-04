//! Camera Format ▸ Accessibility: the Alt Text button opens its pane on a Cameo, the description
//! is written through `shape.altText` (one undo step), and decorative is a separate flag.
#![allow(clippy::unwrap_used)]

use deckcraft_engine::Session;
use deckcraft_model::ShapeId;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

use crate::{Services, SlideApp};

/// A blank slide with one selected Cameo, on the Camera Format tab.
fn cameo_app() -> (Harness<'static, SlideApp>, ShapeId) {
    let mut s = Session::with_new();
    s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("insert.cameo", &json!({})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "Camera Format".into();
    let mut h = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    (h, ShapeId(u32::try_from(id).unwrap()))
}

fn descr(h: &Harness<'_, SlideApp>, id: ShapeId) -> String {
    h.state().session.active().unwrap().shape(id).unwrap().descr.clone()
}

#[test]
fn the_alt_text_button_opens_and_closes_the_pane() {
    let (mut h, _) = cameo_app();
    assert!(h.state().ui.pane.is_none());
    h.get_by_role_and_label(Role::Button, "Alt Text").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.pane.as_deref(), Some("altText"));
    assert!(h.query_by_label("Description").is_some(), "the pane has the description field");
    // A second press closes it, as PowerPoint's button does.
    h.get_by_role_and_label(Role::Button, "Alt Text").click();
    h.run_steps(3);
    assert!(h.state().ui.pane.is_none());
}

#[test]
fn typing_a_description_writes_descr_and_undo_reverts_it() {
    let (mut h, id) = cameo_app();
    let original = descr(&h, id);
    h.state_mut().ui.pane = Some("altText".into());
    h.run_steps(3);
    h.get_by_role_and_label(Role::MultilineTextInput, "Description").click();
    h.run_steps(2);
    // The field shows the shape's current description: select it so typing replaces it.
    h.input_mut().events.push(egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers { command: true, ..egui::Modifiers::NONE },
    });
    h.get_by_role_and_label(Role::MultilineTextInput, "Description").type_text("A webcam view of the speaker");
    h.run_steps(2);
    // Leaving the field commits, as in the Format Shape pane.
    h.get_by_label("Home").click();
    h.run_steps(4);
    assert_eq!(descr(&h, id), "A webcam view of the speaker");
    // The commit is one undo step on the cameo's own path.
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(descr(&h, id), original);
}

#[test]
fn marking_the_cameo_decorative_is_undoable_and_disables_the_description() {
    let (mut h, id) = cameo_app();
    h.state_mut().ui.pane = Some("altText".into());
    h.run_steps(3);
    h.get_by_label("Mark as decorative").click();
    h.run_steps(3);
    let sh = h.state().session.active().unwrap().shape(id).unwrap().clone();
    assert!(sh.decorative);
    assert_eq!(sh.descr, "Camera preview", "ticking decorative keeps the description for when it is unticked");
    assert!(
        h.get_by_role_and_label(Role::MultilineTextInput, "Description").accesskit_node().is_disabled(),
        "a decorative object needs no description"
    );
    assert!(h.query_by_label_contains("screen readers skip it").is_some());
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert!(!h.state().session.active().unwrap().shape(id).unwrap().decorative);
}
