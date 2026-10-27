use super::*;

const DATE_LANGS: &[(&str, &str)] = &[
    ("en-US", "English (United States)"),
    ("en-GB", "English (United Kingdom)"),
    ("fr-FR", "French (France)"),
    ("ja-JP", "Japanese"),
    ("sv-SE", "Swedish"),
];

/// Samples `field_text` can compute for `lang`. Named months and AM/PM exist for English only;
/// other languages keep the numeric date and the 24-hour times.
pub(super) fn date_rows(lang: &str) -> Vec<(&'static str, String)> {
    let now = deckcraft_text::datetime::DateTime::now_local();
    let mut rows = Vec::new();
    for kind in DATE_KINDS {
        if let Some(now) = now
            && let Some(sample) = deckcraft_text::datetime::field_text(kind, Some(lang), now)
        {
            rows.push((kind, sample));
        }
    }
    if rows.is_empty() {
        rows.push(("datetime1", "datetime1".into()));
    }
    rows
}

/// Render language and format controls; OK applies `insert.dateTime`, and the return value says
/// whether the dialog should close.
pub(super) fn date_time_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    ui.set_min_width(440.0);
    ui.label("Available formats:");
    let lang = d.get("language", "en-US");
    let rows = date_rows(&lang);
    if !rows.iter().any(|(k, _)| *k == d.get("format", "datetime1"))
        && let Some((k, _)) = rows.first()
    {
        d.fields.insert("format".into(), (*k).to_string());
    }
    let format = d.get("format", "datetime1");
    egui::ScrollArea::vertical().max_height(280.0).min_scrolled_height(220.0).auto_shrink([false, false]).show(ui, |ui| {
        for (kind, sample) in &rows {
            if ui.selectable_label(format == *kind, sample).clicked() {
                d.fields.insert("format".into(), (*kind).to_string());
            }
        }
    });
    let lang_label = DATE_LANGS.iter().find(|(k, _)| *k == lang).map(|(_, l)| *l).unwrap_or("English (United States)");
    ui.horizontal(|ui| {
        ui.label("Language:");
        let opened = egui::ComboBox::from_id_salt("date-lang").selected_text(lang_label).width(240.0).show_ui(ui, |ui| {
            for (k, l) in DATE_LANGS {
                if ui.selectable_label(lang == *k, *l).clicked() {
                    d.fields.insert("language".into(), (*k).to_string());
                }
            }
        });
        let shown = lang_label.to_string();
        opened.response.widget_info(move || {
            let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Language");
            info.current_text_value = Some(shown.clone());
            info
        });
    });
    ui.label(
        egui::RichText::new("Month names and AM/PM are listed for English. Other languages list the numeric date and 24-hour time.")
            .size(11.0)
            .color(Tokens::get(ui.ctx()).text_dim),
    );
    let update = check(ui, d, "Update automatically", "update", true);
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        let lang = d.get("language", "en-US");
        let mut format = d.get("format", "datetime1");
        let probe = deckcraft_text::datetime::DateTime::new(2026, 10, 9, 20, 5, 7);
        if deckcraft_text::datetime::field_text(&format, Some(&lang), probe).is_none() {
            format = "datetime1".into();
        }
        return app.run("insert.dateTime", json!({"format": format, "update": update, "language": lang})).is_ok();
    }
    cancel
}
