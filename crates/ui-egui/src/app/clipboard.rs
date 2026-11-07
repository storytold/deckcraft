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

/// Whether pasted `text` (and the clipboard's `html`) only name a picture that is on the clipboard
/// too, as a browser's Copy Image leaves them: the picture's address, perhaps followed by its alt
/// text, or HTML that is a lone `<img>`. Then Ctrl+V pastes the picture, as PowerPoint does.
/// Text with NULs is UTF-16 read as UTF-8 (Firefox's `text/x-moz-url`), never words to keep.
pub(crate) fn names_a_picture(text: &str, html: Option<&str>) -> bool {
    if text.contains('\0') {
        return true;
    }
    if let Some(h) = html
        && h.to_ascii_lowercase().contains("<img")
        && visible_text(h).trim().is_empty()
    {
        return true;
    }
    let mut lines = text.trim().lines();
    let first = lines.next().unwrap_or("").trim();
    let link = ["http://", "https://", "file://", "data:image/"].iter().any(|p| first.to_ascii_lowercase().starts_with(p));
    link && !first.contains(char::is_whitespace) && lines.count() <= 1
}

/// The text of an HTML fragment outside its tags (and outside comments and `<style>`/`<script>`
/// bodies, which browsers sometimes put around a copied picture).
fn visible_text(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::new();
    let mut i = 0;
    while i < html.len() {
        let rest = &lower[i..];
        let skip_to = |end: &str| rest.find(end).map(|e| i + e + end.len()).unwrap_or(html.len());
        if rest.starts_with("<!--") {
            i = skip_to("-->");
        } else if rest.starts_with("<style") {
            i = skip_to("</style>");
        } else if rest.starts_with("<script") {
            i = skip_to("</script>");
        } else if rest.starts_with('<') {
            i = skip_to(">");
        } else {
            let end = rest.find('<').map_or(html.len(), |e| i + e);
            out.push_str(&html[i..end]);
            i = end;
        }
    }
    out.replace("&nbsp;", " ")
}

impl SlideApp {
    /// Offer the copy or cut on the system clipboard.
    ///
    /// Text goes out as HTML plus the same plain words, so a paste back into DeckCraft still uses
    /// the internal clipboard and Pages keeps the bold. The HTML also carries the internal
    /// clipboard itself (shapes, slides, media; [`deckcraft_engine::clipboard`]), so another
    /// DeckCraft window pastes the objects; other apps see only the text and HTML. A textless
    /// shape leaves the invisible [`OBJECTS_MARKER`] as the plain text rather than a picture: with
    /// only an image on the clipboard egui-winit drops Ctrl+V, and the shortcut could no longer
    /// paste the shape here (#182).
    pub(super) fn publish_clipboard(&mut self, ctx: &egui::Context, cut: bool) {
        let cmd = if cut { "edit.cut" } else { "edit.copy" };
        let Ok(v) = self.session.execute(cmd, &json!({})) else { return };
        if let Some(plain) = self.offer_clipboard(&v) {
            ctx.copy_text(plain);
        }
    }

    /// Offer what Copy or Cut answered (`{text, html}`) on the system clipboard through the host:
    /// the HTML, with the session clipboard inside, and the plain text alternative. Returns the
    /// plain text still to be put there with `ctx.copy_text` when the host took nothing.
    pub(super) fn offer_clipboard(&mut self, copied: &Value) -> Option<String> {
        // A caret with nothing selected copies nothing: the session clipboard is as it was, and
        // the system clipboard stays whatever it is (not the old copy, published afresh).
        if copied.get("copied").and_then(Value::as_bool) == Some(false) {
            return None;
        }
        let text = copied.get("text").and_then(Value::as_str).unwrap_or("");
        let html = copied.get("html").and_then(Value::as_str).unwrap_or("");
        let plain = system_clipboard_text(text);
        // Without a host that takes HTML (the web, the test harness) the payload is never built.
        if let Some(f) = self.services.clipboard_set_html.as_mut() {
            let carried = deckcraft_engine::clipboard::embed(html, text, &self.session.clipboard);
            if let Some(rich) = carried.as_deref().or((!html.is_empty()).then_some(html)) {
                if f(rich, &plain) {
                    return None;
                }
                log::warn!("the system clipboard refused the HTML copy ({} bytes); offering the plain text only", rich.len());
            }
        }
        Some(plain)
    }

    /// Before a paste that reads nothing itself (the ribbon, a menu, the control channel without
    /// `text`/`html`/`payload`): a copy another DeckCraft window left on the system clipboard
    /// becomes the session's clipboard, as a Ctrl+V paste would make it. Our own copy stays.
    pub(super) fn adopt_system_clipboard(&mut self) {
        let Some(html) = self.services.clipboard_html.as_mut().and_then(|f| f()) else { return };
        let Some(payload) = deckcraft_engine::clipboard::extract(&html) else { return };
        if let Err(e) = deckcraft_engine::clipboard::adopt(&mut self.session, payload) {
            log::warn!("ignoring the DeckCraft clipboard on the system clipboard: {e}");
        }
    }
}

/// Whether running `id` with `params` should look at the system clipboard first
/// ([`SlideApp::adopt_system_clipboard`]): a paste given nothing to paste.
pub(crate) fn pastes_system_clipboard(id: &str, params: &Value) -> bool {
    matches!(id, "edit.paste" | "edit.pasteSpecial" | "edit.pasteText" | "app.pasteSpecial")
        && ["text", "html", "payload"].iter().all(|k| params.get(k).is_none())
}
