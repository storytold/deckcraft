//! Bit-level reading and writing for DeckCraft's codec crates: a re-export of
//! [`craft_media::bitstream`] from craft-libs, plus [`budget`].
//!
//! - [`BitReader`]: MSB-first reader with Exp-Golomb (`ue(v)`, `se(v)`), as used by H.264/HEVC/AV1 headers.
//! - [`BitWriter`]: the matching writer (for encoders and header rewriting).
//! - [`unescape_rbsp`] / [`escape_rbsp`]: remove/insert H.264/HEVC emulation-prevention bytes.
//! - [`annexb_nals`] / [`length_prefixed_nals`]: split Annex-B byte streams and AVCC/HVCC samples into NAL units.
//! - [`budget`]: the memory budget video decoders check their picture store against.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

pub use craft_media::bitstream::*;

pub mod budget;
