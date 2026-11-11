use super::effect_presets;
use super::{Fill, Icon, ShapeKind, SlideApp, Tokens, Ui, color_button, cref_param, json, run, scheme, theme, vec2, widgets};
use deckcraft_model::text::TextDir;

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

fn format_tabs(app: &mut SlideApp, ui: &mut Ui, sh: &deckcraft_model::Shape) {
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
}

fn effects_tab(app: &mut SlideApp, ui: &mut Ui, sh: &deckcraft_model::Shape) {
    ui.label(egui::RichText::new("Shadow").font(theme::bold(13.0)));
    let fx = sh.effects.clone().unwrap_or_default();
    if let Some(v) = effect_presets::shadow_row(ui, effect_presets::shadow_preset_id(&fx)) {
        run(app, "shape.effects", json!({"shadow": v}));
    }
    let shadow = fx.inner_shadow.as_ref().or(fx.outer_shadow.as_ref());
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
        fx_slider(app, ui, ("shadow", sh.id.0, "s"), "Size", sw.sx * 100.0, 1.0..=200.0, "%", "shadowProps", "size", 0.01);
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
    if let Some(r) = effect_presets::glow_row(ui, radius) {
        run(app, "shape.effects", json!({"glowProps": {"radius": r}}));
    }
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
    if let Some(r) = effect_presets::soft_edge_row(ui, s) {
        run(app, "shape.effects", json!({"softEdges": r}));
    }
    if ui.add(egui::Slider::new(&mut s, 0.0..=50.0).text("size (pt)")).drag_stopped() {
        run(app, "shape.effects", json!({"softEdges": s}));
    }
    let raw3d = sh.effects.as_ref().and_then(|e| e.raw3d.clone());
    three_d_options(app, ui, sh.id.0, raw3d.as_deref());
    if ui.button("Reset Effects").clicked() {
        run(app, "shape.effects", json!({"reset": true}));
    }
}

/// A group keeps no slide background fill of its own. It shows Slide background fill when every
/// shape or text box in it has one, including when the group keeps a fill that a picture in it uses.
fn members_have_background_fill(sh: &deckcraft_model::Shape) -> bool {
    if !matches!(sh.kind, ShapeKind::Group { .. }) {
        return false;
    }
    let (mut any, mut all) = (false, true);
    deckcraft_model::walk(sh.children(), &mut |m, _| {
        if matches!(m.kind, ShapeKind::Shape) {
            any = true;
            all &= m.fill == Some(Fill::Background);
        }
    });
    any && all
}

/// Only a shape or text box saves a slide background fill. A group offers it when it holds one.
fn selection_takes_background_fill(app: &SlideApp) -> bool {
    let mut ok = false;
    if let Some(st) = app.session.active() {
        for s in st.selected_shapes() {
            deckcraft_model::walk(std::slice::from_ref(s), &mut |m, _| ok |= matches!(m.kind, ShapeKind::Shape));
        }
    }
    ok
}

