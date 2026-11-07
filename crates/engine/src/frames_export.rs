//! Frame sequence of the slide show (`file.export` with format `frames`): one PNG per video frame,
//! `frame-000001.png`… in a folder, at a constant frame rate, for a video encoder to join.
//!
//! Each exported slide plays its build through its [`Timeline`] as the show would: every main
//! step from `t = 0` to its duration at `1/fps`, then the settled state held `secondsPerStep`
//! (an automatic step runs straight into the next); after the last step the slide is held
//! `secondsPerSlide`. A slide without animations is one still held `secondsPerSlide`. Between
//! slides the incoming slide's own transition (kind, option and duration) plays frame by frame,
//! composited like the show does ([`deckcraft_render::composite_transition`]; Morph is rendered
//! shape by shape); `none` cuts. Sound and video don't play (a frame shows a video's first frame).
//!
//! All parameters are hostile: rate, holds, width and the slide list are checked against fixed
//! bounds, the total number of frames and of pixels is capped before anything is rendered or
//! written, and the bytes written are capped as they are written ([`MAX_TOTAL_BYTES`]), so a
//! request can't fill a disk or run for hours. Identical consecutive frames (holds)
//! are encoded once and written as copies.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use deckcraft_anim::{ShowState, Timeline, state_for, transition_frame, uses_slide_parts};
use deckcraft_model::{Presentation, ShapeId};
use deckcraft_render::{Image, RenderOpts, ShapeState, SlideParts, composite_transition};
use serde_json::Value;

use crate::cmd::bad;
use crate::gif_export::{MAX_WIDTH, MIN_WIDTH, frame_size};
use crate::{EngineError, Result};

const CMD: &str = "file.export";

pub const DEFAULT_FPS: u32 = 30;
pub const MIN_FPS: u32 = 1;
pub const MAX_FPS: u32 = 60;
pub const DEFAULT_SECONDS_PER_STEP: f64 = 1.0;
pub const MAX_SECONDS_PER_STEP: f64 = 30.0;
pub const DEFAULT_SECONDS_PER_SLIDE: f64 = 3.0;
pub const MAX_SECONDS_PER_SLIDE: f64 = 60.0;
/// Frame width when none is given (Full HD for 16:9 slides).
pub const DEFAULT_WIDTH: u32 = 1920;
/// Most frames in one export: 10 minutes at 30 fps.
pub const MAX_FRAMES: usize = 18_000;
/// Cap on frames × width × height: [`MAX_FRAMES`] Full HD frames fit, 4K frames for a third of
/// that time.
pub const MAX_TOTAL_PIXELS: u64 = 40_000_000_000;
/// Most bytes one export may write, every copy of a held frame counted: a PNG's size depends on
/// what the slide shows, so the pixel cap alone does not bound the disk used.
pub const MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024 * 1024;
/// A step or transition longer than this (a hostile `duration_ms` in the file) is cut here; the
/// frame cap then decides.
const MAX_PLAY_SECONDS: f64 = 600.0;

/// What to export.
#[derive(Clone, Debug, PartialEq)]
pub struct FramesOptions {
    pub fps: u32,
    /// Hold after a click step's animations have settled.
    pub seconds_per_step: f64,
    /// Hold after a slide's last step (the whole time of a slide without animations).
    pub seconds_per_slide: f64,
    /// Play the slides' transitions between them (`false`: cut).
    pub transitions: bool,
    /// Use the slides' Advance Slide After times, as the show does with timings (`None`: the deck's
    /// Use Timings setting).
    pub use_timings: Option<bool>,
    /// Frame width in pixels; the height follows the slide's aspect ratio.
    pub width: u32,
    /// Slide indices in play order (default: all).
    pub slides: Option<Vec<usize>>,
    /// Hidden slides are left out unless this is set.
    pub include_hidden: bool,
}

impl Default for FramesOptions {
    fn default() -> Self {
        FramesOptions {
            fps: DEFAULT_FPS,
            seconds_per_step: DEFAULT_SECONDS_PER_STEP,
            seconds_per_slide: DEFAULT_SECONDS_PER_SLIDE,
            transitions: true,
            use_timings: None,
            width: DEFAULT_WIDTH,
            slides: None,
            include_hidden: false,
        }
    }
}

/// Present and not `null`.
fn given<'a>(p: &'a Value, key: &str) -> Option<&'a Value> {
    p.get(key).filter(|v| !v.is_null())
}

/// A finite number of seconds in `0..=max`.
fn seconds(v: &Value, key: &str, max: f64) -> Result<f64> {
    v.as_f64()
        .filter(|s| s.is_finite() && (0.0..=max).contains(s))
        .ok_or_else(|| bad(CMD, format!("`{key}` must be a number of seconds from 0 to {max}")))
}

impl FramesOptions {
    /// Read `{fps?, secondsPerStep?, secondsPerSlide?, transitions?, useTimings?, width?, slides?:
    /// [index], includeHidden?}`. Absent or `null` values take their defaults; anything out of range or of
    /// the wrong type is an error.
    pub fn from_params(p: &Value) -> Result<Self> {
        let mut o = FramesOptions::default();
        if let Some(v) = given(p, "fps") {
            o.fps = v
                .as_u64()
                .and_then(|f| u32::try_from(f).ok())
                .filter(|f| (MIN_FPS..=MAX_FPS).contains(f))
                .ok_or_else(|| bad(CMD, format!("`fps` must be a whole number of frames per second from {MIN_FPS} to {MAX_FPS}")))?;
        }
        if let Some(v) = given(p, "secondsPerStep") {
            o.seconds_per_step = seconds(v, "secondsPerStep", MAX_SECONDS_PER_STEP)?;
        }
        if let Some(v) = given(p, "secondsPerSlide") {
            o.seconds_per_slide = seconds(v, "secondsPerSlide", MAX_SECONDS_PER_SLIDE)?;
        }
        if let Some(v) = given(p, "transitions") {
            o.transitions = v.as_bool().ok_or_else(|| bad(CMD, "`transitions` must be true or false"))?;
        }
        if let Some(v) = given(p, "useTimings") {
            o.use_timings = Some(v.as_bool().ok_or_else(|| bad(CMD, "`useTimings` must be true or false"))?);
        }
        if let Some(v) = given(p, "width") {
            o.width = v
                .as_u64()
                .and_then(|w| u32::try_from(w).ok())
                .filter(|w| (MIN_WIDTH..=MAX_WIDTH).contains(w))
                .ok_or_else(|| bad(CMD, format!("`width` must be a whole number of pixels from {MIN_WIDTH} to {MAX_WIDTH}")))?;
        }
        if let Some(v) = given(p, "slides") {
            let a = v.as_array().ok_or_else(|| bad(CMD, "`slides` must be a list of slide indices"))?;
            if a.is_empty() {
                return Err(bad(CMD, "`slides` is empty"));
            }
            if a.len() > MAX_FRAMES {
                return Err(bad(CMD, format!("a frame sequence can show at most {MAX_FRAMES} slides")));
            }
            let list = a
                .iter()
                .map(|x| x.as_u64().and_then(|i| usize::try_from(i).ok()))
                .collect::<Option<Vec<usize>>>()
                .ok_or_else(|| bad(CMD, "each entry of `slides` must be a non-negative integer (a 0-based index)"))?;
            o.slides = Some(list);
        }
        if let Some(v) = given(p, "includeHidden") {
            o.include_hidden = v.as_bool().ok_or_else(|| bad(CMD, "`includeHidden` must be true or false"))?;
        }
        Ok(o)
    }

