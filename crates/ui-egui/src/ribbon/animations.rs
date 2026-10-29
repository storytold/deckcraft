use super::transitions::option_label;
use super::{
    Align2, AnimClass, CornerRadius, Icon, Rect, Sense, SlideApp, Tokens, Ui, big, big_button, big_button_ex, dropdown, enabled, group, icons, json,
    pos2, rows, run, theme, vec2, widgets,
};

// ---------- Animations ----------

/// Render animation preview, gallery, add/effect options, trigger, timing, and playback controls.
pub(super) fn animations(app: &mut SlideApp, ui: &mut Ui) {
    let has_sel = app.session.active().is_some_and(|d| !d.selection.shapes.is_empty());
    let cur: Option<deckcraft_model::Animation> = app.session.active().and_then(|d| {
        let s = d.current_slide()?;
        let id = d.selection.shapes.first()?;
        s.animations.iter().find(|a| a.shape == *id).cloned()
    });
    group(ui, |ui| {
        let playing = app.anim_preview_active();
        let (resp, _) = big_button_ex(ui, Icon::Preview, "Preview", true, false, playing);
        if resp.on_hover_text("Play this slide's animations on the canvas").clicked() {
            run(app, "animation.preview", json!({}));
        }
    });
    group(ui, |ui| {
        let quick: Vec<_> = deckcraft_model::anim::ANIMATIONS
            .iter()
            .filter(|a| {
                matches!(
                    a.0,
                    "appear"
                        | "fade"
                        | "fly"
                        | "float"
                        | "split"
                        | "wipe"
                        | "zoom"
                        | "pulse"
                        | "spin"
                        | "growShrink"
                        | "fadeOut"
                        | "flyOut"
                        | "lines"
                )
            })
            .collect();
        let sel = cur.as_ref().and_then(|c| quick.iter().position(|a| a.0 == c.effect));
        if let Some(i) = widgets::gallery(
            ui,
            quick.len(),
            vec2(50.0, 56.0),
            sel,
            |p, r, i| {
                if let Some(a) = quick.get(i) {
                    paint_anim_tile(p, r, a.1, a.2, has_sel);
                }
            },
            |i| quick.get(i).map(|a| a.1.to_string()).unwrap_or_default(),
        ) && let Some(a) = quick.get(i)
        {
            run(app, "animation.set", json!({"effect": a.0, "class": a.2.xml()}));
        }
        let more = widgets::drop_button(ui, "", vec2(14.0, 52.0), has_sel);
        egui::Popup::menu(&more).width(420.0).show(|ui| {
            for (cls, label) in
                [(AnimClass::Entrance, "Entrance"), (AnimClass::Emphasis, "Emphasis"), (AnimClass::Exit, "Exit"), (AnimClass::Path, "Motion Paths")]
            {
                ui.label(egui::RichText::new(label).font(theme::bold(12.0)));
                ui.horizontal_wrapped(|ui| {
                    for a in deckcraft_model::anim::ANIMATIONS.iter().filter(|a| a.2 == cls) {
                        let (r, resp) = ui.allocate_exact_size(vec2(56.0, 54.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, CornerRadius::same(4), Tokens::get(ui.ctx()).hover);
                        }
                        paint_anim_tile(ui.painter(), r.shrink(2.0), a.1, a.2, true);
                        if resp.clicked() {
                            run(app, "animation.set", json!({"effect": a.0, "class": a.2.xml()}));
                            ui.close();
                        }
                    }
                });
            }
        });
        let effect_rows = cur.as_ref().map(|c| animation_effect_rows(&c.effect, c.class, Some(c.option.as_str()))).unwrap_or_default();
        let build = cur.as_ref().map(|c| text_build_id(c.text_build));
        let build_rows = marked_options(&["asOne", "byParagraph"], build);
        let r = big_button(ui, Icon::EffectOptions, "Effect\nOptions", !effect_rows.is_empty());
        egui::Popup::menu(&r).show(|ui| {
            for &(o, on) in &effect_rows {
                if ui.selectable_label(on, option_label(o)).clicked() {
                    run(app, "animation.options", json!({"option": o}));
                    ui.close();
                }
            }
            ui.separator();
            for &(id, on) in &build_rows {
                let label = if id == "byParagraph" { "By Paragraph" } else { "As One Object" };
                if ui.selectable_label(on, label).clicked() {
                    run(app, "animation.options", json!({"textBuild": id}));
                    ui.close();
                }
            }
        });
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::AddAnimation, "Add\nAnimation", has_sel);
        egui::Popup::menu(&r).width(260.0).show(|ui| {
            for a in deckcraft_model::anim::ANIMATIONS.iter().filter(|a| a.2 != AnimClass::Media) {
                if ui
                    .button(format!(
                        "{} — {}",
                        a.1,
                        match a.2 {
                            AnimClass::Entrance => "Entrance",
                            AnimClass::Emphasis => "Emphasis",
                            AnimClass::Exit => "Exit",
                            _ => "Motion",
                        }
                    ))
                    .clicked()
                {
                    run(app, "animation.add", json!({"effect": a.0, "class": a.2.xml()}));
                }
            }
        });
        big(app, ui, Icon::AnimationPane, "Animation\nPane", "view.pane", json!({"pane": "animation"}));
        let r = big_button(ui, Icon::Trigger, "Trigger", cur.is_some());
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("On Click of… (none)").clicked() {
                run(app, "animation.timing", json!({"trigger": null}));
            }
            let shapes: Vec<(u32, String)> =
                app.session.active().map(|d| d.shapes().iter().map(|s| (s.id.0, s.name.clone())).collect()).unwrap_or_default();
            for (id, name) in shapes {
                if ui.button(format!("On Click of {name}")).clicked() {
                    run(app, "animation.timing", json!({"trigger": id}));
                }
            }
        });
        let on = app.session.anim_painter.is_some();
        let en = enabled(app, "animation.painter");
        let (r, _) = big_button_ex(ui, Icon::AnimationPainter, "Animation\nPainter", en, false, on);
        let r = r.on_hover_text("Animation Painter (double-click to keep it on)");
        if r.double_clicked() {
            run(app, "animation.painter", json!({"sticky": true}));
        } else if r.clicked() {
            if on {
                app.session.anim_painter = None;
            } else {
                run(app, "animation.painter", json!({}));
            }
        }
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Start:");
                let s = cur.as_ref().map(|c| match c.start {
                    deckcraft_model::AnimStart::OnClick => "On Click",
                    deckcraft_model::AnimStart::WithPrevious => "With Previous",
                    deckcraft_model::AnimStart::AfterPrevious => "After Previous",
                });
                dropdown(ui, "astart", s.unwrap_or(""), 112.0, cur.is_some(), |ui| {
                    for (l, v) in [("On Click", "onClick"), ("With Previous", "withPrevious"), ("After Previous", "afterPrevious")] {
                        if ui.button(l).clicked() {
                            run(app, "animation.timing", json!({"start": v}));
                            ui.close();
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Duration:");
                if let Some(v) =
                    widgets::spinner(ui, "adur", "Duration", cur.as_ref().map(|c| c.duration_ms as f64 / 1000.0).unwrap_or(0.5), 0.25, "", 46.0)
                {
                    run(app, "animation.timing", json!({"duration": (v.max(0.0) * 1000.0).round() as u64}));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Delay:");
                if let Some(v) =
                    widgets::spinner(ui, "adelay", "Delay", cur.as_ref().map(|c| c.delay_ms as f64 / 1000.0).unwrap_or(0.0), 0.25, "", 46.0)
                {
                    run(app, "animation.timing", json!({"delay": (v.max(0.0) * 1000.0).round() as u64}));
                }
            });
        });
    });
}