/// Format Shape ▸ Size & Properties: size, rotation, scale, position and alt text.
fn size_tab(app: &mut SlideApp, ui: &mut Ui, sh: &deckcraft_model::Shape, x: deckcraft_model::Xfrm, single: bool, uid: u64) {
    let lock_id = ui.scope_id().with("fmt-lock-aspect");
    let mut lock: bool = ui.data_mut(|d| d.get_temp(lock_id).unwrap_or(false));
    let locked = lock;
    // Scale is measured from the size the shape had when this tab first showed it, like the
    // Size and Position dialog; the origin choice is kept for the session.
    let base_id = ui.scope_id().with(("fmt-scale-base", uid, sh.id.0));
    let (base_w, base_h): (f64, f64) = ui.data_mut(|d| d.get_temp(base_id).unwrap_or((x.w, x.h)));
    ui.data_mut(|d| d.insert_temp(base_id, (base_w, base_h)));
    let origin_id = ui.scope_id().with("fmt-origin-center");
    let mut center: bool = single && ui.data_mut(|d| d.get_temp(origin_id).unwrap_or(false));
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
    for (label, id, cur, base, key) in [("Scale Height", "fsh", x.h, base_h, "h"), ("Scale Width", "fsw", x.w, base_w, "w")] {
        ui.add_enabled_ui(single, |ui| {
            ui.horizontal(|ui| {
                ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
                let pct = if base > 0.0 { cur / base * 100.0 } else { 100.0 };
                if let Some(nv) = widgets::spinner(ui, id, label, pct, 1.0, "%", 70.0)
                    && nv > 0.0
                {
                    let mut p = json!({key: base * nv / 100.0});
                    if locked {
                        p["lockAspect"] = json!(true);
                    }
                    run(app, "shape.sizePosition", p);
                }
            })
        });
    }
    if ui.checkbox(&mut lock, "Lock aspect ratio").changed() {
        ui.data_mut(|d| d.insert_temp(lock_id, lock));
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Position").font(theme::bold(13.0)));
    ui.add_enabled_ui(single, |ui| {
        ui.horizontal(|ui| {
            ui.add_sized(vec2(90.0, 20.0), egui::Label::new("From"));
            let combo =
                craft_ui::ComboBox::from_id_salt("fmt-origin").selected_text(if center { "Center" } else { "Top Left Corner" }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut center, false, "Top Left Corner");
                    ui.selectable_value(&mut center, true, "Center");
                });
            combo.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "From"));
        })
    });
    if single {
        ui.data_mut(|d| d.insert_temp(origin_id, center));
    }
    let (dx, dy) = if center { (x.w / 2.0, x.h / 2.0) } else { (0.0, 0.0) };
    // The spinner shows `value / unit`; `shape.move` takes the top-left corner, so a centre
    // reading is shifted by half the size in both directions.
    let (px, py) = (x.x + dx, x.y + dy);
    let mut pos = |ui: &mut Ui, label: &str, id: &str, v: f64, off: f64, key: &str| {
        ui.horizontal(|ui| {
            ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
            if let Some(nv) = widgets::spinner(ui, id, label, v / len.points_per_unit(), 0.1, len.suffix(), 70.0) {
                run(app, "shape.move", json!({key: nv * len.points_per_unit() - off}));
            }
        });
    };
    pos(ui, "Horizontal", "fx", px, dx, "x");
    pos(ui, "Vertical", "fy", py, dy, "y");
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

