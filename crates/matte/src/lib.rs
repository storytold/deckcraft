//! Camera backgrounds for a Cameo: the model manifest, the pre/post-processing around a
//! segmentation model, the compositing for PowerPoint's four background modes, and the
//! [`Segmenter`] seam a native backend implements.
//!
//! This crate is pure and wasm-clean: it moves pixels and bytes, and never touches the network,
//! the filesystem beyond the manifest's own verification helper, or a platform API. The model
//! itself (`birefnet-lite-512`, MIT, 191,877,254 bytes) is downloaded by the user and never
//! committed; [`manifest`] records where it comes from and what it must hash to.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod compositing;
pub mod frame;
pub mod manifest;
pub mod process;
pub mod segmenter;
pub mod service;

pub use compositing::{CompositeOptions, composite, composite_prepared, prepare_matte};
pub use frame::{MAX_DIM, MAX_PIXELS, Matte, RgbaFrame};
pub use manifest::{
    ACCELERATOR_UNAVAILABLE, Accelerator, MANIFEST, MODEL_ID, Manifest, ManifestError, SUPPORTED_ACCELERATORS, read_verified, verify_file,
};
pub use segmenter::{EllipseSegmenter, NoSegmenter, Segmenter};
pub use service::{MatteService, ModelState, Timings};

/// Everything that can go wrong while preparing, segmenting or compositing a frame.
#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum MatteError {
    /// A frame, matte or replacement has a zero side, or a pixel count that does not fit its
    /// dimensions.
    #[error("invalid image: {0}")]
    InvalidImage(String),
    /// The image is larger than the bounded pipeline accepts.
    #[error("image is too large: {width}x{height} (limit {limit} pixels)")]
    TooLarge { width: u32, height: u32, limit: u32 },
    /// The model output does not have the shape the manifest promises.
    #[error("unexpected model output: {0}")]
    BadOutput(String),
    /// No model is installed, or the backend is not built for this target.
    #[error("{0}")]
    Unavailable(String),
    /// The backend failed while running the model.
    #[error("segmentation failed: {0}")]
    Inference(String),
}

impl MatteError {
    /// A sentence the UI can show: what the user can do about it.
    pub fn message(&self) -> String {
        match self {
            MatteError::Unavailable(m) => m.clone(),
            other => other.to_string(),
        }
    }
}
