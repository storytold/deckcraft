use super::{InputTarget, SlideApp, show};

impl SlideApp {
    /// What a queued synthetic event is meant for. The control protocol hands out window
    /// coordinates, so a document, slide, view, zoom or canvas-layout change between queueing and
    /// the next UI pass makes the event stale.
    pub(crate) fn input_target(&self) -> InputTarget {
        let bits = |v: f32| if v.is_finite() { v.to_bits() } else { 0 };
        InputTarget {
            documents: self.session.documents().len(),
            active: self.session.active().map(|d| d.uid),
            slide: self.session.active().map(|d| d.selection.slide),
            view: self.ui.view,
            zoom: self.ui.zoom.map(bits),
            canvas: self.canvas_rect.map(|r| [bits(r.min.x), bits(r.min.y), bits(r.max.x), bits(r.max.y)]),
            canvas_scale: bits(self.canvas_scale),
            dialog: self.dialog.as_ref().map(|d| egui::Id::unique(("dialog", d.id.as_str()))),
            palette: self.palette.as_ref().map(|(q, sel)| egui::Id::unique(("palette", q.as_str(), *sel))),
            backstage: self.backstage.as_ref().map(|b| egui::Id::unique(("backstage", b.page.id()))),
            ribbon_tab: egui::Id::unique(("ribbon-tab", self.ui.tab.as_str())),
            ribbon_collapsed: self.ui.ribbon_collapsed,
            ribbon_scroll: self.ribbon_scroll.get(&self.ui.tab).copied().unwrap_or(0),
            show: self.show.as_ref().map(show::Show::input_state),
            opening: self.opening.as_ref().map(|o| (egui::Id::unique(("opening", o.path.as_str())), o.generation)),
        }
    }

    /// Queue one synthetic control-channel event. The target is recorded when the batch starts,
    /// before any host command in the same logic pass can change the document.
    pub fn queue_input(&mut self, event: egui::Event) {
        let target = self.input_target();
        // A batch that was already injected keeps the target it was injected for until its UI
        // pass consumes it or retracts it. New events queued meanwhile belong to the current
        // target, so they wait in a separate batch instead of joining the old one and being
        // cleared with it when that batch turns out to be stale.
        if self.synthetic_pending {
            if self.synthetic_next_target != Some(target) {
                self.synthetic_next.clear();
                self.synthetic_next_target = Some(target);
            }
            self.synthetic_next.push(event);
            return;
        }
        if self.synthetic.is_empty() {
            self.synthetic_target = Some(target);
        } else if self.synthetic_target != Some(target) {
            // The unsent queue belongs to a target that has already changed; drop it (and any
            // older batch still waiting behind it) rather than appending a click intended for the
            // new target to a stale batch.
            self.synthetic.clear();
            self.synthetic_next.clear();
            self.synthetic_next_target = None;
            self.synthetic_target = Some(target);
        }
        self.synthetic.push(event);
    }

    /// Events admitted by the control channel but not yet injected (including a batch waiting
    /// behind the one currently in flight).
    pub(crate) fn synthetic_queued_len(&self) -> usize {
        self.synthetic.len() + self.synthetic_next.len()
    }

    /// Forget the injected or queued synthetic batch and everything recorded about it. Events
    /// waiting in `synthetic_next` are a separate, newer batch and are promoted by the caller.
    fn clear_synthetic_batch(&mut self) {
        self.synthetic.clear();
        self.synthetic_pending = false;
        self.synthetic_injected.clear();
        self.synthetic_injected_at = None;
        self.synthetic_injected_time = None;
        self.synthetic_target = None;
        self.synthetic_focus = None;
    }

    /// Start the batch that was queued behind an older one once the older batch is gone.
    pub(super) fn promote_next_synthetic(&mut self) {
        if self.synthetic_pending || !self.synthetic.is_empty() || self.synthetic_next.is_empty() {
            return;
        }
        self.synthetic = std::mem::take(&mut self.synthetic_next);
        self.synthetic_target = self.synthetic_next_target.take();
        self.synthetic_focus = None;
    }

