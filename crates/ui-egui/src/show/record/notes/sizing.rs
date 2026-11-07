//! Text size of the notes/script: the A-/A+ controls, the limits and the keyboard shortcuts.
//!
//! The header draws the pane's title, a live sample of the current size and the A-/A+ buttons;
//! Ctrl+- and Ctrl++ (Cmd on a Mac) are the same two actions from the keyboard, and Ctrl+0 puts
//! the script back to [`NOTES_DEFAULT`]. The size is stored on [`RecordView::notes_px`] because
//! the read-only script, the editor and the Teleprompter overlay all read it.

use egui::{Align, Color32, Layout as EguiLayout, Rect, RichText, Ui, UiBuilder};

use super::super::views::ViewKind;
use super::RecordView;

pub(crate) const NOTES_DEFAULT: f32 = 18.0;
pub(crate) const NOTES_MIN: f32 = 12.0;
pub(crate) const NOTES_MAX: f32 = 48.0;
pub(crate) const NOTES_STEP: f32 = 4.0;

/// Height of the header row, from the pane's top edge to the body.
pub(super) const HEADER_H: f32 = 24.0;

const TITLE: Color32 = Color32::from_gray(170);
const SAMPLE: Color32 = Color32::from_gray(200);

/// Keep a requested notes size inside the pane's usable range.
///
/// A non-finite request has no meaningful bound, so it falls back to [`NOTES_DEFAULT`].
pub(crate) fn clamp_size(px: f32) -> f32 {
    if px.is_finite() { px.clamp(NOTES_MIN, NOTES_MAX) } else { NOTES_DEFAULT }
}

/// The size after a step, clamped. A non-finite step leaves the current size alone (still
/// clamped), so a bad delta cannot reset the script to the default size.
pub(crate) fn size_after(px: f32, delta: f32) -> f32 {
    if delta.is_finite() { clamp_size(px + delta) } else { clamp_size(px) }
}

/// The sample size shown between the A-/A+ buttons: whole points, rounded.
fn sample_label(px: f32) -> String {
    format!("{:.0}", clamp_size(px).round())
}

/// Apply the keyboard size shortcuts. A focused script editor keeps every key for typing.
///
/// Ctrl+=, Ctrl++ and Ctrl+- step the size; Ctrl+0 returns to [`NOTES_DEFAULT`]. Cmd works the
/// same way on a Mac. Plain keys are never shortcuts.
pub(super) fn shortcuts(ctx: &egui::Context, rec: &mut RecordView) {
    if super::edit::has_focus(ctx) {
        return;
    }
    let mut delta = 0.0;
    let mut reset = false;
    ctx.input_mut(|input| {
        if consume_shortcut(input, egui::Key::Equals) || consume_shortcut(input, egui::Key::Plus) {
            delta += NOTES_STEP;
        } else if consume_shortcut(input, egui::Key::Minus) {
            delta -= NOTES_STEP;
        }
        reset = consume_shortcut(input, egui::Key::Num0);
    });
    if reset {
        rec.notes_px = NOTES_DEFAULT;
    } else if delta != 0.0 {
        rec.notes_px = size_after(rec.notes_px, delta);
    }
}

/// Whether `key` was pressed with Ctrl or the Mac Command key, consuming it.
fn consume_shortcut(input: &mut egui::InputState, key: egui::Key) -> bool {
    input.consume_key(egui::Modifiers::CTRL, key) || input.consume_key(egui::Modifiers::COMMAND, key)
}

/// Draw the pane's header and return the body rect under it.
///
/// The pane background is already painted by the caller; this only adds the title on the left and
/// the A-/A+ group on the right, followed by the Teleprompter's controls when that view is shown.
pub(super) fn header(ui: &mut Ui, rec: &mut RecordView, rect: Rect) -> Rect {
    let row_rect =
        Rect::from_min_max(egui::pos2(rect.min.x + super::INSET, rect.min.y + 2.0), egui::pos2(rect.max.x - super::INSET, rect.min.y + HEADER_H));
    let mut row = ui.new_child(UiBuilder::new().max_rect(row_rect).layout(EguiLayout::left_to_right(Align::Center)));
    row.spacing_mut().item_spacing.x = 4.0;
    row.label(RichText::new("Notes").size(12.0).color(TITLE));

    row.with_layout(EguiLayout::right_to_left(Align::Center), |row| {
        if row
            .add(egui::Button::new(RichText::new("A+").size(12.0).color(SAMPLE)).frame(false).min_size(super::CONTROL))
            .on_hover_text("Larger notes text")
            .clicked()
        {
            rec.set_notes_size(NOTES_STEP);
        }
        row.label(RichText::new(sample_label(rec.notes_px)).size(12.0).color(SAMPLE));
        if row
            .add(egui::Button::new(RichText::new("A−").size(12.0).color(SAMPLE)).frame(false).min_size(super::CONTROL))
            .on_hover_text("Smaller notes text")
            .clicked()
        {
            rec.set_notes_size(-NOTES_STEP);
        }
        if rec.view == ViewKind::Teleprompter {
            super::teleprompter::header_controls(row, rec);
        }
    });

    super::body_rect(rect, HEADER_H)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn clamp_keeps_sizes_inside_the_range() {
        assert_eq!(clamp_size(NOTES_MIN - 10.0), NOTES_MIN);
        assert_eq!(clamp_size(NOTES_MAX + 10.0), NOTES_MAX);
        assert_eq!(clamp_size(21.0), 21.0);
    }

    #[test]
    fn clamp_falls_back_for_non_finite_sizes() {
        assert_eq!(clamp_size(f32::NAN), NOTES_DEFAULT);
        assert_eq!(clamp_size(f32::INFINITY), NOTES_DEFAULT);
        assert_eq!(clamp_size(f32::NEG_INFINITY), NOTES_DEFAULT);
    }

    #[test]
    fn size_after_clamps_an_overshooting_step() {
        assert_eq!(size_after(NOTES_MAX, NOTES_STEP), NOTES_MAX);
        assert_eq!(size_after(NOTES_MIN, -NOTES_STEP), NOTES_MIN);
        assert_eq!(size_after(20.0, NOTES_STEP), 24.0);
    }

    #[test]
    fn size_after_ignores_a_non_finite_step() {
        assert_eq!(size_after(30.0, f32::NAN), 30.0);
        assert_eq!(size_after(30.0, f32::INFINITY), 30.0);
        assert_eq!(size_after(f32::NAN, 4.0), NOTES_DEFAULT);
    }

    #[test]
    fn sample_label_rounds_to_whole_points() {
        assert_eq!(sample_label(18.0), "18");
        assert_eq!(sample_label(17.6), "18");
        assert_eq!(sample_label(17.4), "17");
        assert_eq!(sample_label(f32::NAN), "18");
    }
}
