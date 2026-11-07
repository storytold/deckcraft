//! The notes/script pane of the Record view.
//!
//! The pane shows the current slide's speaker notes over a dark surface. The notes are editable
//! while the recording runs, so a script can be corrected between takes without leaving the
//! Record view; each slide keeps its own draft and an edit is saved with `slide.notes` when the
//! field loses focus, the slide changes or the pane closes. A-/A+ (and Ctrl+-/Ctrl++) change the
//! script's text size within [`NOTES_MIN`]..[`NOTES_MAX`], long scripts scroll inside the pane,
//! and the Teleprompter view adds an auto-scrolling read-only script over the slide while the
//! take is running.
//!
//! The section files own one feature each:
//! - [`display`] draws a read-only formatted script and its empty state;
//! - [`sizing`] owns the A-/A+ controls, the limits and the sample size;
//! - [`scroll`] wraps the script in the pane's scroll area and keeps its offset;
//! - [`edit`] owns the editable draft, focus and saving;
//! - [`teleprompter`] owns the auto-scrolling overlay and its speed.

mod display;
mod edit;
mod scroll;
mod sizing;
mod teleprompter;

use std::sync::Arc;

use deckcraft_model::Presentation;
use egui::{Color32, CornerRadius, Stroke, Ui, UiBuilder, vec2};

use super::views::ViewKind;
use super::{Layout as RecordLayout, RecordView, Show};
use crate::SlideApp;

pub(crate) use edit::State as EditState;
pub(crate) use sizing::{NOTES_DEFAULT, NOTES_MAX, NOTES_MIN, clamp_size};
pub(crate) use teleprompter::State as TeleprompterState;

/// Draw the notes pane. `show` supplies the current slide index; `doc` its notes.
pub(super) fn ui(app: &mut SlideApp, ui: &mut Ui, show: &mut Show, rec: &mut RecordView, doc: &Arc<Presentation>, layout: &RecordLayout) {
    let rect = layout.notes;
    let slide = show.state.slide;
    // The draft is bound to the presentation it was written in, not only to its slide.
    let uid = app.session.active().map(|d| d.uid).unwrap_or(0);
    if rect.height() < 1.0 || rec.view == ViewKind::Slide {
        // Leaving the pane must not lose an edit, and a script written on this slide is kept
        // for the next time the pane opens.
        edit::flush(app, rec, doc, slide);
        return;
    }
    // Follow the slide and the view: a draft belongs to the slide it was written on, and a
    // teleprompter take restarts at the top when it is shown again.
    let slide_id = doc.slides.get(slide).map(|s| s.id);
    // A new presentation can reuse the old slide id, so the document counts too.
    if rec.notes_edit.slide != slide_id || rec.notes_edit.doc != Some(uid) {
        edit::flush(app, rec, doc, slide);
        rec.notes_edit.reset_in(uid, slide_id);
        rec.notes_scroll = 0.0;
        rec.teleprompter.rewind();
    }

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), Color32::from_rgb(26, 26, 30));
    painter.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, Color32::from_gray(58)), egui::StrokeKind::Inside);

    sizing::shortcuts(ui.ctx(), rec);
    let body_rect = sizing::header(ui, rec, rect);
    if body_rect.height() < 1.0 {
        return;
    }
    let mut body = ui.new_child(UiBuilder::new().max_rect(body_rect));
    body.visuals_mut().override_text_color = Some(Color32::from_gray(224));
    // With the teleprompter overlay off, the pane's script is the visible one: it follows the
    // take's offset instead of its own scroll position.
    let following = rec.view == ViewKind::Teleprompter && !rec.teleprompter.overlay;
    let start = if following { rec.teleprompter.offset } else { rec.notes_scroll };
    rec.notes_scroll = scroll::pane(&mut body, start, body_rect, |ui| {
        edit::ui(app, ui, rec, doc, slide);
    });
    if following {
        // The pane hands back the offset it could actually show; the take's offset follows it, so
        // neither runs past the rendered script.
        rec.teleprompter.offset = rec.notes_scroll;
    }
}

/// A read-only script over the slide while a Teleprompter take is running. Drawn after the pane
/// so it cannot be hidden by the pane's background.
pub(super) fn overlay(ui: &mut Ui, rec: &mut RecordView, doc: &Presentation, show: &Show, layout: &RecordLayout) {
    if rec.view != ViewKind::Teleprompter || !rec.teleprompter.overlay || rec.status != super::RecordStatus::Recording {
        // Nothing is drawn, so no part of the slide is chrome: a stale rectangle would keep
        // swallowing clicks there.
        rec.overlay = None;
        return;
    }
    let rect = teleprompter::overlay_rect(layout);
    if rect.height() < 1.0 {
        rec.overlay = None;
        return;
    }
    rec.overlay = Some(rect);
    let slide = doc.slides.get(show.state.slide);
    let body = slide.map_or_else(deckcraft_model::text::TextBody::default, |s| s.notes.clone());
    display::overlay(ui, rect, &body, &mut rec.teleprompter, rec.notes_px);
}

/// Advance the script's scroll for this frame. The shell calls it after the top bar, so a
/// Record or Pause click takes effect on the same frame.
pub(super) fn tick(ctx: &egui::Context, rec: &mut RecordView) {
    teleprompter::tick(ctx, rec);
}

/// Whether a notes editor owns the keyboard this frame. The shell skips show navigation while it
/// does, so Space and the arrows type into the script instead of advancing the show.
/// The editor's focus id, for the tests that check who owns the keyboard.
#[cfg(test)]
pub(super) fn editor_id_for_tests() -> egui::Id {
    edit::editor_id()
}

pub(super) fn has_focus(ctx: &egui::Context) -> bool {
    edit::has_focus(ctx)
}

/// Save an in-progress script edit before the Record view goes away.
pub(crate) fn flush(app: &mut SlideApp, rec: &mut RecordView, doc: &Arc<Presentation>, slide: usize) {
    edit::flush(app, rec, doc, slide);
}

/// The notes pane's inner margin, shared by the header and the body.
const INSET: f32 = 12.0;

/// The body rect under the header, inside the pane's padding.
fn body_rect(rect: egui::Rect, header_height: f32) -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(rect.min.x + INSET, rect.min.y + header_height + 2.0), egui::pos2(rect.max.x - INSET, rect.max.y - 8.0))
}

/// Size of the little resize handle widgets, used by the header layout.
const CONTROL: egui::Vec2 = vec2(22.0, 20.0);

#[cfg(test)]
mod tests;
