//! Home › New Slide › Reuse Slides…: the dialog lists the slides of the picked file and inserts the
//! ticked ones; what is inserted is what was listed, and a failed insert keeps the dialog open.

use std::cell::RefCell;
use std::rc::Rc;

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn deck_bytes(titles: &[&str]) -> Vec<u8> {
    let mut s = Session::with_new();
    for t in titles {
        s.execute("slide.new", &json!({"layout": "titleAndContent", "title": t})).unwrap();
    }
    // Drop the new deck's empty first slide, so slide `i` is `titles[i]`.
    s.execute("slide.delete", &json!({"index": 0})).unwrap();
    deckcraft_engine::cmd::file::save_bytes(&s.doc().unwrap().doc, "deckcraft").unwrap()
}

/// An app whose open dialog picks `src.deckcraft`, read from the shared `file` bytes.
fn harness(file: &Rc<RefCell<Vec<u8>>>) -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let f = Rc::clone(file);
    let services = Services {
        pick_open: Some(Box::new(|_| Some("/tmp/src.deckcraft".to_string()))),
        read: Some(Box::new(move |_| Ok(f.borrow().clone()))),
        ..Services::default()
    };
    let app = SlideApp::new(s, services);
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

fn titles(h: &Harness<'_, SlideApp>) -> Vec<String> {
    h.state().session.active().unwrap().doc.slides.iter().map(|s| s.title()).collect()
}

fn open_dialog(h: &mut Harness<'_, SlideApp>) {
    h.state_mut().run("app.reuseSlidesDialog", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("reuseSlides"));
}

fn press_insert(h: &mut Harness<'_, SlideApp>) {
    // The ribbon has an Insert tab too; the dialog's button is drawn last.
    h.query_all_by_label("Insert").last().expect("the dialog's Insert button").click();
    h.run_steps(3);
}

#[test]
fn inserts_the_ticked_slides_and_closes() {
    let file = Rc::new(RefCell::new(deck_bytes(&["Alpha", "Beta", "Gamma"])));
    let mut h = harness(&file);
    open_dialog(&mut h);
    // Untick the middle slide.
    h.state_mut().dialog.as_mut().unwrap().fields.insert("off1".into(), "1".into());
    h.run_steps(2);
    press_insert(&mut h);
    assert!(h.state().dialog.is_none());
    let t = titles(&h);
    assert!(t.iter().any(|x| x == "Alpha") && t.iter().any(|x| x == "Gamma") && !t.iter().any(|x| x == "Beta"), "{t:?}");
}

#[test]
fn inserts_what_was_listed_even_if_the_file_changed_meanwhile() {
    let file = Rc::new(RefCell::new(deck_bytes(&["Alpha", "Beta"])));
    let mut h = harness(&file);
    open_dialog(&mut h);
    // Someone replaces the file while the dialog is open: slide 0 is now a different one.
    *file.borrow_mut() = deck_bytes(&["Other", "Beta"]);
    h.state_mut().dialog.as_mut().unwrap().fields.insert("off1".into(), "1".into());
    h.run_steps(2);
    press_insert(&mut h);
    let t = titles(&h);
    assert!(t.iter().any(|x| x == "Alpha"), "{t:?}");
    assert!(!t.iter().any(|x| x == "Other"), "{t:?}");
}

#[test]
fn inserts_into_the_presentation_the_picker_was_opened_for() {
    let file = Rc::new(RefCell::new(deck_bytes(&["Alpha"])));
    let mut h = harness(&file);
    open_dialog(&mut h);
    let first = h.state().session.active().unwrap().uid;
    // Another presentation becomes active while the dialog is open.
    h.state_mut().session.execute("file.new", &json!({})).unwrap();
    let second = h.state().session.active().unwrap().uid;
    assert_ne!(first, second);
    press_insert(&mut h);
    assert!(h.state().dialog.is_none());
    let s = &h.state().session;
    let doc = |uid: u64| s.documents().iter().find(|d| d.uid == uid).unwrap();
    assert!(doc(first).doc.slides.iter().any(|x| x.title() == "Alpha"), "inserted where the picker was opened");
    assert_eq!(doc(second).doc.slides.len(), 1, "the other presentation is untouched");
    assert_eq!(s.active().unwrap().uid, second, "the view stays on the presentation the user moved to");
}

#[test]
fn nothing_is_inserted_once_that_presentation_is_closed() {
    let file = Rc::new(RefCell::new(deck_bytes(&["Alpha"])));
    let mut h = harness(&file);
    open_dialog(&mut h);
    h.state_mut().session.execute("file.new", &json!({})).unwrap();
    h.state_mut().session.close_document(0);
    press_insert(&mut h);
    assert_eq!(h.state().session.documents().len(), 1);
    assert!(!titles(&h).iter().any(|x| x == "Alpha"), "{:?}", titles(&h));
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("reuseSlides"), "the dialog stays open with the reason");
}

#[test]
fn a_failed_insert_keeps_the_dialog_open() {
    let file = Rc::new(RefCell::new(deck_bytes(&["Alpha"])));
    let mut h = harness(&file);
    open_dialog(&mut h);
    let before = titles(&h);
    // Offer a slide the file doesn't have, so the insert is refused.
    if let Some(d) = h.state_mut().dialog.as_mut()
        && let Some(a) = d.params["slides"].as_array_mut()
    {
        a.push(json!({"index": 9, "title": "Ghost", "hidden": false}));
    }
    h.run_steps(2);
    press_insert(&mut h);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("reuseSlides"), "the dialog closed on a failed insert");
    assert_eq!(titles(&h), before);
}

