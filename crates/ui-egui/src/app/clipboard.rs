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
    /// Slides go out as plain text. Text goes out as HTML plus the same plain words, so a paste
    /// back into DeckCraft still uses the internal clipboard. A textless shape leaves the
    /// invisible [`OBJECTS_MARKER`] rather than a picture: with only an image on the clipboard
    /// egui-winit drops Ctrl+V, and the shortcut could no longer paste the shape here (#182).
    pub(super) fn publish_clipboard(&mut self, ctx: &egui::Context, cut: bool) {
        let cmd = if cut { "edit.cut" } else { "edit.copy" };
        let Ok(v) = self.session.execute(cmd, &json!({})) else { return };
        let text = v.get("text").and_then(Value::as_str).unwrap_or("").to_string();
        let html = v.get("html").and_then(Value::as_str).unwrap_or("").to_string();
        // HTML only with words to go with it: an empty text box would leave empty plain text, and
        // egui-winit drops Ctrl+V when the clipboard text is empty, so it gets the marker instead.
        if self.session.clipboard.slides.is_empty()
            && !html.is_empty()
            && !text.is_empty()
            && self.services.clipboard_set_html.as_mut().is_some_and(|f| f(&html, &text))
        {
            return;
        }
        ctx.copy_text(system_clipboard_text(&text));
    }
}

/// Whether pasted plain text is SVG markup: what vector apps (VectorCraft, Illustrator, Inkscape,
/// Figma…) put on the clipboard as a copy's plain text, next to a small picture of it.
fn is_svg_markup(text: &str) -> bool {
    let t = text.trim_start_matches('\u{feff}').trim();
    (t.starts_with("<?xml") || t.starts_with("<svg") || t.starts_with("<!DOCTYPE svg")) && t.contains("<svg") && t.ends_with("</svg>")
}

/// The size in points of SVG markup's root element: its `width` and `height` (unitless and `px` as
/// points, as vector apps write them; `pt`, `pc`, `in`, `cm` and `mm` converted), else its `viewBox`.
pub(super) fn svg_size(svg: &str) -> Option<(f64, f64)> {
    let start = svg.find("<svg")?;
    let tag = &svg[start..start + svg[start..].find('>')?];
    let attr = |name: &str| {
        let key = format!("{name}=");
        let at = tag.match_indices(key.as_str()).find(|(i, _)| tag[..*i].ends_with(char::is_whitespace))?.0 + key.len();
        let rest = &tag[at..];
        let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        rest[1..].split(quote).next().map(str::trim)
    };
    let length = |v: &str| {
        let unit_at = v.find(|c: char| c.is_ascii_alphabetic() || c == '%').unwrap_or(v.len());
        let n: f64 = v[..unit_at].trim().parse().ok()?;
        let per = match v[unit_at..].trim() {
            "" | "px" | "pt" => 1.0,
            "pc" => 12.0,
            "in" => 72.0,
            "cm" => 72.0 / 2.54,
            "mm" => 72.0 / 25.4,
            _ => return None,
        };
        Some(n * per).filter(|x| x.is_finite() && *x > 0.0)
    };
    if let (Some(w), Some(h)) = (attr("width").and_then(length), attr("height").and_then(length)) {
        return Some((w, h));
    }
    let vb: Vec<f64> =
        attr("viewBox")?.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
    match vb[..] {
        [_, _, w, h] if w > 0.0 && h > 0.0 => Some((w, h)),
        _ => None,
    }
}

impl SlideApp {
    /// Handle pasted plain text that is SVG markup from another app: `true` when it was dealt with
    /// (a picture inserted, or an error shown), `false` when it should go on to the ordinary text
    /// paste. Our own copy (a shape whose words happen to be markup) stays with `edit.paste`, and a
    /// host that can neither render SVG nor read a clipboard picture pastes the markup as text.
    pub(super) fn paste_svg_text(&mut self, ctx: &egui::Context, text: &str) -> bool {
        if !is_svg_markup(text) || self.session.clipboard.plain == text {
            return false;
        }
        let revision = self.session.active().map(|d| d.revision);
        let handled = match self.paste_svg(text) {
            Ok(done) => done,
            Err(e) => {
                self.set_status(e);
                true
            }
        };
        if self.session.active().map(|d| d.revision) != revision {
            self.note_application_edit(ctx);
        }
        handled
    }

    /// Paste SVG markup from another app as one picture centred on the slide, at the drawing's size
    /// in points (scaled down to fit the slide). Sharp when the host renders SVG
    /// (`Services::clipboard_svg`), else the picture the other app put on the clipboard with it.
    /// `Ok(false)` when the host can do neither (the web build), so the caller pastes the markup
    /// as ordinary text instead.
    pub(super) fn paste_svg(&mut self, svg: &str) -> Result<bool, String> {
        if self.services.clipboard_svg.is_none() && self.services.clipboard_image.is_none() {
            return Ok(false);
        }
        let (png, (w, h)) = match self.services.clipboard_svg.as_mut().and_then(|f| f(svg)) {
            Some((png, w, h)) => (png, svg_size(svg).unwrap_or((w, h))),
            None => {
                let png = self.services.clipboard_image.as_mut().and_then(|f| f()).ok_or("Couldn't read the copied drawing")?;
                (png, svg_size(svg).ok_or("Couldn't read the copied drawing's size")?)
            }
        };
        let Some(d) = self.session.active() else { return Ok(true) };
        let (sw, sh) = (d.doc.slide_size.width, d.doc.slide_size.height);
        let fit = (sw / w).min(sh / h).min(1.0);
        let (w, h) = (w * fit, h * fit);
        let rect = [(sw - w) / 2.0, (sh - h) / 2.0, w, h];
        let data = deckcraft_engine::cmd::base64_encode(&png);
        self.session
            .execute("insert.picture", &json!({"name": "Pasted drawing.png", "data": data, "rect": rect}))
            .map(|_| true)
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{is_svg_markup, svg_size};

    #[test]
    fn svg_size_reads_width_height_units_and_viewbox() {
        assert_eq!(svg_size(r#"<svg width="521.962" height="565.063" viewBox="0 0 521.962 565.063">"#), Some((521.962, 565.063)));
        assert_eq!(svg_size(r#"<svg xmlns="x" stroke-width="3" width="1in" height="72pt">"#), Some((72.0, 72.0)));
        assert_eq!(svg_size(r#"<svg viewBox="0 0 40 30" width="100%">"#), Some((40.0, 30.0)));
        assert_eq!(svg_size("<svg>"), None);
        assert!(is_svg_markup("\u{feff}<?xml version=\"1.0\"?>\n<svg></svg>\n"));
        assert!(!is_svg_markup("hello <svg> world"));
        assert!(!is_svg_markup("<svg> but no end"));
    }
}
