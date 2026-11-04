//! The capture clock: elapsed against the deck's timed length.
use egui::{Align2, Color32, Painter, Rect, pos2};

use super::super::{RecordStatus, RecordView, clock_total};
use super::{DIM, FG, RED};

/// `mm:ss / mm:ss`, e.g. `00:07 / 02:05`.
pub(super) fn text(rec: &RecordView, now: f64) -> String {
    clock_total(rec.elapsed(now), rec.total_seconds())
}

/// The dot's colour for the current state: dim when idle, red while recording, amber when paused.
pub(super) fn dot_color(status: RecordStatus) -> Color32 {
    match status {
        RecordStatus::Idle => DIM,
        RecordStatus::Recording => RED,
        RecordStatus::Paused => Color32::from_rgb(230, 168, 58),
    }
}

/// Paint the status dot and the clock inside the bar. `rect` is the whole floating bar.
pub(super) fn paint(painter: &Painter, rect: Rect, rec: &RecordView, now: f64) {
    let center = rect.center();
    painter.circle_filled(pos2(center.x - 66.0, center.y), 4.0, dot_color(rec.status));
    painter.text(pos2(center.x - 54.0, center.y), Align2::LEFT_CENTER, text(rec, now), crate::theme::font(14.0), FG);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::show::Show;
    use crate::show::presenter::PresenterTimer;
    use deckcraft_engine::Session;

    fn show() -> Show {
        let session = Session::with_new();
        let doc = session.doc().unwrap().doc.clone();
        Show::new(&doc, 0, false, false)
    }

    #[test]
    fn a_fresh_view_shows_zero_against_zero() {
        let rec = RecordView::default();
        assert_eq!(text(&rec, 0.0), "00:00 / 00:00");
    }

    #[test]
    fn the_clock_reads_elapsed_against_the_deck_length() {
        let mut show = show();
        let mut rec = RecordView { total: 125.0, ..Default::default() };
        rec.start(&mut show, 0.0);

        assert_eq!(text(&rec, 65.0), "01:05 / 02:05");
        assert_eq!(text(&rec, 0.0), "00:00 / 02:05");
    }

    #[test]
    fn long_lengths_fold_hours_into_minutes() {
        let rec = RecordView { total: 3661.0, ..Default::default() };

        assert_eq!(text(&rec, 0.0), "00:00 / 61:01");
    }

    #[test]
    fn a_negative_or_future_clock_never_prints_a_negative() {
        let rec = RecordView { timer: PresenterTimer::start(0.0), ..Default::default() };

        assert_eq!(text(&rec, -5.0), "00:00 / 00:00");
        assert_eq!(text(&rec, f64::NAN), "00:00 / 00:00");
    }

    #[test]
    fn an_invalid_length_reads_as_zero() {
        let mut rec = RecordView { total: -5.0, ..Default::default() };

        assert_eq!(text(&rec, 0.0), "00:00 / 00:00");

        rec.total = f64::NAN;
        assert_eq!(text(&rec, 0.0), "00:00 / 00:00");
    }

    #[test]
    fn the_dot_colour_follows_the_state() {
        assert_eq!(dot_color(RecordStatus::Idle), DIM);
        assert_eq!(dot_color(RecordStatus::Recording), RED);
        assert_eq!(dot_color(RecordStatus::Paused), Color32::from_rgb(230, 168, 58));
    }

    #[test]
    fn the_dot_is_grey_when_idle_red_when_recording_and_amber_when_paused() {
        // Pin the palette so a state can never silently borrow its neighbour's colour.
        assert_eq!(dot_color(RecordStatus::Idle), Color32::from_gray(150));
        assert_eq!(dot_color(RecordStatus::Recording), Color32::from_rgb(224, 62, 62));
        assert_ne!(dot_color(RecordStatus::Idle), dot_color(RecordStatus::Paused));
        assert_ne!(dot_color(RecordStatus::Recording), dot_color(RecordStatus::Paused));
    }
}
