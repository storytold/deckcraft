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
    write_file_impl(path, bytes, false)
}

/// Commit a complete new file without replacing any existing directory entry, even if another
/// save creates it while these bytes are being written.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_file_new(path: &str, bytes: &[u8]) -> Result<(), String> {
    write_file_impl(path, bytes, true)
}

#[cfg(not(target_arch = "wasm32"))]
fn write_file_impl(path: &str, bytes: &[u8], create_new: bool) -> Result<(), String> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    crate::file_access::check_write(path)?;
    let err = |e: std::io::Error| format!("{path}: {e}");

    // A no-replace save must also refuse symlinks, not resolve and write their targets.
    let target_path =
        if create_new { std::path::PathBuf::from(path) } else { std::fs::canonicalize(path).unwrap_or_else(|_| std::path::PathBuf::from(path)) };
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
    let result = written.and_then(|()| {
        if create_new {
            // Linking the fully written sibling is an atomic no-replace operation. If the
            // filesystem cannot link, fail safely rather than falling back to replacement.
            std::fs::hard_link(&tmp, &target_path)?;
            let _ = std::fs::remove_file(&tmp);
            Ok(())
        } else {
            std::fs::rename(&tmp, &target_path)
        }
    });
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

#[cfg(target_arch = "wasm32")]
pub(crate) fn write_file_new(path: &str, bytes: &[u8]) -> Result<(), String> {
    write_file(path, bytes)
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn new_file_commit_allows_exactly_one_concurrent_writer() {
        struct Scratch(std::path::PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = Scratch(std::env::temp_dir().join(format!("deckcraft-new-file-{}-{stamp}", std::process::id())));
        std::fs::create_dir(&dir.0).unwrap();
        let path = dir.0.join("Quarterly.pptx");
        let barrier = std::sync::Barrier::new(2);
        let results = std::thread::scope(|scope| {
            let writers: Vec<_> = [b"first".as_slice(), b"second".as_slice()]
                .into_iter()
                .map(|bytes| {
                    let path = &path;
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        (write_file_new(path.to_str().unwrap(), bytes), bytes)
                    })
                })
                .collect();
            writers.into_iter().map(|writer| writer.join().unwrap()).collect::<Vec<_>>()
        });
        assert_eq!(results.iter().filter(|(result, _)| result.is_ok()).count(), 1);
        let winner = results.iter().find(|(result, _)| result.is_ok()).unwrap().1;
        assert_eq!(std::fs::read(&path).unwrap(), winner);
        assert!(write_file_new(path.to_str().unwrap(), b"third").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), winner);
        assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1, "temporary siblings cleaned up");
        write_file(path.to_str().unwrap(), b"confirmed replacement").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"confirmed replacement");
    }

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
