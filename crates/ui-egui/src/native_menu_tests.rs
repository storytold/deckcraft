//! The menu bar: every item runs something that exists, shortcuts read as a Mac shows them, key
//! equivalents come back to egui as the keys they were, and the in-window bar runs commands.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::rc::Rc;

use deckcraft_engine::Session;
use serde_json::json;

use super::*;
use crate::Services;

#[test]
fn every_item_is_a_command_and_the_mac_menus_are_in_place() {
    let app = SlideApp::new(Session::with_new(), Services::default());
    let bar = menu_bar(&app);
    let titles: Vec<&str> = bar.menus.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["DeckCraft", "File", "Edit", "Insert", "Slide", "Format", "Arrange", "View", "Play", "Window", "Help"]);
    for it in bar.items() {
        let known = own(&it.id) || deckcraft_engine::find_command(&it.id).is_some() || crate::UI_COMMANDS.iter().any(|c| c.0 == it.id);
        assert!(known, "{} ({}) isn't a command", it.label, it.id);
    }
    assert_eq!(bar.find("edit.undo").and_then(|i| i.shortcut.clone()).as_deref(), Some("Cmd+Z"));
    assert_eq!(bar.find("arrange.group").and_then(|i| i.shortcut.clone()).as_deref(), Some("Cmd+G"));
    assert_eq!(bar.find(QUIT).map(|i| i.enabled), Some(true));
    assert!(bar.items().iter().any(|i| i.id == "window.doc.0" && i.checked == Some(true)), "the open presentation, checked");
}

#[test]
fn chords_read_as_a_mac_shows_them() {
    assert_eq!(Chord::parse("Cmd+Shift+Z").unwrap().symbols(), "⇧⌘Z");
    assert_eq!(Chord::parse("Cmd+Alt+]").unwrap().symbols(), "⌥⌘]");
    assert_eq!(Chord::parse("Cmd+Shift+Enter").unwrap().symbols(), "⇧⌘↩");
    assert!(!Chord::parse("Alt+Shift+C").unwrap().native_ok(), "no ⌘: stays egui's");
    assert!(Chord::parse("F5").unwrap().native_ok());
}

struct Fake {
    events: Rc<RefCell<Vec<Event>>>,
    synced: Rc<RefCell<usize>>,
}

impl Backend for Fake {
    fn sync(&mut self, _bar: &MenuBar) {
        *self.synced.borrow_mut() += 1;
    }
    fn drain(&mut self) -> Vec<Event> {
        std::mem::take(&mut *self.events.borrow_mut())
    }
}

#[test]
fn key_equivalents_return_to_egui_and_clicks_run_commands() {
    let events = Rc::new(RefCell::new(vec![]));
    let synced = Rc::new(RefCell::new(0));
    let mut menu = NativeMenu::new(Box::new(Fake { events: events.clone(), synced: synced.clone() }), Box::new(|| Some("pasted".into())));
    events.borrow_mut().extend([
        Event::Key(Chord::parse("Cmd+C").unwrap()),
        Event::Key(Chord::parse("Cmd+V").unwrap()),
        Event::Key(Chord::parse("Cmd+D").unwrap()),
        Event::Click("slide.new".into()),
    ]);
    let mut raw = egui::RawInput::default();
    menu.raw_input(&mut raw);
    assert!(matches!(raw.events.first(), Some(egui::Event::Copy)), "{:?}", raw.events);
    assert!(raw.events.iter().any(|e| matches!(e, egui::Event::Paste(t) if t == "pasted")));
    assert!(raw.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::D, pressed: true, modifiers, .. } if modifiers.command)));
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.native_menu = Some(menu);
    let ctx = egui::Context::default();
    let before = app.session.active().unwrap().doc.slides.len();
    let mut out = ctx.run_ui(Default::default(), |ui| run(&mut app, ui.ctx()));
    out.textures_delta.clear();
    assert_eq!(app.session.active().unwrap().doc.slides.len(), before + 1, "the click ran New Slide");
    assert!(*synced.borrow() >= 1, "the menu was brought up to date");
}

#[test]
fn the_in_window_bar_runs_its_items() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.run("view.shell", json!({"shell": "canvas"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    use egui_kittest::kittest::Queryable;
    h.get_by_label("Insert").click();
    h.run_steps(3);
    let before = h.state().session.active().unwrap().doc.slides.len();
    h.query_all_by_label_contains("New Slide").next().expect("the Insert menu is open").click();
    h.run_steps(3);
    assert_eq!(h.state().session.active().unwrap().doc.slides.len(), before + 1);
}
