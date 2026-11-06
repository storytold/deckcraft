//! DeckCraft's collaboration adapter.
//!
//! Turns the normalised operations of `.craft/protocol/ARCHITECTURE.md` into engine commands,
//! answers with an authoritative delta, and keeps session-scoped keys and tombstones for one
//! document session (ADR 0001–0004, 0010).
//!
//! No networking and no UI live here: the transport is a separate crate, so this one is testable
//! headless with two `Session`s in one process (`cargo test -p deckcraft-collab`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod apply;
pub mod client;
pub mod delta;
pub mod history;
pub mod keys;
pub mod presence;
pub mod session;

pub use apply::{Adapter, Rejection, class_of};
pub use client::{ClientSession, Effect, Link, SubmitError};
pub use delta::delta;
pub use history::{ActorHistory, Histories, Image, Snapshot, Step};
pub use keys::{KeyIssuer, KeyMap};
pub use presence::{DEFAULT_TTL_MS, Participant, PresenceSet, color_for};
pub use session::{DocSession, SessionConfig, SnapshotState, warnings};
