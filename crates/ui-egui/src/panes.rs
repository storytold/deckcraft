//! Task panes on the right: Format Shape, Format Background, Animation, Selection, Comments and
//! Design Ideas.

use deckcraft_model::{AnimClass, AnimStart, Fill, ShapeKind};
use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::animtimeline;
use crate::icons::{self, Icon};
use crate::ribbon::{cref_param, paint_anim_tile};
use crate::theme::{self, Tokens};
use crate::{SlideApp, widgets};

pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(pane) = app.ui.pane.clone() else { return };
    egui::Panel::right("task_pane")
        .resizable(true)
        .default_size(app.ui.pane_width)
        .size_range(240.0..=520.0)
        .frame(egui::Frame::NONE.fill(t.card).inner_margin(egui::Margin::same(10)))
        .show(ui, |ui| {
            app.ui.pane_width = ui.available_width() + 20.0;
            let title = match pane.as_str() {
                "format" => "Format Shape",
                "background" => "Format Background",
                "animation" => "Animation Pane",
                "selection" => "Selection",
                "comments" => "Comments",
                "designer" => "Design Ideas",
                "smartart" => "Text Pane",
                "thesaurus" => "Thesaurus",
                "revisions" => "Revisions",
                _ => "Pane",
            };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).font(theme::bold(16.0)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, CornerRadius::same(4), t.hover);
                    }
                    icons::paint(ui.painter(), r.shrink(3.0), Icon::Close, t.text_dim, false);
                    if resp.clicked() {
                        app.ui.pane = None;
                    }
                });
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match pane.as_str() {
                "format" => format_shape(app, ui),
                "background" => background(app, ui),
                "animation" => animation(app, ui),
                "selection" => crate::selpane::show(app, ui),
                "comments" => comments(app, ui),
                "designer" => designer(app, ui),
                "smartart" => smartart(app, ui),
                "thesaurus" => thesaurus(app, ui),
                "revisions" => revisions(app, ui),
                _ => {}
            });
        });
}

/// SmartArt Text Pane: one line per node, indented by level. Enter adds a line after, Tab and
/// Shift+Tab demote and promote, Backspace in an empty line removes it.
fn smartart(app: &mut SlideApp, ui: &mut Ui) {
    let Some((gid, d)) = app.session.active().and_then(deckcraft_engine::cmd::smartart::selected) else {
        ui.label("Select a SmartArt graphic to edit its text.");
        return;
    };
    let texts: Vec<String> = d.nodes.iter().map(|n| n.text.clone()).collect();
    let focused_any = ui.ctx().memory(|m| m.focused().is_some()) || app.ui.smartart_focus.is_some();
    if app.ui.smartart_drafts.0 != gid.0 || app.ui.smartart_drafts.1.len() != texts.len() || !focused_any {
        app.ui.smartart_drafts = (gid.0, texts.clone());
    }
    let mut drafts = std::mem::take(&mut app.ui.smartart_drafts.1);
    let mut action: Option<(&str, Value)> = None;
    let mut commit: Option<usize> = None;
    let mut focus_row: Option<usize> = None;
    ui.label(egui::RichText::new("Type your text here").color(Tokens::get(ui.ctx()).text_dim));
    ui.add_space(4.0);
    for (i, node) in d.nodes.iter().enumerate() {
        let Some(draft) = drafts.get_mut(i) else { break };
        let id = egui::Id::new(("smartart_line", gid.0, i));
        ui.horizontal(|ui| {
            ui.add_space(16.0 * node.level as f32);
            ui.label("•");
            let te = egui::TextEdit::singleline(draft).id(id).desired_width(f32::INFINITY);
            let r = ui.add(te);
            if app.ui.smartart_focus == Some(i) {
                r.request_focus();
                app.ui.smartart_focus = None;
            }
            if r.gained_focus() || r.has_focus() {
                app.ui.smartart_row = i;
            }
            if r.has_focus() {
                let back = ui.input(|inp| inp.key_pressed(egui::Key::Backspace));
                if back && draft.is_empty() && texts.get(i).is_some_and(String::is_empty) && d.nodes.len() > 1 {
                    action = Some(("smartArt.removeNode", json!({"index": i})));
                    focus_row = Some(i.saturating_sub(1));
                }
            }
            if r.lost_focus() {
                let (tab, shift) = ui.input(|inp| (inp.key_pressed(egui::Key::Tab), inp.modifiers.shift));
                if tab {
                    action = Some((if shift { "smartArt.promote" } else { "smartArt.demote" }, json!({"index": i})));
                    focus_row = Some(i);
                } else if ui.input(|inp| inp.key_pressed(egui::Key::Enter)) {
                    action = Some(("smartArt.addNode", json!({"index": i, "position": "after"})));
                    focus_row = Some(i + 1);
                }
                commit = Some(i);
            }
        });
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("+ Add").clicked() {
            action = Some(("smartArt.addNode", json!({"index": app.ui.smartart_row.min(d.nodes.len().saturating_sub(1)), "position": "after"})));
        }
        if ui.button("Remove").clicked() && !d.nodes.is_empty() {
            action = Some(("smartArt.removeNode", json!({"index": app.ui.smartart_row.min(d.nodes.len() - 1)})));
        }
    });
    // Commit a changed line before any structural edit, so the edit sees the new text.
    if let Some(i) = commit
        && drafts.get(i) != texts.get(i)
    {
        run(app, "smartArt.setNodes", json!({"id": gid.0, "index": i, "text": drafts.get(i).cloned().unwrap_or_default()}));
    }
    if action.is_some() {
        for (i, t) in drafts.iter().enumerate() {
            if Some(i) != commit && texts.get(i) != Some(t) {
                run(app, "smartArt.setNodes", json!({"id": gid.0, "index": i, "text": t}));
            }
        }
    }
    app.ui.smartart_drafts.1 = drafts;
    if let Some((cmd, mut p)) = action {
        if let Some(o) = p.as_object_mut() {
            o.insert("id".into(), json!(gid.0));
        }
        if let Ok(r) = app.run(cmd, p) {
            let row = r.get("index").and_then(Value::as_u64).map(|v| v as usize).or(focus_row).unwrap_or(0);
            app.ui.smartart_row = row;
            app.ui.smartart_drafts.0 = 0;
            app.ui.smartart_focus = Some(row);
        }
    }
}

