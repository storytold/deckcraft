//! Slide Show: full-screen playback with transitions and animations (`deckcraft-anim`),
//! keyboard/mouse navigation, pen, black/white screens, See All Slides, zoom, presenter view
//! and rehearsed timings.

use deckcraft_anim::{Click, HitShape, Layer, LinkJump, ShowAction, ShowState, Source, StepStamp, Timeline, TriggerPlay};
use deckcraft_model::text::Hyperlink;
use deckcraft_model::{Presentation, Rgba, ShapeId};
use deckcraft_render::{ParaState, PlacedLink, PlacedShape, RenderOpts, ShapeState};
use egui::{Align2, Color32, CornerRadius, Mesh, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::theme;

mod keys;
mod media;
mod presenter;
pub(crate) mod record;
mod render;
mod ui;
mod zoom;
pub(crate) use zoom::GRID_CAP;

#[cfg(test)]
use media::{on_screen, pointer_on_media_bar, shown_slide_rect, slide_media_bars};
use presenter::{NOTES_DEFAULT, PresenterTimer};
#[cfg(test)]
use presenter::{NOTES_MAX, NOTES_MIN, NOTES_STEP, format_elapsed, presenter_notes, presenter_window, wall_clock};
#[cfg(test)]
use render::{below_playing_video, next_rect, render_frame, start_prefetch};
use render::{render_backdrop, render_threads};
#[cfg(test)]
use ui::transition_layers;
pub use ui::ui;
use zoom::Zoom;
#[cfg(test)]
use zoom::{GOTO_DIGITS, norm_on_slide, see_all_cells, see_all_hit, toggle_blank, unit_uv, wheel_zoom_lines, zoom_uv};

/// What the pointer draws: the arrow selects, the pen and highlighter leave ink, the eraser
/// removes whole strokes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum InkMode {
    #[default]
    Arrow,
    Pen,
    Highlighter,
    Eraser,
}

impl InkMode {
    /// True for the tools that draw (the eraser removes instead).
    pub(crate) fn draws(self) -> bool {
        matches!(self, Self::Pen | Self::Highlighter)
    }

    /// Highlighter ink is wider and translucent; the pen is a thin solid line.
    pub(crate) fn width_scale(self) -> f64 {
        if self == Self::Highlighter { 3.0 } else { 1.0 }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Arrow => "Pointer",
            Self::Pen => "Pen",
            Self::Highlighter => "Highlighter",
            Self::Eraser => "Eraser",
        }
    }
}

/// One live stroke on the show canvas, in normalized slide coordinates.
#[derive(Clone, Debug)]
pub(crate) struct InkStroke {
    pub points: Vec<Pos2>,
    pub mode: InkMode,
    pub color: Rgba,
    /// Width in slide points; the highlighter's is already scaled.
    pub width: f64,
    /// Whether the take owns it: a stroke drawn while the take is paused stays out of the recording.
    pub in_take: bool,
}

pub struct Show {
    /// Monotonic identity of this show instance. Starting a show again (even at the same slide
    /// and step) is a new instance, so a queued click cannot cross an ABA transition.
    pub(crate) instance: u64,
    pub state: ShowState,
    pub reading: bool,
    pub presenter: bool,
    /// Preview from the Transitions/Animations tab: exits after the current slide.
    pub preview_only: bool,
    /// Preview from the Record tab: plays the recorded timings and narration, saving nothing.
    pub recording_preview: bool,
    /// Play every step automatically (animation preview).
    pub autoplay: bool,
    pub rehearse: bool,
    /// Record Slide Show: keep timings, narration, pen ink and laser when the show ends.
    pub record: bool,
    /// A take is capturing right now. The Record view moves [`Self::record_view`] out while its
    /// shell draws, so the show cannot read the view's status during that frame; this flag is set
    /// by the view's own start/pause/resume/stop and is what the capture paths consult.
    pub(crate) capturing: bool,
    /// The Record view shell's own state (clock, layout, sections, device hooks).
    pub(crate) record_view: record::RecordView,
    /// When a recording pause started; the show clock is shifted by the pause length.
    record_paused_at: Option<f64>,
    /// The slide the take was held on, so a slide opened while held can start its clock at
    /// resume instead of inheriting the slide-entry time.
    pause_slide: Option<usize>,
    /// Where the eraser was last seen, so a fast drag erases along the path it swept and not
    /// only at the frame's own point.
    eraser_from: Option<Pos2>,
    /// The frame clock of the last drawn pass, so a commanded end can date the take's last slide.
    pub(crate) last_now: f64,
    /// Why the microphone stopped on its own during the take, kept so the save can say so.
    pub(crate) mic_error: Option<String>,
    /// Set Up Slide Show ▸ Keep mouse cursor visible: a still pointer is not hidden (#1445).
    pub keep_cursor: bool,
    timeline: Option<(usize, Timeline)>,
    /// The slide whose clock is running. Kept apart from [`Self::timeline`] because queued
    /// actions are applied in one pass: the cache may already name the destination slide when the
    /// timing and narration of the slide being left are still to be recorded.
    clock_slide: Option<usize>,
    step_start: f64,
    slide_start: f64,
    step_end: f64,
    trans: Option<Transition>,
    tex: Option<(Key, TextureHandle)>,
    /// The shapes stacked above the playing videos, drawn over the video frames (#1005).
    overlay: Option<(Key, Vec<ShapeId>, TextureHandle)>,
    /// The Presenter View's next-slide preview: the presentation and slide it shows.
    next_tex: Option<(std::sync::Arc<Presentation>, usize, TextureHandle)>,
    /// The one next-slide preview being rendered on another thread (#964).
    next_job: Option<(std::sync::Arc<Presentation>, usize, std::sync::mpsc::Receiver<egui::ColorImage>)>,
    /// Preview renders started on another thread.
    #[cfg(test)]
    preview_spawns: usize,
    remote_preview: Option<crate::presenter::Preview>,
    remote_next: Option<(usize, crate::presenter::Preview)>,
    /// All slides thumbnails sent to the web presenter window while its grid is open (#1866).
    remote_grid: Vec<crate::presenter::Preview>,
    remote_sent: f64,
    remote_opened: bool,
    /// The next slide's first frame, rendered in the background while this one is shown so
    /// advancing only uploads it (#240).
    prefetch: Option<Prefetch>,
    /// The current slide's backdrop, under its animation frames.
    backdrop: Option<Backdrop>,
    blank: Option<Color32>,
    /// See All Slides covers the show until a slide is chosen.
    pub grid: bool,
    /// The Presenter View's own See All Slides: the grid replaces the slide and notes in that window only; the audience keeps the slide.
    pub presenter_grid: bool,
    /// The next click on the slide zooms in there.
    pub zoom_arm: bool,
    zoom: Option<Zoom>,
    thumbs: Vec<(usize, TextureHandle)>,
    pen: bool,
    /// A dot that follows the pointer and leaves no ink (`Cmd/Ctrl+L`).
    laser: bool,
    /// The ink tool selected for the show or the recording toolbar.
    ink_mode: InkMode,
    /// The colour the pen and highlighter draw with.
    ink_color: Rgba,
    /// The pen's width in slide points; the highlighter scales this by three.
    ink_width: f64,
    strokes: Vec<InkStroke>,
    /// Digits typed for Go to Slide, shown until Enter, Esc, or a short pause.
    goto: String,
    goto_at: f64,
    started: bool,
    show_start: f64,
    /// Elapsed time in presenter view. Pausing it does not pause the show.
    timer: PresenterTimer,
    /// Speaker-notes size in presenter view, in points.
    notes_px: f32,
    timings: Vec<(usize, f64)>,
    /// Pen strokes kept while recording, in slide points, with the slide, tool, colour and width
    /// they were drawn with.
    /// The take's ink: slide, points in slide points, mode, colour, width, and when it was drawn
    /// (ms from that slide's entry) so a preview can reveal it on the clock.
    recorded: Vec<(usize, Vec<(f64, f64)>, InkMode, Rgba, f64, u32)>,
    /// Laser samples kept while recording: slide points and milliseconds from the slide start.
    laser_live: Vec<(f64, f64, u32)>,
    laser_saved: Vec<(usize, Vec<(f64, f64, u32)>)>,
    /// WAV bytes per slide, taken from the microphone when the slide is left.
    /// Per slide: the take's rate and the raw samples gathered so far. Encoding happens once, on
    /// save, so a pause and resume appends instead of replacing what was already spoken.
    narration: Vec<(usize, u32, Vec<f32>)>,
    /// A slide's narration hit the ten-minute or WAV limit while capturing, so its tail was dropped
    /// before encoding: the save must not call that take complete.
    narration_cut: bool,
    mic_tried: bool,
    mic_ok: bool,
    last_move: f64,
    /// The last trackpad flick or mouse-wheel notch, so one gesture turns one page.
    last_swipe: f64,
    pub ended: bool,
    /// The slide whose media was last started (auto-play runs once per visit).
    media_slide: Option<usize>,
    /// Autoplay clips that have not started yet: `(shape, seconds on the show clock)`.
    pending_media: Vec<(ShapeId, f64)>,
    /// The slide shown before the current one (the Last Slide Viewed action).
    last_viewed: Option<usize>,
    /// Click areas on slide `.0`, in slide points: hyperlinks and shape actions, and the slide's
    /// shapes in drawing order.
    links: Option<(usize, Vec<PlacedLink>, Vec<PlacedShape>)>,
    /// The main step whose media effects are being set off, and how far into it they have been.
    media_clock: Option<(usize, f64)>,
    /// Media effects set off and not yet carried out: (shape, effect id).
    media_cues: Vec<(ShapeId, String)>,
    /// Hidden-logic time anchor: the last visible egui time and the monotonic instant of that
    /// visible frame. `run_logic` does not refresh `InputState::time`, so this lets a hidden show
    /// keep playing from wall time without a draw.
    hidden_anchor: Option<(f64, web_time::Instant)>,
}

