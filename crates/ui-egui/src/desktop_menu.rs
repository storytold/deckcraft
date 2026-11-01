//! Portable native-menu snapshot. The desktop host owns D-Bus/Wayland; actions still use the
//! existing interactive application routes, including Save As and unsaved-close confirmation.

use craft_desktop::{Action, Item, Menu};
use serde_json::{Value, json};

use crate::{SlideApp, ViewMode, menus, ribbon};

pub(crate) fn blocked(app: &SlideApp) -> bool {
    app.dialog.is_some() || app.opening.is_some() || app.palette.is_some() || app.eyedropper.is_some() || app.show.is_some() || app.quit_confirmed
}

fn document(app: &SlideApp) -> u64 {
    app.session.active().map_or(0, |d| d.uid)
}

fn checked(app: &SlideApp, format: &Value, id: &str) -> Option<bool> {
    match id {
        "format.bold" => format.get("bold").and_then(Value::as_bool),
        "format.italic" => format.get("italic").and_then(Value::as_bool),
        "format.underline" => format.get("underline").and_then(Value::as_bool),
        "view.normal" => Some(app.ui.view == ViewMode::Normal),
        "view.sorter" => Some(app.ui.view == ViewMode::Sorter),
        "view.outline" => Some(app.ui.view == ViewMode::Outline),
        "view.notesPage" => Some(app.ui.view == ViewMode::NotesPage),
        "view.reading" => Some(app.ui.view == ViewMode::Reading),
        "view.ruler" => Some(app.ui.ruler),
        "view.gridlines" => Some(app.ui.gridlines),
        "view.guides" => Some(app.ui.guides),
        "view.collapseRibbon" => Some(app.ui.ribbon_collapsed),
        _ => None,
    }
}

fn entry(app: &SlideApp, format: &Value, label: &str, id: &str) -> Option<Item> {
    if id.is_empty() {
        return None;
    }
    let spec = deckcraft_engine::find_command(id);
    if !ribbon::known(id) {
        return None;
    }
    let shortcut = spec.and_then(|c| c.shortcut).or_else(|| crate::UI_COMMANDS.iter().find(|c| c.0 == id).and_then(|c| c.2));
    let no_document_needed = matches!(id, "file.new" | "app.openDialog" | "app.about" | "app.palette" | "app.preferences");
    let enabled = !blocked(app) && ribbon::enabled(app, id) && (no_document_needed || app.session.active().is_some());
    let action = match id {
        "insert.table" => Action { command: "app.dialog".into(), params: json!({"id": "table"}) },
        "show.setup" => Action { command: "app.dialog".into(), params: json!({"id": "setupShow"}) },
        "app.print" => Action { command: "app.backstage".into(), params: json!({"page": "print"}) },
        "file.properties" => Action { command: "app.backstage".into(), params: json!({"page": "info"}) },
        _ => Action { command: id.into(), params: json!({}) },
    };
    Some(Item {
        key: id.into(),
        label: label.into(),
        enabled,
        checked: checked(app, format, id),
        shortcut: shortcut.unwrap_or_default().replace("Cmd", "Ctrl"),
        action: Some(action),
        ..Default::default()
    })
}

