//! The document window as the window's one canvas stop: Tab, real presses and its keys.

use deckcraft_engine::Session;
use egui::{Event, Id, Key, Modifiers, PointerButton, Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use serde_json::json;

use crate::framework::document_id;
use crate::state::ViewMode;
use crate::{Services, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut session = Session::with_new();
    session.prefs.autocorrect = false;
    let mut app = SlideApp::new(session, Services::default());
    app.ui.notes = false;
    let mut h = Harness::builder().with_context_setup(crate::framework::install_headless).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn focused(h: &Harness<'_, SlideApp>) -> Option<Id> {
    h.ctx.memory(|m| m.focused())
}

fn press(h: &mut Harness<'_, SlideApp>, modifiers: Modifiers, key: Key) {
    h.key_press_modifiers(modifiers, key);
    h.run_steps(2);
}

fn tab(h: &mut Harness<'_, SlideApp>) {
    press(h, Modifiers::NONE, Key::Tab);
}

fn selected(h: &Harness<'_, SlideApp>) -> Vec<deckcraft_model::ShapeId> {
    h.state().session.active().unwrap().selection.shapes.clone()
}

#[test]
fn tab_selects_the_slides_objects_then_moves_on_and_shift_tab_walks_back() {
    let mut h = harness();
    assert_eq!(focused(&h), Some(document_id()), "an opened presentation has the keyboard");
    let objects: Vec<_> = h.state().session.active().unwrap().shapes().iter().map(|s| s.id).collect();
    assert!(objects.len() > 1);
    for object in &objects {
        tab(&mut h);
        assert_eq!((selected(&h), focused(&h)), (vec![*object], Some(document_id())));
    }
    press(&mut h, Modifiers::SHIFT, Key::Tab);
    let before = focused(&h);
    assert!(before.is_some() && before != Some(document_id()), "with the last object selected, Shift+Tab leaves the slide");
    tab(&mut h);
    assert_eq!(focused(&h), Some(document_id()), "Tab from the stop before it stops at the slide");
    tab(&mut h);
    let after = focused(&h);
    assert!(after.is_some() && after != Some(document_id()) && after != before, "past the last object, Tab moves on");
    press(&mut h, Modifiers::SHIFT, Key::Tab);
    assert_eq!(focused(&h), Some(document_id()), "Shift+Tab walks back to the slide");
    assert_eq!(selected(&h), objects[objects.len() - 1..]);
}

#[test]
fn a_press_on_the_slide_takes_the_keyboard_from_a_focused_button() {
    let mut h = harness();
    h.get_by_role_and_label(Role::Button, "File").focus();
    h.run_steps(2);
    assert!(h.get_by_role_and_label(Role::Button, "File").is_focused());
    let pos = h.state().canvas_rect.unwrap().right_bottom() - vec2(10.0, 10.0);
    h.hover_at(pos);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    assert_eq!(focused(&h), Some(document_id()));
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert!(!h.state().backstage_open(), "the File button isn't activated");
    assert_eq!(h.state().session.active().unwrap().doc.slides.len(), 2, "Enter adds a slide");
}

#[test]
fn after_a_dialog_closes_tab_selects_the_next_object() {
    let mut h = harness();
    h.state_mut().dialog = Some(crate::dialogs::Dialog::new("find"));
    h.run_steps(3);
    assert!(h.ctx.text_edit_focused(), "the dialog's field has the focus");
    h.state_mut().dialog = None;
    h.run_steps(4);
    let first = h.state().session.active().unwrap().shapes()[0].id;
    tab(&mut h);
    assert_eq!((selected(&h), focused(&h)), (vec![first], Some(document_id())));
}

#[test]
fn after_enter_in_a_ribbon_field_tab_selects_the_next_object() {
    let mut h = harness();
    h.state_mut().ui.tab = "Transitions".into();
    tab(&mut h);
    let objects: Vec<_> = h.state().session.active().unwrap().shapes().iter().map(|s| s.id).collect();
    assert_eq!(selected(&h), objects[..1]);
    h.get_by_role_and_label(Role::TextInput, "Duration value").focus();
    h.run_steps(2);
    assert!(h.ctx.text_edit_focused());
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert!(!h.ctx.text_edit_focused(), "Enter commits the field");
    assert!(h.state().session.active().unwrap().selection.text.is_none(), "and reaches nothing on the slide");
    tab(&mut h);
    assert_eq!((selected(&h), focused(&h)), (vec![objects[1]], Some(document_id())));
}

#[test]
fn a_press_in_the_sorter_takes_the_keyboard_from_a_focused_button() {
    let mut h = harness();
    h.state_mut().ui.view = ViewMode::Sorter;
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "File").focus();
    h.run_steps(2);
    assert_ne!(focused(&h), Some(document_id()));
    let pos = h.get_by_role_and_label(Role::Button, "Slide 1").rect().center();
    h.hover_at(pos);
    h.run_steps(1);
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_eq!(focused(&h), Some(document_id()));
}

#[test]
fn edited_text_traps_no_focus_in_the_sorter_or_the_file_tab() {
    let mut h = harness();
    let title = h.state().session.active().unwrap().shapes()[0].id.0;
    h.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();
    h.run_steps(2);
    h.state_mut().ui.view = ViewMode::Sorter;
    h.run_steps(2);
    tab(&mut h);
    let stop = focused(&h);
    assert!(stop.is_some_and(|id| id != document_id()), "Tab moves on from the sorter");
    tab(&mut h);
    assert!(focused(&h).is_some_and(|id| Some(id) != stop && id != document_id()), "and on again");
    h.state_mut().open_backstage(crate::backstage::Page::Home);
    h.run_steps(3);
    tab(&mut h);
    h.run_steps(2);
    assert!(h.state().session.active().unwrap().selection.text.is_some());
    assert!(focused(&h).is_some(), "Tab stays on a control of the File tab");
}

#[test]
fn a_presentation_opened_from_the_palette_gets_the_keyboard() {
    let mut h = harness();
    h.state_mut().run("app.palette", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.ctx.text_edit_focused(), "the palette's field has the focus");
    let app = h.state_mut();
    app.palette = None;
    app.run("file.new", json!({})).unwrap();
    app.run("slide.new", json!({"layout": "blank"})).unwrap();
    assert!(!app.session.tab_has_work(), "an empty slide, which asks for the focus only because it opened");
    h.run_steps(4);
    assert_eq!(focused(&h), Some(document_id()));
}

#[test]
fn the_focused_slide_takes_arrows_escape_typing_and_tab() {
    let mut h = harness();
    let left = |h: &Harness<'_, SlideApp>| {
        let st = h.state().session.active().unwrap();
        deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, &st.shapes()[0]).x
    };
    tab(&mut h);
    let x = left(&h);
    press(&mut h, Modifiers::NONE, Key::ArrowRight);
    assert!(left(&h) > x, "the arrow moves the object, not the focus");
    press(&mut h, Modifiers::NONE, Key::Escape);
    assert!(selected(&h).is_empty());
    assert_eq!(focused(&h), Some(document_id()), "Escape leaves the focus on the slide");
    tab(&mut h);
    h.event(Event::Text("ab".into()));
    h.run_steps(2);
    tab(&mut h);
    h.event(Event::Text("c".into()));
    h.run_steps(2);
    let st = h.state().session.active().unwrap();
    assert_eq!(st.shapes()[0].text.as_ref().unwrap().text(), "ab\tc");
    assert!(st.selection.text.is_some());
    press(&mut h, Modifiers::NONE, Key::Escape);
    assert!(h.state().session.active().unwrap().selection.text.is_none(), "Escape ends text editing");
    assert_eq!(focused(&h), Some(document_id()));
}

#[test]
fn every_view_draws_the_document_and_the_file_tab_takes_its_focus() {
    let mut h = harness();
    for view in [ViewMode::Outline, ViewMode::Reading, ViewMode::NotesPage, ViewMode::Sorter] {
        h.state_mut().ui.view = view;
        h.run_steps(3);
        assert_eq!(focused(&h), Some(document_id()), "the focus stays in the document window");
    }
    tab(&mut h);
    assert!(focused(&h).is_some_and(|id| id != document_id()), "Tab has no work in the slide sorter");
    h.state_mut().ui.view = ViewMode::Normal;
    h.state_mut().open_backstage(crate::backstage::Page::Home);
    h.run_steps(3);
    assert!(h.state().backstage_open());
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(!h.state().backstage_open(), "Escape leaves the File tab");
    assert_eq!(focused(&h), Some(document_id()), "and the slide has the keyboard again");
}
