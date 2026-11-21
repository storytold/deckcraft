use super::{Color32, Dialog, SlideApp, Ui, Value, buttons, field, field_of, json, num_text};

/// The Columns dialog's changed fields as `format.columns` params: Number and Spacing (inches,
/// sent in points). A field left as the dialog showed it is left out, so each selected text box
/// keeps its own value; `None` when neither field changed.
fn columns_value(d: &Dialog, shown_count: &str, shown_spacing: &str) -> Result<Option<Value>, String> {
    let mut p = serde_json::Map::new();
    let count = field_of(d, "count");
    if count != shown_count {
        let n = count.trim().parse::<u32>().ok().filter(|n| (1..=16).contains(n)).ok_or("Number must be a whole number from 1 to 16")?;
        p.insert("count".into(), json!(n));
    }
    let spacing = field_of(d, "spacing");
    if spacing != shown_spacing {
        let inches = spacing
            .trim()
            .trim_end_matches("in")
            .trim_end_matches('"')
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && (0.0..=22.0).contains(v))
            .ok_or("Spacing must be from 0 to 22 inches")?;
        p.insert("spacing".into(), json!(inches * 72.0));
    }
    Ok((!p.is_empty()).then_some(Value::Object(p)))
}

/// Columns: the number of columns in the text box and the space between them. OK applies one
/// `format.columns` call with the fields that changed, or none when neither did; Cancel leaves
/// the text box as it was.
pub(super) fn columns_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let count = d.params.get("count").and_then(Value::as_u64).unwrap_or(1).to_string();
    let spacing = num_text(d.params.get("spacing").and_then(Value::as_f64).unwrap_or(0.0) / 72.0);
    field(ui, d, "Number:", "count", &count);
    field(ui, d, "Spacing (in):", "spacing", &spacing);
    if let Some(err) = d.fields.get("error").cloned().filter(|s| !s.is_empty()) {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(err).color(Color32::from_rgb(0xB0, 0x30, 0x30)));
    }
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        let applied = columns_value(d, &count, &spacing).and_then(|p| match p {
            Some(p) => app.run("format.columns", p).map(|_| ()),
            None => Ok(()),
        });
        match applied {
            Ok(()) => return true,
            Err(e) => {
                d.fields.insert("error".into(), e);
                return false;
            }
        }
    }
    cancel
}
