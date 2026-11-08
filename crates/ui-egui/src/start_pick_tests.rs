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
