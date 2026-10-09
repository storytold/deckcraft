//! Slide Show: full-screen playback with transitions and animations (`deckcraft-anim`),
//! keyboard/mouse navigation, pen, black/white screens, presenter view and rehearsed timings.

use deckcraft_anim::{Layer, LinkJump, ShowAction, ShowState, Source, Timeline};
use deckcraft_model::text::Hyperlink;
use deckcraft_model::{Presentation, ShapeId};
use deckcraft_render::{ParaState, RenderOpts, ShapeState};
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
    next_tex: Option<(usize, TextureHandle)>,
    blank: Option<Color32>,
    pen: bool,
    strokes: Vec<Vec<Pos2>>,
    goto: String,
    started: bool,
    show_start: f64,
    timings: Vec<(usize, f64)>,
    last_move: f64,
    pub ended: bool,
    /// The slide whose media was last started (auto-play runs once per visit).
    media_slide: Option<usize>,
    /// The slide shown before the current one (the Last Slide Viewed action).
    last_viewed: Option<usize>,
    /// Click areas of hyperlinks and shape actions on slide `.0`, in slide points.
    links: Option<(usize, Vec<deckcraft_render::PlacedLink>)>,
}

#[derive(Clone, PartialEq)]
struct Key {
    slide: usize,
    step: usize,
    size: (u32, u32),
    doc: usize,
    /// Media hidden while not playing.
    hidden: Vec<ShapeId>,
}

struct Transition {
    old: Option<TextureHandle>,
    /// The slide left behind (Morph matches its shapes) and the Morph frame texture.
    from: Option<usize>,
    morph: Option<TextureHandle>,
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
            next_tex: None,
            blank: None,
            pen: false,
            strokes: vec![],
            goto: String::new(),
            started: false,
            show_start: 0.0,
            timings: vec![],
            last_move: 0.0,
            ended: false,
            media_slide: None,
            last_viewed: None,
            links: None,
        }
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
            ShowAction::PlayStep(_) => {
                self.step_start = now;
            }
            ShowAction::FinishStep(_) | ShowAction::StepBack(_) => {
                self.step_end = now;
            }
            ShowAction::Slide(i) => {
                if self.rehearse {
                    self.timings.push((self.timeline.as_ref().map(|t| t.0).unwrap_or(i), now - self.slide_start));
                }
                let (kind, option, dur) = doc
                    .slides
                    .get(i)
                    .and_then(|s| s.transition.as_ref())
                    .map(|t| (t.kind.clone(), t.option.clone(), t.duration_ms as f64 / 1000.0))
                    .unwrap_or(("none".into(), String::new(), 0.0));
                let old = self.tex.as_ref().map(|(_, t)| t.clone());
                let from = self.tex.as_ref().map(|(k, _)| k.slide);
                if from != Some(i) {
                    self.last_viewed = from;
                }
                self.trans = if kind != "none" && dur > 0.0 && !doc.show.without_animation {
                    Some(Transition { old, from, morph: None, start: now, kind, option, dur })
                } else {
                    None
                };
                self.slide_start = now;
                self.step_start = now;
                self.step_end = now;
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
    fn after_enter(&mut self, doc: &Presentation, now: f64) {
        let auto = self.timeline(doc).step_is_auto(0);
        if auto || self.autoplay {
            let a = self.state.next(doc);
            if matches!(a, ShowAction::PlayStep(_)) {
                self.step_start = now;
            }
        }
    }
}

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

fn fit(avail: Rect, doc: &Presentation) -> Rect {
    let (sw, sh) = (doc.slide_size.width as f32, doc.slide_size.height as f32);
    let s = (avail.width() / sw).min(avail.height() / sh);
    Rect::from_center_size(avail.center(), vec2(sw * s, sh * s))
}

fn quad_mesh(tex: Option<egui::TextureId>, rect: Rect, l: &Layer, color: Color32) -> Mesh {
    let mut m = match tex {
        Some(id) => Mesh::with_texture(id),
        None => Mesh::default(),
    };
    let a = (l.alpha.clamp(0.0, 1.0) * 255.0) as u8;
    let col = if tex.is_some() { Color32::from_white_alpha(a) } else { color.gamma_multiply(l.alpha.clamp(0.0, 1.0) as f32) };
    for k in 0..4 {
        let q = l.quad.get(k).copied().unwrap_or([0.0, 0.0]);
        let uv = l.uv.get(k).copied().unwrap_or([0.0, 0.0]);
        let p = pos2(rect.min.x + q[0] as f32 * rect.width(), rect.min.y + q[1] as f32 * rect.height());
        m.vertices.push(egui::epaint::Vertex { pos: p, uv: pos2(uv[0] as f32, uv[1] as f32), color: col });
    }
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    m
}