/// Render the shape-format pane with fill/line, effects, size/properties, text, and picture tabs.
pub(super) fn format_shape(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let Some(sh) = st.selected_shapes().first().map(|s| (*s).clone()) else {
        ui.label(egui::RichText::new("Select a shape to format it.").color(t.text_dim));
        return;
    };
    let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, &sh);
    // Scale and a centre origin work from the shown shape's size, so they need a single shape.
    let single = st.selected_shapes().len() == 1;
    let uid = st.uid;
    format_tabs(app, ui, &sh);
    ui.separator();
    match app.ui.format_tab.as_str() {
        "effects" => effects_tab(app, ui, &sh),
        "size" => size_tab(app, ui, &sh, x, single, uid),
        "text" => {
            text_fill_options(app, ui, &sh);
            ui.add_space(6.0);
            text_outline_options(app, ui, &sh);
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Text Box").font(theme::bold(13.0)));
            let body = effective_body(app, &sh);
            let dir = match body.vert {
                None | Some(TextDir::Horizontal) => "horizontal",
                Some(TextDir::Vertical270) => "rotate270",
                Some(TextDir::Stacked | TextDir::StackedRtl) => "stacked",
                Some(_) => "rotate90",
            };
            text_choice(
                app,
                ui,
                "Text direction",
                "format.direction",
                "dir",
                dir,
                &[("Horizontal", "horizontal"), ("Rotate all text 90°", "rotate90"), ("Rotate all text 270°", "rotate270"), ("Stacked", "stacked")],
            );
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
            let mut upright = body.upright == Some(true);
            if ui.checkbox(&mut upright, "Do not rotate text").changed() {
                run(app, "format.upright", json!({"on": upright}));
            }
            let wrap = body.wrap.unwrap_or(true);
            let mut w = wrap;
            if ui.checkbox(&mut w, "Wrap text in shape").changed() {
                run(app, "format.wrap", json!({"on": w}));
            }
            ui.label("Margins (pt)");
            let b = body;
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
            let members_background = members_have_background_fill(&sh);
            let kind = match &sh.fill {
                _ if members_background => "background",
                None => "auto",
                Some(Fill::None) => "none",
                Some(Fill::Solid { .. }) => "solid",
                Some(Fill::Gradient(_)) => "gradient",
                Some(Fill::Picture(_)) => "picture",
                Some(Fill::Pattern(_)) => "pattern",
                Some(Fill::Background) => "background",
                _ => "auto",
            };
            let background_ok = selection_takes_background_fill(app);
            for (l, k) in [
                ("No fill", "none"),
                ("Solid fill", "solid"),
                ("Gradient fill", "gradient"),
                ("Picture or texture fill", "picture"),
                ("Pattern fill", "pattern"),
                ("Slide background fill", "background"),
                ("Automatic", "auto"),
            ] {
                if k == "background" && !background_ok {
                    continue;
                }
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
                        "background" => run(app, "shape.fill", json!({"background": true})),
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
            let transparency = !members_background && matches!(sh.fill, None | Some(Fill::Solid { .. } | Fill::Picture(_)));
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
            // A group that keeps a fill for its pictures shows no editor for that fill while its
            // shapes have the slide background fill.
            let shown_fill = sh.fill.as_ref().filter(|_| !members_background);
            if let Some(Fill::Picture(pf)) = shown_fill {
                picture_fill_options(app, ui, sh.id.0, pf);
            }
            if let Some(Fill::Gradient(g)) = shown_fill {
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
            if let Some(Fill::Pattern(pt)) = shown_fill
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

/// A labelled drop-down on the Text Options tab: picks one of `items` (label, value) and runs
/// `cmd` with `{key: value}`.
fn text_choice(app: &mut SlideApp, ui: &mut Ui, label: &str, cmd: &str, key: &str, current: &str, items: &[(&str, &str)]) {
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
        let shown = items.iter().find(|(_, v)| *v == current).map_or(current, |(l, _)| *l);
        let combo = craft_ui::ComboBox::from_id_salt(("fmt-text", cmd, key)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
            for (l, v) in items {
                if ui.selectable_label(*v == current, *l).clicked() {
                    run(app, cmd, json!({key: v}));
                }
            }
        });
        let _ = combo.response.labelled_by(field.id);
    });
}

/// The effective body properties shown by Text Options, including placeholder inheritance.
fn effective_body(app: &SlideApp, sh: &deckcraft_model::Shape) -> deckcraft_model::text::BodyProps {
    let local = || sh.text.as_ref().map(|t| t.body.clone()).unwrap_or_default();
    let Some(st) = app.session.active() else { return local() };
    let Some(ctx) = selection_context(st) else { return local() };
    deckcraft_model::resolve::body(&ctx, sh)
}

fn selection_context(st: &deckcraft_engine::DocState) -> Option<deckcraft_model::resolve::Ctx<'_>> {
    use deckcraft_engine::Target;
    use deckcraft_model::resolve::Ctx;
    match st.selection.target {
        Target::Slides => st.current_slide().and_then(|slide| Ctx::for_slide(&st.doc, slide)),
        Target::Master { master } => st.doc.masters.get(master).map(|m| Ctx::for_master(&st.doc, m)),
        Target::Layout { master, layout } => st.doc.masters.get(master).and_then(|m| m.layouts.get(layout).map(|l| Ctx::for_layout(&st.doc, m, l))),
        target @ (Target::Handout | Target::NotesMaster) => deckcraft_engine::page_master(&st.doc, target).map(|m| Ctx::for_master(&st.doc, m)),
    }
}

