use super::{Dialog, SlideApp, Value, json, theme};

/// What the Preferences dialog can change, as it was when the dialog opened.
pub(super) fn preferences_snapshot(app: &SlideApp) -> Value {
    json!({
        "dark": app.ui.brightness == theme::Brightness::Dark,
        "uiScale": app.ui.ui_scale,
        "interfaceLanguage": app.ui.interface_language,
        "prefs": serde_json::to_value(&app.session.prefs).unwrap_or(Value::Null),
    })
}

/// Capture the original settings before the language control can change them.
pub(super) fn interface_language_row(app: &mut SlideApp, ui: &mut egui::Ui, d: &mut Dialog) {
    if d.params.get("before").is_none() {
        if !d.params.is_object() {
            d.params = json!({});
        }
        d.params["before"] = preferences_snapshot(app);
    }
    let language = app.ui.interface_language;
    ui.horizontal(|ui| {
        let label = ui.label(language.text("Interface language"));
        let combo = craft_ui::ComboBox::from_id_salt("interface-language").selected_text(language.display_name()).show_ui(ui, |ui| {
            for option in [crate::i18n::Language::English, crate::i18n::Language::Russian] {
                ui.selectable_value(&mut app.ui.interface_language, option, option.display_name());
            }
        });
        let _ = combo.response.labelled_by(label.id);
    });
    if app.ui.interface_language != language {
        ui.ctx().request_repaint();
    }
    ui.separator();
}

/// Puts back the settings the Preferences dialog opened with (Cancel, Esc, the close button).
pub(super) fn restore_preferences(app: &mut SlideApp, d: &Dialog) {
    let Some(before) = d.params.get("before") else { return };
    if let Some(language) = before.get("interfaceLanguage").and_then(|v| serde_json::from_value(v.clone()).ok()) {
        app.ui.interface_language = language;
    }
    if let Some(dark) = before.get("dark").and_then(Value::as_bool)
        && dark != (app.ui.brightness == theme::Brightness::Dark)
    {
        let _ = app.run("view.dark", json!({ "on": dark }));
    }
    if let Some(scale) = before.get("uiScale").and_then(Value::as_f64) {
        app.ui.ui_scale = scale as f32;
    }
    if let Some(prefs) = before.get("prefs").and_then(|p| serde_json::from_value(p.clone()).ok()) {
        app.session.prefs = prefs;
    }
}

#[cfg(test)]
mod interface_language_tests {
    use super::*;
    use crate::{Services, UiState, i18n::Language};

    #[test]
    fn old_settings_default_to_english_and_unknown_languages_are_safe() {
        let old = serde_json::from_value::<UiState>(json!({"zoom": 1.25}));
        assert!(old.is_ok());
        if let Ok(state) = old {
            assert_eq!(state.interface_language, Language::English);
            assert_eq!(state.zoom, Some(1.25));
        }
        let unknown = serde_json::from_value::<UiState>(json!({"interfaceLanguage": "future-language"}));
        assert!(unknown.is_ok());
        if let Ok(state) = unknown {
            assert_eq!(state.interface_language, Language::English);
        }
    }

    #[test]
    fn russian_language_round_trips_with_ui_state() {
        let state = UiState { interface_language: Language::Russian, ..Default::default() };
        let encoded = serde_json::to_value(&state);
        assert!(encoded.is_ok());
        if let Ok(value) = encoded {
            assert_eq!(value.get("interfaceLanguage"), Some(&json!("ru")));
            let restored = serde_json::from_value::<UiState>(value);
            assert!(restored.is_ok());
            if let Ok(restored) = restored {
                assert_eq!(restored.interface_language, Language::Russian);
            }
        }
    }

    #[test]
    fn cancelling_preferences_restores_the_interface_language() {
        for language in [Language::English, Language::Russian] {
            let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), Services::default());
            app.ui.interface_language = language;
            let mut dialog = Dialog::new("preferences");
            dialog.params = json!({"before": preferences_snapshot(&app)});
            app.ui.interface_language = if language == Language::English { Language::Russian } else { Language::English };
            restore_preferences(&mut app, &dialog);
            assert_eq!(app.ui.interface_language, language);
        }
    }
}
