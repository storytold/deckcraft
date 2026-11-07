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
    manifest.validate()?;
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

fn fetch(file: &ModelFile, dir: &Path, total: u64, done: &mut u64, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    let name = file.safe_name().ok_or_else(|| format!("the manifest names an unsafe file: {}", file.name))?;
    let part = dir.join(format!("{name}.part"));
    let dest = dir.join(name);
    let clean_up = |why: String| -> String {
        let _ = std::fs::remove_file(&part);
        why
    };
    let mut response = ureq::get(&file.url).call().map_err(|e| clean_up(format!("{name}: {e}")))?;
    if !response.status().is_success() {
        return Err(clean_up(format!("{name}: the server answered {}", response.status())));
    }
    let mut out = std::fs::File::create(&part).map_err(|e| clean_up(format!("{name}: {e}")))?;
    let mut reader = response.body_mut().as_reader();
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
    let (len, hash) = model::sha256_file(&part).map_err(|e| clean_up(format!("{name}: {e}")))?;
    if len != file.bytes || !hash.eq_ignore_ascii_case(&file.sha256) {
        return Err(clean_up(format!("{name}: the SHA-256 does not match the manifest")));
    }
    std::fs::rename(&part, &dest).map_err(|e| clean_up(format!("{name}: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use deckcraft_asr::model::sha256_hex;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("deckcraft-model-download-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_manifest_that_is_already_installed_downloads_nothing() {
        let d = temp("installed");
        let body = b"weights";
        let manifest = ModelManifest {
            id: "test-model".into(),
            name: "Test".into(),
            repo: "owner/test".into(),
            revision: "0".into(),
            license: "MIT".into(),
            runtime: "candle-cpu".into(),
            license_url: "https://example.invalid".into(),
            files: vec![ModelFile {
                name: "model.safetensors".into(),
                bytes: body.len() as u64,
                sha256: sha256_hex(body),
                url: "http://127.0.0.1:1/never".into(),
            }],
        };
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
        let manifest = ModelManifest {
            id: "test-model".into(),
            name: "Test".into(),
            repo: "owner/test".into(),
            revision: "0".into(),
            license: "MIT".into(),
            runtime: "candle-cpu".into(),
            license_url: "https://example.invalid".into(),
            files: vec![ModelFile { name: "../escape".into(), bytes: 1, sha256: sha256_hex(b"x"), url: "http://127.0.0.1:1/x".into() }],
        };
        let err = download(&manifest, &d, &mut |_, _| {}).unwrap_err();
        assert!(err.contains("unsafe"), "{err}");
    }
}
