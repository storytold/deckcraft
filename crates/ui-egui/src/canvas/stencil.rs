//! Draw ▸ Stencils ▸ Ruler on the canvas: painting the translucent scale, moving and turning it,
//! and typing an exact angle. The geometry and the commands are in the engine
//! (`deckcraft_engine::ruler`, `cmd::draw`); this module only turns pointer and wheel input into
//! `draw.ruler` calls, so the state stays engine-owned and every change is one command.
//!
//! It never touches the text ruler or the page rulers: those sit in the canvas margins and are
//! drawn and handled before this overlay.

use deckcraft_engine::ToolKind;
use deckcraft_engine::ruler::{self, RulerState};
use deckcraft_geom::Point;
use egui::epaint::TextShape;
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2, vec2};

use super::{SlideApp, Tokens, Ui, Xf};

/// What a press on the ruler started.
#[derive(Clone, Copy, Debug)]
enum Grab {
    /// Dragging the body, from the press point and the position then.
    Move { from: Point, x: f64, y: f64 },
    /// Dragging the dial around, from the pointer's start bearing, the angle then, and whether the
    /// pointer moved (a click that did not is a request to type the angle).
    Turn { bearing: f64, angle: f64, moved: bool },
}

/// The typed-angle field, in egui's temporary store so it needs no application field.
#[derive(Clone, Debug, Default)]
struct AngleEdit {
    text: String,
}

/// The temp-store key of the typed-angle field.
fn edit_id(ui: &Ui) -> egui::Id {
    ui.scope_id().with("stencil_ruler_angle")
}

/// The temp-store key of the active grab.
fn grab_id(ui: &Ui) -> egui::Id {
    ui.scope_id().with("stencil_ruler_grab")
}

/// Wheel notches left over from the last frame: a trackpad sends a few points at a time, so the
/// fraction that did not yet reach a whole notch is carried, as the slide wheel does.
fn wheel_id(ui: &Ui) -> egui::Id {
    ui.scope_id().with("stencil_ruler_wheel")
}

/// A slide point on screen.
fn to_pos(xf: Xf, p: Point) -> Pos2 {
    xf.to_screen(p)
}

/// A unit direction and normal for the ruler's angle.
fn axes(r: &RulerState) -> (Vec2, Vec2) {
    let a = ruler::normalize_angle(r.angle).to_radians() as f32;
    (vec2(a.cos(), a.sin()), vec2(-a.sin(), a.cos()))
}

/// The body's four corners on screen.
fn body(r: &RulerState, xf: Xf) -> Vec<Pos2> {
    let (dir, nor) = axes(r);
    let c = to_pos(xf, r.center());
    let half = (ruler::LENGTH / 2.0) as f32 * xf.scale;
    let wide = ruler::HALF_WIDTH as f32 * xf.scale;
    let (a, b) = (dir * half, nor * wide);
    vec![c - a - b, c + a - b, c + a + b, c - a + b]
}

/// Is the pointer over the ruler body or its dial?
pub(super) fn hit(r: &RulerState, xf: Xf, at: Pos2) -> bool {
    let p = xf.to_slide(at);
    ruler::contains(r, p) || ruler::on_dial(r, p)
}

/// The text ruler's and page rulers' 16 pt strips belong to them, not to the stencil.
fn over_page_ruler(app: &SlideApp, avail: Rect, at: Pos2) -> bool {
    app.ui.ruler && (at.y < avail.min.y + 16.0 || at.x < avail.min.x + 16.0)
}

/// The wheel over the ruler turns it 1° per notch around the pointer. Returns whether the pointer
/// was over it, so the caller keeps the wheel from scrolling or turning the page. `canvas_hovered`
/// is the canvas response's own hover: a popup or menu over the body takes the hover away from the
/// canvas, and then the wheel belongs to the popup, not to the ruler.
pub(super) fn wheel(app: &mut SlideApp, ui: &Ui, xf: Xf, canvas_hovered: bool, hover: Option<Pos2>, avail: Rect) -> bool {
    let r = app.session.view.ruler;
    if !canvas_hovered || !r.visible || app.session.tool.dragging() || app.anim_preview_active() {
        return false;
    }
    let Some(at) = hover.filter(|p| avail.contains(*p)) else { return false };
    if !hit(&r, xf, at) || over_page_ruler(app, avail, at) {
        return false;
    }
    // Read the raw wheel notches, not the smoothed delta: one notch is one degree. A trackpad
    // delivers a fraction of a notch per frame, so the remainder is kept for the next frame.
    let mut notches: f64 = ui.data_mut(|d| d.get_temp(wheel_id(ui)).unwrap_or(0.0));
    ui.input(|i| {
        for e in &i.events {
            if let egui::Event::MouseWheel { unit, delta, .. } = e {
                let y = f64::from(delta.y);
                if !y.is_finite() {
                    continue;
                }
                let n = match unit {
                    egui::MouseWheelUnit::Line => y,
                    // A point-unit wheel reports about 50 points for one notch.
                    egui::MouseWheelUnit::Point => y / 50.0,
                    egui::MouseWheelUnit::Page => y * 5.0,
                };
                // A change of direction drops what was left, so reversing turns immediately.
                if notches * n < 0.0 {
                    notches = 0.0;
                }
                notches += n;
            }
        }
    });
    // Whole notches only: a slow trackpad flick should not jitter the angle.
    let steps = notches.trunc().clamp(-90.0, 90.0);
    notches = if steps.abs() >= 90.0 { 0.0 } else { notches - steps };
    ui.data_mut(|d| d.insert_temp(wheel_id(ui), notches));
    if steps != 0.0 {
        let turned = ruler::rotate_about(&r, xf.to_slide(at), -steps);
        let _ = app.run("draw.ruler", serde_json::json!({"x": turned.x, "y": turned.y, "angle": turned.angle}));
    }
    true
}

