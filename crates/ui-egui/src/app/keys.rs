use super::clipboard::OBJECTS_MARKER;
use super::{Mods, Shell, SlideApp, Value, ViewMode, backstage, canvas_shell, json, menus, selpane, thumbs};
use crate::states;

impl SlideApp {
    /// The app shortcuts that still make sense on the empty File/start screen: create, open,
    /// the command palette, preferences and the File tab itself. Document commands (save, find,
    /// hyperlink, show, zoom) stay out; none of them can act on a presentation that is not open.
    fn start_screen_shortcut(key: egui::Key, m: Mods) -> bool {
        use egui::Key::*;
        matches!(
            (key, m.cmd, m.shift, m.alt),
            (O, true, false, false) | (P, true, true, false) | (Comma, true, false, false) | (F, false, false, true)
        )
    }

    /// File-tab navigation and document-free shortcuts, also used on the start screen.
    fn route_backstage_keys(&mut self, events: Vec<egui::Event>) {
        if self.backstage.is_none() {
            return;
        }
        // The File tab: its own keys (Esc goes back, arrows walk the side menu) and app
        // shortcuts (New, Open, Command Palette…); typing, pasting or deleting would edit the
        // hidden presentation.
        let start_screen = self.session.active().is_none();
        for e in events {
            if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                let mods = Mods { shift: modifiers.shift, alt: modifiers.alt, cmd: modifiers.command || modifiers.ctrl };
                let handled = backstage::key(self, key, mods)
                    // With no presentation the File home only owns the shortcuts that create or
                    // open one. Borrowing the whole document shortcut table here would also arm
                    // Save/Save As, hyperlink, find/replace, show and zoom against the empty
                    // start screen, which is a scope expansion no editing route needs.
                    || (mods.cmd || mods.alt)
                        && (!start_screen || Self::start_screen_shortcut(key, mods))
                        && menus::ui_shortcut(self, key, mods);
                if handled {
                    continue;
                }
                // Engine shortcuts that don't touch the hidden slide (Ctrl+N).
                if mods.cmd
                    && matches!(key, egui::Key::N)
                    && let Err(e) = self.session.key(key_name(key), mods)
                {
                    self.set_status(e.to_string());
                }
            }
        }
    }

    /// Keyboard and clipboard for the slide (when no text field has focus).
    pub(super) fn keyboard(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        // Ctrl+V with only an image on the clipboard: egui-winit sends no Paste event and swallows the
        // key press, so the release of a V press never seen is the paste. Tracked even while a text
        // field has focus, so that a V typed there is not taken for one.
        let mut image_paste = false;
        let mut pasted_image_event = false;
        for e in &events {
            match e {
                egui::Event::PasteImage(_) => {
                    pasted_image_event = true;
                    self.v_seen = true;
                }
                egui::Event::Paste(_) | egui::Event::Key { key: egui::Key::V, pressed: true, .. } => self.v_seen = true,
                egui::Event::Key { key: egui::Key::V, pressed: false, modifiers, .. } => {
                    // Only a release with Command/Ctrl held is a paste: a bare V release is not.
                    let seen = std::mem::take(&mut self.v_seen);
                    image_paste = !seen && (modifiers.command || modifiers.ctrl);
                }
                _ => {}
            }
        }
        let viewport = self.ime_viewport_or(ctx);
        if self.session.active().is_none() {
            // No document: keep slide-editing input suspended, while the start screen's
            // File-tab navigation and document-free shortcuts remain available.
            self.suspend_ime_reset(viewport);
            self.route_ime_controls_only(ctx, &events);
            if self.palette.is_none() && self.dialog.as_ref().is_none_or(|d| !d.modal()) && ctx.memory(|m| m.focused().is_none()) {
                self.route_backstage_keys(events);
            }
            self.drain_requests();
            return;
        }
        let modal = self.dialog.as_ref().is_some_and(|d| d.modal()) || self.palette.is_some();
        // While slide text is being edited, the text owns the keyboard (#93). Tab and the arrow
        // keys move egui's keyboard focus onto ordinary widgets (the window's drag handle, ribbon
        // buttons), and that stray focus would swallow every later key and end text entry. Let it
        // go; a real text field or an open menu keeps its keys.
        if !modal
            && self.session.active().is_some_and(|d| d.selection.text.is_some())
            && !ctx.text_edit_focused()
            && !ctx.any_popup_open()
            && let Some(id) = ctx.memory(|m| m.focused())
        {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        self.track_slides_focus(ctx);
        // The preview playing in the canvas has the keys (→, Esc).
        if self.show.is_some() {
            return;
        }
        // A Layers row with the keyboard keeps its own keys (↑ / ↓, Home / End, F2, Esc) and takes
        // no typing, but shortcuts (⌘G, ⇧⌘G, ⌘C, Delete…) act on the layers selected there.
        let focused = ctx.memory(|m| m.focused());
        let layer_row = focused.is_some_and(|f| selpane::is_row(self, f));
        let slides_keys = self.slides_focus || (self.ui.view == ViewMode::Sorter && self.ui.shell == Shell::Canvas);
        // Nothing selected on the slide: Delete and Cut have nothing to act on (the engine would
        // take the slide), as in PowerPoint.
        let nothing = self.session.active().is_some_and(|d| d.selection.shapes.is_empty() && d.selection.text.is_none());
        if (focused.is_some() && !layer_row) || modal {
            // A stock field/modal/palette owns input now. Retire the custom fence, then visit
            // controls in raw order; stock text/key events are left for the stock widgets.
            self.suspend_ime_reset(viewport);
            self.route_ime_controls_only(ctx, &events);
            return;
        }
        if self.backstage.is_some() {
            self.suspend_ime_reset(viewport);
            self.route_ime_controls_only(ctx, &events);
            self.route_backstage_keys(events);
            self.drain_requests();
            return;
        }
        if image_paste {
            self.paste_dropped_v(pasted_image_event);
        }
        for e in events {
            // An earlier command in this raw batch may have settled the composition. Request the
            // replacement at this ordered boundary, before the current event is considered.
            self.note_settled_composition(ctx);
            match e {
                egui::Event::ImeResetCompleted { request_id } => self.ime_reset_completed(request_id, ctx),
                egui::Event::PasteImage(image) => match color_image_png(&image) {
                    Ok(png) => {
                        let params = json!({"name": "Pasted image.png", "data": deckcraft_engine::cmd::base64_encode(&png)});
                        if let Err(e) = self.session.execute("insert.picture", &params) {
                            self.set_status(e.to_string());
                        }
                    }
                    Err(e) => self.set_status(e),
                },
                egui::Event::Text(_) if layer_row => {}
                egui::Event::Text(t) => {
                    let revision = self.session.active().map(|d| d.revision);
                    // In the canvas shell letters pick tools unless text is being edited (Enter
                    // edits a selected shape's text); the ribbon types over a selected shape.
                    let types_over = self.ui.shell == Shell::Classic;
                    if self.session.active().is_some_and(|d| d.selection.text.is_some() || (types_over && d.selection.shapes.len() == 1))
                        && let Err(e) = self.session.type_text(&t)
                    {
                        self.set_status(e.to_string());
                    }
                    if self.session.active().map(|d| d.revision) != revision {
                        self.note_application_edit(ctx);
                    }
                }
                egui::Event::Ime(ime) if !layer_row => self.ime_event(ctx, &ime),
                // The slide list copies and cuts whole slides, a slide with its states.
                egui::Event::Copy => self.copy_or_cut(ctx, false, slides_keys),
                egui::Event::Cut if nothing && !slides_keys => {}
                egui::Event::Cut => self.copy_or_cut(ctx, true, slides_keys),
                // ⇧⌘V (and ⌥⇧⌘V, the Mac's Paste and Match Style) pastes the text alone.
                egui::Event::Paste(text) if ctx.input(|i| i.modifiers.shift) => {
                    let text = if text == OBJECTS_MARKER { String::new() } else { text };
                    if let Err(e) = self.session.execute("edit.pasteText", &json!({"text": text})) {
                        self.set_status(e.to_string());
                    }
                }
                egui::Event::Paste(text) => self.paste_event(ctx, text, pasted_image_event),
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    self.key_event(ctx, key, modifiers, KeyContext { layer_row, slides_keys, nothing })
                }
                _ => {}
            }
        }
        self.drain_requests();
    }

    /// ⌘V whose key press egui-winit dropped (only an image on the clipboard): the image, else
    /// what was copied here (shapes, slides).
    fn paste_dropped_v(&mut self, pasted_image_event: bool) {
        // That paste: an image on the clipboard, else what was copied here (shapes, slides).
        let img = self.services.clipboard_image.as_mut().and_then(|f| f());
        if let Some(png) = img {
            let data = deckcraft_engine::cmd::base64_encode(&png);
            if let Err(e) = self.session.execute("insert.picture", &json!({"name": "Pasted image.png", "data": data})) {
                self.set_status(e.to_string());
            }
        } else if !pasted_image_event
            && (!self.session.clipboard.shapes.is_empty() || !self.session.clipboard.slides.is_empty())
            && let Err(e) = self.session.execute("edit.paste", &json!({}))
        {
            self.set_status(e.to_string());
        }
    }

    /// Copy or cut: the slide list copies and cuts whole slides, a slide with its states.
    fn copy_or_cut(&mut self, ctx: &egui::Context, cut: bool, slides_keys: bool) {
        if slides_keys {
            thumbs::select_runs(self);
        }
        self.publish_clipboard(ctx, cut, slides_keys);
    }

    /// A paste from the system clipboard.
    fn paste_event(&mut self, ctx: &egui::Context, text: String, pasted_image_event: bool) {
        // Our own marker for objects without text: paste what the session copied.
        let text = if text == OBJECTS_MARKER { String::new() } else { text };
        // A picture (TIFF from Keynote, a screenshot) lands when the paste has no text.
        // Text keeps its HTML (bold, italic, underline) when the host can read it.
        let html = self.services.clipboard_html.as_mut().and_then(|f| f()).filter(|h| !h.is_empty());
        // egui 0.37 sends `PasteImage` directly; the services fallback stays for
        // integrations that still send an empty `Paste`, and for a browser's Copy
        // Image, whose text is only the picture's link. Never insert twice.
        let wants_picture = text.is_empty() || super::clipboard::names_a_picture(&text, html.as_deref());
        let img = if wants_picture && !pasted_image_event { self.services.clipboard_image.as_mut().and_then(|f| f()) } else { None };
        let revision = self.session.active().map(|d| d.revision);
        let r = match img {
            Some(png) => {
                self.session.execute("insert.picture", &json!({"name": "Pasted image.png", "data": deckcraft_engine::cmd::base64_encode(&png)}))
            }
            None => {
                let mut params = json!({"text": text});
                if let Some(h) = html {
                    params["html"] = Value::String(h);
                }
                self.session.execute("edit.paste", &params)
            }
        };
        if let Err(e) = r {
            self.set_status(e.to_string());
        }
        if self.session.active().map(|d| d.revision) != revision {
            self.note_application_edit(ctx);
        }
    }

    /// A key press for the slide (or the slide list, the states strip, a Layers row).
    fn key_event(&mut self, ctx: &egui::Context, key: egui::Key, modifiers: egui::Modifiers, kc: KeyContext) {
        let KeyContext { layer_row, slides_keys, nothing } = kc;
        use egui::Key::{ArrowDown, ArrowUp, End, Escape, F2, Home, Tab};
        if layer_row && !modifiers.command && !modifiers.ctrl && matches!(key, ArrowUp | ArrowDown | Home | End | F2 | Escape | Tab) {
            return;
        }
        if key == egui::Key::Tab {
            // The slide has the keyboard, so Tab is ours (indent, tab character, next
            // shape). Left to egui it would also focus the first ribbon widget, and
            // every key after it would go there instead of the slide (#78).
            ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
        }
        if key == egui::Key::Escape && self.anim_preview.is_some() && !modifiers.command && !modifiers.ctrl && !modifiers.alt {
            self.anim_preview = None;
            return;
        }
        let mods = Mods { shift: modifiers.shift, alt: modifiers.alt, cmd: modifiers.command || modifiers.ctrl };
        if self.states_focus && states::strip_key(self, key, mods) {
            return;
        }
        if slides_keys && thumbs::slide_key(self, key, mods) {
            return;
        }
        if nothing && !slides_keys && matches!(key, egui::Key::Delete | egui::Key::Backspace) {
            return;
        }
        if self.ui.shell == Shell::Canvas && canvas_shell::tool_key(self, key, mods) {
            return;
        }
        if menus::mac_shortcut(self, key, mods) {
            return;
        }
        if !self.session.tool.drawing_freeform() && menus::ui_shortcut(self, key, mods) {
            return;
        }
        let name = key_name(key);
        let revision = self.session.active().map(|d| d.revision);
        match self.session.key(name, mods) {
            Ok(_) => {}
            Err(e) => self.set_status(e.to_string()),
        }
        if self.session.active().map(|d| d.revision) != revision {
            self.note_application_edit(ctx);
        }
    }
}

