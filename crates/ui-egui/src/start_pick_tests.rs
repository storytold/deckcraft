//! A host with `start_pick` (macOS sheets, #1508) shows file dialogs outside the event handler:
//! the blocking pickers are never called, and what asked for the dialog runs when it answers.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc;

use deckcraft_engine::Session;

use crate::{Services, SlideApp};

/// Save As: the blocking picker is never called, and nothing is saved before the sheet answers.
#[test]
fn save_as_waits_for_the_sheet_and_never_blocks() {
    let blocked = Rc::new(Cell::new(false));
    let b = Rc::clone(&blocked);
    let (tx, rx) = mpsc::channel();
    let rx = Rc::new(Cell::new(Some(rx)));
    let services = Services {
        pick_save: Some(Box::new(move |_| {
            b.set(true);
            None
        })),
        start_pick: Some(Box::new(move |save, _| {
            assert!(save);
            rx.take()
        })),
        ..Services::default()
    };
    let dir = std::env::temp_dir().join("deckcraft-start-pick-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sheet.deckcraft").to_string_lossy().to_string();
    let mut app = SlideApp::new(Session::with_new(), services);

    app.save_as_dialog("deckcraft");
    assert!(app.file_dialog_open(), "the sheet is open");
    app.drain_picks();
    assert!(app.file_dialog_open(), "no answer yet: still open, nothing saved");

    tx.send(Some(path.clone())).unwrap();
    app.drain_picks();
    assert!(!app.file_dialog_open());
    assert!(!blocked.get(), "the blocking picker was never called");
    assert!(std::path::Path::new(&path).exists(), "the answer saved the file");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A cancelled sheet runs nothing.
#[test]
fn a_cancelled_sheet_does_nothing() {
    let (tx, rx) = mpsc::channel();
    let rx = Rc::new(Cell::new(Some(rx)));
    let services = Services { start_pick: Some(Box::new(move |_, _| rx.take())), ..Services::default() };
    let mut app = SlideApp::new(Session::with_new(), services);
    app.pick(false, "open", |_, _| panic!("cancelled"));
    tx.send(None).unwrap();
    app.drain_picks();
    assert!(!app.file_dialog_open());
}

/// An answer that arrives after another presentation was made active runs on the presentation that
/// asked for the dialog.
#[test]
fn an_answer_runs_on_the_presentation_that_asked() {
    let (tx, rx) = mpsc::channel();
    let rx = Rc::new(Cell::new(Some(rx)));
    let services = Services { start_pick: Some(Box::new(move |_, _| rx.take())), ..Services::default() };
    let mut app = SlideApp::new(Session::with_new(), services);
    app.session.execute("file.new", &serde_json::json!({})).unwrap();
    let asked = app.session.active().map(|d| d.uid).unwrap();
    app.pick(true, "x.deckcraft", |_, _| {});
    // another presentation becomes active while the sheet is open
    app.session.set_active(0);
    assert_ne!(app.session.active().map(|d| d.uid), Some(asked));
    let ran_on = Rc::new(Cell::new(None));
    let seen = Rc::clone(&ran_on);
    app.pending_picks[0].then = Box::new(move |app, _| seen.set(app.session.active().map(|d| d.uid)));
    tx.send(Some("/tmp/x.deckcraft".into())).unwrap();
    app.drain_picks();
    assert_eq!(ran_on.get(), Some(asked));
}

/// A second request while a sheet is open is dropped, and the status line says why.
#[test]
fn a_request_while_a_sheet_is_open_says_so() {
    let (_tx, rx) = mpsc::channel();
    let rx = Rc::new(Cell::new(Some(rx)));
    let services = Services { start_pick: Some(Box::new(move |_, _| rx.take())), ..Services::default() };
    let mut app = SlideApp::new(Session::with_new(), services);
    app.pick_free("open", |_, _| {});
    app.pick_free("open", |_, _| {});
    assert_eq!(app.pending_picks.len(), 1);
    assert_eq!(app.status.as_ref().map(|s| s.0.as_str()), Some("A file dialog is already open"));
}

/// A sheet that cannot start leaves nothing pending, and the status line says why.
#[test]
fn a_sheet_that_cannot_start_says_so() {
    let services = Services { start_pick: Some(Box::new(|_, _| None)), ..Services::default() };
    let mut app = SlideApp::new(Session::with_new(), services);
    app.pick_free("open", |_, _| {});
    assert!(app.pending_picks.is_empty());
    assert_eq!(app.status.as_ref().map(|s| s.0.as_str()), Some("The file dialog could not be opened"));
}

/// A sheet whose answer never comes (its thread ended) runs nothing, and the status line says so.
#[test]
fn a_sheet_that_ends_without_an_answer_says_so() {
    let (tx, rx) = mpsc::channel();
    let rx = Rc::new(Cell::new(Some(rx)));
    let services = Services { start_pick: Some(Box::new(move |_, _| rx.take())), ..Services::default() };
    let mut app = SlideApp::new(Session::with_new(), services);
    app.pick_free("open", |_, _| panic!("no answer"));
    drop(tx);
    app.drain_picks();
    assert!(!app.file_dialog_open());
    assert_eq!(app.status.as_ref().map(|s| s.0.as_str()), Some("The file dialog could not be opened"));
}

/// Inserting a file into a presentation with no slide to select binds to the presentation, so the
/// answer runs instead of being refused as a moved selection.
#[test]
fn an_insert_into_a_presentation_with_no_slide_runs() {
    let (tx, rx) = mpsc::channel();
    let rx = Rc::new(Cell::new(Some(rx)));
    let services = Services { start_pick: Some(Box::new(move |_, _| rx.take())), ..Services::default() };
    let mut app = SlideApp::new(Session::with_new(), services);
    while app.session.active().is_some_and(|d| d.current_slide().is_some()) {
        let _ = app.session.execute("slide.delete", &serde_json::json!({}));
        if app.session.active().is_some_and(|d| d.current_slide().is_some()) && app.session.active().map(|d| d.doc.slides.len()) == Some(1) {
            break;
        }
    }
    let ran = Rc::new(Cell::new(false));
    let seen = Rc::clone(&ran);
    app.pick_at("picture", move |_, _| seen.set(true));
    tx.send(Some("/tmp/x.png".into())).unwrap();
    app.drain_picks();
    assert!(ran.get());
}

/// An app whose sheets answer on the channels in `answers`, in the order they are asked for, with
/// `n` untitled presentations that have unsaved changes.
fn untitled_apps(n: usize) -> (SlideApp, Rc<std::cell::RefCell<Vec<mpsc::Sender<Option<String>>>>>) {
    let senders = Rc::new(std::cell::RefCell::new(Vec::new()));
    let kept = Rc::clone(&senders);
    let services = Services {
        start_pick: Some(Box::new(move |_, _| {
            let (tx, rx) = mpsc::channel();
            kept.borrow_mut().push(tx);
            Some(rx)
        })),
        ..Services::default()
    };
    let mut app = SlideApp::new(Session::with_new(), services);
    for i in 0..n {
        if i > 0 {
            app.session.execute("file.new", &serde_json::json!({})).unwrap();
        }
        app.session.execute("slide.new", &serde_json::json!({})).unwrap();
    }
    assert_eq!(app.session.documents().iter().filter(|d| d.is_dirty()).count(), n);
    (app, senders)
}

fn answer(app: &mut SlideApp, senders: &Rc<std::cell::RefCell<Vec<mpsc::Sender<Option<String>>>>>, path: Option<String>) {
    let tx = senders.borrow_mut().pop().unwrap();
    tx.send(path).unwrap();
    app.drain_picks();
}

fn temp_deck(name: &str) -> String {
    let dir = std::env::temp_dir().join("deckcraft-after-save-test");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name).to_string_lossy().to_string()
}

