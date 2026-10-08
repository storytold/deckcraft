//! The slide canvas: the rendered slide, zoom and fit, pointer input routed to the engine's tools,
//! and the overlays drawn on top (selection box and handles, smart guides, marquee, drawing
//! previews, the text caret and text selection).

use deckcraft_engine::tools::{Hit, box_handles};
use deckcraft_engine::{Mods, PointerEvent, PointerKind, Target, ToolKind};
use deckcraft_geom::{Point, Xfrm};
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::theme::{self, Tokens};

/// Slide-to-screen mapping.
#[derive(Clone, Copy, Debug)]
pub struct Xf {
    pub origin: Pos2,
    pub scale: f32,
}

impl Xf {
    pub fn to_screen(&self, p: Point) -> Pos2 {
        pos2(self.origin.x + p.x as f32 * self.scale, self.origin.y + p.y as f32 * self.scale)
    }
    pub fn to_slide(&self, p: Pos2) -> Point {
        Point::new(((p.x - self.origin.x) / self.scale) as f64, ((p.y - self.origin.y) / self.scale) as f64)
    }
}

fn mods(ui: &Ui) -> Mods {
    ui.input(|i| Mods { shift: i.modifiers.shift, alt: i.modifiers.alt, cmd: i.modifiers.command || i.modifiers.ctrl })
}

/// The slide's rect on screen for the current zoom.
pub fn layout(app: &SlideApp, avail: Rect) -> (Rect, f32) {
    let Some(d) = app.session.active() else { return (avail, 1.0) };
    let (sw, sh) = (d.doc.slide_size.width as f32, d.doc.slide_size.height as f32);
    let margin = 28.0;
    let fit = ((avail.width() - margin * 2.0) / sw).min((avail.height() - margin * 2.0) / sh).max(0.05);
    let scale = app.ui.zoom.unwrap_or(fit);
    let size = vec2(sw * scale, sh * scale);
    let mut r = Rect::from_center_size(avail.center(), size);
    if size.x > avail.width() {
        r = Rect::from_min_size(pos2(avail.min.x + margin, r.min.y), size);
    }
    if size.y > avail.height() {
        r = Rect::from_min_size(pos2(r.min.x, avail.min.y + margin), size);
    }
    (r, scale)
}

pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let avail = ui.available_rect_before_wrap();
    app.canvas_rect = Some(avail);
    let Some(st) = app.session.active() else { return };
    if st.doc.slides.is_empty() && st.selection.target == Target::Slides {
        let resp = ui.allocate_rect(avail, Sense::click());
        ui.painter().text(avail.center(), Align2::CENTER_CENTER, "Click to add first slide", theme::font(22.0), t.text_dim);
        if resp.clicked() {
            let _ = app.run("slide.new", json!({"layout": "title"}));
        }
        return;
    }
    let (content_rect, scale) = layout(app, avail);
    // Scroll when zoomed beyond the window.
    let scroll_id = ui.id().with("canvas_scroll");
    let mut offset: egui::Vec2 = ui.data_mut(|d| d.get_temp(scroll_id).unwrap_or_default());
    let overflow = vec2((content_rect.width() + 56.0 - avail.width()).max(0.0), (content_rect.height() + 56.0 - avail.height()).max(0.0));
    let resp = ui.allocate_rect(avail, Sense::click_and_drag());
    let (wheel, zoom_delta, pinch_pos) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
    if resp.hovered() {
        if (zoom_delta - 1.0).abs() > 1e-3 {
            let cur = app.ui.zoom.unwrap_or(scale);
            app.ui.zoom = Some((cur * zoom_delta).clamp(0.1, 4.0));
            let _ = pinch_pos;
        } else if overflow != egui::Vec2::ZERO {
            offset -= wheel;
        } else if wheel.y.abs() > 18.0 && app.session.tool.marquee.is_none() && !app.session.tool.dragging() {
            // Wheel over a fitted slide pages through slides (like PowerPoint).
            let id = if wheel.y < 0.0 { "slide.next" } else { "slide.previous" };
            let gate = ui.id().with("wheel_gate");
            let last: f64 = ui.data_mut(|d| d.get_temp(gate).unwrap_or(0.0));
            let now = ui.input(|i| i.time);
            if now - last > 0.35 {
                ui.data_mut(|d| d.insert_temp(gate, now));
                let _ = app.run(id, json!({}));
            }
        }
    }
    offset.x = offset.x.clamp(0.0, overflow.x);
    offset.y = offset.y.clamp(0.0, overflow.y);
    ui.data_mut(|d| d.insert_temp(scroll_id, offset));
    let slide_rect = content_rect.translate(-offset);
    app.slide_rect = Some(slide_rect);
    app.canvas_scale = scale;
    let xf = Xf { origin: slide_rect.min, scale };
    let painter = ui.painter_at(avail);
    // Shadow + slide.
    painter.rect_filled(slide_rect.translate(vec2(0.0, 1.5)).expand(1.0), CornerRadius::same(2), t.shadow);
    let ppp = ui.ctx().pixels_per_point();
    let px = ((slide_rect.width() * ppp).round().max(1.0) as u32, (slide_rect.height() * ppp).round().max(1.0) as u32);
    let st = app.session.active();
    let tex = match st.map(|d| (d.selection.target, d)) {
        Some((Target::Slides, d)) => {
            let idx = d.selection.slide;
            let doc = d.doc.clone();
            app.textures.slide(ui.ctx(), &doc, idx, px, true, 0, app.ui.grayscale)
        }
        Some((Target::Master { master }, d)) => master_tex(ui.ctx(), &d.doc, master, None, px),
        Some((Target::Layout { master, layout }, d)) => master_tex(ui.ctx(), &d.doc, master, Some(layout), px),
        None => None,
    };
    if let Some(tex) = tex {
        painter.image(tex.id(), slide_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    app.perf.render_ms = app.textures.last_render_ms;
    media_frames(app, &painter, xf);
    if app.ui.gridlines {
        let step = 72.0 / 2.0 * scale;
        let mut x = slide_rect.min.x + step;
        while x < slide_rect.max.x {
            painter.line_segment([pos2(x, slide_rect.min.y), pos2(x, slide_rect.max.y)], Stroke::new(0.5, Color32::from_black_alpha(40)));
            x += step;
        }
        let mut y = slide_rect.min.y + step;
        while y < slide_rect.max.y {
            painter.line_segment([pos2(slide_rect.min.x, y), pos2(slide_rect.max.x, y)], Stroke::new(0.5, Color32::from_black_alpha(40)));
            y += step;
        }
    }
    if app.ui.guides {
        let c = slide_rect.center();
        painter.line_segment(
            [pos2(c.x, slide_rect.min.y), pos2(c.x, slide_rect.max.y)],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(120, 120, 120, 160)),
        );
        painter.line_segment(
            [pos2(slide_rect.min.x, c.y), pos2(slide_rect.max.x, c.y)],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(120, 120, 120, 160)),
        );
    }
    if app.ui.ruler {
        ruler(&painter, avail, slide_rect, scale, &t);
    }
    pointer(app, ui, &resp, xf);
    overlays(app, ui, &painter, xf, &t);
    media_bar(app, ui, xf);
    context_menu(app, &resp);
}

/// Screen rect of a slide-space box (rotation ignored).
fn screen_rect(xf: Xf, x: &Xfrm) -> Rect {
    Rect::from_min_max(xf.to_screen(Point::new(x.x, x.y)), xf.to_screen(Point::new(x.x + x.w, x.y + x.h)))
}

/// Video playing in the editor: its current frame over the poster.
fn media_frames(app: &SlideApp, painter: &egui::Painter, xf: Xf) {
    let Some(d) = app.session.active() else { return };
    if d.selection.target != Target::Slides || !app.media.any_active() {
        return;
    }
    for (id, x, clip) in crate::media::slide_media(&d.doc, d.selection.slide) {
        if clip.video {
            crate::media::paint_frame(painter, &app.media, id, screen_rect(xf, &x));
        }
    }
}

