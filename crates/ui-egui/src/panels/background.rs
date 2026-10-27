use super::*;

pub(super) fn background(app: &mut SlideApp, ui: &mut Ui) {
    let sc = scheme(app);
    let bg = app.session.active().and_then(|d| d.current_slide().and_then(|s| s.background.clone()));
    let fill = match &bg {
        Some(deckcraft_model::Background::Fill { fill }) => Some(fill.clone()),
        _ => None,
    };
    let kind = match &fill {
        Some(Fill::Solid { .. }) => "solid",
        Some(Fill::Gradient(_)) => "gradient",
        Some(Fill::Picture(_)) => "picture",
        Some(Fill::Pattern(_)) => "pattern",
        _ => "auto",
    };
    // What "Apply to All" re-applies: the current slide's background as command params.
    let mut current: Option<Value> = None;
    ui.label(egui::RichText::new("Fill").font(theme::bold(13.0)));
    for (l, k) in [
        ("Solid fill", "solid"),
        ("Gradient fill", "gradient"),
        ("Picture or texture fill", "picture"),
        ("Pattern fill", "pattern"),
        ("Follow the layout", "auto"),
    ] {
        if ui.radio(kind == k, l).clicked() && kind != k {
            match k {
                "solid" => run(app, "design.background", json!({"color": "bg1"})),
                "gradient" => {
                    let mut g = crate::fillui::default_gradient(deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Bg2), 90.0);
                    g.rotate_with_shape = false;
                    run(app, "design.background", json!({"gradient": crate::fillui::gradient_json(&g)}));
                }
                "picture" => {
                    if let Some(pick) = app.services.pick_open.as_mut()
                        && let Some(path) = pick("picture")
                        && let Some(Ok(bytes)) = app.services.read.as_ref().map(|r| r(&path))
                    {
                        let _ = app.run("design.background", json!({"picture": deckcraft_engine::cmd::base64_encode(&bytes)}));
                    }
                }
                "pattern" => run(app, "design.background", json!({"pattern": {"preset": "pct20", "fg": "accent1", "bg": "bg1"}})),
                _ => run(app, "design.background", json!({"reset": true})),
            }
        }
    }
    ui.add_space(4.0);
    match &fill {
        Some(Fill::Solid { color }) => {
            current = Some(json!({"color": cref_param(color)}));
            ui.horizontal(|ui| {
                ui.label("Color");
                let r = widgets::drop_button(ui, "Color", vec2(70.0, 20.0), true);
                widgets::color_popup(&r).show(|ui| {
                    if let Some(Some(c)) = widgets::color_grid(ui, &sc, None) {
                        run(app, "design.background", json!({"color": cref_param(&c)}));
                    }
                });
            });
            let mut tr = ((1.0 - color.alpha()) * 100.0) as f32;
            if ui.add(egui::Slider::new(&mut tr, 0.0..=100.0).text("Transparency").suffix("%")).drag_stopped() {
                run(app, "design.background", json!({"transparency": (f64::from(tr) / 100.0).clamp(0.0, 1.0)}));
            }
        }
        Some(Fill::Gradient(g)) => {
            current = Some(json!({"gradient": crate::fillui::gradient_json(g)}));
            if let Some(ng) = crate::fillui::gradient_editor(ui, "bg-grad", g, &sc) {
                run(app, "design.background", json!({"gradient": crate::fillui::gradient_json(&ng)}));
            }
        }
        Some(Fill::Pattern(pt)) => {
            current = Some(json!({"pattern": crate::fillui::pattern_json(pt)}));
            if let Some(np) = crate::fillui::pattern_editor(ui, pt, &sc) {
                run(app, "design.background", json!({"pattern": crate::fillui::pattern_json(&np)}));
            }
        }
        Some(Fill::Picture(pf)) => {
            if ui.button("Insert picture from file…").clicked()
                && let Some(pick) = app.services.pick_open.as_mut()
                && let Some(path) = pick("picture")
                && let Some(Ok(bytes)) = app.services.read.as_ref().map(|r| r(&path))
            {
                let _ = app.run("design.background", json!({"picture": deckcraft_engine::cmd::base64_encode(&bytes)}));
            }
            picture_background_options(app, ui, pf);
        }
        _ => {}
    }
    ui.add_space(4.0);
    let hide = app.session.active().and_then(|d| d.current_slide().map(|s| !s.show_master_shapes)).unwrap_or(false);
    let mut h = hide;
    if ui.checkbox(&mut h, "Hide background graphics").changed() {
        run(app, "design.hideBackgroundGraphics", json!({"hide": h}));
    }
    ui.add_space(8.0);
    let picture = matches!(fill, Some(Fill::Picture(_)));
    ui.horizontal(|ui| {
        if ui.add_enabled(current.is_some() || picture, egui::Button::new("Apply to All")).clicked() {
            if let Some(Value::Object(mut o)) = current.clone() {
                o.insert("all".into(), json!(true));
                run(app, "design.background", Value::Object(o));
            } else if picture {
                run(app, "design.background", json!({"copy": true}));
            }
        }
        if ui.button("Reset Background").clicked() {
            run(app, "design.background", json!({"reset": true}));
        }
    });
}

