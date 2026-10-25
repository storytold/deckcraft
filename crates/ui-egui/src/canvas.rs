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

/// A two-finger twist on the trackpad. macOS delivers it as [`egui::Event::Rotate`];
/// other platforms leave the delta at zero.
///
/// The selected shapes turn with the fingers. The whole twist is one undo step. Pinch-zoom
/// and two-finger scrolling are left as they are.
pub(crate) fn trackpad_twist(app: &mut SlideApp, ctx: &egui::Context) {
    if ctx.will_discard() {
        // Multi-pass layout runs this function again with the input already consumed. Acting on
        // the discarded pass would start the twist and the replay would immediately end it,
        // splitting one gesture into two undo steps.
        return;
    }
    let delta = ctx.input(|i| i.rotation_delta());
    let delta = if delta.is_finite() { delta.clamp(-std::f32::consts::PI, std::f32::consts::PI) } else { 0.0 };
    let turning = delta.abs() > 1e-4;
    let blocked = app.show.is_some()
        || app.backstage_open()
        || app.dialog.as_ref().is_some_and(|d| d.modal())
        || app.palette.is_some()
        || app.session.tool.dragging();
    let over = app.canvas_rect.is_some_and(|r| ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| r.contains(p)));
    let rotatable = app.session.active().is_some_and(|d| {
        d.selection.text.is_none() && d.selection.shapes.iter().any(|id| d.shape(*id).is_some_and(|sh| !sh.locked && sh.xfrm.is_some()))
    });
    if !blocked && turning && rotatable && (app.twisting || over) {
        let _ = app.run("shape.rotate", json!({"by": f64::from(delta.to_degrees()), "gesture": "move"}));
        app.twisting = true;
    } else if app.twisting {
        let _ = app.run("shape.rotate", json!({"gesture": "end", "ids": []}));
        app.twisting = false;
    }
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

/// A trackpad flick or mouse-wheel notch that should turn the page.
///
/// The stronger axis wins, so a sideways swipe is not read as a vertical scroll. Negative
/// movement — the content moves left or up, which is a swipe left or up with natural scrolling —
/// is the next slide. A nudge under 18 points is ignored so a resting finger does not change slides.
pub(crate) fn page_flick(wheel: egui::Vec2) -> Option<bool> {
    const MIN_FLICK: f32 = 18.0;
    let along = if wheel.x.abs() > wheel.y.abs() { wheel.x } else { wheel.y };
    (along.abs() > MIN_FLICK).then_some(along < 0.0)
}

/// Discrete wheel input must not share the trackpad's gesture cooldown. Keep all the steps,
/// including several notches delivered in one frame, and clamp at each end of the deck.
///
/// High-resolution and free-spin wheels send fractions of a line (0.125 per event on Windows), so
/// the delta is accumulated in `carry` and each whole line is one slide; a change of direction
/// drops what was left over. Returns the target, `None` when no wheel event was read.
fn wheel_slide_target(events: &[egui::Event], current: usize, slides: usize, carry: &mut f32) -> Option<usize> {
    let last = slides.checked_sub(1)?;
    let mut target = current.min(last);
    let mut handled = false;
    for event in events {
        if let egui::Event::MouseWheel { unit, delta, modifiers, .. } = event
            && matches!(unit, egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page)
            && !modifiers.ctrl
            && !modifiers.command
            && !modifiers.shift
            && delta.y.is_finite()
            && delta.y.abs() > 0.0
            && delta.y.abs() >= delta.x.abs()
        {
            if *carry * delta.y < 0.0 {
                *carry = 0.0;
            }
            *carry += delta.y;
            // The small slack keeps sums such as 10 × 0.1 from falling just short of a line.
            let whole = (carry.abs() + 1e-4).floor();
            *carry -= whole.copysign(*carry);
            let steps = whole.min(slides as f32) as usize;
            target = if delta.y < 0.0 { target.saturating_add(steps).min(last) } else { target.saturating_sub(steps) };
            handled = true;
        }
    }
    handled.then_some(target)
}

fn trackpad_scroll(events: &[egui::Event]) -> egui::Vec2 {
    events.iter().fold(egui::Vec2::ZERO, |sum, event| match event {
        egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, modifiers, .. } if !modifiers.ctrl && !modifiers.command => sum + *delta,
        _ => sum,
    })
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
    ui.data_mut(|d| {
        if d.get_temp::<(u64, f32)>(carry_id).is_none_or(|(uid, _)| uid != doc_uid) {
            d.insert_temp(carry_id, (doc_uid, 0.0_f32));
        }
    });
    if st.doc.slides.is_empty() && st.selection.target == Target::Slides {
        let resp = ui.allocate_rect(avail, Sense::click());
        ui.painter().text(avail.center(), Align2::CENTER_CENTER, "Click to add first slide", theme::font(22.0), t.text_dim);
        if resp.clicked() {
            let _ = app.run("slide.new", json!({"layout": "title"}));
        }
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
    if let Some((last_rect, last_scale)) = last
        && (scale - last_scale).abs() > 1e-6
    {
        let anchor = if zooming { hover.unwrap_or(avail.center()) } else { avail.center() };
        let at = (anchor - last_rect.min) / last_scale;
        offset = content_rect.min + at * scale - anchor;
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
        } else if !app.anim_preview_active() && app.session.tool.marquee.is_none() && !app.session.tool.dragging() {
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
    let gifs = match app.session.active() {
        Some(d) if d.selection.target == Target::Slides => app.media.gifs(d.uid, &d.doc, d.selection.slide, now, &app.session.gif_paused),
        _ => vec![],
    };
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
        ruler(&painter, avail, slide_rect, scale, &t);
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

/// Thickness of the canvas scroll bars, in points.
const SCROLL_BAR: f32 = 10.0;

/// The canvas scroll bar along `axis` (0 across, 1 down): its track, its thumb for `offset`, and
/// how far the thumb can travel. `None` when the slide fits that way. `inset` keeps the rulers'
/// strips clear.
fn scroll_bar(avail: Rect, inset: f32, overflow: egui::Vec2, offset: egui::Vec2, axis: usize) -> Option<(Rect, Rect, f32)> {
    let over = overflow[axis];
    if !over.is_finite() || over <= 0.0 {
        return None;
    }
    // Both bars showing leave the corner between them empty.
    let corner = if overflow[1 - axis] > 0.0 { SCROLL_BAR } else { 0.0 };
    let track = if axis == 0 {
        Rect::from_min_max(pos2(avail.min.x + inset, avail.max.y - SCROLL_BAR), pos2(avail.max.x - corner, avail.max.y))
    } else {
        Rect::from_min_max(pos2(avail.max.x - SCROLL_BAR, avail.min.y + inset), pos2(avail.max.x, avail.max.y - corner))
    };
    let len = track.size()[axis].max(0.0);
    let view = avail.size()[axis];
    let thumb_len = (len * view / (view + over)).max(SCROLL_BAR * 2.0).min(len);
    let travel = (len - thumb_len).max(0.0);
    let at = travel * (offset[axis] / over).clamp(0.0, 1.0);
    let mut thumb = track;
    thumb.min[axis] = track.min[axis] + at;
    thumb.max[axis] = thumb.min[axis] + thumb_len;
    Some((track, thumb, travel))
}

/// The canvas `avail` in texture pixels (`pixel`: one pixel's size in points) from the slide's
/// top-left corner.
fn visible_region(avail: Rect, slide: Rect, pixel: egui::Vec2) -> deckcraft_render::Region {
    let px = |v: f32, k: f32| if v.is_finite() && k > 0.0 { (v / k) as f64 } else { 0.0 };
    let (x0, y0) = (px(avail.min.x - slide.min.x, pixel.x).floor(), px(avail.min.y - slide.min.y, pixel.y).floor());
    let (x1, y1) = (px(avail.max.x - slide.min.x, pixel.x).ceil(), px(avail.max.y - slide.min.y, pixel.y).ceil());
    let lim = i32::MAX as f64 / 4.0;
    let (x0, y0) = (x0.clamp(-lim, lim), y0.clamp(-lim, lim));
    deckcraft_render::Region {
        x: x0 as i32,
        y: y0 as i32,
        w: (x1 - x0).clamp(1.0, u32::MAX as f64) as u32,
        h: (y1 - y0).clamp(1.0, u32::MAX as f64) as u32,
    }
}

/// Where an input method composes for the slide (#101), on screen: at the insertion point while
/// editing text, or over the one selected shape (typing on it starts editing). `None` when there is
/// nothing to type into.
pub fn ime_area(app: &SlideApp, xf: Xf) -> Option<Rect> {
    let st = app.session.active()?;
    if let Some(ts) = &st.selection.text {
        if ts.notes {
            return None;
        }
        if let Some((top, bot)) = deckcraft_engine::cmd::text::caret_line(st, ts) {
            return Some(Rect::from_two_pos(xf.to_screen(top), xf.to_screen(bot)).expand2(vec2(1.0, 0.0)));
        }
        let sh = st.shape(ts.shape)?;
        return Some(screen_rect(xf, &deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh)));
    }
    if st.selection.shapes.len() != 1 {
        return None;
    }
    let sh = st.shape(*st.selection.shapes.first()?)?;
    if sh.is_line() {
        return None;
    }
    Some(screen_rect(xf, &deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh)))
}

/// Turn the input method on for the slide while it takes typing, with its candidate window at
/// the insertion point (#101). Without this egui leaves it off, and a Korean keyboard types
/// separate jamo (ㅇ ㅏ ㄴ) instead of syllables. A focused text field sets its own.
///
/// This only records the screen rect for this pass. [`crate::SlideApp`] observes the committed
/// text, owner and cause once at the end of the pass, after every IME event and editor command
/// has settled.
fn ime_output(app: &mut SlideApp, ui: &Ui, xf: Xf) {
    let ctx = ui.ctx();
    if ctx.memory(|m| m.focused().is_some()) || app.dialog.as_ref().is_some_and(|d| d.modal()) || app.palette.is_some() || app.backstage_open() {
        return;
    }
    let Some(r) = ime_area(app, xf) else { return };
    let to_global = ctx.layer_transform_to_global(ui.layer_id()).unwrap_or_default();
    app.ime_pending = Some(to_global * r);
}