/// The play/pause and timeline bar under a selected audio or video (PowerPoint shows it on
/// selection or hover).
fn media_bar(app: &mut SlideApp, ui: &mut Ui, xf: Xf) {
    let Some(d) = app.session.active() else { return };
    if d.selection.target != Target::Slides || d.selection.text.is_some() || app.session.tool.dragging() {
        return;
    }
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let media = crate::media::slide_media(&d.doc, d.selection.slide);
    let pick = media.iter().find(|(id, _, _)| d.selection.shapes.len() == 1 && d.selection.shapes[0] == *id).or_else(|| {
        media.iter().find(|(id, x, _)| {
            let r = screen_rect(xf, x);
            app.media.status(*id).is_some() || pointer.is_some_and(|p| r.expand2(vec2(0.0, 40.0)).contains(p))
        })
    });
    let Some((id, x, clip)) = pick.cloned() else { return };
    let doc = d.doc.clone();
    let r = screen_rect(xf, &x);
    let w = r.width().max(230.0);
    let bar = Rect::from_min_size(pos2(r.center().x - w / 2.0, r.max.y + 6.0), vec2(w, 26.0));
    let status = app.media.status(id);
    match crate::media::control_bar(ui, bar, status.as_ref(), &clip) {
        Some(crate::media::BarAction::Toggle) => app.media.toggle(&doc, id, &clip, crate::media::Owner::Editor),
        Some(crate::media::BarAction::Seek(t)) => app.media.seek(&doc, id, &clip, t, crate::media::Owner::Editor),
        Some(crate::media::BarAction::Nudge(dt)) => {
            let pos = status.map(|s| s.position).unwrap_or(0.0);
            app.media.seek(&doc, id, &clip, (pos + dt).max(0.0), crate::media::Owner::Editor);
        }
        None => {}
    }
}

fn master_tex(
    ctx: &egui::Context,
    doc: &deckcraft_model::Presentation,
    master: usize,
    layout: Option<usize>,
    px: (u32, u32),
) -> Option<egui::TextureHandle> {
    let img = deckcraft_render::render_layout(
        doc,
        master,
        layout,
        &deckcraft_render::RenderOpts { scale: px.0 as f64 / doc.slide_size.width.max(1.0), edit: true, size: Some(px), ..Default::default() },
    );
    Some(ctx.load_texture("master-canvas", crate::textures::to_color_image(&img, false), egui::TextureOptions::LINEAR))
}

fn ruler(p: &egui::Painter, avail: Rect, slide: Rect, scale: f32, t: &Tokens) {
    let top = Rect::from_min_max(avail.min, pos2(avail.max.x, avail.min.y + 16.0));
    let left = Rect::from_min_max(avail.min, pos2(avail.min.x + 16.0, avail.max.y));
    p.rect_filled(top, CornerRadius::ZERO, t.card);
    p.rect_filled(left, CornerRadius::ZERO, t.card);
    let inch = 72.0 * scale;
    let mut k = 0;
    loop {
        let x = slide.min.x + k as f32 * inch / 4.0;
        if x > slide.max.x {
            break;
        }
        let h = if k % 4 == 0 {
            8.0
        } else if k % 2 == 0 {
            5.0
        } else {
            3.0
        };
        p.line_segment([pos2(x, top.max.y - h), pos2(x, top.max.y)], Stroke::new(1.0, t.text_faint));
        if k % 4 == 0 {
            p.text(pos2(x + 2.0, top.min.y + 1.0), Align2::LEFT_TOP, format!("{}", k / 4), theme::font(8.0), t.text_dim);
        }
        k += 1;
    }
    let mut k = 0;
    loop {
        let y = slide.min.y + k as f32 * inch / 4.0;
        if y > slide.max.y {
            break;
        }
        let w = if k % 4 == 0 {
            8.0
        } else if k % 2 == 0 {
            5.0
        } else {
            3.0
        };
        p.line_segment([pos2(left.max.x - w, y), pos2(left.max.x, y)], Stroke::new(1.0, t.text_faint));
        k += 1;
    }
}

