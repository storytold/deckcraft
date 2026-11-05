//! macOS: the native menu bar (DeckCraft, File, Edit, View, Insert, Format, Arrange,
//! Slide Show, Window, Help) built directly from `deckcraft_ui_egui::menus::menu_tree()`.
//! Items dispatch to SlideApp commands, dialogs, or system actions.

use std::str::FromStr;
use std::sync::mpsc::{Receiver, channel};

use deckcraft_ui_egui::{SlideApp, ViewMode, menus::menu_tree};
use muda::accelerator::Accelerator;
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use serde_json::json;

pub struct NativeMenu {
    _menu: Menu,
    events: Receiver<MenuEvent>,
    view_items: Vec<(&'static str, CheckMenuItem)>,
}

fn item(id: &'static str, label: &'static str, acc: Option<&'static str>) -> MenuItem {
    let accel = acc.and_then(|a| Accelerator::from_str(a).ok());
    MenuItem::with_id(id, label, true, accel)
}

/// Builds the DeckCraft application menu (About, Preferences, Services, Hide, Quit).
fn build_app_menu() -> Submenu {
    let app_menu = Submenu::new("DeckCraft", true);
    let _ = app_menu.append_items(&[
        &item("app.about", "About DeckCraft", None),
        &PredefinedMenuItem::separator(),
        &item("app.preferences", "Preferences…", Some("CMD+,")),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::services(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(Some("Hide DeckCraft")),
        &PredefinedMenuItem::hide_others(Some("Hide Others")),
        &PredefinedMenuItem::show_all(Some("Show All")),
        &PredefinedMenuItem::separator(),
        &item("app.quit", "Quit DeckCraft", Some("CMD+Q")),
    ]);
    app_menu
}

/// Builds the Window menu integrated with macOS system window management.
fn build_window_menu() -> Submenu {
    let window = Submenu::new("Window", true);
    let _ = window.append_items(&[
        &item("window.minimize", "Minimize", Some("CMD+M")),
        &item("window.zoom", "Zoom", None),
        &PredefinedMenuItem::separator(),
        &item("view.newWindow", "New Window", None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::bring_all_to_front(None),
    ]);
    window
}

impl NativeMenu {
    /// Installs the native macOS menu bar onto NSApp and registers the event handler.
    pub fn install(ctx: &egui::Context) -> NativeMenu {
        let menu = Menu::new();
        let mut view_items = Vec::new();

        let _ = menu.append(&build_app_menu());

        let tree = menu_tree();
        let mut help_menu = None;

        for (name, entries) in tree {
            if name == "Help" {
                let help = Submenu::new("Help", true);
                for (label, id) in entries {
                    if label == "-" {
                        let _ = help.append(&PredefinedMenuItem::separator());
                    } else {
                        let _ = help.append(&item(id, label, None));
                    }
                }
                help_menu = Some(help);
                continue;
            }

            let sub = Submenu::new(name, true);
            for (label, id) in entries {
                if label == "-" {
                    let _ = sub.append(&PredefinedMenuItem::separator());
                } else if name == "View"
                    && matches!(
                        id,
                        "view.normal" | "view.sorter" | "view.notesPage" | "view.outline" | "view.ruler" | "view.gridlines" | "view.guides"
                    )
                {
                    let chk = CheckMenuItem::with_id(id, label, true, false, None);
                    let _ = sub.append(&chk);
                    view_items.push((id, chk));
                } else {
                    let _ = sub.append(&item(id, label, None));
                }
            }
            let _ = menu.append(&sub);
        }

        let window = build_window_menu();
        let _ = menu.append(&window);

        if let Some(help) = help_menu {
            let _ = menu.append(&help);
        }

        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();

        let (tx, rx) = channel();
        let ctx_clone = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = tx.send(e);
            ctx_clone.request_repaint();
        }));

        NativeMenu { _menu: menu, events: rx, view_items }
    }

    /// Polls pending native menu events, dispatches commands to SlideApp, and syncs view states.
    pub fn poll(&self, app: &mut SlideApp, ctx: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            let id = event.id().as_ref();
            let text_edit_focused = app.dialog.is_some() || ctx.text_edit_focused();
            if text_edit_focused {
                let clipboard = match id {
                    "edit.copy" => Some(egui::ViewportCommand::RequestCopy),
                    "edit.cut" => Some(egui::ViewportCommand::RequestCut),
                    "edit.paste" => Some(egui::ViewportCommand::RequestPaste),
                    _ => None,
                };
                if let Some(cmd) = clipboard {
                    ctx.send_viewport_cmd(cmd);
                    continue;
                }
            }
            let modal_safe = matches!(id, "app.about" | "app.quit");
            if app.dialog.is_some() && !modal_safe {
                continue;
            }
            match id {
                "app.quit" => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                "window.minimize" => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                "window.zoom" => {
                    let is_max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_max));
                }
                "app.closePresentation" => {
                    if app.session.active().is_none() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    } else {
                        app.close_presentation();
                    }
                }
                cmd => {
                    if let Err(e) = app.run(cmd, json!({})) {
                        log::debug!("native menu command '{cmd}' failed: {e}");
                    }
                }
            }
        }

        // Sync view check items
        for (mode, item) in &self.view_items {
            let checked = match *mode {
                "view.normal" => app.ui.view == ViewMode::Normal,
                "view.sorter" => app.ui.view == ViewMode::Sorter,
                "view.notesPage" => app.ui.view == ViewMode::NotesPage,
                "view.outline" => app.ui.view == ViewMode::Outline,
                "view.ruler" => app.ui.ruler,
                "view.gridlines" => app.ui.gridlines,
                "view.guides" => app.ui.guides,
                _ => false,
            };
            if item.is_checked() != checked {
                item.set_checked(checked);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_and_window_accelerators_parse() {
        for acc in ["CMD+,", "CMD+Q", "CMD+M"] {
            if let Err(e) = Accelerator::from_str(acc) {
                panic!("accelerator {acc} failed: {e:?}");
            }
        }
    }

    #[test]
    fn test_menu_tree_structure() {
        let tree = menu_tree();
        let menu_names: Vec<&'static str> = tree.iter().map(|(n, _)| *n).collect();
        assert!(menu_names.contains(&"File"));
        assert!(menu_names.contains(&"Edit"));
        assert!(menu_names.contains(&"View"));
        assert!(menu_names.contains(&"Insert"));
        assert!(menu_names.contains(&"Format"));
        assert!(menu_names.contains(&"Arrange"));
        assert!(menu_names.contains(&"Slide Show"));
        assert!(menu_names.contains(&"Help"));
    }
}
