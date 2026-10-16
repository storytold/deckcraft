//! Crash recovery (PowerPoint's AutoRecover): unsaved changes of every open presentation are
//! written to a recovery folder (`file.recovery.save`, called on a timer by the app and after a
//! command panics); saving or closing a presentation removes its entry; after a crash
//! `file.recovery.open` reopens what was left there as unsaved presentations that remember where
//! they were saved.
//!
//! An entry is `<uid>.deckcraft` (the presentation) plus `<uid>.json` (`{"path", "title",
//! "saved", "owner"}`: the original file, the title, when it was written (Unix seconds) and the
//! instance that owns it).
//!
//! Several instances share the folder. Document uids are unique across processes, each instance
//! holds a lock on `owner-<instance>.lock` while it runs, and `file.recovery.open` leaves alone
//! the entries of an instance whose lock is still held. Opening an entry doesn't delete it: the
//! recovered presentation keeps the entry's uid, so the snapshot stays on disk until the next
//! recovery save replaces it or a save of the presentation removes it.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};

use crate::{DocState, EngineError, Result, Session};

/// The platform's per-user recovery folder.
pub fn default_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return home.map(|h| h.join("Library/Application Support/DeckCraft/Recovery"));
    }
    if cfg!(target_os = "windows") {
        return std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("DeckCraft").join("Recovery"));
    }
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.map(|h| h.join(".local/share"))).map(|d| d.join("deckcraft/recovery"))
}

/// Uids and instance ids stay below 2^53 so they survive JSON numbers read as `f64`.
pub(crate) const UID_MASK: u64 = (1 << 53) - 1;

/// This process's random id, with the low 24 bits clear so its document uids (id + counter) don't
/// run into another instance's.
pub fn instance_id() -> u64 {
    static ID: OnceLock<u64> = OnceLock::new();
    *ID.get_or_init(|| {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        #[cfg(not(target_arch = "wasm32"))]
        h.write_u32(std::process::id());
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(d) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            h.write_u128(d.as_nanos());
        }
        // Never 0: old entries are numbered from 1.
        ((h.finish() & UID_MASK) & !0xFF_FFFF).max(1 << 24)
    })
}

fn lock_path(dir: &Path, owner: u64) -> PathBuf {
    dir.join(format!("owner-{owner}.lock"))
}

/// Hold this instance's lock in `dir` for the rest of the process (once per folder).
fn claim_dir(dir: &Path) {
    static HELD: Mutex<Vec<(PathBuf, File)>> = Mutex::new(Vec::new());
    let Ok(mut held) = HELD.lock() else { return };
    if held.iter().any(|(d, _)| d == dir) {
        return;
    }
    let path = lock_path(dir, instance_id());
    match File::create(&path).and_then(|f| f.try_lock().map(|()| f).map_err(std::io::Error::other)) {
        Ok(f) => held.push((dir.to_path_buf(), f)),
        Err(e) => log::warn!("recovery: could not lock {}: {e}", path.display()),
    }
}

/// Whether another running instance holds `owner`'s lock in `dir`.
fn owner_alive(dir: &Path, owner: u64) -> bool {
    let Ok(f) = File::open(lock_path(dir, owner)) else { return false };
    matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock))
}

/// Write `bytes` to `path` through a temporary file and a rename, so a crash never leaves it torn.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
    std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path)).map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))
}

/// Presentations reopened per start; the rest stay on disk for the next one, so a folder
/// full of entries can't stall start-up.
pub const MAX_REOPEN: usize = 32;
/// Largest entry read back.
const MAX_ENTRY_BYTES: u64 = 2 << 30;
const MAX_META_BYTES: u64 = 64 << 10;

/// `p` as bytes when it's a regular file no bigger than `max`: reading a FIFO left where an
/// entry belongs would block start-up forever.
fn read_regular(p: &Path, max: u64) -> Option<Vec<u8>> {
    let m = std::fs::metadata(p).ok()?;
    (m.is_file() && m.len() <= max).then(|| std::fs::read(p).ok()).flatten()
}

fn files(dir: &Path, uid: u64) -> (PathBuf, PathBuf) {
    (dir.join(format!("{uid}.deckcraft")), dir.join(format!("{uid}.json")))
}

fn now_secs() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

/// Remove a presentation's recovery entry (it was saved or closed).
pub fn discard(dir: &Path, uid: u64) {
    let (d, m) = files(dir, uid);
    let _ = std::fs::remove_file(d);
    let _ = std::fs::remove_file(m);
}