fn run(app: &mut SlideApp, id: &str, p: Value) {
    let _ = app.run(id, p);
}

/// Percent transparency (0 = opaque) of a solid fill, the same number `shape.fill`'s
/// `transparency` parameter writes as an alpha transform. Anything else — no fill, a
/// gradient, a picture, a pattern — is 0, because that command only changes a solid.
fn solid_fill_transparency_percent(fill: Option<&Fill>) -> f32 {
    let Some(Fill::Solid { color }) = fill else {
        return 0.0;
    };
    ((1.0 - color.alpha()) * 100.0) as f32
}

/// Fraction to pass to `shape.fill` when the slider moved. An unchanged value (a focus
/// loss, or switching to another shape) is `None`, so the pane does not rewrite the fill.
fn fill_transparency_to_apply(shown_percent: f32, seed_percent: f32) -> Option<f64> {
    if !shown_percent.is_finite() {
        return None;
    }
    let shown = shown_percent.clamp(0.0, 100.0);
    if (shown - seed_percent).abs() <= 0.01 {
        return None;
    }
    Some(f64::from(shown) / 100.0)
}

fn scheme(app: &SlideApp) -> deckcraft_color::ColorScheme {
    app.session.active().and_then(|d| d.doc.masters.first().map(|m| m.scheme())).unwrap_or_else(|| deckcraft_model::Theme::default().colors)
}

fn color_button(
    app: &mut SlideApp,
    ui: &mut Ui,
    label: &str,
    none_label: Option<&str>,
    on_pick: impl Fn(Option<deckcraft_model::ColorRef>) -> (String, Value),
) {
    let sc = scheme(app);
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = widgets::drop_button(ui, "Color", vec2(70.0, 22.0), true);
            widgets::color_popup(&r).show(|ui| {
                if let Some(c) = widgets::color_grid(ui, &sc, none_label) {
                    let (id, p) = on_pick(c);
                    run(app, &id, p);
                }
            });
        });
    });
}

