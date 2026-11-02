use super::{AnimPreview, SlideApp, Value, ViewMode, json, media, preview, show};

impl SlideApp {
    /// Whether ⌘A should act on the slide list: always in the sorter, in Normal view once the
    /// thumbnail pane has been clicked.
    pub(crate) fn slides_list_focused(&self) -> bool {
        self.ui.view == ViewMode::Sorter || (self.ui.view == ViewMode::Normal && self.slides_focused)
    }

    /// First slide Play from Start should open: the custom show or range, skipping hidden slides.
    pub fn show_start_index(&self) -> usize {
        self.session.active().and_then(|d| deckcraft_anim::show_order(&d.doc).first().copied()).unwrap_or(0)
    }

    /// The slide `show.start {from}` names: `Ok(None)` when absent, `Err` when it is not a slide of
    /// the open deck (negative, non-integer, past the end), so a bad index never plays slide 0 or
    /// opens on the end screen. Hidden slides are allowed, as in "from current slide".
    pub(crate) fn show_from(&self, p: &Value) -> Result<Option<usize>, String> {
        let from = match p.get("from") {
            None | Some(Value::Null) => return Ok(None),
            Some(v) => v.as_u64().and_then(|v| usize::try_from(v).ok()).ok_or_else(|| format!("`from` is not a valid index: {v}"))?,
        };
        let count = self.session.active().map_or(0, |d| d.doc.slides.len());
        if from >= count {
            return Err(format!("no slide {from}"));
        }
        Ok(Some(from))
    }

    /// Leave animation preview, stop editor-owned media, and create a show at the requested slide
    /// with the current reading/presenter configuration; an out-of-range slide starts an ended show.
    pub fn start_show(&mut self, from: usize, reading: bool) {
        self.anim_preview = None;
        self.media.stop_owned(media::Owner::Editor);
        if let Some(d) = self.session.active() {
            self.show_instance = self.show_instance.wrapping_add(1).max(1);
            let mut show = show::Show::new(&d.doc, from, reading, self.ui.presenter_view && !reading);
            show.instance = self.show_instance;
            show.keep_cursor = self.ui.show_keep_cursor;
            self.show = Some(show);
        }
    }

    /// Play custom show `name` from its first slide. Set Up Slide Show is left as it was.
    pub fn start_custom_show(&mut self, name: &str, reading: bool) {
        let state = self.session.active().and_then(|d| deckcraft_anim::ShowState::play_custom(&d.doc, name));
        let Some(state) = state else { return };
        let from = state.slide;
        self.start_show(from, reading);
        if let Some(show) = self.show.as_mut() {
            show.state = state;
        }
    }

    /// The current slide's animations are playing on the editor canvas.
    pub fn anim_preview_active(&self) -> bool {
        self.anim_preview.is_some()
    }

    /// `(slide, seconds played, sequence length)` while a preview is running.
    pub fn anim_preview_report(&self) -> Option<(usize, f64, f64)> {
        self.anim_preview.as_ref().map(|p| (p.slide, p.elapsed, p.timeline.total_duration()))
    }

    pub fn stop_anim_preview(&mut self) {
        self.anim_preview = None;
    }

    /// Timeline and playhead for this frame's canvas raster. `None` when the editor should draw the slide at rest.
    pub fn anim_preview_draw(&self) -> Option<(deckcraft_anim::Timeline, f64)> {
        self.anim_preview.as_ref().map(|p| (p.timeline.clone(), p.elapsed))
    }

