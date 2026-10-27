use super::*;

/// Percent transparency (0 = opaque) of a solid fill, the same number `shape.fill`'s
/// `transparency` parameter writes as an alpha transform. Anything else (no fill, a
/// gradient, a picture, a pattern) is 0.
pub(super) fn solid_fill_transparency_percent(fill: Option<&Fill>) -> f32 {
    let Some(Fill::Solid { color }) = fill else {
        return 0.0;
    };
    ((1.0 - color.alpha()) * 100.0) as f32
}

/// Fraction to pass to `shape.fill` when the slider moved. An unchanged value (a focus
/// loss, or switching to another shape) is `None`, so the pane does not rewrite the fill.
pub(super) fn fill_transparency_to_apply(shown_percent: f32, seed_percent: f32) -> Option<f64> {
    if !shown_percent.is_finite() {
        return None;
    }
    let shown = shown_percent.clamp(0.0, 100.0);
    if (shown - seed_percent).abs() <= 0.01 {
        return None;
    }
    Some(f64::from(shown) / 100.0)
}

/// Render the shape-format pane with fill/line, effects, size/properties, text, and picture tabs.
pub(super) fn format_shape(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let Some(sh) = st.selected_shapes().first().map(|s| (*s).clone()) else {
        ui.label(egui::RichText::new("Select a shape to format it.").color(t.text_dim));
        return;
    };
    let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, &sh);
    let uid = st.uid;
    // Tabs as icons.
    ui.horizontal(|ui| {
        for (tab, icon, tip) in [
            ("fill", Icon::ShapeFill, "Fill & Line"),
            ("effects", Icon::ShapeEffects, "Effects"),
            ("size", Icon::SlideSize, "Size & Properties"),
            ("text", Icon::TextBox, "Text Options"),
        ] {
            let on = app.ui.format_tab == tab;
            if widgets::icon_toggle(ui, icon, tip, true, on).clicked() {
                app.ui.format_tab = tab.into();
            }
        }
        if matches!(sh.kind, ShapeKind::Picture { .. })
            && widgets::icon_toggle(ui, Icon::Picture, "Picture", true, app.ui.format_tab == "picture").clicked()
        {
            app.ui.format_tab = "picture".into();
        }
    });
    ui.separator();
    match app.ui.format_tab.as_str() {
        "effects" => {
            ui.label(egui::RichText::new("Shadow").font(theme::bold(13.0)));
            ui.horizontal_wrapped(|ui| {
                for (l, v) in [
                    ("None", "none"),
                    ("Outer", "outer"),
                    ("Bottom", "bottom"),
                    ("Center", "center"),
                    ("Inner", "inner"),
                    ("Perspective", "perspective"),
                ] {
                    if ui.button(l).clicked() {
                        run(app, "shape.effects", json!({"shadow": v}));
                    }
                }
            });
            let fx = sh.effects.clone().unwrap_or_default();
            let shadow = fx.outer_shadow.as_ref().or(fx.inner_shadow.as_ref());
            if let Some(sw) = shadow {
                color_button(app, ui, "Color", None, |c| ("shape.effects".into(), json!({"shadowProps": {"color": c.as_ref().map(cref_param)}})));
                fx_slider(
                    app,
                    ui,
                    ("shadow", sh.id.0, "t"),
                    "Transparency",
                    (1.0 - sw.color.alpha()) * 100.0,
                    0.0..=100.0,
                    "%",
                    "shadowProps",
                    "transparency",
                    0.01,
                );
                fx_slider(app, ui, ("shadow", sh.id.0, "b"), "Blur", sw.blur, 0.0..=100.0, " pt", "shadowProps", "blur", 1.0);
                fx_slider(app, ui, ("shadow", sh.id.0, "a"), "Angle", sw.dir, 0.0..=359.0, "°", "shadowProps", "dir", 1.0);
                fx_slider(app, ui, ("shadow", sh.id.0, "d"), "Distance", sw.dist, 0.0..=200.0, " pt", "shadowProps", "dist", 1.0);
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Reflection").font(theme::bold(13.0)));
            ui.horizontal_wrapped(|ui| {
                for (l, v) in [("None", "none"), ("Tight", "tight"), ("Half", "half"), ("Full", "full")] {
                    if ui.button(l).clicked() {
                        run(app, "shape.effects", json!({"reflection": v}));
                    }
                }
            });
            if let Some(r) = &fx.reflection {
                fx_slider(
                    app,
                    ui,
                    ("refl", sh.id.0, "t"),
                    "Transparency",
                    (1.0 - r.start_alpha) * 100.0,
                    0.0..=100.0,
                    "%",
                    "reflectionProps",
                    "transparency",
                    0.01,
                );
                fx_slider(app, ui, ("refl", sh.id.0, "s"), "Size", r.end_pos * 100.0, 0.0..=100.0, "%", "reflectionProps", "size", 0.01);
                fx_slider(app, ui, ("refl", sh.id.0, "d"), "Distance", r.dist, 0.0..=100.0, " pt", "reflectionProps", "dist", 1.0);
                fx_slider(app, ui, ("refl", sh.id.0, "b"), "Blur", r.blur, 0.0..=100.0, " pt", "reflectionProps", "blur", 1.0);
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Glow").font(theme::bold(13.0)));
            let radius = fx.glow.as_ref().map_or(0.0, |g| g.radius);
            fx_slider(app, ui, ("glow", sh.id.0, "r"), "Size", radius, 0.0..=150.0, " pt", "glowProps", "radius", 1.0);
            if let Some(g) = &fx.glow {
                color_button(app, ui, "Color", None, |c| ("shape.effects".into(), json!({"glowProps": {"color": c.as_ref().map(cref_param)}})));
                fx_slider(
                    app,
                    ui,
                    ("glow", sh.id.0, "t"),
                    "Transparency",
                    (1.0 - g.color.alpha()) * 100.0,
                    0.0..=100.0,
                    "%",
                    "glowProps",
                    "transparency",
                    0.01,
                );
            }
            ui.label(egui::RichText::new("Soft Edges").font(theme::bold(13.0)));
            let mut s = sh.effects.as_ref().and_then(|e| e.soft_edge).unwrap_or(0.0);
            if ui.add(egui::Slider::new(&mut s, 0.0..=50.0).text("size (pt)")).drag_stopped() {
                run(app, "shape.effects", json!({"softEdges": s}));
            }
            let raw3d = sh.effects.as_ref().and_then(|e| e.raw3d.clone());
            three_d_options(app, ui, sh.id.0, raw3d.as_deref());
            if ui.button("Reset Effects").clicked() {
                run(app, "shape.effects", json!({"reset": true}));
            }
        }
        "size" => {
            let lock_id = egui::Id::unique("fmt-lock-aspect");
            let mut lock: bool = ui.data_mut(|d| d.get_temp(lock_id).unwrap_or(false));
            let locked = lock;
            let len = app.session.prefs.units;
            let mut fields = |ui: &mut Ui, label: &str, id: &str, v: f64, key: &str, cmd: &str, angle: bool| {
                let (unit, suffix) = if angle { (1.0, "°") } else { (len.points_per_unit(), len.suffix()) };
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
                    if let Some(nv) = widgets::spinner(ui, id, label, v / unit, 0.1, suffix, 70.0) {
                        let mut p = json!({key: nv * unit});
                        if cmd == "shape.resize" && locked {
                            p["lockAspect"] = json!(true);
                        }
                        run(app, cmd, p);
                    }
                });
            };
            ui.label(egui::RichText::new("Size").font(theme::bold(13.0)));
            fields(ui, "Height", "fh", x.h, "h", "shape.resize", false);
            fields(ui, "Width", "fw", x.w, "w", "shape.resize", false);
            fields(ui, "Rotation", "fr", x.rot, "deg", "shape.rotate", true);
            if ui.checkbox(&mut lock, "Lock aspect ratio").changed() {
                ui.data_mut(|d| d.insert_temp(lock_id, lock));
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Position").font(theme::bold(13.0)));
            fields(ui, "Horizontal", "fx", x.x, "x", "shape.move", false);
            fields(ui, "Vertical", "fy", x.y, "y", "shape.move", false);
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Alt Text").font(theme::bold(13.0)));
            let id = ui.scope_id().with(("alt", uid, sh.id.0));
            let stored: Option<(String, String)> = ui.data_mut(|d| d.get_temp(id));
            let mut alt = widgets::text_draft(stored, &sh.descr);
            let r = ui.add(egui::TextEdit::multiline(&mut alt).hint_text("Alt text").desired_rows(3).desired_width(f32::INFINITY));
            if r.lost_focus() && alt != sh.descr {
                run(app, "shape.altText", json!({"text": alt}));
            }
            ui.data_mut(|d| d.insert_temp(id, (sh.descr.clone(), alt)));
        }
        "text" => {
            ui.label(egui::RichText::new("Text Fill").font(theme::bold(13.0)));
            color_button(app, ui, "Color", None, |c| match c {
                Some(c) => ("format.color".into(), json!({"color": cref_param(&c)})),
                None => ("format.color".into(), json!({"color": "tx1"})),
            });
            text_transparency_slider(app, ui, &sh);
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Text Box").font(theme::bold(13.0)));
            ui.horizontal(|ui| {
                ui.label("Vertical alignment");
                for (l, a) in [("Top", "top"), ("Middle", "middle"), ("Bottom", "bottom")] {
                    if ui.small_button(l).clicked() {
                        run(app, "format.anchor", json!({"anchor": a}));
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                for (l, m) in [("Do not Autofit", "none"), ("Shrink text on overflow", "shrink"), ("Resize shape to fit text", "resize")] {
                    if ui.small_button(l).clicked() {
                        run(app, "format.autofit", json!({"mode": m}));
                    }
                }
            });
            let wrap = sh.text.as_ref().and_then(|t| t.body.wrap).unwrap_or(true);
            let mut w = wrap;
            if ui.checkbox(&mut w, "Wrap text in shape").changed() {
                run(app, "format.wrap", json!({"on": w}));
            }
            ui.label("Margins (pt)");
            let b = sh.text.as_ref().map(|t| t.body.clone()).unwrap_or_default();
            for (l, k, v) in [
                ("Left", "left", b.inset_l.unwrap_or(7.2)),
                ("Right", "right", b.inset_r.unwrap_or(7.2)),
                ("Top", "top", b.inset_t.unwrap_or(3.6)),
                ("Bottom", "bottom", b.inset_b.unwrap_or(3.6)),
            ] {
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(60.0, 20.0), egui::Label::new(l));
                    if let Some(nv) = widgets::spinner(ui, &format!("m{k}"), l, v, 1.0, " pt", 60.0) {
                        run(app, "format.margins", json!({k: nv.max(0.0)}));
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.label("Columns");
                for n in 1..=3 {
                    if ui.small_button(n.to_string()).clicked() {
                        run(app, "format.columns", json!({"count": n, "spacing": 18}));
                    }
                }
                if ui.small_button("Columns…").clicked() {
                    run(app, "format.columns", json!({}));
                }
            });
        }
        "picture" => {
            let adj = if let ShapeKind::Picture { fill } = &sh.kind { fill.adjust.clone() } else { Default::default() };
            let mut b = adj.brightness as f32;
            if ui.add(egui::Slider::new(&mut b, -1.0..=1.0).text("Brightness")).drag_stopped() {
                run(app, "picture.adjust", json!({"brightness": b}));
            }
            let mut c = adj.contrast as f32;
            if ui.add(egui::Slider::new(&mut c, -1.0..=1.0).text("Contrast")).drag_stopped() {
                run(app, "picture.adjust", json!({"contrast": c}));
            }
            let mut s = adj.saturation.unwrap_or(1.0) as f32;
            if ui.add(egui::Slider::new(&mut s, 0.0..=4.0).text("Saturation")).drag_stopped() {
                run(app, "picture.adjust", json!({"saturation": s}));
            }
            let alpha = if let ShapeKind::Picture { fill } = &sh.kind { fill.alpha.unwrap_or(1.0) } else { 1.0 };
            let mut tr = (1.0 - alpha) as f32;
            if ui.add(egui::Slider::new(&mut tr, 0.0..=1.0).text("Transparency")).drag_stopped() {
                run(app, "picture.adjust", json!({"transparency": tr}));
            }
            if ui.button("Reset Picture").clicked() {
                run(app, "picture.reset", json!({}));
            }
        }
        _ => {
            // Fill & Line.
            ui.label(egui::RichText::new("Fill").font(theme::bold(13.0)));
            let kind = match &sh.fill {
                None => "auto",
                Some(Fill::None) => "none",
                Some(Fill::Solid { .. }) => "solid",
                Some(Fill::Gradient(_)) => "gradient",
                Some(Fill::Picture(_)) => "picture",
                Some(Fill::Pattern(_)) => "pattern",
                _ => "auto",
            };
            for (l, k) in [
                ("No fill", "none"),
                ("Solid fill", "solid"),
                ("Gradient fill", "gradient"),
                ("Picture or texture fill", "picture"),
                ("Pattern fill", "pattern"),
                ("Automatic", "auto"),
            ] {
                if ui.radio(kind == k, l).clicked() {
                    match k {
                        "none" => run(app, "shape.fill", json!({"none": true})),
                        "solid" => run(app, "shape.fill", json!({"color": "accent1"})),
                        "gradient" => run(
                            app,
                            "shape.fill",
                            json!({"gradient": {"stops": [[0, "accent1"], [1, {"scheme": "accent1", "lumMod": 50000}]], "angle": 90}}),
                        ),
                        "pattern" => run(app, "shape.fill", json!({"pattern": {"preset": "pct25", "fg": "accent1", "bg": "bg1"}})),
                        "picture" => pick_fill_picture(app),
                        _ => run(app, "shape.fill", json!({"reset": true})),
                    }
                }
            }
            // The Color button sets a solid fill, so it shows only for a solid or automatic fill. A
            // gradient takes its colors from its stops, and a pattern takes them from its Foreground
            // and Background menus below.
            if matches!(kind, "solid" | "auto") {
                color_button(app, ui, "Color", Some("No Fill"), |c| match c {
                    Some(c) => ("shape.fill".into(), json!({"color": cref_param(&c)})),
                    None => ("shape.fill".into(), json!({"none": true})),
                });
            }
            // `shape.fill {transparency}` changes a solid fill (an automatic one through its
            // effective color) and a picture fill; a gradient has a transparency per stop.
            let transparency = matches!(sh.fill, None | Some(Fill::Solid { .. } | Fill::Picture(_)));
            ui.horizontal(|ui| {
                if !transparency {
                    return;
                }
                let label = ui.label("Transparency");
                // Seed from this shape. A shared temp defaulted to 0, so a shape that was
                // already 40% transparent showed as opaque, and leaving the slider wrote 0.
                let seed = match &sh.fill {
                    Some(Fill::Picture(pf)) => ((1.0 - pf.alpha.unwrap_or(1.0)) * 100.0) as f32,
                    fill => solid_fill_transparency_percent(fill.as_ref()),
                };
                let id = ui.scope_id().with(("ftr", sh.id.0));
                let mut v: f32 = ui.data_mut(|d| d.get_temp(id).unwrap_or(seed));
                let r = ui.add(egui::Slider::new(&mut v, 0.0..=100.0).suffix("%")).labelled_by(label.id);
                if r.drag_stopped() || r.lost_focus() {
                    if let Some(t) = fill_transparency_to_apply(v, seed) {
                        let _ = app.run("shape.fill", json!({"transparency": t}));
                    }
                    ui.data_mut(|d| d.remove::<f32>(id));
                } else if r.dragged() || r.changed() {
                    ui.data_mut(|d| d.insert_temp(id, v));
                }
            });
            if let Some(Fill::Picture(pf)) = &sh.fill {
                picture_fill_options(app, ui, sh.id.0, pf);
            }
            if let Some(Fill::Gradient(g)) = &sh.fill {
                let sc = scheme(app);
                if let Some(ng) = crate::fillui::gradient_editor(ui, "shape-grad", g, &sc) {
                    run(app, "shape.fill", json!({"gradient": crate::fillui::gradient_json(&ng)}));
                }
                let mut rot = g.rotate_with_shape;
                if ui.checkbox(&mut rot, "Rotate with shape").changed() {
                    let ng = deckcraft_model::style::Gradient { rotate_with_shape: rot, ..g.clone() };
                    run(app, "shape.fill", json!({"gradient": crate::fillui::gradient_json(&ng)}));
                }
            }
            if let Some(Fill::Pattern(pt)) = &sh.fill
                && let Some(np) = crate::fillui::pattern_editor(ui, pt, &scheme(app))
            {
                run(app, "shape.fill", json!({"pattern": crate::fillui::pattern_json(&np)}));
            }
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Line").font(theme::bold(13.0)));
            let none = matches!(sh.line.as_ref().and_then(|l| l.fill.as_ref()), Some(Fill::None));
            if ui.radio(none, "No line").clicked() {
                run(app, "shape.line", json!({"none": true}));
            }
            if ui.radio(!none && sh.line.is_some(), "Solid line").clicked() {
                run(app, "shape.line", json!({"color": "tx1", "width": 1}));
            }
            if ui.radio(sh.line.is_none(), "Automatic").clicked() {
                run(app, "shape.line", json!({"reset": true}));
            }
            color_button(app, ui, "Color", Some("No Outline"), |c| match c {
                Some(c) => ("shape.line".into(), json!({"color": cref_param(&c)})),
                None => ("shape.line".into(), json!({"none": true})),
            });
            ui.horizontal(|ui| {
                ui.label("Width");
                let w = sh.line.as_ref().and_then(|l| l.width).unwrap_or(0.75);
                if let Some(v) = widgets::spinner(ui, "lw", "Width", w, 0.25, " pt", 60.0) {
                    run(app, "shape.line", json!({"width": v.max(0.0)}));
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Dash");
                for (l, d) in [("—", "solid"), ("···", "sysDot"), ("- -", "dash"), ("-·-", "dashDot"), ("— —", "lgDash")] {
                    if ui.small_button(l).clicked() {
                        run(app, "shape.line", json!({"dash": d}));
                    }
                }
            });
            line_details(app, ui, &sh);
        }
    }
}

/// A labelled drop-down on the Format Shape pane: picks one of `items` (label, value) and runs
/// `shape.line` with `{key: value}`.
fn line_choice(app: &mut SlideApp, ui: &mut Ui, label: &str, key: &str, current: &str, items: &[(&str, &str)]) {
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
        let shown = items.iter().find(|(_, v)| *v == current).map_or(current, |(l, _)| *l);
        let combo = egui::ComboBox::from_id_salt(("fmt-line", key)).selected_text(shown).width(120.0).show_ui(ui, |ui| {
            for (l, v) in items {
                if ui.selectable_label(*v == current, *l).clicked() {
                    run(app, "shape.line", json!({key: v}));
                }
            }
        });
        let _ = combo.response.labelled_by(field.id);
    });
}

