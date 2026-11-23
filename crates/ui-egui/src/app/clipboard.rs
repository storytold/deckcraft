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
    /// The slide list keeps the keyboard until a click lands elsewhere, or a shape or text is
    /// selected (from the Layers list, say).
    pub(super) fn track_slides_focus(&mut self, ctx: &egui::Context) {
        let elsewhere =
            ctx.input(|i| i.pointer.primary_pressed() && i.pointer.interact_pos().is_some_and(|p| !self.thumbs_rect.is_some_and(|r| r.contains(p))));
        let editing = self.session.active().is_none_or(|d| !d.selection.shapes.is_empty() || d.selection.text.is_some());
        if elsewhere || editing {
            self.slides_focus = false;
        }
        let off_strip =
            ctx.input(|i| i.pointer.primary_pressed() && i.pointer.interact_pos().is_some_and(|p| !self.strip_rect.is_some_and(|r| r.contains(p))));
        if off_strip || editing || !crate::states::strip_shown(self) {
            self.states_focus = false;
        }
    }

    /// Paste to Replace (⇧⌘R), as in canvas editors: what's on the clipboard (an image, else what
    /// was copied here) takes each selected object's place: centred on it, at its place in the
    /// stack, and the object removed. One undo step.
    pub fn paste_to_replace(&mut self) -> Result<Value, String> {
        let Some(st) = self.session.active() else { return Err("no presentation".into()) };
        let top: Vec<deckcraft_model::ShapeId> = st.shapes().iter().map(|s| s.id).collect();
        let targets: Vec<(deckcraft_model::ShapeId, deckcraft_model::Xfrm)> = st
            .selection
            .shapes
            .iter()
            .filter(|id| top.contains(id))
            .filter_map(|id| st.shape(*id).map(|s| (*id, deckcraft_engine::cmd::xfrm_on_slide(&st.doc, &st.selection, s))))
            .collect();
        if targets.is_empty() {
            return Err("Select the objects to replace".into());
        }
        let image = self.services.clipboard_image.as_mut().and_then(|f| f());
        if image.is_none() && self.session.clipboard.shapes.is_empty() {
            return Err("Copy something to paste in their place".into());
        }
        let before = self.session.active().map(|d| d.history.undo.len()).unwrap_or(0);
        let mut pasted = vec![];
        for (target, x) in targets {
            let r = match &image {
                Some(png) => self.run("insert.picture", json!({"name": "Pasted image.png", "data": deckcraft_engine::cmd::base64_encode(png)})),
                None => self.run("edit.paste", json!({})),
            };
            r?;
            let Some(st) = self.session.active() else { break };
            let ids = st.selection.shapes.clone();
            let boxes: Vec<deckcraft_model::Xfrm> =
                ids.iter().filter_map(|id| st.shape(*id).map(|s| deckcraft_engine::cmd::xfrm_on_slide(&st.doc, &st.selection, s))).collect();
            let Some(first) = boxes.first() else { continue };
            let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x + first.w, first.y + first.h);
            for b in &boxes {
                (x0, y0, x1, y1) = (x0.min(b.x), y0.min(b.y), x1.max(b.x + b.w), y1.max(b.y + b.h));
            }
            let (dx, dy) = (x.x + x.w / 2.0 - (x0 + x1) / 2.0, x.y + x.h / 2.0 - (y0 + y1) / 2.0);
            self.run("shape.move", json!({"ids": ids, "dx": dx, "dy": dy}))?;
            // Into the target's place in the stack (it moves up as they go in below it).
            let at = self.session.active().and_then(|d| d.shapes().iter().position(|s| s.id == target)).unwrap_or(0);
            for (k, id) in ids.iter().enumerate() {
                self.run("arrange.reorder", json!({"id": id, "index": at + k}))?;
            }
            self.run("edit.delete", json!({"ids": [target]}))?;
            pasted.extend(ids);
        }
        let _ = self.run("selection.select", json!({"ids": pasted}));
        if let Some(st) = self.session.active_mut() {
            let undo = &mut st.history.undo;
            if undo.len() > before + 1 {
                undo.truncate(before + 1);
            }
            if undo.len() > before
                && let Some(e) = undo.last_mut()
            {
                e.label = "Paste to Replace".into();
            }
        }
        Ok(json!({"ids": pasted}))
    }

    /// Offer the copy or cut on the system clipboard (`slides`: the slide list has the keyboard,
    /// so the selected slides).
    ///
    /// Slides go out as plain text. Text goes out as HTML plus the same plain words, so a paste
    /// back into DeckCraft still uses the internal clipboard. A textless shape leaves the
    /// invisible [`OBJECTS_MARKER`] rather than a picture: with only an image on the clipboard
    /// egui-winit drops Ctrl+V, and the shortcut could no longer paste the shape here (#182).
    pub(super) fn publish_clipboard(&mut self, ctx: &egui::Context, cut: bool, slides: bool) {
        let cmd = if cut { "edit.cut" } else { "edit.copy" };
        let p = if slides { json!({"scope": "slides"}) } else { json!({}) };
        let Ok(v) = self.session.execute(cmd, &p) else { return };
        let text = v.get("text").and_then(Value::as_str).unwrap_or("").to_string();
        let html = v.get("html").and_then(Value::as_str).unwrap_or("").to_string();
        if self.session.clipboard.slides.is_empty() && !html.is_empty() && self.services.clipboard_set_html.as_mut().is_some_and(|f| f(&html, &text))
        {
            return;
        }
        ctx.copy_text(system_clipboard_text(&text));
    }
}