/// Play/pause on an animated GIF under the pointer or selected on its own.
fn gif_buttons(app: &mut SlideApp, ui: &mut Ui, xf: Xf, gifs: &[crate::media::Gif]) {
    let Some(d) = app.session.active() else { return };
    if d.selection.text.is_some() || app.session.tool.dragging() {
        return;
    }
    let only = if d.selection.shapes.len() == 1 { d.selection.shapes.first().copied() } else { None };
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let mut toggle = None;
    for g in gifs {
        let r = screen_rect(xf, &g.rect);
        if only != Some(g.id) && !pointer.is_some_and(|p| r.contains(p)) {
            continue;
        }
        let b = crate::media::gif_button_rect(r);
        // Sensing drags too keeps the press from reaching the canvas (selecting or moving the GIF).
        let resp = ui.interact(b, ui.scope_id().with(("gif_button", g.id.0)), Sense::click_and_drag());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, if g.paused { "Play animated GIF" } else { "Pause animated GIF" }));
        crate::media::paint_gif_button(ui.painter(), b, g.paused, resp.hovered());
        if resp.clicked() {
            toggle = Some(g.id);
        }
    }
    if let Some(id) = toggle {
        let _ = app.run("media.gifPlay", json!({"id": id}));
    }
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
            crate::media::paint_frame(painter, &app.media, id, screen_rect(xf, &x), clip.crop);
        }
    }
}

/// The play/pause and timeline bar under a selected audio or video (PowerPoint shows it on
/// selection or hover).
fn media_bar(app: &mut SlideApp, ui: &mut Ui, xf: Xf) {
    let Some(d) = app.session.active() else { return };
    let uid = d.uid;
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
        Some(crate::media::BarAction::Toggle) => app.media.toggle(uid, &doc, id, &clip, crate::media::Owner::Editor),
        Some(crate::media::BarAction::Seek(t)) => app.media.seek(uid, &doc, id, &clip, t, crate::media::Owner::Editor),
        Some(crate::media::BarAction::Nudge(dt)) => {
            let pos = status.map(|s| s.position).unwrap_or(0.0);
            app.media.seek(uid, &doc, id, &clip, (pos + dt).max(0.0), crate::media::Owner::Editor);
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

/// What is being dragged on the ruler.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RulerDrag {
    /// First-line indent (the top triangle).
    First,
    /// Hanging indent (the bottom triangle): moves the left indent, the first line stays put.
    Hanging,
    /// Left indent (the box under the hanging triangle): moves both together.
    Left,
    /// The tab stop that was at this position (pt from the text area's left).
    Tab(f64),
}

/// The paragraph's indents and tab stops on the horizontal ruler while text is being edited,
/// draggable as in PowerPoint; a click on the ruler adds a left tab, dragging a tab off the
/// ruler removes it.
fn ruler_markers(app: &mut SlideApp, ui: &mut Ui, p: &egui::Painter, avail: Rect, xf: Xf, t: &Tokens) {
    if app.session.active().is_none_or(|d| d.selection.text.is_none()) {
        return;
    }
    let Ok(info) = app.session.execute("format.ruler", &json!({})) else { return };
    if info["rotated"].as_bool().unwrap_or(true) {
        return;
    }
    let f = |k: &str| info[k].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
    let (x0, width, ml, ind) = (f("x"), f("width"), f("marginLeft"), f("indent"));
    let tabs: Vec<(f64, String)> = info["tabs"]
        .as_array()
        .map(|a| a.iter().filter_map(|t| Some((t["pos"].as_f64()?, t["align"].as_str().unwrap_or("l").to_string()))).collect())
        .unwrap_or_default();
    let top = Rect::from_min_max(pos2(avail.min.x + 16.0, avail.min.y), pos2(avail.max.x, avail.min.y + 16.0));
    let sx = |q: f64| xf.to_screen(Point::new(x0 + q, 0.0)).x;
    let to_q = |x: f32| ((xf.to_slide(pos2(x, 0.0)).x - x0).clamp(0.0, width.max(0.0)) * 2.0).round() / 2.0;
    // The text area's span, outlined on the ruler.
    let area = Rect::from_min_max(pos2(sx(0.0), top.min.y + 2.0), pos2(sx(width), top.max.y - 1.0)).intersect(top);
    p.rect_stroke(area, CornerRadius::ZERO, Stroke::new(1.0, t.text_faint.gamma_multiply(0.5)), egui::StrokeKind::Inside);

    let resp = ui.interact(top, ui.scope_id().with("ruler_markers"), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Text ruler"));
    let id = ui.scope_id().with("ruler_drag");
    let mut drag: Option<RulerDrag> = ui.data(|d| d.get_temp::<RulerDrag>(id));
    let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
    if resp.drag_started()
        && let Some(at) = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos())
    {
        let near = |q: f64| (sx(q) - at.x).abs() <= 5.0;
        let y = at.y - top.min.y;
        drag = if let Some((pos, _)) = tabs.iter().find(|(pos, _)| near(*pos) && y >= 8.0) {
            Some(RulerDrag::Tab(*pos))
        } else if y < 6.0 && near(ml + ind) {
            Some(RulerDrag::First)
        } else if (6.0..11.0).contains(&y) && near(ml) {
            Some(RulerDrag::Hanging)
        } else if y >= 11.0 && near(ml) {
            Some(RulerDrag::Left)
        } else {
            None
        };
        ui.data_mut(|d| match drag {
            Some(v) => {
                d.insert_temp(id, v);
            }
            None => {
                d.remove::<RulerDrag>(id);
            }
        });
    }
    // Where each marker is drawn: the dragged one follows the pointer.
    let live = drag.zip(pointer.filter(|_| resp.dragged())).map(|(d, at)| (d, to_q(at.x), at.y > top.max.y + 20.0));
    let (mut first, mut left) = (ml + ind, ml);
    match live {
        Some((RulerDrag::First, q, _)) => first = q,
        Some((RulerDrag::Hanging, q, _)) => left = q,
        Some((RulerDrag::Left, q, _)) => {
            first = q + ind;
            left = q;
        }
        _ => {}
    }
    let col = t.text_dim;
    let tri = |x: f32, y0: f32, down: bool| {
        let (a, b) = if down { (y0, y0 + 5.0) } else { (y0 + 5.0, y0) };
        p.add(egui::Shape::convex_polygon(vec![pos2(x - 4.0, a), pos2(x + 4.0, a), pos2(x, b)], t.card, Stroke::new(1.0, col)));
    };
    tri(sx(first), top.min.y + 1.0, true);
    tri(sx(left), top.min.y + 6.0, false);
    p.rect(
        Rect::from_min_max(pos2(sx(left) - 4.0, top.min.y + 11.5), pos2(sx(left) + 4.0, top.max.y - 0.5)),
        CornerRadius::ZERO,
        t.card,
        Stroke::new(1.0, col),
        egui::StrokeKind::Inside,
    );
    for (pos, align) in &tabs {
        let (q, gone) = match live {
            Some((RulerDrag::Tab(from), q, off)) if (from - pos).abs() < 0.5 => (q, off),
            _ => (*pos, false),
        };
        if gone {
            continue;
        }
        tab_glyph(p, pos2(sx(q), top.max.y - 2.0), align, t.text);
    }
    // The tab selector in the ruler's corner: a click cycles left, center, right, decimal; a click
    // on the ruler then sets that kind of stop.
    let kind_id = ui.scope_id().with("ruler_tab_kind");
    let mut kind: usize = ui.data(|d| d.get_temp(kind_id)).unwrap_or(0);
    let corner = Rect::from_min_size(avail.min, vec2(16.0, 16.0));
    let cr = ui.interact(corner, ui.scope_id().with("ruler_tab_selector"), Sense::click());
    const KINDS: [(&str, &str); 4] = [("l", "Left Tab"), ("ctr", "Center Tab"), ("r", "Right Tab"), ("dec", "Decimal Tab")];
    if cr.clicked() {
        kind = (kind + 1) % KINDS.len();
        ui.data_mut(|d| d.insert_temp(kind_id, kind));
    }
    let (kind_align, kind_label) = KINDS[kind % KINDS.len()];
    cr.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, kind_label));
    let cr = cr.on_hover_text(format!("{kind_label} (click to change)"));
    p.rect_stroke(
        corner.shrink(2.0),
        CornerRadius::same(2),
        Stroke::new(1.0, if cr.hovered() { t.accent } else { t.text_faint }),
        egui::StrokeKind::Inside,
    );
    tab_glyph(p, pos2(corner.center().x - 1.0, corner.center().y + 3.0), kind_align, t.text);
    if drag.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    // Commit on release; a click on bare ruler adds a left tab.
    if resp.drag_stopped() {
        if let (Some(d), Some(at)) = (drag, pointer) {
            let q = to_q(at.x);
            let r = match d {
                RulerDrag::First => app.run("format.paragraph", json!({"indentFirst": q - ml})),
                RulerDrag::Hanging => app.run("format.paragraph", json!({"indentLeft": q, "indentFirst": ml + ind - q})),
                RulerDrag::Left => app.run("format.paragraph", json!({"indentLeft": q})),
                RulerDrag::Tab(from) if at.y > top.max.y + 20.0 => app.run("format.tabs", json!({"remove": from})),
                RulerDrag::Tab(from) => app.run("format.tabs", json!({"move": {"from": from, "to": q}})),
            };
            if let Err(e) = r {
                app.set_status(e);
            }
        }
        ui.data_mut(|d| d.remove::<RulerDrag>(id));
    } else if resp.clicked()
        && let Some(at) = resp.interact_pointer_pos()
        && at.x >= area.min.x
        && at.x <= area.max.x
    {
        let near_tab = tabs.iter().any(|(pos, _)| (sx(*pos) - at.x).abs() <= 5.0);
        if resp.double_clicked() || near_tab {
            // A double-click on a stop opens the Tabs dialog, as in PowerPoint.
            if resp.double_clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("tabs"));
            }
        } else {
            let _ = app.run("format.tabs", json!({"add": {"pos": to_q(at.x), "align": kind_align}}));
        }
    }
}

