use super::{Dialog, SlideApp, Ui, Value, buttons, choice, finite_pt, inches_text, json, json_text, labeled_text, pt_text, seed_once, vec2};

fn seed_paragraph_fields(d: &mut Dialog) {
    seed_once(d, "align", json_text(d.params.get("align"), "왼쪽 맞춤"));
    seed_once(d, "before", inches_text(d.params.get("indentLeft")));
    seed_once(d, "after", inches_text(d.params.get("indentRight")));
    seed_once(d, "특별한 서식", json_text(d.params.get("특별한 서식"), "(없음)"));
    seed_once(d, "값", inches_text(d.params.get("specialBy")));
    seed_once(d, "spaceBefore", pt_text(d.params.get("spaceBefore"), 0.0));
    seed_once(d, "spaceBeforeSeed", pt_text(d.params.get("spaceBefore"), 0.0));
    seed_once(d, "spaceAfter", pt_text(d.params.get("spaceAfter"), 0.0));
    seed_once(d, "spaceAfterSeed", pt_text(d.params.get("spaceAfter"), 0.0));
    seed_once(d, "lineKind", json_text(d.params.get("lineKind"), "배수"));
    seed_once(d, "lineValue", pt_text(d.params.get("lineValue"), 1.0));
}

fn paragraph_return_params(d: &mut Dialog) -> Value {
    let before = finite_pt(&d.get("before", "0")).unwrap_or(0.0) * 72.0;
    let after = finite_pt(&d.get("after", "0")).unwrap_or(0.0) * 72.0;
    let by = finite_pt(&d.get("값", "0")).unwrap_or(0.0) * 72.0;
    let space_before = finite_pt(&d.get("spaceBefore", "0")).unwrap_or(0.0);
    let space_after = finite_pt(&d.get("spaceAfter", "0")).unwrap_or(0.0);
    let line_value = finite_pt(&d.get("lineValue", "1")).unwrap_or(1.0);
    let before_same = d.get("spaceBefore", "") == d.get("spaceBeforeSeed", "");
    let after_same = d.get("spaceAfter", "") == d.get("spaceAfterSeed", "");
    json!({
        "align": d.get("align", "왼쪽 맞춤"),
        "indentLeft": before,
        "indentRight": after,
        "특별한 서식": d.get("특별한 서식", "(없음)"),
        "specialBy": by,
        "spaceBefore": space_before,
        "spaceBeforeKind": if before_same { d.params.get("spaceBeforeKind").and_then(Value::as_str).unwrap_or("pts") } else { "pts" },
        "spaceAfter": space_after,
        "spaceAfterKind": if after_same { d.params.get("spaceAfterKind").and_then(Value::as_str).unwrap_or("pts") } else { "pts" },
        "lineKind": d.get("lineKind", "배수"),
        "lineValue": line_value,
        "tabs": d.params.get("tabs").cloned().unwrap_or_else(|| json!([])),
        "defaultTab": d.params.get("defaultTab").cloned().unwrap_or_else(|| json!(72.0)),
    })
}

fn line_preset_label(kind: &str, value: &str) -> &'static str {
    if kind == "고정" {
        "고정"
    } else {
        match value {
            "1" => "1줄",
            "1.5" => "1.5줄",
            "2" => "2줄",
            _ => "배수",
        }
    }
}

