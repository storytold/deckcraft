//! The notes/script pane. It is read-only in the shell: the section agent owning editing replaces
//! the body with `crate::panels::notes::editor` and saves through `app.run("slide.notes", ...)`.

use std::sync::Arc;

use deckcraft_model::Presentation;
use egui::{Align, Color32, CornerRadius, Layout as EguiLayout, RichText, Ui, UiBuilder, vec2};

use super::{Layout as RecordLayout, RecordView};
use crate::SlideApp;
use crate::panels::notes::{self as notes_panel, Look};
use crate::show::Show;

pub(super) const NOTES_DEFAULT: f32 = 18.0;
pub(super) const NOTES_MIN: f32 = 12.0;
pub(super) const NOTES_MAX: f32 = 48.0;
pub(super) const NOTES_STEP: f32 = 4.0;

/// Keep a requested notes size inside the pane's usable range.
pub(super) fn clamp_size(px: f32) -> f32 {
    if px.is_finite() { px.clamp(NOTES_MIN, NOTES_MAX) } else { NOTES_DEFAULT }
}

/// Draw the notes pane. `show` supplies the current slide index; `doc` its notes.
pub(super) fn ui(_app: &mut SlideApp, ui: &mut Ui, show: &mut Show, rec: &mut RecordView, doc: &Arc<Presentation>, layout: &RecordLayout) {
    let rect = layout.notes;
    if rect.height() < 1.0 || rec.view == super::views::ViewKind::Slide {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), Color32::from_rgb(26, 26, 30));
    painter.rect_stroke(rect, CornerRadius::same(8), egui::Stroke::new(1.0, Color32::from_gray(58)), egui::StrokeKind::Inside);

    let header = egui::Rect::from_min_size(rect.min + vec2(12.0, 6.0), vec2(rect.width() - 24.0, 24.0));
    let mut head = ui.new_child(UiBuilder::new().max_rect(header).layout(EguiLayout::left_to_right(Align::Center)));
    head.spacing_mut().item_spacing.x = 6.0;
    head.label(RichText::new("Notes").size(12.0).color(Color32::from_gray(170)));
    head.with_layout(EguiLayout::right_to_left(Align::Center), |ui| {
        if ui.add(egui::Button::new(RichText::new("A+").size(12.0)).frame(false)).clicked() {
            rec.set_notes_size(NOTES_STEP);
        }
        if ui.add(egui::Button::new(RichText::new("A−").size(12.0)).frame(false)).clicked() {
            rec.set_notes_size(-NOTES_STEP);
        }
    });

    let body_rect = egui::Rect::from_min_max(egui::pos2(rect.min.x + 12.0, header.max.y + 2.0), egui::pos2(rect.max.x - 12.0, rect.max.y - 8.0));
    let mut body = ui.new_child(UiBuilder::new().max_rect(body_rect));
    body.visuals_mut().override_text_color = Some(Color32::from_gray(224));
    let slide = doc.slides.get(show.state.slide);
    let empty = deckcraft_model::text::TextBody::default();
    let notes = slide.map_or(&empty, |s| &s.notes);
    let look = Look { size: rec.notes_px, color: Some(Color32::from_gray(224)) };
    let out = egui::ScrollArea::vertical().id_salt("record_notes").max_height(body_rect.height()).vertical_scroll_offset(rec.notes_scroll).show(
        &mut body,
        |ui| {
            if notes.text().is_empty() {
                ui.label(RichText::new("No notes for this slide").size(rec.notes_px).color(Color32::from_gray(120)));
            } else {
                notes_panel::view(ui, notes, &look);
            }
        },
    );
    rec.notes_scroll = out.state.clamped_offset().y;
}

/// Whether a notes editor owns the keyboard this frame. The shell skips show navigation while it
/// does.
///
/// Nothing in the Record view owns text input yet: the notes pane is read-only, so the show's
/// navigation keys must keep working even when a button holds focus. A notes editor that lands
/// later replaces this with its own id (`ctx.memory(|m| m.has_focus(id))`).
pub(super) fn has_focus(_ctx: &egui::Context) -> bool {
    false
}