#[derive(Clone, Debug, PartialEq)]
struct Key {
    slide: usize,
    step: usize,
    size: (u32, u32),
    doc: usize,
    /// Media hidden while not playing.
    hidden: Vec<ShapeId>,
    /// Clicks played of each triggered animation (what the shapes look like when none is running).
    plays: Vec<(ShapeId, usize)>,
    /// The frames animated GIFs show.
    gif: u64,
    /// Recording preview: recorded ink strokes the clock has reached (a new count is a new frame).
    ink: usize,
}

/// A slide frame being rendered on another thread: what it will be (`key`) and where it arrives.
struct Prefetch {
    key: Key,
    rx: std::sync::mpsc::Receiver<egui::ColorImage>,
}

/// What a slide's backdrop was rendered for.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BackKey {
    slide: usize,
    size: (u32, u32),
    doc: usize,
}

/// A slide's backdrop (background, master and layout graphics), kept while the slide is shown so
/// animation frames render only the slide's own shapes over it. The backdrop can cost far more
/// than the shapes: a full-slide picture with a glow in the layout made animations crawl at 2–3
/// frames a second. `rx` brings it from another thread when it is rendered ahead of time.
struct Backdrop {
    /// The presentation it was rendered from, held so its address (in `key`) can't be reused by
    /// a later edit while the backdrop is kept.
    doc: std::sync::Arc<Presentation>,
    key: BackKey,
    img: Option<std::sync::Arc<deckcraft_render::Image>>,
    rx: Option<std::sync::mpsc::Receiver<deckcraft_render::Image>>,
}

struct Transition {
    old: Option<TextureHandle>,
    /// The slide left behind (Morph matches its shapes) and the Morph frame texture.
    from: Option<usize>,
    morph: Option<TextureHandle>,
    /// Backgrounds and contents of the old and new slides, rendered once when the transition
    /// moves contents over backgrounds (Pan, Ferris Wheel, Reveal…): old back, old content, new
    /// back, new content.
    parts: Option<[Option<TextureHandle>; 4]>,
    start: f64,
    kind: String,
    option: String,
    dur: f64,
    /// Going back: the transition of the slide left behind plays from its end to its start, so
    /// the old slide takes the new slide's part (#1531).
    reverse: bool,
}

/// A copy of the show's clock origins for regressions that must not depend on wall time.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClockSnapshot {
    pub step_start: f64,
    pub slide_start: f64,
    pub step_end: f64,
    pub show_start: f64,
    pub last_move: f64,
}

/// The parts of a show that change what a window-point event means. Coordinates are relative
/// to the whole viewport, so the step, end screen, reading/presenter layout, pen/grid/zoom modes
/// and blank screen all have to match the batch's target as well as the instance and slide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShowInputState {
    pub instance: u64,
    pub slide: usize,
    pub step: usize,
    pub ended: bool,
    pub reading: bool,
    pub presenter: bool,
    pub grid: bool,
    pub pen: bool,
    pub laser: bool,
    pub blank: Option<[u8; 4]>,
    pub zoom: Option<[u32; 3]>,
    pub zoom_arm: bool,
}

/// True unless the root viewport explicitly reports itself minimized or fully occluded.
/// Integrations that do not report visibility stay visible rather than freezing the show.
pub(crate) fn viewport_visible(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().visible() != Some(false))
}

