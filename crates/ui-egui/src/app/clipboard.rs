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

/// What a paste should do with the system clipboard, ahead of the session's own copy.
enum SystemPaste {
    /// The system clipboard is ours, empty, or unreadable. Use the session clipboard.
    Session,
    Text {
        text: String,
        html: Option<String>,
    },
    Image(Vec<u8>),
}

impl SlideApp {
    /// Read the system clipboard for a paste. `pictures` inserts a picture when the clipboard
    /// holds one and no real text (a screenshot, or a copied image whose only text is its URL).
    fn system_paste(&mut self, pictures: bool) -> SystemPaste {
        let text = self.services.clipboard_text.as_mut().and_then(|read| read()).unwrap_or_default();
        // The marker is what a textless copy of ours puts on the system clipboard (#182).
        if text == OBJECTS_MARKER {
            return SystemPaste::Session;
        }
        self.decide_paste(&text, pictures)
    }

    /// `text` is already the plain text (possibly empty). An image wins only when there is no
    /// real text, or the text is just the picture's URL.
    fn decide_paste(&mut self, text: &str, pictures: bool) -> SystemPaste {
        let trimmed = text.trim();
        if pictures
            && (trimmed.is_empty() || text_is_image_label(trimmed))
            && let Some(png) = self.services.clipboard_image.as_mut().and_then(|read| read()).filter(|png| !png.is_empty())
        {
            return SystemPaste::Image(png);
        }
        if trimmed.is_empty() {
            return SystemPaste::Session;
        }
        let html = self.services.clipboard_html.as_mut().and_then(|read| read()).filter(|html| !html.is_empty());
        SystemPaste::Text { text: text.to_string(), html }
    }

    /// Paste `offer`. A `Paste` event with no external content still pastes the session clipboard
    /// (`session_fallback`). A shortcut that never produced an event does not.
    fn offer_clipboard(&mut self, ctx: &egui::Context, offer: SystemPaste, session_fallback: bool) {
        let revision = self.session.active().map(|d| d.revision);
        let result = match offer {
            SystemPaste::Image(png) => {
                let data = deckcraft_engine::cmd::base64_encode(&png);
                self.session.execute("insert.picture", &json!({"name": "Pasted image.png", "data": data}))
            }
            SystemPaste::Text { text, html } => {
                let mut params = json!({"text": text});
                if let Some(h) = html {
                    params["html"] = Value::String(h);
                }
                self.session.execute("edit.paste", &params)
            }
            SystemPaste::Session if session_fallback => self.session.execute("edit.paste", &json!({})),
            SystemPaste::Session => return,
        };
        if let Err(e) = result {
            self.set_status(e.to_string());
        }
        if self.session.active().map(|d| d.revision) != revision {
            self.note_application_edit(ctx);
        }
    }

    /// Ctrl+V delivered a paste event. The objects marker pastes the session copy. Anything else
    /// is the other program's text, or its picture when the event carried no real text.
    pub(super) fn paste_event(&mut self, ctx: &egui::Context, text: &str, allow_image: bool) {
        if text == OBJECTS_MARKER {
            self.offer_clipboard(ctx, SystemPaste::Session, true);
            return;
        }
        let offer = self.decide_paste(text, allow_image);
        self.offer_clipboard(ctx, offer, true);
    }

    /// Ctrl+V was swallowed and no paste event arrived. Read the system clipboard ourselves.
    /// An empty or unreadable clipboard does not fall back to an older copy inside the session.
    pub(super) fn paste_shortcut_without_event(&mut self, ctx: &egui::Context) {
        let offer = self.system_paste(true);
        self.offer_clipboard(ctx, offer, false);
    }

    /// Ribbon, menus and the context menu call `edit.paste` with no text. Fill in the system
    /// clipboard, or turn a picture-only clipboard into `insert.picture`.
    pub(super) fn paste_command(&mut self, id: &str, params: &mut Value) -> Option<&'static str> {
        if !matches!(id, "edit.paste" | "edit.pasteText") || params.get("text").is_some() {
            return None;
        }
        match self.system_paste(id == "edit.paste") {
            SystemPaste::Text { text, html } => {
                params["text"] = Value::String(text);
                if id == "edit.paste"
                    && let Some(h) = html
                {
                    params["html"] = Value::String(h);
                }
                None
            }
            SystemPaste::Image(png) => {
                let data = deckcraft_engine::cmd::base64_encode(&png);
                *params = json!({"name": "Pasted image.png", "data": data});
                Some("insert.picture")
            }
            SystemPaste::Session => None,
        }
    }

    /// Offer the copy or cut on the system clipboard.
    ///
    /// Slides go out as plain text. Text goes out as HTML plus the same plain words, so a paste
    /// back into DeckCraft still uses the internal clipboard. A textless shape leaves the
    /// invisible [`OBJECTS_MARKER`] rather than a picture: with only an image on the clipboard
    /// egui-winit drops Ctrl+V, and the shortcut could no longer paste the shape here (#182).
    pub(super) fn publish_clipboard(&mut self, ctx: &egui::Context, cut: bool) {
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

/// A copied picture often carries its address as the only text. That is the picture, not a caption.
fn text_is_image_label(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("https://").or_else(|| text.strip_prefix("http://")) else {
        return false;
    };
    !rest.is_empty() && text.len() <= 2048 && !text.chars().any(char::is_whitespace)
}
