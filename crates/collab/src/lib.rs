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
pub mod delta;
pub mod keys;

pub use apply::{Adapter, Rejection, class_of};
pub use delta::delta;
pub use keys::{KeyIssuer, KeyMap};