impl Show {
    /// Initialise show state at the requested slide with reading/presenter modes; an out-of-range
    /// slide starts an ended show.
    pub fn new(doc: &Presentation, from: usize, reading: bool, presenter: bool) -> Self {
        Show {
            instance: 0,
            state: ShowState::start_at(doc, from),
            reading,
            presenter,
            preview_only: false,
            recording_preview: false,
            autoplay: false,
            rehearse: false,
            record: false,
            capturing: false,
            record_view: record::RecordView::default(),
            record_paused_at: None,
            pause_slide: None,
            eraser_from: None,
            last_now: 0.0,
            mic_error: None,
            keep_cursor: false,
            timeline: None,
            clock_slide: Some(from),
            step_start: 0.0,
            slide_start: 0.0,
            step_end: 0.0,
            trans: None,
            tex: None,
            overlay: None,
            next_tex: None,
            next_job: None,
            #[cfg(test)]
            preview_spawns: 0,
            remote_preview: None,
            remote_next: None,
            remote_grid: vec![],
            remote_sent: f64::NEG_INFINITY,
            remote_opened: false,
            prefetch: None,
            backdrop: None,
            blank: None,
            grid: false,
            presenter_grid: false,
            zoom_arm: false,
            zoom: None,
            thumbs: vec![],
            pen: false,
            laser: false,
            ink_mode: InkMode::Arrow,
            ink_color: Rgba::rgb(0xFF, 0x00, 0x00),
            ink_width: 2.25,
            strokes: vec![],
            goto: String::new(),
            goto_at: 0.0,
            started: false,
            show_start: 0.0,
            timer: PresenterTimer::start(0.0),
            notes_px: NOTES_DEFAULT,
            timings: vec![],
            recorded: vec![],
            laser_live: vec![],
            laser_saved: vec![],
            narration: vec![],
            narration_cut: false,
            mic_tried: false,
            mic_ok: false,
            last_move: 0.0,
            last_swipe: f64::NEG_INFINITY,
            ended: false,
            media_slide: None,
            pending_media: vec![],
            last_viewed: None,
            links: None,
            media_clock: None,
            media_cues: vec![],
            hidden_anchor: None,
        }
    }

    /// Play this slide's transition from the previous slide (or black for slide 0) as a preview (#1654).
    pub fn start_transition_preview(&mut self, doc: &Presentation, now: f64) {
        let i = self.state.slide;
        let from = i.checked_sub(1);
        let (kind, option, dur) = doc
            .slides
            .get(i)
            .and_then(|s| s.transition.as_ref())
            .map(|t| (t.kind.clone(), t.option.clone(), t.duration_ms as f64 / 1000.0))
            .unwrap_or(("none".into(), String::new(), 0.0));
        self.trans = if kind != "none" && dur > 0.0 && !doc.show.without_animation {
            Some(Transition { old: None, from, morph: None, parts: None, start: now, kind, option, dur, reverse: false })
        } else {
            None
        };
        self.preview_only = true;
        self.slide_start = now;
        self.step_start = now;
        self.step_end = now;
    }

    /// Identity of the show state a queued window-point event belongs to.
    pub(crate) fn input_state(&self) -> ShowInputState {
        let bits = |v: f32| if v.is_finite() { v.to_bits() } else { 0 };
        ShowInputState {
            instance: self.instance,
            slide: self.state.slide,
            step: self.state.step,
            ended: self.ended || self.state.ended,
            reading: self.reading,
            presenter: self.presenter,
            grid: self.grid,
            pen: self.pen,
            laser: self.laser,
            blank: self.blank.map(|c| [c.r(), c.g(), c.b(), c.a()]),
            zoom: self.zoom.map(|z| [bits(z.nx), bits(z.ny), bits(z.scale)]),
            zoom_arm: self.zoom_arm,
        }
    }

    /// The show time for a hidden logic pass. `run_logic` leaves `InputState::time` at the last
    /// visible frame, so anchor that value to the monotonic instant of the visible frame and
    /// advance from wall time. Only a full visible UI pass re-anchors to egui's real frame time;
    /// a visible logic-only pass must not reuse its stale input time as a new anchor.
    pub(crate) fn hidden_logic_time(&self, visible_time: f64, now: web_time::Instant) -> f64 {
        let (base, at) = self.hidden_anchor.unwrap_or((visible_time, now));
        base + now.saturating_duration_since(at).as_secs_f64()
    }

    /// A visible UI pass drives the clocks through its real frame time and records the anchor a
    /// later hidden logic tick advances from.
    pub(crate) fn observe_visible_frame(&mut self, visible_time: f64, now: web_time::Instant) {
        self.hidden_anchor = Some((visible_time, now));
    }

    /// Whether hidden logic is running a show that has already passed its first visible frame.
    pub(crate) fn hidden_started(&self) -> bool {
        self.started
    }

    /// Whether this show keeps timings, narration, ink and laser: Rehearse, or a Record take that
    /// is actually capturing. Never a preview, and never the Record view while it is idle or
    /// paused — there is nothing to record then.
    fn keeps_recording(&self) -> bool {
        if self.recording_preview {
            return false;
        }
        self.rehearse || (self.record && self.capturing)
    }

    /// Whether the Record view's chrome is on screen. Unlike [`Self::keeps_recording`] this holds
    /// while the take is idle, so its buttons never reach the show as a click on the slide.
    fn record_chrome(&self) -> bool {
        self.record && !self.recording_preview
    }

    /// Seconds after which the current slide advances by itself. A Record preview follows the
    /// recorded timings even when the deck's Use Timings is off, so it shows the recording.
    fn auto_advance_after(&self, doc: &Presentation) -> Option<f64> {
        if !self.recording_preview {
            return self.state.advance_after(doc);
        }
        if self.ended {
            return None;
        }
        doc.slides.get(self.state.slide).and_then(|s| s.transition.as_ref()).and_then(|t| t.advance_after_ms).map(|ms| ms as f64 / 1000.0)
    }

    /// Whether narration clips are left off while this show plays. A Record preview plays the
    /// deck's narration even when Play Narrations is off, because it shows what was recorded.
    fn without_narration(&self, doc: &Presentation) -> bool {
        doc.show.without_narration && !self.recording_preview
    }

    /// Whether the slide has waited out its advance-after time and no animation is playing.
    fn auto_advance_due(&self, doc: &Presentation, elapsed: f64) -> bool {
        self.auto_advance_after(doc).is_some_and(|after| !self.state.playing && elapsed.is_finite() && elapsed >= after)
    }

