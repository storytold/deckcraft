//! The host-facing seam: model state for the Camera Format panel, and a service a native build
//! supplies. The UI never runs inference itself; [`MatteService::process`] submits the frame to a
//! worker and returns the composite built from the latest matte.

use crate::MatteError;
use crate::compositing::CompositeOptions;
use crate::frame::{Matte, RgbaFrame};
use crate::manifest::{ACCELERATOR_UNAVAILABLE, Accelerator};

/// What the Camera Format panel can say about the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelState {
    /// The file is installed and verified.
    Ready { path: String, bytes: u64 },
    /// No usable file. `root` is the first model directory searched; the rest is what the
    /// install panel shows.
    Missing { root: String, url: String, bytes: u64, sha256: String, license: String },
    /// A download is running. `total` is 0 until the server sends a length.
    Downloading { received: u64, total: u64 },
    /// The file exists but is the wrong size or hash.
    Failed { detail: String },
}

impl ModelState {
    /// A sentence for the panel.
    pub fn message(&self) -> String {
        match self {
            ModelState::Ready { bytes, .. } => format!("Background model ready ({})", format_bytes(*bytes)),
            ModelState::Missing { .. } => "Background model not installed".into(),
            ModelState::Downloading { received, total } => {
                if *total == 0 {
                    format!("Downloading background model… {}", format_bytes(*received))
                } else {
                    format!("Downloading background model… {}%", (received.saturating_mul(100) / total.max(&1)).min(100))
                }
            }
            ModelState::Failed { detail } => format!("Background model unusable: {detail}"),
        }
    }

    /// The model can segment frames right now.
    pub fn is_ready(&self) -> bool {
        matches!(self, ModelState::Ready { .. })
    }
}

/// How the background worker is doing, for the panel's status line and the docs' measurements.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timings {
    /// Milliseconds the last completed inference took.
    pub last_ms: u64,
    /// Completed inferences since the service started.
    pub runs: u64,
    /// Frames drawn from a cached matte because the next inference had not finished (or the
    /// cadence had not elapsed).
    pub reused: u64,
}

/// The host's camera-background service.
pub trait MatteService: Send {
    fn state(&mut self) -> ModelState;
    /// Re-read the model directory: a host calls this when the panel opens, so a model the user
    /// copied in while the app ran is noticed. A no-op for a host with no model of its own, and
    /// never while a download runs.
    fn refresh(&mut self) {}
    /// Copy a file the user picked into the model directory and verify it. Never leaves a partial
    /// file behind.
    fn install(&mut self, path: &str) -> Result<(), String>;
    /// Start the consented download on a worker; returns before any network work.
    fn start_download(&mut self) -> Result<(), String>;
    /// Stop a running download. A no-op for a host with no download of its own; the transfer fails
    /// on its next read and removes its partial file.
    fn cancel_download(&mut self) {}
    /// Composite one frame with the mode's options. `None` is returned until the first matte is
    /// ready (Normal never waits: it returns the frame unchanged).
    fn process(&mut self, frame: &RgbaFrame, options: &CompositeOptions, replacement: Option<&RgbaFrame>) -> Result<Option<RgbaFrame>, MatteError>;
    fn timings(&self) -> Timings;
    /// Why segmentation cannot run, when it cannot: the sentence the UI shows on a disabled
    /// control.
    fn unavailable_reason(&mut self) -> Option<String>;
    /// The latest matte, for tests and diagnostics.
    fn last_matte(&mut self) -> Option<Matte>;
    /// The accelerator inference uses now.
    fn accelerator(&self) -> Accelerator {
        Accelerator::Cpu
    }
    /// Choose an accelerator. The CPU path is always available; anything else is refused with
    /// [`ACCELERATOR_UNAVAILABLE`] until a separate backend lands.
    fn set_accelerator(&mut self, accelerator: Accelerator) -> Result<(), String> {
        if accelerator == Accelerator::Cpu { Ok(()) } else { Err(ACCELERATOR_UNAVAILABLE.to_string()) }
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} kB", bytes as f64 / 1_000.0)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_read_as_sentences_and_only_ready_is_ready() {
        let ready = ModelState::Ready { path: "m.onnx".into(), bytes: 191_877_254 };
        assert!(ready.is_ready());
        assert!(ready.message().contains("191.9 MB"), "{}", ready.message());
        let missing =
            ModelState::Missing { root: "models".into(), url: "https://example".into(), bytes: 1, sha256: "0".into(), license: "MIT".into() };
        assert!(!missing.is_ready());
        assert!(missing.message().contains("not installed"));
        let downloading = ModelState::Downloading { received: 50, total: 100 };
        assert!(downloading.message().contains("50%"), "{}", downloading.message());
        assert!(ModelState::Downloading { received: 5, total: 0 }.message().contains("5 B"));
        assert!(ModelState::Failed { detail: "hash".into() }.message().contains("hash"));
    }
}
