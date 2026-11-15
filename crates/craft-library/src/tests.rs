//! The core with a toy app: `.notes` text files whose lines are pages (`title|text`, a leading
//! `!` hides one), read through the plain file system.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use super::*;

struct Disk;

impl Fs for Disk {
    fn read_dir(&self, path: &Path, max: usize) -> Result<(Vec<DirEntry>, bool), String> {
        let mut out = vec![];
        for e in std::fs::read_dir(path).map_err(|e| e.to_string())? {
            if out.len() >= max {
                return Ok((out, true));
            }
            let e = e.map_err(|e| e.to_string())?;
            let t = e.file_type().map_err(|e| e.to_string())?;
            let kind = if t.is_symlink() {
                EntryKind::Link
            } else if t.is_dir() {
                EntryKind::Folder
            } else if t.is_file() {
                EntryKind::File
            } else {
                EntryKind::Other
            };
            let stamp = e.metadata().map(|m| Stamp { len: m.len(), modified: m.modified().ok() }).unwrap_or_default();
            out.push(DirEntry { path: e.path(), name: e.file_name().to_str().map(String::from), kind, stamp });
        }
        Ok((out, false))
    }
    fn stat(&self, path: &Path) -> Result<(EntryKind, Stamp), String> {
        let m = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        let kind = if m.is_dir() {
            EntryKind::Folder
        } else if m.is_file() {
            EntryKind::File
        } else {
            EntryKind::Link
        };
        Ok((kind, Stamp { len: m.len(), modified: m.modified().ok() }))
    }
    fn canonical(&self, path: &Path) -> Result<PathBuf, String> {
        std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))
    }
    fn read(&self, path: &Path, max: u64) -> Result<Vec<u8>, String> {
        let b = std::fs::read(path).map_err(|e| e.to_string())?;
        if b.len() as u64 > max { Err("too big".into()) } else { Ok(b) }
    }
}

#[derive(Default)]
struct Notes;

impl Kind for Notes {
    type Doc = Vec<Item>;
    fn extensions(&self) -> &[&str] {
        &["notes"]
    }
    fn nouns(&self) -> Nouns {
        Nouns { one: "page", many: "pages" }
    }
    fn open(&self, name: &str, bytes: &[u8]) -> Result<Vec<Item>, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| format!("{name} is not text"))?;
        if text.starts_with("PANIC") {
            panic!("a reader that panics on a hostile file");
        }
        Ok(text
            .lines()
            .map(|l| {
                let (hidden, l) = match l.strip_prefix('!') {
                    Some(rest) => (true, rest),
                    None => (false, l),
                };
                let (title, text) = l.split_once('|').unwrap_or((l, ""));
                Item { title: title.into(), hidden, text: text.into() }
            })
            .collect())
    }
    fn title(&self, _doc: &Vec<Item>) -> Option<String> {
        None
    }
    fn count(&self, doc: &Vec<Item>) -> usize {
        doc.len()
    }
    fn item(&self, doc: &Vec<Item>, index: usize) -> Item {
        doc.get(index).cloned().unwrap_or_default()
    }
    fn thumbnail(&self, _doc: &Vec<Item>, index: usize, width: u32) -> Result<Thumbnail, String> {
        Ok(Thumbnail { png: Arc::new(format!("page {index} at {width}").into_bytes()), width, height: width / 2 })
    }
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("craft-library-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[test]
fn files_are_named_by_extension_and_hidden_ones_are_not() {
    for yes in ["a.notes", "A.NOTES", "d.e.notes"] {
        assert!(is_listed(yes, &["notes"]), "{yes}");
    }
    for no in [".notes", "~$a.notes", ".hidden.notes", "a.note", "a.notes.txt", "notes"] {
        assert!(!is_listed(no, &["notes"]), "{no}");
    }
}

#[test]
fn folders_from_a_preferences_file_are_bounded() {
    let mut v: Vec<String> = (0..100).map(|i| format!("/f{i}")).collect();
    v.insert(0, String::new());
    v.insert(1, "/f0".into());
    v.insert(2, "x".repeat(MAX_PATH + 1));
    let out = sanitized_folders(v);
    assert_eq!(out.len(), MAX_FOLDERS);
    assert_eq!(out.first().map(String::as_str), Some("/f0"));
}

#[test]
fn caps_keep_whole_characters_and_base64_is_standard() {
    assert_eq!(cap_chars("héllo", 2), "hé");
    let mut t = "ééé".to_string();
    cap_bytes(&mut t, 3);
    assert_eq!(t, "é");
    for (raw, enc) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foobar", "Zm9vYmFy")] {
        assert_eq!(base64(raw.as_bytes()), enc);
    }
}