fn format_shape(app: &mut SlideApp, ui: &mut Ui) {
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
            let lock_id = egui::Id::new("fmt-lock-aspect");
            let mut lock: bool = ui.data_mut(|d| d.get_temp(lock_id).unwrap_or(false));
            let locked = lock;
            let mut fields = |ui: &mut Ui, label: &str, id: &str, v: f64, key: &str, cmd: &str, unit: f64| {
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
                    if let Some(nv) = widgets::spinner(ui, id, v / unit, 0.1, if unit == 72.0 { "\"" } else { "°" }, 70.0) {
                        let mut p = json!({key: nv * unit});
                        if cmd == "shape.resize" && locked {
                            p["lockAspect"] = json!(true);
                        }
                        run(app, cmd, p);
                    }
                });
            };
            ui.label(egui::RichText::new("Size").font(theme::bold(13.0)));
            fields(ui, "Height", "fh", x.h, "h", "shape.resize", 72.0);
            fields(ui, "Width", "fw", x.w, "w", "shape.resize", 72.0);
            fields(ui, "Rotation", "fr", x.rot, "deg", "shape.rotate", 1.0);
            if ui.checkbox(&mut lock, "Lock aspect ratio").changed() {
                ui.data_mut(|d| d.insert_temp(lock_id, lock));
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Position").font(theme::bold(13.0)));
            fields(ui, "Horizontal", "fx", x.x, "x", "shape.move", 72.0);
            fields(ui, "Vertical", "fy", x.y, "y", "shape.move", 72.0);
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Alt Text").font(theme::bold(13.0)));
            let id = ui.id().with(("alt", uid, sh.id.0));
            let stored: Option<(String, String)> = ui.data_mut(|d| d.get_temp(id));
            let mut alt = widgets::text_draft(stored, &sh.descr);
            let r = ui.add(egui::TextEdit::multiline(&mut alt).desired_rows(3).desired_width(f32::INFINITY));
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
                    if let Some(nv) = widgets::spinner(ui, &format!("m{k}"), v, 1.0, " pt", 60.0) {
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
            color_button(app, ui, "Color", Some("No Fill"), |c| match c {
                Some(c) => ("shape.fill".into(), json!({"color": cref_param(&c)})),
                None => ("shape.fill".into(), json!({"none": true})),
            });
            ui.horizontal(|ui| {
                ui.label("Transparency");
                // Seed from this shape. A shared temp defaulted to 0, so a shape that was
                // already 40% transparent showed as opaque, and leaving the slider wrote 0.
                let seed = solid_fill_transparency_percent(sh.fill.as_ref());
                let id = ui.id().with(("ftr", sh.id.0));
                let mut v: f32 = ui.data_mut(|d| d.get_temp(id).unwrap_or(seed));
                let r = ui.add(egui::Slider::new(&mut v, 0.0..=100.0).suffix("%"));
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
                if let Some(v) = widgets::spinner(ui, "lw", w, 0.25, " pt", 60.0) {
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
        ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
        let shown = items.iter().find(|(_, v)| *v == current).map_or(current, |(l, _)| *l);
        egui::ComboBox::from_id_salt(("fmt-line", key)).selected_text(shown).width(120.0).show_ui(ui, |ui| {
            for (l, v) in items {
                if ui.selectable_label(*v == current, *l).clicked() {
                    run(app, "shape.line", json!({key: v}));
                }
            }
        });
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
        ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
        // Hold the value while dragging; otherwise follow the shape.
        let sid = ui.id().with(id);
        let mut v: f64 = ui.data_mut(|d| d.get_temp(sid)).unwrap_or(value);
        let r = ui.add(egui::Slider::new(&mut v, range).suffix(suffix).max_decimals(0));
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
        ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
        let mut v = value;
        let r = ui.add(egui::DragValue::new(&mut v).suffix(suffix).max_decimals(1));
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
                    ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
                    let shown = opts.iter().find(|(k, _)| k == cur).map_or(opts[0].1, |(_, l)| *l);
                    egui::ComboBox::from_id_salt(("picfill", id, key)).selected_text(shown).show_ui(ui, |ui| {
                        for (k, l) in opts {
                            if ui.selectable_label(*k == cur.as_str(), *l).clicked() {
                                run(app, "shape.fill", json!({"pictureProps": {key: k}}));
                            }
                        }
                    });
                });
            }
        }
        PictureMode::Stretch { fill_rect } => {
            for (i, label) in ["Offset left", "Offset top", "Offset right", "Offset bottom"].into_iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
                    let mut v = fill_rect[i] * 100.0;
                    let r = ui.add(egui::DragValue::new(&mut v).suffix("%").max_decimals(0));
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
        ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
        egui::ComboBox::from_id_salt(("3d", id, key)).selected_text(preset_label(cur)).width(170.0).show_ui(ui, |ui| {
            for o in options {
                if ui.selectable_label(*o == cur, preset_label(o)).clicked() {
                    run(app, "shape.effects", json!({"threeD": {key: o}}));
                }
            }
        });
    });
}

/// A number field running `shape.effects` with `{threeD: {key: value}}` when it changes.
fn three_d_drag(app: &mut SlideApp, ui: &mut Ui, label: &str, value: f64, suffix: &str, key: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(vec2(90.0, 20.0), egui::Label::new(label));
        let mut v = value;
        let r = ui.add(egui::DragValue::new(&mut v).suffix(suffix).max_decimals(1));
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

fn background(app: &mut SlideApp, ui: &mut Ui) {
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
            current = Some(json!({"pattern": {"preset": pt.preset, "fg": cref_param(&pt.fg), "bg": cref_param(&pt.bg)}}));
            ui.horizontal_wrapped(|ui| {
                for preset in ["pct5", "pct20", "pct50", "ltHorz", "ltVert", "smGrid", "dkDnDiag", "wave", "dotGrid", "weave"] {
                    if ui.selectable_label(pt.preset == preset, preset).clicked() {
                        run(app, "design.background", json!({"pattern": {"preset": preset, "fg": cref_param(&pt.fg), "bg": cref_param(&pt.bg)}}));
                    }
                }
            });
            for (label, fg) in [("Foreground", true), ("Background", false)] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    let r = widgets::drop_button(ui, "Color", vec2(70.0, 20.0), true);
                    widgets::color_popup(&r).show(|ui| {
                        if let Some(Some(c)) = widgets::color_grid(ui, &sc, None) {
                            let (f, b) = if fg { (c, pt.bg.clone()) } else { (pt.fg.clone(), c) };
                            run(app, "design.background", json!({"pattern": {"preset": pt.preset, "fg": cref_param(&f), "bg": cref_param(&b)}}));
                        }
                    });
                });
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
            if let Some(v) = widgets::spinner(ui, id, value, if scale < 1.0 { 5.0 } else { 1.0 }, suffix, 64.0) {
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
        ui.add_sized(vec2(72.0, 20.0), egui::Label::new("Alignment"));
        egui::ComboBox::from_id_salt("bg-align").selected_text(align_name).width(140.0).show_ui(ui, |ui| {
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
    });
    let mirror_name = match flip.as_str() {
        "x" => "Horizontal",
        "y" => "Vertical",
        "xy" => "Both",
        _ => "None",
    };
    ui.horizontal(|ui| {
        ui.add_sized(vec2(72.0, 20.0), egui::Label::new("Mirror"));
        egui::ComboBox::from_id_salt("bg-mirror").selected_text(mirror_name).width(140.0).show_ui(ui, |ui| {
            for (k, l) in [("none", "None"), ("x", "Horizontal"), ("y", "Vertical"), ("xy", "Both")] {
                if ui.selectable_label(flip == k, l).clicked() {
                    run(app, "design.background", json!({"pictureProps": {"mirror": k}}));
                }
            }
        });
    });
}

