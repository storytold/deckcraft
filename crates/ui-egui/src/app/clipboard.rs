use super::{SlideApp, Value, json};

/// Text put on the system clipboard when the copied objects (a rectangle, a picture, slides…)
/// carry no text. egui-winit drops Ctrl+V when the system clipboard is empty, so without it the
/// shortcut never reached the session and only the context menu could paste objects (#182). An
/// invisible word joiner, so a paste into another app adds nothing visible.
pub(crate) const OBJECTS_MARKER: &str = "\u{2060}";

/// What a copy/cut leaves on the system clipboard for the session's plain-text rendering `t`.
pub(crate) fn system_clipboard_text(t: &str) -> String {
    if t.is_empty() { OBJECTS_MARKER.to_string() } else { t.to_string() }
}

impl SlideApp {
    /// Offer the copy or cut on the system clipboard.
    ///
    /// Native hosts offer objects alongside HTML and plain text. A textless shape leaves the
    /// invisible [`OBJECTS_MARKER`] rather than a picture: with only an image on the clipboard
    /// egui-winit drops Ctrl+V, and the shortcut could no longer paste the shape here (#182).
    pub(super) fn publish_clipboard(&mut self, ctx: &egui::Context, cut: bool) {
        let cmd = if cut { "edit.cut" } else { "edit.copy" };
        let Ok(v) = self.session.execute(cmd, &json!({})) else { return };
        if let Some(offer) = self.clipboard_offer(&v) {
            self.offer_system_clipboard(ctx, offer);
        }
    }
    pub(super) fn clipboard_offer(&self, result: &Value) -> Option<(String, String, Option<Vec<u8>>)> {
        if result.get("copied").and_then(Value::as_bool) == Some(false) {
            return None;
        }
        Some((
            result.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
            result.get("html").and_then(Value::as_str).unwrap_or("").to_string(),
            self.services.clipboard_set_native.as_ref().and_then(|_| self.session.clipboard.to_system_bytes()),
        ))
    }

    pub(super) fn offer_system_clipboard(&mut self, ctx: &egui::Context, (text, html, native): (String, String, Option<Vec<u8>>)) {
        let plain = system_clipboard_text(&text);
        if native.as_ref().is_some_and(|bytes| self.services.clipboard_set_native.as_mut().is_some_and(|f| f(bytes, &html, &plain))) {
            return;
        }
        if !html.is_empty() && self.services.clipboard_set_html.as_mut().is_some_and(|f| f(&html, &text)) {
            return;
        }
        ctx.copy_text(plain);
    }

    /// Prefer objects over their text alternative. An unchanged offer keeps the local paste
    /// counter and clipboard, including when switching tabs inside the source window.
    pub(super) fn import_system_clipboard(&mut self) -> bool {
        let Some(bytes) = self.services.clipboard_native.as_mut().and_then(|f| f()) else { return false };
        self.session.clipboard.adopt_system_bytes(&bytes)
    }
}