/// The selected run's effective props, or the first run for a shape-level selection.
pub(crate) fn first_run_props(app: &SlideApp, sh: &deckcraft_model::Shape) -> Option<deckcraft_model::text::RunProps> {
    let st = app.session.active()?;
    let ctx = selection_context(st)?;
    let shape = if let Some(t) = st.selection.text.as_ref() { st.shape(t.shape)? } else { sh };
    let (para, run) = if let Some(t) = st.selection.text.as_ref() {
        let body = deckcraft_engine::cmd::text::body_of(st, t)?;
        let (a, _) = t.ordered();
        let para = body.paragraphs.get(a.0)?;
        let pos = if t.is_range() { (a.0, a.1.saturating_add(1)) } else { a };
        (para, deckcraft_model::edit::props_at(body, pos))
    } else {
        let body = sh.text.as_ref()?;
        let para = body.paragraphs.iter().find(|p| !p.runs.is_empty()).or_else(|| body.paragraphs.first())?;
        let run = para.runs.first().map(|r| r.props.clone()).unwrap_or_default();
        (para, run)
    };
    Some(deckcraft_model::resolve::run(&ctx, shape, para, &run))
}

/// Text Fill: No fill, Solid fill or Gradient fill (with its angle), the colour and the transparency.
fn text_fill_options(app: &mut SlideApp, ui: &mut Ui, sh: &deckcraft_model::Shape) {
    use deckcraft_model::style::GradientShape;
    ui.label(egui::RichText::new("Text Fill").font(theme::bold(13.0)));
    let fill = first_run_props(app, sh).and_then(|r| r.fill);
    let (kind, angle) = match &fill {
        Some(Fill::None) => ("none", 0.0),
        Some(Fill::Gradient(g)) => ("gradient", if let GradientShape::Linear { angle, .. } = g.shape { angle } else { 0.0 }),
        _ => ("solid", 0.0),
    };
    ui.horizontal(|ui| {
        for (l, m) in [("No fill", "none"), ("Solid fill", "solid"), ("Gradient fill", "gradient")] {
            if ui.selectable_label(kind == m, l).clicked() && kind != m {
                run(app, "format.textFill", json!({"mode": m}));
            }
        }
    });
    if kind == "solid" {
        color_button(app, ui, "Color", None, |c| match c {
            Some(c) => ("format.color".into(), json!({"color": cref_param(&c)})),
            None => ("format.color".into(), json!({"color": "tx1"})),
        });
    }
    if kind == "gradient" {
        ui.horizontal(|ui| {
            ui.add_sized(vec2(90.0, 20.0), egui::Label::new("Angle"));
            if let Some(nv) = widgets::spinner(ui, "text-fill-angle", "Angle", angle, 15.0, "°", 70.0) {
                run(app, "format.textFill", json!({"mode": "gradient", "angle": nv}));
            }
        });
    }
    if kind != "none" {
        text_transparency_slider(app, ui, sh, fill.as_ref());
    }
}

