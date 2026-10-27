use super::*;

// ---------- Design ----------

/// Render the Design tab's themes, variants, colours, fonts, effects, background, and slide
/// size controls.
pub(super) fn design(app: &mut SlideApp, ui: &mut Ui) {
    let themes = deckcraft_model::theme::builtin_themes();
    let cur = app.session.active().and_then(|d| d.doc.masters.first().map(|m| m.theme.name.clone())).unwrap_or_default();
    group(ui, |ui| {
        let sel = themes.iter().position(|th| th.name == cur);
        let names: Vec<String> = themes.iter().map(|t| t.name.clone()).collect();
        if let Some(i) = widgets::gallery(
            ui,
            themes.len(),
            vec2(84.0, 56.0),
            sel,
            |p, r, i| {
                if let Some(th) = themes.get(i) {
                    paint_theme_tile(p, r, th);
                }
            },
            |i| names.get(i).cloned().unwrap_or_default(),
        ) && let Some(n) = names.get(i)
        {
            run(app, "design.theme", json!({"name": n}));
        }
        if small_button(ui, Icon::Themes, "Browse for Themes…", enabled(app, "design.browseTheme"), false).clicked() {
            run(app, "app.browseThemeDialog", json!({}));
        }
    });
    // Variants: the current theme in its four colour variations.
    if let Some(th) = app.session.active().and_then(|d| d.doc.masters.first().map(|m| m.theme.clone())) {
        let variants = deckcraft_model::theme::theme_variants(&th);
        let tiles: Vec<deckcraft_model::Theme> = variants.iter().map(|c| deckcraft_model::Theme { colors: c.clone(), ..th.clone() }).collect();
        let names: Vec<String> = variants.iter().map(|c| c.name.clone()).collect();
        let sel = variants.iter().position(|c| c.colors == th.colors.colors);
        group(ui, |ui| {
            if let Some(i) = widgets::gallery(
                ui,
                tiles.len(),
                vec2(84.0, 56.0),
                sel,
                |p, r, i| {
                    if let Some(t) = tiles.get(i) {
                        paint_theme_tile(p, r, t);
                    }
                },
                |i| names.get(i).cloned().unwrap_or_default(),
            ) {
                run(app, "design.variant", json!({"index": i}));
            }
        });
    }
    group(ui, |ui| {
        let r = big_button(ui, Icon::Colors, "Colors", true);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Colors"));
        egui::Popup::menu(&r).width(240.0).show(|ui| {
            for cs in deckcraft_model::theme::builtin_color_schemes() {
                ui.horizontal(|ui| {
                    let (rect, resp) = ui.allocate_exact_size(vec2(220.0, 20.0), Sense::click());
                    let p = ui.painter();
                    for (k, c) in cs.colors.iter().take(10).skip(2).enumerate() {
                        p.rect_filled(
                            Rect::from_min_size(rect.min + vec2(k as f32 * 12.0, 3.0), vec2(11.0, 14.0)),
                            CornerRadius::ZERO,
                            theme::to_color32(*c),
                        );
                    }
                    p.text(rect.min + vec2(104.0, 10.0), Align2::LEFT_CENTER, &cs.name, theme::font(12.0), Tokens::get(ui.ctx()).text);
                    if resp.clicked() {
                        run(app, "design.colors", json!({"name": cs.name}));
                        ui.close();
                    }
                });
            }
            ui.separator();
            if ui.button("Customize Colors…").clicked() {
                run(app, "design.colors", json!({}));
                ui.close();
            }
        });
        let r = big_button(ui, Icon::Fonts, "Fonts", true);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Fonts"));
        egui::Popup::menu(&r).width(220.0).show(|ui| {
            for fs in deckcraft_model::theme::builtin_font_schemes() {
                if ui.button(format!("{}\n   {} / {}", fs.name, fs.major.latin, fs.minor.latin)).clicked() {
                    run(app, "design.fonts", json!({"name": fs.name}));
                }
            }
            ui.separator();
            if ui.button("Customize Fonts…").clicked() {
                run(app, "design.fonts", json!({}));
                ui.close();
            }
        });
        let r = big_button(ui, Icon::ShapeEffects, "Effects", true);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Effects"));
        egui::Popup::menu(&r).width(180.0).show(|ui| {
            for fs in deckcraft_model::theme::builtin_format_schemes() {
                if ui.button(&fs.name).clicked() {
                    run(app, "design.effects", json!({"name": fs.name}));
                    ui.close();
                }
            }
        });
        let r = big_button(ui, Icon::BackgroundStyles, "Background\nStyles", true);
        egui::Popup::menu(&r).show(|ui| {
            egui::Grid::new("bgstyles").show(ui, |ui| {
                let sc = scheme(app);
                for i in 1..=12 {
                    let slots = [
                        deckcraft_color::SchemeSlot::Lt1,
                        deckcraft_color::SchemeSlot::Lt2,
                        deckcraft_color::SchemeSlot::Dk2,
                        deckcraft_color::SchemeSlot::Dk1,
                    ];
                    let base = theme::to_color32(sc.get(slots[(i - 1) % 4]));
                    let shade = match (i - 1) / 4 {
                        0 => base,
                        1 => base.lerp_to_gamma(Color32::GRAY, 0.15),
                        _ => base.lerp_to_gamma(Color32::BLACK, 0.12),
                    };
                    let (rect, resp) = ui.allocate_exact_size(vec2(48.0, 28.0), Sense::click());
                    ui.painter().rect(rect, CornerRadius::same(2), shade, Stroke::new(1.0, Tokens::get(ui.ctx()).border), egui::StrokeKind::Inside);
                    if resp.on_hover_text(format!("Style {i}")).clicked() {
                        run(app, "design.background", json!({"style": i, "all": true}));
                        ui.close();
                    }
                    if i % 4 == 0 {
                        ui.end_row();
                    }
                }
            });
            if ui.button("Reset Slide Background").clicked() {
                run(app, "design.background", json!({"reset": true}));
            }
        });
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::SlideSize, "Slide\nSize", true);
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("Standard (4:3)").clicked() {
                run(app, "design.slideSize", json!({"preset": "standard"}));
            }
            if ui.button("Widescreen (16:9)").clicked() {
                run(app, "design.slideSize", json!({"preset": "widescreen"}));
            }
            if ui.button("Page Setup…").clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("slideSize"));
            }
        });
        big(app, ui, Icon::FormatBackground, "Format\nBackground", "view.pane", json!({"pane": "background"}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::DesignIdeas, "Design\nIdeas", "view.pane", json!({"pane": "designer"}));
    });
}

pub fn paint_theme_tile(p: &egui::Painter, r: Rect, th: &deckcraft_model::Theme) {
    let bg = theme::to_color32(th.colors.get(deckcraft_color::SchemeSlot::Lt1));
    let fg = theme::to_color32(th.colors.get(deckcraft_color::SchemeSlot::Dk1));
    p.rect(r, CornerRadius::same(2), bg, Stroke::new(1.0, Color32::from_gray(200)), egui::StrokeKind::Inside);
    p.text(r.min + vec2(8.0, r.height() * 0.42), Align2::LEFT_CENTER, "Aa", theme::font(r.height() * 0.38), fg);
    let w = (r.width() - 16.0) / 6.0;
    for k in 0..6 {
        let c = theme::to_color32(th.colors.colors.get(4 + k).copied().unwrap_or(deckcraft_color::Rgba::BLACK));
        p.rect_filled(Rect::from_min_size(pos2(r.min.x + 8.0 + k as f32 * w, r.max.y - 11.0), vec2(w - 1.5, 5.0)), CornerRadius::ZERO, c);
    }
}
