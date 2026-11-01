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

/// Snapshot the command's payload before a later command changes the selection or clipboard.
pub(crate) struct CopiedContent {
    text: String,
    html: String,
}

impl CopiedContent {
    /// Capture plain text and optional HTML; slide payloads use plain text only.
    pub(super) fn from_result(v: &Value, slides: bool) -> Option<Self> {
        let text = v.get("text")?.as_str()?.to_string();
        let html = if slides { String::new() } else { v.get("html").and_then(Value::as_str).unwrap_or("").to_string() };
        Some(Self { text, html })
    }
}

impl SlideApp {
    /// Queue the copy or cut for publication at the end of the UI pass.
    ///
    /// Slides go out as plain text. Text goes out as HTML plus the same plain words, so a paste
    /// back into DeckCraft still uses the internal clipboard. A textless shape leaves the
    /// invisible [`OBJECTS_MARKER`] rather than a picture: with only an image on the clipboard
    /// egui-winit drops Ctrl+V, and the shortcut could no longer paste the shape here (#182).
    pub(super) fn copy_clipboard(&mut self, cut: bool) {
        let cmd = if cut { "edit.cut" } else { "edit.copy" };
        let _ = self.run(cmd, json!({}));
    }

    /// Commands from every UI entry point publish through the same rich-text/marker policy.
    pub(super) fn flush_clipboard(&mut self, ctx: &egui::Context) {
        // Publish only the last copy in this pass. Otherwise an earlier egui CopyText output
        // could overwrite a later host HTML write when the backend processes the frame output.
        let Some(CopiedContent { text, html }) = self.pending_copy.take() else { return };
        if !html.is_empty() && self.services.clipboard_set_html.as_mut().is_some_and(|f| f(&html, &text)) {
            return;
        }
        ctx.copy_text(system_clipboard_text(&text));
    }
}
