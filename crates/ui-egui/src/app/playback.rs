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

    /// Stop the microphone, if the host gave us one. Recording ends through here so a
    /// half-captured clip is dropped rather than left running: the standalone take is cancelled
    /// too, so the bar stops claiming to record and the next take can start.
    pub(crate) fn stop_mic(&mut self) {
        let mic = self.services.mic.as_deref_mut();
        self.record.cancel(mic);
    }

    /// End the show without asking: a take is saved through the shell's own path first, so a
    /// commanded end never drops a recording.
    pub(crate) fn end_show_now(&mut self) {
        if let Some(mut show) = self.show.take() {
            // The shell's finish leaves full screen; a commanded end asks the next pass to.
            if !show.reading {
                self.leave_fullscreen = true;
            }
            if let Some(doc) = self.session.active().map(|d| d.doc.clone()) {
                // An unsaved script edit lives in the Record view's draft: write it back before
                // the show goes, or quitting from the field loses the text.
                let slide = show.state.slide;
                show::record::notes::flush(self, &mut show.record_view, &doc, slide);
            }
            if let Some(doc) = self.session.active().map(|d| d.doc.clone()) {
                let now = show.last_now;
                show::save_take(self, &mut show, &doc, now);
            } else if show.mic_tried {
                // No document left to save the take into (`file.close` beat `show.end`): hand
                // over what was captured so the desktop microphone does not stay open.
                let slide = show.state.slide;
                show::take_narration(self.services.mic.as_deref_mut(), &mut show, slide);
                self.stop_mic();
            }
            if let Some(host) = self.services.presenter.as_mut() {
                host.close();
            }
            self.media.stop_owned(media::Owner::Show);
            // The editor keeps the slide the show reached, as the shell's own finish does; a
            // recording preview is a look at the recording and leaves the editor where it was.
            if !show.recording_preview {
                let last = show.state.slide;
                let _ = self.run("slide.go", json!({"index": last}));
            }
        }
    }

    /// Leave animation preview, stop editor-owned media, and create a show at the requested slide
    /// with the current reading/presenter configuration; an out-of-range slide starts an ended show.
    pub fn start_show(&mut self, from: usize, reading: bool) {
        // A show replaces the current one: an unsaved script edit goes back to its slide first,
        // and a take it holds is saved.
        if let Some(mut show) = self.show.take() {
            if let Some(doc) = self.session.active().map(|d| d.doc.clone()) {
                let slide = show.state.slide;
                show::record::notes::flush(self, &mut show.record_view, &doc, slide);
            }
            self.show = Some(show);
        }
        // A rehearsal holds buffered timings worth keeping too: ending it through the same take
        // path writes them before the new show replaces it. A recording preview saves nothing,
        // so it still ends without writing.
        if self.show.as_ref().is_some_and(|s| s.has_unsaved_take()) {
            self.end_show_now();
        }
        // The show and Record ▸ Audio share one microphone. A finished take is inserted on the
        // slide before the show opens, so starting Play never throws the samples away.
        if self.record.is_recording() && self.stop_audio_recording().is_err() {
            // The take is kept when it cannot be encoded; a show start discards it explicitly
            // rather than leaving a dead recorder behind.
            self.record.cancel(self.services.mic.as_deref_mut());
        }
        self.anim_preview = None;
        self.media.stop_owned(media::Owner::Editor);
        let presenter = self.ui.presenter_view && !reading && self.second_display();
        if let Some(d) = self.session.active() {
            self.show_instance = self.show_instance.wrapping_add(1).max(1);
            let mut show = show::Show::new(&d.doc, from, reading, presenter);
            show.instance = self.show_instance;
            show.keep_cursor = self.ui.show_keep_cursor;
            self.show = Some(show);
        }
        // The show that replaces a captured recording goes fullscreen itself: only a show that
        // does not fill the screen (reading, or browsed in a window) leaves fullscreen here.
        if let Some(d) = self.session.active() {
            if show::show_fills_screen(reading, &d.doc.show.show_type) {
                self.leave_fullscreen = false;
            }
        } else if !reading {
            self.leave_fullscreen = false;
        }
    }

    /// Whether a second display can take Presenter View. Like PowerPoint, one display shows only
    /// the slides: the presenter window would open on the same screen, and whichever of the two
    /// windows ends up behind the other stalls the show (the compositor stops pacing a hidden
    /// window). Hosts that can't count displays keep Presenter View.
    fn second_display(&mut self) -> bool {
        self.services.displays.as_mut().and_then(|f| f()).is_none_or(|n| n >= 2)
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
        if show.presenter_grid && self.services.presenter.as_ref().is_some_and(|host| !host.is_open()) {
            // A web popup closed with its grid open must not hold auto-advance while this tab is hidden (#1866).
            show.presenter_grid = false;
        }
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
        let doc_uid = self.session.active().map_or(0, |d| d.uid);
        // A delayed automatic clip is due on the same clock the visible pass uses: start the
        // queue here too, or a hidden window leaves it silent until the next visible frame.
        let Some(mut show) = self.show.take() else { return };
        // A hidden advance has no visible pass to queue the new slide's own clips.
        show::queue_entry_media(self, &mut show, doc_uid, &doc, show_time);
        show::start_due_media(self, &mut show, doc_uid, &doc, show_time);
        let time_driven = show.hidden_time_driven(&doc);
        if !keep {
            // The show was taken out of `self` above: handing it back and ending it runs the
            // usual cleanup (presenter, show-owned media) instead of dropping it.
            self.show = Some(show);
            self.end_show_now();
            return;
        }
        self.show = Some(show);
        if time_driven {
            // A hidden event loop has no draw to piggyback on. A delayed repaint keeps the
            // logic clock ticking; with no paint request pending eframe services it as a
            // logic-only pass, not a visible frame.
            ctx.request_repaint_after_for(std::time::Duration::from_millis(100), ctx.viewport_id());
        }
    }
}