fn pointer(app: &mut SlideApp, ui: &Ui, resp: &egui::Response, xf: Xf) {
    let tol = (4.0 / xf.scale) as f64;
    let m = mods(ui);
    let Some(pos) = resp.interact_pointer_pos().or(resp.hover_pos()) else { return };
    let p = xf.to_slide(pos);
    let ev = |kind| PointerEvent { kind, x: p.x, y: p.y, mods: m, tol };
    let primary = ui.input(|i| i.pointer.primary_pressed());
    if resp.hovered() && !app.session.tool.dragging() {
        let _ = app.session.pointer(ev(PointerKind::Move));
    }
    let r = if resp.triple_clicked() {
        app.session.pointer(ev(PointerKind::TripleClick))
    } else if resp.double_clicked() {
        app.session.pointer(ev(PointerKind::DoubleClick))
    } else if primary && resp.hovered() && !resp.secondary_clicked() && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary)) {
        app.session.pointer(ev(PointerKind::Down))
    } else if resp.dragged_by(egui::PointerButton::Primary) || (app.session.tool.dragging() && ui.input(|i| i.pointer.primary_down())) {
        app.session.pointer(ev(PointerKind::Drag))
    } else if app.session.tool.dragging() && ui.input(|i| i.pointer.primary_released() || !i.pointer.primary_down()) {
        app.session.pointer(ev(PointerKind::Up))
    } else {
        Ok(serde_json::Value::Null)
    };
    if let Err(e) = r {
        app.set_status(e.to_string());
    }
    // Cursor.
    let cursor = match (&app.session.tool.kind, &app.session.tool.hover) {
        (ToolKind::Shape { .. }, _) => egui::CursorIcon::Crosshair,
        (ToolKind::TextBox, _) => egui::CursorIcon::Text,
        (ToolKind::Ink { .. }, _) => egui::CursorIcon::Crosshair,
        (_, Some(Hit::Resize { handle, .. })) => match handle {
            0 | 4 => egui::CursorIcon::ResizeNwSe,
            2 | 6 => egui::CursorIcon::ResizeNeSw,
            1 | 5 => egui::CursorIcon::ResizeVertical,
            _ => egui::CursorIcon::ResizeHorizontal,
        },
        (_, Some(Hit::Rotate { .. })) => egui::CursorIcon::Alias,
        (_, Some(Hit::Adjust { .. } | Hit::LineEnd { .. })) => egui::CursorIcon::PointingHand,
        (_, Some(Hit::Shape { text: true, .. })) => {
            if app.session.active().is_some_and(|d| d.selection.text.is_some()) {
                egui::CursorIcon::Text
            } else {
                egui::CursorIcon::Move
            }
        }
        (_, Some(Hit::Shape { .. })) => egui::CursorIcon::Move,
        _ => egui::CursorIcon::Default,
    };
    if resp.hovered() {
        ui.ctx().set_cursor_icon(cursor);
    }
    if app.session.tool.dragging() {
        ui.ctx().request_repaint();
    }
}

fn poly(p: &egui::Painter, pts: [Pos2; 4], stroke: Stroke) {
    for i in 0..4 {
        p.line_segment([pts[i], pts[(i + 1) % 4]], stroke);
    }
}

