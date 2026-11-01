use super::{Dialog, SlideApp, Value, json, theme};

/// What the Preferences dialog can change, as it was when the dialog opened.
pub(super) fn preferences_snapshot(app: &SlideApp) -> Value {
    json!({
        "dark": app.ui.brightness == theme::Brightness::Dark,
        "systemTheme": app.ui.system_theme,
        "uiScale": app.ui.ui_scale,
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
    if let Some(system) = before.get("systemTheme").and_then(Value::as_bool) {
        app.ui.system_theme = system;
    }
    if let Some(scale) = before.get("uiScale").and_then(Value::as_f64) {
        app.ui.ui_scale = scale as f32;
    }
    if let Some(prefs) = before.get("prefs").and_then(|p| serde_json::from_value(p.clone()).ok()) {
        app.session.prefs = prefs;
    }
}

#[test]
fn desktop_appearance_cancel_restores_system_preference() {
    let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
    app.ui.system_theme = true;
    let mut dialog = Dialog::new("preferences");
    dialog.params = json!({"before": preferences_snapshot(&app)});
    let _ = app.run("view.dark", json!({"on": true}));
    assert!(!app.ui.system_theme);
    restore_preferences(&mut app, &dialog);
    assert!(app.ui.system_theme);
    assert_eq!(app.ui.brightness, theme::Brightness::Light);
}