    /// Whether hidden logic still has time-driven work, so it must keep requesting wake-ups.
    pub(crate) fn hidden_time_driven(&self, doc: &Presentation) -> bool {
        self.started
            && !self.ended
            && (self.state.playing
                || self.trans.is_some()
                || self.autoplay
                || self.auto_advance_after(doc).is_some()
                // A delayed automatic clip is still waiting for its deadline.
                || !self.pending_media.is_empty())
    }

    /// Advance the show's own clocks and automatic steps on a hidden logic pass. Drawing is a
    /// separate concern, so a minimized show keeps playing instead of inventing a pause policy.
    /// Returns false when the show should close (preview ended).
    pub(crate) fn advance_hidden(&mut self, doc: &Presentation, now: f64) -> bool {
        self.tick_triggers(doc, now);
        self.expire_goto(now);
        if self.trans.as_ref().is_some_and(|trans| now - trans.start >= trans.dur.max(0.01)) {
            self.trans = None;
        }
        let mut actions = Vec::new();
        let mut keep = self.queue_clock_actions(doc, now, &mut actions);
        for action in actions {
            if !self.apply(action, doc, now) {
                keep = false;
            }
        }
        if self.state.playing {
            let step = self.state.step;
            self.cue_media(doc, step, now - self.step_start);
        }
        keep
    }

    /// Queue the step/auto-advance actions for `now`. Shared by the visible UI pass and the
    /// hidden logic tick so both read the same clock. Returns false when a preview should close.
    fn queue_clock_actions(&mut self, doc: &Presentation, now: f64, actions: &mut Vec<ShowAction>) -> bool {
        let (step, playing) = (self.state.step, self.state.playing);
        let t_step = if playing { now - self.step_start } else { 0.0 };
        if playing {
            let dur = self.timeline(doc).step_duration(step);
            if t_step >= dur {
                actions.push(self.state.step_done(doc));
                self.step_end = now;
            }
        } else if self.trans.is_none() {
            // Auto-advance (After: timings) or autoplay preview. An automatic first step is started
            // by `ShowState` as the slide appears; starting it here too replayed it after Back (#844).
            let idle = now - self.step_end.max(self.slide_start);
            let cur_step = self.state.step;
            let n_steps = self.timeline(doc).steps();
            let step_due = self.autoplay && cur_step < n_steps && idle > 0.3;
            let holding = self.grid || self.presenter_grid || self.zoom.is_some() || self.zoom_arm;
            if !holding && (step_due || (!(self.rehearse || self.record) && self.auto_advance_due(doc, idle) && !self.ended)) {
                actions.push(self.state.next(doc));
            } else if self.preview_only && cur_step >= n_steps && idle > 0.8 {
                return false;
            }
        }
        true
    }

    /// The clock origins used by the show, for deterministic hidden/resume regressions.
    #[cfg(test)]
    pub(crate) fn clock_snapshot(&self) -> ClockSnapshot {
        ClockSnapshot {
            step_start: self.step_start,
            slide_start: self.slide_start,
            step_end: self.step_end,
            show_start: self.show_start,
            last_move: self.last_move,
        }
    }

    /// Presenter elapsed seconds at `now`, using the same clock as the view.
    #[cfg(test)]
    pub(crate) fn presenter_elapsed(&self, now: f64) -> f64 {
        self.timer.elapsed(now)
    }

    /// True while the pointer moved in the last 2.5 s; the media controls and GIF button hide after that.
    fn pointer_awake(&self, now: f64) -> bool {
        now - self.last_move < 2.5
    }

    /// Whether a still pointer is hidden: after 2.5 s in a full-screen show, unless Keep mouse
    /// cursor visible is on. Reading View always keeps it.
    fn hides_idle_cursor(&self, now: f64) -> bool {
        !self.pointer_awake(now) && !self.reading && !self.keep_cursor
    }

    /// The red Record button starts capture: restart the slide clocks so the recorded timing
    /// begins at this frame, let the microphone start for this take, and empty the buffers so a
    /// new take replaces the previous one instead of adding to it.
    fn begin_recording(&mut self, now: f64) {
        self.step_start = now;
        self.slide_start = now;
        self.step_end = now;
        self.show_start = now;
        self.last_move = now;
        self.record_paused_at = None;
        self.mic_tried = false;
        self.mic_error = None;
        self.capturing = true;
        self.timings.clear();
        self.recorded.clear();
        self.laser_saved.clear();
        self.narration.clear();
        self.narration_cut = false;
        // Strokes drawn before Record are the presenter's own browsing, not part of the take.
        self.strokes.clear();
        self.laser_live.clear();
    }

    /// Hold the slide clocks for as long as the recording is paused. The animation clock is
    /// shifted on resume, so recorded timings do not count the pause.
    fn pause_recording(&mut self, now: f64) {
        self.capturing = false;
        if self.record_paused_at.is_none() {
            self.record_paused_at = Some(now);
            self.pause_slide = Some(self.state.slide);
        }
    }

    fn resume_recording(&mut self, now: f64) {
        let Some(at) = self.record_paused_at.take() else { return };
        self.capturing = true;
        let dt = (now - at).max(0.0);
        // A slide opened while the take was held starts its clock at resume: shifting its
        // slide-entry time by the pause would undercount it (or go negative).
        let moved = self.pause_slide.take().is_some_and(|held| held != self.state.slide);
        if moved {
            self.step_start = now;
            self.slide_start = now;
        } else if dt.is_finite() {
            self.step_start += dt;
            self.slide_start += dt;
            self.step_end += dt;
        }
        if dt.is_finite() {
            self.show_start += dt;
            self.last_move += dt;
        }
    }

    /// The take stopped: the capture paths stop keeping timings, ink, laser and narration, while
    /// what they already kept stays for the save path.
    fn end_recording(&mut self) {
        self.capturing = false;
        self.record_paused_at = None;
        self.pause_slide = None;
    }

    /// The take stopped on the current slide: its time there is the last timing of the take.
    fn end_take_timing(&mut self, now: f64) {
        let secs = now - self.slide_start;
        if secs.is_finite() && secs >= 0.0 {
            self.timings.push((self.state.slide, secs));
        }
    }

    /// Pen and laser are different pointers: turning one on puts the other away.
    fn set_pen(&mut self, on: bool) {
        if on {
            self.set_ink_mode(InkMode::Pen);
        } else {
            self.ink_mode = InkMode::Arrow;
            self.pen = false;
        }
    }

    fn set_laser(&mut self, on: bool) {
        self.laser = on;
        if on {
            self.pen = false;
            self.ink_mode = InkMode::Arrow;
        }
    }

    /// The arrow pointer: no ink and no laser dot.
    fn arrow(&mut self) {
        self.pen = false;
        self.laser = false;
        self.ink_mode = InkMode::Arrow;
    }

