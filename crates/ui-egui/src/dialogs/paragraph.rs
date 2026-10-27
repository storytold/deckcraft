use super::*;

fn seed_paragraph_fields(d: &mut Dialog) {
    seed_once(d, "align", json_text(d.params.get("align"), "left"));
    seed_once(d, "before", inches_text(d.params.get("indentLeft")));
    seed_once(d, "after", inches_text(d.params.get("indentRight")));
    seed_once(d, "special", json_text(d.params.get("special"), "none"));
    seed_once(d, "by", inches_text(d.params.get("specialBy")));
    seed_once(d, "spaceBefore", pt_text(d.params.get("spaceBefore"), 0.0));
    seed_once(d, "spaceBeforeSeed", pt_text(d.params.get("spaceBefore"), 0.0));
    seed_once(d, "spaceAfter", pt_text(d.params.get("spaceAfter"), 0.0));
    seed_once(d, "spaceAfterSeed", pt_text(d.params.get("spaceAfter"), 0.0));
    seed_once(d, "lineKind", json_text(d.params.get("lineKind"), "multiple"));
    seed_once(d, "lineValue", pt_text(d.params.get("lineValue"), 1.0));
}

fn paragraph_return_params(d: &mut Dialog) -> Value {
    let before = finite_pt(&d.get("before", "0")).unwrap_or(0.0) * 72.0;
    let after = finite_pt(&d.get("after", "0")).unwrap_or(0.0) * 72.0;
    let by = finite_pt(&d.get("by", "0")).unwrap_or(0.0) * 72.0;
    let space_before = finite_pt(&d.get("spaceBefore", "0")).unwrap_or(0.0);
    let space_after = finite_pt(&d.get("spaceAfter", "0")).unwrap_or(0.0);
    let line_value = finite_pt(&d.get("lineValue", "1")).unwrap_or(1.0);
    let before_same = d.get("spaceBefore", "") == d.get("spaceBeforeSeed", "");
    let after_same = d.get("spaceAfter", "") == d.get("spaceAfterSeed", "");
    json!({
        "align": d.get("align", "left"),
        "indentLeft": before,
        "indentRight": after,
        "special": d.get("special", "none"),
        "specialBy": by,
        "spaceBefore": space_before,
        "spaceBeforeKind": if before_same { d.params.get("spaceBeforeKind").and_then(Value::as_str).unwrap_or("pts") } else { "pts" },
        "spaceAfter": space_after,
        "spaceAfterKind": if after_same { d.params.get("spaceAfterKind").and_then(Value::as_str).unwrap_or("pts") } else { "pts" },
        "lineKind": d.get("lineKind", "multiple"),
        "lineValue": line_value,
        "tabs": d.params.get("tabs").cloned().unwrap_or_else(|| json!([])),
        "defaultTab": d.params.get("defaultTab").cloned().unwrap_or_else(|| json!(72.0)),
    })
}

fn line_preset_label(kind: &str, value: &str) -> &'static str {
    if kind == "exactly" {
        "Exactly"
    } else {
        match value {
            "1" => "Single",
            "1.5" => "1.5 lines",
            "2" => "Double",
            _ => "Multiple",
        }
    }
}

