//! The interface state each presentation's window keeps for itself.
//!
//! With a native window per presentation ([`crate::doc_windows`]), [`SlideApp`]'s per-window
//! fields (dialog, Backstage, slide show, panes, view, zoom…) hold the state of the active
//! presentation's window. Every other window's state is parked in a [`WindowUi`] and swapped in
//! when its presentation becomes active, so a dialog or a slide show stays in its own window.

use std::collections::HashMap;

use crate::{AnimPreview, SlideApp, ViewMode, backstage, dialogs, doc_window, eyedropper, notes, selpane, show, sorter, textures, thumbs, toast};

/// One window's parked interface state (see the module docs).
pub struct WindowUi {
    dialog: Option<dialogs::Dialog>,
    backstage: Option<backstage::Backstage>,
    show: Option<show::Show>,
    anim_preview: Option<AnimPreview>,
    palette: Option<(String, usize)>,
    eyedropper: Option<eyedropper::State>,
    toast: Option<toast::Toast>,
    sorter: sorter::State,
    selpane: selpane::State,
    notes_buf: (u64, usize, notes::Draft),
    thumb_drag: Option<thumbs::SlideDrag>,
    align_to: Option<(Vec<deckcraft_model::ShapeId>, bool)>,
    slides_focused: bool,
    doc_window: Option<doc_window::DocWindow>,
    view: ViewMode,
    zoom: Option<f32>,
    thumbs_scroll: f32,
    smartart_focus: Option<usize>,
    comment_focus: Option<(usize, usize)>,
    // Each window renders its own slides and records its own canvas geometry: a shared cache
    // would hand one window's texture to another before it is painted.
    textures: textures::Textures,
    canvas_rect: Option<egui::Rect>,
    canvas_scale: f32,
    slide_rect: Option<egui::Rect>,
    canvas_region: Option<egui::Rect>,
    ribbon_scroll: HashMap<String, u32>,
    reuse_target: Option<crate::FileTarget>,
}

impl Default for WindowUi {
    /// A new window: Normal view fitted to the window, nothing open.
    fn default() -> Self {
        WindowUi {
            dialog: None,
            backstage: None,
            show: None,
            anim_preview: None,
            palette: None,
            eyedropper: None,
            toast: None,
            sorter: sorter::State::default(),
            selpane: selpane::State::default(),
            notes_buf: (0, usize::MAX, notes::Draft::default()),
            thumb_drag: None,
            align_to: None,
            slides_focused: false,
            doc_window: None,
            view: ViewMode::default(),
            zoom: None,
            thumbs_scroll: 0.0,
            smartart_focus: None,
            comment_focus: None,
            textures: textures::Textures::default(),
            canvas_rect: None,
            canvas_scale: 1.0,
            slide_rect: None,
            canvas_region: None,
            ribbon_scroll: HashMap::new(),
            reuse_target: None,
        }
    }
}

impl WindowUi {
    /// Exchange this parked state with the app's live fields.
    fn swap(&mut self, app: &mut SlideApp) {
        use std::mem::swap;
        // Destructured in full so a new field can't be left out of the exchange.
        let WindowUi {
            dialog,
            backstage,
            show,
            anim_preview,
            palette,
            eyedropper,
            toast,
            sorter,
            selpane,
            notes_buf,
            thumb_drag,
            align_to,
            slides_focused,
            doc_window,
            view,
            zoom,
            thumbs_scroll,
            smartart_focus,
            comment_focus,
            textures,
            canvas_rect,
            canvas_scale,
            slide_rect,
            canvas_region,
            ribbon_scroll,
            reuse_target,
        } = self;
        swap(dialog, &mut app.dialog);
        swap(backstage, &mut app.backstage);
        swap(show, &mut app.show);
        swap(anim_preview, &mut app.anim_preview);
        swap(palette, &mut app.palette);
        swap(eyedropper, &mut app.eyedropper);
        swap(toast, &mut app.toast);
        swap(sorter, &mut app.sorter);
        swap(selpane, &mut app.selpane);
        swap(notes_buf, &mut app.notes_buf);
        swap(thumb_drag, &mut app.thumb_drag);
        swap(align_to, &mut app.align_to);
        swap(slides_focused, &mut app.slides_focused);
        swap(doc_window, &mut app.doc_window);
        swap(view, &mut app.ui.view);
        swap(zoom, &mut app.ui.zoom);
        swap(thumbs_scroll, &mut app.ui.thumbs_scroll);
        swap(smartart_focus, &mut app.ui.smartart_focus);
        swap(comment_focus, &mut app.ui.comment_focus);
        swap(textures, &mut app.textures);
        swap(canvas_rect, &mut app.canvas_rect);
        swap(canvas_scale, &mut app.canvas_scale);
        swap(slide_rect, &mut app.slide_rect);
        swap(canvas_region, &mut app.canvas_region);
        swap(ribbon_scroll, &mut app.ribbon_scroll);
        swap(reuse_target, &mut app.reuse_target);
    }
}

