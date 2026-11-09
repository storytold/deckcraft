//! HTTPS download of a speech model, user-initiated from Subtitle Settings.
//!
//! The window is the consent screen: it shows the source, size, licence and hashes before the
//! first byte moves (the dialog reads them from [`deckcraft_asr::whisper_tiny`]). Nothing here
//! runs at start-up, on opening a file or when a pane opens; every file is written beside its
//! final name, checked against the manifest's size and SHA-256, and only then renamed, so an
//! interrupted download never leaves a file that looks installed.

use std::io::{Read, Write};
use std::path::Path;

use deckcraft_asr::model::{self, ModelFile, ModelManifest};

/// Download every missing file of `manifest` into `dir`, reporting `(done, total)` bytes as it
/// goes, and write the provenance sidecar.
pub fn download(manifest: &ModelManifest, dir: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    download_with(manifest, dir, progress, fetch)
}

/// The download loop with an injectable fetch, so every failure path (a short body, an oversized
/// body, a hash mismatch, a refused URL) is testable without a network.
fn download_with<F>(manifest: &ModelManifest, dir: &Path, progress: &mut dyn FnMut(u64, u64), fetch: F) -> Result<(), String>
where
    F: Fn(&ModelFile, &Path, u64, &mut u64, &mut dyn FnMut(u64, u64)) -> Result<(), String>,
{
    manifest.validate()?;
    if std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("{} is a symlink; refusing to install a model through it", dir.display()));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let total = manifest.total_bytes();
    let mut done = 0u64;
    for file in &manifest.files {
        if model::verify_file(dir, file).is_ok() {
            done = done.saturating_add(file.bytes).min(total);
            progress(done, total);
            continue;
        }
        fetch(file, dir, total, &mut done, progress)?;
    }
    model::write_sidecar(dir, manifest).map_err(|e| format!("cannot write the manifest sidecar: {e}"))?;
    progress(total, total);
    Ok(())
}

/// The HTTP agent for model downloads: a stalled host fails instead of blocking the download
/// thread forever, while the body budget stays generous for 151 MB on a slow link.
fn download_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(20)))
        .timeout_recv_response(Some(std::time::Duration::from_secs(60)))
        // Each read resets; this is the stall guard in the middle of a body.
        .timeout_per_read(Some(std::time::Duration::from_secs(60)))
        // The whole body, which is not reset by reads.
        .timeout_recv_body(Some(std::time::Duration::from_secs(60 * 60)))
        .build()
        .into()
}

fn fetch(file: &ModelFile, dir: &Path, total: u64, done: &mut u64, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    let name = file.safe_name().ok_or_else(|| format!("the manifest names an unsafe file: {}", file.name))?;
    // The manifest is user-writable: no request until the URL is HTTPS on the model hub.
    if !model::model_url_allowed(&file.url) {
        return Err(format!("{name}: the source is not HTTPS on the model hub"));
    }
    let part = dir.join(format!("{name}.{}.part", std::process::id()));
    let dest = dir.join(name);
    if std::fs::symlink_metadata(&dest).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("{name}: refusing to write through a symlink"));
    }
    let mut response = download_agent().get(&file.url).call().map_err(|e| format!("{name}: {e}"))?;
    if !response.status().is_success() {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{name}: the server answered {}", response.status()));
    }
    let reader = response.body_mut().as_reader();
    store(file, &part, &dest, reader, total, done, progress)
}

