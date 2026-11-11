//! Export entry: records an explicit request for the video export pipeline.
//!
//! The shell has no export pipeline, so this never reports that the export happened: it only
//! leaves [`RecordCommand::Export`] on the view for the section that eventually owns it.

use egui::Ui;

use super::super::{RecordCommand, RecordView};
use super::button;

/// Queue the export request on the view. Returns whether the request was recorded.
pub(super) fn request(rec: &mut RecordView) -> bool {
    rec.pending = Some(RecordCommand::Export);
    true
}

/// Draw the Export button. Returns true when it was clicked and the request was queued.
pub(super) fn ui(ui: &mut Ui, rec: &mut RecordView) -> bool {
    let clicked = button(ui, "Export ›", true).on_hover_text("Export the finished recording (the export pipeline is not wired yet)").clicked();
    clicked && request(rec)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::show::record::RecordStatus;

    #[test]
    fn request_queues_export_and_changes_nothing_else() {
        let mut rec = RecordView::default();
        assert_eq!(rec.status, RecordStatus::Idle);
        assert!(!rec.captured);
        assert!(!rec.close_confirm);
        assert_eq!(rec.elapsed(12.5), 0.0);
        assert_eq!(rec.total_seconds(), 0.0);

        assert!(request(&mut rec));

        assert_eq!(rec.pending, Some(RecordCommand::Export));
        assert_eq!(rec.status, RecordStatus::Idle);
        assert!(!rec.captured);
        assert!(!rec.close_confirm);
        assert_eq!(rec.elapsed(12.5), 0.0);
        assert_eq!(rec.total_seconds(), 0.0);
    }

    #[test]
    fn request_during_a_take_keeps_the_take_state() {
        let mut rec = RecordView { status: RecordStatus::Recording, captured: true, ..Default::default() };

        assert!(request(&mut rec));

        assert_eq!(rec.pending, Some(RecordCommand::Export));
        assert_eq!(rec.status, RecordStatus::Recording);
        assert!(rec.captured);
    }
}
