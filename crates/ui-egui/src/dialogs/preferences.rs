use super::{Dialog, SlideApp, Value, json, theme};

/// What the Preferences dialog can change, as it was when the dialog opened.
pub(super) fn preferences_snapshot(app: &SlideApp) -> Value {
    json!({
        "dark": app.ui.brightness == theme::Brightness::Dark,
        "uiScale": app.ui.ui_scale,
        "language": app.ui.language,
        "prefs": serde_json::to_value(&app.session.prefs).unwrap_or(Value::Null),
    })
}

/// Puts back the settings the Preferences dialog opened with (Cancel, Esc, the close button).
pub(super) fn restore_preferences(app: &mut SlideApp, d: &Dialog) {
    let Some(before) = d.params.get("before") else { return };
    if let Some(dark) = before.get("dark").and_then(Value::as_bool)
        && dark != (app.ui.brightness == theme::Brightness::Dark)
    {
        let _ = app.run("view.dark", json!({ "on": dark }));
    }
    if let Some(scale) = before.get("uiScale").and_then(Value::as_f64) {
        app.ui.ui_scale = scale as f32;
    }
    if let Some(language) = before.get("language").and_then(Value::as_str) {
        let _ = app.run("app.language", json!({ "language": language }));
    }
    if let Some(prefs) = before.get("prefs").and_then(|p| serde_json::from_value(p.clone()).ok()) {
        app.session.prefs = prefs;
    }
}

/// Preferences › Interface language: follow the system, or one of the languages this build has
/// catalogs for, each named in itself. It applies at once, like the other controls here.
pub(super) fn language_row(app: &mut SlideApp, ui: &mut egui::Ui) {
    let auto = crate::i18n::AUTO;
    let name = |code: &str| if code == auto { tl!("System language").to_string() } else { crate::i18n::Lang::from_pref(code).name().to_string() };
    let mut chosen = app.ui.language.clone();
    ui.horizontal(|ui| {
        ui.label(tl!("Interface language:"));
        let combo = craft_ui::ComboBox::from_id_salt("preferences.language").selected_text(name(&chosen)).width(160.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut chosen, auto.to_string(), name(auto));
            for lang in crate::i18n::Lang::all() {
                ui.selectable_value(&mut chosen, lang.code().to_string(), lang.name());
            }
        });
        combo.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, tl!("Interface language")));
    });
    if chosen != app.ui.language {
        let _ = app.run("app.language", json!({ "language": chosen }));
    }
}
