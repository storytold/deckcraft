//! Editable notes draft: focus, editing during a take and saving with `slide.notes`.
//!
//! The draft belongs to the slide it was written on. It is loaded from the slide's notes when the
//! slide changes and refreshed when the notes change elsewhere in the document, as long as the
//! field is not focused. An edit is written back with `slide.notes {body}` when the field loses
//! focus (including a click on another control or Escape), when the slide changes and when the
//! pane closes, so a script can be corrected between takes without leaving the Record view.

use std::sync::Arc;

use deckcraft_model::Presentation;
use egui::{Color32, Ui};
use serde_json::json;

use crate::SlideApp;
use crate::panels::notes::{self as notes_panel, Draft, Look};

use super::RecordView;

/// The notes editor's per-slide draft and the slide it belongs to.
///
/// The slide is held by id, not by index: the draft can outlive a `slide.move` or `slide.delete`
/// while the field has focus, and an index would then write the text to a different slide.
#[derive(Default)]
pub(crate) struct State {
    pub(super) slide: Option<deckcraft_model::SlideId>,
    /// The presentation the draft was written in. New presentations reuse slide ids, so the id
    /// alone cannot tell the draft's slide from a fresh one.
    pub(super) doc: Option<u64>,
    draft: Draft,
}

impl State {
    /// Start a fresh draft for `slide`; the notes are loaded on the next [`ui`].
    pub(super) fn reset(&mut self, slide: Option<deckcraft_model::SlideId>) {
        self.slide = slide;
        self.draft = Draft::default();
    }

    /// The draft for `slide` in `doc`; a different document starts a fresh one.
    pub(super) fn reset_in(&mut self, doc: u64, slide: Option<deckcraft_model::SlideId>) {
        self.doc = Some(doc);
        self.reset(slide);
    }
}

/// The editor's fixed id, shared by every slide so focus survives a slide change cleanly.
pub(super) fn editor_id() -> egui::Id {
    egui::Id::unique("record_notes_edit")
}

/// Whether the notes editor owns the keyboard this frame.
pub(super) fn has_focus(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.has_focus(editor_id()))
}

/// Draw the editable script for `slide`, saving an edit once the field loses focus.
pub(super) fn ui(app: &mut SlideApp, ui: &mut Ui, rec: &mut RecordView, doc: &Arc<Presentation>, slide: usize) {
    let notes = doc.slides.get(slide).map(|s| s.notes.clone()).unwrap_or_default();
    let focused = has_focus(ui.ctx());
    // A draft written for another slide, or notes changed elsewhere while the field is not
    // focused, is replaced by what the document holds now. An unsaved edit is kept: it is saved
    // once the field loses focus, so a refresh must not overwrite it first.
    let changed_elsewhere = !focused && !rec.notes_edit.draft.edited && rec.notes_edit.draft.body != notes;
    let slide_id = doc.slides.get(slide).map(|s| s.id);
    if rec.notes_edit.slide != slide_id || changed_elsewhere {
        rec.notes_edit.slide = slide_id;
        rec.notes_edit.draft = Draft::new(&notes);
    }
    // Escape leaves the field; it must not leave the Record view or stop the take.
    if focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        ui.ctx().memory_mut(|m| m.surrender_focus(editor_id()));
    }

    let look = Look { size: rec.notes_px, color: Some(Color32::from_gray(224)) };
    let output = notes_panel::editor(ui, &mut rec.notes_edit.draft, &look, |e| {
        e.id(editor_id()).desired_width(f32::INFINITY).desired_rows(2).frame(egui::Frame::NONE).hint_text("Click to add notes")
    });

    if rec.notes_edit.draft.edited {
        // Clicking any other control blurs the field in this same frame, so the edit is not left
        // waiting for the next one.
        let clicked_outside =
            ui.input(|i| i.pointer.primary_clicked() && i.pointer.interact_pos().is_some_and(|p| !output.response.response.rect.contains(p)));
        if clicked_outside {
            ui.ctx().memory_mut(|m| m.surrender_focus(editor_id()));
        }
        if !has_focus(ui.ctx()) {
            save(app, rec, slide);
        }
    }
}