impl SlideApp {
    /// Make presentation `uid` active, with its window's interface state.
    pub fn enter_deck(&mut self, uid: u64) {
        if let Some(i) = self.session.documents().iter().position(|d| d.uid == uid) {
            self.session.set_active(i);
        }
        self.follow_active_deck();
    }

    /// Swap in the active presentation's window state after anything made another presentation
    /// active (opening, File › New, Next Window). A new presentation's window starts fresh.
    pub(crate) fn follow_active_deck(&mut self) {
        if !self.native_windows {
            return;
        }
        let active = self.session.active().map(|d| d.uid);
        // The first presentation keeps the state the fields already hold.
        if let Some(previous) = self.live_deck
            && active != Some(previous)
        {
            let mut parked = active.and_then(|uid| self.parked.remove(&uid)).unwrap_or_default();
            parked.swap(self);
            self.parked.insert(previous, parked);
        }
        self.live_deck = active;
        let open: std::collections::HashSet<u64> = self.session.documents().iter().map(|d| d.uid).collect();
        self.parked.retain(|uid, _| open.contains(uid));
    }

    /// Quit: with a window per presentation, each unsaved presentation asks in its own window,
    /// one after the other ([`Self::continue_quit`]); otherwise the window's close asks.
    pub fn request_quit(&mut self, ctx: &egui::Context) {
        if self.native_windows {
            self.quitting.get_or_insert(None);
        } else {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        }
    }

    /// Advance a quit ([`Self::request_quit`]): ask the next unsaved presentation, stop when
    /// one was cancelled (or failed to save), quit when none is left. Returns the presentation
    /// whose window should come forward.
    pub fn continue_quit(&mut self, ctx: &egui::Context) -> Option<u64> {
        let asked = self.quitting?;
        if let Some(uid) = asked.filter(|uid| self.session.documents().iter().any(|d| d.uid == *uid)) {
            let dialog = if self.live_deck == Some(uid) { &self.dialog } else { &self.parked.get(&uid)?.dialog };
            if dialog.as_ref().is_none_or(|d| d.id != "close") {
                // Answered and still open: Cancel, or the save didn't happen.
                self.quitting = None;
            }
            return None;
        }
        let Some(uid) = self.session.documents().iter().find(|d| d.is_dirty()).map(|d| d.uid) else {
            self.quitting = None;
            self.quit_confirmed = true;
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
            return None;
        };
        self.enter_deck(uid);
        self.show = None;
        self.dialog = Some(dialogs::Dialog::new("close"));
        self.quitting = Some(Some(uid));
        Some(uid)
    }
}

#[cfg(test)]
mod tests {
    use deckcraft_engine::Session;
    use serde_json::json;

    use crate::{FilePlace, FileTarget, Services, SlideApp, ViewMode, dialogs};

    #[test]
    fn each_presentation_keeps_its_own_dialog() {
        let mut app = SlideApp::new(Session::new(), Services::default());
        app.native_windows = true;
        app.run("file.new", json!({})).unwrap();
        app.follow_active_deck();
        let first = app.session.active().unwrap().uid;
        app.dialog = Some(dialogs::Dialog::new("close"));
        app.run("file.new", json!({})).unwrap();
        app.follow_active_deck();
        let second = app.session.active().unwrap().uid;
        assert_ne!(first, second);
        assert!(app.dialog.is_none(), "the new window starts without the first one's dialog");
        app.enter_deck(first);
        assert_eq!(app.dialog.as_ref().map(|d| d.id.as_str()), Some("close"));
        app.enter_deck(second);
        assert!(app.dialog.is_none());
    }

