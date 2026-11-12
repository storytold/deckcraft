use super::keys::{handle_keys, keyboard};
use super::media::{
    Pick, Under, cancel_pending_media, gif_hit, media_bar_target, media_hit, on_screen, pointer_on_media_bar, run_media_cues, run_pending_media,
    shapes_under_pointer, show_media,
};
use super::presenter::{PresenterTimer, format_elapsed, presenter_window};
use super::render::{below_playing_video, fit, part_textures, quad_mesh, render_frame, render_overlay, render_threads, start_prefetch};
use super::zoom::{
    Zoom, grid_indices, norm_on_slide, paint_grid, see_all_hit, slide_aspect, slide_box, slide_to_screen, toggle_blank, tool_bar_rect, unit_uv,
    wheel_zoom_lines,
};
use super::{
    Align2, BackKey, Click, Color32, CornerRadius, InkMode, InkStroke, Key, Layer, LinkJump, Pos2, Rect, RenderOpts, Sense, ShapeId, ShapeState,
    Show, ShowAction, ShowState, SlideApp, Source, Stroke, TextureHandle, Ui, before_slide_change, json, laser_pos, pen_width_slide,
    pointer_is_laser, pos2, record, save_take, show_fills_screen, state_for, theme, vec2, viewport_visible,
};
use deckcraft_anim::{StepStamp, TriggerPlay};
use deckcraft_model::Presentation;

/// Per-frame values the show's draw steps share.
#[derive(Clone, Copy)]
struct Fr<'a> {
    ctx: &'a egui::Context,
    doc: &'a std::sync::Arc<Presentation>,
    doc_uid: u64,
    now: f64,
    full: Rect,
    srect: Rect,
    clicked: bool,
    secondary: bool,
    pointer: Option<Pos2>,
    /// The Record view's section rects; None in an ordinary show.
    layout: Option<record::Layout>,
}

/// Whether the show goes on, and the steps input asked for.
struct Out {
    keep: bool,
    actions: Vec<ShowAction>,
}

/// What is drawn on the current slide this frame.
struct Sl {
    idx: usize,
    step: usize,
    animating: bool,
    t: f64,
    plays: Vec<TriggerPlay>,
    stamps: Vec<StepStamp>,
    media: Vec<(ShapeId, deckcraft_geom::Xfrm, deckcraft_model::MediaClip)>,
    hidden: Vec<ShapeId>,
    times: Vec<(ShapeId, f64)>,
    gifs: Vec<crate::media::Gif>,
    key: Key,
    back_key: BackKey,
    size: (u32, u32),
    /// Recording preview: how many recorded strokes the clock has reached, and when the next one
    /// is due. The count is part of the frame key, so the frame is rendered once per revealed
    /// stroke rather than on every tick.
    ink_next: Option<u32>,
}

