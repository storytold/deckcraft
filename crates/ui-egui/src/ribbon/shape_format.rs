use deckcraft_model::{Generated, ShapeId};

use super::arrange::{arrange_group, size_group};
use super::galleries::{cref_param, effects_menu, outline_menu, quick_styles, shapes_gallery};
use super::insert::{CHECK_STATES, paint_checkbox, paint_harvey_ball, picture_row};
use super::{
    Color32, CornerRadius, Icon, Rect, Sense, SlideApp, Stroke, Tokens, Ui, big, big_button, enabled, group, json, pos2, rows, run, scheme,
    small_button, small_button_ex, small_button_named, theme, vec2, widgets,
};

// ---------- contextual tabs ----------

/// Shape Format ▸ Text Effects ▸ Transform: No Transform, Follow Path, then the other presets.
fn transform_gallery(app: &mut SlideApp, ui: &mut Ui) {
    let current = selected_warp(app);
    ui.label(egui::RichText::new("Transform").font(theme::bold(12.0)));
    ui.label("Follow Path");
    let mut picked = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
        for p in deckcraft_geom::warp::CATALOG.iter().filter(|p| deckcraft_geom::warp::is_follow(p.id)) {
            if warp_tile(ui, p.id, p.label, current.as_deref() == Some(p.id)) {
                run(app, "format.textWarp", json!({"preset": p.id}));
                picked = true;
            }
        }
    });
    ui.label("Warp");
    egui::ScrollArea::vertical().max_height(220.0).id_salt("text_warp_gallery").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            if warp_tile(ui, "", "No Transform", current.is_none()) {
                run(app, "format.textWarp", json!({"preset": "none"}));
                picked = true;
            }
            for p in deckcraft_geom::warp::CATALOG.iter().filter(|p| !deckcraft_geom::warp::is_follow(p.id)) {
                if warp_tile(ui, p.id, p.label, current.as_deref() == Some(p.id)) {
                    run(app, "format.textWarp", json!({"preset": p.id}));
                    picked = true;
                }
            }
        });
    });
    if picked {
        ui.close();
    }
}

fn selected_warp(app: &SlideApp) -> Option<String> {
    let doc = app.session.active()?;
    let id = doc.selection.text.as_ref().map(|t| t.shape).or_else(|| doc.selection.shapes.first().copied())?;
    doc.shape(id)?.text.as_ref()?.body.warp.clone()
}

fn warp_tile(ui: &mut Ui, preset: &str, label: &str, selected: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 28.0), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, CornerRadius::same(3), t.selected);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(3), t.hover);
    }
    paint_warp_icon(ui.painter(), rect.shrink(4.0), (!preset.is_empty()).then_some(preset), t.text);
    resp.on_hover_text(label).clicked()
}

/// The preset's guides, or two straight lines for No Transform.
fn paint_warp_icon(p: &egui::Painter, r: Rect, name: Option<&str>, color: Color32) {
    let Some(name) = name else {
        let y0 = r.min.y + r.height() * 0.32;
        let y1 = r.min.y + r.height() * 0.68;
        p.line_segment([pos2(r.min.x, y0), pos2(r.max.x, y0)], Stroke::new(1.0, color));
        p.line_segment([pos2(r.min.x, y1), pos2(r.max.x, y1)], Stroke::new(1.0, color));
        return;
    };
    let rect = deckcraft_geom::Rect::new((r.min.x + 1.0) as f64, (r.min.y + 1.0) as f64, (r.max.x - 1.0) as f64, (r.max.y - 1.0) as f64);
    let Some(w) = deckcraft_geom::warp::warp(name, &[], rect) else { return };
    for band in &w.bands {
        match band {
            deckcraft_geom::warp::Band::Fill { top, bottom } => {
                stroke_guide(p, top, color);
                stroke_guide(p, bottom, color);
            }
            deckcraft_geom::warp::Band::Follow { path, .. } => stroke_guide(p, path, color),
        }
    }
}

