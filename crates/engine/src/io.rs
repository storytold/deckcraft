//! Reading and writing the files a command names by path.
//!
//! Every such read and write goes through [`read_file`] and [`write_file`], so the automation
//! roots ([`crate::file_access`]) are checked in one place. A command module that called
//! `std::fs` itself would bypass them; a test in `file_access` keeps that from happening.

/// The bytes of the file at `path`, inside the automation read root when there is one.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_file(path: &str) -> Result<Vec<u8>, String> {
    crate::file_access::check_read(path)?;
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

#[cfg(target_arch = "wasm32")]
pub fn read_file(path: &str) -> Result<Vec<u8>, String> {
    Err(format!("{path}: files are opened through the browser on the web"))
}

/// Write `bytes` to the file at `path`, inside the automation write root when there is one.
///
/// The bytes go to a temporary sibling first and are renamed over `path` (see [`write_atomic`]),
/// so a failed save never truncates the old file. The sibling is in the same folder, so it is
/// inside the root whenever `path` is.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_file(path: &str, bytes: &[u8]) -> Result<(), String> {
    crate::file_access::check_write(path)?;
    write_atomic(std::path::Path::new(path), bytes).map_err(|e| e.to_string())
}

/// Why a save failed, worded for the status bar.
#[cfg(not(target_arch = "wasm32"))]
fn save_err(path: &std::path::Path, what: impl std::fmt::Display) -> crate::EngineError {
    crate::EngineError::Other(format!("{}: {what}", path.display()))
}

/// The file a save to `path` should replace: the target of a symbolic link (even one whose
/// target doesn't exist yet), otherwise `path` itself.
#[cfg(not(target_arch = "wasm32"))]
fn resolve_link(path: &std::path::Path) -> std::path::PathBuf {
    let mut p = path.to_path_buf();
    // A chain of links, but not a loop.
    for _ in 0..16 {
        let Ok(meta) = std::fs::symlink_metadata(&p) else { break };
        if !meta.file_type().is_symlink() {
            break;
        }
        let Ok(target) = std::fs::read_link(&p) else { break };
        p = if target.is_absolute() { target } else { p.parent().map(|d| d.join(&target)).unwrap_or(target) };
    }
    p
}

