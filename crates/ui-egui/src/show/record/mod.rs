//! The Record Slide Show view: a dark, full-screen shell drawn around a show that is being
//! recorded. The top command bar carries the recorder (Edit, clock, Reset/Undo, Record, Pause,
//! camera, microphone, screen, More and Export), the notes pane shows the current slide's
//! speaker notes, the slide canvas is the show's own render, and the bottom strip holds slide
//! navigation and the ink tools.
//!
//! The shell is a child of [`super`] so it can drive the show's clocks and navigation directly.
//! Its sections live next to this file; a section owns its own widgets and leaves the state on
//! [`RecordView`] for the next frame and for the control channel.

mod bottom;
mod canvas;
mod devices;
mod notes;
mod top;
mod views;

use std::sync::Arc;

use deckcraft_model::Presentation;
use egui::{Color32, Pos2, Rect, Ui, pos2, vec2};

use super::presenter::PresenterTimer;
use super::render::fit;
use super::{Show, SlideApp};

/// Where the recorder is in its capture cycle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum RecordStatus {
    /// The view is open and the slides can be inspected; nothing is captured.
    #[default]
    Idle,
    /// Timings, ink and narration are being captured.
    Recording,
    /// Capture is held; the elapsed clock and the slide clocks stay where they were.
    Paused,
}

/// Ink tool selected in the bottom bar. `Arrow` leaves the pointer alone; the three ink tools are
/// mapped onto the show's pen by [`bottom`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum InkTool {
    #[default]
    Arrow,
    Pen,
    Highlighter,
    Eraser,
}

/// Bottom-bar ink choices, kept apart from the show so the controls survive a slide change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct InkState {
    pub tool: InkTool,
    /// Index into [`bottom::INK_COLORS`].
    pub color: usize,
    /// A stroke is being drawn this frame.
    pub active: bool,
}

impl Default for InkState {
    fn default() -> Self {
        InkState { tool: InkTool::Arrow, color: 0, active: false }
    }
}

/// Everything the Record view owns. `Show::record` is the mode flag; this is the shell's own
/// state, and it is kept on the show so it travels with the recording instance.
pub(crate) struct RecordView {
    pub(super) status: RecordStatus,
    pub(super) timer: PresenterTimer,
    /// The deck's timed length in seconds: the sum of the slides' recorded advance times.
    pub(super) total: f64,
    /// Notes text size in points and the scroll offset the pane last showed.
    pub(super) notes_px: f32,
    pub(super) notes_scroll: f32,
    /// The notes pane is part of the view.
    pub(super) notes_open: bool,
    /// Selected record view. Defined in [`views`] so the dropdown and the shell share it.
    pub(super) view: views::ViewKind,
    pub(super) ink: InkState,
    pub(super) devices: devices::Devices,
    /// The host's capture status, cached so the frame does not query the OS for it.
    pub(super) screen_status: crate::screen::ScreenStatusCache,
    /// The cameo bubble on the slide canvas is shown.
    pub(super) camera_bubble: bool,
    /// The separate camera preview window is shown.
    pub(super) camera_preview: bool,
    /// A take has started in this view, so leaving it should save the recording.
    pub(crate) captured: bool,
    /// The slide's captured length in seconds when Stop froze it: time after Stop (and the rest of
    /// a pause) is not part of the take.
    frozen_slide_secs: Option<f64>,
    /// The clock value replaced by the last Reset, so Undo can put it back.
    undo_elapsed: Option<f64>,
}

impl Default for RecordView {
    fn default() -> Self {
        RecordView {
            status: RecordStatus::Idle,
            timer: PresenterTimer::start(0.0),
            total: 0.0,
            notes_px: notes::NOTES_DEFAULT,
            notes_scroll: 0.0,
            notes_open: true,
            view: views::ViewKind::default(),
            ink: InkState::default(),
            devices: devices::Devices::default(),
            screen_status: crate::screen::ScreenStatusCache::default(),
            camera_bubble: true,
            camera_preview: false,
            captured: false,
            frozen_slide_secs: None,
            undo_elapsed: None,
        }
    }
}

