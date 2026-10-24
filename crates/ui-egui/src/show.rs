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

pub struct Show {
    pub state: ShowState,
    pub reading: bool,
    pub presenter: bool,
    /// Preview from the Transitions/Animations tab: exits after the current slide.
    pub preview_only: bool,
    /// Play every step automatically (animation preview).
    pub autoplay: bool,
    pub rehearse: bool,
    timeline: Option<(usize, Timeline)>,
    step_start: f64,
    slide_start: f64,
    step_end: f64,
    trans: Option<Transition>,
    tex: Option<(Key, TextureHandle)>,
    /// The shapes stacked above the playing videos, drawn over the video frames (#1005).
    overlay: Option<(Key, Vec<ShapeId>, TextureHandle)>,
    next_tex: Option<(usize, TextureHandle)>,
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

fn render_threads() -> u16 {
    if cfg!(target_arch = "wasm32") { 0 } else { std::thread::available_parallelism().map(|n| n.get().min(8) as u16).unwrap_or(0) }
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

fn render_backdrop(doc: &Presentation, key: BackKey, threads: u16) -> deckcraft_render::Image {
    let opts = RenderOpts { scale: key.size.0 as f64 / doc.slide_size.width.max(1.0), size: Some(key.size), threads, ..Default::default() };
    deckcraft_render::render_slide_part(doc, key.slide, deckcraft_render::SlidePart::Backdrop, &opts)
}

/// `top` laid over `back` (both premultiplied RGBA of the same size; `top` alone otherwise).
fn over(back: &deckcraft_render::Image, top: &deckcraft_render::Image) -> egui::ColorImage {
    if back.width != top.width || back.height != top.height || back.pixels.len() != top.pixels.len() {
        return crate::textures::to_color_image(top, false);
    }
    let px = back
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(top.pixels.as_chunks::<4>().0)
        .map(|(b, t)| {
            let k = 255 - t[3] as u32;
            let mix = |i: usize| (t[i] as u32 + (b[i] as u32 * k + 127) / 255).min(255) as u8;
            Color32::from_rgba_premultiplied(mix(0), mix(1), mix(2), mix(3))
        })
        .collect();
    egui::ColorImage::new([top.width as usize, top.height as usize], px)
}

#[allow(clippy::too_many_arguments)]
/// Slide `idx` at `step`, `t` seconds into it, `size` pixels, with `hidden` media hidden. With a
/// `backdrop` (of this slide, at this size), only the slide's shapes are rendered, over it.
fn render_frame(
    doc: &Presentation,
    idx: usize,
    tl: Timeline,
    step: usize,
    t: f64,
    size: (u32, u32),
    hidden: Vec<ShapeId>,
    times: &[(ShapeId, f64)],
    threads: u16,
    plays: &[TriggerPlay],
    stamps: &[StepStamp],
    backdrop: Option<&deckcraft_render::Image>,
) -> egui::ColorImage {
    let f = move |id: ShapeId| {
        let mut st = state_for(&tl, id, step, t, plays, stamps);
        if hidden.contains(&id) {
            st.visible = false;
        }
        st
    };
    let opts = RenderOpts {
        scale: size.0 as f64 / doc.slide_size.width.max(1.0),
        size: Some(size),
        state: Some(&f),
        gif_times: times,
        threads,
        ..Default::default()
    };
    match backdrop {
        Some(back) => over(back, &deckcraft_render::render_slide_part(doc, idx, deckcraft_render::SlidePart::Content, &opts)),
        None => crate::textures::to_color_image(&deckcraft_render::render_slide(doc, idx, &opts), false),
    }
}

/// Render only the slide's own shapes except `skip`, over transparency: what is stacked above a
/// playing video, to be drawn over the video frame.
#[allow(clippy::too_many_arguments)]
fn render_overlay(
    doc: &Presentation,
    idx: usize,
    tl: Timeline,
    step: usize,
    t: f64,
    size: (u32, u32),
    hidden: Vec<ShapeId>,
    skip: &[ShapeId],
    times: &[(ShapeId, f64)],
    threads: u16,
    plays: &[TriggerPlay],
    stamps: &[StepStamp],
) -> egui::ColorImage {
    let f = move |id: ShapeId| {
        let mut st = state_for(&tl, id, step, t, plays, stamps);
        if hidden.contains(&id) {
            st.visible = false;
        }
        st
    };
    let opts = RenderOpts {
        scale: size.0 as f64 / doc.slide_size.width.max(1.0),
        size: Some(size),
        state: Some(&f),
        skip,
        gif_times: times,
        threads,
        ..Default::default()
    };
    let img = deckcraft_render::render_slide_part(doc, idx, deckcraft_render::SlidePart::Content, &opts);
    crate::textures::to_color_image(&img, false)
}

/// The shapes of slide `idx` to leave out when drawing what is stacked above the lowest of the
/// playing `videos` (that video, everything below it and the other videos). Empty when nothing
/// is above a video: the video frame is then painted last.
fn below_playing_video(doc: &Presentation, idx: usize, videos: &[ShapeId]) -> Vec<ShapeId> {
    let Some(slide) = doc.slides.get(idx) else { return vec![] };
    let Some(first) = slide.shapes.iter().position(|s| videos.contains(&s.id)) else { return vec![] };
    if slide.shapes.iter().skip(first + 1).all(|s| videos.contains(&s.id)) {
        return vec![];
    }
    slide.shapes.iter().enumerate().filter(|(i, s)| *i <= first || videos.contains(&s.id)).map(|(_, s)| s.id).collect()
}

/// Media on slide `idx` hidden until it plays, as it is when the slide is entered.
fn hidden_on_entry(doc: &Presentation, idx: usize) -> Vec<ShapeId> {
    crate::media::slide_media(doc, idx).into_iter().filter(|(_, _, clip)| clip.hide_while_not_playing).map(|m| m.0).collect()
}

/// Start rendering slide `next`'s first frame on another thread. `None` on the web (no threads).
fn start_prefetch(doc: &std::sync::Arc<Presentation>, next: usize, size: (u32, u32)) -> Option<Prefetch> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let hidden = hidden_on_entry(doc, next);
    let key = Key { slide: next, step: 0, size, doc: std::sync::Arc::as_ptr(doc) as usize, hidden: hidden.clone(), gif: 0, plays: vec![] };
    let (tx, rx) = std::sync::mpsc::channel();
    let doc = doc.clone();
    // Leave a core for the UI thread.
    let threads = render_threads().saturating_sub(1).max(1);
    let spawned = std::thread::Builder::new().name("show-prefetch".into()).spawn(move || {
        let tl = ShowState::timeline(&doc, next);
        let _ = tx.send(render_frame(&doc, next, tl, 0, 0.0, size, hidden, &[], threads, &[], &[], None));
    });
    spawned.ok().map(|_| Prefetch { key, rx })
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
}

impl Show {
    pub fn new(doc: &Presentation, from: usize, reading: bool, presenter: bool) -> Self {
        Show {
            state: ShowState::start_at(doc, from),
            reading,
            presenter,
            preview_only: false,
            autoplay: false,
            rehearse: false,
            timeline: None,
            step_start: 0.0,
            slide_start: 0.0,
            step_end: 0.0,
            trans: None,
            tex: None,
            overlay: None,
            next_tex: None,
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
                if self.rehearse {
                    self.timings.push((self.timeline.as_ref().map(|t| t.0).unwrap_or(i), now - self.slide_start));
                }
                let (kind, option, dur) = doc
                    .slides
                    .get(i)
                    .and_then(|s| s.transition.as_ref())
                    .map(|t| (t.kind.clone(), t.option.clone(), t.duration_ms as f64 / 1000.0))
                    .unwrap_or(("none".into(), String::new(), 0.0));
                // Take the old slide's texture rather than cloning its handle: the new slide is
                // otherwise rendered into the same texture, and the transition blends the new
                // slide with itself (#33).
                let (from, old) = self.tex.take().map(|(k, t)| (k.slide, t)).unzip();
                if from != Some(i) {
                    self.last_viewed = from;
                }
                self.trans = if kind != "none" && dur > 0.0 && !doc.show.without_animation {
                    Some(Transition { old, from, morph: None, parts: None, start: now, kind, option, dur })
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

fn fit(avail: Rect, doc: &Presentation) -> Rect {
    // Loaded sizes are repaired, but a zero side here would make every rectangle NaN.
    let side = |v: f64| if v.is_finite() { (v as f32).max(1.0) } else { 1.0 };
    let (sw, sh) = (side(doc.slide_size.width), side(doc.slide_size.height));
    let s = (avail.width() / sw).min(avail.height() / sh).max(0.0);
    Rect::from_center_size(avail.center(), vec2(sw * s, sh * s))
}

/// A transition layer as an egui mesh: [`deckcraft_anim::layer_mesh`]'s triangles (perspective
/// cards subdivided so their pictures stay straight) with per-vertex opacity.
/// Where presenter view's "Next" preview goes: the column's width below its label, kept inside
/// the top 60% of the column. A portrait slide used to get the full width and a height many
/// times the window (a white slab over the notes and past the window edge).
fn next_rect(right: Rect, doc: &Presentation) -> Rect {
    let area = Rect::from_min_max(right.min + vec2(0.0, 22.0), pos2(right.max.x, right.min.y + 22.0 + (right.height() - 22.0).max(0.0) * 0.6));
    Rect::from_min_size(area.min, fit(area, doc).size())
}

fn quad_mesh(tex: Option<egui::TextureId>, rect: Rect, l: &Layer, color: Color32) -> Mesh {
    let mut m = match tex {
        Some(id) => Mesh::with_texture(id),
        None => Mesh::default(),
    };
    let (verts, tris) = deckcraft_anim::layer_mesh(l);
    for v in &verts {
        let a = if v.a.is_finite() { v.a.clamp(0.0, 1.0) } else { 0.0 };
        let col = if tex.is_some() { Color32::from_white_alpha((a * 255.0).round() as u8) } else { color.gamma_multiply(a as f32) };
        let p = pos2(rect.min.x + v.pos[0] as f32 * rect.width(), rect.min.y + v.pos[1] as f32 * rect.height());
        m.vertices.push(egui::epaint::Vertex { pos: p, uv: pos2(v.uv[0] as f32, v.uv[1] as f32), color: col });
    }
    for t in tris {
        m.add_triangle(t[0], t[1], t[2]);
    }
    m
}

/// Render the backgrounds and contents of a transition's two slides into textures, once.
fn part_textures(ctx: &egui::Context, doc: &Presentation, from: Option<usize>, to: usize, size: (u32, u32)) -> [Option<TextureHandle>; 4] {
    use deckcraft_render::SlidePart;
    let threads = if cfg!(target_arch = "wasm32") { 0 } else { std::thread::available_parallelism().map(|n| n.get().min(8) as u16).unwrap_or(0) };
    let opts = RenderOpts { scale: size.0 as f64 / doc.slide_size.width.max(1.0), size: Some(size), threads, ..Default::default() };
    let tex = |i: usize, part: SlidePart, name: &str| {
        let img = deckcraft_render::render_slide_part(doc, i, part, &opts);
        crate::textures::upload(ctx, name, crate::textures::to_color_image(&img, false))
    };
    [
        from.map(|i| tex(i, SlidePart::Backdrop, "trans-old-back")),
        from.map(|i| tex(i, SlidePart::Content, "trans-old-content")),
        Some(tex(to, SlidePart::Backdrop, "trans-new-back")),
        Some(tex(to, SlidePart::Content, "trans-new-content")),
    ]
}

/// Read the current viewport's show keys before its widgets can consume them. Both the
/// audience window and Presenter View use this path, including animation steps and end-of-show.
/// `presenter` is true for Presenter View, where G opens that window's own grid (#1130).
fn keyboard(ctx: &egui::Context, show: &mut Show, doc: &Presentation, now: f64, presenter: bool) -> bool {
    let keys: Vec<(egui::Key, egui::Modifiers)> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| if let egui::Event::Key { key, pressed: true, modifiers, .. } = e { Some((*key, *modifiers)) } else { None })
            .collect()
    });
    let mut keep = true;
    let mut actions = vec![];
    for (k, m) in keys {
        use egui::Key::*;
        let presenter_grid = presenter && show.presenter_grid;
        let grid_open = if presenter { show.presenter_grid } else { show.grid };
        if grid_open && !matches!(k, Escape | Minus) {
            // See All Slides takes no other keys until a slide is chosen.
            if k == G && plain(m) {
                if presenter_grid {
                    show.presenter_grid = false;
                } else {
                    show.grid = false;
                }
            } else {
                continue;
            }
        } else {
            match k {
                Escape | Minus => {
                    let cancelled = k == Escape && show.clear_goto();
                    if !cancelled && !show.back_out(presenter) {
                        keep = false;
                    }
                }
                ArrowRight | ArrowDown | Space | PageDown | N => {
                    if show.ended {
                        keep = false;
                    } else {
                        actions.push(show.state.next(doc));
                    }
                }
                Enter => {
                    if !show.goto.is_empty() {
                        if let Some(i) = show.take_goto()
                            && i < doc.slides.len()
                        {
                            actions.push(show.state.goto(doc, i));
                        }
                    } else if show.ended {
                        keep = false;
                    } else {
                        actions.push(show.state.next(doc));
                    }
                }
                Backspace if !show.goto.is_empty() => {
                    show.goto.pop();
                }
                ArrowLeft | ArrowUp | Backspace | PageUp | P if !(k == P && m.command) => {
                    show.ended = false;
                    actions.push(show.state.prev(doc));
                }
                P if m.command => show.set_pen(!show.pen),
                L if m.command => show.set_laser(!show.laser),
                A if m.command => show.arrow(),
                Home => {
                    if let Some(i) = show.state.first_index(doc) {
                        actions.push(show.state.goto(doc, i));
                    }
                }
                End => {
                    if let Some(i) = show.state.last_index(doc) {
                        actions.push(show.state.goto(doc, i));
                    }
                }
                B | Period => toggle_blank(show, Color32::BLACK),
                W | Comma => toggle_blank(show, Color32::WHITE),
                E => show.strokes.clear(),
                G if plain(m) => show.open_grid(presenter),
                Z if plain(m) => show.zoom_key(),
                T if plain(m) => show.timer.toggle(now),
                R if plain(m) => show.timer.reset(now),
                OpenBracket if plain(m) => show.nudge_notes(-NOTES_STEP),
                CloseBracket if plain(m) => show.nudge_notes(NOTES_STEP),
                Num0 | Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9 => {
                    if let Some(d) = k.name().chars().next() {
                        show.push_slide_number(d, now);
                    }
                }
                _ => continue,
            }
        }
        // The same key must not also activate a focused presenter button or be handled again
        // by an embedded presenter viewport.
        ctx.input_mut(|i| i.consume_key(m, k));
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
    for action in actions {
        if !show.apply(action, doc, now) {
            keep = false;
        }
    }
    keep
}

pub fn ui(app: &mut SlideApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let Some((doc_uid, doc)) = app.session.active().map(|d| (d.uid, d.doc.clone())) else {
        app.show = None;
        return;
    };
    let now = ctx.input(|i| i.time);
    let Some(mut show) = app.show.take() else { return };
    if !show.started {
        show.started = true;
        show.slide_start = now;
        show.step_start = now;
        show.show_start = now;
        show.timer = PresenterTimer::start(now);
        show.last_move = now;
        if show_fills_screen(show.reading, &doc.show.show_type) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
        show.after_enter(&doc, now);
    }
    let mut keep = keyboard(&ctx, &mut show, &doc, now, false);
    let (clicked, secondary, moved) = ctx.input(|i| (i.pointer.primary_clicked(), i.pointer.secondary_clicked(), i.pointer.delta().length() > 0.0));
    if moved {
        show.last_move = now;
    }
    let mut actions: Vec<ShowAction> = vec![];
    let full = ui.max_rect();
    let avail = if show.reading { Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - 34.0)) } else { full };
    let srect = fit(avail, &doc);
    let pointer = ctx.input(|i| i.pointer.interact_pos());
    show.expire_goto(now);
    // The clock of the triggered animations runs before anything reacts to the pointer.
    show.tick_triggers(&doc, now);
    let on_slide = !show.grid
        && !show.pen
        && !show.laser
        && !show.ended
        && show.blank.is_none()
        && show.zoom.is_none()
        && !show.zoom_arm
        && !show.reading_bar_hit(&ctx)
        && !pointer_on_media_bar(&ctx, ui, &doc, &show);
    let gif_click = if clicked && on_slide && doc.show.show_media_controls { gif_hit(&ctx, ui, &show, doc_uid, &doc, app) } else { None };
    let media_click = if clicked && on_slide && gif_click.is_none() { media_hit(&ctx, ui, &show, &doc) } else { None };
    let under = if on_slide && gif_click.is_none() && media_click.is_none() { shapes_under_pointer(&ctx, ui, &mut show, &doc, now) } else { None };
    let pointing = under.as_ref().is_some_and(|u| u.pick.is_some());
    if let Some(id) = gif_click {
        let _ = app.run("media.gifPlay", json!({"id": id}));
    } else if let Some((id, clip)) = media_click {
        app.media.toggle(doc_uid, &doc, id, &clip, crate::media::Owner::Show);
    } else if clicked && let Some(Pick::Text(l)) = under.as_ref().and_then(|u| u.pick.clone()) {
        let last = show.last_viewed;
        match show.state.follow_link(&doc, &l, last) {
            LinkJump::Show(a) => actions.push(a),
            LinkJump::Open(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            LinkJump::Exit => keep = false,
            LinkJump::None => {}
        }
    } else if show.grid && clicked {
        let aspect = slide_aspect(&doc);
        if let Some(p) = pointer
            && let Some(n) = see_all_hit(full, grid_indices(doc.slides.len()).len(), aspect, p)
            && let Some(&i) = grid_indices(doc.slides.len()).get(n)
        {
            show.grid = false;
            show.blank = None;
            show.zoom = None;
            show.zoom_arm = false;
            actions.push(show.state.goto(&doc, i));
        }
    } else if clicked && show.zoom_arm && !show.reading_bar_hit(&ctx) {
        if let Some(p) = pointer
            && let Some((nx, ny)) = norm_on_slide(srect, unit_uv(), p)
        {
            show.zoom = Some(Zoom::at(nx, ny));
            show.zoom_arm = false;
        }
    } else if clicked && show.zoom.is_some() && !show.pen && !show.reading_bar_hit(&ctx) {
        show.dismiss_zoom();
    } else if clicked && !show.pen && !show.reading_bar_hit(&ctx) && !pointer_on_media_bar(&ctx, ui, &doc, &show) {
        if show.ended {
            keep = false;
        } else if show.blank.is_some() {
            show.blank = None;
        } else {
            // A shape with an action or a triggered animation, or the background.
            let none = Under::default();
            match show.click_slide(&doc, under.as_ref().unwrap_or(&none), now) {
                Click::Show(a) => actions.push(a),
                Click::Open(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                Click::Exit => keep = false,
                Click::Animated | Click::None => {}
            }
        }
    }
    if secondary && !show.ended && !show.grid && show.zoom.is_none() && !show.zoom_arm {
        actions.push(show.state.prev(&doc));
    }
    // A trackpad swipe (and the mouse wheel) turns the page. The pen is drawing, so a flick
    // there stays on the slide. Pinch zoom is not a show gesture.
    if !show.pen
        && let Some(next) = crate::canvas::page_flick(ctx.input(|i| i.smooth_scroll_delta))
        && now - show.last_swipe > 0.35
    {
        show.last_swipe = now;
        if next {
            if show.ended {
                keep = false;
            } else {
                actions.push(show.state.next(&doc));
            }
        } else {
            show.ended = false;
            actions.push(show.state.prev(&doc));
        }
    }
    // Apply input first: a step started by this click must restart the clock before it is read.
    for a in actions.drain(..) {
        if !show.apply(a, &doc, now) {
            keep = false;
        }
    }
    // Step clock.
    let (step, playing) = (show.state.step, show.state.playing);
    let t_step = if playing { now - show.step_start } else { 0.0 };
    if playing {
        let dur = show.timeline(&doc).step_duration(step);
        if t_step >= dur {
            actions.push(show.state.step_done(&doc));
            show.step_end = now;
        }
    } else if show.trans.is_none() {
        // Auto-advance (After: timings) or autoplay preview. An automatic first step is started
        // by `ShowState` as the slide appears; starting it here too replayed it after Back (#844).
        let idle = now - show.step_end.max(show.slide_start);
        let cur_step = show.state.step;
        let n_steps = show.timeline(&doc).steps();
        let step_due = show.autoplay && cur_step < n_steps && idle > 0.3;
        let holding = show.grid || show.presenter_grid || show.zoom.is_some() || show.zoom_arm;
        if !holding && (step_due || (!show.rehearse && show.state.auto_advance_due(&doc, idle) && !show.ended)) {
            actions.push(show.state.next(&doc));
        } else if show.preview_only && cur_step >= n_steps && idle > 0.8 {
            keep = false;
        }
    }
    for a in actions {
        if !show.apply(a, &doc, now) {
            keep = false;
        }
    }
    if show.state.playing {
        let step = show.state.step;
        show.cue_media(&doc, step, now - show.step_start);
    }
    show_media(app, &mut show, doc_uid, &doc);
    run_media_cues(app, &mut show, doc_uid, &doc);
    // Draw.
    let painter = ui.painter_at(full);
    painter.rect_filled(full, CornerRadius::ZERO, Color32::BLACK);
    let ppp = ctx.pixels_per_point();
    // A huge screen or DPI mustn't ask for a picture the GPU can't hold: render at most its limit
    // (drawn stretched to the screen).
    let size = crate::textures::fit_size(
        ((srect.width() * ppp).round().clamp(1.0, u32::MAX as f32) as u32, (srect.height() * ppp).round().clamp(1.0, u32::MAX as f32) as u32),
        crate::textures::max_side(&ctx),
    );
    if show.ended {
        painter.text(
            pos2(full.center().x, full.min.y + 40.0),
            Align2::CENTER_CENTER,
            "End of slide show, click to exit.",
            theme::font(18.0),
            Color32::from_gray(200),
        );
    } else {
        let idx = show.state.slide;
        let step = show.state.step;
        // A main step or a triggered animation is running: the picture changes every frame.
        let animating = show.state.playing || show.state.trigger_playing();
        let t = if show.state.playing { now - show.step_start } else { 0.0 };
        let plays = show.state.trigger_plays();
        let stamps = show.state.trigger_stamps();
        let media = crate::media::slide_media(&doc, idx);
        let hidden: Vec<ShapeId> = media
            .iter()
            .filter(|(id, _, clip)| {
                clip.hide_while_not_playing && !app.media.status(*id).is_some_and(|s| s.state != deckcraft_media::PlayState::Ended)
            })
            .map(|m| m.0)
            .collect();
        let gifs = app.media.gifs(doc_uid, &doc, idx, now, &app.session.gif_paused);
        let times: Vec<_> = gifs.iter().map(|g| (g.id, g.t)).collect();
        crate::media::gif_repaint(&ctx, &gifs);
        let gif = crate::media::gif_key(&gifs);
        let done: Vec<(ShapeId, usize)> = plays.iter().map(|p| (p.trigger, p.step)).collect();
        let key = Key { slide: idx, step, size, doc: std::sync::Arc::as_ptr(&doc) as usize, hidden: hidden.clone(), plays: done, gif };
        let back_key = BackKey { slide: idx, size, doc: key.doc };
        let tex = if !animating && show.tex.as_ref().is_some_and(|(k, _)| *k == key) {
            show.tex.as_ref().map(|(_, t)| t.clone())
        } else {
            // The frame rendered ahead of time, when it is this one (waiting for it beats starting over).
            let ready = match show.prefetch.take() {
                Some(p) if p.key == key && !animating => p.rx.recv().ok(),
                _ => None,
            };
            let ci = match ready {
                Some(ci) => ci,
                None => {
                    let tl = show.timeline(&doc).clone();
                    // Animating: the backdrop doesn't change, only the shapes over it are drawn.
                    let back = animating.then(|| show.backdrop(&doc, back_key));
                    render_frame(&doc, idx, tl, step, t, size, hidden.clone(), &times, render_threads(), &plays, &stamps, back.as_deref())
                }
            };
            let tex = match show.tex.take() {
                Some((_, mut h)) => {
                    crate::textures::replace(&ctx, &mut h, ci);
                    h
                }
                None => crate::textures::upload(&ctx, "show", ci),
            };
            show.tex = Some((key.clone(), tex.clone()));
            Some(tex)
        };
        // While this slide rests, render the next one's first frame (not one with GIFs, whose
        // frame depends on when it is entered).
        // A slide with animations gets its backdrop ready while it rests, so they start smoothly.
        if !animating && !show.timeline(&doc).is_empty() {
            show.prefetch_backdrop(&doc, back_key);
        }
        if !animating && show.trans.is_none() && !show.preview_only {
            let next = show.state.peek_next(&doc);
            let wanted = next.filter(|n| app.media.gifs(doc_uid, &doc, *n, now, &app.session.gif_paused).is_empty());
            if let Some(n) = wanted
                && show.prefetch.as_ref().is_none_or(|p| p.key.slide != n || p.key.size != size || p.key.doc != std::sync::Arc::as_ptr(&doc) as usize)
            {
                show.prefetch = start_prefetch(&doc, n, size);
            }
        }
        let mut drew = false;
        let morphing = show.trans.as_ref().is_some_and(|tr| tr.kind == "morph" && tr.from.is_some() && now - tr.start < tr.dur);
        let tl = if morphing { Some(show.timeline(&doc).clone()) } else { None };
        let morph = match (&show.trans, tl) {
            (Some(tr), Some(tl)) => {
                let p = ((now - tr.start) / tr.dur.max(0.01)).clamp(0.0, 1.0);
                let hidden_b = |id: ShapeId| {
                    let st = state_for(&tl, id, 0, 0.0, &[], &[]);
                    !st.visible || st.opacity <= 0.0 || hidden.contains(&id)
                };
                tr.from
                    .and_then(|i| doc.slides.get(i).map(|a| (i, a)))
                    .zip(doc.slides.get(idx))
                    .map(|((i, a), b)| (i, deckcraft_anim::morph_frame(&doc, a, b, p, &hidden_b)))
            }
            _ => None,
        };
        if let (Some((from, m)), Some(tr)) = (morph, show.trans.as_mut()) {
            let f = |id: ShapeId| ShapeState { opacity: m.opacity_of(id), ..Default::default() };
            let threads =
                if cfg!(target_arch = "wasm32") { 0 } else { std::thread::available_parallelism().map(|n| n.get().min(8) as u16).unwrap_or(0) };
            let opts =
                RenderOpts { scale: size.0 as f64 / doc.slide_size.width.max(1.0), size: Some(size), state: Some(&f), threads, ..Default::default() };
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
            let img = deckcraft_render::render_blend(&doc, from, idx, m.mix, &m.shapes, &text, &paths, &opts);
            let ci = crate::textures::to_color_image(&img, false);
            let frame = match tr.morph.take() {
                Some(mut h) => {
                    crate::textures::replace(&ctx, &mut h, ci);
                    h
                }
                None => crate::textures::upload(&ctx, "morph", ci),
            };
            painter.image(frame.id(), srect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            tr.morph = Some(frame);
            drew = true;
            ctx.request_repaint();
        } else if let (Some(tr), Some(new)) = (show.trans.as_mut(), &tex) {
            let p = ((now - tr.start) / tr.dur.max(0.01)).clamp(0.0, 1.0);
            if p < 1.0 {
                // Slides split into background and content are rendered once per transition;
                // every frame only composites cached textures.
                if tr.parts.is_none() && deckcraft_anim::uses_slide_parts(&tr.kind, &tr.option) {
                    tr.parts = Some(part_textures(&ctx, &doc, tr.from, idx, size));
                }
                let aspect = doc.slide_size.width / doc.slide_size.height.max(1.0);
                let part = |k: usize| tr.parts.as_ref().and_then(|p| p.get(k).cloned().flatten());
                let parts = [part(0), part(1), part(2), part(3)];
                for l in deckcraft_anim::transition_frame(&tr.kind, &tr.option, p, aspect) {
                    let old_or_black = |t: Option<&TextureHandle>| match t {
                        Some(o) => quad_mesh(Some(o.id()), srect, &l, Color32::WHITE),
                        None => quad_mesh(None, srect, &l, Color32::BLACK),
                    };
                    let mesh = match l.source {
                        Source::Old => old_or_black(tr.old.as_ref()),
                        Source::New => quad_mesh(Some(new.id()), srect, &l, Color32::WHITE),
                        Source::Black => quad_mesh(None, srect, &l, Color32::BLACK),
                        Source::White => quad_mesh(None, srect, &l, Color32::WHITE),
                        Source::OldBack => old_or_black(parts[0].as_ref().or(tr.old.as_ref())),
                        Source::NewBack => quad_mesh(Some(parts[2].as_ref().unwrap_or(new).id()), srect, &l, Color32::WHITE),
                        Source::OldContent => match &parts[1] {
                            Some(t) => quad_mesh(Some(t.id()), srect, &l, Color32::WHITE),
                            None => continue,
                        },
                        Source::NewContent => match &parts[3] {
                            Some(t) => quad_mesh(Some(t.id()), srect, &l, Color32::WHITE),
                            None => continue,
                        },
                    };
                    painter.add(egui::Shape::mesh(mesh));
                }
                drew = true;
                ctx.request_repaint();
            }
        }
        if !drew {
            show.trans = None;
            if let Some(tex) = &tex {
                painter.image(tex.id(), srect, show.zoom_uv(), Color32::WHITE);
            }
            // Playing video: the current frame in its box, or full screen.
            let (sw, sh) = (doc.slide_size.width as f32, doc.slide_size.height as f32);
            let uv = show.zoom_uv();
            let mut framed: Vec<ShapeId> = vec![];
            let mut full_screen = false;
            for (id, x, clip) in &media {
                if !clip.video || hidden.contains(id) {
                    continue;
                }
                let r = if clip.full_screen && app.media.is_playing(*id) {
                    full_screen = true;
                    full
                } else {
                    slide_box(srect, uv, sw, sh, x.x as f32, x.y as f32, x.w as f32, x.h as f32)
                };
                if crate::media::paint_frame(&painter, &app.media, *id, r, clip.crop) {
                    framed.push(*id);
                }
            }
            // Shapes stacked above a playing video show over it, as in PowerPoint (#1005); a video
            // playing full screen covers the whole slide.
            let skip = if full_screen { vec![] } else { below_playing_video(&doc, idx, &framed) };
            if skip.is_empty() {
                show.overlay = None;
            } else {
                let fresh = !animating && show.overlay.as_ref().is_some_and(|(k, s, _)| *k == key && *s == skip);
                if !fresh {
                    let tl = show.timeline(&doc).clone();
                    let ci = render_overlay(&doc, idx, tl, step, t, size, hidden.clone(), &skip, &times, render_threads(), &plays, &stamps);
                    let handle = match show.overlay.take() {
                        Some((_, _, mut h)) => {
                            crate::textures::replace(&ctx, &mut h, ci);
                            h
                        }
                        None => crate::textures::upload(&ctx, "show-overlay", ci),
                    };
                    show.overlay = Some((key.clone(), skip, handle));
                }
                if let Some((_, _, h)) = &show.overlay {
                    painter.image(h.id(), srect, uv, Color32::WHITE);
                }
            }
            // Play/pause on the animated GIF under the pointer, and the audio/video bar.
            // Slide Show ▸ Show Media Controls hides both (the click still plays a clip).
            // Not while zoomed: the controls sit where the whole slide would be.
            if doc.show.show_media_controls && !show.zoom_arm && show.zoom.is_none() {
                let k = srect.width() / sw.max(1.0);
                if let Some(p) = ctx.input(|i| i.pointer.hover_pos()).filter(|_| !show.pen && !show.laser && now - show.last_move < 2.5) {
                    for g in &gifs {
                        let r = on_screen(srect, k, &g.rect);
                        if r.contains(p) {
                            let b = crate::media::gif_button_rect(r);
                            crate::media::paint_gif_button(&painter, b, g.paused, b.contains(p));
                        }
                    }
                }
                if let Some((id, clip, bar)) = media_bar_target(&ctx, &doc, &show, srect) {
                    let action = {
                        let status = app.media.status(id);
                        crate::media::control_bar(ui, bar, status.as_ref(), &clip)
                    };
                    let pos = app.media.status(id).map(|s| s.position).unwrap_or(0.0);
                    match action {
                        Some(crate::media::BarAction::Toggle) => app.media.toggle(doc_uid, &doc, id, &clip, crate::media::Owner::Show),
                        Some(crate::media::BarAction::Seek(t)) => app.media.seek(doc_uid, &doc, id, &clip, t, crate::media::Owner::Show),
                        Some(crate::media::BarAction::Nudge(dt)) => {
                            app.media.seek(doc_uid, &doc, id, &clip, (pos + dt).max(0.0), crate::media::Owner::Show)
                        }
                        None => {}
                    }
                }
            }
        }
        if animating {
            ctx.request_repaint();
        }
    }
    if let Some(c) = show.blank {
        painter.rect_filled(full, CornerRadius::ZERO, c);
    }
    if !show.goto.is_empty() {
        let label = format!("Go to slide {}", show.goto);
        let w = (28.0 + label.len() as f32 * 10.0).clamp(160.0, full.width() - 32.0);
        let r = Rect::from_min_size(pos2(full.min.x + 16.0, full.max.y - 96.0), vec2(w, 36.0));
        painter.rect_filled(r, CornerRadius::same(6), Color32::from_black_alpha(190));
        painter.text(r.center(), Align2::CENTER_CENTER, label, theme::font(18.0), Color32::WHITE);
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    // Pen.
    let resp = ui.interact(full, ui.id().with("show_pen"), if show.pen { Sense::drag() } else { Sense::hover() });
    if show.pen {
        if resp.drag_started() {
            show.strokes.push(vec![]);
        }
        let uv = show.zoom_uv();
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
            && let Some((nx, ny)) = norm_on_slide(srect, uv, p)
            && let Some(s) = show.strokes.last_mut()
        {
            s.push(pos2(nx, ny));
        }
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    } else if show.laser {
        ctx.set_cursor_icon(egui::CursorIcon::None);
    } else if show.zoom_arm || show.grid {
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    } else if now - show.last_move > 2.5 && !show.reading {
        // Idle first: a still pointer hides also over a clickable shape, such as a playing clip.
        ctx.set_cursor_icon(egui::CursorIcon::None);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    } else if pointing {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
    let pen_col = theme::to_color32(doc.show.pen_color);
    let uv = show.zoom_uv();
    for s in &show.strokes {
        let pts: Vec<Pos2> = s.iter().map(|q| slide_to_screen(srect, uv, q.x, q.y)).collect();
        painter.add(egui::Shape::line(pts, Stroke::new(3.0, pen_col)));
    }
    if show.laser
        && !show.ended
        && let Some(p) = ctx.input(|i| i.pointer.hover_pos())
    {
        let c = theme::to_color32(doc.show.laser_color);
        painter.circle_filled(p, 16.0, Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 70));
        painter.circle_filled(p, 5.5, c);
    }
    if show.zoom_arm && !show.grid {
        painter.text(
            pos2(full.center().x, full.min.y + 36.0),
            Align2::CENTER_CENTER,
            "Click the slide to zoom in. Esc cancels.",
            theme::font(16.0),
            Color32::WHITE,
        );
    }
    // Reading view bar / on-screen controls.
    if show.reading {
        let bar = Rect::from_min_max(pos2(full.min.x, full.max.y - 34.0), full.max);
        painter.rect_filled(bar, CornerRadius::ZERO, Color32::from_gray(32));
        painter.text(
            pos2(bar.min.x + 14.0, bar.center().y),
            Align2::LEFT_CENTER,
            format!("Slide {} of {}", show.state.slide + 1, doc.slides.len()),
            theme::font(12.0),
            Color32::from_gray(200),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(bar.max.x - 260.0, bar.min.y), bar.max))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        if child.button("Close").clicked() {
            keep = false;
        }
        if child.button("▶").clicked() {
            let a = show.state.next(&doc);
            if !show.apply(a, &doc, now) {
                keep = false;
            }
        }
        if child.button("◀").clicked() {
            let a = show.state.prev(&doc);
            show.apply(a, &doc, now);
        }
    } else if !show.grid && (now - show.last_move < 2.5 || show.zoom_arm) {
        let bar = tool_bar_rect(full);
        painter.rect_filled(bar, CornerRadius::same(17), Color32::from_black_alpha(140));
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(10.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let b = |ui: &mut Ui, s: &str| ui.add(egui::Button::new(egui::RichText::new(s).color(Color32::WHITE).size(15.0)).frame(false));
        if b(&mut child, "◀").clicked() {
            let a = show.state.prev(&doc);
            show.apply(a, &doc, now);
        }
        if b(&mut child, "▶").clicked() {
            let a = show.state.next(&doc);
            if !show.apply(a, &doc, now) {
                keep = false;
            }
        }
        if b(&mut child, if show.pen { "✏ on" } else { "✏" }).clicked() {
            show.set_pen(!show.pen);
        }
        if b(&mut child, if show.laser { "◉ on" } else { "◉" }).clicked() {
            show.set_laser(!show.laser);
        }
        if b(&mut child, "Black").clicked() {
            toggle_blank(&mut show, Color32::BLACK);
        }
        if b(&mut child, "White").clicked() {
            toggle_blank(&mut show, Color32::WHITE);
        }
        if b(&mut child, "All").clicked() {
            show.grid = !show.grid;
        }
        if b(&mut child, show.zoom_label()).clicked() {
            show.zoom_key();
        }
        if b(&mut child, "End").clicked() {
            keep = false;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
    }
    if show.grid && !show.ended {
        paint_grid(&painter, &ctx, &mut show, &doc, full);
    }
    if show.rehearse {
        let t = now - show.slide_start;
        let total = now - show.show_start;
        let r = Rect::from_min_size(pos2(full.min.x + 12.0, full.min.y + 12.0), vec2(220.0, 30.0));
        painter.rect_filled(r, CornerRadius::same(6), Color32::from_black_alpha(160));
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            format!("Slide {:02}:{:04.1}   Total {:02}:{:02}", (t / 60.0) as u32, t % 60.0, (total / 60.0) as u32, (total % 60.0) as u32),
            theme::font(13.0),
            Color32::WHITE,
        );
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    // Presenter view in a second window.
    if keep && show.presenter && !show.reading {
        presenter_window(&ctx, &mut show, &doc, now, &mut keep);
    }
    if keep {
        app.show = Some(show);
    } else {
        app.media.stop_owned(crate::media::Owner::Show);
        if !show.reading {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
        if show.rehearse {
            if let Some((_, tl)) = &show.timeline {
                let _ = tl;
            }
            show.timings.push((show.state.slide, now - show.slide_start));
            for (i, secs) in show.timings.clone() {
                let _ = app.run("transition.timing", json!({"index": i, "after": (secs * 1000.0).round() as u64}));
            }
            app.set_status("Slide timings recorded.");
        }
        let last = show.state.slide;
        let _ = app.run("slide.go", json!({"index": last}));
    }
}

/// The media-host voice for slide transition sounds (no shape has this id).
pub(crate) const TRANSITION_SOUND: ShapeId = ShapeId(u32::MAX);

/// Start the media of a newly shown slide: media from the previous slide stops unless it plays
/// across slides; media set to start automatically plays. Everything stops at the end.
fn show_media(app: &mut SlideApp, show: &mut Show, doc_uid: u64, doc: &Presentation) {
    let cur = if show.ended { None } else { Some(show.state.slide) };
    if show.media_slide == cur {
        return;
    }
    show.media_slide = cur;
    let trans = cur.and_then(|i| doc.slides.get(i)).and_then(|s| s.transition.as_ref());
    let sound = trans.and_then(|t| t.sound);
    let sound_loop = trans.is_some_and(|t| t.sound_loop);
    // [Stop Previous Sound] ends a transition sound still playing.
    let stop_previous = trans.is_some_and(|t| t.stop_sound);
    let keep: Vec<ShapeId> = match cur {
        Some(i) => app
            .media
            .ids()
            .into_iter()
            .filter(|id| app.media.clip(*id).is_some_and(|c| crate::media::carries_over(c, doc, *id, i)))
            // A transition sound plays to its end, as in PowerPoint, unless the new slide has its own.
            .chain((sound.is_none() && !stop_previous).then_some(TRANSITION_SOUND).filter(|id| app.media.is_playing(*id)))
            .collect(),
        None => vec![],
    };
    for id in app.media.ids() {
        if !keep.contains(&id) {
            app.media.stop(id);
        }
    }
    if let Some(media) = sound {
        // Loop Until Next Sound: it repeats until another slide's sound or Stop Previous Sound.
        let clip = deckcraft_model::MediaClip { media, volume: 1.0, loop_play: sound_loop, ..Default::default() };
        app.media.play(doc_uid, doc, TRANSITION_SOUND, &clip, None, crate::media::Owner::Show);
    }
    if let Some(i) = cur {
        for (id, _, clip) in crate::media::slide_media(doc, i) {
            if clip.autoplay && !keep.contains(&id) {
                app.media.play(doc_uid, doc, id, &clip, None, crate::media::Owner::Show);
            }
        }
    }
}

/// Carry out the media effects the show's steps set off: Play starts from the beginning unless
/// the clip is already playing, Pause toggles, Stop stops.
fn run_media_cues(app: &mut SlideApp, show: &mut Show, doc_uid: u64, doc: &Presentation) {
    for (id, effect) in std::mem::take(&mut show.media_cues) {
        let Some(clip) = crate::media::find_clip(doc, id, Some(show.state.slide)) else { continue };
        match effect.as_str() {
            "pause" => app.media.toggle(doc_uid, doc, id, &clip, crate::media::Owner::Show),
            "stop" => app.media.stop(id),
            // Already playing (it also starts automatically): leave it running.
            _ if app.media.is_playing(id) => {}
            _ => app.media.play(doc_uid, doc, id, &clip, Some(0.0), crate::media::Owner::Show),
        }
    }
}

/// Slide rectangle the show draws into (the reading bar keeps its strip).
fn shown_slide_rect(ui: &Ui, show: &Show, doc: &Presentation) -> Rect {
    let full = ui.max_rect();
    let avail = if show.reading { Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - 34.0)) } else { full };
    fit(avail, doc)
}

/// Playback bar for one clip: along the bottom inside a large shape, just under a small icon.
fn media_bar_rect(shape: Rect) -> Rect {
    let h = 26.0f32;
    let w = shape.width().clamp(160.0, 420.0);
    if shape.height() >= h + 16.0 && shape.width() >= 160.0 {
        Rect::from_min_size(pos2(shape.min.x + 4.0, shape.max.y - h - 4.0), vec2((shape.width() - 8.0).min(w), h))
    } else {
        Rect::from_min_size(pos2(shape.center().x - w / 2.0, shape.max.y + 4.0), vec2(w, h))
    }
}

fn slide_media_bars(srect: Rect, doc: &Presentation, slide: usize) -> Vec<(ShapeId, deckcraft_model::MediaClip, Rect, Rect)> {
    let k = srect.width() / doc.slide_size.width.max(1.0) as f32;
    crate::media::slide_media(doc, slide)
        .into_iter()
        .map(|(id, x, clip)| {
            let r = Rect::from_min_size(pos2(srect.min.x + x.x as f32 * k, srect.min.y + x.y as f32 * k), vec2(x.w as f32 * k, x.h as f32 * k));
            let bar = media_bar_rect(r);
            (id, clip, r, bar)
        })
        .collect()
}

/// The clip whose bar or shape is under the pointer, when Show Media Controls is on.
fn media_bar_target(ctx: &egui::Context, doc: &Presentation, show: &Show, srect: Rect) -> Option<(ShapeId, deckcraft_model::MediaClip, Rect)> {
    if !doc.show.show_media_controls || show.pen || show.ended || show.blank.is_some() {
        return None;
    }
    let p = ctx.input(|i| i.pointer.hover_pos())?;
    let items = slide_media_bars(srect, doc, show.state.slide);
    if let Some((id, clip, _, bar)) = items.iter().rev().find(|(_, _, _, bar)| bar.contains(p)) {
        return Some((*id, clip.clone(), *bar));
    }
    let (id, clip, _, bar) = items.iter().rev().find(|(_, _, r, _)| r.contains(p))?;
    Some((*id, clip.clone(), *bar))
}

/// True when the pointer is on a visible playback bar, so that click must not advance the show.
fn pointer_on_media_bar(ctx: &egui::Context, ui: &Ui, doc: &Presentation, show: &Show) -> bool {
    if !doc.show.show_media_controls || show.pen || show.ended || show.blank.is_some() {
        return false;
    }
    let Some(p) = ctx.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())) else { return false };
    let srect = shown_slide_rect(ui, show, doc);
    slide_media_bars(srect, doc, show.state.slide).iter().any(|(_, _, _, bar)| bar.contains(p))
}

/// The media shape under the pointer on the shown slide.
fn media_hit(ctx: &egui::Context, ui: &Ui, show: &Show, doc: &Presentation) -> Option<(ShapeId, deckcraft_model::MediaClip)> {
    let p = ctx.input(|i| i.pointer.interact_pos())?;
    let full = ui.max_rect();
    let avail = if show.reading { Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - 34.0)) } else { full };
    let srect = fit(avail, doc);
    let k = srect.width() / doc.slide_size.width.max(1.0) as f32;
    crate::media::slide_media(doc, show.state.slide).into_iter().rev().find_map(|(id, x, clip)| {
        let r = Rect::from_min_size(pos2(srect.min.x + x.x as f32 * k, srect.min.y + x.y as f32 * k), vec2(x.w as f32 * k, x.h as f32 * k));
        r.contains(p).then_some((id, clip))
    })
}

/// What a click on the pointer's spot acts on.
#[derive(Clone)]
enum Pick {
    /// A hyperlink in text.
    Text(Hyperlink),
    /// A shape with an action or a triggered animation.
    Shape,
}

/// The slide's shapes at the pointer, for [`ShowState::click`].
#[derive(Default)]
struct Under {
    shapes: Vec<HitShape>,
    /// Whether each of `shapes` is currently shown (an animation may have hidden it).
    visible: Vec<bool>,
    pick: Option<Pick>,
}

impl Under {
    fn visible(&self, id: ShapeId) -> bool {
        self.shapes.iter().zip(&self.visible).find(|(h, _)| h.id == id).is_none_or(|(_, v)| *v)
    }
}

/// What is under the pointer on the shown slide: the slide's shapes with their animation state, the
/// topmost of them that does something when clicked, and a text hyperlink on it. Only shapes that
/// are shown can be clicked, and a shape without an action hides the ones beneath it.
fn shapes_under_pointer(ctx: &egui::Context, ui: &Ui, show: &mut Show, doc: &Presentation, now: f64) -> Option<Under> {
    let p = ctx.input(|i| i.pointer.hover_pos())?;
    let full = ui.max_rect();
    let avail = if show.reading { Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - 34.0)) } else { full };
    let srect = fit(avail, doc);
    let k = (srect.width() / doc.slide_size.width.max(1.0) as f32) as f64;
    let (x, y) = ((p.x - srect.min.x) as f64 / k, (p.y - srect.min.y) as f64 / k);
    let i = show.state.slide;
    if show.links.as_ref().is_none_or(|(s, ..)| *s != i) {
        let placed = deckcraft_render::place_slide(doc, i);
        show.links = Some((i, placed.links, placed.shapes));
    }
    show.timeline(doc);
    let (Some((_, tl)), Some((_, links, shapes))) = (&show.timeline, &show.links) else { return None };
    let inside = |x0: f64, y0: f64, x1: f64, y1: f64| x0 <= x && x <= x1 && y0 <= y && y <= y1;
    let hits: Vec<HitShape> = shapes
        .iter()
        .map(|s| HitShape {
            id: s.id,
            parent: s.parent,
            is_group: s.is_group,
            link: s.link.as_ref().map(|l| l.action.clone()),
            under_pointer: inside(s.rect.x0, s.rect.y0, s.rect.x1, s.rect.y1),
        })
        .collect();
    let (step, plays, stamps) = (show.state.step, show.state.trigger_plays(), show.state.trigger_stamps());
    let t = if show.state.playing { now - show.step_start } else { 0.0 };
    let visible: Vec<bool> = shapes
        .iter()
        .map(|s| {
            let a = tl.state_ordered(s.id, None, step, t, &plays, &stamps);
            a.visible && a.opacity > 0.0
        })
        .collect();
    let mut under = Under { shapes: hits, visible, pick: None };
    let shown = |id: ShapeId| under.visible(id);
    // A hyperlink in text applies when its shape is the one on top.
    let top = show.state.top(doc, tl, &under.shapes, &shown);
    let text = links.iter().rev().filter(|l| l.text && inside(l.rect.x0, l.rect.y0, l.rect.x1, l.rect.y1)).find(|l| {
        let mut at = top;
        (0..=under.shapes.len()).any(|_| match at.and_then(|k| under.shapes.get(k)) {
            Some(h) if h.id == l.shape => true,
            Some(h) => {
                at = h.parent;
                false
            }
            None => false,
        })
    });
    under.pick = match text {
        Some(l) => Some(Pick::Text(l.link.clone())),
        None => show.state.target(doc, tl, &under.shapes, &shown).map(|_| Pick::Shape),
    };
    Some(under)
}