    /// Choose the ink tool. The pen and highlighter capture pointer drags; the eraser removes
    /// whole strokes and leaves no ink of its own. Selecting any ink tool puts the laser away.
    pub(crate) fn set_ink_mode(&mut self, mode: InkMode) {
        self.ink_mode = mode;
        self.pen = mode.draws();
        if mode != InkMode::Arrow {
            self.laser = false;
        }
    }

    pub(crate) fn ink_mode(&self) -> InkMode {
        self.ink_mode
    }

    /// The colour the pen and highlighter use.
    pub(crate) fn set_ink_color(&mut self, color: Rgba) {
        self.ink_color = color;
    }

    pub(crate) fn ink_color(&self) -> Rgba {
        self.ink_color
    }

    /// True while the selected tool leaves ink (the pen or the highlighter).
    pub(crate) fn ink_active(&self) -> bool {
        self.ink_mode.draws()
    }

    /// Whether the canvas has live strokes the eraser could remove.
    pub(crate) fn has_ink(&self) -> bool {
        !self.strokes.is_empty()
    }

    pub(crate) fn stroke_count(&self) -> usize {
        self.strokes.len()
    }

    /// Remove every stroke touched by the eraser segment `a`–`b` (normalized slide points).
    /// Whole-stroke removal is PowerPoint's default eraser. Returns how many strokes went.
    pub(crate) fn erase_ink_at(&mut self, a: Pos2, b: Pos2, radius: f32) -> usize {
        let radius = radius.max(0.0);
        let before = self.strokes.len();
        self.strokes.retain(|s| !stroke_touched(s, a, b, radius));
        before - self.strokes.len()
    }

    /// The backdrop for `key`: kept from earlier, waited for if it is being rendered ahead of
    /// time, or rendered now.
    fn backdrop(&mut self, doc: &std::sync::Arc<Presentation>, key: BackKey) -> std::sync::Arc<deckcraft_render::Image> {
        if let Some(b) = self.backdrop.as_mut().filter(|b| b.key == key && std::sync::Arc::ptr_eq(&b.doc, doc)) {
            if b.img.is_none() {
                b.img = b.rx.take().and_then(|rx| rx.recv().ok()).map(std::sync::Arc::new);
            }
            if let Some(img) = &b.img {
                return img.clone();
            }
        }
        let img = std::sync::Arc::new(render_backdrop(doc, key, render_threads()));
        self.backdrop = Some(Backdrop { doc: doc.clone(), key, img: Some(img.clone()), rx: None });
        img
    }

    /// Start rendering the backdrop for `key` on another thread, unless it is already there or
    /// on its way. Nothing on the web (no threads): the first animation frame renders it.
    fn prefetch_backdrop(&mut self, doc: &std::sync::Arc<Presentation>, key: BackKey) {
        if cfg!(target_arch = "wasm32") || self.backdrop.as_ref().is_some_and(|b| b.key == key && std::sync::Arc::ptr_eq(&b.doc, doc)) {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let worker_doc = doc.clone();
        let threads = render_threads().saturating_sub(1).max(1);
        let spawned = std::thread::Builder::new().name("show-backdrop".into()).spawn(move || {
            let _ = tx.send(render_backdrop(&worker_doc, key, threads));
        });
        self.backdrop = spawned.ok().map(|_| Backdrop { doc: doc.clone(), key, img: None, rx: Some(rx) });
    }

    fn timeline(&mut self, doc: &Presentation) -> &Timeline {
        let i = self.state.slide;
        if self.timeline.as_ref().is_none_or(|(k, _)| *k != i) {
            self.timeline = Some((i, ShowState::timeline(doc, i)));
        }
        match &self.timeline {
            Some((_, t)) => t,
            None => {
                static EMPTY: std::sync::OnceLock<Timeline> = std::sync::OnceLock::new();
                EMPTY.get_or_init(Timeline::default)
            }
        }
    }

    fn apply(&mut self, a: ShowAction, doc: &Presentation, now: f64) -> bool {
        match a {
            ShowAction::PlayStep(k) => {
                self.step_start = now;
                self.media_clock = Some((k, f64::NEG_INFINITY));
            }
            ShowAction::FinishStep(n) => {
                // A step cut short by a click still sets off the media it holds.
                self.cue_media(doc, n, f64::INFINITY);
                self.step_end = now;
            }
            ShowAction::StepBack(_) => {
                self.media_clock = None;
                self.step_end = now;
            }
            ShowAction::Slide(i) => {
                self.ended = false;
                self.zoom = None;
                self.zoom_arm = false;
                let left = self.clock_slide.unwrap_or(i);
                self.clock_slide = Some(i);
                let recording = self.keeps_recording();
                if recording {
                    self.timings.push((left, now - self.slide_start));
                }
                self.stash_ink(doc, left, recording, now);
                self.stash_laser(left, recording);
                // Take the old slide's texture rather than cloning its handle: the new slide is
                // otherwise rendered into the same texture, and the transition blends the new
                // slide with itself (#33).
                let (from, old) = self.tex.take().map(|(k, t)| (k.slide, t)).unzip();
                if from != Some(i) {
                    self.last_viewed = from;
                }
                // Going back plays the transition the slide left behind came in with, reversed
                // (#1531); a queued Back that a later step overtook keeps the forward one.
                let back = self.state.backward && self.state.slide == i && from.is_some_and(|f| f != i);
                let (kind, option, dur) = doc
                    .slides
                    .get(if back { from.unwrap_or(i) } else { i })
                    .and_then(|s| s.transition.as_ref())
                    .map(|t| (t.kind.clone(), t.option.clone(), t.duration_ms as f64 / 1000.0))
                    .unwrap_or(("none".into(), String::new(), 0.0));
                // Morph needs no reversing: a Morph from the old slide to the new one is the
                // reversed Morph.
                let reverse = back && kind != "morph";
                self.trans = if kind != "none" && dur > 0.0 && !doc.show.without_animation {
                    Some(Transition { old, from, morph: None, parts: None, start: now, kind, option, dur, reverse })
                } else {
                    None
                };
                self.slide_start = now;
                self.step_start = now;
                self.step_end = now;
                self.media_clock = None;
                self.strokes.clear();
                if self.preview_only {
                    return false;
                }
                self.after_enter(doc, now);
            }
            ShowAction::End => {
                self.ended = true;
                if self.preview_only {
                    return false;
                }
            }
            ShowAction::Exit => return false,
            ShowAction::None => {}
        }
        true
    }

    /// On entering a slide: auto-play its first step when it begins With/After Previous.
    /// `ShowState` already starts such a step when it enters the slide (#631: asking it for the
    /// next step again finished the step at once, so the slide appeared fully built), so only
    /// its clock starts here. Autoplay previews start an On Click first step themselves.
    fn after_enter(&mut self, doc: &Presentation, now: f64) {
        let started = if self.state.playing {
            Some(self.state.step)
        } else if self.autoplay {
            match self.state.next(doc) {
                ShowAction::PlayStep(k) => Some(k),
                _ => None,
            }
        } else {
            None
        };
        if let Some(k) = started {
            self.step_start = now;
            self.media_clock = Some((k, f64::NEG_INFINITY));
        }
    }

    /// Queue the media effects (Play, Pause, Stop) of main step `step` that the show reached since
    /// the last call, up to `t` seconds into the step (#116).
    fn cue_media(&mut self, doc: &Presentation, step: usize, t: f64) {
        let Some((k, from)) = self.media_clock else { return };
        if k != step {
            self.media_clock = None;
            return;
        }
        let slide = self.state.slide;
        let cues = self.timeline(doc).media_cues(step, from, t);
        let effects: Vec<(ShapeId, String)> =
            cues.iter().filter_map(|e| doc.slides.get(slide).and_then(|s| s.animations.get(e.anim)).map(|a| (e.shape, a.effect.clone()))).collect();
        self.media_cues.extend(effects);
        self.media_clock = if t.is_finite() { Some((k, t)) } else { None };
    }

    /// Move the pen's strokes into the recording, in slide points. Always clears the live strokes.
    /// `keep` is the caller's recording decision, so the final save can stash a take that has
    /// already stopped.
    fn stash_ink(&mut self, doc: &Presentation, slide: usize, keep: bool, now: f64) {
        let at_ms = ((now - self.slide_start).max(0.0) * 1000.0) as u32;
        let strokes = std::mem::take(&mut self.strokes);
        let w = doc.slide_size.width;
        let h = doc.slide_size.height;
        if !keep || !(w > 0.0 && h > 0.0) {
            return;
        }
        for stroke in strokes {
            // Ink drawn while the take was paused is the presenter's, not the recording's.
            if !stroke.in_take {
                continue;
            }
            let mut pts = Vec::new();
            for q in stroke.points.iter().take(4096) {
                let x = q.x as f64 * w;
                let y = q.y as f64 * h;
                if x.is_finite() && y.is_finite() {
                    pts.push((x, y));
                }
            }
            if pts.len() >= 2 {
                self.recorded.push((slide, pts, stroke.mode, stroke.color, stroke.width, at_ms));
            }
        }
    }

    fn stash_laser(&mut self, slide: usize, keep: bool) {
        let pts = std::mem::take(&mut self.laser_live);
        if keep && !pts.is_empty() {
            self.laser_saved.push((slide, pts));
        }
    }

    fn note_laser(&mut self, x: f64, y: f64, t_ms: u32) {
        if !x.is_finite() || !y.is_finite() || self.laser_live.len() >= 4096 {
            return;
        }
        if let Some((px, py, pt)) = self.laser_live.last() {
            let moved = (x - px).hypot(y - py) >= 1.5;
            if t_ms.saturating_sub(*pt) < 40 && !moved {
                return;
            }
        }
        self.laser_live.push((x, y, t_ms));
    }

    /// Whether the pointer is over the Record view's chrome: clicks there must not advance the
    /// show, take a stroke, or follow a link.
    fn record_bar_hit(&self, ctx: &egui::Context) -> bool {
        ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| self.record_bar_hit_at(ctx, p))
    }