fn overlays(app: &mut SlideApp, ui: &Ui, painter: &egui::Painter, xf: Xf, t: &Tokens) {
    let Some(st) = app.session.active() else { return };
    let sel_stroke = Stroke::new(1.0, t.selection);
    let editing = st.selection.text.clone();
    for id in &st.selection.shapes {
        let Some(sh) = st.shape(*id) else { continue };
        let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
        let a = x.affine();
        let corners = [Point::new(0.0, 0.0), Point::new(x.w, 0.0), Point::new(x.w, x.h), Point::new(0.0, x.h)].map(|q| xf.to_screen(a * q));
        if sh.is_line() {
            let (p0, p1) = (xf.to_screen(a * Point::new(0.0, 0.0)), xf.to_screen(a * Point::new(x.w, x.h)));
            let glued = match sh.kind {
                deckcraft_model::ShapeKind::Connector { start, end } => [start.is_some(), end.is_some()],
                _ => [false, false],
            };
            for (q, g) in [p0, p1].into_iter().zip(glued) {
                // Glued ends are filled with the accent, like attached connector ends.
                let fill = if g { t.accent } else { t.handle_fill };
                painter.circle(q, 4.5, fill, Stroke::new(1.0, if g { t.handle_fill } else { t.handle_stroke }));
            }
            continue;
        }
        let editing_this = editing.as_ref().is_some_and(|e| e.shape == *id && !e.notes);
        if editing_this {
            // Dashed box while editing text.
            for i in 0..4 {
                let (s, e) = (corners[i], corners[(i + 1) % 4]);
                painter.add(egui::Shape::dashed_line(&[s, e], Stroke::new(1.0, t.selection), 4.0, 3.0));
            }
        } else {
            poly(painter, corners, sel_stroke);
        }
        if st.selection.shapes.len() == 1 || !editing_this {
            let (hs, rot) = box_handles(&x);
            let rot_s = xf.to_screen(rot);
            if !editing_this && sh.ph.as_ref().is_none_or(|_| true) {
                let top_mid = xf.to_screen(hs[1]);
                painter.line_segment([top_mid, rot_s], sel_stroke);
                rotate_handle(painter, rot_s, t);
            }
            for h in hs {
                painter.circle(xf.to_screen(h), 4.5, t.handle_fill, Stroke::new(1.0, t.handle_stroke));
            }
            let geo = deckcraft_render::shape_geometry(sh, x.w, x.h);
            for hd in &geo.handles {
                let q = xf.to_screen(a * hd.pos);
                painter.circle(q, 4.5, Color32::from_rgb(0xFF, 0xD2, 0x3F), Stroke::new(1.0, Color32::from_rgb(0x8A, 0x6A, 0x00)));
            }
        }
    }
    // Caret / text selection.
    if let Some(ts) = &editing
        && !ts.notes
        && ts.cell.is_none()
        && let Some(sh) = st.shape(ts.shape)
        && let Some(l) = deckcraft_engine::cmd::text::layout_for(st, ts)
    {
        let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
        let a = x.affine();
        let rot = if l.rotation != 0.0 {
            let c = l.inner.center().to_vec2();
            deckcraft_geom::Affine::translate(c) * deckcraft_geom::Affine::rotate(l.rotation.to_radians()) * deckcraft_geom::Affine::translate(-c)
        } else {
            deckcraft_geom::Affine::IDENTITY
        };
        let m = a * rot;
        let (s0, s1) = ts.ordered();
        for r in l.selection_rects(deckcraft_text::Pos { para: s0.0, ch: s0.1 }, deckcraft_text::Pos { para: s1.0, ch: s1.1 }) {
            let pts = [Point::new(r.x0, r.y0), Point::new(r.x1, r.y0), Point::new(r.x1, r.y1), Point::new(r.x0, r.y1)].map(|q| xf.to_screen(m * q));
            painter.add(egui::Shape::convex_polygon(pts.to_vec(), t.text_selection, Stroke::NONE));
        }
        if !ts.is_range() {
            let blink = (ui.input(|i| i.time) * 1.6).fract() < 0.6;
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
            if blink && let Some((cx, top, bot)) = l.caret(deckcraft_text::Pos { para: ts.caret.0, ch: ts.caret.1 }) {
                let (p0, p1) = (xf.to_screen(m * Point::new(cx, top)), xf.to_screen(m * Point::new(cx, bot)));
                painter.line_segment([p0, p1], Stroke::new(1.5, t.caret));
            }
        }
    }
    // Table cell editing: outline the cell.
    if let Some(ts) = &editing
        && let (Some((r, c)), Some(sh)) = (ts.cell, st.shape(ts.shape))
        && let deckcraft_model::ShapeKind::Table(tb) = &sh.kind
    {
        let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
        let heights = deckcraft_engine::tools::table_rows(st, tb);
        let y0: f64 = heights.iter().take(r).sum();
        let x0: f64 = tb.cols.iter().take(c).sum();
        let (w, h) = (tb.cols.get(c).copied().unwrap_or(0.0), heights.get(r).copied().unwrap_or(0.0));
        let a = x.affine();
        let pts = [Point::new(x0, y0), Point::new(x0 + w, y0), Point::new(x0 + w, y0 + h), Point::new(x0, y0 + h)].map(|q| xf.to_screen(a * q));
        poly(painter, pts, Stroke::new(2.0, t.accent));
    }
    // Smart guides, marquee, previews.
    for g in &app.session.tool.guides {
        let (a, b) =
            if g.vertical { (Point::new(g.pos, g.from), Point::new(g.pos, g.to)) } else { (Point::new(g.from, g.pos), Point::new(g.to, g.pos)) };
        painter.add(egui::Shape::dashed_line(&[xf.to_screen(a), xf.to_screen(b)], Stroke::new(1.0, t.guide), 5.0, 3.0));
    }
    if let Some(m) = app.session.tool.marquee {
        let r = Rect::from_two_pos(xf.to_screen(Point::new(m.x0, m.y0)), xf.to_screen(Point::new(m.x1, m.y1)));
        painter.rect(
            r,
            CornerRadius::ZERO,
            Color32::from_rgba_unmultiplied(80, 130, 230, 30),
            Stroke::new(1.0, Color32::from_rgb(80, 130, 230)),
            egui::StrokeKind::Inside,
        );
    }
    if let Some(pv) = app.session.tool.preview {
        draw_preview(app, painter, xf, pv, t);
    }
    // Freeform being drawn.
    let path = &app.session.tool.path_preview;
    if path.len() > 1 {
        let pts: Vec<Point> = if app.session.tool.path_smooth && path.len() > 2 {
            let d = deckcraft_engine::cmd::shape::freeform_path(path, false, true);
            let mut v = vec![];
            let bp = deckcraft_render::parse_path(&d);
            deckcraft_geom::preset_flatten(&bp, &mut |el| match el {
                deckcraft_geom::PathEl::MoveTo(q) | deckcraft_geom::PathEl::LineTo(q) => v.push(q),
                _ => {}
            });
            v
        } else {
            path.clone()
        };
        painter.add(egui::Shape::line(pts.iter().map(|q| xf.to_screen(*q)).collect(), Stroke::new(1.5, t.accent)));
        if app.session.tool.path_closing
            && let Some(s0) = path.first()
        {
            painter.circle(xf.to_screen(*s0), 5.0, t.accent, Stroke::new(1.5, t.handle_fill));
        }
    }
    // Connection sites of the shape under a connector end, and the site it would glue to.
    for q in &app.session.tool.sites {
        let c = xf.to_screen(*q);
        painter.circle(c, 3.5, t.handle_fill, Stroke::new(1.0, t.handle_stroke));
    }
    if let Some(g) = app.session.tool.glue {
        painter.circle(xf.to_screen(g), 5.0, t.accent, Stroke::new(1.5, t.handle_fill));
    }
    if app.session.tool.ink_preview.len() > 1
        && let ToolKind::Ink { color, width, mode } = &app.session.tool.kind
    {
        let pts: Vec<Pos2> = app.session.tool.ink_preview.iter().map(|(x, y, _)| xf.to_screen(Point::new(*x, *y))).collect();
        let c = theme::to_color32(*color);
        let c = if mode == "highlighter" { c.gamma_multiply(0.45) } else { c };
        painter.add(egui::Shape::line(pts, Stroke::new((*width as f32 * xf.scale).max(1.0), c)));
    }
}