/// The rest of PowerPoint's Line page: compound type, cap and join types, and the begin and end
/// arrows with their sizes.
fn line_details(app: &mut SlideApp, ui: &mut Ui, sh: &deckcraft_model::Shape) {
    use deckcraft_model::style::{Compound, LineCap, LineJoin};
    let l = sh.line.clone().unwrap_or_default();
    let compound = match l.compound.unwrap_or_default() {
        Compound::Single => "single",
        Compound::Double => "double",
        Compound::ThickThin => "thickThin",
        Compound::ThinThick => "thinThick",
        Compound::Triple => "triple",
    };
    line_choice(
        app,
        ui,
        "Compound type",
        "compound",
        compound,
        &[("Single", "single"), ("Double", "double"), ("Thick Thin", "thickThin"), ("Thin Thick", "thinThick"), ("Triple", "triple")],
    );
    let cap = match l.cap.unwrap_or_default() {
        LineCap::Flat => "flat",
        LineCap::Round => "round",
        LineCap::Square => "square",
    };
    line_choice(app, ui, "Cap type", "cap", cap, &[("Flat", "flat"), ("Round", "round"), ("Square", "square")]);
    let join = match l.join.unwrap_or_default() {
        LineJoin::Round => "round",
        LineJoin::Bevel => "bevel",
        LineJoin::Miter => "miter",
    };
    line_choice(app, ui, "Join type", "join", join, &[("Round", "round"), ("Bevel", "bevel"), ("Miter", "miter")]);
    let arrows = [("None", "none"), ("Arrow", "triangle"), ("Open Arrow", "arrow"), ("Stealth", "stealth"), ("Diamond", "diamond"), ("Oval", "oval")];
    let sizes = [("Small", "sm"), ("Medium", "med"), ("Large", "lg")];
    for (label, key, size_label, size_key, end) in
        [("Begin arrow", "head", "Begin size", "headSize", &l.head), ("End arrow", "tail", "End size", "tailSize", &l.tail)]
    {
        line_choice(app, ui, label, key, end.as_ref().map_or("none", |e| e.kind.as_str()), &arrows);
        if let Some(e) = end {
            line_choice(app, ui, size_label, size_key, &e.w, &sizes);
        }
    }
}