/// Write `bytes` to `path` without risking the copy already on disk.
///
/// A uniquely named temporary sibling, created exclusively (never following or reusing an
/// existing file or symlink), is written, flushed to the disk, then renamed over the target, and
/// the folder is synced: a crash, power cut, full disk or failed write leaves either the old file
/// or the new one, never a torn or empty one, and concurrent saves never share temporary bytes.
/// It also keeps what the user set up around the file: a read-only file is refused (as
/// PowerPoint does) rather than silently replaced, a symbolic link keeps pointing at the file it
/// named, and the file keeps its permissions.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> crate::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let target = resolve_link(path);
    let dir = match target.parent() {
        Some(d) if d.as_os_str().is_empty() => std::path::Path::new("."),
        Some(d) => d,
        None => return Err(save_err(path, "not a file path")),
    };
    if !dir.is_dir() {
        return Err(save_err(path, "the folder no longer exists"));
    }
    let existing = std::fs::metadata(&target).ok();
    if let Some(m) = &existing {
        if !m.is_file() {
            return Err(save_err(path, "not a file"));
        }
        if m.permissions().readonly() {
            return Err(save_err(path, "the file is read-only; save a copy under another name"));
        }
    }
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut attempts = 0;
    let (tmp, mut file) = loop {
        let tmp = dir.join(format!("{name}.{}.{}.saving", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempts < 16 => attempts += 1,
            Err(e) => return Err(save_err(path, describe_io(&e))),
        }
    };
    let result = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        if let Some(m) = &existing {
            // The new file takes the old one's permissions (a private file stays private).
            file.set_permissions(m.permissions())?;
        }
        // On the disk before the rename, so a power cut can't leave the new name on empty data.
        file.sync_all()
    })();
    drop(file);
    if let Err(e) = result.and_then(|()| std::fs::rename(&tmp, &target)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(save_err(path, describe_io(&e)));
    }
    // The rename itself on the disk (best effort: not every system can sync a folder).
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn describe_io(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::StorageFull => "the disk is full; the file on disk was left as it was".into(),
        std::io::ErrorKind::PermissionDenied => "permission denied; the file on disk was left as it was".into(),
        _ => format!("{e}; the file on disk was left as it was"),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn write_file(path: &str, _bytes: &[u8]) -> Result<(), String> {
    Err(format!("{path}: saving to a path is not available in the browser"))
}

/// The size in bytes of the file at `path`; `None` when it can't be read (or is outside the
/// automation read root, so File › Info doesn't reveal files the agent may not read).
#[cfg(not(target_arch = "wasm32"))]
pub fn file_len(path: &str) -> Option<u64> {
    crate::file_access::check_read(path).ok()?;
    std::fs::metadata(path).ok().map(|m| m.len())
}

#[cfg(target_arch = "wasm32")]
pub fn file_len(_path: &str) -> Option<u64> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("deckcraft-fsx-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn leftovers(d: &Path) -> Vec<String> {
        std::fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".saving")).collect()
    }

    #[test]
    fn replaces_the_file_keeping_its_permissions() {
        let d = dir("perm");
        let f = d.join("a.deckcraft");
        std::fs::write(&f, b"old").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic(&f, b"new").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"new");
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(leftovers(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_read_only_file_is_refused_and_left_alone() {
        let d = dir("ro");
        let f = d.join("a.deckcraft");
        std::fs::write(&f, b"old").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o444)).unwrap();
        let e = write_atomic(&f, b"new").unwrap_err().to_string();
        assert!(e.contains("read-only"), "{e}");
        assert_eq!(std::fs::read(&f).unwrap(), b"old");
        assert!(leftovers(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_link_keeps_pointing_at_its_file() {
        let d = dir("link");
        std::fs::create_dir(d.join("real")).unwrap();
        std::fs::write(d.join("real/a.deckcraft"), b"old").unwrap();
        std::os::unix::fs::symlink("real/a.deckcraft", d.join("link.deckcraft")).unwrap();
        write_atomic(&d.join("link.deckcraft"), b"new").unwrap();
        assert!(std::fs::symlink_metadata(d.join("link.deckcraft")).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(d.join("real/a.deckcraft")).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn failures_leave_the_old_file_and_no_temporary() {
        let d = dir("fail");
        // The folder is gone.
        let e = write_atomic(&d.join("gone/a.deckcraft"), b"x").unwrap_err().to_string();
        assert!(e.contains("no longer exists"), "{e}");
        // A folder where the file should be.
        std::fs::create_dir(d.join("b.deckcraft")).unwrap();
        assert!(write_atomic(&d.join("b.deckcraft"), b"x").is_err());
        // A folder that can't be written to (root ignores permissions; skip the check then).
        std::fs::create_dir(d.join("locked")).unwrap();
        std::fs::write(d.join("locked/c.deckcraft"), b"old").unwrap();
        std::fs::set_permissions(d.join("locked"), std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::File::create(d.join("locked/probe")).is_err() {
            let e = write_atomic(&d.join("locked/c.deckcraft"), b"new").unwrap_err().to_string();
            assert!(e.contains("permission denied"), "{e}");
            assert_eq!(std::fs::read(d.join("locked/c.deckcraft")).unwrap(), b"old");
        }
        std::fs::set_permissions(d.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(leftovers(&d).is_empty() && leftovers(&d.join("locked")).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Through File › Save As: a failed save reports why and leaves the presentation unsaved
    /// (still dirty, AutoRecover entry kept), so nothing is lost.
    #[test]
    fn a_failed_save_keeps_the_presentation_unsaved() {
        let d = dir("session");
        let mut s = crate::Session::with_new();
        s.recovery_dir = Some(d.join("recovery"));
        s.execute("slide.new", &serde_json::json!({})).unwrap();
        assert_eq!(crate::recovery::save(&s, &d.join("recovery")).unwrap(), 1);
        let ro = d.join("ro.deckcraft");
        std::fs::write(&ro, b"old").unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o444)).unwrap();
        for path in [ro.clone(), d.join("gone/x.deckcraft")] {
            let e = s.execute("file.saveAs", &serde_json::json!({"path": path.to_string_lossy()})).unwrap_err().to_string();
            assert!(e.contains("read-only") || e.contains("no longer exists"), "{e}");
            assert!(s.doc().unwrap().is_dirty());
            assert_eq!(crate::recovery::list(&d.join("recovery")).len(), 1, "the AutoRecover copy stays");
        }
        assert_eq!(std::fs::read(&ro).unwrap(), b"old");
        let _ = std::fs::remove_dir_all(&d);
    }
}
