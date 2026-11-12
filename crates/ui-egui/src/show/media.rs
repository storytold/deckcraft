use super::render::fit;
use super::{Click, HitShape, Hyperlink, PlacedShape, Presentation, Rect, ShapeId, Show, SlideApp, Ui, pos2, vec2};
use deckcraft_anim::AnimState;

/// The media-host voice for slide transition sounds (no shape has this id).
pub(crate) const TRANSITION_SOUND: ShapeId = ShapeId(u32::MAX);

/// Start the media of a newly shown slide: media from the previous slide stops unless it plays
/// across slides; media set to start automatically plays. Everything stops at the end.
pub(super) fn show_media(app: &mut SlideApp, show: &mut Show, doc_uid: u64, doc: &Presentation, now: f64) {
    let cur = if show.ended { None } else { Some(show.state.slide) };
    if show.media_slide == cur {
        return;
    }
    show.media_slide = cur;
    // A pending start belongs to the slide it was scheduled on.
    show.pending_media.clear();
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
            if !super::play_on_enter(&clip, show.without_narration(doc)) || keep.contains(&id) {
                continue;
            }
            match clip.start_delay_ms.unwrap_or(0) {
                // A delayed automatic clip starts that long after the slide, as in PowerPoint.
                delay if delay > 0 => show.pending_media.push((id, now + f64::from(delay) / 1000.0)),
                _ => app.media.play(doc_uid, doc, id, &clip, None, crate::media::Owner::Show),
            }
        }
    }
}

/// Start the automatic clips whose slide-start delay has come. Entries for a slide the show has
/// left are dropped, so a pending start never fires late on another slide.
pub(super) fn run_pending_media(app: &mut SlideApp, show: &mut Show, doc_uid: u64, doc: &Presentation, now: f64) {
    let current = Some(show.state.slide);
    show.pending_media.retain(|(id, due)| {
        if *due > now {
            return true;
        }
        if show.media_slide != current {
            return false;
        }
        let Some(clip) = crate::media::find_clip(doc, *id, current) else { return false };
        app.media.play(doc_uid, doc, *id, &clip, None, crate::media::Owner::Show);
        false
    });
}

/// Drop a clip's pending automatic start: the presenter took it over by hand, so the deadline
/// must not fire the clip back up (or resume a manual pause) later.
pub(super) fn cancel_pending_media(show: &mut Show, id: ShapeId) {
    show.pending_media.retain(|(pending, _)| *pending != id);
}

