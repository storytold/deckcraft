use super::clock::{monotonic_secs, now_ms};
use super::ime_hooks::ime_in_ui_pass_key;
use super::services::{InboxEvent, Services};
use super::state::{FilePlace, FilePurpose, ImeDescriptorKey, ImeDestination, ImeTerminalStamp, LocatedPlace, RECENT_MAX, UiState, ViewMode};
#[cfg(not(target_arch = "wasm32"))]
use super::updates_ui;
use super::{
    AnimPreview, ControlRequest, FileTarget, HashMap, ImeCancelled, ImeComposition, ImeResetPhase, ImeTerminalPending, InputTarget, Mods,
    PendingFile, Perf, Receiver, Session, SlideApp, Target, UI_COMMANDS, UiRequest, Value, backstage, canvas, capture, dialogs, doc_window,
    eyedropper, json, media, menus, notes, panes, preview, qat, ribbon, screen, selpane, show, sorter, status, textures, theme, thumbs, toast,
    uifonts,
};
mod clipboard;
mod commands;
mod control;
pub(crate) mod files;
mod frame;
mod ime;
mod input;
mod keys;
mod playback;

#[cfg(test)]
pub(crate) use clipboard::{OBJECTS_MARKER, system_clipboard_text};
#[cfg(test)]
pub(crate) use files::web_image_files;
pub use keys::key_name;

/// How long a transient status message stays visible.
const STATUS_LINGER_MS: f64 = 6000.0;

/// What a logic pass does with a close request while a screen take is in flight.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ScreenClose {
    /// Nothing waits for a take: the normal close path runs.
    Normal,
    /// Stop the take if it still records, cancel this close, and wait for the insert.
    Wait,
    /// The take has landed: close again, so the normal path sees the now-dirty deck.
    Retry,
    /// The take landed but could not be inserted: keep the window (and the kept take's message)
    /// up instead of closing over it.
    KeepOpen,
}

/// What the close path sees when it decides about a screen take.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct ScreenCloseState {
    /// A take is recording.
    recording: bool,
    /// A take is being written (Stop or a failure asked for the file, or retained pictures drain).
    finishing: bool,
    /// The window was asked to close this pass.
    closing: bool,
    /// A close request is already waiting for a take to land.
    pending: bool,
    /// The finished take could not be inserted (its file is kept).
    insert_failed: bool,
    /// The user has confirmed the quit.
    quit_confirmed: bool,
}

/// The close path for a screen take: a take is part of the document only once the session has
/// inserted it, so a close request must wait for the encoder instead of dropping it.
fn screen_close(s: ScreenCloseState) -> ScreenClose {
    if s.quit_confirmed {
        // The user has already confirmed the quit: the take cannot be held back any longer.
        return ScreenClose::Normal;
    }
    if s.closing {
        // A fresh request supersedes a satisfied wait, and waits again while a take is in flight.
        return if s.recording || s.finishing { ScreenClose::Wait } else { ScreenClose::Normal };
    }
    if s.pending && !(s.recording || s.finishing) {
        // A failed insert leaves the take on disk with a message saying where: closing again would
        // take the message (and the retry's dirty-deck check) away from a deck that may be clean.
        return if s.insert_failed { ScreenClose::KeepOpen } else { ScreenClose::Retry };
    }
    ScreenClose::Normal
}

/// egui data key: a close request is waiting for a screen take to land. Kept in the context's
/// temporary data so the close path needs no extra [`SlideApp`] field.
fn pending_close_key() -> egui::Id {
    egui::Id::unique("deckcraft.pending_close")
}

impl SlideApp {
    /// Remove temporary PDFs created for printing.
    pub fn cleanup_print_previews(&mut self) {
        self.print_previews.cleanup();
    }

