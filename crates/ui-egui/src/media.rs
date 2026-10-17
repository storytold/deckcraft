//! Media playback in the UI host: the [`deckcraft_media::Player`] (audio out + clocks), video
//! feeds and their textures, the editor's media control bar, and the engine's `media.*` requests.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use deckcraft_engine::{MediaStatus, Session};
use deckcraft_media::{AudioOut, ClipParams, Frame, PlayState, Player, VideoFeed, VoiceSpec, VoiceStatus};
use deckcraft_model::{MediaClip, Presentation, ShapeId, ShapeKind};
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, pos2, vec2};

use crate::icons::{self, Icon};

/// Who started a voice: the editor's control bar stops when you leave the slide, the slide show
/// manages its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Editor,
    Show,
}

struct Video {
    feed: Option<VideoFeed>,
    frame: Option<Arc<Frame>>,
    tex: Option<TextureHandle>,
}

/// A voice started for one shape in one presentation.
struct Owned {
    owner: Owner,
    clip: MediaClip,
    /// Address of the source bytes this voice was started with.
    source: usize,
}

/// A probed duration. `source` keeps that allocation alive so a later buffer cannot reuse the
/// address and inherit this length.
struct Probed {
    source: Arc<Vec<u8>>,
    duration: f64,
}

pub struct MediaHost {
    player: Player,
    /// Presentation the voices and GIF clocks belong to. `None` before the first document is
    /// bound, and after the last one closes.
    doc: Option<u64>,
    owners: HashMap<ShapeId, Owned>,
    videos: HashMap<ShapeId, Video>,
    /// Probed durations keyed by the source allocation (media imported without a length).
    durations: HashMap<usize, Probed>,
    /// Animated GIF clocks for `doc`: when each started and, while paused, the time it holds.
    gifs: HashMap<ShapeId, (f64, Option<f64>)>,
}

/// An animated GIF picture on a slide: its box, play time, frame and seconds to the next frame.
pub struct Gif {
    pub id: ShapeId,
    pub rect: deckcraft_geom::Xfrm,
    pub t: f64,
    pub frame: usize,
    pub next: f64,
    pub paused: bool,
}

impl Default for MediaHost {
    fn default() -> Self {
        MediaHost::new(None)
    }
}

fn key(id: ShapeId) -> u64 {
    id.0 as u64
}

impl MediaHost {
    pub fn new(out: Option<Box<dyn AudioOut>>) -> MediaHost {
        MediaHost {
            player: Player::new(out),
            doc: None,
            owners: HashMap::new(),
            videos: HashMap::new(),
            durations: HashMap::new(),
            gifs: HashMap::new(),
        }
    }

    /// Bind playback to the open presentation. `None` (nothing open) stops every voice. Switching
    /// to a different document stops the previous one's voices and GIF clocks, so a shape id that
    /// exists in both decks cannot keep the old clip.
    pub fn set_document(&mut self, uid: Option<u64>) {
        if self.doc == uid {
            return;
        }
        if uid.is_none() || self.doc.is_some() {
            self.stop_voices();
            self.gifs.clear();
        }
        self.doc = uid;
    }

    fn ensure_doc(&mut self, uid: u64) {
        self.set_document(Some(uid));
    }

    fn stop_voices(&mut self) {
        self.player.stop_all();
        self.owners.clear();
        self.videos.clear();
    }

    /// Length of `bytes` in seconds, remembered for this allocation. The map keeps the bytes, and
    /// stays within 16 entries.
    fn duration_of(&mut self, bytes: &Arc<Vec<u8>>) -> f64 {
        let media_key = Arc::as_ptr(bytes) as usize;
        if let Some(hit) = self.durations.get(&media_key) {
            return hit.duration;
        }
        if self.durations.len() >= 16 {
            let drop_key = self
                .durations
                .iter()
                .find(|(_, p)| Arc::strong_count(&p.source) <= 1)
                .map(|(k, _)| *k)
                .or_else(|| self.durations.keys().next().copied());
            if let Some(k) = drop_key {
                self.durations.remove(&k);
            }
        }
        let duration = deckcraft_media::probe(bytes).map(|i| i.duration()).unwrap_or(0.0);
        self.durations.insert(media_key, Probed { source: Arc::clone(bytes), duration });
        duration
    }

