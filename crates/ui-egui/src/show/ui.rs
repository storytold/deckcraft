use super::keys::keyboard;
use super::media::{
    Pick, Under, gif_hit, media_bar_target, media_hit, on_screen, pointer_on_media_bar, run_media_cues, shapes_under_pointer, show_media,
};
use super::presenter::{PresenterTimer, presenter_window};
use super::render::{below_playing_video, fit, part_textures, quad_mesh, render_frame, render_overlay, render_threads, start_prefetch};
use super::zoom::{
    Zoom, grid_indices, norm_on_slide, paint_grid, see_all_hit, slide_aspect, slide_box, slide_to_screen, toggle_blank, tool_bar_rect, unit_uv,
    wheel_zoom_lines,
};
use super::*;

/// Draw and advance the active show, handling hidden frames, timing, input, transitions, and
/// presenter/reading presentation.
pub fn ui(app: &mut SlideApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let Some((doc_uid, doc)) = app.session.active().map(|d| (d.uid, d.doc.clone())) else {
        app.show = None;
        return;
    };
    let now = ctx.input(|i| i.time);
    let Some(mut show) = app.show.take() else { return };
    if !viewport_visible(&ctx) {
        // A forced hidden paint (for example a pending screenshot) draws the current frame;
        // the show clocks are advanced by logic, not by this draw, so it must not advance them a
        // second time. Keep drawing the last frame rather than returning a blank.
        if show.started {
            let full = ui.max_rect();
            let avail = if show.reading { Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - 34.0)) } else { full };
            let srect = fit(avail, &doc);
            let painter = ui.painter_at(full);
            painter.rect_filled(full, CornerRadius::ZERO, Color32::BLACK);
            if let Some((_, tex)) = &show.tex {
                painter.image(tex.id(), srect, unit_uv(), Color32::WHITE);
            }
        }
        app.show = Some(show);
        return;
    }
    show.observe_visible_frame(now, web_time::Instant::now());
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
        && !pointer_on_media_bar(&ctx, ui, &doc, &show, now);
    // The button is drawn only while the pointer is awake; a hidden button takes no click.
    let gif_click = if clicked && on_slide && doc.show.show_media_controls && show.pointer_awake(now) {
        gif_hit(&ctx, ui, &show, doc_uid, &doc, app)
    } else {
        None
    };
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
    } else if clicked && !show.pen && !show.reading_bar_hit(&ctx) && !pointer_on_media_bar(&ctx, ui, &doc, &show, now) {
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
    // Step clock, shared with the hidden logic tick.
    if !show.queue_clock_actions(&doc, now, &mut actions) {
        keep = false;
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
                if let Some(p) = ctx.input(|i| i.pointer.hover_pos()).filter(|_| !show.pen && !show.laser && show.pointer_awake(now)) {
                    for g in &gifs {
                        let r = on_screen(srect, k, &g.rect);
                        if r.contains(p) {
                            let b = crate::media::gif_button_rect(r);
                            crate::media::paint_gif_button(&painter, b, g.paused, b.contains(p));
                        }
                    }
                }
                if let Some((id, clip, bar)) = media_bar_target(&ctx, &doc, &show, srect).filter(|_| show.pointer_awake(now)) {
                    ctx.request_repaint_after(std::time::Duration::from_millis(500));
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
    let resp = ui.interact(full, ui.scope_id().with("show_pen"), if show.pen { Sense::drag() } else { Sense::hover() });
    if show.pen {
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Pen"));
    }
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
    } else if !show.pointer_awake(now) && !show.reading {
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
    } else if !show.grid && (show.pointer_awake(now) || show.zoom_arm) {
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