    /// The slides to play, in order: the chosen ones (or all), without hidden slides unless
    /// `include_hidden`.
    pub fn order(&self, doc: &Presentation) -> Result<Vec<usize>> {
        let n = doc.slides.len();
        let chosen: Vec<usize> = match &self.slides {
            Some(list) => {
                if let Some(i) = list.iter().find(|i| **i >= n) {
                    return Err(bad(CMD, format!("no slide at index {i}; indices run 0 through {}", n.saturating_sub(1))));
                }
                list.clone()
            }
            None => (0..n).collect(),
        };
        let order: Vec<usize> = chosen.into_iter().filter(|i| self.include_hidden || doc.slides.get(*i).is_some_and(|s| !s.hidden)).collect();
        if order.is_empty() {
            return Err(bad(CMD, "no slides to export (they are all hidden, or the presentation is empty)"));
        }
        Ok(order)
    }
}

/// One frame of the sequence: what to draw.
#[derive(Clone, Debug, PartialEq)]
enum Frame {
    /// Slide `slide` with `step` main steps done and step `step` playing for `t` seconds
    /// (`step == steps()`: everything settled).
    Build { slide: usize, step: usize, t: f64 },
    /// The transition into `to` from `from`, `p` (0..1) of the way through.
    Transition { from: usize, to: usize, p: f64 },
}

/// First and last frame number (from 1) of one exported slide; its transition in counts as its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlideRange {
    pub index: usize,
    pub from: usize,
    pub to: usize,
}

/// Every frame of the export, decided before anything is drawn.
#[derive(Clone, Debug)]
pub struct Plan {
    frames: Vec<Frame>,
    pub slides: Vec<SlideRange>,
    pub width: u32,
    pub height: u32,
}

impl Plan {
    pub fn frames(&self) -> usize {
        self.frames.len()
    }
}

/// A finite number of seconds from the document, cut to [`MAX_PLAY_SECONDS`].
fn play_seconds(s: f64) -> f64 {
    if s.is_finite() && s > 0.0 { s.min(MAX_PLAY_SECONDS) } else { 0.0 }
}

/// `v` rounded to the nearest even number (at least 2; `MAX_SIDE` is even, so it stays in range).
fn even(v: u32) -> u32 {
    (v.saturating_add(1) / 2 * 2).max(2)
}

/// Decide the frames of `doc` under `opts`, refusing when there would be more than
/// [`MAX_FRAMES`] or more than [`MAX_TOTAL_PIXELS`] pixels.
pub fn plan(doc: &Presentation, opts: &FramesOptions) -> Result<Plan> {
    let order = opts.order(doc)?;
    let fps = f64::from(opts.fps.clamp(MIN_FPS, MAX_FPS));
    // Frames a span of `s` seconds takes while playing (the frame at its end belongs to what follows).
    let playing = |s: f64| (play_seconds(s) * fps).ceil().clamp(0.0, 1e9) as usize;
    // Frames a hold of `s` seconds takes.
    let holding = |s: f64| (play_seconds(s) * fps).round().clamp(0.0, 1e9) as usize;
    let hold_step = holding(opts.seconds_per_step);
    let hold_slide = holding(opts.seconds_per_slide).max(1);
    // With timings, a slide's Advance Slide After time is how long it waits idle before each click
    // step and before it advances, as the show plays it; slides without one take the holds above.
    let use_timings = opts.use_timings.unwrap_or(doc.show.use_timings);
    let after = |slide: usize| {
        let ms = doc.slides.get(slide)?.transition.as_ref()?.advance_after_ms?;
        use_timings.then(|| holding(f64::from(ms) / 1000.0))
    };
    // Video encoders working in 4:2:0 refuse odd sizes, so both sides are even.
    let (width, height) = frame_size(doc, opts.width);
    let (width, height) = (even(width), even(height));
    let too_many = |n: usize| {
        bad(
            CMD,
            format!(
                "{n} frames is more than the {MAX_FRAMES} a frame sequence can have ({} minutes at {} fps): lower fps, secondsPerSlide or secondsPerStep, or export fewer slides",
                MAX_FRAMES / 60 / DEFAULT_FPS as usize,
                DEFAULT_FPS
            ),
        )
    };
    let mut frames: Vec<Frame> = Vec::new();
    let mut slides = Vec::with_capacity(order.len());
    let mut prev: Option<usize> = None;
    for &slide in &order {
        let from = frames.len() + 1;
        // Show without animation cuts between slides.
        if let Some(from_slide) = prev.filter(|_| opts.transitions && !doc.show.without_animation)
            && let Some(tr) = doc.slides.get(slide).and_then(|s| s.transition.as_ref())
            && tr.kind != "none"
        {
            let dur = play_seconds(f64::from(tr.duration_ms) / 1000.0);
            let n = playing(dur);
            // At most MAX_PLAY_SECONDS × MAX_FPS: counted against the cap below.
            frames.extend((0..n).map(|j| Frame::Transition { from: from_slide, to: slide, p: ((j as f64 / fps) / dur).clamp(0.0, 1.0) }));
        }
        let tl: Timeline = ShowState::timeline(doc, slide);
        let steps = tl.steps();
        let (hold_step, hold_slide) = after(slide).map_or((hold_step, hold_slide), |a| (a, a.max(1)));
        if doc.show.without_animation {
            // The show plays no animation and shows each slide fully built: one settled still.
            frames.extend(std::iter::repeat_n(Frame::Build { slide, step: steps, t: 0.0 }, hold_slide));
        } else if steps == 0 {
            frames.extend(std::iter::repeat_n(Frame::Build { slide, step: 0, t: 0.0 }, hold_slide));
        }
        // What the slide shows before its first click is held like any settled state; an automatic
        // first step plays at once, so there is nothing to hold.
        if steps > 0 && !doc.show.without_animation && !tl.step_is_auto(0) {
            frames.extend(std::iter::repeat_n(Frame::Build { slide, step: 0, t: 0.0 }, hold_step));
        }
        for step in 0..if doc.show.without_animation { 0 } else { steps } {
            let n = playing(tl.step_duration(step));
            frames.extend((0..n).map(|j| Frame::Build { slide, step, t: j as f64 / fps }));
            let hold = if step + 1 == steps {
                hold_slide
            } else {
                // Only the first main step can start by itself (`Timeline`), so every later one
                // follows a click and the pause before it is a step hold.
                hold_step
            };
            frames.extend(std::iter::repeat_n(Frame::Build { slide, step: step + 1, t: 0.0 }, hold));
            if frames.len() > MAX_FRAMES {
                return Err(too_many(frames.len()));
            }
        }
        if frames.len() > MAX_FRAMES {
            return Err(too_many(frames.len()));
        }
        slides.push(SlideRange { index: slide, from, to: frames.len() });
        prev = Some(slide);
    }
    let total = (frames.len() as u64).saturating_mul(u64::from(width)).saturating_mul(u64::from(height));
    if total > MAX_TOTAL_PIXELS {
        return Err(bad(
            CMD,
            format!(
                "{} frames at {width}×{height} is {total} pixels, more than the {MAX_TOTAL_PIXELS} a frame sequence can have: lower width, fps, secondsPerSlide or secondsPerStep, or export fewer slides",
                frames.len()
            ),
        ));
    }
    Ok(Plan { frames, slides, width, height })
}

