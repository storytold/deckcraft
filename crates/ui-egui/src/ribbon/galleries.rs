use super::{Align2, Color32, CornerRadius, Rect, Sense, SlideApp, Stroke, Tokens, Ui, Value, json, pos2, run, scheme, theme, vec2, widgets};

/// A ColorRef as a command colour parameter.
pub fn cref_param(c: &deckcraft_model::ColorRef) -> Value {
    match &c.base {
        deckcraft_model::ColorBase::Scheme { slot } => {
            let mut o = serde_json::Map::new();
            o.insert("scheme".into(), json!(slot.xml_name()));
            for m in &c.mods {
                if let Some(v) = m.value() {
                    o.insert(m.xml_name().into(), json!(v));
                }
            }
            Value::Object(o)
        }
        deckcraft_model::ColorBase::Rgb { rgb } if c.mods.is_empty() => json!(format!("#{}", rgb.hex())),
        deckcraft_model::ColorBase::Rgb { rgb } => {
            let mut o = serde_json::Map::new();
            o.insert("rgb".into(), json!(format!("#{}", rgb.hex())));
            for m in &c.mods {
                if let Some(v) = m.value() {
                    o.insert(m.xml_name().into(), json!(v));
                }
            }
            Value::Object(o)
        }
        deckcraft_model::ColorBase::Preset { name } => {
            let mut o = serde_json::Map::new();
            o.insert("preset".into(), json!(name));
            for m in &c.mods {
                if let Some(v) = m.value() {
                    o.insert(m.xml_name().into(), json!(v));
                }
            }
            Value::Object(o)
        }
        deckcraft_model::ColorBase::System { name, last } => {
            let mut o = serde_json::Map::new();
            o.insert("system".into(), json!(name));
            o.insert("last".into(), json!(format!("#{}", last.hex())));
            for m in &c.mods {
                if let Some(v) = m.value() {
                    o.insert(m.xml_name().into(), json!(v));
                }
            }
            Value::Object(o)
        }
    }
}

/// Convert to SmartArt: the layouts by category, three to a row; a click converts the selected
/// text.
/// A layout gallery grouped by category, each layout as a small cached picture with its name.
/// Clicking runs `cmd` with `{"kind": …}` plus `extra`; hovering shows the layout larger, drawn
/// with `nodes` (the selected graphic's outline) when given: a live preview without touching the
/// document.
pub(super) fn smartart_gallery(
    app: &mut SlideApp,
    ui: &mut Ui,
    cmd: &str,
    extra: Value,
    current: &str,
    (style, colors): (&str, &str),
    nodes: Option<&[deckcraft_model::DiagramNode]>,
) {
    use deckcraft_engine::cmd::smartart::{LAYOUTS, category};
    ui.set_min_width(440.0);
    let slide = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
    egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
        for (cat, title) in [
            ("list", "List"),
            ("process", "Process"),
            ("cycle", "Cycle"),
            ("hierarchy", "Hierarchy"),
            ("relationship", "Relationship"),
            ("matrix", "Matrix"),
            ("pyramid", "Pyramid"),
            ("picture", "Picture"),
        ] {
            let kinds: Vec<(&str, &str)> = LAYOUTS.iter().copied().filter(|(k, _)| category(k) == cat).collect();
            if kinds.is_empty() {
                continue;
            }
            ui.label(egui::RichText::new(title).strong());
            egui::Grid::new(("smartart_gallery", cat)).num_columns(4).spacing([6.0, 6.0]).show(ui, |ui| {
                for (i, (k, label)) in kinds.iter().enumerate() {
                    let ctx = ui.ctx().clone();
                    let thumb = app.session.active().and_then(|d| app.textures.smartart(&ctx, &d.doc, slide, (k, style, colors), None, 192));
                    let r = ui
                        .vertical_centered(|ui| {
                            ui.set_width(100.0);
                            let r = match &thumb {
                                Some(t) => {
                                    ui.add(egui::Button::image(egui::Image::new(t).fit_to_exact_size(egui::vec2(96.0, 60.0))).selected(*k == current))
                                }
                                None => ui.add_sized([96.0, 60.0], egui::Button::new("").selected(*k == current)),
                            };
                            ui.add(egui::Label::new(egui::RichText::new(*label).small()).truncate().selectable(false));
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, *label));
                            r.on_hover_text(*label)
                        })
                        .inner;
                    if r.hovered() {
                        let preview = app.session.active().and_then(|d| app.textures.smartart(&ctx, &d.doc, slide, (k, style, colors), nodes, 480));
                        if let Some(t) = preview {
                            r.show_tooltip_ui(|ui| {
                                ui.add(egui::Image::new(&t).fit_to_exact_size(egui::vec2(320.0, 200.0)));
                            });
                        }
                    }
                    if r.clicked() {
                        let mut args = extra.clone();
                        args["kind"] = json!(k);
                        run(app, cmd, args);
                        ui.close();
                    }
                    if i % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
            ui.separator();
        }
    });
}

