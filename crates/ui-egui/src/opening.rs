//! Opening a presentation in the background (#350): reading and parsing a large PPTX no longer
//! freezes the window. A modal shows the file being opened and the time so far, and Cancel (or
//! Escape) gives the window back at once; the abandoned read finishes on its thread and is dropped.

use crate::SlideApp;
use deckcraft_engine::{EngineError, cmd::file::LoadedFile};
use serde_json::{Value, json};
use std::sync::mpsc::{Receiver, TryRecvError};

/// Where a read gets its bytes: a file to open, or data the host already holds (a browser open, or
/// the retry after the password prompt).
enum Source {
    Path(String),
    Bytes { name: String, data: Vec<u8> },
}

/// A presentation being read on a background thread.
pub struct Opening {
    pub path: String,
    pub started_ms: f64,
    /// Monotonic identity of this attempt. Cancelling and reopening the same path must not match
    /// a click queued for the previous progress window's Cancel button.
    pub(crate) generation: u64,
    source: Source,
    /// The password that was tried, so a rejected one goes back into the field.
    password: Option<String>,
    /// What a password prompt is reopened with when the read asks for one.
    prompt: Value,
    rx: Receiver<Result<LoadedFile, EngineError>>,
}

impl Opening {
    /// The file name shown in the window.
    pub fn name(&self) -> String {
        std::path::Path::new(&self.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| self.path.clone())
    }

    /// Opened from bytes the host held: there is no file to save back to.
    fn opened_from_bytes(&self) -> bool {
        matches!(self.source, Source::Bytes { .. })
    }

    /// A progress window for identity tests; the receiver never yields a file.
    #[cfg(test)]
    pub(crate) fn for_test(path: &str) -> Self {
        let (_tx, rx) = std::sync::mpsc::channel();
        Opening {
            path: path.to_string(),
            started_ms: 0.0,
            generation: 0,
            source: Source::Path(path.to_string()),
            password: None,
            prompt: Value::Null,
            rx,
        }
    }
}

impl SlideApp {
    /// Open `path` without blocking the UI: the file is read on a thread and the progress window
    /// shows until it arrives. Without threads (the web build) it opens at once.
    pub fn open_path_in_background(&mut self, path: &str) {
        self.start(Source::Path(path.to_string()), None, json!({"name": path, "path": path}));
    }

    /// Retry `path` with the password just typed. On a thread like any other read: the key
    /// derivation runs up to 10,000,000 hashes, which must not freeze the window.
    pub fn open_path_with_password_in_background(&mut self, path: &str, password: &str) {
        self.start(Source::Path(path.to_string()), Some(password.to_string()), json!({"name": path, "path": path}));
    }

    /// Retry a bytes-open (`file.openBytes`) with the password just typed, on the same thread.
    pub fn open_bytes_with_password_in_background(&mut self, name: &str, data: &[u8], password: &str) {
        self.start(
            Source::Bytes { name: name.to_string(), data: data.to_vec() },
            Some(password.to_string()),
            json!({"name": name, "data": deckcraft_engine::cmd::base64_encode(data)}),
        );
    }

    /// The one place a read is started: on a thread here, at once in the web build (no threads) and
    /// when the thread cannot be spawned.
    fn start(&mut self, source: Source, password: Option<String>, prompt: Value) {
        if let Some(o) = &self.opening {
            let name = o.name();
            self.set_status(format!("Still opening {name}"));
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = prompt;
            self.open_at_once(&source, password.as_deref());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = match &source {
                Source::Path(p) => p.clone(),
                Source::Bytes { name, .. } => name.clone(),
            };
            let (tx, rx) = std::sync::mpsc::channel();
            let src = match &source {
                Source::Path(p) => Source::Path(p.clone()),
                Source::Bytes { name, data } => Source::Bytes { name: name.clone(), data: data.clone() },
            };
            let pw = password.clone();
            let spawned = std::thread::Builder::new().name("deckcraft-open".into()).spawn(move || {
                let pw = pw.as_deref();
                let r = match (&src, pw) {
                    (Source::Path(p), pw) => deckcraft_engine::cmd::file::read_presentation_file_with_password(p, pw),
                    (Source::Bytes { name, data }, pw) => deckcraft_engine::cmd::file::read_presentation_bytes_with_password(name, data, pw),
                };
                let _ = tx.send(r);
            });
            match spawned {
                Ok(_) => {
                    self.opening_generation = self.opening_generation.wrapping_add(1).max(1);
                    self.opening =
                        Some(Opening { path, started_ms: crate::now_ms(), generation: self.opening_generation, source, password, prompt, rx });
                }
                Err(_) => self.open_at_once(&source, password.as_deref()),
            }
        }
    }