fn stroke_guide(p: &egui::Painter, g: &deckcraft_geom::warp::Guide, color: Color32) {
    let len = g.length();
    if !(len.is_finite() && len > 1.0) {
        return;
    }
    let n = 20i32;
    let mut pts = Vec::with_capacity((n + 1) as usize);
    for i in 0..=n {
        let (q, _) = g.at(len * f64::from(i) / f64::from(n));
        if q.x.is_finite() && q.y.is_finite() {
            pts.push(pos2(q.x as f32, q.y as f32));
        }
    }
    if pts.len() >= 2 {
        p.add(egui::Shape::Path(egui::epaint::PathShape {
            points: pts,
            closed: false,
            fill: Color32::TRANSPARENT,
            stroke: Stroke::new(1.0, color).into(),
        }));
    }
}

/// Render the Shape Format tab's shape, fill, outline, effects, text, arrange, and size controls.
pub(super) fn shape_format(app: &mut SlideApp, ui: &mut Ui) {
    let sc = scheme(app);
    if let Some((gid, g)) = app.session.active().and_then(deckcraft_engine::cmd::visuals::selected) {
        generated_group(app, ui, gid, &g, &sc);
    }
    let sel = enabled(app, "shape.fill");
    group(ui, |ui| {
        let r = big_button(ui, Icon::Shapes, "Shapes", true);
        egui::Popup::menu(&r).show(|ui| {
            if let Some(p) = shapes_gallery(ui) {
                app.session.set_tool(deckcraft_engine::ToolKind::Shape { preset: p.to_string() });
            }
        });
        big(app, ui, Icon::TextBox, "Text\nBox", "insert.textBox", json!({}));
        let r = big_button(ui, Icon::EditShape, "Edit\nShape", sel);
        egui::Popup::menu(&r).show(|ui| {
            ui.label("Change Shape");
            if let Some(p) = shapes_gallery(ui) {
                run(app, "shape.change", json!({"preset": p}));
            }
            ui.separator();
            if ui.add_enabled(enabled(app, "shape.editPoints"), egui::Button::new("Edit Points")).clicked() {
                run(app, "shape.editPoints", json!({"on": true}));
                ui.close();
            }
            ui.separator();
            smooth_corners_row(app, ui);
        });
        let can_merge = enabled(app, "shape.merge");
        let r = big_button(ui, Icon::MergeShapes, "Merge\nShapes", can_merge).on_hover_text("Merge Shapes");
        egui::Popup::menu(&r).show(|ui| {
            for (op, label) in deckcraft_engine::cmd::merge::OPS {
                if ui.button(label).clicked() {
                    run(app, "shape.merge", json!({"op": op}));
                    ui.close();
                }
            }
        });
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::QuickStyles, "Shape\nStyles", sel);
        egui::Popup::menu(&r).width(330.0).show(|ui| quick_styles(app, ui));
        rows(ui, |ui| {
            let (m, a) = small_button_ex(ui, Icon::ShapeFill, "Shape Fill", sel, false, true);
            if m.clicked() {
                let c = app.last_fill.clone();
                run(app, "shape.fill", json!({"color": c}));
            }
            if let Some(a) = a {
                widgets::color_popup(&a).show(|ui| {
                    if let Some(c) = widgets::color_grid(ui, &sc, Some("No Fill")) {
                        match c {
                            Some(c) => {
                                app.last_fill = cref_param(&c);
                                run(app, "shape.fill", json!({"color": cref_param(&c)}));
                            }
                            None => run(app, "shape.fill", json!({"none": true})),
                        }
                    }
                    if ui.button("Gradient…").clicked() {
                        let _ = app.run("view.pane", json!({"pane": "format", "tab": "fill"}));
                        ui.close();
                    }
                });
            }
            let (m, a) = small_button_ex(ui, Icon::ShapeOutline, "Shape Outline", sel, false, true);
            if m.clicked() {
                let c = app.last_outline.clone();
                run(app, "shape.line", json!({"color": c}));
            }
            if let Some(a) = a {
                widgets::color_popup(&a).show(|ui| outline_menu(app, ui, &sc));
            }
            let (main, arrow) = small_button_ex(ui, Icon::ShapeEffects, "Shape Effects", sel, false, true);
            if let Some(arrow) = arrow {
                widgets::split_menu(&main, &arrow, |ui| effects_menu(app, ui));
            }
        });
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            let (main, arrow) = small_button_ex(ui, Icon::TextFill, "Text Fill", sel, false, true);
            if let Some(arrow) = arrow {
                widgets::split_color_popup(&main, &arrow).show(|ui| {
                    if let Some(Some(c)) = widgets::color_grid(ui, &sc, None) {
                        run(app, "format.color", json!({"color": cref_param(&c)}));
                    }
                    if let Some(sh) = app.session.active().and_then(|d| d.selected_shapes().first().map(|s| (*s).clone())) {
                        ui.separator();
                        crate::panes::text_transparency_slider(app, ui, &sh);
                    }
                });
            }
            let (main, arrow) = small_button_ex(ui, Icon::TextOutline, "Text Outline", sel, false, true);
            if let Some(arrow) = arrow {
                widgets::split_color_popup(&main, &arrow).show(|ui| {
                    if let Some(c) = widgets::color_grid(ui, &sc, Some("No Outline")) {
                        run(app, "format.textOutline", json!({"color": c.map(|c| cref_param(&c))}));
                    }
                });
            }
            let r = small_button(ui, Icon::TextEffects, "Text Effects", sel, false);
            egui::Popup::menu(&r).width(312.0).show(|ui| {
                if ui.button("Shadow").clicked() {
                    run(app, "format.textShadow", json!({}));
                    ui.close();
                }
                ui.separator();
                transform_gallery(app, ui);
            });
        });
    });
    group(ui, |ui| {
        big(app, ui, Icon::AltText, "Alt\nText", "app.dialog", json!({"id": "altText"}));
    });
    arrange_group(app, ui);
    size_group(app, ui);
    group(ui, |ui| {
        big(app, ui, Icon::FormatBackground, "Format\nPane", "view.pane", json!({"pane": "format"}));
    });
}

