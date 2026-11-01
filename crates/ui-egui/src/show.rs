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

mod keys;
mod media;
mod presenter;
mod render;
mod ui;
mod zoom;

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
            if !holding && (step_due || (!self.rehearse && self.state.auto_advance_due(doc, idle) && !self.ended)) {
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
                if self.rehearse {
                    self.timings.push((self.timeline.as_ref().map(|t| t.0).unwrap_or(i), now - self.slide_start));
                }
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

#[cfg(test)]
mod tests;
