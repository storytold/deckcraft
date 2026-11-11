//! Pause and resume the capture without ending the take.
use egui::{RichText, Ui};

use super::super::{RecordStatus, RecordView};
use crate::show::Show;

/// The button's face. A paused take offers Resume; otherwise the button pauses.
pub(super) fn label(status: RecordStatus) -> &'static str {
    match status {
        RecordStatus::Paused => "Resume",
        RecordStatus::Idle | RecordStatus::Recording => "Pause",
    }
}

/// Pause needs a running or paused take; an idle view has nothing to hold.
pub(super) fn enabled(status: RecordStatus) -> bool {
    status != RecordStatus::Idle
}

/// Draw Pause/Resume. Returns whether the take changed state this frame.
pub(super) fn ui(ui: &mut Ui, show: &mut Show, rec: &mut RecordView, now: f64) -> bool {
    let button = ui.add_enabled(enabled(rec.status), egui::Button::new(RichText::new(label(rec.status)).size(13.0)).frame(false));
    if !button.clicked() {
        return false;
    }
    match rec.status {
        RecordStatus::Recording => rec.pause(show, now),
        RecordStatus::Paused => rec.resume(show, now),
        RecordStatus::Idle => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn show() -> Show {
        let session = deckcraft_engine::Session::with_new();
        let doc = session.active().unwrap().doc.clone();
        Show::new(&doc, 0, false, false)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// The face follows the capture cycle: Pause until the take is held, then Resume.
    #[test]
    fn the_face_flips_with_the_status() {
        assert_eq!(label(RecordStatus::Idle), "Pause");
        assert_eq!(label(RecordStatus::Recording), "Pause");
        assert_eq!(label(RecordStatus::Paused), "Resume");
    }

    /// Only a take in progress can be paused or resumed.
    #[test]
    fn only_a_take_can_be_paused() {
        assert!(!enabled(RecordStatus::Idle));
        assert!(enabled(RecordStatus::Recording));
        assert!(enabled(RecordStatus::Paused));
    }

    /// Pausing holds the recorded time; resuming continues from there.
    #[test]
    fn pause_holds_the_clock_and_resume_continues_it() {
        let mut show = show();
        let mut rec = RecordView::default();
        rec.start(&mut show, 0.0);
        assert_eq!(rec.status, RecordStatus::Recording);

        rec.pause(&mut show, 5.0);
        assert_eq!(rec.status, RecordStatus::Paused);
        assert!(close(rec.elapsed(20.0), 5.0), "a paused take keeps its time");

        rec.resume(&mut show, 20.0);
        assert_eq!(rec.status, RecordStatus::Recording);
        assert!(close(rec.elapsed(25.0), 10.0), "resuming continues from the held time");
    }

    /// Pause and Resume are no-ops while the view is idle.
    #[test]
    fn pause_and_resume_do_nothing_when_idle() {
        let mut show = show();
        let mut rec = RecordView::default();
        assert_eq!(rec.status, RecordStatus::Idle);

        rec.pause(&mut show, 3.0);
        assert_eq!(rec.status, RecordStatus::Idle);
        rec.resume(&mut show, 4.0);
        assert_eq!(rec.status, RecordStatus::Idle);
    }
}