/// Write the draft back to its slide if it has an unsaved edit, then clear the flag.
pub(super) fn save(app: &mut SlideApp, rec: &mut RecordView, slide: usize) {
    if !rec.notes_edit.draft.edited {
        return;
    }
    // Resolve the draft's slide id against the document as it is now. A slide deleted meanwhile
    // means the edit has nowhere to go: skip it rather than write to whichever slide took the
    // index.
    let index = match (rec.notes_edit.slide, app.session.active()) {
        (Some(id), Some(d)) if rec.notes_edit.doc.is_none_or(|uid| uid == d.uid) => match d.doc.slide_index(id) {
            Some(i) => i,
            None => {
                log::warn!("record notes: the slide this edit was written on is gone; the edit is dropped");
                rec.notes_edit.draft.edited = false;
                return;
            }
        },
        // The draft belongs to another presentation: writing it here would overwrite a fresh
        // slide that happens to reuse the id.
        (Some(_), Some(_)) => {
            log::warn!("record notes: the draft belongs to another presentation; the edit is dropped");
            rec.notes_edit.draft.edited = false;
            return;
        }
        _ => slide,
    };
    rec.notes_edit.draft.edited = false;
    if let Err(e) = app.run("slide.notes", json!({"body": rec.notes_edit.draft.body.clone(), "index": index})) {
        log::warn!("record notes: edit to slide {index} not saved: {e}");
    }
}

/// Save an in-progress edit before the pane or the Record view goes away.
pub(super) fn flush(app: &mut SlideApp, rec: &mut RecordView, _doc: &Arc<Presentation>, slide: usize) {
    save(app, rec, slide);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::sync::Arc;

    use deckcraft_model::Presentation;
    use deckcraft_model::text::TextBody;
    use serde_json::json;

    use super::*;

    fn app_with(text: &str) -> (SlideApp, Arc<Presentation>) {
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("slide.notes", &json!({"index": 0, "text": text})).unwrap();
        let doc = session.active().unwrap().doc.clone();
        (SlideApp::new(session, crate::Services::default()), doc)
    }

    fn notes_text(app: &SlideApp) -> String {
        app.session.active().unwrap().doc.slides[0].notes_text()
    }

    #[test]
    fn flush_saves_an_edited_draft_and_is_idempotent() {
        let (mut app, doc) = app_with("original");
        let mut rec = RecordView::default();
        rec.notes_edit.reset(Some(doc.slides[0].id));
        rec.notes_edit.draft = Draft::new(&TextBody::from_text("edited"));
        rec.notes_edit.draft.edited = true;

        flush(&mut app, &mut rec, &doc, 0);
        assert_eq!(notes_text(&app), "edited", "the draft is written to the slide");
        assert!(!rec.notes_edit.draft.edited, "the edit is marked saved");

        // A second flush has nothing to save, even if the body is changed without an edit.
        rec.notes_edit.draft = Draft::new(&TextBody::from_text("stale"));
        flush(&mut app, &mut rec, &doc, 0);
        assert_eq!(notes_text(&app), "edited", "an unedited draft does not overwrite the notes");
    }

    #[test]
    fn an_unedited_draft_is_not_saved() {
        let (mut app, doc) = app_with("keep me");
        let mut rec = RecordView::default();
        rec.notes_edit.reset(Some(doc.slides[0].id));
        rec.notes_edit.draft = Draft::new(&TextBody::from_text("discarded"));

        flush(&mut app, &mut rec, &doc, 0);
        assert_eq!(notes_text(&app), "keep me");
    }

    #[test]
    fn reset_moves_the_draft_to_the_new_slide_and_clears_it() {
        let (first, second) = (deckcraft_model::SlideId(1), deckcraft_model::SlideId(2));
        let mut rec = RecordView {
            notes_edit: State { slide: Some(first), doc: None, draft: Draft::new(&TextBody::from_text("first")) },
            ..Default::default()
        };

        rec.notes_edit.reset(Some(second));
        assert_eq!(rec.notes_edit.slide, Some(second));
        assert_eq!(rec.notes_edit.draft, Draft::default(), "the old slide's text is not carried over");
    }
}
