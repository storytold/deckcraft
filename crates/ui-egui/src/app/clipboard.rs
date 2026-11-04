use super::{SlideApp, Value, json};

/// Text put on the system clipboard when the copied objects (a rectangle, a picture, slides…)
/// carry no text. The marker preserves the session's editable object payload instead of
/// replacing it with a flattened native image. It is an invisible word joiner, so a paste
/// into another app adds nothing visible.
pub(crate) const OBJECTS_MARKER: &str = "\u{2060}";

/// What a copy/cut leaves on the system clipboard for the session's plain-text rendering `t`.
pub(crate) fn system_clipboard_text(t: &str) -> String {
    if t.is_empty() { OBJECTS_MARKER.to_string() } else { t.to_string() }
}

impl SlideApp {
    /// Supply appearance without overriding an explicit application theme.
    pub fn set_desktop_dark(&mut self, dark: Option<bool>) {
        self.desktop_dark = dark;
    }

    /// Ask the host for clipboard input, retaining its target until delivery.
    pub(crate) fn request_desktop_clipboard(&mut self, ctx: &egui::Context, kind: crate::DesktopClipboard) {
        if let Some(doc) = self.session.active() {
            if self.desktop_clipboard.iter().any(|request| request.kind == kind) {
                self.set_status("Clipboard request still pending; click back into the document, then retry.");
                return;
            }
            let focus = ctx.memory(|memory| memory.focused());
            self.desktop_clipboard.push(crate::DesktopClipboardRequest {
                kind,
                document: doc.uid,
                revision: doc.revision,
                selection: doc.selection.clone(),
                viewport: ctx.viewport_id(),
                focus,
                cursor: focus.and_then(|id| egui::TextEdit::load_state(ctx, id)).and_then(|state| state.cursor.char_range()),
                arrived: false,
                rejected: false,
            });
            ctx.send_viewport_cmd(match kind {
                crate::DesktopClipboard::Copy => egui::ViewportCommand::RequestCopy,
                crate::DesktopClipboard::Cut => egui::ViewportCommand::RequestCut,
                crate::DesktopClipboard::Paste => egui::ViewportCommand::RequestPaste,
            });
            ctx.request_repaint();
        }
    }

    /// Retract clipboard work if the original document or modal owner changed.
    pub(super) fn guard_desktop_clipboard(&mut self, ctx: &egui::Context) {
        self.guard_desktop_clipboard_events(ctx, &mut Vec::new(), false);
    }

    pub(super) fn guard_desktop_clipboard_delivery(&mut self, ctx: &egui::Context) {
        let mut events = ctx.input(|input| input.events.clone());
        self.guard_desktop_clipboard_events(ctx, &mut events, true);
        ctx.input_mut(|input| input.events = events);
    }

