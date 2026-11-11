//! Read-only formatted script and the Teleprompter overlay's body.
//!
//! The script is drawn with the notes' formatting (bold, italics, bullets and indents) exactly as
//! the presenter's notes pane draws it; only the edit affordances are missing. The Teleprompter
//! overlay puts the same script on a translucent panel over the slide while a take is running.

use deckcraft_model::text::TextBody;
use egui::{Color32, CornerRadius, Rect, Response, RichText, Ui, UiBuilder};

use crate::panels::notes::{self as notes_panel, Look};

use super::teleprompter::State as TeleprompterState;
use super::{INSET, NOTES_DEFAULT, NOTES_MAX, NOTES_MIN};

/// Colour of the script, one step brighter than the editable pane so it reads over the slide.
const SCRIPT: Color32 = Color32::from_gray(240);

/// Colour of the empty state, dim enough not to look like a script.
const EMPTY: Color32 = Color32::from_gray(140);

/// The label shown when the slide has no notes.
pub(super) fn empty(ui: &mut Ui, px: f32) -> Response {
    ui.label(RichText::new("No notes for this slide").size(clamp_px(px)).color(EMPTY))
}

/// The formatted notes drawn read-only, wrapped to the available width.
///
/// An empty body shows [`empty`] instead of an empty galley, so a slide without notes still says
/// so in the pane and in the overlay.
pub(super) fn script(ui: &mut Ui, body: &TextBody, px: f32) -> Response {
    if body.is_empty() {
        return empty(ui, px);
    }
    notes_panel::view(ui, body, &Look { size: clamp_px(px), color: Some(SCRIPT) })
}

/// The Teleprompter script on a rounded dark panel, clipped to `rect`.
///
/// The offset comes from the overlay's auto-scroll state; a non-finite or negative offset reads
/// as the top of the script so a broken state can't scroll it out of view.
pub(super) fn overlay(ui: &mut Ui, rect: Rect, body: &TextBody, tele: &mut TeleprompterState, px: f32) {
    if !overlay_rect_is_usable(rect) {
        return;
    }
    let px = clamp_px(px);
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, CornerRadius::same(8), Color32::from_black_alpha(200));

    let inner = rect.shrink(INSET);
    if inner.width() < 1.0 || inner.height() < 1.0 {
        return;
    }
    let mut panel = ui.new_child(UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::LEFT)));
    panel.set_clip_rect(panel.clip_rect().intersect(rect));
    panel.label(RichText::new("Teleprompter").size((px * 0.6).clamp(10.0, 14.0)).color(Color32::from_gray(160)));

    let area = Rect::from_min_max(panel.available_rect_before_wrap().min, inner.max);
    if area.height() < 1.0 {
        return;
    }
    let mut scroller = panel.new_child(UiBuilder::new().max_rect(area));
    scroller.set_clip_rect(scroller.clip_rect().intersect(rect));
    let out = egui::ScrollArea::vertical()
        .id_salt("record_teleprompter")
        .max_height(area.height())
        .auto_shrink([false, false])
        .vertical_scroll_offset(sanitize(tele.offset))
        .show(&mut scroller, |ui| {
            script(ui, body, px);
        });
    // Keep the offset inside the rendered script: an unbounded one would be handed back to the
    // scroll area every frame and the text would sit outside the clipped panel.
    tele.offset = out.state.clamped_offset().y.max(0.0);
}

/// A rectangle big enough to hold a legible overlay, with finite bounds.
pub(super) fn overlay_rect_is_usable(rect: Rect) -> bool {
    rect.min.x.is_finite()
        && rect.min.y.is_finite()
        && rect.max.x.is_finite()
        && rect.max.y.is_finite()
        && rect.width() >= 1.0
        && rect.height() >= 1.0
}

/// The script size in pixels: the notes' limits, or the default for a broken size.
fn clamp_px(px: f32) -> f32 {
    if px.is_finite() { px.clamp(NOTES_MIN, NOTES_MAX) } else { NOTES_DEFAULT }
}

