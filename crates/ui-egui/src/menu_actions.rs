//! The menu bar, built with craft-menu: the native macOS menus are another frontend to the
//! existing commands. The menus come from [`menus::menu_tree`] with the shortcuts the ribbon and
//! palette show; enabled states, view checkmarks and the Undo/Redo labels follow the app; a click
//! runs its command as the palette does. No platform menu types live here, so all of it is
//! exercised in the egui harness. The desktop app installs the native menus (`craft_menu::mac`)
//! and drives them each frame.

use craft_menu::{Chord, CommandRef, Gate, Item, ItemRole, ItemState, Menu, MenuBar, MenuRole, Node};
use egui::{Context, Event, Key, Modifiers, ViewportCommand, ViewportId};
use serde_json::json;

use crate::{SlideApp, UI_COMMANDS, ViewMode, menus};

/// On/off view commands, shown as check items.
const CHECKS: &[&str] = &["view.ruler", "view.gridlines", "view.guides"];
/// The views, shown as one group of choices.
const VIEWS: &[&str] = &["view.normal", "view.sorter", "view.outline", "view.notesPage", "view.reading"];

/// The shortcut a command shows: the one the ribbon and palette show, which the app's key
/// handling runs (the native menu hands its key equivalents back to egui).
pub fn shortcut(id: &str) -> Option<&'static str> {
    UI_COMMANDS.iter().find(|c| c.0 == id).and_then(|c| c.2).or_else(|| deckcraft_engine::find_command(id).and_then(|c| c.shortcut))
}

/// Edit commands a focused text field handles itself.
fn edit_input(id: &str) -> bool {
    matches!(id, "edit.cut" | "edit.copy" | "edit.paste" | "edit.pasteText" | "edit.undo" | "edit.redo" | "edit.selectAll")
}

impl SlideApp {
    /// The menu bar for the window `target`, with each item's state (see [`SlideApp::menu_state`]).
    /// The application menu holds Settings and Quit; About is tagged where it is (Help), and the
    /// macOS layout moves all three to the application menu.
    pub fn menu_bar(&self, ctx: &Context, target: ViewportId) -> MenuBar {
        let item = |label: &str, id: &str| -> Node {
            let mut it = Item::new(id, self.menu_label(ctx, id, label));
            if let Some(chord) = shortcut(id).and_then(|s| Chord::parse(s).ok()) {
                it = it.shortcut(chord);
            }
            if CHECKS.contains(&id) {
                it = it.check(false);
            } else if VIEWS.contains(&id) {
                it = it.radio(false);
            }
            it.into()
        };
        let mut menus: Vec<Menu> =
            vec![Menu::new("DeckCraft", vec![item("Settings…", "app.preferences"), item("Quit DeckCraft", "app.quit")]).role(MenuRole::App)];
        for (title, rows) in menus::menu_tree() {
            let children = rows.into_iter().map(|(label, id)| if label == "-" { Node::Separator } else { item(label, id) }).collect();
            let role = match title {
                "File" => MenuRole::File,
                "Edit" => MenuRole::Edit,
                "View" => MenuRole::View,
                "Window" => MenuRole::Window,
                "Help" => MenuRole::Help,
                _ => MenuRole::Other,
            };
            menus.push(Menu::new(title, craft_menu::tidy(children)).role(role));
        }
        let mut bar = MenuBar { menus, documents: Vec::new() };
        bar.tag_item(&CommandRef::new("app.about"), ItemRole::About);
        bar.tag_item(&CommandRef::new("app.preferences"), ItemRole::Settings);
        bar.tag_item(&CommandRef::new("app.quit"), ItemRole::Quit);
        craft_menu::apply_state(&mut bar, |command| self.menu_state(ctx, &command.id, target));
        bar
    }

    /// What may run from the menus now: while a dialog, the palette or an opening presentation
    /// holds the window, the menus are greyed (a text field's own Undo, Copy… stay) and their
    /// clicks dropped.
    pub fn menu_gate(&self, ctx: &Context) -> Gate {
        Gate {
            modal: self.dialog.is_some() || self.palette.is_some() || self.opening.is_some(),
            recording_shortcut: false,
            text_editing: ctx.text_edit_focused(),
        }
    }