    /// The animated GIF pictures directly on slide `index` at `now`: each plays from when it was
    /// first shown; a paused one holds its time and resumes from it. Clocks belong to `doc_uid`
    /// and are dropped when the open presentation changes.
    pub fn gifs(&mut self, doc_uid: u64, doc: &Presentation, index: usize, now: f64, paused: &HashSet<ShapeId>) -> Vec<Gif> {
        self.ensure_doc(doc_uid);
        let Some(slide) = doc.slides.get(index) else { return vec![] };
        let mut out = vec![];
        for sh in &slide.shapes {
            let (ShapeKind::Picture { fill }, Some(rect)) = (&sh.kind, sh.xfrm) else { continue };
            let Some(item) = doc.media(fill.media).filter(|m| m.data.starts_with(b"GIF8")) else { continue };
            let p = paused.contains(&sh.id);
            let c = self.gifs.entry(sh.id).or_insert((now, None));
            if p && c.1.is_none() {
                c.1 = Some(now - c.0);
            } else if !p && let Some(held) = c.1.take() {
                c.0 = now - held;
            }
            let t = c.1.unwrap_or(now - c.0);
            if let Some((frame, next)) = deckcraft_render::gif_frame(&item.data, &fill.adjust, t) {
                out.push(Gif { id: sh.id, rect, t, frame, next, paused: p });
            }
        }
        out
    }

    /// Replace the audio output (the host injects cpal after start-up).
    pub fn set_output(&mut self, out: Option<Box<dyn AudioOut>>) {
        self.stop_voices();
        self.player = Player::new(out);
    }

    pub fn status(&self, id: ShapeId) -> Option<VoiceStatus> {
        self.player.status(key(id))
    }

    /// Playing or loading (not paused / ended).
    pub fn is_playing(&self, id: ShapeId) -> bool {
        self.status(id).is_some_and(|s| matches!(s.state, PlayState::Playing | PlayState::Loading))
    }

    pub fn owner(&self, id: ShapeId) -> Option<Owner> {
        self.owners.get(&id).map(|o| o.owner)
    }

    /// Shapes with a voice (playing, paused or ended).
    pub fn ids(&self) -> Vec<ShapeId> {
        self.owners.keys().copied().collect()
    }

    pub fn any_active(&self) -> bool {
        !self.owners.is_empty()
    }

    /// Start (or resume) shape `id` of presentation `doc_uid` from `from` seconds.
    pub fn play(&mut self, doc_uid: u64, doc: &Presentation, id: ShapeId, clip: &MediaClip, from: Option<f64>, owner: Owner) {
        self.ensure_doc(doc_uid);
        let Some(item) = doc.media(clip.media) else { return };
        let bytes = item.data.clone();
        let media_key = Arc::as_ptr(&bytes) as usize;
        let duration = if clip.duration_ms > 0 { clip.duration_ms as f64 / 1000.0 } else { self.duration_of(&bytes) };
        let spec = VoiceSpec {
            clip: ClipParams::from_ms(clip.trim_start_ms, clip.trim_end_ms, clip.fade_in_ms, clip.fade_out_ms, clip.volume),
            looping: clip.loop_play,
            duration,
            audio: clip.volume > 0.0,
        };
        let source_changed = self.owners.get(&id).is_some_and(|o| o.source != media_key);
        if clip.video && (source_changed || !self.videos.contains_key(&id)) {
            let feed = match VideoFeed::new(bytes.clone()) {
                Ok(f) => Some(f),
                Err(e) => {
                    log::info!("video {id}: {e}");
                    None
                }
            };
            self.videos.insert(id, Video { feed, frame: None, tex: None });
        } else if !clip.video {
            self.videos.remove(&id);
        }
        self.player.play(key(id), media_key as u64, &bytes, spec, from);
        self.owners.insert(id, Owned { owner, clip: clip.clone(), source: media_key });
    }

    pub fn pause(&mut self, id: ShapeId) {
        self.player.pause(key(id));
    }

    fn same_source(&self, doc: &Presentation, id: ShapeId, clip: &MediaClip) -> bool {
        doc.media(clip.media).is_some_and(|item| self.owners.get(&id).is_some_and(|o| o.source == Arc::as_ptr(&item.data) as usize))
    }

    pub fn toggle(&mut self, doc_uid: u64, doc: &Presentation, id: ShapeId, clip: &MediaClip, owner: Owner) {
        self.ensure_doc(doc_uid);
        if self.is_playing(id) && self.same_source(doc, id, clip) {
            self.pause(id);
        } else {
            self.play(doc_uid, doc, id, clip, None, owner);
        }
    }

    pub fn stop(&mut self, id: ShapeId) {
        self.player.stop(key(id));
        self.owners.remove(&id);
        self.videos.remove(&id);
    }

