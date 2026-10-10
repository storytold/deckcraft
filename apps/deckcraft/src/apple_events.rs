//! macOS open-documents and quit Apple events.
//!
//! Finder double-clicks, Open With, drops on the Dock icon and `open -a DeckCraft deck.pptx` don't
//! pass paths on the command line: LaunchServices sends the running (or just-launched) app a
//! `kAEOpenDocuments` ('odoc') Apple event. winit 0.30 doesn't handle it and owns the
//! `NSApplicationDelegate`, so the presentation never opened. Handling it ourselves needs
//! Objective-C class declarations, i.e. `unsafe`, which this workspace forbids. The audited
//! `fmv-macos-events` crate (also used by PdfCraft and PhotoCraft) wraps exactly that, an
//! `NSAppleEventManager` handler registered before Finder's launch event that leaves winit's
//! delegate alone, behind a safe main-thread API.

use deckcraft_ui_egui::SlideApp;
use fmv_macos_events::{Event, Inbox, Registration};

/// Keeps the Apple-event handlers registered; hold it until the event loop returns.
pub struct AppleEvents {
    _registration: Registration,
    inbox: Inbox,
}

impl AppleEvents {
    /// Register the handlers. Call on the main thread before the event loop starts, so the event
    /// that launched the app (a Finder double-click) is caught too.
    pub fn install() -> Self {
        let (registration, inbox) = Registration::install();
        Self { _registration: registration, inbox }
    }

    /// The queue the app drains every frame ([`poll`]); events arriving later wake `ctx`.
    pub fn connect(&self, ctx: &egui::Context) -> Inbox {
        let ctx = ctx.clone();
        self.inbox.set_wake(move || ctx.request_repaint());
        self.inbox.clone()
    }
}

/// Open the presentations and act on the quit requests that arrived since the last frame.
pub fn poll(inbox: &Inbox, app: &mut SlideApp, ctx: &egui::Context) {
    for e in inbox.drain() {
        match e {
            Event::Open(paths) => {
                for p in paths.iter().filter_map(|p| p.to_str()) {
                    if let Err(e) = app.open_path(p) {
                        log::error!("{p}: {e}");
                        app.set_status(e);
                    }
                }
            }
            Event::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }
}
