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
            feather: 2,
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
        if self.download.is_none() {
            self.state = state;
        }
    }

    fn ensure_worker(&mut self) -> Result<(), MatteError> {
        if self.worker.is_some() {
            return Ok(());
        }
        let ModelState::Ready { path, .. } = &self.state else {
            return Err(MatteError::Unavailable(self.state.message()));
        };
        let segmenter = RtenSegmenter::load(Path::new(path)).map_err(MatteError::Unavailable)?.with_feather(self.feather);
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
        self.download_state = None;
        // The worker either installed the file or wrote a failure; probing tells them apart.
        self.refresh();
        if !self.state.is_ready()
            && let ModelState::Failed { detail } = &self.state
        {
            log::warn!("camera background model download did not install: {detail}");
        }
    }
}

impl MatteService for Service {
    fn state(&mut self) -> ModelState {
        self.poll_download();
        self.state.clone()
    }

    fn install(&mut self, path: &str) -> Result<(), String> {
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
}
