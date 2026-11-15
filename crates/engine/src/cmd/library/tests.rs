//! Library commands against folders of decks built here in code (no third-party files).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::file_access::{AutomationRoots, confine};
use crate::library::{MAX_DEPTH, MAX_ENTRIES, MAX_FOLDERS, MAX_SLIDES};
use crate::{Prefs, Session};

/// A fresh, empty folder for one test.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dc-library-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// A presentation (`format`: pptx or deckcraft) whose slides have these titles and body texts.
fn deck(slides: &[(&str, &str)], format: &str) -> Vec<u8> {
    let mut d = Session::with_new();
    for (title, body) in slides {
        d.execute("slide.new", &json!({"layout": "titleAndContent", "title": title, "body": body})).expect("slide");
    }
    d.execute("slide.delete", &json!({"index": 0})).expect("drop the first, empty slide");
    crate::cmd::file::save_bytes(&d.doc().expect("doc").doc, format).expect("save")
}

/// A session with one presentation open and `dir` in the Library.
fn session_with(dir: &Path) -> Session {
    let mut ss = Session::with_new();
    ss.execute("library.addFolder", &json!({"path": s(dir)})).expect("add folder");
    ss
}

fn list(ss: &mut Session, p: Value) -> Vec<Value> {
    ss.execute("library.list", &p).expect("list").as_array().cloned().unwrap_or_default()
}

fn titles(entry: &Value) -> Vec<String> {
    entry["slides"].as_array().map(|a| a.iter().map(|x| x["title"].as_str().unwrap_or_default().to_string()).collect()).unwrap_or_default()
}