/// A tab-stop mark with its foot at `at`: └ left, ┴ center, ┘ right, ┴ with a dot decimal.
fn tab_glyph(p: &egui::Painter, at: Pos2, align: &str, col: Color32) {
    let (x, y) = (at.x, at.y);
    let st = Stroke::new(1.5, col);
    p.line_segment([pos2(x, y - 6.0), pos2(x, y)], st);
    match align {
        "r" => {
            p.line_segment([pos2(x - 5.0, y), pos2(x, y)], st);
        }
        "ctr" => {
            p.line_segment([pos2(x - 4.0, y), pos2(x + 4.0, y)], st);
        }
        "dec" => {
            p.line_segment([pos2(x - 4.0, y), pos2(x + 4.0, y)], st);
            p.circle_filled(pos2(x + 3.0, y - 4.0), 1.0, col);
        }
        _ => {
            p.line_segment([pos2(x, y), pos2(x + 5.0, y)], st);
        }
    }
}

/// The presentation's drawing guides; the one being dragged at its new place, labelled with its
/// distance from the slide's centre in points.
fn draw_guides(app: &SlideApp, painter: &egui::Painter, xf: Xf, slide: Rect, t: &Tokens) {
    let Some(st) = app.session.active() else { return };
    let size = st.doc.slide_size;
    let drag = app.session.tool.guide_drag;
    for (i, g) in st.doc.guides.iter().enumerate() {
        let (pos, active) = match drag {
            Some((di, p)) if di == i => (p, true),
            _ => (g.pos, app.session.tool.guide_hover == Some(i)),
        };
        let col = if active { t.accent } else { Color32::from_rgba_unmultiplied(120, 120, 120, 170) };
        let stroke = Stroke::new(1.0, col);
        let (a, b) = if g.vertical {
            let x = xf.to_screen(Point::new(pos, 0.0)).x;
            (pos2(x, slide.min.y), pos2(x, slide.max.y))
        } else {
            let y = xf.to_screen(Point::new(0.0, pos)).y;
            (pos2(slide.min.x, y), pos2(slide.max.x, y))
        };
        // Dashed, like PowerPoint's guides.
        let len = (b - a).length();
        let dir = (b - a) / len.max(1.0);
        let mut d = 0.0;
        while d < len {
            painter.line_segment([a + dir * d, a + dir * (d + 4.0).min(len)], stroke);
            d += 7.0;
        }
        if drag.is_some_and(|(di, _)| di == i) {
            let centre = if g.vertical { size.width / 2.0 } else { size.height / 2.0 };
            let off = pos - centre;
            let side = match (g.vertical, off < 0.0, off > 0.0) {
                (true, true, _) => " left of centre",
                (true, _, true) => " right of centre",
                (false, true, _) => " above centre",
                (false, _, true) => " below centre",
                _ => " (centre)",
            };
            let label = format!("{:.0} pt{side}", off.abs());
            let at = if g.vertical { pos2(a.x + 6.0, slide.min.y + 6.0) } else { pos2(slide.min.x + 6.0, a.y + 4.0) };
            let galley = painter.layout_no_wrap(label, theme::font(11.0), t.text);
            let r = Rect::from_min_size(at, galley.size()).expand(3.0);
            painter.rect_filled(r, CornerRadius::same(3), t.card);
            painter.galley(at, galley, t.text);
        }
    }
}

fn pointer(app: &mut SlideApp, ui: &Ui, resp: &egui::Response, xf: Xf) {
    if crate::eyedropper::canvas(app, ui, resp, xf) {
        return;
    }
    if app.anim_preview_active() {
        // A click on the slide ends the preview, then selects as usual.
        if resp.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
            app.stop_anim_preview();
        } else {
            return;
        }
    }
    let tol = (4.0 / xf.scale) as f64;
    let m = mods(ui);
    let Some(pos) = resp.interact_pointer_pos().or(resp.hover_pos()) else { return };
    // The rulers have their own controls; a press there mustn't reach the slide (and end text
    // editing). A drag already under way keeps going over them.
    if app.ui.ruler && !app.session.tool.dragging() && (pos.y < resp.rect.min.y + 16.0 || pos.x < resp.rect.min.x + 16.0) {
        return;
    }
    let p = xf.to_slide(pos);
    let pressure = reported_pressure(ui);
    let ev = |kind| PointerEvent { kind, x: p.x, y: p.y, mods: m, tol, pressure };
    let primary = ui.input(|i| i.pointer.primary_pressed());
    if resp.hovered() && !app.session.tool.dragging() {
        let _ = app.session.pointer(ev(PointerKind::Move));
    }
    let secondary = resp.hovered() && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));
    let r = if secondary && !app.session.tool.dragging() {
        // Select what's under the pointer before the context menu opens (on release).
        app.session.pointer(ev(PointerKind::ContextClick))
    } else if resp.triple_clicked() {
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
    // Cursor. The Format and Animation Painters override everything: their whole point is to
    // tell the user the tool is armed and waiting for a click (#830).
    let painter_armed = app.session.painter.is_some() || app.session.anim_painter.is_some();
    let cursor = if painter_armed {
        egui::CursorIcon::Copy
    } else {
        match (&app.session.tool.kind, &app.session.tool.hover) {
            (ToolKind::Shape { .. }, _) => egui::CursorIcon::Crosshair,
            (ToolKind::TextBox, _) => egui::CursorIcon::Text,
            (ToolKind::Ink { .. }, _) => egui::CursorIcon::Crosshair,
            (_, Some(Hit::Resize { id, handle })) => resize_cursor(&app.session, *id, *handle),
            (_, Some(Hit::Rotate { .. })) => egui::CursorIcon::Alias,
            (_, Some(Hit::Model3dTurn { .. })) => egui::CursorIcon::Grab,
            (_, Some(Hit::TableColumn { .. })) => egui::CursorIcon::ResizeColumn,
            (_, Some(Hit::TableRow { .. })) => egui::CursorIcon::ResizeRow,
            (_, Some(Hit::Adjust { .. } | Hit::Warp { .. } | Hit::LineEnd { .. })) => egui::CursorIcon::PointingHand,
            (_, Some(Hit::Shape { text: true, .. })) => {
                if app.session.active().is_some_and(|d| d.selection.text.is_some()) {
                    egui::CursorIcon::Text
                } else {
                    egui::CursorIcon::Move
                }
            }
            (_, Some(Hit::Shape { .. } | Hit::Frame { .. })) => egui::CursorIcon::Move,
            _ => match app.session.tool.guide_drag.map(|g| g.0).or(app.session.tool.guide_hover) {
                Some(i) if app.session.active().and_then(|d| d.doc.guides.get(i)).is_some_and(|g| g.vertical) => egui::CursorIcon::ResizeHorizontal,
                Some(_) => egui::CursorIcon::ResizeVertical,
                None => egui::CursorIcon::Default,
            },
        }
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
    // Spelling squiggles under misspelt words (checked in the background).
    if let Some(st) = app.session.active() {
        let lines = app.squiggles.update(st, Some(ui.ctx()));
        let scale = (xf.to_screen(Point::new(1.0, 0.0)) - xf.to_screen(Point::new(0.0, 0.0))).length();
        for (a, b) in lines {
            crate::squiggles::paint(painter, xf.to_screen(a), xf.to_screen(b), scale);
        }
    }
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
        if app.session.tool.editing_points(st) == Some(sh.id) {
            // Edit Points: the outline's vertices are the handles.
            // No selection box: the outline and its vertices are what's edited.
            if let Some((path, index)) = app.session.tool.edit_vertex
                && let Some((hin, hout, v)) = deckcraft_engine::cmd::points::slide_handles(&st.doc, &st.selection, sh, path, index)
            {
                for h in [hin, hout].into_iter().flatten() {
                    painter.line_segment([xf.to_screen(v), xf.to_screen(h)], sel_stroke);
                    painter.rect(
                        egui::Rect::from_center_size(xf.to_screen(h), egui::vec2(6.0, 6.0)),
                        0.0,
                        Color32::WHITE,
                        Stroke::new(1.0, t.selection),
                        egui::StrokeKind::Middle,
                    );
                }
            }
            for q in deckcraft_engine::cmd::points::slide_vertices(&st.doc, &st.selection, sh).into_iter().flatten() {
                let c = xf.to_screen(q);
                painter.rect(
                    egui::Rect::from_center_size(c, egui::vec2(7.0, 7.0)),
                    0.0,
                    Color32::BLACK,
                    Stroke::new(1.0, Color32::WHITE),
                    egui::StrokeKind::Middle,
                );
            }
            continue;
        }
        if editing_this {
            // Dashed box while editing text.
            for i in 0..4 {
                let (s, e) = (corners[i], corners[(i + 1) % 4]);
                painter.add(egui::Shape::dashed_line(&[s, e], Stroke::new(1.0, t.selection), 4.0, 3.0));
            }
        } else {
            poly(painter, corners, sel_stroke);
        }
        if sh.locked && !editing_this {
            // Locked: greyed handles that don't respond, no rotate or adjust handles.
            for h in box_handles(&x).0 {
                painter.circle(xf.to_screen(h), 4.0, t.text_faint, Stroke::new(1.0, t.handle_fill));
            }
            continue;
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
            for hd in deckcraft_engine::tools::wordart_handles(&st.doc, &st.selection, sh, x.w, x.h) {
                let q = xf.to_screen(a * hd.pos);
                painter.circle(q, 4.5, Color32::from_rgb(0xFF, 0xD2, 0x3F), Stroke::new(1.0, Color32::from_rgb(0x8A, 0x6A, 0x00)));
            }
            if matches!(sh.kind, deckcraft_model::ShapeKind::Model3d { .. }) && st.selection.shapes.len() == 1 {
                model_turn_handle(painter, xf.to_screen(x.center()), t);
            }
        }
    }
    // Caret / text selection, in a shape or a table cell.
    if let Some(ts) = &editing {
        for quad in text_marks(st, ts).iter().flat_map(|m| &m.selection) {
            painter.add(egui::Shape::convex_polygon(quad.iter().map(|q| xf.to_screen(*q)).collect(), t.text_selection, Stroke::NONE));
        }
        // An input method's composition: underlined in place, thicker under the part being converted.
        for (a, b, thick) in deckcraft_engine::cmd::text::composition_underlines(st) {
            painter.line_segment([xf.to_screen(a), xf.to_screen(b)], Stroke::new(if thick { 2.0 } else { 1.0 }, t.caret));
        }
        if !ts.is_range() {
            let blink = (ui.input(|i| i.time) * 1.6).fract() < 0.6;
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
            // An empty placeholder shows the caret where typing begins too.
            if blink && let Some((top, bot)) = deckcraft_engine::cmd::text::caret_line(st, ts) {
                // In the colour of the text being typed: a black caret is lost on a dark slide.
                let color = deckcraft_engine::cmd::text::caret_color(st, ts).map_or(t.caret, theme::to_color32);
                painter.line_segment([xf.to_screen(top), xf.to_screen(bot)], Stroke::new(1.5, color));
            }
        }
    }
    // Table cell editing: outline the cell (merged cells as drawn).
    if let Some(ts) = &editing
        && let (Some((r, c)), Some(sh)) = (ts.cell, st.shape(ts.shape))
        && let Some(f) = deckcraft_engine::cmd::table::table_frame(st, sh)
        && let Some(cr) = f.cell_rect(r, c)
    {
        let pts = [Point::new(cr.x0, cr.y0), Point::new(cr.x1, cr.y0), Point::new(cr.x1, cr.y1), Point::new(cr.x0, cr.y1)]
            .map(|q| xf.to_screen(f.affine * q));
        poly(painter, pts, Stroke::new(2.0, t.accent));
    }
    // Shade the selected table cells in the text-selection color, each merged cell as it is drawn.
    if let Some((id, ..)) = st.selection.cells
        && st.selection.text.is_none()
        && let Some(sh) = st.shape(id)
        && let deckcraft_model::ShapeKind::Table(table) = &sh.kind
        && let Some(f) = deckcraft_engine::cmd::table::table_frame(st, sh)
    {
        use deckcraft_engine::cmd::table::{in_range, selected_range};
        let range = selected_range(st.selection.cells, id, table);
        for (_, _, cr) in f.cells.iter().filter(|(r, c, _)| range.is_some() && in_range(range, *r, *c)) {
            let pts = [Point::new(cr.x0, cr.y0), Point::new(cr.x1, cr.y0), Point::new(cr.x1, cr.y1), Point::new(cr.x0, cr.y1)];
            painter.add(egui::Shape::convex_polygon(pts.iter().map(|q| xf.to_screen(f.affine * *q)).collect(), t.text_selection, Stroke::NONE));
        }
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
        let pts = &app.session.tool.ink_preview;
        let legacy = deckcraft_engine::model::ink_pressure_is_legacy(pts);
        let c = theme::to_color32(*color);
        let c = if mode == "highlighter" { c.gamma_multiply(0.45) } else { c };
        if legacy || mode == "highlighter" {
            // One line, so see-through highlighter ink doesn't darken where segments meet.
            let line: Vec<Pos2> = pts.iter().map(|(x, y, _)| xf.to_screen(Point::new(*x, *y))).collect();
            painter.add(egui::Shape::line(line, Stroke::new((*width as f32 * xf.scale).max(1.0), c)));
        } else {
            for pair in pts.windows(2) {
                let [q0, q1] = pair else { continue };
                let a = deckcraft_engine::model::ink_sample_width(*width, q0.2, legacy);
                let b = deckcraft_engine::model::ink_sample_width(*width, q1.2, legacy);
                let sw = ((a + b) * 0.5 * f64::from(xf.scale)).max(1.0) as f32;
                let p0 = xf.to_screen(Point::new(q0.0, q0.1));
                let p1 = xf.to_screen(Point::new(q1.0, q1.1));
                painter.add(egui::Shape::line(vec![p0, p1], Stroke::new(sw, c)));
            }
        }
    }
}

