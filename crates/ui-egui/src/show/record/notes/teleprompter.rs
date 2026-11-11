//! The Teleprompter view: an auto-scrolling read-only script over the slide while a take runs.
//!
//! The script scrolls at a steady pace measured in points per second. [`State`] keeps the scroll
//! offset, the pace and two switches: playing pauses the scroll without pausing the take, and the
//! overlay decides whether the script is drawn on top of the slide or only in the notes pane.
//! [`tick`] advances the offset once per frame while the take records, and [`header_controls`]
//! draws the compact controls in the notes pane's header.

use egui::{Rect, RichText, Ui, pos2};

use super::super::RecordStatus;
use super::super::views::ViewKind;
use super::{CONTROL, RecordLayout, RecordView};

/// The pace a fresh teleprompter scrolls at, in points per second.
pub(super) const SPEED_DEFAULT: f32 = 24.0;
/// The slowest pace the controls allow.
pub(super) const SPEED_MIN: f32 = 4.0;
/// The fastest pace the controls allow.
pub(super) const SPEED_MAX: f32 = 120.0;
/// How much one click of the slower/faster buttons changes the pace.
pub(super) const SPEED_STEP: f32 = 4.0;

/// The teleprompter's scroll, pace and visibility switches.
pub(crate) struct State {
    /// The script's scroll offset in points.
    pub(super) offset: f32,
    /// The scroll pace in points per second.
    pub(super) speed: f32,
    /// The script is drawn on top of the slide, not only in the notes pane.
    pub(super) overlay: bool,
    /// The scroll is running. Pausing keeps the offset and leaves the take recording.
    pub(super) playing: bool,
    /// The input clock time of the previous tick, used to measure the frame's delta.
    last: f64,
}

impl Default for State {
    fn default() -> Self {
        State { offset: 0.0, speed: SPEED_DEFAULT, overlay: true, playing: true, last: 0.0 }
    }
}

impl State {
    /// Back to the top of the script, keeping the pace, the overlay switch and whether the scroll
    /// is playing: a slide change restarts where the reading starts, not the user's settings.
    pub(super) fn rewind(&mut self) {
        self.offset = 0.0;
    }
}

/// Keep a requested pace inside the teleprompter's range.
pub(super) fn clamp_speed(speed: f32) -> f32 {
    if speed.is_finite() { speed.clamp(SPEED_MIN, SPEED_MAX) } else { SPEED_DEFAULT }
}

/// Move the scroll offset by one frame's worth of the pace.
///
/// `dt` is the frame's elapsed time in seconds. A paused teleprompter, a clock that did not
/// advance and a broken frame all leave the offset where it was; the result is always a finite,
/// non-negative offset.
pub(super) fn advance(offset: f32, speed: f32, dt: f64, playing: bool) -> f32 {
    let offset = if offset.is_finite() { offset.max(0.0) } else { 0.0 };
    if !playing || !dt.is_finite() || dt <= 0.0 {
        return offset;
    }
    let step = f64::from(clamp_speed(speed)) * dt;
    let next = (f64::from(offset) + step) as f32;
    if next.is_finite() && next >= 0.0 { next } else { offset }
}

/// Advance the teleprompter while it is the visible view of a recording take.
///
/// The frame delta is clamped to a tenth of a second so a stalled window does not push the script
/// past a paragraph, and the first frame of a take only establishes the clock. Outside a running
/// take the clock reference is forgotten, so resuming starts from the current offset rather than
/// jumping over the time the take was idle.
pub(super) fn tick(ctx: &egui::Context, rec: &mut RecordView) {
    let running = rec.view == ViewKind::Teleprompter && rec.status == RecordStatus::Recording;
    let state = &mut rec.teleprompter;
    if !running {
        state.last = 0.0;
        return;
    }
    let now = ctx.input(|i| i.time);
    let dt = if state.last > 0.0 { (now - state.last).clamp(0.0, 0.1) } else { 0.0 };
    state.last = now;
    if state.playing {
        state.offset = advance(state.offset, state.speed, dt, true);
        // The scroll moves once per rendered frame: without this it advances in the record bar's
        // 10 Hz steps and looks choppy.
        ctx.request_repaint();
    }
}