/// Draw and advance the active show, handling hidden frames, timing, input, transitions, and
/// presenter/reading presentation.
pub fn ui(app: &mut SlideApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let Some((doc_uid, doc)) = app.session.active().map(|d| (d.uid, d.doc.clone())) else {
        if let Some(mut show) = app.show.take() {
            // The show's own teardown saves the take when a document is still open; with none
            // (`file.close` won the race) hand over what was captured so the microphone stops.
            if show.mic_tried {
                let slide = show.state.slide;
                super::take_narration(app.services.mic.as_deref_mut(), &mut show, slide);
                app.stop_mic();
            }
        }
        if let Some(host) = app.services.presenter.as_mut() {
            host.close();
        }
        return;
    };
    let now = ctx.input(|i| i.time);
    let Some(mut show) = app.show.take() else { return };
    show.last_now = now;
    if !viewport_visible(&ctx) {
        // A forced hidden paint (for example a pending screenshot) draws the current frame;
        // the show clocks are advanced by logic, not by this draw, so it must not advance them a
        // second time. Keep drawing the last frame rather than returning a blank.
        paint_hidden(ui, &doc, &show);
        app.show = Some(show);
        return;
    }
    show.observe_visible_frame(now, web_time::Instant::now());
    begin_show(&ctx, &mut show, &doc, now, app.services.presenter.is_some());
    let mic_on = show.record_view.devices.mic.is_on();
    if show.record && !show.recording_preview && mic_on && show.record_view.status == record::RecordStatus::Recording && !show.mic_tried {
        show.mic_tried = true;
        match app.services.mic.as_mut() {
            Some(mic) => match mic.start() {
                Ok(()) => show.mic_ok = true,
                Err(e) => {
                    log::info!("record narration: {e}");
                    show.mic_ok = false;
                    // Say so, and keep the toolbar from claiming a capture that is not happening.
                    let device = &mut show.record_view.devices.mic;
                    device.failed = true;
                    device.status = super::record::devices::DeviceStatus::Unavailable;
                    app.set_status(format!("Microphone: {e}"));
                }
            },
            None => show.mic_ok = false,
        }
    }
    // `MicIn::start` can return before the device opens, and a device can be lost mid-take: both
    // reach the caller only through `failure()`. Hand over what was captured, then stop and say so.
    mic_failure(app, &mut show);
    // The toolbar's Microphone switch owns capture: turning it off stops the take's microphone.
    if !mic_on && show.mic_ok {
        if show.keeps_recording() {
            let slide = show.state.slide;
            super::take_narration(app.services.mic.as_deref_mut(), &mut show, slide);
        }
        app.stop_mic();
        show.mic_ok = false;
        show.mic_tried = false;
    }
    let editing = show.record && record::notes_has_focus(&ctx);
    // The show reads its keys only when the notes editor does not own them.
    let mut out = Out { keep: editing || keyboard(&mut app.services.mic, &ctx, &mut show, &doc, now, false), actions: vec![] };
    if show.presenter
        && !show.reading
        && let Some(host) = app.services.presenter.as_mut()
    {
        if show.remote_opened && show.presenter_grid && !host.is_open() {
            // A grid left open in the closed popup would hold auto-advance for the rest of the show.
            show.presenter_grid = false;
        }
        if !show.remote_opened {
            show.remote_opened = true;
            if let Err(message) = host.open() {
                app.status = Some((message, crate::now_ms()));
            }
        }
        for command in host.commands() {
            match command {
                crate::presenter::Command::End => out.keep = false,
                crate::presenter::Command::Zoom { x, y } if x.is_finite() && y.is_finite() && show.zoom_arm => {
                    show.zoom = Some(Zoom::at(x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)));
                    show.zoom_arm = false;
                }
                crate::presenter::Command::Key { name, command, shift, alt, ctrl } => {
                    if let Some(key) = egui::Key::from_name(&name) {
                        let modifiers = egui::Modifiers { command, shift, alt, ctrl, mac_cmd: command && !ctrl };
                        // The popup is the Presenter View: G opens All slides there, not on the audience screen (#1866).
                        out.keep &= handle_keys(&mut app.services.mic, &mut show, &doc, now, vec![(key, modifiers)], true).0;
                    }
                }
                crate::presenter::Command::Goto { slide } if show.presenter_grid && slide < doc.slides.len() => {
                    show.presenter_grid = false;
                    show.blank = None;
                    show.zoom = None;
                    show.zoom_arm = false;
                    let action = show.state.goto(&doc, slide);
                    out.keep &= show.apply(action, &doc, now);
                }
                _ => {}
            }
        }
    }
    let (clicked, secondary, moved) = ctx.input(|i| (i.pointer.primary_clicked(), i.pointer.secondary_clicked(), i.pointer.delta().length() > 0.0));
    if moved {
        show.last_move = now;
    }
    let full = ui.max_rect();
    let record_layout = if show.record { Some(record::layout(full, &doc, &show.record_view)) } else { None };
    let avail = if let Some(layout) = &record_layout {
        layout.slide
    } else if show.reading {
        Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - 34.0))
    } else {
        full
    };
    let srect = if let Some(layout) = &record_layout { layout.slide } else { fit(avail, &doc) };
    let pointer = ctx.input(|i| i.pointer.interact_pos());
    show.expire_goto(now);
    // The clock of the triggered animations runs before anything reacts to the pointer.
    show.tick_triggers(&doc, now);
    let f = Fr { ctx: &ctx, doc: &doc, doc_uid, now, full, srect, clicked, secondary, pointer, layout: record_layout };
    let pointing = handle_clicks(app, ui, &mut show, &f, &mut out);
    handle_wheel_and_swipe(&mut show, &f, &mut out);
    advance(app, &mut show, &f, &mut out);
    let painter = ui.painter_at(full);
    painter.rect_filled(full, CornerRadius::ZERO, Color32::BLACK);
    draw_slide(app, ui, &mut show, &f, &painter);
    draw_pen_and_hints(ui, &mut show, &f, &painter, pointing);
    draw_bars(app, ui, &mut show, &f, &painter, &mut out);
    draw_extras(app, ui, &mut show, &f, &painter, &mut out);
    if out.keep
        && show.presenter
        && !show.reading
        && let Some(host) = app.services.presenter.as_mut()
    {
        if now - show.remote_sent >= 0.2 {
            let next = show.state.peek_next(&doc);
            if let Some(n) = next
                && show.remote_next.as_ref().is_none_or(|(old, _)| *old != n)
            {
                let size =
                    crate::textures::fit_size((480, (480.0 * doc.slide_size.height / doc.slide_size.width.max(1.0)).clamp(1.0, 1e9) as u32), 640);
                let image = deckcraft_render::render_slide(
                    &doc,
                    n,
                    &RenderOpts { scale: size.0 as f64 / doc.slide_size.width.max(1.0), size: Some(size), ..Default::default() },
                );
                show.remote_next = crate::presenter::Preview::from_image(&crate::textures::to_color_image(&image, false)).map(|image| (n, image));
            }
            if !show.presenter_grid {
                show.remote_grid.clear();
            } else if show.remote_grid.len() < grid_indices(doc.slides.len()).len() {
                // A few thumbnails per update keep the audience window responsive on the web.
                for i in grid_indices(doc.slides.len()).into_iter().skip(show.remote_grid.len()).take(4) {
                    // At most 96 px on the long side: 48 thumbnails stay well under the popup's message limit.
                    let (w, h) = (doc.slide_size.width.max(1.0), doc.slide_size.height.max(1.0));
                    let scale = REMOTE_THUMB / w.max(h);
                    let size = (((w * scale).round() as u32).max(1), ((h * scale).round() as u32).max(1));
                    let image = deckcraft_render::render_slide(&doc, i, &RenderOpts { scale, size: Some(size), ..Default::default() });
                    let preview = crate::presenter::Preview::from_image(&crate::textures::to_color_image(&image, false));
                    show.remote_grid.push(preview.unwrap_or(crate::presenter::Preview { width: 1, height: 1, rgba: vec![0; 4] }));
                }
            }
            host.update(crate::presenter::Snapshot {
                slide: show.state.slide,
                total: doc.slides.len(),
                ended: show.ended,
                notes: doc.slides.get(show.state.slide).map(|s| s.notes_text()).unwrap_or_default(),
                elapsed: format_elapsed(show.timer.elapsed(now)),
                paused: show.timer.paused(),
                notes_px: show.notes_px,
                blank: show.blank_name().map(str::to_owned),
                grid: show.presenter_grid,
                zoom_armed: show.zoom_arm,
                // The grid replaces both previews in the popup; leaving them out keeps one message small.
                current: show.remote_preview.clone().filter(|_| !show.presenter_grid),
                next: show.remote_next.as_ref().filter(|(n, _)| Some(*n) == next && !show.presenter_grid).map(|(_, image)| image.clone()),
                slides: show.presenter_grid.then(|| show.remote_grid.clone()),
            });
            show.remote_sent = now;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    finish(app, show, &f, out.keep);
}

/// Longest side of an All slides thumbnail sent to the web presenter window (#1866).
const REMOTE_THUMB: f64 = 96.0;

/// A microphone that fails after `start` returned — a refused permission, no input device, or one
/// unplugged mid-take — reports only through [`deckcraft_media::MicIn::failure`]. Hand over what it
/// captured, stop it, and tell the user instead of recording a silent take.
pub(super) fn mic_failure(app: &mut SlideApp, show: &mut Show) -> bool {
    if !show.mic_ok || !show.keeps_recording() {
        return false;
    }
    let Some(e) = app.services.mic.as_ref().and_then(|m| m.failure()) else { return false };
    let slide = show.state.slide;
    super::take_narration(app.services.mic.as_deref_mut(), show, slide);
    app.stop_mic();
    show.mic_ok = false;
    show.mic_error = Some(e.clone());
    let device = &mut show.record_view.devices.mic;
    device.failed = true;
    device.status = super::record::devices::DeviceStatus::Unavailable;
    app.set_status(format!("Microphone: {e}"));
    true
}

fn paint_hidden(ui: &Ui, doc: &Presentation, show: &Show) {
    if show.started {
        let full = ui.max_rect();
        let avail = if show.reading { Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - 34.0)) } else { full };
        let srect = fit(avail, doc);
        let painter = ui.painter_at(full);
        painter.rect_filled(full, CornerRadius::ZERO, Color32::BLACK);
        if let Some((_, tex)) = &show.tex {
            painter.image(tex.id(), srect, unit_uv(), Color32::WHITE);
        }
    }
}