/// Paint the ruler and handle presses and drags on it. Returns `true` when it owns the pointer
/// this frame (the slide must not also receive the press).
pub(super) fn overlay(app: &mut SlideApp, ui: &mut Ui, painter: &egui::Painter, xf: Xf, avail: Rect, t: &Tokens) -> bool {
    let r = app.session.view.ruler;
    if !r.visible {
        ui.data_mut(|d| d.remove_temp::<Option<Grab>>(grab_id(ui)));
        ui.data_mut(|d| d.remove_temp::<AngleEdit>(edit_id(ui)));
        ui.data_mut(|d| d.remove_temp::<f64>(wheel_id(ui)));
        return false;
    }
    paint(painter, xf, &r, t, app.session.prefs.units);
    let dial_rect = Rect::from_center_size(to_pos(xf, r.center()), vec2(2.0 * ruler::DIAL as f32 * xf.scale, 2.0 * ruler::DIAL as f32 * xf.scale));
    if ui.data_mut(|d| d.get_temp::<AngleEdit>(edit_id(ui))).is_some() {
        return angle_editor(app, ui, &dial_rect);
    }
    if app.session.tool.dragging() || app.anim_preview_active() {
        return false;
    }
    // With a pen, the ruler is a drawing guide: the stroke starts on the slide even over the body.
    if matches!(app.session.tool.kind, ToolKind::Ink { .. }) {
        return false;
    }
    let (pressed, down, released, at, motion) =
        ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.any_released(), i.pointer.interact_pos(), i.pointer.delta()));
    let over_canvas = at.is_some_and(|p| avail.contains(p));
    let mut grab: Option<Grab> = ui.data_mut(|d| d.get_temp::<Option<Grab>>(grab_id(ui))).flatten();
    let mut consumed = false;
    if pressed
        && over_canvas
        && let Some(at) = at
        && !over_page_ruler(app, avail, at)
        && hit(&r, xf, at)
    {
        let slide = xf.to_slide(at);
        grab = Some(if ruler::on_dial(&r, slide) {
            let bearing = atan2_deg(r.center(), slide);
            Grab::Turn { bearing, angle: r.angle, moved: false }
        } else {
            Grab::Move { from: slide, x: r.x, y: r.y }
        });
        consumed = true;
        ui.ctx().request_repaint();
    }
    if down && let (Some(g), Some(at)) = (grab, at) {
        let slide = xf.to_slide(at);
        consumed = true;
        match g {
            Grab::Move { from, x, y } => {
                let d = slide - from;
                let _ = app.run("draw.ruler", serde_json::json!({"x": x + d.x, "y": y + d.y}));
            }
            Grab::Turn { bearing, angle, moved } => {
                let turn = atan2_deg(r.center(), slide) - bearing;
                let _ = app.run("draw.ruler", serde_json::json!({"angle": ruler::normalize_angle(angle + turn)}));
                grab = Some(Grab::Turn { bearing, angle, moved: moved || motion != Vec2::ZERO });
            }
        }
    }
    if released {
        if let Some(Grab::Turn { moved: false, .. }) = grab
            && let Some(at) = at
            && ruler::on_dial(&r, xf.to_slide(at))
        {
            // A click on the degree readout types an exact angle; the field starts empty so the
            // typed number replaces the angle the dial already shows.
            ui.data_mut(|d| d.insert_temp(edit_id(ui), AngleEdit::default()));
            ui.ctx().request_repaint();
            consumed = true;
        }
        grab = None;
    }
    ui.data_mut(|d| d.insert_temp(grab_id(ui), grab));
    // The pointer moved while a ruler grab was active: still the ruler's frame.
    consumed || (grab.is_some() && motion != Vec2::ZERO)
}

fn atan2_deg(from: Point, to: Point) -> f64 {
    (to.y - from.y).atan2(to.x - from.x) * 180.0 / std::f64::consts::PI
}

