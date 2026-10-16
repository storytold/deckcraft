//! Ported from FilmCraft (`crates/aac`, commit cee1feb), same authors and licence.
//!
//! Clean-room AAC-LC encoder and decoder (ISO/IEC 14496-3, MPEG-4 Audio, Low Complexity profile).
//!
//! - [`Encoder`]: planar f32 → raw access units (+ [`Encoder::audio_specific_config`] for MP4 `esds`,
//!   optional ADTS framing). Psychoacoustic model, window switching, M/S, TNS, bit reservoir, CBR/VBR,
//!   1–8 channels.
//! - [`Decoder`]: `AudioSpecificConfig` + raw access units → planar f32. Supports M/S, intensity
//!   stereo, PNS, TNS, pulse data, both window shapes and every window sequence.
//!
//! HE-AAC (SBR/PS), Main/SSR/LTP profiles, coupling channels and 960-sample frames are not supported;
//! for explicitly signalled HE-AAC only the AAC-LC core is decoded (at the core rate).
//!
//! Layer L0: depends only on `deckcraft-bitstream` (+ `thiserror`, optional `rayon`); no `unsafe`;
//! builds for `wasm32-unknown-unknown`.
//!
//! ```
//! use deckcraft_aac::{Decoder, Encoder, EncoderConfig};
//!
//! let mut enc = Encoder::new(EncoderConfig::cbr(48_000, 2, 128_000))?;
//! let left: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
//! let right = left.clone();
//! let mut aus = enc.encode(&[&left, &right]); // 0..n access units
//! aus.extend(enc.flush());
//! let asc = enc.audio_specific_config(); // for the MP4 `esds`
//! assert_eq!(enc.priming_samples(), 1024); // MP4 edit list media_time
//!
//! let mut dec = Decoder::new(&asc)?;
//! for au in &aus {
//!     let pcm = dec.decode(au)?; // planar, 1024 samples per channel
//!     assert_eq!(pcm.len(), 2);
//! }
//! # Ok::<(), deckcraft_aac::Error>(())
//! ```

// Index loops over parallel per-band arrays mirror the spec text and stay as in FilmCraft so
// the port can be re-synced.
#![allow(clippy::needless_range_loop)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

mod config;
mod decoder;
mod encoder;
mod huffman;
mod huffman_tables;
mod ics;
mod mdct;
mod tables;
mod tns;

pub use config::{AdtsHeader, AudioSpecificConfig, ElementType, ProgramConfig, config_layout, split_adts};
pub use decoder::Decoder;
pub use encoder::{BitrateMode, Encoder, EncoderConfig, EncoderStats};
pub use mdct::WindowShape;
pub use tables::{SAMPLE_RATES, sample_rate_index};

/// Samples per AAC-LC frame (per channel).
pub const FRAME_LEN: usize = 1024;

/// Errors from configuration parsing, encoding and decoding.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("invalid configuration: {0}")]
    InvalidConfig(&'static str),
    #[error("unsupported feature: {0}")]
    Unsupported(&'static str),
    #[error("invalid bitstream: {0}")]
    Bitstream(&'static str),
    #[error("unexpected end of access unit")]
    Eof,
}

impl From<deckcraft_bitstream::BitError> for Error {
    fn from(_: deckcraft_bitstream::BitError) -> Self {
        Error::Eof
    }
}

pub type Result<T> = std::result::Result<T, Error>;