/// Write `reader` to `part`, verify it against `file`, then rename it over `dest`. Every failure
/// removes the temporary file, so a refused body never looks installed, and the temporary name
/// is created exclusively (`create_new`) so a planted file or symlink is not followed.
fn store(
    file: &ModelFile,
    part: &Path,
    dest: &Path,
    mut reader: impl Read,
    total: u64,
    done: &mut u64,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    let name = file.safe_name().ok_or_else(|| format!("the manifest names an unsafe file: {}", file.name))?;
    let clean_up = |why: String| -> String {
        let _ = std::fs::remove_file(part);
        why
    };
    // A stale temporary from an interrupted run is replaced, but a symlink there is removed as a
    // link (never followed) before the exclusive create.
    if std::fs::symlink_metadata(part).is_ok() {
        std::fs::remove_file(part).map_err(|e| clean_up(format!("{name}: {e}")))?;
    }
    let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(part).map_err(|e| clean_up(format!("{name}: {e}")))?;
    let mut buf = vec![0u8; 64 << 10];
    let mut written = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| clean_up(format!("{name}: {e}")))?;
        if n == 0 {
            break;
        }
        written = written.saturating_add(n as u64);
        if written > file.bytes {
            return Err(clean_up(format!("{name}: the file is larger than the manifest says")));
        }
        out.write_all(buf.get(..n).unwrap_or_default()).map_err(|e| clean_up(format!("{name}: {e}")))?;
        *done = done.saturating_add(n as u64).min(total);
        progress(*done, total);
    }
    out.flush().map_err(|e| clean_up(format!("{name}: {e}")))?;
    drop(out);
    if written != file.bytes {
        return Err(clean_up(format!("{name}: got {written} bytes, the manifest says {}", file.bytes)));
    }
    // Verify the bytes that were written, then re-check the destination: a symlink planted while
    // the body was being read is refused rather than followed by the rename.
    let (len, hash) = model::sha256_file(part).map_err(|e| clean_up(format!("{name}: {e}")))?;
    if len != file.bytes || !hash.eq_ignore_ascii_case(&file.sha256) {
        return Err(clean_up(format!("{name}: the SHA-256 does not match the manifest")));
    }
    if std::fs::symlink_metadata(dest).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(clean_up(format!("{name}: refusing to replace a symlink")));
    }
    std::fs::rename(part, dest).map_err(|e| clean_up(format!("{name}: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use deckcraft_asr::model::sha256_hex;

    fn temp(name: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("deckcraft-model-download-{}-{name}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn manifest_with(body: &[u8], url: &str) -> ModelManifest {
        ModelManifest {
            id: "test-model".into(),
            name: "Test".into(),
            repo: "owner/test".into(),
            revision: "0".into(),
            license: "MIT".into(),
            runtime: "candle-cpu".into(),
            license_url: "https://huggingface.co/owner/test".into(),
            files: vec![ModelFile { name: "model.safetensors".into(), bytes: body.len() as u64, sha256: sha256_hex(body), url: url.into() }],
        }
    }

    #[test]
    fn a_manifest_that_is_already_installed_downloads_nothing() {
        let d = temp("installed");
        let body = b"weights";
        let manifest = manifest_with(body, "https://huggingface.co/owner/test/model.safetensors");
        let model_dir = d.join(&manifest.id);
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("model.safetensors"), body).unwrap();
        let mut seen = Vec::new();
        download(&manifest, &model_dir, &mut |done, total| seen.push((done, total))).unwrap();
        assert_eq!(seen.last(), Some(&(body.len() as u64, body.len() as u64)));
        // The sidecar is written and the file is untouched.
        assert!(model_dir.join("manifest.json").is_file());
        assert_eq!(std::fs::read(model_dir.join("model.safetensors")).unwrap(), body);
    }

    #[test]
    fn a_damaged_manifest_is_refused_before_any_network_call() {
        let d = temp("damaged");
        let mut manifest = manifest_with(b"x", "https://huggingface.co/owner/test/x");
        manifest.files[0].name = "../escape".into();
        let err = download(&manifest, &d, &mut |_, _| {}).unwrap_err();
        assert!(err.contains("unsafe"), "{err}");
    }

    #[test]
    fn a_url_off_the_model_hub_is_refused_before_a_request() {
        let file = manifest_with(b"x", "https://huggingface.co/owner/test/x").files[0].clone();
        let mut done = 0;
        let err = fetch(&file, Path::new("."), 1, &mut done, &mut |_, _| {}).unwrap_err();
        assert!(err.contains("model hub"), "{err}");
        let mut plain = file.clone();
        plain.url = "http://huggingface.co/owner/test/x".into();
        let err = fetch(&plain, Path::new("."), 1, &mut done, &mut |_, _| {}).unwrap_err();
        assert!(err.contains("model hub"), "{err}");
    }

    fn store_body(name: &str, body: &[u8], server: &[u8]) -> Result<(), String> {
        let d = temp(name);
        let file = manifest_with(body, "https://huggingface.co/owner/test/model.safetensors").files[0].clone();
        let part = d.join(format!("model.safetensors.{}.part", std::process::id()));
        let dest = d.join("model.safetensors");
        let mut done = 0;
        let result = store(&file, &part, &dest, server, file.bytes, &mut done, &mut |_, _| {});
        if result.is_ok() {
            assert!(!part.exists(), "the temporary file is renamed away");
            assert_eq!(std::fs::read(&dest).unwrap(), body);
        } else {
            assert!(!part.exists(), "a failure removes the temporary file");
            assert!(!dest.exists(), "a refused body is not installed");
        }
        result
    }

    #[test]
    fn a_short_body_is_refused_and_cleaned_up() {
        let err = store_body("short", b"weights", b"weig").unwrap_err();
        assert!(err.contains("got 4 bytes"), "{err}");
    }

    #[test]
    fn an_oversized_body_is_refused_and_cleaned_up() {
        let err = store_body("oversize", b"weights", b"weights plus more").unwrap_err();
        assert!(err.contains("larger than the manifest says"), "{err}");
    }

    #[test]
    fn a_hash_mismatch_is_refused_and_cleaned_up() {
        let err = store_body("hash", b"weights", b"WEIGHTS").unwrap_err();
        assert!(err.contains("SHA-256"), "{err}");
    }

    #[test]
    fn a_complete_body_is_verified_and_installed() {
        store_body("complete", b"weights", b"weights").unwrap();
    }

    #[test]
    fn the_download_loop_reports_a_fetch_failure_and_writes_no_sidecar() {
        let d = temp("failure");
        let manifest = manifest_with(b"weights", "https://huggingface.co/owner/test/model.safetensors");
        let model_dir = d.join(&manifest.id);
        let err = download_with(&manifest, &model_dir, &mut |_, _| {}, |_, _, _, _, _| Err("the server answered 503".into())).unwrap_err();
        assert!(err.contains("503"), "{err}");
        assert!(!model_dir.join("manifest.json").exists(), "a failed download leaves no provenance");
    }

    #[test]
    fn the_download_loop_passes_the_transport_through() {
        let d = temp("injected");
        let manifest = manifest_with(b"weights", "https://huggingface.co/owner/test/model.safetensors");
        let model_dir = d.join(&manifest.id);
        let mut calls = 0;
        download_with(&manifest, &model_dir, &mut |_, _| {}, |file, dir, _, done, progress| {
            calls += 1;
            let part = dir.join(format!("{}.test.part", file.name));
            std::fs::write(&part, b"weights").unwrap();
            *done = file.bytes;
            progress(*done, file.bytes);
            std::fs::rename(part, dir.join(&file.name)).unwrap();
            Ok(())
        })
        .unwrap();
        assert_eq!(calls, 1);
        assert!(model_dir.join("manifest.json").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_destination_is_refused() {
        let d = temp("symlink");
        let file = manifest_with(b"weights", "https://huggingface.co/owner/test/model.safetensors").files[0].clone();
        std::fs::write(d.join("outside"), b"untouched").unwrap();
        std::os::unix::fs::symlink(d.join("outside"), d.join("model.safetensors")).unwrap();
        let part = d.join("part");
        let mut done = 0;
        let err = store(&file, &part, &d.join("model.safetensors"), &b"weights"[..], file.bytes, &mut done, &mut |_, _| {}).unwrap_err();
        assert!(err.contains("symlink"), "{err}");
        assert_eq!(std::fs::read(d.join("outside")).unwrap(), b"untouched", "the link target is untouched");
        assert!(!part.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_temporary_file_is_not_followed() {
        let d = temp("symlink-part");
        let file = manifest_with(b"weights", "https://huggingface.co/owner/test/model.safetensors").files[0].clone();
        std::fs::write(d.join("outside"), b"untouched").unwrap();
        let part = d.join("part");
        std::os::unix::fs::symlink(d.join("outside"), &part).unwrap();
        let mut done = 0;
        store(&file, &part, &d.join("model.safetensors"), &b"weights"[..], file.bytes, &mut done, &mut |_, _| {}).unwrap();
        assert_eq!(std::fs::read(d.join("outside")).unwrap(), b"untouched", "the planted link's target is untouched");
        assert_eq!(std::fs::read(d.join("model.safetensors")).unwrap(), b"weights");
    }
}
