use super::animations::marked_options;
use super::{
    Align2, Color32, CornerRadius, Icon, Rect, Sense, SlideApp, Stroke, Tokens, Ui, Value, big, big_button, group, json, pos2, rows, run, theme,
    vec2, widgets,
};

// ---------- Transitions ----------

/// Render transition preview, gallery, options, sound, and timing controls for the current
/// slide.
pub(super) fn transitions(app: &mut SlideApp, ui: &mut Ui) {
    let cur = app.session.active().and_then(|d| d.current_slide().and_then(|s| s.transition.clone()));
    let kind = cur.as_ref().map(|t| t.kind.clone()).unwrap_or_else(|| "none".into());
    group(ui, |ui| {
        if big_button(ui, Icon::Preview, "Preview", true).clicked() {
            let from = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            app.start_show(from, true);
            if let Some(s) = app.show.as_mut() {
                s.preview_only = true;
            }
        }
    });
    group(ui, |ui| {
        let list = deckcraft_model::anim::TRANSITIONS;
        let shown: Vec<_> = list.iter().take(12).collect();
        let sel = shown.iter().position(|x| x.0 == kind);
        if let Some(i) = widgets::gallery(
            ui,
            shown.len(),
            vec2(54.0, 56.0),
            sel,
            |p, r, i| paint_transition_tile(p, r, shown.get(i).map(|x| (x.0, x.1)).unwrap_or(("", ""))),
            |i| shown.get(i).map(|x| x.1.to_string()).unwrap_or_default(),
        ) && let Some(x) = shown.get(i)
        {
            run(app, "transition.set", json!({"kind": x.0}));
        }
        let more = widgets::drop_button(ui, "", vec2(14.0, 52.0), true);
        egui::Popup::menu(&more).width(480.0).show(|ui| {
            for cat in ["Subtle", "Exciting", "Dynamic Content"] {
                ui.label(egui::RichText::new(crate::tl!(cat)).font(theme::bold(12.0)));
                ui.horizontal_wrapped(|ui| {
                    for x in list.iter().filter(|x| x.2 == cat) {
                        let (r, resp) = ui.allocate_exact_size(vec2(56.0, 58.0), Sense::click());
                        if resp.hovered() || x.0 == kind {
                            ui.painter().rect_filled(r, CornerRadius::same(4), Tokens::get(ui.ctx()).hover);
                        }
                        paint_transition_tile(ui.painter(), r.shrink(3.0), (x.0, x.1));
                        if resp.clicked() {
                            run(app, "transition.set", json!({"kind": x.0}));
                            ui.close();
                        }
                    }
                });
            }
        });
    });
    group(ui, |ui| {
        let selected = cur.as_ref().map(|t| t.option.as_str());
        let rows = transition_effect_rows(&kind, selected);
        let r = big_button(ui, Icon::EffectOptions, "Effect\nOptions", !rows.is_empty());
        egui::Popup::menu(&r).show(|ui| {
            for &(o, on) in &rows {
                if ui.selectable_label(on, deckcraft_model::anim::transition_option_label(&kind, o)).clicked() {
                    run(app, "transition.options", json!({"option": o}));
                    ui.close();
                }
            }
        });
    });
    group(ui, |ui| {
        // Sound in its own column, so the timing column keeps its three rows.
        rows(ui, |ui| {
            ui.horizontal(|ui| {
                let field = ui.label(crate::tl!("Sound:"));
                let stop = cur.as_ref().is_some_and(|t| t.stop_sound);
                let name = cur
                    .as_ref()
                    .and_then(|t| t.sound)
                    .and_then(|m| app.session.active().and_then(|d| d.doc.media(m).map(|i| i.name.trim_end_matches(".wav").to_string())))
                    .unwrap_or_else(|| if stop { crate::tl!("[Stop Previous Sound]").into() } else { crate::tl!("[No Sound]").into() });
                let combo = craft_ui::ComboBox::from_id_salt("transition_sound").width(140.0).selected_text(name).show_ui(ui, |ui| {
                    let none = cur.as_ref().is_none_or(|t| t.sound.is_none() && !t.stop_sound);
                    if ui.selectable_label(none, crate::tl!("[No Sound]")).clicked() {
                        run(app, "transition.sound", json!({"none": true}));
                    }
                    if ui.selectable_label(stop, crate::tl!("[Stop Previous Sound]")).clicked() {
                        run(app, "transition.sound", json!({"stopPrevious": true}));
                    }
                    ui.separator();
                    for (id, label) in deckcraft_engine::sounds::BUILTIN {
                        if ui.button(label).clicked() {
                            let lp = cur.as_ref().is_some_and(|t| t.sound_loop);
                            run(app, "transition.sound", json!({"builtin": id, "loop": lp}));
                        }
                    }
                    ui.separator();
                    let can_pick = app.services.pick_open.is_some();
                    let r = ui.add_enabled(can_pick, egui::Button::new(crate::tl!("Other Sound…")));
                    if r.clicked() {
                        app.pick_transition_sound();
                    }
                    r.on_disabled_hover_text("Choosing a sound file needs the desktop app");
                    ui.separator();
                    let has = cur.as_ref().is_some_and(|t| t.sound.is_some());
                    let mut lp = cur.as_ref().is_some_and(|t| t.sound_loop);
                    if ui.add_enabled(has, egui::Checkbox::new(&mut lp, crate::tl!("Loop Until Next Sound"))).changed() {
                        run(app, "transition.sound", json!({"loop": lp}));
                    }
                });
                let _ = combo.response.labelled_by(field.id);
            });
        });
        rows(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::tl!("Duration:"));
                let d = cur.as_ref().map(|t| t.duration_ms as f64 / 1000.0).unwrap_or(1.0);
                if let Some(v) = widgets::spinner(ui, "tdur", "Duration", d, 0.25, "", 46.0) {
                    run(app, "transition.timing", json!({"duration": (v.max(0.0) * 1000.0).round() as u64}));
                }
            });
            ui.horizontal(|ui| {
                let mut click = cur.as_ref().is_none_or(|t| t.advance_on_click);
                if ui.checkbox(&mut click, crate::tl!("On Mouse Click")).changed() {
                    run(app, "transition.timing", json!({"onClick": click}));
                }
            });
            ui.horizontal(|ui| {
                let after = cur.as_ref().and_then(|t| t.advance_after_ms);
                let mut on = after.is_some();
                if ui.checkbox(&mut on, crate::tl!("After:")).changed() {
                    run(app, "transition.timing", json!({"after": if on { json!(after.unwrap_or(0)) } else { Value::Null }}));
                }
                if let Some(v) = widgets::spinner(ui, "tafter", "After", after.unwrap_or(0) as f64 / 1000.0, 1.0, "", 46.0) {
                    run(app, "transition.timing", json!({"after": (v.max(0.0) * 1000.0).round() as u64}));
                }
            });
        });
        big(app, ui, Icon::ApplyToAll, "Apply\nTo All", "transition.applyAll", json!({}));
    });
}