fn rotate_handle(p: &egui::Painter, c: Pos2, t: &Tokens) {
    p.circle_filled(c, 6.0, t.handle_fill);
    p.add(egui::Shape::Path(egui::epaint::PathShape {
        points: (0..=20)
            .map(|i| {
                let a = -0.4 + i as f32 / 20.0 * 5.2;
                pos2(c.x + 4.0 * a.cos(), c.y + 4.0 * a.sin())
            })
            .collect(),
        closed: false,
        fill: Color32::TRANSPARENT,
        stroke: Stroke::new(1.3, t.handle_stroke).into(),
    }));
}

fn draw_preview(app: &SlideApp, p: &egui::Painter, xf: Xf, x: Xfrm, t: &Tokens) {
    let preset = match &app.session.tool.kind {
        ToolKind::Shape { preset } => preset.clone(),
        _ => "rect".into(),
    };
    if deckcraft_geom::preset::is_line_like(&preset) {
        let a = x.affine();
        p.line_segment([xf.to_screen(a * Point::new(0.0, 0.0)), xf.to_screen(a * Point::new(x.w, x.h))], Stroke::new(1.5, t.accent));
        return;
    }
    let r = Rect::from_two_pos(xf.to_screen(Point::new(x.x, x.y)), xf.to_screen(Point::new(x.x + x.w, x.y + x.h)));
    if r.width() < 1.0 || r.height() < 1.0 {
        return;
    }
    crate::ribbon::paint_preset(p, r, &preset, t.accent.gamma_multiply(0.25), t.accent);
}

