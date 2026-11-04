//! System camera capture for the Cameo live preview, over the `cameras` crate (Media Foundation,
//! AVFoundation, V4L2). The sessions, errors and frames cross into the UI as the neutral
//! [`deckcraft_media::camera`] types; nothing above this file names a platform API.
//!
//! The worker is created on the first `start`, because enumeration is what triggers the macOS
//! permission sheet. One worker serves every session: `start` replaces the running camera and
//! `stop` drops it, so turning the preview off and on again never races another open for the same
//! device. Nothing here logs from a capture path and nothing joins a worker on the UI thread.

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
mod native;
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub use native::SystemCamera;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod stub;
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub use stub::SystemCamera;
