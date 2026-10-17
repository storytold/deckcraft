//! DeckCraft's implementation of the Office JavaScript API for Office add-ins' task panes.
//!
//! An Office add-in's task pane is a web page that calls `Office.onReady`, `PowerPoint.run` and
//! the proxy objects (`context.presentation.slides.getItemAt(0).shapes.addTextBox("Hi")`,
//! `shapes.load("items/name")`, `context.sync()`). Microsoft's `office.js` never runs in
//! DeckCraft: this crate, compiled to wasm, defines those globals itself, written from the public
//! API reference on learn.microsoft.com (clean room). It covers a subset of PowerPointApi 1.1–1.4
//! (and reports 1.4) plus the selection members of later sets; docs/officejs.md lists it. Each `context.sync()` sends the queued actions as one JSON batch to the host, which runs
//! it as `office.sync` (one undo step) and answers with the loaded values or an error:
//!
//! - request `{"id": n, "actions": [{"op": "load"|"set"|"call", "path": [...], …}]}`, posted
//!   with `globalThis.__craftOffice.post(json)`;
//! - answer `{"id": n, "results": [...]}` or `{"id": n, "error": {"code", "message", "action"}}`,
//!   delivered with `globalThis.__craftOfficeReply(json)`.
//!
//! An object added in a batch (`shapes.addTextBox(…)`) is named, until the sync answers its id, by
//! `{"m": "getItem", "a": [{"result": i}]}`: the id answered by action `i` of the same batch.
//! PowerPoint's enum values (`"GeometricShape"`, `"Rectangle"`, `"AutoSizeNone"`) travel in
//! camelCase (`geometricShape`, `rectangle`, `autoSizeNone`).
//!
//! [`core`] is the batching and the cache of loaded properties (plain Rust, tested natively);
//! the JavaScript side (`js`, wasm only) is the globals and proxy classes. What isn't covered
//! throws an error with code `NotImplemented`.
//!
//! # Building the library a task pane loads
//!
//! ```sh
//! cargo build --release -p deckcraft-office-js --target wasm32-unknown-unknown
//! mkdir -p "$CARGO_TARGET_DIR/office-js"
//! wasm-bindgen --target no-modules --no-typescript --out-name office_js \
//!     --out-dir "$CARGO_TARGET_DIR/office-js" \
//!     "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/deckcraft_office_js.wasm"
//! cp crates/office-js/bootstrap.js "$CARGO_TARGET_DIR/office-js/bootstrap.js"
//! ```
//!
//! (`wasm-bindgen-cli` at the `wasm-bindgen` version in `Cargo.lock`.) The output is
//! `office_js.js` (glue defining `wasm_bindgen`), `office_js_bg.wasm` and `bootstrap.js`. The
//! desktop app injects, before the page's own scripts: `bootstrap.js`, then `office_js.js`, then
//! `__craftOfficeStart(wasm_bindgen, "<office_js_bg.wasm as base64>")`.
//!
//! # Tests
//!
//! `cargo test -p deckcraft-office-js` runs the batching tests natively; the library as a page
//! sees it runs in Node against a fake host:
//!
//! ```sh
//! CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
//!     cargo test -p deckcraft-office-js --target wasm32-unknown-unknown
//! ```
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod core;
#[cfg(target_arch = "wasm32")]
mod js;
#[cfg(target_arch = "wasm32")]
pub use js::*;
