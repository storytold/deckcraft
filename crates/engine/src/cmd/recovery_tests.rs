//! The recovery commands — `file.recovery.save`, `.list`, `.open` and `.discard`, what the app
//! writes on a timer and reads back after a crash — plus `file.revert` and `window.next`, the rest
//! of `file.rs` that had no test. The folder inventory behind the commands is tested in
//! `recovery.rs`; these pin what the commands hand callers and the work a crash leaves behind.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("deckcraft-recovery-cmd-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

fn done(root: &Path) {
    let _ = std::fs::remove_dir_all(root);
}

/// A session with one never-saved, edited presentation and `dir` as its recovery folder.
fn crashing(dir: &Path) -> Session {
    let mut s = Session::with_new();
    s.recovery_dir = Some(dir.to_path_buf());
    s.execute("slide.new", &json!({"layout": "blank"})).expect("an edit");
    s
}

/// The uids of the snapshots in `dir`, in the order a caller reads them back.
fn snapshot_uids(dir: &Path) -> Vec<u64> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut out: Vec<u64> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("deckcraft") {
                return None;
            }
            p.file_stem()?.to_str()?.parse().ok()
        })
        .collect();
    out.sort_unstable();
    out
}

fn meta_of(dir: &Path, uid: u64) -> Value {
    let bytes = std::fs::read(dir.join(format!("{uid}.json"))).expect("the entry's metadata");
    serde_json::from_slice(&bytes).expect("JSON")
}

#[test]
fn the_recovery_commands_need_a_folder() {
    let mut s = Session::with_new();
    for id in ["file.recovery.save", "file.recovery.list", "file.recovery.open", "file.recovery.discard"] {
        let err = s.execute(id, &json!({})).expect_err("no recovery folder is set");
        assert!(err.to_string().contains("no recovery folder is set"), "{id}: {err}");
    }
    assert_eq!(s.documents().len(), 1, "a refused call touches nothing");

    // `file.recovery.save` is the one that also needs a presentation to write.
    let mut empty = Session::new();
    let err = empty.execute("file.recovery.save", &json!({})).expect_err("no presentation is open");
    assert!(err.to_string().contains("no presentation open"), "{err}");
}

#[test]
fn a_recovery_save_writes_the_unsaved_presentations_and_skips_the_saved_ones() {
    let root = temp("save");
    let dir = root.join("recovery");
    let mut s = Session::with_new();
    s.recovery_dir = Some(dir.clone());

    // Nothing has been edited: nothing to write, and the folder is claimed (locked) for us.
    let v = s.execute("file.recovery.save", &json!({})).expect("a recovery save");
    assert_eq!(v["written"], 0);
    assert!(snapshot_uids(&dir).is_empty());
    assert!(dir.join(format!("owner-{}.lock", crate::recovery::instance_id())).exists(), "the instance holds its lock");

    // An edit leaves the presentation itself and its metadata behind.
    let st = s.doc().expect("a document");
    let (uid, slides) = (st.uid, st.doc.slides.len());
    s.execute("slide.new", &json!({"layout": "blank"})).expect("an edit");
    let v = s.execute("file.recovery.save", &json!({})).expect("a recovery save");
    assert_eq!(v["written"], 1);
    assert_eq!(snapshot_uids(&dir), vec![uid], "one entry, whatever else the folder holds");
    let bytes = std::fs::read(dir.join(format!("{uid}.deckcraft"))).expect("the snapshot");
    let loaded = deckcraft_format::load(&bytes).expect("a presentation");
    assert_eq!(loaded.slides.len(), slides + 1, "the snapshot is the presentation as it is now");
    let meta = meta_of(&dir, uid);
    assert!(meta["path"].is_null(), "never saved, so nowhere to go back to");
    assert!(meta["title"].as_str().is_some_and(|t| !t.is_empty()));
    assert!(meta["saved"].as_u64().expect("a stamp") > 0);
    assert_eq!(meta["owner"].as_u64(), Some(crate::recovery::instance_id()));

    // Saving the presentation ends its recovery now, not at the next checkpoint.
    let out = root.join("talk.deckcraft");
    s.execute("file.saveAs", &json!({"path": out.to_string_lossy()})).expect("save it");
    assert!(snapshot_uids(&dir).is_empty(), "saving drops the entry");
    let v = s.execute("file.recovery.save", &json!({})).expect("a recovery save");
    assert_eq!(v["written"], 0, "and nothing comes back");
    done(&root);
}