/// The ways `harveyBall.set` divides a ball, as the `of` drop-down lists them.
const HARVEY_PARTS: [u16; 6] = [2, 3, 4, 5, 8, 10];

/// Shape Format ▸ Harvey Ball or Checkbox, shown while the selection is one (or one of its parts):
/// the ball's value, parts and colours, or the checkbox's state and colour. Colours and state go to
/// every selected element of the kind; the stepper and parts, whose values come from the first
/// one, to that one.
fn generated_group(app: &mut SlideApp, ui: &mut Ui, gid: ShapeId, g: &Generated, sc: &deckcraft_color::ColorScheme) {
    match g {
        Generated::HarveyBall(b) => group(ui, |ui| {
            rows(ui, |ui| {
                ui.horizontal(|ui| {
                    let (less, _) = small_button_named(ui, Icon::Minus, "", "Fill Less", b.value > 0, false, false);
                    if less.on_hover_text("One part less").clicked() {
                        run(app, "harveyBall.set", json!({"id": gid, "value": b.value.saturating_sub(1)}));
                    }
                    let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                    paint_harvey_ball(ui.painter(), rect, b.value, b.of, Tokens::get(ui.ctx()).text);
                    ui.label(format!("{}/{}", b.value, b.of));
                    let (more, _) = small_button_named(ui, Icon::Plus, "", "Fill More", b.value < b.of, false, false);
                    if more.on_hover_text("One part more").clicked() {
                        run(app, "harveyBall.set", json!({"id": gid, "value": b.value.saturating_add(1).min(b.of)}));
                    }
                });
                widgets::dropdown(ui, "Parts", &format!("Parts: {}", b.of), 96.0, true, |ui| {
                    for n in HARVEY_PARTS {
                        let label = format!("{}/{n}", b.value.min(n));
                        if picture_row(ui, &label, b.of == n, |p, r, ink| paint_harvey_ball(p, r, b.value.min(n), n, ink)) {
                            // The value is kept within the new number of parts.
                            run(app, "harveyBall.set", json!({"id": gid, "of": n, "value": b.value.min(n)}));
                            ui.close();
                        }
                    }
                });
            });
            rows(ui, |ui| {
                colour_menu(app, ui, sc, (Icon::ShapeFill, "Ball Fill"), None, |c| ("harveyBall.set", json!({"fill": c})));
                colour_menu(app, ui, sc, (Icon::FormatBackground, "Ball Background"), Some("No Fill"), |c| {
                    ("harveyBall.set", json!({"background": c}))
                });
                colour_menu(app, ui, sc, (Icon::ShapeOutline, "Ball Outline"), Some("No Outline"), |c| ("harveyBall.set", json!({"outline": c})));
            });
        }),
        Generated::Checkbox(c) => group(ui, |ui| {
            rows(ui, |ui| {
                let current = CHECK_STATES.iter().find(|(k, _)| *k == c.state.name()).map_or("", |(_, l)| *l);
                widgets::dropdown(ui, "Checkbox State", current, 110.0, true, |ui| {
                    for (state, label) in CHECK_STATES {
                        if picture_row(ui, label, c.state.name() == state, |p, r, ink| paint_checkbox(p, r, state, ink)) {
                            run(app, "checkbox.set", json!({"state": state}));
                            ui.close();
                        }
                    }
                });
                colour_menu(app, ui, sc, (Icon::ShapeOutline, "Checkbox Color"), None, |col| ("checkbox.set", json!({"color": col})));
            });
        }),
    }
}