fn animation(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let Some(slide) = st.current_slide() else { return };
    let anims = slide.animations.clone();
    let size = (st.doc.slide_size.width, st.doc.slide_size.height);
    let tl = deckcraft_anim::Timeline::new(slide, size.0, size.1);
    let effects = tl.effects();
    let names: Vec<(u32, String)> = st.shapes().iter().map(|s| (s.id.0, s.name.clone())).collect();
    let selected: Vec<u32> = st.selection.shapes.iter().map(|i| i.0).collect();
    // Play From starts at the effect last clicked here, or else the first effect of the selected
    // shape; with nothing selected it is Play All. Reordering or removing forgets the click: the
    // index would point at another effect.
    let pick_id = egui::Id::new(("anim_pane_pick", st.uid, st.selection.slide));
    let pick = ui.data(|m| m.get_temp::<usize>(pick_id));
    // A triggered effect is not on the click sequence, so it can't be played from.
    let eligible = |i: usize| anims.get(i).is_some_and(|a| selected.contains(&a.shape.0)) && tl.start_of(i).is_some();
    let from = pick.filter(|&i| eligible(i)).or_else(|| (0..anims.len()).find(|&i| eligible(i)));
    ui.horizontal(|ui| {
        let playing = app.anim_preview_active();
        let label = match (playing, from) {
            (true, _) => "■ Stop",
            (false, Some(_)) => "▶ Play From",
            (false, None) => "▶ Play All",
        };
        if ui.button(label).clicked() {
            let p = match (playing, from) {
                (false, Some(i)) => json!({"on": true, "from": i}),
                _ => json!({"on": !playing}),
            };
            run(app, "animation.preview", p);
        }
    });
    ui.separator();
    if anims.is_empty() {
        ui.label(egui::RichText::new("Select an object on the slide and add an animation from the Animations tab.").color(t.text_dim));
        return;
    }
    let bars = animtimeline::bars(anims.len(), &effects);
    let seconds = animtimeline::ruler_seconds(&bars);
    let track_w = (ui.available_width() * 0.45).max(60.0);
    let pps = animtimeline::pixels_per_second(track_w, seconds);
    let mut click = 0;
    for (i, a) in anims.iter().enumerate() {
        if a.start == AnimStart::OnClick {
            click += 1;
        }
        let name = names.iter().find(|(id, _)| *id == a.shape.0).map(|(_, n)| n.clone()).unwrap_or_else(|| format!("Shape {}", a.shape));
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
        let sel = selected.contains(&a.shape.0);
        if sel || resp.hovered() {
            ui.painter().rect_filled(r, CornerRadius::same(4), if sel { t.selected } else { t.hover });
        }
        if from == Some(i) {
            ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
        }
        let p = ui.painter();
        p.text(
            pos2(r.min.x + 6.0, r.center().y),
            Align2::LEFT_CENTER,
            if a.start == AnimStart::OnClick { click.to_string() } else { String::new() },
            theme::font(12.0),
            t.text_dim,
        );
        let icon = match a.class {
            AnimClass::Entrance => Icon::AnimEntrance,
            AnimClass::Emphasis => Icon::AnimEmphasis,
            AnimClass::Exit => Icon::AnimExit,
            _ => Icon::AnimPath,
        };
        icons::paint(p, Rect::from_center_size(pos2(r.min.x + 30.0, r.center().y), vec2(18.0, 18.0)), icon, t.text, false);
        p.text(pos2(r.min.x + 44.0, r.center().y), Align2::LEFT_CENTER, &name, theme::font(12.5), t.text);
        // Advanced timeline bar: drag the body to change the delay, the right edge for the duration.
        if let Some(Some(b)) = bars.get(i) {
            let col = match a.class {
                AnimClass::Entrance => Color32::from_rgb(0x2E, 0xA3, 0x6F),
                AnimClass::Emphasis => Color32::from_rgb(0xE3, 0xB2, 0x1C),
                AnimClass::Exit => Color32::from_rgb(0xD8, 0x3F, 0x6B),
                _ => Color32::from_rgb(0x2E, 0x6F, 0xD8),
            };
            let x0 = r.max.x - track_w - 6.0;
            let x_of = |sec: f64| x0 + (sec as f32 * pps).min(track_w);
            let mut bar = Rect::from_min_max(
                pos2(x_of(b.start), r.center().y - 5.0),
                pos2(x_of(b.start + b.span).max(x_of(b.start) + 4.0), r.center().y + 5.0),
            );
            let edge = Rect::from_min_max(pos2(bar.max.x - 3.0, bar.min.y - 3.0), pos2(bar.max.x + 4.0, bar.max.y + 3.0));
            let body_resp = ui.interact(bar, ui.id().with(("anim_bar", i)), Sense::drag()).on_hover_text(animtimeline::label(b));
            body_resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Timing bar {}", i + 1)));
            let edge_resp = ui.interact(edge, ui.id().with(("anim_edge", i)), Sense::drag());
            edge_resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Duration handle {}", i + 1)));
            if edge_resp.hovered() || edge_resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            } else if body_resp.hovered() || body_resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            // The drag offset is remembered: the pointer's press origin is gone on release.
            let dx_id = ui.id().with(("anim_drag_dx", i));
            let live = ui.input(|inp| match (inp.pointer.press_origin(), inp.pointer.interact_pos()) {
                (Some(o), Some(p)) => Some(p.x - o.x),
                _ => None,
            });
            let dx = match live {
                Some(d) if body_resp.dragged() || edge_resp.dragged() => {
                    ui.data_mut(|m| m.insert_temp(dx_id, d));
                    d
                }
                _ => ui.data_mut(|m| m.get_temp::<f32>(dx_id)).unwrap_or(0.0),
            };
            if body_resp.drag_stopped() || edge_resp.drag_stopped() {
                ui.data_mut(|m| m.remove::<f32>(dx_id));
            }
            let iter_secs = f64::from(a.duration_ms) / 1000.0;
            let factor = if iter_secs > 0.0 && b.span > 0.0 { b.span / iter_secs } else { 1.0 };
            if edge_resp.dragged() || edge_resp.drag_stopped() {
                let d = animtimeline::dragged_duration(a.duration_ms, dx, pps, factor);
                bar.max.x = x_of(b.start + f64::from(d) / 1000.0 * factor).max(bar.min.x + 4.0);
                if edge_resp.drag_stopped() && d != a.duration_ms {
                    run(app, "animation.timing", json!({"index": i, "duration": d}));
                }
            } else if body_resp.dragged() || body_resp.drag_stopped() {
                let d = animtimeline::dragged_delay(a.delay_ms, dx, pps);
                let shift = (f64::from(d) - f64::from(a.delay_ms)) / 1000.0;
                let w = bar.width();
                bar.min.x = x_of((b.start + shift).max(0.0));
                bar.max.x = bar.min.x + w;
                if body_resp.drag_stopped() && d != a.delay_ms {
                    run(app, "animation.timing", json!({"index": i, "delay": d}));
                }
            }
            let p = ui.painter();
            p.rect_filled(bar, CornerRadius::same(2), col);
            if a.repeat == u32::MAX {
                p.text(pos2(bar.max.x + 2.0, bar.center().y), Align2::LEFT_CENTER, "⟳", theme::font(10.0), t.text_dim);
            }
        }
        if resp.clicked() {
            ui.data_mut(|m| m.insert_temp(pick_id, i));
            run(app, "edit.select", json!({"ids": [a.shape.0]}));
        }
        resp.context_menu(|ui| {
            for (l, v) in [("Start On Click", "onClick"), ("Start With Previous", "withPrevious"), ("Start After Previous", "afterPrevious")] {
                if ui.button(l).clicked() {
                    run(app, "animation.timing", json!({"index": i, "start": v}));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Move Up").clicked() {
                ui.data_mut(|m| m.remove::<usize>(pick_id));
                run(app, "animation.move", json!({"index": i, "to": i.saturating_sub(1)}));
                ui.close();
            }
            if ui.button("Move Down").clicked() {
                ui.data_mut(|m| m.remove::<usize>(pick_id));
                run(app, "animation.move", json!({"index": i, "to": i + 1}));
                ui.close();
            }
            if ui.button("Remove").clicked() {
                ui.data_mut(|m| m.remove::<usize>(pick_id));
                run(app, "animation.remove", json!({"index": i}));
                ui.close();
            }
        });
    }
    // Seconds ruler under the bars.
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    let x0 = r.max.x - track_w - 6.0;
    let p = ui.painter();
    p.line_segment([pos2(x0, r.min.y + 2.0), pos2(x0 + track_w, r.min.y + 2.0)], Stroke::new(1.0, t.text_faint));
    let every = (seconds / 6).max(1);
    for s in (0..=seconds).step_by(every as usize) {
        let x = x0 + s as f32 * pps;
        if x > x0 + track_w + 0.5 {
            break;
        }
        p.line_segment([pos2(x, r.min.y), pos2(x, r.min.y + 5.0)], Stroke::new(1.0, t.text_faint));
        p.text(pos2(x, r.min.y + 6.0), Align2::CENTER_TOP, format!("{s}s"), theme::font(10.0), t.text_dim);
    }
    let _ = paint_anim_tile;
}

