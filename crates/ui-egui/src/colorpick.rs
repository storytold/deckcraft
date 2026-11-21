//! A colour picker as in canvas editors (after FigCraft's `colorpicker.rs`): a saturation /
//! brightness square, a hue bar, an optional opacity bar, and the colour as Hex, RGB or HSB
//! fields. The hue is remembered while the colour is grey, so dragging to white and back keeps it.

use deckcraft_color::Rgba;
use egui::{Color32, CornerRadius, Id, Mesh, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::theme::{self, Tokens};

/// Hue (0–360), saturation and brightness (0–1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hsb {
    pub h: f32,
    pub s: f32,
    pub b: f32,
}

impl Hsb {
    pub fn from_rgb(c: Rgba) -> Self {
        let (r, g, b) = (c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        let h = if d <= f32::EPSILON {
            0.0
        } else if max == r {
            60.0 * ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        Hsb { h, s: if max <= f32::EPSILON { 0.0 } else { d / max }, b: max }
    }

    pub fn to_rgb(self, a: u8) -> Rgba {
        let h = self.h.rem_euclid(360.0) / 60.0;
        let c = self.b * self.s;
        let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = self.b - c;
        let ch = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
        Rgba::rgba(ch(r), ch(g), ch(b), a)
    }
}

/// What the picker did this frame: the colour while dragging (`done` false), or a finished
/// change (`done` true: the drag ended, or a field was typed).
pub struct Picked {
    pub color: Rgba,
    pub done: bool,
}

const MODELS: [&str; 3] = ["Hex", "RGB", "HSB"];

/// The picker for `color`, 220 points wide. `alpha`: show the opacity bar and field.
pub fn picker(ui: &mut Ui, id: Id, color: Rgba, alpha: bool) -> Option<Picked> {
    let t = Tokens::get(ui.ctx());
    // The hue and saturation the picker is on: kept while the colour is grey or black.
    let state_id = id.with("hsb");
    let shown = Hsb::from_rgb(color);
    // While a drag is under way the picker follows itself (the caller may only take the colour
    // when the drag ends); otherwise it follows `color`.
    let (mut hsb, mut a): (Hsb, u8) = match ui.data(|d| d.get_temp::<(Hsb, Rgba, bool)>(state_id)) {
        Some((h, last, true)) => (h, last.a),
        Some((h, last, _)) if last == color => (h, color.a),
        Some((h, _, _)) if shown.s <= 0.001 || shown.b <= 0.001 => {
            (Hsb { b: shown.b, s: if shown.b <= 0.001 { h.s } else { shown.s }, h: h.h }, color.a)
        }
        _ => (shown, color.a),
    };
    let color = if ui.data(|d| d.get_temp::<(Hsb, Rgba, bool)>(state_id)).is_some_and(|s| s.2) { hsb.to_rgb(a) } else { color };
    let mut out: Option<Picked> = None;
    let w = 220.0;
    ui.spacing_mut().item_spacing.y = 8.0;
    let (sq, _) = ui.allocate_exact_size(vec2(w, 150.0), Sense::hover());
    let (changed, done) = sv_square(ui, sq, &mut hsb, id);
    if changed || done {
        out = Some(Picked { color: hsb.to_rgb(a), done });
    }
    let (hr, _) = ui.allocate_exact_size(vec2(w, 12.0), Sense::hover());
    let (changed, done) = hue_slider(ui, hr, &mut hsb, id);
    if changed || done {
        out = Some(Picked { color: hsb.to_rgb(a), done });
    }
    if alpha {
        let (ar, _) = ui.allocate_exact_size(vec2(w, 12.0), Sense::hover());
        if let Some((v, done)) = alpha_slider(ui, ar, hsb.to_rgb(255), a, id) {
            a = (v * 255.0).round() as u8;
            out = Some(Picked { color: hsb.to_rgb(a), done });
        }
    }
    // The colour as numbers.
    let model_id = id.with("model");
    let model: usize = ui.data(|d| d.get_temp(model_id)).unwrap_or(0).min(MODELS.len() - 1);
    let current = out.as_ref().map(|p| p.color).unwrap_or(color);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let label = MODELS.get(model).copied().unwrap_or("Hex");
        let r = ui.add(egui::Button::new(egui::RichText::new(label).font(theme::font(12.0))).fill(t.field).min_size(vec2(44.0, 24.0)));
        egui::Popup::menu(&r).show(|ui| {
            for (i, m) in MODELS.iter().enumerate() {
                if ui.selectable_label(i == model, *m).clicked() {
                    ui.data_mut(|d| d.insert_temp(model_id, i));
                    ui.close();
                }
            }
        });
        let typed = match label {
            "RGB" => {
                let vals = [current.r, current.g, current.b];
                let mut res = None;
                for (k, v) in vals.iter().enumerate() {
                    if let Some(n) = num_field(ui, id.with(("rgb", k)), f64::from(*v), 34.0) {
                        let mut rgb = vals;
                        if let Some(slot) = rgb.get_mut(k) {
                            *slot = n.clamp(0.0, 255.0).round() as u8;
                        }
                        res = Some(Rgba::rgba(rgb[0], rgb[1], rgb[2], current.a));
                    }
                }
                res
            }
            "HSB" => {
                let vals = [f64::from(hsb.h).round(), f64::from(hsb.s * 100.0).round(), f64::from(hsb.b * 100.0).round()];
                let mut res = None;
                for (k, v) in vals.iter().enumerate() {
                    if let Some(n) = num_field(ui, id.with(("hsb", k)), *v, 34.0) {
                        let mut h = hsb;
                        match k {
                            0 => h.h = n.rem_euclid(360.0) as f32,
                            1 => h.s = (n.clamp(0.0, 100.0) / 100.0) as f32,
                            _ => h.b = (n.clamp(0.0, 100.0) / 100.0) as f32,
                        }
                        hsb = h;
                        res = Some(h.to_rgb(current.a));
                    }
                }
                res
            }
            _ => {
                let hex = format!("{:02X}{:02X}{:02X}", current.r, current.g, current.b);
                text_field(ui, id.with("hex"), &hex, 110.0)
                    .and_then(|s| crate::props::parse_hex(&s))
                    .and_then(|h| Rgba::from_hex(&h[1..]))
                    .map(|c| Rgba { a: current.a, ..c })
            }
        };
        if alpha && let Some(n) = num_field(ui, id.with("alpha"), (f64::from(current.a) / 2.55).round(), 40.0) {
            out = Some(Picked { color: Rgba { a: (n.clamp(0.0, 100.0) * 2.55).round() as u8, ..typed.unwrap_or(current) }, done: true });
        } else if let Some(c) = typed {
            out = Some(Picked { color: c, done: true });
        }
    });
    if let Some(p) = &out {
        if !matches!(model, 2) {
            // Typed RGB or hex: the square follows (keeping the hue when the colour is grey).
            let n = Hsb::from_rgb(p.color);
            if n.s > 0.001 && n.b > 0.001 && p.color != hsb.to_rgb(p.color.a) {
                hsb = n;
            }
        }
        ui.data_mut(|d| d.insert_temp(state_id, (hsb, p.color, !p.done)));
    } else {
        ui.data_mut(|d| d.insert_temp(state_id, (hsb, color, false)));
    }
    out
}

