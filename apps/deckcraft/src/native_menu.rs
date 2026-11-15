//! macOS: the native menu bar, built from `deckcraft_ui_egui::menus::menu_tree()`. Entries run
//! DeckCraft's commands and are dimmed while their command can't run. The Window menu is NSApp's
//! `windowsMenu`, so macOS adds Move & Resize, Fill, Center and Full-Screen Tile to it.

use std::str::FromStr;
use std::sync::mpsc::{Receiver, channel};

use deckcraft_ui_egui::SlideApp;
use deckcraft_ui_egui::menus::{self, Entry};
use muda::accelerator::Accelerator;
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use serde_json::json;

const QUIT: &str = "app.quit";
const DOC: &str = "doc:";

enum Widget {
    Plain(MenuItem),
    Check(CheckMenuItem),
}

/// A menu entry; its menu id is its index in `NativeMenu::items`.
struct Item {
    action: &'static str,
    keys: Option<&'static str>,
    widget: Widget,
    /// What the menu shows now (enabled, checked), so a frame only calls AppKit for changes.
    shown: (bool, bool),
}

pub struct NativeMenu {
    _menu: Menu,
    events: Receiver<MenuEvent>,
    items: Vec<Item>,
    window_menu: Submenu,
    /// The open presentations at the end of the Window menu, and the titles they show.
    docs: Vec<CheckMenuItem>,
    doc_titles: Vec<String>,
}

fn add(parent: &Submenu, items: &mut Vec<Item>, app: &SlideApp, label: &str, action: &'static str, keys: Option<&'static str>) {
    let id = items.len().to_string();
    let accel = keys.and_then(|k| Accelerator::from_str(k).ok());
    let widget = match menus::menu_checked(app, action) {
        Some(on) => Widget::Check(CheckMenuItem::with_id(id, label, false, on, accel)),
        None => Widget::Plain(MenuItem::with_id(id, label, false, accel)),
    };
    let _ = match &widget {
        Widget::Plain(w) => parent.append(w),
        Widget::Check(w) => parent.append(w),
    };
    items.push(Item { action, keys, widget, shown: (false, false) });
}

fn fill(parent: &Submenu, entries: &[Entry], items: &mut Vec<Item>, app: &SlideApp) {
    for e in entries {
        match e {
            Entry::Item { label, action, keys } => add(parent, items, app, label, action, *keys),
            Entry::Sub(label, sub) => {
                let s = Submenu::new(*label, true);
                fill(&s, sub, items, app);
                let _ = parent.append(&s);
            }
            Entry::Sep => {
                let _ = parent.append(&PredefinedMenuItem::separator());
            }
        }
    }
}