/// Quit › Save with two untitled presentations asks for each in turn, and quits after the last.
#[test]
fn quit_save_goes_on_after_each_sheet_and_quits_at_the_end() {
    let ctx = egui::Context::default();
    let (mut app, senders) = untitled_apps(2);
    app.save_all_then_quit(&ctx);
    assert!(!app.quit_confirmed, "the sheet has not answered");
    app.resume_after_save(&ctx);
    assert!(!app.quit_confirmed, "still waiting for the dialog");

    answer(&mut app, &senders, Some(temp_deck("quit-1.deckcraft")));
    app.resume_after_save(&ctx);
    assert!(!app.quit_confirmed, "one presentation is still unsaved");
    assert!(app.file_dialog_open(), "its dialog is open");

    answer(&mut app, &senders, Some(temp_deck("quit-2.deckcraft")));
    app.resume_after_save(&ctx);
    assert!(app.quit_confirmed);
}

/// Cancelling the Save sheet keeps the app open and ends the wait.
#[test]
fn quit_save_cancelled_does_not_quit() {
    let ctx = egui::Context::default();
    let (mut app, senders) = untitled_apps(1);
    app.save_all_then_quit(&ctx);
    answer(&mut app, &senders, None);
    app.resume_after_save(&ctx);
    assert!(!app.quit_confirmed);
    assert!(!app.file_dialog_open());
    app.resume_after_save(&ctx);
    assert!(!app.quit_confirmed, "the wait ended");
}

/// Close › Save closes the presentation once the sheet has saved it, and not if it was cancelled.
#[test]
fn close_save_closes_after_the_sheet_answers() {
    let ctx = egui::Context::default();
    let (mut app, senders) = untitled_apps(2);
    let before = app.session.documents().len();
    app.save();
    app.close_after_save();
    answer(&mut app, &senders, Some(temp_deck("close.deckcraft")));
    app.resume_after_save(&ctx);
    assert_eq!(app.session.documents().len(), before - 1);

    app.save();
    app.close_after_save();
    answer(&mut app, &senders, None);
    app.resume_after_save(&ctx);
    assert_eq!(app.session.documents().len(), before - 1, "a cancelled save closes nothing");
}

/// Save All keeps saving after a failure, and the status line keeps that failure.
#[test]
fn save_all_keeps_the_first_failure_on_the_status_line() {
    let (mut app, _senders) = untitled_apps(2);
    let missing = std::env::temp_dir().join("deckcraft-no-such-dir").join("a.deckcraft");
    app.session.set_active(0);
    app.session.active_mut().unwrap().path = Some(missing.to_string_lossy().to_string());
    app.session.set_active(1);
    app.session.active_mut().unwrap().path = Some(temp_deck("save-all-2.deckcraft"));
    assert!(!app.save_all());
    assert!(!app.session.documents()[1].is_dirty(), "the one after the failure was saved");
    assert_ne!(app.status.as_ref().map(|s| s.0.as_str()), Some("Saved"));
}