fn begin_show(ctx: &egui::Context, show: &mut Show, doc: &Presentation, now: f64, remote: bool) {
    if !show.started {
        show.started = true;
        show.slide_start = now;
        show.step_start = now;
        // The first step is settled at the show's own start: every clock origin is a real frame
        // time, so hiding and resuming shifts all of them by the same interval.
        show.step_end = now;
        show.show_start = now;
        show.timer = PresenterTimer::start(now);
        show.last_move = now;
        if show_fills_screen(show.reading, &doc.show.show_type) && !(show.presenter && remote) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
        show.after_enter(doc, now);
    }
}

fn handle_clicks(app: &mut SlideApp, ui: &mut Ui, show: &mut Show, f: &Fr, out: &mut Out) -> bool {
    let Fr { ctx, doc, doc_uid, now, full, srect, clicked, secondary, pointer, .. } = *f;
    let ctrl = ctx.input(|i| i.modifiers.ctrl);
    let laser_on = pointer_is_laser(show.laser, ctrl, show.blank.is_some());
    // With the eraser out a tap aims at ink, not at the slide: it must not advance the show.
    let eraser = show.ink_mode == InkMode::Eraser;
    let on_slide = !show.grid
        && !show.pen
        && !eraser
        && !laser_on
        && !show.ended
        && show.blank.is_none()
        && show.zoom.is_none()
        && !show.zoom_arm
        && !show.reading_bar_hit(ctx)
        && !show.record_bar_hit(ctx)
        && !pointer_on_media_bar(ctx, ui, doc, show, now);
    // The button is drawn only while the pointer is awake; a hidden button takes no click.
    let gif_click =
        if clicked && on_slide && doc.show.show_media_controls && show.pointer_awake(now) { gif_hit(ctx, ui, show, doc_uid, doc, app) } else { None };
    let media_click = if clicked && on_slide && gif_click.is_none() { media_hit(ctx, ui, show, doc) } else { None };
    let under = if on_slide && gif_click.is_none() && media_click.is_none() { shapes_under_pointer(ctx, ui, show, doc, now) } else { None };
    let pointing = under.as_ref().is_some_and(|u| u.pick.is_some());
    if let Some(id) = gif_click {
        let _ = app.run("media.gifPlay", json!({"id": id}));
    } else if let Some((id, clip)) = media_click {
        cancel_pending_media(show, id);
        app.media.toggle(doc_uid, doc, id, &clip, crate::media::Owner::Show);
    } else if clicked && let Some(Pick::Text(l)) = under.as_ref().and_then(|u| u.pick.clone()) {
        let last = show.last_viewed;
        match show.state.follow_link(doc, &l, last) {
            LinkJump::Show(a) => out.actions.push(a),
            LinkJump::Open(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            LinkJump::Exit => out.keep = false,
            LinkJump::None => {}
        }
    } else if show.grid && clicked {
        let aspect = slide_aspect(doc);
        if let Some(p) = pointer
            && let Some(n) = see_all_hit(full, grid_indices(doc.slides.len()).len(), aspect, p)
            && let Some(&i) = grid_indices(doc.slides.len()).get(n)
        {
            show.grid = false;
            show.blank = None;
            show.zoom = None;
            show.zoom_arm = false;
            out.actions.push(show.state.goto(doc, i));
        }
    } else if clicked && show.zoom_arm && !show.reading_bar_hit(ctx) {
        if let Some(p) = pointer
            && let Some((nx, ny)) = norm_on_slide(srect, unit_uv(), p)
        {
            show.zoom = Some(Zoom::at(nx, ny));
            show.zoom_arm = false;
        }
    } else if clicked && show.zoom.is_some() && !show.pen && !show.reading_bar_hit(ctx) {
        show.dismiss_zoom();
    } else if clicked
        && !show.pen
        && !eraser
        && !laser_on
        && !show.reading_bar_hit(ctx)
        && !show.record_bar_hit(ctx)
        && !pointer_on_media_bar(ctx, ui, doc, show, now)
    {
        if show.ended {
            out.keep = false;
        } else if show.blank.is_some() {
            show.blank = None;
        } else {
            // A shape with an action or a triggered animation, or the background.
            let none = Under::default();
            match show.click_slide(doc, under.as_ref().unwrap_or(&none), now) {
                Click::Show(a) => out.actions.push(a),
                Click::Open(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                Click::Exit => out.keep = false,
                Click::Animated | Click::None => {}
            }
        }
    }
    if secondary && !show.ended && !laser_on && !eraser && !show.record_bar_hit(ctx) && !show.grid && show.zoom.is_none() && !show.zoom_arm {
        out.actions.push(show.state.prev(doc));
    }
    pointing
}

fn handle_wheel_and_swipe(show: &mut Show, f: &Fr, out: &mut Out) {
    let Fr { ctx, doc, now, srect, pointer, .. } = *f;
    // The teleprompter overlay scrolls itself: a flick over it does not turn the page.
    if pointer.is_some_and(|p| show.record_bar_hit_at(ctx, p)) {
        return;
    }
    // Ctrl+wheel (Cmd+wheel on a Mac) zooms around the pointer, as in PowerPoint (#1286). egui
    // keeps it out of `smooth_scroll_delta`, so it does not also turn the page below.
    let line_points = ctx.options(|o| o.input_options.line_scroll_speed);
    if !show.grid
        && !show.ended
        && show.blank.is_none()
        && let Some(p) = pointer
        && let Some(lines) = ctx.input(|i| wheel_zoom_lines(&i.events, line_points))
    {
        show.wheel_zoom(srect, p, lines);
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
                out.keep = false;
            } else {
                out.actions.push(show.state.next(doc));
            }
        } else {
            show.ended = false;
            out.actions.push(show.state.prev(doc));
        }
    }
}