    /// Whether command `id` can run from the menus of window `target`. Rechecked when a click is
    /// run: a queued menu event must not edit behind a dialog that opened in the meantime.
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
        if matches!(
            id,
            "app.closePresentation" | "app.openDialog" | "file.new" | "app.about" | "app.preferences" | "app.palette" | "app.checkUpdates"
        ) {
            return true;
        }
        if self.session.active().is_none() {
            return false;
        }
        // Save asks for a path for a new presentation; Paste can take the system clipboard.
        if matches!(id, "file.save" | "edit.paste") {
            return true;
        }
        if let Some(c) = deckcraft_engine::find_command(id) {
            return (c.enabled)(&self.session).is_ok() && (id == "insert.table" || !deckcraft_engine::cmd::needs_arguments(c.params));
        }
        UI_COMMANDS.iter().any(|c| c.0 == id)
    }

    /// An item's state: whether it can run, and the checkmark of a view setting.
    pub fn menu_state(&self, ctx: &Context, id: &str, target: ViewportId) -> ItemState {
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
        ItemState { enabled: self.menu_enabled(ctx, id, target), checked }
    }

    /// An item's label: Undo and Redo name the edit they would undo or redo ("Undo Typing"),
    /// except while a text field has the keys (its own undo runs).
    fn menu_label(&self, ctx: &Context, id: &str, label: &str) -> String {
        let history = self.session.active().and_then(|d| match id {
            "edit.undo" => d.history.undo.last(),
            "edit.redo" => d.history.redo.last(),
            _ => None,
        });
        match history {
            Some(entry) if !ctx.text_edit_focused() => format!("{label} {}", entry.label),
            _ => label.to_string(),
        }
    }

    /// Run a menu click in window `target`, after this frame's UI, so a field's edit from this
    /// frame is committed before Save or Close. Copy, Cut and Paste go through egui as their keys
    /// do, so a focused text field or the slide (with its rich-text and picture clipboard)
    /// handles them. Returns key presses for the host to feed egui on the next pass: a focused
    /// text field's own Undo, Redo and Select All.
    pub fn run_menu_command(&mut self, ctx: &Context, command: &CommandRef, target: ViewportId) -> Vec<Event> {
        let id = command.id.as_str();
        if !self.menu_enabled(ctx, id, target) {
            return Vec::new();
        }
        let tap = |key: Key, modifiers: Modifiers| {
            [true, false].map(|pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }).to_vec()
        };
        let mut keys = Vec::new();
        match id {
            "edit.copy" => ctx.send_viewport_cmd_to(target, ViewportCommand::RequestCopy),
            "edit.cut" => ctx.send_viewport_cmd_to(target, ViewportCommand::RequestCut),
            "edit.paste" => ctx.send_viewport_cmd_to(target, ViewportCommand::RequestPaste),
            "edit.pasteText" if ctx.text_edit_focused() => ctx.send_viewport_cmd_to(target, ViewportCommand::RequestPaste),
            "edit.undo" if ctx.text_edit_focused() => keys = tap(Key::Z, Modifiers::COMMAND),
            "edit.redo" if ctx.text_edit_focused() => keys = tap(Key::Z, Modifiers { shift: true, ..Modifiers::COMMAND }),
            "edit.selectAll" if ctx.text_edit_focused() => keys = tap(Key::A, Modifiers::COMMAND),
            "app.quit" => ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close),
            // A secondary window closes on its own; so does the window of the empty start screen.
            "app.closePresentation" if target != ViewportId::ROOT || self.session.active().is_none() => {
                ctx.send_viewport_cmd_to(target, ViewportCommand::Close);
            }
            "app.closePresentation" => {
                self.show = None;
                self.close_presentation();
            }
            // Save asks where to save a new presentation, as ⌘S does.
            "file.save" => self.save(),
            "file.properties" => self.open_backstage(crate::backstage::Page::Info),
            // Print… opens File ▸ Print, as ⌘P does.
            "app.print" => self.open_backstage(crate::backstage::Page::Print),
            "insert.table" => self.dialog = Some(crate::dialogs::Dialog::new("table")),
            "edit.selectAll" if self.slides_list_focused() => {
                let _ = self.run(id, json!({"scope": "slides"}));
            }
            _ => {
                let _ = self.run(id, json!({}));
            }
        }
        ctx.request_repaint();
        keys
    }
}

