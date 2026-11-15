//! Reading and writing the files a command names by path.
//!
//! Every such read and write goes through [`read_file`] and [`write_file`], so the automation
//! roots ([`crate::file_access`]) are checked in one place. A command module that called
//! `std::fs` itself would bypass them; a test in `file_access` keeps that from happening.

/// The bytes of the file at `path`, inside the automation read root when there is one.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_file(path: &str) -> Result<Vec<u8>, String> {
    crate::file_access::check_read(path)?;
    let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{path}: not a regular file"));
    }
    const MAX_FILE_SIZE: u64 = 2 << 30; // 2 GiB
    if meta.len() > MAX_FILE_SIZE {
        return Err(format!("{path}: file size {} exceeds 2 GiB limit", meta.len()));
    }
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

#[cfg(target_arch = "wasm32")]
pub fn read_file(path: &str) -> Result<Vec<u8>, String> {
    Err(format!("{path}: files are opened through the browser on the web"))
}

/// Write `bytes` to the file at `path`, inside the automation write root when there is one.
///
/// The bytes go to a temporary sibling first and are renamed over `path`, so a failed save never
/// truncates the old file. The sibling is in the same folder, so it is inside the root whenever
/// `path` is.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_file(path: &str, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    crate::file_access::check_write(path)?;
    let err = |e: std::io::Error| format!("{path}: {e}");

    let target_path = match std::fs::canonicalize(path) {
        Ok(canonical) => canonical,
        Err(_) => std::path::PathBuf::from(path),
    };
    let existing_perms = std::fs::metadata(&target_path).ok().map(|m| m.permissions());

    let mut attempts = 0;
    let (tmp, mut file) = loop {
        // A short fixed-size name: appending to the target's name could exceed the file name limit.
        let tmp_name = format!(".deckcraft.{}.{}.saving", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
        let tmp = target_path.with_file_name(tmp_name);
        // The sibling must pass the same check: when `path` is the write root itself it lands outside.
        crate::file_access::check_write(&tmp.to_string_lossy())?;
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempts < 16 => attempts += 1,
            Err(e) => return Err(err(e)),
        }
    };
    if let Some(perms) = existing_perms {
        let _ = file.set_permissions(perms);
    }
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    let result = written.and_then(|()| std::fs::rename(&tmp, &target_path));
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(err(e));
    }
    Ok(())
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

/// What a folder entry is. A link is reported as a link and never followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Folder,
    Link,
    Other,
}

/// A file's size and when it last changed: a cached reading of the file is current while these
/// are the same.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp {
    pub len: u64,
    pub modified: Option<std::time::SystemTime>,
}

/// One entry of a folder ([`read_dir`]).
#[derive(Clone, Debug)]
pub struct DirEntry {
    pub path: std::path::PathBuf,
    /// The file name; `None` when it isn't UTF-8 (a path a command could not name back).
    pub name: Option<String>,
    pub kind: EntryKind,
    /// For a file; the default for anything else.
    pub stamp: Stamp,
}

#[cfg(not(target_arch = "wasm32"))]
fn stamp_of(meta: &std::fs::Metadata) -> Stamp {
    Stamp { len: meta.len(), modified: meta.modified().ok() }
}

#[cfg(not(target_arch = "wasm32"))]
fn kind_of(t: std::fs::FileType) -> EntryKind {
    if t.is_symlink() {
        EntryKind::Link
    } else if t.is_dir() {
        EntryKind::Folder
    } else if t.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

/// At most `max` entries of the folder at `path` (inside the automation read root when there is
/// one), and whether it holds more. Links are not followed: each is reported as a link.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_dir(path: &std::path::Path, max: usize) -> Result<(Vec<DirEntry>, bool), String> {
    crate::file_access::check_read_path(path)?;
    let shown = path.display();
    let mut out = vec![];
    for entry in std::fs::read_dir(path).map_err(|e| format!("{shown}: {e}"))? {
        if out.len() >= max {
            return Ok((out, true));
        }
        // An entry that vanished or can't be examined is left out.
        let Ok(entry) = entry else { continue };
        let Ok(t) = entry.file_type() else { continue };
        let kind = kind_of(t);
        let stamp = if kind == EntryKind::File { entry.metadata().map(|m| stamp_of(&m)).unwrap_or_default() } else { Stamp::default() };
        out.push(DirEntry { path: entry.path(), name: entry.file_name().to_str().map(String::from), kind, stamp });
    }
    Ok((out, false))
}

