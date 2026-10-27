use super::*;

fn seed_font_fields(d: &mut Dialog) {
    seed_once(d, "family", json_text(d.params.get("family"), ""));
    seed_once(d, "size", json_text(d.params.get("size"), "18"));
    seed_once(d, "bold", json_text(d.params.get("bold"), "0"));
    seed_once(d, "italic", json_text(d.params.get("italic"), "0"));
    seed_once(d, "underline", json_text(d.params.get("underline"), "none"));
    seed_once(d, "strike", json_text(d.params.get("strike"), "none"));
    seed_once(d, "baseline", json_text(d.params.get("baseline"), "none"));
    seed_once(d, "caps", json_text(d.params.get("caps"), "none"));
    seed_once(d, "spacing", json_text(d.params.get("spacing"), "0"));
    let shown = d.params.get("color").map(color_label).unwrap_or_default();
    seed_once(d, "color", shown.clone());
    seed_once(d, "colorSeed", shown);
}

/// Home › Font › Font…, the shape menu and Cmd+T. One OK applies every field together.
pub(super) fn font_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    ui.set_min_width(460.0);
    seed_font_fields(d);
    let mut fams = crate::ribbon::font_families(app);
    let family = d.get("family", "");
    if !family.is_empty() && !fams.iter().any(|f| f == &family) {
        fams.insert(0, family.clone());
    }
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(88.0, 20.0), egui::Label::new("Font"));
        let combo = egui::ComboBox::from_id_salt("font_dialog_family")
            .selected_text(if family.is_empty() { " " } else { family.as_str() })
            .width(300.0)
            .show_ui(ui, |ui| {
                egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                    for f in &fams {
                        if ui.selectable_label(family == *f, f).clicked() {
                            d.fields.insert("family".into(), f.clone());
                        }
                    }
                });
            });
        let _ = combo.response.labelled_by(field.id);
    });
    ui.horizontal(|ui| {
        let _ = labeled_text(ui, d, "Size", "size", 64.0);
        ui.add_space(16.0);
        check(ui, d, "Bold", "bold", false);
        check(ui, d, "Italic", "italic", false);
    });
    ui.horizontal(|ui| {
        ui.add_sized(vec2(88.0, 20.0), egui::Label::new("Underline"));
        choice(
            ui,
            d,
            "font_dialog_underline",
            "Underline",
            "underline",
            &[("none", "None"), ("sng", "Single"), ("dbl", "Double"), ("heavy", "Heavy"), ("dotted", "Dotted"), ("dash", "Dash"), ("wavy", "Wavy")],
        );
        ui.add_space(8.0);
        let _ = labeled_text(ui, d, "Color", "color", 110.0);
    });
    ui.horizontal(|ui| {
        ui.add_sized(vec2(88.0, 20.0), egui::Label::new("Strikethrough"));
        choice(ui, d, "font_dialog_strike", "Strikethrough", "strike", &[("none", "None"), ("sng", "Single"), ("dbl", "Double")]);
    });
    ui.horizontal(|ui| {
        ui.add_sized(vec2(88.0, 20.0), egui::Label::new("Baseline"));
        choice(ui, d, "font_dialog_baseline", "Baseline", "baseline", &[("none", "None"), ("super", "Superscript"), ("sub", "Subscript")]);
        ui.add_space(8.0);
        ui.label("Caps");
        choice(ui, d, "font_dialog_caps", "Caps", "caps", &[("none", "None"), ("small", "Small caps"), ("all", "All caps")]);
    });
    ui.horizontal(|ui| {
        ui.add_sized(vec2(88.0, 20.0), egui::Label::new("Spacing"));
        let mut spacing = d.get("spacing", "0");
        let edited = ui.add(egui::TextEdit::singleline(&mut spacing).desired_width(64.0).hint_text("pt"));
        let mut picked = None;
        let combo = egui::ComboBox::from_id_salt("font_dialog_spacing")
            .selected_text(match spacing.as_str() {
                "-3" => "Very tight",
                "-1.5" => "Tight",
                "0" => "Normal",
                "3" => "Loose",
                "6" => "Very loose",
                other => other,
            })
            .width(120.0)
            .show_ui(ui, |ui| {
                for (v, l) in [("-3", "Very tight"), ("-1.5", "Tight"), ("0", "Normal"), ("3", "Loose"), ("6", "Very loose")] {
                    if ui.selectable_label(spacing == v, l).clicked() {
                        picked = Some(v.to_string());
                    }
                }
            });
        combo.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Spacing"));
        let _ = edited;
        if let Some(v) = picked {
            d.fields.insert("spacing".into(), v);
        } else {
            d.fields.insert("spacing".into(), spacing);
        }
    });
    let bold = d.get("bold", "0") == "1";
    let italic = d.get("italic", "0") == "1";
    let underline = d.get("underline", "none");
    let strike = d.get("strike", "none");
    let shown = finite_pt(&d.get("size", "18")).unwrap_or(18.0).clamp(12.0, 28.0) as f32;
    let mut sample = egui::RichText::new("AaBbCcDd").size(shown);
    if bold {
        sample = sample.strong();
    }
    if italic {
        sample = sample.italics();
    }
    if underline != "none" {
        sample = sample.underline();
    }
    if strike != "none" {
        sample = sample.strikethrough();
    }
    ui.add_space(6.0);
    ui.label(sample);
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        let family = d.get("family", "");
        let family = family.trim();
        if family.is_empty() {
            app.set_status("Choose a font");
            return false;
        }
        let Some(size) = finite_pt(&d.get("size", "")) else {
            app.set_status("Font size must be a number");
            return false;
        };
        let Some(spacing) = finite_pt(&d.get("spacing", "0")) else {
            app.set_status("Character spacing must be a number");
            return false;
        };
        let mut params = json!({
            "family": family,
            "size": size,
            "bold": d.get("bold", "0") == "1",
            "italic": d.get("italic", "0") == "1",
            "underline": d.get("underline", "none"),
            "strike": d.get("strike", "none"),
            "baseline": d.get("baseline", "none"),
            "caps": d.get("caps", "none"),
            "spacing": spacing,
        });
        let color = d.get("color", "");
        let seed = d.get("colorSeed", "");
        if color == seed {
            if let Some(c) = d.params.get("color").filter(|v| !v.is_null()) {
                params["color"] = c.clone();
            }
        } else if !color.trim().is_empty() {
            params["color"] = json!(color.trim());
        }
        let _ = app.run("format.fontDialog", params);
    }
    ok || cancel
}
