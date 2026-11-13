use std::ffi::OsString;

mod cli;
#[cfg(unix)]
mod closed_stdout;
mod convert;
mod damaged;
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md).
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod mcp_roots;

fn deckcraft_cli() -> OsString {
    std::env::var_os("NEXTEST_BIN_EXE_deckcraft_cli").unwrap_or_else(|| env!("CARGO_BIN_EXE_deckcraft-cli").into())
}
