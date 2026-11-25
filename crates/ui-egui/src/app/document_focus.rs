//! The document window (the slide canvas, the slide sorter or the notes page) as the window's one document canvas: its
//! widget, its Tab stop and when it takes the keyboard focus.

use craft_ui::runtime::overlay::{self, OpenFilter};

use super::{SlideApp, ViewMode};
use crate::framework::{document_id, focus_document, focused_widget, host_canvas};

/// How many passes the document asks for the focus: egui names the widget of a closed palette, dialog or menu as
/// focused for one pass after it was last drawn.
const ASKING_PASSES: u8 = 2;

/// What the document window's focus depends on from pass to pass.
#[derive(Default)]
pub(crate) struct DocumentFocus {
    /// The presentation (its uid) whose document window the pass drew; `None` on the start screen, the File tab and in
    /// a slide show.
    pub(super) drawn: Option<u64>,
    /// The passes left in which a presentation that opened or became active asks for the focus.
    opened: u8,
    /// The passes left in which the document asks for the focus after a dialog, menu or popup closed.
    uncovered: u8,
}

impl SlideApp {
    /// Draws the document window's widget over `rect` and declares it as the window's canvas. A press in it gives it
    /// the focus, unless a dialog is open. Real presses only: with the document focused, egui makes Enter and Space
    /// clicks of it.
    pub(crate) fn document_widget(&mut self, ui: &egui::Ui, rect: egui::Rect, sense: egui::Sense) -> egui::Response {
        let resp = ui.interact(rect, document_id(), sense);
        self.document_focus.drawn = self.session.active().map(|d| d.uid);
        host_canvas(ui.ctx(), self.document_keeps_tab(ui.ctx()));
        if resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.any_pressed()) && !self.dialog_open(ui.ctx()) {
            focus_document(ui.ctx());
        }
        resp
    }

    /// [`Self::document_widget`] behind widgets that take the presses (the slide sorter's thumbnails): a press on any of
    /// them gives the document the focus, unless a text field has it.
    pub(crate) fn document_behind(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        self.document_widget(ui, rect, egui::Sense::focusable_noninteractive());
        let pressed = ui.input(|i| i.pointer.any_pressed()) && ui.rect_contains_pointer(rect);
        if pressed && !ui.ctx().text_edit_focused() && !self.overlay_open(ui.ctx()) {
            focus_document(ui.ctx());
        }
    }

    /// Whether the document window is the slide being edited, where Tab can have work.
    fn editing_slide(&self) -> bool {
        self.backstage.is_none() && matches!(self.ui.view, ViewMode::Normal | ViewMode::Outline | ViewMode::Reading)
    }

    /// Whether the focused document keeps Tab: on the slide, while the session has something for it to do. A modal
    /// dialog, a menu or a popup that is up gets it instead.
    fn document_keeps_tab(&self, ctx: &egui::Context) -> bool {
        self.editing_slide() && self.session.tab_has_work() && !self.overlay_open(ctx)
    }

    /// Whether a dialog, DeckCraft's or a modal one of the framework's, the palette or the opening overlay is up.
    fn dialog_open(&self, ctx: &egui::Context) -> bool {
        self.dialog.as_ref().is_some_and(|d| d.modal())
            || self.palette.is_some()
            || self.opening.is_some()
            || overlay::any_open(ctx, OpenFilter::Modal)
    }

    /// Whether a dialog ([`Self::dialog_open`]), or an egui menu or popup, is up.
    fn overlay_open(&self, ctx: &egui::Context) -> bool {
        self.dialog_open(ctx) || ctx.any_popup_open() || egui::Popup::is_any_open(ctx)
    }

    /// Whether the document may take the focus: no text field and no widget of another layer (the framework's update
    /// notice) has it.
    fn focus_is_free(ctx: &egui::Context) -> bool {
        let layer = |id| ctx.read_response(id).map(|widget| widget.layer_id);
        !ctx.text_edit_focused() && focused_widget(ctx).is_none_or(|id| layer(id) == layer(document_id()))
    }

    /// Ends the document window's pass, after the views; `before` is the presentation the last pass drew. The pass's
    /// input has changed what Tab has to do, so the document is declared again. A presentation that opened or became
    /// active asks for the focus, and so does the document when a dialog, menu or popup closes and leaves nothing
    /// focused. Both wait for the last of them to close.
    pub(super) fn finish_document(&mut self, ctx: &egui::Context, before: Option<u64>) {
        let drawn = self.document_focus.drawn;
        if drawn.is_none() {
            self.document_focus.opened = 0;
            self.document_focus.uncovered = 0;
            return;
        }
        host_canvas(ctx, self.document_keeps_tab(ctx));
        if drawn != before {
            self.document_focus.opened = ASKING_PASSES;
        }
        if self.overlay_open(ctx) {
            self.document_focus.uncovered = ASKING_PASSES;
            return;
        }
        let DocumentFocus { opened, uncovered, .. } = &mut self.document_focus;
        if *opened == 0 && *uncovered == 0 {
            return;
        }
        let unfocused = ctx.memory(|m| m.focused().is_none());
        if unfocused || (*opened > 0 && Self::focus_is_free(ctx)) {
            (*opened, *uncovered) = (0, 0);
            focus_document(ctx);
        } else {
            (*opened, *uncovered) = (opened.saturating_sub(1), uncovered.saturating_sub(1));
            ctx.request_repaint();
        }
    }

    /// While slide text is being edited, the text owns the keyboard (#93). The focus on an ordinary widget (the ribbon
    /// button whose Space inserted the text box) would swallow every later key and end text entry, and with nothing
    /// focused (a ribbon field committed with Enter) Tab would leave the text. The document takes the focus; a text
    /// field or an open menu keeps its keys.
    pub(super) fn edited_text_takes_focus(&self, ctx: &egui::Context) {
        let editing = self.editing_slide() && self.session.active().is_some_and(|d| d.selection.text.is_some());
        if editing && !ctx.memory(|m| m.has_focus(document_id())) && !self.overlay_open(ctx) && Self::focus_is_free(ctx) {
            focus_document(ctx);
        }
    }
}