    /// Build application UI state, taking ownership of audio output and initialising media, dialogs,
    /// synthetic input, and timing fields.
    pub fn new(mut session: Session, mut services: Services) -> Self {
        let audio_out = services.audio_out.take();
        session.set_screen_host(services.screen_in.take());
        // The window's own controls are the user's; automation transports clear this while they
        // run a remote command (screen capture starts only here).
        session.set_ui_caller(true);
        SlideApp {
            last_fill: serde_json::json!("accent1"),
            last_outline: serde_json::json!("tx1"),
            table_pen: serde_json::json!({"width": 1.0}),
            session,
            services,
            screen: screen::ScreenUi::default(),
            ui: UiState::default(),
            textures: textures::Textures::default(),
            control_rx: None,
            squiggles: Default::default(),
            dialog: None,
            thesaurus_query: String::new(),
            show: None,
            anim_preview: None,
            reuse_target: None,
            media: media::MediaHost::new(audio_out),
            status: None,
            opening: None,
            toast: None,
            fonts_noted: Default::default(),
            perf: Perf { frame_ms: 0.0, render_ms: 0.0, fps: 0.0 },
            last_ctx: None,
            synthetic: vec![],
            synthetic_pending: false,
            synthetic_next: vec![],
            synthetic_next_target: None,
            synthetic_injected: vec![],
            synthetic_injected_at: None,
            synthetic_generation: 0,
            synthetic_target: None,
            synthetic_focus: None,
            synthetic_injected_time: None,
            ribbon_scroll: HashMap::new(),
            show_instance: 0,
            opening_generation: 0,
            canvas_rect: None,
            eyedropper: None,
            canvas_scale: 1.0,
            slide_rect: None,
            canvas_region: None,
            palette: None,
            integrated_titlebar: false,
            backstage: None,
            pending_urls: vec![],
            print_previews: files::PrintPreviews::default(),
            #[cfg(not(target_arch = "wasm32"))]
            updates_rt: updates_ui::Runtime::default(),
            pending_copy: None,
            capture: capture::Capture::default(),
            styled: false,
            restyle: true,
            fonts_ready: false,
            last_time: 0.0,
            last_recovery: 0.0,
            quit_confirmed: false,
            notes_buf: (0, usize::MAX, notes::Draft::default()),
            ui_fonts: uifonts::UiFonts::default(),
            thumb_drag: None,
            selpane: selpane::State::default(),
            align_to: None,
            doc_window: None,
            sorter: sorter::State::default(),
            slides_focused: false,
            v_seen: false,
            recent_checked: HashMap::new(),
            twisting: false,
            next_file_id: 1,
            pending_files: Vec::new(),
            host_start: None,
            ime_tracker: egui::ime_context::ImeContextTracker::new(),
            ime_pending: None,
            ime_frame_viewport: None,
            ime_viewport: None,
            ime_composition: None,
            ime_terminal_pending: None,
            ime_cancelled: None,
            ime_generation: 0,
            ime_settles_seen: 0,
            ime_interrupt_pending: false,
            ime_reset: HashMap::new(),
        }
    }