    pub fn stop_owned(&mut self, owner: Owner) {
        let ids: Vec<ShapeId> = self.owners.iter().filter(|(_, o)| o.owner == owner).map(|(id, _)| *id).collect();
        for id in ids {
            self.stop(id);
        }
    }

    /// Move to `t` seconds; a shape that isn't playing, or whose clip changed, is cued there, paused.
    pub fn seek(&mut self, doc_uid: u64, doc: &Presentation, id: ShapeId, clip: &MediaClip, t: f64, owner: Owner) {
        self.ensure_doc(doc_uid);
        if self.status(id).is_none() || !self.same_source(doc, id, clip) {
            self.play(doc_uid, doc, id, clip, Some(t), owner);
            self.pause(id);
        }
        self.player.seek(key(id), t);
    }

    /// The current video frame of shape `id`, once one has decoded.
    pub fn texture(&self, id: ShapeId) -> Option<&TextureHandle> {
        self.videos.get(&id).and_then(|v| v.tex.as_ref())
    }

    /// Per frame: run the clocks, upload due video frames, apply loop/rewind, and report state to
    /// the engine. Returns whether anything is playing (the caller keeps repainting).
    pub fn tick(&mut self, ctx: &egui::Context, session: &mut Session) -> bool {
        let now = ctx.input(|i| i.time);
        self.player.tick(now);
        let mut ended = vec![];
        for (id, owned) in &self.owners {
            if let Some(st) = self.player.status(key(*id))
                && st.state == PlayState::Ended
                && owned.clip.rewind
            {
                ended.push(*id);
            }
        }
        // Rewind after playing: back to the poster.
        for id in ended {
            self.stop(id);
        }
        for (id, v) in &mut self.videos {
            let Some(st) = self.player.status(key(*id)) else { continue };
            let Some(feed) = v.feed.as_mut() else { continue };
            let f = feed.frame_at(st.position);
            if let Some(f) = f
                && v.frame.as_ref().is_none_or(|old| !Arc::ptr_eq(old, &f))
            {
                let img = egui::ColorImage::from_rgba_unmultiplied([f.width as usize, f.height as usize], &f.rgba);
                match v.tex.as_mut() {
                    Some(t) => crate::textures::replace(ctx, t, img),
                    None => v.tex = Some(crate::textures::upload(ctx, format!("media-{id}"), img)),
                }
                v.frame = Some(f);
            }
        }
        session.media_status = self
            .owners
            .keys()
            .filter_map(|id| {
                self.player.status(key(*id)).map(|s| {
                    (
                        *id,
                        MediaStatus {
                            state: s.state.name().into(),
                            position_ms: (s.position.max(0.0) * 1000.0).round() as u64,
                            duration_ms: (s.duration.max(0.0) * 1000.0).round() as u64,
                            error: s.error,
                        },
                    )
                })
            })
            .collect();
        let playing = self.owners.keys().any(|id| self.is_playing(*id));
        if playing {
            ctx.request_repaint_after(std::time::Duration::from_millis(15));
        }
        playing
    }

    /// Carry out a `media.*` request from the engine for presentation `doc_uid`.
    pub fn request(&mut self, doc_uid: u64, doc: &Presentation, action: &str, id: ShapeId, ms: Option<u64>, owner: Owner) {
        self.ensure_doc(doc_uid);
        let Some(clip) = find_clip(doc, id) else { return };
        let t = ms.map(|v| v as f64 / 1000.0);
        match action {
            "play" => self.play(doc_uid, doc, id, &clip, t, owner),
            "pause" => self.pause(id),
            "toggle" => self.toggle(doc_uid, doc, id, &clip, owner),
            "stop" => self.stop(id),
            "seek" => self.seek(doc_uid, doc, id, &clip, t.unwrap_or(0.0), owner),
            _ => {}
        }
    }
}

/// The clip of media shape `id` anywhere in the deck.
pub fn find_clip(doc: &Presentation, id: ShapeId) -> Option<MediaClip> {
    fn walk(shapes: &[deckcraft_model::Shape], id: ShapeId) -> Option<MediaClip> {
        shapes.iter().find_map(|s| match &s.kind {
            ShapeKind::Media(m) if s.id == id => Some(m.clone()),
            ShapeKind::Group { children, .. } => walk(children, id),
            _ => None,
        })
    }
    doc.slides.iter().find_map(|s| walk(&s.shapes, id))
}