    /// A font/layout pass will be discarded: egui already consumed the injected events but the
    /// app deliberately lays nothing out, so put them back at the head of the queue. The next
    /// frame injects them again after the new fonts are installed; otherwise a control click that
    /// happened to arrive on the very first frame would be acknowledged without ever reaching the
    /// UI.
    pub(super) fn requeue_injected_synthetic(&mut self) {
        if !self.synthetic_pending {
            return;
        }
        let mut combined = std::mem::take(&mut self.synthetic_injected);
        combined.append(&mut self.synthetic);
        self.synthetic = combined;
        self.synthetic_pending = false;
        self.synthetic_injected_at = None;
        self.synthetic_injected_time = None;
        self.synthetic_focus = None;
        if self.synthetic.is_empty() {
            self.synthetic_target = None;
            self.promote_next_synthetic();
        }
    }

    /// Remove exactly the recorded synthetic events from `events`. The recorded index is tried
    /// first so an identical host event earlier in the batch is never removed; the last exact
    /// match is only a fallback for a raw input that was rewritten around the events.
    fn drain_synthetic_from(&self, events: &mut Vec<egui::Event>) -> bool {
        let injected = &self.synthetic_injected;
        let n = injected.len();
        if n == 0 {
            return false;
        }
        let exact = self.synthetic_injected_at.filter(|start| events.get(*start..*start + n).is_some_and(|w| w == injected.as_slice()));
        let at = exact.or_else(|| events.windows(n).rposition(|w| w == injected.as_slice()));
        if let Some(at) = at {
            events.drain(at..at + n);
            true
        } else {
            false
        }
    }

    /// Drop an injected batch whose document, canvas, focus or modal owner changed during
    /// `logic`. eframe calls `raw_input_hook` before `App::logic`, so a control request drained
    /// there can still retarget the same `RawInput`.
    ///
    /// This must not infer the pass phase from `ViewportInfo::visible()`: a visible root can run
    /// logic-only (an overdue redraw), and a hidden root can run a full pass for a screenshot or
    /// `RequestPaintWhileHidden`. A logic-only callback leaves `InputState` from the *previous*
    /// pass; remove the batch only when the live input carries exactly what this pass injected
    /// (same `RawInput::time` and the exact recorded event window). Otherwise keep the pending
    /// bookkeeping so the next `raw_input_hook` can retract the events from the carried
    /// `RawInput` before eframe delivers them in a full pass.
    pub(super) fn retract_synthetic_after_logic(&mut self, ctx: &egui::Context) {
        if !self.synthetic_pending {
            return;
        }
        let current = self.input_target();
        let focus = ctx.memory(|m| m.focused());
        if self.synthetic_target == Some(current) && focus == self.synthetic_focus {
            return;
        }
        let injected_time = self.synthetic_injected_time;
        let mut removed = false;
        ctx.input_mut(|i| {
            // A logic-only call keeps the previous pass's `InputState`; its `raw.time` is not the
            // time of the RawInput this batch was injected into. Never drain an older batch that
            // happens to look similar.
            if i.raw.time != injected_time {
                return;
            }
            let mut events = std::mem::take(&mut i.events);
            let ok = self.drain_synthetic_from(&mut events);
            i.events = events;
            if ok {
                // Keep `InputState::raw` consistent with what UI code reads as `events`.
                let mut raw_events = std::mem::take(&mut i.raw.events);
                let _ = self.drain_synthetic_from(&mut raw_events);
                i.raw.events = raw_events;
                removed = true;
            }
        });
        if removed {
            self.clear_synthetic_batch();
            self.promote_next_synthetic();
        } else {
            // Logic-only (or a pass whose input does not contain the batch): the events are in
            // eframe's carried RawInput. Keep `synthetic_pending`, `synthetic_injected` and the
            // stale target so the next hook retracts them there. Events queued after the batch
            // belong to the same stale target as well.
            self.synthetic.clear();
        }
    }