#[test]
fn recovery_list_reports_every_entry_with_its_uid() {
    let root = temp("list");
    let dir = root.join("recovery");
    let mut one = crashing(&dir);
    let mut two = crashing(&dir);
    let (a, b) = (one.doc().expect("a document").uid, two.doc().expect("a document").uid);
    one.execute("file.recovery.save", &json!({})).expect("its entry");
    two.execute("file.recovery.save", &json!({})).expect("its entry");

    let v = one.execute("file.recovery.list", &json!({})).expect("the entries");
    let list = v.as_array().expect("an array");
    let got: Vec<u64> = list.iter().map(|e| e["uid"].as_u64().expect("a uid")).collect();
    assert_eq!(got, vec![a.min(b), a.max(b)], "the pane reads them uid-ordered");
    for e in list {
        assert!(e["saved"].as_u64().is_some_and(|t| t > 0), "an entry says when it was written: {e}");
    }

    // An entry whose metadata was hand-edited or torn is still listed with what could be read, so
    // the pane can show it and `discard` can get rid of it.
    std::fs::write(dir.join("999.deckcraft"), b"torn").expect("a broken entry");
    std::fs::write(dir.join("999.json"), b"[1, 2]").expect("metadata that is not an object");
    let v = one.execute("file.recovery.list", &json!({})).expect("the entries");
    let list = v.as_array().expect("an array");
    assert_eq!(list.len(), 3);
    let broken = list.iter().find(|e| e["uid"] == 999).expect("the broken entry is listed too");
    assert_eq!(broken, &json!({"uid": 999}), "only its uid could be read");

    // A folder that was never written to lists nothing rather than failing.
    one.recovery_dir = Some(root.join("never-used"));
    assert_eq!(one.execute("file.recovery.list", &json!({})).expect("the entries"), json!([]));
    done(&root);
}

#[test]
fn a_recovery_open_reopens_the_entries_as_unsaved_recovered_presentations() {
    let root = temp("open");
    let dir = root.join("recovery");
    let out = root.join("talk.deckcraft");
    let out_str = out.to_string_lossy().to_string();
    // A presentation that had been saved, and then edited when the "crash" happened.
    let mut run = Session::with_new();
    run.recovery_dir = Some(dir.clone());
    run.execute("file.saveAs", &json!({"path": out_str})).expect("save it");
    run.execute("slide.new", &json!({"layout": "blank"})).expect("an edit");
    let st = run.doc().expect("a document");
    let (uid, slides) = (st.uid, st.doc.slides.len());
    run.execute("file.recovery.save", &json!({})).expect("its entry");
    drop(run);

    let mut next = Session::with_new();
    next.recovery_dir = Some(dir.clone());
    let v = next.execute("file.recovery.open", &json!({})).expect("the entries");
    let opened = v["opened"].as_array().expect("an array");
    assert_eq!(opened.len(), 1);
    assert_eq!(Some(opened[0].as_u64().expect("a document index") as usize), next.active_index(), "the recovered presentation comes to the front");
    let st = next.doc().expect("a document");
    assert_eq!(st.uid, uid, "it keeps the entry's uid, so the snapshot stays tied to it");
    assert_eq!(st.doc.slides.len(), slides, "with the slides the snapshot holds");
    assert!(st.is_dirty(), "recovered work is unsaved work");
    assert_eq!(st.path.as_deref(), Some(out_str.as_str()), "remembering where it was saved");
    assert_eq!(st.title(), "talk", "named after the file it came from");

    // Asking again opens nothing: it is already here.
    let v = next.execute("file.recovery.open", &json!({})).expect("the entries");
    assert_eq!(v["opened"], json!([]));

    // The snapshot stays on disk until the recovered presentation is saved — then it goes.
    next.execute("file.save", &json!({})).expect("save the recovered work");
    assert!(snapshot_uids(&dir).is_empty());
    assert!(!next.doc().expect("a document").is_dirty(), "saving settles it");
    done(&root);
}

#[test]
fn a_recovered_presentation_that_was_never_saved_asks_for_a_path() {
    let root = temp("nosave");
    let dir = root.join("recovery");
    let mut run = crashing(&dir);
    run.execute("file.recovery.save", &json!({})).expect("its entry");
    drop(run);

    let mut next = Session::with_new();
    next.recovery_dir = Some(dir.clone());
    next.execute("file.recovery.open", &json!({})).expect("the entries");
    let st = next.doc().expect("a document");
    assert!(st.path.is_none(), "it was never saved anywhere");
    assert!(st.title().ends_with("(Recovered)"), "{}", st.title());

    // `file.save` has nowhere to go, so it asks the host for a path instead of guessing — and the
    // snapshot is left alone until then.
    let v = next.execute("file.save", &json!({})).expect("save");
    assert_eq!(v, json!({"needsPath": true}));
    assert_eq!(snapshot_uids(&dir).len(), 1);
    done(&root);
}

