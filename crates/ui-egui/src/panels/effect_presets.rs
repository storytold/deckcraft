//! Format Shape ▸ Effects: preset tiles that draw what each Shadow, Glow and Soft Edges preset does
//! to a small card, so the choice is made by looking rather than by reading names.

use deckcraft_model::style::Effects;
use egui::{Color32, CornerRadius, Painter, Rect, Role, Sense, Stroke, StrokeKind, Ui, WidgetInfo, pos2, vec2};

use crate::theme::Tokens;

const TILE: egui::Vec2 = vec2(46.0, 38.0);
const SHADOWS: [(&str, &str); 6] = [
    ("none", "No shadow"),
    ("outer", "Outer shadow"),
    ("bottom", "Bottom shadow"),
    ("center", "Centered shadow"),
    ("inner", "Inner shadow"),
    ("perspective", "Perspective shadow"),
];
/// The glow radii (pt) the ribbon's Glow menu offers.
pub(super) const GLOWS: [f64; 4] = [5.0, 8.0, 11.0, 18.0];
/// The soft-edge radii (pt) the ribbon's Soft Edges menu offers; 0 is "none".
pub(super) const SOFT_EDGES: [f64; 6] = [0.0, 1.0, 2.5, 5.0, 10.0, 25.0];

/// The `shape.effects` shadow preset id that `fx`'s current shadow came from.
pub(super) fn shadow_preset_id(fx: &Effects) -> &'static str {
    if fx.inner_shadow.is_some() {
        return "inner";
    }
    match &fx.outer_shadow {
        None => "none",
        Some(s) if s.align == "bl" => "perspective",
        Some(s) if s.dist == 0.0 => "center",
        Some(s) if s.dir == 90.0 => "bottom",
        Some(_) => "outer",
    }
}

/// A row of shadow preset tiles; returns the id of the one clicked.
pub(super) fn shadow_row(ui: &mut Ui, current: &str) -> Option<&'static str> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        for (id, label) in SHADOWS {
            if tile(ui, label, id == current, |p, r| paint_shadow(p, r, id)).clicked() {
                picked = Some(id);
            }
        }
    });
    picked
}

/// A row of glow tiles (the first clears it); returns the radius clicked, 0 for none.
pub(super) fn glow_row(ui: &mut Ui, current: f64) -> Option<f64> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        for r in std::iter::once(0.0).chain(GLOWS) {
            let label = if r == 0.0 { "No glow".to_string() } else { format!("{r} pt glow") };
            if tile(ui, &label, r == current, |p, rect| paint_glow(p, rect, r)).clicked() {
                picked = Some(r);
            }
        }
    });
    picked
}

/// A row of soft-edge tiles (the first clears it); returns the radius clicked, 0 for none.
pub(super) fn soft_edge_row(ui: &mut Ui, current: f64) -> Option<f64> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        for r in SOFT_EDGES {
            let label = if r == 0.0 { "No soft edges".to_string() } else { format!("{r} pt soft edges") };
            if tile(ui, &label, r == current, |p, rect| paint_soft(p, rect, r)).clicked() {
                picked = Some(r);
            }
        }
    });
    picked
}

fn tile(ui: &mut Ui, label: &str, selected: bool, paint: impl FnOnce(&Painter, Rect)) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(TILE, Sense::click());
    resp.widget_info(|| WidgetInfo::selected(Role::Button, true, selected, label));
    let p = ui.painter();
    let round = CornerRadius::same(5);
    p.rect_filled(rect, round, if resp.hovered() { t.hover } else { t.field_bg });
    paint(&p.with_clip_rect(rect.shrink(1.0)), rect);
    let stroke = if selected { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.border) };
    p.rect_stroke(rect, round, stroke, StrokeKind::Inside);
    resp.on_hover_text(label)
}

/// The sample card every preview draws on, centred in `tile`.
fn card(tile: Rect) -> Rect {
    Rect::from_center_size(tile.center(), vec2(22.0, 16.0))
}

fn card_fill(p: &Painter, r: Rect) {
    p.rect_filled(r, CornerRadius::same(2), Tokens::get(p.ctx()).accent);
}

/// `layers` rounded rects growing from `r` by up to `spread`, each faint, so the edge falls off.
fn halo(p: &Painter, r: Rect, spread: f32, color: Color32, layers: u32) {
    for i in (1..=layers).rev() {
        let k = i as f32 / layers as f32;
        let a = (f32::from(color.a()) * (1.0 - k * 0.8) / layers as f32 * 2.0).min(255.0) as u8;
        p.rect_filled(
            r.expand(spread * k),
            CornerRadius::same((2.0 + spread * k) as u8),
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a),
        );
    }
}