    /// Whether `pos` lies on the Record view's chrome (or its teleprompter overlay), where a click
    /// or wheel belongs to the shell and not to the slide.
    fn record_bar_hit_at(&self, ctx: &egui::Context, pos: Pos2) -> bool {
        self.record_chrome() && self.record_view.chrome_contains(ctx.content_rect(), pos)
    }
}

fn state_for(tl: &Timeline, id: ShapeId, step: usize, t: f64, plays: &[TriggerPlay], stamps: &[StepStamp]) -> ShapeState {
    let a = tl.state_ordered(id, None, step, t, plays, stamps);
    ShapeState {
        visible: a.visible,
        opacity: a.opacity,
        offset: deckcraft_geom::Vec2::new(a.offset_x, a.offset_y),
        scale: (a.scale_x, a.scale_y),
        rotate: a.rotate,
        clip: a.clip,
        tint: a.tint,
        paras: tl
            .paragraph_targets(id)
            .into_iter()
            .map(|p| {
                let a = tl.state(id, Some(p), step, t);
                (p, ParaState { visible: a.visible, opacity: a.opacity, offset: deckcraft_geom::Vec2::new(a.offset_x, a.offset_y) })
            })
            .collect(),
    }
}

/// Whether starting the show takes the whole screen. Reading view and "Browsed by an individual (window)" stay in the window.
fn show_fills_screen(reading: bool, show_type: &str) -> bool {
    !reading && show_type != "browsed"
}

/// Slide points per on-screen pixel, so the 3px stroke keeps its width in the deck. `srect` is
/// the rectangle the slide is actually drawn in — the Record view's is smaller than the window.
fn pen_width_slide(doc: &Presentation, srect: Rect) -> f64 {
    let scale = srect.width() / doc.slide_size.width.max(1.0) as f32;
    if scale.is_finite() && scale > 0.01 { (3.0 / scale) as f64 } else { 2.25 }
}

/// A narration clip is left off when Play Narrations is unchecked; other media still plays.
fn play_on_enter(clip: &deckcraft_model::MediaClip, without_narration: bool) -> bool {
    clip.autoplay && !(clip.narration && without_narration)
}

/// The microphone a take writes into: the app's service slot, borrowed so a section can hand the
/// take over without holding the whole app.
pub(crate) type MicSlot = Option<Box<dyn deckcraft_media::MicIn>>;

/// Start the automatic clips whose slide-start delay has come. The visible pass and the hidden
/// logic tick both call this, on the same step clock.
pub(crate) fn start_due_media(app: &mut SlideApp, show: &mut Show, doc_uid: u64, doc: &Presentation, now: f64) {
    media::run_pending_media(app, show, doc_uid, doc, now);
}

/// Queue the clip a slide starts with, as the visible pass does on entry. A hidden window that
/// advances has no visible pass, so without this a slide's automatic narration stays silent.
pub(crate) fn queue_entry_media(app: &mut SlideApp, show: &mut Show, doc_uid: u64, doc: &Presentation, now: f64) {
    media::show_media(app, show, doc_uid, doc, now);
}