impl RecordView {
    /// Seconds recorded so far. A paused view keeps the value it had when it was paused.
    pub(crate) fn elapsed(&self, now: f64) -> f64 {
        self.timer.elapsed(now)
    }

    /// The take's status as the commands and the status bar print it.
    pub(super) fn status_name(&self) -> &'static str {
        match self.status {
            RecordStatus::Idle => "idle",
            RecordStatus::Recording => "recording",
            RecordStatus::Paused => "paused",
        }
    }

    /// The deck's timed length, or zero when no slide carries a recorded advance.
    pub(super) fn total_seconds(&self) -> f64 {
        if self.total.is_finite() && self.total >= 0.0 { self.total } else { 0.0 }
    }

    /// Set the timed length from the deck: the sum of each slide's recorded advance.
    pub(super) fn read_total(&mut self, doc: &Presentation) {
        let secs: f64 = doc
            .slides
            .iter()
            .filter_map(|s| s.transition.as_ref().and_then(|t| t.advance_after_ms))
            .map(|ms| f64::from(ms) / 1000.0)
            .filter(|s| s.is_finite() && *s >= 0.0)
            .sum();
        self.total = if secs.is_finite() { secs.min(359_999.0) } else { 0.0 };
    }

    /// Start capturing from `now`. The slide clocks restart so the first recorded timing does not
    /// include the time spent looking at the view before Record was pressed.
    pub(super) fn start(&mut self, show: &mut Show, now: f64) {
        self.status = RecordStatus::Recording;
        self.timer = PresenterTimer::start(now);
        self.undo_elapsed = None;
        self.captured = true;
        self.frozen_slide_secs = None;
        self.ink.active = false;
        // The take now holds the capture device: the cached status may be stale.
        self.screen_status.invalidate();
        show.begin_recording(now);
    }

    /// Hold the capture clock. The slide clock is held by the show's pause bookkeeping.
    pub(super) fn pause(&mut self, show: &mut Show, now: f64) {
        if self.status == RecordStatus::Recording {
            self.timer.toggle(now);
            self.status = RecordStatus::Paused;
            show.pause_recording(now);
        }
    }

    /// Continue a paused capture from `now`.
    pub(super) fn resume(&mut self, show: &mut Show, now: f64) {
        if self.status == RecordStatus::Paused {
            self.timer.toggle(now);
            self.status = RecordStatus::Recording;
            show.resume_recording(now);
        }
    }

    /// Stop capturing and leave the view open on the frozen clock. Record starts a new take. The
    /// slide's captured length is frozen here, so time after Stop (or the rest of a pause) never
    /// reaches the saved timing.
    pub(super) fn stop(&mut self, show: &mut Show, now: f64) {
        if self.status != RecordStatus::Idle {
            // The take is releasing the capture device: re-read the host's status next frame.
            self.screen_status.invalidate();
            self.frozen_slide_secs = Some(show.captured_slide_seconds(now));
            // File the slide's segment now, so a navigation after Stop cannot move it.
            show.finalize_take_slide(now);
        }
        match self.status {
            RecordStatus::Recording => {
                self.timer.toggle(now);
                self.status = RecordStatus::Idle;
            }
            RecordStatus::Paused => self.status = RecordStatus::Idle,
            RecordStatus::Idle => {}
        }
        self.ink.active = false;
    }

    /// The slide's captured length when Stop froze it, in seconds.
    pub(super) fn frozen_slide_seconds(&self) -> Option<f64> {
        self.frozen_slide_secs
    }

    /// Record or stop, the red button's one action.
    pub(super) fn toggle_record(&mut self, show: &mut Show, now: f64) {
        match self.status {
            RecordStatus::Idle => self.start(show, now),
            RecordStatus::Recording | RecordStatus::Paused => self.stop(show, now),
        }
    }

    /// Zero the capture clock. The previous value is kept for one Undo; a paused clock stays
    /// paused at zero.
    pub(super) fn reset(&mut self, now: f64) {
        self.undo_elapsed = Some(self.elapsed(now));
        self.timer.reset(now);
    }

    /// Put back the clock value the last Reset replaced. Returns whether anything was undone.
    ///
    /// The timer's own state decides whether the restored clock runs: after Stop the status is
    /// `Idle` but the clock is frozen, and Undo must keep it frozen.
    pub(super) fn undo(&mut self, now: f64) -> bool {
        let Some(secs) = self.undo_elapsed.take() else { return false };
        let was_paused = self.timer.paused();
        self.timer = PresenterTimer::start(now - secs.max(0.0));
        if was_paused {
            self.timer.toggle(now);
        }
        true
    }

    /// A-/A+ in the notes pane.
    pub(super) fn set_notes_size(&mut self, delta: f32) {
        self.notes_px = notes::clamp_size(self.notes_px + delta);
    }

    /// Whether `pos` is over shell chrome rather than the live slide. Chrome must not advance the
    /// show or take an ink stroke.
    pub(super) fn chrome_contains(&self, full: Rect, pos: Pos2) -> bool {
        let top = top_rect(full);
        top.contains(pos) || notes_rect(full, top, self).contains(pos) || bottom_rect(full, top).contains(pos)
    }
}

