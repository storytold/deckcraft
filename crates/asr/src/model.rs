//! The model manifest and model directories.
//!
//! Model weights are never committed or shipped: the repository holds what a model *is* (its
//! id, revision, files, byte sizes, SHA-256 hashes, licence and source URL), and the user either
//! downloads the files through the Subtitle Settings dialog (after a consent screen showing the
//! source, size, licence and hashes) or places them by hand in the documented directory.
//!
//! Layout, the same in every Craft app (see the shared model policy):
//!
//! ```text
//! <model root>/<model-id>/manifest.json   provenance sidecar written by us
//! <model root>/<model-id>/<files…>        the files the manifest lists
//! ```
//!
//! `<model root>` is resolved first hit wins: `$DECKCRAFT_MODELS`, the platform config directory
//! plus `models`, `models/` beside the executable, then `models/` in the source tree.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The environment variable that puts the model root first.
pub const ENV_MODELS: &str = "DECKCRAFT_MODELS";
/// The provenance sidecar's file name inside a model directory.
pub const SIDECAR: &str = "manifest.json";
/// The model live subtitles use.
pub const WHISPER_TINY_ID: &str = "whisper-tiny";
/// The revision of `openai/whisper-tiny` the hashes below belong to.
pub const WHISPER_TINY_REVISION: &str = "169d4a4341b33bc18d8881c4b69c2e104e1cc0af";
/// The largest file a manifest entry may claim (1 GiB): a hand-edited manifest must not ask for
/// an allocation or a download the disk cannot hold.
pub const MAX_FILE_BYTES: u64 = 1 << 30;
/// The hosts a model file or its licence page may come from. A manifest (or a `.deckcraft`
/// file that carries one) is user-writable, so a URL is checked before it is used: HTTPS only,
/// and only the model hub this feature documents.
pub const ALLOWED_MODEL_HOSTS: &[&str] = &["huggingface.co", "hf.co"];

/// Whether `url` is an `https://` URL on an allowlisted model host. Anything else (plain HTTP,
/// another host, a `file:`/`data:` scheme, a userinfo trick) is refused before a request or a
/// link is used.
pub fn model_url_allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    if rest.chars().any(|c| c.is_control() || c.is_whitespace() || c == '\\') {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    // `user@host` and `host:port`: only the actual host counts.
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host).trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    ALLOWED_MODEL_HOSTS.iter().any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
}

/// One file of a model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFile {
    /// A plain file name inside the model directory (never a path).
    pub name: String,
    pub bytes: u64,
    /// Lower-case hex SHA-256 of the file's contents.
    pub sha256: String,
    pub url: String,
}

impl ModelFile {
    /// The name when it is a single, safe path component; `None` for `..`, separators, an
    /// absolute path or an over-long name.
    pub fn safe_name(&self) -> Option<&str> {
        let name = self.name.as_str();
        let ok = !name.is_empty()
            && name.chars().count() <= 128
            && !name.starts_with('.')
            && !name.contains(['/', '\\', ':'])
            && !name.chars().any(|c| c.is_control());
        ok.then_some(name)
    }

    /// Whether the recorded hash is shaped like a SHA-256 (64 hex digits).
    pub fn has_valid_hash(&self) -> bool {
        self.sha256.len() == 64 && self.sha256.chars().all(|c| c.is_ascii_hexdigit())
    }
}

/// Everything needed to recognise and fetch a model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    pub id: String,
    pub name: String,
    /// The upstream repository (`owner/name`).
    pub repo: String,
    /// The exact revision the files and hashes belong to.
    pub revision: String,
    /// The model card's licence, as SPDX where one exists.
    pub license: String,
    /// What runs the model in this build, and on what: the accelerator seam's hint.
    pub runtime: String,
    /// Where the licence and model card can be read.
    pub license_url: String,
    pub files: Vec<ModelFile>,
}