/// A slider for one property of a shape effect: shows `value` (in display units), and when the
/// drag ends runs `shape.effects` with `{key: {prop: shown × scale}}`.
#[allow(clippy::too_many_arguments)]
fn fx_slider(
    app: &mut SlideApp,
    ui: &mut Ui,
    id: (&str, u32, &str),
    label: &str,
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
    key: &str,
    prop: &str,
    scale: f64,
) {
    ui.horizontal(|ui| {
        let label = ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
        // Hold the value while dragging; otherwise follow the shape.
        let sid = ui.scope_id().with(id);
        let mut v: f64 = ui.data_mut(|d| d.get_temp(sid)).unwrap_or(value);
        let r = ui.add(egui::Slider::new(&mut v, range).suffix(suffix).max_decimals(0)).labelled_by(label.id);
        if r.dragged() || r.has_focus() {
            ui.data_mut(|d| d.insert_temp(sid, v));
        } else {
            ui.data_mut(|d| d.remove::<f64>(sid));
        }
        if r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged()) {
            ui.data_mut(|d| d.remove::<f64>(sid));
            run(app, "shape.effects", json!({key: {prop: v * scale}}));
        }
    });
}

/// Asks for a picture file and makes it the selected shapes' fill.
fn pick_fill_picture(app: &mut SlideApp) {
    if let Some(pick) = app.services.pick_open.as_mut()
        && let Some(path) = pick("picture")
        && let Some(Ok(bytes)) = app.services.read.as_ref().map(|r| r(&path))
    {
        run(app, "shape.fill", json!({"picture": deckcraft_engine::cmd::base64_encode(&bytes)}));
    }
}

