//! Video export (File › Export › Create a Video): the deck as an MP4 or MOV. Each slide plays
//! its transition, then its animations on the slide-show timeline (click steps one after another,
//! a pause before each), then stays up for its timing. Frames come from the slide renderer and are
//! encoded with DeckCraft's own H.264 encoder ([`deckcraft_h264::Encoder`]: intra-coded changes,
//! skipped unchanged macroblocks); the sound of the slides' audio and video clips (narration is
//! recorded as such clips) and of transitions is mixed and encoded as AAC ([`deckcraft_aac`]), or
//! kept as PCM in a MOV. Video clips play their pictures (decoded by [`deckcraft_media`]) while
//! they play. Everything is muxed by [`deckcraft_isobmff::Mp4Writer`]; no external tool is needed.

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_anim::{Sequence, ShowState, Timeline};
use deckcraft_model::anim::AnimClass;
use deckcraft_model::{MediaId, Presentation, ShapeId, ShapeKind};
use deckcraft_render::{ParaState, ShapeState};

/// The most frames one export writes (two hours at 30 fps), against runaway timings.
pub const MAX_FRAMES: u64 = 2 * 60 * 60 * 30;
/// Audio sample rate of the exported track.
pub const AUDIO_RATE: u32 = 48_000;

/// How the sound is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioFormat {
    Aac,
    /// 16-bit PCM (MOV only).
    Pcm,
    None,
}

/// Export options, as `file.export` takes them.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoOpts {
    /// Frame height in pixels (PowerPoint's 480p / 720p / 1080p / 4K); the width follows the
    /// slide's aspect ratio. Both are rounded to even numbers, as H.264 needs.
    pub height: u32,
    pub fps: u32,
    /// How long a slide stays up after its animations when it has no "advance after" timing.
    pub seconds_per_slide: f64,
    /// The pause before each click step of a slide's animations.
    pub seconds_per_click: f64,
    pub include_hidden: bool,
    pub audio: AudioFormat,
}

impl Default for VideoOpts {
    fn default() -> Self {
        VideoOpts { height: 1080, fps: 30, seconds_per_slide: 5.0, seconds_per_click: 1.0, include_hidden: false, audio: AudioFormat::Aac }
    }
}

/// One step of the timeline: a transition frame at progress `t`; a frame of slide `index`'s
/// animations at main step `step`, `t` seconds in; the slide still before main step `step` for
/// `frames` frames; or the slide with every animation done, held for `frames` frames.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Transition { index: usize, t: f64 },
    Anim { index: usize, step: usize, t: f64 },
    Still { index: usize, step: usize, frames: u64 },
    Hold { index: usize, frames: u64 },
}

impl Step {
    pub fn index(&self) -> usize {
        match self {
            Step::Transition { index, .. } | Step::Anim { index, .. } | Step::Still { index, .. } | Step::Hold { index, .. } => *index,
        }
    }
    fn frames(&self) -> u64 {
        match self {
            Step::Still { frames, .. } | Step::Hold { frames, .. } => *frames,
            _ => 1,
        }
    }
}

/// A sound that starts at `start` seconds into the video and stops by `until`.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioCue {
    pub media: MediaId,
    pub start: f64,
    pub until: f64,
    pub trim_start: f64,
    pub trim_end: f64,
    pub volume: f64,
    pub fade_in: f64,
    pub fade_out: f64,
    pub looped: bool,
    /// The video clip whose pictures play with this cue: (slide index, shape).
    pub picture: Option<(usize, ShapeId)>,
    /// Back to the poster frame when the clip ends.
    pub rewind: bool,
    /// The clip's length (0: unknown, play until the pictures run out).
    pub length: f64,
}

/// The video's frames and sounds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub cues: Vec<AudioCue>,
    pub frames: u64,
}

/// The frame size for `opts` on this deck's slide size: even, at least 2×2.
pub fn frame_size(pres: &Presentation, opts: &VideoOpts) -> (u32, u32) {
    let h = opts.height.clamp(2, 4320) & !1;
    let aspect = pres.slide_size.width / pres.slide_size.height.max(1.0);
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect.clamp(0.1, 10.0) } else { 16.0 / 9.0 };
    let w = ((h as f64 * aspect).round() as u32).clamp(2, 8192) & !1;
    (w, h)
}

/// Seconds as frames at `fps`, at least one.
fn frames_for(seconds: f64, fps: u32) -> u64 {
    let s = if seconds.is_finite() { seconds.clamp(0.0, 3600.0) } else { 0.0 };
    ((s * fps as f64).round() as u64).max(1)
}

fn secs(ms: u32) -> f64 {
    f64::from(ms) / 1000.0
}

/// The clip a cue plays, from a media shape.
fn clip_cue(c: &deckcraft_model::MediaClip, index: usize, shape: ShapeId, start: f64, until: f64) -> AudioCue {
    AudioCue {
        media: c.media,
        start,
        until,
        trim_start: secs(c.trim_start_ms),
        trim_end: secs(c.trim_end_ms),
        volume: if c.volume.is_finite() { c.volume.clamp(0.0, 1.0) } else { 1.0 },
        fade_in: secs(c.fade_in_ms),
        fade_out: secs(c.fade_out_ms),
        looped: c.loop_play,
        picture: c.video.then_some((index, shape)),
        rewind: c.rewind,
        length: secs(c.duration_ms),
    }
}

/// Pushes steps while counting frames against [`MAX_FRAMES`].
struct Layout {
    steps: Vec<Step>,
    frames: u64,
    fps: u32,
}

