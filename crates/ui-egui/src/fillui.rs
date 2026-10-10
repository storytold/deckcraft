//! The gradient editor of the Format pane (shape fill and slide background): preset variations,
//! type, direction, angle and a stop bar (add, drag, recolour, set transparency, remove stops).

use deckcraft_color::{ColorScheme, ColorTransform, SchemeSlot};
use deckcraft_model::ColorRef;
use deckcraft_model::style::{Gradient, GradientShape, GradientStop};
use egui::{Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::ribbon::cref_param;
use crate::theme::{self, Tokens};
use crate::widgets;

/// A default two-stop gradient on `base`: the colour and a darker shade.
pub fn default_gradient(base: ColorRef, angle: f64) -> Gradient {
    let mut dark = base.clone();
    dark.mods.push(ColorTransform::LumMod(60_000));
    Gradient {
        stops: vec![GradientStop { pos: 0.0, color: base }, GradientStop { pos: 1.0, color: dark }],
        shape: GradientShape::Linear { angle, scaled: false },
        rotate_with_shape: true,
    }
}

/// The command parameter for `g` (`shape.fill {gradient}` / `design.background {gradient}`).
pub fn gradient_json(g: &Gradient) -> Value {
    let stops: Vec<Value> = g.stops.iter().map(|s| json!([s.pos, cref_param(&s.color)])).collect();
    match &g.shape {
        GradientShape::Linear { angle, .. } => json!({"stops": stops, "kind": "linear", "angle": angle, "rotateWithShape": g.rotate_with_shape}),
        GradientShape::Path { path, focus } => {
            let kind = match path.as_str() {
                "rect" => "rectangular",
                "shape" => "path",
                _ => "radial",
            };
            json!({"stops": stops, "kind": kind, "focus": focus, "rotateWithShape": g.rotate_with_shape})
        }
    }
}

fn alpha_of(c: &ColorRef) -> f64 {
    c.mods.iter().find_map(|m| if let ColorTransform::Alpha(a) = m { Some(*a as f64 / 100_000.0) } else { None }).unwrap_or(1.0)
}

fn with_alpha(mut c: ColorRef, a: f64) -> ColorRef {
    c.mods.retain(|m| !matches!(m, ColorTransform::Alpha(_)));
    if a < 0.999 {
        c.mods.push(ColorTransform::Alpha((a.clamp(0.0, 1.0) * 100_000.0).round() as i32));
    }
    c
}

fn c32(c: &ColorRef, sc: &ColorScheme) -> Color32 {
    theme::to_color32(c.resolve(sc, None))
}

/// Paint `g` left→right into `r` (the stop bar and preset swatches).
fn paint_bar(ui: &Ui, r: Rect, g: &Gradient, sc: &ColorScheme) {
    // Checkerboard under transparent stops.
    let p = ui.painter();
    let n = (r.width() / 6.0).ceil() as i32;
    for i in 0..n {
        for j in 0..((r.height() / 6.0).ceil() as i32) {
            if (i + j) % 2 == 0 {
                let cell = Rect::from_min_size(pos2(r.min.x + i as f32 * 6.0, r.min.y + j as f32 * 6.0), vec2(6.0, 6.0)).intersect(r);
                p.rect_filled(cell, CornerRadius::ZERO, Color32::from_gray(220));
            }
        }
    }
    let mut mesh = egui::Mesh::default();
    let stops: Vec<(f32, Color32)> = g.stops.iter().map(|s| (s.pos as f32, c32(&s.color, sc))).collect();
    let mut pts: Vec<(f32, Color32)> = vec![];
    if let Some(first) = stops.first() {
        pts.push((0.0, first.1));
    }
    pts.extend(stops.iter().copied());
    if let Some(last) = stops.last() {
        pts.push((1.0, last.1));
    }
    for (k, (t, c)) in pts.iter().enumerate() {
        let x = r.min.x + t.clamp(0.0, 1.0) * r.width();
        mesh.colored_vertex(pos2(x, r.min.y), *c);
        mesh.colored_vertex(pos2(x, r.max.y), *c);
        if k > 0 {
            let i = (k as u32) * 2;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i, i + 1);
        }
    }
    p.add(egui::Shape::mesh(mesh));
    p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, Tokens::get(ui.ctx()).border), egui::StrokeKind::Inside);
}