/// Media shapes directly on slide `index`: (id, box in slide points, clip).
pub fn slide_media(doc: &Presentation, index: usize) -> Vec<(ShapeId, deckcraft_geom::Xfrm, MediaClip)> {
    let Some(s) = doc.slides.get(index) else { return vec![] };
    s.shapes
        .iter()
        .filter_map(|sh| match (&sh.kind, sh.xfrm) {
            (ShapeKind::Media(m), Some(x)) => Some((sh.id, x, m.clone())),
            _ => None,
        })
        .collect()
}

/// "1:05.25" style time.
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0);
    format!("{:02}:{:05.2}", (s / 60.0) as u32, s % 60.0)
}

/// Draw the playing video frame of `id` into `rect` (letterboxed to the frame's aspect).
pub fn paint_frame(painter: &egui::Painter, host: &MediaHost, id: ShapeId, rect: Rect) -> bool {
    let Some(tex) = host.texture(id) else { return false };
    let [w, h] = tex.size();
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let k = (rect.width() / w).min(rect.height() / h);
    let r = Rect::from_center_size(rect.center(), vec2(w * k, h * k));
    painter.rect_filled(rect, CornerRadius::ZERO, Color32::BLACK);
    painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    true
}

/// Changes whenever a GIF shows another frame (part of the slide texture's cache key).
pub fn gif_key(gifs: &[Gif]) -> u64 {
    gifs.iter().fold(0, |h: u64, g| (h ^ (((g.id.0 as u64) << 32) | g.frame as u64)).wrapping_mul(1099511628211))
}

/// Come back when the next frame of a playing GIF is due.
pub fn gif_repaint(ctx: &egui::Context, gifs: &[Gif]) {
    if let Some(next) = gifs.iter().filter(|g| !g.paused).map(|g| g.next).reduce(f64::min) {
        ctx.request_repaint_after(std::time::Duration::from_millis((next * 1000.0).clamp(5.0, 1000.0) as u64));
    }
}

/// The play/pause button in the bottom-left corner of a GIF's box `r` (PowerPoint puts it there).
pub fn gif_button_rect(r: Rect) -> Rect {
    let s = 24.0f32.min(r.width() - 8.0).min(r.height() - 8.0).max(12.0);
    Rect::from_min_size(pos2(r.min.x + 6.0, r.max.y - 6.0 - s), vec2(s, s))
}

pub fn paint_gif_button(painter: &egui::Painter, b: Rect, paused: bool, hot: bool) {
    painter.circle_filled(b.center(), b.width() / 2.0, Color32::from_black_alpha(if hot { 210 } else { 150 }));
    let r = b.shrink(b.width() * 0.3);
    let white = Color32::from_gray(235);
    if paused {
        icons::paint(painter, r, Icon::Play, white, false);
    } else {
        let w = r.width() * 0.3;
        painter.rect_filled(Rect::from_min_size(r.min, vec2(w, r.height())), CornerRadius::ZERO, white);
        painter.rect_filled(Rect::from_min_size(pos2(r.max.x - w, r.min.y), vec2(w, r.height())), CornerRadius::ZERO, white);
    }
}

/// What the control bar asks for.
pub enum BarAction {
    Toggle,
    Seek(f64),
    Nudge(f64),
}