/// A colour menu (`icon`, `label`) that runs the command and params `set` makes of the colour
/// picked; with `none`, that entry gives `null`.
fn colour_menu(
    app: &mut SlideApp,
    ui: &mut Ui,
    sc: &deckcraft_color::ColorScheme,
    (icon, label): (Icon, &str),
    none: Option<&str>,
    set: impl Fn(serde_json::Value) -> (&'static str, serde_json::Value),
) {
    let (main, arrow) = small_button_ex(ui, icon, label, true, false, true);
    if let Some(arrow) = arrow {
        widgets::split_color_popup(&main, &arrow).show(|ui| {
            if let Some(c) = widgets::color_grid(ui, sc, none) {
                let (cmd, p) = set(c.map_or(serde_json::Value::Null, |c| cref_param(&c)));
                run(app, cmd, p);
            }
        });
    }
}

/// Edit Shape ▸ Smooth Corners (#286): a slider that smooths a rectangle's corners into a
/// squircle. The radius is taken from the rectangle when the slider is first used and kept (in egui
/// memory, per shape) so later moves re-smooth the same corners.
fn smooth_corners_row(app: &mut SlideApp, ui: &mut egui::Ui) {
    let Some((id, radius)) = app.session.active().and_then(|d| {
        let id = *d.selection.shapes.first()?;
        let sh = d.shape(id)?;
        let x = sh.xfrm?;
        let ss = x.w.min(x.h).max(0.0);
        let own = match &sh.geom {
            deckcraft_model::Geom::Preset { name, adj } if name == "roundRect" => {
                Some(ss * adj.first().copied().unwrap_or(16667.0).clamp(0.0, 50000.0) / 100000.0)
            }
            deckcraft_model::Geom::Preset { name, .. } if name == "rect" => Some(ss * 16667.0 / 100000.0),
            _ => None,
        };
        Some((id, own))
    }) else {
        return;
    };
    let key = egui::Id::unique(("deckcraft-smooth-corners", id.0));
    let mut state: (f64, f64) = ui.data(|d| d.get_temp(key)).unwrap_or((radius.unwrap_or(0.0), 0.0));
    if let Some(r) = radius {
        state.0 = r;
    }
    let usable = radius.is_some() || ui.data(|d| d.get_temp::<(f64, f64)>(key)).is_some();
    ui.horizontal(|ui| {
        let label = ui.label("Smooth Corners");
        let mut pct = state.1;
        let resp = ui.add_enabled(usable, egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%").integer()).labelled_by(label.id);
        let resp = if usable { resp } else { resp.on_disabled_hover_text("For rectangles and rounded rectangles") };
        if resp.changed() {
            state.1 = pct;
            ui.data_mut(|d| d.insert_temp(key, state));
        }
        // One undo step per adjustment: apply when the drag ends (or on a click / keyboard step).
        if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
            run(app, "shape.smoothCorners", json!({"smoothing": state.1, "radius": state.0}));
        }
    });
}