fn context_menu(app: &mut SlideApp, resp: &egui::Response) {
    resp.context_menu(|ui| {
        let has_sel = app.session.active().is_some_and(|d| !d.selection.shapes.is_empty());
        for (l, id) in [("Cut", "edit.cut"), ("Copy", "edit.copy"), ("Paste", "edit.paste")] {
            if ui.add_enabled(crate::ribbon::enabled(app, id), egui::Button::new(l)).clicked() {
                let _ = app.run(id, json!({}));
                ui.close();
            }
        }
        ui.separator();
        if has_sel {
            if ui.button("Edit Text").clicked() {
                let _ = app.run("text.edit", json!({}));
                ui.close();
            }
            ui.menu_button("Bring to Front", |ui| {
                crate::ribbon::menu_item(app, ui, crate::icons::Icon::BringToFront, "Bring to Front", "arrange.bringToFront", json!({}));
                crate::ribbon::menu_item(app, ui, crate::icons::Icon::BringForward, "Bring Forward", "arrange.bringForward", json!({}));
            });
            ui.menu_button("Send to Back", |ui| {
                crate::ribbon::menu_item(app, ui, crate::icons::Icon::SendToBack, "Send to Back", "arrange.sendToBack", json!({}));
                crate::ribbon::menu_item(app, ui, crate::icons::Icon::SendBackward, "Send Backward", "arrange.sendBackward", json!({}));
            });
            ui.menu_button("Group", |ui| {
                crate::ribbon::menu_item(app, ui, crate::icons::Icon::Group, "Group", "arrange.group", json!({}));
                crate::ribbon::menu_item(app, ui, crate::icons::Icon::Ungroup, "Ungroup", "arrange.ungroup", json!({}));
            });
            if ui.button("Link…").clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("hyperlink"));
                ui.close();
            }
            if ui.button("Edit Alt Text…").clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("altText"));
                ui.close();
            }
            if ui.button("Set as Default Shape").clicked() {
                let _ = app.run("shape.setDefault", json!({}));
                ui.close();
            }
            if ui.button("Format Shape…").clicked() {
                let _ = app.run("view.pane", json!({"pane": "format"}));
                ui.close();
            }
        } else {
            ui.menu_button("Layout", |ui| {
                let layouts: Vec<(String, u32)> = app
                    .session
                    .active()
                    .and_then(|d| d.doc.masters.first().map(|m| m.layouts.iter().map(|l| (l.name.clone(), l.id.0)).collect()))
                    .unwrap_or_default();
                for (n, id) in layouts {
                    if ui.button(n).clicked() {
                        let _ = app.run("slide.layout", json!({"layout": id}));
                        ui.close();
                    }
                }
            });
            if ui.button("Reset Slide").clicked() {
                let _ = app.run("slide.reset", json!({}));
                ui.close();
            }
            if ui.button("Format Background…").clicked() {
                let _ = app.run("view.pane", json!({"pane": "background"}));
                ui.close();
            }
            if ui.button("New Comment").clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("comment"));
                ui.close();
            }
        }
    });
}
