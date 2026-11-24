//! DeckCraft in the browser.
//!
//! Runs the same [`deckcraft_ui_egui::SlideApp`] as the desktop app through eframe's web
//! runner (wgpu: WebGPU where available, WebGL2 otherwise). Build with `trunk build --release`
//! from this directory (output in `dist/web`).
//!
//! Differences from the desktop app:
//! - no TCP control channel (browsers can't listen on sockets);
//! - File → Open and Insert → Pictures/Audio/Video use the browser file picker; bytes arrive
//!   asynchronously through `Services::inbox`, applied to the document, slide and operation
//!   that started the pick (a dismissed picker or a closed document drops the file);
//! - Save and Export trigger a browser download (or go to the framing page with `?host=parent`);
//! - dropped files are read asynchronously by `web::WebShell` and delivered through the inbox;
//! - preferences (including which subject packs are on) are kept in the browser's local storage;
//!   decks are never stored by the app: they live in files the teacher opens and saves.
//! - a private build can carry what the Mac app has: the curriculum, own questions, Teacher Mode
//!   tools and lesson templates, with the exam question library beside the site (`school`).
//!
//! URL query flags: `?webgl` forces the WebGL2 backend instead of WebGPU; `?sample` starts with the
//! demo deck instead of an empty presentation; `?show` starts the slide show; `?smoke` walks through
//! the main screens and logs each step (the CI browser check, `tests/smoke.mjs`).
//! `?host=parent` (inside an `<iframe>`) talks to a same-origin parent page instead: it sends the
//! presentation to open and stores what DeckCraft saves (`host`), and `&author=` names the user
//! for comments.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

#[cfg(any(target_arch = "wasm32", test))]
mod api;
#[cfg(target_arch = "wasm32")]
mod host;
#[cfg(target_arch = "wasm32")]
mod presenter;
#[cfg(target_arch = "wasm32")]
mod school;
#[cfg(target_arch = "wasm32")]
mod smoke;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("deckcraft-web only runs in the browser: build it with `trunk build --release` in apps/deckcraft-web");
}