impl NativeMenu {
    /// Installs the menu bar on NSApp; its clicks wake `ctx` and are run by [`NativeMenu::poll`].
    pub fn install(ctx: &egui::Context, app: &SlideApp) -> NativeMenu {
        let menu = Menu::new();
        let mut items = Vec::new();

        let app_menu = Submenu::new("DeckCraft", true);
        add(&app_menu, &mut items, app, "About DeckCraft", "app.about", None);
        let _ = app_menu.append(&PredefinedMenuItem::separator());
        add(&app_menu, &mut items, app, "Preferences…", "app.preferences", Some("Cmd+,"));
        let _ = app_menu.append_items(&[
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(Some("Hide DeckCraft")),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            // Not AppKit's terminate:, which would skip the unsaved-changes prompt.
            &MenuItem::with_id(QUIT, "Quit DeckCraft", true, Accelerator::from_str("Cmd+Q").ok()),
        ]);
        let _ = menu.append(&app_menu);

        let mut window_menu = None;
        for (name, entries) in menus::menu_tree() {
            let sub = Submenu::new(name, true);
            if name == "Window" {
                let _ = sub.append_items(&[&PredefinedMenuItem::minimize(None), &PredefinedMenuItem::zoom(None), &PredefinedMenuItem::separator()]);
                fill(&sub, &entries, &mut items, app);
                let _ = sub.append_items(&[
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::bring_all_to_front(None),
                    &PredefinedMenuItem::separator(),
                ]);
                window_menu = Some(sub.clone());
            } else {
                fill(&sub, &entries, &mut items, app);
            }
            let _ = menu.append(&sub);
        }
        menu.init_for_nsapp();
        let window_menu = window_menu.unwrap_or_else(|| Submenu::new("Window", true));
        window_menu.set_as_windows_menu_for_nsapp();

        let (tx, events) = channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e| {
            let _ = tx.send(e);
            ctx.request_repaint();
        }));
        NativeMenu { _menu: menu, events, items, window_menu, docs: Vec::new(), doc_titles: Vec::new() }
    }

    /// Runs the entries clicked (or whose shortcut was pressed) since the last frame.
    pub fn poll(&self, app: &mut SlideApp, ctx: &egui::Context) {
        let text_field = ctx.egui_wants_keyboard_input();
        while let Ok(e) = self.events.try_recv() {
            let id = e.id().0.as_str();
            if id == QUIT {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                continue;
            }
            if let Some(i) = id.strip_prefix(DOC).and_then(|i| i.parse::<usize>().ok()) {
                let _ = app.run("window.next", json!({"index": i}));
                continue;
            }
            let Some(item) = id.parse::<usize>().ok().and_then(|i| self.items.get(i)) else { continue };
            // The state may have changed since the menu was drawn.
            if !menus::menu_enabled(app, item.action, text_field) {
                continue;
            }
            match item.action {
                // The system clipboard events reach the text field or the slide like the keys do.
                "edit.copy" => ctx.send_viewport_cmd(egui::ViewportCommand::RequestCopy),
                "edit.cut" => ctx.send_viewport_cmd(egui::ViewportCommand::RequestCut),
                "edit.paste" => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
                // The menu bar took the key from the text field: give it back.
                "edit.undo" | "edit.redo" | "edit.selectAll" if text_field => {
                    if let Some((key, m)) = item.keys.and_then(menus::parse_keys) {
                        let modifiers = egui::Modifiers { command: m.cmd, mac_cmd: m.cmd, shift: m.shift, alt: m.alt, ..Default::default() };
                        ctx.input_mut(|i| i.events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }));
                    }
                }
                action => menus::run_menu_entry(app, action, item.keys),
            }
        }
    }

    /// Dims, enables and checks the entries for the state at the end of this frame.
    pub fn refresh(&mut self, app: &SlideApp, ctx: &egui::Context) {
        let text_field = ctx.egui_wants_keyboard_input();
        for item in &mut self.items {
            let now = (menus::menu_enabled(app, item.action, text_field), menus::menu_checked(app, item.action).unwrap_or(false));
            if now == item.shown {
                continue;
            }
            match &item.widget {
                Widget::Plain(w) => w.set_enabled(now.0),
                Widget::Check(w) => {
                    w.set_enabled(now.0);
                    w.set_checked(now.1);
                }
            }
            item.shown = now;
        }
        let titles: Vec<String> = app.session.documents().iter().map(|d| d.title()).collect();
        if titles != self.doc_titles {
            for w in self.docs.drain(..) {
                let _ = self.window_menu.remove(&w);
            }
            for (i, t) in titles.iter().enumerate() {
                let w = CheckMenuItem::with_id(format!("{DOC}{i}"), t, true, false, None);
                let _ = self.window_menu.append(&w);
                self.docs.push(w);
            }
            self.doc_titles = titles;
        }
        let active = app.session.active_index();
        for (i, w) in self.docs.iter().enumerate() {
            if w.is_checked() != (active == Some(i)) {
                w.set_checked(active == Some(i));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use deckcraft_ui_egui::menus::{Entry, menu_tree};

    fn keys(entries: &[Entry], out: &mut Vec<&'static str>) {
        for e in entries {
            match e {
                Entry::Item { keys: Some(k), .. } => out.push(k),
                Entry::Sub(_, sub) => keys(sub, out),
                _ => {}
            }
        }
    }

    /// muda reads every shortcut in the menu tree.
    #[test]
    fn every_menu_shortcut_is_an_accelerator() {
        let mut all = vec!["Cmd+,", "Cmd+Q"];
        for (_, entries) in menu_tree() {
            keys(&entries, &mut all);
        }
        for k in all {
            assert!(<muda::accelerator::Accelerator as std::str::FromStr>::from_str(k).is_ok(), "{k}");
        }
    }
}