/// Run a Record-view command on the shell's state: `record.start`, `record.toggle`, `record.pause`,
/// `record.resume`, `record.stop`, `record.reset`, `record.undo`. The shell's buttons and the
/// command channel share this one implementation; returns the take's status after it.
pub(super) fn command(rec: &mut RecordView, show: &mut Show, id: &str, now: f64) -> Result<&'static str, String> {
    match id {
        // Starting over a running or paused take would restart the slide clocks (losing the
        // current slide's timing) and clear the microphone buffer; Stop first.
        "record.start" if rec.status == RecordStatus::Idle => rec.start(show, now),
        "record.start" => return Err(format!("a take is already {}", rec.status_name())),
        "record.toggle" => rec.toggle_record(show, now),
        "record.pause" => rec.pause(show, now),
        "record.resume" => rec.resume(show, now),
        "record.stop" => rec.stop(show, now),
        "record.reset" => rec.reset(now),
        "record.undo" => {
            let _ = rec.undo(now);
        }
        other => return Err(format!("`{other}` isn't a Record-view command")),
    }
    Ok(rec.status_name())
}

/// Where every section lives on screen this frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Layout {
    pub top: Rect,
    pub notes: Rect,
    pub slide: Rect,
    pub bottom: Rect,
    /// The cameo bubble's circular safe area (bottom-right of the slide).
    pub camera: Rect,
    /// Bottom-right Views dropdown.
    pub views: Rect,
    /// Camera-preview icon, left of the Views dropdown.
    pub camera_preview: Rect,
}

/// Horizontal margin shared by the chrome.
const EDGE: f32 = 40.0;

/// The shell's fixed geometry. `slide` is fitted into the canvas area so the show's own renderer
/// can draw it; a zero area (a view with no room) still yields a valid empty rect.
pub(super) fn layout(full: Rect, doc: &Presentation, rec: &RecordView) -> Layout {
    let top = top_rect(full);
    let bottom = bottom_rect(full, top);
    let notes = notes_rect(full, top, rec);
    let canvas =
        Rect::from_min_max(pos2(full.min.x + EDGE, notes.max.y + 12.0), pos2(full.max.x - EDGE, (bottom.min.y - 14.0).max(notes.max.y + 12.0)));
    let slide = if canvas.width() > 1.0 && canvas.height() > 1.0 { fit(canvas, doc) } else { Rect::from_min_size(canvas.min, vec2(0.0, 0.0)) };
    let radius = (slide.height() * 0.14).clamp(48.0, 120.0);
    let camera = Rect::from_center_size(pos2(slide.max.x - radius - 16.0, slide.max.y - radius - 16.0), vec2(radius * 2.0, radius * 2.0));
    let views = views_rect(bottom);
    let camera_preview = Rect::from_min_size(pos2(views.min.x - 44.0, views.min.y), vec2(30.0, 30.0));
    Layout { top, notes, slide, bottom, views, camera, camera_preview }
}