fn advance(app: &mut SlideApp, show: &mut Show, f: &Fr, out: &mut Out) {
    let Fr { ctx, doc, doc_uid, now, .. } = *f;
    // Apply input first: a step started by this click must restart the clock before it is read.
    for a in out.actions.drain(..) {
        before_slide_change(app, show, &a);
        if !show.apply(a, doc, now) {
            out.keep = false;
        }
    }
    // Step clock, shared with the hidden logic tick.
    if !show.queue_clock_actions(doc, now, &mut out.actions) {
        out.keep = false;
    }
    for a in std::mem::take(&mut out.actions) {
        before_slide_change(app, show, &a);
        if !show.apply(a, doc, now) {
            out.keep = false;
        }
    }
    // The Record view draws the slide smaller than the window: measure the rect it uses.
    show.ink_width = pen_width_slide(doc, f.srect);
    if show.state.playing {
        let step = show.state.step;
        show.cue_media(doc, step, now - show.step_start);
    }
    show_media(app, show, doc_uid, doc, now);
    run_pending_media(app, show, doc_uid, doc, now);
    if let Some(due) = show.pending_media.iter().map(|(_, due)| *due).reduce(f64::min) {
        // Nothing else may ask for a frame in a static Reading View: wake at the deadline.
        ctx.request_repaint_after(std::time::Duration::from_secs_f64((due - now).max(0.0)));
    }
    run_media_cues(app, show, doc_uid, doc);
}

fn begin_slide(app: &mut SlideApp, show: &Show, f: &Fr, size: (u32, u32)) -> Sl {
    let Fr { ctx, doc, doc_uid, now, .. } = *f;
    let idx = show.state.slide;
    let step = show.state.step;
    // A main step or a triggered animation is running: the picture changes every frame.
    let animating = show.state.playing || show.state.trigger_playing();
    let t = if show.state.playing { now - show.step_start } else { 0.0 };
    let plays = show.state.trigger_plays();
    let stamps = show.state.trigger_stamps();
    let media = crate::media::slide_media(doc, idx);
    let hidden: Vec<ShapeId> = media
        .iter()
        .filter(|(id, _, clip)| clip.hide_while_not_playing && !app.media.status(*id).is_some_and(|s| s.state != deckcraft_media::PlayState::Ended))
        .map(|m| m.0)
        .collect();
    let gifs = app.media.gifs(doc_uid, doc, idx, now, &app.session.gif_paused);
    let times: Vec<_> = gifs.iter().map(|g| (g.id, g.t)).collect();
    crate::media::gif_repaint(ctx, &gifs);
    let gif = crate::media::gif_key(&gifs);
    let done: Vec<(ShapeId, usize)> = plays.iter().map(|p| (p.trigger, p.step)).collect();
    let (ink_due, ink_next) = if show.recording_preview {
        let elapsed = ((now - show.slide_start).max(0.0) * 1000.0) as u32;
        let mut due = 0usize;
        let mut next: Option<u32> = None;
        if let Some(sl) = doc.slides.get(idx) {
            count_due_strokes(&sl.shapes, elapsed, &mut due, &mut next);
        }
        (due, next)
    } else {
        (0, None)
    };
    let key = Key { slide: idx, step, size, doc: std::sync::Arc::as_ptr(doc) as usize, hidden: hidden.clone(), plays: done, gif, ink: ink_due };
    let back_key = BackKey { slide: idx, size, doc: key.doc };
    Sl { idx, step, animating, t, plays, stamps, media, hidden, times, gifs, key, back_key, size, ink_next }
}

/// Recorded strokes the clock has reached (`due`) and the next one's clock (`next`): a
/// recording preview reveals ink on the clock, and the frame key carries the count so the
/// frame renders once per revealed stroke. Grouped strokes draw too, so the count sees them
/// inside groups as well.
pub(super) fn count_due_strokes(shapes: &[deckcraft_model::Shape], elapsed: u32, due: &mut usize, next: &mut Option<u32>) {
    for sh in shapes {
        match &sh.kind {
            deckcraft_model::ShapeKind::Ink { strokes } => {
                for st in strokes {
                    match st.at_ms {
                        Some(at) if at <= elapsed => *due += 1,
                        Some(at) => *next = Some(next.map_or(at, |n| n.min(at))),
                        None => *due += 1,
                    }
                }
            }
            deckcraft_model::ShapeKind::Group { children, .. } => count_due_strokes(children, elapsed, due, next),
            _ => {}
        }
    }
}

