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
//! - Save and Export trigger a browser download;
//! - dropped files are read asynchronously by `web::WebShell` and delivered through the inbox;
//! - preferences are not persisted.
//!
//! URL query flags: `?webgl` forces the WebGL2 backend instead of WebGPU; `?blank` starts with an
//! empty presentation instead of the sample deck; `?show` starts the slide show.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(any(target_arch = "wasm32", test))]
fn attachment_size_allowed(size: f64) -> bool {
    size.is_finite() && size >= 0.0 && size.fract() == 0.0 && size <= deckcraft_ui_egui::MAX_ATTACHMENT_BYTES as f64
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("deckcraft-web only runs in the browser: build it with `trunk build --release` in apps/deckcraft-web");
}

#[cfg(test)]
mod attachment_size_tests {
    #[test]
    fn browser_attachment_size_is_bounded_before_array_buffer_allocation() {
        let max = deckcraft_ui_egui::MAX_ATTACHMENT_BYTES as f64;
        for size in [0.0, 1.0, max] {
            assert!(super::attachment_size_allowed(size));
        }
        for size in [-1.0, 0.5, max + 1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(!super::attachment_size_allowed(size));
        }
    }
}