/// Review › Compare: the revised copy's slide-level changes; click to go there, Accept to take it.
fn revisions(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Ok(v) = app.session.execute("review.revisions", &json!({})) else {
        ui.label(egui::RichText::new("Compare this presentation with a revised copy from Review › Compare.").color(t.text_dim));
        return;
    };
    let changes = v["changes"].as_array().cloned().unwrap_or_default();
    ui.label(egui::RichText::new(format!("Compared with {}", v["name"].as_str().unwrap_or("?"))).color(t.text_dim));
    if v["byPosition"].as_bool() == Some(true) {
        ui.label(egui::RichText::new("Not a copy of this presentation: slides are matched by position.").size(11.0).color(t.text_faint));
    }
    ui.add_space(4.0);
    if changes.is_empty() {
        ui.label("No differences left.");
        if ui.button("End Review").clicked() {
            run(app, "review.endCompare", json!({}));
            app.ui.pane = None;
        }
        return;
    }
    if ui.button(format!("Accept All ({})", changes.len())).clicked() {
        run(app, "review.accept", json!({"all": true}));
    }
    ui.separator();
    for (i, c) in changes.iter().enumerate() {
        let slide = c["slide"].as_u64().unwrap_or(0);
        let (head, col) = match c["kind"].as_str().unwrap_or("") {
            "inserted" => (format!("Slide inserted after {slide}"), Color32::from_rgb(0x2E, 0xA3, 0x6F)),
            "deleted" => (format!("Slide {} deleted", slide.saturating_add(1)), Color32::from_rgb(0xD8, 0x3F, 0x6B)),
            _ => (format!("Slide {} changed", slide.saturating_add(1)), Color32::from_rgb(0x2E, 0x6F, 0xD8)),
        };
        egui::Frame::NONE.fill(t.chrome).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(4.0, 16.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(2), col);
                if ui.link(egui::RichText::new(&head).font(theme::bold(12.5))).clicked() {
                    let max = app.session.active().map(|d| d.doc.slides.len().saturating_sub(1)).unwrap_or(0);
                    run(app, "slide.go", json!({"index": (slide as usize).min(max)}));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Accept").clicked() {
                        run(app, "review.accept", json!({"index": i}));
                    }
                });
            });
            for d in c["details"].as_array().into_iter().flatten().filter_map(Value::as_str).take(8) {
                ui.label(egui::RichText::new(format!("• {d}")).size(11.5).color(t.text_dim));
            }
        });
        ui.add_space(4.0);
    }
}