/// Thumbnails drawn so far for the open picker: `(held, rendered ever)`.
fn thumbs(h: &Harness<'_, SlideApp>) -> (usize, u64) {
    h.state().reuse_preview.as_ref().map(|p| (p.textures.thumb_count(), p.textures.thumb_renders())).unwrap_or_default()
}

#[test]
fn each_listed_slide_shows_a_thumbnail_of_the_picked_file() {
    let file = Rc::new(RefCell::new(deck_bytes(&["Alpha", "Beta", "Gamma"])));
    let mut h = harness(&file);
    open_dialog(&mut h);
    h.run_steps(6);
    assert_eq!(thumbs(&h), (3, 3));
    // Settled: later frames draw the same textures without rendering again.
    h.run_steps(6);
    assert_eq!(thumbs(&h), (3, 3));
    assert!(h.query_all_by_label("No preview").next().is_none());
}

#[test]
fn ticking_only_the_second_slide_inserts_the_second_slide() {
    let file = Rc::new(RefCell::new(deck_bytes(&["Alpha", "Beta", "Gamma"])));
    let mut h = harness(&file);
    open_dialog(&mut h);
    h.run_steps(4);
    for i in [0, 2] {
        h.state_mut().dialog.as_mut().unwrap().fields.insert(format!("off{i}"), "1".into());
    }
    h.run_steps(2);
    press_insert(&mut h);
    assert!(h.state().dialog.is_none());
    assert!(h.state().reuse_preview.is_none(), "the thumbnails go with the dialog");
    let t = titles(&h);
    assert!(t.iter().any(|x| x == "Beta") && !t.iter().any(|x| x == "Alpha" || x == "Gamma"), "{t:?}");
}

#[test]
fn the_thumbnails_go_when_a_script_closes_the_dialog() {
    let file = Rc::new(RefCell::new(deck_bytes(&["Alpha", "Beta"])));
    let mut h = harness(&file);
    open_dialog(&mut h);
    h.run_steps(6);
    assert_eq!(thumbs(&h).0, 2);
    let (req, _rx) = crate::control::ControlRequest::new("ui.dialog.close", json!({}));
    let ctx = h.ctx.clone();
    assert!(matches!(crate::control::handle(h.state_mut(), &ctx, &req), crate::control::Outcome::Done(_)));
    h.run_steps(1);
    assert!(h.state().dialog.is_none());
    assert!(h.state().reuse_preview.is_none(), "the parsed file and its textures are released");
}

#[test]
fn a_slide_that_cannot_be_drawn_keeps_its_title_beside_a_placeholder() {
    let file = Rc::new(RefCell::new(deck_bytes(&["Alpha"])));
    let mut h = harness(&file);
    open_dialog(&mut h);
    if let Some(d) = h.state_mut().dialog.as_mut()
        && let Some(a) = d.params["slides"].as_array_mut()
    {
        a.push(json!({"index": 9, "title": "Ghost", "hidden": false}));
    }
    h.run_steps(6);
    h.get_by_label("10  Ghost");
    assert_eq!(h.query_all_by_label("No preview").count(), 1);
    assert_eq!(thumbs(&h), (1, 1));
}

#[test]
fn only_the_first_200_slides_get_thumbnails_and_only_rows_in_view_render() {
    let names: Vec<String> = (1..=205).map(|i| format!("S{i}")).collect();
    let file = Rc::new(RefCell::new(deck_bytes(&names.iter().map(String::as_str).collect::<Vec<_>>())));
    let mut h = harness(&file);
    open_dialog(&mut h);
    h.run_steps(10);
    assert_eq!(h.query_all_by_label("No preview").count(), 5, "slides 201 to 205 are listed by title only");
    let (_, rendered) = thumbs(&h);
    assert!((1..=20).contains(&rendered), "rendered {rendered} of 205");
}

/// An app whose open dialog picks `path` and that has no reader of its own (the engine reads it).
fn path_only_harness(path: &str) -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let picked = path.to_string();
    let services = Services { pick_open: Some(Box::new(move |_| Some(picked.clone()))), ..Services::default() };
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(s, services),
    );
    h.run_steps(3);
    h
}

#[test]
fn a_host_without_a_reader_lists_previews_and_inserts_the_same_bytes() {
    let dir = std::env::temp_dir().join(format!("deckcraft-reuse-path-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("src.deckcraft");
    std::fs::write(&path, deck_bytes(&["Alpha", "Beta"])).unwrap();
    let mut h = path_only_harness(path.to_str().unwrap());
    open_dialog(&mut h);
    h.run_steps(6);
    assert_eq!(thumbs(&h).0, 2, "each listed slide has a thumbnail");
    // The file changes after it was listed: what is inserted is still what was listed.
    std::fs::write(&path, deck_bytes(&["Changed"])).unwrap();
    press_insert(&mut h);
    let t = titles(&h);
    assert!(t.ends_with(&["Alpha".to_string(), "Beta".to_string()]) && !t.contains(&"Changed".to_string()), "{t:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_long_title_wraps_beside_its_thumbnail() {
    let long = "A very long slide title that keeps going ".repeat(6);
    let file = Rc::new(RefCell::new(deck_bytes(&[long.trim()])));
    let mut h = harness(&file);
    open_dialog(&mut h);
    h.run_steps(6);
    let row = h.get_by_label_contains("A very long slide title").rect();
    assert!(row.width() <= crate::dialogs::REUSE_TITLE_W + 1.0, "the title is {} wide", row.width());
    assert!(row.height() > 2.0 * 14.0, "the title wraps onto more lines: {}", row.height());
}
