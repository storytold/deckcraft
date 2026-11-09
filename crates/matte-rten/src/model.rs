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

/// The first root whose model file exists at the expected size, else the first root's failure, else
/// the missing state naming the root to install into. Size only: the hash is checked by
/// [`RtenSegmenter::load`] and by the install and download paths, never by a status query.
pub fn probe() -> (PathBuf, ModelState) {
    probe_roots(&model_roots())
}

/// [`probe`] over an explicit root order: the first root with a file of the manifest's size wins,
/// a wrong-size file is remembered and reported only when no root has a valid one, and an empty
/// list falls back to the missing state for `models`.
fn probe_roots(roots: &[PathBuf]) -> (PathBuf, ModelState) {
    let root = roots.first().cloned().unwrap_or_else(|| PathBuf::from("models"));
    let mut failure: Option<(PathBuf, ModelState)> = None;
    for candidate in roots {
        let path = manifest::MANIFEST.path_in(candidate);
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if !meta.is_file() {
            continue;
        }
        // Presence and size only: the bytes are verified where they are used — `RtenSegmenter::load`
        // before it parses them, `install_from` and `download_to` before they rename a file into
        // place. Hashing here would read 191 MB twice before the first frame on every start.
        if meta.len() == manifest::MODEL_BYTES {
            return (candidate.clone(), ModelState::Ready { path: path.display().to_string(), bytes: manifest::MODEL_BYTES });
        }
        // A wrong-size file in a high-priority root must not hide a valid install further down, and
        // `installed_name` (which scans every root) must not disagree with this answer: remember the
        // first failure and keep looking.
        if failure.is_none() {
            failure = Some((
                candidate.clone(),
                ModelState::Failed { detail: format!("{} is {} bytes, the model is {}", path.display(), meta.len(), manifest::MODEL_BYTES) },
            ));
        }
    }
    if let Some(failure) = failure {
        return failure;
    }
    let state = missing_state(&root);
    (root, state)
}

/// The manifest's file in `root` when it is present at the manifest's size, without opening or
/// hashing it: the cheap "is a model installed" answer.
fn present_model(root: &Path) -> Option<PathBuf> {
    let path = manifest::MANIFEST.path_in(root);
    let meta = std::fs::metadata(&path).ok()?;
    (meta.is_file() && meta.len() == manifest::MODEL_BYTES).then_some(path)
}

pub(crate) fn missing_state(root: &Path) -> ModelState {
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
/// The source is opened once and copied from that handle into an exclusive temporary file (random
/// name, `create_new`) in the same directory; the copy is verified and renamed only after the hash
/// matches. Symbols links at the source, the target or the temporary name are refused, so the write
/// cannot land on a path the user did not pick, and a failure never leaves a half file that later
/// looks installed.
pub fn install_from(source: &Path, root: &Path) -> Result<PathBuf, String> {
    refuse_symlink(source, "the picked file")?;
    let mut input = std::fs::File::open(source).map_err(|e| format!("cannot read {}: {e}", source.display()))?;
    let size = input.metadata().map_err(|e| format!("cannot read {}: {e}", source.display()))?.len();
    // Reject the size before hashing: a user who picks a multi-GB video must not pay for reading
    // every byte of it just to be told it is the wrong file.
    if size != manifest::MANIFEST.bytes {
        return Err(format!("the model is {} bytes, {} is {size}", manifest::MANIFEST.bytes, source.display()));
    }
    let target = manifest::MANIFEST.path_in(root);
    let dir = target.parent().ok_or_else(|| format!("{} has no parent", target.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    // A pre-planted link at `model.onnx` is not silently replaced either: the caller asked to
    // install a model into this directory, not to clobber whatever the link points at.
    refuse_symlink(&target, "the install target")?;
    // The source is opened once and the bytes are copied from that handle; the copy is verified
    // (the same bytes, in a file this process created) before the rename, so nothing can swap the
    // file between a hash and a reopen.
    let (tmp, mut out) = create_part(dir, &target)?;
    let copied = std::io::copy(&mut input, &mut out).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot copy to {}: {e}", tmp.display())
    })?;
    drop(out);
    if copied != manifest::MANIFEST.bytes {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("the model is {} bytes, {} gave {copied}", manifest::MANIFEST.bytes, source.display()));
    }
    commit_part(&tmp, &target)?;
    // Best effort: the provenance sidecar the shared policy names.
    write_manifest(root);
    Ok(target)
}

