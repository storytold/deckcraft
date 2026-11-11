//! The host service the Camera Format panel drives: model state, install, consented download and
//! the cached-matte processing loop.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use deckcraft_matte::Accelerator;
use deckcraft_matte::compositing::CompositeOptions;
use deckcraft_matte::frame::RgbaFrame;
use deckcraft_matte::service::{MatteService, ModelState, Timings};
use deckcraft_matte::{Matte, MatteError, Segmenter};

use crate::model::{self, RtenSegmenter};
use crate::worker::MatteWorker;

/// The default minimum time between inference runs. The measured CPU time is about 2.1 s, so a
/// shorter interval only queues a frame the worker cannot reach sooner.
pub const DEFAULT_INTERVAL_MS: u64 = 2_000;

/// What the desktop host installs into `Services::matte`.
pub struct Service {
    root: PathBuf,
    state: ModelState,
    worker: Option<MatteWorker>,
    download: Option<std::thread::JoinHandle<()>>,
    download_state: Option<Arc<Mutex<ModelState>>>,
    /// A running download can be stopped (the panel's Cancel button), and its thread fails on the
    /// next read instead of trickling on for ever.
    download_control: Option<Arc<model::DownloadControl>>,
    /// The current `Failed` state came from a download and stays on screen until the user acts
    /// on it; probe and load failures are re-probed by [`Service::refresh`].
    download_failed: bool,
    /// The model path whose worker load failed, so `refresh` only starts a new load when the
    /// ready path actually changed: re-hashing 191 MB every probe interval would be worse than
    /// the cached failure.
    failed_load_path: Option<String>,
    interval: Duration,
    accelerator: Accelerator,
}

impl Default for Service {
    fn default() -> Self {
        Service::new(DEFAULT_INTERVAL_MS)
    }
}

impl Service {
    /// Discover the model now. No network, no load until the first frame asks for a matte.
    pub fn new(interval_ms: u64) -> Self {
        let (root, state) = model::probe();
        Self::with_state(root, state, interval_ms)
    }

    /// A service over one explicit model root, for tests and for a host that decides where the
    /// model belongs. The state comes from that root alone, so a test never depends on a model
    /// installed on the machine.
    pub fn with_root(root: PathBuf, interval_ms: u64) -> Self {
        let path = deckcraft_matte::manifest::MANIFEST.path_in(&root);
        let state = match std::fs::metadata(&path) {
            Ok(meta) if meta.is_file() && meta.len() == deckcraft_matte::manifest::MODEL_BYTES => {
                ModelState::Ready { path: path.display().to_string(), bytes: deckcraft_matte::manifest::MODEL_BYTES }
            }
            _ => model::missing_state(&root),
        };
        Self::with_state(root, state, interval_ms)
    }

    fn with_state(root: PathBuf, state: ModelState, interval_ms: u64) -> Self {
        Service {
            root,
            state,
            worker: None,
            download: None,
            download_state: None,
            download_control: None,
            download_failed: false,
            failed_load_path: None,
            interval: Duration::from_millis(interval_ms.clamp(0, 60_000)),
            accelerator: Accelerator::default(),
        }
    }

    /// The directory installs go into (the first root, or `$DECKCRAFT_MODELS`).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Re-read the model directory. The panel calls this when it opens.
    pub fn refresh(&mut self) {
        let (root, state) = model::probe();
        self.root = root;
        if self.download.is_some() {
            return;
        }
        // A worker whose load failed keeps that failure for ever. Drop it only when the ready path
        // changed (a model installed under another root, or a stale worker from a previous path):
        // replacing the file in place keeps the same path, and re-hashing 191 MB on every probe
        // would be worse than keeping the cached failure until the user installs again.
        let load_failed = self.worker.as_ref().is_some_and(|w| matches!(w.error(), Some(MatteError::Unavailable(_))));
        if let ModelState::Ready { path, .. } = &state
            && load_failed
        {
            if self.failed_load_path.as_deref() == Some(path.as_str()) {
                // The worker's failure is the current answer for this path: keep it, or the panel
                // would alternate between Ready and Failed every probe interval.
                return;
            }
            self.worker = None;
            self.failed_load_path = None;
        }
        // Only a download failure is sticky: the panel should keep showing why the download failed
        // until the user acts on it. A wrong-size file or a load failure is re-probed, so deleting
        // or replacing the bad file clears it.
        if self.download_failed && !state.is_ready() {
            return;
        }
        self.download_failed = false;
        self.state = state;
    }

