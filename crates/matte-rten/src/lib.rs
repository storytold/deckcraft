//! The native camera-background backend: model discovery, a verified install, a consented
//! download, and RTen inference on a worker thread with a reduced frame cadence.
//!
//! The model is `birefnet-lite-512` (MIT, 191,877,254 bytes). It is never committed and never
//! loaded at startup unless the user already installed it. Inference is CPU-only and measured at
//! about 2.1 s/frame on this class of machine, so it runs off the UI thread: the compositor keeps
//! the last matte until a new one arrives and reports [`deckcraft_matte::Timings`].
//!
//! This crate is layer 6 (native), like the UI. wasm builds use the pure pipeline in
//! `deckcraft-matte` and the unavailable service.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// A temporary directory for tests: unique per call, removed on `Drop` even when an assertion
/// fails, and cleared first so a stale one from an earlier run cannot leak in.
#[cfg(test)]
pub(crate) mod test_temp {
    use std::path::{Path, PathBuf};

    pub(crate) struct TempDir(PathBuf);

    impl TempDir {
        pub(crate) fn new(tag: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("deckcraft-matte-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            let _ = std::fs::create_dir_all(&path);
            TempDir(path)
        }

        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

pub mod model;
pub mod service;
pub mod worker;

pub use model::{DOWNLOAD_TIMEOUT, DownloadControl, RtenSegmenter, download_to, install_from, model_roots, probe};
pub use service::Service;
pub use worker::MatteWorker;