impl Show {
    /// Advance the clock of the running triggered animations.
    fn tick_triggers(&mut self, doc: &Presentation, now: f64) {
        self.timeline(doc);
        if let Some((_, tl)) = &self.timeline {
            self.state.tick(tl, now);
        }
    }

    /// A click on the slide: the shape under the pointer acts, or the background advances.
    fn click_slide(&mut self, doc: &Presentation, under: &Under, now: f64) -> Click {
        self.timeline(doc);
        let last = self.last_viewed;
        match &self.timeline {
            Some((_, tl)) => self.state.click(doc, tl, &under.shapes, &|id| under.visible(id), now, last),
            None => Click::None,
        }
    }
}

/// A slide-space box on screen, the slide being drawn in `srect` at `k` pixels per point.
fn on_screen(srect: Rect, k: f32, x: &deckcraft_geom::Xfrm) -> Rect {
    Rect::from_min_size(pos2(srect.min.x + x.x as f32 * k, srect.min.y + x.y as f32 * k), vec2(x.w as f32 * k, x.h as f32 * k))
}

/// The animated GIF whose play/pause button is under the pointer on the shown slide.
fn gif_hit(ctx: &egui::Context, ui: &Ui, show: &Show, doc_uid: u64, doc: &Presentation, app: &mut SlideApp) -> Option<ShapeId> {
    let p = ctx.input(|i| i.pointer.interact_pos())?;
    let full = ui.max_rect();
    let avail = if show.reading { Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - 34.0)) } else { full };
    let srect = fit(avail, doc);
    let k = srect.width() / doc.slide_size.width.max(1.0) as f32;
    let gifs = app.media.gifs(doc_uid, doc, show.state.slide, ctx.input(|i| i.time), &app.session.gif_paused);
    gifs.iter().rev().find(|g| crate::media::gif_button_rect(on_screen(srect, k, &g.rect)).contains(p)).map(|g| g.id)
}