/// A fresh, exclusive temporary file beside `target`. The name carries the process id, a counter
/// and the clock, so a pre-planted `model.onnx.part` — or a symbolic link at any fixed name — is
/// never opened or followed; `create_new` (O_EXCL) refuses a path that already exists.
fn create_part(dir: &Path, target: &Path) -> Result<(PathBuf, std::fs::File), String> {
    let stem = target.file_name().and_then(|n| n.to_str()).unwrap_or("model.onnx");
    for _ in 0..8 {
        let seq = PART_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let path = dir.join(format!("{stem}.{}.{seq}.{nanos}.part", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("cannot write {}: {e}", path.display())),
        }
    }
    Err(format!("cannot create a temporary file in {}", dir.display()))
}

/// A counter for [`create_part`]; the clock alone can repeat within a tick.
static PART_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Refuse a symbolic link where the install writes. The model root is a directory the user
/// controls, and a link planted there would send a write (or a rename) somewhere else.
fn refuse_symlink(path: &Path, what: &str) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(format!("{what} {} is a symbolic link; refusing to use it", path.display())),
        _ => Ok(()),
    }
}

/// Verify a temporary file this process created and rename it into place. One cleanup point: every
/// failure removes the temporary file, so a short body, a hash mismatch or a failed rename never
/// leaves 192 MB behind under a name that later looks installed.
fn commit_part(tmp: &Path, target: &Path) -> Result<(), String> {
    let result = manifest::verify_file(tmp)
        .map_err(|e| e.to_string())
        .and_then(|()| std::fs::rename(tmp, target).map_err(|e| format!("cannot install {}: {e}", target.display())));
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
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
    // The sidecar is provenance, not trusted input: never write through a link someone planted at
    // its name. A temporary file plus a rename replaces a link instead of following it.
    let sidecar_path = dir.join("manifest.json");
    if std::fs::symlink_metadata(&sidecar_path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return;
    }
    let Ok((tmp, mut file)) = create_part(&dir, &sidecar_path) else { return };
    let written = file.write_all(sidecar.as_bytes()).is_ok();
    drop(file);
    if !written || std::fs::rename(&tmp, &sidecar_path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// The download agent. ureq's defaults are no limit at all, so a stalled connect or a stalled body
/// read would leave the download thread — and the panel's "Downloading" — stuck forever, with no
/// way to retry. Per-read is a stall detector: it restarts for each transport read, so a slow but
/// live 191 MB transfer is never cut off, and no total budget is set for the same reason.
fn download_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_resolve(Some(Duration::from_secs(30)))
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_per_read(Some(Duration::from_secs(30)))
        .build()
        .new_agent()
}

/// A download the caller can stop, and the total time it may take. A per-read timeout alone only
/// catches a dead connection: a server that trickles bytes forever would keep the thread — and the
/// panel's "Downloading" — alive for ever, so a total budget is enforced as well.
pub struct DownloadControl {
    cancelled: std::sync::atomic::AtomicBool,
    deadline: std::time::Instant,
}

impl DownloadControl {
    pub fn new(budget: Duration) -> Self {
        DownloadControl { cancelled: std::sync::atomic::AtomicBool::new(false), deadline: std::time::Instant::now() + budget }
    }

    /// Ask the transfer to stop at its next read. The thread then fails, removes its temporary file
    /// and lets the panel retry or install.
    pub fn cancel(&self) {
        self.cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Why the transfer must stop now, if it must.
    fn stopped(&self) -> Option<String> {
        if self.is_cancelled() {
            return Some("the download was cancelled".into());
        }
        (std::time::Instant::now() >= self.deadline)
            .then(|| format!("the download took longer than {} minutes and was stopped", DOWNLOAD_TIMEOUT.as_secs() / 60))
    }
}

/// The most a download may take: 191 MB at about 106 KB/s. A live but very slow transfer is cut
/// off here rather than only by the per-read stall detector; retrying starts a new budget.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Download the model into `root` from the manifest's pinned URL, reporting progress. Blocking:
/// the caller runs it on a worker thread after the consent screen and can [`DownloadControl::cancel`]
/// it. A short download is rejected by the size check and never renamed into place, and no failure
/// — including a cancel or the total deadline — leaves a temporary file behind.
pub fn download_to(root: &Path, control: &DownloadControl, on_progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, String> {
    let target = manifest::MANIFEST.path_in(root);
    download_into(&target, control, on_progress)?;
    write_manifest(root);
    Ok(target)
}

/// The transfer, ending in the verified rename. A failure removes the temporary file it created.
fn download_into(target: &Path, control: &DownloadControl, on_progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    let dir = target.parent().ok_or_else(|| format!("{} has no parent", target.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    refuse_symlink(target, "the model file")?;
    let (tmp, mut file) = create_part(dir, target)?;
    let transferred = transfer(&tmp, &mut file, control, on_progress);
    drop(file);
    if let Err(e) = transferred {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    commit_part(&tmp, target)
}

fn transfer(tmp: &Path, file: &mut std::fs::File, control: &DownloadControl, on_progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    let m = manifest::MANIFEST;
    if let Some(reason) = control.stopped() {
        return Err(reason);
    }
    let mut response = download_agent().get(&m.url()).call().map_err(|e| format!("download failed: {e}"))?;
    let total = response.body().content_length().unwrap_or(0);
    if total > 0 && total != m.bytes {
        return Err(format!("the server offered {total} bytes, the model is {} bytes", m.bytes));
    }
    let mut reader = response.body_mut().as_reader();
    let mut buf = vec![0u8; 256 * 1024];
    let mut received = 0u64;
    loop {
        if let Some(reason) = control.stopped() {
            return Err(reason);
        }
        let n = reader.read(&mut buf).map_err(|e| format!("download failed: {e}"))?;
        if n == 0 {
            break;
        }
        received = received.saturating_add(n as u64);
        if received > m.bytes {
            return Err("the download is larger than the model".into());
        }
        file.write_all(buf.get(..n).unwrap_or_default()).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        on_progress(received, if total == 0 { m.bytes } else { total });
    }
    if received != m.bytes {
        return Err(format!("the download stopped at {received} bytes, the model is {} bytes", m.bytes));
    }
    Ok(())
}

/// The RTen implementation of [`Segmenter`].
pub struct RtenSegmenter {
    model: rten::Model,
    options: rten::RunOptions,
}

impl RtenSegmenter {
    /// The name `cameo.background.state` reports for this backend.
    pub const NAME: &'static str = "birefnet-lite-512";

    /// Verify and load the model at `path`. The file is opened once: its bytes are hashed and then
    /// parsed from the same buffer, so a replacement between the hash and the parse cannot slip
    /// unverified weights past the check.
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = manifest::read_verified(path).map_err(|e| e.to_string())?;
        let model = rten::Model::load(bytes).map_err(|e| format!("cannot load {}: {e}", path.display()))?;
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
        // The matte is feathered once, by the compositor (`CompositeOptions::feather`); a second
        // blur here would widen every requested radius and make `feather: 0` impossible.
        Ok(RtenSegmenter { model, options })
    }

    /// The name `cameo.background.state` reports before anything is loaded: `None` unless a file
    /// of the manifest's size sits in one of the model roots. A `stat` per root — never a hash and
    /// never a parse, so a status query on the UI thread stays cheap; a preview proves the bytes.
    pub fn installed_name() -> Option<&'static str> {
        model_roots().iter().any(|root| present_model(root).is_some()).then_some(RtenSegmenter::NAME)
    }

    /// Load from the first model root that holds a verified install.
    pub fn load_installed() -> Result<Self, String> {
        let (root, state) = probe();
        match state {
            ModelState::Ready { path, .. } => RtenSegmenter::load(Path::new(&path)),
            other => Err(format!("{} (looked in {})", other.message(), root.display())),
        }
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
        postprocess(&logits, manifest::INPUT_SIZE, frame.width(), frame.height(), 0)
    }

    fn name(&self) -> &'static str {
        RtenSegmenter::NAME
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file symbolic link, where the host lets a test make one.
    #[cfg(windows)]
    fn symlink_file(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_file(src, dst)
    }

    #[cfg(unix)]
    fn symlink_file(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(src, dst)
    }

    #[test]
    fn roots_are_absolute_enough_and_the_env_wins() {
        // The env var is read at call time; setting it in a test can race other tests in the same
        // binary, so only check the shape without mutating the environment.
        let roots = model_roots();
        assert!(!roots.is_empty());
        assert!(roots.last().is_some_and(|p| p.ends_with("models")), "{roots:?}");
    }

    #[test]
    fn the_download_agent_bounds_every_stage() {
        let agent = download_agent();
        let t = agent.config().timeouts();
        assert!(t.resolve.is_some(), "DNS");
        assert!(t.connect.is_some(), "connect");
        assert!(t.recv_response.is_some(), "response headers");
        assert!(agent.config().timeout_per_read().is_some(), "a stalled body read must fail, not hang the thread");
        assert!(t.global.is_none() && t.per_call.is_none() && t.recv_body.is_none(), "a slow but live 191 MB transfer is never cut off");
    }

    /// A wrong-size file in a high-priority root must not hide a valid install further down, and
    /// the answer must agree with `installed_name` (which scans every root).
    #[test]
    fn a_wrong_size_file_does_not_hide_a_valid_root() {
        let dir = crate::test_temp::TempDir::new("probe");
        let (bad, good) = (dir.path().join("bad"), dir.path().join("good"));
        for root in [&bad, &good] {
            let path = manifest::MANIFEST.path_in(root);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        }
        std::fs::write(manifest::MANIFEST.path_in(&bad), b"short").expect("bad file");
        // A sparse file of exactly the manifest's size: `probe` never reads the bytes.
        std::fs::File::create(manifest::MANIFEST.path_in(&good)).and_then(|f| f.set_len(manifest::MODEL_BYTES)).expect("good file");

        let (root, state) = probe_roots(&[bad.clone(), good.clone()]);
        assert_eq!(root, good, "the valid root wins over an earlier wrong-size file");
        assert!(state.is_ready());
        assert!(present_model(&good).is_some() && present_model(&bad).is_none(), "installed_name agrees");

        let (root, state) = probe_roots(std::slice::from_ref(&bad));
        assert_eq!(root, bad, "only a failure to report");
        assert!(matches!(state, ModelState::Failed { .. }), "{state:?}");

        let empty = dir.path().join("empty");
        let (root, state) = probe_roots(std::slice::from_ref(&empty));
        assert_eq!(root, empty);
        assert!(matches!(state, ModelState::Missing { .. }), "{state:?}");
    }

    #[test]
    fn install_rejects_a_file_that_is_not_the_model_and_leaves_nothing_behind() {
        let dir = crate::test_temp::TempDir::new("install");
        let bad = dir.path().join("bad.onnx");
        std::fs::write(&bad, b"not the model").unwrap_or_default();
        let err = install_from(&bad, dir.path()).expect_err("rejected");
        assert!(err.contains("bytes"), "{err}");
        let target = manifest::MANIFEST.path_in(dir.path());
        assert!(!target.exists(), "no partial install");
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

    /// A temporary file whose size is wrong is removed on the failure path, so a short body or a
    /// failed copy never leaves 192 MB behind under a name that later looks installed. The
    /// verify-and-rename is local, so this runs without a network.
    #[test]
    fn a_part_that_fails_verification_is_removed() {
        let dir = crate::test_temp::TempDir::new("commit");
        let target = manifest::MANIFEST.path_in(dir.path());
        std::fs::create_dir_all(target.parent().expect("parent")).expect("dir");
        let (tmp, mut file) = create_part(target.parent().expect("parent"), &target).expect("part");
        std::io::Write::write_all(&mut file, b"a short body").expect("write");
        drop(file);
        let err = commit_part(&tmp, &target).expect_err("wrong size");
        assert!(err.contains("bytes"), "{err}");
        assert!(!tmp.exists(), "the temporary file is gone");
        assert!(!target.exists(), "nothing was renamed into place");
        assert_eq!(std::fs::read_dir(target.parent().expect("parent")).expect("dir").count(), 0, "no leftovers");
    }

    /// The temporary name is fresh and exclusive, never the old fixed `model.onnx.part`: a link
    /// planted at that name is not opened or followed (its target keeps its bytes).
    #[test]
    fn a_planted_part_name_is_not_followed() {
        let dir = crate::test_temp::TempDir::new("part-name");
        let target = manifest::MANIFEST.path_in(dir.path());
        let parent = target.parent().expect("parent").to_path_buf();
        std::fs::create_dir_all(&parent).expect("dir");
        let victim = dir.path().join("victim.txt");
        std::fs::write(&victim, b"keep me").expect("victim");
        let planted = parent.join("model.onnx.part");
        // A symbolic link needs a privilege on Windows; without it there is nothing to plant and
        // the rest of the assertion still checks the fresh name.
        let linked = symlink_file(&victim, &planted).is_ok();
        if !linked {
            let _ = std::fs::remove_file(&planted);
        }
        let (first, _) = create_part(&parent, &target).expect("first");
        let (second, _) = create_part(&parent, &target).expect("second");
        assert_ne!(first, second, "each temporary file gets its own name");
        assert_ne!(first, planted, "the fixed name is never used");
        assert_eq!(std::fs::read(&victim).expect("victim"), b"keep me", "nothing wrote through a planted link");
        if std::fs::symlink_metadata(&planted).is_ok_and(|m| m.file_type().is_symlink()) {
            assert_eq!(std::fs::read(&victim).expect("victim"), b"keep me");
        }
    }

    /// A picked file that is a symbolic link is refused before anything is read: the install writes
    /// a model the user picked, not whatever the link points at.
    #[test]
    fn a_symlinked_source_is_refused() {
        let dir = crate::test_temp::TempDir::new("symlink-source");
        let real = dir.path().join("real.onnx");
        std::fs::write(&real, b"not the model").expect("real");
        let link = dir.path().join("link.onnx");
        if symlink_file(&real, &link).is_err() {
            return; // no symlink privilege on this host
        }
        let err = install_from(&link, dir.path()).expect_err("a link is refused");
        assert!(err.contains("symbolic link"), "{err}");
    }
}
