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
    d.fields.insert("seeded".into(), "1".into());
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
    Ok(json!({"name": name, "colors": colors}))
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