/// PowerPoint-style media controls: play/pause, ¼-second nudges, a seekable timeline and the time.
pub fn control_bar(ui: &mut Ui, rect: Rect, status: Option<&VoiceStatus>, clip: &MediaClip) -> Option<BarAction> {
    let painter = ui.painter_at(rect.expand(2.0));
    painter.rect_filled(rect, CornerRadius::same(4), Color32::from_rgba_unmultiplied(28, 28, 32, 230));
    let playing = status.is_some_and(|s| matches!(s.state, PlayState::Playing | PlayState::Loading));
    let duration = status.map(|s| s.duration).filter(|d| *d > 0.0).unwrap_or(clip.duration_ms as f64 / 1000.0);
    let pos = status.map(|s| s.position).unwrap_or(clip.trim_start_ms as f64 / 1000.0);
    let mut action = None;
    let h = rect.height();
    let mut x = rect.min.x + 4.0;
    let button = |ui: &mut Ui, x: &mut f32, id: &str, paint: &dyn Fn(&egui::Painter, Rect)| -> bool {
        let r = Rect::from_min_size(pos2(*x, rect.min.y + 2.0), vec2(h - 4.0, h - 4.0));
        *x += h - 2.0;
        let resp = ui.interact(r, ui.id().with(("media_bar", id)), Sense::click());
        if resp.hovered() {
            painter.rect_filled(r, CornerRadius::same(3), Color32::from_white_alpha(28));
        }
        paint(&painter, r.shrink(4.0));
        resp.clicked()
    };
    let white = Color32::from_gray(235);
    if button(ui, &mut x, "play", &|p, r| {
        if playing {
            let w = r.width() * 0.3;
            p.rect_filled(Rect::from_min_size(r.min, vec2(w, r.height())), CornerRadius::ZERO, white);
            p.rect_filled(Rect::from_min_size(pos2(r.max.x - w, r.min.y), vec2(w, r.height())), CornerRadius::ZERO, white);
        } else {
            icons::paint(p, r, Icon::Play, white, false);
        }
    }) {
        action = Some(BarAction::Toggle);
    }
    let tri = |p: &egui::Painter, r: Rect, left: bool| {
        let (a, b) = if left { (r.max.x, r.min.x) } else { (r.min.x, r.max.x) };
        let pts: Vec<Pos2> = vec![pos2(a, r.min.y + 2.0), pos2(b, r.center().y), pos2(a, r.max.y - 2.0)];
        p.add(egui::Shape::convex_polygon(pts, white, Stroke::NONE));
    };
    if button(ui, &mut x, "back", &|p, r| tri(p, r.shrink(2.0), true)) {
        action = Some(BarAction::Nudge(-0.25));
    }
    if button(ui, &mut x, "fwd", &|p, r| tri(p, r.shrink(2.0), false)) {
        action = Some(BarAction::Nudge(0.25));
    }
    // Time label on the right, timeline between.
    let label = clock(pos);
    let label_w = 64.0;
    painter.text(pos2(rect.max.x - 6.0, rect.center().y), Align2::RIGHT_CENTER, &label, egui::FontId::monospace(11.0), white);
    let track = Rect::from_min_max(pos2(x + 6.0, rect.center().y - 3.0), pos2(rect.max.x - label_w - 6.0, rect.center().y + 3.0));
    if track.width() > 10.0 {
        painter.rect_filled(track, CornerRadius::same(3), Color32::from_gray(90));
        let frac = if duration > 0.0 { (pos / duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let done = Rect::from_min_max(track.min, pos2(track.min.x + track.width() * frac, track.max.y));
        painter.rect_filled(done, CornerRadius::same(3), Color32::from_rgb(0x4C, 0x9A, 0xFF));
        // Trim marks.
        if duration > 0.0 {
            for t in [clip.trim_start_ms as f64 / 1000.0, duration - clip.trim_end_ms as f64 / 1000.0] {
                if t > 0.0 && t < duration {
                    let tx = track.min.x + track.width() * (t / duration) as f32;
                    painter.line_segment(
                        [pos2(tx, track.min.y - 3.0), pos2(tx, track.max.y + 3.0)],
                        Stroke::new(1.5, Color32::from_rgb(0xF2, 0xB1, 0x34)),
                    );
                }
            }
        }
        painter.circle_filled(pos2(done.max.x, track.center().y), 5.0, white);
        let hit = track.expand2(vec2(4.0, 8.0));
        let resp = ui.interact(hit, ui.id().with("media_bar_track"), Sense::click_and_drag());
        if (resp.clicked() || resp.dragged())
            && duration > 0.0
            && let Some(p) = resp.interact_pointer_pos()
        {
            let f = ((p.x - track.min.x) / track.width()).clamp(0.0, 1.0) as f64;
            action = Some(BarAction::Seek(f * duration));
        }
    }
    if let Some(err) = status.and_then(|s| s.error.as_ref()) {
        painter.text(
            pos2(rect.min.x, rect.max.y + 3.0),
            Align2::LEFT_TOP,
            crate::i18n::format("No sound: {err}", std::slice::from_ref(err)),
            egui::FontId::proportional(10.0),
            Color32::from_rgb(0xC0, 0x40, 0x40),
        );
    }
    action
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use deckcraft_geom::Xfrm;
    use deckcraft_model::style::PictureFill;
    use deckcraft_model::{MediaClip, Presentation, Shape, ShapeId, ShapeKind};

    use super::*;

    /// 16-bit mono WAV of a constant level, `secs` long at 8 kHz.
    fn wav_level(secs: f64, amp: f32) -> Vec<u8> {
        let rate = 8_000u32;
        let n = (rate as f64 * secs).round() as usize;
        let data: Vec<u8> = (0..n).flat_map(|_| ((amp * 32767.0) as i16).to_le_bytes()).collect();
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(&data);
        b
    }

    fn deck(shape: u32, bytes: Vec<u8>) -> (Presentation, ShapeId, MediaClip) {
        let mut doc = Presentation::default();
        let media = doc.add_media("clip.wav", "audio/wav", bytes).expect("media");
        let clip = MediaClip { media, volume: 1.0, ..Default::default() };
        let id = ShapeId(shape);
        let slide = Arc::make_mut(&mut doc.slides[0]);
        slide.shapes.push(Shape { id, xfrm: Some(Xfrm::new(0.0, 0.0, 120.0, 40.0)), kind: ShapeKind::Media(clip.clone()), ..Default::default() });
        (doc, id, clip)
    }

    /// First mixed sample of shape `id`, after its audio has decoded.
    fn level(host: &MediaHost, id: ShapeId) -> f32 {
        assert!(host.player.wait_loaded(key(id), std::time::Duration::from_secs(5)), "decode");
        let mut buf = vec![0.0f32; 4];
        host.player.render(&mut buf, 1, 8_000);
        buf[0]
    }

    #[test]
    fn playback_follows_the_document_and_the_clip() {
        let (loud_doc, id, loud) = deck(4000, wav_level(0.05, 0.5));
        let (quiet_doc, _, quiet) = deck(4000, wav_level(0.10, 0.25));
        let mut host = MediaHost::new(None);

        host.play(1, &loud_doc, id, &loud, None, Owner::Editor);
        let sample = level(&host, id);
        assert!((sample - 0.5).abs() < 0.02, "loud sample {sample}");
        let dur = host.status(id).map(|s| s.duration).unwrap_or(-1.0);
        assert!((dur - 0.05).abs() < 0.01, "loud duration {dur}");
        let loud_bytes = &loud_doc.media(loud.media).expect("media").data;
        let ptr = Arc::as_ptr(loud_bytes) as usize;
        assert!(host.durations.get(&ptr).is_some_and(|p| Arc::ptr_eq(&p.source, loud_bytes)));

        // Replacing the clip on the same shape plays the new bytes, not the voice already running.
        host.play(1, &quiet_doc, id, &quiet, None, Owner::Editor);
        let sample = level(&host, id);
        assert!((sample - 0.25).abs() < 0.02, "replaced sample {sample}");
        let dur = host.status(id).map(|s| s.duration).unwrap_or(-1.0);
        assert!((dur - 0.10).abs() < 0.01, "replaced duration {dur}");
        assert_eq!(host.player.keys(), vec![key(id)]);

        // A second deck reuses the shape id. Leaving the first deck stops its voice.
        host.set_document(Some(2));
        assert!(host.status(id).is_none(), "the previous document's voice is gone");
        host.play(2, &loud_doc, id, &loud, None, Owner::Editor);
        let sample = level(&host, id);
        assert!((sample - 0.5).abs() < 0.02, "second document sample {sample}");
        let dur = host.status(id).map(|s| s.duration).unwrap_or(-1.0);
        assert!((dur - 0.05).abs() < 0.01, "second document duration {dur}");

        host.set_document(None);
        assert!(host.ids().is_empty(), "closing the last document stops playback");
        assert!(host.status(id).is_none());
    }

    #[test]
    fn gif_clocks_do_not_carry_across_documents() {
        let gif = |shape: u32| {
            let mut doc = Presentation::default();
            let media = doc.add_media("a.gif", "image/gif", b"GIF89a-not-animated".to_vec()).expect("media");
            let id = ShapeId(shape);
            let slide = Arc::make_mut(&mut doc.slides[0]);
            slide.shapes.push(Shape {
                id,
                xfrm: Some(Xfrm::new(0.0, 0.0, 80.0, 80.0)),
                kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
                ..Default::default()
            });
            (doc, id)
        };
        let (a, id) = gif(7);
        let (b, _) = gif(7);
        let mut host = MediaHost::new(None);
        let none = HashSet::new();
        host.gifs(1, &a, 0, 10.0, &none);
        assert_eq!(host.gifs.get(&id).map(|c| c.0), Some(10.0));
        host.gifs(1, &a, 0, 12.0, &none);
        assert_eq!(host.gifs.get(&id).map(|c| c.0), Some(10.0), "the same document keeps its clock");
        host.gifs(2, &b, 0, 50.0, &none);
        assert_eq!(host.gifs.get(&id).map(|c| c.0), Some(50.0), "a new document starts its own clock");
        host.set_document(None);
        assert!(host.gifs.is_empty());
    }
}