fn frame_texture(show: &mut Show, f: &Fr, sl: &Sl) -> Option<TextureHandle> {
    let Fr { ctx, doc, .. } = *f;
    let Sl { idx, step, animating, size, .. } = *sl;
    // Woken on every pass, rendered or cached: the deadline must not depend on which branch runs.
    if let Some(at) = sl.ink_next {
        let wait = (at as f64 / 1000.0 - (f.now - show.slide_start).max(0.0)).max(0.0);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
    }
    if !animating && show.tex.as_ref().is_some_and(|(k, _)| *k == sl.key) {
        show.tex.as_ref().map(|(_, t)| t.clone())
    } else {
        // The frame rendered ahead of time, when it is this one (waiting for it beats starting over).
        let ready = match show.prefetch.take() {
            Some(p) if p.key == sl.key && !animating => p.rx.recv().ok(),
            _ => None,
        };
        let ci = match ready {
            Some(ci) => ci,
            None => {
                let tl = show.timeline(doc).clone();
                // Animating: the backdrop doesn't change, only the shapes over it are drawn.
                let back = animating.then(|| show.backdrop(doc, sl.back_key));
                // A recording preview shows each recorded stroke when the take drew it.
                let ink_until = show.recording_preview.then(|| ((f.now - show.slide_start).max(0.0) * 1000.0) as u32);
                render_frame(
                    doc,
                    idx,
                    tl,
                    step,
                    sl.t,
                    size,
                    sl.hidden.clone(),
                    &sl.times,
                    render_threads(),
                    &sl.plays,
                    &sl.stamps,
                    back.as_deref(),
                    ink_until,
                )
            }
        };
        if show.remote_opened && show.presenter {
            show.remote_preview = crate::presenter::Preview::from_image(&ci);
        }
        let tex = match show.tex.take() {
            Some((_, mut h)) => {
                crate::textures::replace(ctx, &mut h, ci);
                h
            }
            None => crate::textures::upload(ctx, "show", ci),
        };
        show.tex = Some((sl.key.clone(), tex.clone()));
        Some(tex)
    }
}

fn prefetch_next(app: &mut SlideApp, show: &mut Show, f: &Fr, sl: &Sl) {
    let Fr { doc, doc_uid, now, .. } = *f;
    let Sl { animating, size, .. } = *sl;
    // While this slide rests, render the next one's first frame (not one with GIFs, whose
    // frame depends on when it is entered).
    // A slide with animations gets its backdrop ready while it rests, so they start smoothly.
    if !animating && !show.timeline(doc).is_empty() {
        show.prefetch_backdrop(doc, sl.back_key);
    }
    if !animating && show.trans.is_none() && !show.preview_only {
        let next = show.state.peek_next(doc);
        let wanted = next.filter(|n| app.media.gifs(doc_uid, doc, *n, now, &app.session.gif_paused).is_empty());
        if let Some(n) = wanted
            && show.prefetch.as_ref().is_none_or(|p| p.key.slide != n || p.key.size != size || p.key.doc != std::sync::Arc::as_ptr(doc) as usize)
        {
            show.prefetch = start_prefetch(doc, n, size);
        }
    }
}

