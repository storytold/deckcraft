//! Screen capture: the host contract and the screen-recording encoder.
//!
//! The UI host provides the platform capture (native screen APIs behind [`ScreenIn`]); the
//! engine owns the recording: [`ScreenRecorder`] turns BGRA frames into H.264 NAL units and
//! muxes them into an MP4 with [`deckcraft_isobmff::Mp4Writer`]. This crate is platform-neutral
//! and builds for `wasm32`; [`NoScreen`] is the stub a host without capture installs.
//!
//! Layer L2: depends only on the codec crates, never on the model, the engine or the UI.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod frame;
mod recorder;

pub use frame::{CaptureOptions, CaptureRegion, CaptureStatus, CaptureTarget, MIN_REGION, NoScreen, ScreenError, ScreenFrame, ScreenIn, TargetKind};
pub use recorder::{MAX_FRAMES, MAX_SIDE, RecorderConfig, RecorderError, RecorderOutput, RecorderSink, ScreenRecorder};