pub fn snapshot(app: &mut SlideApp) -> Menu {
    let format = ribbon::fmt_state(app);
    let mut items: Vec<Item> = menus::menu_tree()
        .into_iter()
        .map(|(label, entries)| Item {
            key: label.into(),
            label: label.into(),
            enabled: true,
            children: entries
                .into_iter()
                .enumerate()
                .filter_map(|(index, (label, id))| {
                    if id.is_empty() {
                        Some(Item { key: format!("separator-{index}"), separator: true, ..Default::default() })
                    } else {
                        entry(app, &format, label, id)
                    }
                })
                .collect(),
            ..Default::default()
        })
        .collect();
    if let Some(view) = items.iter_mut().find(|i| i.key == "View") {
        view.children.push(Item {
            key: "appearance".into(),
            label: "Appearance".into(),
            enabled: !blocked(app),
            children: [("system", "System"), ("light", "Light"), ("dark", "Dark")]
                .into_iter()
                .map(|(mode, label)| Item {
                    key: mode.into(),
                    label: label.into(),
                    enabled: !blocked(app),
                    checked: Some(if mode == "system" {
                        app.ui.system_theme
                    } else {
                        !app.ui.system_theme && (app.ui.brightness == crate::theme::Brightness::Dark) == (mode == "dark")
                    }),
                    action: Some(Action { command: "view.theme".into(), params: json!({"mode": mode}) }),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        });
    }
    Menu { document: document(app), items }
}

fn available(items: &[Item], action: &Action) -> bool {
    items.iter().any(|item| item.enabled && (item.action.as_ref() == Some(action) || available(&item.children, action)))
}

/// Recheck every event: a queued click must not affect a replacement document or pass a modal.
pub fn activate(app: &mut SlideApp, ctx: &egui::Context, doc: u64, action: &Action) {
    if doc != document(app) || blocked(app) || !available(&snapshot(app).items, action) {
        return;
    }
    match action.command.as_str() {
        "file.save" => app.save(),
        "edit.paste" => app.request_desktop_clipboard(ctx, crate::DesktopClipboard::Paste),
        "edit.copy" | "edit.cut" if ctx.egui_wants_keyboard_input() => {
            app.request_desktop_clipboard(
                ctx,
                if action.command == "edit.copy" { crate::DesktopClipboard::Copy } else { crate::DesktopClipboard::Cut },
            );
        }
        // Reuse the canvas's rich HTML/object publisher immediately, before a later queued
        // menu action can change documents. Text editors retain their toolkit event route.
        "edit.copy" | "edit.cut" => app.publish_clipboard(ctx, action.command == "edit.cut"),
        "edit.selectAll" if app.slides_list_focused() => {
            let _ = app.run("edit.selectAll", json!({"scope": "slides"}));
        }
        _ => {
            let _ = app.run(&action.command, action.params.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    fn app() -> SlideApp {
        SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default())
    }
    fn find<'a>(items: &'a [Item], command: &str) -> Option<&'a Item> {
        items.iter().find_map(|i| if i.key == command { Some(i) } else { find(&i.children, command) })
    }
    #[test]
    fn desktop_menu_state_changes_and_stale_clicks_are_rejected() {
        let mut app = app();
        let before = snapshot(&mut app);
        let ruler = find(&before.items, "view.ruler").unwrap().action.clone().unwrap();
        activate(&mut app, &egui::Context::default(), before.document, &ruler);
        assert!(app.ui.ruler);
        assert_eq!(find(&snapshot(&mut app).items, "view.ruler").unwrap().checked, Some(true));
        app.run("file.new", json!({})).unwrap();
        activate(&mut app, &egui::Context::default(), before.document, &ruler);
        assert!(app.ui.ruler, "retired document cannot toggle it again");
        app.dialog = Some(crate::dialogs::Dialog::new("about"));
        let now = snapshot(&mut app);
        assert!(!find(&now.items, "view.ruler").unwrap().enabled);
    }
    #[test]
    fn desktop_menu_table_opens_dialog_and_close_preserves_dirty_document() {
        let mut app = app();
        let before = snapshot(&mut app);
        let table = find(&before.items, "insert.table").unwrap().action.clone().unwrap();
        activate(&mut app, &egui::Context::default(), before.document, &table);
        assert_eq!(app.dialog.as_ref().map(|d| d.id.as_str()), Some("table"));
        app.dialog = None;
        app.run("slide.new", json!({})).unwrap();
        let before = snapshot(&mut app);
        let close = find(&before.items, "app.closePresentation").unwrap().action.clone().unwrap();
        activate(&mut app, &egui::Context::default(), before.document, &close);
        assert!(app.session.active().is_some());
        assert!(app.dialog.is_some(), "unsaved changes must prompt");
    }

    #[test]
    fn desktop_menu_keeps_every_existing_catalogue_command() {
        let mut app = app();
        let menu = snapshot(&mut app);
        for (_, entries) in menus::menu_tree() {
            for (label, command) in entries.into_iter().filter(|(_, command)| !command.is_empty()) {
                assert!(find(&menu.items, command).is_some(), "catalogue entry {label}: {command}");
            }
        }
    }

    #[test]
    fn desktop_menu_print_and_show_setup_open_interactive_settings() {
        let mut app = app();
        let menu = snapshot(&mut app);
        let print = find(&menu.items, "app.print").unwrap().action.clone().unwrap();
        activate(&mut app, &egui::Context::default(), menu.document, &print);
        assert_eq!(app.backstage.as_ref().map(|b| b.page), Some(crate::backstage::Page::Print));
        let setup = find(&menu.items, "show.setup").unwrap().action.clone().unwrap();
        activate(&mut app, &egui::Context::default(), menu.document, &setup);
        assert_eq!(app.dialog.as_ref().map(|d| d.id.as_str()), Some("setupShow"));
    }

    #[test]
    fn desktop_menu_native_image_delivery_is_deferred_and_never_duplicates_text() {
        let session = deckcraft_engine::Session::with_new();
        let png = deckcraft_engine::cmd::file::render_png(&session.active().unwrap().doc, 0, 0.1, false).0;
        let services = crate::Services { clipboard_image: Some(Box::new(move || Some(png.clone()))), ..Default::default() };
        let mut h = egui_kittest::Harness::builder().build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(session, services),
        );
        h.run_steps(2);
        let pictures = |app: &SlideApp| {
            app.session.active().unwrap().doc.slides[0].shapes.iter().filter(|s| matches!(s.kind, deckcraft_model::ShapeKind::Picture { .. })).count()
        };
        let paste = Action { command: "edit.paste".into(), params: json!({}) };
        let ctx = h.ctx.clone();
        let doc = document(h.state());
        activate(h.state_mut(), &ctx, doc, &paste);
        h.event(egui::Event::Paste("normal text".into()));
        h.run_steps(3);
        assert_eq!(pictures(h.state()), 0, "normal text Paste suppresses the image fallback");
        activate(h.state_mut(), &ctx, doc, &paste);
        assert_eq!(pictures(h.state()), 0, "do not guess the host's asynchronous result");
        h.run_steps(8);
        assert_eq!(pictures(h.state()), 0, "no synchronous fallback while native transfer is pending");
        h.event(egui::Event::PasteImage(std::sync::Arc::new(egui::ColorImage::new([1, 1], vec![egui::Color32::RED]))));
        h.run_steps(3);
        assert_eq!(pictures(h.state()), 1, "native image delivery must suppress the services fallback");
        h.run_steps(3);
        assert_eq!(pictures(h.state()), 1, "native image delivery is one-shot");
        activate(h.state_mut(), &ctx, doc, &paste);
        h.state_mut().run("file.new", json!({})).unwrap();
        h.run_steps(8);
        h.event(egui::Event::PasteImage(std::sync::Arc::new(egui::ColorImage::new([1, 1], vec![egui::Color32::RED]))));
        h.run_steps(3);
        assert_eq!(pictures(h.state()), 0, "a queued image cannot enter the replacement document");

        let doc = document(h.state());
        activate(h.state_mut(), &ctx, doc, &paste);
        h.state_mut().run("file.new", json!({})).unwrap();
        h.event(egui::Event::Paste("stale native paste".into()));
        h.run_steps(3);
        let text = |app: &SlideApp| {
            app.session.active().unwrap().doc.slides[0]
                .shapes
                .iter()
                .filter_map(|shape| shape.text.as_ref())
                .map(|body| body.text())
                .collect::<String>()
        };
        assert!(!text(h.state()).contains("stale native paste"), "the deferred text event also belongs to the old document");
        h.event(egui::Event::Paste("ordinary keyboard paste".into()));
        h.run_steps(3);
        assert!(text(h.state()).contains("ordinary keyboard paste"), "only a pending native request is guarded");
    }

    #[test]
    fn desktop_menu_text_editor_copy_cut_are_guarded_until_the_event_arrives() {
        for (command, event) in [("edit.copy", egui::Event::Copy), ("edit.cut", egui::Event::Cut)] {
            let mut h = egui_kittest::Harness::builder().build_ui_state(
                |ui, (app, received): &mut (SlideApp, Vec<egui::Event>)| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                    // Observe the input where a TextEdit would consume it, after app logic.
                    received.extend(
                        ui.input(|i| i.events.iter().filter(|e| matches!(e, egui::Event::Copy | egui::Event::Cut)).cloned().collect::<Vec<_>>()),
                    );
                },
                (app(), Vec::new()),
            );
            h.run_steps(2);
            let ctx = h.ctx.clone();
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::unique("clipboard_text_editor")));
            let doc = document(&h.state().0);
            activate(&mut h.state_mut().0, &ctx, doc, &Action { command: command.into(), params: json!({}) });
            assert_eq!(h.state().0.desktop_clipboard.len(), 1, "focused text input must use the guarded deferred path");
            h.state_mut().0.run("file.new", json!({})).unwrap();
            h.event(event.clone());
            h.run_steps(3);
            assert!(h.state().1.is_empty(), "{command} must not reach a replacement document's text input");
            h.event(event);
            h.run_steps(2);
            assert_eq!(h.state().1.len(), 1, "ordinary keyboard {command} remains available");
        }
    }
}