    pub fn with_control(mut self, rx: Receiver<ControlRequest>) -> Self {
        self.control_rx = Some(rx);
        self
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), now_ms()));
    }

    /// Turn on the automatic check for a newer release at start-up (desktop shell only; tests and
    /// the web build never touch the network on their own).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn enable_update_checks(&mut self) {
        updates_ui::enable(self);
    }

    /// A message that must be seen wherever the user is: a toast, which also shows over the start
    /// screen and the File tab, where there is no status bar.
    pub fn notify(&mut self, msg: impl Into<String>) {
        self.toast = Some(toast::Toast { text: msg.into(), at: now_ms() });
    }

    /// The transient status message while it is fresh; clears it once stale. Both the status bar
    /// and the start screen draw it — the start screen has no status bar, and errors raised there
    /// (a missing recent file, a failed sample) used to vanish (issue #172).
    pub fn fresh_status(&mut self) -> Option<String> {
        let (msg, at) = self.status.clone()?;
        if now_ms() - at < STATUS_LINGER_MS {
            Some(msg)
        } else {
            self.status = None;
            None
        }
    }

    /// Return the current document count plus the active document uid, so callers can detect a
    /// replacement document rather than only an index change.
    pub(crate) fn docs_key(&self) -> (usize, Option<u64>) {
        // The document uid, not just its index: closing one presentation and opening another
        // must not look like the same document to the Backstage view or a queued input batch.
        (self.session.documents().len(), self.session.active().map(|d| d.uid))
    }

    /// The File tab's view is open over a presentation (not just the start screen with nothing
    /// open).
    pub fn backstage_open(&self) -> bool {
        self.backstage.is_some() && self.session.active().is_some()
    }

    /// Opens the File tab's view on `page` (keeping its state when it is already open).
    pub fn open_backstage(&mut self, page: backstage::Page) {
        let key = self.docs_key();
        match &mut self.backstage {
            Some(b) => b.go(page),
            None => self.backstage = Some(backstage::Backstage::new(page, key)),
        }
    }

    /// Back to the presentation (no effect on the start screen, where nothing is open).
    pub fn close_backstage(&mut self) {
        if self.session.active().is_some() {
            self.backstage = None;
        }
    }

    /// Keeps the File tab's view in step with the open presentations: it closes when one is
    /// opened, created or closed (the result shows), and it is the start screen when none is open.
    fn sync_backstage(&mut self) {
        let key = self.docs_key();
        let has_doc = self.session.active().is_some();
        let stale = self.backstage.as_ref().is_some_and(|b| b.docs != key);
        if has_doc && stale {
            self.backstage = None;
        } else if !has_doc && (stale || self.backstage.is_none()) {
            self.backstage = Some(backstage::Backstage::new(backstage::Page::Home, key));
        }
    }

    /// A screen take is still on the encoder: recording, or having its file written after Stop.
    /// Either state needs [`Session::screen_tick`], since the finished take is only inserted there.
    fn screen_take_busy(&self) -> bool {
        self.session.screen.is_recording() || self.session.screen.is_finishing()
    }

    /// A close request is waiting for a screen take to land (see [`Self::logic`]).
    fn pending_close(&self, ctx: &egui::Context) -> bool {
        ctx.data(|d| d.get_temp::<bool>(pending_close_key()).unwrap_or(false))
    }

    fn set_pending_close(&self, ctx: &egui::Context, pending: bool) {
        ctx.data_mut(|d| d.insert_temp(pending_close_key(), pending));
    }

    /// Background work, run on every logic call (visible, hidden or `run_logic`).
    ///
    /// Only consumed queues and monotonic deadlines live here: control requests, asynchronous
    /// opening/inbox results, screenshot completions/deadlines, AutoRecover pacing and media
    /// clocks. UI events are handled in [`Self::ui`], where a forced hidden paint also arrives.
    pub fn logic(&mut self, ctx: &egui::Context) {
        if !Self::root_viewport_admitted(ctx) {
            log::debug!("ignoring SlideApp::logic for render-only viewport {:?}", ctx.viewport_id());
            return;
        }
        self.last_ctx = Some(ctx.clone());
        self.tick_hidden_show(ctx, web_time::Instant::now());
        // `raw_input_hook` runs before `App::logic`. In a full pass egui has already built
        // `PointerState` and `keys_down` from every injected event, so retracting the events after
        // this pass would not stop the derived click or key from reaching the UI. Defer instead:
        // a control request, background-open result or inbox file that changes the document,
        // target or modal owner waits until the admitted batch has been consumed by the old
        // target's UI pass. A logic-only callback carries the RawInput into the next full pass
        // (no live `InputState` here), so that pass still drains normally and the next
        // `raw_input_hook` retracts a batch that went stale.
        if !self.synthetic_batch_is_live(ctx) {
            self.drain_control(ctx);
            self.poll_opening();
            #[cfg(not(target_arch = "wasm32"))]
            updates_ui::tick(self, ctx);
            // Inbox results are host file bytes and do not need fonts or a painted frame. A
            // window that starts hidden must still apply a file picked before its first UI pass.
            self.drain_inbox();
        }
        self.drain_screenshots();
        // `raw_input_hook` ran before this logic pass. This is normally a no-op now that
        // target-changing work is deferred for a live batch; it still covers a target change
        // made by other logic, and keeps a logic-only batch pending for the next hook.
        self.retract_synthetic_after_logic(ctx);
        // Queued/settling screenshots force the bounded hidden paints they need; issued ones
        // rely on the screenshot command and the completion callback, with only a deadline wake.
        self.capture.request_paint_if_needed(ctx, web_time::Instant::now());
        // A take keeps the encoder busy until its file is written: tick it while it records or
        // finishes, so a stopped take is collected and inserted instead of being left behind in
        // the worker. The repaint keeps the UI live while the worker writes the file.
        if self.screen_take_busy() {
            self.session.screen_tick(monotonic_secs());
            // While a take captures, wake at its own interval instead of a fixed 60 fps: the
            // encoder has nothing new to give faster than that. Finishing is a short wait for the
            // worker's file, so it keeps the quick repaint.
            let ms = if self.session.screen.is_recording() { u64::from(1000 / self.session.screen.fps().max(1)).max(16) } else { 16 };
            ctx.request_repaint_after(std::time::Duration::from_millis(ms));
        }
        if !self.synthetic.is_empty() || !self.synthetic_next.is_empty() || self.synthetic_pending {
            ctx.request_repaint();
        }
        let mut closing = ctx.input(|i| i.viewport().close_requested()) && !self.quit_confirmed;
        // A screen recording is not part of the document until it is inserted: stop it here, so
        // closing the window keeps the take and the insert makes the deck dirty (which then asks
        // the user), instead of dropping the worker with an unfinished MP4 in the temp folder.
        match screen_close(ScreenCloseState {
            recording: self.session.screen.is_recording(),
            finishing: self.session.screen.is_finishing(),
            closing,
            pending: self.pending_close(ctx),
            insert_failed: self.session.screen_insert_failed,
            quit_confirmed: self.quit_confirmed,
        }) {
            ScreenClose::Wait => {
                // If the take cannot be inserted (no slide to put it on, say), keep the window
                // open: the message and the kept MP4 stay visible instead of the app closing over
                // them.
                let stopped = !self.session.screen.is_recording()
                    || match self.session.execute("show.screenRecord", &serde_json::json!({"action": "stop"})) {
                        Ok(_) => true,
                        Err(e) => {
                            self.set_status(e.to_string());
                            self.set_pending_close(ctx, false);
                            // The status bar is hidden during a slide show: keep the recorder's
                            // window up, which shows the message (and the kept take's path).
                            self.screen.open = true;
                            // Nothing was inserted, so the deck may well be clean: a close that
                            // went ahead would take the message and the kept take with it.
                            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                            closing = false;
                            false
                        }
                    };
                if stopped {
                    // A take that is still being written must land first: the insert dirties the
                    // deck, so the close is retried after it.
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.set_pending_close(ctx, true);
                    closing = false;
                }
            }
            ScreenClose::Retry => {
                // The take has landed: close again, which re-enters the normal close path now
                // that the insert has made the deck dirty.
                self.set_pending_close(ctx, false);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ScreenClose::KeepOpen => {
                // The take could not be inserted (the presentation was closed during the
                // recording, say): its MP4 is kept and the message says where. Closing now would
                // take both with it, so drop the wait and leave the app running with the
                // recorder's window up — the status bar is hidden in a show, and that window
                // shows the message (and the kept take's path). The repaint paints it now that
                // no further frame is otherwise scheduled.
                self.set_pending_close(ctx, false);
                self.screen.open = true;
                ctx.request_repaint();
            }
            ScreenClose::Normal => {
                if closing {
                    // A fresh request (or a confirmed quit) owns the close from here.
                    self.set_pending_close(ctx, false);
                }
            }
        }
        // Closing during a take must not lose it, even when the deck itself is clean: nothing is
        // written until the take is saved, so end the show before the unsaved-changes check.
        if closing
            && let Some(now) = self.show.as_ref().filter(|s| s.take_captured()).map(|s| s.command_now())
            && let Some(show) = self.show.take()
            && let Some(doc) = self.session.active().map(|d| d.doc.clone())
        {
            self.end_show(ctx, show, &doc, now);
        }
        // Closing the window with unsaved changes asks first.
        if closing && self.session.documents().iter().any(|d| d.is_dirty()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.stop_mic();
            self.show = None;
            if self.dialog.as_ref().is_none_or(|d| d.id != "quit") {
                self.dialog = Some(dialogs::Dialog::new("quit"));
            }
        }
        self.autosave_if_due(ctx);
        self.tick_media_clock(ctx);
        self.note_settled_composition(ctx);
        // In a full run_ui pass the UI callback after this logic half still owns the pass's final
        // IME descriptor. Only a standalone run_logic callback proves that no custom editor was
        // painted for this tick. Do not infer this from cumulative frame counters or stale
        // geometry.
        if !self.ime_in_ui_pass(ctx) {
            self.finalize_ime_logic(ctx);
        }
    }

    /// Lay out the whole window.
    ///
    /// Every return route, including fonts/layout discards, show mode, modal/Backstage and the
    /// no-document paths, reaches the same outer reset observation and finalizer below. No early
    /// route may publish or inspect IME output on its own.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !Self::root_viewport_admitted(&ctx) {
            log::debug!("ignoring SlideApp::ui for render-only viewport {:?}", ctx.viewport_id());
            return;
        }
        self.ui_inner(ui);
        // The last writer (all panels, dialogs, the custom producer and deferred child windows)
        // has run. Observe the actual custom descriptor for this pass, then reconcile the reset
        // lifetime against the actual committed output. This also runs for discarded passes.
        self.ime_frame_viewport = Some(ctx.viewport_id());
        let custom = self.observe_ime(&ctx);
        self.finalize_ime_reset(&ctx, custom);
    }

    /// Run the admitted root UI pass, including setup gating, panels, canvas, dialogs, overlays, and
    /// show mode.
    fn ui_inner(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.ime_frame_viewport = Some(ctx.viewport_id());
        self.ime_pending = None;
        if !self.ui_setup(&ctx) {
            // This pass is discarded for the new fonts. The input events were already consumed
            // by egui, so keep them queued for the next frame instead of silently losing them.
            self.requeue_injected_synthetic();
            return;
        }
        let t0 = now_ms();
        let t = theme::Tokens::get(&ctx);
        self.note_ui_text(&ctx);
        self.media.sync_video_textures(&ctx);
        canvas::trackpad_twist(self, &ctx);
        eyedropper::tick(self, &ctx);
        self.opening_ui(&ctx);
        if self.show.is_some() {
            self.anim_preview = None;
            show::ui(self, ui);
            // Requests a command (or the take's tick) queued become the status text below: drain
            // them before the recorder's window reads it, or the kept file's path stays invisible.
            self.drain_requests();
            doc_window::show(self, &ctx);
            // A screen recording started before the show keeps its control window, so "Stop and
            // insert" is reachable while the show runs.
            screen::window(self, &ctx);
            self.perf.frame_ms = now_ms() - t0;
            self.ui_end(&ctx);
            return;
        }
        self.sync_backstage();
        self.keyboard(&ctx);
        self.sync_backstage();
        ribbon::title_bar(self, ui);
        if self.backstage.is_none() && self.session.active().is_some() {
            ribbon::show(self, ui);
            status::status_bar(self, ui);
        }
        let now = ui.input(|i| i.time);
        if self.tick_anim_preview(now) {
            ctx.request_repaint();
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
            if self.backstage.is_some() {
                // File tab: the Backstage view replaces everything below the title bar.
                backstage::show(self, ui);
                return;
            }
            if self.session.active().is_none() {
                return;
            }
            match self.ui.view {
                ViewMode::Sorter => sorter::show(self, ui),
                ViewMode::NotesPage => sorter::notes_page(self, ui),
                ViewMode::Outline | ViewMode::Normal | ViewMode::Reading => {
                    if self.ui.pane.is_some() {
                        panes::show(self, ui);
                    }
                    if self.ui.view == ViewMode::Outline {
                        sorter::outline_pane(self, ui);
                    } else {
                        thumbs::show(self, ui);
                    }
                    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.canvas)).show(ui, |ui| {
                        if self.ui.notes {
                            status::notes_pane(self, ui);
                        }
                        canvas::show(self, ui);
                    });
                }
            }
        });
        toast::note_missing_fonts(self);
        toast::show(self, &ctx);
        dialogs::show(self, &ctx);
        screen::window(self, &ctx);
        #[cfg(not(target_arch = "wasm32"))]
        updates_ui::show(self, &ctx);
        menus::palette(self, &ctx);
        for url in std::mem::take(&mut self.pending_urls) {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        if let Some(t) = self.pending_copy.take() {
            ctx.copy_text(t);
        }
        self.drain_requests();
        doc_window::show(self, &ctx);
        self.perf.frame_ms = now_ms() - t0;
        self.ui_end(&ctx);
    }

    pub fn open_url(&mut self, url: &str) {
        self.pending_urls.push(url.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::{ScreenClose, ScreenCloseState, screen_close};

    /// The state a test asks about: named fields, so no two flags can be swapped by position.
    fn state(recording: bool, finishing: bool, closing: bool, pending: bool, insert_failed: bool, quit_confirmed: bool) -> ScreenCloseState {
        ScreenCloseState { recording, finishing, closing, pending, insert_failed, quit_confirmed }
    }

    #[test]
    fn close_waits_for_a_take_in_flight() {
        // A close request while the take records or finishes is held back for the insert.
        assert_eq!(screen_close(state(true, false, true, false, false, false)), ScreenClose::Wait);
        assert_eq!(screen_close(state(false, true, true, false, false, false)), ScreenClose::Wait);
        // The take has landed: close again, so the normal path sees the dirty deck.
        assert_eq!(screen_close(state(false, false, false, true, false, false)), ScreenClose::Retry);
        // A fresh request or a confirmed quit runs the normal path (and clears the wait).
        assert_eq!(screen_close(state(false, false, true, true, false, false)), ScreenClose::Normal);
        assert_eq!(screen_close(state(false, false, false, true, false, true)), ScreenClose::Normal);
        // A pending wait keeps waiting quietly while the take is still in flight.
        assert_eq!(screen_close(state(false, true, false, true, false, false)), ScreenClose::Normal);
        assert_eq!(screen_close(state(false, false, false, false, false, false)), ScreenClose::Normal);
    }

    #[test]
    fn a_failed_insert_keeps_the_window_up() {
        // The take has landed but its insert failed: no retry may close a deck that the kept take
        // never touched, so the wait is dropped and the recorder's message stays reachable.
        assert_eq!(screen_close(state(false, false, false, true, true, false)), ScreenClose::KeepOpen);
        // While the take is still in flight the insert cannot have failed yet: wait as usual.
        assert_eq!(screen_close(state(false, true, false, true, true, false)), ScreenClose::Normal);
        assert_eq!(screen_close(state(true, false, true, false, true, false)), ScreenClose::Wait);
        // The flag alone never holds a fresh close: only a waiting close reads it.
        assert_eq!(screen_close(state(false, false, true, false, true, false)), ScreenClose::Normal);
        assert_eq!(screen_close(state(false, false, false, false, true, false)), ScreenClose::Normal);
        // A confirmed quit is the user's decision, even over a kept take.
        assert_eq!(screen_close(state(false, false, false, true, true, true)), ScreenClose::Normal);
    }
}