impl Show {
    fn zoom_uv(&self) -> Rect {
        self.zoom.map(Zoom::uv).unwrap_or_else(unit_uv)
    }

    fn zoom_label(&self) -> &'static str {
        if self.zoom.is_some() {
            "Zoom on"
        } else if self.zoom_arm {
            "Zoom…"
        } else {
            "Zoom"
        }
    }

    /// G opens See All Slides in the window it was pressed in: Presenter View's grid stays off the audience screen (#1130).
    fn open_grid(&mut self, presenter: bool) {
        if presenter {
            self.presenter_grid = true;
        } else {
            self.grid = true;
        }
    }

    /// Z, or the Zoom button: arm a click-to-zoom, or leave a zoom that is already on.
    fn zoom_key(&mut self) {
        if self.zoom.is_some() || self.zoom_arm {
            self.dismiss_zoom();
        } else {
            self.zoom_arm = true;
            self.pen = false;
        }
    }

    /// Esc closes the See All Slides of the window it was pressed in, then a zoom, and only then leaves the show.
    fn back_out(&mut self, presenter: bool) -> bool {
        if presenter && self.presenter_grid {
            self.presenter_grid = false;
            true
        } else if !presenter && self.grid {
            self.grid = false;
            true
        } else {
            self.dismiss_zoom()
        }
    }

    /// Esc backs out of a zoom before it leaves the show. True when it did.
    fn dismiss_zoom(&mut self) -> bool {
        if self.zoom.is_some() || self.zoom_arm {
            self.zoom = None;
            self.zoom_arm = false;
            true
        } else {
            false
        }
    }

    pub fn blank_name(&self) -> Option<&'static str> {
        match self.blank {
            Some(c) if c == Color32::BLACK => Some("black"),
            Some(c) if c == Color32::WHITE => Some("white"),
            Some(_) => Some("blank"),
            None => None,
        }
    }

    pub fn zoom_view(&self) -> Option<(f32, f32, f32)> {
        self.zoom.map(|z| (z.nx, z.ny, z.scale))
    }

    fn push_slide_number(&mut self, digit: char, now: f64) {
        if !digit.is_ascii_digit() || self.goto.len() >= GOTO_DIGITS {
            return;
        }
        self.goto.push(digit);
        self.goto_at = if now.is_finite() { now } else { 0.0 };
    }

    /// 1-based slide number, or `None` for 0 or a non-number. The buffer is cleared either way.
    fn take_goto(&mut self) -> Option<usize> {
        let raw = std::mem::take(&mut self.goto);
        raw.parse::<usize>().ok().and_then(|n| n.checked_sub(1))
    }

    fn expire_goto(&mut self, now: f64) {
        if self.goto.is_empty() || !now.is_finite() || !self.goto_at.is_finite() {
            return;
        }
        if now - self.goto_at > GOTO_HOLD {
            self.goto.clear();
        }
    }

    /// Esc drops a number in progress. True when it did.
    fn clear_goto(&mut self) -> bool {
        if self.goto.is_empty() {
            false
        } else {
            self.goto.clear();
            true
        }
    }

    fn nudge_notes(&mut self, delta: f32) {
        let next = self.notes_px + delta;
        if next.is_finite() {
            self.notes_px = next.clamp(NOTES_MIN, NOTES_MAX);
        }
    }

    pub fn showing_value(&self, n_slides: usize, aspect: f32, full: Rect, now: f64) -> serde_json::Value {
        let mut v = json!({
            "slide": self.state.slide,
            "step": self.state.step,
            "ended": self.ended,
            "grid": self.grid,
            "zoomArm": self.zoom_arm,
            "blank": self.blank_name(),
            "zoom": self.zoom_view().map(|(x, y, scale)| json!({"x": x, "y": y, "scale": scale})),
            "presenterPaused": self.timer.paused(),
            "presenterSecs": self.timer.elapsed(now),
            "notesPx": self.notes_px,
            "custom": self.state.custom_name(),
            "goto": if self.goto.is_empty() { None } else { Some(self.goto.clone()) },
        });
        if self.grid {
            let slides = grid_indices(n_slides);
            let cells = see_all_cells(full, slides.len(), aspect);
            v["cells"] =
                serde_json::Value::Array(slides.iter().zip(cells).map(|(i, r)| json!({"slide": i, "x": r.center().x, "y": r.center().y})).collect());
        }
        v
    }

    fn reading_bar_hit(&self, ctx: &egui::Context) -> bool {
        // Clicks on the on-screen controls must not also advance.
        ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| {
            let r = ctx.content_rect();
            (self.reading && p.y > r.max.y - 34.0) || (!self.reading && tool_bar_rect(r).expand(8.0).contains(p))
        })
    }
}

