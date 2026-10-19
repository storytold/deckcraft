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
    let target_str = target_path.to_str().unwrap_or(path);
    let existing_perms = std::fs::metadata(&target_path).ok().map(|m| m.permissions());

    let mut attempts = 0;
    let (tmp, mut file) = loop {
        let tmp = format!("{target_str}.{}.{}.saving", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
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
    for e in entries.flatten() {
        let name = e.file_name();
        if matches(&name.to_string_lossy()) && e.file_type().is_ok_and(|t| t.is_file()) {
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

/// Write `bytes` straight to `path` inside the automation write root: no temporary sibling and no
/// sync, for the many generated files of a frame sequence, which are rebuilt rather than precious.
/// The folder must exist ([`create_folder`]).
#[cfg(not(target_arch = "wasm32"))]
pub fn write_file_direct(path: &str, bytes: &[u8]) -> Result<(), String> {
    crate::file_access::check_write(path)?;
    std::fs::write(path, bytes).map_err(|e| format!("{path}: {e}"))
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
