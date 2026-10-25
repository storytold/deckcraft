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
}
