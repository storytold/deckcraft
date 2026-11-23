//! A native window for each open presentation (desktop).
//!
//! The engine's session holds the open presentations and which one is active. [`DocWindows`]
//! keeps one OS window per presentation: the first is eframe's root window, the others are egui
//! immediate viewports. Each window is drawn with its presentation active and its own interface
//! state swapped in ([`crate::window_ui`]), so File › New, Open, Open Recent and files opened
//! from Finder each get a window, and closing a window closes its presentation (asking first
//! when it has unsaved changes).

use craft_window::{Frame, Viewports, WindowEvent, WindowId, WindowList};
use egui::{ViewportBuilder, ViewportCommand, ViewportId};

use crate::SlideApp;

/// The open presentations' windows (see the module docs). Each window holds its presentation's
/// engine uid.
pub struct DocWindows {
    list: WindowList<u64>,
    viewports: Viewports,
    /// The builder for a new window: the app's own window options.
    base: ViewportBuilder,
    /// The largest inner size a new window opens at, once the screen is known (see [`Self::show`]).
    fit: Option<egui::Vec2>,
}

impl DocWindows {
    /// Windows built like `base`, and the app switched to a window per presentation.
    pub fn new(app: &mut SlideApp, base: ViewportBuilder) -> Self {
        app.native_windows = true;
        DocWindows { list: WindowList::new(), viewports: Viewports::new(Frame::Desktop), base, fit: None }
    }

    /// The presentation each window shows, most recently focused first.
    pub fn decks(&self) -> Vec<u64> {
        self.list.recent().filter_map(|id| self.list.get(id).copied()).collect()
    }

    /// The viewport that draws presentation `uid`'s window.
    pub fn viewport_of(&self, uid: u64) -> Option<ViewportId> {
        let id = self.list.find(|w| *w == uid)?;
        Some(if self.viewports.root() == Some(id) { ViewportId::ROOT } else { self.viewports.viewport_id(id) })
    }

    /// Match the windows to the open presentations and bring the active one's window forward.
    fn sync(&mut self, app: &mut SlideApp, ctx: &egui::Context) {
        let open: Vec<u64> = app.session.documents().iter().map(|d| d.uid).collect();
        let gone: Vec<WindowId> = self.list.iter().filter(|(_, w)| !open.contains(w)).map(|(id, _)| id).collect();
        for id in gone {
            self.list.close(id);
            if self.viewports.root() == Some(id) {
                // The root window stays open as the next window, or as the empty app window.
                match self.list.active() {
                    Some(next) => self.viewports.adopt(ctx, &self.list, next),
                    None => self.viewports.set_root(None),
                }
            }
        }
        for uid in open {
            if self.list.find(|w| *w == uid).is_none() {
                self.list.open(uid);
            }
        }
        if self.viewports.root().is_none() {
            self.viewports.set_root(self.list.active());
        }
        if let Some(active) = app.session.active().map(|d| d.uid)
            && let Some(id) = self.list.find(|w| *w == active)
            && self.list.active() != Some(id)
        {
            self.list.focus(id);
            self.viewports.focus(ctx, id);
        }
    }

    /// Draw every window. Called once per frame from the root window's UI pass.
    pub fn show(&mut self, app: &mut SlideApp, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut requested = app.continue_quit(&ctx);
        if let Some(uid) = requested {
            app.enter_deck(uid);
        }
        self.sync(app, &ctx);
        if self.list.is_empty() {
            // No presentation: the root window shows the app without one.
            app.ui(ui);
            self.sync(app, &ctx);
            return;
        }
        let titles: std::collections::HashMap<u64, String> =
            app.session.documents().iter().map(|d| (d.uid, crate::window_ui::window_title(d))).collect();
        // craft-window's cascade puts a window that doesn't fit at the screen's corner, which is
        // exactly on a screen-filling window before it: a new window is a few steps smaller than
        // the screen, so it opens below and right of the last one. Kept once known, since a
        // changed builder size would resize the open windows.
        self.fit = self.fit.or_else(|| {
            ctx.input(|i| {
                let v = i.viewport();
                let frame = v.outer_rect?.size() - v.inner_rect?.size();
                Some(v.monitor_size? - frame - egui::Vec2::splat(3.0 * craft_window::CASCADE_STEP))
            })
        });
        let (base, fit) = (&self.base, self.fit);
        let events = self.viewports.show(
            ui,
            &mut self.list,
            |_, w| {
                let builder = base.clone().with_title(titles.get(w).cloned().unwrap_or_default());
                match (builder.inner_size, fit) {
                    (Some(size), Some(fit)) => builder.with_inner_size(size.min(fit)),
                    _ => builder,
                }
            },
            |ui, _, w| {
                app.enter_deck(*w);
                let viewport = ui.ctx().viewport_id();
                app.window_viewport = (viewport != ViewportId::ROOT).then_some(viewport);
                if app.window_viewport.is_some() {
                    // The root's dropped files arrive through `raw_input_hook`; a child window's
                    // input reaches only its own pass.
                    for f in ui.ctx().input(|i| i.raw.dropped_files.clone()) {
                        let name = f.path().file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "dropped file".into());
                        match f.bytes() {
                            Ok(bytes) => app.open_or_insert_bytes(&name, &bytes),
                            Err(e) => app.set_status(e),
                        }
                    }
                }
                // Each presentation's widget memory (scroll positions…) stays its own, also when
                // another presentation takes over the root window.
                ui.push_id(("deckcraft.deck", *w), |ui| app.ui(ui));
                app.window_viewport = None;
                // A command in this window made another presentation active (File › New, Open,
                // Next Window): that window comes forward.
                if let Some(active) = app.session.active().map(|d| d.uid).filter(|a| a != w) {
                    requested = Some(active);
                }
            },
        );
        for event in events {
            if let WindowEvent::CloseRequested(id) = event
                && let Some(uid) = self.close_requested(app, &ctx, id)
            {
                requested = Some(uid);
            }
        }
        if let Some(uid) = requested.or_else(|| self.list.active().and_then(|id| self.list.get(id).copied())) {
            app.enter_deck(uid);
        }
        self.sync(app, &ctx);
    }

    /// The user closed window `id`: close its presentation, asking in that window when it has
    /// unsaved changes. Closing the last window quits, except on macOS, where the app stays open.
    /// Returns the presentation whose window should come forward.
    fn close_requested(&mut self, app: &mut SlideApp, ctx: &egui::Context, id: WindowId) -> Option<u64> {
        if app.quit_confirmed {
            return None;
        }
        if self.viewports.root() == Some(id) {
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::CancelClose);
        }
        if self.list.len() == 1 && !cfg!(target_os = "macos") {
            app.request_quit(ctx);
            return None;
        }
        let uid = *self.list.get(id)?;
        app.enter_deck(uid);
        app.close_presentation();
        app.session.documents().iter().any(|d| d.uid == uid).then_some(uid)
    }
}
