//! Native menus are another frontend to the existing commands. No platform menu types live
//! here, so focus, enablement and close/save behaviour can be exercised in the egui harness.

use egui::{Context, Event, Key, Modifiers, RawInput, ViewportCommand, ViewportId};
use serde_json::json;

use crate::{SlideApp, UI_COMMANDS, ViewMode};

/// A menu item's changing presentation. The desktop updates only values that changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub label: String,
    pub enabled: bool,
    pub checked: Option<bool>,
}

/// The same shortcuts as the ribbon/palette, with the macOS Hide key reserved for the OS.
pub fn shortcut(id: &str) -> Option<&'static str> {
    match id {
        "app.quit" => Some("Cmd+Q"),
        "edit.redo" => Some("Cmd+Shift+Z"),
        "app.replace" => Some("Cmd+Shift+H"),
        _ => UI_COMMANDS.iter().find(|c| c.0 == id).and_then(|c| c.2).or_else(|| deckcraft_engine::find_command(id).and_then(|c| c.shortcut)),
    }
}

fn edit_input(id: &str) -> bool {
    matches!(id, "edit.cut" | "edit.copy" | "edit.paste" | "edit.pasteText" | "edit.undo" | "edit.redo" | "edit.selectAll")
}

impl SlideApp {
    /// Rechecked on activation too: a queued menu event must not edit behind a new modal.
    pub fn menu_enabled(&self, ctx: &Context, id: &str, target: ViewportId) -> bool {
        if id == "app.quit" {
            return self.dialog.is_none();
        }
        if target != ViewportId::ROOT {
            return id == "app.closePresentation";
        }
        if ctx.text_edit_focused() && edit_input(id) {
            return true;
        }
        if self.dialog.is_some() || self.palette.is_some() || self.opening.is_some() {
            return false;
        }
        if self.show.is_some() {
            return id == "app.closePresentation";
        }
        if matches!(id, "app.closePresentation" | "app.openDialog" | "file.new" | "app.about" | "app.preferences" | "app.palette") {
            return true;
        }
        if self.session.active().is_none() {
            return false;
        }
        // Save asks for a path for new presentations; Paste can use the system clipboard.
        if matches!(id, "file.save" | "edit.paste") {
            return true;
        }
        if let Some(c) = deckcraft_engine::find_command(id) {
            return (c.enabled)(&self.session).is_ok() && (id == "insert.table" || !deckcraft_engine::cmd::needs_arguments(c.params));
        }
        UI_COMMANDS.iter().any(|c| c.0 == id)
    }

    pub fn menu_state(&self, ctx: &Context, id: &str, label: &str, target: ViewportId) -> State {
        let checked = match id {
            "view.ruler" => Some(self.ui.ruler),
            "view.gridlines" => Some(self.ui.gridlines),
            "view.guides" => Some(self.ui.guides),
            "view.normal" => Some(self.ui.view == ViewMode::Normal),
            "view.sorter" => Some(self.ui.view == ViewMode::Sorter),
            "view.outline" => Some(self.ui.view == ViewMode::Outline),
            "view.notesPage" => Some(self.ui.view == ViewMode::NotesPage),
            "view.reading" => Some(self.ui.view == ViewMode::Reading),
            _ => None,
        };
        let history = self.session.active().and_then(|d| match id {
            "edit.undo" => d.history.undo.last(),
            "edit.redo" => d.history.redo.last(),
            _ => None,
        });
        let label = if !ctx.text_edit_focused()
            && let Some(entry) = history
        {
            format!("{label} {}", entry.label)
        } else {
            label.to_string()
        };
        State { label, enabled: self.menu_enabled(ctx, id, target), checked }
    }