/// Shapes gallery popup body: every preset by category. Returns the chosen preset.
pub fn shapes_gallery(ui: &mut Ui) -> Option<&'static str> {
    let t = Tokens::get(ui.ctx());
    let mut out = None;
    ui.set_max_width(330.0);
    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        for cat in deckcraft_geom::preset::Category::ALL {
            ui.label(egui::RichText::new(cat.label()).font(theme::bold(12.0)));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
                for p in deckcraft_geom::preset::CATALOG.iter().filter(|p| p.category == cat) {
                    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, CornerRadius::same(3), t.hover);
                    }
                    paint_preset(ui.painter(), r.shrink(4.0), p.name, Color32::TRANSPARENT, t.text);
                    if resp.on_hover_text(p.label).clicked() {
                        out = Some(p.name);
                    }
                }
                if cat == deckcraft_geom::preset::Category::Lines {
                    for (name, label) in deckcraft_engine::tools::FREEFORM_TOOLS {
                        let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, CornerRadius::same(3), t.hover);
                        }
                        paint_freeform_icon(ui.painter(), r.shrink(5.0), name, t.text);
                        if resp.on_hover_text(label).clicked() {
                            out = Some(name);
                        }
                    }
                }
            });
            ui.add_space(4.0);
        }
    });
    if out.is_some() {
        ui.close();
    }
    out
}

/// Draw a preset's outline (gallery icons).
pub fn paint_preset(p: &egui::Painter, r: Rect, name: &str, fill: Color32, stroke: Color32) {
    let (w, h) = (r.width() as f64, r.height() as f64);
    let Some(g) = deckcraft_geom::preset::build(name, w, h, &[]) else { return };
    for sp in &g.paths {
        let mut pts: Vec<egui::Pos2> = vec![];
        let mut polys: Vec<(Vec<egui::Pos2>, bool)> = vec![];
        kurbo_flatten(&sp.path, |el| match el {
            Fl::Move(x, y) => {
                if pts.len() > 1 {
                    polys.push((std::mem::take(&mut pts), false));
                }
                pts.clear();
                pts.push(pos2(r.min.x + x as f32, r.min.y + y as f32));
            }
            Fl::Line(x, y) => pts.push(pos2(r.min.x + x as f32, r.min.y + y as f32)),
            Fl::Close => {
                if pts.len() > 1 {
                    polys.push((std::mem::take(&mut pts), true));
                }
            }
        });
        if pts.len() > 1 {
            polys.push((pts, false));
        }
        for (poly, closed) in polys {
            if closed && fill != Color32::TRANSPARENT && sp.fill != deckcraft_geom::preset::FillMode::None {
                p.add(egui::Shape::Path(egui::epaint::PathShape { points: poly.clone(), closed: true, fill, stroke: Stroke::NONE.into() }));
            }
            if sp.stroke {
                p.add(egui::Shape::Path(egui::epaint::PathShape {
                    points: poly,
                    closed,
                    fill: Color32::TRANSPARENT,
                    stroke: Stroke::new(1.0, stroke).into(),
                }));
            }
        }
    }
}

