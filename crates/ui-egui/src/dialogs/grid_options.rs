use super::{Color32, Dialog, SlideApp, Ui, buttons, check, field, json, num, theme};

fn format_spacing(v: f64) -> String {
    if !v.is_finite() {
        return "6".into();
    }
    if (v - v.round()).abs() < 1e-6 && v.abs() < 1.0e6 { format!("{}", v.round() as i64) } else { format!("{v:.2}") }
}

/// Grid Options: spacing, snap and whether the grid and guides are drawn. OK applies one
/// `view.gridOptions` call; Cancel leaves the presentation as it was.
pub(super) fn grid_options(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    ui.label(egui::RichText::new("맞춤").font(theme::bold(13.0)));
    let snap = check(ui, d, "개체를 눈금에 맞춤", "snap", app.session.prefs.snap_to_grid);
    let smart = check(ui, d, "개체를 다른 개체에 맞춤", "smart", app.session.prefs.smart_guides);
    let spacing_default = format_spacing(app.session.prefs.grid_spacing);
    field(ui, d, "간격(pt):", "spacing", &spacing_default);
    ui.add_space(6.0);
    ui.label(egui::RichText::new("표시").font(theme::bold(13.0)));
    let gridlines = check(ui, d, "화면에 눈금 표시", "gridlines", app.ui.gridlines);
    let guides = check(ui, d, "화면에 그리기 안내선 표시", "guides", app.ui.guides);
    if let Some(err) = d.fields.get("error").cloned().filter(|s| !s.is_empty()) {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(err).color(Color32::from_rgb(0xB0, 0x30, 0x30)));
    }
    let (ok, cancel) = buttons(ui, "확인");
    if ok {
        let spacing = num(d, "spacing", app.session.prefs.grid_spacing);
        match app.run("view.gridOptions", json!({"spacing": spacing, "snap": snap, "smartGuides": smart, "gridlines": gridlines, "guides": guides})) {
            Ok(_) => return true,
            Err(e) => {
                d.fields.insert("error".into(), e);
                return false;
            }
        }
    }
    cancel
}
