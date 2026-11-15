//! The Library pane driven like a person would, over a folder of decks built here in code.

use std::path::{Path, PathBuf};

use deckcraft_engine::Session;
use egui::{Event, Modifiers, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dc-library-pane-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A .pptx whose slides have these titles; the second slide also has a star.
fn write_deck(path: &Path, titles: &[&str]) {
    let mut d = Session::with_new();
    for (i, t) in titles.iter().enumerate() {
        d.execute("slide.new", &json!({"layout": "titleOnly", "title": t})).unwrap();
        if i == 1 {
            d.execute("shape.insert", &json!({"preset": "star5", "rect": [100, 120, 200, 150]})).unwrap();
        }
    }
    d.execute("slide.delete", &json!({"index": 0})).unwrap();
    std::fs::write(path, deckcraft_engine::cmd::file::save_bytes(&d.doc().unwrap().doc, "pptx").unwrap()).unwrap();
}

fn harness(app: SlideApp) -> Harness<'static, SlideApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

/// The pane open on `dir`, every file read and the thumbnails drawn.
fn open_on(dir: &Path) -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("library.addFolder", &json!({"path": dir.to_string_lossy()})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.pane = Some("library".into());
    let mut h = harness(app);
    h.run_steps(20);
    assert!(h.state().library.ready());
    h
}

fn slides(h: &Harness<'_, SlideApp>) -> usize {
    h.state().session.active().unwrap().doc.slides.len()
}

fn right_click(h: &mut Harness<'_, SlideApp>, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        h.step();
    }
    h.run_steps(2);
}

