//! Save As… PowerPoint (#1929): a save picker that returns a name without an extension must
//! still produce `Name.pptx`, and must not replace a `Name.pptx` the picker never asked about.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use deckcraft_ui_egui::{Services, SlideApp};
use serde_json::json;

fn app_picking(path: String) -> SlideApp {
    let services = Services { pick_save: Some(Box::new(move |_: &str| Some(path.clone()))), ..Default::default() };
    SlideApp::new(Session::with_new(), services)
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("deckcraft-regression-1929-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn save_as_pptx_adds_the_extension_when_the_picker_does_not() {
    let dir = temp_dir("save-as");
    let mut app = app_picking(dir.join("Talk").to_string_lossy().to_string());
    app.save_as_dialog("pptx");
    let saved = dir.join("Talk.pptx");
    assert!(saved.is_file(), "expected {saved:?}");
    let bytes = std::fs::read(&saved).unwrap();
    assert!(bytes.starts_with(b"PK"), "a .pptx file is a zip package");
    assert!(!dir.join("Talk").exists(), "no file without an extension");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_as_pptx_adds_the_extension_when_the_picker_does_not() {
    let dir = temp_dir("export");
    let mut app = app_picking(dir.join("Talk").to_string_lossy().to_string());
    assert!(app.export_as("pptx", json!({})));
    let saved = dir.join("Talk.pptx");
    assert!(saved.is_file(), "expected {saved:?}");
    assert!(!dir.join("Talk").exists(), "no file without an extension");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_as_pptx_does_not_replace_a_file_the_picker_did_not_confirm() {
    let dir = temp_dir("save-as-existing");
    let existing = dir.join("Talk.pptx");
    std::fs::write(&existing, b"existing").unwrap();
    let mut app = app_picking(dir.join("Talk").to_string_lossy().to_string());
    app.save_as_dialog("pptx");
    assert_eq!(std::fs::read(&existing).unwrap(), b"existing");
    assert!(!dir.join("Talk").exists(), "no file without an extension");
    let status = app.status.as_ref().map(|(m, _)| m.clone()).unwrap_or_default();
    assert!(status.contains("already exists"), "{status}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_as_pptx_does_not_replace_a_file_the_picker_did_not_confirm() {
    let dir = temp_dir("export-existing");
    let existing = dir.join("Talk.pptx");
    std::fs::write(&existing, b"existing").unwrap();
    let mut app = app_picking(dir.join("Talk").to_string_lossy().to_string());
    assert!(!app.export_as("pptx", json!({})));
    assert_eq!(std::fs::read(&existing).unwrap(), b"existing");
    assert!(!dir.join("Talk").exists(), "no file without an extension");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn typing_the_full_name_replaces_an_existing_file() {
    let dir = temp_dir("full-name");
    let existing = dir.join("Talk.pptx");
    std::fs::write(&existing, b"existing").unwrap();
    let mut app = app_picking(existing.to_string_lossy().to_string());
    app.save_as_dialog("pptx");
    assert!(std::fs::read(&existing).unwrap().starts_with(b"PK"));
    let _ = std::fs::remove_dir_all(&dir);
}
