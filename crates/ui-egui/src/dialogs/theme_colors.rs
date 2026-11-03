use super::{Color32, CornerRadius, Dialog, SchemeSlot, Sense, SlideApp, Stroke, Tokens, Ui, Value, buttons, json, theme, vec2};

fn seed_theme_colors(d: &mut Dialog) {
    if d.fields.contains_key("seeded") {
        return;
    }
    let name = d.params.get("name").and_then(Value::as_str).unwrap_or("Custom");
    d.fields.insert("schemeName".into(), name.to_string());
    let obj = d.params.get("colors").and_then(Value::as_object);
    for slot in SchemeSlot::THEME {
        let hex = obj.and_then(|o| o.get(slot.xml_name())).and_then(Value::as_str).unwrap_or("000000");
        d.fields.insert(slot.xml_name().into(), hex.to_string());
    }
    let extra = d.params.get("extra").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    d.fields.insert("extraCount".into(), extra.len().to_string());
    d.fields.insert("computedTints".into(), if d.params.get("computedTints").and_then(Value::as_bool).unwrap_or(true) { "1" } else { "0" }.into());
    for (i, e) in extra.iter().enumerate() {
        d.fields.insert(format!("extra.{i}.name"), e.get("name").and_then(Value::as_str).unwrap_or("").to_string());
        d.fields.insert(format!("extra.{i}.color"), e.get("color").and_then(Value::as_str).unwrap_or("000000").to_string());
        // Which of the scheme's extra colours the row edits: its tint rows follow it through a
        // rename or the removal of a row above (an added row has none).
        d.fields.insert(format!("extra.{i}.from"), i.to_string());
    }
    d.fields.insert("seeded".into(), "1".into());
}

fn extra_count(d: &mut Dialog) -> usize {
    d.get("extraCount", "0").parse().unwrap_or(0)
}