impl Layout {
    /// Add `step`; `false` once the cap is reached.
    fn push(&mut self, step: Step) -> bool {
        let room = MAX_FRAMES.saturating_sub(self.frames);
        if room == 0 {
            return false;
        }
        let step = match step {
            Step::Still { index, step, frames } => Step::Still { index, step, frames: frames.min(room) },
            Step::Hold { index, frames } => Step::Hold { index, frames: frames.min(room) },
            s => s,
        };
        self.frames = self.frames.saturating_add(step.frames());
        self.steps.push(step);
        true
    }
    fn now(&self) -> f64 {
        self.frames as f64 / f64::from(self.fps)
    }
}

/// The timeline: for each slide shown, its transition's frames (none for "none", a cut or a
/// zero duration), its main animation sequence (each click step after a `seconds_per_click`
/// pause; steps that start by themselves follow on), then the slide held for its "advance
/// after" time (counted from the slide's start, as recorded timings are), else
/// `seconds_per_slide`. Sounds: transition sounds, clips that play automatically or from a
/// media effect, until their end or the slide's (the deck's when they play across slides).
/// The total is capped at [`MAX_FRAMES`].
pub fn plan(pres: &Presentation, opts: &VideoOpts) -> Plan {
    let fps = opts.fps.clamp(1, 60);
    let mut lay = Layout { steps: vec![], frames: 0, fps };
    let mut cues: Vec<(AudioCue, bool)> = vec![]; // (cue, ends with its slide)
    let animate = !pres.show.without_animation;
    'slides: for (index, slide) in pres.slides.iter().enumerate() {
        if slide.hidden && !opts.include_hidden {
            continue;
        }
        let slide_start = lay.now();
        if let Some(tr) = slide.transition.as_ref() {
            if let Some(m) = tr.sound {
                cues.push((
                    AudioCue {
                        media: m,
                        start: slide_start,
                        until: f64::MAX,
                        trim_start: 0.0,
                        trim_end: 0.0,
                        volume: 1.0,
                        fade_in: 0.0,
                        fade_out: 0.0,
                        looped: false,
                        picture: None,
                        rewind: false,
                        length: 0.0,
                    },
                    false,
                ));
            }
            if !matches!(tr.kind.as_str(), "none" | "cut" | "") && tr.duration_ms > 0 {
                let n = frames_for(secs(tr.duration_ms), fps);
                for k in 0..n {
                    if !lay.push(Step::Transition { index, t: k as f64 / n as f64 }) {
                        break 'slides;
                    }
                }
            }
        }
        let content_start = lay.now();
        let first_cue = cues.len();
        let mut started: Vec<ShapeId> = vec![];
        for sh in &slide.shapes {
            if let ShapeKind::Media(c) = &sh.kind
                && c.autoplay
            {
                cues.push((clip_cue(c, index, sh.id, content_start, f64::MAX), !c.play_across_slides));
                started.push(sh.id);
            }
        }
        let tl = if animate { ShowState::timeline(pres, index) } else { Timeline::default() };
        let effects = tl.effects();
        for step in 0..tl.steps() {
            if !tl.step_is_auto(step) && !lay.push(Step::Still { index, step, frames: frames_for(opts.seconds_per_click, fps) }) {
                break 'slides;
            }
            let step_start = lay.now();
            for e in effects.iter().filter(|e| e.sequence == Sequence::Main && e.step == step && e.class == AnimClass::Media) {
                if started.contains(&e.shape) {
                    continue;
                }
                if let Some(ShapeKind::Media(c)) = slide.shape(e.shape).map(|s| &s.kind) {
                    cues.push((clip_cue(c, index, e.shape, step_start + e.start.max(0.0), f64::MAX), !c.play_across_slides));
                    started.push(e.shape);
                }
            }
            let dur = tl.step_duration(step);
            if dur > 0.0 {
                let n = frames_for(dur, fps);
                for k in 0..n {
                    if !lay.push(Step::Anim { index, step, t: k as f64 * dur / n as f64 }) {
                        break 'slides;
                    }
                }
            }
        }
        let hold = match slide.transition.as_ref().and_then(|t| t.advance_after_ms) {
            Some(ms) => (secs(ms) - (lay.now() - slide_start)).max(0.5),
            None => opts.seconds_per_slide,
        };
        if !lay.push(Step::Hold { index, frames: frames_for(hold, fps) }) {
            break;
        }
        let end = lay.now();
        for (c, ends) in cues.iter_mut().skip(first_cue) {
            if *ends {
                c.until = end;
            }
        }
    }
    let total = lay.now();
    let cues = cues.into_iter().map(|(mut c, _)| {
        c.until = c.until.min(total);
        c
    });
    Plan { steps: lay.steps, cues: cues.filter(|c| c.until > c.start).collect(), frames: lay.frames }
}

/// The steps of [`plan`].
pub fn timeline(pres: &Presentation, opts: &VideoOpts) -> Vec<Step> {
    plan(pres, opts).steps
}

/// An image fitted to exactly `w`×`h` RGBA: centred in a black frame (letterboxed when the
/// slide's shape differs from the frame's; the renderer's size can also differ by a pixel),
/// cropped where it overhangs.
fn fit(img: &deckcraft_render::Image, w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (w as usize).saturating_mul(h as usize).saturating_mul(4)];
    for px in out.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    let cw = img.width.min(w) as usize;
    let (ox, oy) = (((w - cw as u32) / 2) as usize, ((h - img.height.min(h)) / 2) as usize);
    for y in 0..img.height.min(h) as usize {
        let src = y.saturating_mul(img.width as usize).saturating_mul(4);
        let dst = (y + oy).saturating_mul(w as usize).saturating_add(ox).saturating_mul(4);
        if let (Some(s), Some(d)) = (img.pixels.get(src..src + cw * 4), out.get_mut(dst..dst + cw * 4)) {
            d.copy_from_slice(s);
        }
    }
    out
}