    /// Feed edit actions through egui before layout, preserving text-field undo/selection and
    /// the document's rich-text/image clipboard path. `paste` is read by the desktop host only.
    pub fn menu_input(&mut self, ctx: &Context, raw: &mut RawInput, id: &str, target: ViewportId, paste: impl FnOnce() -> String) -> bool {
        if !self.menu_enabled(ctx, id, target) {
            return true;
        }
        let event = match id {
            "edit.copy" => Event::Copy,
            "edit.cut" => Event::Cut,
            "edit.paste" => Event::Paste(paste()),
            "edit.pasteText" if ctx.text_edit_focused() => Event::Paste(paste()),
            "edit.pasteText" => {
                let _ = self.run(id, json!({"text": paste()}));
                return true;
            }
            "edit.undo" | "edit.redo" | "edit.selectAll" if ctx.text_edit_focused() => {
                let key = if id == "edit.selectAll" { Key::A } else { Key::Z };
                let modifiers = Modifiers { shift: id == "edit.redo", ..Modifiers::COMMAND };
                raw.events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
                Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }
            }
            _ => return false,
        };
        raw.events.push(event);
        true
    }

    /// Run after layout so field edits from this frame are committed before Save or Close.
    pub fn menu_action(&mut self, ctx: &Context, id: &str, target: ViewportId) {
        if !self.menu_enabled(ctx, id, target) {
            return;
        }
        match id {
            "app.quit" => ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close),
            "app.closePresentation" if target != ViewportId::ROOT || self.session.active().is_none() => {
                ctx.send_viewport_cmd_to(target, ViewportCommand::Close);
            }
            "app.closePresentation" => {
                self.show = None;
                self.close_presentation();
            }
            "file.save" => self.save(),
            "file.properties" => self.open_backstage(crate::backstage::Page::Info),
            "insert.table" => self.dialog = Some(crate::dialogs::Dialog::new("table")),
            "edit.selectAll" if self.slides_list_focused() => {
                let _ = self.run(id, json!({"scope": "slides"}));
            }
            _ => {
                let _ = self.run(id, json!({}));
            }
        }
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Services, dialogs::Dialog};
    use deckcraft_engine::Session;
    use egui_kittest::{Harness, kittest::Queryable};

    fn app() -> SlideApp {
        SlideApp::new(Session::with_new(), Services::default())
    }

    fn activate(app: &mut SlideApp, ctx: &Context, id: &str, target: ViewportId) -> egui::FullOutput {
        ctx.set_embed_viewports(false);
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            if target != ViewportId::ROOT {
                ui.ctx().show_viewport_deferred(target, egui::ViewportBuilder::default(), |_, _| {});
            }
            app.menu_action(ui.ctx(), id, target);
        });
        out.textures_delta.clear();
        out
    }

    #[test]
    fn close_empty_window_and_secondary_window_target_the_right_viewport() {
        let ctx = Context::default();
        let mut app = SlideApp::new(Session::new(), Services::default());
        let out = activate(&mut app, &ctx, "app.closePresentation", ViewportId::ROOT);
        assert!(out.viewport_output[&ViewportId::ROOT].commands.contains(&ViewportCommand::Close));

        let mut app = self::app();
        app.run("slide.new", json!({})).unwrap();
        let secondary = ViewportId::from_hash_of("deckcraft_doc_window");
        let out = activate(&mut app, &ctx, "app.closePresentation", secondary);
        assert!(out.viewport_output[&secondary].commands.contains(&ViewportCommand::Close));
        assert!(!out.viewport_output[&ViewportId::ROOT].commands.contains(&ViewportCommand::Close));
        assert_eq!(app.session.documents().len(), 1);
        assert!(app.dialog.is_none(), "closing the preview must not close its document");
    }

    #[test]
    fn close_dirty_document_asks_once_and_cancel_keeps_it() {
        let mut app = app();
        app.run("slide.new", json!({})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(3);
        let ctx = h.ctx.clone();
        h.state_mut().menu_action(&ctx, "app.closePresentation", ViewportId::ROOT);
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("close"));
        assert!(!h.state().menu_enabled(&ctx, "app.closePresentation", ViewportId::ROOT));
        h.get_by_label("Cancel").click();
        h.run_steps(2);
        assert_eq!(h.state().session.documents().len(), 1);
        assert!(h.state().session.active().unwrap().is_dirty());
    }

    #[test]
    fn save_uses_the_picker_for_an_untitled_document_even_with_button_focus() {
        let asked = std::rc::Rc::new(std::cell::Cell::new(false));
        let record = asked.clone();
        let mut app = SlideApp::new(
            Session::with_new(),
            Services {
                pick_save: Some(Box::new(move |_| {
                    record.set(true);
                    None
                })),
                ..Services::default()
            },
        );
        let ctx = Context::default();
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("a button")));
        activate(&mut app, &ctx, "file.save", ViewportId::ROOT);
        assert!(asked.get(), "Save must use the UI path, not file.save without a filename");
        assert_eq!(app.session.documents().len(), 1);
    }

    #[test]
    fn menu_edits_go_to_the_focused_text_field_and_leave_document_history_alone() {
        let mut app = app();
        app.run("slide.new", json!({})).unwrap();
        let history = app.session.active().unwrap().history.undo.len();
        let mut h = Harness::builder().build_ui_state(
            |ui, state: &mut (SlideApp, String)| {
                ui.text_edit_singleline(&mut state.1).request_focus();
            },
            (app, "alpha".to_string()),
        );
        h.run_steps(2);
        let input = |h: &mut Harness<'_, (SlideApp, String)>, id: &str, paste: &str| {
            let ctx = h.ctx.clone();
            let mut raw = RawInput::default();
            assert!(h.state_mut().0.menu_input(&ctx, &mut raw, id, ViewportId::ROOT, || paste.into()));
            h.input_mut().events.extend(raw.events);
            h.run_steps(1);
        };
        input(&mut h, "edit.selectAll", "");
        input(&mut h, "edit.copy", "");
        assert!(h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "alpha")));
        input(&mut h, "edit.paste", "beta");
        assert_eq!(h.state().1, "beta");
        input(&mut h, "edit.undo", "");
        assert_eq!(h.state().1, "alpha");
        input(&mut h, "edit.redo", "");
        assert_eq!(h.state().1, "beta");
        assert_eq!(h.state().0.session.active().unwrap().history.undo.len(), history);
    }

    #[test]
    fn menu_state_tracks_history_and_checks_and_revalidates_modal_actions() {
        let mut app = app();
        let ctx = Context::default();
        assert!(!app.menu_state(&ctx, "edit.undo", "Undo", ViewportId::ROOT).enabled);
        app.run("slide.new", json!({})).unwrap();
        let undo = app.menu_state(&ctx, "edit.undo", "Undo", ViewportId::ROOT);
        assert!(undo.enabled && undo.label.starts_with("Undo "));
        activate(&mut app, &ctx, "view.gridlines", ViewportId::ROOT);
        assert_eq!(app.menu_state(&ctx, "view.gridlines", "Gridlines", ViewportId::ROOT).checked, Some(true));
        let n = app.session.active().unwrap().doc.slides.len();
        app.dialog = Some(Dialog::new("preferences"));
        activate(&mut app, &ctx, "slide.new", ViewportId::ROOT);
        assert_eq!(app.session.active().unwrap().doc.slides.len(), n);
        assert_eq!(app.dialog.as_ref().unwrap().id, "preferences");
    }
}
