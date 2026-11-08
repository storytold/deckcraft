//! A host with `start_pick` (macOS sheets, #1508) shows file dialogs outside the event handler:
//! the blocking pickers are never called, and what asked for the dialog runs when it answers.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc;

use deckcraft_engine::Session;

use crate::{Services, SlideApp};

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
