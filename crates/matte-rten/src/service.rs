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
    interval: Duration,
    feather: u32,
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
        Service {
            root,
            state,
            worker: None,
            download: None,
            download_state: None,
            interval: Duration::from_millis(interval_ms.clamp(0, 60_000)),
            // One stage owns the feather: the compositor (`CompositeOptions`), not the model.
            feather: 0,
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
        // A download failure sentence stays until the user acts on it: a probe that finds no
        // file must not replace it with "not installed". A model that appeared does win.
        if matches!(self.state, ModelState::Failed { .. }) && !state.is_ready() {
            return;
        }
        self.state = state;
    }

    fn ensure_worker(&mut self) -> Result<(), MatteError> {
        if self.worker.is_some() {
            return Ok(());
        }
        let path = match &self.state {
            ModelState::Ready { path, .. } => path.clone(),
            other => return Err(MatteError::Unavailable(other.message())),
        };
        // A model RTen cannot load (a bad DECKCRAFT_MATTE_THREADS value, a file the hash check
        // accepted but the parser rejects) must not be re-hashed and re-parsed on every frame:
        // remember the sentence as the model state until `refresh()` or `install()` runs again.
        let segmenter = match RtenSegmenter::load(Path::new(&path)) {
            Ok(s) => s.with_feather(self.feather),
            Err(detail) => {
                self.state = ModelState::Failed { detail: format!("{path}: {detail}") };
                return Err(MatteError::Unavailable(self.state.message()));
            }
        };
        self.worker = Some(MatteWorker::new(Box::new(segmenter) as Box<dyn Segmenter>, self.interval));
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
        // The thread records why the download failed. Probing the disk cannot tell a network
        // error, a size mismatch or a hash failure from "never installed" — both leave no
        // file — so keep the failure sentence for the panel.
        let failure =
            self.download_state.take().and_then(|shared| shared.lock().ok().map(|s| s.clone())).filter(|s| matches!(s, ModelState::Failed { .. }));
        self.refresh();
        if !self.state.is_ready()
            && let Some(failed) = failure
        {
            if let ModelState::Failed { detail } = &failed {
                log::warn!("camera background model download did not install: {detail}");
            }
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
        self.state = ModelState::Ready { path: target.display().to_string(), bytes: deckcraft_matte::manifest::MODEL_BYTES };
        self.worker = None;
        Ok(())
    }

    fn start_download(&mut self) -> Result<(), String> {
        if self.download.is_some() {
            return Ok(());
        }
        let root = self.root.clone();
        let shared = Arc::new(Mutex::new(ModelState::Downloading { received: 0, total: deckcraft_matte::manifest::MODEL_BYTES }));
        let progress = Arc::clone(&shared);
        let handle = std::thread::Builder::new()
            .name("deckcraft-matte-download".into())
            .spawn(move || {
                let mut on_progress = |received: u64, total: u64| {
                    if let Ok(mut s) = progress.lock() {
                        *s = ModelState::Downloading { received, total };
                    }
                };
                if let Err(e) = model::download_to(&root, &mut on_progress)
                    && let Ok(mut s) = progress.lock()
                {
                    *s = ModelState::Failed { detail: e };
                }
            })
            .map_err(|e| format!("cannot start the download: {e}"))?;
        self.download = Some(handle);
        self.download_state = Some(shared);
        self.state = ModelState::Downloading { received: 0, total: deckcraft_matte::manifest::MODEL_BYTES };
        Ok(())
    }

    fn process(&mut self, frame: &RgbaFrame, options: &CompositeOptions, replacement: Option<&RgbaFrame>) -> Result<Option<RgbaFrame>, MatteError> {
        if options.mode == deckcraft_model::CameraBackgroundMode::Normal {
            return Ok(Some(frame.clone()));
        }
        self.poll_download();
        self.ensure_worker()?;
        let worker = self.worker.as_ref().ok_or_else(|| MatteError::Unavailable("worker did not start".into()))?;
        worker.process(frame, options, replacement, true)
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
        self.worker.as_ref().and_then(MatteWorker::latest)
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

    /// One stage owns the feather: the compositor's `CompositeOptions.feather`. The segmenter must
    /// not soften the matte too, or `feather: 0` could never give a hard edge.
    #[test]
    fn the_service_leaves_feathering_to_the_compositor() {
        assert_eq!(Service::new(0).feather, 0);
    }

    /// `install` and a running download both write `model.onnx.part`; racing them can install a
    /// truncated file. A finished download must not block a later install.
    #[test]
    fn install_is_refused_while_a_download_runs() {
        let mut s = Service::new(0);
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

    #[test]
    fn a_missing_model_is_named_and_a_normal_frame_needs_no_model() {
        let mut s = Service::new(0);
        match s.state() {
            ModelState::Ready { .. } => {}
            other => assert!(other.message().contains("not installed") || other.message().contains("unusable"), "{other:?}"),
        }
        let f = RgbaFrame::solid(8, 8, [1, 2, 3, 255]).expect("frame");
        let out = s.process(&f, &CompositeOptions::new(deckcraft_model::CameraBackgroundMode::Normal), None).expect("normal").expect("returned");
        assert_eq!(out, f);
    }

    /// A download that fails leaves no file, so probing the disk cannot say why: the panel must
    /// still show the thread's sentence instead of "not installed".
    #[test]
    fn a_failed_download_keeps_the_threads_sentence() {
        let mut s = Service::new(0);
        if s.state().is_ready() {
            return; // a machine with the model installed always probes Ready; nothing to prove
        }
        let shared = Arc::new(Mutex::new(ModelState::Failed { detail: "download failed: connection reset".into() }));
        s.download = Some(std::thread::spawn(|| {}));
        s.download_state = Some(Arc::clone(&shared));
        match s.state() {
            ModelState::Failed { detail } => assert!(detail.contains("connection reset"), "{detail}"),
            other => panic!("{other:?}"),
        }
    }

    /// A model the hash check accepts but RTen cannot load is remembered as the state, so the
    /// next frame does not re-hash and re-parse the file.
    #[test]
    fn a_model_that_cannot_load_is_not_retried_on_every_frame() {
        let dir = std::env::temp_dir().join(format!("deckcraft-matte-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("model.onnx");
        std::fs::write(&path, b"not the model").expect("file");
        let mut s = Service::new(0);
        s.state = ModelState::Ready { path: path.display().to_string(), bytes: 13 };
        let f = RgbaFrame::solid(8, 8, [1, 2, 3, 255]).expect("frame");
        let opts = CompositeOptions::new(deckcraft_model::CameraBackgroundMode::Removed);
        let first = s.process(&f, &opts, None).expect_err("unloadable").message();
        assert!(first.contains("unusable"), "{first}");
        assert!(!s.state.is_ready(), "the load failure is the model state now");
        std::fs::remove_file(&path).expect("remove");
        let second = s.process(&f, &opts, None).expect_err("still remembered").message();
        assert_eq!(first, second, "the failure is cached, not re-probed");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
