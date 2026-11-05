//! Bottom strip: slide navigation and the ink tools. The swipe/arrow keys still navigate through
//! the show itself; these are the pointer controls the Record view shows.
//!
//! Ink is the show's own pen: selecting Pen or Highlighter turns `show.set_pen` on, Arrow turns it
//! off. The richer ink model (highlighter width, eraser hit-testing, recorded stroke colour) is
//! the ink section's to extend; [`InkState`] is the shell's selection state.

use std::sync::Arc;

use deckcraft_model::Presentation;
use egui::{Align, Color32, CornerRadius, Layout as EguiLayout, RichText, Sense, Ui, UiBuilder, vec2};

use super::{InkTool, Layout, RecordView};
use crate::show::Show;
use crate::theme;

/// The ten ink colours offered by the bar. Index 0 is the deck's pen colour when it matches none.
pub(super) const INK_COLORS: [Color32; 10] = [
    Color32::from_rgb(20, 20, 20),
    Color32::from_rgb(120, 120, 120),
    Color32::from_rgb(200, 30, 40),
    Color32::from_rgb(230, 110, 30),
    Color32::from_rgb(240, 200, 40),
    Color32::from_rgb(40, 160, 80),
    Color32::from_rgb(30, 150, 210),
    Color32::from_rgb(50, 80, 200),
    Color32::from_rgb(140, 70, 200),
    Color32::from_rgb(235, 235, 235),
];

/// Draw the bottom strip. Returns the slide change the buttons asked for, for the shell to apply
/// once its own state is back on the show (the timings and the microphone take both need it);
/// a navigation that leaves the show's end does nothing and returns `None`.
pub(super) fn ui(ui: &mut Ui, show: &mut Show, rec: &mut RecordView, doc: &Arc<Presentation>, layout: &Layout) -> Option<super::super::ShowAction> {
    let rect = layout.bottom;
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(23, 23, 27));
    painter.rect_stroke(
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + 1.0)),
        CornerRadius::ZERO,
        egui::Stroke::new(1.0, Color32::from_gray(52)),
        egui::StrokeKind::Inside,
    );

    // Ink row: reserved in the middle-top of the strip.
    let ink_row = egui::Rect::from_min_max(egui::pos2(rect.min.x + 180.0, rect.min.y + 10.0), egui::pos2(rect.max.x - 180.0, rect.min.y + 42.0));
    let mut ink_ui = ui.new_child(UiBuilder::new().max_rect(ink_row));
    ink_ui.with_layout(EguiLayout::left_to_right(Align::Center), |ui| {
        ui.add_space(((ink_row.width() - 520.0) / 2.0).max(0.0));
        ui.spacing_mut().item_spacing.x = 4.0;
        for (tool, label) in [(InkTool::Pen, "Pen"), (InkTool::Highlighter, "Highlighter"), (InkTool::Eraser, "Eraser")] {
            let selected = rec.ink.tool == tool;
            let resp = ui.selectable_label(selected, RichText::new(label).size(12.0));
            if resp.clicked() {
                rec.ink.tool = if selected { InkTool::Arrow } else { tool };
                apply_ink(show, rec);
            }
        }
        ui.separator();
        for (i, color) in INK_COLORS.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
            let selected = rec.ink.color == i;
            let radius = if selected { 7.0 } else { 5.5 };
            let stroke = if selected { egui::Stroke::new(1.5, Color32::WHITE) } else { egui::Stroke::new(1.0, Color32::from_gray(90)) };
            painter.circle_filled(r.center(), radius, *color);
            painter.circle_stroke(r.center(), radius, stroke);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, selected, format!("Ink colour {}", i + 1)));
            if resp.clicked() {
                rec.ink.color = i;
                apply_ink(show, rec);
            }
        }
    });

    // Navigation row and progress.
    let nav_row = egui::Rect::from_min_max(egui::pos2(rect.min.x + 180.0, rect.min.y + 48.0), egui::pos2(rect.max.x - 180.0, rect.min.y + 82.0));
    let mut nav_ui = ui.new_child(UiBuilder::new().max_rect(nav_row));
    nav_ui.visuals_mut().override_text_color = Some(Color32::from_gray(225));
    let mut nav = None;
    nav_ui.with_layout(EguiLayout::left_to_right(Align::Center), |ui| {
        let total = doc.slides.len().max(1);
        ui.add_space(((nav_row.width() - 300.0) / 2.0).max(0.0));
        let prev = ui.add_enabled(show.state.slide > 0, egui::Button::new(RichText::new("‹").size(18.0)).frame(false));
        prev.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, show.state.slide > 0, "Previous slide"));
        if prev.clicked() {
            nav = Some(show.state.prev(doc));
        }
        ui.label(RichText::new(format!("Slide {} of {}", show.state.slide + 1, total)).size(12.0));
        let next = ui.add_enabled(show.state.slide + 1 < total, egui::Button::new(RichText::new("›").size(18.0)).frame(false));
        next.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, show.state.slide + 1 < total, "Next slide"));
        if next.clicked() {
            nav = Some(show.state.next(doc));
        }
    });

    // Progress line across the strip's bottom edge.
    let track = egui::Rect::from_min_size(egui::pos2(rect.min.x + 180.0, rect.max.y - 8.0), vec2((rect.width() - 360.0).max(1.0), 3.0));
    painter.rect_filled(track, CornerRadius::same(2), Color32::from_gray(52));
    let fraction = ((show.state.slide + 1) as f32 / doc.slides.len().max(1) as f32).clamp(0.0, 1.0);
    let filled = egui::Rect::from_min_size(track.min, vec2((track.width() * fraction).max(0.0), track.height()));
    painter.rect_filled(filled, CornerRadius::same(2), theme::Tokens::get(ui.ctx()).accent);
    nav
}

/// Map the shell's ink selection onto the show's pen. The ink section agent adds the highlighter
/// and eraser modes; until then both ink tools draw with the show's pen and Arrow leaves it.
fn apply_ink(show: &mut Show, rec: &mut RecordView) {
    show.set_pen(matches!(rec.ink.tool, InkTool::Pen | InkTool::Highlighter));
    rec.ink.active = show.pen;
}