/// Home › Paragraph launcher. One OK applies alignment, indentation and spacing together.
pub(super) fn paragraph_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    ui.set_min_width(460.0);
    seed_paragraph_fields(d);
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 20.0), egui::Label::new("Alignment"));
        choice(
            ui,
            d,
            "paragraph_align",
            "Alignment",
            "align",
            &[
                ("left", "Left"),
                ("center", "Center"),
                ("right", "Right"),
                ("justify", "Justify"),
                ("justLow", "Justify low"),
                ("distributed", "Distributed"),
            ],
        );
    });
    ui.label(egui::RichText::new("Indentation").strong());
    ui.horizontal(|ui| {
        let _ = labeled_text(ui, d, "Before text", "before", 64.0);
        ui.label("in");
        ui.add_space(12.0);
        let _ = labeled_text(ui, d, "After text", "after", 64.0);
        ui.label("in");
    });
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 20.0), egui::Label::new("Special"));
        choice(ui, d, "paragraph_special", "Special", "special", &[("none", "None"), ("first", "First line"), ("hanging", "Hanging")]);
        ui.add_space(8.0);
        let _ = labeled_text(ui, d, "By", "by", 64.0);
        ui.label("in");
    });
    ui.label(egui::RichText::new("Spacing").strong());
    ui.horizontal(|ui| {
        let _ = labeled_text(ui, d, "Spacing before", "spaceBefore", 64.0);
        ui.label("pt");
        ui.add_space(12.0);
        let _ = labeled_text(ui, d, "Spacing after", "spaceAfter", 64.0);
        ui.label("pt");
    });
    let mut picked: Option<(&'static str, Option<&'static str>)> = None;
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(110.0, 20.0), egui::Label::new("Line spacing"));
        let kind = d.get("lineKind", "multiple");
        let value = d.get("lineValue", "1");
        let combo = egui::ComboBox::from_id_salt("paragraph_line").selected_text(line_preset_label(&kind, &value)).width(140.0).show_ui(ui, |ui| {
            for (label, k, v) in [
                ("Single", "multiple", Some("1")),
                ("1.5 lines", "multiple", Some("1.5")),
                ("Double", "multiple", Some("2")),
                ("Multiple", "multiple", None),
                ("Exactly", "exactly", None),
            ] {
                if ui.selectable_label(line_preset_label(&kind, &value) == label, label).clicked() {
                    picked = Some((k, v));
                }
            }
        });
        let _ = combo.response.labelled_by(field.id);
        ui.add_space(8.0);
        let _ = labeled_text(ui, d, "At", "lineValue", 64.0);
    });
    if let Some((kind, value)) = picked {
        d.fields.insert("lineKind".into(), kind.into());
        if let Some(v) = value {
            d.fields.insert("lineValue".into(), v.into());
        } else if kind == "exactly" && finite_pt(&d.get("lineValue", "1")).is_some_and(|n| n <= 9.99) {
            d.fields.insert("lineValue".into(), "18".into());
        } else if kind == "multiple" && finite_pt(&d.get("lineValue", "1")).is_some_and(|n| n > 9.99) {
            d.fields.insert("lineValue".into(), "1".into());
        }
    }
    let mut open_tabs = false;
    ui.horizontal(|ui| {
        if ui.button("Tabs…").clicked() {
            open_tabs = true;
        }
    });
    let (ok, cancel) = buttons(ui, "OK");
    if open_tabs {
        let mut tabs = Dialog::new("tabs");
        tabs.params = json!({
            "tabs": d.params.get("tabs").cloned().unwrap_or_else(|| json!([])),
            "defaultTab": d.params.get("defaultTab").cloned().unwrap_or_else(|| json!(72.0)),
            "returnId": "paragraph",
            "returnParams": paragraph_return_params(d),
        });
        app.dialog = Some(tabs);
        return true;
    }
    if ok {
        let Some(before_in) = finite_pt(&d.get("before", "")) else {
            app.set_status("Indent before text must be a number of inches");
            return false;
        };
        let Some(after_in) = finite_pt(&d.get("after", "")) else {
            app.set_status("Indent after text must be a number of inches");
            return false;
        };
        let Some(by_in) = finite_pt(&d.get("by", "")) else {
            app.set_status("Special indent must be a number of inches");
            return false;
        };
        let Some(space_before) = finite_pt(&d.get("spaceBefore", "")) else {
            app.set_status("Spacing before must be a number");
            return false;
        };
        let Some(space_after) = finite_pt(&d.get("spaceAfter", "")) else {
            app.set_status("Spacing after must be a number");
            return false;
        };
        let Some(line_value) = finite_pt(&d.get("lineValue", "")) else {
            app.set_status("Line spacing must be a number");
            return false;
        };
        let mut params = json!({
            "align": d.get("align", "left"),
            "indentLeft": before_in * 72.0,
            "indentRight": after_in * 72.0,
            "special": d.get("special", "none"),
            "specialBy": by_in * 72.0,
            "spaceBefore": space_before,
            "spaceAfter": space_after,
            "spaceBeforeKind": if d.get("spaceBefore", "") == d.get("spaceBeforeSeed", "") { d.params.get("spaceBeforeKind").and_then(Value::as_str).unwrap_or("pts") } else { "pts" },
            "spaceAfterKind": if d.get("spaceAfter", "") == d.get("spaceAfterSeed", "") { d.params.get("spaceAfterKind").and_then(Value::as_str).unwrap_or("pts") } else { "pts" },
        });
        if d.get("lineKind", "multiple") == "exactly" {
            params["lineSpacingPt"] = json!(line_value);
        } else {
            params["lineSpacing"] = json!(line_value);
        }
        if app.run("format.paragraphDialog", params).is_err() {
            return false;
        }
    }
    ok || cancel
}