pub fn ui(app: &mut SlideApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else {
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
        show.last_move = now;
        if !show.reading {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
        show.after_enter(&doc, now);
    }
    let mut keep = true;
    // Input.
    let (keys, clicked, secondary, moved) = ctx.input(|i| {
        let keys: Vec<(egui::Key, egui::Modifiers)> = i
            .events
            .iter()
            .filter_map(|e| if let egui::Event::Key { key, pressed: true, modifiers, .. } = e { Some((*key, *modifiers)) } else { None })
            .collect();
        (keys, i.pointer.primary_clicked(), i.pointer.secondary_clicked(), i.pointer.delta().length() > 0.0)
    });
    if moved {
        show.last_move = now;
    }
    let mut actions: Vec<ShowAction> = vec![];
    for (k, m) in keys {
        use egui::Key::*;
        match k {
            Escape | Minus => keep = false,
            ArrowRight | ArrowDown | Space | PageDown | N => {
                if show.ended {
                    keep = false;
                } else {
                    actions.push(show.state.next(&doc));
                }
            }
            Enter => {
                if let Ok(n) = show.goto.parse::<usize>() {
                    actions.push(show.state.goto(&doc, n.saturating_sub(1)));
                    show.goto.clear();
                } else if show.ended {
                    keep = false;
                } else {
                    actions.push(show.state.next(&doc));
                }
            }
            ArrowLeft | ArrowUp | Backspace | PageUp | P if !(k == P && m.command) => {
                show.ended = false;
                actions.push(show.state.prev(&doc));
            }
            P if m.command => show.pen = !show.pen,
            A if m.command => show.pen = false,
            Home => actions.push(show.state.goto(&doc, 0)),
            End => actions.push(show.state.goto(&doc, doc.slides.len().saturating_sub(1))),
            B | Period => show.blank = if show.blank == Some(Color32::BLACK) { None } else { Some(Color32::BLACK) },
            W | Comma => show.blank = if show.blank == Some(Color32::WHITE) { None } else { Some(Color32::WHITE) },
            E => show.strokes.clear(),
            Num0 | Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9 => {
                show.goto.push_str(k.name());
            }
            _ => {}
        }
    }
    let on_slide = !show.pen && !show.ended && show.blank.is_none() && !show.reading_bar_hit(&ctx);
    let media_click = if clicked && on_slide { media_hit(&ctx, ui, &show, &doc) } else { None };
    let link = if on_slide && media_click.is_none() { link_hit(&ctx, ui, &mut show, &doc) } else { None };
    if let Some((id, clip)) = media_click {
        app.media.toggle(&doc, id, &clip, crate::media::Owner::Show);
    } else if clicked && let Some(l) = &link {
        let last = show.last_viewed;
        match show.state.follow(&doc, &l.action, last) {
            LinkJump::Show(a) => actions.push(a),
            LinkJump::Open(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            LinkJump::Exit => keep = false,
            LinkJump::None => {}
        }
    } else if clicked && !show.pen && !show.reading_bar_hit(&ctx) {
        if show.ended {
            keep = false;
        } else if show.blank.is_some() {
            show.blank = None;
        } else {
            actions.push(show.state.next(&doc));
        }
    }
    if secondary && !show.ended {
        actions.push(show.state.prev(&doc));
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
        // Auto-advance (After: timings) or autoplay preview.
        let idle = now - show.step_end.max(show.slide_start);
        let cur_step = show.state.step;
        let next_auto = show.timeline(&doc).step_is_auto(cur_step);
        let n_steps = show.timeline(&doc).steps();
        let step_due = cur_step < n_steps && ((show.autoplay && idle > 0.3) || next_auto);
        if step_due || (!show.rehearse && show.state.auto_advance_due(&doc, idle) && !show.ended) {
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
    show_media(app, &mut show, &doc);
    // Draw.
    let full = ui.max_rect();
    let painter = ui.painter_at(full);
    painter.rect_filled(full, CornerRadius::ZERO, Color32::BLACK);
    let avail = if show.reading { Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - 34.0)) } else { full };
    let srect = fit(avail, &doc);
    let ppp = ctx.pixels_per_point();
    let size = ((srect.width() * ppp).round().max(1.0) as u32, (srect.height() * ppp).round().max(1.0) as u32);
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
        let animating = show.state.playing;
        let t = if animating { now - show.step_start } else { 0.0 };
        let media = crate::media::slide_media(&doc, idx);
        let hidden: Vec<ShapeId> = media
            .iter()
            .filter(|(id, _, clip)| {
                clip.hide_while_not_playing && !app.media.status(*id).is_some_and(|s| s.state != deckcraft_media::PlayState::Ended)
            })
            .map(|m| m.0)
            .collect();
        let key = Key { slide: idx, step, size, doc: std::sync::Arc::as_ptr(&doc) as usize, hidden: hidden.clone() };
        let tex = if !animating && show.tex.as_ref().is_some_and(|(k, _)| *k == key) {
            show.tex.as_ref().map(|(_, t)| t.clone())
        } else {
            let tl = show.timeline(&doc).clone();
            let hide = hidden.clone();
            let f = move |id: ShapeId| {
                let mut st = state_for(&tl, id, step, t);
                if hide.contains(&id) {
                    st.visible = false;
                }
                st
            };
            let threads =
                if cfg!(target_arch = "wasm32") { 0 } else { std::thread::available_parallelism().map(|n| n.get().min(8) as u16).unwrap_or(0) };
            let img = deckcraft_render::render_slide(
                &doc,
                idx,
                &RenderOpts {
                    scale: size.0 as f64 / doc.slide_size.width.max(1.0),
                    size: Some(size),
                    state: Some(&f),
                    threads,
                    ..Default::default()
                },
            );
            let ci = crate::textures::to_color_image(&img, false);
            let tex = match show.tex.take() {
                Some((_, mut h)) => {
                    h.set(ci, egui::TextureOptions::LINEAR);
                    h
                }
                None => ctx.load_texture("show", ci, egui::TextureOptions::LINEAR),
            };
            show.tex = Some((key, tex.clone()));
            Some(tex)
        };
        let mut drew = false;
        let morphing = show.trans.as_ref().is_some_and(|tr| tr.kind == "morph" && tr.from.is_some() && now - tr.start < tr.dur);
        let tl = if morphing { Some(show.timeline(&doc).clone()) } else { None };
        let morph = match (&show.trans, tl) {
            (Some(tr), Some(tl)) => {
                let p = ((now - tr.start) / tr.dur.max(0.01)).clamp(0.0, 1.0);
                let hidden_b = |id: ShapeId| {
                    let st = state_for(&tl, id, 0, 0.0);
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
            let img = deckcraft_render::render_blend(&doc, from, idx, m.mix, &m.shapes, &text, &opts);
            let ci = crate::textures::to_color_image(&img, false);
            let frame = match tr.morph.take() {
                Some(mut h) => {
                    h.set(ci, egui::TextureOptions::LINEAR);
                    h
                }
                None => ctx.load_texture("morph", ci, egui::TextureOptions::LINEAR),
            };
            painter.image(frame.id(), srect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            tr.morph = Some(frame);
            drew = true;
            ctx.request_repaint();
        } else if let (Some(tr), Some(new)) = (&show.trans, &tex) {
            let p = ((now - tr.start) / tr.dur.max(0.01)).clamp(0.0, 1.0);
            if p < 1.0 {
                for l in deckcraft_anim::transition_layers(&tr.kind, &tr.option, p) {
                    let mesh = match l.source {
                        Source::Old => match &tr.old {
                            Some(o) => quad_mesh(Some(o.id()), srect, &l, Color32::WHITE),
                            None => quad_mesh(None, srect, &l, Color32::BLACK),
                        },
                        Source::New => quad_mesh(Some(new.id()), srect, &l, Color32::WHITE),
                        Source::Black => quad_mesh(None, srect, &l, Color32::BLACK),
                        Source::White => quad_mesh(None, srect, &l, Color32::WHITE),
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
                painter.image(tex.id(), srect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            // Playing video: the current frame in its box, or full screen.
            let k = srect.width() / doc.slide_size.width.max(1.0) as f32;
            for (id, x, clip) in &media {
                if !clip.video || hidden.contains(id) {
                    continue;
                }
                let r = if clip.full_screen && app.media.is_playing(*id) {
                    full
                } else {
                    Rect::from_min_size(pos2(srect.min.x + x.x as f32 * k, srect.min.y + x.y as f32 * k), vec2(x.w as f32 * k, x.h as f32 * k))
                };
                crate::media::paint_frame(&painter, &app.media, *id, r);
            }
        }
        if animating {
            ctx.request_repaint();
        }
    }
    if let Some(c) = show.blank {
        painter.rect_filled(full, CornerRadius::ZERO, c);
    }
    // Pen.
    let resp = ui.interact(full, ui.id().with("show_pen"), if show.pen { Sense::drag() } else { Sense::hover() });
    if show.pen {
        if resp.drag_started() {
            show.strokes.push(vec![]);
        }
        if resp.dragged()
            && let (Some(p), Some(s)) = (resp.interact_pointer_pos(), show.strokes.last_mut())
        {
            s.push(pos2((p.x - srect.min.x) / srect.width(), (p.y - srect.min.y) / srect.height()));
        }
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    } else if link.is_some() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    } else if now - show.last_move > 2.5 && !show.reading {
        ctx.set_cursor_icon(egui::CursorIcon::None);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
    let pen_col = theme::to_color32(doc.show.pen_color);
    for s in &show.strokes {
        let pts: Vec<Pos2> = s.iter().map(|q| pos2(srect.min.x + q.x * srect.width(), srect.min.y + q.y * srect.height())).collect();
        painter.add(egui::Shape::line(pts, Stroke::new(3.0, pen_col)));
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
    } else if now - show.last_move < 2.5 {
        let bar = Rect::from_min_size(pos2(full.min.x + 20.0, full.max.y - 50.0), vec2(250.0, 34.0));
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
            show.pen = !show.pen;
        }
        if b(&mut child, "⬛").clicked() {
            show.blank = Some(Color32::BLACK);
        }
        if b(&mut child, "End").clicked() {
            keep = false;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
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
    if show.presenter && !show.reading && !show.ended {
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

/// Start the media of a newly shown slide: media from the previous slide stops unless it plays
/// across slides; media set to start automatically plays. Everything stops at the end.
fn show_media(app: &mut SlideApp, show: &mut Show, doc: &Presentation) {
    let cur = if show.ended { None } else { Some(show.state.slide) };
    if show.media_slide == cur {
        return;
    }
    show.media_slide = cur;
    let keep: Vec<ShapeId> = match cur {
        Some(_) => app.media.ids().into_iter().filter(|id| crate::media::find_clip(doc, *id).is_some_and(|c| c.play_across_slides)).collect(),
        None => vec![],
    };
    for id in app.media.ids() {
        if !keep.contains(&id) {
            app.media.stop(id);
        }
    }
    if let Some(i) = cur {
        for (id, _, clip) in crate::media::slide_media(doc, i) {
            if clip.autoplay && !keep.contains(&id) {
                app.media.play(doc, id, &clip, None, crate::media::Owner::Show);
            }
        }
    }
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

/// The link under the pointer: the topmost shape action or text hyperlink on the current slide
/// (a text link wins over its shape's action).
fn link_hit(ctx: &egui::Context, ui: &Ui, show: &mut Show, doc: &Presentation) -> Option<Hyperlink> {
    let p = ctx.input(|i| i.pointer.hover_pos())?;
    let full = ui.max_rect();
    let avail = if show.reading { Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - 34.0)) } else { full };
    let srect = fit(avail, doc);
    let k = (srect.width() / doc.slide_size.width.max(1.0) as f32) as f64;
    let (x, y) = ((p.x - srect.min.x) as f64 / k, (p.y - srect.min.y) as f64 / k);
    let i = show.state.slide;
    if show.links.as_ref().is_none_or(|(s, _)| *s != i) {
        show.links = Some((i, deckcraft_render::place_slide(doc, i).links));
    }
    let (_, links) = show.links.as_ref()?;
    links.iter().rev().find(|l| l.rect.x0 <= x && x <= l.rect.x1 && l.rect.y0 <= y && y <= l.rect.y1).map(|l| l.link.clone())
}

impl Show {
    fn reading_bar_hit(&self, ctx: &egui::Context) -> bool {
        // Clicks on the on-screen controls must not also advance.
        ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| {
            let r = ctx.content_rect();
            (self.reading && p.y > r.max.y - 34.0) || (!self.reading && p.y > r.max.y - 56.0 && p.x < r.min.x + 280.0)
        })
    }
}

fn presenter_window(ctx: &egui::Context, show: &mut Show, doc: &Presentation, now: f64, keep: &mut bool) {
    let cur = show.state.slide;
    let next = deckcraft_anim::show_order(doc).into_iter().find(|i| *i > cur);
    let cur_tex = show.tex.as_ref().map(|(_, t)| t.clone());
    if let Some(n) = next
        && show.next_tex.as_ref().is_none_or(|(k, _)| *k != n)
    {
        let img = deckcraft_render::render_slide(
            doc,
            n,
            &RenderOpts {
                scale: 480.0 / doc.slide_size.width.max(1.0),
                size: Some((480, (480.0 * doc.slide_size.height / doc.slide_size.width.max(1.0)) as u32)),
                ..Default::default()
            },
        );
        show.next_tex = Some((n, ctx.load_texture("presenter-next", crate::textures::to_color_image(&img, false), egui::TextureOptions::LINEAR)));
    }
    let next_tex = show.next_tex.as_ref().filter(|(k, _)| Some(*k) == next).map(|(_, t)| t.clone());
    let notes = doc.slides.get(cur).map(|s| s.notes_text()).unwrap_or_default();
    let elapsed = now - show.show_start;
    let mut actions = vec![];
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("deckcraft_presenter"),
        egui::ViewportBuilder::default().with_title("Presenter View — DeckCraft").with_inner_size([1100.0, 700.0]),
        |ui, _| {
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::from_gray(24)).inner_margin(egui::Margin::same(16))).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{:02}:{:02}:{:02}",
                            (elapsed / 3600.0) as u32,
                            (elapsed / 60.0) as u32 % 60,
                            elapsed as u32 % 60
                        ))
                        .size(22.0)
                        .color(Color32::WHITE),
                    );
                    ui.add_space(20.0);
                    ui.label(egui::RichText::new(format!("Slide {} of {}", cur + 1, doc.slides.len())).size(16.0).color(Color32::from_gray(200)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("End Show").clicked() {
                            *keep = false;
                        }
                        if ui.button("Next ▶").clicked() {
                            actions.push(show.state.next(doc));
                        }
                        if ui.button("◀ Previous").clicked() {
                            actions.push(show.state.prev(doc));
                        }
                    });
                });
                ui.add_space(10.0);
                let avail = ui.available_rect_before_wrap();
                let left = Rect::from_min_max(avail.min, pos2(avail.min.x + avail.width() * 0.62, avail.max.y));
                let right = Rect::from_min_max(pos2(left.max.x + 16.0, avail.min.y), avail.max);
                let cr = fit(Rect::from_min_max(left.min, pos2(left.max.x, left.min.y + left.height() * 0.8)), doc);
                let cr = Rect::from_min_size(left.min, cr.size());
                if let Some(t) = &cur_tex {
                    ui.painter().image(t.id(), cr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                let nr = Rect::from_min_size(
                    right.min + vec2(0.0, 22.0),
                    vec2(right.width(), right.width() * (doc.slide_size.height / doc.slide_size.width.max(1.0)) as f32),
                );
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
                let notes_rect = Rect::from_min_max(pos2(right.min.x, nr.max.y + 16.0), right.max);
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(notes_rect));
                egui::ScrollArea::vertical().show(&mut child, |ui| {
                    ui.label(egui::RichText::new(if notes.is_empty() { "No notes." } else { &notes }).size(18.0).color(Color32::from_gray(230)));
                });
            });
            if ui.input(|i| i.viewport().close_requested()) {
                show.presenter = false;
            }
        },
    );
    for a in actions {
        if !show.apply(a, doc, now) {
            *keep = false;
        }
    }
}