    #[test]
    fn each_window_keeps_its_own_canvas_geometry() {
        let mut app = SlideApp::new(Session::new(), Services::default());
        app.native_windows = true;
        app.run("file.new", json!({})).unwrap();
        app.follow_active_deck();
        let first = app.session.active().unwrap().uid;
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(300.0, 200.0));
        app.canvas_rect = Some(rect);
        app.canvas_scale = 0.5;
        app.run("file.new", json!({})).unwrap();
        app.follow_active_deck();
        assert_eq!(app.canvas_rect, None, "the new window has not laid out its canvas yet");
        app.enter_deck(first);
        assert_eq!((app.canvas_rect, app.canvas_scale), (Some(rect), 0.5));
    }

    #[test]
    fn opening_an_open_file_brings_it_forward_instead_of_a_copy() {
        let dir = std::env::temp_dir().join(format!("deckcraft-reopen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.pptx").to_string_lossy().to_string();
        let mut app = SlideApp::new(Session::new(), Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("file.saveAs", json!({"path": path})).unwrap();
        let saved = app.session.active().unwrap().uid;
        app.run("file.new", json!({})).unwrap();
        app.open_path(&path).unwrap();
        app.open_path_in_background(&path);
        assert_eq!(app.session.documents().len(), 2, "no second copy");
        assert_eq!(app.session.active().map(|d| d.uid), Some(saved));
        assert!(app.opening.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn opening_a_file_leaves_the_other_window_in_its_view() {
        let dir = std::env::temp_dir().join(format!("deckcraft-open-view-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.pptx").to_string_lossy().to_string();
        let mut app = SlideApp::new(Session::new(), Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("file.saveAs", json!({"path": path})).unwrap();
        app.run("file.close", json!({})).unwrap();
        app.native_windows = true;
        app.run("file.new", json!({})).unwrap();
        app.follow_active_deck();
        let sorter = app.session.active().unwrap().uid;
        app.ui.view = ViewMode::Sorter;
        app.open_path(&path).unwrap();
        assert_eq!(app.ui.view, ViewMode::Normal, "the opened presentation shows in Normal view");
        app.enter_deck(sorter);
        assert_eq!(app.ui.view, ViewMode::Sorter, "the other window stays in Slide Sorter");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn each_window_keeps_its_own_reuse_slides_target() {
        let mut app = SlideApp::new(Session::new(), Services::default());
        app.native_windows = true;
        app.run("file.new", json!({})).unwrap();
        app.follow_active_deck();
        let first = app.session.active().unwrap().uid;
        let target = Some(FileTarget::Document { doc: first, place: FilePlace::Unspecified });
        app.reuse_target = target;
        app.run("file.new", json!({})).unwrap();
        app.follow_active_deck();
        assert_eq!(app.reuse_target, None, "the new window has no Reuse Slides picker open");
        app.reuse_target = Some(FileTarget::Absent);
        app.enter_deck(first);
        assert_eq!(app.reuse_target, target, "the first window's picker still inserts into it");
    }

    #[test]
    fn quitting_asks_each_unsaved_presentation_and_cancel_stops() {
        let ctx = egui::Context::default();
        let mut app = SlideApp::new(Session::new(), Services::default());
        app.native_windows = true;
        let mut dirty = Vec::new();
        for _ in 0..3 {
            app.run("file.new", json!({})).unwrap();
            app.follow_active_deck();
            dirty.push(app.session.active().unwrap().uid);
        }
        app.run("slide.new", json!({})).unwrap();
        let third = app.session.active().unwrap().uid;
        app.enter_deck(dirty[0]);
        app.run("slide.new", json!({})).unwrap();
        let first = dirty[0];
        app.request_quit(&ctx);
        // The first unsaved presentation asks; Don't Save closes it.
        assert_eq!(app.continue_quit(&ctx), Some(first));
        assert_eq!(app.dialog.as_ref().map(|d| d.id.as_str()), Some("close"));
        assert_eq!(app.continue_quit(&ctx), None, "waits for the answer");
        app.run("file.close", json!({})).unwrap();
        app.dialog = None;
        app.follow_active_deck();
        // Then the next one asks in its own window; Cancel stops the quit.
        assert_eq!(app.continue_quit(&ctx), Some(third));
        assert_eq!(app.session.active().map(|d| d.uid), Some(third));
        app.dialog = None;
        assert_eq!(app.continue_quit(&ctx), None);
        assert!(app.quitting.is_none() && !app.quit_confirmed);
        assert_eq!(app.continue_quit(&ctx), None, "a cancelled quit stays cancelled");
    }
}
