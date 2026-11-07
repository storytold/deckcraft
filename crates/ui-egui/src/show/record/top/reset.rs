//! Reset zeroes the capture clock; Undo puts back the value Reset replaced.
use egui::Ui;

use super::super::RecordView;
use super::button;

/// Whether the clock holds a value Reset replaced, so Undo can put it back.
pub(super) fn undo_enabled(rec: &RecordView) -> bool {
    rec.undo_elapsed.is_some()
}

/// Draw Reset and Undo. Returns whether either was clicked this frame.
pub(super) fn ui(ui: &mut Ui, rec: &mut RecordView, now: f64) -> bool {
    let mut clicked = false;

    if button(ui, "Reset", true).on_hover_text("Reset the capture clock").clicked() {
        rec.reset(now);
        clicked = true;
    }

    // Undo is only live while a Reset value is stored; a second Undo has nothing to put back.
    let undo = ui
        .add_enabled(undo_enabled(rec), egui::Button::new(egui::RichText::new("Undo").size(13.0)).frame(false))
        .on_hover_text("Restore the clock from before Reset");
    if undo.clicked() {
        rec.undo(now);
        clicked = true;
    }

    clicked
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use crate::show::Show;

    use super::*;

    /// A restored clock stays frozen unless a take is running: an idle view must not count up
    /// between Reset and Undo.
    #[test]
    fn undo_off_a_take_leaves_the_clock_frozen() {
        let mut rec = RecordView::default();
        let doc = deckcraft_model::Presentation::default();
        let mut show = Show::new(&doc, 0, false, false);
        rec.start(&mut show, 0.0);
        rec.reset(10.0);
        rec.stop(&mut show, 10.0);
        assert!(rec.undo(10.0));
        assert_eq!(rec.elapsed(20.0), rec.elapsed(10.0), "nothing records, so nothing counts");
    }

    #[test]
    fn fresh_view_has_nothing_to_undo() {
        let rec = RecordView::default();
        assert!(!undo_enabled(&rec));
    }

    #[test]
    fn reset_zeroes_the_clock_and_offers_undo() {
        let mut rec = RecordView::default();
        let doc = deckcraft_model::Presentation::default();
        let mut show = Show::new(&doc, 0, false, false);
        rec.start(&mut show, 0.0);
        assert!(rec.elapsed(10.0) > 9.9);

        rec.reset(10.0);
        assert!(undo_enabled(&rec));
        assert_eq!(rec.elapsed(10.0), 0.0);
    }

    #[test]
    fn undo_restores_the_pre_reset_value_once() {
        let mut rec = RecordView::default();
        let doc = deckcraft_model::Presentation::default();
        let mut show = Show::new(&doc, 0, false, false);
        rec.start(&mut show, 0.0);
        rec.reset(10.0);

        assert!(rec.undo(10.0));
        assert!((rec.elapsed(10.0) - 10.0).abs() < 1e-6);
        assert!(!undo_enabled(&rec));

        assert!(!rec.undo(10.0));
    }
}