impl ModelManifest {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes.min(MAX_FILE_BYTES)).fold(0, u64::saturating_add)
    }

    pub fn file(&self, name: &str) -> Option<&ModelFile> {
        self.files.iter().find(|f| f.name == name)
    }

    /// Every entry names a safe file, carries a plausible hash and size, and no name repeats.
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() || !self.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(format!("`{}` is not a model id", self.id));
        }
        for (i, f) in self.files.iter().enumerate() {
            if f.safe_name().is_none() {
                return Err(format!("unsafe model file name `{}`", f.name));
            }
            if !f.has_valid_hash() {
                return Err(format!("`{}` has no SHA-256", f.name));
            }
            if f.bytes == 0 || f.bytes > MAX_FILE_BYTES {
                return Err(format!("`{}` has an implausible size", f.name));
            }
            if !model_url_allowed(&f.url) {
                return Err(format!("`{}` has no HTTPS URL on an allowlisted host", f.name));
            }
            if self.files[..i].iter().any(|g| g.name.eq_ignore_ascii_case(&f.name)) {
                return Err(format!("`{}` is listed twice", f.name));
            }
        }
        if !model_url_allowed(&self.license_url) {
            return Err("the licence URL is not HTTPS on an allowlisted host".into());
        }
        Ok(())
    }
}

/// The manifest of the model live subtitles use: `openai/whisper-tiny`, Apache-2.0, multilingual,
/// 151 MB. The hashes were measured from the Hugging Face revision in
/// [`WHISPER_TINY_REVISION`]; see `docs/captions.md`.
pub fn whisper_tiny() -> ModelManifest {
    let base = format!("https://huggingface.co/openai/whisper-tiny/resolve/{WHISPER_TINY_REVISION}");
    let file =
        |name: &str, bytes: u64, sha256: &str| ModelFile { name: name.to_string(), bytes, sha256: sha256.to_string(), url: format!("{base}/{name}") };
    ModelManifest {
        id: WHISPER_TINY_ID.to_string(),
        name: "Whisper Tiny".to_string(),
        repo: "openai/whisper-tiny".to_string(),
        revision: WHISPER_TINY_REVISION.to_string(),
        license: "Apache-2.0".to_string(),
        runtime: "candle-cpu".to_string(),
        license_url: "https://huggingface.co/openai/whisper-tiny".to_string(),
        files: vec![
            file("model.safetensors", 151_061_672, "7ebd0e69e78190ffe1438491fa05cc1f5c1aa3a4c4db3bc1723adbb551ea2395"),
            file("config.json", 1_983, "ffdccec4f3211f4c63310f2b7098f309fe70f3952cedc5e4d11e43f5b2379b98"),
            file("tokenizer.json", 2_480_466, "27fc476bfe7f17299480be2273fc0608e4d5a99aba2ab5dec5374b4482d1a566"),
        ],
    }
}

/// The roots a model directory may live under, in the order they are searched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelRoots {
    /// `$DECKCRAFT_MODELS`.
    pub env: Option<PathBuf>,
    /// The platform config directory plus `models`.
    pub config: Option<PathBuf>,
    /// `models/` beside the executable.
    pub exe: Option<PathBuf>,
    /// `models/` in the source tree (developer convenience).
    pub source: Option<PathBuf>,
}

impl ModelRoots {
    /// The roots in search order, empty paths dropped.
    pub fn candidates(&self) -> Vec<PathBuf> {
        [&self.env, &self.config, &self.exe, &self.source].into_iter().flatten().filter(|p| !p.as_os_str().is_empty()).cloned().collect()
    }

    /// Where a download goes: the explicit root when set, else the config directory. `None`
    /// when the host has neither, in which case the user places the files by hand.
    pub fn install_root(&self) -> Option<PathBuf> {
        self.env.clone().or_else(|| self.config.clone()).or_else(|| self.exe.clone())
    }
}

/// The model's state after looking at a directory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelState {
    /// Every listed file is present with its recorded size and hash.
    Ready,
    /// Nothing there.
    #[default]
    Absent,
    /// The directory exists but files are missing.
    Incomplete,
    /// A file is there with the wrong size or hash.
    Corrupt,
}

impl ModelState {
    pub fn as_str(self) -> &'static str {
        match self {
            ModelState::Ready => "ready",
            ModelState::Absent => "absent",
            ModelState::Incomplete => "incomplete",
            ModelState::Corrupt => "corrupt",
        }
    }
}

/// What was found, and why when it is not [`ModelState::Ready`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelStatus {
    pub state: ModelState,
    pub dir: Option<PathBuf>,
    pub problem: Option<String>,
}