/// A number field for one picture-fill option; runs `shape.fill` with `{pictureProps: {key: v × scale}}`
/// when it changes.
fn pic_drag(app: &mut SlideApp, ui: &mut Ui, label: &str, value: f64, suffix: &str, key: &str, scale: f64) {
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
        let mut v = value;
        let r = ui.add(egui::DragValue::new(&mut v).suffix(suffix).max_decimals(1)).labelled_by(field.id);
        if r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged()) {
            run(app, "shape.fill", json!({"pictureProps": {key: v * scale}}));
        }
    });
}

/// Format Shape ▸ Fill ▸ Picture or texture fill: source, tiling, offsets, transparency.
fn picture_fill_options(app: &mut SlideApp, ui: &mut Ui, id: u32, pf: &deckcraft_model::style::PictureFill) {
    use deckcraft_model::style::PictureMode;
    ui.horizontal(|ui| {
        ui.label("Picture source");
        if ui.button("Insert…").clicked() {
            pick_fill_picture(app);
        }
    });
    let mut tile = matches!(pf.mode, PictureMode::Tile { .. });
    if ui.checkbox(&mut tile, "Tile picture as texture").changed() {
        run(app, "shape.fill", json!({"pictureProps": {"tile": tile}}));
    }
    match &pf.mode {
        PictureMode::Tile { tx, ty, sx, sy, flip, align } => {
            pic_drag(app, ui, "Offset X", *tx, " pt", "offsetX", 1.0);
            pic_drag(app, ui, "Offset Y", *ty, " pt", "offsetY", 1.0);
            pic_drag(app, ui, "Scale X", sx * 100.0, "%", "scaleX", 0.01);
            pic_drag(app, ui, "Scale Y", sy * 100.0, "%", "scaleY", 0.01);
            let aligns = [
                ("tl", "Top left"),
                ("t", "Top"),
                ("tr", "Top right"),
                ("l", "Left"),
                ("ctr", "Center"),
                ("r", "Right"),
                ("bl", "Bottom left"),
                ("b", "Bottom"),
                ("br", "Bottom right"),
            ];
            let mirrors = [("none", "None"), ("x", "Horizontal"), ("y", "Vertical"), ("xy", "Both")];
            for (label, key, opts, cur) in [("Alignment", "align", &aligns[..], align), ("Mirror type", "mirror", &mirrors[..], flip)] {
                ui.horizontal(|ui| {
                    let field = ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
                    let shown = opts.iter().find(|(k, _)| k == cur).map_or(opts[0].1, |(_, l)| *l);
                    let combo = egui::ComboBox::from_id_salt(("picfill", id, key)).selected_text(shown).show_ui(ui, |ui| {
                        for (k, l) in opts {
                            if ui.selectable_label(*k == cur.as_str(), *l).clicked() {
                                run(app, "shape.fill", json!({"pictureProps": {key: k}}));
                            }
                        }
                    });
                    let _ = combo.response.labelled_by(field.id);
                });
            }
        }
        PictureMode::Stretch { fill_rect } => {
            for (i, label) in ["Offset left", "Offset top", "Offset right", "Offset bottom"].into_iter().enumerate() {
                ui.horizontal(|ui| {
                    let field = ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
                    let mut v = fill_rect[i] * 100.0;
                    let r = ui.add(egui::DragValue::new(&mut v).suffix("%").max_decimals(0)).labelled_by(field.id);
                    if r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged()) {
                        let mut rect = *fill_rect;
                        rect[i] = v / 100.0;
                        run(app, "shape.fill", json!({"pictureProps": {"stretch": rect}}));
                    }
                });
            }
        }
    }
    let mut rot = !pf.upright;
    if ui.checkbox(&mut rot, "Rotate with shape").changed() {
        run(app, "shape.fill", json!({"pictureProps": {"rotateWithShape": rot}}));
    }
}