fn num_field(ui: &mut Ui, id: Id, v: f64, w: f32) -> Option<f64> {
    let s = crate::widgets::fmt_num(v, "");
    text_field(ui, id, &s, w).and_then(|t| crate::widgets::parse_num(&t))
}

/// A small grey text field; returns its text when it's committed (Enter or leaving it).
fn text_field(ui: &mut Ui, id: Id, value: &str, w: f32) -> Option<String> {
    let t = Tokens::get(ui.ctx());
    let key = id.with("typing");
    let mut text: String = ui.data(|d| d.get_temp(key)).unwrap_or_else(|| value.to_string());
    let r = egui::Frame::NONE.fill(t.field).corner_radius(CornerRadius::same(5)).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
        ui.add(egui::TextEdit::singleline(&mut text).frame(egui::Frame::NONE).font(theme::font(12.0)).desired_width(w - 12.0).id(id))
            .accessible_name("Hex")
    });
    let te = r.inner;
    if te.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, text.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(key));
    }
    let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
    (te.lost_focus() && !escaped && text != value).then_some(text)
}

fn c32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

fn quad(ui: &Ui, rect: Rect, colors: [Color32; 4]) {
    let mut m = Mesh::default();
    for (p, c) in [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()].into_iter().zip(colors) {
        m.colored_vertex(p, c);
    }
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    ui.painter().add(m);
}