fn draw_transition(show: &mut Show, f: &Fr, sl: &Sl, painter: &egui::Painter, tex: &Option<TextureHandle>) -> bool {
    let Fr { ctx, doc, now, srect, .. } = *f;
    let Sl { idx, size, .. } = *sl;
    let mut drew = false;
    let morphing = show.trans.as_ref().is_some_and(|tr| tr.kind == "morph" && tr.from.is_some() && now - tr.start < tr.dur);
    let tl = if morphing { Some(show.timeline(doc).clone()) } else { None };
    let morph = match (&show.trans, tl) {
        (Some(tr), Some(tl)) => {
            let p = ((now - tr.start) / tr.dur.max(0.01)).clamp(0.0, 1.0);
            let hidden_b = |id: ShapeId| {
                let st = state_for(&tl, id, 0, 0.0, &[], &[]);
                !st.visible || st.opacity <= 0.0 || sl.hidden.contains(&id)
            };
            tr.from
                .and_then(|i| doc.slides.get(i).map(|a| (i, a)))
                .zip(doc.slides.get(idx))
                .map(|((i, a), b)| (i, deckcraft_anim::morph_frame(doc, a, b, p, &hidden_b)))
        }
        _ => None,
    };
    if let (Some((from, m)), Some(tr)) = (morph, show.trans.as_mut()) {
        let f = |id: ShapeId| ShapeState { opacity: m.opacity_of(id), ..Default::default() };
        let threads = if cfg!(target_arch = "wasm32") { 0 } else { std::thread::available_parallelism().map(|n| n.get().min(8) as u16).unwrap_or(0) };
        let opts =
            RenderOpts { scale: size.0 as f64 / doc.slide_size.width.max(1.0), size: Some(size), state: Some(&f), threads, ..Default::default() };
        let text: Vec<deckcraft_render::TextMorph> =
            m.text.iter().map(|x| deckcraft_render::TextMorph { old: x.old, new: x.new, from: x.from, to: x.to, chars: x.chars, t: m.mix }).collect();
        let paths: Vec<deckcraft_render::PathMorph> = m
            .paths
            .iter()
            .map(|x| deckcraft_render::PathMorph { id: x.id, from: x.from.clone(), from_box: x.from_box, to_box: x.to_box, t: m.mix })
            .collect();
        let img = deckcraft_render::render_blend(doc, (from, idx), m.mix, &m.shapes, &text, &paths, &opts);
        let ci = crate::textures::to_color_image(&img, false);
        let frame = match tr.morph.take() {
            Some(mut h) => {
                crate::textures::replace(ctx, &mut h, ci);
                h
            }
            None => crate::textures::upload(ctx, "morph", ci),
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
                tr.parts = Some(part_textures(ctx, doc, tr.from, idx, size));
            }
            if tr.old.is_none()
                && let Some(prev) = tr.from
                && !deckcraft_anim::uses_slide_parts(&tr.kind, &tr.option)
            {
                let threads =
                    if cfg!(target_arch = "wasm32") { 0 } else { std::thread::available_parallelism().map(|n| n.get().min(8) as u16).unwrap_or(0) };
                let timeline = ShowState::timeline(doc, prev);
                let step = timeline.steps();
                let ci = render_frame(doc, prev, timeline, step, 0.0, size, vec![], &[], threads, &[], &[], None, None);
                tr.old = Some(crate::textures::upload(ctx, "trans-old", ci));
            }
            let aspect = doc.slide_size.width / doc.slide_size.height.max(1.0);
            let part = |k: usize| tr.parts.as_ref().and_then(|p| p.get(k).cloned().flatten());
            let parts = [part(0), part(1), part(2), part(3)];
            for l in transition_layers(&tr.kind, &tr.option, tr.reverse, p, aspect) {
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
    drew
}

/// The layers of a transition at progress `p`. A reversed transition (going back, #1531) runs
/// from its end to its start, so the slide being left plays the part of the new slide and the
/// slide coming back the part of the old one.
pub(super) fn transition_layers(kind: &str, option: &str, reverse: bool, p: f64, aspect: f64) -> Vec<Layer> {
    if !reverse {
        return deckcraft_anim::transition_frame(kind, option, p, aspect);
    }
    let mut layers = deckcraft_anim::transition_frame(kind, option, 1.0 - p, aspect);
    for l in &mut layers {
        l.source = swap_old_new(l.source);
    }
    layers
}

fn swap_old_new(s: Source) -> Source {
    match s {
        Source::Old => Source::New,
        Source::New => Source::Old,
        Source::OldBack => Source::NewBack,
        Source::NewBack => Source::OldBack,
        Source::OldContent => Source::NewContent,
        Source::NewContent => Source::OldContent,
        Source::Black | Source::White => s,
    }
}

fn draw_videos(app: &mut SlideApp, show: &mut Show, f: &Fr, sl: &Sl, painter: &egui::Painter, tex: &Option<TextureHandle>) -> (Vec<ShapeId>, bool) {
    let Fr { doc, full, srect, .. } = *f;
    if let Some(tex) = &tex {
        painter.image(tex.id(), srect, show.zoom_uv(), Color32::WHITE);
    }
    // Playing video: the current frame in its box, or full screen.
    let (sw, sh) = (doc.slide_size.width as f32, doc.slide_size.height as f32);
    let uv = show.zoom_uv();
    let mut framed: Vec<ShapeId> = vec![];
    let mut full_screen = false;
    for (id, x, clip) in &sl.media {
        if !clip.video || sl.hidden.contains(id) {
            continue;
        }
        let r = if clip.full_screen && app.media.is_playing(*id) {
            full_screen = true;
            full
        } else {
            slide_box(srect, uv, (sw, sh), (x.x as f32, x.y as f32, x.w as f32, x.h as f32))
        };
        if crate::media::paint_frame(painter, &app.media, *id, r, clip.crop) {
            framed.push(*id);
        }
    }
    (framed, full_screen)
}

fn draw_overlay(show: &mut Show, f: &Fr, sl: &Sl, painter: &egui::Painter, framed: &[ShapeId], full_screen: bool) {
    let Fr { ctx, doc, srect, .. } = *f;
    let Sl { idx, step, animating, size, .. } = *sl;
    let uv = show.zoom_uv();
    // Shapes stacked above a playing video show over it, as in PowerPoint (#1005); a video
    // playing full screen covers the whole slide.
    let skip = if full_screen { vec![] } else { below_playing_video(doc, idx, framed) };
    if skip.is_empty() {
        show.overlay = None;
    } else {
        // A recording preview reveals ink on the clock: the overlay's texture is only fresh while
        // its cutoff is unchanged.
        let ink_until = show.recording_preview.then(|| ((f.now - show.slide_start).max(0.0) * 1000.0) as u32);
        let fresh = !animating && ink_until.is_none() && show.overlay.as_ref().is_some_and(|(k, s, _)| *k == sl.key && *s == skip);
        if !fresh {
            let tl = show.timeline(doc).clone();
            let ci = render_overlay(
                doc,
                idx,
                tl,
                step,
                sl.t,
                size,
                sl.hidden.clone(),
                &skip,
                &sl.times,
                render_threads(),
                &sl.plays,
                &sl.stamps,
                ink_until,
            );
            let handle = match show.overlay.take() {
                Some((_, _, mut h)) => {
                    crate::textures::replace(ctx, &mut h, ci);
                    h
                }
                None => crate::textures::upload(ctx, "show-overlay", ci),
            };
            show.overlay = Some((sl.key.clone(), skip, handle));
        }
        if let Some((_, _, h)) = &show.overlay {
            painter.image(h.id(), srect, uv, Color32::WHITE);
        }
    }
}

fn draw_media_controls(app: &mut SlideApp, ui: &mut Ui, show: &mut Show, f: &Fr, sl: &Sl, painter: &egui::Painter) {
    let Fr { ctx, doc, doc_uid, now, srect, .. } = *f;
    let sw = doc.slide_size.width as f32;
    // Play/pause on the animated GIF under the pointer, and the audio/video bar.
    // Slide Show ▸ Show Media Controls hides both (the click still sl.plays a clip).
    // Not while zoomed: the controls sit where the whole slide would be.
    if doc.show.show_media_controls && !show.zoom_arm && show.zoom.is_none() {
        let k = srect.width() / sw.max(1.0);
        if let Some(p) = ctx.input(|i| i.pointer.hover_pos()).filter(|_| !show.pen && !show.laser && show.pointer_awake(now)) {
            for g in &sl.gifs {
                let r = on_screen(srect, k, &g.rect);
                if r.contains(p) {
                    let b = crate::media::gif_button_rect(r);
                    crate::media::paint_gif_button(painter, b, g.paused, b.contains(p));
                }
            }
        }
        if let Some((id, clip, bar)) = media_bar_target(ctx, doc, show, srect).filter(|_| show.pointer_awake(now)) {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
            let action = {
                let status = app.media.status(id);
                crate::media::control_bar(ui, bar, status.as_ref(), &clip)
            };
            let pos = app.media.status(id).map(|s| s.position).unwrap_or(0.0);
            match action {
                // The presenter took over the clip: its pending automatic start is cancelled,
                // or it would fire (and resume a pause) at the deadline.
                Some(crate::media::BarAction::Toggle) => {
                    cancel_pending_media(show, id);
                    app.media.toggle(doc_uid, doc, id, &clip, crate::media::Owner::Show);
                }
                Some(crate::media::BarAction::Seek(t)) => {
                    cancel_pending_media(show, id);
                    app.media.seek(doc_uid, doc, id, &clip, t, crate::media::Owner::Show);
                }
                Some(crate::media::BarAction::Nudge(dt)) => {
                    cancel_pending_media(show, id);
                    app.media.seek(doc_uid, doc, id, &clip, (pos + dt).max(0.0), crate::media::Owner::Show);
                }
                None => {}
            }
        }
    }
}

fn draw_slide(app: &mut SlideApp, ui: &mut Ui, show: &mut Show, f: &Fr, painter: &egui::Painter) {
    let Fr { ctx, full, srect, .. } = *f;
    let ppp = ctx.pixels_per_point();
    // A huge screen or DPI mustn't ask for a picture the GPU can't hold: render at most its limit
    // (drawn stretched to the screen).
    let size = crate::textures::fit_size(
        ((srect.width() * ppp).round().clamp(1.0, u32::MAX as f32) as u32, (srect.height() * ppp).round().clamp(1.0, u32::MAX as f32) as u32),
        crate::textures::max_side(ctx),
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
        let sl = begin_slide(app, show, f, size);
        let tex = frame_texture(show, f, &sl);
        // A recording preview reveals ink on the clock, and the prefetched frame has no cutoff:
        // rendering it would show the next slide's strokes early.
        if !show.recording_preview {
            prefetch_next(app, show, f, &sl);
        }
        if !draw_transition(show, f, &sl, painter, &tex) {
            show.trans = None;
            let (framed, full_screen) = draw_videos(app, show, f, &sl, painter, &tex);
            draw_overlay(show, f, &sl, painter, &framed, full_screen);
            draw_media_controls(app, ui, show, f, &sl, painter);
        }
        if sl.animating {
            ctx.request_repaint();
        }
    }
}

fn draw_pen_and_hints(ui: &mut Ui, show: &mut Show, f: &Fr, painter: &egui::Painter, pointing: bool) {
    let Fr { ctx, doc, now, full, srect, .. } = *f;
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
    // Ink: the pen and highlighter capture pointer drags; the eraser removes whole strokes.
    let ctrl = ctx.input(|i| i.modifiers.ctrl);
    let laser_on = pointer_is_laser(show.laser, ctrl, show.blank.is_some());
    let erasing = show.ink_mode == InkMode::Eraser;
    let pen_rect = f.layout.map_or(full, |l| l.slide);
    let pointer = if (show.pen || erasing) && !laser_on { Sense::drag() } else { Sense::hover() };
    let resp = ui.interact(pen_rect, ui.scope_id().with("show_pen"), pointer);
    if (show.pen || erasing) && !laser_on {
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, show.ink_mode.label()));
    }
    if show.pen && !laser_on {
        if resp.drag_started() {
            show.strokes.push(InkStroke {
                points: vec![],
                mode: show.ink_mode,
                color: show.ink_color,
                width: show.ink_width * show.ink_mode.width_scale(),
                in_take: show.keeps_recording(),
            });
        }
        let uv = show.zoom_uv();
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
            && let Some((nx, ny)) = norm_on_slide(srect, uv, p)
            && let Some(s) = show.strokes.last_mut()
        {
            s.points.push(pos2(nx, ny));
        }
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    } else if erasing && !laser_on {
        let uv = show.zoom_uv();
        if (resp.drag_started() || resp.dragged())
            && let Some(p) = resp.interact_pointer_pos()
            && let Some((nx, ny)) = norm_on_slide(srect, uv, p)
        {
            // Erase along the path the pointer swept since the last frame, not only at today's
            // point: a fast drag can cross a whole stroke between two samples.
            let here = pos2(nx, ny);
            let from = if resp.drag_started() { here } else { show.eraser_from.unwrap_or(here) };
            // The eraser is wide enough to feel like PowerPoint's; whole strokes go.
            show.erase_ink_at(from, here, 0.015);
            show.eraser_from = Some(here);
        } else {
            show.eraser_from = None;
        }
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    } else if laser_on {
        ctx.set_cursor_icon(egui::CursorIcon::None);
    } else if show.zoom_arm || show.grid {
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    } else if show.hides_idle_cursor(now) {
        // Idle first: a still pointer hides also over a clickable shape, such as a playing clip.
        ctx.set_cursor_icon(egui::CursorIcon::None);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    } else if pointing {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
    let uv = show.zoom_uv();
    for s in &show.strokes {
        let pts: Vec<Pos2> = s.points.iter().map(|q| slide_to_screen(srect, uv, q.x, q.y)).collect();
        if pts.len() < 2 {
            continue;
        }
        // Highlighter ink is translucent and three times wider; the pen is opaque and thin.
        let k = srect.width() / doc.slide_size.width.max(1.0) as f32 / uv.width().max(1e-4);
        let (width, alpha) = match s.mode {
            InkMode::Highlighter => ((s.width as f32 * k).max(2.0), 0.4),
            _ => (3.0, 1.0),
        };
        let c = theme::to_color32(s.color);
        let c = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (c.a() as f32 * alpha) as u8);
        painter.add(egui::Shape::line(pts, Stroke::new(width, c)));
    }
    if laser_on
        && !show.ended
        && let Some(p) = ctx.input(|i| i.pointer.hover_pos())
    {
        let c = theme::to_color32(doc.show.laser_color);
        painter.circle_filled(p, 16.0, Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 70));
        painter.circle_filled(p, 5.5, c);
    }
    if show.keeps_recording() && laser_on {
        if let Some(p) = ctx.input(|i| i.pointer.hover_pos()) {
            // The same mapping the pen uses: a zoomed show must record the point the presenter
            // indicated, not where the pointer is relative to the whole screen rect.
            let uv = show.zoom_uv();
            if let Some((nx, ny)) = norm_on_slide(srect, uv, p) {
                let ms = (now - show.slide_start).max(0.0) * 1000.0;
                if ms.is_finite() {
                    show.note_laser(nx as f64 * doc.slide_size.width, ny as f64 * doc.slide_size.height, ms.min(3_600_000.0) as u32);
                }
            }
        }
    } else if !show.keeps_recording()
        && let Some(sl) = doc.slides.get(show.state.slide)
        && let Some(trace) = &sl.laser
    {
        let elapsed = (now - show.slide_start).max(0.0) * 1000.0;
        if elapsed.is_finite()
            && let Some((x, y)) = laser_pos(&trace.points, elapsed.min(3_600_000.0) as u32)
        {
            let k = srect.width() / doc.slide_size.width.max(1.0) as f32;
            let p = pos2(srect.min.x + x as f32 * k, srect.min.y + y as f32 * k);
            painter.circle_filled(p, 7.0, theme::to_color32(trace.color));
            painter.circle_stroke(p, 11.0, Stroke::new(2.0, Color32::from_white_alpha(200)));
            ctx.request_repaint();
        }
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
}

