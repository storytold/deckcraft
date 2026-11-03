use super::{Dialog, SlideApp, Ui, Value, buttons, field, json, num, vec2};

const TAB_KINDS: [(&str, &str); 4] = [("l", "Left"), ("ctr", "Center"), ("r", "Right"), ("dec", "Decimal")];

/// Format › Tabs: the default spacing and the tab stops of the selected paragraphs, edited as a
/// list (inches, as PowerPoint's dialog) and applied together with OK.
pub(super) fn tabs_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    if !d.fields.contains_key("stops") {
        // Opened from Paragraph (or by `format.tabs` with no fields) the stops come with the
        // dialog; otherwise they are read from the caret's paragraph.
        let r = if d.params.get("tabs").is_some() { d.params.clone() } else { app.session.execute("format.ruler", &json!({})).unwrap_or_default() };
        let stops: Vec<String> = r["tabs"]
            .as_array()
            .map(|a| a.iter().filter_map(|t| Some(format!("{}:{}", t["pos"].as_f64()? / 72.0, t["align"].as_str().unwrap_or("l")))).collect())
            .unwrap_or_default();
        d.fields.insert("stops".into(), stops.join("|"));
        d.fields.insert("default".into(), format!("{}", r["defaultTab"].as_f64().unwrap_or(72.0) / 72.0));
    }
    let mut stops: Vec<(f64, String)> = d
        .get("stops", "")
        .split('|')
        .filter_map(|x| {
            let (p, a) = x.split_once(':')?;
            Some((p.parse::<f64>().ok().filter(|v| v.is_finite())?, a.to_string()))
        })
        .collect();
    field(ui, d, "Default tab stops (in):", "default", "1");
    ui.add_space(6.0);
    ui.label("Tab stops:");
    let mut remove = None;
    egui::Grid::new("tab_stops").num_columns(3).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
        if stops.is_empty() {
            ui.label("(none)");
            ui.end_row();
        }
        for (i, (pos, align)) in stops.iter().enumerate() {
            ui.label(format!("{pos:.2}\""));
            ui.label(TAB_KINDS.iter().find(|k| k.0 == align).map(|k| k.1).unwrap_or("Left"));
            if ui.small_button("Clear").on_hover_text(format!("Clear the tab at {pos:.2}\"")).clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        stops.remove(i);
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let l = ui.label("Tab stop position (in):");
        let mut v = d.get("newpos", "");
        ui.add(egui::TextEdit::singleline(&mut v).desired_width(60.0)).labelled_by(l.id);
        d.fields.insert("newpos".into(), v);
    });
    let cur = d.get("newalign", "l");
    ui.horizontal(|ui| {
        ui.label("Alignment:");
        for (k, label) in TAB_KINDS {
            if ui.radio(cur == k, label).clicked() {
                d.fields.insert("newalign".into(), k.into());
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Set").clicked()
            && let Some(p) = Some(num(d, "newpos", f64::NAN)).filter(|v| v.is_finite() && *v >= 0.0 && *v <= 56.0)
        {
            stops.retain(|(q, _)| (q - p).abs() > 0.005);
            stops.push((p, d.get("newalign", "l")));
            stops.sort_by(|a, b| a.0.total_cmp(&b.0));
            d.fields.insert("newpos".into(), String::new());
        }
        if ui.button("Clear All").clicked() {
            stops.clear();
        }
    });
    d.fields.insert("stops".into(), stops.iter().map(|(p, a)| format!("{p}:{a}")).collect::<Vec<_>>().join("|"));
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        let set: Vec<Value> = stops.iter().map(|(p, a)| json!({"pos": p * 72.0, "align": a})).collect();
        let dt = num(d, "default", 1.0).clamp(0.02, 56.0) * 72.0;
        if let Err(e) = app.run("format.tabs", json!({"set": set, "defaultTab": dt})) {
            app.set_status(e);
        } else if let Some(ret) = d.params.get_mut("returnParams").and_then(Value::as_object_mut) {
            // Back in Paragraph, its Tabs… shows what was just applied.
            ret.insert("tabs".into(), json!(set));
            ret.insert("defaultTab".into(), json!(dt));
        }
    }
    ok || cancel
}