/// Draw the teleprompter's compact controls into the notes pane's header.
///
/// The controls are only relevant to the Teleprompter view, so the other views keep the header
/// for the A-/A+ pair alone.
pub(super) fn header_controls(ui: &mut Ui, rec: &mut RecordView) {
    if rec.view != ViewKind::Teleprompter {
        return;
    }
    let state = &mut rec.teleprompter;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (text, label) = if state.playing { ("Pause", "Pause teleprompter") } else { ("Play", "Play teleprompter") };
        let play = ui.add(egui::Button::new(RichText::new(text).size(12.0)).frame(false).min_size(CONTROL));
        play.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        if play.on_hover_text(label).clicked() {
            state.playing = !state.playing;
        }
        let slower = ui.add(egui::Button::new(RichText::new("−").size(12.0)).frame(false).min_size(CONTROL));
        slower.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Teleprompter slower"));
        if slower.on_hover_text("Teleprompter slower").clicked() {
            state.speed = clamp_speed(state.speed - SPEED_STEP);
        }
        ui.label(RichText::new(format!("{:.0}", state.speed)).size(11.0)).on_hover_text("Teleprompter speed, in points per second");
        let faster = ui.add(egui::Button::new(RichText::new("+").size(12.0)).frame(false).min_size(CONTROL));
        faster.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Teleprompter faster"));
        if faster.on_hover_text("Teleprompter faster").clicked() {
            state.speed = clamp_speed(state.speed + SPEED_STEP);
        }
        ui.checkbox(&mut state.overlay, RichText::new("Teleprompter overlay").size(11.0))
            .on_hover_text("Show the script over the slide while recording");
    });
}