pub(super) fn option_label(o: &str) -> String {
    match o {
        "l" => "From Left".into(),
        "r" => "From Right".into(),
        "u" => "From Top".into(),
        "d" => "From Bottom".into(),
        "horz" => "Horizontal".into(),
        "vert" => "Vertical".into(),
        "in" => "In".into(),
        "out" => "Out".into(),
        other => {
            let mut s = String::new();
            for (i, c) in other.chars().enumerate() {
                if i == 0 {
                    s.extend(c.to_uppercase());
                } else if c.is_uppercase() {
                    s.push(' ');
                    s.push(c);
                } else {
                    s.push(c);
                }
            }
            s
        }
    }
}

pub fn paint_transition_tile(p: &egui::Painter, r: Rect, (id, label): (&str, &str)) {
    let t = Tokens::get(p.ctx());
    let pic = Rect::from_min_size(r.min + vec2(4.0, 2.0), vec2(r.width() - 8.0, r.height() - 18.0));
    let blue = Color32::from_rgb(0x2E, 0x6F, 0xD8);
    let light = Color32::from_rgb(0xBF, 0xD4, 0xF5);
    p.rect_filled(pic, CornerRadius::same(2), light);
    match id {
        "none" => {
            p.rect_filled(pic, CornerRadius::same(2), Color32::from_rgb(0xE6, 0xEC, 0xF5));
        }
        "fade" | "dissolve" | "flash" => {
            p.rect_filled(pic, CornerRadius::same(2), blue.gamma_multiply(0.5));
        }
        "push" | "cover" | "uncover" | "pan" | "conveyor" => {
            p.rect_filled(Rect::from_min_max(pos2(pic.center().x, pic.min.y), pic.max), CornerRadius::same(2), blue);
            p.arrow(pos2(pic.center().x + 10.0, pic.center().y), vec2(-12.0, 0.0), Stroke::new(1.5, Color32::WHITE));
        }
        "wipe" | "reveal" => {
            p.rect_filled(Rect::from_min_max(pic.min, pos2(pic.center().x, pic.max.y)), CornerRadius::same(2), blue);
        }
        "split" | "doors" | "window" | "curtains" => {
            p.rect_filled(
                Rect::from_min_max(pos2(pic.min.x + pic.width() * 0.3, pic.min.y), pos2(pic.max.x - pic.width() * 0.3, pic.max.y)),
                CornerRadius::ZERO,
                blue,
            );
        }
        "randomBar" | "blinds" | "comb" | "shred" => {
            for k in 0..5 {
                let x = pic.min.x + k as f32 * pic.width() / 5.0;
                p.rect_filled(Rect::from_min_size(pos2(x, pic.min.y), vec2(pic.width() / 10.0, pic.height())), CornerRadius::ZERO, blue);
            }
        }
        "checker" | "honeycomb" | "glitter" | "fracture" => {
            for k in 0..12 {
                if k % 2 == (k / 4) % 2 {
                    let x = pic.min.x + (k % 4) as f32 * pic.width() / 4.0;
                    let y = pic.min.y + (k / 4) as f32 * pic.height() / 3.0;
                    p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(pic.width() / 4.0, pic.height() / 3.0)), CornerRadius::ZERO, blue);
                }
            }
        }
        "shape" | "zoom" | "ripple" | "vortex" | "clock" => {
            p.circle_filled(pic.center(), pic.height() * 0.38, blue);
        }
        "morph" => {
            p.circle_filled(pos2(pic.min.x + pic.width() * 0.3, pic.center().y), pic.height() * 0.25, blue);
            p.rect_filled(
                Rect::from_center_size(pos2(pic.max.x - pic.width() * 0.3, pic.center().y), vec2(pic.height() * 0.45, pic.height() * 0.45)),
                CornerRadius::same(2),
                blue.gamma_multiply(0.7),
            );
        }
        _ => {
            // Fake 3-D: a tilted card.
            let pts = vec![
                pos2(pic.min.x + 6.0, pic.min.y + 4.0),
                pos2(pic.max.x - 3.0, pic.min.y + 1.0),
                pos2(pic.max.x - 3.0, pic.max.y - 1.0),
                pos2(pic.min.x + 6.0, pic.max.y - 4.0),
            ];
            p.add(egui::Shape::convex_polygon(pts, blue, Stroke::NONE));
        }
    }
    p.rect_stroke(pic, CornerRadius::same(2), Stroke::new(1.0, Color32::from_rgb(0x9B, 0xB4, 0xDB)), egui::StrokeKind::Inside);
    p.text(pos2(r.center().x, r.max.y - 7.0), Align2::CENTER_CENTER, label, theme::font(10.0), t.text);
}

/// One row of Transitions ▸ Effect Options: the stored option and whether it is the current one.
pub(super) fn transition_effect_rows(kind: &str, current: Option<&str>) -> Vec<(&'static str, bool)> {
    let opts = deckcraft_model::anim::TRANSITIONS.iter().find(|x| x.0 == kind).map(|x| x.4).unwrap_or(&[]);
    marked_options(opts, current)
}