/// The extra colours (name + hex rows), with Remove on each and Add colour below. The rows scroll,
/// so the dialog keeps its height with any number of them.
fn extra_color_rows(ui: &mut Ui, d: &mut Dialog) {
    ui.label(egui::RichText::new("Extra colours").font(theme::bold(12.0)));
    ui.label(
        egui::RichText::new(
            "Named colours shown after the theme colours in every colour gallery (DeckCraft only; PowerPoint shows the twelve slots).",
        )
        .size(11.0)
        .color(Tokens::get(ui.ctx()).text_dim),
    );
    let mut n = extra_count(d);
    let mut remove = None;
    egui::ScrollArea::vertical().id_salt("extra-colours").max_height(96.0).show(ui, |ui| {
        for i in 0..n {
            let (name_key, color_key) = (format!("extra.{i}.name"), format!("extra.{i}.color"));
            let mut name = d.get(&name_key, "");
            let mut hex = d.get(&color_key, "000000");
            ui.horizontal(|ui| {
                let resp = ui.add(egui::TextEdit::singleline(&mut name).desired_width(160.0).hint_text("Name").id_salt(("extra-name", i)));
                let shown = name.clone();
                resp.widget_info(move || {
                    let mut info = egui::WidgetInfo::labeled(egui::Role::TextInput, true, format!("Extra colour {} name", i + 1));
                    info.current_text_value = Some(shown.clone());
                    info
                });
                let fill = deckcraft_color::Rgba::from_hex(&hex).map(theme::to_color32).unwrap_or_else(|| Color32::from_gray(200));
                let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                ui.painter().rect(rect, CornerRadius::same(2), fill, Stroke::new(1.0, Tokens::get(ui.ctx()).border), egui::StrokeKind::Inside);
                let resp = ui.add(egui::TextEdit::singleline(&mut hex).desired_width(84.0).id_salt(("extra-hex", i)));
                let shown = hex.clone();
                resp.widget_info(move || {
                    let mut info = egui::WidgetInfo::labeled(egui::Role::TextInput, true, format!("Extra colour {} hex", i + 1));
                    info.current_text_value = Some(shown.clone());
                    info
                });
                let resp = ui.button("Remove");
                resp.widget_info(move || egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Remove extra colour {}", i + 1)));
                if resp.clicked() {
                    remove = Some(i);
                }
            });
            d.fields.insert(name_key, name);
            d.fields.insert(color_key, hex);
        }
    });
    if let Some(r) = remove {
        for i in r..n.saturating_sub(1) {
            for k in ["name", "color", "from"] {
                match d.fields.get(&format!("extra.{}.{k}", i + 1)).cloned() {
                    Some(next) => d.fields.insert(format!("extra.{i}.{k}"), next),
                    None => d.fields.remove(&format!("extra.{i}.{k}")),
                };
            }
        }
        n = n.saturating_sub(1);
        d.fields.insert("extraCount".into(), n.to_string());
        for k in ["name", "color", "from"] {
            d.fields.remove(&format!("extra.{n}.{k}"));
        }
    }
    if ui.button("Add colour").clicked() && n < deckcraft_color::MAX_EXTRA_COLORS {
        d.fields.insert(format!("extra.{n}.name"), String::new());
        d.fields.insert(format!("extra.{n}.color"), "000000".into());
        d.fields.remove(&format!("extra.{n}.from"));
        d.fields.insert("extraCount".into(), (n + 1).to_string());
    }
    let mut computed = d.get("computedTints", "1") == "1";
    ui.checkbox(&mut computed, "Computed tints and shades under colours without their own rows")
        .on_hover_text("Off: a colour shows only the rows set with design.colors (tints), none when it has no list.");
    d.fields.insert("computedTints".into(), if computed { "1" } else { "0" }.into());
}

/// Draw one theme-colour row with its hex editor and store the value for the scheme slot.
fn theme_color_row(ui: &mut Ui, d: &mut Dialog, slot: SchemeSlot) {
    let key = slot.xml_name();
    let mut v = d.get(key, "000000");
    ui.horizontal(|ui| {
        ui.add_sized(vec2(188.0, 18.0), egui::Label::new(slot.label()));
        let fill = deckcraft_color::Rgba::from_hex(&v).map(theme::to_color32).unwrap_or_else(|| Color32::from_gray(200));
        let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        ui.painter().rect(rect, CornerRadius::same(2), fill, Stroke::new(1.0, Tokens::get(ui.ctx()).border), egui::StrokeKind::Inside);
        let resp = ui.add(egui::TextEdit::singleline(&mut v).desired_width(84.0).id_salt(("theme-hex", key)));
        let shown = v.clone();
        resp.widget_info(move || {
            let mut info = egui::WidgetInfo::labeled(egui::Role::TextInput, true, format!("{} hex", slot.label()));
            info.current_text_value = Some(shown.clone());
            info
        });
    });
    d.fields.insert(key.into(), v);
}

fn theme_color_payload(d: &mut Dialog) -> Result<Value, String> {
    let name = d.get("schemeName", "").trim().to_string();
    if name.is_empty() {
        return Err("Enter a scheme name.".into());
    }
    let mut colors = serde_json::Map::new();
    for slot in SchemeSlot::THEME {
        let raw = d.get(slot.xml_name(), "");
        let Some(rgb) = deckcraft_color::Rgba::from_hex(raw.trim()) else {
            return Err(format!("{} needs a colour like #RRGGBB.", slot.label()));
        };
        colors.insert(slot.xml_name().into(), Value::String(format!("#{}", rgb.hex())));
    }
    // Extra colours as edited; a row seeded from one of the scheme's keeps that colour's explicit
    // tint rows (`extra.N.from`, whatever its name is now), and the slots' tint rows pass through
    // untouched (the dialog does not edit them).
    let before = d.params.get("extra").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut extra = Vec::new();
    for i in 0..extra_count(d) {
        let name = d.get(&format!("extra.{i}.name"), "").trim().to_string();
        if name.is_empty() {
            return Err(format!("Extra colour {} needs a name.", i + 1));
        }
        let raw = d.get(&format!("extra.{i}.color"), "");
        let Some(rgb) = deckcraft_color::Rgba::from_hex(raw.trim()) else {
            return Err(format!("{name} needs a colour like #RRGGBB."));
        };
        let mut e = json!({"name": name, "color": format!("#{}", rgb.hex())});
        if let Some(t) = d.fields.get(&format!("extra.{i}.from")).and_then(|f| f.parse::<usize>().ok()).and_then(|f| before.get(f)?.get("tints"))
            && let Some(o) = e.as_object_mut()
        {
            o.insert("tints".into(), t.clone());
        }
        extra.push(e);
    }
    let mut payload = json!({"name": name, "colors": colors, "extra": extra, "computedTints": d.get("computedTints", "1") == "1"});
    if let (Some(t), Some(o)) = (d.params.get("tints").filter(|t| t.is_object()), payload.as_object_mut()) {
        o.insert("tints".into(), t.clone());
    }
    Ok(payload)
}

/// Create New Theme Colors. Returns true when the dialog should close.
pub(super) fn theme_colors_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    seed_theme_colors(d);
    // The window keeps the height of its contents and will not grow after the first frame, so the
    // rows stay short enough that Save is inside that rect.
    ui.style_mut().spacing.interact_size.y = 16.0;
    ui.style_mut().spacing.item_spacing.y = 2.0;
    ui.style_mut().spacing.button_padding.y = 2.0;
    let left = [SchemeSlot::Dk1, SchemeSlot::Lt1, SchemeSlot::Dk2, SchemeSlot::Lt2, SchemeSlot::Hlink, SchemeSlot::FolHlink];
    let right = [SchemeSlot::Accent1, SchemeSlot::Accent2, SchemeSlot::Accent3, SchemeSlot::Accent4, SchemeSlot::Accent5, SchemeSlot::Accent6];
    egui::Grid::new("theme-colors").spacing(vec2(18.0, 4.0)).show(ui, |ui| {
        for (a, b) in left.into_iter().zip(right) {
            theme_color_row(ui, d, a);
            theme_color_row(ui, d, b);
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    extra_color_rows(ui, d);
    ui.add_space(6.0);
    let mut name = d.get("schemeName", "");
    ui.horizontal(|ui| {
        ui.label("Name:");
        let resp = ui.add(egui::TextEdit::singleline(&mut name).desired_width(280.0).id_salt("schemeName"));
        let shown = name.clone();
        resp.widget_info(move || {
            let mut info = egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Scheme name");
            info.current_text_value = Some(shown.clone());
            info
        });
    });
    d.fields.insert("schemeName".into(), name);
    ui.label(egui::RichText::new("Applies to this presentation's theme colours.").size(11.0).color(Tokens::get(ui.ctx()).text_dim));
    let (ok, cancel) = buttons(ui, "Save");
    if ok {
        match theme_color_payload(d) {
            Ok(payload) => return app.run("design.colors", payload).is_ok(),
            Err(msg) => {
                app.set_status(msg);
                return false;
            }
        }
    }
    cancel
}
