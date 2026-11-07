//! Discovery, verification, install, download and the RTen segmenter itself.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use deckcraft_matte::frame::RgbaFrame;
use deckcraft_matte::manifest;
use deckcraft_matte::process::{postprocess, preprocess};
use deckcraft_matte::service::ModelState;
use deckcraft_matte::{Matte, MatteError, Segmenter};

/// Model roots, first hit wins, in the order the shared model policy documents.
pub fn model_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(dir) = std::env::var_os("DECKCRAFT_MODELS").filter(|v| !v.is_empty()) {
        roots.push(PathBuf::from(dir));
    }
    if let Some(dir) = config_dir() {
        roots.push(dir.join("models"));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        roots.push(dir.join("models"));
    }
    roots.push(PathBuf::from("models"));
    roots
}

/// The per-user settings directory the desktop app uses (`APPDATA/DeckCraft`,
/// `~/Library/Application Support/DeckCraft`, `$XDG_CONFIG_HOME/deckcraft`).
fn config_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/DeckCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("DeckCraft"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|c| c.join("deckcraft"))
    }
}

/// The first root whose model file exists at the expected size, or the missing state naming the
/// first root to install into.
pub fn probe() -> (PathBuf, ModelState) {
    let roots = model_roots();
    let root = roots.first().cloned().unwrap_or_else(|| PathBuf::from("models"));
    for candidate in &roots {
        let path = manifest::MANIFEST.path_in(candidate);
        if !path.is_file() {
            continue;
        }
        match manifest::verify_file(&path) {
            Ok(()) => return (candidate.clone(), ModelState::Ready { path: path.display().to_string(), bytes: manifest::MODEL_BYTES }),
            Err(e) => {
                return (candidate.clone(), ModelState::Failed { detail: format!("{}: {e}", path.display()) });
            }
        }
    }
    let state = missing_state(&root);
    (root, state)
}

fn missing_state(root: &Path) -> ModelState {
    let m = manifest::MANIFEST;
    ModelState::Missing {
        root: root.display().to_string(),
        url: m.url(),
        bytes: m.bytes,
        sha256: m.sha256.to_string(),
        license: m.license.to_string(),
    }
}

