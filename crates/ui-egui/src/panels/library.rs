//! Insert ▸ Library (#958): the Library pane the Craft apps share ([`craft_library_egui`]), with
//! DeckCraft's words and colours, over its `library.*` commands. The Slides tab shows the slides
//! of the presentations in the user's Library folders; clicking one inserts it, and its context
//! menu puts its shapes, or one named element, on the current slide.
//!
//! A tab of DeckCraft's own (the deck's objects, #909; shapes, icons, diagrams, charts) joins
//! [`LABELS`]`.tabs` and is drawn in [`DeckHost::tab`]; the shared pane draws the strip.

use craft_library_egui::{Host, Labels, Palette};
pub use craft_library_egui::{ITEMS, MAX_SHOWN_FILES, State};
use deckcraft_engine::library::Nouns;
use egui::Ui;
use serde_json::Value;

use crate::SlideApp;
use crate::theme::Tokens;

/// DeckCraft's words in the shared pane.
pub const LABELS: Labels = Labels {
    nouns: Nouns { one: "slide", many: "slides" },
    tabs: &[(ITEMS, "Slides")],
    item: "Slide",
    insert_item: "Insert slide",
    insert_shapes: Some("Insert shapes on current slide"),
    insert_element: Some("Insert element"),
    search_hint: "Search slides",
    add_folder_tip: "Show the slides of a folder of .pptx, .potx or .deckcraft files",
    empty: "Add a folder of .pptx, .potx or .deckcraft presentations to see their slides here. Click a slide to insert it.",
    not_in_browser: "The Library shows the slides in folders of presentations on this computer, which the browser version can't read. Use the desktop app to browse a Library.",
    aspect: 0.5625,
    one_per_file: false,
};

/// The app, as the shared pane sees it.
pub struct DeckHost<'a>(pub &'a mut SlideApp);

impl Host for DeckHost<'_> {
    fn act(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.0.run(id, params)
    }
    fn ask(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.0.session.execute(id, &params).map_err(|e| e.to_string())
    }
    fn folders(&self) -> Vec<String> {
        self.0.session.prefs.library_folders.clone()
    }
    fn pick_folder(&mut self) -> Option<Option<String>> {
        self.0.services.pick_open.as_mut().map(|pick| pick("folder"))
    }
    fn can_place(&self) -> bool {
        self.0.session.active().is_some_and(|d| d.current_slide().is_some())
    }
    fn report(&mut self, message: String) {
        self.0.set_status(message);
    }
}

pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let pal =
        Palette { text_dim: t.text_dim, text_faint: t.text_faint, separator: t.separator, control: t.control, border: t.border, accent: t.accent };
    // The pane's state is the app's; it is lent to the pane while the app is the pane's host.
    let mut state = std::mem::take(&mut app.library);
    craft_library_egui::show(ui, &mut state, &mut DeckHost(app), &LABELS, &pal);
    app.library = state;
}

#[cfg(test)]
mod ui_tests;
