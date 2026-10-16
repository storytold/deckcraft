//! macOS menu objects stay in the desktop host and on the main thread. The egui frontend
//! supplies command state and dispatch; this adapter owns no document behaviour.

use std::sync::mpsc::{self, Receiver};

use deckcraft_ui_egui::{SlideApp, menu_actions, menus};
use egui::{Context, RawInput, ViewportId};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu, accelerator::KeyAccelerator};

enum Item {
    Normal(MenuItem),
    Check(CheckMenuItem),
}

struct Entry {
    id: &'static str,
    label: &'static str,
    item: Item,
    state: menu_actions::State,
}

pub struct NativeMenu {
    // Keep the menu tree alive for the entire application lifetime.
    _menu: Menu,
    entries: Vec<Entry>,
    events: Receiver<MenuEvent>,
    pending: Vec<(String, ViewportId)>,
    target: ViewportId,
}

impl NativeMenu {
    pub fn install(ctx: &Context, app: &SlideApp) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let menu = Menu::new();
        let mut entries = Vec::new();
        let app_menu = Submenu::new("DeckCraft", true);
        add(&app_menu, &mut entries, ctx, app, "About DeckCraft", "app.about")?;
        app_menu.append(&PredefinedMenuItem::separator())?;
        add(&app_menu, &mut entries, ctx, app, "Settings…", "app.preferences")?;
        app_menu.append(&PredefinedMenuItem::separator())?;
        app_menu.append(&PredefinedMenuItem::services(None))?;
        app_menu.append(&PredefinedMenuItem::separator())?;
        app_menu.append(&PredefinedMenuItem::hide(Some("Hide DeckCraft")))?;
        app_menu.append(&PredefinedMenuItem::hide_others(None))?;
        app_menu.append(&PredefinedMenuItem::show_all(None))?;
        app_menu.append(&PredefinedMenuItem::separator())?;
        add(&app_menu, &mut entries, ctx, app, "Quit DeckCraft", "app.quit")?;
        menu.append(&app_menu)?;

        let window = Submenu::new("Window", true);
        window.append(&PredefinedMenuItem::minimize(None))?;
        window.append(&PredefinedMenuItem::maximize(Some("Zoom")))?;
        window.append(&PredefinedMenuItem::fullscreen(None))?;
        window.append(&PredefinedMenuItem::separator())?;
        window.append(&PredefinedMenuItem::bring_all_to_front(None))?;
        let mut help = None;
        for (name, commands) in menus::menu_tree() {
            if name == "Help" {
                menu.append(&window)?;
            }
            let submenu = Submenu::new(name, true);
            for (label, id) in commands {
                if id == "app.about" {
                    continue; // About belongs in the application menu on macOS.
                }
                if label == "-" {
                    submenu.append(&PredefinedMenuItem::separator())?;
                } else {
                    add(&submenu, &mut entries, ctx, app, label, id)?;
                }
            }
            menu.append(&submenu)?;
            if name == "Help" {
                help = Some(submenu);
            }
        }
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        if let Some(help) = help {
            help.set_as_help_menu_for_nsapp();
        }
        let (tx, events) = mpsc::channel();
        let wake = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event| {
            if tx.send(event).is_ok() {
                wake.request_repaint();
            }
        }));
        Ok(Self { _menu: menu, entries, events, pending: Vec::new(), target: ViewportId::ROOT })
    }

    pub fn input(&mut self, ctx: &Context, raw: &mut RawInput, app: &mut SlideApp) {
        if let Some((id, _)) = raw.viewports.iter().find(|(_, info)| info.focused == Some(true)) {
            self.target = *id;
        }
        while let Ok(event) = self.events.try_recv() {
            let id = event.id.0;
            // A native accelerator owns this activation, even if the backend also forwards a
            // key event. Do not remove synthetic edit events, which are added below.
            if let Some(shortcut) = menu_actions::shortcut(&id) {
                raw.events.retain(|event| match event {
                    egui::Event::Key { key, modifiers, .. } => !deckcraft_engine::keys::shortcut_matches(
                        shortcut,
                        deckcraft_ui_egui::key_name(*key),
                        deckcraft_engine::Mods { cmd: modifiers.command, shift: modifiers.shift, alt: modifiers.alt },
                    ),
                    _ => true,
                });
            }
            if !app.menu_input(ctx, raw, &id, self.target, clipboard_text) {
                self.pending.push((id, self.target));
            }
        }
    }

    pub fn after_ui(&mut self, ctx: &Context, app: &mut SlideApp) {
        for (id, target) in self.pending.drain(..) {
            app.menu_action(ctx, &id, target);
        }
        for entry in &mut self.entries {
            let state = app.menu_state(ctx, entry.id, entry.label, self.target);
            // AppKit toggles check items before dispatch, even if the command was rejected
            // or selected an already active view. The document remains the source of truth.
            if let Item::Check(item) = &entry.item
                && item.is_checked() != state.checked.unwrap_or(false)
            {
                item.set_checked(state.checked.unwrap_or(false));
            }
            if state == entry.state {
                continue;
            }
            match &entry.item {
                Item::Normal(item) => {
                    item.set_text(&state.label);
                    item.set_enabled(state.enabled);
                }
                Item::Check(item) => {
                    item.set_text(&state.label);
                    item.set_enabled(state.enabled);
                }
            }
            entry.state = state;
        }
    }
}

fn clipboard_text() -> String {
    arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default()
}

fn add(
    parent: &Submenu,
    entries: &mut Vec<Entry>,
    ctx: &Context,
    app: &SlideApp,
    label: &'static str,
    id: &'static str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state = app.menu_state(ctx, id, label, ViewportId::ROOT);
    let shortcut = menu_actions::shortcut(id).map(str::parse::<KeyAccelerator>).transpose()?;
    let item = if let Some(checked) = state.checked {
        let item = CheckMenuItem::with_id(id, &state.label, state.enabled, checked, None);
        item.set_key_accelerator(shortcut)?;
        parent.append(&item)?;
        Item::Check(item)
    } else {
        let item = MenuItem::with_id(id, &state.label, state.enabled, None);
        item.set_key_accelerator(shortcut)?;
        parent.append(&item)?;
        Item::Normal(item)
    };
    entries.push(Entry { id, label, item, state });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_menu_shortcut_parses_and_has_one_owner() {
        let mut shortcuts = std::collections::HashMap::new();
        let mut ids: Vec<_> = menus::menu_tree().into_iter().flat_map(|(_, items)| items).map(|(_, id)| id).collect();
        ids.extend(["app.preferences", "app.quit"]);
        for id in ids {
            if let Some(shortcut) = menu_actions::shortcut(id) {
                let key = shortcut.parse::<KeyAccelerator>().unwrap_or_else(|e| panic!("{id}: {shortcut}: {e}"));
                assert!(shortcuts.insert(key, id).is_none(), "duplicate accelerator for {id}");
            }
        }
        assert!(!shortcuts.contains_key(&"Cmd+M".parse::<KeyAccelerator>().unwrap()));
        assert!(!shortcuts.contains_key(&"Cmd+H".parse::<KeyAccelerator>().unwrap()));
    }
}