/// Render picture-fill background controls and route transparency, tiling, and tile geometry
/// changes through the engine.
fn picture_background_options(app: &mut SlideApp, ui: &mut Ui, pf: &deckcraft_model::style::PictureFill) {
    use deckcraft_model::style::PictureMode;
    let alpha = pf.alpha.unwrap_or(1.0);
    let mut tr = ((1.0 - alpha) * 100.0) as f32;
    if ui.add(egui::Slider::new(&mut tr, 0.0..=100.0).text("Transparency").suffix("%")).drag_stopped() {
        run(app, "design.background", json!({"transparency": (f64::from(tr) / 100.0).clamp(0.0, 1.0)}));
    }
    let mut tile = matches!(pf.mode, PictureMode::Tile { .. });
    if ui.checkbox(&mut tile, "Tile picture as texture").changed() {
        run(app, "design.background", json!({"pictureProps": {"tile": tile}}));
    }
    let PictureMode::Tile { tx, ty, sx, sy, flip, align } = &pf.mode else { return };
    for (label, id, value, suffix, scale) in [
        ("Offset X", "bg-ox", *tx, " pt", 1.0),
        ("Offset Y", "bg-oy", *ty, " pt", 1.0),
        ("Scale X", "bg-sx", *sx * 100.0, "%", 0.01),
        ("Scale Y", "bg-sy", *sy * 100.0, "%", 0.01),
    ] {
        ui.horizontal(|ui| {
            ui.add_sized(vec2(72.0, 20.0), egui::Label::new(label));
            if let Some(v) = widgets::spinner(ui, id, label, value, if scale < 1.0 { 5.0 } else { 1.0 }, suffix, 64.0) {
                let key = match id {
                    "bg-ox" => "offsetX",
                    "bg-oy" => "offsetY",
                    "bg-sx" => "scaleX",
                    _ => "scaleY",
                };
                run(app, "design.background", json!({"pictureProps": {key: v * scale}}));
            }
        });
    }
    let align_name = match align.as_str() {
        "t" => "Top",
        "tr" => "Top Right",
        "l" => "Left",
        "ctr" => "Center",
        "r" => "Right",
        "bl" => "Bottom Left",
        "b" => "Bottom",
        "br" => "Bottom Right",
        _ => "Top Left",
    };
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(72.0, 20.0), egui::Label::new("Alignment"));
        let combo = egui::ComboBox::from_id_salt("bg-align").selected_text(align_name).width(140.0).show_ui(ui, |ui| {
            for (k, l) in [
                ("tl", "Top Left"),
                ("t", "Top"),
                ("tr", "Top Right"),
                ("l", "Left"),
                ("ctr", "Center"),
                ("r", "Right"),
                ("bl", "Bottom Left"),
                ("b", "Bottom"),
                ("br", "Bottom Right"),
            ] {
                if ui.selectable_label(align == k, l).clicked() {
                    run(app, "design.background", json!({"pictureProps": {"align": k}}));
                }
            }
        });
        let _ = combo.response.labelled_by(field.id);
    });
    let mirror_name = match flip.as_str() {
        "x" => "Horizontal",
        "y" => "Vertical",
        "xy" => "Both",
        _ => "None",
    };
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(72.0, 20.0), egui::Label::new("Mirror"));
        let combo = egui::ComboBox::from_id_salt("bg-mirror").selected_text(mirror_name).width(140.0).show_ui(ui, |ui| {
            for (k, l) in [("none", "None"), ("x", "Horizontal"), ("y", "Vertical"), ("xy", "Both")] {
                if ui.selectable_label(flip == k, l).clicked() {
                    run(app, "design.background", json!({"pictureProps": {"mirror": k}}));
                }
            }
        });
        let _ = combo.response.labelled_by(field.id);
    });
}
