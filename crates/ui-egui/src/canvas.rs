//! The slide canvas: the rendered slide, zoom and fit, pointer input routed to the engine's tools,
//! and the overlays drawn on top (selection box and handles, smart guides, marquee, drawing
//! previews, the text caret and text selection).
//!
//! As in PowerPoint's Normal view, shapes, pictures and text that sit partly or wholly off the
//! slide are drawn on the area around it (the pasteboard), where they can be selected and moved;
//! the slide show, exports and thumbnails clip to the slide.

use deckcraft_engine::tools::{Hit, box_handles};
use deckcraft_engine::{Mods, PointerEvent, PointerKind, Target, ToolKind};
use deckcraft_geom::{Point, Xfrm};
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::theme::{self, Tokens};

mod context_menu;
mod guides;
mod ime;
mod input;
mod media;
mod overlays;
mod rulers;
mod scroll;

use context_menu::context_menu;
use guides::{draw_grid, draw_guides, grid_line_step};
pub use ime::ime_area;
use ime::ime_output;
#[cfg(test)]
use input::resize_cursor;
pub(crate) use input::{page_flick, trackpad_twist};
use input::{pointer, trackpad_scroll, wheel_slide_target};
use media::{gif_buttons, media_bar, media_frames};
use overlays::overlays;
#[cfg(test)]
pub(crate) use overlays::text_marks;
#[cfg(test)]
pub(crate) use rulers::ruler_steps;
use rulers::{ruler, ruler_markers};
use scroll::{scroll_bar, visible_region};

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
    let page = deckcraft_engine::page_size(d);
    // Loaded sizes are repaired; a zero or non-finite side would still make the fit infinite.
    let side = |v: f64| if v.is_finite() { (v as f32).max(1.0) } else { 1.0 };
    let (sw, sh) = (side(page.width), side(page.height));
    let margin = 28.0;
    let fit = ((avail.width() - margin * 2.0) / sw).min((avail.height() - margin * 2.0) / sh).max(0.05);
    // A zoom from outside the editor's own controls (settings, the control protocol) is kept in range.
    let scale = app.ui.zoom.filter(|z| z.is_finite() && *z > 0.0).map_or(fit, |z| z.clamp(*crate::ZOOM_RANGE.start(), *crate::ZOOM_RANGE.end()));
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

/// The mouse wheel or a trackpad flick turns slides when the canvas has nothing to scroll.
fn wheel_navigation(app: &mut SlideApp, ui: &mut Ui, carry_id: egui::Id, doc_uid: u64) {
    if app.anim_preview_active() || app.session.tool.marquee.is_some() || app.session.tool.dragging() {
        return;
    }
    let (_, mut carry): (u64, f32) = ui.data_mut(|d| d.get_temp(carry_id).unwrap_or_default());
    let wheel_step = app.session.active().and_then(|d| {
        let current = d.selection.slide;
        ui.input(|i| wheel_slide_target(&i.events, current, d.doc.slides.len(), &mut carry)).map(|target| (current, target))
    });
    ui.data_mut(|d| d.insert_temp(carry_id, (doc_uid, carry)));
    if let Some((current, index)) = wheel_step {
        if index != current {
            let _ = app.run("slide.go", json!({"index": index}));
        }
    } else if let Some(next) = page_flick(ui.input(|i| trackpad_scroll(&i.events))) {
        // Point-based trackpad gestures keep their cooldown. Raw input avoids reading
        // the smoothed tail of a discrete mouse notch as another page turn.
        let gate = ui.scope_id().with("wheel_gate");
        let last: f64 = ui.data_mut(|d| d.get_temp(gate).unwrap_or(f64::NEG_INFINITY));
        let now = ui.input(|i| i.time);
        if now - last > 0.35 {
            ui.data_mut(|d| d.insert_temp(gate, now));
            let _ = app.run(if next { "slide.next" } else { "slide.previous" }, json!({}));
        }
    }
}

fn empty_deck_prompt(app: &mut SlideApp, ui: &mut Ui, avail: Rect, t: &Tokens) {
    let resp = ui.allocate_rect(avail, Sense::click());
    ui.painter().text(avail.center(), Align2::CENTER_CENTER, "Click to add first slide", theme::font(22.0), t.text_dim);
    if resp.clicked() {
        let _ = app.run("slide.new", json!({"layout": "title"}));
    }
}

fn reset_wheel_carry(ui: &Ui, carry_id: egui::Id, doc_uid: u64) {
    ui.data_mut(|d| {
        if d.get_temp::<(u64, f32)>(carry_id).is_none_or(|(uid, _)| uid != doc_uid) {
            d.insert_temp(carry_id, (doc_uid, 0.0_f32));
        }
    });
}