/// Write entries for the unsaved presentations (and drop those of saved ones). Returns how many
/// were written.
pub fn save(s: &Session, dir: &Path) -> Result<usize> {
    std::fs::create_dir_all(dir).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
    claim_dir(dir);
    let mut n = 0;
    for d in s.documents() {
        if !d.is_dirty() {
            discard(dir, d.uid);
            continue;
        }
        let (doc, meta) = files(dir, d.uid);
        let bytes = deckcraft_format::save(&d.doc).map_err(|e| EngineError::Other(e.to_string()))?;
        write_atomic(&doc, &bytes)?;
        let m = json!({"path": d.path, "title": d.title(), "saved": now_secs(), "owner": instance_id()});
        write_atomic(&meta, m.to_string().as_bytes())?;
        n += 1;
    }
    Ok(n)
}

/// Entries in `dir`: (uid, metadata).
pub fn list(dir: &Path) -> Vec<(u64, Value)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("deckcraft") {
            continue;
        }
        if !std::fs::metadata(&p).is_ok_and(|m| m.is_file()) {
            continue;
        }
        let Some(uid) = p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u64>().ok()) else { continue };
        // Metadata is a file on disk: anything but an object (corrupt, hand-edited) counts as none.
        let meta = read_regular(&p.with_extension("json"), MAX_META_BYTES)
            .and_then(|t| serde_json::from_slice::<Value>(&t).ok())
            .filter(Value::is_object)
            .unwrap_or(json!({}));
        out.push((uid, meta));
    }
    out.sort_by_key(|e| e.0);
    out
}

/// Reopen every entry left by an instance that is no longer running as an unsaved presentation.
/// Entries stay on disk, now owned by this instance: the presentation keeps the entry's uid, so
/// the next recovery save replaces the snapshot and saving or closing it removes it. Entries of a
/// running instance, unreadable entries and entries already open here are left alone. Returns the
/// opened documents' indices. At most [`MAX_REOPEN`] are opened per start, the newest first; the rest
/// wait for the next one.
pub fn open(s: &mut Session, dir: &Path) -> Result<Vec<usize>> {
    let me = instance_id();
    let mut opened = Vec::new();
    let mut entries = list(dir);
    entries.sort_by_key(|(uid, meta)| std::cmp::Reverse((meta.get("saved").and_then(Value::as_u64).unwrap_or(0), *uid)));
    for (uid, mut meta) in entries {
        if opened.len() >= MAX_REOPEN {
            break;
        }
        if s.documents().iter().any(|d| d.uid == uid) {
            continue;
        }
        if let Some(owner) = meta.get("owner").and_then(Value::as_u64)
            && owner != me
            && owner_alive(dir, owner)
        {
            continue;
        }
        let (doc, meta_path) = files(dir, uid);
        let Some(bytes) = read_regular(&doc, MAX_ENTRY_BYTES) else { continue };
        let Ok(mut d) = deckcraft_format::load(&bytes) else { continue };
        deckcraft_format::repair(&mut d);
        let path = meta.get("path").and_then(Value::as_str).map(str::to_string);
        let title = meta.get("title").and_then(Value::as_str).filter(|t| !t.is_empty()).unwrap_or("Presentation").to_string();
        // Take the entry over (so another instance starting now leaves it alone), keeping its
        // snapshot until this one writes a newer one.
        claim_dir(dir);
        if let Some(o) = meta.as_object_mut() {
            o.insert("owner".into(), json!(me));
        }
        if let Err(e) = write_atomic(&meta_path, meta.to_string().as_bytes()) {
            log::warn!("recovery: {e}");
        }
        let mut st = DocState::new(d, path, format!("{title} (Recovered)"));
        st.uid = uid;
        // Unsaved: the copy on disk (if any) is older than what was recovered.
        st.saved_doc = std::sync::Arc::new((*st.doc).clone());
        st.revision += 1;
        opened.push(s.add_document(st));
    }
    remove_stale_locks(dir);
    Ok(opened)
}