/// Copy a user-picked file into `<root>/birefnet-lite-512/onnx/model.onnx`, verifying it first.
/// The write goes to a temporary file in the same directory and is renamed only after the hash
/// matches, so a failure never leaves a half file that later looks installed.
pub fn install_from(source: &Path, root: &Path) -> Result<PathBuf, String> {
    if !source.is_file() {
        return Err(format!("{} is not a file", source.display()));
    }
    let size = std::fs::metadata(source).map_err(|e| format!("cannot read {}: {e}", source.display()))?.len();
    manifest::MANIFEST.verify(size, &manifest::sha256_file(source).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let target = manifest::MANIFEST.path_in(root);
    let dir = target.parent().ok_or_else(|| format!("{} has no parent", target.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let tmp = dir.join("model.onnx.part");
    std::fs::copy(source, &tmp).map_err(|e| format!("cannot copy to {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot install {}: {e}", target.display())
    })?;
    // Best effort: the provenance sidecar the shared policy names.
    write_manifest(root);
    Ok(target)
}

fn write_manifest(root: &Path) {
    let m = manifest::MANIFEST;
    let dir = root.join(m.id);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let sidecar = format!(
        "{{\n  \"id\": \"{}\",\n  \"repo\": \"{}\",\n  \"revision\": \"{}\",\n  \"file\": \"{}\",\n  \"bytes\": {},\n  \"sha256\": \"{}\",\n  \"license\": \"{}\"\n}}\n",
        m.id, m.repo, m.revision, m.file, m.bytes, m.sha256, m.license
    );
    let _ = std::fs::write(dir.join("manifest.json"), sidecar);
}

/// Download the model into `root` from the manifest's pinned URL, reporting progress. Blocking:
/// the caller runs it on a worker thread after the consent screen. A short download is rejected
/// by the size check and never renamed into place.
pub fn download_to(root: &Path, on_progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, String> {
    let m = manifest::MANIFEST;
    let target = m.path_in(root);
    let dir = target.parent().ok_or_else(|| format!("{} has no parent", target.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let tmp = dir.join("model.onnx.part");
    let mut response = ureq::Agent::new_with_defaults().get(&m.url()).call().map_err(|e| format!("download failed: {e}"))?;
    let total = response.body().content_length().unwrap_or(0);
    if total > 0 && total != m.bytes {
        return Err(format!("the server offered {total} bytes, the model is {} bytes", m.bytes));
    }
    let mut reader = response.body_mut().as_reader();
    let mut file = std::fs::File::create(&tmp).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut received = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("download failed: {e}"))?;
        if n == 0 {
            break;
        }
        received = received.saturating_add(n as u64);
        if received > m.bytes {
            let _ = std::fs::remove_file(&tmp);
            return Err("the download is larger than the model".into());
        }
        file.write_all(buf.get(..n).unwrap_or_default()).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        on_progress(received, if total == 0 { m.bytes } else { total });
    }
    drop(file);
    if let Err(e) = manifest::verify_file(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot install {}: {e}", target.display())
    })?;
    write_manifest(root);
    Ok(target)
}

/// The RTen implementation of [`Segmenter`].
pub struct RtenSegmenter {
    model: rten::Model,
    options: rten::RunOptions,
    feather: u32,
}

impl RtenSegmenter {
    /// Verify and load the model at `path`. The hash is checked again here even if a caller
    /// probed it earlier: the bytes are about to be interpreted, so they must match.
    pub fn load(path: &Path) -> Result<Self, String> {
        manifest::verify_file(path).map_err(|e| e.to_string())?;
        let model = rten::Model::load_file(path).map_err(|e| format!("cannot load {}: {e}", path.display()))?;
        // RTen sizes its pool to the machine's physical cores. A hand-made pool was measured much
        // slower (16.6 s/frame at 21 threads, 3.96 s with RTen's own), so only an explicit cap
        // creates one, for constrained machines and benchmarks.
        let options = match std::env::var("DECKCRAFT_MATTE_THREADS").as_deref() {
            Ok("default") | Err(_) => rten::RunOptions::default(),
            Ok(n) => {
                let threads = n.parse::<usize>().map_err(|_| "DECKCRAFT_MATTE_THREADS must be a number".to_string())?.clamp(1, 64);
                rten::RunOptions::default().with_thread_pool(Some(Arc::new(rten::ThreadPool::with_num_threads(threads))))
            }
        };
        Ok(RtenSegmenter { model, options, feather: 2 })
    }

    /// Load from the first model root that holds a verified install.
    pub fn load_installed() -> Result<Self, String> {
        let (root, state) = probe();
        match state {
            ModelState::Ready { path, .. } => RtenSegmenter::load(Path::new(&path)),
            other => Err(format!("{} (looked in {})", other.message(), root.display())),
        }
    }

    pub fn with_feather(mut self, feather: u32) -> Self {
        self.feather = feather;
        self
    }

    /// Run the model on one frame and return the raw 512x512 logits. Public for the real-model
    /// test and for measuring speed without compositing.
    pub fn logits(&self, frame: &RgbaFrame) -> Result<Vec<f32>, MatteError> {
        let size = manifest::INPUT_SIZE;
        let input = preprocess(frame, size)?;
        let tensor = rten_tensor::NdTensor::from_data([1, 3, size as usize, size as usize], input);
        let output = self.model.run_one(tensor.into(), Some(self.options.clone())).map_err(|e| MatteError::Inference(e.to_string()))?;
        let tensor: rten_tensor::NdTensor<f32, 4> =
            output.try_into().map_err(|_| MatteError::BadOutput(format!("expected an f32 [1, 1, {size}, {size}] tensor")))?;
        let data = tensor.into_data();
        let expected = size as usize * size as usize;
        if data.len() != expected {
            return Err(MatteError::BadOutput(format!("got {} values, expected {expected}", data.len())));
        }
        Ok(data)
    }
}

impl Segmenter for RtenSegmenter {
    fn matte(&mut self, frame: &RgbaFrame) -> Result<Matte, MatteError> {
        let logits = self.logits(frame)?;
        postprocess(&logits, manifest::INPUT_SIZE, frame.width(), frame.height(), self.feather)
    }

    fn name(&self) -> &'static str {
        "birefnet-lite-512"
    }
}

/// Timeout for a model load: it reads 192 MB and parses a graph. Not used by the download (which
/// is a separate, longer operation) but kept here so callers can bound the UI's "loading" state.
pub const LOAD_TIMEOUT: Duration = Duration::from_secs(60);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_are_absolute_enough_and_the_env_wins() {
        // The env var is read at call time; setting it in a test can race other tests in the same
        // binary, so only check the shape without mutating the environment.
        let roots = model_roots();
        assert!(!roots.is_empty());
        assert!(roots.last().is_some_and(|p| p.ends_with("models")), "{roots:?}");
    }

    #[test]
    fn install_rejects_a_file_that_is_not_the_model_and_leaves_nothing_behind() {
        let dir = std::env::temp_dir().join(format!("deckcraft-matte-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let bad = dir.join("bad.onnx");
        std::fs::write(&bad, b"not the model").unwrap_or_default();
        let err = install_from(&bad, &dir).expect_err("rejected");
        assert!(err.contains("bytes"), "{err}");
        let target = manifest::MANIFEST.path_in(&dir);
        assert!(!target.exists(), "no partial install");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_install_names_the_root_and_the_provenance() {
        let dir = std::env::temp_dir().join(format!("deckcraft-matte-missing-{}", std::process::id()));
        let state = missing_state(&dir);
        match state {
            ModelState::Missing { root, url, bytes, sha256, license } => {
                assert_eq!(root, dir.display().to_string());
                assert!(url.contains("962e67969ec6acec9c19fab76acee4f5e22e990c"), "{url}");
                assert_eq!(bytes, manifest::MODEL_BYTES);
                assert_eq!(sha256.len(), 64);
                assert_eq!(license, "MIT");
            }
            other => panic!("{other:?}"),
        }
    }
}