#[test]
fn insert_library_opens_the_pane_on_its_slides_tab() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.ui.tab = "Insert".into();
    let mut h = harness(app);
    h.get_by_label("Library").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.pane.as_deref(), Some("library"));
    assert_eq!(h.state().library.tab, super::ITEMS);
    assert!(h.query_by_label("Slides").is_some(), "the tab strip");
    // No folders yet: the pane says what to do, and Add Folder… (no picker here) takes a path.
    assert!(h.query_by_label_contains("Add a folder of .pptx").is_some());
    let dir = folder("add");
    write_deck(&dir.join("deck.pptx"), &["Agenda"]);
    h.get_by_label("Add Folder…").click();
    h.run_steps(2);
    assert!(h.state().library.adding);
    h.state_mut().library.new_folder = dir.to_string_lossy().into_owned();
    h.run_steps(1);
    h.get_by_label("Add").click();
    h.run_steps(20);
    assert_eq!(h.state().session.prefs.library_folders.len(), 1);
    assert!(h.query_by_label("Slide 1: Agenda").is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn clicking_a_slide_inserts_it_and_the_menu_inserts_its_shapes() {
    let dir = folder("click");
    write_deck(&dir.join("deck.pptx"), &["Agenda", "Thanks"]);
    let mut h = open_on(&dir);
    let n = slides(&h);
    let undo = h.state().session.active().unwrap().history.undo.len();
    h.get_by_label("Slide 2: Thanks").click();
    h.run_steps(3);
    assert_eq!(slides(&h), n + 1);
    let st = h.state().session.active().unwrap();
    assert_eq!(st.current_slide().map(|s| s.title()), Some("Thanks".to_string()));
    assert_eq!(st.history.undo.len(), undo + 1, "one undo step");
    // The context menu puts the slide's shapes on the current slide instead.
    h.state_mut().run("slide.new", json!({"layout": "blank"})).unwrap();
    h.run_steps(2);
    let before = slides(&h);
    let at = h.get_by_label("Slide 2: Thanks").rect().center();
    right_click(&mut h, at);
    h.get_by_label("Insert shapes on current slide").click();
    h.run_steps(3);
    assert_eq!(slides(&h), before, "no slide added");
    let shapes = h.state().session.active().unwrap().current_slide().unwrap().shapes.len();
    assert_eq!(shapes, 2, "the title and the star");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn search_shows_only_the_slides_that_match() {
    let dir = folder("search");
    write_deck(&dir.join("one.pptx"), &["Agenda", "Thanks"]);
    write_deck(&dir.join("two.pptx"), &["Pricing"]);
    let mut h = open_on(&dir);
    assert!(h.query_by_label("Slide 1: Pricing").is_some() && h.query_by_label("Slide 1: Agenda").is_some());
    h.state_mut().library.query = "THANK".into();
    h.run_steps(30);
    assert_eq!(h.state().library.hits(), Some(1));
    assert!(h.query_by_label("Slide 2: Thanks").is_some());
    assert!(h.query_by_label("Slide 1: Pricing").is_none() && h.query_by_label("Slide 1: Agenda").is_none());
    h.state_mut().library.query = "nothing like it".into();
    h.run_steps(30);
    assert!(h.query_by_label_contains("No slides match").is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_damaged_file_shows_its_error_and_the_others_still_show() {
    let dir = folder("damaged");
    write_deck(&dir.join("good.pptx"), &["Fine"]);
    std::fs::write(dir.join("bad.pptx"), b"PK\x03\x04 not a zip").unwrap();
    let h = open_on(&dir);
    let files = &h.state().library.files;
    assert_eq!(files.len(), 2);
    assert!(files.iter().any(|f| f.error.is_some()));
    assert!(h.query_by_label("Slide 1: Fine").is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_very_large_library_shows_its_first_300_files_and_says_so() {
    let dir = folder("large");
    let mut d = Session::with_new();
    d.execute("slide.new", &json!({"layout": "titleOnly", "title": "Copy"})).unwrap();
    let bytes = deckcraft_engine::cmd::file::save_bytes(&d.doc().unwrap().doc, "deckcraft").unwrap();
    for i in 0..=super::MAX_SHOWN_FILES {
        std::fs::write(dir.join(format!("deck{i:03}.deckcraft")), &bytes).unwrap();
    }
    let mut s = Session::with_new();
    s.execute("library.addFolder", &json!({"path": dir.to_string_lossy()})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.pane = Some("library".into());
    let mut h = harness(app);
    for _ in 0..400 {
        if h.state().library.ready() {
            break;
        }
        h.step();
    }
    assert!(h.state().library.ready());
    h.run_steps(2);
    assert!(h.query_by_label_contains("Showing the first 300 of 301 files").is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn insert_element_puts_one_named_shape_on_the_slide() {
    let dir = folder("element");
    let mut d = Session::with_new();
    d.execute("slide.new", &json!({"layout": "titleOnly", "title": "Kit"})).unwrap();
    let star = d.execute("shape.insert", &json!({"preset": "star5", "rect": [100, 120, 200, 150]})).unwrap()["id"].as_u64().unwrap();
    d.execute("shape.rename", &json!({"id": star, "name": "Gold star"})).unwrap();
    let boxed = d.execute("shape.insert", &json!({"preset": "rect", "rect": [400, 120, 200, 150]})).unwrap()["id"].as_u64().unwrap();
    d.execute("shape.rename", &json!({"id": boxed, "name": "Plain box"})).unwrap();
    d.execute("slide.delete", &json!({"index": 0})).unwrap();
    std::fs::write(dir.join("kit.pptx"), deckcraft_engine::cmd::file::save_bytes(&d.doc().unwrap().doc, "pptx").unwrap()).unwrap();
    let mut h = open_on(&dir);
    h.state_mut().run("slide.new", json!({"layout": "blank"})).unwrap();
    h.run_steps(2);
    let undo = h.state().session.active().unwrap().history.undo.len();
    let at = h.get_by_label("Slide 1: Kit").rect().center();
    right_click(&mut h, at);
    h.get_by_label("Insert element").click();
    h.run_steps(2);
    h.get_by_label("Gold star").click();
    h.run_steps(3);
    let st = h.state().session.active().unwrap();
    let names: Vec<&str> = st.current_slide().unwrap().shapes.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["Gold star"]);
    assert_eq!(st.history.undo.len(), undo + 1, "one undo step");
    let _ = std::fs::remove_dir_all(dir);
}