/// Carry out the media effects the show's steps set off: Play starts from the beginning unless
/// the clip is already playing, Pause toggles, Stop stops.
pub(super) fn run_media_cues(app: &mut SlideApp, show: &mut Show, doc_uid: u64, doc: &Presentation) {
    for (id, effect) in std::mem::take(&mut show.media_cues) {
        let Some(clip) = crate::media::find_clip(doc, id, Some(show.state.slide)) else { continue };
        // The presenter took over the clip: a start scheduled for it must not fire later.
        cancel_pending_media(show, id);
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
pub(super) fn shown_slide_rect(ui: &Ui, show: &Show, doc: &Presentation) -> Rect {
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

pub(super) fn slide_media_bars(srect: Rect, doc: &Presentation, slide: usize) -> Vec<(ShapeId, deckcraft_model::MediaClip, Rect, Rect)> {
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
pub(super) fn media_bar_target(
    ctx: &egui::Context,
    doc: &Presentation,
    show: &Show,
    srect: Rect,
) -> Option<(ShapeId, deckcraft_model::MediaClip, Rect)> {
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
pub(super) fn pointer_on_media_bar(ctx: &egui::Context, ui: &Ui, doc: &Presentation, show: &Show, now: f64) -> bool {
    if !doc.show.show_media_controls || !show.pointer_awake(now) || show.pen || show.ended || show.blank.is_some() {
        return false;
    }
    let Some(p) = ctx.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())) else { return false };
    let srect = shown_slide_rect(ui, show, doc);
    slide_media_bars(srect, doc, show.state.slide).iter().any(|(_, _, _, bar)| bar.contains(p))
}

/// The media shape under the pointer on the shown slide.
pub(super) fn media_hit(ctx: &egui::Context, ui: &Ui, show: &Show, doc: &Presentation) -> Option<(ShapeId, deckcraft_model::MediaClip)> {
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
pub(super) enum Pick {
    /// A hyperlink in text.
    Text(Hyperlink),
    /// A shape with an action or a triggered animation.
    Shape,
}

/// The slide's shapes at the pointer, for [`ShowState::click`].
#[derive(Default)]
pub(super) struct Under {
    shapes: Vec<HitShape>,
    /// Whether each of `shapes` is currently shown (an animation may have hidden it).
    visible: Vec<bool>,
    pub(super) pick: Option<Pick>,
}

/// Convert a slide-space point back into a shape's own coordinates, undoing its animation.
fn hit_point(point: (f64, f64), shapes: &[PlacedShape], states: &[AnimState], index: usize) -> Option<(f64, f64)> {
    let shape = shapes.get(index)?;
    inverse_transform_point(point, (shape.rect.center().x, shape.rect.center().y), *states.get(index)?)
}

fn inverse_transform_point(p: (f64, f64), center: (f64, f64), state: AnimState) -> Option<(f64, f64)> {
    if state.scale_x.abs() <= f64::EPSILON || state.scale_y.abs() <= f64::EPSILON {
        return None;
    }
    let (x, y) = (p.0 - center.0 - state.offset_x, p.1 - center.1 - state.offset_y);
    let angle = -state.rotate.to_radians();
    let (x, y) = (x * angle.cos() - y * angle.sin(), x * angle.sin() + y * angle.cos());
    Some((center.0 + x / state.scale_x, center.1 + y / state.scale_y))
}

impl Under {
    fn visible(&self, id: ShapeId) -> bool {
        self.shapes.iter().zip(&self.visible).find(|(h, _)| h.id == id).is_none_or(|(_, v)| *v)
    }
}

/// What is under the pointer on the shown slide: the slide's shapes with their animation state, the
/// topmost of them that does something when clicked, and a text hyperlink on it. Only shapes that
/// are shown can be clicked, and a shape without an action hides the ones beneath it.
pub(super) fn shapes_under_pointer(ctx: &egui::Context, ui: &Ui, show: &mut Show, doc: &Presentation, now: f64) -> Option<Under> {
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
    let (step, plays, stamps) = (show.state.step, show.state.trigger_plays(), show.state.trigger_stamps());
    let t = if show.state.playing { now - show.step_start } else { 0.0 };
    let states: Vec<AnimState> = shapes.iter().map(|s| tl.state_ordered(s.id, None, step, t, &plays, &stamps)).collect();
    let point_for = |i: usize| hit_point((x, y), shapes, &states, i);
    let hits: Vec<HitShape> = shapes
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let under_pointer = point_for(i).is_some_and(|(x, y)| s.rect.x0 <= x && x <= s.rect.x1 && s.rect.y0 <= y && y <= s.rect.y1);
            HitShape { id: s.id, parent: s.parent, is_group: s.is_group, link: s.link.as_ref().map(|l| l.action.clone()), under_pointer }
        })
        .collect();
    let visible: Vec<bool> = states.iter().map(|a| a.visible && a.opacity > 0.0).collect();
    let mut under = Under { shapes: hits, visible, pick: None };
    let shown = |id: ShapeId| under.visible(id);
    // A hyperlink in text applies when its shape is the one on top.
    let top = show.state.top(doc, tl, &under.shapes, &shown);
    let text = links.iter().rev().filter(|l| l.text).find(|l| {
        let Some(i) = shapes.iter().position(|s| s.id == l.shape) else { return false };
        let Some((x, y)) = point_for(i) else { return false };
        if !(l.rect.x0 <= x && x <= l.rect.x1 && l.rect.y0 <= y && y <= l.rect.y1) {
            return false;
        }
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
    pub(super) fn tick_triggers(&mut self, doc: &Presentation, now: f64) {
        self.timeline(doc);
        if let Some((_, tl)) = &self.timeline {
            self.state.tick(tl, now);
        }
    }

    /// A click on the slide: the shape under the pointer acts, or the background advances.
    pub(super) fn click_slide(&mut self, doc: &Presentation, under: &Under, now: f64) -> Click {
        self.timeline(doc);
        let last = self.last_viewed;
        match &self.timeline {
            Some((_, tl)) => self.state.click(doc, tl, &under.shapes, &|id| under.visible(id), now, last),
            None => Click::None,
        }
    }
}

/// A slide-space box on screen, the slide being drawn in `srect` at `k` pixels per point.
pub(super) fn on_screen(srect: Rect, k: f32, x: &deckcraft_geom::Xfrm) -> Rect {
    Rect::from_min_size(pos2(srect.min.x + x.x as f32 * k, srect.min.y + x.y as f32 * k), vec2(x.w as f32 * k, x.h as f32 * k))
}

/// The animated GIF whose play/pause button is under the pointer on the shown slide.
pub(super) fn gif_hit(ctx: &egui::Context, ui: &Ui, show: &Show, doc_uid: u64, doc: &Presentation, app: &mut SlideApp) -> Option<ShapeId> {
    let p = ctx.input(|i| i.pointer.interact_pos())?;
    let full = ui.max_rect();
    let avail = if show.reading { Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - 34.0)) } else { full };
    let srect = fit(avail, doc);
    let k = srect.width() / doc.slide_size.width.max(1.0) as f32;
    let gifs = app.media.gifs(doc_uid, doc, show.state.slide, ctx.input(|i| i.time), &app.session.gif_paused);
    gifs.iter().rev().find(|g| crate::media::gif_button_rect(on_screen(srect, k, &g.rect)).contains(p)).map(|g| g.id)
}

#[cfg(test)]
mod hit_tests {
    use super::*;

    fn shape(id: u32, parent: Option<usize>, rect: deckcraft_geom::Rect) -> PlacedShape {
        PlacedShape { id: ShapeId(id), parent, is_group: parent.is_none(), rect, link: None }
    }

    #[test]
    fn motion_path_moves_the_hit_box() {
        let shapes = [shape(1, None, deckcraft_geom::Rect::new(10.0, 10.0, 30.0, 30.0))];
        let states = [AnimState { offset_x: 50.0, scale_x: 2.0, ..Default::default() }];

        let moved = hit_point((65.0, 20.0), &shapes, &states, 0);
        let abandoned = hit_point((15.0, 20.0), &shapes, &states, 0);
        assert!(moved.is_some_and(|(x, y)| (10.0..=30.0).contains(&x) && (10.0..=30.0).contains(&y)));
        assert!(!abandoned.is_some_and(|(x, y)| (10.0..=30.0).contains(&x) && (10.0..=30.0).contains(&y)));
    }

    #[test]
    fn scale_and_rotation_move_the_hit_box() {
        let shapes = [shape(1, None, deckcraft_geom::Rect::new(10.0, 10.0, 30.0, 30.0))];
        let states = [AnimState { scale_x: 2.0, scale_y: 1.0, rotate: 90.0, ..Default::default() }];

        // The shape center is (20, 20); local (25, 25) scales and rotates to (15, 30).
        let transformed = hit_point((15.0, 30.0), &shapes, &states, 0);
        assert!(transformed.is_some_and(|(x, y)| (24.9..=25.1).contains(&x) && (24.9..=25.1).contains(&y)));
    }

    #[test]
    fn parent_animation_does_not_move_child_hit_box() {
        let shapes =
            [shape(1, None, deckcraft_geom::Rect::new(0.0, 0.0, 100.0, 100.0)), shape(2, Some(0), deckcraft_geom::Rect::new(10.0, 10.0, 30.0, 30.0))];
        let states = [AnimState { offset_x: 40.0, ..Default::default() }, AnimState::default()];

        let unchanged = hit_point((15.0, 20.0), &shapes, &states, 1);
        let displaced = hit_point((55.0, 20.0), &shapes, &states, 1);
        assert!(unchanged.is_some_and(|(x, y)| (10.0..=30.0).contains(&x) && (10.0..=30.0).contains(&y)));
        assert!(!displaced.is_some_and(|(x, y)| (10.0..=30.0).contains(&x) && (10.0..=30.0).contains(&y)));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use deckcraft_model::{MediaClip, ShapeKind};
    use serde_json::json;

    use super::*;
    use crate::SlideApp;

    /// A blank deck whose first slide holds one automatic audio clip starting `delay_ms` late.
    fn app_with_delayed_clip(delay_ms: Option<u32>) -> SlideApp {
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        s.edit(|doc, _| {
            let media = doc
                .add_media("tone.wav", "audio/wav", vec![0u8; 64])
                .ok_or_else(|| deckcraft_engine::EngineError::BadParams { cmd: "test".into(), msg: "no media".into() })?;
            let id = deckcraft_model::ShapeId(
                doc.alloc_id().ok_or_else(|| deckcraft_engine::EngineError::BadParams { cmd: "test".into(), msg: "no id".into() })?,
            );
            let shape = deckcraft_model::Shape {
                id,
                xfrm: Some(deckcraft_geom::Xfrm::new(0.0, 0.0, 40.0, 40.0)),
                kind: ShapeKind::Media(MediaClip { media, autoplay: true, duration_ms: 500, start_delay_ms: delay_ms, ..Default::default() }),
                ..Default::default()
            };
            std::sync::Arc::make_mut(&mut doc.slides[0]).shapes.push(shape);
            Ok(())
        })
        .unwrap();
        SlideApp::new(s, crate::Services::default())
    }

    /// A delayed automatic clip waits for its delay instead of starting with the slide.
    #[test]
    fn a_delayed_automatic_clip_waits_for_its_delay() {
        let mut app = app_with_delayed_clip(Some(2_000));
        let doc = app.session.active().unwrap().doc.clone();
        let uid = app.session.active().unwrap().uid;
        let mut show = Show::new(&doc, 0, false, false);
        show_media(&mut app, &mut show, uid, &doc, 10.0);
        assert_eq!(show.pending_media.len(), 1, "the clip is scheduled, not started");
        assert!((show.pending_media[0].1 - 12.0).abs() < 1e-9);
        // Before its time the start is kept; the slide it belongs to is still showing.
        run_pending_media(&mut app, &mut show, uid, &doc, 11.0);
        assert_eq!(show.pending_media.len(), 1);
    }

    /// The hidden logic tick must keep waking for a pending start, or a hidden window leaves the
    /// clip silent past its deadline.
    #[test]
    fn a_pending_start_keeps_hidden_playback_awake() {
        let mut app = app_with_delayed_clip(Some(2_000));
        let doc = app.session.active().unwrap().doc.clone();
        let uid = app.session.active().unwrap().uid;
        let mut show = Show::new(&doc, 0, false, false);
        show.started = true;
        show_media(&mut app, &mut show, uid, &doc, 10.0);
        assert!(show.hidden_time_driven(&doc), "the pending clip is time-driven");
        show.pending_media.clear();
        assert!(!show.hidden_time_driven(&doc), "nothing is left to wake for");
    }

    /// A clip with no delay is started by the slide itself, so nothing is scheduled.
    #[test]
    fn a_clip_without_a_delay_is_not_scheduled() {
        let mut app = app_with_delayed_clip(None);
        let doc = app.session.active().unwrap().doc.clone();
        let uid = app.session.active().unwrap().uid;
        let mut show = Show::new(&doc, 0, false, false);
        show_media(&mut app, &mut show, uid, &doc, 10.0);
        assert!(show.pending_media.is_empty());
    }
    /// A presenter who starts and pauses a delayed clip owns it: the pending automatic start
    /// must not resume it at the deadline (thread `show/media.rs:70`).
    #[test]
    fn a_manually_paused_clip_stays_paused_past_its_deadline() {
        let mut app = app_with_delayed_clip(Some(2_000));
        let doc = app.session.active().unwrap().doc.clone();
        let uid = app.session.active().unwrap().uid;
        let mut show = Show::new(&doc, 0, false, false);
        show_media(&mut app, &mut show, uid, &doc, 10.0);
        assert_eq!(show.pending_media.len(), 1, "the delayed start is scheduled");
        let id = show.pending_media[0].0;
        // The presenter takes over before the deadline: start, then pause.
        app.media.play(uid, &doc, id, &clip_of(&doc, id), None, crate::media::Owner::Show);
        super::cancel_pending_media(&mut show, id);
        app.media.toggle(uid, &doc, id, &clip_of(&doc, id), crate::media::Owner::Show);
        assert!(show.pending_media.is_empty(), "the manual take owns the clip now");
        // Past the deadline the automatic start must not fire the clip back up.
        run_pending_media(&mut app, &mut show, uid, &doc, 13.0);
        assert!(!app.media.is_playing(id), "manual pause holds past the deadline");
    }

    fn clip_of(doc: &Presentation, id: deckcraft_model::ShapeId) -> MediaClip {
        crate::media::find_clip(doc, id, Some(0)).expect("the delayed clip")
    }
}
