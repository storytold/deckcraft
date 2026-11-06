//! The More menu: view toggles and the clock commands that do not fit the bar.
use egui::{RichText, Ui};

use super::super::RecordView;
use super::button;

/// Whether the clock can be put back after a reset. Only a reset leaves a snapshot to undo.
pub(super) fn can_undo(rec: &RecordView) -> bool {
    rec.undo_elapsed.is_some()
}

/// Draw the `…` button and its menu. Returns true when an item actually changed something.
pub(super) fn ui(ui: &mut Ui, rec: &mut RecordView, now: f64) -> bool {
    let more = button(ui, "…", true);
    more.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "More"));
    let more = more.on_hover_text("More recording options");

    let shown = egui::Popup::menu(&more).width(220.0).show(|ui| {
        let mut changed = false;
        // Slide view has no notes area, so the switch would toggle nothing.
        let notes = ui.add_enabled(
            rec.view != super::super::views::ViewKind::Slide,
            egui::Checkbox::new(&mut rec.notes_open, RichText::new("Notes pane").size(13.0)),
        );
        changed |= notes.changed();
        changed |= ui.checkbox(&mut rec.camera_bubble, RichText::new("Camera bubble").size(13.0)).changed();
        // No camera surface in the Record view yet: the switch reads as unavailable.
        let camera = ui.add_enabled(false, egui::Checkbox::new(&mut rec.camera_preview, RichText::new("Camera preview").size(13.0)));
        let _ = camera.on_hover_text("The Record view's camera preview arrives with the camera section");
        ui.separator();
        let undo = egui::Button::new(RichText::new("Undo reset").size(13.0)).frame(false);
        if ui.add_enabled(can_undo(rec), undo).clicked() {
            changed |= rec.undo(now);
        }
        let reset = egui::Button::new(RichText::new("Reset clock").size(13.0)).frame(false);
        if ui.add(reset).clicked() {
            rec.reset(now);
            changed = true;
        }
        ui.separator();
        // Nothing owns recording settings yet, so the entry is present but inert.
        ui.add_enabled(false, egui::Button::new(RichText::new("Recording settings…").size(13.0)));
        changed
    });

    rec.more_open = shown.is_some();
    shown.map(|shown| shown.inner).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn nothing_to_undo_on_a_fresh_view() {
        let rec = RecordView::default();
        assert!(!can_undo(&rec));
    }

    #[test]
    fn reset_arms_undo_and_undo_disarms_it() {
        let mut rec = RecordView::default();
        rec.reset(1.0);
        assert!(can_undo(&rec));
        assert!(rec.undo(2.0), "the reset snapshot is consumed by an undo");
        assert!(!can_undo(&rec));
        assert!(!rec.undo(3.0), "a second undo has nothing left to restore");
    }

    #[test]
    fn undo_before_reset_does_nothing() {
        let mut rec = RecordView::default();
        assert!(!rec.undo(1.0));
        assert!(!can_undo(&rec));
    }
}