fn comments(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    if ui.button("+ New Comment").clicked() {
        app.dialog = Some(crate::dialogs::Dialog::new("comment"));
    }
    ui.separator();
    let threads: Vec<(usize, Vec<deckcraft_model::Comment>)> = app
        .session
        .active()
        .map(|st| st.doc.slides.iter().enumerate().filter(|(_, sl)| !sl.comments.is_empty()).map(|(i, sl)| (i, sl.comments.clone())).collect())
        .unwrap_or_default();
    if threads.is_empty() {
        ui.label(egui::RichText::new("No comments.").color(t.text_dim));
        return;
    }
    let focus = app.ui.comment_focus;
    for (si, list) in threads {
        ui.label(egui::RichText::new(format!("Slide {}", si + 1)).font(theme::bold(12.0)).color(t.text_dim));
        ui.add_space(2.0);
        for (i, c) in list.iter().enumerate() {
            comment_card(app, ui, si, i, c, focus == Some((si, i)));
            ui.add_space(6.0);
        }
    }
}

fn focus_comment(app: &mut SlideApp, slide: usize, index: usize) {
    let _ = app.run("slide.go", json!({"index": slide}));
    app.ui.comment_focus = Some((slide, index));
}

fn comment_card(app: &mut SlideApp, ui: &mut Ui, slide: usize, index: usize, c: &deckcraft_model::Comment, focused: bool) {
    let t = Tokens::get(ui.ctx());
    let stroke = if focused { Stroke::new(2.0, t.accent) } else { Stroke::NONE };
    let shown = egui::Frame::NONE
        .fill(if focused { t.selected } else { t.chrome })
        .stroke(stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let head = ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 12.0, t.accent);
                ui.painter().text(r.center(), Align2::CENTER_CENTER, &c.initials, theme::bold(11.0), Color32::WHITE);
                ui.vertical(|ui| {
                    let name = ui.add(egui::Label::new(egui::RichText::new(&c.author).font(theme::bold(12.5))).sense(Sense::click()));
                    if name.clicked() {
                        focus_comment(app, slide, index);
                    }
                    let when = c.date.replace('T', " ").trim_end_matches('Z').to_string();
                    let when = if c.resolved { format!("{when} · Resolved") } else { when };
                    ui.label(egui::RichText::new(when).size(10.5).color(t.text_faint));
                });
            });
            if head.response.clicked() {
                focus_comment(app, slide, index);
            }
            let body = ui.add(egui::Label::new(&c.text).sense(Sense::click()));
            if body.clicked() {
                focus_comment(app, slide, index);
            }
            for r in &c.replies {
                ui.label(egui::RichText::new(format!("↳ {}: {}", r.author, r.text)).color(t.text_dim));
            }
            ui.horizontal(|ui| {
                let id = ui.id().with(("reply", slide, index));
                let mut txt: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
                let r = ui.add(egui::TextEdit::singleline(&mut txt).hint_text("Reply…").desired_width(ui.available_width() - 70.0));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !txt.trim().is_empty() {
                    focus_comment(app, slide, index);
                    run(app, "comment.reply", json!({"index": index, "text": txt}));
                    txt.clear();
                }
                ui.data_mut(|d| d.insert_temp(id, txt));
                if ui.small_button(if c.resolved { "Reopen" } else { "Resolve" }).clicked() {
                    focus_comment(app, slide, index);
                    run(app, "comment.resolve", json!({"index": index, "resolved": !c.resolved}));
                }
                if ui.small_button("🗑").clicked() {
                    let _ = app.run("slide.go", json!({"index": slide}));
                    let _ = app.run("comment.delete", json!({"index": index}));
                    match app.ui.comment_focus {
                        Some((fs, fi)) if fs == slide && fi == index => app.ui.comment_focus = None,
                        Some((fs, fi)) if fs == slide && fi > index => app.ui.comment_focus = Some((fs, fi - 1)),
                        _ => {}
                    }
                }
            });
        });
    if focused {
        let key = egui::Id::new("comment-focus-scrolled");
        let seen = ui.data(|d| d.get_temp::<(usize, usize)>(key));
        if seen != Some((slide, index)) {
            ui.scroll_to_rect(shown.response.rect, Some(egui::Align::Center));
            ui.data_mut(|d| d.insert_temp(key, (slide, index)));
        }
    }
}

