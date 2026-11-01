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
    /// Read `{fps?, secondsPerStep?, secondsPerSlide?, transitions?, width?, slides?: [index],
    /// includeHidden?}`. Absent or `null` values take their defaults; anything out of range or of
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
    let (width, height) = frame_size(doc, opts.width);
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
        if let Some(from_slide) = prev.filter(|_| opts.transitions)
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
        if steps == 0 {
            frames.extend(std::iter::repeat_n(Frame::Build { slide, step: 0, t: 0.0 }, hold_slide));
        }
        // What the slide shows before its first click is held like any settled state; an automatic
        // first step plays at once, so there is nothing to hold.
        if steps > 0 && !tl.step_is_auto(0) {
            frames.extend(std::iter::repeat_n(Frame::Build { slide, step: 0, t: 0.0 }, hold_step));
        }
        for step in 0..steps {
            let n = playing(tl.step_duration(step));
            frames.extend((0..n).map(|j| Frame::Build { slide, step, t: j as f64 / fps }));
            let hold = if step + 1 == steps {
                hold_slide
            } else if tl.step_is_auto(step + 1) {
                // The pause before a step depends on that step: an automatic one runs on.
                0
            } else {
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
/// replaces the earlier `frame-NNNNNN.png` files only once every frame is written; a failed export
/// leaves them as they were. Nothing is written when the request is refused, or when a frame's name
/// in `folder` is taken by something other than a regular file (a folder, a link).
pub fn export(doc: &Presentation, opts: &FramesOptions, folder: &str) -> Result<FramesOutput> {
    export_capped(doc, opts, folder, MAX_TOTAL_BYTES)
}

/// The folder inside the export folder that a new sequence is written to before it replaces the
/// old one; a dot name, so it is not mistaken for part of the sequence.
const STAGING: &str = ".deckcraft-frames-new";

/// [`export`] with the byte cap as a parameter, so a test can make the export fail part way.
fn export_capped(doc: &Presentation, opts: &FramesOptions, folder: &str, cap: u64) -> Result<FramesOutput> {
    let plan = plan(doc, opts)?;
    let dir = std::path::Path::new(folder);
    let io = |e: String| EngineError::Other(format!("frames export: {e}"));
    crate::file_access::check_write(&dir.join(frame_name(1)).to_string_lossy()).map_err(io)?;
    crate::io::create_folder(folder).map_err(io)?;
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
    // Then the earlier frames go, so a video encoder reading the numbered sequence stops where
    // this one ends, and the new ones take their place.
    crate::io::remove_files(folder, &is_frame_name).map_err(io)?;
    crate::io::move_files(&staging_s, folder, &is_frame_name).map_err(io)?;
    crate::io::remove_empty_folder(&staging_s).map_err(io)?;
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
            let mut v: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
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
}
