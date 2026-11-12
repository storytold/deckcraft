//! The slide canvas decorations the Record view shell owns: the cameo bubble placeholder.
//! The slide itself is drawn by the show's own renderer into `Layout.slide`; the canvas section
//! agent owns bubble drag/resize and the live preview texture.

use egui::{Color32, CornerRadius, Stroke, Ui, vec2};

use super::{Layout, RecordView};

/// The camera bubble. Until a camera backend supplies frames this is a clearly empty placeholder:
/// a dashed ring and a camera glyph, never a fabricated live picture.
pub(super) fn bubble(ui: &Ui, rec: &RecordView, layout: &Layout) {
    if !rec.camera_bubble {
        return;
    }
    let rect = layout.camera;
    if rect.width() < 8.0 {
        return;
    }
    let painter = ui.painter();
    painter.circle_filled(rect.center(), rect.width() * 0.5, Color32::from_rgba_unmultiplied(20, 20, 24, 210));
    painter.circle_stroke(rect.center(), rect.width() * 0.5, Stroke::new(2.0, Color32::from_gray(110)));
    let r = rect.width() * 0.22;
    let body = egui::Rect::from_center_size(rect.center(), vec2(r * 2.0, r * 1.4));
    let body = body.translate(vec2(-r * 0.25, 0.0));
    painter.rect_filled(body, CornerRadius::same((r * 0.25) as u8), Color32::from_gray(150));
    let lens = egui::Rect::from_center_size(body.center(), egui::vec2(r * 0.8, r * 0.8));
    painter.circle_filled(lens.center(), r * 0.4, Color32::from_gray(60));
    painter.text(rect.center() + vec2(0.0, r * 1.25), egui::Align2::CENTER_CENTER, "Camera", crate::theme::font(10.0), Color32::from_gray(160));
}