    /// Create the worker, if there is not one. The model is loaded on the worker's own thread: a
    /// 191 MB verify and a graph parse must not block the caller, so this returns as soon as the
    /// thread is spawned and [`MatteService::process`] answers `None` until the first matte.
    fn ensure_worker(&mut self) -> Result<(), MatteError> {
        if self.worker.is_some() {
            return Ok(());
        }
        let path = match &self.state {
            ModelState::Ready { path, .. } => path.clone(),
            other => return Err(MatteError::Unavailable(other.message())),
        };
        let target = PathBuf::from(path);
        let worker = MatteWorker::with_loader(
            Box::new(move || RtenSegmenter::load(&target).map(|s| Box::new(s) as Box<dyn Segmenter>).map_err(MatteError::Unavailable)),
            self.interval,
        )
        .inspect_err(|e| {
            self.state = ModelState::Failed { detail: e.message() };
        })?;
        self.worker = Some(worker);
        Ok(())
    }

    fn poll_download(&mut self) {
        let Some(handle) = &self.download else { return };
        if !handle.is_finished() {
            if let Some(shared) = &self.download_state
                && let Ok(s) = shared.lock()
            {
                self.state = s.clone();
            }
            return;
        }
        if let Some(h) = self.download.take() {
            let _ = h.join();
        }
        self.download_control = None;
        // The thread records why the download failed. Probing the disk cannot tell a network
        // error, a size mismatch or a hash failure from "never installed" — both leave no
        // file — so keep the failure sentence for the panel.
        let failure =
            self.download_state.take().and_then(|shared| shared.lock().ok().map(|s| s.clone())).filter(|s| matches!(s, ModelState::Failed { .. }));
        // A *successful* download replaced whatever the worker failed to load at this path: drop the
        // cached failure before probing, so `refresh` can report Ready. A failed download left the
        // file alone, so its worker failure (and the retry path the panel offers for it) stays.
        if failure.is_none() && self.worker.as_ref().is_some_and(|w| matches!(w.error(), Some(MatteError::Unavailable(_)))) {
            self.worker = None;
            self.failed_load_path = None;
        }
        self.refresh();
        if !self.state.is_ready()
            && let Some(failed) = failure
        {
            if let ModelState::Failed { detail } = &failed {
                log::warn!("camera background model download did not install: {detail}");
            }
            self.download_failed = true;
            self.state = failed;
        }
    }
}

impl MatteService for Service {
    fn state(&mut self) -> ModelState {
        self.poll_download();
        self.state.clone()
    }

    fn refresh(&mut self) {
        Service::refresh(self);
    }

    fn install(&mut self, path: &str) -> Result<(), String> {
        // A finished download no longer blocks an install; a running one does: both write the same
        // `model.onnx.part`, so racing them can install a truncated file.
        self.poll_download();
        if self.download.is_some() {
            return Err("a download is running: wait for it to finish before importing the model".into());
        }
        let target = model::install_from(Path::new(path), &self.root)?;
        self.download_failed = false;
        self.failed_load_path = None;
        self.state = ModelState::Ready { path: target.display().to_string(), bytes: deckcraft_matte::manifest::MODEL_BYTES };
        self.worker = None;
        Ok(())
    }

