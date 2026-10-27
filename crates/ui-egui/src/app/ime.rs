use super::*;

impl SlideApp {
    /// Borrow faces for interface text in scripts the interface fonts lack (#101): the open
    /// presentations' names and text, recent files, and what is typed or composed this frame.
    pub(super) fn note_ui_text(&mut self, ctx: &egui::Context) {
        let typed: String = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Text(t) | egui::Event::Paste(t) => Some(t.as_str()),
                    egui::Event::Ime(egui::ImeEvent::Preedit { text, .. } | egui::ImeEvent::Commit(text)) => Some(text.as_str()),
                    _ => None,
                })
                .collect()
        });
        self.ui_fonts.note(ctx, &typed);
        for d in self.session.documents() {
            self.ui_fonts.note(ctx, &d.title());
        }
        for r in &self.ui.recent {
            self.ui_fonts.note(ctx, r);
        }
        if let Some(st) = self.session.active() {
            self.ui_fonts.note_doc(ctx, st);
        }
        self.ui_fonts.apply(ctx);
    }

    /// Stable IME owner for the canvas text target: document, view, shape and table cell.
    ///
    /// The caret, selection, text, revision and geometry are deliberately absent, so ordinary
    /// edits and caret movement keep the same owner. A new document, shape or cell is a new
    /// owner; [`egui::ime_context::ImeTarget::replacement_epoch`] stays zero because that key
    /// itself changes on replacement. `file.revert` keeps the ids, so a composition it settles is
    /// covered by the settle interrupt and by [`Self::ime_event`]'s staleness check instead.
    pub(crate) fn ime_target(&self) -> Option<egui::ime_context::ImeTarget> {
        let st = self.session.active()?;
        let (shape, cell, notes) = match &st.selection.text {
            Some(t) => (t.shape, t.cell, t.notes),
            None => (*st.selection.shapes.first()?, None, false),
        };
        if st.selection.text.is_none() && st.selection.shapes.len() != 1 {
            return None;
        }
        let target = match st.selection.target {
            Target::Slides => (0u8, 0usize, 0usize),
            Target::Master { master } => (1, master, 0),
            Target::Layout { master, layout } => (2, master, layout),
            Target::Handout => (3, 0, 0),
            Target::NotesMaster => (4, 0, 0),
        };
        let owner = egui::Id::unique(("deckcraft-ime", st.uid, target, shape.0, cell, notes));
        Some(egui::ime_context::ImeTarget::new(owner, 0))
    }

    /// The committed surrounding snapshot for the current edit target, or `Unavailable` when
    /// there is an owner but no committed text to describe.
    ///
    /// An unavailable stamp carries the document revision. The tracker requires a strictly
    /// increasing revision on both sides of a receipt that involves `Unavailable`; without it, a
    /// wide selection or a commit that starts from a selected shape would poison the whole frame.
    fn ime_receipt_stamp(&self) -> Option<egui::ime_context::ImeMutationStamp> {
        let st = self.session.active()?;
        self.ime_target()?;
        Some(st.selection.text.as_ref().and_then(|t| deckcraft_engine::cmd::text::ime_surrounding(st, t)).map_or_else(
            || egui::ime_context::ImeMutationStamp::unavailable_at_revision(st.revision),
            |s| egui::ime_context::ImeMutationStamp::from_committed(&s.text, s.cursor, s.anchor, None),
        ))
    }

    /// Report an application-origin edit in this frame. The tracker lets `Other` dominate any
    /// earlier or later IME receipt in the same frame.
    pub(super) fn note_application_edit(&mut self, ctx: &egui::Context) {
        let Some(viewport) = self.ime_frame_viewport else { return };
        if let Some(target) = self.ime_target() {
            let frame = ctx.cumulative_frame_nr_for(viewport);
            self.ime_tracker.note_application_change(viewport, frame, target);
        }
    }

    /// Record the expected selection after an accepted IME event. Its own deletion may move
    /// the caret/revision; that is not an application interruption of the following commit.
    fn refresh_ime_terminal_selection(&mut self) {
        let state = self.session.active().and_then(|st| st.selection.text.as_ref().map(|t| (st.revision, t.anchor, t.caret)));
        if let (Some(pending), Some((revision, anchor, caret))) = (&mut self.ime_terminal_pending, state) {
            pending.revision = revision;
            pending.anchor = anchor;
            pending.caret = caret;
        }
    }

    /// The current live destination for an IME reset fence.
    fn ime_destination(&self) -> Option<ImeDestination> {
        let st = self.session.active()?;
        let target = self.ime_target()?;
        let (anchor, caret) = st.selection.text.as_ref().map_or((None, None), |t| (Some(t.anchor), Some(t.caret)));
        Some(ImeDestination { owner: target.owner, revision: st.revision, anchor, caret })
    }

    /// Use the recorded IME viewport only while it matches the current context viewport.
    pub(super) fn ime_viewport_or(&self, ctx: &egui::Context) -> egui::ViewportId {
        self.ime_frame_viewport.filter(|v| *v == ctx.viewport_id()).unwrap_or_else(|| ctx.viewport_id())
    }

    /// The reset guard belonging to exactly this viewport. There is deliberately no fallback to
    /// another viewport's phase.
    pub(crate) fn ime_reset_for(&self, viewport: egui::ViewportId) -> ImeResetPhase {
        self.ime_reset.get(&viewport).copied().unwrap_or(ImeResetPhase::Clear)
    }

    /// Store a non-clear reset phase for one viewport, or remove the entry when the phase is `Clear`.
    pub(crate) fn set_ime_reset(&mut self, viewport: egui::ViewportId, phase: ImeResetPhase) {
        if matches!(phase, ImeResetPhase::Clear) {
            let _ = self.ime_reset.remove(&viewport);
        } else {
            self.ime_reset.insert(viewport, phase);
        }
    }

    /// The live stamp used to compare against the origin/interrupted terminal destination.
    fn ime_terminal_stamp(&self) -> Option<ImeTerminalStamp> {
        let st = self.session.active()?;
        let (anchor, caret) = st.selection.text.as_ref().map_or((None, None), |t| (Some(t.anchor), Some(t.caret)));
        Some(ImeTerminalStamp { owner: self.ime_target().map(|t| t.owner), revision: st.revision, anchor, caret })
    }

    /// Positive route predicate for the canvas IME producer: this pass must be one that can
    /// actually paint `canvas::show` for the active document, with no modal, stock field or
    /// overlay owning input.
    ///
    /// Sorter and Notes Page are editor views without a canvas producer, so they are named here
    /// even though the retained selection still yields an `ime_target`. A hidden, unfocused or
    /// closing window, a missing document/target, a focused stock field, modal dialog, palette,
    /// Backstage, slide show and opening overlay lose admission too. The finalizer remains the
    /// authority for this pass's actual descriptor; keeping this predicate positive means a view
    /// added later is not admitted by omission simply because it does not call `canvas::show`.
    pub(crate) fn ime_custom_canvas_route(&self, ctx: &egui::Context) -> bool {
        matches!(self.ui.view, ViewMode::Normal | ViewMode::Outline | ViewMode::Reading)
            && self.session.active().is_some()
            && self.ime_target().is_some()
            && ctx.input(|i| i.viewport().focused.unwrap_or(true))
            && ctx.input(|i| i.viewport().visible() != Some(false))
            && !ctx.input(|i| i.viewport().close_requested())
            && ctx.memory(|m| m.focused().is_none())
            && self.dialog.as_ref().is_none_or(|d| !d.modal())
            && self.palette.is_none()
            && self.backstage.is_none()
            && self.show.is_none()
            && self.opening.is_none()
    }

    /// The singleton app's session, composition tracker and terminal guard belong to its owning
    /// viewport. View › New Window and Presenter View render through their own immediate
    /// viewports and never call `logic`/`ui`; admitting such a callback would let a render-only
    /// viewport mutate root composition state while only the reset map is per-viewport. Reject it
    /// before any state changes instead of pretending to support multi-view editing.
    pub(super) fn root_viewport_admitted(ctx: &egui::Context) -> bool {
        ctx.viewport_id() == egui::ViewportId::ROOT
    }

    /// Report whether the current pass is a full UI pass for the IME-owning viewport rather than
    /// standalone logic.
    pub(super) fn ime_in_ui_pass(&self, ctx: &egui::Context) -> bool {
        let key = ime_in_ui_pass_key(self.ime_viewport_or(ctx));
        ctx.data(|d| d.get_temp::<bool>(key).unwrap_or(false))
    }

    /// A new application-origin interruption supersedes any previous pending fence.
    ///
    /// This runs from the logic half of a full pass and from raw input handling, before the
    /// finalizer has seen this pass's actual custom descriptor. No request is allocated here:
    /// putting a token into egui `Memory` now would let egui attach it to a later stock
    /// descriptor, or to a canvas this route does not paint. Retire the superseded id
    /// immediately and store an unbound `Suspended { request: None }`; only the finalizer may
    /// allocate its one replacement after confirming an eligible custom output.
    pub(crate) fn note_ime_interruption(&mut self, ctx: &egui::Context, fresh: bool) {
        let viewport = self.ime_viewport_or(ctx);
        if !fresh
            && matches!(
                self.ime_reset_for(viewport),
                ImeResetPhase::Awaiting { viewport: guard_viewport, .. }
                    | ImeResetPhase::Suspended { viewport: guard_viewport, .. }
                    if guard_viewport == viewport
            )
        {
            // This viewport already has a live or suspended guard. The finalizer replaces it
            // once if the actual destination changed; do not churn a request per event.
            return;
        }
        self.set_ime_reset(viewport, ImeResetPhase::Suspended { viewport, needs_rearm: true, request: None });
    }

    /// Custom admission was lost on this route. Retire the old id without inventing an
    /// acknowledgement and block until a later eligible pass can request a replacement.
    pub(super) fn suspend_ime_reset(&mut self, viewport: egui::ViewportId) {
        if self.ime_reset_for(viewport).blocks_custom_input() {
            self.set_ime_reset(viewport, ImeResetPhase::Suspended { viewport, needs_rearm: true, request: None });
        }
    }

    /// An ordered completion for the exact newest request. Wrong/foreign/obsolete markers are
    /// inert; this never mutates text, caret or undo history.
    pub(super) fn ime_reset_completed(&mut self, request_id: egui::ImeResetId, ctx: &egui::Context) {
        let viewport = self.ime_viewport_or(ctx);
        if request_id.viewport_id() != viewport {
            return;
        }
        if let ImeResetPhase::Awaiting { id, viewport: waiting_viewport, .. } = self.ime_reset_for(viewport)
            && id == request_id
            && waiting_viewport == viewport
        {
            self.set_ime_reset(viewport, ImeResetPhase::Clear);
            // The exact ordered completion is the platform-reset boundary. It also releases a
            // cleared-preedit terminal guard: the app no longer has to wait for the old
            // Commit/Enabled/Disabled before a fresh composition may start. Events ordered
            // before this marker remain rejected; a late old terminal after the reset has the
            // same documented residual as any other event crossing a platform reset.
            if self.ime_terminal_pending.as_ref().is_some_and(|p| p.cancelled) {
                self.ime_terminal_pending = None;
            }
            if self.ime_composition.is_some_and(|c| c.settled) {
                self.ime_composition = None;
            }
            self.ime_cancelled = None;
            self.ime_interrupt_pending = false;
        }
    }

    /// Controls-only routing for a route where the custom editor cannot mutate. Markers are
    /// visited in raw order; stock text/key events stay untouched for the stock widgets.
    pub(super) fn route_ime_controls_only(&mut self, ctx: &egui::Context, events: &[egui::Event]) {
        for e in events {
            if let egui::Event::ImeResetCompleted { request_id } = e {
                self.ime_reset_completed(*request_id, ctx);
            }
        }
    }

    /// Bind a suspended reset to the actual custom descriptor, or retire it on loss.
    pub(crate) fn finalize_ime_reset(&mut self, ctx: &egui::Context, custom: Option<ImeDescriptorKey>) {
        let viewport = self.ime_viewport_or(ctx);
        let phase = self.ime_reset_for(viewport);
        let actual = ctx.output(|o| o.ime.as_ref().map(|i| ImeDescriptorKey::new(i.owner, i.context_generation, i.purpose)));
        let destination = self.ime_destination();
        // The finalizer is the allocation authority, so it rechecks the positive canvas route
        // and that this pass is not already known to be discarded. Sorter/Notes Page, a modal,
        // palette, Backstage, show, opening overlay, hidden window, stock focus, font/layout
        // discard or a missing target must suspend without putting a token into egui Memory.
        let eligible = self.ime_custom_canvas_route(ctx) && !ctx.will_discard();
        let custom_output = match (eligible, custom, actual, destination) {
            (true, Some(custom), Some(actual), Some(destination)) if custom == actual => Some((custom, destination)),
            _ => None,
        };
        let Some((output_context, destination)) = custom_output else {
            match phase {
                ImeResetPhase::Clear => {}
                ImeResetPhase::Awaiting { .. } => {
                    self.set_ime_reset(viewport, ImeResetPhase::Suspended { viewport, needs_rearm: true, request: None });
                }
                ImeResetPhase::Suspended { viewport: guard_viewport, request, .. } => {
                    // A request that was not bound to the actual custom output in this pass is
                    // retired locally; a later eligible activation obtains a fresh one. A guard
                    // stamped for another viewport (or a request issued for one) is never reused.
                    if guard_viewport != viewport || request.is_some() {
                        self.set_ime_reset(viewport, ImeResetPhase::Suspended { viewport, needs_rearm: true, request: None });
                    }
                }
                ImeResetPhase::AllocationFailed { .. } => {
                    self.set_ime_reset(viewport, ImeResetPhase::Suspended { viewport, needs_rearm: true, request: None });
                }
            }
            return;
        };
        match phase {
            ImeResetPhase::Clear => {}
            ImeResetPhase::Awaiting { viewport: old_viewport, destination: old_destination, output_context: old_context, .. } => {
                if old_viewport != viewport || old_destination != destination || old_context != output_context {
                    self.request_ime_rearm(ctx, viewport, destination, output_context);
                }
            }
            ImeResetPhase::Suspended { viewport: guard_viewport, request, needs_rearm } => {
                let request = if needs_rearm && guard_viewport == viewport {
                    // Only a request issued for this exact viewport may be bound here.
                    request.filter(|id| id.viewport_id() == viewport)
                } else {
                    None
                };
                match request.or_else(|| ctx.request_ime_reset(viewport)) {
                    Some(id) => {
                        self.set_ime_reset(viewport, ImeResetPhase::Awaiting { id, viewport, destination, output_context });
                    }
                    None => {
                        self.set_ime_reset(viewport, ImeResetPhase::AllocationFailed { viewport, destination, output_context });
                    }
                }
            }
            ImeResetPhase::AllocationFailed { viewport: old_viewport, destination: old_destination, output_context: old_context } => {
                // Repeated idle output for the same destination does not retry allocation.
                if old_viewport != viewport || old_destination != destination || old_context != output_context {
                    self.request_ime_rearm(ctx, viewport, destination, output_context);
                }
            }
        }
    }

    /// Request one replacement IME reset token and record `Awaiting` on success or
    /// `AllocationFailed` when no token can be allocated.
    fn request_ime_rearm(&mut self, ctx: &egui::Context, viewport: egui::ViewportId, destination: ImeDestination, output_context: ImeDescriptorKey) {
        match ctx.request_ime_reset(viewport) {
            Some(id) => self.set_ime_reset(viewport, ImeResetPhase::Awaiting { id, viewport, destination, output_context }),
            None => self.set_ime_reset(viewport, ImeResetPhase::AllocationFailed { viewport, destination, output_context }),
        }
    }

    /// A standalone run_logic tick paints no custom geometry. Suspend once, without replaying
    /// stale input; the next full UI pass can rearm from its actual descriptor.
    pub(super) fn finalize_ime_logic(&mut self, ctx: &egui::Context) {
        let viewport = self.ime_viewport_or(ctx);
        if self.ime_reset_for(viewport).blocks_custom_input() {
            // Only this viewport's guard is touched. A root/child logic tick must not suspend or
            // rebind another viewport's independent reset lifetime.
            self.set_ime_reset(viewport, ImeResetPhase::Suspended { viewport, needs_rearm: true, request: None });
        }
    }

    /// Note compositions the application settled since the last check (a click, a key, another
    /// command). Those are announced to the platform as an interrupt in [`Self::observe_ime`],
    /// and a platform event for them is stale from that moment on, even in the same pass.
    pub(crate) fn note_settled_composition(&mut self, ctx: &egui::Context) {
        let settles = self.session.composition_settles();
        let current_stamp = self.ime_terminal_stamp();
        // `file.close` then `file.new` can replace the active uid before any pass observes the
        // empty document. That is not a safe boundary for the old terminal: the platform may
        // still deliver its suffix. Keep the guard bound to its retired owner and treat the new
        // destination as a fresh interruption; only exact reset completion or an observed
        // teardown may clear the terminal metadata.
        let pending_interrupted = self.ime_terminal_pending.as_ref().is_some_and(|p| {
            let reference = if p.interrupted {
                p.interrupted_at
            } else {
                Some(ImeTerminalStamp { owner: Some(p.owner), revision: p.revision, anchor: Some(p.anchor), caret: Some(p.caret) })
            };
            reference != current_stamp
        });
        if settles == self.ime_settles_seen && !pending_interrupted {
            return;
        }
        let fresh = settles != self.ime_settles_seen;
        self.ime_settles_seen = settles;
        if let Some(pending) = &mut self.ime_terminal_pending {
            pending.cancelled = true;
            pending.interrupted = true;
            // Compare later passes against the destination where interruption actually happened,
            // not forever against the retired composition's origin. The destination may be absent
            // (the interruption closed its target); `interrupted` keeps that distinct from "not
            // interrupted yet".
            pending.interrupted_at = current_stamp;
        }
        self.ime_interrupt_pending = true;
        self.note_ime_interruption(ctx, fresh);
        let cancelled = self.ime_composition.as_mut().map(|c| {
            c.settled = true;
            (c.owner, c.generation)
        });
        if let Some((owner, generation)) = cancelled {
            // Retain a tombstone beyond the first stale event: clearing the composition must
            // not let the rest of this input batch commit into the next target. `observe_ime`
            // retires it in the pass that publishes the matching interrupt.
            self.ime_cancelled = Some(ImeCancelled { owner, generation });
        }
    }

    /// An input method's event for the slide's text (#101): the preedit is shown in the text and
    /// replaced on each update; the commit replaces it as one undo step.
    ///
    /// An event only ever changes the target its composition began in. The application settles a
    /// composition itself on a click, a key, a command or a target change, and the platform may
    /// still hold it: pre-edits and commits queued for that generation in the settling input batch
    /// are dropped (with [`egui::Memory::interrupt_ime`]) instead of being typed into whatever is
    /// selected now. After the interrupt has been published the platform is reset, so the next
    /// batch is treated as new input; egui's event payload has no generation, so an OS-queued
    /// event that crosses that reset cannot be distinguished from a legitimate one.
    ///
    /// A receipt is captured immediately before and after a successful commit or
    /// delete-surrounding mutation. Preedit, rejected/failed events and no-ops never earn
    /// [`egui::output::ImeChangeCause::InputMethod`].
    pub(crate) fn ime_event(&mut self, ctx: &egui::Context, e: &egui::ImeEvent) {
        self.note_settled_composition(ctx);
        let viewport = self.ime_frame_viewport.unwrap_or_else(|| ctx.viewport_id());
        let frame = ctx.cumulative_frame_nr_for(viewport);
        let target = self.ime_target();
        let owner = target.map(|t| t.owner);
        let accepted =
            matches!(e, egui::ImeEvent::Commit(_) | egui::ImeEvent::DeleteSurrounding { .. } | egui::ImeEvent::DeleteSurroundingBytes { .. });
        if self.reject_stale_ime_event(ctx, e, owner) {
            return;
        }
        // Until the exact newest reset request completes at its ordered stream position, custom
        // mutations stay blocked. Platform enable/disable signals are not acknowledgements and do
        // not release this guard; they still settle the old composition below.
        #[allow(deprecated)]
        if self.ime_reset_for(viewport).blocks_custom_input() && !matches!(e, egui::ImeEvent::Disabled | egui::ImeEvent::Enabled) {
            return;
        }
        let before = if accepted { self.ime_receipt_stamp() } else { None };
        let r = self.apply_ime_event(e);
        self.settle_ime_event_state(e, owner, r.is_ok());
        if r.is_ok() && accepted {
            self.refresh_ime_terminal_selection();
        }
        if let Err(e) = &r {
            self.set_status(e.to_string());
        } else if accepted
            && let (Some(target), Some(before), Some(after)) = (target, before, self.ime_receipt_stamp())
            && before != after
            && r.as_ref().is_ok_and(|v| !v.is_null() && v.get("handled").and_then(Value::as_bool) != Some(false))
        {
            // A no-op receipt (an empty commit, a zero-length delete, same text reinserted) is
            // not a mutation and must not poison the frame for a real commit later in the batch.
            self.ime_tracker.note_input_method_apply(viewport, frame, target, before, after);
        }
    }

    /// Drop an event that belongs to a composition the application already settled, or to another
    /// owner; returns `true` when the event must not be applied.
    ///
    /// A commit or deletion belongs to the composition that began the pre-edit: another owner
    /// (the user clicked elsewhere) or a composition the app already settled makes it stale.
    /// While the reset is still pending, the tombstone rejects pre-edits too (the platform has
    /// not yet been told to cancel its copy). `observe_ime` retires the tombstone in the pass
    /// that publishes the interrupt (or in an accepted no-descriptor pass, which resets the
    /// backend just the same), so a later commit-only is not lost forever: it is a
    /// legitimate direct commit or a new platform composition. egui's events carry no
    /// generation, so an event queued by the OS across that reset is indistinguishable from
    /// one the user just typed; only events in the same input batch as the settle are rejected
    /// deterministically. A cleared-preedit terminal is a distinct case: its pending owner
    /// remains guarded across that output boundary until its actual commit/enable/disable
    /// arrives. Publishing a reset is not evidence that the separately queued terminal was
    /// delivered. With no incoming event epoch, the first ambiguous new composition after an
    /// unobserved reset may be rejected; no timer guesses that it became safe.
    /// The tombstone is not filtered by the *current* target: a click can change the owner in
    /// the settling pass, and every remaining event of that batch still belongs to the
    /// pre-reset composition. Filtering would let the event after the first dropped pre-edit
    /// start composing in the new target before the platform is reset.
    fn reject_stale_ime_event(&mut self, ctx: &egui::Context, e: &egui::ImeEvent, owner: Option<egui::Id>) -> bool {
        let cancelled = self.ime_cancelled;
        let pending_cancelled = self.ime_terminal_pending.as_ref().is_some_and(|p| p.cancelled);
        #[allow(deprecated)]
        let stale = !matches!(e, egui::ImeEvent::Disabled | egui::ImeEvent::Enabled)
            && (self.ime_composition.is_some_and(|c| c.settled || Some(c.owner) != owner) || cancelled.is_some() || pending_cancelled);
        if !stale {
            return false;
        }
        if pending_cancelled && matches!(e, egui::ImeEvent::Commit(_)) {
            // The old terminal finally arrived: reject it and release its pending owner now,
            // so a new preedit later in this same raw batch can belong to the current target.
            self.ime_terminal_pending = None;
            self.ime_composition = None;
            self.ime_cancelled = None;
            self.ime_interrupt_pending = false;
            return true;
        }
        if pending_cancelled {
            // note_settled_composition already queued the reset. Do not also latch the
            // frame's memory interrupt: a terminal later in this batch can consume it and
            // permit a fresh preedit without resetting that new composition at pass end.
            self.ime_composition = None;
            return true;
        }
        if let Some(c) = cancelled {
            log::debug!("IGNORING IME event for settled composition owner {:?} generation {}", c.owner, c.generation);
        }
        self.ime_composition = None;
        // Ask the backend to drop its copy, so this is one stale event and not a stream.
        ctx.memory_mut(|m| m.interrupt_ime());
        true
    }

    /// Apply the document mutation an IME event asks for, before any receipt or tracker state.
    fn apply_ime_event(&mut self, e: &egui::ImeEvent) -> deckcraft_engine::Result<Value> {
        match e {
            egui::ImeEvent::Preedit { text, active_range_chars } => {
                self.session.compose_text(text, active_range_chars.as_ref().map(|r| (r.start, r.end)))
            }
            // A commit only replaces text that is being edited. It must never start editing a
            // merely selected shape: without a tracked pre-edit, a stale platform commit would
            // replace that shape's whole text.
            egui::ImeEvent::Commit(text) => {
                if self.session.active().is_some_and(|d| d.selection.text.is_some()) {
                    self.session.commit_text(text)
                } else {
                    Ok(Value::Null)
                }
            }
            egui::ImeEvent::DeleteSurrounding { before_chars, after_chars } => {
                if self.session.active().is_some_and(|d| d.selection.text.is_some()) {
                    self.session.delete_surrounding(*before_chars, *after_chars, false)
                } else {
                    Ok(Value::Null)
                }
            }
            egui::ImeEvent::DeleteSurroundingBytes { before_bytes, after_bytes } => {
                if self.session.active().is_some_and(|d| d.selection.text.is_some()) {
                    self.session.delete_surrounding(*before_bytes, *after_bytes, true)
                } else {
                    Ok(Value::Null)
                }
            }
            #[allow(deprecated)]
            egui::ImeEvent::Disabled | egui::ImeEvent::Enabled => {
                // The platform ended (or is about to restart) its composition itself.
                self.session.end_composition();
                Ok(Value::Null)
            }
        }
    }

    /// Update the tracked composition after `e` has been applied; `applied` says whether the
    /// document mutation succeeded.
    fn settle_ime_event_state(&mut self, e: &egui::ImeEvent, owner: Option<egui::Id>, applied: bool) {
        match e {
            // The platform ends its composition on a commit (and on Disabled/Enabled): nothing is
            // left to interrupt. A pre-edit keeps (or starts) the tracked composition.
            egui::ImeEvent::Commit(_) => {
                self.ime_terminal_pending = None;
                self.ime_composition = None;
                self.ime_cancelled = None;
                self.ime_interrupt_pending = false;
            }
            #[allow(deprecated)]
            egui::ImeEvent::Disabled | egui::ImeEvent::Enabled => {
                // A platform-side disable/enable already reset the composition. Do not carry an
                // older app-settled interrupt into the next (possibly new) composition.
                self.ime_terminal_pending = None;
                self.ime_composition = None;
                self.ime_cancelled = None;
                self.ime_interrupt_pending = false;
            }
            egui::ImeEvent::Preedit { text, .. } => {
                match (owner, applied, self.session.active().is_some_and(|d| d.composition.is_some())) {
                    (Some(owner), true, true) => {
                        // Updating the same composition keeps its generation. After a settle (or
                        // a first pre-edit) this is a new generation and clears the tombstone.
                        let generation = match self.ime_composition {
                            Some(c) if c.owner == owner && !c.settled => c.generation,
                            _ => {
                                self.ime_generation = self.ime_generation.wrapping_add(1).max(1);
                                self.ime_generation
                            }
                        };
                        self.ime_composition = Some(ImeComposition { owner, generation, settled: false });
                        self.ime_terminal_pending = None;
                        self.ime_cancelled = None;
                    }
                    (Some(owner), true, false) if text.is_empty() && self.ime_composition.is_some_and(|c| !c.settled && c.owner == owner) => {
                        // The model restored committed text, but no terminal was delivered. Keep
                        // this owner across passes; a later pointer/key/target change must reject
                        // its separately queued old delete/commit, even after an outgoing reset.
                        if let Some(st) = self.session.active()
                            && let Some(t) = &st.selection.text
                        {
                            self.ime_terminal_pending = Some(ImeTerminalPending {
                                owner,
                                revision: st.revision,
                                anchor: t.anchor,
                                caret: t.caret,
                                cancelled: false,
                                interrupted: false,
                                interrupted_at: None,
                            });
                        }
                    }
                    _ => {
                        self.ime_composition = None;
                        self.ime_terminal_pending = None;
                    }
                }
            }
            _ => {}
        }
    }

    /// Observe the canvas IME owner once per pass and publish its descriptor.
    ///
    /// No canvas target, a Sorter/Notes Page view, a focused egui field, a modal, the palette,
    /// the Backstage view, a slide show, an opening overlay, or an unfocused/hidden window means
    /// the custom owner is hidden: the tracker is told so and the next focus starts a new
    /// generation. The descriptor is rebuilt from the current committed text every pass.
    ///
    /// A composition the application settled itself is announced once as
    /// `should_interrupt_composition`, so the backend cancels the platform's copy of it. That
    /// settled pass is also the active-composition tombstone's lifecycle end. A cleared-preedit
    /// terminal retains its separate pending owner until the incoming terminal is observed.
    pub(crate) fn observe_ime(&mut self, ctx: &egui::Context) -> Option<ImeDescriptorKey> {
        let viewport = self.ime_frame_viewport.unwrap_or_else(|| ctx.viewport_id());
        self.note_settled_composition(ctx);
        // The editing window going away really ends the viewport's context; a merely hidden
        // owner (modal, palette, unfocused field) stays observed as unfocused instead.
        if self.session.active().is_none() || ctx.input(|i| i.viewport().close_requested()) {
            self.ime_interrupt_pending = false;
            if let Some(old) = self.ime_viewport.take() {
                self.ime_tracker.end_viewport(old);
                // Explicit viewport destruction removes only that viewport's guard.
                let _ = self.ime_reset.remove(&old);
            }
            let _ = self.ime_reset.remove(&viewport);
            self.ime_composition = None;
            self.ime_cancelled = None;
            self.ime_terminal_pending = None;
            return None;
        }
        if !ctx.input(|i| i.viewport().focused.unwrap_or(true)) {
            // The native window itself lost focus: no custom editor is eligible this pass, even
            // if the model still has a text selection and a last-pass geometry rect.
            self.ime_tracker.observe(viewport, None);
            return None;
        }
        let Some(rect) = self.ime_pending.take() else {
            // No canvas editor was painted in this pass. A discarded font/layout pass publishes
            // nothing the platform sees, so keep the interrupt and tombstone for a later pass.
            // An accepted pass is a backend reset only when it publishes *no* IME descriptor at
            // all: a focused stock `TextEdit` (notes pane, dialog, palette field) publishes its
            // own `o.ime` while `ime_pending` is `None`, and that descriptor is not a reset. Gate
            // retirement on the actual committed output, not on the absence of the canvas rect;
            // otherwise the next legitimate commit in the focused field would be rejected.
            let no_descriptor = ctx.output(|o| o.ime.is_none());
            if !ctx.will_discard() && no_descriptor {
                self.ime_interrupt_pending = false;
                self.ime_cancelled = None;
                if self.ime_composition.is_some_and(|c| c.settled) {
                    self.ime_composition = None;
                }
            }
            self.ime_tracker.observe(viewport, None);
            return None;
        };
        if !self.ime_custom_canvas_route(ctx) {
            // The canvas can still paint behind an opening overlay or a Sorter/Notes Page
            // reroute, and a forced hidden paint can run after the window lost focus or
            // visibility. The positive route and the UI wrapper both reject those, so a pending
            // rect must not republish the custom owner.
            self.ime_tracker.observe(viewport, None);
            return None;
        }
        let Some(st) = self.session.active() else {
            self.ime_interrupt_pending = false;
            self.ime_tracker.observe(viewport, None);
            return None;
        };
        let Some(target) = self.ime_target() else {
            self.ime_interrupt_pending = false;
            self.ime_cancelled = None;
            self.ime_tracker.observe(viewport, None);
            return None;
        };
        if self.ime_viewport != Some(viewport)
            && let Some(old) = self.ime_viewport.replace(viewport)
        {
            self.ime_tracker.end_viewport(old);
            let _ = self.ime_reset.remove(&old);
        }
        let committed = st.selection.text.as_ref().and_then(|t| deckcraft_engine::cmd::text::ime_surrounding(st, t));
        let mut observation =
            egui::ime_context::ImeObservation::new(ctx.cumulative_frame_nr_for(viewport), ctx.cumulative_pass_nr_for(viewport), target)
                .purpose(egui::IMEPurpose::Normal)
                .committed(committed.as_ref().map(|s| egui::ime_context::CommittedText::new(&s.text, s.cursor, s.anchor)));
        if committed.is_none() {
            observation = observation.unavailable_revision(st.revision);
        }
        let update = self.ime_tracker.observe(viewport, Some(observation))?;
        // Clear the interrupt only once it is part of a settled pass's output descriptor. A
        // discarded font/layout pass publishes nothing the platform sees, so keep it pending;
        // that pass is not the boundary at which stale pre-edits become a new generation.
        let interrupt = if ctx.will_discard() { false } else { std::mem::take(&mut self.ime_interrupt_pending) };
        if interrupt {
            // This pass's output carries the backend reset. The tombstone did its job for the
            // events already processed in this batch; from the next batch on every pre-edit or
            // commit is a new platform generation. A late event the OS queued across the reset
            // has no generation in its payload, so it cannot be rejected without also dropping
            // legitimate direct commits (documented residual in `ime_event`).
            self.ime_cancelled = None;
            if self.ime_composition.is_some_and(|c| c.settled) {
                self.ime_composition = None;
            }
        }
        let key = ImeDescriptorKey::new(update.owner, update.context_generation, update.purpose);
        ctx.output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput {
                owner: update.owner,
                context_generation: update.context_generation,
                purpose: update.purpose,
                rect,
                cursor_rect: rect,
                should_interrupt_composition: interrupt,
                reset_request: None,
                change_cause: update.change_cause,
                surrounding_text: update.surrounding_text,
            });
        });
        Some(key)
    }
}