    pub(super) fn desktop_clipboard_input_changes(events: &[egui::Event]) -> bool {
        events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key { pressed: true, .. }
                    | egui::Event::PointerButton { pressed: true, .. }
                    | egui::Event::Ime(egui::ImeEvent::Commit(_))
                    | egui::Event::Text(_)
                    | egui::Event::WindowFocused(_)
            )
        })
    }

    pub(super) fn guard_desktop_clipboard_events(&mut self, ctx: &egui::Context, events: &mut Vec<egui::Event>, delivered: bool) {
        let blocked = crate::desktop_menu::blocked(self);
        let focus = ctx.memory(|memory| memory.focused());
        let cursor = focus.and_then(|id| egui::TextEdit::load_state(ctx, id)).and_then(|state| state.cursor.char_range());
        let doc = self.session.active();
        let lost_focus = events.iter().any(|event| matches!(event, egui::Event::WindowFocused(false)));
        // Scrolling cancels PRIMARY in the native bridge, but not a regular RequestPaste.
        // Invalidate this target without losing the guard for its still-pending response.
        let scrolled = events.iter().any(|event| matches!(event, egui::Event::MouseWheel { .. }));
        let canceled = Self::desktop_clipboard_input_changes(events);
        let mut refused = false;
        self.desktop_clipboard.retain_mut(|request| {
            request.rejected |= blocked
                || lost_focus
                || scrolled
                || ctx.viewport_id() != request.viewport
                || focus != request.focus
                || cursor != request.cursor
                || doc.is_none_or(|doc| doc.uid != request.document || doc.revision != request.revision || doc.selection != request.selection);
            let arrived = events.iter().any(|event| request.kind.matches(event));
            request.rejected |= canceled;
            request.arrived |= arrived;
            if arrived && request.rejected {
                events.retain(|event| !request.kind.matches(event));
                refused = true;
            }
            // Keep rejected identities until delivery: switching away and back must not revive
            // a stale request. Paste events carry no request id; a conflicting paste is refused.
            // Do not time out app bookkeeping: output can stall before the host starts its
            // three-second transfer. At most one outstanding request of each kind is retained.
            // Check delivery before cancellation when both occur in the same batch.
            // Only the raw hook can observe native cancellation. A synthetic key/text event
            // injected later invalidates the app target without canceling the host transfer.
            !(delivered && request.arrived)
        });
        if refused {
            self.set_status("Clipboard target changed; paste/copy was canceled. Please try again.");
        }
    }

    /// Offer the copy or cut on the system clipboard.
    ///
    /// Slides go out as plain text. Text goes out as HTML plus the same plain words, so a paste
    /// back into DeckCraft still uses the internal clipboard. A textless shape leaves the
    /// invisible [`OBJECTS_MARKER`] rather than a flattened picture, preserving the editable
    /// internal shape payload on a later paste (#182).
    pub(crate) fn publish_clipboard(&mut self, ctx: &egui::Context, cut: bool) {
        let cmd = if cut { "edit.cut" } else { "edit.copy" };
        let Ok(v) = self.session.execute(cmd, &json!({})) else { return };
        let text = v.get("text").and_then(Value::as_str).unwrap_or("").to_string();
        let html = v.get("html").and_then(Value::as_str).unwrap_or("").to_string();
        if self.session.clipboard.slides.is_empty() && !html.is_empty() && self.services.clipboard_set_html.as_mut().is_some_and(|f| f(&html, &text))
        {
            return;
        }
        ctx.copy_text(system_clipboard_text(&text));
    }
}

#[cfg(test)]
mod desktop_tests {
    use super::SlideApp;
    use crate::DesktopClipboard;

