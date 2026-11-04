//! Record / Stop: the one button that starts and ends a take.
use egui::{RichText, Ui};

use super::super::{RecordStatus, RecordView};
use super::RED;
use crate::show::Show;

/// The button's face. Idle offers a new take; a running or paused take offers Stop, so a pause
/// never leaves the red button looking like it would start a second recording.
pub(super) fn label(status: RecordStatus) -> &'static str {
    match status {
        RecordStatus::Idle => "● Record",
        RecordStatus::Recording | RecordStatus::Paused => "■ Stop",
    }
}

/// What the button will do, for the hover text.
fn hint(status: RecordStatus) -> &'static str {
    match status {
        RecordStatus::Idle => "Start recording",
        RecordStatus::Recording | RecordStatus::Paused => "Stop recording",
    }
}

/// Draw Record/Stop. Returns whether the take changed state this frame.
pub(super) fn ui(ui: &mut Ui, show: &mut Show, rec: &mut RecordView, now: f64) -> bool {
    let button = ui.add(egui::Button::new(RichText::new(label(rec.status)).size(13.0).color(RED)).frame(false));
    // Screen readers must hear what the button does, not what it used to be called.
    let name = match rec.status {
        RecordStatus::Idle => "Record",
        RecordStatus::Recording | RecordStatus::Paused => "Stop",
    };
    button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    if button.on_hover_text(hint(rec.status)).clicked() {
        rec.toggle_record(show, now);
        return true;
    }
    false
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

    /// The face follows the capture cycle: Record while idle, Stop once a take exists.
    #[test]
    fn the_face_flips_with_the_status() {
        assert_eq!(label(RecordStatus::Idle), "● Record");
        assert_eq!(label(RecordStatus::Recording), "■ Stop");
        assert_eq!(label(RecordStatus::Paused), "■ Stop");
    }

    /// Idle → Recording marks the take captured; a second press ends it.
    #[test]
    fn one_button_starts_and_ends_a_take() {
        let mut show = show();
        let mut rec = RecordView::default();
        assert_eq!(rec.status, RecordStatus::Idle);

        rec.toggle_record(&mut show, 0.0);
        assert_eq!(rec.status, RecordStatus::Recording);
        assert!(rec.captured, "a started take is captured");

        rec.toggle_record(&mut show, 3.0);
        assert_eq!(rec.status, RecordStatus::Idle);
        assert!(rec.pending.is_none(), "Stop queues nothing by itself");
    }

    /// A paused take still stops: Stop must work from Paused, not only while Recording.
    #[test]
    fn stop_works_from_paused() {
        let mut show = show();
        let mut rec = RecordView::default();
        rec.start(&mut show, 0.0);
        rec.pause(&mut show, 5.0);
        assert_eq!(rec.status, RecordStatus::Paused);

        rec.toggle_record(&mut show, 6.0);
        assert_eq!(rec.status, RecordStatus::Idle);
    }
}