/// "perspectiveHeroicLeftFacing" → "Perspective heroic left facing".
fn preset_label(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if i == 0 {
            out.extend(c.to_uppercase());
        } else if c.is_uppercase() {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A drop-down of `options`; picking one runs `shape.effects` with `{threeD: {key: option}}`.
fn three_d_combo(app: &mut SlideApp, ui: &mut Ui, id: u32, label: &str, key: &str, cur: &str, options: &[&str]) {
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
        let combo = egui::ComboBox::from_id_salt(("3d", id, key)).selected_text(preset_label(cur)).width(170.0).show_ui(ui, |ui| {
            for o in options {
                if ui.selectable_label(*o == cur, preset_label(o)).clicked() {
                    run(app, "shape.effects", json!({"threeD": {key: o}}));
                }
            }
        });
        let _ = combo.response.labelled_by(field.id);
    });
}

/// A number field running `shape.effects` with `{threeD: {key: value}}` when it changes.
fn three_d_drag(app: &mut SlideApp, ui: &mut Ui, label: &str, value: f64, suffix: &str, key: &str) {
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
        let mut v = value;
        let r = ui.add(egui::DragValue::new(&mut v).suffix(suffix).max_decimals(1)).labelled_by(field.id);
        if r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged()) {
            run(app, "shape.effects", json!({"threeD": {key: v}}));
        }
    });
}