    /// Advance the preview clock. Stops when the slide changes, the slide is edited, or the sequence
    /// (plus a short hold) has finished.
    pub fn tick_anim_preview(&mut self, now: f64) -> bool {
        let Some(p) = self.anim_preview.as_ref() else { return false };
        let (uid, slide, ptr) = (p.uid, p.slide, p.slide_ptr);
        let alive = self.show.is_none()
            && self.session.active().is_some_and(|d| {
                d.uid == uid
                    && d.selection.target == deckcraft_engine::Target::Slides
                    && d.selection.slide == slide
                    && d.doc.slides.get(slide).is_some_and(|s| std::sync::Arc::as_ptr(s) as usize == ptr)
            });
        if !alive {
            self.anim_preview = None;
            return false;
        }
        let Some(p) = self.anim_preview.as_mut() else { return false };
        let now = if now.is_finite() { now } else { p.started.unwrap_or(0.0) };
        let start = *p.started.get_or_insert(now);
        let elapsed = p.from + (now - start).max(0.0);
        if !elapsed.is_finite() || elapsed > p.timeline.total_duration() + preview::HOLD_SECS {
            self.anim_preview = None;
            return false;
        }
        p.elapsed = elapsed;
        true
    }

    /// Play the current slide's animations on the canvas, from effect `from` (index into the slide's
    /// animations) or from the start.
    pub(super) fn start_anim_preview(&mut self, from: Option<usize>) -> Result<Value, String> {
        let (uid, slide, ptr, timeline) = {
            let Some(d) = self.session.active() else {
                return Err("no presentation is open".into());
            };
            if d.selection.target != deckcraft_engine::Target::Slides {
                return Err("animation preview plays the current slide".into());
            }
            let slide = d.selection.slide;
            let Some(slide_arc) = d.doc.slides.get(slide) else {
                return Err("no slide to preview".into());
            };
            let ptr = std::sync::Arc::as_ptr(slide_arc) as usize;
            let timeline = deckcraft_anim::ShowState::timeline(&d.doc, slide);
            (d.uid, slide, ptr, timeline)
        };
        if timeline.is_empty() {
            self.anim_preview = None;
            self.set_status("This slide has no animations");
            return Ok(json!({"playing": false}));
        }
        let from_secs = match from {
            Some(i) => timeline.start_of(i).ok_or_else(|| format!("no effect {i} in this slide's click sequence"))?,
            None => 0.0,
        };
        if matches!(self.ui.view, ViewMode::Sorter | ViewMode::NotesPage) {
            self.ui.view = ViewMode::Normal;
        }
        self.close_backstage();
        let duration = timeline.total_duration();
        self.anim_preview = Some(AnimPreview { uid, slide, slide_ptr: ptr, started: None, from: from_secs, elapsed: from_secs, timeline });
        self.set_status("Previewing animations");
        Ok(json!({"playing": true, "slide": slide, "duration": duration, "from": from_secs}))
    }

    /// Keep a hidden show playing from logic alone. `run_logic` does not refresh
    /// `InputState::time`, so the show anchors its last visible time to this monotonic instant
    /// and advances from there. A forced hidden paint draws the current state but does not
    /// advance it a second time, and no paused-show product policy is invented.
    pub(crate) fn tick_hidden_show(&mut self, ctx: &egui::Context, now: web_time::Instant) {
        let visible = show::viewport_visible(ctx);
        let Some(doc) = self.session.active().map(|d| d.doc.clone()) else { return };
        let Some(show) = self.show.as_mut() else { return };
        if visible {
            // A visible logic-only pass (`run_logic`) carries the last full frame's input time.
            // Anchoring that stale time to `now` would move the hidden clock backwards if the
            // window hides before the next full pass. `show::ui` already records the anchor on
            // every visible full pass, where `InputState::time` is current.
            return;
        }
        if !show.hidden_started() {
            return;
        }
        let input_time = ctx.input(|i| i.time);
        let show_time = show.hidden_logic_time(input_time, now);
        let keep = show.advance_hidden(&doc, show_time);
        let time_driven = show.hidden_time_driven(&doc);
        if !keep {
            self.show = None;
            return;
        }
        if time_driven {
            // A hidden event loop has no draw to piggyback on. A delayed repaint keeps the
            // logic clock ticking; with no paint request pending eframe services it as a
            // logic-only pass, not a visible frame.
            ctx.request_repaint_after_for(std::time::Duration::from_millis(100), ctx.viewport_id());
        }
    }
}
