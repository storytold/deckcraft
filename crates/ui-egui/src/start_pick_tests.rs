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
