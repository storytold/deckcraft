//! The native macOS menu bar, through craft-menu (`craft_menu::mac`), the menu layer the other
//! Craft apps share. The menu objects stay in the desktop host, on the main thread; the egui
//! frontend builds the menus and runs their commands ([`deckcraft_ui_egui::menu_actions`]).
//! craft-menu hands key equivalents back to egui, so shortcuts run through the app's own key
//! handling (text fields keep ⌘C, ⌘V and ⌘Z) as without a native menu.

use craft_menu::{Layout, LayoutOptions, NativeMenu};
use deckcraft_ui_egui::{SlideApp, menu_actions};
use egui::{Context, Event, RawInput, ViewportId};

pub struct Menus {
    menu: NativeMenu,
    /// The window in front, which Close acts on.
    target: ViewportId,
    /// Key presses for a focused text field's own Undo, Redo and Select All, for egui's next pass.
    keys: Vec<Event>,
}

impl Menus {
    /// Install the menu bar, once, from eframe's app creator (the main thread, before winit's
    /// default menu would stay up). `None` when AppKit refused it.
    pub fn install(ctx: &Context, app: &SlideApp) -> Option<Self> {
        let menu = craft_menu::mac::install(ctx, &layout(ctx, app, ViewportId::ROOT).bar)?;
        Some(Self { menu, target: ViewportId::ROOT, keys: Vec::new() })
    }

    /// Before egui reads the input: note the window in front, and add the menu's key
    /// equivalents and the key presses queued for a text field.
    pub fn raw_input(&mut self, raw: &mut RawInput) {
        if let Some((id, _)) = raw.viewports.iter().find(|(_, info)| info.focused == Some(true)) {
            self.target = *id;
        }
        raw.events.append(&mut self.keys);
        self.menu.raw_input(raw);
    }

    /// After the frame's UI: run the menu's clicks and bring the menus in step with the app.
    pub fn after_ui(&mut self, ctx: &Context, app: &mut SlideApp) {
        self.menu.set_gate(app.menu_gate(ctx));
        for command in self.menu.take_clicks(ctx) {
            self.keys.extend(app.run_menu_command(ctx, &command, self.target));
        }
        if !self.keys.is_empty() {
            ctx.request_repaint();
        }
        for c in self.menu.sync(layout(ctx, app, self.target)) {
            log::info!("menu: {} is {} on macOS and {} in DeckCraft; {}", c.shortcut, c.system, c.command.id, c.resolution);
        }
    }
}

fn layout(ctx: &Context, app: &SlideApp, target: ViewportId) -> Layout {
    craft_menu::mac_layout(&menu_actions::for_mac(app.menu_bar(ctx, target)), &LayoutOptions::new("DeckCraft"))
}