#[test]
fn the_contract_speaks_the_apps_nouns() {
    let dir = folder("nouns");
    std::fs::write(dir.join("Plans.notes"), "Agenda|first steps\n!Hidden|secret\nBudget|costs").unwrap();
    std::fs::write(dir.join("Broken.notes"), [0xFF, 0xFE]).unwrap();
    std::fs::write(dir.join("Boom.notes"), "PANIC").unwrap();
    std::fs::write(dir.join("other.txt"), "not listed").unwrap();
    let mut folders = vec![];
    Library::<Notes>::add_folder(&Disk, &mut folders, &json!({"path": s(&dir)})).unwrap();
    let mut lib = Library::new(Notes);
    let listed = lib.list(&Disk, &folders, &json!({})).unwrap();
    let files = listed.as_array().unwrap();
    assert_eq!(files.len(), 3);
    let plans = files.iter().find(|f| f["title"] == json!("Plans")).unwrap();
    assert_eq!(plans["pageCount"], json!(3));
    assert_eq!(plans["pages"][1], json!({"index": 1, "title": "Hidden", "hidden": true}));
    // A file that can't be read, and one whose reader panics, are listed with their errors.
    for name in ["Broken", "Boom"] {
        let f = files.iter().find(|f| f["title"] == json!(name)).unwrap();
        assert!(f["error"].as_str().is_some_and(|e| !e.is_empty()), "{f}");
    }
    let hits = lib.search(&Disk, &folders, &json!({"query": "BUDGET"})).unwrap();
    assert_eq!(hits[0]["page"], json!(2));
    assert_eq!(hits[0]["match"], json!("title"));
    let hits = lib.search(&Disk, &folders, &json!({"query": "plans"})).unwrap();
    assert_eq!(hits.as_array().map(Vec::len), Some(3), "every page of a file whose name matches");
    let file = s(&dir.join("Plans.notes"));
    let t = lib.thumbnail(&Disk, &folders, &json!({"file": file, "page": 2, "width": 64})).unwrap();
    assert_eq!((t["width"].clone(), t["height"].clone()), (json!(64), json!(32)));
    assert_eq!(t["png"], json!(base64(b"page 2 at 64")));
    assert!(lib.thumbnail(&Disk, &folders, &json!({"file": file, "page": 3})).is_err());
    assert!(lib.thumbnail(&Disk, &folders, &json!({"file": file, "slide": 0})).unwrap_err().contains("missing `page`"));
    assert!(lib.thumbnail(&Disk, &folders, &json!({"file": s(&dir.join("other.txt")), "page": 0})).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn walking_stops_at_the_bounds() {
    let dir = folder("bounds");
    for i in 0..=MAX_ENTRIES {
        std::fs::write(dir.join(format!("f{i:05}.txt")), "").unwrap();
    }
    let w = walk(&Disk, &dir, &["notes"]).unwrap();
    assert_eq!((w.entries, w.truncated), (MAX_ENTRIES, true));
    let deep = folder("deep");
    let mut at = deep.clone();
    for level in 0..=MAX_DEPTH + 1 {
        std::fs::write(at.join(format!("level{level}.notes")), "x").unwrap();
        at = at.join(format!("d{level}"));
        std::fs::create_dir_all(&at).unwrap();
    }
    std::fs::create_dir_all(deep.join(".git")).unwrap();
    std::fs::write(deep.join(".git/hidden.notes"), "x").unwrap();
    let w = walk(&Disk, &deep, &["notes"]).unwrap();
    assert_eq!(w.files.len(), MAX_DEPTH + 1);
    assert_eq!(w.too_deep, 1);
    assert!(walk(&Disk, &deep.join("missing"), &["notes"]).is_err());
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(deep);
}

#[test]
fn folders_are_kept_bounded_and_removed_by_any_spelling() {
    let dir = folder("many");
    let mut folders = vec![];
    for i in 0..MAX_FOLDERS {
        let d = dir.join(format!("f{i}"));
        std::fs::create_dir_all(&d).unwrap();
        Library::<Notes>::add_folder(&Disk, &mut folders, &json!({"path": s(&d)})).unwrap();
    }
    let extra = dir.join("extra");
    std::fs::create_dir_all(&extra).unwrap();
    let e = Library::<Notes>::add_folder(&Disk, &mut folders, &json!({"path": s(&extra)})).unwrap_err();
    assert!(e.contains("at most 32 folders"), "{e}");
    let sep = std::path::MAIN_SEPARATOR;
    let other_spelling = format!("{}{sep}f0{sep}..{sep}f0", s(&dir));
    let r: Value = Library::<Notes>::remove_folder(&Disk, &mut folders, &json!({"path": other_spelling})).unwrap();
    assert_eq!(r["path"], json!(s(&dir.join("f0"))));
    assert_eq!(folders.len(), MAX_FOLDERS - 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// Every folder listing and file read goes through the app's [`Fs`], which is where an app
/// confines them (DeckCraft's automation read roots). The core reaching for the file system itself
/// would bypass that, so it may not (its tests may).
#[test]
fn the_core_reads_files_only_through_the_apps_fs() {
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs")).unwrap();
    let code = src.split("#[cfg(test)]").next().unwrap_or_default();
    for word in ["std::fs", "fs::", "File::open", "File::create", "OpenOptions"] {
        let used: Vec<&str> = code.lines().filter(|l| !l.trim_start().starts_with("//") && l.contains(word)).collect();
        assert!(used.is_empty(), "{word}: {used:?}");
    }
}