/// The scroll offset that keeps the slide point under `pointer` (or the canvas middle) fixed
/// when the scale changed since the last frame.
fn anchored_offset(last: Option<(Rect, f32)>, content_rect: Rect, scale: f32, pointer: Option<Pos2>, avail: Rect) -> Option<egui::Vec2> {
    let (last_rect, last_scale) = last?;
    if (scale - last_scale).abs() <= 1e-6 {
        return None;
    }
    let anchor = pointer.unwrap_or(avail.center());
    let at = (anchor - last_rect.min) / last_scale;
    Some(content_rect.min + at * scale - anchor)
}

fn slide_gifs(app: &mut SlideApp, now: f64) -> Vec<crate::media::Gif> {
    match app.session.active() {
        Some(d) if d.selection.target == Target::Slides => app.media.gifs(d.uid, &d.doc, d.selection.slide, now, &app.session.gif_paused),
        _ => vec![],
    }
}

/// Paint the editing canvas, including rulers, selection handles, overlays, and current
/// zoom/scroll state.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let avail = ui.available_rect_before_wrap();
    app.canvas_rect = Some(avail);
    let Some(st) = app.session.active() else { return };
    // The leftover fraction of a wheel line belongs to one presentation. Drop it whenever another
    // one is drawn here, hovered or not, so switching away and back starts from zero.
    let carry_id = ui.scope_id().with("wheel_carry");
    let doc_uid = st.uid;
    reset_wheel_carry(ui, carry_id, doc_uid);
    if st.doc.slides.is_empty() && st.selection.target == Target::Slides {
        empty_deck_prompt(app, ui, avail, &t);
        return;
    }
    // Scroll when zoomed beyond the window.
    let scroll_id = ui.scope_id().with("canvas_scroll");
    let last_id = scroll_id.with("last");
    let mut offset: egui::Vec2 = ui.data_mut(|d| d.get_temp(scroll_id).unwrap_or_default());
    let last: Option<(Rect, f32)> = ui.data_mut(|d| d.get_temp(last_id));
    let resp = ui.allocate_rect(avail, Sense::click_and_drag());
    let (wheel, zoom_delta, hover) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
    let zooming = resp.hovered() && (zoom_delta - 1.0).abs() > 1e-3;
    if zooming {
        let cur = app.ui.zoom.unwrap_or_else(|| layout(app, avail).1);
        app.ui.zoom = Some((cur * zoom_delta).clamp(0.1, 4.0));
    }
    let (content_rect, scale) = layout(app, avail);
    // A new zoom keeps the slide point under the pointer (wheel, pinch) or under the middle of the
    // canvas (Zoom In/Out, the slider) where it was, instead of growing from the corner (#106).
    if let Some(o) = anchored_offset(last, content_rect, scale, zooming.then_some(hover).flatten(), avail) {
        offset = o;
    }
    let overflow = vec2((content_rect.width() + 56.0 - avail.width()).max(0.0), (content_rect.height() + 56.0 - avail.height()).max(0.0));
    // Scroll bars on the axes that overflow (#1157). They sit over the canvas edge and take their
    // presses before the slide does; the rulers keep their strips.
    let inset = if app.ui.ruler { 16.0 } else { 0.0 };
    let mut on_bar = false;
    let mut bar_active = [false; 2];
    for axis in 0..2 {
        let Some((track, thumb, travel)) = scroll_bar(avail, inset, overflow, offset, axis) else { continue };
        let bar_id = scroll_id.with(("bar", axis));
        let bar = ui.interact(track, bar_id, Sense::click_and_drag());
        on_bar |= bar.hovered();
        bar_active[axis] = bar.hovered() || bar.dragged();
        // Where the button went down and the offset then: egui calls a press a click until it
        // moves 6 pt, so the release point can't tell the thumb from the track, and drag_delta
        // misses those first points.
        let grab_id = bar_id.with("grab");
        if bar.is_pointer_button_down_on()
            && let Some(origin) = ui.input(|i| i.pointer.primary_pressed().then(|| i.pointer.press_origin()).flatten())
        {
            ui.data_mut(|d| d.insert_temp(grab_id, (origin, offset[axis])));
        }
        let grab: Option<(Pos2, f32)> = ui.data_mut(|d| d.get_temp(grab_id));
        if let Some((origin, start)) = grab {
            if bar.dragged_by(egui::PointerButton::Primary)
                && travel > 0.0
                && let Some(p) = bar.interact_pointer_pos()
            {
                if last.is_some_and(|(_, s)| (scale - s).abs() > 1e-6) {
                    // A zoom during the drag moved the slide about its anchor: carry on from there.
                    ui.data_mut(|d| d.insert_temp(grab_id, (p, offset[axis])));
                } else {
                    offset[axis] = start + (p[axis] - origin[axis]) * overflow[axis] / travel;
                }
            } else if bar.clicked() && !thumb.contains(origin) {
                // A click on the track beside the thumb moves one window's length toward it.
                let page = avail.size()[axis] * 0.9;
                offset[axis] += if origin[axis] < thumb.min[axis] { -page } else { page };
            }
            if bar.drag_stopped() || bar.clicked() {
                ui.data_mut(|d| d.remove_temp::<(Pos2, f32)>(grab_id));
            }
        }
    }
    if (resp.hovered() || on_bar) && !zooming {
        if overflow != egui::Vec2::ZERO {
            offset -= wheel;
        } else {
            wheel_navigation(app, ui, carry_id, doc_uid);
        }
    }
    offset.x = offset.x.clamp(0.0, overflow.x);
    offset.y = offset.y.clamp(0.0, overflow.y);
    // On the physical pixel grid, so the slide texture is one texel per device pixel (#184). The
    // zoom anchor keeps the unsnapped rect, so snapping never accumulates into drift.
    let ppp = ui.ctx().pixels_per_point();
    let exact_rect = content_rect.translate(-offset);
    let slide_rect = crate::textures::device_pixel_rect(exact_rect, ppp);
    ui.data_mut(|d| {
        d.insert_temp(scroll_id, offset);
        d.insert_temp(last_id, (exact_rect, scale));
    });
    app.slide_rect = Some(slide_rect);
    app.canvas_scale = scale;
    let xf = Xf { origin: slide_rect.min, scale };
    let painter = ui.painter_at(avail);
    // Shadow + slide.
    painter.rect_filled(slide_rect.translate(vec2(0.0, 1.5)).expand(1.0), CornerRadius::same(2), t.shadow);
    let px = ((slide_rect.width() * ppp).round().max(1.0) as u32, (slide_rect.height() * ppp).round().max(1.0) as u32);
    let now = ui.input(|i| i.time);
    let gifs = slide_gifs(app, now);
    crate::media::gif_repaint(ui.ctx(), &gifs);
    let times: Vec<_> = gifs.iter().map(|g| (g.id, g.t)).collect();
    let anim = app.anim_preview_draw();
    let st = app.session.active();
    // One texture pixel in screen points, and the canvas in pixels from the slide's corner.
    let pixel = vec2(slide_rect.width() / px.0 as f32, slide_rect.height() / px.1 as f32);
    let visible = visible_region(avail, slide_rect, pixel);
    let typing_into = st.and_then(|d| d.selection.text.as_ref()).filter(|t| !t.notes).map(|t| t.shape);
    let tex = match st.map(|d| (d.selection.target, d)) {
        Some((Target::Slides, d)) => {
            let idx = d.selection.slide;
            let doc = d.doc.clone();
            let frame = anim.as_ref().map(|(tl, t)| (tl, *t));
            app.textures.slide(ui.ctx(), &doc, idx, px, visible, true, crate::media::gif_key(&gifs), app.ui.grayscale, &times, typing_into, frame)
        }
        Some((Target::Master { master }, d)) => master_tex(ui.ctx(), &d.doc, master, None, px, visible, typing_into),
        Some((Target::Layout { master, layout }, d)) => master_tex(ui.ctx(), &d.doc, master, Some(layout), px, visible, typing_into),
        Some((t @ (Target::Handout | Target::NotesMaster), d)) => {
            deckcraft_engine::master_page(&d.doc, t).and_then(|p| master_tex(ui.ctx(), &p, 0, None, px, visible, typing_into))
        }
        None => None,
    };
    if let Some((tex, region)) = tex {
        let at = Rect::from_min_size(
            slide_rect.min + vec2(region.x as f32 * pixel.x, region.y as f32 * pixel.y),
            vec2(region.w as f32 * pixel.x, region.h as f32 * pixel.y),
        );
        app.canvas_region = Some(at);
        painter.image(tex.id(), at, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    app.perf.render_ms = app.textures.last_render_ms;
    media_frames(app, &painter, xf);
    // Notes Master view: the current slide inside the slide-image placeholder, as in PowerPoint.
    let notes_preview = app.session.active().filter(|d| d.selection.target == Target::NotesMaster).and_then(|d| {
        let frame = d.doc.notes_master.as_ref()?.shapes.iter().find(|s| s.ph_type() == Some(deckcraft_model::PhType::SlideImage))?.xfrm?;
        Some((d.doc.clone(), d.selection.slide.min(d.doc.slides.len().saturating_sub(1)), frame))
    });
    if let Some((doc, idx, f)) = notes_preview
        && f.w > 1.0
        && f.h > 1.0
    {
        let aspect = doc.slide_size.width / doc.slide_size.height.max(1.0);
        let (w, h) = if f.w / f.h > aspect { (f.h * aspect, f.h) } else { (f.w, f.w / aspect) };
        let (x, y) = (f.x + (f.w - w) / 2.0, f.y + (f.h - h) / 2.0);
        let r = Rect::from_min_size(
            pos2(slide_rect.min.x + x as f32 * scale, slide_rect.min.y + y as f32 * scale),
            vec2(w as f32 * scale, h as f32 * scale),
        );
        let mut budget = 2;
        if let Some(tex) = app.textures.thumb(ui.ctx(), &doc, idx, (r.width() * ppp).round().max(1.0) as u32, &mut budget) {
            painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
    }
    if let Some(d) = app.session.active().filter(|d| d.selection.target == Target::Handout) {
        // Where the slides go on a printed handout, as in PowerPoint's preview.
        let (n, s) = (d.doc.notes_size, d.doc.slide_size);
        let aspect = s.width / s.height.max(1.0);
        for b in deckcraft_engine::pdf::handout_boxes(app.ui.handout_per_page, (n.width, n.height), aspect) {
            let r = Rect::from_min_max(
                pos2(slide_rect.min.x + b.x0 as f32 * scale, slide_rect.min.y + b.y0 as f32 * scale),
                pos2(slide_rect.min.x + b.x1 as f32 * scale, slide_rect.min.y + b.y1 as f32 * scale),
            );
            let dash = egui::Shape::dashed_line(
                &[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()],
                Stroke::new(1.0, Color32::from_gray(140)),
                4.0,
                3.0,
            );
            painter.extend(dash);
        }
    }
    if app.ui.gridlines
        && let Some(step) = grid_line_step(app.session.prefs.grid_spacing, scale)
    {
        draw_grid(&painter, slide_rect, step);
    }
    app.session.tool.guides_visible = app.ui.guides;
    if app.ui.guides {
        draw_guides(app, &painter, xf, slide_rect, &t);
    }
    if app.ui.ruler {
        ruler(&painter, avail, slide_rect, scale, app.session.prefs.units, &t);
        ruler_markers(app, ui, &painter, avail, xf, &t);
    }
    pointer(app, ui, &resp, xf);
    if !app.anim_preview_active() {
        overlays(app, ui, &painter, xf, &t);
        ime_output(app, ui, xf);
        media_bar(app, ui, xf);
        gif_buttons(app, ui, xf, &gifs);
        if resp.secondary_clicked()
            && let Some(p) = resp.interact_pointer_pos()
        {
            let word = app.session.active().and_then(|st| app.squiggles.word_at(st, xf.to_slide(p), 4.0 / xf.scale.max(0.01) as f64));
            app.squiggles.menu_word = word;
        }
        context_menu(app, &resp);
    }
    for (axis, active) in bar_active.into_iter().enumerate() {
        if let Some((track, thumb, _)) = scroll_bar(avail, inset, overflow, offset, axis) {
            painter.rect_filled(track, CornerRadius::ZERO, t.chrome);
            let fill = if active { t.text_dim } else { t.text_faint };
            painter.rect_filled(thumb.shrink(2.0), CornerRadius::same(3), fill);
        }
    }
}

/// Screen rect of a slide-space box (rotation ignored).
fn screen_rect(xf: Xf, x: &Xfrm) -> Rect {
    Rect::from_min_max(xf.to_screen(Point::new(x.x, x.y)), xf.to_screen(Point::new(x.x + x.w, x.y + x.h)))
}

fn master_tex(
    ctx: &egui::Context,
    doc: &deckcraft_model::Presentation,
    master: usize,
    layout: Option<usize>,
    px: (u32, u32),
    visible: deckcraft_render::Region,
    editing_text: Option<deckcraft_model::ShapeId>,
) -> Option<(egui::TextureHandle, deckcraft_render::Region)> {
    let scale = px.0 as f64 / doc.slide_size.width.max(1.0);
    let region = crate::textures::fit_texture(
        deckcraft_render::pasteboard_region(px, scale, deckcraft_render::layout_extent(doc, master, layout), visible),
        visible,
        crate::textures::max_side(ctx),
    );
    let img = deckcraft_render::render_layout(
        doc,
        master,
        layout,
        &deckcraft_render::RenderOpts { scale, edit: true, editing_text, offset: region.offset(), size: Some(region.size()), ..Default::default() },
    );
    Some((crate::textures::upload_editor(ctx, "master-canvas", crate::textures::to_color_image(&img, false)), region))
}

pub(super) mod preview;
pub mod squiggles;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod ruler_tests;

#[cfg(test)]
mod guides_tests;

#[cfg(test)]
#[path = "media_bar_tests.rs"]
mod media_bar_tests;