/// The renderer options for a `w`×`h` frame: the whole slide, as large as fits. The frame's
/// aspect is limited to 1:10–10:1 and the slide's isn't (Design › Slide Size allows 1:56), so
/// scaling to the frame height alone rendered a very wide slide far past the frame (cropped to
/// its left part) and, past the 16× cap, a very short one smaller than the frame.
fn render_opts<'a>(pres: &Presentation, (w, h): (u32, u32), state: Option<&'a dyn Fn(ShapeId) -> ShapeState>) -> deckcraft_render::RenderOpts<'a> {
    let scale = (w as f64 / pres.slide_size.width.max(1.0)).min(h as f64 / pres.slide_size.height.max(1.0));
    deckcraft_render::RenderOpts { scale: if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }, state, ..Default::default() }
}

/// A shape's state at main step `step`, `t` seconds in (as the slide show draws it).
fn state_for(tl: &Timeline, id: ShapeId, step: usize, t: f64) -> ShapeState {
    let a = tl.state(id, None, step, t);
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

/// Every frame of `steps`, in order, handed to `sink` (RGBA, `w`×`h`). A still or held slide
/// is rendered once. Stops at the first error `sink` returns. Video clips show their poster.
pub fn frames(pres: &Presentation, steps: &[Step], (w, h): (u32, u32), sink: &mut dyn FnMut(&[u8]) -> Result<(), String>) -> Result<u64, String> {
    let mut r = Renderer::new(pres, &[], 30, (w, h));
    let mut n: u64 = 0;
    for step in steps {
        n = n.saturating_add(r.step(step, sink)?);
    }
    Ok(n)
}

/// A video clip's pictures, decoded in order as the export reaches them.
struct ClipPictures {
    dec: deckcraft_media::VideoDecoder,
    cur: Option<deckcraft_media::Frame>,
    next: Option<deckcraft_media::Frame>,
    done: bool,
}

impl ClipPictures {
    /// The picture shown `t` seconds into the media (the first one before it starts).
    fn at(&mut self, t: f64) -> Option<&deckcraft_media::Frame> {
        const EPS: f64 = 1e-6;
        if self.cur.as_ref().is_some_and(|c| c.time > t + EPS) {
            self.dec.seek(t);
            self.cur = None;
            self.next = None;
            self.done = false;
        }
        loop {
            if self.next.is_none() && !self.done {
                match self.dec.next_frame() {
                    Ok(Some(f)) => self.next = Some(f),
                    // The end, or a broken stream: keep the last picture.
                    Ok(None) | Err(_) => self.done = true,
                }
            }
            match &self.next {
                Some(n) if n.time <= t + EPS || self.cur.is_none() => {
                    let late = n.time > t + EPS;
                    self.cur = self.next.take();
                    if late {
                        break;
                    }
                }
                _ => break,
            }
        }
        self.cur.as_ref()
    }
}

/// Renders steps in order, knowing where on the video's clock each frame falls so playing
/// video clips show the right picture.
struct Renderer<'p> {
    pres: &'p Presentation,
    cues: Vec<AudioCue>,
    fps: u32,
    size: (u32, u32),
    /// Frames rendered so far: the clock.
    at: u64,
    timelines: HashMap<usize, Arc<Timeline>>,
    pictures: HashMap<MediaId, Option<ClipPictures>>,
    warnings: Vec<String>,
}

impl<'p> Renderer<'p> {
    fn new(pres: &'p Presentation, cues: &[AudioCue], fps: u32, size: (u32, u32)) -> Self {
        let cues = cues.iter().filter(|c| c.picture.is_some()).cloned().collect();
        Renderer { pres, cues, fps: fps.max(1), size, at: 0, timelines: HashMap::new(), pictures: HashMap::new(), warnings: vec![] }
    }

    /// Where in its media each video clip of slide `index` is at `t` seconds of the video.
    fn clip_times(&self, index: usize, t: f64) -> Vec<(ShapeId, MediaId, f64)> {
        let mut out = vec![];
        for c in &self.cues {
            let Some((i, shape)) = c.picture else { continue };
            if i != index || t < c.start || t >= c.until {
                continue;
            }
            let local = t - c.start;
            let span = (c.length - c.trim_start - c.trim_end).max(0.0);
            let pos = if c.length <= 0.0 {
                local
            } else if c.looped && span > 0.0 {
                local % span
            } else if local >= span && c.rewind {
                continue;
            } else {
                local.min(span)
            };
            out.push((shape, c.media, c.trim_start + pos));
        }
        out
    }

    /// Whether any clip of slide `index` plays during `n` frames from the clock.
    fn plays_during(&self, index: usize, n: u64) -> bool {
        let fps = f64::from(self.fps);
        let (t0, t1) = (self.at as f64 / fps, self.at.saturating_add(n) as f64 / fps);
        self.cues.iter().any(|c| c.picture.is_some_and(|p| p.0 == index) && c.start < t1 && c.until > t0)
    }

    fn pictures_for(&mut self, media: MediaId) -> Option<&mut ClipPictures> {
        let pres = self.pres;
        let warnings = &mut self.warnings;
        self.pictures
            .entry(media)
            .or_insert_with(|| {
                let m = pres.media(media)?;
                match deckcraft_media::VideoDecoder::open(m.data.clone()) {
                    Ok(dec) => Some(ClipPictures { dec, cur: None, next: None, done: false }),
                    Err(e) => {
                        warnings.push(format!("{}: the video shows its poster frame ({e})", m.name));
                        None
                    }
                }
            })
            .as_mut()
    }