fn draw_bars(app: &mut SlideApp, ui: &mut Ui, show: &mut Show, f: &Fr, painter: &egui::Painter, out: &mut Out) {
    let Fr { ctx, doc, now, full, .. } = *f;
    // The Record view has its own top and bottom bars.
    if show.record {
        return;
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
            out.keep = false;
        }
        if child.button("▶").clicked() {
            let a = show.state.next(doc);
            before_slide_change(app, show, &a);
            if !show.apply(a, doc, now) {
                out.keep = false;
            }
        }
        if child.button("◀").clicked() {
            let a = show.state.prev(doc);
            before_slide_change(app, show, &a);
            show.apply(a, doc, now);
        }
    } else if !show.grid && (show.pointer_awake(now) || show.zoom_arm) {
        let bar = tool_bar_rect(full);
        painter.rect_filled(bar, CornerRadius::same(17), Color32::from_black_alpha(140));
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(10.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let b = |ui: &mut Ui, s: &str| ui.add(egui::Button::new(egui::RichText::new(s).color(Color32::WHITE).size(15.0)).frame(false));
        if b(&mut child, "◀").clicked() {
            let a = show.state.prev(doc);
            before_slide_change(app, show, &a);
            show.apply(a, doc, now);
        }
        if b(&mut child, "▶").clicked() {
            let a = show.state.next(doc);
            before_slide_change(app, show, &a);
            if !show.apply(a, doc, now) {
                out.keep = false;
            }
        }
        if b(&mut child, if show.pen { "✏ on" } else { "✏" }).clicked() {
            show.set_pen(!show.pen);
        }
        if b(&mut child, if show.laser { "◉ on" } else { "◉" }).clicked() {
            show.set_laser(!show.laser);
        }
        if b(&mut child, "Black").clicked() {
            toggle_blank(show, Color32::BLACK);
        }
        if b(&mut child, "White").clicked() {
            toggle_blank(show, Color32::WHITE);
        }
        if b(&mut child, "All").clicked() {
            show.grid = !show.grid;
        }
        if b(&mut child, show.zoom_label()).clicked() {
            show.zoom_key();
        }
        if b(&mut child, "End").clicked() {
            out.keep = false;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
    }
}

fn draw_extras(app: &mut SlideApp, ui: &mut Ui, show: &mut Show, f: &Fr, painter: &egui::Painter, out: &mut Out) {
    let Fr { ctx, doc, now, full, .. } = *f;
    if show.grid && !show.ended {
        paint_grid(painter, ctx, show, doc, full);
    }
    if let Some(layout) = f.layout {
        out.keep = record::ui(app, ui, show, doc, now, &layout) && out.keep;
    } else if show.rehearse {
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
    if out.keep && show.presenter && !show.reading && !show.remote_opened {
        presenter_window(app, ctx, show, doc, now, &mut out.keep);
    }
}

fn finish(app: &mut SlideApp, mut show: Show, f: &Fr, keep: bool) {
    let Fr { ctx, doc, now, .. } = *f;
    if keep {
        app.show = Some(show);
    } else {
        if let Some(host) = app.services.presenter.as_mut() {
            host.close();
        }
        app.media.stop_owned(crate::media::Owner::Show);
        if !show.reading {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
        // A Record preview plays the recording; it never writes timings, ink, narration or laser.
        save_take(app, &mut show, doc, now);
        // A preview is a look at the recording: the editor keeps the slide it had.
        if !show.recording_preview {
            let last = show.state.slide;
            let _ = app.run("slide.go", json!({"index": last}));
        }
    }
}