/// Design Ideas: offline suggestions — the current slide in each theme and layout variations.
/// Rendered idea thumbnails, kept until the deck, slide or pane width changes (rendering eight
/// slides every frame made the pane cost ~25 ms a frame).
#[derive(Clone, Default)]
struct IdeaThumbs {
    key: (u64, u64, usize, u32),
    tex: Vec<(String, egui::TextureHandle, u32, u32)>,
}

fn designer(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new("Pick a design for this slide. Ideas are generated on your device.").color(t.text_dim));
    ui.add_space(6.0);
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let idx = st.selection.slide;
    let sel = st.selection.clone();
    let key_doc = (st.uid, st.revision);
    let w = ui.available_width() - 8.0;
    let ppp = ui.ctx().pixels_per_point();
    let px = (w * ppp).round().max(16.0) as u32;
    let ideas = deckcraft_engine::cmd::ideas::ideas_for(&doc, idx);
    let themes = deckcraft_model::theme::builtin_themes();
    let cache_id = egui::Id::new("design-idea-thumbs");
    let key = (key_doc.0, key_doc.1, idx, px);
    let mut cache: IdeaThumbs = ui.ctx().data_mut(|d| d.get_temp(cache_id)).unwrap_or_default();
    if cache.key != key || cache.tex.is_empty() {
        cache = IdeaThumbs { key, tex: vec![] };
        let mut render = |name: String, d: &deckcraft_model::Presentation| {
            let (px, h) = crate::textures::fit_size(
                (px, (px as f64 * d.slide_size.height / d.slide_size.width.max(1.0)).clamp(1.0, 1e9) as u32),
                crate::textures::max_side(ui.ctx()),
            );
            let img = deckcraft_render::render_slide(
                d,
                idx,
                &deckcraft_render::RenderOpts { scale: px as f64 / d.slide_size.width.max(1.0), size: Some((px, h)), ..Default::default() },
            );
            let tex = crate::textures::upload_editor(ui.ctx(), format!("idea-{name}"), crate::textures::to_color_image(&img, false));
            cache.tex.push((name, tex, px, h));
        };
        for (id, _) in &ideas {
            let mut d = (*doc).clone();
            if deckcraft_engine::cmd::ideas::apply_idea(&mut d, &sel, idx, id).is_ok() {
                render(format!("layout:{id}"), &d);
            }
        }
        for th in &themes {
            let mut d = (*doc).clone();
            if let Some(m) = d.masters.first_mut() {
                std::sync::Arc::make_mut(m).theme = th.clone();
            }
            render(format!("theme:{}", th.name), &d);
        }
        ui.ctx().data_mut(|d| d.insert_temp(cache_id, cache.clone()));
    }
    let thumb = |ui: &mut Ui, name: &str, tip: &str| -> bool {
        let Some((_, tex, tw, th)) = cache.tex.iter().find(|e| e.0 == name) else { return false };
        let (r, resp) = ui.allocate_exact_size(vec2(w, w * *th as f32 / (*tw).max(1) as f32), Sense::click());
        // On the physical pixel grid, one texel per device pixel (#184).
        let at = crate::textures::device_pixel_rect(r, ppp);
        let at = Rect::from_min_size(at.min, vec2(*tw as f32 / ppp, *th as f32 / ppp));
        ui.painter().image(tex.id(), at, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        ui.painter().rect_stroke(
            at,
            CornerRadius::same(2),
            Stroke::new(if resp.hovered() { 2.5 } else { 1.0 }, if resp.hovered() { t.accent } else { t.border }),
            egui::StrokeKind::Outside,
        );
        let clicked = resp.on_hover_text(tip).clicked();
        ui.add_space(8.0);
        clicked
    };
    if ideas.is_empty() {
        ui.label(egui::RichText::new("Add a title with a picture or some text to get layout ideas.").color(t.text_dim));
    }
    for (id, label) in &ideas {
        if thumb(ui, &format!("layout:{id}"), label) {
            run(app, "design.applyIdea", json!({"id": id}));
        }
    }
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Themes (whole presentation)").color(t.text_dim));
    ui.add_space(4.0);
    for th in &themes {
        if thumb(ui, &format!("theme:{}", th.name), &th.name) {
            run(app, "design.theme", json!({"name": th.name}));
        }
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
        ui.label("Transparency");
        let id = ui.id().with(("text-transparency", sh.id));
        let stored = (text_transparency_of(sh) * 100.0) as f32;
        let mut v: f32 = ui.data_mut(|d| d.get_temp(id).unwrap_or(stored));
        let r = ui.add(egui::Slider::new(&mut v, 0.0..=100.0).suffix("%"));
        if r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged()) {
            let _ = app.run("format.textTransparency", json!({"value": v as f64 / 100.0}));
            ui.data_mut(|d| d.remove::<f32>(id));
        } else if r.dragged() {
            ui.data_mut(|d| d.insert_temp(id, v));
        }
    });
}

#[cfg(test)]
#[path = "smartart_tests.rs"]
mod smartart_tests;