    /// Render one step's frames into `sink`.
    fn step(&mut self, step: &Step, sink: &mut dyn FnMut(&[u8]) -> Result<(), String>) -> Result<u64, String> {
        let pres = self.pres;
        let animate = !pres.show.without_animation;
        let index = step.index();
        let tl = self
            .timelines
            .entry(index)
            .or_insert_with(|| Arc::new(if animate { ShowState::timeline(pres, index) } else { Timeline::default() }))
            .clone();
        let (st, t) = match step {
            Step::Transition { .. } => (0, 0.0),
            Step::Anim { step, t, .. } => (*step, *t),
            Step::Still { step, .. } => (*step, 0.0),
            Step::Hold { .. } => (tl.steps(), 0.0),
        };
        // During a transition only the incoming slide's shapes take its starting state.
        let incoming = pres.slides.get(index).cloned();
        let f = move |id: ShapeId| {
            if incoming.as_ref().is_some_and(|s| s.shape(id).is_some()) { state_for(&tl, id, st, t) } else { ShapeState::default() }
        };
        let total = step.frames();
        // A still slide is drawn once, unless a video clip on it is playing.
        let each = total > 1 && self.plays_during(index, total);
        let mut px = vec![];
        for k in 0..total {
            if k == 0 || each {
                let now = self.at as f64 / f64::from(self.fps);
                let times = self.clip_times(index, now);
                let mut held: Vec<(ShapeId, deckcraft_media::Frame)> = vec![];
                for (shape, media, ct) in times {
                    if let Some(p) = self.pictures_for(media).and_then(|p| p.at(ct)) {
                        held.push((shape, p.clone()));
                    }
                }
                let vf: Vec<(ShapeId, deckcraft_render::VideoFrame)> =
                    held.iter().map(|(s, f)| (*s, deckcraft_render::VideoFrame { width: f.width, height: f.height, rgba: &f.rgba })).collect();
                let mut opts = render_opts(pres, self.size, Some(&f));
                opts.video_frames = &vf;
                let img = match step {
                    Step::Transition { index, t } => deckcraft_render::render_transition(pres, *index, *t, &opts),
                    _ => deckcraft_render::render_slide(pres, index, &opts),
                };
                px = fit(&img, self.size.0, self.size.1);
            }
            sink(&px)?;
            self.at = self.at.saturating_add(1);
        }
        Ok(total)
    }
}

/// Decoded clips at [`AUDIO_RATE`], interleaved stereo, by media id (`None`: undecodable).
fn decode_clips(pres: &Presentation, cues: &[AudioCue], warnings: &mut Vec<String>) -> HashMap<MediaId, Option<Arc<Vec<f32>>>> {
    let mut out = HashMap::new();
    for c in cues {
        if out.contains_key(&c.media) {
            continue;
        }
        let pcm = pres.media(c.media).filter(|m| !m.data.is_empty()).and_then(|m| match deckcraft_media::audio::decode(&m.data) {
            Ok(p) => Some(p),
            Err(e) => {
                warnings.push(format!("{}: no sound in the video ({e})", m.name));
                None
            }
        });
        let stereo = pcm.map(|p| {
            let p = deckcraft_media::audio::resample(&p, AUDIO_RATE);
            let ch = usize::from(p.channels.max(1));
            let mut s = Vec::with_capacity(p.frames().saturating_mul(2));
            for f in 0..p.frames() {
                let l = p.samples.get(f * ch).copied().unwrap_or(0.0);
                let r = if ch > 1 { p.samples.get(f * ch + 1).copied().unwrap_or(0.0) } else { l };
                s.push(l);
                s.push(r);
            }
            Arc::new(s)
        });
        out.insert(c.media, stereo);
    }
    out
}

/// Mix every cue into the stereo block starting at sample frame `at` (`out`: interleaved).
fn mix_block(cues: &[AudioCue], clips: &HashMap<MediaId, Option<Arc<Vec<f32>>>>, at: u64, out: &mut [f32]) {
    out.iter_mut().for_each(|s| *s = 0.0);
    let rate = f64::from(AUDIO_RATE);
    let len = (out.len() / 2) as u64;
    for c in cues {
        let Some(Some(clip)) = clips.get(&c.media) else { continue };
        let frames = (clip.len() / 2) as u64;
        let first = (c.trim_start * rate) as u64;
        let last = frames.saturating_sub((c.trim_end * rate) as u64);
        if last <= first {
            continue;
        }
        let (start, until) = ((c.start * rate) as u64, (c.until * rate) as u64);
        if until <= at || start >= at + len {
            continue;
        }
        let span = (last - first) as f64 / rate;
        for k in 0..len {
            let g = at + k;
            if g < start || g >= until {
                continue;
            }
            let local = g - start;
            if !c.looped && first + local >= last {
                break;
            }
            let pos = first + if c.looped { local % (last - first) } else { local };
            let tl = local as f64 / rate;
            let mut gain = c.volume;
            if c.fade_in > 0.0 && tl < c.fade_in {
                gain *= tl / c.fade_in;
            }
            let left = if c.looped { f64::MAX } else { span - tl };
            if c.fade_out > 0.0 && left < c.fade_out {
                gain *= (left / c.fade_out).max(0.0);
            }
            let (i, o) = (pos as usize * 2, k as usize * 2);
            if let (Some(l), Some(r)) = (clip.get(i), clip.get(i + 1)) {
                out[o] += l * gain as f32;
                out[o + 1] += r * gain as f32;
            }
        }
    }
    for s in out.iter_mut() {
        *s = s.clamp(-1.0, 1.0);
    }
}

