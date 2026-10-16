//! `.deck` folders through the file commands, on disk.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::*;
use crate::Session;
use crate::cmd::base64_encode;

/// A fresh, empty scratch folder for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("deckcraft-folder-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn png(rgb: [u8; 3]) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(4, 3, image::Rgb(rgb));
    let mut out = std::io::Cursor::new(vec![]);
    image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// The sample deck plus a slide of every kind the commands make: table, charts (one without
/// title and legend), picture, SmartArt, action button, WordArt, notes and sections.
fn rich_session() -> Session {
    let mut s = Session::new();
    crate::sample::open_sample(&mut s).unwrap();
    run(&mut s, "slide.new", json!({}));
    run(&mut s, "insert.table", json!({"rows": 3, "cols": 4}));
    run(&mut s, "insert.chart", json!({"type": "pie"}));
    run(&mut s, "chart.options", json!({"title": null, "legend": null}));
    run(&mut s, "insert.chart", json!({"type": "column", "rect": [500, 300, 300, 200]}));
    run(&mut s, "insert.picture", json!({"name": "red.png", "data": base64_encode(&png([200, 30, 30]))}));
    run(&mut s, "insert.picture", json!({"name": "same.png", "data": base64_encode(&png([200, 30, 30]))}));
    run(&mut s, "slide.new", json!({}));
    run(&mut s, "insert.smartArt", json!({"kind": "process", "items": ["Plan", "Build", "Ship"]}));
    run(&mut s, "insert.actionButton", json!({"kind": "home"}));
    run(&mut s, "insert.wordArt", json!({"text": "Ünïcödé ✓"}));
    run(&mut s, "slide.notes", json!({"text": "Notes on the last slide"}));
    run(&mut s, "section.add", json!({"name": "Start", "at": 0}));
    let n = s.doc().unwrap().doc.slides.len();
    run(&mut s, "section.add", json!({"name": "More", "at": n - 2}));
    s
}

fn normalized(mut p: Presentation) -> Presentation {
    p.next_id = 0;
    p
}