    fn start_download(&mut self) -> Result<(), String> {
        // A finished download must not block a retry: `download` is `Some` until the next poll
        // joins it.
        self.poll_download();
        if self.download.is_some() {
            return Ok(());
        }
        let root = self.root.clone();
        let control = Arc::new(model::DownloadControl::new(model::DOWNLOAD_TIMEOUT));
        let shared = Arc::new(Mutex::new(ModelState::Downloading { received: 0, total: deckcraft_matte::manifest::MODEL_BYTES }));
        let progress = Arc::clone(&shared);
        let download_control = Arc::clone(&control);
        let handle = std::thread::Builder::new()
            .name("deckcraft-matte-download".into())
            .spawn(move || {
                let mut on_progress = |received: u64, total: u64| {
                    if let Ok(mut s) = progress.lock() {
                        *s = ModelState::Downloading { received, total };
                    }
                };
                if let Err(e) = model::download_to(&root, &download_control, &mut on_progress)
                    && let Ok(mut s) = progress.lock()
                {
                    *s = ModelState::Failed { detail: e };
                }
            })
            .map_err(|e| format!("cannot start the download: {e}"))?;
        self.download = Some(handle);
        self.download_state = Some(shared);
        self.download_control = Some(control);
        self.download_failed = false;
        self.state = ModelState::Downloading { received: 0, total: deckcraft_matte::manifest::MODEL_BYTES };
        Ok(())
    }

    fn cancel_download(&mut self) {
        if let Some(control) = &self.download_control {
            control.cancel();
        }
        self.poll_download();
    }

    fn process(&mut self, frame: &RgbaFrame, options: &CompositeOptions, replacement: Option<&RgbaFrame>) -> Result<Option<RgbaFrame>, MatteError> {
        if options.mode == deckcraft_model::CameraBackgroundMode::Normal {
            return Ok(Some(frame.clone()));
        }
        self.poll_download();
        self.ensure_worker()?;
        let result = match self.worker.as_ref() {
            Some(worker) => worker.process(frame, options, replacement, true),
            None => return Err(MatteError::Unavailable("worker did not start".into())),
        };
        // The worker's own load failed: say so in the model state, and keep the error on the
        // worker so the 191 MB file is not re-verified on every frame.
        if let Err(MatteError::Unavailable(detail)) = &result {
            if let ModelState::Ready { path, .. } = &self.state {
                self.failed_load_path = Some(path.clone());
            }
            self.state = ModelState::Failed { detail: detail.clone() };
        }
        result
    }

    fn timings(&self) -> Timings {
        self.worker.as_ref().map(MatteWorker::timings).unwrap_or_default()
    }

    fn unavailable_reason(&mut self) -> Option<String> {
        self.poll_download();
        match &self.state {
            ModelState::Ready { .. } => None,
            other => Some(other.message()),
        }
    }

    fn last_matte(&mut self) -> Option<Matte> {
        self.worker.as_ref().and_then(MatteWorker::latest).map(|m| (*m).clone())
    }

    fn accelerator(&self) -> Accelerator {
        self.accelerator
    }