    /// Read at once, on this thread: the web build, and a spawn that failed. A failure is reported
    /// as it always was (a locked file still comes back as the prompt).
    fn open_at_once(&mut self, source: &Source, password: Option<&str>) {
        let r = match (source, password) {
            (Source::Path(p), None) => self.open_path(p),
            (Source::Path(p), pw) => self.run("file.open", json!({"path": p, "password": pw})).map(|_| ()),
            (Source::Bytes { name, data }, pw) => {
                self.run("file.openBytes", json!({"name": name, "data": deckcraft_engine::cmd::base64_encode(data), "password": pw})).map(|_| ())
            }
        };
        if let Err(e) = r {
            self.notify(e);
        }
    }

    /// Stop waiting for the file being opened; the presentation stays as it is.
    pub fn cancel_opening(&mut self) {
        if let Some(o) = self.opening.take() {
            self.set_status(format!("Stopped opening {}", o.name()));
        }
    }

    /// Take the background read's result when it has arrived.
    pub(crate) fn poll_opening(&mut self) {
        let Some(o) = &self.opening else { return };
        let r = match o.rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(EngineError::Other(format!("{}: reading the file stopped unexpectedly", o.path))),
        };
        let Some(o) = self.opening.take() else { return };
        match r {
            Ok(f) => {
                // A retry that succeeded answers the prompt: close it (a click-path open already
                // dismissed it, so this only matters for a programmatic retry).
                if self.dialog.as_ref().is_some_and(|d| d.id == "password") {
                    self.dialog = None;
                }
                // Into the session through the command path, like file.open: journaled and guarded.
                let path = f.path.clone();
                let token = self.session.stage_loaded(f);
                let mut params = json!({"path": path, "token": token});
                if o.opened_from_bytes() {
                    // A bytes-open has nothing to save back to, but a replayed journal entry would
                    // otherwise have to read the file it was never opened from.
                    if let Some(data) = o.prompt.get("data") {
                        params["data"] = data.clone();
                    }
                }
                // `run` reports a failure itself (and `report_error` keeps it out of the bar when
                // the prompt asked): nothing to do with an Err here.
                if self.run("file.adopt", params).is_ok() && !o.opened_from_bytes() {
                    self.note_recent(&o.path);
                }
            }
            // A locked file hands the prompt back instead of a status-bar error, and a rejected
            // password hands it back with the text typed, explained under the field.
            Err(e) => {
                let (name, rejected) = match &e {
                    EngineError::PasswordRequired(n) => (n.clone(), false),
                    EngineError::WrongPassword(n) => (n.clone(), true),
                    _ => {
                        // Over the start screen too, like a failed file.open (#346).
                        self.notify(e.to_string());
                        return;
                    }
                };
                let mut d = crate::dialogs::Dialog::new("password");
                d.params = o.prompt.clone();
                if rejected {
                    d.fields.insert("password".into(), o.password.clone().unwrap_or_default());
                    d.fields.insert("error".into(), crate::dialogs::rejected_password(&name, &e.to_string()));
                }
                self.dialog = Some(d);
            }
        }
    }

    /// The progress window over everything while a file opens.
    pub(crate) fn opening_ui(&mut self, ctx: &egui::Context) {
        let Some(o) = &self.opening else { return };
        let name = o.name();
        let secs = ((crate::now_ms() - o.started_ms) / 1000.0).max(0.0);
        let mut cancel = false;
        egui::Modal::new(egui::Id::unique("deckcraft-opening")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(18.0));
                ui.label(format!("Opening {name}…"));
            });
            ui.add_space(6.0);
            ui.weak(format!("Reading the file: {secs:.0} s"));
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel = ui.button("Cancel").clicked();
            });
        });
        if cancel || ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.cancel_opening();
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use deckcraft_engine::Session;

    fn saved_deck(name: &str, slides: usize) -> String {
        let mut s = Session::with_new();
        for _ in 1..slides {
            s.execute("slide.new", &serde_json::json!({})).unwrap();
        }
        let path = std::env::temp_dir().join(format!("deckcraft-opening-{}-{name}.deckcraft", std::process::id()));
        let path = path.to_string_lossy().to_string();
        s.execute("file.saveAs", &serde_json::json!({"path": path})).unwrap();
        path
    }

    fn wait(app: &mut SlideApp) {
        for _ in 0..500 {
            app.poll_opening();
            if app.opening.is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("opening did not finish");
    }

    #[test]
    fn opens_in_the_background_and_adds_the_document() {
        let path = saved_deck("ok", 3);
        let mut app = SlideApp::new(Session::new(), crate::Services::default());
        app.open_path_in_background(&path);
        assert!(app.opening.is_some(), "the read runs on a thread");
        wait(&mut app);
        assert_eq!(app.session.documents().len(), 1);
        assert_eq!(app.session.active().unwrap().doc.slides.len(), 3);
        assert_eq!(app.ui.recent.first(), Some(&path));
        // It went in through the command path (journaled like file.open).
        assert_eq!(app.session.journal.last().map(|(id, _)| id.as_str()), Some("file.adopt"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn cancel_drops_the_result() {
        let path = saved_deck("cancel", 2);
        let mut app = SlideApp::new(Session::with_new(), crate::Services::default());
        app.open_path_in_background(&path);
        app.cancel_opening();
        assert!(app.opening.is_none());
        std::thread::sleep(std::time::Duration::from_millis(200));
        app.poll_opening();
        assert_eq!(app.session.documents().len(), 1, "nothing is added after Cancel");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn errors_are_shown_and_a_second_open_waits() {
        let mut app = SlideApp::new(Session::new(), crate::Services::default());
        app.open_path_in_background("/nonexistent/deckcraft/missing.pptx");
        app.open_path_in_background("/nonexistent/deckcraft/other.pptx");
        assert!(app.status.as_ref().is_some_and(|(m, _)| m.starts_with("Still opening missing.pptx")));
        wait(&mut app);
        assert!(app.session.documents().is_empty());
        assert!(app.toast.as_ref().is_some_and(|t| t.text.contains("missing.pptx")), "the reason is shown");
    }

    /// Opening a path displays progress; cancelling closes the progress window without opening the
    /// deck.
    #[test]
    fn the_progress_window_shows_and_cancel_closes_it() {
        use egui_kittest::kittest::Queryable;
        let mut app = SlideApp::new(Session::with_new(), crate::Services::default());
        // A read that never answers: the window stays until Cancel.
        let (_tx, rx) = std::sync::mpsc::channel();
        app.opening = Some(Opening {
            path: "/decks/Big deck.pptx".into(),
            started_ms: crate::now_ms(),
            generation: 1,
            source: Source::Path("/decks/Big deck.pptx".into()),
            password: None,
            prompt: Value::Null,
            rx,
        });
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1000.0, 700.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        harness.run_steps(3);
        harness.get_by_label("Opening Big deck.pptx…");
        harness.get_by_label("Cancel").click();
        harness.run_steps(2);
        assert!(harness.state().opening.is_none());
        assert_eq!(harness.state().session.documents().len(), 1);
    }

    /// A deck of `slides` slides saved as `file` (its extension picks the format).
    fn saved_as(file: &str, slides: usize) -> String {
        let mut s = Session::with_new();
        for _ in 1..slides {
            s.execute("slide.new", &serde_json::json!({})).unwrap();
        }
        let dir = std::env::temp_dir().join(format!("deckcraft-opening-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(file).to_string_lossy().to_string();
        s.execute("file.saveAs", &serde_json::json!({"path": path})).unwrap();
        path
    }

    /// #862: a PowerPoint Show handed over by the system (command line, double-click) starts its
    /// slide show at once, full screen and from the first slide.
    #[test]
    fn a_ppsx_from_the_system_starts_the_show() {
        let path = saved_as("Talk.PPSX", 3);
        let mut app = SlideApp::new(Session::new(), crate::Services::default());
        assert!(app.open_from_system(std::slice::from_ref(&path)).is_empty());
        assert_eq!(app.session.documents().len(), 1);
        let show = app.show.as_ref().expect("the slide show started");
        assert_eq!(show.state.slide, 0);
        assert!(!show.reading, "full screen, not Reading View");
        std::fs::remove_file(&path).ok();
    }

    /// #862: a `.pptx` from the system opens for editing, and so does a `.ppsx` followed by a
    /// `.pptx`: the show would otherwise play the deck opened last.
    #[test]
    fn a_pptx_from_the_system_opens_for_editing() {
        let pptx = saved_as("edit.pptx", 3);
        let ppsx = saved_as("first.ppsx", 2);
        let mut app = SlideApp::new(Session::new(), crate::Services::default());
        assert!(app.open_from_system(std::slice::from_ref(&pptx)).is_empty());
        assert!(app.show.is_none());
        assert!(app.open_from_system(&[ppsx.clone(), pptx.clone()]).is_empty());
        assert_eq!(app.session.documents().len(), 3);
        assert!(app.show.is_none(), "the last file is a .pptx");
        std::fs::remove_file(&pptx).ok();
        std::fs::remove_file(&ppsx).ok();
    }

    /// #862: File › Open opens a `.ppsx` for editing, as PowerPoint does.
    #[test]
    fn file_open_opens_a_ppsx_for_editing() {
        let path = saved_as("open.ppsx", 2);
        let mut app = SlideApp::new(Session::new(), crate::Services::default());
        app.open_path_in_background(&path);
        wait(&mut app);
        assert_eq!(app.session.active().unwrap().doc.slides.len(), 2);
        assert!(app.show.is_none());
        assert_eq!(app.ui.view, crate::ViewMode::Normal);
        std::fs::remove_file(&path).ok();
    }

    /// #862: Esc, or a click past the end screen, ends the `.ppsx` show and leaves the deck open in
    /// the editor (PowerPoint closes it instead).
    #[test]
    fn leaving_the_ppsx_show_keeps_the_deck_open() {
        let path = saved_as("leave.ppsx", 2);
        for leave in [egui::Key::Escape, egui::Key::ArrowRight] {
            let mut app = SlideApp::new(Session::new(), crate::Services::default());
            assert!(app.open_from_system(std::slice::from_ref(&path)).is_empty());
            let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1000.0, 700.0)).build_ui_state(
                |ui, app: &mut SlideApp| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
                app,
            );
            harness.run_steps(3);
            assert!(harness.state().show.is_some(), "the show is running");
            if leave == egui::Key::ArrowRight {
                for _ in 0..2 {
                    harness.key_press(egui::Key::ArrowRight);
                    harness.run_steps(2);
                }
                assert!(harness.state().show.as_ref().is_some_and(|s| s.ended), "the end screen");
            }
            harness.key_press(leave);
            harness.run_steps(2);
            assert!(harness.state().show.is_none(), "{leave:?} ends the show");
            assert_eq!(harness.state().session.documents().len(), 1, "{leave:?}: the deck stays open");
            assert_eq!(harness.state().ui.view, crate::ViewMode::Normal);
        }
        std::fs::remove_file(&path).ok();
    }
}