    fn app() -> SlideApp {
        SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default())
    }

    fn image() -> egui::Event {
        egui::Event::PasteImage(std::sync::Arc::new(egui::ColorImage::new([1, 1], vec![egui::Color32::RED])))
    }

    #[test]
    fn desktop_clipboard_keeps_delayed_delivery_and_bounds_outstanding_requests() {
        for event in [egui::Event::Paste("text".into()), image()] {
            let mut app = app();
            let ctx = egui::Context::default();
            app.request_desktop_clipboard(&ctx, DesktopClipboard::Paste);
            app.request_desktop_clipboard(&ctx, DesktopClipboard::Paste);
            assert_eq!(app.desktop_clipboard.len(), 1);
            for _ in 0..20 {
                app.guard_desktop_clipboard_events(&ctx, &mut Vec::new(), true);
            }
            assert_eq!(app.desktop_clipboard.len(), 1, "a stalled host must not erase the delivery identity");
            let mut events = vec![event];
            app.guard_desktop_clipboard_events(&ctx, &mut events, true);
            assert_eq!(events.len(), 1);
            assert!(app.desktop_clipboard.is_empty());
        }
    }

    #[test]
    fn desktop_clipboard_rejects_changed_selection_revision_and_window() {
        for event in [egui::Event::Paste("text".into()), image()] {
            for change in 0..3 {
                let mut app = app();
                let ctx = egui::Context::default();
                app.request_desktop_clipboard(&ctx, DesktopClipboard::Paste);
                if change == 0 {
                    app.session.active_mut().unwrap().selection.slide = 1;
                } else if change == 1 {
                    app.session.active_mut().unwrap().revision += 1;
                }
                let mut raw = egui::RawInput { events: vec![event.clone()], ..Default::default() };
                if change == 2 {
                    raw.viewport_id = egui::ViewportId::from_hash_of("different-window");
                }
                app.raw_input_hook(&ctx, &mut raw);
                assert!(raw.events.is_empty(), "change {change} must reject delivery before widgets");
            }
        }
    }

    #[test]
    fn desktop_clipboard_rechecks_after_logic_and_does_not_revive_invalidated_target() {
        let mut app = app();
        let ctx = egui::Context::default();
        app.request_desktop_clipboard(&ctx, DesktopClipboard::Paste);
        let mut raw = egui::RawInput { events: vec![image()], ..Default::default() };
        app.raw_input_hook(&ctx, &mut raw);
        assert_eq!(raw.events.len(), 1);
        let revision = app.session.active().unwrap().revision;
        app.session.active_mut().unwrap().revision += 1;
        app.guard_desktop_clipboard(&ctx);
        app.session.active_mut().unwrap().revision = revision;
        app.guard_desktop_clipboard_events(&ctx, &mut raw.events, true);
        assert!(raw.events.is_empty(), "a change after the raw hook must still cancel delivery");
    }

    #[test]
    fn desktop_clipboard_cancellation_batch_rejects_delivery_then_allows_retry() {
        let mut app = app();
        let ctx = egui::Context::default();
        app.request_desktop_clipboard(&ctx, DesktopClipboard::Paste);
        let key = egui::Event::Key { key: egui::Key::ArrowRight, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        let mut events = vec![image(), key.clone()];
        app.guard_desktop_clipboard_events(&ctx, &mut events, true);
        assert_eq!(events, vec![key]);
        assert!(app.desktop_clipboard.is_empty());
        let mut retry = vec![egui::Event::Paste("keyboard retry".into())];
        app.guard_desktop_clipboard_events(&ctx, &mut retry, true);
        assert_eq!(retry.len(), 1);
    }

    #[test]
    fn desktop_clipboard_scroll_keeps_regular_delivery_guard() {
        let mut app = app();
        let ctx = egui::Context::default();
        app.request_desktop_clipboard(&ctx, DesktopClipboard::Paste);
        let mut scroll = vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 20.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
        }];
        app.guard_desktop_clipboard_events(&ctx, &mut scroll, true);
        assert_eq!(app.desktop_clipboard.len(), 1, "wheel does not cancel the native regular transfer");
        app.run("file.new", serde_json::json!({})).unwrap();
        let mut events = vec![egui::Event::Paste("late old clipboard".into())];
        app.guard_desktop_clipboard_events(&ctx, &mut events, true);
        assert!(events.is_empty());
    }

    #[test]
    fn desktop_clipboard_synthetic_edit_keeps_guard_until_native_response() {
        let mut app = app();
        let ctx = egui::Context::default();
        app.request_desktop_clipboard(&ctx, DesktopClipboard::Paste);
        let mut injected = vec![egui::Event::Text("scripted edit".into())];
        app.guard_desktop_clipboard_events(&ctx, &mut injected, true);
        assert_eq!(app.desktop_clipboard.len(), 1);
        let mut response = vec![image()];
        app.guard_desktop_clipboard_events(&ctx, &mut response, true);
        assert!(response.is_empty());
        assert!(app.desktop_clipboard.is_empty());

        app.request_desktop_clipboard(&ctx, DesktopClipboard::Paste);
        let mut raw = egui::RawInput {
            events: vec![egui::Event::PointerButton {
                pos: egui::pos2(1.0, 1.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        app.raw_input_hook(&ctx, &mut raw);
        assert!(app.desktop_clipboard.is_empty(), "an actual native pointer press cancels the pending host generation");

        app.request_desktop_clipboard(&ctx, DesktopClipboard::Paste);
        let carried = egui::Event::Text("carried synthetic edit".into());
        app.synthetic_injected = vec![carried.clone()];
        app.synthetic_injected_at = Some(0);
        let mut raw = egui::RawInput { events: vec![carried], ..Default::default() };
        app.raw_input_hook(&ctx, &mut raw);
        assert_eq!(app.desktop_clipboard.len(), 1, "carried synthetic input does not cancel the host transfer");
        let mut response = vec![image()];
        app.guard_desktop_clipboard_events(&ctx, &mut response, true);
        assert!(response.is_empty());
    }
}