/// What was written.
#[derive(Clone, Debug)]
pub struct FramesOutput {
    pub folder: String,
    pub frames: usize,
    pub fps: u32,
    pub width: u32,
    pub height: u32,
    pub slides: Vec<SlideRange>,
}

impl FramesOutput {
    /// Length of the sequence in seconds.
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.fps.max(1))
    }
}

/// The file name of frame `n` (from 1).
pub fn frame_name(n: usize) -> String {
    format!("frame-{n:06}.png")
}

/// Is `name` a frame file this export writes (`frame-` + six digits + `.png`)?
fn is_frame_name(name: &str) -> bool {
    name.strip_prefix("frame-").and_then(|r| r.strip_suffix(".png")).is_some_and(|d| d.len() == 6 && d.bytes().all(|b| b.is_ascii_digit()))
}

fn render_threads() -> u16 {
    if cfg!(target_arch = "wasm32") { 0 } else { std::thread::available_parallelism().map(|n| n.get().min(8) as u16).unwrap_or(0) }
}

/// Draws the frames of one export, keeping the last few slide images (a hold's settled frame, the
/// two slides of a transition) so nothing is rendered twice.
struct Painter<'a> {
    doc: &'a Presentation,
    size: (u32, u32),
    scale: f64,
    threads: u16,
    /// Slide `.0`'s timeline.
    timeline: Option<(usize, Timeline)>,
    /// Recently drawn build frames, newest last.
    builds: Vec<(Frame, Arc<Image>)>,
    /// Backgrounds and contents of the transition `.0` → `.1` in progress.
    parts: Option<(usize, usize, SlideParts)>,
}

impl Painter<'_> {
    fn opts(&self) -> RenderOpts<'static> {
        RenderOpts { scale: self.scale, size: Some(self.size), threads: self.threads, ..Default::default() }
    }

    fn timeline(&mut self, slide: usize) -> &Timeline {
        if self.timeline.as_ref().is_none_or(|(k, _)| *k != slide) {
            self.timeline = Some((slide, ShowState::timeline(self.doc, slide)));
        }
        match &self.timeline {
            Some((_, t)) => t,
            None => {
                static EMPTY: std::sync::OnceLock<Timeline> = std::sync::OnceLock::new();
                EMPTY.get_or_init(Timeline::default)
            }
        }
    }

    /// Slide `slide` at `step`, `t` seconds in.
    fn build(&mut self, slide: usize, step: usize, t: f64) -> Arc<Image> {
        let key = Frame::Build { slide, step, t };
        if let Some((_, img)) = self.builds.iter().find(|(k, _)| *k == key) {
            return Arc::clone(img);
        }
        let tl = self.timeline(slide).clone();
        let f = move |id: ShapeId| ShapeState::from(state_for(&tl, id, step, t, &[], &[]));
        let img = Arc::new(deckcraft_render::render_slide(self.doc, slide, &RenderOpts { state: Some(&f), ..self.opts() }));
        if self.builds.len() >= 3 {
            self.builds.remove(0);
        }
        self.builds.push((key, Arc::clone(&img)));
        img
    }

    /// The transition into `to` from `from` at progress `p`: the settled outgoing slide going out,
    /// the incoming slide as it is before its first step coming in.
    fn transition(&mut self, from: usize, to: usize, p: f64) -> Arc<Image> {
        let doc = self.doc;
        let (kind, option) =
            doc.slides.get(to).and_then(|s| s.transition.as_ref()).map(|tr| (tr.kind.clone(), tr.option.clone())).unwrap_or_default();
        let steps_from = self.timeline(from).steps();
        if kind == "morph"
            && let (Some(a), Some(b)) = (doc.slides.get(from), doc.slides.get(to))
            && p < 1.0
        {
            let tl = self.timeline(to).clone();
            let hidden_b = |id: ShapeId| {
                let st = state_for(&tl, id, 0, 0.0, &[], &[]).state;
                !st.visible || st.opacity <= 0.0
            };
            let m = deckcraft_anim::morph_frame(doc, a, b, p, &hidden_b);
            let f = |id: ShapeId| ShapeState { opacity: m.opacity_of(id), ..Default::default() };
            let o = RenderOpts { state: Some(&f), ..self.opts() };
            let text: Vec<deckcraft_render::TextMorph> = m
                .text
                .iter()
                .map(|x| deckcraft_render::TextMorph { old: x.old, new: x.new, from: x.from, to: x.to, chars: x.chars, t: m.mix })
                .collect();
            let paths: Vec<deckcraft_render::PathMorph> = m
                .paths
                .iter()
                .map(|x| deckcraft_render::PathMorph { id: x.id, from: x.from.clone(), from_box: x.from_box, to_box: x.to_box, t: m.mix })
                .collect();
            return Arc::new(deckcraft_render::render_blend(doc, (from, to), m.mix, &m.shapes, &text, &paths, &o));
        }
        let old = self.build(from, steps_from, 0.0);
        let new = self.build(to, 0, 0.0);
        if uses_slide_parts(&kind, &option) && self.parts.as_ref().is_none_or(|(a, b, _)| (*a, *b) != (from, to)) {
            self.parts = Some((from, to, SlideParts::render(doc, Some(from), to, &self.opts())));
        }
        let parts = self.parts.as_ref().filter(|(a, b, _)| (*a, *b) == (from, to)).map(|(_, _, p)| p);
        let aspect = doc.slide_size.width / doc.slide_size.height.max(1.0);
        Arc::new(composite_transition(Some(&old), &new, parts, &transition_frame(&kind, &option, p, aspect)))
    }

    fn draw(&mut self, f: &Frame) -> Arc<Image> {
        match *f {
            Frame::Build { slide, step, t } => self.build(slide, step, t),
            Frame::Transition { from, to, p } => self.transition(from, to, p),
        }
    }
}