fn top_rect(full: Rect) -> Rect {
    let top_h = 48.0;
    let top_w = (full.width() * 0.82).clamp(520.0, 1280.0).min(full.width().max(1.0));
    Rect::from_center_size(pos2(full.center().x, full.min.y + 10.0 + top_h / 2.0), vec2(top_w, top_h))
}

fn bottom_rect(full: Rect, top: Rect) -> Rect {
    Rect::from_min_max(pos2(full.min.x, (full.max.y - 96.0).max(top.max.y)), full.max)
}

fn notes_rect(full: Rect, top: Rect, rec: &RecordView) -> Rect {
    let notes_h = if rec.notes_open && rec.view != views::ViewKind::Slide {
        match rec.view {
            views::ViewKind::Teleprompter => (full.height() * 0.34).clamp(120.0, 420.0),
            views::ViewKind::Presenter => (full.height() * 0.22).clamp(100.0, 300.0),
            views::ViewKind::Slide => 0.0,
        }
    } else {
        0.0
    };
    let notes_top = top.max.y + 12.0;
    Rect::from_min_max(pos2(full.min.x + EDGE, notes_top), pos2(full.max.x - EDGE, notes_top + notes_h))
}

fn views_rect(bottom: Rect) -> Rect {
    Rect::from_min_size(pos2(bottom.max.x - EDGE - 118.0, bottom.min.y + 58.0), vec2(118.0, 30.0))
}

/// The Record view's background, painted by the show under the slide (bars and panes sit on it).
pub(super) const BACKDROP: Color32 = Color32::from_rgb(16, 16, 18);

/// Draw the shell. Returns false when Edit asked to leave the Record view (the show's normal
/// finish path then saves the recording and returns to the editor).
pub(super) fn ui(app: &mut SlideApp, ui: &mut Ui, show: &mut Show, doc: &Arc<Presentation>, now: f64, layout: &Layout) -> bool {
    // Take the state out of the show so its sections can borrow both `show` and the shell state.
    let mut rec = std::mem::take(&mut show.record_view);
    // `ScreenState::status` queries the OS, so the frame uses `rec`'s cached value (re-read at
    // most once a second, and whenever a take starts or stops) instead of asking every paint.
    let (capture, recording) = (rec.screen_status.read(now, || app.session.screen.status()), app.session.screen.is_recording());
    devices::connect(&mut rec, &app.services, show, (capture, recording));
    rec.read_total(doc);
    let mut keep = true;
    // The backdrop (`BACKDROP`) is painted by the show before it draws the slide: filling the
    // whole frame here would cover the slide that is already on screen.
    notes::ui(app, ui, show, &mut rec, doc, layout);
    let top = top::ui(app, ui, show, &mut rec, now, layout);
    keep &= !top.close;
    let nav = bottom::ui(ui, show, &mut rec, doc, layout);
    canvas::bubble(ui, &rec, layout);
    views::ui(ui, &mut rec, layout);
    if keep {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    show.record_view = rec;
    // The shell's own prev/next run once the take is back on the show: `Show::apply` and
    // `before_slide_change` both read `show.capturing()`, which is false while `rec` is out.
    if let Some(action) = nav {
        super::before_slide_change(app, show, &action);
        keep &= show.apply(action, doc, now);
    }
    keep
}

/// Whether a section's text field owns the keyboard, so the shell must not navigate slides.
pub(super) fn notes_has_focus(ctx: &egui::Context) -> bool {
    notes::has_focus(ctx)
}

fn clock(secs: f64) -> String {
    // The record view uses a two-part clock; hours are folded into the minutes for very long takes.
    let s = secs.max(0.0).round() as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// `00:00 / 12:30` for the top bar.
fn clock_total(elapsed: f64, total: f64) -> String {
    format!("{} / {}", clock(elapsed), clock(total))
}

#[cfg(test)]
mod tests;
