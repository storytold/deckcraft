use super::{
    Align2, AnimClass, CornerRadius, Icon, Rect, Sense, SlideApp, Tokens, Ui, big, big_button, big_button_ex, dropdown, enabled, group, icons, json,
    pos2, rows, run, theme, vec2, widgets,
};

// ---------- Animations ----------

/// The effect the ribbon shows and changes: the Animation Pane row in `row`; else the chosen
/// bullet's own (`DocState::animated_paragraphs`), or the whole-box effect that plays it; else the
/// first selected shape's first effect.
fn current_effect(d: &deckcraft_engine::DocState, row: Option<usize>) -> Option<deckcraft_model::Animation> {
    let s = d.current_slide()?;
    if let Some(i) = row {
        return s.animations.get(i).cloned();
    }
    let id = d.selection.shapes.first()?;
    let mut mine = s.animations.iter().filter(|a| a.shape == *id);
    let Some((_, range)) = d.animated_paragraphs().filter(|x| x.0 == *id) else { return mine.next().cloned() };
    let own = mine.clone().find(|a| a.paragraph.is_some_and(|i| range.contains(&(i as usize))));
    own.or_else(|| mine.find(|a| a.paragraph.is_none())).cloned()
}

/// `p` aimed at the Animation Pane row in `row`, if there is one.
fn at_row(mut p: serde_json::Value, row: Option<usize>) -> serde_json::Value {
    if let (Some(i), Some(map)) = (row, p.as_object_mut()) {
        map.insert("index".into(), i.into());
    }
    p
}

