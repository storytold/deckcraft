//! Live captions and subtitles for DeckCraft's slide show.
//!
//! This crate is the part of the feature that has no platform or model dependency, so it is
//! checked for `wasm32-unknown-unknown` like every other layer 2 crate:
//!
//! - [`provider`]: the [`Transcriber`] protocol a speech engine implements (start, stop, status,
//!   partial and final lines) and a deterministic [`FakeTranscriber`] for tests.
//! - [`audio`]: microphone buffers, channel downmix and linear resampling to the 16 kHz mono the
//!   models take.
//! - [`caption`]: where a caption goes on screen (four positions), how much it wraps, and the
//!   geometry the slide show draws.
//! - [`settings`]: the per-user subtitle settings (never deck state).
//! - [`model`]: the model manifest (id, revision, files, sizes, SHA-256, licence, source URL),
//!   verification and the documented model directories.
//!
//! Nothing here reads audio, loads a model or draws: the native backend is `deckcraft-asr-candle`
//! and the UI is `deckcraft-ui-egui`. Every function treats its input as hostile (NaN, huge or
//! negative sizes, damaged files) and never panics.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod accelerator;
pub mod audio;
pub mod caption;
pub mod model;
pub mod provider;
pub mod settings;

pub use accelerator::{Accelerator, Capabilities, resolve as resolve_accelerator};
pub use audio::{AudioBuffer, TARGET_RATE};
pub use caption::{CaptionLayout, LayoutInput, Position, Rect};
pub use model::{ModelFile, ModelManifest, ModelRoots, ModelState, ModelStatus, WHISPER_TINY_ID, whisper_tiny};
pub use provider::{AsrError, Event, FakeTranscriber, ProviderState, ProviderStatus, Transcriber};
pub use settings::{LANGUAGE_CODES, SubtitleSettings, is_known_language};
