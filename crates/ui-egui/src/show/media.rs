use super::render::fit;
use super::*;

/// The media-host voice for slide transition sounds (no shape has this id).
pub(crate) const TRANSITION_SOUND: ShapeId = ShapeId(u32::MAX);

/// Start the media of a newly shown slide: media from the previous slide stops unless it plays
/// across slides; media set to start automatically plays. Everything stops at the end.
pub(super) fn show_media(app: &mut SlideApp, show: &mut Show, doc_uid: u64, doc: &Presentation) {
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
pub(super) fn run_media_cues(app: &mut SlideApp, show: &mut Show, doc_uid: u64, doc: &Presentation) {
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
