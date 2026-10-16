//! Save As on the title bar and with ⇧⌘S: a new file, and the one already saved left as it was.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::rc::Rc;

use deckcraft_engine::Session;
use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use super::{Services, SlideApp};

fn harness(app: SlideApp) -> Harness<'static, SlideApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("deckcraft-saveas-ui-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A save dialog that records the suggested name and returns `path` (`None` cancels).
fn saving_as(path: Option<String>, log: Rc<RefCell<Vec<String>>>) -> Services {
    Services {
        pick_save: Some(Box::new(move |suggested: &str| {
            log.borrow_mut().push(suggested.to_string());
            path.clone()
        })),
        ..Default::default()
    }
}

fn doc_text(s: &Session) -> String {
    s.doc()
        .unwrap()
        .doc
        .slides
        .iter()
        .flat_map(|sl| sl.shapes.iter())
        .filter_map(|sh| sh.text.as_ref().map(|t| t.text()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn open_text(path: &str) -> String {
    let mut s = Session::new();
    s.execute("file.open", &json!({"path": path})).unwrap();
    doc_text(&s)
}

/// Title bar Save As writes a PowerPoint copy and does not touch the file already saved.
#[test]
fn title_bar_save_as_copies_a_pptx_and_leaves_the_original() {
    let dir = temp_dir("pptx");
    let original = dir.join("Talk.pptx").to_string_lossy().into_owned();
    let copy = dir.join("Talk copy.pptx").to_string_lossy().into_owned();
    let mut session = Session::with_new();
    session.execute("file.saveAs", &json!({"path": original})).unwrap();
    let before = std::fs::read(&original).unwrap();
    session.execute("slide.new", &json!({"title": "Kept aside"})).unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut h = harness(SlideApp::new(session, saving_as(Some(copy.clone()), log.clone())));
    assert_eq!(h.query_all_by_label("Save As…").count(), 1);
    h.get_by_label("Save As…").click();
    h.run_steps(3);
    assert_eq!(log.borrow().as_slice(), ["Talk.pptx"]);
    assert_eq!(std::fs::read(&original).unwrap(), before, "the original file is not rewritten");
    assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some(copy.as_str()));
    assert!(!h.state().session.active().unwrap().is_dirty());
    assert!(!open_text(&original).contains("Kept aside"));
    assert!(open_text(&copy).contains("Kept aside"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⇧⌘S on a DeckCraft file suggests that name and leaves the original file alone.
#[test]
fn shortcut_save_as_copies_a_deckcraft_file() {
    let dir = temp_dir("deck");
    let original = dir.join("Notes.deckcraft").to_string_lossy().into_owned();
    let copy = dir.join("Notes copy.deckcraft").to_string_lossy().into_owned();
    let mut session = Session::with_new();
    session.execute("file.saveAs", &json!({"path": original})).unwrap();
    let before = std::fs::read(&original).unwrap();
    session.execute("slide.new", &json!({"title": "Kept aside"})).unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut h = harness(SlideApp::new(session, saving_as(Some(copy.clone()), log.clone())));
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    h.run_steps(3);
    assert_eq!(log.borrow().as_slice(), ["Notes.deckcraft"]);
    assert_eq!(std::fs::read(&original).unwrap(), before);
    assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some(copy.as_str()));
    assert!(open_text(&copy).contains("Kept aside"));
    assert!(!open_text(&original).contains("Kept aside"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Cancelling the dialog leaves the presentation and its file as they were.
#[test]
fn cancelling_save_as_changes_nothing() {
    let dir = temp_dir("cancel");
    let original = dir.join("Talk.pptx").to_string_lossy().into_owned();
    let mut session = Session::with_new();
    session.execute("file.saveAs", &json!({"path": original})).unwrap();
    let before = std::fs::read(&original).unwrap();
    session.execute("slide.new", &json!({"title": "Kept aside"})).unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut h = harness(SlideApp::new(session, saving_as(None, log.clone())));
    h.state_mut().run("file.saveAs", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(log.borrow().as_slice(), ["Talk.pptx"], "a presentation with no path in the command still asks, in its own format");
    assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some(original.as_str()));
    assert!(h.state().session.active().unwrap().is_dirty());
    assert_eq!(std::fs::read(&original).unwrap(), before);
    let _ = std::fs::remove_dir_all(&dir);
}

/// On the web there is no path to adopt: Save As downloads a copy and the presentation stays.
#[test]
fn web_save_as_downloads_a_copy_and_leaves_the_presentation() {
    let dir = temp_dir("web");
    let original = dir.join("Talk.pptx").to_string_lossy().into_owned();
    let mut session = Session::with_new();
    session.execute("file.saveAs", &json!({"path": original})).unwrap();
    let before = std::fs::read(&original).unwrap();
    session.execute("slide.new", &json!({"title": "Kept aside"})).unwrap();
    let got: Rc<RefCell<Vec<(String, Vec<u8>)>>> = Default::default();
    let record = got.clone();
    let services = Services {
        download: Some(Box::new(move |name: &str, bytes: &[u8]| record.borrow_mut().push((name.to_string(), bytes.to_vec())))),
        ..Default::default()
    };
    let mut h = harness(SlideApp::new(session, services));
    h.get_by_label("Save As…").click();
    h.run_steps(3);
    assert_eq!(std::fs::read(&original).unwrap(), before);
    assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some(original.as_str()));
    assert!(h.state().session.active().unwrap().is_dirty(), "the open presentation is not retargeted onto a download");
    let saved = got.borrow();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].0, "Talk.pptx");
    let downloaded = dir.join("downloaded.pptx");
    std::fs::write(&downloaded, &saved[0].1).unwrap();
    assert!(open_text(&downloaded.to_string_lossy()).contains("Kept aside"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A presentation that has never been saved is offered as PowerPoint, as on File › Save As.
#[test]
fn an_unsaved_presentation_is_saved_as_powerpoint() {
    let dir = temp_dir("new");
    let dest = dir.join("copy.pptx").to_string_lossy().into_owned();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut h = harness(SlideApp::new(Session::with_new(), saving_as(Some(dest.clone()), log.clone())));
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    h.run_steps(3);
    assert_eq!(log.borrow().len(), 1);
    assert!(log.borrow()[0].ends_with(".pptx"), "suggested {}", log.borrow()[0]);
    assert!(std::path::Path::new(&dest).is_file());
    assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some(dest.as_str()));
    assert!(!h.state().session.active().unwrap().is_dirty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// With nothing open, Save As is shown and does not ask for a file.
#[test]
fn save_as_does_nothing_without_a_presentation() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut h = harness(SlideApp::new(Session::new(), saving_as(Some("unused.pptx".into()), log.clone())));
    assert!(h.query_by_label("Save As…").is_some());
    h.get_by_label("Save As…").click();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    h.run_steps(2);
    assert!(log.borrow().is_empty());
    assert!(h.state().session.active().is_none());
}