/// `bar` as the native macOS menus show it. Two shortcuts run their items from the menu rather
/// than through the app's keys (`layout_shortcut`): ⌘W, so that it closes the window in front
/// (a secondary window on its own, the empty start screen's window, else the presentation with
/// its save prompt), and Replace, which takes ⇧⌘H as in PowerPoint for Mac so that ⌘H stays the
/// system's Hide.
pub fn for_mac(mut bar: MenuBar) -> MenuBar {
    fn walk(nodes: &mut [Node]) {
        for n in nodes {
            match n {
                Node::Item(it) if it.command.id == "app.closePresentation" => it.layout_shortcut = it.shortcut.is_some(),
                Node::Item(it) if it.command.id == "app.replace" => {
                    it.shortcut = Chord::parse("Cmd+Shift+H").ok();
                    it.layout_shortcut = true;
                }
                Node::Submenu { children, .. } => walk(children),
                _ => {}
            }
        }
    }
    for m in &mut bar.menus {
        walk(&mut m.children);
    }
    bar
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Services, dialogs::Dialog};
    use craft_menu::{ItemKind, LayoutOptions};
    use deckcraft_engine::Session;
    use egui::RawInput;
    use egui_kittest::{Harness, kittest::Queryable};

    fn app() -> SlideApp {
        SlideApp::new(Session::with_new(), Services::default())
    }

    fn run(app: &mut SlideApp, ctx: &Context, id: &str, target: ViewportId) -> egui::FullOutput {
        ctx.set_embed_viewports(false);
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            if target != ViewportId::ROOT {
                ui.ctx().show_viewport_deferred(target, egui::ViewportBuilder::default(), |_, _| {});
            }
            app.run_menu_command(ui.ctx(), &CommandRef::new(id), target);
        });
        out.textures_delta.clear();
        out
    }

    fn item<'a>(bar: &'a MenuBar, id: &str) -> &'a Item {
        bar.find(&CommandRef::new(id)).unwrap_or_else(|| panic!("no {id} item"))
    }

    #[test]
    fn every_shortcut_parses_and_has_one_owner_and_the_mac_keeps_its_window_keys() {
        let ctx = Context::default();
        let bar = for_mac(app().menu_bar(&ctx, ViewportId::ROOT));
        let mut owners = std::collections::HashMap::new();
        for it in bar.items() {
            if let Some(s) = shortcut(it.command.id.as_str()).filter(|_| it.command.id != "app.replace") {
                assert_eq!(it.shortcut, Chord::parse(s).ok(), "{}: {s} doesn't parse", it.command.id);
            }
            if let Some(c) = it.shortcut {
                assert!(owners.insert(c, it.command.id.clone()).is_none(), "{c} has two items");
            }
        }
        // ⌘H hides and ⌘M minimizes; Replace takes ⇧⌘H and runs from the menu.
        let layout = craft_menu::mac_layout(&bar, &LayoutOptions::new("DeckCraft"));
        let replace = item(&layout.bar, "app.replace");
        assert_eq!((replace.shortcut, replace.layout_shortcut), (Chord::parse("Cmd+Shift+H").ok(), true));
        assert!(layout.clashes.iter().all(|c| !matches!(c.system, "Hide" | "Minimize")), "{:?}", layout.clashes);
        // ⌘W is Close, run from the menu, so it closes the window in front.
        let close = item(&layout.bar, "app.closePresentation");
        assert_eq!((close.shortcut, close.layout_shortcut), (Chord::parse("Cmd+W").ok(), true));
        // About, Settings and Quit are in the application menu.
        let app_menu = &layout.bar.menus[0];
        for id in ["app.about", "app.preferences", "app.quit"] {
            assert!(app_menu.children.iter().any(|n| matches!(n, Node::Item(it) if it.command.id == id)), "{id} not in the {} menu", app_menu.title);
        }
    }

    #[test]
    fn close_empty_window_and_secondary_window_target_the_right_viewport() {
        let ctx = Context::default();
        let mut app = SlideApp::new(Session::new(), Services::default());
        let out = run(&mut app, &ctx, "app.closePresentation", ViewportId::ROOT);
        assert!(out.viewport_output[&ViewportId::ROOT].commands.contains(&ViewportCommand::Close));

        let mut app = self::app();
        app.run("slide.new", json!({})).unwrap();
        let secondary = ViewportId::from_hash_of("deckcraft_doc_window");
        let out = run(&mut app, &ctx, "app.closePresentation", secondary);
        assert!(out.viewport_output[&secondary].commands.contains(&ViewportCommand::Close));
        assert!(!out.viewport_output[&ViewportId::ROOT].commands.contains(&ViewportCommand::Close));
        assert_eq!(app.session.documents().len(), 1);
        assert!(app.dialog.is_none(), "closing the preview must not close its document");
        // In a secondary window only Close (and Quit) work.
        assert!(
            !app.menu_bar(&ctx, secondary)
                .items()
                .iter()
                .any(|it| it.enabled && !matches!(it.command.id.as_str(), "app.closePresentation" | "app.quit"))
        );
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
        h.state_mut().run_menu_command(&ctx, &CommandRef::new("app.closePresentation"), ViewportId::ROOT);
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("close"));
        assert!(!h.state().menu_enabled(&ctx, "app.closePresentation", ViewportId::ROOT));
        assert!(h.state().menu_gate(&ctx).modal, "the prompt greys the menus");
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
        ctx.memory_mut(|m| m.request_focus(egui::Id::unique("a button")));
        run(&mut app, &ctx, "file.save", ViewportId::ROOT);
        assert!(asked.get(), "Save must use the UI path, not file.save without a filename");
        assert_eq!(app.session.documents().len(), 1);
    }

    #[test]
    fn menu_edits_go_to_the_focused_text_field_and_leave_document_history_alone() {
        let mut app = app();
        app.run("slide.new", json!({})).unwrap();
        let history = app.session.active().unwrap().history.undo.len();
        // The field, and a menu click run after it in the frame, as the host runs clicks.
        type State = (SlideApp, String, Option<&'static str>, Vec<Event>);
        let mut h = Harness::builder().build_ui_state(
            |ui, state: &mut State| {
                let label = ui.label("Field");
                ui.text_edit_singleline(&mut state.1).labelled_by(label.id).request_focus();
                if let Some(id) = state.2.take() {
                    state.3 = state.0.run_menu_command(ui.ctx(), &CommandRef::new(id), ViewportId::ROOT);
                }
            },
            (app, "alpha".to_string(), None, Vec::new()),
        );
        h.run_steps(2);
        let click = |h: &mut Harness<'_, State>, id: &'static str| {
            h.state_mut().2 = Some(id);
            h.run_steps(1);
            // What the host does next: feed egui the key presses the click returned and the
            // clipboard events it asked for (egui-winit's part), on the next pass.
            let mut events = std::mem::take(&mut h.state_mut().3);
            for c in h.output().viewport_output.get(&ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default() {
                match c {
                    ViewportCommand::RequestCopy => events.push(Event::Copy),
                    ViewportCommand::RequestCut => events.push(Event::Cut),
                    ViewportCommand::RequestPaste => events.push(Event::Paste("beta".into())),
                    _ => {}
                }
            }
            h.input_mut().events.extend(events);
            h.run_steps(1);
        };
        click(&mut h, "edit.selectAll");
        click(&mut h, "edit.copy");
        assert!(h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "alpha")));
        click(&mut h, "edit.paste");
        assert_eq!(h.state().1, "beta");
        click(&mut h, "edit.undo");
        assert_eq!(h.state().1, "alpha");
        click(&mut h, "edit.redo");
        assert_eq!(h.state().1, "beta");
        assert_eq!(h.state().0.session.active().unwrap().history.undo.len(), history);
        // While the field has the keys, Undo is its own: no edit named.
        let ctx = h.ctx.clone();
        assert_eq!(item(&h.state().0.menu_bar(&ctx, ViewportId::ROOT), "edit.undo").label, "Undo");
    }

    #[test]
    fn menu_state_tracks_history_and_checks_and_revalidates_modal_actions() {
        let mut app = app();
        let ctx = Context::default();
        let undo = |app: &SlideApp| item(&app.menu_bar(&ctx, ViewportId::ROOT), "edit.undo").clone();
        assert!(!undo(&app).enabled);
        app.run("slide.new", json!({})).unwrap();
        let u = undo(&app);
        assert!(u.enabled && u.label.starts_with("Undo "), "{u:?}");
        run(&mut app, &ctx, "view.gridlines", ViewportId::ROOT);
        let bar = app.menu_bar(&ctx, ViewportId::ROOT);
        assert_eq!(item(&bar, "view.gridlines").kind, ItemKind::Check(true));
        assert_eq!(item(&bar, "view.normal").kind, ItemKind::Radio(true));
        assert_eq!(item(&bar, "view.sorter").kind, ItemKind::Radio(false));
        let n = app.session.active().unwrap().doc.slides.len();
        app.dialog = Some(Dialog::new("preferences"));
        run(&mut app, &ctx, "slide.new", ViewportId::ROOT);
        assert_eq!(app.session.active().unwrap().doc.slides.len(), n);
        assert_eq!(app.dialog.as_ref().unwrap().id, "preferences");
    }
}