/// The band the teleprompter overlay covers at the bottom of the slide.
///
/// The band is the slide's bottom 38%, between 120 and 300 points tall, inset from the slide's
/// edges. A slide too small to hold a readable script gets [`Rect::NOTHING`], and a slide with
/// broken geometry keeps the overlay off the screen.
pub(super) fn overlay_rect(layout: &RecordLayout) -> Rect {
    const INSET: f32 = 18.0;
    let slide = layout.slide;
    let finite = slide.min.x.is_finite() && slide.min.y.is_finite() && slide.max.x.is_finite() && slide.max.y.is_finite();
    if !finite || slide.width() < 200.0 || slide.height() < 160.0 {
        return Rect::NOTHING;
    }
    let inner = slide.shrink(INSET);
    let height = (slide.height() * 0.38).clamp(120.0, 300.0);
    let top = (slide.max.y - INSET - height).max(inner.min.y);
    Rect::from_min_max(pos2(inner.min.x, top), pos2(inner.max.x, slide.max.y - INSET))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn layout_with(slide: Rect) -> RecordLayout {
        RecordLayout {
            full: Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(1280.0, 800.0)),
            top: Rect::NOTHING,
            notes: Rect::NOTHING,
            slide,
            bottom: Rect::NOTHING,
            camera: Rect::NOTHING,
            views: Rect::NOTHING,
            camera_preview: Rect::NOTHING,
            next: None,
        }
    }

    fn view(view: ViewKind, status: RecordStatus) -> RecordView {
        RecordView { view, status, ..Default::default() }
    }

    #[test]
    fn the_pace_is_kept_inside_its_range() {
        assert!((clamp_speed(SPEED_MIN - 10.0) - SPEED_MIN).abs() < f32::EPSILON);
        assert!((clamp_speed(SPEED_MAX + 10.0) - SPEED_MAX).abs() < f32::EPSILON);
        assert!((clamp_speed(SPEED_DEFAULT) - SPEED_DEFAULT).abs() < f32::EPSILON);
        assert!((clamp_speed(f32::NAN) - SPEED_DEFAULT).abs() < f32::EPSILON);
        assert!((clamp_speed(f32::INFINITY) - SPEED_DEFAULT).abs() < f32::EPSILON);
        assert!((clamp_speed(f32::NEG_INFINITY) - SPEED_DEFAULT).abs() < f32::EPSILON);
    }

    #[test]
    fn advancing_moves_only_a_playing_script_on_a_live_clock() {
        assert!((advance(10.0, 24.0, 0.5, true) - 22.0).abs() < 1e-4);
        assert!((advance(10.0, 24.0, 0.5, false) - 10.0).abs() < 1e-4, "a paused script stays put");
        assert!((advance(10.0, 24.0, -1.0, true) - 10.0).abs() < 1e-4, "time cannot run backwards");
        assert!((advance(10.0, 24.0, f64::NAN, true) - 10.0).abs() < 1e-4, "a broken clock does nothing");
        assert_eq!(advance(f32::NAN, 24.0, 0.5, true), 12.0, "a broken offset restarts at the top");
        assert_eq!(advance(-5.0, 24.0, 0.5, true), 12.0, "a negative offset restarts at the top");
        assert_eq!(advance(0.0, f32::INFINITY, 0.5, true), SPEED_DEFAULT * 0.5, "a broken pace falls back");
        assert_eq!(advance(0.0, 24.0, 0.0, true), 0.0);
    }

    #[test]
    fn ticking_follows_the_view_and_the_take() {
        let ctx = egui::Context::default();

        // Outside the Teleprompter view, or while the take is not recording, nothing moves.
        let mut rec = view(ViewKind::Presenter, RecordStatus::Recording);
        rec.teleprompter.offset = 12.0;
        tick(&ctx, &mut rec);
        assert_eq!(rec.teleprompter.offset, 12.0);
        assert_eq!(rec.teleprompter.last, 0.0);

        let mut rec = view(ViewKind::Teleprompter, RecordStatus::Paused);
        rec.teleprompter.offset = 12.0;
        tick(&ctx, &mut rec);
        assert_eq!(rec.teleprompter.offset, 12.0);
        assert_eq!(rec.teleprompter.last, 0.0, "a paused take forgets the clock reference");

        // The first recorded frame only establishes the clock; the next frame scrolls.
        let mut rec = view(ViewKind::Teleprompter, RecordStatus::Recording);
        ctx.input_mut(|i| i.time = 1.0);
        tick(&ctx, &mut rec);
        assert_eq!(rec.teleprompter.offset, 0.0);
        ctx.input_mut(|i| i.time = 1.2);
        tick(&ctx, &mut rec);
        assert!((rec.teleprompter.offset - 2.4).abs() < 1e-3, "a 0.2s frame is clamped to 0.1s of scrolling");

        // Pausing the scroll keeps the offset; resuming the take does not jump over the pause.
        rec.teleprompter.playing = false;
        ctx.input_mut(|i| i.time = 1.4);
        tick(&ctx, &mut rec);
        assert!((rec.teleprompter.offset - 2.4).abs() < 1e-3);
        rec.status = RecordStatus::Paused;
        tick(&ctx, &mut rec);
        assert_eq!(rec.teleprompter.last, 0.0);
        rec.status = RecordStatus::Recording;
        rec.teleprompter.playing = true;
        ctx.input_mut(|i| i.time = 9.0);
        tick(&ctx, &mut rec);
        assert!((rec.teleprompter.offset - 2.4).abs() < 1e-3, "resuming starts from where the script stopped");
    }

    #[test]
    fn rewinding_keeps_the_pace_the_overlay_and_the_paused_state() {
        let mut state = State { offset: 220.0, speed: SPEED_MAX, overlay: false, playing: false, last: 7.0 };
        state.rewind();
        assert_eq!(state.offset, 0.0, "back to the top");
        assert!((state.speed - SPEED_MAX).abs() < f32::EPSILON, "the user's pace stays");
        assert!(!state.overlay && !state.playing, "and their switches");
    }

    #[test]
    fn the_overlay_stays_inside_the_slide() {
        let slide = Rect::from_min_size(pos2(100.0, 50.0), egui::vec2(800.0, 400.0));
        let rect = overlay_rect(&layout_with(slide));
        assert!((rect.height() - 152.0).abs() < 1e-3, "38% of the slide");
        assert!((rect.width() - (slide.width() - 36.0)).abs() < 1e-3);
        assert!(rect.min.x >= slide.min.x);
        assert!(rect.min.y >= slide.min.y);
        assert!(rect.max.x <= slide.max.x);
        assert!(rect.max.y <= slide.max.y);
        assert!((rect.max.y - (slide.max.y - 18.0)).abs() < 1e-3);
        assert!((rect.min.y - (slide.max.y - 18.0 - 152.0)).abs() < 1e-3);
    }

    #[test]
    fn the_overlay_height_is_clamped_and_tiny_slides_get_nothing() {
        let tall = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(900.0, 1000.0));
        assert!((overlay_rect(&layout_with(tall)).height() - 300.0).abs() < 1e-3, "never taller than 300");

        let short = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(900.0, 170.0));
        let rect = overlay_rect(&layout_with(short));
        assert!((rect.height() - 120.0).abs() < 1e-3, "never shorter than 120");
        assert!(rect.min.y >= short.min.y);

        assert_eq!(overlay_rect(&layout_with(Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(199.0, 400.0)))), Rect::NOTHING);
        assert_eq!(overlay_rect(&layout_with(Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(400.0, 159.0)))), Rect::NOTHING);
        assert_eq!(overlay_rect(&layout_with(Rect::NOTHING)), Rect::NOTHING);
        let broken = Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(400.0, 400.0));
        assert_eq!(overlay_rect(&layout_with(broken)), Rect::NOTHING);
    }
}