/// Write the take's buffered timings, narration, ink and laser into the deck. The shell's finish
/// path, a commanded `show.end` and a confirmed quit all end here, so no path can drop a take.
pub(crate) fn save_take(app: &mut SlideApp, show: &mut Show, doc: &Presentation, now: f64) {
    if !(show.rehearse || (show.record && show.record_view.captured)) {
        return;
    }
    let live = show.rehearse || show.record_view.status == record::RecordStatus::Recording;
    // Only this take's microphone counts: `failure()` stays set after a device is lost until the
    // next `start`, so an unrelated earlier take must not colour this one's status.
    if show.mic_tried
        && let Some(e) = app.services.mic.as_ref().and_then(|mic| mic.failure())
    {
        // The take is no longer being captured: narrate what was recorded, but keep why it
        // stopped so the saved status does not read as a complete take.
        show.mic_error = Some(e);
    }
    if show.record {
        // The microphone's buffer belongs to the slide the show ends on.
        let ending = show.state.slide;
        take_narration(app.services.mic.as_deref_mut(), show, ending);
    }
    if show.mic_tried {
        app.stop_mic();
    }
    show.stash_ink(doc, show.state.slide, live, now);
    show.stash_laser(show.state.slide, live);
    if live {
        show.timings.push((show.state.slide, now - show.slide_start));
    }
    // The whole save is one undo step: hundreds of separate commands would evict the user's own
    // history (`edit.batch` runs them inside one outer command, which pushes one entry).
    let mut commands: Vec<serde_json::Value> = Vec::new();
    // A Record take replaces what its slides had recorded, ink included (`ink.stroke` appends to
    // the slide's recorded layer). A rehearsal keeps timings only, and must not delete the deck's
    // existing recording.
    if show.record {
        let mut touched: Vec<usize> = show.timings.iter().map(|(i, _)| *i).collect();
        touched.extend(show.recorded.iter().map(|(i, ..)| *i));
        touched.extend(show.narration.iter().map(|(i, ..)| *i));
        touched.extend(show.laser_saved.iter().map(|(i, _)| *i));
        touched.sort_unstable();
        touched.dedup();
        for i in touched {
            commands.push(json!({"id": "show.clearRecording", "params": {"scope": "current", "index": i}}));
        }
    }
    for (i, secs) in show.timings.clone() {
        let ms = if secs.is_finite() { (secs.max(0.0) * 1000.0).round() as u64 } else { 0 };
        commands.push(json!({"id": "transition.timing", "params": {"index": i, "after": ms}}));
    }
    let mut cut = 0usize;
    // The clips the deck actually gained: near-silence and too-short takes never become one.
    let mut recorded_narration = 0usize;
    if show.record {
        for (i, pts, mode, color, width, at_ms) in &show.recorded {
            let points: Vec<_> = pts.iter().map(|(x, y)| json!([x, y, 0.5])).collect();
            commands.push(json!({"id": "ink.stroke", "params": {
                "index": i,
                "points": points,
                "width": width,
                "color": [color.r, color.g, color.b],
                "highlighter": *mode == InkMode::Highlighter,
                "at": at_ms,
            }}));
        }
        for (i, rate, samples) in &show.narration {
            // Encoded once, here: the clip is final.
            let Some(wav) = narration_wav(*rate, samples) else { continue };
            cut += usize::from(wav.cut);
            recorded_narration = recorded_narration.saturating_add(1);
            commands.push(json!({"id": "show.narration", "params": {
                "index": i, "name": "narration.wav", "data": deckcraft_engine::cmd::base64_encode(&wav.wav)
            }}));
        }
        let lc = doc.show.laser_color;
        for (i, pts) in &show.laser_saved {
            let points: Vec<_> = pts.iter().map(|(x, y, t)| json!([x, y, t])).collect();
            commands.push(json!({"id": "show.laser", "params": {"index": i, "points": points, "color": [lc.r, lc.g, lc.b]}}));
        }
        commands.push(json!({"id": "show.setup", "params": {"useTimings": true}}));
    }
    let mut failed: Vec<String> = Vec::new();
    if !commands.is_empty() {
        match app.run("edit.batch", json!({"commands": commands})) {
            Ok(v) => {
                failed = v
                    .get("failed")
                    .and_then(|f| f.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default()
            }
            Err(e) => failed.push(e),
        }
    }
    if show.record {
        let narrated = recorded_narration;
        let limit = if cut == 0 && !show.narration_cut {
            None
        } else {
            let slides = cut.max(usize::from(show.narration_cut));
            Some(format!(
                "the narration on {slides} slide{} was cut at the {:.0} MB limit",
                if slides == 1 { "" } else { "s" },
                MAX_NARRATION_BYTES as f64 / 1_000_000.0
            ))
        };
        // A part that would not save is omitted work too, and it is named first.
        let parts = if failed.is_empty() {
            None
        } else {
            Some(format!("{} part{} of the take could not be saved ({})", failed.len(), if failed.len() == 1 { "" } else { "s" }, failed[0]))
        };
        let status = match (&parts, &show.mic_error, &limit) {
            (Some(p), _, _) => format!("Recording saved, but {p}"),
            // A microphone that stopped mid-take makes "saved" a half truth: say what happened.
            (None, Some(e), Some(l)) => format!("Recording saved, but the microphone stopped: {e}, and {l}"),
            (None, Some(e), None) => format!("Recording saved, but the microphone stopped: {e}"),
            (None, None, Some(l)) => format!("Recording saved, but {l}"),
            (None, None, None) => saved_status(narrated > 0, !show.recorded.is_empty(), !show.laser_saved.is_empty()).to_string(),
        };
        app.set_status(status);
    } else {
        app.set_status("Slide timings recorded.");
    }
}

/// The largest narration WAV `show.narration` accepts, in bytes. The engine rejects anything
/// larger, so a take is capped to this before it is encoded.
const MAX_NARRATION_BYTES: usize = 48_000_000;

/// The samples a WAV can hold at [`MAX_NARRATION_BYTES`]: a mono 16-bit header is 44 bytes.
fn narration_sample_cap() -> usize {
    (MAX_NARRATION_BYTES - 44) / 2
}

/// How many samples a slide's clip can still take: its ten-minute allowance and the WAV cap, both
/// measured from what it already holds.
fn narration_room(rate: u32, kept: usize) -> usize {
    (rate as usize).saturating_mul(600).saturating_sub(kept).min(narration_sample_cap().saturating_sub(kept))
}

/// A slide's encoded narration, and whether the byte limit cut its tail.
struct NarrationWav {
    wav: Vec<u8>,
    cut: bool,
}

/// Skip near-silence and clips shorter than 0.2 s so an idle microphone adds no shape.
fn narration_wav(rate: u32, samples: &[f32]) -> Option<NarrationWav> {
    narration_wav_within(rate, samples, MAX_NARRATION_BYTES)
}

/// [`narration_wav`] with an explicit byte limit, so the cut can be tested without a 48 MB take.
fn narration_wav_within(rate: u32, samples: &[f32], max_bytes: usize) -> Option<NarrationWav> {
    if !(8_000..=192_000).contains(&rate) {
        return None;
    }
    // The engine refuses a clip under 200 ms; a sample of margin keeps the boundary from rounding
    // the wrong way once the WAV is probed.
    let min_n = (rate as u64).saturating_mul(200) / 1000 + (rate as u64) / 100;
    // `show.narration` rejects a WAV over `max_bytes`: cap the samples here so the take that
    // passes never fails on the way in, and report the cut so the save is not called complete.
    let max = max_bytes.saturating_sub(44) / 2;
    let cut = samples.len() > max;
    let samples = &samples[..samples.len().min(max)];
    let mut sum = 0.0f64;
    let mut n = 0u64;
    for s in samples.iter() {
        if s.is_finite() {
            let v = *s as f64;
            sum += v * v;
            n += 1;
        }
    }
    if n < min_n.max(1) {
        return None;
    }
    let rms = (sum / n as f64).sqrt();
    if !rms.is_finite() || rms < 0.008 {
        return None;
    }
    Some(NarrationWav { wav: deckcraft_media::wav_mono(rate, samples).ok()?, cut })
}

fn saved_status(narr: bool, ink: bool, laser: bool) -> &'static str {
    match (narr, ink, laser) {
        (false, false, false) => "Recording saved: slide timings.",
        (true, false, false) => "Recording saved: slide timings and narration.",
        (false, true, false) => "Recording saved: slide timings and ink.",
        (false, false, true) => "Recording saved: slide timings and laser.",
        (true, true, false) => "Recording saved: slide timings, narration and ink.",
        (true, false, true) => "Recording saved: slide timings, narration and laser.",
        (false, true, true) => "Recording saved: slide timings, ink and laser.",
        (true, true, true) => "Recording saved: slide timings, narration, ink and laser.",
    }
}

