use super::*;

/// The export dialog's Animated GIF options (size, seconds per slide, loop, slide range, hidden
/// slides) as `file.export` params; a slide range that doesn't fit the deck is an error message.
pub(super) fn gif_options(app: &SlideApp, ui: &mut Ui, d: &mut Dialog) -> Result<Value, String> {
    use deckcraft_engine::gif_export::{MAX_SECONDS, MAX_WIDTH, MIN_SECONDS, MIN_WIDTH, SIZE_PRESETS};
    let n = app.session.active().map_or(0, |s| s.doc.slides.len());
    let size = d.get("gifSize", "medium");
    let size_label = |k: &str| match k {
        "small" => "Small (640 px wide)",
        "large" => "Large (1280 px wide)",
        "extraLarge" => "Extra Large (1920 px wide)",
        "custom" => "Custom width",
        _ => "Medium (960 px wide)",
    };
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(120.0, 20.0), egui::Label::new("Size:"));
        let combo = egui::ComboBox::from_id_salt("gif_size").selected_text(size_label(&size)).show_ui(ui, |ui| {
            for k in SIZE_PRESETS.iter().map(|(k, _)| *k).chain(["custom"]) {
                if ui.selectable_label(size == k, size_label(k)).clicked() {
                    d.fields.insert("gifSize".into(), k.into());
                }
            }
        });
        let _ = combo.response.labelled_by(field.id);
    });
    if size == "custom" {
        field(ui, d, "Width (pixels):", "gifWidth", "960");
    }
    field(ui, d, "Seconds per slide:", "gifSeconds", "5");
    let n_text = n.max(1).to_string();
    field(ui, d, "From slide:", "gifFrom", "1");
    field(ui, d, "To slide:", "gifTo", &n_text);
    let forever = check(ui, d, "Loop forever", "gifLoop", true);
    let hidden = check(ui, d, "Include hidden slides", "gifHidden", false);

    let width = if size == "custom" {
        let w = num(d, "gifWidth", 960.0).round();
        if !(f64::from(MIN_WIDTH)..=f64::from(MAX_WIDTH)).contains(&w) {
            return Err(format!("Width: enter {MIN_WIDTH} to {MAX_WIDTH} pixels"));
        }
        json!(w as u32)
    } else {
        json!(size)
    };
    let seconds = num(d, "gifSeconds", f64::NAN);
    if deckcraft_engine::gif_export::check_seconds(seconds).is_err() {
        return Err(format!("Seconds per slide: enter {MIN_SECONDS} to {MAX_SECONDS}"));
    }
    let mut p = json!({"width": width, "secondsPerSlide": seconds, "loop": forever, "includeHidden": hidden});
    let (from, to) = (num(d, "gifFrom", 1.0).round(), num(d, "gifTo", n as f64).round());
    if !(from >= 1.0 && from <= to && to <= n as f64) {
        return Err(format!("Slides: choose a range from 1 to {n}"));
    }
    if from > 1.0 || to < n as f64 {
        p["slides"] = json!(((from as usize - 1)..(to as usize)).collect::<Vec<_>>());
    }
    Ok(p)
}
