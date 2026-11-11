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
pub(super) mod devices;
pub(crate) mod notes;
mod top;
mod views;

use std::sync::Arc;

use deckcraft_model::Presentation;
use egui::{Color32, CornerRadius, Pos2, Rect, Ui, pos2, vec2};

use super::presenter::PresenterTimer;
use super::render::fit;
use super::{Show, SlideApp, take_narration};

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

/// An explicit request for a section that is not wired yet. A section consumes it and must not
/// report success unless it actually did the work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RecordCommand {
    /// Export the recorded show as a video (no renderer is wired in this shell).
    Export,
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
    /// The notes editor's per-slide draft, focus and saving state.
    pub(super) notes_edit: notes::EditState,
    /// The Teleprompter overlay's scroll, speed and visibility.
    pub(super) teleprompter: notes::TeleprompterState,
    /// The notes pane is part of the view.
    pub(super) notes_open: bool,
    /// Selected record view. Defined in [`views`] so the dropdown and the shell share it.
    pub(super) view: views::ViewKind,
    pub(super) more_open: bool,
    pub(super) ink: InkState,
    pub(in crate::show) devices: devices::Devices,
    /// The teleprompter overlay drawn last frame, so a click or wheel over it is not the slide's.
    pub(super) overlay: Option<Rect>,
    /// The cameo bubble on the slide canvas is shown.
    pub(super) camera_bubble: bool,
    /// The separate camera preview window is shown.
    pub(super) camera_preview: bool,
    /// A shell button asked for work that only a section can do.
    pub(super) pending: Option<RecordCommand>,
    /// A take has started in this view, so leaving it should save the recording.
    pub(crate) captured: bool,
    /// Leaving the view while a take is running asks for a confirmation first.
    pub(super) close_confirm: bool,
    /// The clock value replaced by the last Reset, so Undo can put it back.
    /// The elapsed value a Reset replaced, and when it was taken: Undo adds the time since.
    undo_elapsed: Option<(f64, f64)>,
}

impl Default for RecordView {
    fn default() -> Self {
        RecordView {
            status: RecordStatus::Idle,
            // A paused clock reads zero until the first take starts it.
            timer: PresenterTimer::paused_at_zero(),
            total: 0.0,
            notes_px: notes::NOTES_DEFAULT,
            notes_scroll: 0.0,
            notes_edit: notes::EditState::default(),
            teleprompter: notes::TeleprompterState::default(),
            notes_open: true,
            view: views::ViewKind::default(),
            more_open: false,
            ink: InkState::default(),
            devices: devices::Devices::default(),
            overlay: None,
            camera_bubble: true,
            camera_preview: false,
            pending: None,
            captured: false,
            close_confirm: false,
            undo_elapsed: None,
        }
    }
}