/// Render animation preview, gallery, add/effect options, trigger, timing, and playback controls.
pub(super) fn animations(app: &mut SlideApp, ui: &mut Ui) {
    let has_sel = app.session.active().is_some_and(|d| !d.selection.shapes.is_empty());
    let row = app.anim_row_index();
    let cur = app.session.active().and_then(|d| current_effect(d, row));
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
            run(app, "animation.set", at_row(json!({"effect": a.0, "class": a.2.xml()}), row));
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
                            run(app, "animation.set", at_row(json!({"effect": a.0, "class": a.2.xml()}), row));
                            ui.close();
                        }
                    }
                });
            }
        });
        let effect_rows = cur.as_ref().map(|c| animation_effect_rows(&c.effect, c.class, Some(c.option.as_str()))).unwrap_or_default();
        let effect = cur.as_ref().map(|c| c.effect.as_str()).unwrap_or_default();
        let build = cur.as_ref().map(|c| text_build_id(c.text_build));
        // How a whole box plays: a single bullet's effect has no such choice.
        let whole = cur.as_ref().is_some_and(|c| c.paragraph.is_none());
        let build_rows = if whole { marked_options(&["asOne", "byParagraph"], build) } else { vec![] };
        // By Paragraph applies to any text, even an effect with no options of its own.
        let has_text = app.session.active().zip(cur.as_ref()).and_then(|(d, c)| d.shape(c.shape)).is_some_and(|sh| whole && sh.text.is_some());
        let r = big_button(ui, Icon::EffectOptions, "Effect\nOptions", !effect_rows.is_empty() || has_text);
        egui::Popup::menu(&r).show(|ui| {
            for &(o, on) in &effect_rows {
                if ui.selectable_label(on, animation_option_label(effect, o)).clicked() {
                    run(app, "animation.options", at_row(json!({"option": o}), row));
                    ui.close();
                }
            }
            if !effect_rows.is_empty() && !build_rows.is_empty() {
                ui.separator();
            }
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
        // Every effect in one column is taller than a 1440p screen, so cap the height and scroll (#1370).
        // The menu opens under the title bar and the ribbon; keep that chrome out of the cap.
        let max_h = (r.ctx.viewport_rect().height() - 160.0).max(120.0);
        egui::Popup::menu(&r).width(260.0).show(|ui| {
            egui::ScrollArea::vertical().id_salt("add_animation").max_height(max_h).show(ui, |ui| {
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
                        let mut p = json!({"effect": a.0, "class": a.2.xml()});
                        // A second effect on the row's bullet; on a whole-box row, on the box.
                        if let Some(c) = cur.as_ref().filter(|c| row.is_some() && c.paragraph.is_some()) {
                            p["ids"] = json!([c.shape.0]);
                            p["paragraphs"] = json!([c.paragraph]);
                        }
                        run(app, "animation.add", p);
                    }
                }
            });
        });
        big(app, ui, Icon::AnimationPane, "Animation\nPane", "view.pane", json!({"pane": "animation"}));
        let r = big_button(ui, Icon::Trigger, "Trigger", cur.is_some());
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("On Click of… (none)").clicked() {
                run(app, "animation.timing", at_row(json!({"trigger": null}), row));
            }
            let shapes: Vec<(u32, String)> =
                app.session.active().map(|d| d.shapes().iter().map(|s| (s.id.0, s.name.clone())).collect()).unwrap_or_default();
            for (id, name) in shapes {
                if ui.button(format!("On Click of {name}")).clicked() {
                    run(app, "animation.timing", at_row(json!({"trigger": id}), row));
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
                dropdown(ui, "Start", s.unwrap_or(""), 112.0, cur.is_some(), |ui| {
                    for (l, v) in [("On Click", "onClick"), ("With Previous", "withPrevious"), ("After Previous", "afterPrevious")] {
                        if ui.button(l).clicked() {
                            run(app, "animation.timing", at_row(json!({"start": v}), row));
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
                    run(app, "animation.timing", at_row(json!({"duration": (v.max(0.0) * 1000.0).round() as u64}), row));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Delay:");
                if let Some(v) =
                    widgets::spinner(ui, "adelay", "Delay", cur.as_ref().map(|c| c.delay_ms as f64 / 1000.0).unwrap_or(0.0), 0.25, "", 46.0)
                {
                    run(app, "animation.timing", at_row(json!({"delay": (v.max(0.0) * 1000.0).round() as u64}), row));
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

fn animation_option_label(effect: &str, option: &str) -> String {
    let direction = match option {
        "b" => Some("From Bottom"),
        "lb" => Some("From Bottom-Left"),
        "l" => Some("From Left"),
        "lt" => Some("From Top-Left"),
        "t" => Some("From Top"),
        "rt" => Some("From Top-Right"),
        "r" => Some("From Right"),
        "rb" => Some("From Bottom-Right"),
        _ => None,
    };
    if matches!(effect, "fly" | "flyOut" | "wipe" | "wipeOut" | "peek" | "peekOut" | "crawl" | "crawlOut")
        && let Some(label) = direction
    {
        if matches!(effect, "flyOut" | "peekOut" | "crawlOut") {
            return label.replacen("From", "To", 1);
        }
        return label.into();
    }

    match (effect, option) {
        ("float" | "floatOut", "u") => "Up".into(),
        ("float" | "floatOut", "d") => "Down".into(),
        ("split" | "splitOut", "horzIn") => "Horizontal In".into(),
        ("split" | "splitOut", "horzOut") => "Horizontal Out".into(),
        ("split" | "splitOut", "vertIn") => "Vertical In".into(),
        ("split" | "splitOut", "vertOut") => "Vertical Out".into(),
        ("shape" | "shapeOut", o) if o.ends_with("In") => format!("{} In", title_first(shape_option_name(o))),
        ("shape" | "shapeOut", o) if o.ends_with("Out") => format!("{} Out", title_first(shape_option_name(o))),
        ("wheel" | "wheelOut", "1") => "1 Spoke".into(),
        ("wheel" | "wheelOut", n) => format!("{n} Spokes"),
        ("box" | "boxOut" | "circle" | "diamond" | "diamondOut", "in") => format!("{} In", title_first(effect.trim_end_matches("Out"))),
        ("box" | "boxOut" | "circle" | "diamond" | "diamondOut", "out") => format!("{} Out", title_first(effect.trim_end_matches("Out"))),
        ("zoom" | "zoomOut", "in") => "Zoom In".into(),
        ("zoom" | "zoomOut", "out") => "Zoom Out".into(),
        ("strips" | "stripsOut", "lu") => "Left Up".into(),
        ("strips" | "stripsOut", "ru") => "Right Up".into(),
        ("strips" | "stripsOut", "ld") => "Left Down".into(),
        ("strips" | "stripsOut", "rd") => "Right Down".into(),
        (_, "horz") => "Horizontal".into(),
        (_, "vert") => "Vertical".into(),
        (_, "across") => "Across".into(),
        (_, "down") => "Down".into(),
        (_, "up") => "Up".into(),
        (_, "right") => "Right".into(),
        (_, "left") => "Left".into(),
        (_, "clockwise") => "Clockwise".into(),
        (_, "counterClockwise") => "Counterclockwise".into(),
        (_, "in") => "In".into(),
        (_, "out") => "Out".into(),
        (_, other) => super::transitions::option_label(other),
    }
}

fn shape_option_name(option: &str) -> &str {
    option.strip_suffix("In").or_else(|| option.strip_suffix("Out")).unwrap_or(option)
}

fn title_first(value: &str) -> String {
    let mut chars = value.chars();
    chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

pub(super) fn text_build_id(build: deckcraft_model::anim::TextBuild) -> &'static str {
    match build {
        deckcraft_model::anim::TextBuild::ByParagraph => "byParagraph",
        deckcraft_model::anim::TextBuild::ByWord => "byWord",
        deckcraft_model::anim::TextBuild::ByLetter => "byLetter",
        deckcraft_model::anim::TextBuild::AsOne => "asOne",
    }
}

#[cfg(test)]
mod animation_option_label_tests {
    use super::{animation_option_label, current_effect};
    use deckcraft_model::anim::ANIMATIONS;

    #[test]
    fn labels_every_animation_gallery_option_without_changing_its_code() {
        for &(effect, _, _, _, _, options) in ANIMATIONS {
            for &option in options {
                let label = animation_option_label(effect, option);
                assert!(!label.is_empty(), "{effect}/{option}");
                assert_ne!(label, option, "raw option shown for {effect}/{option}");
            }
        }
        assert_eq!(animation_option_label("fly", "lb"), "From Bottom-Left");
        assert_eq!(animation_option_label("flyOut", "rt"), "To Top-Right");
        assert_eq!(animation_option_label("flyOut", "b"), "To Bottom");
        assert_eq!(animation_option_label("peekOut", "l"), "To Left");
        assert_eq!(animation_option_label("wipe", "b"), "From Bottom");
        assert_eq!(animation_option_label("wheel", "1"), "1 Spoke");
        assert_eq!(animation_option_label("split", "vertOut"), "Vertical Out");
        assert_eq!(animation_option_label("shape", "circleIn"), "Circle In");
        assert_eq!(animation_option_label("diamond", "out"), "Diamond Out");
        assert_eq!(animation_option_label("diamondOut", "in"), "Diamond In");
        assert_eq!(animation_option_label("diamondOut", "out"), "Diamond Out");
        assert_eq!(animation_option_label("strips", "lu"), "Left Up");
        assert_eq!(animation_option_label("strips", "ru"), "Right Up");
        assert_eq!(animation_option_label("strips", "ld"), "Left Down");
        assert_eq!(animation_option_label("strips", "rd"), "Right Down");
        assert_eq!(animation_option_label("crawl", "b"), "From Bottom");
        assert_eq!(animation_option_label("crawl", "l"), "From Left");
        assert_eq!(animation_option_label("crawl", "t"), "From Top");
        assert_eq!(animation_option_label("crawl", "r"), "From Right");
        assert_eq!(animation_option_label("crawlOut", "l"), "To Left");
        assert_eq!(animation_option_label("crawlOut", "r"), "To Right");
        assert_eq!(animation_option_label("plus", "in"), "In");
        assert_eq!(animation_option_label("plus", "out"), "Out");
        assert_eq!(animation_option_label("changeFontSize", "increase"), "Increase");
        assert_eq!(animation_option_label("changeFontSize", "decrease"), "Decrease");
    }

    /// The ribbon's Duration and Delay send absolute values, so they must show the effect they
    /// will change: the highlighted bullet's, not the box's first.
    #[test]
    fn the_ribbon_shows_the_highlighted_bullets_effect() {
        let mut s = deckcraft_engine::Session::with_new();
        let tb =
            s.execute("insert.textBox", &serde_json::json!({"rect": [50, 50, 400, 200], "text": "One\nTwo\nThree"})).unwrap()["id"].as_u64().unwrap();
        s.execute("text.edit", &serde_json::json!({"id": tb})).unwrap();
        s.execute("animation.add", &serde_json::json!({"effect": "fade", "class": "entrance", "ids": [tb], "paragraphs": [0, 1], "duration": 500}))
            .unwrap();
        s.execute("animation.timing", &serde_json::json!({"index": 1, "duration": 2000})).unwrap();
        let shown = |s: &deckcraft_engine::Session| current_effect(s.active().unwrap(), None).map(|a| (a.paragraph, a.duration_ms));
        s.execute("text.select", &serde_json::json!({"anchor": [0, 1], "caret": [0, 1]})).unwrap();
        assert_eq!(shown(&s), Some((Some(0), 500)), "a caret shows its own bullet's effect");
        s.execute("text.select", &serde_json::json!({"anchor": [1, 0], "caret": [1, 3]})).unwrap();
        assert_eq!(shown(&s), Some((Some(1), 2000)));
        s.execute("text.select", &serde_json::json!({"anchor": [2, 0], "caret": [2, 5]})).unwrap();
        assert_eq!(shown(&s), None, "this bullet has no effect to show");
    }
}