#[cfg(target_arch = "wasm32")]
pub fn read_dir(path: &std::path::Path, _max: usize) -> Result<(Vec<DirEntry>, bool), String> {
    Err(format!("{}: folders on this computer are not available in the browser build", path.display()))
}

/// What is at `path` (a link is reported as a link) and, for a file, its [`Stamp`].
#[cfg(not(target_arch = "wasm32"))]
pub fn stat(path: &std::path::Path) -> Result<(EntryKind, Stamp), String> {
    crate::file_access::check_read_path(path)?;
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((kind_of(meta.file_type()), stamp_of(&meta)))
}

#[cfg(target_arch = "wasm32")]
pub fn stat(path: &std::path::Path) -> Result<(EntryKind, Stamp), String> {
    Err(format!("{}: files on this computer are not available in the browser build", path.display()))
}

/// `path` made absolute with its links followed, when it may be read.
#[cfg(not(target_arch = "wasm32"))]
pub fn canonical(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    crate::file_access::check_read_path(path)?;
    std::fs::canonicalize(path).map(crate::file_access::plain).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(target_arch = "wasm32")]
pub fn canonical(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    Err(format!("{}: files on this computer are not available in the browser build", path.display()))
}

/// The bytes of the regular file at `path`, inside the automation read root when there is one,
/// and no more than `max` of them: a larger file is an error, also one that grew after it was
/// measured.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_path(path: &std::path::Path, max: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    crate::file_access::check_read_path(path)?;
    let shown = path.display();
    let meta = std::fs::metadata(path).map_err(|e| format!("{shown}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{shown}: not a regular file"));
    }
    if meta.len() > max {
        return Err(format!("{shown}: file size {} exceeds the {max}-byte limit", meta.len()));
    }
    let file = std::fs::File::open(path).map_err(|e| format!("{shown}: {e}"))?;
    let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    file.take(max.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| format!("{shown}: {e}"))?;
    if bytes.len() as u64 > max {
        return Err(format!("{shown}: the file grew past the {max}-byte limit while it was read"));
    }
    Ok(bytes)
}

#[cfg(target_arch = "wasm32")]
pub fn read_path(path: &std::path::Path, _max: u64) -> Result<Vec<u8>, String> {
    Err(format!("{}: files on this computer are not available in the browser build", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_file_rejects_directory() {
        let dir = std::env::temp_dir();
        let res = read_file(dir.to_str().unwrap());
        assert!(res.is_err());
    }

    #[test]
    fn write_file_preserves_permissions_and_writes_content() {
        let tmp_dir = std::env::temp_dir().join("deckcraft_io_test");
        let _ = std::fs::create_dir_all(&tmp_dir);
        let test_file = tmp_dir.join("test_write.txt");
        let path_str = test_file.to_str().unwrap();

        assert!(write_file(path_str, b"hello world").is_ok());
        let read = read_file(path_str).unwrap();
        assert_eq!(read, b"hello world");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o640);
            std::fs::set_permissions(&test_file, perms).unwrap();

            assert!(write_file(path_str, b"updated world").is_ok());
            let new_meta = std::fs::metadata(&test_file).unwrap();
            assert_eq!(new_meta.permissions().mode() & 0o777, 0o640);
        }

        let _ = std::fs::remove_file(&test_file);
        let _ = std::fs::remove_dir(&tmp_dir);
    }

    #[test]
    fn write_file_overwrites_file_with_name_near_the_length_limit() {
        let dir = std::env::temp_dir().join("deckcraft_io_long_name_test");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join(format!("{}.pptx", "a".repeat(250)));
        std::fs::write(&file, b"old").unwrap();
        let path_str = file.to_str().unwrap();

        write_file(path_str, b"new").unwrap();
        assert_eq!(read_file(path_str).unwrap(), b"new");

        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn write_file_to_the_write_root_itself_creates_no_sibling_outside_it() {
        let base = std::env::temp_dir().join(format!("deckcraft_io_root_test_{}", std::process::id()));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let roots = crate::file_access::AutomationRoots::new(&[], std::slice::from_ref(&root)).unwrap().unwrap();
        let res = crate::file_access::confine(Some(&roots), || write_file(root.to_str().unwrap(), b"x"));
        assert!(res.unwrap_err().contains("outside the write root"));
        assert_eq!(std::fs::read_dir(&base).unwrap().count(), 1, "only the root is there");
        let _ = std::fs::remove_dir_all(&base);
    }
}