impl RecordView {
    /// Seconds recorded so far. A paused view keeps the value it had when it was paused.
    /// Capture time so far. A view opens with a paused clock, so nothing is counted until Record.
    pub(super) fn elapsed(&self, now: f64) -> f64 {
        self.timer.elapsed(now)
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
        self.ink.active = false;
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

    /// Continue a paused capture from `now`. The microphone starts a fresh buffer, so nothing
    /// spoken while the take was held reaches this slide's narration.
    pub(super) fn resume(&mut self, show: &mut Show, now: f64) {
        if self.status == RecordStatus::Paused {
            self.timer.toggle(now);
            self.status = RecordStatus::Recording;
            show.mic_tried = false;
            show.resume_recording(now);
        }
    }

    /// Stop capturing and leave the view open on the frozen clock. Record starts a new take.
    pub(super) fn stop(&mut self, show: &mut Show, now: f64) {
        match self.status {
            RecordStatus::Recording => {
                self.timer.toggle(now);
                self.status = RecordStatus::Idle;
            }
            RecordStatus::Paused => self.status = RecordStatus::Idle,
            RecordStatus::Idle => {}
        }
        show.end_recording();
        self.ink.active = false;
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
        // Keep when the reset happened, so Undo can add the time the run has gone on for since.
        self.undo_elapsed = Some((self.elapsed(now), now));
        self.timer.reset(now);
    }

    /// Put back the clock value the last Reset replaced. Returns whether anything was undone.
    pub(super) fn undo(&mut self, now: f64) -> bool {
        let Some((secs, _at)) = self.undo_elapsed.take() else { return false };
        // `reset` zeroes the timer, so its own elapsed value is already the time recorded
        // since Reset, excluding paused stretches. Adding the pre-Reset value restores the run.
        let since = self.timer.elapsed(now).max(0.0);
        self.timer = PresenterTimer::start(now - (secs + since).max(0.0));
        if self.status != RecordStatus::Recording {
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
        top.contains(pos)
            || notes_rect(full, top, self).contains(pos)
            || bottom_rect(full, top).contains(pos)
            || self.overlay.is_some_and(|r| r.contains(pos))
    }
}

/// Where every section lives on screen this frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Layout {
    pub full: Rect,
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
    /// Presenter view's next-slide strip, below the notes pane. None in the other views. The
    /// shell does not paint in it yet; the tests pin the geometry its section will use.
    #[allow(dead_code)]
    pub next: Option<Rect>,
}

/// Horizontal margin shared by the chrome.
const EDGE: f32 = 40.0;

/// The shell's fixed geometry. `slide` is fitted into the canvas area so the show's own renderer
/// can draw it; a zero area (a view with no room) still yields a valid empty rect.
pub(super) fn layout(full: Rect, doc: &Presentation, rec: &RecordView) -> Layout {
    let top = top_rect(full);
    let bottom = bottom_rect(full, top);
    let notes = notes_rect(full, top, rec);
    let notes_below = if rec.view == views::ViewKind::Presenter && notes.height() > 1.0 {
        // The presenter view keeps a next-slide strip under the notes.
        let h = (notes.height() * 0.34).clamp(56.0, 120.0);
        Rect::from_min_max(pos2(notes.min.x, notes.max.y - h), notes.max)
    } else {
        Rect::NOTHING
    };
    let canvas =
        Rect::from_min_max(pos2(full.min.x + EDGE, notes.max.y + 12.0), pos2(full.max.x - EDGE, (bottom.min.y - 14.0).max(notes.max.y + 12.0)));
    let slide = if canvas.width() > 1.0 && canvas.height() > 1.0 { fit(canvas, doc) } else { Rect::from_min_size(canvas.min, vec2(0.0, 0.0)) };
    let radius = (slide.height() * 0.14).clamp(48.0, 120.0);
    let camera = Rect::from_center_size(pos2(slide.max.x - radius - 16.0, slide.max.y - radius - 16.0), vec2(radius * 2.0, radius * 2.0));
    let views = views_rect(bottom);
    let camera_preview = Rect::from_min_size(pos2(views.min.x - 44.0, views.min.y), vec2(30.0, 30.0));
    let next = (rec.view == views::ViewKind::Presenter).then_some(notes_below);
    Layout { full, top, notes, slide, bottom, views, camera, camera_preview, next }
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

/// Draw the shell. Returns false when Edit asked to leave the Record view (the show's normal
/// finish path then saves the recording and returns to the editor).
pub(super) fn ui(app: &mut SlideApp, ui: &mut Ui, show: &mut Show, doc: &Arc<Presentation>, now: f64, layout: &Layout) -> bool {
    // Take the state out of the show so its sections can borrow both `show` and the shell state.
    let mut rec = std::mem::take(&mut show.record_view);
    devices::connect(&mut rec, &app.services);
    rec.read_total(doc);
    let was_recording = rec.status == RecordStatus::Recording;
    let mut keep = true;
    ui.painter().rect_filled(layout.full, CornerRadius::ZERO, Color32::from_rgb(16, 16, 18));
    // The slide is drawn by the show before this call; the pane backgrounds sit around it.
    notes::ui(app, ui, show, &mut rec, doc, layout);
    notes::overlay(ui, &mut rec, doc, show, layout);
    let top = top::ui(app, ui, show, &mut rec, now, layout);
    keep &= !top.close;
    // The script scrolls with the take, so it follows this frame's Record/Pause click.
    notes::tick(ui.ctx(), &mut rec);
    bottom::ui(&mut app.services.mic, ui, show, &mut rec, doc, now, layout);
    canvas::bubble(ui, &rec, layout);
    views::ui(ui, &mut rec, layout);
    // Pause or Stop ends capture. Everything the take gathered on the slide it stopped on — its
    // narration, its ink, its laser trace and its time — moves into the take now, and nothing
    // gathered afterwards may join it. Resume starts a fresh microphone buffer.
    if was_recording && rec.status != RecordStatus::Recording {
        // A microphone this take never started cannot have failed for it.
        if show.mic_tried
            && let Some(e) = app.services.mic.as_ref().and_then(|mic| mic.failure())
        {
            show.mic_error = Some(e);
        }
        take_narration(app.services.mic.as_deref_mut(), show, show.state.slide);
        app.stop_mic();
        show.stash_ink(doc, show.state.slide, true, now);
        show.stash_laser(show.state.slide, true);
        show.end_take_timing(now);
    }
    if !keep {
        notes::flush(app, &mut rec, doc, show.state.slide);
    }
    if keep {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    show.record_view = rec;
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