fn sv_square(ui: &mut Ui, rect: Rect, hsb: &mut Hsb, id: Id) -> (bool, bool) {
    // White → hue across, then transparent → black down.
    let hue = c32(Hsb { h: hsb.h, s: 1.0, b: 1.0 }.to_rgb(255));
    quad(ui, rect, [Color32::WHITE, hue, hue, Color32::WHITE]);
    quad(ui, rect, [Color32::TRANSPARENT, Color32::TRANSPARENT, Color32::BLACK, Color32::BLACK]);
    let resp = ui.interact(rect, id.with("sv"), Sense::click_and_drag());
    let mut changed = false;
    if (resp.dragged() || resp.clicked() || resp.drag_started())
        && let Some(p) = resp.interact_pointer_pos()
    {
        hsb.s = ((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
        hsb.b = 1.0 - ((p.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
        changed = true;
    }
    let knob = pos2(rect.min.x + hsb.s * rect.width(), rect.min.y + (1.0 - hsb.b) * rect.height());
    ui.painter().circle_stroke(knob, 6.0, Stroke::new(2.0, Color32::WHITE));
    ui.painter().circle_stroke(knob, 7.0, Stroke::new(1.0, Color32::from_black_alpha(80)));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, "Saturation and brightness"));
    (changed, resp.drag_stopped() || resp.clicked())
}

fn hue_slider(ui: &mut Ui, rect: Rect, hsb: &mut Hsb, id: Id) -> (bool, bool) {
    let n = 12;
    let mut m = Mesh::default();
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let x = rect.min.x + t * rect.width();
        let c = c32(Hsb { h: t * 360.0, s: 1.0, b: 1.0 }.to_rgb(255));
        m.colored_vertex(pos2(x, rect.min.y), c);
        m.colored_vertex(pos2(x, rect.max.y), c);
        if i > 0 {
            let k = (i * 2) as u32;
            m.add_triangle(k - 2, k - 1, k);
            m.add_triangle(k - 1, k, k + 1);
        }
    }
    ui.painter().add(m);
    slider_knob(ui, rect, hsb.h / 360.0, id.with("hue"), "Hue")
        .map(|(t, done)| {
            hsb.h = (t * 360.0).clamp(0.0, 359.9);
            (true, done)
        })
        .unwrap_or((false, false))
}

fn slider_knob(ui: &mut Ui, rect: Rect, t: f32, id: Id, label: &str) -> Option<(f32, bool)> {
    let resp = ui.interact(rect.expand(4.0), id, Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, label));
    let x = rect.min.x + t.clamp(0.0, 1.0) * rect.width();
    ui.painter().circle_stroke(pos2(x, rect.center().y), rect.height() / 2.0 + 1.0, Stroke::new(2.0, Color32::WHITE));
    ui.painter().circle_stroke(pos2(x, rect.center().y), rect.height() / 2.0 + 2.0, Stroke::new(1.0, Color32::from_black_alpha(80)));
    if (resp.dragged() || resp.clicked() || resp.drag_started())
        && let Some(p) = resp.interact_pointer_pos()
    {
        return Some((((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0), resp.drag_stopped() || resp.clicked()));
    }
    if resp.drag_stopped() {
        return Some((t, true));
    }
    None
}

fn alpha_slider(ui: &mut Ui, rect: Rect, opaque: Rgba, a: u8, id: Id) -> Option<(f32, bool)> {
    crate::widgets::checker(ui.painter(), rect, rect.height() / 2.0);
    quad(ui, rect, [Color32::TRANSPARENT, c32(opaque), c32(opaque), Color32::TRANSPARENT]);
    slider_knob(ui, rect, f32::from(a) / 255.0, id.with("alpha"), "Opacity")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsb_round_trips_rgb() {
        for c in [
            Rgba::rgb(255, 0, 0),
            Rgba::rgb(0, 128, 255),
            Rgba::rgb(51, 102, 204),
            Rgba::rgb(0, 0, 0),
            Rgba::rgb(255, 255, 255),
            Rgba::rgb(128, 128, 128),
        ] {
            let back = Hsb::from_rgb(c).to_rgb(255);
            assert!(
                (i32::from(back.r) - i32::from(c.r)).abs() <= 1
                    && (i32::from(back.g) - i32::from(c.g)).abs() <= 1
                    && (i32::from(back.b) - i32::from(c.b)).abs() <= 1,
                "{c:?} → {back:?}"
            );
        }
        let h = Hsb::from_rgb(Rgba::rgb(0, 255, 0));
        assert!((h.h - 120.0).abs() < 0.5 && (h.s - 1.0).abs() < 1e-3 && (h.b - 1.0).abs() < 1e-3);
    }
}
