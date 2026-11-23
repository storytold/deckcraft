//! Dialogs: the shared frame (Esc, Enter, multi-line text), Paragraph opening on the paragraph's
//! values, and Preferences changing nothing until OK or Apply.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::Dialog;
use crate::{Services, SlideApp};

fn harness(dialog: &str) -> Harness<'static, SlideApp> {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.dialog = Some(Dialog::new(dialog));
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn key(h: &mut Harness<'static, SlideApp>, key: egui::Key, modifiers: egui::Modifiers) {
    h.event(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
    h.event(egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
    h.run_steps(3);
}

#[test]
fn escape_closes_and_the_title_names_the_dialog() {
    let mut h = harness("table");
    assert!(h.query_by_label("Insert Table").is_some());
    key(&mut h, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(h.state().dialog.is_none());
}

#[test]
fn enter_is_a_new_line_in_a_comment_and_command_enter_posts_it() {
    let mut h = harness("comment");
    h.state_mut().dialog.as_mut().unwrap().fields.insert("text".into(), "Looks good".into());
    h.run_steps(2);
    key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
    assert!(h.state().dialog.is_some(), "Enter alone doesn't post");
    key(&mut h, egui::Key::Enter, egui::Modifiers::COMMAND);
    assert!(h.state().dialog.is_none(), "⌘Enter posts");
}