const GOTO_HOLD: f64 = 3.0;
const GOTO_DIGITS: usize = 4;
const ZOOM_SCALE: f32 = 2.0;
const GRID_CAP: usize = 48;
const TOOL_BAR_W: f32 = 520.0;
const NOTES_DEFAULT: f32 = 18.0;
const NOTES_MIN: f32 = 12.0;
const NOTES_MAX: f32 = 48.0;
const NOTES_STEP: f32 = 4.0;

/// Presenter-view elapsed time. Pausing freezes the number; the show keeps playing.
#[derive(Clone, Copy)]
struct PresenterTimer {
    origin: f64,
    frozen: f64,
    paused: bool,
}

impl PresenterTimer {
    fn start(now: f64) -> Self {
        PresenterTimer { origin: finite_time(now), frozen: 0.0, paused: false }
    }

    fn elapsed(self, now: f64) -> f64 {
        if self.paused {
            return finite_secs(self.frozen);
        }
        let dt = now - self.origin;
        finite_secs(self.frozen + if dt.is_finite() && dt > 0.0 { dt } else { 0.0 })
    }

    fn paused(self) -> bool {
        self.paused
    }

    fn toggle(&mut self, now: f64) {
        if self.paused {
            self.origin = finite_time(now);
            self.paused = false;
        } else {
            self.frozen = self.elapsed(now);
            self.paused = true;
        }
    }