pub(super) enum Fl {
    Move(f64, f64),
    Line(f64, f64),
    Close,
}

pub(super) fn kurbo_flatten(path: &deckcraft_geom::BezPath, mut f: impl FnMut(Fl)) {
    deckcraft_geom::preset_flatten(path, &mut |el| match el {
        deckcraft_geom::PathEl::MoveTo(p) => f(Fl::Move(p.x, p.y)),
        deckcraft_geom::PathEl::LineTo(p) => f(Fl::Line(p.x, p.y)),
        deckcraft_geom::PathEl::ClosePath => f(Fl::Close),
        _ => {}
    });
}

pub fn outline_menu(app: &mut SlideApp, ui: &mut Ui, sc: &deckcraft_color::ColorScheme) {
    if let Some(c) = widgets::color_grid(ui, sc, Some("No Outline")) {
        match c {
            Some(c) => {
                app.last_outline = cref_param(&c);
                run(app, "shape.line", json!({"color": cref_param(&c)}));
            }
            None => run(app, "shape.line", json!({"none": true})),
        }
    }
    ui.menu_button("Weight", |ui| {
        for w in [0.25, 0.5, 0.75, 1.0, 1.5, 2.25, 3.0, 4.5, 6.0] {
            if ui.button(format!("{w} pt")).clicked() {
                run(app, "shape.line", json!({"width": w}));
                ui.close();
            }
        }
    });
    ui.menu_button("Dashes", |ui| {
        for (l, d) in [
            ("Solid", "solid"),
            ("Round Dot", "sysDot"),
            ("Square Dot", "sysDash"),
            ("Dash", "dash"),
            ("Dash Dot", "dashDot"),
            ("Long Dash", "lgDash"),
            ("Long Dash Dot", "lgDashDot"),
        ] {
            if ui.button(l).clicked() {
                run(app, "shape.line", json!({"dash": d}));
                ui.close();
            }
        }
    });
    ui.menu_button("Arrows", |ui| {
        for (l, h, tl) in [
            ("None", "none", "none"),
            ("Arrow at end", "none", "triangle"),
            ("Arrow at start", "triangle", "none"),
            ("Arrows at both ends", "triangle", "triangle"),
            ("Stealth end", "none", "stealth"),
            ("Oval ends", "oval", "oval"),
        ] {
            if ui.button(l).clicked() {
                run(app, "shape.line", json!({"head": h, "tail": tl}));
                ui.close();
            }
        }
    });
}

pub fn effects_menu(app: &mut SlideApp, ui: &mut Ui) {
    ui.menu_button("Shadow", |ui| {
        for (l, v) in [
            ("No Shadow", "none"),
            ("Outer", "outer"),
            ("Bottom", "bottom"),
            ("Centered", "center"),
            ("Inner", "inner"),
            ("Perspective", "perspective"),
        ] {
            if ui.button(l).clicked() {
                run(app, "shape.effects", json!({"shadow": v}));
            }
        }
    });
    ui.menu_button("Reflection", |ui| {
        for (l, v) in [("No Reflection", "none"), ("Tight", "tight"), ("Half", "half"), ("Full", "full")] {
            if ui.button(l).clicked() {
                run(app, "shape.effects", json!({"reflection": v}));
            }
        }
    });
    ui.menu_button("Glow", |ui| {
        if ui.button("No Glow").clicked() {
            run(app, "shape.effects", json!({"glow": null}));
        }
        for r in [5.0, 8.0, 11.0, 18.0] {
            if ui.button(format!("{r} pt glow")).clicked() {
                run(app, "shape.effects", json!({"glow": r}));
            }
        }
    });
    ui.menu_button("Soft Edges", |ui| {
        for r in [0.0, 1.0, 2.5, 5.0, 10.0, 25.0] {
            if ui.button(if r == 0.0 { "No Soft Edges".to_string() } else { format!("{r} pt") }).clicked() {
                run(app, "shape.effects", json!({"softEdges": r}));
            }
        }
    });
    ui.menu_button("Bevel", |ui| {
        for b in [
            "none",
            "circle",
            "relaxedInset",
            "cross",
            "coolSlant",
            "angle",
            "softRound",
            "convex",
            "slope",
            "divot",
            "riblet",
            "hardEdge",
            "artDeco",
        ] {
            if ui.button(b).clicked() {
                run(app, "shape.effects", json!({"bevel": b}));
            }
        }
    });
    if ui.button("Format Shape…").clicked() {
        let _ = app.run("view.pane", json!({"pane": "format", "tab": "effects"}));
    }
}