const DIRECTIONS_LINEAR: [(&str, f64); 8] =
    [("→", 0.0), ("↘", 45.0), ("↓", 90.0), ("↙", 135.0), ("←", 180.0), ("↖", 225.0), ("↑", 270.0), ("↗", 315.0)];
const DIRECTIONS_PATH: [(&str, [f64; 4]); 5] = [
    ("From center", [0.5, 0.5, 0.5, 0.5]),
    ("From top left", [0.0, 0.0, 1.0, 1.0]),
    ("From top right", [1.0, 0.0, 0.0, 1.0]),
    ("From bottom left", [0.0, 1.0, 1.0, 0.0]),
    ("From bottom right", [1.0, 1.0, 0.0, 0.0]),
];

/// The editor for `g`; returns the new gradient when the user changed something.
pub fn gradient_editor(ui: &mut Ui, id: &str, g: &Gradient, sc: &ColorScheme) -> Option<Gradient> {
    let t = Tokens::get(ui.ctx());
    let mut out: Option<Gradient> = None;
    let sel_id = ui.id().with((id, "stop"));
    let mut sel: usize = ui.data_mut(|d| d.get_temp(sel_id).unwrap_or(0)).min(g.stops.len().saturating_sub(1));

    // Preset variations of the first stop's colour.
    ui.label("Preset gradients");
    ui.horizontal_wrapped(|ui| {
        let base = g.stops.first().map(|s| s.color.clone()).unwrap_or(ColorRef::scheme(SchemeSlot::Accent1));
        let mut light = base.clone();
        light.mods.push(ColorTransform::LumMod(40_000));
        light.mods.push(ColorTransform::LumOff(60_000));
        let mut dark = base.clone();
        dark.mods.push(ColorTransform::LumMod(50_000));
        let variants: Vec<Gradient> = vec![
            default_gradient(base.clone(), 90.0),
            Gradient {
                stops: vec![GradientStop { pos: 0.0, color: light.clone() }, GradientStop { pos: 1.0, color: base.clone() }],
                ..default_gradient(base.clone(), 90.0)
            },
            Gradient {
                stops: vec![GradientStop { pos: 0.0, color: base.clone() }, GradientStop { pos: 1.0, color: dark.clone() }],
                ..default_gradient(base.clone(), 0.0)
            },
            Gradient {
                stops: vec![
                    GradientStop { pos: 0.0, color: light.clone() },
                    GradientStop { pos: 0.5, color: base.clone() },
                    GradientStop { pos: 1.0, color: dark.clone() },
                ],
                ..default_gradient(base.clone(), 45.0)
            },
            Gradient {
                stops: vec![GradientStop { pos: 0.0, color: light }, GradientStop { pos: 1.0, color: dark }],
                shape: GradientShape::Path { path: "circle".into(), focus: [0.5, 0.5, 0.5, 0.5] },
                rotate_with_shape: true,
            },
            Gradient {
                stops: vec![GradientStop { pos: 0.0, color: base.clone() }, GradientStop { pos: 1.0, color: with_alpha(base, 0.0) }],
                ..default_gradient(ColorRef::scheme(SchemeSlot::Accent1), 90.0)
            },
        ];
        for v in variants {
            let (r, resp) = ui.allocate_exact_size(vec2(34.0, 22.0), Sense::click());
            paint_bar(ui, r.shrink(2.0), &v, sc);
            if resp.hovered() {
                ui.painter().rect_stroke(r, CornerRadius::same(2), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
            }
            if resp.clicked() {
                out = Some(v);
            }
        }
    });

    // Type and direction.
    let kind = match &g.shape {
        GradientShape::Linear { .. } => "Linear",
        GradientShape::Path { path, .. } if path == "rect" => "Rectangular",
        GradientShape::Path { path, .. } if path == "shape" => "Path",
        GradientShape::Path { .. } => "Radial",
    };
    ui.horizontal(|ui| {
        ui.add_sized(vec2(70.0, 20.0), egui::Label::new("Type"));
        egui::ComboBox::from_id_salt((id, "kind")).selected_text(kind).width(120.0).show_ui(ui, |ui| {
            for k in ["Linear", "Radial", "Rectangular", "Path"] {
                if ui.selectable_label(kind == k, k).clicked() && kind != k {
                    let focus = match &g.shape {
                        GradientShape::Path { focus, .. } => *focus,
                        _ => [0.5, 0.5, 0.5, 0.5],
                    };
                    let shape = match k {
                        "Linear" => GradientShape::Linear { angle: 90.0, scaled: false },
                        "Rectangular" => GradientShape::Path { path: "rect".into(), focus },
                        "Path" => GradientShape::Path { path: "shape".into(), focus },
                        _ => GradientShape::Path { path: "circle".into(), focus },
                    };
                    out = Some(Gradient { shape, ..g.clone() });
                }
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        ui.add_sized(vec2(70.0, 20.0), egui::Label::new("Direction"));
        match &g.shape {
            GradientShape::Linear { angle, .. } => {
                for (l, a) in DIRECTIONS_LINEAR {
                    let on = (angle - a).abs() < 0.5;
                    if ui.selectable_label(on, l).on_hover_text(format!("{a}°")).clicked() {
                        out = Some(Gradient { shape: GradientShape::Linear { angle: a, scaled: false }, ..g.clone() });
                    }
                }
            }
            GradientShape::Path { path, focus } => {
                for (l, f) in DIRECTIONS_PATH {
                    let on = focus.iter().zip(f).all(|(a, b)| (a - b).abs() < 1e-3);
                    if ui.selectable_label(on, l.trim_start_matches("From ")).on_hover_text(l).clicked() {
                        out = Some(Gradient { shape: GradientShape::Path { path: path.clone(), focus: f }, ..g.clone() });
                    }
                }
            }
        }
    });
    if let GradientShape::Linear { angle, .. } = &g.shape {
        ui.horizontal(|ui| {
            ui.add_sized(vec2(70.0, 20.0), egui::Label::new("Angle"));
            if let Some(a) = widgets::spinner(ui, &format!("{id}-angle"), *angle, 15.0, "°", 70.0) {
                out = Some(Gradient { shape: GradientShape::Linear { angle: a.rem_euclid(360.0), scaled: false }, ..g.clone() });
            }
        });
    }

    // Stop bar: click to add, drag a stop to move it.
    ui.add_space(4.0);
    ui.label("Gradient stops");
    let (bar, bar_resp) = ui.allocate_exact_size(vec2(ui.available_width().min(260.0), 18.0), Sense::click());
    paint_bar(ui, bar, g, sc);
    let (marks, _) = ui.allocate_exact_size(vec2(bar.width(), 14.0), Sense::hover());
    let drag_id = ui.id().with((id, "drag"));
    let mut dragging: Option<(usize, f64)> = ui.data_mut(|d| d.get_temp(drag_id));
    for (i, s) in g.stops.iter().enumerate() {
        let pos = dragging.filter(|d| d.0 == i).map_or(s.pos, |d| d.1);
        let x = bar.min.x + pos as f32 * bar.width();
        let r = Rect::from_center_size(pos2(x, marks.center().y), vec2(10.0, 12.0));
        let resp = ui.interact(r, ui.id().with((id, "mark", i)), Sense::click_and_drag());
        let fill = c32(&s.color, sc);
        let stroke = if i == sel { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.handle_stroke) };
        let tri = vec![pos2(x, r.min.y), pos2(r.max.x, r.min.y + 4.0), pos2(r.max.x, r.max.y), pos2(r.min.x, r.max.y), pos2(r.min.x, r.min.y + 4.0)];
        ui.painter().add(egui::Shape::convex_polygon(tri, fill, stroke));
        if resp.clicked() || resp.drag_started() {
            sel = i;
        }
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
        {
            dragging = Some((i, ((p.x - bar.min.x) / bar.width()).clamp(0.0, 1.0) as f64));
        }
        if resp.drag_stopped()
            && let Some((di, dp)) = dragging.take()
        {
            let mut ng = g.clone();
            if let Some(st) = ng.stops.get_mut(di) {
                st.pos = (dp * 1000.0).round() / 1000.0;
            }
            out = Some(ng);
        }
    }
    ui.data_mut(|d| match dragging {
        Some(v) => {
            d.insert_temp(drag_id, v);
        }
        None => d.remove::<(usize, f64)>(drag_id),
    });
    if bar_resp.clicked()
        && let Some(p) = bar_resp.interact_pointer_pos()
        && g.stops.len() < 10
    {
        // A new stop takes the colour of the nearest stop.
        let pos = (((p.x - bar.min.x) / bar.width()).clamp(0.0, 1.0) as f64 * 1000.0).round() / 1000.0;
        let near = g.stops.iter().min_by(|a, b| (a.pos - pos).abs().total_cmp(&(b.pos - pos).abs())).map(|s| s.color.clone());
        let mut ng = g.clone();
        ng.stops.push(GradientStop { pos, color: near.unwrap_or(ColorRef::scheme(SchemeSlot::Accent1)) });
        ng.stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        sel = ng.stops.iter().position(|s| s.pos == pos).unwrap_or(0);
        out = Some(ng);
    }

    // The selected stop.
    if let Some(s) = g.stops.get(sel) {
        ui.horizontal(|ui| {
            ui.add_sized(vec2(70.0, 20.0), egui::Label::new("Color"));
            let (sw, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            ui.painter().rect(sw, CornerRadius::same(3), c32(&s.color, sc), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
            let r = widgets::drop_button(ui, "", vec2(24.0, 20.0), true);
            widgets::color_popup(&r).show(|ui| {
                if let Some(Some(c)) = widgets::color_grid(ui, sc, None) {
                    let mut ng = g.clone();
                    ng.stops[sel].color = with_alpha(c, alpha_of(&s.color));
                    out = Some(ng);
                }
            });
            let can_remove = g.stops.len() > 2;
            if ui.add_enabled(can_remove, egui::Button::new("Remove stop")).clicked() {
                let mut ng = g.clone();
                ng.stops.remove(sel);
                sel = sel.saturating_sub(1);
                out = Some(ng);
            }
        });
        ui.horizontal(|ui| {
            ui.add_sized(vec2(70.0, 20.0), egui::Label::new("Position"));
            if let Some(v) = widgets::spinner(ui, &format!("{id}-pos-{sel}"), (s.pos * 100.0).round(), 5.0, "%", 70.0) {
                let mut ng = g.clone();
                ng.stops[sel].pos = (v / 100.0).clamp(0.0, 1.0);
                ng.stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
                out = Some(ng);
            }
        });
        ui.horizontal(|ui| {
            ui.add_sized(vec2(70.0, 20.0), egui::Label::new("Transparency"));
            let key = ui.id().with((id, "tr", sel));
            let mut tr: f32 = ui.data_mut(|d| d.get_temp(key).unwrap_or(((1.0 - alpha_of(&s.color)) * 100.0) as f32));
            let r = ui.add(egui::Slider::new(&mut tr, 0.0..=100.0).suffix("%"));
            if r.changed() {
                ui.data_mut(|d| d.insert_temp(key, tr));
            }
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                let mut ng = g.clone();
                ng.stops[sel].color = with_alpha(s.color.clone(), 1.0 - tr as f64 / 100.0);
                ui.data_mut(|d| d.remove::<f32>(key));
                out = Some(ng);
            }
        });
    }
    ui.data_mut(|d| d.insert_temp(sel_id, sel));
    out
}