pub fn paint_anim_tile(p: &egui::Painter, r: Rect, label: &str, class: AnimClass, enabled: bool) {
    let t = Tokens::get(p.ctx());
    let icon = match class {
        AnimClass::Entrance => Icon::AnimEntrance,
        AnimClass::Emphasis => Icon::AnimEmphasis,
        AnimClass::Exit => Icon::AnimExit,
        _ => Icon::AnimPath,
    };
    icons::paint(p, Rect::from_center_size(pos2(r.center().x, r.min.y + 18.0), vec2(28.0, 28.0)), icon, t.text, !enabled);
    p.text(pos2(r.center().x, r.max.y - 8.0), Align2::CENTER_CENTER, label, theme::font(10.0), if enabled { t.text } else { t.text_faint });
}

/// The stored id is the one marked row. Every other id stays unmarked.
pub(super) fn marked_options<'a>(options: &'a [&'a str], current: Option<&str>) -> Vec<(&'a str, bool)> {
    options.iter().copied().map(|o| (o, current == Some(o))).collect()
}

/// One row of Animations ▸ Effect Options for the selected effect.
pub(super) fn animation_effect_rows(effect: &str, class: AnimClass, current: Option<&str>) -> Vec<(&'static str, bool)> {
    let opts = deckcraft_model::anim::animation_info(effect, class).map(|x| x.5).unwrap_or(&[]);
    marked_options(opts, current)
}

pub(super) fn text_build_id(build: deckcraft_model::anim::TextBuild) -> &'static str {
    match build {
        deckcraft_model::anim::TextBuild::ByParagraph => "byParagraph",
        deckcraft_model::anim::TextBuild::ByWord => "byWord",
        deckcraft_model::anim::TextBuild::ByLetter => "byLetter",
        deckcraft_model::anim::TextBuild::AsOne => "asOne",
    }
}