/// A scroll offset that can be handed to the scroll area.
fn sanitize(offset: f32) -> f32 {
    if offset.is_finite() && offset > 0.0 { offset } else { 0.0 }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use deckcraft_model::text::{Bullet, ParaProps, Paragraph, Run};
    use egui::{Context, pos2, vec2};

    use super::*;

    /// Notes with headings, bullets and plain lines, long enough to overflow the overlay.
    fn long_notes() -> TextBody {
        let bullet = || Bullet::Char { char: "•".into() };
        let props = || ParaProps { margin_left: Some(22.5), indent: Some(-22.5), bullet: Some(bullet()), ..Default::default() };
        let mut paragraphs = vec![Paragraph::new("Opening")];
        for i in 0..40 {
            paragraphs.push(Paragraph {
                level: 1,
                props: props(),
                runs: vec![Run::new(format!("Point {i} of a long script that keeps going well past the overlay."))],
                ..Default::default()
            });
        }
        paragraphs.push(Paragraph::new("Closing"));
        TextBody { paragraphs, ..Default::default() }
    }

    #[test]
    fn sizes_are_clamped_and_broken_ones_fall_back() {
        assert_eq!(clamp_px(24.0), 24.0);
        assert_eq!(clamp_px(-4.0), NOTES_MIN);
        assert_eq!(clamp_px(1.0e9), NOTES_MAX);
        assert_eq!(clamp_px(f32::NAN), NOTES_DEFAULT);
        assert_eq!(clamp_px(f32::INFINITY), NOTES_DEFAULT);
    }

    #[test]
    fn offsets_are_sanitized() {
        assert_eq!(sanitize(42.0), 42.0);
        assert_eq!(sanitize(0.0), 0.0);
        assert_eq!(sanitize(-3.0), 0.0);
        assert_eq!(sanitize(f32::NAN), 0.0);
        assert_eq!(sanitize(f32::NEG_INFINITY), 0.0);
    }

    #[test]
    fn unusable_overlay_rects_are_rejected() {
        assert!(!overlay_rect_is_usable(Rect::from_min_size(pos2(10.0, 10.0), vec2(0.0, 0.0))));
        assert!(!overlay_rect_is_usable(Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(200.0, 100.0))));
        assert!(overlay_rect_is_usable(Rect::from_min_size(pos2(10.0, 10.0), vec2(200.0, 100.0))));
    }

    #[test]
    fn script_lays_out_long_formatted_notes_and_empty_state_labels() {
        let ctx = Context::default();
        let body = long_notes();
        let mut laid = None;
        let mut empty_rect = None;
        let mut out = ctx.run_ui(Default::default(), |ui| {
            laid = Some(script(ui, &body, 64.0));
            empty_rect = Some(empty(ui, 64.0).rect);
        });
        out.textures_delta.clear();
        let laid = laid.unwrap();
        assert!(laid.rect.height() > 12.0, "the script lays out: {:?}", laid.rect);
        assert!(!out.shapes.is_empty(), "the script paints");
        let empty_rect = empty_rect.unwrap();
        assert!(empty_rect.height() > 0.0, "the empty state draws a label: {empty_rect:?}");
    }

    #[test]
    fn overlay_draws_and_ignores_zero_rects() {
        let ctx = Context::default();
        let body = long_notes();
        let mut tele = TeleprompterState::default();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            overlay(ui, Rect::from_min_size(pos2(0.0, 0.0), vec2(320.0, 180.0)), &body, &mut tele, 18.0);
        });
        out.textures_delta.clear();
        assert!(!out.shapes.is_empty(), "the overlay paints its panel and script");

        // A pass that draws nothing is the baseline; the rejected rect must add no shapes to it.
        let mut idle = Context::default().run_ui(Default::default(), |_| {});
        idle.textures_delta.clear();
        let mut out = Context::default().run_ui(Default::default(), |ui| {
            overlay(ui, Rect::from_min_size(pos2(0.0, 0.0), vec2(0.0, 0.0)), &body, &mut tele, 18.0);
        });
        out.textures_delta.clear();
        assert_eq!(out.shapes.len(), idle.shapes.len(), "a zero rect paints nothing");
    }
}