#[test]
fn recovery_discard_takes_one_entry_or_the_whole_folder() {
    let root = temp("discard");
    let dir = root.join("recovery");
    let mut one = crashing(&dir);
    let mut two = crashing(&dir);
    let (a, b) = (one.doc().expect("a document").uid, two.doc().expect("a document").uid);
    one.execute("file.recovery.save", &json!({})).expect("its entry");
    two.execute("file.recovery.save", &json!({})).expect("its entry");
    // An entry that would never load is still the discards' job: one torn snapshot can't wedge the
    // folder.
    std::fs::write(dir.join("999.deckcraft"), b"torn").expect("a broken entry");
    std::fs::write(dir.join("999.json"), b"{").expect("its torn metadata");
    assert_eq!(snapshot_uids(&dir).len(), 3);

    // One uid takes that entry and leaves the rest alone.
    one.execute("file.recovery.discard", &json!({"uid": a})).expect("discard one");
    let left = snapshot_uids(&dir);
    assert_eq!(left.len(), 2);
    assert!(!left.contains(&a) && left.contains(&b) && left.contains(&999));
    assert!(!dir.join(format!("{a}.json")).exists(), "the metadata goes with the snapshot");

    // A uid that is not there is nothing to do, not an error.
    assert_eq!(one.execute("file.recovery.discard", &json!({"uid": 1_234_567_890_123u64})).expect("nothing to discard"), Value::Null);
    assert_eq!(snapshot_uids(&dir).len(), 2);

    // Without a uid the whole folder is cleared, unreadable entries included.
    one.execute("file.recovery.discard", &json!({})).expect("discard all");
    assert!(snapshot_uids(&dir).is_empty());
    assert!(!dir.join(format!("{b}.json")).exists() && !dir.join("999.json").exists());
    done(&root);
}

#[test]
fn file_revert_throws_away_the_edits_since_the_last_save() {
    let root = temp("revert");
    let mut s = Session::with_new();
    s.execute("file.saveAs", &json!({"path": root.join("talk.deckcraft").to_string_lossy()})).expect("save it");
    let slides = s.doc().expect("a document").doc.slides.len();
    s.execute("slide.new", &json!({"layout": "blank"})).expect("an edit");
    assert!(s.doc().expect("a document").is_dirty());
    assert_eq!(s.doc().expect("a document").history.undo.len(), 1, "the edit is one step back");

    assert_eq!(s.execute("file.revert", &json!({})).expect("revert"), Value::Null);
    let st = s.doc().expect("a document");
    assert_eq!(st.doc.slides.len(), slides, "back to what was last saved");
    assert!(!st.is_dirty());
    assert!(st.history.undo.is_empty() && st.history.redo.is_empty(), "there is nothing left to undo or redo");

    // Calling it with nothing to revert to is harmless.
    assert_eq!(s.execute("file.revert", &json!({})).expect("nothing to revert"), Value::Null);
    assert_eq!(s.doc().expect("a document").doc.slides.len(), slides);
    done(&root);
}

#[test]
fn window_next_walks_the_open_presentations_and_can_jump() {
    let mut s = Session::with_new();
    let first = s.doc().expect("a document").uid;
    s.execute("file.new", &json!({})).expect("a second presentation");
    let second = s.doc().expect("a document").uid;
    assert_eq!(s.active_index(), Some(1), "a new presentation comes to the front");

    s.execute("window.next", &json!({})).expect("next window");
    assert_eq!(s.doc().expect("a document").uid, first, "next wraps round");
    s.execute("window.next", &json!({})).expect("next window");
    assert_eq!(s.doc().expect("a document").uid, second);

    // `index` jumps straight there; past the end it goes to the last window rather than failing.
    s.execute("window.next", &json!({"index": 0})).expect("the first window");
    assert_eq!(s.doc().expect("a document").uid, first);
    s.execute("window.next", &json!({"index": 9})).expect("past the end");
    assert_eq!(s.doc().expect("a document").uid, second, "the last window it has");
}
