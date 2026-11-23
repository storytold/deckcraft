//! DeckCraft's MCP server.
//!
//! [Model Context Protocol](https://modelcontextprotocol.io) over stdio: newline-delimited
//! JSON-RPC 2.0, hand-written (no async runtime). The server exposes DeckCraft as a set of MCP
//! tools and resources and forwards everything to a [`Backend`]:
//!
//! - [`Remote`] talks to a running desktop app through its loopback JSON-lines control channel
//!   (`deckcraft --control 7979`): one `{"id","method","params"}` line in, one
//!   `{"id","ok","result"|"error"}` line out (see `docs/control-protocol.md`).
//! - [`Headless`] hosts an in-process [`deckcraft_engine::Session`] and implements the
//!   engine-level control-channel methods itself (rendering pages with `deckcraft-render`), so
//!   agents can build layouts and look at them without a window.
//!
//! Entry points: [`Server::serve`] (stdio loop) and [`Server::handle_line`] (one message).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod backend;
pub mod control_auth;
mod headless;
mod server;
mod tools;

pub use backend::{Backend, Remote};
pub use headless::Headless;
pub use server::{PROTOCOL_VERSION, Server};
pub use tools::{ToolResult, call_tool, tool_definitions};

/// Default control-channel port of the desktop app.
pub const DEFAULT_PORT: u16 = 7979;

/// `"7979"` → `"127.0.0.1:7979"`; `"host:port"` is kept as is.
pub fn control_addr(s: &str) -> String {
    if s.parse::<u16>().is_ok() { format!("127.0.0.1:{s}") } else { s.to_string() }
}

#[cfg(test)]
mod tests;