impl ModelStatus {
    /// A message the dialog and the status line can show.
    pub fn summary(&self) -> String {
        match (&self.state, &self.problem) {
            (ModelState::Ready, _) => "Model installed".to_string(),
            (ModelState::Absent, _) => "Model not installed".to_string(),
            (state, Some(problem)) => format!("Model {}: {problem}", state.as_str()),
            (state, None) => format!("Model {}", state.as_str()),
        }
    }
}

/// Lower-case hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in hash.iter() {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Stream a file through SHA-256. Returns the length and the digest.
pub fn sha256_file(path: &Path) -> std::io::Result<(u64, String)> {
    sha256_file_until(path, &std::sync::atomic::AtomicBool::new(false))?.ok_or_else(|| std::io::Error::other("hashing was cancelled"))
}

/// Stream a file through SHA-256, stopping between blocks when `cancel` is set. `Ok(None)` means
/// the caller cancelled; a background check uses this so closing the dialog does not leave a
/// thread hashing 151 MB.
pub fn sha256_file_until(path: &Path, cancel: &std::sync::atomic::AtomicBool) -> std::io::Result<Option<(u64, String)>> {
    use std::sync::atomic::Ordering;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut len = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        len = len.saturating_add(n as u64);
        hasher.update(buf.get(..n).unwrap_or_default());
    }
    let hash = hasher.finalize();
    Ok(Some((len, hex(&hash))))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Check one file against its manifest entry, stopping when `cancel` is set (`Err` then says the
/// check was cancelled).
pub fn verify_file_until(dir: &Path, file: &ModelFile, cancel: &std::sync::atomic::AtomicBool) -> Result<(), String> {
    let name = file.safe_name().ok_or_else(|| format!("unsafe model file name `{}`", file.name))?;
    let path = dir.join(name);
    let Ok(meta) = std::fs::metadata(&path) else {
        return Err(format!("{name} is missing"));
    };
    if !meta.is_file() {
        return Err(format!("{name} is not a file"));
    }
    if meta.len() != file.bytes {
        return Err(format!("{name} is {} bytes, the manifest says {}", meta.len(), file.bytes));
    }
    let Some((len, hash)) = sha256_file_until(&path, cancel).map_err(|e| format!("{name}: {e}"))? else {
        return Err("the check was cancelled".into());
    };
    if len != file.bytes {
        return Err(format!("{name} changed while it was read"));
    }
    if !hash.eq_ignore_ascii_case(&file.sha256) {
        return Err(format!("{name} does not match its SHA-256"));
    }
    Ok(())
}

/// Check one file against its manifest entry.
pub fn verify_file(dir: &Path, file: &ModelFile) -> Result<(), String> {
    verify_file_until(dir, file, &std::sync::atomic::AtomicBool::new(false))
}

/// Check a model directory (usually `<root>/<model id>`), hashing every file.
pub fn check_dir(dir: &Path, manifest: &ModelManifest) -> ModelStatus {
    check_impl(dir, manifest, true)
}

/// Check a model directory without hashing: presence only, cheap enough for a UI frame or a
/// show start. The hash and the size are checked when a model is installed or downloaded.
pub fn check_dir_files(dir: &Path, manifest: &ModelManifest) -> ModelStatus {
    check_impl(dir, manifest, false)
}

fn check_impl(dir: &Path, manifest: &ModelManifest, hash: bool) -> ModelStatus {
    if manifest.validate().is_err() {
        return ModelStatus { state: ModelState::Corrupt, dir: Some(dir.to_path_buf()), problem: Some("the manifest is damaged".into()) };
    }
    if !dir.is_dir() {
        return ModelStatus { state: ModelState::Absent, dir: None, problem: None };
    }
    let missing: Vec<&str> =
        manifest.files.iter().filter(|f| f.safe_name().is_none_or(|n| !dir.join(n).is_file())).map(|f| f.name.as_str()).collect();
    if !missing.is_empty() {
        return ModelStatus { state: ModelState::Incomplete, dir: Some(dir.to_path_buf()), problem: Some(format!("missing {}", missing.join(", "))) };
    }
    if !hash {
        return ModelStatus { state: ModelState::Ready, dir: Some(dir.to_path_buf()), problem: None };
    }
    for file in &manifest.files {
        if let Err(problem) = verify_file(dir, file) {
            return ModelStatus { state: ModelState::Corrupt, dir: Some(dir.to_path_buf()), problem: Some(problem) };
        }
    }
    ModelStatus { state: ModelState::Ready, dir: Some(dir.to_path_buf()), problem: None }
}

/// Check a model directory (usually `<root>/<model id>`), hashing every file, stopping when
/// `cancel` is set. `None` means the caller cancelled before the hashes finished.
pub fn check_dir_until(dir: &Path, manifest: &ModelManifest, cancel: &std::sync::atomic::AtomicBool) -> Option<ModelStatus> {
    use std::sync::atomic::Ordering;
    if manifest.validate().is_err() {
        return Some(ModelStatus { state: ModelState::Corrupt, dir: Some(dir.to_path_buf()), problem: Some("the manifest is damaged".into()) });
    }
    if !dir.is_dir() {
        return Some(ModelStatus { state: ModelState::Absent, dir: None, problem: None });
    }
    let missing: Vec<&str> =
        manifest.files.iter().filter(|f| f.safe_name().is_none_or(|n| !dir.join(n).is_file())).map(|f| f.name.as_str()).collect();
    if !missing.is_empty() {
        return Some(ModelStatus {
            state: ModelState::Incomplete,
            dir: Some(dir.to_path_buf()),
            problem: Some(format!("missing {}", missing.join(", "))),
        });
    }
    for file in &manifest.files {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        match verify_file_until(dir, file, cancel) {
            Ok(()) => {}
            Err(problem) if problem == "the check was cancelled" => return None,
            Err(problem) => return Some(ModelStatus { state: ModelState::Corrupt, dir: Some(dir.to_path_buf()), problem: Some(problem) }),
        }
    }
    Some(ModelStatus { state: ModelState::Ready, dir: Some(dir.to_path_buf()), problem: None })
}

/// Where a download for `manifest` installs: the explicit root when set, else the config
/// directory, else the executable's `models/`. `None` when the host has no writable root, in
/// which case the user places the files by hand.
pub fn install_root(roots: &ModelRoots) -> Option<PathBuf> {
    roots.install_root()
}

/// Where a download for `manifest` installs, with the manifest id appended.
pub fn install_dir(roots: &ModelRoots, manifest: &ModelManifest) -> PathBuf {
    roots.install_root().unwrap_or_else(|| PathBuf::from("models")).join(&manifest.id)
}

/// Find the model among `roots` by presence only. Returns the status and the directory a
/// download would use.
pub fn discover_files(roots: &ModelRoots, manifest: &ModelManifest) -> (ModelStatus, PathBuf) {
    let candidates = roots.candidates();
    let mut first_existing: Option<ModelStatus> = None;
    for root in &candidates {
        let dir = root.join(&manifest.id);
        let status = check_dir_files(&dir, manifest);
        if status.state == ModelState::Ready {
            return (status, dir);
        }
        if first_existing.is_none() && dir.is_dir() {
            first_existing = Some(status);
        }
    }
    // Report the first directory that exists but is not ready, else "absent" with the install
    // target the download or the user would use.
    let install = install_dir(roots, manifest);
    if let Some(status) = first_existing {
        return (status, install);
    }
    (ModelStatus { state: ModelState::Absent, dir: None, problem: None }, install)
}

/// Write the provenance sidecar (`manifest.json`) into a model directory.
///
/// The directory is user-writable and may be a folder the user just picked, so the temporary
/// file is created exclusively (never through a pre-planted symlink), and an existing symlink at
/// the final name is refused rather than followed.
pub fn write_sidecar(dir: &Path, manifest: &ModelManifest) -> std::io::Result<PathBuf> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(SIDECAR);
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(std::io::Error::other("refusing to write through the sidecar symlink"));
    }
    let json = serde_json::to_vec_pretty(manifest).map_err(std::io::Error::other)?;
    let tmp = dir.join(format!("manifest.json.{}.tmp", std::process::id()));
    if std::fs::symlink_metadata(&tmp).is_ok() {
        return Err(std::io::Error::other("refusing to reuse the sidecar temporary file"));
    }
    let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    out.write_all(&json)?;
    out.sync_all()?;
    drop(out);
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