/// Text Outline: No line or Solid line, with its colour, width and dash.
fn text_outline_options(app: &mut SlideApp, ui: &mut Ui, sh: &deckcraft_model::Shape) {
    ui.label(egui::RichText::new("Text Outline").font(theme::bold(13.0)));
    let line = first_run_props(app, sh).and_then(|r| r.outline).filter(|l| !matches!(l.fill, Some(Fill::None)));
    ui.horizontal(|ui| {
        if ui.selectable_label(line.is_none(), "No line").clicked() && line.is_some() {
            run(app, "format.textOutline", json!({"none": true}));
        }
        if ui.selectable_label(line.is_some(), "Solid line").clicked() && line.is_none() {
            run(app, "format.textOutline", json!({"width": 0.75}));
        }
    });
    let Some(line) = line else { return };
    color_button(app, ui, "Color", None, |c| match c {
        Some(c) => ("format.textOutline".into(), json!({"color": cref_param(&c)})),
        None => ("format.textOutline".into(), json!({"none": true})),
    });
    ui.horizontal(|ui| {
        ui.add_sized(vec2(90.0, 20.0), egui::Label::new("Width"));
        if let Some(nv) = widgets::spinner(ui, "text-outline-width", "Width", line.width.unwrap_or(0.75), 0.25, " pt", 70.0) {
            run(app, "format.textOutline", json!({"width": nv.clamp(0.0, 1584.0)}));
        }
    });
    let dash = match &line.dash {
        None => "solid",
        Some(deckcraft_model::style::Dash::Preset { name }) => name.as_str(),
        Some(_) => "custom",
    };
    text_choice(
        app,
        ui,
        "Dash type",
        "format.textOutline",
        "dash",
        dash,
        &[
            ("Solid", "solid"),
            ("Round Dot", "sysDot"),
            ("Square Dot", "sysDash"),
            ("Dash", "dash"),
            ("Dash Dot", "dashDot"),
            ("Long Dash", "lgDash"),
            ("Long Dash Dot", "lgDashDot"),
            ("Long Dash Dot Dot", "lgDashDotDot"),
        ],
    );
}

/// A labelled drop-down on the Format Shape pane: picks one of `items` (label, value) and runs
/// `shape.line` with `{key: value}`.
fn line_choice(app: &mut SlideApp, ui: &mut Ui, label: &str, key: &str, current: &str, items: &[(&str, &str)]) {
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
        let shown = items.iter().find(|(_, v)| *v == current).map_or(current, |(l, _)| *l);
        let combo = craft_ui::ComboBox::from_id_salt(("fmt-line", key)).selected_text(shown).width(120.0).show_ui(ui, |ui| {
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
        // `changed()` also fires when the slider only rounds the value it shows (55.000000000000007 to 55,
        // or 0.5 to 1 on focus, which re-added a reflection the pane had just cleared): only a move to
        // another shown value is an edit. Accessibility and keyboard edits are such moves too.
        let shown_changed = r.changed() && v.round() != value.round();
        // A slider that only had focus when another control took it (the Reflection "None" button)
        // must not commit its stale value: only an edit the user made is committed on losing focus.
        let edited_id = sid.with("edited");
        if shown_changed {
            ui.data_mut(|d| d.insert_temp(edited_id, true));
        }
        let edited = ui.data_mut(|d| d.get_temp::<bool>(edited_id)).unwrap_or(false);
        if r.drag_stopped() || (r.lost_focus() && edited) || (shown_changed && !r.dragged()) {
            ui.data_mut(|d| {
                d.remove::<f64>(sid);
                d.remove::<bool>(edited_id);
            });
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
                    let combo = craft_ui::ComboBox::from_id_salt(("picfill", id, key)).selected_text(shown).show_ui(ui, |ui| {
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
        let combo = craft_ui::ComboBox::from_id_salt(("3d", id, key)).selected_text(preset_label(cur)).width(170.0).show_ui(ui, |ui| {
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

/// The transparency (0–1) of the effective fill shown by Text Options.
pub(crate) fn text_transparency_of(fill: Option<&Fill>) -> f64 {
    match fill {
        Some(Fill::Solid { color }) => 1.0 - color.alpha(),
        Some(Fill::Gradient(g)) => g.stops.first().map_or(0.0, |s| 1.0 - s.color.alpha()),
        _ => 0.0,
    }
}

/// Text Fill › Transparency, as in PowerPoint's Format Shape › Text Options (#32).
pub(crate) fn text_transparency_slider(app: &mut SlideApp, ui: &mut Ui, sh: &deckcraft_model::Shape, fill: Option<&Fill>) {
    ui.horizontal(|ui| {
        let label = ui.label("Transparency");
        let id = ui.scope_id().with(("text-transparency", sh.id));
        let stored = (text_transparency_of(fill) * 100.0) as f32;
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