    fn set_accelerator(&mut self, accelerator: Accelerator) -> Result<(), String> {
        if accelerator == Accelerator::Cpu {
            self.accelerator = Accelerator::Cpu;
            Ok(())
        } else {
            Err(deckcraft_matte::ACCELERATOR_UNAVAILABLE.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `install` and a running download both write a temporary file in the model directory; racing
    /// them can install a truncated file. A finished download must not block a later install.
    #[test]
    fn install_is_refused_while_a_download_runs() {
        let dir = crate::test_temp::TempDir::new("install-race");
        let mut s = Service::with_root(dir.path().to_path_buf(), 0);
        // The thread stays alive until this test releases it, so a slow CI machine cannot
        // finish the "download" before `install` looks.
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        s.download = Some(std::thread::spawn(move || {
            let _ = rx.recv();
        }));
        let e = s.install("picked-model.onnx").expect_err("download running");
        assert!(e.contains("download"), "{e}");
        drop(tx);
        if let Some(h) = s.download.take() {
            let _ = h.join();
        }
    }

    /// The model state comes from the service's own root, so a model installed on the machine does
    /// not make this test vacuous.
    #[test]
    fn a_missing_model_is_named_and_a_normal_frame_needs_no_model() {
        let dir = crate::test_temp::TempDir::new("missing");
        let mut s = Service::with_root(dir.path().to_path_buf(), 0);
        let message = s.state().message();
        assert!(message.contains("not installed"), "{message}");
        let f = RgbaFrame::solid(8, 8, [1, 2, 3, 255]).expect("frame");
        let out = s.process(&f, &CompositeOptions::new(deckcraft_model::CameraBackgroundMode::Normal), None).expect("normal").expect("returned");
        assert_eq!(out, f);
    }

    /// A download that fails leaves no file, so probing the disk cannot say why: the panel must
    /// still show the thread's sentence instead of "not installed".
    /// Only a download failure is sticky: a probe or load failure is re-read when the panel
    /// refreshes, so deleting or replacing the bad file clears the sentence.
    #[test]
    fn a_stale_probe_failure_clears_on_refresh() {
        let mut s = Service::new(0);
        s.state = ModelState::Failed { detail: "old probe failure".into() };
        s.refresh();
        assert!(!matches!(&s.state, ModelState::Failed { detail } if detail == "old probe failure"), "{:?}", s.state);
    }

    /// The sentence a failed download left behind survives the panel's refresh: a probe that finds
    /// no file cannot say why the download failed.
    #[test]
    fn a_download_failure_is_sticky_until_acted_on() {
        let dir = crate::test_temp::TempDir::new("sticky");
        let mut s = Service::with_root(dir.path().to_path_buf(), 0);
        s.download_failed = true;
        s.state = ModelState::Failed { detail: "download failed: connection reset".into() };
        s.refresh();
        assert!(matches!(&s.state, ModelState::Failed { detail } if detail.contains("connection reset")), "{:?}", s.state);
    }

    /// A failed download reports the thread's sentence, not "not installed": the state comes from
    /// the service's own temporary root, so this is unconditional.
    #[test]
    fn a_failed_download_keeps_the_threads_sentence() {
        let dir = crate::test_temp::TempDir::new("download-sentence");
        let mut s = Service::with_root(dir.path().to_path_buf(), 0);
        let shared = Arc::new(Mutex::new(ModelState::Failed { detail: "download failed: connection reset".into() }));
        s.download = Some(std::thread::spawn(|| {}));
        s.download_state = Some(Arc::clone(&shared));
        match s.state() {
            ModelState::Failed { detail } => assert!(detail.contains("connection reset"), "{detail}"),
            other => panic!("{other:?}"),
        }
    }

    /// A model the hash check accepts but RTen cannot load is remembered as the state, so the
    /// next frame does not re-hash and re-parse the file. The load runs on the worker thread, so
    /// the failure becomes visible a moment after the first call.
    #[test]
    fn a_model_that_cannot_load_is_not_retried_on_every_frame() {
        let dir = crate::test_temp::TempDir::new("load");
        let root = dir.path().to_path_buf();
        let path = deckcraft_matte::manifest::MANIFEST.path_in(&root);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(&path, b"not the model").expect("file");
        let mut s = Service::with_root(root, 0);
        s.state = ModelState::Ready { path: path.display().to_string(), bytes: 13 };
        let f = RgbaFrame::solid(8, 8, [1, 2, 3, 255]).expect("frame");
        let opts = CompositeOptions::new(deckcraft_model::CameraBackgroundMode::Removed);
        let mut first = None;
        for _ in 0..200 {
            match s.process(&f, &opts, None) {
                Err(e) => {
                    first = Some(e.message());
                    break;
                }
                Ok(_) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        let first = first.expect("the worker's load failure is reported");
        assert!(first.contains("bytes") || first.contains("unusable"), "{first}");
        assert!(matches!(&s.state, ModelState::Failed { .. }), "the load failure is the model state now: {:?}", s.state);
        std::fs::remove_file(&path).expect("remove");
        let second = s.process(&f, &opts, None).expect_err("still remembered").message();
        assert_eq!(first, second, "the failure is cached, not re-probed");
    }
}