#[test]
fn folders_are_added_listed_and_removed() {
    let dir = folder("add");
    std::fs::write(dir.join("Pricing.pptx"), deck(&[("Price list", "Basic\nPro")], "pptx")).expect("write");
    std::fs::write(dir.join("notes.txt"), "not a presentation").expect("write");
    let mut ss = session_with(&dir);
    let again = ss.execute("library.addFolder", &json!({"path": s(&dir)})).expect("again");
    assert_eq!(again["added"], json!(false), "adding a folder twice keeps one");
    let folders = ss.execute("library.folders", &json!({})).expect("folders");
    assert_eq!(folders.as_array().map(Vec::len), Some(1));
    assert_eq!(folders[0]["files"], json!(1), "{folders}");
    assert_eq!(folders[0]["entries"], json!(2));
    let files = list(&mut ss, json!({}));
    assert_eq!(files.len(), 1);
    assert_eq!(titles(&files[0]), vec!["Price list"]);
    assert_eq!(files[0]["slideCount"], json!(1));
    // Without opening the files.
    let names = list(&mut ss, json!({"slides": false}));
    assert_eq!(names[0]["title"], json!("Pricing"));
    assert!(names[0].get("slides").is_none());
    // A missing folder, a file, and a folder that isn't in the Library are refused.
    assert!(ss.execute("library.addFolder", &json!({"path": s(&dir.join("missing"))})).is_err());
    assert!(ss.execute("library.addFolder", &json!({"path": s(&dir.join("notes.txt"))})).is_err());
    assert!(ss.execute("library.addFolder", &json!({})).is_err());
    assert!(ss.execute("library.removeFolder", &json!({"path": "/no/such/library"})).is_err());
    assert!(ss.execute("library.list", &json!({"folder": "/no/such/library"})).is_err());
    let removed = ss.execute("library.removeFolder", &json!({"path": s(&dir)})).expect("remove");
    assert_eq!(removed["folders"], json!([]));
    assert_eq!(list(&mut ss, json!({})).len(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn at_most_32_folders() {
    let dir = folder("many");
    let mut ss = Session::with_new();
    for i in 0..MAX_FOLDERS {
        let d = dir.join(format!("f{i}"));
        std::fs::create_dir_all(&d).expect("mkdir");
        ss.execute("library.addFolder", &json!({"path": s(&d)})).expect("add");
    }
    let extra = dir.join("extra");
    std::fs::create_dir_all(&extra).expect("mkdir");
    let e = ss.execute("library.addFolder", &json!({"path": s(&extra)})).expect_err("33rd folder");
    assert!(e.to_string().contains("at most 32 folders"), "{e}");
    assert_eq!(ss.prefs.library_folders.len(), MAX_FOLDERS);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_folder_is_scanned_to_5000_entries() {
    let dir = folder("entries");
    for i in 0..=MAX_ENTRIES {
        std::fs::write(dir.join(format!("f{i:05}.txt")), "").expect("write");
    }
    let mut ss = session_with(&dir);
    let f = ss.execute("library.folders", &json!({})).expect("folders");
    assert_eq!(f[0]["entries"], json!(MAX_ENTRIES), "{f}");
    assert_eq!(f[0]["truncated"], json!(true));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn folders_deeper_than_8_are_not_entered() {
    let dir = folder("depth");
    let mut at = dir.clone();
    let bytes = deck(&[("Deep", "")], "deckcraft");
    for level in 0..=MAX_DEPTH + 1 {
        std::fs::write(at.join(format!("level{level}.deckcraft")), &bytes).expect("write");
        at = at.join(format!("d{level}"));
        std::fs::create_dir_all(&at).expect("mkdir");
    }
    let mut ss = session_with(&dir);
    let files = list(&mut ss, json!({"slides": false}));
    let names: Vec<String> = files.iter().map(|f| f["title"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(names.len(), MAX_DEPTH + 1, "levels 0 to 8: {names:?}");
    assert!(names.contains(&format!("level{MAX_DEPTH}")) && !names.contains(&format!("level{}", MAX_DEPTH + 1)));
    let f = ss.execute("library.folders", &json!({})).expect("folders");
    assert_eq!(f[0]["skippedDeep"], json!(1), "{f}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn files_over_256_mb_are_listed_with_an_error_and_never_opened() {
    let dir = folder("large");
    // Sparse: no disk space is used.
    let big = std::fs::File::create(dir.join("huge.pptx")).expect("create");
    big.set_len(crate::library::MAX_FILE_BYTES + 1).expect("set_len");
    drop(big);
    let mut ss = session_with(&dir);
    for open in [true, false] {
        let files = list(&mut ss, json!({"slides": open}));
        assert_eq!(files.len(), 1);
        assert!(files[0]["error"].as_str().is_some_and(|e| e.contains("256 MB")), "{}", files[0]);
    }
    let path = s(&dir.join("huge.pptx"));
    assert!(ss.execute("library.thumbnail", &json!({"file": path, "slide": 0})).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn at_most_2000_slides_are_listed_per_file() {
    let dir = folder("slides");
    let mut doc = deckcraft_model::defaults::new_presentation(None);
    let first = doc.slides.first().cloned().expect("a slide");
    while doc.slides.len() <= MAX_SLIDES {
        let mut sl = (*first).clone();
        sl.id = deckcraft_model::SlideId(doc.alloc_id().expect("id"));
        doc.slides.push(Arc::new(sl));
    }
    std::fs::write(dir.join("long.deckcraft"), crate::cmd::file::save_bytes(&doc, "deckcraft").expect("save")).expect("write");
    let mut ss = session_with(&dir);
    let files = list(&mut ss, json!({}));
    assert_eq!(files[0]["slides"].as_array().map(Vec::len), Some(MAX_SLIDES));
    assert_eq!(files[0]["slideCount"], json!(MAX_SLIDES + 1));
    assert_eq!(files[0]["truncated"], json!(true));
    let path = s(&dir.join("long.deckcraft"));
    assert!(ss.execute("library.thumbnail", &json!({"file": path, "slide": MAX_SLIDES})).is_err(), "past the listed slides");
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn links_are_not_followed_out_of_a_folder() {
    let base = folder("links");
    let (lib, outside) = (base.join("lib"), base.join("outside"));
    std::fs::create_dir_all(&lib).expect("mkdir");
    std::fs::create_dir_all(&outside).expect("mkdir");
    std::fs::write(outside.join("secret.pptx"), deck(&[("Secret", "")], "pptx")).expect("write");
    std::os::unix::fs::symlink(&outside, lib.join("away")).expect("link to a folder");
    std::os::unix::fs::symlink(outside.join("secret.pptx"), lib.join("linked.pptx")).expect("link to a file");
    let mut ss = session_with(&lib);
    assert!(list(&mut ss, json!({})).is_empty());
    let f = ss.execute("library.folders", &json!({})).expect("folders");
    assert_eq!(f[0]["skippedLinks"], json!(2), "{f}");
    // Naming the link doesn't reach the file outside either.
    let e = ss.execute("library.thumbnail", &json!({"file": s(&lib.join("linked.pptx")), "slide": 0})).expect_err("outside");
    assert!(e.to_string().contains("not in a Library folder"), "{e}");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn a_corrupt_file_is_listed_with_its_error() {
    let dir = folder("corrupt");
    std::fs::write(dir.join("broken.pptx"), b"PK\x03\x04 this is not a zip").expect("write");
    std::fs::write(dir.join("empty.potx"), b"").expect("write");
    std::fs::write(dir.join("good.pptx"), deck(&[("Fine", "")], "pptx")).expect("write");
    let mut ss = session_with(&dir);
    let files = list(&mut ss, json!({}));
    assert_eq!(files.len(), 3, "{files:?}");
    for f in &files {
        let name = f["file"].as_str().unwrap_or_default();
        if name.ends_with("good.pptx") {
            assert!(f.get("error").is_none(), "{f}");
        } else {
            assert!(f["error"].as_str().is_some_and(|e| !e.is_empty()), "{f}");
            assert_eq!(f["slides"], json!([]));
        }
    }
    let bad = s(&dir.join("broken.pptx"));
    assert!(ss.execute("library.insert", &json!({"file": bad, "slide": 0})).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn search_finds_titles_text_and_file_names_in_any_case() {
    let dir = folder("search");
    std::fs::create_dir_all(dir.join("sub")).expect("mkdir");
    std::fs::write(dir.join("Roadmap.pptx"), deck(&[("Quarterly plan", "Hire a team\nShip the BETA"), ("Risks", "Budget")], "pptx")).expect("write");
    std::fs::write(dir.join("sub/beta-launch.deckcraft"), deck(&[("Launch", "Press")], "deckcraft")).expect("write");
    std::fs::write(dir.join("sub/Beta notes.pptx"), deck(&[("Beta", "")], "pptx")).expect("write");
    let mut ss = session_with(&dir);
    let hits = ss.execute("library.search", &json!({"query": "beta"})).expect("search");
    let hits = hits.as_array().cloned().unwrap_or_default();
    let found: Vec<(String, u64, String)> = hits
        .iter()
        .map(|h| {
            let file = h["file"].as_str().and_then(|f| Path::new(f).file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            (file, h["slide"].as_u64().unwrap_or(99), h["match"].as_str().unwrap_or_default().to_string())
        })
        .collect();
    assert_eq!(
        found,
        vec![
            ("Beta notes.pptx".to_string(), 0, "title".to_string()),
            ("Roadmap.pptx".to_string(), 0, "text".to_string()),
            ("beta-launch.deckcraft".to_string(), 0, "file".to_string()),
        ],
        "title matches first, then text, then file names"
    );
    assert_eq!(hits[0]["title"], json!("Beta"));
    let one = ss.execute("library.search", &json!({"query": "RISKS", "limit": 1})).expect("search");
    assert_eq!(one.as_array().map(Vec::len), Some(1));
    assert_eq!(one[0]["title"], json!("Risks"));
    assert_eq!(ss.execute("library.search", &json!({"query": "nothing like this"})).expect("search"), json!([]));
    for bad in
        [json!({}), json!({"query": "  "}), json!({"query": "a", "limit": 0}), json!({"query": "a", "limit": 501}), json!({"query": "x".repeat(300)})]
    {
        assert!(ss.execute("library.search", &bad).is_err(), "{bad}");
    }
    // One folder only.
    let other = folder("search-other");
    std::fs::write(other.join("beta.pptx"), deck(&[("Other", "")], "pptx")).expect("write");
    ss.execute("library.addFolder", &json!({"path": s(&other)})).expect("add");
    let all = ss.execute("library.search", &json!({"query": "beta"})).expect("search");
    let mine = ss.execute("library.search", &json!({"query": "beta", "folder": s(&other)})).expect("search");
    assert_eq!(all.as_array().map(Vec::len), Some(4));
    assert_eq!(mine.as_array().map(Vec::len), Some(1));
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(other);
}

#[test]
fn thumbnails_are_bounded_and_cached() {
    let dir = folder("thumbs");
    std::fs::write(dir.join("a.pptx"), deck(&[("One", ""), ("Two", "")], "pptx")).expect("write");
    let path = s(&dir.join("a.pptx"));
    let mut ss = session_with(&dir);
    let size = |v: &Value| (v["width"].as_u64().unwrap_or(0), v["height"].as_u64().unwrap_or(0));
    let t = ss.execute("library.thumbnail", &json!({"file": path, "slide": 1})).expect("thumb");
    assert_eq!(size(&t), (160, 90), "16:9 at the default width");
    let png = crate::cmd::base64_decode(t["png"].as_str().unwrap_or_default()).expect("base64");
    assert!(png.starts_with(b"\x89PNG"));
    for (w, h) in [(32, 18), (512, 288)] {
        assert_eq!(size(&ss.execute("library.thumbnail", &json!({"file": path, "slide": 0, "width": w})).expect("thumb")), (w, h));
    }
    for bad in [json!(31), json!(513), json!(-1), json!("big"), json!(1e9)] {
        assert!(ss.execute("library.thumbnail", &json!({"file": path, "slide": 0, "width": bad})).is_err(), "{bad}");
    }
    assert!(ss.execute("library.thumbnail", &json!({"file": path, "slide": 2})).is_err(), "no third slide");
    assert!(ss.execute("library.thumbnail", &json!({"file": path})).is_err(), "slide is required");
    // Asked again: the same picture from the cache.
    let (count, _, _, _) = ss.library.sizes();
    let again = ss.execute("library.thumbnail", &json!({"file": path, "slide": 1})).expect("thumb");
    assert_eq!(again["png"], t["png"]);
    assert_eq!(ss.library.sizes().0, count);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_tall_slide_makes_a_bounded_thumbnail() {
    let dir = folder("tall");
    let mut doc = deckcraft_model::defaults::new_presentation(None);
    doc.slide_size = deckcraft_geom::Size::new(100.0, 50_000.0);
    std::fs::write(dir.join("tall.deckcraft"), crate::cmd::file::save_bytes(&doc, "deckcraft").expect("save")).expect("write");
    let mut ss = session_with(&dir);
    let t = ss.execute("library.thumbnail", &json!({"file": s(&dir.join("tall.deckcraft")), "slide": 0, "width": 512})).expect("thumb");
    assert!(t["height"].as_u64().is_some_and(|h| h <= 2048), "{}", t["height"]);
    assert!(t["width"].as_u64().is_some_and(|w| (1..=512).contains(&w)));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn inserting_a_slide_is_one_undo_step() {
    let dir = folder("insert-slide");
    std::fs::write(dir.join("lib.pptx"), deck(&[("Agenda", "One\nTwo"), ("Thanks", "")], "pptx")).expect("write");
    let path = s(&dir.join("lib.pptx"));
    let mut ss = session_with(&dir);
    let before = ss.doc().expect("doc").doc.slides.len();
    let undo = ss.doc().expect("doc").history.undo.len();
    let r = ss.execute("library.insert", &json!({"file": path, "slide": 1})).expect("insert");
    assert_eq!(r["as"], json!("slide"));
    assert_eq!(r["ids"].as_array().map(Vec::len), Some(1));
    let st = ss.doc().expect("doc");
    assert_eq!(st.doc.slides.len(), before + 1);
    assert_eq!(st.current_slide().map(|x| x.title()), Some("Thanks".to_string()));
    assert_eq!(st.history.undo.len(), undo + 1, "one undo step");
    assert!(st.doc.validate().is_empty(), "{:?}", st.doc.validate());
    ss.execute("edit.undo", &json!({})).expect("undo");
    assert_eq!(ss.doc().expect("doc").doc.slides.len(), before);
    // Placed where asked, with the source's look.
    ss.execute("library.insert", &json!({"file": path, "slide": 0, "at": 0, "keepFormatting": true})).expect("insert");
    assert_eq!(ss.doc().expect("doc").doc.slides.first().map(|x| x.title()), Some("Agenda".to_string()));
    for bad in [
        json!({"file": path, "slide": 9}),
        json!({"file": path, "slide": 0, "as": "picture"}),
        json!({"file": path, "slide": -1}),
        json!({"slide": 0}),
    ] {
        assert!(ss.execute("library.insert", &bad).is_err(), "{bad}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn inserting_shapes_keeps_their_places_and_is_one_undo_step() {
    let dir = folder("insert-shapes");
    let mut lib = Session::with_new();
    lib.execute("slide.new", &json!({"layout": "titleOnly", "title": "Arrows"})).expect("slide");
    let star = lib.execute("shape.insert", &json!({"preset": "star5", "rect": [100, 120, 200, 150]})).expect("star")["id"].as_u64().unwrap_or(0);
    lib.execute("shape.rename", &json!({"id": star, "name": "Rating star"})).expect("rename");
    let title_box = {
        let st = lib.doc().expect("doc");
        let slide = st.current_slide().expect("slide");
        let t = slide.shapes.iter().find(|x| x.ph.is_some()).expect("title");
        crate::cmd::xfrm_of(&st.doc, &st.selection, t)
    };
    std::fs::write(dir.join("elements.pptx"), crate::cmd::file::save_bytes(&lib.doc().expect("doc").doc, "pptx").expect("save")).expect("write");
    let path = s(&dir.join("elements.pptx"));
    let mut ss = session_with(&dir);
    ss.execute("slide.new", &json!({"layout": "blank"})).expect("slide");
    let slide_before = ss.doc().expect("doc").doc.slides.len();
    let undo = ss.doc().expect("doc").history.undo.len();
    let r = ss.execute("library.insert", &json!({"file": path, "slide": 1, "as": "shapes"})).expect("insert shapes");
    assert_eq!(r["as"], json!("shapes"));
    let st = ss.doc().expect("doc");
    assert_eq!(st.doc.slides.len(), slide_before, "no slide added");
    assert_eq!(st.history.undo.len(), undo + 1, "one undo step");
    let shapes = &st.current_slide().expect("slide").shapes;
    assert_eq!(shapes.len(), 2, "the title and the star");
    let ids: Vec<u64> = r["ids"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    assert_eq!(ids, shapes.iter().map(|x| u64::from(x.id.0)).collect::<Vec<_>>());
    let title = shapes.iter().find(|x| x.text.as_ref().is_some_and(|t| t.text() == "Arrows")).expect("the title came as a shape");
    assert!(title.ph.is_none(), "a placeholder becomes a regular shape");
    let near = |a: f64, b: f64| (a - b).abs() < 0.5;
    let tx = title.xfrm.expect("explicit box");
    assert!(near(tx.x, title_box.x) && near(tx.y, title_box.y) && near(tx.w, title_box.w), "{tx:?} vs {title_box:?}");
    let s5 = shapes.iter().find(|x| x.text.is_none() || x.text.as_ref().is_some_and(|t| t.text().is_empty())).expect("star");
    let x = s5.xfrm.expect("box");
    assert!(near(x.x, 100.0) && near(x.y, 120.0) && near(x.w, 200.0) && near(x.h, 150.0), "{x:?}");
    assert_eq!(st.selection.shapes.len(), 2, "the new shapes are selected");
    assert!(st.doc.validate().is_empty(), "{:?}", st.doc.validate());
    // A second insert gives fresh ids and free names.
    ss.execute("library.insert", &json!({"file": path, "slide": 1, "as": "shapes"})).expect("again");
    let st = ss.doc().expect("doc");
    let all = &st.current_slide().expect("slide").shapes;
    let mut seen = std::collections::HashSet::new();
    assert!(all.iter().all(|x| seen.insert(x.id)), "ids are unique");
    ss.execute("edit.undo", &json!({})).expect("undo");
    ss.execute("edit.undo", &json!({})).expect("undo");
    assert!(ss.doc().expect("doc").current_slide().expect("slide").shapes.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_read_root_confines_every_library_command() {
    let base = folder("confined");
    let (inside, outside) = (base.join("in"), base.join("out"));
    std::fs::create_dir_all(&inside).expect("mkdir");
    std::fs::create_dir_all(&outside).expect("mkdir");
    std::fs::write(inside.join("ok.pptx"), deck(&[("Inside", "")], "pptx")).expect("write");
    std::fs::write(outside.join("secret.pptx"), deck(&[("Secret", "")], "pptx")).expect("write");
    // The person at the keyboard added both folders earlier.
    let mut ss = session_with(&inside);
    ss.execute("library.addFolder", &json!({"path": s(&outside)})).expect("add");
    let roots: Arc<AutomationRoots> = AutomationRoots::new(std::slice::from_ref(&inside), &[]).expect("roots").expect("some");
    confine(Some(&roots), || {
        let other = base.join("other");
        std::fs::create_dir_all(&other).expect("mkdir");
        let e = ss.execute("library.addFolder", &json!({"path": s(&other)})).expect_err("outside the read root");
        assert!(e.to_string().contains("outside the read root"), "{e}");
        let f = ss.execute("library.folders", &json!({})).expect("folders");
        assert!(f[0].get("error").is_none(), "{f}");
        assert!(f[1]["error"].as_str().is_some_and(|e| e.contains("outside the read root")), "{f}");
        let files = list(&mut ss, json!({}));
        assert_eq!(files.len(), 1, "only the folder inside the root: {files:?}");
        assert!(ss.execute("library.list", &json!({"folder": s(&outside)})).is_err());
        let hits = ss.execute("library.search", &json!({"query": "secret"})).expect("search");
        assert_eq!(hits, json!([]));
        let secret = s(&outside.join("secret.pptx"));
        for (id, p) in [
            ("library.thumbnail", json!({"file": secret, "slide": 0})),
            ("library.insert", json!({"file": secret, "slide": 0})),
            ("library.list", json!({"file": secret})),
        ] {
            let e = ss.execute(id, &p).expect_err(id);
            assert!(e.to_string().contains("outside the read root"), "{id}: {e}");
        }
        let ok = s(&inside.join("ok.pptx"));
        ss.execute("library.thumbnail", &json!({"file": ok, "slide": 0})).expect("inside the root");
    });
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn library_folders_survive_a_fresh_session() {
    let dir = folder("persist");
    std::fs::write(dir.join("kept.pptx"), deck(&[("Kept", "")], "pptx")).expect("write");
    let ss = session_with(&dir);
    let saved = serde_json::to_string(&ss.prefs).expect("prefs");
    assert!(saved.contains("libraryFolders"), "{saved}");
    let mut fresh = Session::with_new();
    fresh.prefs = serde_json::from_str::<Prefs>(&saved).expect("prefs").sanitized();
    let files = list(&mut fresh, json!({}));
    assert_eq!(files.len(), 1);
    assert_eq!(titles(&files[0]), vec!["Kept"]);
    // A preferences file from before the Library has none.
    let old: Prefs = serde_json::from_str(r#"{"author": "Ann"}"#).expect("old prefs");
    assert!(old.library_folders.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn reading_commands_are_not_undo_steps_or_journal_entries() {
    let dir = folder("query");
    std::fs::write(dir.join("a.pptx"), deck(&[("A", "")], "pptx")).expect("write");
    let path = s(&dir.join("a.pptx"));
    let mut ss = session_with(&dir);
    let journal = ss.journal.len();
    let undo = ss.doc().expect("doc").history.undo.len();
    ss.execute("library.folders", &json!({})).expect("folders");
    ss.execute("library.list", &json!({})).expect("list");
    ss.execute("library.search", &json!({"query": "a"})).expect("search");
    ss.execute("library.thumbnail", &json!({"file": path, "slide": 0})).expect("thumb");
    assert_eq!(ss.journal.len(), journal);
    assert_eq!(ss.doc().expect("doc").history.undo.len(), undo);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn hostile_library_params_are_errors_not_panics() {
    let dir = folder("hostile");
    std::fs::write(dir.join("a.pptx"), deck(&[("A", "x")], "pptx")).expect("write");
    let file = s(&dir.join("a.pptx"));
    let mut ss = session_with(&dir);
    let long = "x".repeat(100_000);
    let values = [
        json!(null),
        json!(""),
        json!("../../../../etc/passwd"),
        json!("a\u{0}b.pptx"),
        json!(long),
        json!(-1),
        json!(1e308),
        json!(u64::MAX),
        json!([file]),
        json!({"file": file}),
        json!(file),
        json!(s(&dir)),
    ];
    let ids: Vec<&str> =
        crate::command_specs().iter().map(|c| c.id).filter(|id| id.starts_with("library.") && *id != "library.removeFolder").collect();
    assert_eq!(ids.len(), 6);
    for id in &ids {
        for v in &values {
            for key in ["path", "file", "folder", "query", "slide", "width", "limit", "as", "at", "slides"] {
                let mut p = json!({"file": file, "slide": 0, "query": "a", "path": s(&dir)});
                p[key] = v.clone();
                let _ = ss.execute(id, &p);
            }
        }
    }
    assert_eq!(ss.panics_caught, 0);
    assert!(ss.prefs.library_folders.len() <= MAX_FOLDERS);
    assert!(ss.doc().expect("doc").doc.validate().is_empty());
    ss.execute("library.removeFolder", &json!({"path": s(&dir)})).expect("remove");
    let _ = std::fs::remove_dir_all(dir);
}
