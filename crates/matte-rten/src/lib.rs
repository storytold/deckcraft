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

pub mod model;
pub mod service;
pub mod worker;

pub use model::{RtenSegmenter, download_to, install_from, model_roots, probe};
pub use service::Service;
pub use worker::MatteWorker;