/// The platform config directory plus `models`, matching the desktop app's settings folder.
#[cfg(not(target_arch = "wasm32"))]
pub fn config_models_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/DeckCraft/models"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("DeckCraft/models"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|c| c.join("deckcraft/models"))
    }
}

/// The documented model roots in search order.
#[cfg(not(target_arch = "wasm32"))]
pub fn default_roots() -> ModelRoots {
    ModelRoots {
        env: std::env::var_os(ENV_MODELS).map(PathBuf::from),
        config: config_models_dir(),
        exe: std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("models"))),
        source: option_env!("CARGO_MANIFEST_DIR").map(|d| Path::new(d).join("../../models")),
    }
}

/// The browser has no model directory; a page host supplies the manifest and the model another
/// way.
#[cfg(target_arch = "wasm32")]
pub fn default_roots() -> ModelRoots {
    ModelRoots::default()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn dir(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("deckcraft-asr-{}-{name}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn tiny_manifest(files: &[(&str, &[u8])]) -> ModelManifest {
        ModelManifest {
            id: "test-model".into(),
            name: "Test".into(),
            repo: "owner/test".into(),
            revision: "0".into(),
            license: "MIT".into(),
            runtime: "candle-cpu".into(),
            license_url: "https://huggingface.co/owner/test".into(),
            files: files
                .iter()
                .map(|(name, bytes)| ModelFile {
                    name: (*name).into(),
                    bytes: bytes.len() as u64,
                    sha256: sha256_hex(bytes),
                    url: format!("https://huggingface.co/owner/test/{name}"),
                })
                .collect(),
        }
    }

    #[test]
    fn the_whisper_manifest_is_well_formed_and_matches_its_evidence() {
        let m = whisper_tiny();
        assert_eq!(m.validate(), Ok(()));
        assert_eq!(m.id, WHISPER_TINY_ID);
        assert_eq!(m.license, "Apache-2.0");
        assert_eq!(m.revision, WHISPER_TINY_REVISION);
        assert_eq!(m.total_bytes(), 153_544_121);
        let weights = m.file("model.safetensors").expect("weights entry");
        assert_eq!(weights.bytes, 151_061_672);
        assert_eq!(weights.sha256, "7ebd0e69e78190ffe1438491fa05cc1f5c1aa3a4c4db3bc1723adbb551ea2395");
        assert!(weights.url.ends_with("/model.safetensors") && weights.url.contains(WHISPER_TINY_REVISION));
        assert_eq!(m.file("config.json").map(|f| f.bytes), Some(1_983));
        assert_eq!(m.file("tokenizer.json").map(|f| f.bytes), Some(2_480_466));
    }

    #[test]
    fn hashing_matches_known_digests() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        let d = dir("hash");
        let p = d.join("a.bin");
        std::fs::write(&p, b"abc").unwrap();
        let (len, hex) = sha256_file(&p).unwrap();
        assert_eq!(len, 3);
        assert_eq!(hex, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn a_directory_is_absent_incomplete_corrupt_or_ready() {
        let manifest = tiny_manifest(&[("a.bin", b"hello"), ("b.bin", b"world")]);
        let d = dir("states");
        let model = d.join(&manifest.id);
        assert_eq!(check_dir(&model, &manifest).state, ModelState::Absent);
        std::fs::create_dir_all(&model).unwrap();
        let missing = check_dir(&model, &manifest);
        assert_eq!(missing.state, ModelState::Incomplete);
        assert!(missing.problem.as_deref().is_some_and(|p| p.contains("a.bin")));
        std::fs::write(model.join("a.bin"), b"hello").unwrap();
        std::fs::write(model.join("b.bin"), b"world").unwrap();
        assert_eq!(check_dir(&model, &manifest).state, ModelState::Ready);
        std::fs::write(model.join("b.bin"), b"World").unwrap();
        let corrupt = check_dir(&model, &manifest);
        assert_eq!(corrupt.state, ModelState::Corrupt);
        assert!(corrupt.summary().contains("SHA-256"));
    }

    #[test]
    fn discovery_prefers_the_environment_root_and_reports_the_install_target() {
        let manifest = tiny_manifest(&[("a.bin", b"hello")]);
        let base = dir("discover");
        let (env, config) = (base.join("env"), base.join("config"));
        let ready = env.join(&manifest.id);
        std::fs::create_dir_all(&ready).unwrap();
        std::fs::write(ready.join("a.bin"), b"hello").unwrap();
        let roots = ModelRoots { env: Some(env.clone()), config: Some(config.clone()), exe: None, source: None };
        let (status, install) = discover_files(&roots, &manifest);
        assert_eq!(status.state, ModelState::Ready);
        assert_eq!(status.dir.as_deref(), Some(ready.as_path()));
        assert_eq!(install, ready);

        let empty = ModelRoots { env: None, config: Some(config.clone()), exe: None, source: None };
        let (status, install) = discover_files(&empty, &manifest);
        assert_eq!(status.state, ModelState::Absent);
        assert_eq!(install, config.join(&manifest.id));

        // An incomplete config install is reported, not skipped for a lower root.
        std::fs::create_dir_all(install.clone()).unwrap();
        let (status, _) = discover_files(&empty, &manifest);
        assert_eq!(status.state, ModelState::Incomplete);
    }

    #[test]
    fn unsafe_names_and_damaged_manifests_are_rejected() {
        for name in ["../escape", "a/b", "C:evil", "", ".hidden", "a\\b"] {
            let mut m = tiny_manifest(&[("a.bin", b"x")]);
            m.files[0].name = name.into();
            assert!(m.validate().is_err(), "{name}");
            assert!(m.files[0].safe_name().is_none(), "{name}");
        }
        let mut m = tiny_manifest(&[("a.bin", b"x")]);
        m.files[0].sha256 = "nothex".into();
        assert!(m.validate().is_err());
        let mut m = tiny_manifest(&[("a.bin", b"x")]);
        m.files.push(m.files[0].clone());
        assert!(m.validate().is_err(), "a repeated file");
        let d = dir("bad-manifest");
        assert_eq!(check_dir(&d, &m).state, ModelState::Corrupt);
    }

    #[test]
    fn the_sidecar_is_written_atomically_and_parses_back() {
        let manifest = whisper_tiny();
        let d = dir("sidecar");
        let path = write_sidecar(&d, &manifest).unwrap();
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some(SIDECAR));
        let back: ModelManifest = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(back, manifest);
        let leftovers: Vec<_> =
            std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert!(leftovers.is_empty(), "the temporary file is renamed away");
    }

    #[test]
    fn only_https_urls_on_the_model_hub_are_accepted() {
        for ok in [
            "https://huggingface.co/openai/whisper-tiny/resolve/abc/model.safetensors",
            "https://cdn-lfs.huggingface.co/repos/x",
            "https://HF.CO/x",
            "https://huggingface.co:443/x",
        ] {
            assert!(model_url_allowed(ok), "{ok}");
        }
        for bad in [
            "http://huggingface.co/x",
            "https://example.com/x",
            "https://huggingface.co.evil.example/x",
            "https://user@evil.example/x",
            "file:///etc/passwd",
            "https://",
            "https://huggingface.co\\evil",
        ] {
            assert!(!model_url_allowed(bad), "{bad}");
        }
        let mut m = tiny_manifest(&[("a.bin", b"x")]);
        m.files[0].url = "http://huggingface.co/a.bin".into();
        assert!(m.validate().is_err(), "a plain-HTTP model file is refused");
        let mut m = tiny_manifest(&[("a.bin", b"x")]);
        m.license_url = "https://example.com/licence".into();
        assert!(m.validate().is_err(), "a licence page off the hub is refused");
    }

    #[test]
    fn a_cancelled_hash_check_stops_between_blocks() {
        let manifest = tiny_manifest(&[("a.bin", b"hello")]);
        let d = dir("cancel");
        let model = d.join(&manifest.id);
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("a.bin"), b"hello").unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(true);
        assert_eq!(check_dir_until(&model, &manifest, &cancel), None, "a cancelled check reports nothing");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        assert_eq!(check_dir_until(&model, &manifest, &cancel).map(|s| s.state), Some(ModelState::Ready));
    }

    #[cfg(unix)]
    #[test]
    fn a_planted_sidecar_symlink_is_refused() {
        let manifest = whisper_tiny();
        let d = dir("sidecar-symlink");
        let outside = d.join("outside.json");
        std::fs::write(&outside, b"{}").unwrap();
        std::os::unix::fs::symlink(&outside, d.join(SIDECAR)).unwrap();
        assert!(write_sidecar(&d, &manifest).is_err(), "the sidecar is not written through a symlink");
        assert_eq!(std::fs::read(&outside).unwrap(), b"{}", "the link target is untouched");
    }
}
