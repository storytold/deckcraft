//! Co-editing (plugin protocol 2): the presentation as a shared Yjs document.
//!
//! DeckCraft schema 1 (normative in `docs/collab.md`) maps a presentation onto Yjs shared types:
//! root `meta` `{app, schema, title}`, root `slides` (an array of slide ids), a root map
//! `slide:<id>` per slide (its properties, the array `shapes` of its top-level shape ids, a map
//! `shape:<id>` per shape with its properties and its text as Yjs text, the notes as Yjs text),
//! `presentation` (presentation-wide settings), `masters`, `layouts`, `theme` and `media`.
//!
//! A [`Replica`] keeps one such document in step with the engine's presentation:
//! [`Replica::seed`] shares a presentation, [`Replica::commit`] turns each committed command into
//! one transaction (typing becomes character inserts, so concurrent typing merges),
//! [`Replica::apply_update`] and [`Replica::refresh`] bring remote changes back into the model,
//! and [`Replica::undo`] is the per-user undo.
//!
//! [`crypto`] is the end-to-end encryption of section 4 of the protocol, [`awareness`] the
//! y-protocols awareness encoding (who's there and what they have selected). Nothing here knows
//! about plugins, processes or the UI.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod awareness;
mod chat;
pub mod crypto;
pub mod json;
mod replica;
pub mod text;

pub use replica::{Refreshed, Replica, TextAt, fingerprint, reseed_ids};

/// The wire format (`host.collab.formats`).
pub const FORMAT: &str = "yjs-v1";
/// This app in the shared document's `meta.app`.
pub const APP: &str = "deckcraft";
/// The document schema this build reads and writes (`meta.schema`).
pub const SCHEMA: u32 = 1;
/// Largest update sent or applied, before base64 (protocol 2, section 7).
pub const MAX_UPDATE: usize = 16 << 20;
/// Largest awareness update.
pub const MAX_AWARENESS: usize = 64 << 10;

/// Why a collab operation failed. Never a panic: a bad update is reported and dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollabError {
    /// The update (or state vector, or awareness message) can't be decoded or applied.
    Corrupt(String),
    /// The room holds another app's document, or another schema of ours.
    Mismatch {
        app: String,
        schema: u64,
    },
    /// Decryption failed: wrong key, tampering, or the message was meant for another room.
    Decrypt,
    /// Larger than the protocol allows.
    TooLarge(usize),
    /// The room's state hasn't arrived yet.
    NotReady,
    Other(String),
}

impl std::fmt::Display for CollabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CollabError::Corrupt(m) => write!(f, "a damaged update was dropped: {m}"),
            CollabError::Mismatch { app, schema } => write!(f, "This room is for {app} {schema}"),
            CollabError::Decrypt => write!(f, "a message that can't be decrypted was dropped"),
            CollabError::TooLarge(n) => write!(f, "a message of {n} bytes is larger than the 16 MiB allowed"),
            CollabError::NotReady => write!(f, "the room's document hasn't arrived yet"),
            CollabError::Other(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for CollabError {}

/// Standard base64 with padding (protocol 2's binary payloads).
pub fn b64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Decodes standard base64 (padding optional).
pub fn b64_decode(text: &str) -> Result<Vec<u8>, CollabError> {
    use base64::Engine;
    let t = text.trim();
    base64::engine::general_purpose::STANDARD
        .decode(t)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(t.trim_end_matches('=')))
        .map_err(|e| CollabError::Corrupt(format!("base64: {e}")))
}

/// Fills `buf` with random bytes from the operating system. The browser build has no source
/// here (and no transports): it fails there.
pub fn random_bytes(buf: &mut [u8]) -> Result<(), CollabError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        getrandom::getrandom(buf).map_err(|e| CollabError::Other(format!("no random numbers: {e}")))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = buf;
        Err(CollabError::Other("live sessions need the desktop app".into()))
    }
}

/// A fresh Yjs client id: random, below 2^53 (JavaScript-safe), never 0.
pub fn new_client_id() -> u64 {
    let mut b = [0u8; 8];
    let v = if random_bytes(&mut b).is_ok() { u64::from_le_bytes(b) } else { yrs::block::ClientID::random().get() };
    (v & ((1u64 << 53) - 1)).max(1)
}

/// A small deterministic generator (SplitMix64), seeded randomly unless asked not to.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// Seeded from `seed` and the system's randomness.
    pub fn new(seed: u64) -> Rng {
        let mut b = [0u8; 8];
        let extra = if random_bytes(&mut b).is_ok() { u64::from_le_bytes(b) } else { 0 };
        Rng(seed ^ extra ^ 0x9E37_79B9_7F4A_7C15)
    }
    /// Seeded from `seed` only (tests, reproducible fuzzing).
    pub fn seeded(seed: u64) -> Rng {
        Rng(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// A number in `lo..hi` (`lo` when the range is empty).
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo { lo } else { lo + self.next_u64() % (hi - lo) }
    }
}

/// Ids of slides, shapes, layouts, masters and media in the shared document: the base-36 form
/// (`0-9a-z`), zero-padded to 8 characters, of the object's 32-bit number (schema 1, "Ids"). The
/// same number is the object's id in the model, so references inside property values (a
/// connector's ends, an animation's shape, a link to a slide, a picture's media) are plain
/// numbers that mean the same on every peer.
pub fn id_str(n: u32) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut v = u64::from(n);
    let mut out = [b'0'; 8];
    for slot in out.iter_mut().rev() {
        *slot = DIGITS.get((v % 36) as usize).copied().unwrap_or(b'0');
        v /= 36;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The number of an id written by [`id_str`]; `None` for anything else (an id another
/// implementation made up, which readers skip).
pub fn parse_id(s: &str) -> Option<u32> {
    if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase()) {
        return None;
    }
    let v = u64::from_str_radix(s, 36).ok()?;
    u32::try_from(v).ok().filter(|v| *v != 0)
}

#[cfg(test)]
mod tests;
