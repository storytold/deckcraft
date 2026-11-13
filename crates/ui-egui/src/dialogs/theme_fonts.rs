use super::{Dialog, SlideApp, Tokens, Ui, Value, buttons, json, vec2};

fn seed_theme_fonts(d: &mut Dialog) {
    if d.fields.contains_key("seeded") {
        return;
    }
    let name = d.params.get("name").and_then(Value::as_str).unwrap_or("Custom");
    let major = d.params.get("major").and_then(Value::as_str).unwrap_or("Inter");
    let minor = d.params.get("minor").and_then(Value::as_str).unwrap_or(major);
    d.fields.insert("schemeName".into(), name.to_string());
    d.fields.insert("major".into(), major.to_string());
    d.fields.insert("minor".into(), minor.to_string());
    d.fields.insert("seeded".into(), "1".into());
}

fn theme_font_choices(app: &SlideApp, selected: &str) -> Vec<String> {
    let mut v = crate::ribbon::font_families(app);
    for scheme in deckcraft_model::theme::builtin_font_schemes() {
        v.push(scheme.major.latin);
        v.push(scheme.minor.latin);
    }
    if !selected.is_empty() {
        v.push(selected.to_string());
    }
    let mut seen = std::collections::HashSet::new();
    v.retain(|face| !face.is_empty() && seen.insert(face.clone()));
    v
}

/// Draw an accessible font combo box and update the selected family; the caller supplies the
/// visible label.
fn font_combo(ui: &mut Ui, label: &str, choices: &[String], selected: &mut String) {
    let resp = craft_ui::ComboBox::from_id_salt(label).selected_text(selected.as_str()).width(240.0).show_ui(ui, |ui| {
        for face in choices {
            if ui.selectable_label(selected == face, face).clicked() {
                *selected = face.clone();
            }
        }
    });
    let shown = selected.clone();
    let access = label.to_string();
    resp.response.widget_info(move || {
        let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, true, access.clone());
        info.current_text_value = Some(shown.clone());
        info
    });
}

fn theme_font_payload(d: &mut Dialog) -> Result<Value, String> {
    let name = d.get("schemeName", "").trim().to_string();
    if name.is_empty() {
        return Err("Enter a scheme name.".into());
    }
    let major = d.get("major", "").trim().to_string();
    if major.is_empty() {
        return Err("Choose a heading font.".into());
    }
    let minor = d.get("minor", "").trim().to_string();
    if minor.is_empty() {
        return Err("Choose a body font.".into());
    }
    Ok(json!({"name": name, "major": major, "minor": minor}))
}

/// Create New Theme Fonts. Returns true when the dialog should close.
pub(super) fn theme_fonts_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    seed_theme_fonts(d);
    let major = d.get("major", "Inter");
    let minor = d.get("minor", "Inter");
    let headings = theme_font_choices(app, &major);
    let bodies = theme_font_choices(app, &minor);
    let mut major = major;
    let mut minor = minor;
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 18.0), egui::Label::new("Heading:"));
        font_combo(ui, "Heading font", &headings, &mut major);
    });
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 18.0), egui::Label::new("Body:"));
        font_combo(ui, "Body font", &bodies, &mut minor);
    });
    d.fields.insert("major".into(), major.clone());
    d.fields.insert("minor".into(), minor.clone());
    ui.add_space(4.0);
    // Theme faces are not egui families; naming an unbound family panics. The lines show the
    // chosen names at heading and body sizes.
    ui.label(egui::RichText::new(format!("Heading sample  {major}")).size(20.0));
    ui.label(egui::RichText::new(format!("Body sample  {minor}")).size(14.0));
    ui.add_space(4.0);
    let mut name = d.get("schemeName", "");
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 18.0), egui::Label::new("Name:"));
        let resp = ui.add(egui::TextEdit::singleline(&mut name).desired_width(240.0).id_salt("schemeName"));
        let shown = name.clone();
        resp.widget_info(move || {
            let mut info = egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Scheme name");
            info.current_text_value = Some(shown.clone());
            info
        });
    });
    d.fields.insert("schemeName".into(), name);
    ui.label(egui::RichText::new("Heading and body fonts for this presentation.").size(11.0).color(Tokens::get(ui.ctx()).text_dim));
    let (ok, cancel) = buttons(ui, "Save");
    if ok {
        match theme_font_payload(d) {
            Ok(payload) => return app.run("design.fonts", payload).is_ok(),
            Err(msg) => {
                app.set_status(msg);
                return false;
            }
        }
    }
    cancel
}