    /// Zero the elapsed time. A paused timer stays paused at zero.
    fn reset(&mut self, now: f64) {
        self.frozen = 0.0;
        self.origin = finite_time(now);
    }
}

fn finite_time(now: f64) -> f64 {
    if now.is_finite() { now } else { 0.0 }
}

fn finite_secs(n: f64) -> f64 {
    if n.is_finite() && n >= 0.0 { n } else { 0.0 }
}

fn format_elapsed(secs: f64) -> String {
    let s = finite_secs(secs) as u64;
    let s = s.min(99 * 3600 + 59 * 60 + 59);
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// Local wall clock for presenter view. `--:--` where there is no clock (wasm).
fn wall_clock() -> String {
    match deckcraft_text::datetime::DateTime::now_local() {
        Some(t) if t.hour < 24 && t.minute < 60 => format!("{:02}:{:02}", t.hour, t.minute),
        _ => "--:--".to_string(),
    }
}

#[derive(Clone, Copy)]
struct Zoom {
    nx: f32,
    ny: f32,
    scale: f32,
}

impl Zoom {
    fn at(nx: f32, ny: f32) -> Self {
        Zoom { nx: finite_unit(nx), ny: finite_unit(ny), scale: ZOOM_SCALE }
    }

    fn uv(self) -> Rect {
        zoom_uv(self.nx, self.ny, self.scale)
    }
}

fn finite_unit(n: f32) -> f32 {
    if n.is_finite() { n.clamp(0.0, 1.0) } else { 0.5 }
}

fn plain(m: egui::Modifiers) -> bool {
    !m.command && !m.ctrl && !m.alt && !m.shift
}

fn unit_uv() -> Rect {
    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
}

fn slide_aspect(doc: &Presentation) -> f32 {
    let a = doc.slide_size.width as f32 / doc.slide_size.height.max(1.0) as f32;
    if a.is_finite() && a > 0.05 { a } else { 16.0 / 9.0 }
}

/// The part of the slide a zoom shows, in 0–1 texture space. The window stays on the slide.
fn zoom_uv(nx: f32, ny: f32, scale: f32) -> Rect {
    let scale = if scale.is_finite() && scale > 1.0 { scale } else { ZOOM_SCALE };
    let w = 1.0 / scale;
    if w >= 1.0 {
        return unit_uv();
    }
    let x = (nx - w * 0.5).clamp(0.0, 1.0 - w);
    let y = (ny - w * 0.5).clamp(0.0, 1.0 - w);
    Rect::from_min_size(pos2(x, y), vec2(w, w))
}

fn zoom_frame(slide: Rect, uv: Rect) -> Rect {
    Rect::from_min_size(
        pos2(slide.min.x + uv.min.x * slide.width(), slide.min.y + uv.min.y * slide.height()),
        vec2(uv.width() * slide.width(), uv.height() * slide.height()),
    )
}

fn toggle_blank(show: &mut Show, color: Color32) {
    show.blank = if show.blank == Some(color) { None } else { Some(color) };
}

fn tool_bar_rect(full: Rect) -> Rect {
    let w = TOOL_BAR_W.min((full.width() - 32.0).max(32.0));
    Rect::from_min_size(pos2(full.min.x + 16.0, full.max.y - 50.0), vec2(w, 34.0))
}

fn grid_indices(n: usize) -> Vec<usize> {
    (0..n.min(GRID_CAP)).collect()
}

/// Thumbnail rectangles for `n` slides inside `full`, largest 16:9-style thumbs that fit.
fn see_all_cells(full: Rect, n: usize, aspect: f32) -> Vec<Rect> {
    if n == 0 || full.width() < 16.0 || full.height() < 16.0 {
        return vec![];
    }
    let aspect = if aspect.is_finite() && aspect > 0.05 { aspect } else { 16.0 / 9.0 };
    let margin = 28.0;
    let gap = 16.0;
    let label_h = 22.0;
    let inner = full.shrink(margin);
    let inner = Rect::from_min_max(pos2(inner.min.x, inner.min.y + 36.0), inner.max);
    if inner.width() < 8.0 || inner.height() < 8.0 {
        return vec![];
    }
    let mut best: Vec<Rect> = vec![];
    let mut best_area = 0.0_f32;
    for cols in 1..=n.min(8) {
        let rows = n.div_ceil(cols);
        let cell_w = (inner.width() - gap * (cols.saturating_sub(1) as f32)) / cols as f32;
        let cell_h = (inner.height() - gap * (rows.saturating_sub(1) as f32)) / rows as f32;
        if cell_w < 8.0 || cell_h < label_h + 8.0 {
            continue;
        }
        let thumb_max_h = cell_h - label_h;
        let mut tw = cell_w;
        let mut th = tw / aspect;
        if th > thumb_max_h {
            th = thumb_max_h;
            tw = th * aspect;
        }
        if tw < 8.0 || th < 8.0 {
            continue;
        }
        let area = tw * th;
        if area <= best_area {
            continue;
        }
        best_area = area;
        best = (0..n)
            .map(|i| {
                let c = i % cols;
                let r = i / cols;
                let ox = inner.min.x + c as f32 * (cell_w + gap) + (cell_w - tw) * 0.5;
                let oy = inner.min.y + r as f32 * (cell_h + gap);
                Rect::from_min_size(pos2(ox, oy), vec2(tw, th))
            })
            .collect();
    }
    best
}

fn see_all_hit(full: Rect, n: usize, aspect: f32, p: Pos2) -> Option<usize> {
    see_all_cells(full, n, aspect).into_iter().position(|r| r.expand2(vec2(4.0, 18.0)).contains(p))
}

fn norm_on_slide(srect: Rect, uv: Rect, p: Pos2) -> Option<(f32, f32)> {
    if srect.width() <= 1.0 || srect.height() <= 1.0 || !srect.contains(p) {
        return None;
    }
    let u = (p.x - srect.min.x) / srect.width();
    let v = (p.y - srect.min.y) / srect.height();
    let nx = uv.min.x + u * uv.width();
    let ny = uv.min.y + v * uv.height();
    (nx.is_finite() && ny.is_finite()).then_some((nx.clamp(0.0, 1.0), ny.clamp(0.0, 1.0)))
}

fn slide_to_screen(srect: Rect, uv: Rect, nx: f32, ny: f32) -> Pos2 {
    let u = (nx - uv.min.x) / uv.width().max(1e-4);
    let v = (ny - uv.min.y) / uv.height().max(1e-4);
    pos2(srect.min.x + u * srect.width(), srect.min.y + v * srect.height())
}

fn slide_box(srect: Rect, uv: Rect, slide_w: f32, slide_h: f32, x: f32, y: f32, w: f32, h: f32) -> Rect {
    let a = slide_to_screen(srect, uv, x / slide_w.max(1.0), y / slide_h.max(1.0));
    let b = slide_to_screen(srect, uv, (x + w) / slide_w.max(1.0), (y + h) / slide_h.max(1.0));
    Rect::from_min_max(a, b)
}

fn paint_grid(painter: &egui::Painter, ctx: &egui::Context, show: &mut Show, doc: &Presentation, full: Rect) {
    let slides = grid_indices(doc.slides.len());
    let aspect = slide_aspect(doc);
    let cells = see_all_cells(full, slides.len(), aspect);
    painter.rect_filled(full, CornerRadius::ZERO, Color32::from_gray(16));
    painter.text(pos2(full.center().x, full.min.y + 28.0), Align2::CENTER_CENTER, "All slides", theme::font(20.0), Color32::WHITE);
    if doc.slides.len() > GRID_CAP {
        painter.text(
            pos2(full.center().x, full.min.y + 50.0),
            Align2::CENTER_CENTER,
            format!("First {GRID_CAP} slides"),
            theme::font(13.0),
            Color32::from_gray(180),
        );
    }
    let budget = if slides.len() <= 12 { 12 } else { 4 };
    let mut made = 0;
    for (i, rect) in slides.iter().copied().zip(cells) {
        painter.rect_filled(rect, CornerRadius::same(4), Color32::from_gray(32));
        let tex = match show.thumbs.iter().find(|(k, _)| *k == i).map(|(_, t)| t.clone()) {
            Some(t) => Some(t),
            None if made < budget => {
                made += 1;
                render_thumb(ctx, show, doc, i)
            }
            None => {
                ctx.request_repaint();
                None
            }
        };
        if let Some(t) = tex {
            painter.image(t.id(), rect, unit_uv(), Color32::WHITE);
        }
        let current = i == show.state.slide;
        painter.rect_stroke(
            rect,
            CornerRadius::same(4),
            Stroke::new(if current { 3.0 } else { 1.0 }, if current { Color32::WHITE } else { Color32::from_gray(70) }),
            egui::StrokeKind::Inside,
        );
        let hidden = doc.slides.get(i).is_some_and(|s| s.hidden);
        let label = if hidden { format!("{}  Hidden", i + 1) } else { format!("{}", i + 1) };
        painter.text(
            pos2(rect.center().x, rect.max.y + 12.0),
            Align2::CENTER_CENTER,
            label,
            theme::font(13.0),
            if current { Color32::WHITE } else { Color32::from_gray(180) },
        );
    }
}

fn render_thumb(ctx: &egui::Context, show: &mut Show, doc: &Presentation, i: usize) -> Option<TextureHandle> {
    if show.thumbs.len() >= GRID_CAP {
        return None;
    }
    let w = 320.0_f64;
    let h = (w * doc.slide_size.height / doc.slide_size.width.max(1.0)).round().max(1.0);
    let img = deckcraft_render::render_slide(
        doc,
        i,
        &RenderOpts { scale: w / doc.slide_size.width.max(1.0), size: Some((w as u32, h as u32)), ..Default::default() },
    );
    let tex = crate::textures::upload(ctx, format!("show-grid-{i}"), crate::textures::to_color_image(&img, false));
    show.thumbs.push((i, tex.clone()));
    Some(tex)
}

fn presenter_window(ctx: &egui::Context, show: &mut Show, doc: &Presentation, now: f64, keep: &mut bool) {
    let cur = show.state.slide;
    let next = show.state.peek_next(doc);
    let cur_tex = show.tex.as_ref().map(|(_, t)| t.clone());
    if let Some(n) = next
        && show.next_tex.as_ref().is_none_or(|(k, _)| *k != n)
    {
        // A very tall slide shape would otherwise ask for a picture taller than the GPU's limit.
        let next_size = crate::textures::fit_size(
            (480, (480.0 * doc.slide_size.height / doc.slide_size.width.max(1.0)).clamp(1.0, 1e9) as u32),
            crate::textures::max_side(ctx),
        );
        let img = deckcraft_render::render_slide(
            doc,
            n,
            &RenderOpts { scale: next_size.0 as f64 / doc.slide_size.width.max(1.0), size: Some(next_size), ..Default::default() },
        );
        show.next_tex = Some((n, crate::textures::upload(ctx, "presenter-next", crate::textures::to_color_image(&img, false))));
    }
    let next_tex = show.next_tex.as_ref().filter(|(k, _)| Some(*k) == next).map(|(_, t)| t.clone());
    let no_notes = deckcraft_model::text::TextBody::default();
    let notes = doc.slides.get(cur).map_or(&no_notes, |s| &s.notes);
    let clock = wall_clock();
    // The presenter clock only moves when this window is drawn again.
    ctx.request_repaint_after(std::time::Duration::from_millis(500));
    let mut actions = vec![];
    // The presenter window is dark; default buttons draw dark text on it (#221).
    let button = |ui: &mut Ui, s: &str| {
        ui.add(egui::Button::new(egui::RichText::new(s).size(15.0).color(Color32::WHITE)).fill(Color32::from_gray(60)).min_size(vec2(0.0, 30.0)))
    };
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("deckcraft_presenter"),
        egui::ViewportBuilder::default().with_title("Presenter View — DeckCraft").with_inner_size([1100.0, 700.0]),
        |ui, _| {
            if !keyboard(ui.ctx(), show, doc, now, true) {
                *keep = false;
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            let elapsed = show.timer.elapsed(now);
            let paused = show.timer.paused();
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::from_gray(24)).inner_margin(egui::Margin::same(16))).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format_elapsed(elapsed)).size(22.0).color(if paused {
                        Color32::from_gray(160)
                    } else {
                        Color32::WHITE
                    }));
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new(clock.clone()).size(16.0).color(Color32::from_gray(200)));
                    ui.add_space(20.0);
                    ui.label(egui::RichText::new(format!("Slide {} of {}", cur + 1, doc.slides.len())).size(16.0).color(Color32::from_gray(200)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if button(ui, "End Show").clicked() {
                            *keep = false;
                        }
                        if button(ui, "Next ▶").clicked() {
                            actions.push(show.state.next(doc));
                        }
                        if button(ui, "◀ Previous").clicked() {
                            actions.push(show.state.prev(doc));
                        }
                    });
                });
                ui.horizontal(|ui| {
                    if button(ui, if paused { "Resume" } else { "Pause" }).clicked() {
                        show.timer.toggle(now);
                    }
                    if button(ui, "Reset").clicked() {
                        show.timer.reset(now);
                    }
                    if button(ui, if show.blank == Some(Color32::BLACK) { "Black on" } else { "Black" }).clicked() {
                        toggle_blank(show, Color32::BLACK);
                    }
                    if button(ui, if show.blank == Some(Color32::WHITE) { "White on" } else { "White" }).clicked() {
                        toggle_blank(show, Color32::WHITE);
                    }
                    if button(ui, if show.presenter_grid { "All slides on" } else { "All slides" }).clicked() {
                        show.presenter_grid = !show.presenter_grid;
                    }
                    if button(ui, show.zoom_label()).clicked() {
                        show.zoom_key();
                    }
                });
                ui.add_space(8.0);
                let avail = ui.available_rect_before_wrap();
                if show.presenter_grid {
                    // The grid fills this window; the audience keeps seeing the current slide.
                    paint_grid(ui.painter(), ui.ctx(), show, doc, avail);
                    let hit = ui.interact(avail, ui.id().with("presenter_grid"), Sense::click());
                    if hit.clicked()
                        && let Some(p) = hit.interact_pointer_pos()
                        && let Some(n) = see_all_hit(avail, grid_indices(doc.slides.len()).len(), slide_aspect(doc), p)
                        && let Some(&i) = grid_indices(doc.slides.len()).get(n)
                    {
                        show.presenter_grid = false;
                        show.blank = None;
                        show.zoom = None;
                        show.zoom_arm = false;
                        actions.push(show.state.goto(doc, i));
                    }
                    return;
                }
                let left = Rect::from_min_max(avail.min, pos2(avail.min.x + avail.width() * 0.62, avail.max.y));
                let right = Rect::from_min_max(pos2(left.max.x + 16.0, avail.min.y), avail.max);
                let cr = fit(Rect::from_min_max(left.min, pos2(left.max.x, left.min.y + left.height() * 0.8)), doc);
                let cr = Rect::from_min_size(left.min, cr.size());
                if let Some(t) = &cur_tex {
                    ui.painter().image(t.id(), cr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                if let Some(z) = show.zoom {
                    let mark = zoom_frame(cr, z.uv());
                    ui.painter().rect_stroke(mark, CornerRadius::ZERO, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Inside);
                }
                if show.zoom_arm {
                    let hit = ui.interact(cr, ui.id().with("presenter_zoom"), Sense::click());
                    if hit.clicked()
                        && let Some(p) = hit.interact_pointer_pos()
                        && cr.width() > 1.0
                        && cr.height() > 1.0
                    {
                        show.zoom = Some(Zoom::at((p.x - cr.min.x) / cr.width(), (p.y - cr.min.y) / cr.height()));
                        show.zoom_arm = false;
                    }
                }
                if let Some(c) = show.blank {
                    ui.painter().rect_filled(cr, CornerRadius::ZERO, c);
                }
                let nr = next_rect(right, doc);
                ui.painter().text(right.min, Align2::LEFT_TOP, "Next", theme::font(13.0), Color32::from_gray(170));
                match &next_tex {
                    Some(t) => {
                        ui.painter().image(t.id(), nr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        ui.painter().rect_filled(nr, CornerRadius::ZERO, Color32::from_gray(40));
                        ui.painter().text(nr.center(), Align2::CENTER_CENTER, "End of slide show", theme::font(13.0), Color32::from_gray(170));
                    }
                }
                let notes_head = Rect::from_min_size(pos2(right.min.x, nr.max.y + 12.0), vec2(right.width(), 28.0));
                let mut head = ui.new_child(egui::UiBuilder::new().max_rect(notes_head));
                head.horizontal(|ui| {
                    ui.label(egui::RichText::new("Notes").size(13.0).color(Color32::from_gray(170)));
                    if ui.button("A-").clicked() {
                        show.nudge_notes(-NOTES_STEP);
                    }
                    ui.label(egui::RichText::new(format!("{}", show.notes_px as i32)).size(13.0).color(Color32::from_gray(200)));
                    if ui.button("A+").clicked() {
                        show.nudge_notes(NOTES_STEP);
                    }
                });
                let notes_rect = Rect::from_min_max(pos2(right.min.x, notes_head.max.y + 4.0), right.max);
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(notes_rect));
                egui::ScrollArea::vertical().show(&mut child, |ui| {
                    presenter_notes(ui, notes, show.notes_px);
                });
            });
            if ui.input(|i| i.viewport().close_requested()) {
                show.presenter = false;
                // A grid left open in the closed window would hold auto-advance for the rest of the show.
                show.presenter_grid = false;
            }
        },
    );
    for a in actions {
        if !show.apply(a, doc, now) {
            *keep = false;
        }
    }
}

