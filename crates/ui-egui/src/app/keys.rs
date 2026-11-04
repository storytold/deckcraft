use super::{Mods, SlideApp, backstage, json, menus};

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
        if self.backstage.is_some() {
            self.suspend_ime_reset(viewport);
            self.route_ime_controls_only(ctx, &events);
            self.route_backstage_keys(events);
            self.drain_requests();
            return;
        }
        // A text field pastes into itself. Anything else that holds focus (a ribbon button, a
        // slider) must not swallow a copy from another program.
        let text_field = ctx.text_edit_focused();
        if !modal && !text_field {
            self.apply_clipboard(ctx, &events, image_paste, pasted_image_event);
        }
        if ctx.memory(|m| m.focused().is_some()) || modal {
            // A stock field/modal/palette owns input now. Retire the custom fence, then visit
            // controls in raw order; stock text/key events are left for the stock widgets.
            // Formatting shortcuts still reach the notes, the same ones the slide text uses.
            self.suspend_ime_reset(viewport);
            self.route_ime_controls_only(ctx, &events);
            if !modal && self.notes_format_target() {
                self.notes_format_keys(ctx, &events);
            }
            return;
        }
        for e in events {
            // An earlier command in this raw batch may have settled the composition. Request the
            // replacement at this ordered boundary, before the current event is considered.
            if matches!(e, egui::Event::Paste(_) | egui::Event::PasteImage(_) | egui::Event::Copy | egui::Event::Cut) {
                continue;
            }
            self.note_settled_composition(ctx);
            match e {
                egui::Event::ImeResetCompleted { request_id } => self.ime_reset_completed(request_id, ctx),
                egui::Event::Text(t) => {
                    let revision = self.session.active().map(|d| d.revision);
                    if self.session.active().is_some_and(|d| d.selection.text.is_some() || d.selection.shapes.len() == 1)
                        && let Err(e) = self.session.type_text(&t)
                    {
                        self.set_status(e.to_string());
                    }
                    if self.session.active().map(|d| d.revision) != revision {
                        self.note_application_edit(ctx);
                    }
                }
                egui::Event::Ime(ime) => self.ime_event(ctx, &ime),
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    if key == egui::Key::Tab {
                        // The slide has the keyboard, so Tab is ours (indent, tab character, next
                        // shape). Left to egui it would also focus the first ribbon widget, and
                        // every key after it would go there instead of the slide (#78).
                        ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
                    }
                    if key == egui::Key::Escape && self.anim_preview.is_some() && !modifiers.command && !modifiers.ctrl && !modifiers.alt {
                        self.anim_preview = None;
                        continue;
                    }
                    let mods = Mods { shift: modifiers.shift, alt: modifiers.alt, cmd: modifiers.command || modifiers.ctrl };
                    if !self.session.tool.drawing_freeform() && menus::ui_shortcut(self, key, mods) {
                        continue;
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
                _ => {}
            }
        }
        self.drain_requests();
    }

    /// Copy, cut and paste. Also a Ctrl+V that egui swallowed because the system clipboard had no
    /// text it could read (a screenshot on Windows).
    fn apply_clipboard(&mut self, ctx: &egui::Context, events: &[egui::Event], image_paste: bool, pasted_image_event: bool) {
        let saw_paste = events.iter().any(|e| matches!(e, egui::Event::Paste(_) | egui::Event::PasteImage(_)));
        if image_paste && !saw_paste {
            self.note_settled_composition(ctx);
            self.paste_shortcut_without_event(ctx);
        }
        for e in events {
            self.note_settled_composition(ctx);
            match e {
                egui::Event::PasteImage(image) => match color_image_png(image) {
                    Ok(png) => {
                        let data = deckcraft_engine::cmd::base64_encode(&png);
                        let revision = self.session.active().map(|d| d.revision);
                        if let Err(e) = self.session.execute("insert.picture", &json!({"name": "Pasted image.png", "data": data})) {
                            self.set_status(e.to_string());
                        }
                        if self.session.active().map(|d| d.revision) != revision {
                            self.note_application_edit(ctx);
                        }
                    }
                    Err(e) => self.set_status(e),
                },
                egui::Event::Copy => self.publish_clipboard(ctx, false),
                egui::Event::Cut => self.publish_clipboard(ctx, true),
                egui::Event::Paste(text) => self.paste_event(ctx, text, !pasted_image_event),
                _ => {}
            }
        }
    }

    fn notes_format_target(&self) -> bool {
        self.notes_focused && self.session.active().is_some_and(|d| d.selection.text.as_ref().is_some_and(|t| t.notes))
    }

    /// Ctrl/Cmd+B and the other font shortcuts, while the notes field has the keyboard.
    fn notes_format_keys(&mut self, ctx: &egui::Context, events: &[egui::Event]) {
        for event in events {
            let egui::Event::Key { key, pressed: true, modifiers, .. } = event else { continue };
            if !notes_format_shortcut(*key, *modifiers) {
                continue;
            }
            ctx.input_mut(|i| i.consume_key(*modifiers, *key));
            crate::notes::commit_draft(self);
            let mods = Mods { shift: modifiers.shift, alt: modifiers.alt, cmd: modifiers.command || modifiers.ctrl };
            if let Err(e) = self.session.key(key_name(*key), mods) {
                self.set_status(e.to_string());
            }
        }
    }
}

/// Font shortcuts the notes field should not swallow: the same bindings as slide text.
fn notes_format_shortcut(key: egui::Key, modifiers: egui::Modifiers) -> bool {
    if !(modifiers.command || modifiers.ctrl) || modifiers.alt {
        return false;
    }
    match key {
        egui::Key::B | egui::Key::I | egui::Key::U if !modifiers.shift => true,
        egui::Key::X if modifiers.shift => true,
        egui::Key::Period | egui::Key::Comma if modifiers.shift => true,
        egui::Key::Equals => true,
        egui::Key::Space if !modifiers.shift => true,
        _ => false,
    }
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