/// One PNG to encode once and write under every number in `numbers` (a hold is one image).
struct Job {
    numbers: Vec<usize>,
    image: Arc<Image>,
}

/// Encode `job` and write its copies, counting their bytes in `written` (shared by the workers);
/// past `cap` bytes the export fails instead of writing on.
fn encode_and_write(folder: &std::path::Path, job: &Job, written: &AtomicU64, cap: u64) -> std::result::Result<(), String> {
    let bytes = job.image.to_png();
    if bytes.is_empty() {
        return Err("a frame could not be encoded as PNG".into());
    }
    let add = (bytes.len() as u64).saturating_mul(job.numbers.len() as u64);
    if written.fetch_add(add, Ordering::Relaxed).saturating_add(add) > cap {
        return Err(format!(
            "the frames come to more than the {cap} bytes a frame sequence may write: lower width, fps, secondsPerSlide or secondsPerStep, or export fewer slides"
        ));
    }
    for &n in &job.numbers {
        crate::io::write_file_direct(&folder.join(frame_name(n)).to_string_lossy(), &bytes)?;
    }
    Ok(())
}

/// Encode and write the jobs of `rx` on a few threads; the first error stops the rest.
#[cfg(not(target_arch = "wasm32"))]
fn run_jobs(folder: &std::path::Path, jobs: impl Iterator<Item = Job>, cap: u64) -> std::result::Result<(), String> {
    let workers = std::thread::available_parallelism().map(|n| n.get().clamp(1, 8)).unwrap_or(1);
    let (tx, rx) = std::sync::mpsc::sync_channel::<Job>(workers * 2);
    let rx = std::sync::Mutex::new(rx);
    let failed = std::sync::Mutex::new(None::<String>);
    let written = AtomicU64::new(0);
    // The workers write under the caller's automation roots (or lack of them), not the process's.
    let roots = crate::file_access::current();
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let job = match rx.lock() {
                        Ok(r) => r.recv(),
                        Err(_) => return,
                    };
                    let Ok(job) = job else { return };
                    let write = || encode_and_write(folder, &job, &written, cap);
                    let written = match &roots {
                        Some(r) => crate::file_access::confine(Some(r), write),
                        None => crate::file_access::unconfined(write),
                    };
                    if let Err(e) = written
                        && let Ok(mut f) = failed.lock()
                    {
                        f.get_or_insert(e);
                    }
                }
            });
        }
        for job in jobs {
            if failed.lock().ok().is_some_and(|f| f.is_some()) {
                break;
            }
            if tx.send(job).is_err() {
                break;
            }
        }
        drop(tx);
    });
    match failed.into_inner() {
        Ok(Some(e)) => Err(e),
        _ => Ok(()),
    }
}

#[cfg(target_arch = "wasm32")]
fn run_jobs(folder: &std::path::Path, jobs: impl Iterator<Item = Job>, cap: u64) -> std::result::Result<(), String> {
    let written = AtomicU64::new(0);
    for job in jobs {
        encode_and_write(folder, &job, &written, cap)?;
    }
    Ok(())
}

/// Render the frames of `doc` under `opts` into `folder` (created if missing). The new sequence
/// replaces the earlier `frame-NNNNNN.png` files only once every frame is written; an export that
/// fails before then (a write error, the byte cap) leaves them as they were. If removing them
/// afterwards fails, the error says so and the new frames stay. Nothing is written when the request is refused, when a frame's name
/// in `folder` is taken by something other than a regular file (a folder, a link), when an
/// interrupted export left earlier frames in its backup folder, or when another export to the same
/// folder (in this process or another) is still running: one runs at a time per folder.
pub fn export(doc: &Presentation, opts: &FramesOptions, folder: &str) -> Result<FramesOutput> {
    export_capped(doc, opts, folder, MAX_TOTAL_BYTES)
}

/// The folder inside the export folder that a new sequence is written to before it replaces the
/// old one; a dot name, so it is not mistaken for part of the sequence.
const STAGING: &str = ".deckcraft-frames-new";

/// The folder inside the export folder that the earlier sequence moves to while the new one moves
/// in, so a move that fails part way can put it back.
const BACKUP: &str = ".deckcraft-frames-old";

/// The lock file inside the export folder that one export holds from its first check to its last
/// cleanup, so exports to the same folder cannot share the staging and backup folders.
const LOCK: &str = ".deckcraft-frames-lock";

