use super::*;

/// Preset percents in the Zoom dialog, largest first.
const ZOOM_PRESETS: &[u32] = &[400, 200, 150, 100, 75, 66, 50, 33];

/// A typed zoom percent as a scale in `0.1..=4` (10%–400%). `None` when it is not a finite number.
pub fn parse_zoom_percent(text: &str) -> Option<f32> {
    let t = text.trim().trim_end_matches('%').trim();
    if t.is_empty() {
        return None;
    }
    let v = t.parse::<f64>().ok().filter(|n| n.is_finite())?;
    Some(((v / 100.0) as f32).clamp(0.1, 4.0))
}

/// Zoom dialog: a percent or Fit. Nothing changes until OK.
pub(super) fn zoom_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    if !d.fields.contains_key("mode") {
        let shown = app.ui.zoom.unwrap_or(app.canvas_scale);
        let pct = if shown.is_finite() { (shown * 100.0).round().clamp(10.0, 400.0) } else { 100.0 };
        d.fields.insert("mode".into(), if app.ui.zoom.is_none() { "fit".into() } else { "percent".into() });
        d.fields.insert("percent".into(), format!("{pct:.0}"));
    }
    let mut mode = d.get("mode", "percent");
    let mut percent = d.get("percent", "100");
    ui.label("Zoom to");
    if ui.radio(mode == "fit", "Fit").clicked() {
        mode = "fit".into();
    }
    for pct in ZOOM_PRESETS {
        let on = mode == "percent" && percent == pct.to_string();
        if ui.radio(on, format!("{pct}%")).clicked() {
            mode = "percent".into();
            percent = pct.to_string();
        }
    }
    ui.horizontal(|ui| {
        let l = ui.label("Percent:");
        let edit = ui.add(egui::TextEdit::singleline(&mut percent).desired_width(72.0)).labelled_by(l.id);
        if edit.changed() {
            mode = "percent".into();
        }
        ui.label("%");
    });
    d.fields.insert("mode".into(), mode.clone());
    d.fields.insert("percent".into(), percent.clone());
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        if mode == "fit" {
            app.ui.zoom = None;
            return true;
        }
        match parse_zoom_percent(&percent) {
            Some(scale) => app.ui.zoom = Some(scale),
            None => {
                app.set_status("Enter a zoom percent");
                return false;
            }
        }
    }
    ok || cancel
}

#[cfg(test)]
mod zoom_tests {
    use super::parse_zoom_percent;

    fn near(v: f32, want: f32) -> bool {
        (v - want).abs() < 1.0e-4
    }

    #[test]
    fn zoom_percent_accepts_a_number_and_clamps() {
        assert!(parse_zoom_percent("150").is_some_and(|v| near(v, 1.5)));
        assert!(parse_zoom_percent("150%").is_some_and(|v| near(v, 1.5)));
        assert!(parse_zoom_percent("  100 ").is_some_and(|v| near(v, 1.0)));
        assert!(parse_zoom_percent("1").is_some_and(|v| near(v, 0.1)));
        assert!(parse_zoom_percent("900").is_some_and(|v| near(v, 4.0)));
        assert!(parse_zoom_percent("").is_none());
        assert!(parse_zoom_percent("fit").is_none());
        assert!(parse_zoom_percent("nan").is_none());
        assert!(parse_zoom_percent("inf").is_none());
    }
}
