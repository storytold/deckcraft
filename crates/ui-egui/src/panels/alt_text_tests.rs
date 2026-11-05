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

/// Focus the description field and replace its text, as `typing_a_description_writes_descr_and_undo_reverts_it`
/// does, without leaving it.
fn type_description(h: &mut Harness<'_, SlideApp>, text: &str) {
    h.get_by_role_and_label(Role::MultilineTextInput, "Description").click();
    h.run_steps(2);
    h.input_mut().events.push(egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers { command: true, ..egui::Modifiers::NONE },
    });
    h.get_by_role_and_label(Role::MultilineTextInput, "Description").type_text(text);
    h.run_steps(2);
}

#[test]
fn closing_the_pane_commits_the_typed_description() {
    let (mut h, id) = cameo_app();
    let original = descr(&h, id);
    h.state_mut().ui.pane = Some("altText".into());
    h.run_steps(3);
    type_description(&mut h, "A webcam view of the speaker");
    // The field is still drawn and still holds the draft: nothing has been written yet.
    assert_eq!(descr(&h, id), original);
    assert!(h.state().ui.alt_text_draft.is_some(), "the pane keeps the typed text");
    // The ribbon button closes the pane without the field ever losing focus to another widget.
    h.get_by_role_and_label(Role::Button, "Alt Text").click();
    h.run_steps(4);
    assert!(h.state().ui.pane.is_none());
    assert_eq!(descr(&h, id), "A webcam view of the speaker", "closing the pane keeps what was typed");
    assert!(h.state().ui.alt_text_draft.is_none());
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(descr(&h, id), original, "and the commit is one undo step");
}

#[test]
fn selecting_another_object_commits_the_typed_description() {
    let (mut h, id) = cameo_app();
    let other =
        h.state_mut().session.execute("insert.textBox", &json!({"rect": [40, 400, 200, 80], "text": "Other"})).unwrap()["id"].as_u64().unwrap();
    // Inserting the box selects it; go back to the Cameo so the pane describes it.
    h.state_mut().session.execute("edit.select", &json!({"ids": [id.0]})).unwrap();
    h.state_mut().ui.pane = Some("altText".into());
    h.run_steps(3);
    type_description(&mut h, "Kept on selection change");
    // Selecting the text box takes the pane away from the Cameo in the same frame.
    h.state_mut().session.execute("edit.select", &json!({"ids": [other]})).unwrap();
    h.run_steps(3);
    assert_eq!(descr(&h, id), "Kept on selection change");
}

#[test]
fn ticking_decorative_with_a_typed_description_is_one_undo_step() {
    let (mut h, id) = cameo_app();
    let original = descr(&h, id);
    h.state_mut().ui.pane = Some("altText".into());
    h.run_steps(3);
    type_description(&mut h, "Typed over");
    h.get_by_label("Mark as decorative").click();
    h.run_steps(3);
    let sh = h.state().session.active().unwrap().shape(id).unwrap().clone();
    assert!(sh.decorative);
    assert_eq!(sh.descr, "Typed over", "the click carries the typed description");
    // One undo puts back both the flag and the description.
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    let sh = h.state().session.active().unwrap().shape(id).unwrap().clone();
    assert!(!sh.decorative, "one undo step, not two");
    assert_eq!(sh.descr, original);
}

#[test]
fn a_view_that_hides_the_pane_commits_the_typed_description() {
    let (mut h, id) = cameo_app();
    h.state_mut().ui.pane = Some("altText".into());
    h.run_steps(3);
    type_description(&mut h, "Typed before the sorter");
    // Slide Sorter hides the task pane but leaves its key set: the pane stops drawing, so its
    // draft is committed even though nothing about the pane's state changes.
    h.state_mut().ui.view = crate::state::ViewMode::Sorter;
    h.run_steps(3);
    assert_eq!(h.state().ui.pane.as_deref(), Some("altText"));
    assert_eq!(descr(&h, id), "Typed before the sorter");
}

#[test]
fn changing_slides_commits_the_description_to_the_object_it_was_typed_for() {
    let (mut h, id) = cameo_app();
    // A second slide, then back to the first with the Cameo selected.
    h.state_mut().run("slide.new", json!({})).unwrap();
    let first = h.state().session.active().unwrap().doc.slides[0].id;
    h.state_mut().run("slide.selectSlides", json!({"ids": [first.0]})).unwrap();
    h.state_mut().run("edit.select", json!({"ids": [id.0]})).unwrap();
    h.state_mut().ui.pane = Some("altText".into());
    h.run_steps(3);
    type_description(&mut h, "Typed before the slide changed");
    // Opening another slide commits first: the description lands on the slide it belongs to.
    h.state_mut().run("slide.new", json!({})).unwrap();
    h.run_steps(2);
    let st = h.state().session.active().unwrap();
    assert_ne!(st.selection.slide, 0, "the new slide is open");
    let kept = st.doc.slides[0].shape(id).map(|s| s.descr.clone());
    assert_eq!(kept.as_deref(), Some("Typed before the slide changed"));
}

#[test]
fn closing_with_a_typed_description_treats_it_as_unsaved_work() {
    let (mut h, id) = cameo_app();
    let original = descr(&h, id);
    h.state_mut().ui.pane = Some("altText".into());
    h.run_steps(3);
    type_description(&mut h, "Typed and never left");
    assert_eq!(descr(&h, id), original, "typing alone edits nothing");
    h.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    h.run_steps(2);
    assert_eq!(descr(&h, id), "Typed and never left", "the close request writes the description first");
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("quit"), "and the close question appears");
}