/// Replace the frames in `folder` with those in `staging`. The earlier ones move to `backup` first
/// and are removed only once every new one is in place; a move that fails part way removes the new
/// ones it placed and puts the earlier ones back, so the folder never holds half a sequence
/// (unless putting them back fails too, which the error says).
fn replace_sequence(folder: &str, staging: &str, backup: &str) -> std::result::Result<(), String> {
    crate::io::create_folder(backup)?;
    if let Err(e) = crate::io::move_files(folder, backup, &is_frame_name) {
        return Err(restore_sequence(folder, backup, e));
    }
    if let Err(e) = crate::io::move_files(staging, folder, &is_frame_name) {
        if let Err(r) = crate::io::remove_files(folder, &is_frame_name) {
            return Err(format!("{e}; the new frames already moved could not be removed ({r}), and the earlier frames are in {backup}"));
        }
        return Err(restore_sequence(folder, backup, e));
    }
    crate::io::remove_files(backup, &is_frame_name).map_err(|e| {
        format!("the new frames are in place, but the earlier ones in {backup} could not all be removed ({e}): delete them before the next export")
    })?;
    // The new sequence is in place: emptied folders that won't go are left, not an error.
    let _ = crate::io::remove_empty_folder(backup);
    let _ = crate::io::remove_empty_folder(staging);
    Ok(())
}

/// Put the earlier frames in `backup` back into `folder` after `error`; answers the error to report.
fn restore_sequence(folder: &str, backup: &str, error: String) -> String {
    match crate::io::move_files(backup, folder, &is_frame_name) {
        Ok(_) => {
            let _ = crate::io::remove_empty_folder(backup);
            error
        }
        Err(r) => format!("{error}; the earlier frames could not all be put back ({r}) and the rest are in {backup}"),
    }
}

