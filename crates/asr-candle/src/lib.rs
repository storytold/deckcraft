//! DeckCraft's native live-subtitle backend: Whisper (`candle`) on the CPU, a cpal microphone,
//! and the model loader for the files the user installs.
//!
//! This crate is native only (layer 6, skipped by the wasm check): candle, cpal and the
//! 151 MB weights have no browser build. The pure protocol, caption layout and manifest live in
//! `deckcraft-asr`.
//!
//! Model weights are never shipped. [`deckcraft_asr::model::whisper_tiny`] describes the files
//! and their SHA-256 hashes; the user downloads them through the Subtitle Settings dialog or
//! places them in the documented model directory.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod capture;
pub mod engine;
pub mod mel;
pub mod provider;

pub use capture::input_devices;
pub use engine::WhisperEngine;
pub use provider::{WhisperTranscriber, capabilities, detected_accelerators};
