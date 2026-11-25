//! The document window (the slide canvas, the slide sorter or the notes page) as the window's one document canvas: its
//! widget, its Tab stop and when it takes the keyboard focus.

use craft_ui::runtime::overlay::{self, OpenFilter};

use super::{SlideApp, ViewMode};
use crate::framework::{document_id, focus_document, focused_widget, host_canvas};

/// What the document window's focus depends on from pass to pass.
#[derive(Default)]
pub(crate) struct DocumentFocus {
    /// The presentation (its uid) whose document window the pass drew; `None` on the start screen, the File tab and in
    /// a slide show.
    pub(super) drawn: Option<u64>,
    /// A presentation opened or became active and has yet to ask for the focus.
    wanted: bool,
}

impl SlideApp {
    /// Draws the document window's widget over `rect` and declares it as the window's canvas. A press in it gives it
    /// the focus. Real presses only: with the document focused, egui makes Enter and Space clicks of it.
    pub(crate) fn document_widget(&mut self, ui: &egui::Ui, rect: egui::Rect, sense: egui::Sense) -> egui::Response {
        let resp = ui.interact(rect, document_id(), sense);
        self.document_focus.drawn = self.session.active().map(|d| d.uid);
        host_canvas(ui.ctx(), self.document_keeps_tab(ui.ctx()));
        if resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.any_pressed()) {
            focus_document(ui.ctx());
        }
        resp
    }

    /// Whether the focused document keeps Tab: on the slide, while the session has something for it to do. A modal
    /// dialog, a menu or a popup that is up gets it instead.
    fn document_keeps_tab(&self, ctx: &egui::Context) -> bool {
        matches!(self.ui.view, ViewMode::Normal | ViewMode::Outline | ViewMode::Reading) && self.session.tab_has_work() && !self.overlay_open(ctx)
    }

    /// Whether a modal dialog, DeckCraft's or the framework's, the palette, the opening overlay, or an egui menu or
    /// popup is up.
    pub(super) fn overlay_open(&self, ctx: &egui::Context) -> bool {
        self.dialog.as_ref().is_some_and(|d| d.modal())
            || self.palette.is_some()
            || self.opening.is_some()
            || ctx.any_popup_open()
            || egui::Popup::is_any_open(ctx)
            || overlay::any_open(ctx, OpenFilter::Modal)
    }

    /// Ends the document window's pass, after the views; `before` is the presentation the last pass drew. The pass's
    /// input has changed what Tab has to do, so the document is declared again. One that opened or became active asks
    /// for the focus. It waits for a modal dialog, menu or popup to close, and leaves the focus with a text field and
    /// with a widget of another layer.
    pub(super) fn finish_document(&mut self, ctx: &egui::Context, before: Option<u64>) {
        let drawn = self.document_focus.drawn;
        if drawn.is_none() {
            self.document_focus.wanted = false;
            return;
        }
        host_canvas(ctx, self.document_keeps_tab(ctx));
        self.document_focus.wanted |= drawn != before;
        if !self.document_focus.wanted || self.overlay_open(ctx) {
            return;
        }
        self.document_focus.wanted = false;
        let layer = |id| ctx.read_response(id).map(|widget| widget.layer_id);
        if !ctx.text_edit_focused() && focused_widget(ctx).is_none_or(|id| layer(id) == layer(document_id())) {
            focus_document(ctx);
        }
    }

    /// While slide text is being edited, the text owns the keyboard (#93): a stray focus on an ordinary widget (the
    /// window's drag handle, a ribbon button) would swallow every later key and end text entry, and with nothing
    /// focused Tab would leave the text. The document takes the focus back; a text field or an open menu keeps its keys.
    pub(super) fn edited_text_takes_focus(&self, ctx: &egui::Context) {
        let editing = self.session.active().is_some_and(|d| d.selection.text.is_some());
        if !editing || ctx.text_edit_focused() || self.overlay_open(ctx) {
            return;
        }
        if ctx.read_response(document_id()).is_some() {
            if !ctx.memory(|m| m.has_focus(document_id())) {
                focus_document(ctx);
            }
        } else if let Some(id) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
    }
}