pub(super) fn quick_styles(app: &mut SlideApp, ui: &mut Ui) {
    let sc = scheme(app);
    let slots = [
        deckcraft_color::SchemeSlot::Dk1,
        deckcraft_color::SchemeSlot::Accent1,
        deckcraft_color::SchemeSlot::Accent2,
        deckcraft_color::SchemeSlot::Accent3,
        deckcraft_color::SchemeSlot::Accent4,
        deckcraft_color::SchemeSlot::Accent5,
        deckcraft_color::SchemeSlot::Accent6,
    ];
    egui::Grid::new("quickstyles").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        for row in 0..6 {
            for (k, slot) in slots.iter().enumerate() {
                let c = theme::to_color32(sc.get(*slot));
                let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 26.0), Sense::click());
                let p = ui.painter();
                let (fill, stroke, text) = match row {
                    0 => (Color32::WHITE, c, Color32::BLACK),
                    2 => (c.gamma_multiply(0.35).to_opaque().lerp_to_gamma(Color32::WHITE, 0.6), Color32::TRANSPARENT, c),
                    5 => (c.lerp_to_gamma(Color32::BLACK, 0.25), Color32::WHITE, Color32::WHITE),
                    _ => (c, c.lerp_to_gamma(Color32::BLACK, 0.3), Color32::WHITE),
                };
                p.rect(rect.shrink(2.0), CornerRadius::same(3), fill, Stroke::new(1.5, stroke), egui::StrokeKind::Inside);
                p.text(rect.center(), Align2::CENTER_CENTER, "Abc", theme::font(11.0), text);
                if resp.hovered() {
                    p.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.5, Tokens::get(ui.ctx()).accent), egui::StrokeKind::Inside);
                }
                if resp.clicked() {
                    run(app, "shape.quickStyle", json!({"index": row * 7 + k}));
                    ui.close();
                }
            }
            ui.end_row();
        }
    });
}

/// Gallery glyphs for the freeform tools: a wave (curve), a polygon (freeform), a squiggle (scribble).
fn paint_freeform_icon(p: &egui::Painter, r: Rect, name: &str, ink: Color32) {
    let at = |x: f32, y: f32| egui::pos2(r.min.x + x * r.width(), r.min.y + y * r.height());
    let pts: Vec<egui::Pos2> = match name {
        "curve" => (0..=24).map(|i| i as f32 / 24.0).map(|u| at(u, 0.5 - 0.38 * (u * std::f32::consts::TAU).sin())).collect(),
        "freeform" => {
            [(0.0, 0.85), (0.15, 0.2), (0.55, 0.45), (0.8, 0.05), (1.0, 0.75), (0.45, 1.0), (0.0, 0.85)].iter().map(|(x, y)| at(*x, *y)).collect()
        }
        _ => (0..=40).map(|i| i as f32 / 40.0).map(|u| at(u, 0.5 + 0.35 * (u * 19.0).sin() * (1.0 - u * 0.4))).collect(),
    };
    p.add(egui::Shape::line(pts, egui::Stroke::new(1.3, ink)));
}