/// [`export`] with the byte cap as a parameter, so a test can make the export fail part way.
fn export_capped(doc: &Presentation, opts: &FramesOptions, folder: &str, cap: u64) -> Result<FramesOutput> {
    let plan = plan(doc, opts)?;
    let dir = std::path::Path::new(folder);
    let io = |e: String| EngineError::Other(format!("frames export: {e}"));
    crate::file_access::check_write(&dir.join(frame_name(1)).to_string_lossy()).map_err(io)?;
    crate::io::create_folder(folder).map_err(io)?;
    let _lock = crate::io::lock_folder(folder, LOCK).map_err(io)?;
    // The clearing below removes regular files only. A frame name taken by anything else would
    // survive it: a folder makes the move fail after the earlier frames are gone, and a link past
    // the new sequence's end would carry it on with an old frame. Any such name in the folder is
    // refused before anything is written.
    if let Some(odd) = crate::io::non_file_named(folder, &is_frame_name).map_err(io)? {
        return Err(io(format!("{odd} is not a regular file (a folder, a link, …): move it away and export again; nothing was written")));
    }
    // The new frames go to a staging folder first: a failed export (a write error, the byte cap)
    // leaves the earlier sequence whole instead of half replaced. A staging folder an interrupted
    // export left behind is cleared first.
    let staging = dir.join(STAGING);
    let staging_s = staging.to_string_lossy().to_string();
    // A work folder that is a link could point at the export folder, and the clearing below would
    // delete the earlier frames (or the new ones) through it: refuse both before any cleanup.
    for work in [STAGING, BACKUP] {
        if crate::io::is_link(&dir.join(work).to_string_lossy()) {
            return Err(io(format!("{} is a link: delete it and export again; nothing was written", dir.join(work).to_string_lossy())));
        }
    }
    // A backup folder still holding frames is an earlier sequence that an interrupted export could
    // not put back (or a finished one could not remove): it is the user's to sort out, so it is
    // refused rather than overwritten.
    let backup_s = dir.join(BACKUP).to_string_lossy().to_string();
    if crate::io::count_files(&backup_s, &is_frame_name).map_err(io)? > 0 {
        return Err(io(format!(
            "{backup_s} still holds earlier frames (an export was interrupted, or could not put them back or remove them): delete them, or, if the frames in the folder are incomplete, delete those and move these back; then export again; nothing was written"
        )));
    }
    crate::io::remove_files(&staging_s, &is_frame_name).map_err(io)?;
    crate::io::create_folder(&staging_s).map_err(io)?;
    let scale = f64::from(plan.width) / doc.slide_size.width;
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let mut painter = Painter { doc, size: (plan.width, plan.height), scale, threads: render_threads(), timeline: None, builds: vec![], parts: None };
    // Consecutive identical frames (holds) become one job.
    let mut runs: Vec<(Frame, Vec<usize>)> = Vec::new();
    for (k, f) in plan.frames.iter().enumerate() {
        match runs.last_mut() {
            Some((last, numbers)) if last == f => numbers.push(k + 1),
            _ => runs.push((f.clone(), vec![k + 1])),
        }
    }
    let jobs = runs.into_iter().map(|(f, numbers)| Job { numbers, image: painter.draw(&f) });
    if let Err(e) = run_jobs(&staging, jobs, cap) {
        let _ = crate::io::remove_files(&staging_s, &is_frame_name);
        let _ = crate::io::remove_empty_folder(&staging_s);
        return Err(io(e));
    }
    // Then the new frames take the earlier ones' place, all of them, so a video encoder reading
    // the numbered sequence stops where this one ends.
    if let Err(e) = replace_sequence(folder, &staging_s, &backup_s) {
        let _ = crate::io::remove_files(&staging_s, &is_frame_name);
        let _ = crate::io::remove_empty_folder(&staging_s);
        return Err(io(e));
    }
    Ok(FramesOutput {
        folder: folder.to_string(),
        frames: plan.frames.len(),
        fps: opts.fps,
        width: plan.width,
        height: plan.height,
        slides: plan.slides,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use serde_json::json;

    use super::*;

    /// With timings, a slide's Advance Slide After time sets its holds, as the show plays it: the
    /// wait before each click step and before the slide advances. `useTimings` overrides the deck's
    /// Use Timings setting either way; a slide without an After time keeps the requested holds.
    #[test]
    fn advance_after_times_set_the_holds_with_timings() {
        use deckcraft_model::anim::{AnimStart, Animation};
        let mut doc = deckcraft_model::defaults::new_presentation(None);
        let len = |doc: &Presentation, p: Value| plan(doc, &FramesOptions::from_params(&p).unwrap()).unwrap().frames();
        let opts = json!({"fps": 2, "secondsPerSlide": 5, "secondsPerStep": 3, "width": 32});
        assert_eq!(len(&doc, opts.clone()), 10, "no After time: secondsPerSlide");
        std::sync::Arc::make_mut(&mut doc.slides[0]).transition =
            Some(deckcraft_model::anim::Transition { kind: "none".into(), advance_after_ms: Some(2000), ..Default::default() });
        assert_eq!(len(&doc, opts.clone()), 4, "two seconds at 2 fps");
        assert_eq!(len(&doc, json!({"fps": 2, "secondsPerSlide": 5, "width": 32, "useTimings": false})), 10);
        doc.show.use_timings = false;
        assert_eq!(len(&doc, opts.clone()), 10, "the deck does not use timings");
        assert_eq!(len(&doc, json!({"fps": 2, "secondsPerSlide": 5, "width": 32, "useTimings": true})), 4);
        doc.show.use_timings = true;
        // A click step waits the After time before it plays, and the slide waits it again after.
        let shape = doc.slides[0].shapes[0].id;
        std::sync::Arc::make_mut(&mut doc.slides[0]).animations.push(Animation {
            shape,
            effect: "fade".into(),
            start: AnimStart::OnClick,
            ..Default::default()
        });
        let timed = len(&doc, opts.clone());
        let held = len(&doc, json!({"fps": 2, "secondsPerSlide": 2, "secondsPerStep": 2, "width": 32, "useTimings": false}));
        assert_eq!(timed, held, "the After time is both holds");
        assert!(timed > 8, "the step plays between them: {timed}");
    }

    /// Show without animation cuts between slides and shows each one fully built: one settled
    /// still per slide, no transition frames and no build frames.
    #[test]
    fn show_without_animation_plans_one_settled_still_per_slide() {
        use deckcraft_model::anim::{AnimStart, Animation};
        let mut doc = deckcraft_model::defaults::new_presentation(None);
        let copy = doc.slides[0].clone();
        doc.slides.push(copy);
        let shape = doc.slides[1].shapes[0].id;
        let second = std::sync::Arc::make_mut(&mut doc.slides[1]);
        second.transition = Some(deckcraft_model::anim::Transition { kind: "fade".into(), duration_ms: 1000, ..Default::default() });
        second.animations.push(Animation { shape, effect: "fade".into(), start: AnimStart::OnClick, ..Default::default() });
        let opts = FramesOptions::from_params(&json!({"fps": 2, "secondsPerSlide": 3, "secondsPerStep": 1, "width": 32})).unwrap();
        let animated = plan(&doc, &opts).unwrap();
        assert!(animated.frames.iter().any(|f| matches!(f, Frame::Transition { .. })));
        doc.show.without_animation = true;
        let still = plan(&doc, &opts).unwrap();
        let steps = ShowState::timeline(&doc, 1).steps();
        assert!(steps > 0);
        assert_eq!(still.frames.len(), 12, "two slides of 3 s at 2 fps");
        assert!(still.frames[..6].iter().all(|f| *f == Frame::Build { slide: 0, step: ShowState::timeline(&doc, 0).steps(), t: 0.0 }));
        assert!(still.frames[6..].iter().all(|f| *f == Frame::Build { slide: 1, step: steps, t: 0.0 }));
        assert_eq!(still.slides.iter().map(|s| (s.from, s.to)).collect::<Vec<_>>(), [(1, 6), (7, 12)]);
    }

    /// A failed export (here the byte cap) leaves the earlier sequence whole and no staging folder;
    /// a successful one replaces it, shorter sequences included. A frame name taken by a folder is
    /// refused before anything is written.
    #[test]
    fn a_failed_export_keeps_the_earlier_frames() {
        let dir = std::env::temp_dir().join(format!("dc-frames-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let folder = dir.to_string_lossy().to_string();
        let doc = deckcraft_model::defaults::new_presentation(None);
        let opts = FramesOptions::from_params(&json!({"fps": 2, "secondsPerSlide": 2, "width": 32})).unwrap();
        let names = || {
            let mut v: Vec<String> =
                std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).filter(|n| n != LOCK).collect();
            v.sort();
            v
        };
        let first = export(&doc, &opts, &folder).unwrap();
        let before = names();
        assert_eq!(before.len(), first.frames, "{before:?}");
        let old = std::fs::read(dir.join(frame_name(1))).unwrap();
        let longer = FramesOptions::from_params(&json!({"fps": 4, "secondsPerSlide": 2, "width": 48})).unwrap();
        assert!(export_capped(&doc, &longer, &folder, 1).is_err());
        assert_eq!(names(), before, "the earlier sequence is whole and the staging folder is gone");
        assert_eq!(std::fs::read(dir.join(frame_name(1))).unwrap(), old);
        let shorter = FramesOptions::from_params(&json!({"fps": 1, "secondsPerSlide": 2, "width": 48})).unwrap();
        let out = export(&doc, &shorter, &folder).unwrap();
        assert_eq!(names().len(), out.frames, "the new sequence replaced the old one: {:?}", names());
        assert!(out.frames < first.frames);
        // Something other than frames in the staging folder stays, and the export still succeeds.
        std::fs::create_dir_all(dir.join(STAGING)).unwrap();
        std::fs::write(dir.join(STAGING).join("notes.txt"), b"x").unwrap();
        let out = export(&doc, &opts, &folder).unwrap();
        assert_eq!(names().iter().filter(|n| is_frame_name(n)).count(), out.frames);
        assert!(dir.join(STAGING).join("notes.txt").exists());
        // A folder with a frame's name is refused before anything changes: the earlier frames stay.
        std::fs::remove_dir_all(dir.join(STAGING)).unwrap();
        let before = names();
        let odd = dir.join(frame_name(out.frames + 1));
        std::fs::create_dir(&odd).unwrap();
        let e = export(&doc, &longer, &folder).unwrap_err().to_string();
        assert!(e.contains("is not a regular file") && e.contains(&frame_name(out.frames + 1)), "{e}");
        let mut expected = before.clone();
        expected.push(frame_name(out.frames + 1));
        expected.sort();
        assert_eq!(names(), expected, "nothing was removed or written");
        std::fs::remove_dir(&odd).unwrap();
        assert_eq!(export(&doc, &longer, &folder).unwrap().frames, names().len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A link with a frame's name, past the end of the new sequence, would survive the clearing and
    /// carry the sequence on with an old frame: it is refused before anything is written.
    #[cfg(unix)]
    #[test]
    fn a_link_with_a_frame_name_is_refused() {
        let dir = std::env::temp_dir().join(format!("dc-frames-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("old.png"), b"old").unwrap();
        std::os::unix::fs::symlink(dir.join("old.png"), dir.join(frame_name(MAX_FRAMES))).unwrap();
        let doc = deckcraft_model::defaults::new_presentation(None);
        let opts = FramesOptions::from_params(&json!({"fps": 1, "secondsPerSlide": 1, "width": 32})).unwrap();
        let e = export(&doc, &opts, &dir.to_string_lossy()).unwrap_err().to_string();
        assert!(e.contains("is not a regular file"), "{e}");
        assert!(!dir.join(frame_name(1)).exists() && !dir.join(STAGING).exists(), "nothing was written");
        assert_eq!(std::fs::read(dir.join("old.png")).unwrap(), b"old");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A staging or backup folder that is a link to the export folder would have the clearing delete
    /// the earlier frames through it: both are refused before anything is touched.
    #[cfg(unix)]
    #[test]
    fn a_linked_work_folder_is_refused() {
        let doc = deckcraft_model::defaults::new_presentation(None);
        let opts = FramesOptions::from_params(&json!({"fps": 1, "secondsPerSlide": 1, "width": 32})).unwrap();
        for work in [STAGING, BACKUP] {
            let dir = std::env::temp_dir().join(format!("dc-frames-linked-{work}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(frame_name(1)), b"old").unwrap();
            std::fs::write(dir.join(frame_name(2)), b"old").unwrap();
            std::os::unix::fs::symlink(&dir, dir.join(work)).unwrap();
            let e = export(&doc, &opts, &dir.to_string_lossy()).unwrap_err().to_string();
            assert!(e.contains("is a link") && e.contains(work), "{e}");
            for n in [1, 2] {
                assert_eq!(std::fs::read(dir.join(frame_name(n))).unwrap(), b"old", "{work}: frame {n} is kept");
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// A link with a frame's name left in the staging folder is not written through: the export
    /// fails and the file it points at is as it was.
    #[cfg(unix)]
    #[test]
    fn a_link_in_the_staging_folder_is_not_written_through() {
        let dir = std::env::temp_dir().join(format!("dc-frames-stage-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(STAGING)).unwrap();
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, b"keep").unwrap();
        std::os::unix::fs::symlink(&victim, dir.join(STAGING).join(frame_name(1))).unwrap();
        let doc = deckcraft_model::defaults::new_presentation(None);
        let opts = FramesOptions::from_params(&json!({"fps": 1, "secondsPerSlide": 1, "width": 32})).unwrap();
        assert!(export(&doc, &opts, &dir.to_string_lossy()).is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A move of the new frames that fails part way (here onto a folder with a frame's name) puts
    /// the earlier frames back and removes the new ones it placed: no half sequence, no backup.
    #[cfg(unix)]
    #[test]
    fn a_failed_move_puts_the_earlier_frames_back() {
        let dir = std::env::temp_dir().join(format!("dc-frames-restore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let staging = dir.join(STAGING);
        std::fs::create_dir_all(&staging).unwrap();
        for n in [1, 3] {
            std::fs::write(dir.join(frame_name(n)), b"old").unwrap();
        }
        std::fs::create_dir(dir.join(frame_name(2))).unwrap();
        for n in 1..=3 {
            std::fs::write(staging.join(frame_name(n)), b"new").unwrap();
        }
        let s = |p: std::path::PathBuf| p.to_string_lossy().to_string();
        assert!(replace_sequence(&s(dir.clone()), &s(staging.clone()), &s(dir.join(BACKUP))).is_err());
        for n in [1, 3] {
            assert_eq!(std::fs::read(dir.join(frame_name(n))).unwrap(), b"old", "frame {n} is the earlier one");
        }
        assert!(dir.join(frame_name(2)).is_dir());
        assert!(!dir.join(BACKUP).exists(), "the backup folder is gone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An export to a folder that another export holds the lock of is refused with nothing written
    /// (the lock stands in for a running export, in this process or another), and goes ahead once
    /// the lock is released.
    #[test]
    fn an_export_to_a_folder_in_use_is_refused() {
        let dir = std::env::temp_dir().join(format!("dc-frames-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let folder = dir.to_string_lossy().to_string();
        let held = crate::io::lock_folder(&folder, LOCK).unwrap();
        let doc = deckcraft_model::defaults::new_presentation(None);
        let opts = FramesOptions::from_params(&json!({"fps": 1, "secondsPerSlide": 1, "width": 32})).unwrap();
        let e = export(&doc, &opts, &folder).unwrap_err().to_string();
        assert!(e.contains("another export"), "{e}");
        assert!(!dir.join(frame_name(1)).exists() && !dir.join(STAGING).exists() && !dir.join(BACKUP).exists(), "nothing was written");
        drop(held);
        assert_eq!(export(&doc, &opts, &folder).unwrap().frames, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Frames left in the backup folder by an interrupted export are the user's to recover: the
    /// export is refused, nothing is written, and it goes ahead once they are gone.
    #[test]
    fn frames_left_in_the_backup_folder_are_refused() {
        let dir = std::env::temp_dir().join(format!("dc-frames-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(BACKUP)).unwrap();
        std::fs::write(dir.join(BACKUP).join(frame_name(1)), b"old").unwrap();
        let doc = deckcraft_model::defaults::new_presentation(None);
        let opts = FramesOptions::from_params(&json!({"fps": 1, "secondsPerSlide": 1, "width": 32})).unwrap();
        let folder = dir.to_string_lossy().to_string();
        let e = export(&doc, &opts, &folder).unwrap_err().to_string();
        assert!(e.contains("interrupted") && e.contains(BACKUP), "{e}");
        assert!(!dir.join(frame_name(1)).exists() && !dir.join(STAGING).exists(), "nothing was written");
        assert_eq!(std::fs::read(dir.join(BACKUP).join(frame_name(1))).unwrap(), b"old");
        std::fs::remove_dir_all(dir.join(BACKUP)).unwrap();
        assert_eq!(export(&doc, &opts, &folder).unwrap().frames, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A path that is a file is refused, and the file is left as it was.
    #[test]
    fn a_path_that_is_a_file_is_refused() {
        let dir = std::env::temp_dir().join(format!("dc-frames-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("frames");
        std::fs::write(&file, b"keep").unwrap();
        let doc = deckcraft_model::defaults::new_presentation(None);
        let opts = FramesOptions::from_params(&json!({"fps": 1, "secondsPerSlide": 1, "width": 32})).unwrap();
        let e = export(&doc, &opts, &file.to_string_lossy()).unwrap_err().to_string();
        assert!(e.contains("is not a folder"), "{e}");
        assert_eq!(std::fs::read(&file).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Past the byte cap the export fails instead of writing on; a hold's copies count.
    #[test]
    fn frames_past_the_byte_cap_are_refused() {
        let dir = std::env::temp_dir().join(format!("dc-frames-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let image = Arc::new(Image { width: 2, height: 2, pixels: vec![255; 16] });
        let png = image.to_png().len() as u64;
        let jobs = |n: usize| (0..n).map(|i| Job { numbers: vec![2 * i + 1, 2 * i + 2], image: image.clone() });
        assert_eq!(run_jobs(&dir, jobs(2), 4 * png), Ok(()), "four copies fit exactly");
        assert!(dir.join(frame_name(4)).exists());
        // Frames are created new, so the first run's files are cleared as an export's staging is.
        crate::io::remove_files(&dir.to_string_lossy(), &is_frame_name).unwrap();
        let e = run_jobs(&dir, jobs(3), 5 * png).unwrap_err();
        assert!(e.contains("bytes") && e.contains("width"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hostile_params_are_errors_not_panics() {
        for p in [
            json!({"fps": 0}),
            json!({"fps": 61}),
            json!({"fps": -1}),
            json!({"fps": 29.97}),
            json!({"fps": "30"}),
            json!({"fps": 1e308}),
            json!({"secondsPerStep": -0.1}),
            json!({"secondsPerStep": 30.1}),
            json!({"secondsPerStep": "1"}),
            json!({"secondsPerSlide": -1}),
            json!({"secondsPerSlide": 61}),
            json!({"secondsPerSlide": 1e308}),
            json!({"transitions": "yes"}),
            json!({"useTimings": "yes"}),
            json!({"width": 15}),
            json!({"width": 3841}),
            json!({"width": "large"}),
            json!({"width": 640.5}),
            json!({"slides": []}),
            json!({"slides": [-1]}),
            json!({"slides": "1-3"}),
            json!({"slides": vec![0; MAX_FRAMES + 1]}),
            json!({"includeHidden": 1}),
        ] {
            assert!(FramesOptions::from_params(&p).is_err(), "{p}");
        }
        assert_eq!(FramesOptions::from_params(&json!({"fps": null, "width": null, "transitions": null})).unwrap(), FramesOptions::default());
        let o =
            FramesOptions::from_params(&json!({"fps": 60, "secondsPerStep": 0, "secondsPerSlide": 60, "transitions": false, "width": 16})).unwrap();
        assert_eq!((o.fps, o.seconds_per_step, o.seconds_per_slide, o.transitions, o.width), (60, 0.0, 60.0, false, 16));
    }

    /// A destination under a write root that is not also a read root is exported to: the folder
    /// checks before the write need write access only. A folder outside the root is still refused.
    #[test]
    fn export_needs_only_write_access_to_the_destination() {
        let root = std::env::temp_dir().join(format!("dc-frames-wo-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("dc-frames-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let roots = crate::file_access::AutomationRoots::new(&[], std::slice::from_ref(&root)).unwrap().unwrap();
        let doc = deckcraft_model::defaults::new_presentation(None);
        let opts = FramesOptions { seconds_per_slide: 0.0, width: 32, ..Default::default() };
        let inside = root.join("out").to_string_lossy().to_string();
        let ok = crate::file_access::confine(Some(&roots), || export(&doc, &opts, &inside));
        assert!(ok.is_ok(), "{ok:?}");
        assert!(root.join("out").join(frame_name(1)).is_file());
        let elsewhere = outside.join("out").to_string_lossy().to_string();
        let refused = crate::file_access::confine(Some(&roots), || export(&doc, &opts, &elsewhere));
        assert!(refused.is_err());
        assert!(!outside.join("out").exists(), "nothing is written outside the root");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn frame_names_are_six_digits_and_recognised() {
        assert_eq!(frame_name(1), "frame-000001.png");
        assert_eq!(frame_name(MAX_FRAMES), "frame-018000.png");
        assert!(is_frame_name("frame-000001.png"));
        assert!(!is_frame_name("frame-1.png"));
        assert!(!is_frame_name("frame-000001.PNG"));
        assert!(!is_frame_name("slide-000001.png"));
        assert!(!is_frame_name("frame-00000a.png"));
    }

    /// The pixel budget refuses a long 4K export before a frame is drawn; every slide is at least
    /// one frame even with a zero hold.
    #[test]
    fn plan_is_capped_and_never_empty() {
        let mut s = crate::Session::with_new();
        s.execute("slide.new", &json!({})).unwrap();
        let doc = &s.doc().unwrap().doc;
        let o = FramesOptions { fps: 60, seconds_per_slide: 60.0, width: 3840, ..Default::default() };
        let e = plan(doc, &o).unwrap_err().to_string();
        assert!(e.contains(&MAX_TOTAL_PIXELS.to_string()) && e.contains("width"), "{e}");
        let o = FramesOptions { seconds_per_slide: 0.0, width: 64, ..Default::default() };
        let p = plan(doc, &o).unwrap();
        assert_eq!(p.frames(), 2);
        assert_eq!(p.slides, vec![SlideRange { index: 0, from: 1, to: 1 }, SlideRange { index: 1, from: 2, to: 2 }]);
        assert_eq!((p.width, p.height), (64, 36));
    }

    /// Frames are for a video encoder: both sides come out even, whatever the width and aspect.
    #[test]
    fn frame_sizes_are_even() {
        let s = crate::Session::with_new();
        for (size, w, expect) in [
            (deckcraft_geom::Size::new(960.0, 540.0), 333, (334, 188)),
            (deckcraft_geom::Size::new(720.0, 540.0), 333, (334, 250)),
            (deckcraft_geom::Size::new(720.0, 540.0), 17, (18, 14)),
            (deckcraft_geom::Size::new(960.0, 540.0), 3839, (3840, 2160)),
        ] {
            let mut doc = Presentation::clone(&s.doc().unwrap().doc);
            doc.slide_size = size;
            let o = FramesOptions { width: w, ..Default::default() };
            let p = plan(&doc, &o).unwrap();
            assert_eq!((p.width, p.height), expect, "{w} on {size:?}");
        }
    }
}