/// Home › Paragraph launcher. One OK applies alignment, indentation and spacing together.
pub(super) fn paragraph_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    ui.set_min_width(460.0);
    seed_paragraph_fields(d);
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 20.0), egui::Label::new("맞춤"));
        choice(
            ui,
            d,
            "paragraph_align",
            "맞춤",
            "align",
            &[
                ("왼쪽 맞춤", "왼쪽 맞춤"),
                ("가운데 맞춤", "가운데 맞춤"),
                ("오른쪽 맞춤", "오른쪽 맞춤"),
                ("양쪽 맞춤", "양쪽 맞춤"),
                ("justLow", "낮은 분산"),
                ("distributed", "균등 분할"),
            ],
        );
    });
    ui.label(egui::RichText::new("들여쓰기").strong());
    ui.horizontal(|ui| {
        let _ = labeled_text(ui, d, "텍스트 앞", "before", 64.0);
        ui.label("in");
        ui.add_space(12.0);
        let _ = labeled_text(ui, d, "텍스트 뒤", "after", 64.0);
        ui.label("in");
    });
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 20.0), egui::Label::new("특별한 서식"));
        choice(ui, d, "paragraph_special", "특별한 서식", "특별한 서식", &[("(없음)", "(없음)"), ("first", "첫 행"), ("둘째 줄 이하", "둘째 줄 이하")]);
        ui.add_space(8.0);
        let _ = labeled_text(ui, d, "값", "값", 64.0);
        ui.label("in");
    });
    ui.label(egui::RichText::new("간격").strong());
    ui.horizontal(|ui| {
        let _ = labeled_text(ui, d, "단락 앞", "spaceBefore", 64.0);
        ui.label("pt");
        ui.add_space(12.0);
        let _ = labeled_text(ui, d, "단락 뒤", "spaceAfter", 64.0);
        ui.label("pt");
    });
    let mut picked: Option<(&'static str, Option<&'static str>)> = None;
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(110.0, 20.0), egui::Label::new("줄 간격"));
        let kind = d.get("lineKind", "배수");
        let value = d.get("lineValue", "1");
        let combo =
            craft_ui::ComboBox::from_id_salt("paragraph_line").selected_text(line_preset_label(&kind, &value)).width(140.0).show_ui(ui, |ui| {
                for (label, k, v) in [
                    ("1줄", "배수", Some("1")),
                    ("1.5줄", "배수", Some("1.5")),
                    ("2줄", "배수", Some("2")),
                    ("배수", "배수", None),
                    ("고정", "고정", None),
                ] {
                    if ui.selectable_label(line_preset_label(&kind, &value) == label, label).clicked() {
                        picked = Some((k, v));
                    }
                }
            });
        let _ = combo.response.labelled_by(field.id);
        ui.add_space(8.0);
        let _ = labeled_text(ui, d, "값", "lineValue", 64.0);
    });
    if let Some((kind, value)) = picked {
        d.fields.insert("lineKind".into(), kind.into());
        if let Some(v) = value {
            d.fields.insert("lineValue".into(), v.into());
        } else if kind == "고정" && finite_pt(&d.get("lineValue", "1")).is_some_and(|n| n <= 9.99) {
            d.fields.insert("lineValue".into(), "18".into());
        } else if kind == "배수" && finite_pt(&d.get("lineValue", "1")).is_some_and(|n| n > 9.99) {
            d.fields.insert("lineValue".into(), "1".into());
        }
    }
    let mut open_tabs = false;
    ui.horizontal(|ui| {
        if ui.button("Tabs…").clicked() {
            open_tabs = true;
        }
    });
    let (ok, cancel) = buttons(ui, "확인");
    if open_tabs {
        let mut tabs = Dialog::new("tabs");
        tabs.params = json!({
            "tabs": d.params.get("tabs").cloned().unwrap_or_else(|| json!([])),
            "defaultTab": d.params.get("defaultTab").cloned().unwrap_or_else(|| json!(72.0)),
            "returnId": "단락",
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
        let Some(by_in) = finite_pt(&d.get("값", "")) else {
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
            "align": d.get("align", "왼쪽 맞춤"),
            "indentLeft": before_in * 72.0,
            "indentRight": after_in * 72.0,
            "특별한 서식": d.get("특별한 서식", "(없음)"),
            "specialBy": by_in * 72.0,
            "spaceBefore": space_before,
            "spaceAfter": space_after,
            "spaceBeforeKind": if d.get("spaceBefore", "") == d.get("spaceBeforeSeed", "") { d.params.get("spaceBeforeKind").and_then(Value::as_str).unwrap_or("pts") } else { "pts" },
            "spaceAfterKind": if d.get("spaceAfter", "") == d.get("spaceAfterSeed", "") { d.params.get("spaceAfterKind").and_then(Value::as_str).unwrap_or("pts") } else { "pts" },
        });
        if d.get("lineKind", "배수") == "고정" {
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
