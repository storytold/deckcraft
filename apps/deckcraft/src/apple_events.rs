//! macOS open-documents and quit Apple events.
//!
//! Finder double-clicks, Open With, drops on the Dock icon and `open -a DeckCraft deck.pptx` don't
//! pass paths on the command line: LaunchServices sends the running (or just-launched) app a
//! `kAEOpenDocuments` ('odoc') Apple event. winit 0.30 doesn't handle it and owns the
//! `NSApplicationDelegate`. Handling it ourselves needs Objective-C class declarations, i.e.
//! `unsafe`, which this workspace forbids. The audited `fmv-macos-events` crate (also used by
//! GridCraft, PdfCraft and PhotoCraft) wraps exactly that, an `NSAppleEventManager` handler
//! registered before Finder's launch event that leaves winit's delegate alone, behind a safe
//! main-thread API.

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

/// Open the documents and act on the quit requests that arrived since the last frame.
pub fn poll(inbox: &Inbox, app: &mut SlideApp, ctx: &egui::Context) {
    for event in inbox.drain() {
        handle(event, app, ctx);
    }
}

fn handle(event: Event, app: &mut SlideApp, ctx: &egui::Context) {
    match event {
        Event::Open(paths) => {
            let paths: Vec<String> = paths.iter().filter_map(|p| p.to_str().map(str::to_string)).collect();
            for (path, error) in app.open_from_system(&paths) {
                log::error!("{path}: {error}");
                app.set_status(format!("Couldn't open {path}: {error}"));
            }
        }
        Event::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_engine::Session;
    use deckcraft_ui_egui::Services;
    use serde_json::json;
    use std::path::PathBuf;

    fn presentation(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("deckcraft-apple-events-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut session = Session::with_new();
        session.execute("file.saveAs", &json!({"path": path})).unwrap();
        path
    }

    #[test]
    fn open_events_append_presentations_at_launch_and_while_running() {
        let first = presentation("Finder launch ü.pptx");
        let second = presentation("Finder second.deckcraft");
        let mut app = SlideApp::new(Session::new(), Services::default());
        let ctx = egui::Context::default();
        handle(Event::Open(vec![first.clone()]), &mut app, &ctx);
        assert_eq!(app.session.documents().len(), 1);
        handle(Event::Open(vec![second.clone()]), &mut app, &ctx);
        assert_eq!(app.session.documents().len(), 2);
        assert_eq!(app.ui.recent.first(), second.to_str().map(str::to_string).as_ref());
        std::fs::remove_file(first).unwrap();
        std::fs::remove_file(second).unwrap();
    }

    #[test]
    fn open_event_starts_only_the_last_successful_powerpoint_show() {
        let show = presentation("Finder show.PPSX");
        let edit = presentation("Finder edit.pptx");
        let ctx = egui::Context::default();
        let mut app = SlideApp::new(Session::new(), Services::default());
        handle(Event::Open(vec![show.clone(), edit.clone()]), &mut app, &ctx);
        assert!(app.show.is_none(), "the last file opens for editing");
        handle(Event::Open(vec![show.clone()]), &mut app, &ctx);
        assert!(app.show.is_some(), "a system-opened PPSX starts its show");
        std::fs::remove_file(show).unwrap();
        std::fs::remove_file(edit).unwrap();
    }

    #[test]
    fn bad_open_event_reports_the_error_and_keeps_the_document() {
        let mut app = SlideApp::new(Session::with_new(), Services::default());
        let ctx = egui::Context::default();
        handle(Event::Open(vec![PathBuf::from("/nonexistent/deckcraft-finder-missing.pptx")]), &mut app, &ctx);
        assert_eq!(app.session.documents().len(), 1);
        // Status is visible through the public UI control inspection.
        let result = deckcraft_ui_egui::control::inspect(&app, &ctx);
        assert!(result.to_string().contains("Couldn't open"), "{result}");
    }
}
