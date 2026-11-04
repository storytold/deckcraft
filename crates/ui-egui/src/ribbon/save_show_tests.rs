//! Record ▸ Export ▸ Save as Show (#1819): save a `.ppsx` copy of the active presentation
//! through the normal Save dialog, with the slide show's main content type.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::io::Read as _;
use std::rc::Rc;

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// A folder of its own for one test's files.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("deckcraft-save-show-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The Record tab, with a Save dialog that answers with the path `answer` chooses for the
/// suggested name and records the names it was offered.
fn harness(answer: impl Fn(&str) -> std::path::PathBuf + 'static) -> (Harness<'static, SlideApp>, Rc<RefCell<Vec<String>>>) {
    let asked: Rc<RefCell<Vec<String>>> = Default::default();
    let log = asked.clone();
    let services = Services {
        pick_save: Some(Box::new(move |suggested: &str| {
            log.borrow_mut().push(suggested.to_string());
            Some(answer(suggested).to_string_lossy().to_string())
        })),
        ..Default::default()
    };
    let mut session = Session::with_new();
    session.execute("slide.new", &json!({})).unwrap();
    let mut app = SlideApp::new(session, services);
    app.ui.tab = "Record".into();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    (h, asked)
}

/// The main content type declared by the OOXML package at `path`.
fn main_content_type(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut types = String::new();
    zip.by_name("[Content_Types].xml").unwrap().read_to_string(&mut types).unwrap();
    let at = types.find(r#"PartName="/ppt/presentation.xml""#).unwrap();
    types[at..].split('>').next().unwrap_or_default().to_string()
}

#[test]
fn save_as_show_writes_a_ppsx_with_the_slideshow_content_type() {
    let dir = temp_dir("ok");
    let target = dir.clone();
    let (mut h, asked) = harness(move |suggested: &str| target.join(suggested));
    let before = h.state().session.active().unwrap().path.clone();

    h.get_by_label("Save as Show").click();
    h.run_steps(3);

    assert!(asked.borrow().last().is_some_and(|name| name.ends_with(".ppsx")), "the Save dialog suggests a show: {:?}", asked.borrow());
    let path = dir.join(asked.borrow().last().unwrap());
    assert!(path.is_file(), "the show was written to {}", path.display());
    assert!(
        main_content_type(&path).contains("application/vnd.openxmlformats-officedocument.presentationml.slideshow.main+xml"),
        "a .ppsx declares the slide show's main part"
    );
    let status = h.state().status.as_ref().map(|(m, _)| m.clone()).unwrap_or_default();
    assert!(status.contains("Exported") && status.contains(".ppsx"), "the status bar names the copy: {status}");
    assert_eq!(h.state().session.active().unwrap().path, before, "a show is a copy: the presentation keeps its own file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_as_show_reports_a_failed_save_in_the_status_bar() {
    let dir = temp_dir("failed");
    let target = dir.join("missing");
    let (mut h, _) = harness(move |suggested: &str| target.join(suggested));

    h.get_by_label("Save as Show").click();
    h.run_steps(3);

    let status = h.state().status.as_ref().map(|(m, _)| m.clone()).unwrap_or_default();
    assert!(!status.is_empty() && !status.contains("Exported"), "the failure is shown: {status}");
    let _ = std::fs::remove_dir_all(&dir);
}