/// Format Shape ▸ Effects ▸ 3-D Format and 3-D Rotation.
fn three_d_options(app: &mut SlideApp, ui: &mut Ui, id: u32, raw: Option<&str>) {
    use deckcraft_engine::cmd::three_d::{BEVELS, CAMERAS, LIGHTS, MATERIALS, Shape3d};
    let s3 = raw.map(Shape3d::parse).unwrap_or_default();
    let with_none: Vec<&str> = std::iter::once("none").chain(BEVELS.iter().copied()).collect();
    ui.add_space(6.0);
    ui.label(egui::RichText::new("3-D Format").font(theme::bold(13.0)));
    for (label, key, b) in [("Top bevel", "bevelTop", &s3.top), ("Bottom bevel", "bevelBottom", &s3.bottom)] {
        three_d_combo(app, ui, id, label, key, b.as_ref().map_or("none", |b| b.preset.as_str()), &with_none);
        if let Some(b) = b {
            three_d_drag(app, ui, "   Width", b.w, " pt", &format!("{key}W"));
            three_d_drag(app, ui, "   Height", b.h, " pt", &format!("{key}H"));
        }
    }
    three_d_drag(app, ui, "Depth", s3.depth, " pt", "depth");
    three_d_drag(app, ui, "Contour", s3.contour, " pt", "contour");
    three_d_combo(app, ui, id, "Material", "material", &s3.material, MATERIALS);
    three_d_combo(app, ui, id, "Lighting", "lighting", &s3.light, LIGHTS);
    ui.add_space(6.0);
    ui.label(egui::RichText::new("3-D Rotation").font(theme::bold(13.0)));
    three_d_combo(app, ui, id, "Presets", "camera", &s3.camera, CAMERAS);
    let (rx, ry, rz) = s3.rot.unwrap_or((0.0, 0.0, 0.0));
    three_d_drag(app, ui, "X Rotation", rx, "°", "rotX");
    three_d_drag(app, ui, "Y Rotation", ry, "°", "rotY");
    three_d_drag(app, ui, "Z Rotation", rz, "°", "rotZ");
    three_d_drag(app, ui, "Distance from ground", s3.z, " pt", "z");
    if ui.add_enabled(raw.is_some(), egui::Button::new("Reset 3-D")).clicked() {
        run(app, "shape.effects", json!({"threeD": {"reset": true}}));
    }
}

