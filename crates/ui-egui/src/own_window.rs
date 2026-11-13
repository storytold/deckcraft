//! One window per presentation: File › New and File › Open start another DeckCraft window for the
//! presentation instead of taking over the one being edited (PowerPoint's behaviour).
//!
//! Each window is its own DeckCraft process, so it has its own selection, tool, undo history and
//! panels. The host supplies [`crate::Services::spawn_window`]; without it (the web build, tests,
//! hosts that cannot start a process) everything opens in the current window as before. A window
//! that holds only a new, untouched, unnamed presentation is reused, as PowerPoint does.

use crate::SlideApp;

/// A path that names a presentation DeckCraft opens (as opposed to a picture or sound dropped
/// on a slide).
pub(crate) fn is_presentation_path(path: &str) -> bool {
    let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
    matches!(ext.as_deref(), Some("deckcraft" | "pptx" | "potx" | "ppsx"))
}

impl SlideApp {
    /// The window is editing something worth keeping on screen: a presentation that has a file or
    /// unsaved changes (the same test that asks to save on close). A new untitled presentation
    /// nobody has touched is not.
    pub(crate) fn window_in_use(&self) -> bool {
        self.session.active().is_some_and(|d| d.path.is_some() || d.is_dirty())
    }

    /// Start another window with `args` (a file to open, or none for a new presentation) when the
    /// preference is on, the host can, and this window is in use. `true` when it was started, in
    /// which case the caller must not also open the presentation here.
    pub(crate) fn open_in_own_window(&mut self, args: &[String]) -> bool {
        if !self.ui.window_per_presentation || !self.window_in_use() {
            return false;
        }
        let Some(spawn) = self.services.spawn_window.as_mut() else { return false };
        if !spawn(args) {
            self.set_status("Couldn't start another window; opening here instead".to_string());
            return false;
        }
        self.close_backstage();
        self.set_status("Opened in a new window".to_string());
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::{Services, SlideApp};
    use deckcraft_engine::Session;
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Calls = Rc<RefCell<Vec<Vec<String>>>>;

    /// An app whose host records the windows it is asked to start.
    fn app_with_spawner(ok: bool) -> (SlideApp, Calls) {
        let calls: Calls = Rc::default();
        let log = calls.clone();
        let services = Services {
            spawn_window: Some(Box::new(move |args: &[String]| {
                log.borrow_mut().push(args.to_vec());
                ok
            })),
            ..Default::default()
        };
        let mut app = SlideApp::new(Session::with_new(), services);
        app.ui.window_per_presentation = true;
        (app, calls)
    }

    #[test]
    fn a_new_untouched_presentation_is_reused_not_left_behind() {
        let (mut app, calls) = app_with_spawner(true);
        assert!(!app.window_in_use());
        app.run("file.new", json!({})).unwrap();
        assert!(calls.borrow().is_empty(), "nothing to preserve, so no second window");
    }

    #[test]
    fn file_new_starts_another_window_once_this_one_has_changes() {
        let (mut app, calls) = app_with_spawner(true);
        app.session.execute("slide.new", &json!({})).unwrap();
        assert!(app.window_in_use());
        let docs = app.session.documents().len();
        app.run("file.new", json!({})).unwrap();
        assert_eq!(*calls.borrow(), vec![Vec::<String>::new()], "one new window, no file");
        assert_eq!(app.session.documents().len(), docs, "this window keeps its presentation");
    }

    #[test]
    fn opening_a_file_while_working_starts_another_window_with_that_file() {
        let (mut app, calls) = app_with_spawner(true);
        app.session.execute("slide.new", &json!({})).unwrap();
        let docs = app.session.documents().len();
        app.open_path_in_background("/tmp/other.pptx");
        assert_eq!(*calls.borrow(), vec![vec!["/tmp/other.pptx".to_string()]]);
        assert_eq!(app.session.documents().len(), docs);
        assert!(app.opening.is_none(), "nothing is read in this window");
    }

    #[test]
    fn it_is_on_by_default_and_the_preference_turns_it_off() {
        assert!(crate::UiState::default().window_per_presentation, "a window per presentation is the default");
        let (mut app, calls) = app_with_spawner(true);
        app.session.execute("slide.new", &json!({})).unwrap();
        app.ui.window_per_presentation = false;
        let docs = app.session.documents().len();
        app.run("file.new", json!({})).unwrap();
        assert!(calls.borrow().is_empty());
        assert_eq!(app.session.documents().len(), docs + 1, "it opens here, as tabs");
    }

    #[test]
    fn ctrl_n_starts_another_window_too() {
        let (mut app, calls) = app_with_spawner(true);
        app.session.execute("slide.new", &json!({})).unwrap();
        let mods = deckcraft_engine::Mods { cmd: true, ..Default::default() };
        assert!(crate::menus::ui_shortcut(&mut app, egui::Key::N, mods));
        assert_eq!(*calls.borrow(), vec![Vec::<String>::new()]);
    }

    /// A file dropped on the window, read from disk as the native window reads it.
    #[cfg(not(target_arch = "wasm32"))]
    #[derive(Debug)]
    struct Dropped(std::path::PathBuf);

    #[cfg(not(target_arch = "wasm32"))]
    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.0).map_err(|e| e.to_string())
        }
    }

    /// The window's dropped files, as the native window hands them over.
    #[cfg(not(target_arch = "wasm32"))]
    fn drop_files(app: &mut SlideApp, paths: &[&str]) {
        let mut raw = egui::RawInput {
            dropped_files: paths
                .iter()
                .map(|p| std::sync::Arc::new(Dropped((*p).into())) as std::sync::Arc<dyn egui::DroppedFile + Send + Sync>)
                .collect(),
            ..Default::default()
        };
        app.raw_input_hook(&egui::Context::default(), &mut raw);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_presentation_dropped_while_working_opens_in_another_window() {
        let (mut app, calls) = app_with_spawner(true);
        app.session.execute("slide.new", &json!({})).unwrap();
        let docs = app.session.documents().len();
        drop_files(&mut app, &["/tmp/dropped.pptx"]);
        assert_eq!(*calls.borrow(), vec![vec!["/tmp/dropped.pptx".to_string()]]);
        assert_eq!(app.ui.recent.first().map(String::as_str), Some("/tmp/dropped.pptx"), "it is listed under Open Recent");
        assert_eq!(app.session.documents().len(), docs, "this window keeps its presentation");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_picture_dropped_while_working_stays_in_this_window() {
        let (mut app, calls) = app_with_spawner(true);
        app.session.execute("slide.new", &json!({})).unwrap();
        // A 1 × 1 PNG.
        const PNG: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
            0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF,
            0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60,
            0x82,
        ];
        let png = std::env::temp_dir().join(format!("deckcraft-dropped-{}.png", std::process::id()));
        std::fs::write(&png, PNG).unwrap();
        let before = app.session.active().unwrap().current_slide().unwrap().shapes.len();
        drop_files(&mut app, &[png.to_str().unwrap()]);
        std::fs::remove_file(&png).ok();
        assert!(calls.borrow().is_empty(), "a picture never starts a window");
        assert_eq!(app.session.active().unwrap().current_slide().unwrap().shapes.len(), before + 1, "it lands on the slide");
    }

    #[test]
    fn only_presentations_open_in_their_own_window() {
        for yes in ["/a/deck.pptx", "/a/Deck.DECKCRAFT", "b.potx", "c.ppsx"] {
            assert!(super::is_presentation_path(yes), "{yes}");
        }
        for no in ["/a/photo.png", "sound.mp3", "noext", ""] {
            assert!(!super::is_presentation_path(no), "{no}");
        }
    }

    #[test]
    fn a_window_that_cannot_start_falls_back_to_this_one() {
        let (mut app, calls) = app_with_spawner(false);
        app.session.execute("slide.new", &json!({})).unwrap();
        let docs = app.session.documents().len();
        app.run("file.new", json!({})).unwrap();
        assert_eq!(calls.borrow().len(), 1);
        assert_eq!(app.session.documents().len(), docs + 1);
    }

    #[test]
    fn without_a_host_that_starts_windows_nothing_changes() {
        let mut app = SlideApp::new(Session::with_new(), Services::default());
        app.session.execute("slide.new", &json!({})).unwrap();
        let docs = app.session.documents().len();
        app.run("file.new", json!({})).unwrap();
        assert_eq!(app.session.documents().len(), docs + 1);
    }
}