/// Delete the lock files of instances that are no longer running.
fn remove_stale_locks(dir: &Path) {
    let me = instance_id();
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name();
        let Some(owner) =
            name.to_str().and_then(|n| n.strip_prefix("owner-")).and_then(|n| n.strip_suffix(".lock")).and_then(|n| n.parse::<u64>().ok())
        else {
            continue;
        };
        if owner != me && !owner_alive(dir, owner) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("deckcraft-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn recovers_unsaved_presentations() {
        let dir = temp("recovery");
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.new", &json!({})).unwrap();
        // Only the edited presentation is written.
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        assert_eq!(save(&s, &dir).unwrap(), 1);
        assert_eq!(list(&dir).len(), 1);
        // "Crash": a new session reopens it, unsaved, with its slides.
        let mut s2 = Session::new();
        assert_eq!(open(&mut s2, &dir).unwrap().len(), 1);
        let st = s2.doc().unwrap();
        assert!(st.is_dirty());
        assert_eq!(st.doc.slides.len(), 2);
        assert!(st.title().ends_with("(Recovered)"));
        assert_eq!(list(&dir).len(), 1, "the snapshot stays until it is replaced or saved");
        // A second "crash" before any checkpoint: the presentation is recovered again.
        let mut s3 = Session::new();
        assert_eq!(open(&mut s3, &dir).unwrap().len(), 1);
        assert_eq!(s3.doc().unwrap().doc.slides.len(), 2);
        // Opening again in the same session doesn't duplicate it.
        assert!(open(&mut s3, &dir).unwrap().is_empty());
        // Saving the recovered presentation drops the entry.
        s3.recovery_dir = Some(dir.clone());
        let path = dir.join("saved.deckcraft");
        s3.execute("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
        assert!(list(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn uids_are_unique_beyond_this_process() {
        let a = DocState::new(Default::default(), None, "a".into());
        let b = DocState::new(Default::default(), None, "b".into());
        assert_ne!(a.uid, b.uid);
        // Not the small per-process numbers every instance used to start from.
        assert!(a.uid >= 1 << 24 && a.uid <= UID_MASK);
    }

    #[test]
    fn another_running_instance_keeps_its_entries() {
        let dir = temp("recovery-shared");
        std::fs::create_dir_all(&dir).unwrap();
        // Another instance (its own owner id and uid, its lock held) has an unsaved presentation.
        let other = instance_id() ^ (1 << 40);
        let lock = File::create(lock_path(&dir, other)).unwrap();
        lock.try_lock().unwrap();
        let mut o = Session::new();
        o.execute("file.new", &json!({})).unwrap();
        o.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        let bytes = deckcraft_format::save(&o.doc().unwrap().doc).unwrap();
        let uid = other + 5;
        let (d, m) = files(&dir, uid);
        std::fs::write(&d, bytes).unwrap();
        std::fs::write(&m, json!({"title": "Theirs", "owner": other}).to_string()).unwrap();
        // This instance saving its clean and dirty presentations leaves that entry alone...
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        assert_eq!(save(&s, &dir).unwrap(), 1);
        assert!(list(&dir).iter().any(|e| e.0 == uid));
        // ...and a recovery open here skips it while its owner runs.
        let mut s2 = Session::new();
        let opened = open(&mut s2, &dir).unwrap();
        assert!(s2.documents().iter().all(|d| d.uid != uid), "{opened:?}");
        assert!(lock_path(&dir, other).exists(), "a live owner's lock stays");
        // Once the owner is gone, its entry is recovered and its lock file cleaned up.
        drop(lock);
        let mut s3 = Session::new();
        open(&mut s3, &dir).unwrap();
        assert!(s3.documents().iter().any(|d| d.uid == uid && d.title().starts_with("Theirs")));
        assert!(!lock_path(&dir, other).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_entries_are_skipped() {
        let dir = temp("recovery-corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("7.deckcraft"), b"not a presentation").unwrap();
        std::fs::write(dir.join("7.json"), b"[1, 2]").unwrap();
        let mut s = Session::new();
        assert!(open(&mut s, &dir).unwrap().is_empty());
        s.recovery_dir = Some(dir.clone());
        let v = s.execute("file.recovery.list", &json!({})).unwrap();
        assert_eq!(v.as_array().map(Vec::len), Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A FIFO where an entry or its metadata belongs is skipped rather than read (that blocked
    /// start-up forever), and a folder full of entries reopens only [`MAX_REOPEN`] per start.
    #[cfg(unix)]
    #[test]
    fn hostile_recovery_folders_dont_stall_start_up() {
        let dir = temp("recovery-hostile");
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        save(&s, &dir).unwrap();
        let (good, _) = files(&dir, s.doc().unwrap().uid);
        for i in 1..=(MAX_REOPEN as u64 + 3) {
            std::fs::copy(&good, dir.join(format!("{}.deckcraft", 100_000 + i))).unwrap();
        }
        for name in ["5.deckcraft", "6.json"] {
            let _ = std::process::Command::new("mkfifo").arg(dir.join(name)).status();
        }
        std::fs::write(dir.join("6.deckcraft"), std::fs::read(&good).unwrap()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let d = dir.clone();
        std::thread::spawn(move || {
            let mut s2 = Session::new();
            let _ = tx.send(open(&mut s2, &d).map(|v| v.len()).unwrap_or(0));
        });
        let n = rx.recv_timeout(std::time::Duration::from_secs(60)).expect("recovery hung");
        assert_eq!(n, MAX_REOPEN);
        // Every entry stays on disk (main keeps snapshots until a newer save); the 32 opened plus
        // the rest that wait for the next start.
        assert_eq!(list(&dir).len(), MAX_REOPEN + 5, "the rest wait for the next start");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
