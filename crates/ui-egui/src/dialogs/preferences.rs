use super::{Dialog, SlideApp, Value, json, theme};

/// What the Preferences dialog can change, as it was when the dialog opened.
pub(super) fn preferences_snapshot(app: &SlideApp) -> Value {
    json!({
        "language": app.ui.language,
        "dark": app.ui.brightness == theme::Brightness::Dark,
        "uiScale": app.ui.ui_scale,
        "prefs": serde_json::to_value(&app.session.prefs).unwrap_or(Value::Null),
    })
}

/// Puts back the settings the Preferences dialog opened with (Cancel, Esc, the close button).
pub(super) fn restore_preferences(app: &mut SlideApp, d: &Dialog) {
    let Some(before) = d.params.get("before") else { return };
    if let Some(language) = before.get("language").and_then(Value::as_str) {
        app.ui.language = language.to_string();
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

/// Preferences › Interface language: Automatic (the system's) or one of the languages whose
/// catalogs this build embeds. Applies as it changes; Cancel puts the old one back
/// ([`restore_preferences`]). Not shown when English is the only choice.
pub(super) fn interface_language_row(app: &mut SlideApp, ui: &mut egui::Ui) {
    use crate::i18n::{self, Lang};
    if Lang::all().nth(1).is_none() {
        return;
    }
    // Each language is named in itself; a name the interface fonts can't draw shows its code
    // (egui 0.37 aborts on a missing glyph).
    let name_of = |ui: &egui::Ui, lang: Lang| {
        let name = lang.name();
        if ui.ctx().fonts_mut(|f| f.has_glyphs(&egui::FontId::proportional(13.0), name)) { name.to_string() } else { lang.code().to_string() }
    };
    ui.horizontal(|ui| {
        let label = ui.label(crate::tl!("Interface language:"));
        let chosen = Lang::from_code(&app.ui.language);
        let shown = chosen.map_or_else(|| crate::tl!("Automatic").to_string(), |l| name_of(ui, l));
        craft_ui::ComboBox::from_id_salt("pref_interface_language")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui.selectable_label(chosen.is_none(), crate::tl!("Automatic")).clicked() {
                    app.ui.language = i18n::AUTO.to_string();
                }
                for lang in Lang::for_picker() {
                    if ui.selectable_label(chosen == Some(lang), name_of(ui, lang)).clicked() {
                        app.ui.language = lang.code().to_string();
                    }
                }
            })
            .response
            .labelled_by(label.id);
    });
}
