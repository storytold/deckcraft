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

/// Create the folder `path` (and its parents) inside the automation write root; one that exists is
/// left as it is. A file of that name is an error.
#[cfg(not(target_arch = "wasm32"))]
pub fn create_folder(path: &str) -> Result<(), String> {
    crate::file_access::check_write(path)?;
    let dir = std::path::Path::new(path);
    if dir.exists() && !dir.is_dir() {
        return Err(format!("{path} is not a folder"));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{path}: cannot create the folder: {e}"))
}

#[cfg(target_arch = "wasm32")]
pub fn create_folder(path: &str) -> Result<(), String> {
    Err(format!("{path}: saving to a path is not available in the browser"))
}

/// Remove the plain files in folder `path` whose names `matches` says are ours, inside the
/// automation write root (each one is checked). A folder that isn't there has nothing to remove.
/// Answers how many were removed.
#[cfg(not(target_arch = "wasm32"))]
pub fn remove_files(path: &str, matches: &dyn Fn(&str) -> bool) -> Result<usize, String> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(format!("{path}: {err}")),
    };
    let mut removed = 0;
    for e in entries {
        // An entry (or its type) that can't be read is an error, not a file skipped, so no earlier
        // frame stays.
        let e = e.map_err(|err| format!("{path}: {err}"))?;
        let name = e.file_name();
        if matches(&name.to_string_lossy()) && e.file_type().map_err(|err| format!("{}: {err}", e.path().to_string_lossy()))?.is_file() {
            let file = e.path();
            let file = file.to_string_lossy();
            crate::file_access::check_write(&file)?;
            std::fs::remove_file(&*file).map_err(|err| format!("{file}: {err}"))?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(target_arch = "wasm32")]
pub fn remove_files(_path: &str, _matches: &dyn Fn(&str) -> bool) -> Result<usize, String> {
    Ok(0)
}

/// How many regular files directly in folder `path` have names `matches` says are ours. A folder
/// that isn't there has none. Listing is checked as a write, like the folder's other operations:
/// the caller is about to replace what it counts, so a write-only root must be enough.
#[cfg(not(target_arch = "wasm32"))]
pub fn count_files(path: &str, matches: &dyn Fn(&str) -> bool) -> Result<usize, String> {
    crate::file_access::check_write(path)?;
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(format!("{path}: {err}")),
    };
    let mut n = 0;
    for e in entries {
        let e = e.map_err(|err| format!("{path}: {err}"))?;
        if matches(&e.file_name().to_string_lossy()) && e.file_type().map_err(|err| format!("{}: {err}", e.path().to_string_lossy()))?.is_file() {
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(target_arch = "wasm32")]
pub fn count_files(_path: &str, _matches: &dyn Fn(&str) -> bool) -> Result<usize, String> {
    Ok(0)
}

/// The first entry directly in folder `path` whose name `matches` but which is not a regular file
/// (a folder, a link, …), if any: [`remove_files`] and [`move_files`] leave such an entry alone, so
/// a caller replacing those files checks for one before it removes anything. An entry outside the
/// automation write root (a link leading out) is refused as such. A folder that is not there has
/// none.
#[cfg(not(target_arch = "wasm32"))]
pub fn non_file_named(path: &str, matches: &dyn Fn(&str) -> bool) -> Result<Option<String>, String> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("{path}: {err}")),
    };
    for e in entries {
        let e = e.map_err(|err| format!("{path}: {err}"))?;
        let name = e.file_name();
        if matches(&name.to_string_lossy()) && !e.file_type().map_err(|err| format!("{path}: {err}"))?.is_file() {
            let entry = e.path().to_string_lossy().to_string();
            crate::file_access::check_write(&entry)?;
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

#[cfg(target_arch = "wasm32")]
pub fn non_file_named(_path: &str, _matches: &dyn Fn(&str) -> bool) -> Result<Option<String>, String> {
    Ok(None)
}

/// Move the files directly in folder `from` whose names `matches` into folder `to`, keeping their
/// names; both folders must be inside the automation write root and on one file system, as a
/// staging folder inside `to` is. Returns how many moved.
#[cfg(not(target_arch = "wasm32"))]
pub fn move_files(from: &str, to: &str, matches: &dyn Fn(&str) -> bool) -> Result<usize, String> {
    let entries = std::fs::read_dir(from).map_err(|err| format!("{from}: {err}"))?;
    let mut moved = 0;
    for e in entries {
        // An entry (or its type) that can't be read is an error, not a file skipped, so no frame
        // is left behind.
        let e = e.map_err(|err| format!("{from}: {err}"))?;
        let name = e.file_name();
        if matches(&name.to_string_lossy()) && e.file_type().map_err(|err| format!("{}: {err}", e.path().to_string_lossy()))?.is_file() {
            let (src, dst) = (e.path(), std::path::Path::new(to).join(&name));
            let (src, dst) = (src.to_string_lossy(), dst.to_string_lossy());
            crate::file_access::check_write(&src)?;
            crate::file_access::check_write(&dst)?;
            std::fs::rename(&*src, &*dst).map_err(|err| format!("{dst}: {err}"))?;
            moved += 1;
        }
    }
    Ok(moved)
}

#[cfg(target_arch = "wasm32")]
pub fn move_files(_from: &str, _to: &str, _matches: &dyn Fn(&str) -> bool) -> Result<usize, String> {
    Ok(0)
}

/// Remove the folder at `path` if it is there and empty. A folder that is gone, or still holds
/// something, is left as it is and counts as done: this is cleanup, not part of the result.
#[cfg(not(target_arch = "wasm32"))]
pub fn remove_empty_folder(path: &str) -> Result<(), String> {
    crate::file_access::check_write(path)?;
    match std::fs::remove_dir(path) {
        Err(err) if !matches!(err.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty) => Err(format!("{path}: {err}")),
        _ => Ok(()),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn remove_empty_folder(_path: &str) -> Result<(), String> {
    Ok(())
}

/// An advisory lock on a folder, held until dropped (or the process ends, however it ends).
pub struct FolderLock(
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    std::fs::File,
);

/// Take the lock `name` in `folder`, or fail at once if another export (this process or another)
/// holds it. The lock file stays behind: removing it while others wait could hand the lock to two.
#[cfg(not(target_arch = "wasm32"))]
pub fn lock_folder(folder: &str, name: &str) -> Result<FolderLock, String> {
    let path = std::path::Path::new(folder).join(name).to_string_lossy().to_string();
    crate::file_access::check_write(&path)?;
    if is_link(&path) {
        return Err(format!("{path} is a link: delete it and export again"));
    }
    let file = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(&path).map_err(|e| format!("{path}: {e}"))?;
    match file.try_lock() {
        Ok(()) => Ok(FolderLock(file)),
        Err(std::fs::TryLockError::WouldBlock) => {
            Err(format!("another export to this folder is running ({path} is locked): wait for it and export again; nothing was written"))
        }
        Err(std::fs::TryLockError::Error(e)) => Err(format!("{path}: {e}")),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn lock_folder(_folder: &str, _name: &str) -> Result<FolderLock, String> {
    Ok(FolderLock())
}

/// Is `path` itself a symbolic link (not what it points at)?
#[cfg(not(target_arch = "wasm32"))]
pub fn is_link(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

#[cfg(target_arch = "wasm32")]
pub fn is_link(_path: &str) -> bool {
    false
}

/// Write `bytes` straight to `path` inside the automation write root: no temporary sibling and no
/// sync, for the many generated files of a frame sequence, which are rebuilt rather than precious.
/// The file is created new and an existing name (a link included) is an error, so nothing already
/// there is followed or overwritten. The folder must exist ([`create_folder`]).
#[cfg(not(target_arch = "wasm32"))]
pub fn write_file_direct(path: &str, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    crate::file_access::check_write(path)?;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| format!("{path}: {e}"))?;
    file.write_all(bytes).map_err(|e| format!("{path}: {e}"))
}

#[cfg(target_arch = "wasm32")]
pub fn write_file_direct(path: &str, _bytes: &[u8]) -> Result<(), String> {
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

    /// A missing folder has nothing to remove; a folder that can't be read is an error, not
    /// zero files, so old frames are not left in place silently.
    #[test]
    fn remove_files_reports_a_folder_it_cannot_read() {
        let missing = std::env::temp_dir().join(format!("deckcraft_io_missing_{}", std::process::id()));
        assert_eq!(remove_files(missing.to_str().unwrap(), &|_| true), Ok(0));
        let file = std::env::temp_dir().join(format!("deckcraft_io_not_a_folder_{}", std::process::id()));
        std::fs::write(&file, b"x").unwrap();
        let res = remove_files(file.to_str().unwrap(), &|_| true);
        let _ = std::fs::remove_file(&file);
        assert!(res.is_err(), "{res:?}");
    }
}
