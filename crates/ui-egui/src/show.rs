//! Slide Show: full-screen playback with transitions and animations (`deckcraft-anim`),
//! keyboard/mouse navigation, pen, black/white screens, See All Slides, zoom, presenter view
//! and rehearsed timings.

use deckcraft_anim::{Click, HitShape, Layer, LinkJump, ShowAction, ShowState, Source, StepStamp, Timeline, TriggerPlay};
use deckcraft_model::text::Hyperlink;
use deckcraft_model::{Presentation, ShapeId};
use deckcraft_render::{ParaState, PlacedLink, PlacedShape, RenderOpts, ShapeState};
use egui::{Align2, Color32, CornerRadius, Mesh, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::theme;

pub(super) mod keys;
mod media;
mod presenter;
mod record;
mod render;
pub(crate) mod ui;
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

pub struct Show {
    /// Monotonic identity of this show instance. Starting a show again (even at the same slide
    /// and step) is a new instance, so a queued click cannot cross an ABA transition.
    pub(crate) instance: u64,
    pub state: ShowState,
    pub reading: bool,
    pub presenter: bool,
    /// Preview from the Transitions/Animations tab: exits after the current slide.
    pub preview_only: bool,
    /// Play every step automatically (animation preview).
    pub autoplay: bool,
    pub rehearse: bool,
    /// Record Slide Show: keep timings, narration, pen ink and laser when the show ends.
    pub record: bool,
    /// The UI clock of the last frame. A command runs between frames and needs the same time base
    /// the shell and the show's timers use.
    pub(crate) last_now: f64,
    /// The slide the show has applied (the one on screen). `ShowState` enters the destination
    /// before `apply` runs, so the capture paths must not read `state.slide`.
    applied_slide: usize,
    /// The slide whose take segment is open: its length is not in `timings` yet. Cleared when the
    /// segment is filed, so a stopped take is never attributed to the slide shown afterwards.
    take_slide: Option<usize>,
    /// The Record view shell's own state (clock, layout, sections, device hooks).
    pub(crate) record_view: record::RecordView,
    /// When a recording pause started; the show clock is shifted by the pause length.
    record_paused_at: Option<f64>,
    /// Set Up Slide Show ▸ Keep mouse cursor visible: a still pointer is not hidden (#1445).
    pub keep_cursor: bool,
    timeline: Option<(usize, Timeline)>,
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
    strokes: Vec<Vec<Pos2>>,
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
    /// Pen strokes kept while recording, in slide points, with the slide they were drawn on.
    recorded: Vec<(usize, Vec<(f64, f64)>)>,
    /// Laser samples kept while recording: slide points and milliseconds from the slide start.
    laser_live: Vec<(f64, f64, u32)>,
    laser_saved: Vec<(usize, Vec<(f64, f64, u32)>)>,
    /// Narration per slide: the microphone's rate and the mono samples taken from it. A take that
    /// is paused and resumed on the same slide continues the same entry, so no audio is lost.
    narration: Vec<(usize, u32, Vec<f32>)>,
    mic_tried: bool,
    mic_ok: bool,
    /// The slide the last narration samples were filed under. Pause/resume on one visit appends;
    /// coming back to a slide in the same take replaces what it had.
    narration_slide: Option<usize>,
    /// A resumed take was cut at the ten-minute limit, so the save status must not claim the
    /// narration was kept in full.
    narration_cut: bool,
    /// Pen width in slide points, matching the 3px stroke on screen.
    ink_width: f64,
    last_move: f64,
    /// The last trackpad flick or mouse-wheel notch, so one gesture turns one page.
    last_swipe: f64,
    pub ended: bool,
    /// The slide whose media was last started (auto-play runs once per visit).
    media_slide: Option<usize>,
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
            autoplay: false,
            rehearse: false,
            record: false,
            record_view: record::RecordView::default(),
            last_now: 0.0,
            applied_slide: from.min(doc.slides.len().saturating_sub(1)),
            take_slide: None,
            record_paused_at: None,
            keep_cursor: false,
            timeline: None,
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
            mic_tried: false,
            mic_ok: false,
            narration_slide: None,
            narration_cut: false,
            ink_width: 2.25,
            last_move: 0.0,
            last_swipe: f64::NEG_INFINITY,
            ended: false,
            media_slide: None,
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

    /// Whether hidden logic still has time-driven work, so it must keep requesting wake-ups.
    pub(crate) fn hidden_time_driven(&self, doc: &Presentation) -> bool {
        self.started && !self.ended && (self.state.playing || self.trans.is_some() || self.autoplay || self.state.advance_after(doc).is_some())
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
            if !holding && (step_due || (!(self.rehearse || self.record) && self.state.auto_advance_due(doc, idle) && !self.ended)) {
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
    /// begins at this frame, and let the microphone start for this take.
    fn begin_recording(&mut self, now: f64) {
        // A new take owns its own narration: it replaces what an earlier take left on this slide,
        // while pause and resume inside a take still append (see `take_narration`).
        self.narration_slide = None;
        self.narration_cut = false;
        self.step_start = now;
        self.slide_start = now;
        self.step_end = now;
        self.show_start = now;
        self.last_move = now;
        self.record_paused_at = None;
        self.mic_tried = false;
        self.take_slide = Some(self.applied_slide);
    }

    /// Hold the slide clocks for as long as the recording is paused. The animation clock is
    /// shifted on resume, so recorded timings do not count the pause.
    fn pause_recording(&mut self, now: f64) {
        if self.record_paused_at.is_none() {
            self.record_paused_at = Some(now);
        }
    }

    fn resume_recording(&mut self, now: f64) {
        let Some(at) = self.record_paused_at.take() else { return };
        let dt = (now - at).max(0.0);
        if dt.is_finite() {
            self.step_start += dt;
            self.slide_start += dt;
            self.step_end += dt;
            self.show_start += dt;
            self.last_move += dt;
        }
    }

    /// Pen and laser are different pointers: turning one on puts the other away.
    fn set_pen(&mut self, on: bool) {
        self.pen = on;
        if on {
            self.laser = false;
        }
    }

    fn set_laser(&mut self, on: bool) {
        self.laser = on;
        if on {
            self.pen = false;
        }
    }

    /// The arrow pointer: no ink and no laser dot.
    fn arrow(&mut self) {
        self.pen = false;
        self.laser = false;
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
                // A new visit to a slide starts its own narration: audio filed for a slide the show
                // left must not be appended to (the timing keeps the last visit too).
                self.narration_slide = None;
                let left = self.applied_slide;
                self.applied_slide = i;
                if self.rehearse {
                    self.timings.push((left, now - self.slide_start));
                } else if self.take_captured() {
                    // A take that is running, paused or stopped still owns the slide being left:
                    // file its segment under that slide now, so leaving never loses it and finish
                    // never attributes it to the slide shown afterwards.
                    self.finalize_take_slide(now);
                    if self.record_view.status != record::RecordStatus::Idle {
                        self.take_slide = Some(i);
                        if self.record_paused_at.is_some() {
                            // The pause belonged to the slide left behind; the new slide's pause,
                            // if any, starts now.
                            self.record_paused_at = Some(now);
                        }
                    }
                }
                self.stash_ink(doc, left);
                self.stash_laser(left);
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

    /// Move the pen's strokes into the recording, in slide points, when a take is capturing.
    /// Always clears the live strokes.
    fn stash_ink(&mut self, doc: &Presentation, slide: usize) {
        if self.take_captured() {
            self.keep_ink(doc, slide);
        } else {
            self.strokes.clear();
        }
    }

    /// File the live strokes under `slide` whatever the take's state: the end of a take uses this
    /// so ink drawn while recording is kept even when Stop came before the show closed.
    fn keep_ink(&mut self, doc: &Presentation, slide: usize) {
        let strokes = std::mem::take(&mut self.strokes);
        let w = doc.slide_size.width;
        let h = doc.slide_size.height;
        if !(w > 0.0 && h > 0.0) {
            return;
        }
        for stroke in strokes {
            let mut pts = Vec::new();
            for q in stroke.iter().take(4096) {
                let x = q.x as f64 * w;
                let y = q.y as f64 * h;
                if x.is_finite() && y.is_finite() {
                    pts.push((x, y));
                }
            }
            if pts.len() >= 2 {
                self.recorded.push((slide, pts));
            }
        }
    }

    /// File the live laser trace under `slide` when a take is capturing; always clears it.
    fn stash_laser(&mut self, slide: usize) {
        if self.take_captured() {
            self.keep_laser(slide);
        } else {
            self.laser_live.clear();
        }
    }

    /// File the live laser trace under `slide` whatever the take's state (see [`keep_ink`]).
    ///
    /// [`keep_ink`]: Self::keep_ink
    fn keep_laser(&mut self, slide: usize) {
        let pts = std::mem::take(&mut self.laser_live);
        if !pts.is_empty() {
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

    /// Whether the narration microphone was started for this show (so a show being replaced can
    /// stop the stream it owns).
    pub(crate) fn mic_started(&self) -> bool {
        self.mic_tried
    }

    /// Whether the narration microphone actually started: the shell reports this, not the attempt.
    pub(crate) fn mic_live(&self) -> bool {
        self.mic_ok
    }

    /// File the open take segment's length under `slide`, once. Stop froze it; a running segment
    /// is measured to `now`. A segment under a millisecond is not a visit and is dropped, so
    /// navigating never writes a zero advance.
    /// File the segment a take has open, under the slide that owns it: `take_slide` records it, and
    /// the caller's idea of the current slide can already be the destination (a Stop click in the
    /// same frame as a page turn arrives after `state.slide` moved).
    fn finalize_take_slide(&mut self, now: f64) {
        let Some(owner) = self.take_slide.take() else { return };
        let secs = self.record_view.frozen_slide_seconds().unwrap_or_else(|| self.captured_slide_seconds(now));
        if secs.is_finite() && secs * 1000.0 >= 1.0 {
            self.timings.push((owner, secs));
        }
    }

    /// Save what a take captured (timings, narration, ink and laser) and stop the microphone.
    /// `end_show` calls this for the visible and the hidden end of a show alike.
    pub(crate) fn save_take(&mut self, app: &mut SlideApp, doc: &Presentation, now: f64) {
        let recording = self.take_captured();
        if !(self.rehearse || recording) {
            return;
        }
        if recording {
            take_narration(app, self);
        }
        if self.mic_tried {
            app.stop_mic();
        }
        if recording {
            // Whatever was drawn or pointed at during the take is filed under its slide even if
            // Stop came before the show closed.
            self.keep_ink(doc, self.applied_slide);
            self.keep_laser(self.applied_slide);
            self.finalize_take_slide(now);
        } else {
            self.stash_ink(doc, self.applied_slide);
            self.stash_laser(self.applied_slide);
            self.timings.push((self.applied_slide, now - self.slide_start));
        }
        for (i, secs) in self.timings.clone() {
            let ms = if secs.is_finite() { (secs.max(0.0) * 1000.0).round() as u64 } else { 0 };
            let _ = app.run("transition.timing", json!({"index": i, "after": ms}));
        }
        if !recording {
            app.set_status("Slide timings recorded.");
            return;
        }
        let c = doc.show.pen_color;
        for (i, pts) in &self.recorded {
            let points: Vec<_> = pts.iter().map(|(x, y)| json!([x, y, 0.5])).collect();
            let _ = app.run("ink.stroke", json!({"index": i, "points": points, "width": self.ink_width, "color": [c.r, c.g, c.b]}));
        }
        let mut narrations = 0usize;
        // Resampling and WAV building are the heaviest part of saving a take (a ten-minute
        // 96 kHz track is 57.6M samples): one worker per slide does it off the UI thread, and
        // the finished files are inserted here in slide order.
        let wavs: Vec<(usize, Vec<u8>)> = std::thread::scope(|scope| {
            let workers: Vec<_> = self
                .narration
                .iter()
                .map(|(i, rate, samples)| {
                    let (i, rate, samples) = (*i, *rate, samples.as_slice());
                    scope.spawn(move || narration_wav(rate, samples).map(|wav| (i, wav)))
                })
                .collect();
            workers.into_iter().filter_map(|w| w.join().ok().flatten()).collect()
        });
        for (i, wav) in wavs {
            let _ = app.run("show.narration", json!({"index": i, "name": "narration.wav", "data": deckcraft_engine::cmd::base64_encode(&wav)}));
            narrations += 1;
        }
        let lc = doc.show.laser_color;
        for (i, pts) in &self.laser_saved {
            let points: Vec<_> = pts.iter().map(|(x, y, t)| json!([x, y, t])).collect();
            let _ = app.run("show.laser", json!({"index": i, "points": points, "color": [lc.r, lc.g, lc.b]}));
        }
        let _ = app.run("show.setup", json!({"useTimings": true}));
        app.set_status(saved_status(narrations > 0, !self.recorded.is_empty(), !self.laser_saved.is_empty(), self.narration_cut));
    }

    /// The UI clock of the last frame, for a command that runs between frames.
    pub(crate) fn command_now(&self) -> f64 {
        self.last_now
    }

    /// Seconds this slide has been captured: up to the pause while a take is held, `now` while it
    /// runs. A take that stopped keeps the value `RecordView::stop` froze.
    pub(crate) fn captured_slide_seconds(&self, now: f64) -> f64 {
        let at = self.record_paused_at.unwrap_or(now);
        (at - self.slide_start).max(0.0)
    }

    /// Run a Record-view command (`record.start`, `record.toggle`, `record.pause`, `record.resume`,
    /// `record.stop`, `record.reset`, `record.undo`) with this show's clock. The shell's buttons
    /// and the command channel share one implementation. Returns the take's status after it.
    pub(crate) fn record_command(&mut self, id: &str, now: f64) -> Result<&'static str, String> {
        let mut rec = std::mem::take(&mut self.record_view);
        let r = record::command(&mut rec, self, id, now);
        self.record_view = rec;
        r
    }

    /// Whether this show has a take at all: it captured something, whether that take is running,
    /// paused or already stopped. Its ink, laser and narration are still waiting for the finish
    /// path, so nothing may drop them.
    pub(crate) fn take_captured(&self) -> bool {
        self.record && self.record_view.captured
    }

    /// Whether a take is capturing right now: the Record view is open *and* running. Timings,
    /// ink, laser and narration belong to the take, so browsing the Record view before Record,
    /// while paused, or after Stop must not collect them. `record` alone is the mode flag and is
    /// only for layout and chrome.
    pub(crate) fn capturing(&self) -> bool {
        self.record && self.record_view.status == record::RecordStatus::Recording
    }

    /// Whether the pointer is over the Record view's chrome: clicks there must not advance the
    /// show, take a stroke, or follow a link.
    fn record_bar_hit(&self, ctx: &egui::Context) -> bool {
        if !self.record {
            return false;
        }
        ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| self.record_view.chrome_contains(ctx.content_rect(), p))
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

/// Slide points per on-screen pixel, so the 3px stroke keeps its width in the deck.
fn pen_width_slide(doc: &Presentation, full: Rect, reading: bool) -> f64 {
    let avail = if reading { Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - 34.0)) } else { full };
    let srect = render::fit(avail, doc);
    let scale = srect.width() / doc.slide_size.width.max(1.0) as f32;
    if scale.is_finite() && scale > 0.01 { (3.0 / scale) as f64 } else { 2.25 }
}

/// A narration clip is left off when Play Narrations is unchecked; other media still plays.
fn play_on_enter(clip: &deckcraft_model::MediaClip, without_narration: bool) -> bool {
    clip.autoplay && !(clip.narration && without_narration)
}

/// Skip near-silence and clips shorter than 0.2 s so an idle microphone adds no shape.
/// The samples one slide's narration keeps at `rate`: the ten minutes `wav_mono` writes
/// ([`deckcraft_media::audio::wav_mono`]), so a resumed take is cut here, in the take's own rate,
/// rather than silently by the writer.
fn narration_cap(rate: u32) -> usize {
    (rate as usize).saturating_mul(60 * 10)
}

fn narration_wav(rate: u32, samples: &[f32]) -> Option<Vec<u8>> {
    if !(8_000..=192_000).contains(&rate) {
        return None;
    }
    let min_n = (rate as u64).saturating_mul(200) / 1000;
    // Ten minutes at the device's own rate: a 96 kHz take holds 57.6M samples, so a fixed
    // 48 kHz cap would only hear the first five minutes.
    let len = samples.len().min((rate as usize).saturating_mul(60 * 10));
    // Audibility is per one-second window: a second of speech must survive even when the rest
    // of the ten-minute take is silence, which would dilute a whole-take RMS below the gate.
    let window = (rate as usize).max(1);
    let mut total = 0u64;
    let mut audible = false;
    for w in samples[..len].chunks(window) {
        let mut sum = 0.0f64;
        let mut n = 0u64;
        for s in w {
            if s.is_finite() {
                let v = *s as f64;
                sum += v * v;
                n += 1;
            }
        }
        total += n;
        if n > 0 {
            let rms = (sum / n as f64).sqrt();
            if rms.is_finite() && rms >= 0.008 {
                audible = true;
                // One audible second is enough: stop scanning the other nine minutes. The
                // window also covers the minimum length, so `total` is not needed past this.
                break;
            }
        }
    }
    if !audible || total < min_n.max(1) {
        return None;
    }
    // Narration is written at NARRATION_RATE whatever the device gave us: the WAV cap is ten
    // minutes at the written rate, so a 96 kHz microphone keeps its full take instead of half.
    let resampled;
    let (rate, samples) = if rate == NARRATION_RATE {
        (rate, samples)
    } else {
        // Borrowing resample: the take is not cloned just to be converted.
        resampled = deckcraft_media::audio::resample_samples(rate, 1, samples, NARRATION_RATE);
        (resampled.rate, resampled.samples.as_slice())
    };
    deckcraft_media::wav_mono(rate, samples).ok()
}

/// The rate narration is written at: ten minutes fit [`deckcraft_media::MAX_WAV_SAMPLES`].
const NARRATION_RATE: u32 = 48_000;

fn saved_status(narr: bool, ink: bool, laser: bool, narration_cut: bool) -> String {
    let base = match (narr, ink, laser) {
        (false, false, false) => "Recording saved: slide timings.",
        (true, false, false) => "Recording saved: slide timings and narration.",
        (false, true, false) => "Recording saved: slide timings and ink.",
        (false, false, true) => "Recording saved: slide timings and laser.",
        (true, true, false) => "Recording saved: slide timings, narration and ink.",
        (true, false, true) => "Recording saved: slide timings, narration and laser.",
        (false, true, true) => "Recording saved: slide timings, ink and laser.",
        (true, true, true) => "Recording saved: slide timings, narration, ink and laser.",
    };
    if narration_cut {
        // A resumed take stopped at the ten-minute limit: say so instead of reporting it saved.
        return format!("{base} The narration was cut at ten minutes.");
    }
    base.to_string()
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
    if show.capturing() && matches!(action, ShowAction::Slide(_)) {
        take_narration(app, show);
    }
}

fn take_narration(app: &mut SlideApp, show: &mut Show) {
    if !show.record || !show.mic_ok {
        return;
    }
    let Some(mic) = app.services.mic.as_mut() else { return };
    let (rate, samples) = mic.take();
    if samples.is_empty() {
        return;
    }
    // `ShowState::next`/`prev`/`goto` already moved the state to the new slide; the take belongs
    // to the one the show has applied, which is the slide being left.
    let slide = show.applied_slide;
    let same_visit = show.narration_slide == Some(slide);
    if let Some(i) = show.narration.iter().position(|(s, _, _)| *s == slide).filter(|_| same_visit) {
        // A take paused on this slide and resumed: the microphone restarted, so its samples are
        // appended. A restarted device can report another rate; the take is one buffer at the
        // first rate, so the new samples are resampled to it instead of playing at the wrong speed.
        let stored = show.narration[i].1;
        let appended = if stored == rate || stored == 0 || rate == 0 {
            samples
        } else {
            let mono = deckcraft_media::audio::Pcm { rate, channels: 1, samples };
            deckcraft_media::audio::resample(&mono, stored).samples
        };
        // Stop where `wav_mono` stops (`narration_cap`, the writer's ten minutes) instead of
        // appending samples the file would silently drop.
        let room = narration_cap(stored.max(1)).saturating_sub(show.narration[i].2.len());
        if appended.len() > room {
            show.narration_cut = true;
        }
        show.narration[i].2.extend_from_slice(&appended[..appended.len().min(room)]);
    } else {
        // A new visit to the slide replaces what it had: the timing and the laser keep the last
        // visit too, so the audio must not be longer than the slide's recorded advance.
        show.narration.retain(|(s, _, _)| *s != slide);
        show.narration.push((slide, rate, samples));
    }
    show.narration_slide = Some(slide);
}

#[cfg(test)]
mod tests;
