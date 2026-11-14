use super::{Dialog, SlideApp, Value, json, theme};

/// What the Preferences dialog can change, as it was when the dialog opened.
pub(super) fn preferences_snapshot(app: &SlideApp) -> Value {
    json!({
        "theme": match app.ui.brightness {
            None => "system",
            Some(theme::Brightness::Light) => "light",
            Some(theme::Brightness::Dark) => "dark",
        },
        // What was on screen, for a system setting the desktop gives no answer for.
        "dark": app.shown_brightness == theme::Brightness::Dark,
        "uiScale": app.ui.ui_scale,
        "prefs": serde_json::to_value(&app.session.prefs).unwrap_or(Value::Null),
    })
}

/// Puts back the settings the Preferences dialog opened with (Cancel, Esc, the close button).
pub(super) fn restore_preferences(app: &mut SlideApp, d: &Dialog) {
    let Some(before) = d.params.get("before") else { return };
    if let Some(value) = before.get("theme").and_then(Value::as_str) {
        if Some(value)
            != match app.ui.brightness {
                None => Some("system"),
                Some(theme::Brightness::Light) => Some("light"),
                Some(theme::Brightness::Dark) => Some("dark"),
            }
        {
            let _ = app.run("view.theme", json!({ "value": value }));
        }
    } else if let Some(dark) = before.get("dark").and_then(Value::as_bool)
        && dark != (app.shown_brightness == theme::Brightness::Dark)
    {
        let _ = app.run("view.dark", json!({ "on": dark }));
    }
    if let Some(dark) = before.get("dark").and_then(Value::as_bool) {
        // With no system answer the restyle keeps what was drawn, so put that back explicitly:
        // Cancel must not leave the dark choice the user tried.
        app.shown_brightness = if dark { theme::Brightness::Dark } else { theme::Brightness::Light };
        app.restyle = true;
    }
    if let Some(scale) = before.get("uiScale").and_then(Value::as_f64) {
        app.ui.ui_scale = scale as f32;
    }
    if let Some(prefs) = before.get("prefs").and_then(|p| serde_json::from_value(p.clone()).ok()) {
        app.session.prefs = prefs;
    }
}
