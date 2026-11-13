use super::{Dialog, SlideApp, Ui, Value, check, field, json, vec2};

/// Home › Find / Replace: step through matches (slide text, table cells, notes) or replace them.
pub(super) fn find_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let replace_mode = d.get("replace", "0") == "1";
    // "Find what:" has the keyboard when the dialog opens, so Ctrl+F and typing finds (#951).
    let mut what = d.get("text", "");
    ui.horizontal(|ui| {
        ui.add_sized(vec2(120.0, 20.0), egui::Label::new("Find what:"));
        let r = ui.add(egui::TextEdit::singleline(&mut what).desired_width(180.0));
        // Enter finds the next match and leaves the keyboard here, so the search can be changed.
        if !d.fields.contains_key("focused") || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
            r.request_focus();
            d.fields.insert("focused".into(), "1".into());
        }
    });
    d.fields.insert("text".into(), what);
    if replace_mode {
        field(ui, d, "Replace with:", "with", "");
    }
    let case = check(ui, d, "Match case", "matchCase", false);
    let whole = check(ui, d, "Find whole words only", "wholeWords", false);
    let text = d.get("text", "");
    let q = json!({"text": text, "matchCase": case, "wholeWords": whole});
    let mut close = false;
    let mut find_next = false;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let has = !text.is_empty();
        if ui.add_enabled(has, egui::Button::new("Find Next")).clicked() || (has && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
            find_next = true;
        }
        if replace_mode {
            if ui.add_enabled(has, egui::Button::new("Replace")).clicked() {
                let cur: Option<Value> = serde_json::from_str(&d.get("cur", "")).ok();
                if let Some(at) = cur {
                    let mut p = q.clone();
                    p["with"] = json!(d.get("with", ""));
                    p["at"] = at;
                    if app.session.execute("text.replace", &p).is_ok() {
                        let n = d.get("next", "1").parse::<usize>().unwrap_or(1);
                        d.fields.insert("next".into(), n.saturating_sub(1).to_string());
                    }
                }
                find_next = true;
            }
            if ui.add_enabled(has, egui::Button::new("Replace All")).clicked() {
                let mut p = q.clone();
                p["with"] = json!(d.get("with", ""));
                let msg = match app.session.execute("text.replace", &p) {
                    Ok(v) => format!("Replaced {} occurrence(s).", v.get("replaced").and_then(Value::as_u64).unwrap_or(0)),
                    Err(_) => format!("DeckCraft can't find “{text}”."),
                };
                d.fields.insert("status".into(), msg);
                d.fields.remove("cur");
            }
        } else if ui.button("Replace…").clicked() {
            d.fields.insert("replace".into(), "1".into());
        }
        if ui.button("Close").clicked() {
            close = true;
        }
    });
    if find_next {
        let list = app.session.execute("text.find", &q).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        if list.is_empty() {
            d.fields.insert("status".into(), format!("DeckCraft can't find “{text}”."));
            d.fields.remove("cur");
        } else {
            let n = d.get("next", "0").parse::<usize>().unwrap_or(0) % list.len();
            let m = &list[n];
            let _ = app.run("slide.go", json!({"index": m.get("slide").cloned().unwrap_or_default()}));
            if let Some(id) = m.get("shape").filter(|v| !v.is_null()) {
                let _ = app.run("edit.select", json!({"ids": [id]}));
            }
            d.fields.insert("cur".into(), m.to_string());
            d.fields.insert("next".into(), (n + 1).to_string());
            let where_ = if m.get("shape").is_some_and(Value::is_null) { " (notes)" } else { "" };
            d.fields.insert(
                "status".into(),
                format!("Match {} of {} on slide {}{where_}.", n + 1, list.len(), m.get("slide").and_then(Value::as_u64).unwrap_or(0) + 1),
            );
        }
    }
    let status = d.get("status", "");
    if !status.is_empty() {
        ui.add_space(4.0);
        ui.label(status);
    }
    close
}