/// Pressure from a touch or pen this frame (`0..=1`). A mouse reports none.
fn reported_pressure(ui: &Ui) -> Option<f32> {
    ui.input(|i| {
        i.events.iter().rev().find_map(|e| match e {
            egui::Event::Touch { force: Some(f), phase, .. }
                if matches!(phase, egui::TouchPhase::Start | egui::TouchPhase::Move) && f.is_finite() =>
            {
                Some(f.clamp(0.0, 1.0))
            }
            _ => None,
        })
    })
}

/// The text selection (quads) of the text being edited, in slide space: in a shape or a table
/// cell, wherever its text is drawn. The caret is `cmd::text::caret_line`.
pub(crate) struct TextMarks {
    pub selection: Vec<[Point; 4]>,
}

pub(crate) fn text_marks(st: &deckcraft_engine::DocState, ts: &deckcraft_engine::TextSel) -> Option<TextMarks> {
    let (l, m) = deckcraft_engine::cmd::text::text_frame(st, ts)?;
    let pos = |p: (usize, usize)| deckcraft_text::Pos { para: p.0, ch: p.1 };
    let (s0, s1) = ts.ordered();
    let selection = l
        .selection_rects(pos(s0), pos(s1))
        .into_iter()
        .map(|r| [Point::new(r.x0, r.y0), Point::new(r.x1, r.y0), Point::new(r.x1, r.y1), Point::new(r.x0, r.y1)].map(|q| m * q))
        .collect();
    Some(TextMarks { selection })
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

/// The 3-D turn handle in the middle of a selected model: a disc crossed by an orbit and a
/// meridian.
fn model_turn_handle(p: &egui::Painter, c: Pos2, t: &Tokens) {
    p.circle(c, 11.0, t.handle_fill, Stroke::new(1.0, t.handle_stroke));
    let ellipse = |rx: f32, ry: f32| egui::epaint::PathShape {
        points: (0..32).map(|i| i as f32 / 32.0 * std::f32::consts::TAU).map(|a| pos2(c.x + rx * a.cos(), c.y + ry * a.sin())).collect(),
        closed: true,
        fill: Color32::TRANSPARENT,
        stroke: Stroke::new(1.2, t.handle_stroke).into(),
    };
    p.add(egui::Shape::Path(ellipse(7.0, 2.8)));
    p.add(egui::Shape::Path(ellipse(2.8, 7.0)));
}

fn draw_preview(app: &SlideApp, p: &egui::Painter, xf: Xf, x: Xfrm, t: &Tokens) {
    let preset = match &app.session.tool.kind {
        ToolKind::Shape { preset } => preset.clone(),
        _ => "rect".into(),
    };
    if preset == deckcraft_engine::tools::DRAW_TABLE {
        // A border inside a table (flat), or the outline of a new table.
        let (a, b) = (xf.to_screen(Point::new(x.x, x.y)), xf.to_screen(Point::new(x.x + x.w, x.y + x.h)));
        if x.w < 0.5 || x.h < 0.5 {
            p.line_segment([a, b], Stroke::new(1.5, t.accent));
        } else {
            p.rect_stroke(Rect::from_two_pos(a, b), 0.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
        }
        return;
    }
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
    // Right-clicking a vertex in Edit Points picks it for the point menu.
    if resp.secondary_clicked()
        && let Some(Hit::Vertex { path, index, .. }) = app.session.tool.hover.clone()
    {
        app.session.tool.edit_vertex = Some((path, index));
    }
    resp.context_menu(|ui| {
        let editing = app.session.active().and_then(|d| app.session.tool.editing_points(d).and_then(|id| d.shape(id).cloned()));
        if let Some(sh) = editing {
            if let Some((path, index)) = app.session.tool.edit_segment {
                let cur = deckcraft_engine::cmd::points::segment_kind_of(&sh, path, index);
                for (l, k) in [("Straight Segment", "straight"), ("Curved Segment", "curved")] {
                    if ui.radio(cur == Some(k), l).clicked() {
                        let _ = app.run("shape.segmentType", json!({"path": path, "segment": index, "type": k}));
                        ui.close();
                    }
                }
                // Each of these renumbers the points, so the picked segment is dropped.
                if ui.button("Add Point").clicked() {
                    let t = app.session.tool.edit_segment_t.unwrap_or(0.5);
                    let _ = app.run("shape.addPoint", json!({"path": path, "segment": index, "t": t}));
                    app.session.tool.edit_segment = None;
                    ui.close();
                }
                let closed = deckcraft_engine::cmd::points::closed_at(&sh, path, index);
                if ui.add_enabled(closed == Some(true), egui::Button::new("Delete Segment")).clicked() {
                    let _ = app.run("shape.deleteSegment", json!({"path": path, "segment": index}));
                    app.session.tool.edit_segment = None;
                    ui.close();
                }
                // Open Path opens the outline at the segment's start point.
                let (label, id, at) = match closed {
                    Some(false) => ("Close Path", "shape.closePath", index),
                    _ => ("Open Path", "shape.openPath", deckcraft_engine::cmd::points::segment_start_of(&sh, path, index).unwrap_or(index)),
                };
                if ui.button(label).clicked() {
                    let _ = app.run(id, json!({"path": path, "index": at}));
                    app.session.tool.edit_segment = None;
                    ui.close();
                }
                ui.separator();
            } else if let Some((path, index)) = app.session.tool.edit_vertex {
                let cur = deckcraft_engine::cmd::points::kind_of(&sh, path, index);
                for (l, k) in [("Smooth Point", "smooth"), ("Straight Point", "straight"), ("Corner Point", "corner")] {
                    if ui.radio(cur == Some(k), l).clicked() {
                        let _ = app.run("shape.pointType", json!({"path": path, "index": index, "type": k}));
                        ui.close();
                    }
                }
                if ui.button("Delete Point").clicked() {
                    let _ = app.run("shape.deletePoint", json!({"path": path, "index": index}));
                    app.session.tool.edit_vertex = None;
                    ui.close();
                }
                let (label, id) = match deckcraft_engine::cmd::points::closed_at(&sh, path, index) {
                    Some(false) => ("Close Path", "shape.closePath"),
                    _ => ("Open Path", "shape.openPath"),
                };
                if ui.button(label).clicked() {
                    let _ = app.run(id, json!({"path": path, "index": index}));
                    // The points are numbered from the new start now.
                    app.session.tool.edit_vertex = None;
                    ui.close();
                }
                ui.separator();
            }
            if ui.button("Exit Edit Points").clicked() {
                let _ = app.run("shape.editPoints", json!({"on": false}));
                ui.close();
            }
            return;
        }
        // A misspelt word under the pointer: its suggestions, then Ignore All and Add to Dictionary.
        if let Some(w) = app.squiggles.menu_word.clone() {
            if let Some(from) = w.delete_from {
                if ui.button(egui::RichText::new("Delete Repeated Word").strong()).clicked() {
                    let cut: String = format!("{}{}", " ".repeat(w.start - from), w.word);
                    // The gap is spaces; take it from the text so tabs or no-break spaces match.
                    let gap = app
                        .session
                        .active()
                        .and_then(|st| match w.tag {
                            crate::squiggles::Tag::Shape { id, para } => st.shape(id)?.text.as_ref()?.paragraphs.get(para).map(|p| p.text()),
                            crate::squiggles::Tag::Cell { id, row, col, para } => match &st.shape(id)?.kind {
                                deckcraft_model::ShapeKind::Table(tb) => tb.cell(row, col)?.text.paragraphs.get(para).map(|p| p.text()),
                                _ => None,
                            },
                            crate::squiggles::Tag::Notes { .. } => None,
                        })
                        .map(|t| t.chars().skip(from).take(w.start + w.word.chars().count() - from).collect::<String>())
                        .unwrap_or(cut);
                    let mut p = json!({"para": 0, "start": from, "word": gap, "to": ""});
                    if let Some(o) = p.as_object_mut() {
                        match w.tag {
                            crate::squiggles::Tag::Shape { id, para } => {
                                o.insert("shape".into(), json!(id));
                                o.insert("para".into(), json!(para));
                            }
                            crate::squiggles::Tag::Cell { id, row, col, para } => {
                                o.insert("shape".into(), json!(id));
                                o.insert("row".into(), json!(row));
                                o.insert("col".into(), json!(col));
                                o.insert("para".into(), json!(para));
                            }
                            crate::squiggles::Tag::Notes { .. } => {}
                        }
                    }
                    let _ = app.run("review.spellingChange", p);
                    app.squiggles.menu_word = None;
                    ui.close();
                }
            } else if w.suggestions.is_empty() {
                ui.label(egui::RichText::new("(no spelling suggestions)").weak());
            }
            for to in &w.suggestions {
                if ui.button(egui::RichText::new(to).strong()).clicked() {
                    let p = match w.tag {
                        crate::squiggles::Tag::Shape { id, para } => json!({"shape": id, "para": para, "start": w.start, "word": w.word, "to": to}),
                        crate::squiggles::Tag::Cell { id, row, col, para } => {
                            json!({"shape": id, "row": row, "col": col, "para": para, "start": w.start, "word": w.word, "to": to})
                        }
                        crate::squiggles::Tag::Notes { .. } => json!({}),
                    };
                    let _ = app.run("review.spellingChange", p);
                    app.squiggles.menu_word = None;
                    ui.close();
                }
            }
            if ui.button("Ignore All").clicked() {
                app.squiggles.ignore(&w.word);
                app.squiggles.menu_word = None;
                ui.close();
            }
            if ui.button("Add to Dictionary").clicked() {
                if let Err(e) = app.session.execute("review.addToDictionary", &json!({"word": w.word})) {
                    app.set_status(e.to_string());
                }
                app.squiggles.menu_word = None;
                ui.close();
            }
            ui.separator();
        }
        let has_sel = app.session.active().is_some_and(|d| !d.selection.shapes.is_empty());
        for (l, id) in [("Cut", "edit.cut"), ("Copy", "edit.copy"), ("Paste", "edit.paste")] {
            if ui.add_enabled(crate::ribbon::enabled(app, id), egui::Button::new(l)).clicked() {
                let _ = app.run(id, json!({}));
                ui.close();
            }
        }
        ui.separator();
        // Right-clicking the text being edited keeps it (`PointerKind::ContextClick`): text items.
        let editing = app.session.active().is_some_and(|d| d.selection.text.as_ref().is_some_and(|t| !t.notes));
        if editing {
            if ui.button("Paragraph…").clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("paragraph"));
                ui.close();
            }
            for (l, id) in [("Bullets", "format.bullets"), ("Numbering", "format.numbering")] {
                if ui.add_enabled(crate::ribbon::enabled(app, id), egui::Button::new(l)).clicked() {
                    let _ = app.run(id, json!({}));
                    ui.close();
                }
            }
            if ui.button("Link…").clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("hyperlink"));
                ui.close();
            }
            ui.separator();
            if ui.button("Format Shape…").clicked() {
                let _ = app.run("view.pane", json!({"pane": "format"}));
                ui.close();
            }
        } else if has_sel {
            if ui.button("Edit Text").clicked() {
                let _ = app.run("text.edit", json!({}));
                ui.close();
            }
            if crate::ribbon::enabled(app, "shape.editPoints")
                && app.session.active().is_some_and(|d| d.selection.shapes.len() == 1 && d.selected_shapes().iter().all(|s| !s.is_line()))
                && ui.button("Edit Points").clicked()
            {
                let _ = app.run("shape.editPoints", json!({"on": true}));
                ui.close();
            }
            if ui.button("Font…").clicked() {
                let _ = app.run("format.fontDialog", json!({}));
                ui.close();
            }
            if ui.button("Paragraph…").clicked() {
                let _ = app.run("format.paragraphDialog", json!({}));
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
            let locked = app.session.active().is_some_and(|d| d.selected_shapes().iter().all(|s| s.locked));
            if ui.button(if locked { "Unlock" } else { "Lock" }).clicked() {
                let _ = app.run("shape.lock", json!({"locked": !locked}));
                ui.close();
            }
            let picture = app
                .session
                .active()
                .is_some_and(|d| d.selected_shapes().iter().any(|s| matches!(s.kind, deckcraft_model::ShapeKind::Picture { .. })));
            if picture && ui.button("Change Picture…").clicked() {
                let _ = app.run("app.changePictureDialog", json!({}));
                ui.close();
            }
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
            if ui.button("Size and Position…").clicked() {
                let _ = app.run("shape.sizePosition", json!({}));
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
            ui.menu_button("Grid and Guides", |ui| {
                let mut on = app.ui.guides;
                if ui.checkbox(&mut on, "Guides").clicked() {
                    let _ = app.run("view.guides", json!({"on": on}));
                }
                for (label, vertical) in [("Add Vertical Guide", true), ("Add Horizontal Guide", false)] {
                    if ui.button(label).clicked() {
                        let _ = app.run("guide.add", json!({"vertical": vertical}));
                        let _ = app.run("view.guides", json!({"on": true}));
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

/// Screen-point distance between grid lines. `spacing` is the grid spacing in slide points;
/// `scale` maps slide points onto the screen. `None` when the grid would not be a finite set of
/// lines (bad inputs, or so fine that a line would land on every pixel).
fn grid_line_step(spacing: f64, scale: f32) -> Option<f32> {
    if !spacing.is_finite() || !(1.0..=288.0).contains(&spacing) || !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let step = (spacing as f32) * scale;
    if !step.is_finite() || step < 1.0 {
        return None;
    }
    Some(step)
}

fn draw_grid(painter: &egui::Painter, slide: Rect, step: f32) {
    let stroke = Stroke::new(0.5, Color32::from_black_alpha(40));
    // A denser grid than this paints a grey slab and costs a frame. Skip it.
    if !grid_fits(slide.width(), step) || !grid_fits(slide.height(), step) {
        return;
    }
    let mut x = slide.min.x + step;
    while x < slide.max.x {
        painter.line_segment([pos2(x, slide.min.y), pos2(x, slide.max.y)], stroke);
        x += step;
    }
    let mut y = slide.min.y + step;
    while y < slide.max.y {
        painter.line_segment([pos2(slide.min.x, y), pos2(slide.max.x, y)], stroke);
        y += step;
    }
}

fn grid_fits(span: f32, step: f32) -> bool {
    span.is_finite() && span > 0.0 && step >= 1.0 && span / step <= 800.0
}

/// The resize cursor for `handle` of shape `id`: the handle's resize axis is computed in the
/// shape's own space and mapped through its transform, so a rotated or flipped shape shows the
/// diagonal that matches where the drag actually goes (#745). Without a transform the mapping
/// reduces to the classic NW/NE/E/SE/S/SW/W assignment.
fn resize_cursor(session: &deckcraft_engine::Session, id: deckcraft_model::ShapeId, handle: u8) -> egui::CursorIcon {
    // The resize handles are drawn from the shape's effective box (a placeholder inherits it
    // from its layout), so the cursor follows the same one.
    let Some(d) = session.active() else { return egui::CursorIcon::Default };
    let Some(sh) = d.shape(id) else { return egui::CursorIcon::Default };
    let x = deckcraft_engine::cmd::xfrm_of(&d.doc, &d.selection, sh);
    // The resize axis in the shape's own coordinates, as (dx, dy) from the box centre toward
    // the handle. Corners resize along the diagonal, edge handles along their edge. The
    // direction is normalised - the diagonal at 45 degrees whatever the shape's proportions -
    // so a wide rectangle still shows a diagonal cursor on its corners.
    let (dx, dy): (f64, f64) = match handle {
        0 => (-1.0, -1.0),
        1 => (0.0, -1.0),
        2 => (1.0, -1.0),
        3 => (1.0, 0.0),
        4 => (1.0, 1.0),
        5 => (0.0, 1.0),
        6 => (-1.0, 1.0),
        _ => (-1.0, 0.0),
    };
    // Map through the linear part of the transform (rotation, flips): two points along the
    // axis, the difference is the slide-space direction.
    let centre = Point::new(x.x + x.w / 2.0, x.y + x.h / 2.0);
    let a = x.affine();
    let p0 = a * centre;
    let p1 = a * Point::new(centre.x + dx, centre.y + dy);
    let v = p1 - p0;
    if !v.x.is_finite() || !v.y.is_finite() || (v.x.abs() < 1e-12 && v.y.abs() < 1e-12) {
        return egui::CursorIcon::Default;
    }
    // Angle in [0, 180): 0° = right (horizontal), 45° = down-right (NW-SE), 90° = down
    // (vertical), 135° = down-left (NE-SW). Pick the closest of the four.
    let mut deg = v.y.atan2(v.x).to_degrees();
    if deg < 0.0 {
        deg += 180.0;
    }
    if (22.5..67.5).contains(&deg) {
        egui::CursorIcon::ResizeNwSe
    } else if (67.5..112.5).contains(&deg) {
        egui::CursorIcon::ResizeVertical
    } else if (112.5..157.5).contains(&deg) {
        egui::CursorIcon::ResizeNeSw
    } else {
        egui::CursorIcon::ResizeHorizontal
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::sync::Arc;

    use deckcraft_engine::Session;
    use egui::{Event, Modifiers, MouseWheelUnit, Pos2, TouchPhase, pos2, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    use super::*;
    use crate::{Services, SlideApp};

    /// #745: the resize cursor follows the handle's on-screen diagonal. A 45°-rotated shape's
    /// NW corner sits at the top-right of the screen, where the drag axis is NE-SW — the old
    /// fixed handle-to-cursor mapping showed NW-SE there.
    #[test]
    fn resize_cursor_tracks_rotation() {
        let mut s = Session::with_new();
        // A square shape: the NW handle's diagonal is at 45° unrotated, so each 45° of shape
        // rotation cleanly moves the cursor to the next of the four resize directions.
        let id = s.execute("shape.insert", &serde_json::json!({"preset": "rect", "rect": [100.0, 100.0, 100.0, 100.0]})).unwrap()["id"]
            .as_u64()
            .unwrap() as u32;
        let sid = deckcraft_model::ShapeId(id);
        // Unrotated: the NW handle (0) drags the NW-SE diagonal.
        assert_eq!(resize_cursor(&s, sid, 0), egui::CursorIcon::ResizeNwSe);
        assert_eq!(resize_cursor(&s, sid, 4), egui::CursorIcon::ResizeNwSe);
        assert_eq!(resize_cursor(&s, sid, 2), egui::CursorIcon::ResizeNeSw);
        assert_eq!(resize_cursor(&s, sid, 6), egui::CursorIcon::ResizeNeSw);
        assert_eq!(resize_cursor(&s, sid, 1), egui::CursorIcon::ResizeVertical);
        assert_eq!(resize_cursor(&s, sid, 3), egui::CursorIcon::ResizeHorizontal);
        // 45° rotation: the NW corner's drag axis rotates 45° with the shape — it is now vertical.
        {
            let doc = s.doc_mut().unwrap();
            let pres = Arc::make_mut(&mut doc.doc);
            let slide = Arc::make_mut(&mut pres.slides[0]);
            if let Some(sh) = slide.shapes.iter_mut().find(|x| x.id == sid)
                && let Some(x) = sh.xfrm.as_mut()
            {
                x.rot = 45.0;
            }
        }
        assert_eq!(resize_cursor(&s, sid, 0), egui::CursorIcon::ResizeVertical, "45°: NW handle drags vertically");
        assert_eq!(resize_cursor(&s, sid, 2), egui::CursorIcon::ResizeHorizontal, "45°: NE handle drags horizontally");
        // 90° rotation: the NW corner is at the top-right; the drag axis is NE-SW.
        {
            let doc = s.doc_mut().unwrap();
            let pres = Arc::make_mut(&mut doc.doc);
            let slide = Arc::make_mut(&mut pres.slides[0]);
            if let Some(sh) = slide.shapes.iter_mut().find(|x| x.id == sid)
                && let Some(x) = sh.xfrm.as_mut()
            {
                x.rot = 90.0;
            }
        }
        assert_eq!(resize_cursor(&s, sid, 0), egui::CursorIcon::ResizeNeSw, "90°: the diagonal is NE-SW");
        assert_eq!(resize_cursor(&s, sid, 2), egui::CursorIcon::ResizeNwSe, "90°: NE diagonal is NW-SE");
        assert_eq!(resize_cursor(&s, sid, 1), egui::CursorIcon::ResizeHorizontal, "90°: the top edge is horizontal");
    }

    /// #745: a corner handle shows a diagonal cursor whatever the shape's proportions. The
    /// corner's direction is the diagonal, not the line to the box's centre, so a wide
    /// rectangle's NW corner still drags along the NW-SE diagonal.
    #[test]
    fn a_wide_shape_keeps_its_corner_cursors_diagonal() {
        let mut s = Session::with_new();
        let id = s.execute("shape.insert", &serde_json::json!({"preset": "rect", "rect": [100.0, 100.0, 400.0, 80.0]})).unwrap()["id"]
            .as_u64()
            .unwrap() as u32;
        let sid = deckcraft_model::ShapeId(id);
        assert_eq!(resize_cursor(&s, sid, 0), egui::CursorIcon::ResizeNwSe, "the NW corner drags NW-SE");
        assert_eq!(resize_cursor(&s, sid, 4), egui::CursorIcon::ResizeNwSe, "the SE corner drags NW-SE");
        assert_eq!(resize_cursor(&s, sid, 2), egui::CursorIcon::ResizeNeSw, "the NE corner drags NE-SW");
        assert_eq!(resize_cursor(&s, sid, 6), egui::CursorIcon::ResizeNeSw, "the SW corner drags NE-SW");
        // The edges keep their own cursors.
        assert_eq!(resize_cursor(&s, sid, 1), egui::CursorIcon::ResizeVertical, "the top edge drags vertically");
        assert_eq!(resize_cursor(&s, sid, 3), egui::CursorIcon::ResizeHorizontal, "the right edge drags horizontally");
    }

    /// #745: a placeholder that inherits its box from the layout still shows a resize cursor.
    /// Its handles are drawn from the inherited box, so the cursor follows that one.
    #[test]
    fn a_placeholder_that_inherits_its_box_has_a_resize_cursor() {
        let s = Session::with_new();
        let st = s.doc().unwrap();
        let ph = st
            .current_slide()
            .unwrap()
            .shapes
            .iter()
            .find(|sh| sh.xfrm.is_none() && sh.ph.is_some())
            .expect("the new slide has a placeholder without a box of its own")
            .id;
        assert_eq!(resize_cursor(&s, ph, 0), egui::CursorIcon::ResizeNwSe, "the NW corner drags NW-SE");
        assert_eq!(resize_cursor(&s, ph, 2), egui::CursorIcon::ResizeNeSw, "the NE corner drags NE-SW");
        assert_eq!(resize_cursor(&s, ph, 1), egui::CursorIcon::ResizeVertical, "the top edge drags vertically");
    }

    fn harness() -> Harness<'static, SlideApp> {
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        h.run_steps(3);
        h
    }

    /// The slide point (in slide points) under screen position `p`.
    fn slide_point(h: &Harness<'_, SlideApp>, p: Pos2) -> Pos2 {
        let (r, s) = (h.state().slide_rect.unwrap(), h.state().canvas_scale);
        pos2((p.x - r.min.x) / s, (p.y - r.min.y) / s)
    }

    /// Test helper: inject a platform-command-modified line wheel event and run thirty harness
    /// steps.
    fn ctrl_wheel(h: &mut Harness<'_, SlideApp>, lines: f32) {
        let wheel = Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: vec2(0.0, lines),
            phase: TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::COMMAND,
        };
        h.event(wheel);
        // egui spreads a wheel notch over several frames.
        h.run_steps(30);
    }

    /// Within half a slide point, plus the half device pixel the slide moves to sit on the pixel
    /// grid (#184).
    fn assert_near(a: Pos2, b: Pos2, what: &str) {
        assert!((a - b).length() < 1.25, "{what}: {a:?} moved to {b:?}");
    }

    /// #106: wheel and pinch zoom zoomed toward the slide's corner instead of the pointer.
    #[test]
    fn wheel_and_pinch_zoom_keep_the_point_under_the_pointer() {
        let mut h = harness();
        let fit = h.state().canvas_scale;
        let r = h.state().slide_rect.unwrap();
        let p = r.min + vec2(r.width() * 0.8, r.height() * 0.7);
        h.event(Event::PointerMoved(p));
        h.run_steps(1);
        let under = slide_point(&h, p);

        h.event(Event::Zoom(2.0));
        h.run_steps(2);
        assert!((h.state().canvas_scale - fit * 2.0).abs() < 1e-3);
        assert_near(under, slide_point(&h, p), "pinch in");

        ctrl_wheel(&mut h, 1.0);
        assert!(h.state().canvas_scale > fit * 2.1, "Ctrl/Cmd + wheel up zooms in");
        assert_near(under, slide_point(&h, p), "wheel in");

        ctrl_wheel(&mut h, -1.0);
        ctrl_wheel(&mut h, -1.0);
        assert!(h.state().canvas_scale < fit * 2.0, "Ctrl/Cmd + wheel down zooms out");
        assert_near(under, slide_point(&h, p), "wheel out");

        h.event(Event::Zoom(0.8));
        h.run_steps(2);
        assert_near(under, slide_point(&h, p), "pinch out");

        // Zoomed out below the window size, the slide is centred again.
        h.event(Event::Zoom(0.25));
        h.run_steps(2);
        assert!(h.state().canvas_scale < fit);
        let (canvas, slide) = (h.state().canvas_rect.unwrap(), h.state().slide_rect.unwrap());
        assert!((canvas.center() - slide.center()).length() < 1.0, "{canvas:?} {slide:?}");
    }

    /// Zoom In/Out (keys, buttons, slider) keeps the middle of the view where it is.
    #[test]
    fn zoom_commands_keep_the_middle_of_the_view() {
        let mut h = harness();
        let c = h.state().canvas_rect.unwrap().center();
        let mid = slide_point(&h, c);
        for _ in 0..4 {
            h.state_mut().run("view.zoomIn", serde_json::json!({})).unwrap();
            h.run_steps(2);
            assert_near(mid, slide_point(&h, c), "zoom in");
        }
        h.state_mut().run("view.zoomOut", serde_json::json!({})).unwrap();
        h.run_steps(2);
        assert_near(mid, slide_point(&h, c), "zoom out");
    }

    /// Drags with the primary button from `a` to `b`; `during` runs after every move.
    fn drag_with(h: &mut Harness<'_, SlideApp>, a: Pos2, b: Pos2, mut during: impl FnMut(&SlideApp)) {
        h.hover_at(a);
        h.run_steps(1);
        h.event(Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.run_steps(1);
        for i in 1..=8 {
            h.event(Event::PointerMoved(a + (b - a) * (i as f32 / 8.0)));
            h.run_steps(1);
            during(h.state());
        }
        h.event(Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }

    /// #1157: a slide zoomed past the window has a scroll bar on each axis that overflows, and
    /// dragging its thumb pans the slide, not a marquee on the canvas.
    #[test]
    fn scroll_bars_pan_a_zoomed_slide() {
        let mut h = harness();
        let fit = h.state().canvas_scale;
        h.state_mut().ui.zoom = Some(fit * 3.0);
        h.run_steps(3);
        let canvas = h.state().canvas_rect.unwrap();
        let before = h.state().slide_rect.unwrap();
        let no_marquee =
            |app: &SlideApp| assert!(app.session.tool.marquee.is_none() && !app.session.tool.dragging(), "the bar drag reached the slide");

        // Zoomed about the middle, so each thumb sits in the middle of its track.
        let bottom = pos2(canvas.center().x, canvas.max.y - 4.0);
        drag_with(&mut h, bottom, bottom + vec2(150.0, 0.0), no_marquee);
        let after = h.state().slide_rect.unwrap();
        assert!(before.min.x - after.min.x > 150.0, "the horizontal thumb pans right: {before:?} -> {after:?}");
        assert!((before.min.y - after.min.y).abs() < 1.0, "only across: {before:?} -> {after:?}");

        let right = pos2(canvas.max.x - 4.0, canvas.center().y);
        drag_with(&mut h, right, right + vec2(0.0, 100.0), no_marquee);
        let down = h.state().slide_rect.unwrap();
        assert!(after.min.y - down.min.y > 100.0, "the vertical thumb pans down: {after:?} -> {down:?}");
        assert!((after.min.x - down.min.x).abs() < 1.0, "only down: {after:?} -> {down:?}");

        // A click on the track beside the thumb pages back toward it.
        let left = pos2(canvas.min.x + 20.0, canvas.max.y - 4.0);
        h.hover_at(left);
        h.run_steps(1);
        h.event(Event::PointerButton { pos: left, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.event(Event::PointerButton { pos: left, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.run_steps(2);
        let paged = h.state().slide_rect.unwrap();
        assert!(paged.min.x - down.min.x > 100.0, "a track click pages left: {down:?} -> {paged:?}");
        assert!(h.state().session.tool.marquee.is_none());

        // The ends of the tracks reach the slide's far edges.
        drag_with(&mut h, right, right + vec2(0.0, 2000.0), |_| {});
        let end = h.state().slide_rect.unwrap();
        assert!((end.max.y - (canvas.max.y - 28.0)).abs() < 2.0, "the bottom of the slide comes into view: {end:?} in {canvas:?}");
    }

    /// A press on the thumb that ends just past its end is a nudge, not a page: the track click
    /// is decided where the button went down, not where it came up.
    #[test]
    fn a_short_press_on_the_thumb_does_not_page() {
        let mut h = harness();
        let fit = h.state().canvas_scale;
        h.state_mut().ui.zoom = Some(fit * 3.0);
        h.run_steps(3);
        let (canvas, before) = (h.state().canvas_rect.unwrap(), h.state().slide_rect.unwrap());
        let overflow = vec2((before.width() + 56.0 - canvas.width()).max(0.0), (before.height() + 56.0 - canvas.height()).max(0.0));
        let offset = canvas.min + vec2(28.0, 28.0) - before.min;
        let (_, thumb, _) = scroll_bar(canvas, 0.0, overflow, offset, 0).unwrap();
        let (a, b) = (pos2(thumb.max.x - 3.0, canvas.max.y - 4.0), pos2(thumb.max.x + 3.0, canvas.max.y - 4.0));
        h.hover_at(a);
        h.run_steps(1);
        h.event(Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.run_steps(1);
        h.event(Event::PointerMoved(b));
        h.run_steps(1);
        h.event(Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.run_steps(2);
        let after = h.state().slide_rect.unwrap();
        let moved = before.min.x - after.min.x;
        assert!((0.0..40.0).contains(&moved), "a 6 pt nudge of the thumb, not a page: {before:?} -> {after:?}");
    }

    /// Only the primary button drags a thumb: a right or middle drag after a left one leaves the
    /// slide where it is.
    #[test]
    fn only_the_primary_button_drags_a_thumb() {
        let mut h = harness();
        let fit = h.state().canvas_scale;
        h.state_mut().ui.zoom = Some(fit * 3.0);
        h.run_steps(3);
        let canvas = h.state().canvas_rect.unwrap();
        let bottom = pos2(canvas.center().x, canvas.max.y - 4.0);
        drag_with(&mut h, bottom, bottom + vec2(40.0, 0.0), |_| {});
        let before = h.state().slide_rect.unwrap();
        for button in [egui::PointerButton::Secondary, egui::PointerButton::Middle] {
            let a = pos2(canvas.center().x + 40.0, canvas.max.y - 4.0);
            h.hover_at(a);
            h.run_steps(1);
            h.event(Event::PointerButton { pos: a, button, pressed: true, modifiers: Modifiers::NONE });
            h.run_steps(1);
            for i in 1..=4 {
                h.event(Event::PointerMoved(a + vec2(30.0 * i as f32, 0.0)));
                h.run_steps(1);
            }
            h.event(Event::PointerButton { pos: a + vec2(120.0, 0.0), button, pressed: false, modifiers: Modifiers::NONE });
            h.run_steps(2);
            let after = h.state().slide_rect.unwrap();
            assert!((before.min - after.min).length() < 1.0, "{button:?} drag moved the slide: {before:?} -> {after:?}");
        }
    }

    /// A slide that fits has no scroll bars: a drag along the canvas edge is a marquee.
    #[test]
    fn a_fitted_slide_has_no_scroll_bars() {
        let mut h = harness();
        let canvas = h.state().canvas_rect.unwrap();
        let a = pos2(canvas.center().x, canvas.max.y - 4.0);
        let mut marquee = false;
        drag_with(&mut h, a, a + vec2(150.0, 0.0), |app| marquee |= app.session.tool.marquee.is_some());
        assert!(marquee, "the drag selects on the canvas");
    }

    fn app_with_table() -> (egui_kittest::Harness<'static, SlideApp>, deckcraft_model::ShapeId) {
        let mut s = Session::with_new();
        let data = json!([["Hello world", "b"], ["c", ""]]);
        let r = s.execute("insert.table", &json!({"rows": 2, "cols": 2, "rect": [100.0, 100.0, 400.0, 80.0], "data": data})).unwrap();
        let id = deckcraft_model::ShapeId(r["id"].as_u64().unwrap() as u32);
        s.execute("edit.deselect", &json!({})).unwrap();
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(s, Services::default()),
        );
        // The app keeps requesting repaints, so step frames instead of run().
        harness.run_steps(2);
        (harness, id)
    }

    /// Screen position of slide point (x, y).
    fn screen(app: &SlideApp, x: f64, y: f64) -> Pos2 {
        Xf { origin: app.slide_rect.unwrap().min, scale: app.canvas_scale }.to_screen(Point::new(x, y))
    }

    /// Press and release at `p`, 50 ms apart in input time (two clicks make a double-click).
    fn click(harness: &mut egui_kittest::Harness<'static, SlideApp>, p: Pos2) {
        for pressed in [true, false] {
            let t = harness.ctx.input(|i| i.time) + 0.05;
            harness.input_mut().time = Some(t);
            harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            harness.step();
        }
    }

    /// The first empty placeholder and the paragraph after Enter keep a list marker and indent.
    #[test]
    fn empty_bullet_preview_follows_click_type_enter_and_toggle() {
        let mut h = harness();
        h.state_mut().run("slide.new", json!({"layout": "titleAndContent"})).unwrap();
        h.run_steps(3);
        let st = h.state().session.active().unwrap();
        let sh = st
            .shapes()
            .iter()
            .find(|s| s.ph_type() == Some(deckcraft_model::PhType::Body) || s.ph_type() == Some(deckcraft_model::PhType::Obj))
            .unwrap();
        let id = sh.id;
        let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
        let p = screen(h.state(), x.x + 40.0, x.y + 20.0);
        click(&mut h, p);
        h.run_steps(3);
        let caret = |h: &Harness<'_, SlideApp>| {
            let st = h.state().session.active().unwrap();
            let ts = st.selection.text.as_ref().expect("click starts typing");
            assert_eq!(ts.shape, id);
            deckcraft_engine::cmd::text::caret_line(st, ts).unwrap().0
        };
        let first = caret(&h);
        h.event(Event::Text("First point".into()));
        h.run_steps(3);
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        let second = caret(&h);
        assert!((first.x - second.x).abs() < 0.01, "empty lines retain the bullet indent");
        assert!(second.y > first.y);
        let st = h.state().session.active().unwrap();
        let l = deckcraft_engine::cmd::text::layout_for(st, st.selection.text.as_ref().unwrap()).unwrap();
        assert_eq!(l.empty_bullets.len(), 1);
        assert_eq!(l.empty_bullets[0].para, 1);
        let renders = h.state().textures.canvas_renders;
        h.state_mut().run("text.exit", json!({})).unwrap();
        h.run_steps(3);
        assert!(h.state().textures.canvas_renders > renders, "leaving text editing refreshes the marker texture");
        h.state_mut().run("text.edit", json!({"id": id, "at": [1, 0]})).unwrap();
        h.run_steps(3);
        h.event(Event::Text("Second point".into()));
        h.run_steps(3);
        let st = h.state().session.active().unwrap();
        let ts = st.selection.text.as_ref().unwrap();
        assert_eq!(deckcraft_engine::cmd::text::body_of(st, ts).unwrap().text(), "First point\nSecond point");
        assert!(deckcraft_engine::cmd::text::layout_for(st, ts).unwrap().empty_bullets.is_empty());
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        h.state_mut().run("format.bullets", json!({})).unwrap();
        h.run_steps(3);
        let st = h.state().session.active().unwrap();
        assert!(deckcraft_engine::cmd::text::layout_for(st, st.selection.text.as_ref().unwrap()).unwrap().empty_bullets.is_empty());
    }

    /// Double-clicking a table cell edits it with the caret drawn in that cell, where clicked.
    #[test]
    fn double_click_in_a_table_cell_shows_the_caret_there() {
        let (mut harness, id) = app_with_table();
        // The empty cell (1, 1) spans x 300..500, y 140..180.
        let p = screen(harness.state(), 400.0, 160.0);
        harness.hover_at(p);
        harness.step();
        click(&mut harness, p);
        click(&mut harness, p);
        harness.run_steps(2);
        let st = harness.state().session.active().unwrap();
        let ts = st.selection.text.clone().expect("editing the cell");
        assert_eq!((ts.shape, ts.cell), (id, Some((1, 1))));
        assert!(text_marks(st, &ts).is_some_and(|m| m.selection.is_empty()), "no range");
        let (top, bottom) = deckcraft_engine::cmd::text::caret_line(st, &ts).expect("the cell's text has a caret");
        for q in [top, bottom] {
            assert!(q.x > 300.0 && q.x < 500.0 && q.y >= 140.0 && q.y <= 180.0, "caret {q:?} outside cell (1, 1)");
        }
        // A click in another cell moves the caret there.
        let p = screen(harness.state(), 110.0, 120.0);
        click(&mut harness, p);
        harness.run_steps(1);
        let st = harness.state().session.active().unwrap();
        let ts = st.selection.text.clone().unwrap();
        assert_eq!((ts.cell, ts.caret), (Some((0, 0)), (0, 0)));
        let (top, _) = deckcraft_engine::cmd::text::caret_line(st, &ts).unwrap();
        assert!(top.x > 100.0 && top.x < 120.0 && top.y >= 100.0 && top.y < 140.0, "caret {top:?} at the start of cell (0, 0)");
    }

    /// Table borders show a resize cursor and drag to resize columns.
    #[test]
    fn table_borders_resize_with_the_pointer() {
        let (mut harness, id) = app_with_table();
        let p = screen(harness.state(), 300.0, 120.0);
        harness.hover_at(p);
        harness.step();
        harness.step();
        assert_eq!(harness.output().platform_output.cursor_icon, egui::CursorIcon::ResizeColumn);
        let q = screen(harness.state(), 340.0, 120.0);
        harness.drag_at(p);
        harness.step();
        for k in 1..=4 {
            harness.hover_at(p + (q - p) * k as f32 / 4.0);
            harness.step();
        }
        harness.drop_at(q);
        harness.run_steps(2);
        let st = harness.state().session.active().unwrap();
        let deckcraft_model::ShapeKind::Table(t) = &st.shape(id).unwrap().kind else { panic!("not a table") };
        assert!((t.cols[0] - 240.0).abs() < 0.5 && (t.cols[1] - 160.0).abs() < 0.5, "{:?}", t.cols);
    }

    fn right_click(h: &mut Harness<'_, SlideApp>, pos: Pos2) {
        h.event(Event::PointerMoved(pos));
        h.step();
        for pressed in [true, false] {
            h.event(Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
            h.step();
        }
        h.run_steps(2);
    }

    /// In Edit Points, right-clicking a segment offers Straight/Curved Segment and right-clicking
    /// a point offers Open Path, then Close Path once the outline is open.
    #[test]
    fn edit_points_menu_sets_segment_types_and_opens_the_path() {
        let mut h = harness();
        h.state_mut().run("slide.layout", json!({"layout": "blank"})).unwrap();
        h.state_mut().run("shape.insert", json!({"preset": "rect", "rect": [200, 150, 300, 200]})).unwrap();
        h.state_mut().run("shape.editPoints", json!({"on": true})).unwrap();
        h.run_steps(3);
        let q = |h: &mut Harness<'_, SlideApp>| h.state_mut().session.execute("shape.points", &json!({})).unwrap();
        // The middle of the right edge: the segment ending at point 2.
        let p = screen(h.state(), 500.0, 250.0);
        right_click(&mut h, p);
        h.get_by_label("Curved Segment").click();
        h.run_steps(3);
        assert_eq!(q(&mut h)["segments"][0], json!(["straight", "straight", "curved", "straight"]));
        // The bottom-right corner: Open Path breaks the outline there.
        let p = screen(h.state(), 500.0, 350.0);
        right_click(&mut h, p);
        h.get_by_label("Open Path").click();
        h.run_steps(3);
        assert_eq!(q(&mut h)["paths"][0].as_array().map(Vec::len), Some(5));
        right_click(&mut h, p);
        h.get_by_label("Close Path").click();
        h.run_steps(3);
        assert_eq!(q(&mut h)["paths"][0].as_array().map(Vec::len), Some(4));
    }

    /// In Edit Points, right-clicking a segment offers Add Point at the pointer, Delete Segment on
    /// a closed outline, and Open Path at the segment's start point.
    #[test]
    fn edit_points_segment_menu_adds_points_and_deletes_segments() {
        let mut h = harness();
        h.state_mut().run("slide.layout", json!({"layout": "blank"})).unwrap();
        h.state_mut().run("shape.insert", json!({"preset": "rect", "rect": [200, 150, 300, 200]})).unwrap();
        h.state_mut().run("shape.editPoints", json!({"on": true})).unwrap();
        h.run_steps(3);
        let q = |h: &mut Harness<'_, SlideApp>| h.state_mut().session.execute("shape.points", &json!({})).unwrap();
        // A quarter of the way along the top edge: the new point goes there.
        let p = screen(h.state(), 275.0, 150.0);
        right_click(&mut h, p);
        h.get_by_label("Add Point").click();
        h.run_steps(3);
        assert_eq!(q(&mut h)["paths"][0], json!([[200.0, 150.0], [275.0, 150.0], [500.0, 150.0], [500.0, 350.0], [200.0, 350.0]]));
        // The right edge: Open Path opens the outline at its top end.
        let p = screen(h.state(), 500.0, 250.0);
        right_click(&mut h, p);
        h.get_by_label("Open Path").click();
        h.run_steps(3);
        assert_eq!(q(&mut h)["paths"][0][0], json!([500.0, 150.0]));
        assert_eq!(q(&mut h)["paths"][0].as_array().map(Vec::len), Some(6));
        h.state_mut().run("edit.undo", json!({})).unwrap();
        h.run_steps(3);
        // Delete Segment removes the right edge: the outline now runs from its bottom end to its top end.
        right_click(&mut h, p);
        h.get_by_label("Delete Segment").click();
        h.run_steps(3);
        assert_eq!(q(&mut h)["paths"][0], json!([[500.0, 350.0], [200.0, 350.0], [200.0, 150.0], [275.0, 150.0], [500.0, 150.0]]));
        assert_eq!(q(&mut h)["segments"][0][0], json!(null));
        // On the open outline Delete Segment is grayed out; Close Path joins the ends again.
        let p = screen(h.state(), 350.0, 350.0);
        right_click(&mut h, p);
        assert!(h.get_by_label("Delete Segment").accesskit_node().is_disabled());
        h.get_by_label("Close Path").click();
        h.run_steps(3);
        assert_eq!(q(&mut h)["segments"][0][0], json!("straight"));
    }

    #[test]
    fn grid_step_follows_spacing_and_rejects_unusable_values() {
        let step = super::grid_line_step(72.0, 0.5).unwrap_or(0.0);
        assert!((step - 36.0).abs() < 1e-3, "{step}");
        assert!(super::grid_line_step(6.0, 1.0).is_some());
        assert!(super::grid_line_step(0.0, 1.0).is_none());
        assert!(super::grid_line_step(f64::NAN, 1.0).is_none());
        assert!(super::grid_line_step(1_000.0, 1.0).is_none());
        assert!(super::grid_line_step(6.0, 0.0).is_none());
        assert!(super::grid_line_step(6.0, 0.1).is_none(), "a 6 pt grid at 10% zoom is finer than one screen point");
    }

    /// #830: when the Format or Animation Painter is armed, the cursor shows the copy icon so the
    /// user can see the tool is waiting for a click.
    #[test]
    fn format_painter_sets_the_cursor() {
        let mut h = harness();
        // Hover over the canvas so the pointer function runs and sets the cursor.
        h.hover_at(egui::pos2(600.0, 400.0));
        h.run_steps(2);
        let before = h.output().platform_output.cursor_icon;
        assert_ne!(before, egui::CursorIcon::Copy, "no painter is armed yet");
        // Arming the format painter overrides the cursor to Copy (#830).
        h.state_mut().session.painter = Some((Default::default(), false));
        h.run_steps(3);
        let after = h.output().platform_output.cursor_icon;
        assert_eq!(after, egui::CursorIcon::Copy, "the format painter arms the Copy cursor");
    }

    /// #830: the Animation Painter arms the same cursor.
    #[test]
    fn animation_painter_sets_the_cursor() {
        let mut h = harness();
        h.hover_at(egui::pos2(600.0, 400.0));
        h.run_steps(2);
        let before = h.output().platform_output.cursor_icon;
        assert_ne!(before, egui::CursorIcon::Copy, "no painter is armed yet");
        h.state_mut().session.anim_painter = Some((vec![], false));
        h.run_steps(3);
        let after = h.output().platform_output.cursor_icon;
        assert_eq!(after, egui::CursorIcon::Copy, "the animation painter arms the Copy cursor");
    }
}

#[cfg(test)]
#[path = "ruler_tests.rs"]
mod ruler_tests;

#[cfg(test)]
#[path = "guides_tests.rs"]
mod guides_tests;