/// The current slide's notes in the presenter's notes pane, `px` for 12 pt text.
fn presenter_notes(ui: &mut Ui, notes: &deckcraft_model::text::TextBody, px: f32) -> egui::Response {
    let color = Color32::from_gray(230);
    if notes.text().is_empty() {
        ui.label(egui::RichText::new("No notes.").size(px).color(color))
    } else {
        crate::notes::view(ui, notes, &crate::notes::Look { size: px, color: Some(color) })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// #1005: shapes stacked above a playing video must be drawn over its frame; the video, what
    /// is below it and other videos stay out of that overlay.
    #[test]
    fn shapes_above_a_playing_video_are_the_overlay() {
        let mut session = deckcraft_engine::Session::with_new();
        for _ in 0..3 {
            session.execute("shape.insert", &json!({"preset": "rect", "rect": [0.0, 0.0, 100.0, 100.0]})).unwrap();
        }
        let mut doc = (*session.active().unwrap().doc).clone();
        // Keep only the three inserted rects (a new slide may start with placeholders).
        let slide = std::sync::Arc::make_mut(&mut doc.slides[0]);
        let keep = slide.shapes.len() - 3;
        slide.shapes.drain(..keep);
        let ids: Vec<ShapeId> = slide.shapes.iter().map(|s| s.id).collect();
        slide.shapes[1].kind = deckcraft_model::ShapeKind::Media(deckcraft_model::MediaClip { video: true, ..Default::default() });
        // Video in the middle: the shape above it is drawn over it.
        assert_eq!(below_playing_video(&doc, 0, &[ids[1]]), vec![ids[0], ids[1]]);
        // Video on top: nothing above it, the frame is painted last as before.
        std::sync::Arc::make_mut(&mut doc.slides[0]).shapes.swap(1, 2);
        assert!(below_playing_video(&doc, 0, &[ids[1]]).is_empty());
        // No video playing.
        assert!(below_playing_video(&doc, 0, &[]).is_empty());
    }

    /// #968: over a shape a click acts on (a clip with its play trigger, a link) the pointing hand
    /// never gave way to the idle hide, so a still pointer stayed on the playing video.
    #[test]
    fn a_still_cursor_hides_over_a_clickable_shape() {
        let mut session = deckcraft_engine::Session::with_new();
        let size = session.active().unwrap().doc.slide_size;
        let r = session.execute("shape.insert", &json!({"preset": "rect", "rect": [0.0, 0.0, size.width, size.height]})).unwrap();
        session.execute("insert.hyperlink", &json!({"url": "https://example.org", "ids": [r["id"]]})).unwrap();
        let app = SlideApp::new(session, crate::Services::default());
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(1000.0, 700.0)).with_step_dt(0.25).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        harness.run_steps(3);
        harness.state_mut().start_show(0, false);
        harness.run_steps(2);
        harness.hover_at(pos2(500.0, 350.0));
        harness.run_steps(2);
        assert_eq!(harness.output().platform_output.cursor_icon, egui::CursorIcon::PointingHand);
        harness.run_steps(12);
        assert_eq!(harness.output().platform_output.cursor_icon, egui::CursorIcon::None, "still for 3 s: hidden");
    }

    /// #1130: the Presenter View's All slides button opens the grid in that window, not on the audience screen.
    #[test]
    fn presenter_all_slides_does_not_cover_the_audience_screen() {
        use egui_kittest::kittest::Queryable;

        let mut session = deckcraft_engine::Session::with_new();
        session.execute("slide.new", &json!({})).unwrap();
        let doc = session.active().unwrap().doc.clone();
        let show = Show::new(&doc, 0, false, true);
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(1100.0, 700.0)).build_ui_state(
            |ui, (doc, show, keep): &mut (std::sync::Arc<Presentation>, Show, bool)| {
                let now = ui.input(|i| i.time);
                presenter_window(ui.ctx(), show, doc, now, keep);
            },
            (doc, show, true),
        );
        harness.run_steps(2);
        harness.get_by_label("All slides").click();
        harness.run_steps(2);
        assert!(harness.state().1.presenter_grid, "the grid opens in the Presenter View");
        assert!(!harness.state().1.grid, "the audience screen keeps showing the slide");
        harness.key_press(egui::Key::Escape);
        harness.run_steps(2);
        assert!(!harness.state().1.presenter_grid, "Esc closes the Presenter View grid");
        assert!(harness.state().2, "and does not end the show");
        harness.key_press(egui::Key::G);
        harness.run_steps(2);
        assert!(harness.state().1.presenter_grid, "G in Presenter View opens its own grid");
        assert!(!harness.state().1.grid, "G in Presenter View leaves the audience screen alone");
        harness.key_press(egui::Key::G);
        harness.run_steps(2);
        assert!(!harness.state().1.presenter_grid, "G again closes it");
    }

    /// Send real egui key events to the presenter UI, without the audience window's handler.
    #[test]
    fn presenter_navigation_uses_its_own_keyboard_events() {
        use egui_kittest::kittest::Queryable;

        let mut session = deckcraft_engine::Session::with_new();
        session.execute("slide.new", &json!({})).unwrap();
        session.execute("slide.new", &json!({})).unwrap();
        let doc = session.active().unwrap().doc.clone();
        let show = Show::new(&doc, 0, false, true);
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(1100.0, 700.0)).build_ui_state(
            |ui, (doc, show, keep): &mut (std::sync::Arc<Presentation>, Show, bool)| {
                let now = ui.input(|i| i.time);
                presenter_window(ui.ctx(), show, doc, now, keep);
            },
            (doc, show, true),
        );
        harness.run_steps(2);
        for (key, slide) in [
            (egui::Key::ArrowRight, 1),
            (egui::Key::ArrowLeft, 0),
            (egui::Key::Space, 1),
            (egui::Key::PageDown, 2),
            (egui::Key::PageUp, 1),
            (egui::Key::Home, 0),
            (egui::Key::End, 2),
        ] {
            harness.key_press(key);
            harness.run_steps(2);
            assert_eq!(harness.state().1.state.slide, slide, "presenter key {key:?}");
        }
        harness.key_press(egui::Key::ArrowRight);
        harness.run_steps(2);
        assert!(harness.state().1.ended);
        harness.key_press(egui::Key::ArrowLeft);
        harness.run_steps(2);
        assert!(!harness.state().1.ended);
        assert_eq!(harness.state().1.state.slide, 2, "back from the end screen restores the last slide");
        harness.key_press(egui::Key::ArrowRight);
        harness.run_steps(2);
        assert!(harness.state().1.ended);
        harness.get_by_label("◀ Previous").click();
        harness.run_steps(2);
        assert!(!harness.state().1.ended, "the presenter Previous button also leaves the end screen");
        harness.key_press(egui::Key::Home);
        harness.run_steps(2);
        harness.get_by_label("Next ▶").click();
        harness.run_steps(2);
        assert_eq!(harness.state().1.state.slide, 1);
        harness.key_press(egui::Key::Space);
        harness.run_steps(2);
        assert_eq!(harness.state().1.state.slide, 2);
        assert!(!harness.state().1.ended, "Space must not also activate the focused Next button");
        harness.key_press(egui::Key::Escape);
        harness.run_steps(2);
        assert!(!harness.state().2, "Escape from Presenter View ends the show");
    }

    #[test]
    fn slideshow_navigation_advances_once_with_presenter_view() {
        use egui_kittest::kittest::Queryable;

        let mut session = deckcraft_engine::Session::with_new();
        session.execute("slide.new", &json!({})).unwrap();
        session.execute("slide.new", &json!({})).unwrap();
        let mut app = SlideApp::new(session, crate::Services::default());
        app.ui.presenter_view = true;
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(1000.0, 700.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        harness.run_steps(3);
        harness.state_mut().start_show(0, false);
        harness.run_steps(2);
        harness.key_press(egui::Key::ArrowRight);
        harness.run_steps(2);
        assert_eq!(harness.state().show.as_ref().unwrap().state.slide, 1, "one key advances one slide");
        harness.key_press(egui::Key::End);
        harness.run_steps(2);
        harness.key_press(egui::Key::ArrowRight);
        harness.run_steps(2);
        assert!(harness.state().show.as_ref().unwrap().ended);
        assert!(harness.query_by_label("End Show").is_some(), "the presenter remains available at the end screen");
        harness.key_press(egui::Key::Escape);
        harness.run_steps(2);
        assert!(harness.state().show.is_none());
    }

    #[test]
    fn the_prefetched_frame_is_the_frame_shown_on_entry() {
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &json!({"title": "Second"})).unwrap();
        let doc = s.active().unwrap().doc.clone();
        let size = (96, 54);
        let p = start_prefetch(&doc, 1, size).expect("prefetch starts");
        assert_eq!(p.key, Key { slide: 1, step: 0, size, doc: std::sync::Arc::as_ptr(&doc) as usize, hidden: vec![], gif: 0, plays: vec![] });
        let got = p.rx.recv().expect("the frame arrives");
        let want = render_frame(&doc, 1, ShowState::timeline(&doc, 1), 0, 0.0, size, vec![], &[], 0, &[], &[], None);
        assert_eq!(got.size, want.size);
        assert_eq!(got.pixels, want.pixels);
    }

    /// A glowing full-slide picture in a layout made every animation frame re-render (and re-blur)
    /// the backdrop: 2–3 frames a second. Frames now draw the slide's shapes over the kept
    /// backdrop, and must look the same as a whole-slide render.
    #[test]
    fn an_animation_frame_over_the_kept_backdrop_matches_a_full_render() {
        let mut s = deckcraft_engine::Session::with_new();
        let r = s.execute("shape.insert", &json!({"preset": "star5", "rect": [100.0, 100.0, 300.0, 200.0]})).unwrap();
        s.execute("animation.set", &json!({"effect": "fade", "ids": [r["id"]]})).unwrap();
        let mut doc = (*s.active().unwrap().doc).clone();
        // A glowing master graphic under the animated star.
        let mut deco = doc.slides[0].shapes.last().unwrap().clone();
        deco.id = ShapeId(9000);
        deco.xfrm = deco.xfrm.map(|x| deckcraft_geom::Xfrm { x: x.x - 60.0, y: x.y + 40.0, ..x });
        deco.effects = Some(deckcraft_model::Effects {
            glow: Some(deckcraft_model::style::Glow {
                color: deckcraft_model::style::ColorRef::rgb(deckcraft_color::Rgba::rgb(0, 120, 255)),
                radius: 12.0,
            }),
            ..Default::default()
        });
        std::sync::Arc::make_mut(&mut doc.masters[0]).shapes.push(deco);
        let doc = std::sync::Arc::new(doc);
        let size = (480, 270);
        let key = BackKey { slide: 0, size, doc: std::sync::Arc::as_ptr(&doc) as usize };
        let mut show = Show::new(&doc, 0, false, false);
        show.prefetch_backdrop(&doc, key);
        let back = show.backdrop(&doc, key);
        let tl = ShowState::timeline(&doc, 0);
        assert!(!tl.is_empty());
        // Half-way through the fade, the star half see-through over the glow.
        let fast = render_frame(&doc, 0, tl.clone(), 0, 0.25, size, vec![], &[], 0, &[], &[], Some(&back));
        let full = render_frame(&doc, 0, tl, 0, 0.25, size, vec![], &[], 0, &[], &[], None);
        assert_eq!(fast.size, full.size);
        let worst = fast
            .pixels
            .iter()
            .zip(&full.pixels)
            .map(|(a, b)| a.to_array().iter().zip(b.to_array()).map(|(x, y)| x.abs_diff(y)).max().unwrap_or(0))
            .max();
        assert!(worst.is_some_and(|d| d <= 2), "frames differ by up to {worst:?}");
        // The backdrop is kept, not rendered again.
        assert!(std::sync::Arc::ptr_eq(&back, &show.backdrop(&doc, key)));
        // Another document under the same key (an edit whose copy reused the old address) gets
        // its own backdrop.
        let edited = std::sync::Arc::new((*doc).clone());
        assert!(!std::sync::Arc::ptr_eq(&back, &show.backdrop(&edited, key)));
    }

    #[test]
    fn a_transition_keeps_the_old_slide_in_its_own_texture() {
        // #33: the old slide's handle was cloned, so the new slide was rendered into the same
        // texture and every transition blended the new slide with itself.
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &json!({})).unwrap();
        s.execute("transition.set", &json!({"kind": "fade", "index": 1})).unwrap();
        let doc = s.active().unwrap().doc.clone();
        let ctx = egui::Context::default();
        let red = crate::textures::upload(&ctx, "show", egui::ColorImage::filled([1, 1], Color32::RED));
        let mut show = Show::new(&doc, 0, false, false);
        show.tex = Some((Key { slide: 0, step: 0, size: (1, 1), doc: 0, hidden: vec![], gif: 0, plays: vec![] }, red.clone()));
        assert!(show.apply(ShowAction::Slide(1), &doc, 0.0));
        let tr = show.trans.as_ref().expect("the fade plays");
        assert_eq!(tr.old.as_ref().map(TextureHandle::id), Some(red.id()));
        assert_eq!(tr.from, Some(0));
        assert!(show.tex.is_none(), "the new slide gets a texture of its own instead of overwriting the old one");
    }

    #[test]
    fn media_in_the_click_sequence_plays_with_its_step() {
        // #116: media set to play in the click sequence never started; only a click on it did.
        let s = deckcraft_engine::Session::with_new();
        let mut doc = s.active().unwrap().doc.clone();
        let id = doc.slides[0].shapes[0].id;
        let play = |delay_ms| deckcraft_model::Animation {
            shape: id,
            class: deckcraft_model::AnimClass::Media,
            effect: "play".into(),
            duration_ms: 5000,
            delay_ms,
            ..Default::default()
        };
        std::sync::Arc::make_mut(&mut std::sync::Arc::make_mut(&mut doc).slides[0]).animations = vec![play(0), play(2000)];
        let mut show = Show::new(&doc, 0, false, false);
        assert!(show.apply(ShowAction::Slide(0), &doc, 0.0));
        // Next: the first step plays and sets off its Play once.
        let a = show.state.next(&doc);
        assert_eq!(a, ShowAction::PlayStep(0));
        assert!(show.apply(a, &doc, 1.0));
        show.cue_media(&doc, 0, 0.0);
        show.cue_media(&doc, 0, 0.5);
        assert_eq!(std::mem::take(&mut show.media_cues), [(id, "play".to_string())]);
        // The step doesn't wait for the video: the step clock ends it at once.
        assert_eq!(show.timeline(&doc).step_duration(0), 0.0);
        let a = show.state.step_done(&doc);
        assert!(show.apply(a, &doc, 1.0));
        assert!(show.media_cues.is_empty(), "set off once");
        // Next again, before the second Play's delay: cutting the step short still sets it off.
        let a = show.state.next(&doc);
        assert_eq!(a, ShowAction::PlayStep(1));
        assert!(show.apply(a, &doc, 2.0));
        show.cue_media(&doc, 1, 0.5);
        assert!(show.media_cues.is_empty());
        let a = show.state.step_done(&doc);
        assert!(show.apply(a, &doc, 2.6));
        assert_eq!(std::mem::take(&mut show.media_cues), [(id, "play".to_string())]);
        // Going back doesn't set anything off.
        let a = show.state.prev(&doc);
        assert!(show.apply(a, &doc, 3.0));
        show.cue_media(&doc, 1, 9.0);
        assert!(show.media_cues.is_empty());
    }

    #[test]
    fn the_laser_and_the_pen_are_not_on_together() {
        let doc = deckcraft_model::Presentation::default();
        let mut show = Show::new(&doc, 0, false, false);
        show.set_laser(true);
        assert!(show.laser && !show.pen);
        show.set_pen(true);
        assert!(show.pen && !show.laser);
        show.set_laser(true);
        show.arrow();
        assert!(!show.laser && !show.pen);
        show.set_laser(false);
        assert!(!show.laser);
    }

    fn deck_with_first_animation(start: deckcraft_model::anim::AnimStart) -> Presentation {
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &json!({})).unwrap();
        let mut doc = Presentation::clone(&s.active().unwrap().doc);
        let slide = std::sync::Arc::make_mut(&mut doc.slides[1]);
        let shape = slide.shapes[0].id;
        slide.animations = vec![deckcraft_model::anim::Animation { shape, start, duration_ms: 3000, ..Default::default() }];
        doc
    }

    #[test]
    fn a_first_animation_after_or_with_previous_plays_when_the_slide_appears() {
        // #631: it was started twice and the second start finished it, so the slide appeared
        // fully built.
        use deckcraft_model::anim::AnimStart;
        for start in [AnimStart::AfterPrevious, AnimStart::WithPrevious] {
            let doc = deck_with_first_animation(start);
            // Starting the show on the slide.
            let mut show = Show::new(&doc, 1, false, false);
            show.after_enter(&doc, 5.0);
            assert_eq!((show.state.step, show.state.playing), (0, true), "{start:?} from the start");
            assert_eq!(show.step_start, 5.0);
            // Arriving from the slide before.
            let mut show = Show::new(&doc, 0, false, false);
            let a = show.state.next(&doc);
            assert!(matches!(a, ShowAction::Slide(1)));
            assert!(show.apply(a, &doc, 7.0));
            assert_eq!((show.state.step, show.state.playing), (0, true), "{start:?} on arrival");
        }
        // On Click still waits for the click.
        let doc = deck_with_first_animation(AnimStart::OnClick);
        let mut show = Show::new(&doc, 1, false, false);
        show.after_enter(&doc, 5.0);
        assert_eq!((show.state.step, show.state.playing), (0, false));
        assert!(matches!(show.state.next(&doc), ShowAction::PlayStep(0)));
    }

    #[test]
    fn zoom_stays_on_the_slide_and_the_grid_hits_a_thumbnail() {
        let uv = zoom_uv(0.5, 0.5, 2.0);
        assert!((uv.min.x - 0.25).abs() < 1e-4 && (uv.min.y - 0.25).abs() < 1e-4);
        assert!((uv.width() - 0.5).abs() < 1e-4);
        let corner = zoom_uv(0.0, 0.0, 2.0);
        assert_eq!(corner.min, pos2(0.0, 0.0));
        assert!(corner.max.x <= 1.0 && corner.max.y <= 1.0);
        let full = Rect::from_min_size(Pos2::ZERO, vec2(1728.0, 1084.0));
        let cells = see_all_cells(full, 7, 16.0 / 9.0);
        assert_eq!(cells.len(), 7);
        for (i, a) in cells.iter().enumerate() {
            assert!(full.contains_rect(*a), "cell {i} leaves the window");
            for b in cells.iter().skip(i + 1) {
                let overlap = a.intersect(*b);
                assert!(overlap.width() < 1.0 || overlap.height() < 1.0, "thumbnails overlap {a:?} {b:?}");
            }
        }
        let hit = see_all_hit(full, 7, 16.0 / 9.0, cells[3].center());
        assert_eq!(hit, Some(3));
        assert_eq!(see_all_hit(full, 7, 16.0 / 9.0, pos2(2.0, 2.0)), None);
        let srect = Rect::from_min_size(pos2(10.0, 20.0), vec2(200.0, 100.0));
        assert_eq!(norm_on_slide(srect, unit_uv(), pos2(110.0, 70.0)), Some((0.5, 0.5)));
        let zoomed = zoom_uv(0.5, 0.5, 2.0);
        let back = norm_on_slide(srect, zoomed, pos2(110.0, 70.0)).expect("on the slide");
        assert!((back.0 - 0.5).abs() < 1e-3 && (back.1 - 0.5).abs() < 1e-3);
    }

    #[test]
    fn escape_only_closes_the_grid_of_its_own_window() {
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &json!({})).unwrap();
        let doc = s.active().unwrap().doc.clone();
        let mut show = Show::new(&doc, 0, false, true);
        show.presenter_grid = true;
        assert!(!show.back_out(false), "audience Esc leaves the show, the Presenter View grid stays");
        assert!(show.presenter_grid);
        assert!(show.back_out(true));
        assert!(!show.presenter_grid);
        show.grid = true;
        assert!(!show.back_out(true), "Presenter View Esc does not close the audience grid");
        assert!(show.grid);
        assert!(show.back_out(false));
        assert!(!show.grid);
    }

    #[test]
    fn escape_leaves_the_grid_then_the_zoom_and_blank_toggles() {
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &json!({})).unwrap();
        let doc = s.active().unwrap().doc.clone();
        let mut show = Show::new(&doc, 0, false, true);
        show.grid = true;
        show.zoom = Some(Zoom::at(0.2, 0.3));
        assert!(show.back_out(false));
        assert!(!show.grid);
        assert!(show.zoom.is_some(), "closing the grid leaves the zoom");
        assert!(show.back_out(false));
        assert!(show.zoom.is_none());
        assert!(!show.back_out(false));
        show.zoom_key();
        assert!(show.zoom_arm);
        show.zoom_key();
        assert!(!show.zoom_arm);
        toggle_blank(&mut show, Color32::BLACK);
        assert_eq!(show.blank_name(), Some("black"));
        toggle_blank(&mut show, Color32::WHITE);
        assert_eq!(show.blank_name(), Some("white"));
        toggle_blank(&mut show, Color32::WHITE);
        assert_eq!(show.blank_name(), None);
        show.zoom = Some(Zoom::at(0.4, 0.6));
        assert!(show.apply(ShowAction::Slide(1), &doc, 1.0));
        assert!(show.zoom.is_none(), "a new slide is shown whole");
    }

    /// #1128: the presenter drew the notes as plain text, so bullets, numbers and bold were lost
    /// although the notes pane shows them; the notes stay selectable.
    #[test]
    fn presenter_notes_keep_bullets_numbers_and_bold() {
        use deckcraft_model::text::{Bullet, ParaProps, Paragraph, Run, RunProps, TextBody};
        let point = |text: &str, bullet: Bullet| Paragraph {
            level: 1,
            props: ParaProps { bullet: Some(bullet), ..Default::default() },
            runs: vec![Run::new(text)],
            ..Default::default()
        };
        let num = || Bullet::AutoNum { scheme: "arabicPeriod".into(), start_at: 1 };
        let notes = TextBody {
            paragraphs: vec![
                Paragraph { runs: vec![Run::with("Opening", RunProps { bold: Some(true), ..Default::default() })], ..Default::default() },
                point("Welcome", Bullet::Char { char: "•".into() }),
                point("Agenda", num()),
                point("Demo", num()),
            ],
            ..Default::default()
        };
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let mut sense = None;
        let mut out = ctx.run_ui(Default::default(), |ui| sense = Some(presenter_notes(ui, &notes, 24.0).sense));
        out.textures_delta.clear();
        assert!(sense.unwrap().senses_drag(), "the notes can be selected, as the plain label could");
        let mut texts = vec![];
        let mut stack: Vec<egui::Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
        while let Some(shape) = stack.pop() {
            match shape {
                egui::Shape::Vec(v) => stack.extend(v),
                egui::Shape::Text(t) => texts.push(t.galley),
                _ => {}
            }
        }
        let painted: Vec<&str> = texts.iter().map(|g| g.job.text.as_str()).collect();
        for marker in ["•", "1.", "2."] {
            assert!(painted.contains(&marker), "{marker} is drawn: {painted:?}");
        }
        let notes_galley = texts.iter().find(|g| g.job.text.contains("Agenda")).expect("the notes are drawn");
        let font_of = |s: &str| {
            let b = notes_galley.job.text.find(s).unwrap();
            notes_galley.job.sections.iter().find(|x| (x.byte_range.start.0..x.byte_range.end.0).contains(&b)).unwrap().format.font_id.clone()
        };
        assert_eq!(font_of("Opening"), theme::bold(24.0), "bold runs stay bold at the pane's size");
        assert_eq!(font_of("Agenda"), theme::font(24.0));
    }

    #[test]
    fn presenter_timer_pauses_resets_and_notes_size_clamps() {
        let mut t = PresenterTimer::start(10.0);
        assert!((t.elapsed(12.5) - 2.5).abs() < 1e-9);
        t.toggle(15.0);
        assert!(t.paused());
        assert!((t.elapsed(100.0) - 5.0).abs() < 1e-9);
        t.reset(100.0);
        assert!(t.paused());
        assert_eq!(t.elapsed(200.0), 0.0);
        t.toggle(200.0);
        assert!(!t.paused());
        assert!((t.elapsed(203.0) - 3.0).abs() < 1e-9);
        t.reset(203.0);
        assert!(!t.paused());
        assert!((t.elapsed(204.0) - 1.0).abs() < 1e-9);
        let bad = PresenterTimer::start(f64::NAN);
        assert_eq!(bad.elapsed(f64::INFINITY), 0.0);
        assert_eq!(format_elapsed(3661.4), "01:01:01");
        assert_eq!(format_elapsed(-1.0), "00:00:00");
        assert_eq!(format_elapsed(f64::NAN), "00:00:00");
        let clock = wall_clock();
        assert!(clock == "--:--" || (clock.len() == 5 && clock.as_bytes().get(2) == Some(&b':')));

        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &json!({})).unwrap();
        let doc = s.active().unwrap().doc.clone();
        let mut show = Show::new(&doc, 0, false, true);
        assert_eq!(show.notes_px, NOTES_DEFAULT);
        show.nudge_notes(NOTES_STEP);
        assert_eq!(show.notes_px, 22.0);
        show.nudge_notes(1000.0);
        assert_eq!(show.notes_px, NOTES_MAX);
        show.nudge_notes(-1000.0);
        assert_eq!(show.notes_px, NOTES_MIN);
        show.nudge_notes(f32::NAN);
        assert_eq!(show.notes_px, NOTES_MIN);
        let v = show.showing_value(1, 16.0 / 9.0, Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0)), 0.0);
        assert_eq!(v["presenterPaused"], json!(false));
        assert_eq!(v["notesPx"], json!(NOTES_MIN));
    }

    #[test]
    fn a_typed_slide_number_jumps_and_then_expires() {
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &json!({})).unwrap();
        let doc = s.active().unwrap().doc.clone();
        let mut show = Show::new(&doc, 0, false, false);
        show.push_slide_number('1', 10.0);
        show.push_slide_number('2', 10.2);
        assert_eq!(show.goto, "12");
        assert_eq!(show.take_goto(), Some(11));
        assert!(show.goto.is_empty());
        show.push_slide_number('0', 1.0);
        assert_eq!(show.take_goto(), None, "slide 0 is not a slide");
        show.push_slide_number('4', 5.0);
        show.expire_goto(7.0);
        assert_eq!(show.goto, "4");
        show.expire_goto(8.1);
        assert!(show.goto.is_empty());
        show.push_slide_number('9', 0.0);
        assert!(show.clear_goto());
        assert!(!show.clear_goto());
        for _ in 0..6 {
            show.push_slide_number('9', 1.0);
        }
        assert_eq!(show.goto.len(), GOTO_DIGITS);
        show.push_slide_number('a', 1.0);
        assert!(show.goto.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn browsed_by_an_individual_stays_in_a_window() {
        assert!(!show_fills_screen(false, "browsed"));
        assert!(!show_fills_screen(true, "speaker"));
        assert!(show_fills_screen(false, "speaker"));
        assert!(show_fills_screen(false, "kiosk"));
    }

    #[test]
    fn the_next_preview_fits_its_column_whatever_the_slide_shape() {
        let right = Rect::from_min_size(pos2(700.0, 100.0), vec2(400.0, 600.0));
        let mut s = deckcraft_engine::Session::with_new();
        for (w, h) in [(72, 4032), (4032, 72), (960, 540)] {
            s.execute("design.slideSize", &json!({"w": w, "h": h})).unwrap();
            let doc = s.active().unwrap().doc.clone();
            let r = next_rect(right, &doc);
            assert!(right.contains_rect(r), "{w}x{h}: {r:?}");
            let aspect = r.height() / r.width();
            assert!((aspect - h as f32 / w as f32).abs() / (h as f32 / w as f32) < 0.01, "{w}x{h}: {r:?}");
        }
        // 16:9 still takes the column's full width.
        let doc = s.active().unwrap().doc.clone();
        assert_eq!(next_rect(right, &doc).width(), 400.0);
    }

    #[test]
    fn back_leaves_a_slide_whose_first_animation_is_with_previous() {
        // #844: Back on such a slide undid its first step, and the next frame played that step
        // again by itself, so Back never reached the slide before.
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("slide.new", &json!({})).unwrap();
        let id = session.active().unwrap().doc.slides[1].shapes[0].id;
        session
            .execute("animation.add", &json!({"effect": "fade", "class": "entrance", "ids": [id.0], "start": "withPrevious", "duration": 500}))
            .unwrap();
        let app = SlideApp::new(session, crate::Services::default());
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(1000.0, 700.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        harness.run_steps(3);
        harness.state_mut().start_show(0, false);
        harness.run_steps(2);
        harness.key_press(egui::Key::ArrowRight);
        // The first step plays by itself and ends (0.5 s; a frame is 0.25 s).
        harness.run_steps(8);
        let st = &harness.state().show.as_ref().unwrap().state;
        assert_eq!((st.slide, st.step, st.playing), (1, 1, false), "on slide 2, fully built");
        for _ in 0..5 {
            harness.key_press(egui::Key::ArrowLeft);
            harness.run_steps(2);
        }
        let st = &harness.state().show.as_ref().unwrap().state;
        assert_eq!(st.slide, 0, "five Backs leave slide 2 (step {}, playing {})", st.step, st.playing);
    }
}