#[cfg(test)]
#[path = "background_tests.rs"]
mod background_tests;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use deckcraft_color::{ColorTransform, SchemeSlot};
    use deckcraft_engine::Session;
    use deckcraft_model::{ColorRef, Fill, ShapeId};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use serde_json::json;

    use super::{fill_transparency_to_apply, solid_fill_transparency_percent};
    use crate::{Services, SlideApp};

    #[test]
    fn solid_fill_transparency_reads_the_alpha_shape_fill_writes() {
        assert_eq!(solid_fill_transparency_percent(None), 0.0);
        assert_eq!(solid_fill_transparency_percent(Some(&Fill::None)), 0.0);
        assert_eq!(solid_fill_transparency_percent(Some(&Fill::Group)), 0.0);
        let plain = Fill::solid(ColorRef::scheme(SchemeSlot::Accent1));
        assert_eq!(solid_fill_transparency_percent(Some(&plain)), 0.0);
        // `shape.fill` transparency 0.4 stores Alpha((1 - 0.4) * 100_000).
        let forty = Fill::solid(ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::Alpha(60_000)));
        let p = solid_fill_transparency_percent(Some(&forty));
        assert!((p - 40.0).abs() < 0.01, "{p}");
        let clear = Fill::solid(ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::Alpha(0)));
        assert!((solid_fill_transparency_percent(Some(&clear)) - 100.0).abs() < 0.01);
        // A later alpha wins, matching ColorRef::alpha.
        let layered = Fill::solid(ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::Alpha(0)).with(ColorTransform::Alpha(60_000)));
        assert!((solid_fill_transparency_percent(Some(&layered)) - 40.0).abs() < 0.01);

        assert_eq!(fill_transparency_to_apply(40.0, 40.0), None);
        assert_eq!(fill_transparency_to_apply(40.005, 40.0), None);
        assert_eq!(fill_transparency_to_apply(f32::NAN, 40.0), None);
        assert_eq!(fill_transparency_to_apply(55.0, 40.0), Some(0.55));
        assert_eq!(fill_transparency_to_apply(0.0, 40.0), Some(0.0));
    }

    fn solid_alpha(app: &SlideApp, id: u32) -> Option<i32> {
        let sh = app.session.active()?.shape(ShapeId(id))?;
        let Fill::Solid { color } = sh.fill.as_ref()? else { return None };
        color.mods.iter().find_map(|m| match m {
            ColorTransform::Alpha(v) => Some(*v),
            _ => None,
        })
    }

    fn slider_values(h: &Harness<'_, SlideApp>) -> Vec<f64> {
        h.query_all_by_role(egui::accesskit::Role::Slider).filter_map(|n| n.accesskit_node().numeric_value()).collect()
    }

    /// The Fill slider shows the selected solid's transparency, and opening the pane
    /// (or looking at another shape first) does not write 0% over it.
    #[test]
    fn fill_transparency_slider_follows_the_selection() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 40.0, 120.0, 80.0]})).unwrap()["id"].as_u64().unwrap() as u32;
        let b = s.execute("shape.insert", &json!({"preset": "rect", "rect": [200.0, 40.0, 120.0, 80.0]})).unwrap()["id"].as_u64().unwrap() as u32;
        s.execute("shape.fill", &json!({"id": a, "color": "accent1", "transparency": 0.0})).unwrap();
        s.execute("shape.fill", &json!({"id": b, "color": "accent1", "transparency": 0.4})).unwrap();
        s.execute("edit.select", &json!({"ids": [a]})).unwrap();
        let mut app = SlideApp::new(s, Services::default());
        app.ui.pane = Some("format".into());
        app.ui.format_tab = "fill".into();
        let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(3);
        let opaque: Vec<f64> = slider_values(&h);
        assert!(opaque.iter().any(|v| (*v - 0.0).abs() < 0.5), "opaque shape should show 0%, got {opaque:?}");
        assert!(opaque.iter().all(|v| (*v - 40.0).abs() > 0.5), "opaque shape should not show 40%, got {opaque:?}");

        h.state_mut().session.execute("edit.select", &json!({"ids": [b]})).unwrap();
        h.run_steps(3);
        let clear: Vec<f64> = slider_values(&h);
        assert!(clear.iter().any(|v| (*v - 40.0).abs() < 0.5), "40% fill should show 40%, got {clear:?}");
        assert_eq!(solid_alpha(h.state(), b), Some(60_000));

        h.get_by_label("Transparency").click();
        h.run_steps(2);
        assert_eq!(solid_alpha(h.state(), b), Some(60_000));
    }
}

/// Review › Thesaurus: look a word up; clicking a synonym puts it in place of the selected word
/// when editing text, otherwise looks that synonym up in turn.
fn thesaurus(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let mut q = app.thesaurus_query.clone();
    let edit = ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search for a word").desired_width(f32::INFINITY));
    if edit.changed() {
        app.thesaurus_query = q.clone();
    }
    ui.add_space(6.0);
    let senses = deckcraft_engine::thesaurus::lookup(&q);
    if q.trim().is_empty() {
        ui.label(egui::RichText::new("Select a word on the slide, or type one above.").color(t.text_dim));
        return;
    }
    if senses.is_empty() {
        ui.label(egui::RichText::new(format!("No synonyms for \"{}\".", q.trim())).color(t.text_dim));
        return;
    }
    let editing = app.session.active().is_some_and(|d| d.selection.text.is_some());
    for sense in senses {
        ui.label(egui::RichText::new(&sense.word).font(theme::bold(13.0)));
        for syn in sense.synonyms {
            let r = ui.add(egui::Button::new(&syn).frame(false)).on_hover_text(if editing { "Insert" } else { "Look up" });
            if r.clicked() {
                if editing {
                    run(app, "review.replaceWord", json!({"with": syn}));
                }
                app.thesaurus_query = syn;
            }
        }
        ui.add_space(6.0);
    }
}

#[cfg(test)]
#[path = "thesaurus_tests.rs"]
mod thesaurus_tests;