/// A key frame at least this often (seconds), so players can seek.
const KEY_EVERY_S: u32 = 2;

/// The encoded file.
pub struct Encoded {
    pub bytes: Vec<u8>,
    pub frames: u64,
    pub size: (u32, u32),
    /// Seconds of sound (0: no audio track).
    pub audio_seconds: f64,
    pub warnings: Vec<String>,
}

/// Encode the deck as `mp4` or `mov`. The file is assembled in memory.
pub fn encode(pres: &Presentation, format: &str, opts: &VideoOpts) -> Result<Encoded, String> {
    encode_with(pres, format, opts, &mut |_, _| true)
}

/// What [`encode_with`] fails with when its progress callback asks it to stop.
pub const CANCELLED: &str = "the video export was cancelled";

/// [`encode`], calling `progress(frames done, frames in all)` after each frame; when it returns
/// `false` the export stops with [`CANCELLED`].
pub fn encode_with(pres: &Presentation, format: &str, opts: &VideoOpts, progress: &mut dyn FnMut(u64, u64) -> bool) -> Result<Encoded, String> {
    use deckcraft_isobmff::{AvcConfig, Brand, CodecConfig, FourCc, Mp4Writer, PcmConfig, SampleEntry, TrackConfig, WriteSample, WriterOptions};
    if opts.audio == AudioFormat::Pcm && format != "mov" {
        return Err("PCM sound needs a .mov file; use AAC for .mp4".into());
    }
    let plan = plan(pres, opts);
    if plan.steps.is_empty() {
        return Err("there are no slides to put in the video".into());
    }
    let (w, h) = frame_size(pres, opts);
    let fps = opts.fps.clamp(1, 60);
    let mut enc = deckcraft_h264::Encoder::new(deckcraft_h264::EncoderConfig { width: w, height: h, fps, qp: 24 }).map_err(|e| e.to_string())?;
    let avc = AvcConfig::new(vec![enc.sps()], vec![enc.pps()], 4);
    let entry = SampleEntry::video(FourCc::new(b"avc1"), CodecConfig::Avc(avc), w.min(u32::from(u16::MAX)) as u16, h.min(u32::from(u16::MAX)) as u16);
    let brand = if format == "mov" { Brand::Mov } else { Brand::Mp4 };
    let mut mux = Mp4Writer::new(std::io::Cursor::new(Vec::new()), WriterOptions::new(brand)).map_err(|e| e.to_string())?;
    let track = mux.add_track(TrackConfig::new(entry, fps)).map_err(|e| e.to_string())?;
    let key_every = u64::from(fps * KEY_EVERY_S);
    let mut since_key: u64 = 0;
    let mut total: u64 = 0;
    let mut done: u64 = 0;
    let mut renderer = Renderer::new(pres, &plan.cues, fps, (w, h));
    for (i, step) in plan.steps.iter().enumerate() {
        // A key frame starts each slide (at its transition), so the file seeks by slide.
        let mut start = i.checked_sub(1).and_then(|p| plan.steps.get(p)).is_none_or(|p| p.index() != step.index());
        let all = plan.frames;
        let mut sink = |px: &[u8]| -> Result<(), String> {
            let pic = enc.encode_rgba(px, start || since_key >= key_every);
            start = false;
            since_key = if pic.key { 1 } else { since_key.saturating_add(1) };
            let data = pic.length_prefixed();
            mux.write_sample(track, WriteSample { data: &data, duration: 1, composition_offset: 0, is_sync: pic.key }).map_err(|e| e.to_string())?;
            done = done.saturating_add(1);
            if progress(done, all) { Ok(()) } else { Err(CANCELLED.to_string()) }
        };
        total = total.saturating_add(renderer.step(step, &mut sink)?);
    }
    // Sound.
    let mut warnings = std::mem::take(&mut renderer.warnings);
    let mut audio_seconds = 0.0;
    if opts.audio != AudioFormat::None && !plan.cues.is_empty() {
        let clips = decode_clips(pres, &plan.cues, &mut warnings);
        if clips.values().any(Option::is_some) {
            let samples = total.saturating_mul(u64::from(AUDIO_RATE)) / u64::from(fps);
            audio_seconds = samples as f64 / f64::from(AUDIO_RATE);
            const BLOCK: usize = deckcraft_aac::FRAME_LEN;
            let mut buf = vec![0f32; BLOCK * 2];
            match opts.audio {
                AudioFormat::Pcm => {
                    let pcm = PcmConfig { bits: 16, float: false, big_endian: false, signed: true, channels: 2, sample_rate: f64::from(AUDIO_RATE) };
                    let at = mux.add_track(TrackConfig::new(SampleEntry::pcm(pcm), AUDIO_RATE)).map_err(|e| e.to_string())?;
                    let mut pos = 0u64;
                    while pos < samples {
                        let n = (samples - pos).min(BLOCK as u64) as usize;
                        mix_block(&plan.cues, &clips, pos, &mut buf[..n * 2]);
                        let data: Vec<u8> = buf[..n * 2].iter().flat_map(|s| ((s * 32767.0).round() as i16).to_le_bytes()).collect();
                        mux.write_sample(at, WriteSample { data: &data, duration: n as u32, composition_offset: 0, is_sync: true })
                            .map_err(|e| e.to_string())?;
                        pos += n as u64;
                    }
                }
                _ => {
                    let mut aac =
                        deckcraft_aac::Encoder::new(deckcraft_aac::EncoderConfig::cbr(AUDIO_RATE, 2, 160_000)).map_err(|e| e.to_string())?;
                    let mut cfg = TrackConfig::new(SampleEntry::aac(aac.audio_specific_config(), 2, AUDIO_RATE), AUDIO_RATE);
                    cfg.media_start = Some(i64::from(aac.priming_samples()));
                    let at = mux.add_track(cfg).map_err(|e| e.to_string())?;
                    let write = |units: Vec<Vec<u8>>, mux: &mut Mp4Writer<std::io::Cursor<Vec<u8>>>| -> Result<(), String> {
                        for u in units {
                            mux.write_sample(at, WriteSample { data: &u, duration: BLOCK as u32, composition_offset: 0, is_sync: true })
                                .map_err(|e| e.to_string())?;
                        }
                        Ok(())
                    };
                    let mut pos = 0u64;
                    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
                    while pos < samples {
                        let n = (samples - pos).min(BLOCK as u64) as usize;
                        mix_block(&plan.cues, &clips, pos, &mut buf[..n * 2]);
                        l.clear();
                        r.clear();
                        for k in 0..n {
                            l.push(buf[k * 2]);
                            r.push(buf[k * 2 + 1]);
                        }
                        write(aac.encode(&[&l, &r]), &mut mux)?;
                        pos += n as u64;
                    }
                    write(aac.flush(), &mut mux)?;
                }
            }
        }
    }
    let bytes = mux.finish_faststart().map_err(|e| e.to_string())?.into_inner();
    Ok(Encoded { bytes, frames: total, size: (w, h), audio_seconds, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::anim::Transition;

    fn deck(n: usize) -> Presentation {
        let mut p = deckcraft_model::defaults::new_presentation(None);
        while p.slides.len() < n {
            let s = p.slides[0].clone();
            p.slides.push(s);
        }
        p.slides.truncate(n);
        p
    }

    #[test]
    fn timeline_holds_slides_and_plays_transitions() {
        let mut p = deck(3);
        std::sync::Arc::make_mut(&mut p.slides[1]).transition =
            Some(Transition { kind: "fade".into(), duration_ms: 500, advance_after_ms: Some(2000), ..Default::default() });
        std::sync::Arc::make_mut(&mut p.slides[2]).hidden = true;
        let o = VideoOpts { fps: 10, seconds_per_slide: 3.0, ..Default::default() };
        let t = timeline(&p, &o);
        assert_eq!(t.first(), Some(&Step::Hold { index: 0, frames: 30 }));
        assert_eq!(t.iter().filter(|s| matches!(s, Step::Transition { index: 1, .. })).count(), 5);
        // "Advance after" counts from the slide's start, its transition included.
        assert_eq!(t.last(), Some(&Step::Hold { index: 1, frames: 15 }), "the hidden slide is left out");
        let with_hidden = timeline(&p, &VideoOpts { include_hidden: true, ..o });
        assert_eq!(with_hidden.last(), Some(&Step::Hold { index: 2, frames: 30 }));
    }

    #[test]
    fn timeline_is_capped_and_sizes_are_even() {
        let mut p = deck(2);
        for s in &mut p.slides {
            std::sync::Arc::make_mut(s).transition = Some(Transition { advance_after_ms: Some(u32::MAX), ..Default::default() });
        }
        let total: u64 = timeline(&p, &VideoOpts { fps: 60, ..Default::default() }).iter().map(Step::frames).sum();
        assert!(total <= MAX_FRAMES);
        let (w, h) = frame_size(&p, &VideoOpts { height: 721, ..Default::default() });
        assert_eq!((w % 2, h), (0, 720));
    }

    #[test]
    fn frames_are_exactly_the_frame_size() {
        let p = deck(1);
        let size = frame_size(&p, &VideoOpts { height: 90, ..Default::default() });
        let mut seen = vec![];
        let n = frames(&p, &[Step::Hold { index: 0, frames: 2 }], size, &mut |px| {
            seen.push(px.len());
            Ok(())
        });
        assert_eq!(n, Ok(2));
        assert!(seen.iter().all(|l| *l == size.0 as usize * size.1 as usize * 4));
    }

    /// Slides outside the frame's 1:10–10:1 shape are drawn whole, letterboxed in black, not
    /// cropped to their left part (the frame scale used to follow the height alone).
    #[test]
    fn extreme_slide_shapes_are_letterboxed_whole() {
        for (sw, sh) in [(4032.0, 72.0), (72.0, 4032.0)] {
            let mut p = deck(1);
            p.slide_size = deckcraft_model::Size::new(sw, sh);
            let (w, h) = frame_size(&p, &VideoOpts { height: 480, ..Default::default() });
            let mut frame = vec![];
            frames(&p, &[Step::Hold { index: 0, frames: 1 }], (w, h), &mut |px| {
                frame = px.to_vec();
                Ok(())
            })
            .unwrap();
            let at = |x: u32, y: u32| frame[((y * w + x) * 4) as usize..][..3].to_vec();
            let white = |c: Vec<u8>| c.iter().all(|v| *v > 200);
            let black = |c: Vec<u8>| c.iter().all(|v| *v < 30);
            // The slide's centre and both of its ends along its long side are in the frame.
            assert!(white(at(w / 2, h / 2)), "{sw}x{sh}: centre");
            if sw > sh {
                assert!(white(at(1, h / 2)) && white(at(w - 2, h / 2)), "{sw}x{sh}: ends");
                assert!(black(at(w / 2, 1)) && black(at(w / 2, h - 2)), "{sw}x{sh}: bars above and below");
            } else {
                assert!(white(at(w / 2, 1)) && white(at(w / 2, h - 2)), "{sw}x{sh}: ends");
                assert!(black(at(0, h / 2)) && black(at(w - 1, h / 2)), "{sw}x{sh}: bars at the sides");
            }
        }
    }

    #[test]
    fn encodes_a_playable_mp4() {
        let mut p = deck(2);
        std::sync::Arc::make_mut(&mut p.slides[1]).transition = Some(Transition { kind: "fade".into(), duration_ms: 200, ..Default::default() });
        let o = VideoOpts { height: 72, fps: 10, seconds_per_slide: 0.5, ..Default::default() };
        let v = encode(&p, "mp4", &o).unwrap();
        let (bytes, n, (w, h)) = (v.bytes, v.frames, v.size);
        assert_eq!(n, 5 + 2 + 5);
        let f = deckcraft_isobmff::open(bytes.as_slice()).unwrap();
        let t = &f.tracks[0];
        assert_eq!(t.samples.len(), n as usize);
        // Key frames start each slide (at its transition).
        let keys: Vec<usize> = t.samples.iter().enumerate().filter(|(_, s)| s.is_sync).map(|(i, _)| i).collect();
        assert_eq!(keys, vec![0, 5]);
        let mut dec = deckcraft_h264::Decoder::new();
        // Rebuild an Annex B stream from the length-prefixed samples and the avcC parameter sets.
        let Some(deckcraft_isobmff::CodecConfig::Avc(avc)) = t.entries.first().map(|e| &e.codec) else { panic!("not AVC") };
        let mut stream = vec![];
        for ps in avc.sps.iter().chain(&avc.pps) {
            stream.extend([0, 0, 0, 1]);
            stream.extend(ps);
        }
        for i in 0..t.samples.len() {
            let data = f.read_sample(bytes.as_slice(), 0, i).unwrap();
            let mut k = 0;
            while k + 4 <= data.len() {
                let len = u32::from_be_bytes([data[k], data[k + 1], data[k + 2], data[k + 3]]) as usize;
                stream.extend([0, 0, 0, 1]);
                stream.extend(&data[k + 4..k + 4 + len]);
                k += 4 + len;
            }
        }
        let mut pics = dec.decode(&stream, 0).unwrap();
        pics.extend(dec.flush());
        assert_eq!(pics.len(), n as usize);
        assert_eq!((pics[0].width, pics[0].height), (w, h));
    }

    /// A mono 16-bit WAV of a sine tone.
    fn wav(rate: u32, secs: f64, hz: f64) -> Vec<u8> {
        let n = (f64::from(rate) * secs) as u32;
        let mut b = vec![];
        b.extend(b"RIFF");
        b.extend((36 + n * 2).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(rate.to_le_bytes());
        b.extend((rate * 2).to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend((n * 2).to_le_bytes());
        for k in 0..n {
            let v = ((k as f64 / f64::from(rate) * hz * std::f64::consts::TAU).sin() * 12000.0) as i16;
            b.extend(v.to_le_bytes());
        }
        b
    }

    /// A slide with a text box flying in on click and a tone that plays by itself.
    fn animated_deck() -> Presentation {
        use serde_json::json;
        let mut s = crate::Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        s.execute("insert.textBox", &json!({"rect": [100, 100, 400, 120], "text": "Hello"})).unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "start": "onClick", "duration": 1000})).unwrap();
        let data = crate::cmd::base64_encode(&wav(22_050, 1.0, 440.0));
        s.execute("insert.audio", &json!({"data": data, "name": "tone.wav"})).unwrap();
        s.execute("media.options", &json!({"autoplay": true})).unwrap();
        let mut p = (*s.doc().unwrap().doc).clone();
        p.slides.retain(|sl| sl.shapes.iter().any(|sh| matches!(sh.kind, ShapeKind::Media(_))));
        p
    }

    #[test]
    fn animations_play_on_the_show_timeline() {
        let p = animated_deck();
        let o = VideoOpts { fps: 10, seconds_per_slide: 1.0, seconds_per_click: 0.5, height: 90, ..Default::default() };
        let plan = plan(&p, &o);
        assert_eq!(plan.steps.first(), Some(&Step::Still { index: 0, step: 0, frames: 5 }), "{:?}", plan.steps);
        assert_eq!(plan.steps.iter().filter(|s| matches!(s, Step::Anim { .. })).count(), 10);
        assert_eq!(plan.steps.last(), Some(&Step::Hold { index: 0, frames: 10 }));
        // The tone plays from the slide's start, for the slide.
        assert_eq!(plan.cues.len(), 1);
        assert_eq!((plan.cues[0].start, plan.cues[0].until), (0.0, 2.5));
        // Before the click the text is hidden; halfway through the fade it shows faintly; at
        // the end fully.
        let size = frame_size(&p, &o);
        let ink = |step: Step| {
            let mut dark = 0usize;
            frames(&p, &[step], size, &mut |px| {
                dark = px.as_chunks::<4>().0.iter().filter(|q| q[0] < 200).count();
                Ok(())
            })
            .unwrap();
            dark
        };
        let (before, mid, after) = (
            ink(Step::Still { index: 0, step: 0, frames: 1 }),
            ink(Step::Anim { index: 0, step: 0, t: 0.5 }),
            ink(Step::Hold { index: 0, frames: 1 }),
        );
        assert!(before < after && after > 0, "{before} {mid} {after}");
    }

    #[test]
    fn sound_is_mixed_and_encoded_as_aac() {
        let p = animated_deck();
        let o = VideoOpts { fps: 10, seconds_per_slide: 1.0, seconds_per_click: 0.5, height: 72, ..Default::default() };
        let v = encode(&p, "mp4", &o).unwrap();
        assert!((v.audio_seconds - 2.5).abs() < 0.01, "{}", v.audio_seconds);
        let f = deckcraft_isobmff::open(v.bytes.as_slice()).unwrap();
        assert_eq!(f.tracks.len(), 2);
        let a = &f.tracks[1];
        let Some(deckcraft_isobmff::CodecConfig::Aac(cfg)) = a.entries.first().map(|e| &e.codec) else { panic!("not AAC") };
        let mut dec = deckcraft_aac::Decoder::new(&cfg.asc).unwrap();
        let mut energy = vec![];
        for i in 0..a.samples.len() {
            let pcm = dec.decode(&f.read_sample(v.bytes.as_slice(), 1, i).unwrap()).unwrap();
            energy.push(pcm[0].iter().map(|x| x * x).sum::<f32>() / pcm[0].len().max(1) as f32);
        }
        // The one-second tone, then silence.
        let loud = energy.iter().filter(|e| **e > 0.01).count();
        let quiet = energy.iter().filter(|e| **e < 1e-5).count();
        assert!(loud >= 40 && quiet >= 50, "loud {loud} quiet {quiet} of {}", energy.len());
        // PCM in a MOV.
        let v = encode(&p, "mov", &VideoOpts { audio: AudioFormat::Pcm, ..o.clone() }).unwrap();
        let f = deckcraft_isobmff::open(v.bytes.as_slice()).unwrap();
        assert!(matches!(f.tracks[1].entries.first().map(|e| &e.codec), Some(deckcraft_isobmff::CodecConfig::Pcm(_))));
        assert!(encode(&p, "mp4", &VideoOpts { audio: AudioFormat::Pcm, ..o }).is_err());
    }

    /// A 64×64 H.264 MP4 at 10 fps: one second red, then one second blue.
    fn red_then_blue() -> Vec<u8> {
        use deckcraft_isobmff::{AvcConfig, Brand, CodecConfig, FourCc, Mp4Writer, SampleEntry, TrackConfig, WriteSample, WriterOptions};
        let mut enc = deckcraft_h264::Encoder::new(deckcraft_h264::EncoderConfig { width: 64, height: 64, fps: 10, qp: 20 }).unwrap();
        let avc = AvcConfig::new(vec![enc.sps()], vec![enc.pps()], 4);
        let entry = SampleEntry::video(FourCc::new(b"avc1"), CodecConfig::Avc(avc), 64, 64);
        let mut mux = Mp4Writer::new(std::io::Cursor::new(Vec::new()), WriterOptions::new(Brand::Mp4)).unwrap();
        let t = mux.add_track(TrackConfig::new(entry, 10)).unwrap();
        for k in 0..20 {
            let c = if k < 10 { [230u8, 20, 20, 255] } else { [20, 20, 230, 255] };
            let px: Vec<u8> = std::iter::repeat_n(c, 64 * 64).flatten().collect();
            let pic = enc.encode_rgba(&px, k == 0 || k == 10);
            mux.write_sample(t, WriteSample { data: &pic.length_prefixed(), duration: 1, composition_offset: 0, is_sync: pic.key }).unwrap();
        }
        mux.finish().unwrap().into_inner()
    }

    #[test]
    fn video_clips_play_their_pictures() {
        use serde_json::json;
        let mut s = crate::Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        let data = crate::cmd::base64_encode(&red_then_blue());
        s.execute("insert.video", &json!({"data": data, "name": "clip.mp4", "rect": [0, 0, 960, 540]})).unwrap();
        s.execute("media.options", &json!({"autoplay": true})).unwrap();
        let mut p = (*s.doc().unwrap().doc).clone();
        p.slides.retain(|sl| sl.shapes.iter().any(|sh| matches!(sh.kind, ShapeKind::Media(_))));
        for sl in &mut p.slides {
            Arc::make_mut(sl).transition = None;
        }
        let o = VideoOpts { fps: 10, seconds_per_slide: 3.0, height: 64, audio: AudioFormat::None, ..Default::default() };
        let v = encode(&p, "mp4", &o).unwrap();
        assert_eq!(v.frames, 30);
        let mut dec = deckcraft_media::VideoDecoder::open(Arc::new(v.bytes)).unwrap();
        let mut centre = vec![];
        while let Some(f) = dec.next_frame().unwrap() {
            let i = ((f.height / 2 * f.width + f.width / 2) * 4) as usize;
            centre.push([f.rgba[i], f.rgba[i + 1], f.rgba[i + 2]]);
        }
        let red = |c: &[u8; 3]| c[0] > 150 && c[2] < 100;
        let blue = |c: &[u8; 3]| c[2] > 150 && c[0] < 100;
        // Red for the first second, blue for the second, then the last picture held.
        assert!(centre[..9].iter().all(red), "{centre:?}");
        assert!(centre[11..].iter().all(blue), "{centre:?}");
    }

    #[test]
    fn progress_counts_frames_and_can_cancel() {
        let p = animated_deck();
        let o = VideoOpts { fps: 10, seconds_per_slide: 1.0, seconds_per_click: 0.5, height: 64, audio: AudioFormat::None, ..Default::default() };
        let mut seen = vec![];
        let v = encode_with(&p, "mp4", &o, &mut |d, n| {
            seen.push((d, n));
            true
        })
        .unwrap();
        assert_eq!(seen.len() as u64, v.frames);
        assert_eq!(seen.last(), Some(&(v.frames, v.frames)));
        let r = encode_with(&p, "mp4", &o, &mut |d, _| d < 3);
        assert_eq!(r.err().as_deref(), Some(CANCELLED));
    }
}