fn paint_shadow(p: &Painter, tile: Rect, id: &str) {
    let r = card(tile);
    let ink = Color32::from_rgba_unmultiplied(0, 0, 0, 120);
    match id {
        "outer" => halo(p, r.translate(vec2(3.0, 3.0)), 3.0, ink, 4),
        "bottom" => halo(p, r.translate(vec2(0.0, 4.0)), 3.0, ink, 4),
        "center" => halo(p, r, 5.0, ink, 5),
        "perspective" => {
            let base = r.max.y + 1.0;
            let slant = [pos2(r.min.x - 6.0, base), pos2(r.max.x - 8.0, base), pos2(r.max.x - 2.0, base + 5.0), pos2(r.min.x - 12.0, base + 5.0)];
            p.add(egui::Shape::convex_polygon(slant.to_vec(), Color32::from_rgba_unmultiplied(0, 0, 0, 90), Stroke::NONE));
        }
        _ => {}
    }
    card_fill(p, r);
    if id == "inner" {
        for i in 0..4 {
            let a = 110 - i * 25;
            p.rect_stroke(
                r.shrink(i as f32),
                CornerRadius::same(2),
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, a as u8)),
                StrokeKind::Inside,
            );
        }
    }
    if id == "none" {
        p.line_segment([tile.left_bottom() + vec2(6.0, -6.0), tile.right_top() + vec2(-6.0, 6.0)], Stroke::new(1.5, Tokens::get(p.ctx()).text_faint));
    }
}

fn paint_glow(p: &Painter, tile: Rect, radius: f64) {
    let r = card(tile);
    if radius > 0.0 {
        // 18 pt fills the tile's margin; smaller radii scale down.
        halo(p, r, (radius as f32 / 18.0 * 9.0).max(2.0), Tokens::get(p.ctx()).accent.gamma_multiply(0.9), 6);
    }
    card_fill(p, r);
    if radius == 0.0 {
        p.line_segment([tile.left_bottom() + vec2(6.0, -6.0), tile.right_top() + vec2(-6.0, 6.0)], Stroke::new(1.5, Tokens::get(p.ctx()).text_faint));
    }
}

fn paint_soft(p: &Painter, tile: Rect, radius: f64) {
    let t = Tokens::get(p.ctx());
    let r = card(tile).expand(2.0);
    card_fill(p, r);
    // Rings of the tile's own background, opaque at the edge and clear inward, feather the rim.
    let depth = (radius as f32 / 25.0 * 7.0).round().max(if radius > 0.0 { 1.0 } else { 0.0 }) as u32;
    let bg = t.field_bg;
    for i in 0..depth {
        let a = (255.0 * (1.0 - i as f32 / depth as f32)) as u8;
        let c = Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), a);
        p.rect_stroke(r.shrink(i as f32), CornerRadius::same(2), Stroke::new(1.0, c), StrokeKind::Inside);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::style::Shadow;
    use deckcraft_model::{ColorRef, SchemeSlot};

    fn shadow(dist: f64, dir: f64, align: &str, inner: bool) -> Shadow {
        Shadow {
            color: ColorRef::scheme(SchemeSlot::Dk1),
            blur: 4.0,
            dist,
            dir,
            inner,
            sx: 1.0,
            sy: 1.0,
            kx: 0.0,
            ky: 0.0,
            align: align.into(),
            rotate_with_shape: false,
        }
    }

    #[test]
    fn shadow_preset_id_names_each_preset() {
        let with = |o: Option<Shadow>, i: Option<Shadow>| Effects { outer_shadow: o, inner_shadow: i, ..Effects::default() };
        assert_eq!(shadow_preset_id(&with(None, None)), "none");
        assert_eq!(shadow_preset_id(&with(Some(shadow(3.0, 45.0, "tl", false)), None)), "outer");
        assert_eq!(shadow_preset_id(&with(Some(shadow(3.0, 90.0, "tl", false)), None)), "bottom");
        assert_eq!(shadow_preset_id(&with(Some(shadow(0.0, 45.0, "tl", false)), None)), "center");
        assert_eq!(shadow_preset_id(&with(Some(shadow(0.0, 90.0, "bl", false)), None)), "perspective");
        assert_eq!(shadow_preset_id(&with(None, Some(shadow(3.0, 45.0, "tl", true)))), "inner");
    }
}