/// Who has the keyboard besides the slide, for [`SlideApp::key_event`].
#[derive(Clone, Copy)]
struct KeyContext {
    /// A Layers row has focus.
    layer_row: bool,
    /// The slide list (or the canvas layout's grid) has the keyboard.
    slides_keys: bool,
    /// Nothing is selected on the slide.
    nothing: bool,
}

/// Engine key name for an egui key.
pub fn key_name(key: egui::Key) -> &'static str {
    use egui::Key::*;
    match key {
        Enter => "Enter",
        Escape => "Escape",
        Backspace => "Backspace",
        Delete => "Delete",
        Tab => "Tab",
        ArrowLeft => "Left",
        ArrowRight => "Right",
        ArrowUp => "Up",
        ArrowDown => "Down",
        Home => "Home",
        End => "End",
        PageUp => "PageUp",
        PageDown => "PageDown",
        Space => "Space",
        F2 => "F2",
        F5 => "F5",
        F7 => "F7",
        other => other.name(),
    }
}

/// Largest clipboard image accepted into the document: 16 megapixels, the same bound as
/// background removal. The image was already allocated by the integration; this keeps the
/// RGBA copy, PNG and base64 from multiplying an arbitrarily large paste before `insert.picture`.
const MAX_PASTE_IMAGE_PIXELS: usize = 16_000_000;

/// Encode an egui color image as the PNG bytes `insert.picture` stores.
fn color_image_png(image: &egui::ColorImage) -> Result<Vec<u8>, String> {
    let [w, h] = image.size;
    let pixels = w.checked_mul(h).ok_or_else(|| "the pasted image is too large".to_string())?;
    if pixels == 0 {
        return Err("the pasted image is empty".into());
    }
    if pixels > MAX_PASTE_IMAGE_PIXELS {
        return Err(format!("the pasted image is {w}×{h} pixels; the limit is 16 megapixels"));
    }
    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    Ok(deckcraft_render::Image { width: w as u32, height: h as u32, pixels: rgba }.to_png())
}