    /// True when the pending synthetic batch is in this pass's live `InputState`, i.e. this is
    /// the full pass the batch was admitted for and egui has already derived pointer/key state
    /// from it. A logic-only callback leaves the previous pass's `InputState` (with another
    /// `raw.time`) and must not be mistaken for the live pass, so control/background work is
    /// still drained there and the next `raw_input_hook` retracts a batch that went stale.
    pub(super) fn synthetic_batch_is_live(&self, ctx: &egui::Context) -> bool {
        if !self.synthetic_pending || self.synthetic_injected.is_empty() {
            return false;
        }
        let Some(injected_time) = self.synthetic_injected_time else { return false };
        let injected = &self.synthetic_injected;
        ctx.input(|i| i.raw.time == Some(injected_time) && i.events.windows(injected.len()).any(|w| w == injected.as_slice()))
    }

    /// Input that belongs to the pass about to run: native dropped files and one synthetic
    /// control-channel batch. The synthetic batch is injected at most once; while it is
    /// unacknowledged, later passes cannot replay it. A document, canvas, focus or modal change
    /// drops the whole queued batch rather than typing into the wrong place.
    ///
    /// The hook always injects into the `RawInput`; it does not try to guess whether eframe will
    /// run this as a full or a logic-only pass. `run_logic` does not consume events but eframe
    /// carries the whole `RawInput` into the next full pass, so an already-injected batch is
    /// retracted at the next hook if a control request changed the target, and otherwise is
    /// delivered exactly once in the next real UI pass. `ViewportInfo::visible()` is not a phase
    /// signal: a hidden root still gets a full pass for a screenshot or `RequestPaintWhileHidden`.
    pub fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        #[cfg(not(target_arch = "wasm32"))]
        for f in std::mem::take(&mut raw.dropped_files) {
            let name = f.path().file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "dropped file".into());
            match f.bytes() {
                Ok(bytes) => self.open_or_insert_bytes(&name, &bytes),
                Err(e) => self.set_status(e),
            }
        }
        let focus = ctx.memory(|m| m.focused());
        if self.synthetic_pending {
            let current = self.input_target();
            if self.synthetic_target != Some(current) || focus != self.synthetic_focus {
                // eframe keeps the whole RawInput from a logic-only pass in `pending_raw_input`
                // and appends the next host input to it. The events are already in `raw.events`;
                // clearing only the unsent queue would still deliver the stale click or key into
                // the new document on the next full pass. Retract exactly the batch we appended.
                let _ = self.drain_synthetic_from(&mut raw.events);
                self.clear_synthetic_batch();
                // New control input queued for the current target while the stale batch was in
                // flight is a separate batch; keep it instead of clearing it with the old one.
                self.promote_next_synthetic();
            } else {
                // This RawInput is the carrier for the next pass, and eframe refreshes its time
                // on every call. Record the current time so `retract_synthetic_after_logic` can
                // tell this full pass's live `InputState` from a logic-only callback's stale one.
                self.synthetic_injected_time = raw.time;
            }
        }
        if self.synthetic_pending || self.synthetic.is_empty() {
            return;
        }
        let current = self.input_target();
        if self.synthetic_target != Some(current) {
            // The batch was queued for a document/slide/canvas that has since changed.
            self.synthetic.clear();
            self.synthetic_target = None;
            self.promote_next_synthetic();
            if self.synthetic.is_empty() {
                return;
            }
            if self.synthetic_target != Some(current) {
                // The newer batch is stale too: drop it rather than delivering it to this target.
                self.synthetic.clear();
                self.synthetic_target = None;
                self.synthetic_next.clear();
                self.synthetic_next_target = None;
                return;
            }
        }
        self.synthetic_generation = self.synthetic_generation.wrapping_add(1).max(1);
        log::debug!("injecting synthetic input batch {}", self.synthetic_generation);
        let n = match self.synthetic.first() {
            Some(egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. }) => 1,
            _ => self.synthetic.iter().position(|e| matches!(e, egui::Event::Key { pressed: false, .. })).map_or(self.synthetic.len(), |i| i + 1),
        };
        let n = n.min(self.synthetic.len());
        let batch: Vec<egui::Event> = self.synthetic.drain(..n).collect();
        let at = raw.events.len();
        raw.events.extend(batch.iter().cloned());
        self.synthetic_injected = batch;
        self.synthetic_injected_at = Some(at);
        self.synthetic_injected_time = raw.time;
        self.synthetic_pending = true;
        self.synthetic_focus = focus;
    }
}
