use super::{Dialog, SlideApp, Ui, Value, json, theme};

/// What the Preferences dialog can change, as it was when the dialog opened.
pub(super) fn preferences_snapshot(app: &SlideApp) -> Value {
    json!({
        "darkMode": app.ui.dark_mode,
        "uiScale": app.ui.ui_scale,
        "prefs": serde_json::to_value(&app.session.prefs).unwrap_or(Value::Null),
    })
}

/// Puts back the settings the Preferences dialog opened with (Cancel, Esc, the close button).
pub(super) fn restore_preferences(app: &mut SlideApp, d: &Dialog) {
    let Some(before) = d.params.get("before") else { return };
    if let Some(mode) = before.get("darkMode").and_then(|m| serde_json::from_value(m.clone()).ok()) {
        app.ui.dark_mode = mode;
    }
    if let Some(scale) = before.get("uiScale").and_then(Value::as_f64) {
        app.ui.ui_scale = scale as f32;
    }
    if let Some(prefs) = before.get("prefs").and_then(|p| serde_json::from_value(p.clone()).ok()) {
        app.session.prefs = prefs;
    }
}

/// The Dark appearance menu (Preferences, File ▸ Account), after its label: Use system setting, On
/// or Off. A choice applies at once.
pub(crate) fn dark_mode_menu(app: &mut SlideApp, ui: &mut Ui) {
    const LABEL: &str = "Dark appearance";
    let current = app.ui.dark_mode;
    ui.horizontal(|ui| {
        ui.label(LABEL);
        let menu = craft_ui::ComboBox::from_id_salt(LABEL).selected_text(current.label()).width(150.0).show_ui(ui, |ui| {
            for mode in theme::DarkMode::ALL {
                if ui.selectable_label(mode == current, mode.label()).clicked() {
                    let _ = app.run("view.dark", json!({ "mode": mode }));
                }
            }
        });
        menu.response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, true, LABEL);
            info.current_text_value = Some(current.label().to_string());
            info
        });
    });
}