/// The laser dot: the button (or L while recording) leaves it on, and holding Ctrl shows it
/// in any show. A blank screen hides it. It never draws ink.
fn pointer_is_laser(sticky: bool, ctrl: bool, blank: bool) -> bool {
    !blank && (sticky || ctrl)
}

fn laser_pos(points: &[deckcraft_model::LaserSample], elapsed_ms: u32) -> Option<(f64, f64)> {
    let last = points.last()?;
    if elapsed_ms > last.t_ms.saturating_add(300) {
        return None;
    }
    let mut chosen = &points[0];
    for p in points {
        if p.t_ms <= elapsed_ms {
            chosen = p;
        } else {
            break;
        }
    }
    Some((chosen.x, chosen.y))
}

/// A slide change ends the microphone take and hands it to the slide being left.
fn before_slide_change(app: &mut SlideApp, show: &mut Show, action: &ShowAction) {
    before_slide_change_with(&mut app.services.mic, show, action);
}

/// The same, for a section that holds the microphone rather than the whole app.
fn before_slide_change_with(mic: &mut MicSlot, show: &mut Show, action: &ShowAction) {
    if show.keeps_recording() && matches!(action, ShowAction::Slide(_)) {
        // Navigation has already entered the destination: the take belongs to the slide left,
        // the same index `Show::apply` records its timing under.
        let left = show.clock_slide.unwrap_or(show.state.slide);
        take_narration(mic.as_deref_mut(), show, left);
    }
}

/// Hand the microphone's buffer to the show's current slide. The callers decide whether a take is
/// running: the end of a take also saves what was spoken on the slide it stopped on.
fn take_narration(mic: Option<&mut (dyn deckcraft_media::MicIn + 'static)>, show: &mut Show, slide: usize) {
    if !show.mic_ok {
        return;
    }
    let Some(mic) = mic else { return };
    // The device drops samples once its buffer is full; `take` cannot show them, so the take is
    // reported as cut rather than complete.
    if mic.dropped() {
        show.narration_cut = true;
    }
    let (rate, samples) = mic.take();
    if samples.is_empty() || !(8_000..=192_000).contains(&rate) {
        return;
    }
    match show.narration.iter_mut().find(|(i, _, _)| *i == slide) {
        // A resumed take adds to the same slide's clip, keeping its rate.
        Some((_, take_rate, kept)) if *take_rate == rate => {
            // The clip also has to fit the engine's WAV limit: buffering more would only be cut
            // later, and the save has to say so.
            let room = narration_room(rate, kept.len());
            if samples.len() > room {
                show.narration_cut = true;
            }
            kept.extend(samples.into_iter().take(room));
        }
        // A replacement clip keeps the same limits as an append.
        Some(entry) => {
            if samples.len() > narration_sample_cap() {
                show.narration_cut = true;
            }
            *entry = (slide, rate, samples.into_iter().take(narration_sample_cap()).collect());
        }
        None => {
            if samples.len() > narration_sample_cap() {
                show.narration_cut = true;
            }
            show.narration.push((slide, rate, samples.into_iter().take(narration_sample_cap()).collect()));
        }
    }
}

/// Whether `stroke` comes within `radius` of the eraser's segment `a`–`b`, in normalized slide
/// coordinates. Each stroke segment is measured, so a stroke the eraser crosses between two
/// samples is caught. The eraser removes the whole stroke, as PowerPoint's default eraser does.
fn stroke_touched(stroke: &InkStroke, a: Pos2, b: Pos2, radius: f32) -> bool {
    match stroke.points.as_slice() {
        [] => false,
        [p] => point_segment_distance(*p, a, b) <= radius,
        points => points.windows(2).any(|w| segment_distance(w[0], w[1], a, b) <= radius),
    }
}

/// Distance between the segments `a`–`b` and `c`–`d`; zero when they cross.
fn segment_distance(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> f32 {
    if segments_cross(a, b, c, d) {
        return 0.0;
    }
    point_segment_distance(a, c, d).min(point_segment_distance(b, c, d)).min(point_segment_distance(c, a, b)).min(point_segment_distance(d, a, b))
}

/// Whether the segments `a`–`b` and `c`–`d` cross, ends touching included.
fn segments_cross(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> bool {
    let side = |p: Pos2, q: Pos2, r: Pos2| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    let (ab_c, ab_d) = (side(a, b, c), side(a, b, d));
    let (cd_a, cd_b) = (side(c, d, a), side(c, d, b));
    (ab_c > 0.0) != (ab_d > 0.0) && (cd_a > 0.0) != (cd_b > 0.0)
}

/// Distance from `p` to the segment `a`–`b`; the segment of a click has zero length.
fn point_segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if !len2.is_finite() || len2 <= f32::EPSILON {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

#[cfg(test)]
mod tests;
