use super::{SlideApp, json, media, monotonic_secs, theme};

impl SlideApp {
    /// Style, fonts and zoom setup. Runs in a real UI pass, not from `run_logic`, because
    /// `set_fonts`/`set_visuals` only take effect for the pass that lays out the UI.
    pub(super) fn ui_setup(&mut self, ctx: &egui::Context) -> bool {
        let scale = self.ui.ui_scale.clamp(0.5, 3.0);
        if (ctx.zoom_factor() - scale).abs() > 1e-3 {
            ctx.set_zoom_factor(scale);
        }
        if !self.styled {
            theme::install_fonts_with(ctx, &self.ui_fonts.faces);
            self.styled = true;
            self.restyle = true;
            // Font definitions take effect at the next pass boundary. Do not paint a language
            // or symbol with the old font set while the new set is still pending.
            ctx.request_discard("install interface fonts before layout");
            return false;
        }
        if self.restyle {
            theme::apply(ctx, &theme::Tokens::for_brightness(self.ui.brightness));
            self.restyle = false;
        }
        self.fonts_ready = true;
        true
    }

    /// AutoRecover on a monotonic deadline, even when nothing is painted.
    pub(super) fn autosave_if_due(&mut self, ctx: &egui::Context) {
        if self.session.recovery_dir.is_none() {
            return;
        }
        let every = (self.session.prefs.recovery_minutes * 60.0).max(10.0);
        let now = monotonic_secs();
        if now - self.last_recovery > every {
            self.last_recovery = now;
            if self.session.documents().iter().any(|d| d.is_dirty())
                && let Err(e) = self.session.execute("file.recovery.save", &json!({}))
            {
                self.set_status(format!("Couldn't save AutoRecover information: {e}"));
            }
        }
        if self.session.documents().iter().any(|d| d.is_dirty()) {
            let wait = (every - (monotonic_secs() - self.last_recovery)).clamp(0.5, every);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
    }

    /// Issue due screenshots and finish pass-local UI bookkeeping. Call once at the end of a real
    /// pass after layout; the outer [`Self::ui`] wrapper observes IME output after this returns.
    pub(super) fn ui_end(&mut self, ctx: &egui::Context) {
        self.ime_frame_viewport = None;
        self.capture.observe_ui(ctx);
        if !ctx.will_discard() {
            let doc = self.session.active().map(|d| d.uid);
            self.capture.issue(ctx, doc, web_time::Instant::now());
        }
        if self.fonts_ready && !ctx.will_discard() {
            self.synthetic_pending = false;
            self.synthetic_injected.clear();
            self.synthetic_injected_at = None;
            self.synthetic_injected_time = None;
            self.synthetic_focus = None;
            if self.synthetic.is_empty() {
                self.synthetic_target = None;
            }
            self.promote_next_synthetic();
        }
        if self.fonts_ready && !ctx.will_discard() {
            let now = ctx.input(|i| i.time);
            let dt = now - self.last_time;
            if dt > 0.0 {
                self.perf.fps = self.perf.fps * 0.9 + (1.0 / dt).min(240.0) * 0.1;
            }
            self.last_time = now;
        }
    }

    /// Media clocks. Voices belong to the open presentation: closing the last one, or switching
    /// decks, stops them. The editor's media also stops when its slide is left. This runs from
    /// `logic` on every call so a minimized window keeps playing; texture upload stays in `ui`.
    pub(super) fn tick_media_clock(&mut self, ctx: &egui::Context) {
        let uid = self.session.active().map(|d| d.uid);
        self.media.set_document(uid);
        if !self.media.any_active() {
            self.session.media_status.clear();
            return;
        }
        if self.show.is_none()
            && let Some(d) = self.session.active()
        {
            let here: Vec<deckcraft_model::ShapeId> = if d.selection.target == deckcraft_engine::Target::Slides {
                media::slide_media(&d.doc, d.selection.slide).into_iter().map(|m| m.0).collect()
            } else {
                vec![]
            };
            let gone: Vec<_> =
                self.media.ids().into_iter().filter(|id| !here.contains(id) && self.media.owner(*id) == Some(media::Owner::Editor)).collect();
            for id in gone {
                self.media.stop(id);
            }
        }
        let playing = self.media.tick_clock(monotonic_secs(), &mut self.session);
        if playing {
            ctx.request_repaint_after(std::time::Duration::from_millis(15));
        }
    }
}
