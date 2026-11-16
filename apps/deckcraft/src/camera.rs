//! System camera capture for the Cameo live preview, over the `cameras` crate (Media Foundation,
//! AVFoundation, V4L2). The sessions, errors and frames cross into the UI as the neutral
//! [`deckcraft_media::camera`] types; nothing above this file names a platform API.
//!
//! The worker is created on the first `start`, because enumeration is what triggers the macOS
//! permission sheet. One worker serves every session: `start` replaces the running camera and
//! `stop` drops it, so turning the preview off and on again never races another open for the same
//! device. Nothing here logs from a capture path and nothing joins a worker on the UI thread.
//!
//! Only the desktop OSes build this module (`main` gates it the same way): anywhere else no
//! backend is injected, so the UI disables the camera buttons instead of offering a camera that
//! always reports `Unsupported`.

mod native;
pub use native::SystemCamera;
