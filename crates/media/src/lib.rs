//! DeckCraft media: everything between embedded media bytes and the speakers / the screen.
//!
//! - [`probe`]: container, duration, audio channels / sample rate, video size / frame rate.
//! - [`audio::decode`]: every common audio format to interleaved f32 [`audio::Pcm`] — MP3, AAC
//!   (M4A, ADTS), ALAC, WAV, AIFF, CAF, FLAC, Ogg Vorbis, Opus (Ogg / WebM / Matroska) and the audio
//!   track of MP4/MOV/WebM videos. WMA (ASF) is recognised and probed but not decoded.
//! - [`video::VideoDecoder`]: H.264, HEVC, VP9 and AV1 in MP4/MOV and WebM/Matroska to RGBA
//!   [`video::Frame`]s (our own decoders, ported from FilmCraft); [`video::poster_png`].
//! - [`clip::ClipParams`]: trim, fade in/out and volume.
//! - [`player::Player`]: the playback mixer and clock the UI host drives, over an
//!   [`player::AudioOut`] it injects (cpal on desktop; none on the web or in tests, where the clock
//!   runs on wall time); [`feed::VideoFeed`] decodes video ahead of that clock.
//!
//! Layer L2: depends only on the codec crates, never on the model or the UI. Builds for
//! `wasm32-unknown-unknown` (no threads there: decoding runs inline).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod audio;
pub mod clip;
pub mod feed;
mod mp4guard;
pub mod player;
pub mod probe;
pub mod video;
mod yuv;

use std::sync::Arc;

pub use audio::Pcm;
pub use clip::ClipParams;
pub use feed::VideoFeed;
pub use player::{AudioOut, PlayState, Player, VoiceSpec, VoiceStatus};
pub use probe::{AudioInfo, MediaInfo, VideoInfo, probe};
pub use video::{Frame, VideoDecoder, poster_frame, poster_png};

/// Shared media bytes (the model's `MediaItem::data`).
pub type Bytes = Arc<Vec<u8>>;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MediaError {
    #[error("{0} isn't supported")]
    Unsupported(String),
    #[error("the media can't be read: {0}")]
    Corrupt(String),
    #[error("the file has no {0}")]
    Missing(&'static str),
}

pub type Result<T> = std::result::Result<T, MediaError>;

/// Container formats recognised from the first bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    /// ISO BMFF: MP4, M4A, MOV, 3GP.
    Mp4,
    /// Matroska / WebM.
    Matroska,
    Ogg,
    Wav,
    Aiff,
    Caf,
    Flac,
    Mp3,
    /// AAC in ADTS frames (`.aac`).
    Adts,
    /// Advanced Systems Format (WMA / WMV).
    Asf,
    Unknown,
}

/// The ASF header object GUID (30 26 B2 75 8E 66 CF 11 A6 D9 00 AA 00 62 CE 6C).
pub(crate) const ASF_HEADER: [u8; 16] = [0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C];

impl Container {
    pub fn sniff(b: &[u8]) -> Container {
        let at = |o: usize, m: &[u8]| b.get(o..o + m.len()) == Some(m);
        if at(4, b"ftyp") || at(4, b"moov") || at(4, b"mdat") || at(4, b"wide") || at(4, b"free") {
            Container::Mp4
        } else if at(0, &[0x1A, 0x45, 0xDF, 0xA3]) {
            Container::Matroska
        } else if at(0, b"OggS") {
            Container::Ogg
        } else if (at(0, b"RIFF") || at(0, b"RF64")) && at(8, b"WAVE") {
            Container::Wav
        } else if at(0, b"FORM") && (at(8, b"AIFF") || at(8, b"AIFC")) {
            Container::Aiff
        } else if at(0, b"caff") {
            Container::Caf
        } else if at(0, b"fLaC") {
            Container::Flac
        } else if at(0, &ASF_HEADER) {
            Container::Asf
        } else if at(0, b"ID3") {
            // ID3v2 is followed by MP3 frames (or, rarely, ADTS). The chain of tags is walked
            // iteratively — a crafted file can carry thousands of empty tags — and only the
            // bytes after the last one are sniffed.
            let mut rest = b;
            while rest.starts_with(b"ID3") {
                let size = rest.get(6..10).map(|s| s.iter().fold(0usize, |a, &x| (a << 7) | (x & 0x7f) as usize)).unwrap_or(0);
                rest = rest.get(10 + size..).unwrap_or(&[]);
            }
            match Container::sniff(rest) {
                Container::Adts => Container::Adts,
                _ => Container::Mp3,
            }
        } else if b.len() > 1 && b[0] == 0xFF && b[1] & 0xF0 == 0xF0 && b[1] & 0x06 == 0 {
            Container::Adts
        } else if b.len() > 1 && b[0] == 0xFF && b[1] & 0xE0 == 0xE0 && b[1] & 0x06 != 0 {
            Container::Mp3
        } else {
            Container::Unknown
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Matroska => "matroska",
            Container::Ogg => "ogg",
            Container::Wav => "wav",
            Container::Aiff => "aiff",
            Container::Caf => "caf",
            Container::Flac => "flac",
            Container::Mp3 => "mp3",
            Container::Adts => "adts",
            Container::Asf => "asf",
            Container::Unknown => "unknown",
        }
    }
}

/// Worker threads are available (not on wasm32).
pub(crate) const THREADS: bool = !cfg!(target_arch = "wasm32");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id3_tag_chains_do_not_recurse_the_stack() {
        // 2 MB of empty ID3v2 tags (10 bytes each) chained ahead of the MP3 frames: walking
        // the chain must not cost one stack frame per tag.
        let mut b = Vec::with_capacity(200_000 * 10 + 2);
        for _ in 0..200_000 {
            b.extend_from_slice(&[b'I', b'D', b'3', 4, 0, 0, 0, 0, 0, 0]);
        }
        b.extend_from_slice(&[0xFF, 0xFB]); // an MP3 frame sync
        assert_eq!(Container::sniff(&b), Container::Mp3);
    }
}