/// The degree field on the dial: Return commits, Escape cancels, clicking away commits.
fn angle_editor(app: &mut SlideApp, ui: &mut Ui, dial: &Rect) -> bool {
    let Some(mut ed) = ui.data_mut(|d| d.get_temp::<AngleEdit>(edit_id(ui))) else { return true };
    let field = Rect::from_center_size(dial.center(), vec2(78.0, 22.0));
    let resp = ui.put(
        field,
        egui::TextEdit::singleline(&mut ed.text)
            .align(egui::Align2::CENTER_CENTER)
            .font(egui::FontId::proportional(12.0))
            .margin(vec2(3.0, 2.0))
            .hint_text("0-360"),
    );
    if !resp.has_focus() {
        resp.request_focus();
    }
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter)) && (resp.has_focus() || resp.lost_focus());
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        ui.ctx().memory_mut(|m| m.stop_text_input());
        ui.data_mut(|d| d.remove_temp::<AngleEdit>(edit_id(ui)));
        return true;
    }
    if enter || resp.lost_focus() {
        if let Some(v) = ed.text.trim().trim_end_matches(['°', ' ']).trim().parse::<f64>().ok().filter(|v| v.is_finite()) {
            let _ = app.run("draw.ruler", serde_json::json!({"angle": ruler::normalize_angle(v)}));
        }
        ui.data_mut(|d| d.remove_temp::<AngleEdit>(edit_id(ui)));
        return true;
    }
    ui.data_mut(|d| d.insert_temp(edit_id(ui), ed));
    true
}

/// The ruler body, its scale and the degree dial.
fn paint(p: &egui::Painter, xf: Xf, r: &RulerState, t: &Tokens, unit: deckcraft_geom::Unit) {
    let quad = body(r, xf);
    let shadow: Vec<Pos2> = quad.iter().map(|q| *q + vec2(1.5, 2.5)).collect();
    p.add(egui::Shape::convex_polygon(shadow, Color32::from_rgba_unmultiplied(0, 0, 0, 30), Stroke::NONE));
    let body_fill = Color32::from_rgba_unmultiplied(150, 150, 150, 64);
    p.add(egui::Shape::convex_polygon(quad, body_fill, Stroke::new(1.0, t.text_faint)));
    let (dir, nor) = axes(r);
    let half = (ruler::LENGTH / 2.0) as f32;
    let edge = ruler::HALF_WIDTH as f32;
    let scale = xf.scale;
    let c = to_pos(xf, r.center());
    // The deck's unit sets the scale: quarter inches or half centimetres between minor ticks.
    let (step, sub) = super::rulers::ruler_steps(unit);
    let major = step as f32;
    let minor = (step / f64::from(sub.max(1))) as f32;
    // Ticks and numbers are both counted from the left end, as on a ruler, so each long tick sits
    // under its number and neither runs past the ends of the body.
    let left = c - dir * half * scale;
    let n = (ruler::LENGTH as f32 / minor).floor() as i32;
    for i in 0..=n {
        let at = i as f32 * minor;
        let q = left + dir * at * scale;
        let fraction = (at / major).fract().abs();
        let is_major = fraction < 0.01 || 1.0 - fraction < 0.01;
        let len = if is_major { 11.0 } else { 5.5 };
        // Both edges, ticks pointing into the body.
        p.line_segment([q - nor * edge * scale, q - nor * (edge - len) * scale], Stroke::new(1.0, t.text_faint));
        p.line_segment([q + nor * edge * scale, q + nor * (edge - len) * scale], Stroke::new(1.0, t.text_faint));
    }
    // A number at each major step on both edges, counted from the left end as on a ruler.
    let a = text_angle(r);
    for v in 0..=(ruler::LENGTH as f32 / major).floor() as i32 {
        let q = left + dir * (v as f32 * major) * scale;
        let label = format!("{v}");
        for side in [1.0, -1.0] {
            let at = q + nor * (side * 8.0) * scale;
            let galley = p.layout_no_wrap(label.clone(), FontId::proportional(9.0), t.text_dim);
            p.add(TextShape::new(at, galley, t.text_dim).with_angle(a));
        }
    }
    // The degree dial: the rotation grip and the readout.
    let rad = ruler::DIAL as f32 * xf.scale;
    p.circle_filled(c + vec2(1.0, 1.5), rad, Color32::from_rgba_unmultiplied(0, 0, 0, 30));
    p.circle(c, rad, Color32::from_rgba_unmultiplied(250, 250, 250, 225), Stroke::new(1.0, t.text_faint));
    let red = Color32::from_rgb(0xC0, 0x39, 0x2B);
    for k in 0..12 {
        let ang = (k as f32) / 12.0 * std::f32::consts::TAU;
        let d = vec2(ang.cos(), ang.sin());
        if k % 3 == 0 {
            p.line_segment([c + d * (rad - 0.5), c + d * (rad - 4.0)], Stroke::new(1.4, red));
        }
    }
    p.text(c, Align2::CENTER_CENTER, format!("{:.0}", r.angle), FontId::proportional(12.0), t.text);
}

/// Text along the ruler but never upside down, so both edges read.
fn text_angle(r: &RulerState) -> f32 {
    let mut a = ruler::normalize_angle(r.angle) as f32;
    if a > 90.0 && a <= 270.0 {
        a += 180.0;
    }
    a.to_radians()
}