/// The transparency (0–1) of a shape's first text run as stored on it.
pub(crate) fn text_transparency_of(sh: &deckcraft_model::Shape) -> f64 {
    let run = sh.text.as_ref().and_then(|t| t.paragraphs.iter().flat_map(|p| p.runs.iter()).next());
    match run.and_then(|r| r.props.fill.as_ref()) {
        Some(Fill::Solid { color }) => 1.0 - color.alpha(),
        Some(Fill::Gradient(g)) => g.stops.first().map_or(0.0, |s| 1.0 - s.color.alpha()),
        _ => 0.0,
    }
}

/// Text Fill › Transparency, as in PowerPoint's Format Shape › Text Options (#32).
pub(crate) fn text_transparency_slider(app: &mut SlideApp, ui: &mut Ui, sh: &deckcraft_model::Shape) {
    ui.horizontal(|ui| {
        let label = ui.label("Transparency");
        let id = ui.scope_id().with(("text-transparency", sh.id));
        let stored = (text_transparency_of(sh) * 100.0) as f32;
        let mut v: f32 = ui.data_mut(|d| d.get_temp(id).unwrap_or(stored));
        let r = ui.add(egui::Slider::new(&mut v, 0.0..=100.0).suffix("%")).labelled_by(label.id);
        if r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged()) {
            let _ = app.run("format.textTransparency", json!({"value": v as f64 / 100.0}));
            ui.data_mut(|d| d.remove::<f32>(id));
        } else if r.dragged() {
            ui.data_mut(|d| d.insert_temp(id, v));
        }
    });
}