fn names(dir: &Path) -> Vec<String> {
    fn walk(d: &Path, base: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else {
                out.push(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = vec![];
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn save_as_and_open_a_folder() {
    let root = scratch("roundtrip");
    let dir = root.join("Talk.deck");
    let path = dir.to_string_lossy().to_string();
    let mut s = rich_session();
    let original = (*s.doc().unwrap().doc).clone();
    let r = run(&mut s, "file.saveAs", json!({"path": path}));
    assert!(r["written"].as_u64().unwrap() > 10, "{r}");
    assert_eq!(s.doc().unwrap().path.as_deref(), Some(path.as_str()));
    assert!(!s.doc().unwrap().is_dirty());
    let files = names(&dir);
    assert!(files.contains(&"deck.json".to_string()) && files.contains(&"slides.order".to_string()), "{files:?}");
    assert!(files.iter().all(|f| deckcraft_format::folder::is_owned(f)), "no temporary files left: {files:?}");
    // Two pictures with the same bytes: one media file.
    assert_eq!(files.iter().filter(|f| f.starts_with("media/")).count(), 1, "{files:?}");

    // Same model as through a .deckcraft file.
    let zip = deckcraft_format::load(&deckcraft_format::save(&original).unwrap()).unwrap();
    for open_path in [path.clone(), format!("{path}/"), dir.join("deck.json").to_string_lossy().to_string()] {
        let mut t = Session::new();
        let r = run(&mut t, "file.open", json!({"path": open_path}));
        assert_eq!(r["slides"], json!(original.slides.len()));
        let st = t.doc().unwrap();
        assert!(normalized((*st.doc).clone()) == normalized(zip.clone()), "{open_path}");
        assert_eq!(st.path.as_deref(), Some(path.as_str()));
        assert_eq!(st.title(), "Talk");
    }

    // An unchanged deck saves without rewriting anything.
    let r = run(&mut s, "file.save", json!({}));
    assert_eq!((r["written"].as_u64(), r["removed"].as_u64()), (Some(0), Some(0)), "{r}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn save_removes_stale_files_and_keeps_the_users() {
    let root = scratch("stale");
    let dir = root.join("t.deck");
    let path = dir.to_string_lossy().to_string();
    let mut s = rich_session();
    run(&mut s, "file.saveAs", json!({"path": path}));
    std::fs::write(dir.join("README.md"), "mine").unwrap();
    std::fs::write(dir.join("slides").join("draft.txt"), "mine").unwrap();
    std::fs::create_dir_all(dir.join("notes")).unwrap();
    std::fs::write(dir.join("notes").join("a.json"), "{}").unwrap();
    let before = names(&dir);

    // Delete a slide: its file goes, nothing else (the model keeps its media list).
    let n = s.doc().unwrap().doc.slides.len();
    run(&mut s, "slide.delete", json!({"index": n - 2}));
    let r = run(&mut s, "file.save", json!({}));
    let after = names(&dir);
    let gone: Vec<&String> = before.iter().filter(|f| !after.contains(f)).collect();
    assert_eq!(gone.len(), 1, "{gone:?}");
    assert!(gone.iter().all(|f| f.starts_with("slides/")), "{gone:?}");
    assert_eq!(r["removed"].as_u64(), Some(gone.len() as u64));
    for mine in ["README.md", "slides/draft.txt", "notes/a.json"] {
        assert!(after.contains(&mine.to_string()), "{mine}");
    }
    // Moving a slide rewrites slides.order only.
    run(&mut s, "slide.move", json!({"from": 0, "to": 1}));
    let r = run(&mut s, "file.save", json!({}));
    assert_eq!(r["written"].as_u64(), Some(1), "{r}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn refuses_to_spill_into_another_folder() {
    let root = scratch("refuse");
    std::fs::write(root.join("thesis.txt"), "not a deck").unwrap();
    let mut s = rich_session();
    let e = s.execute("file.saveAs", &json!({"path": root.to_string_lossy(), "format": "deck"})).unwrap_err().to_string();
    assert!(e.contains("isn't a DeckCraft folder deck"), "{e}");
    assert_eq!(names(&root), vec!["thesis.txt".to_string()]);
    // A file where the folder should be.
    let f = root.join("x.deck");
    std::fs::write(&f, "file").unwrap();
    assert!(s.execute("file.saveAs", &json!({"path": f.to_string_lossy()})).is_err());
    // Opening an empty or foreign folder is an error, not a crash.
    let empty = root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(s.execute("file.open", &json!({"path": empty.to_string_lossy()})).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn convert_between_zip_and_folder() {
    let root = scratch("convert");
    let mut s = rich_session();
    let zip = root.join("a.deckcraft").to_string_lossy().to_string();
    let deck = root.join("a.deck").to_string_lossy().to_string();
    let back = root.join("b.deckcraft").to_string_lossy().to_string();
    run(&mut s, "file.saveAs", json!({"path": zip}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": zip}));
    run(&mut t, "file.saveAs", json!({"path": deck}));
    let mut u = Session::new();
    run(&mut u, "file.open", json!({"path": deck}));
    run(&mut u, "file.export", json!({"path": back}));
    assert_eq!(u.doc().unwrap().path.as_deref(), Some(deck.as_str()), "export doesn't change the document's file");
    let a = deckcraft_format::load(&std::fs::read(&zip).unwrap()).unwrap();
    let b = deckcraft_format::load(&std::fs::read(&back).unwrap()).unwrap();
    assert!(normalized(a) == normalized(b));
    // file.export to a folder, and save_bytes refuses the folder format.
    let exported = root.join("e.deck");
    run(&mut u, "file.export", json!({"path": exported.to_string_lossy(), "format": "deck"}));
    assert!(exported.join("deck.json").is_file());
    assert!(crate::cmd::file::save_bytes(&u.doc().unwrap().doc, "deck").is_err());
    assert_eq!(crate::cmd::file::format_for_path("x.DECK"), "deck");
    assert_eq!(crate::cmd::file::format_for_path(&root.to_string_lossy()), "deck");
    assert_eq!(crate::cmd::file::format_for_path("x.deckcraft"), "deckcraft");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn slide_keys_are_stable_on_disk() {
    let root = scratch("keys");
    let dir = root.join("k.deck");
    let mut s = rich_session();
    run(&mut s, "file.saveAs", json!({"path": dir.to_string_lossy()}));
    let slides = |d: &Path| names(d).into_iter().filter(|f| f.starts_with("slides/")).collect::<Vec<_>>();
    let before = slides(&dir);
    run(&mut s, "slide.new", json!({}));
    run(&mut s, "file.save", json!({}));
    let after = slides(&dir);
    assert_eq!(after.len(), before.len() + 1);
    assert!(before.iter().all(|f| after.contains(f)));
    // Reopened and saved by another session: same files.
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": dir.to_string_lossy()}));
    let r = run(&mut t, "file.save", json!({}));
    assert_eq!(r["written"].as_u64(), Some(0), "{r}");
    let _ = std::fs::remove_dir_all(&root);
}

/// The File tab's Info page: a presentation saved as a folder reports the folder format and the
/// size of its files.
#[test]
fn info_reports_a_folder_and_its_size() {
    let root = scratch("info");
    let dir = root.join("Info.deck");
    let mut s = rich_session();
    run(&mut s, "file.saveAs", json!({"path": dir.to_string_lossy()}));
    let v = run(&mut s, "file.info", json!({}));
    assert_eq!(v["format"], "deck");
    let files: u64 = names(&dir).iter().map(|f| std::fs::metadata(dir.join(f)).unwrap().len()).sum();
    assert_eq!(v["bytes"].as_u64(), Some(files), "{v}");
    let _ = std::fs::remove_dir_all(&root);
}
