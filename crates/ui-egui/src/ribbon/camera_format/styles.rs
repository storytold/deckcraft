//! Camera Format ▸ Camera Styles: the nine preset frames (shape, outline, effects), Camera Shape,
//! Camera Border, Camera Effects and Format Object. A style is plain DrawingML on the picture, so
//! every other picture command keeps working on a styled cameo.

use super::super::galleries::{effects_menu, mask_gallery, outline_menu, paint_preset};
use super::super::{Color32, Icon, Rect, SlideApp, Ui, enabled, group, json, rows, run, scheme, small, vec2, widgets};
use deckcraft_engine::cmd::camera::{STYLES, style_of};

const TILE: egui::Vec2 = vec2(34.0, 34.0);

pub(super) fn styles_section(app: &mut SlideApp, ui: &mut Ui) {
    // A tile is selected only when every selected Cameo has the same style: a mixed selection
    // shows none, instead of the first shape's style standing for all of them.
    let current = app.session.active().and_then(|d| {
        let selected = d.selected_shapes();
        let mut styles = selected.iter().filter(|sh| sh.is_cameo()).filter_map(|sh| style_of(sh));
        let first = styles.next()?;
        styles.all(|s| s == first).then_some(first)
    });
    let selected = current.and_then(|id| STYLES.iter().position(|s| s.id == id));
    group(ui, |ui| {
        let labels: Vec<&str> = STYLES.iter().map(|s| s.label).collect();
        // The gallery follows the command: a mixed selection or no Cameo greys it out instead of
        // letting a tile report an error.
        let on = enabled(app, "camera.style");
        let picked = ui
            .add_enabled_ui(on, |ui| {
                widgets::gallery(ui, STYLES.len(), TILE, selected, paint_style, |i| labels.get(i).copied().unwrap_or_default().to_string())
            })
            .inner;
        if let Some(i) = picked
            && let Some(st) = STYLES.get(i)
        {
            run(app, "camera.style", json!({"style": st.id}));
        }
        rows(ui, |ui| {
            small(app, ui, Icon::ApplyToAll, "Apply to All Slides", "camera.applyToAll", json!({}));
            // These commands act on the selection, so a mixed selection must not send them to a
            // plain shape beside the Cameo.
            let cameo = enabled(app, "camera.style");
            ui.add_enabled_ui(cameo, |ui| {
                let r = widgets::small_button(ui, Icon::Shapes, "Camera Shape", true, false);
                egui::Popup::menu(&r).show(|ui| {
                    if let Some(p) = mask_gallery(ui) {
                        run(app, "shape.change", json!({"preset": p}));
                    }
                });
                let r = widgets::small_button(ui, Icon::ShapeOutline, "Camera Border", true, false);
                egui::Popup::menu(&r).show(|ui| {
                    let sc = scheme(app);
                    outline_menu(app, ui, &sc);
                });
            });
        });
        rows(ui, |ui| {
            let cameo = enabled(app, "camera.style");
            ui.add_enabled_ui(cameo, |ui| {
                let r = widgets::small_button(ui, Icon::ShapeEffects, "Camera Effects", true, false);
                egui::Popup::menu(&r).show(|ui| effects_menu(app, ui));
            });
            small(app, ui, Icon::FormatBackground, "Format Object…", "view.pane", json!({"pane": "format"}));
        });
    });
}

/// A gallery tile: the style's shape, frame and shadow over a camera-blue body. The shadow and
/// soft edge follow the same preset as the body, so an ellipse looks like an ellipse.
fn paint_style(p: &egui::Painter, r: Rect, i: usize) {
    let Some(st) = STYLES.get(i) else { return };
    let r = r.shrink(5.0);
    let body = Color32::from_rgb(0x6F, 0xB8, 0xDD);
    let stroke = if st.frame.is_some() { Color32::WHITE } else { Color32::from_gray(130) };
    // "No Style" keeps whatever Camera Shape the user chose; the tile shows the default round
    // Cameo rather than imposing a rectangle.
    let preset = if st.preset.is_empty() { "ellipse" } else { st.preset };
    if st.shadow {
        paint_preset(p, r.translate(vec2(1.5, 1.5)), preset, Color32::from_black_alpha(55), Color32::TRANSPARENT);
    }
    if st.soft_edge.is_some() {
        paint_preset(p, r.expand(2.0), preset, body.gamma_multiply(0.25), Color32::TRANSPARENT);
    }
    paint_preset(p, r, preset, body, stroke);
    // The lens, so the tile reads as a camera rather than a bare shape.
    let lens = r.center();
    let lr = (r.width().min(r.height()) * 0.18).max(1.5);
    p.add(egui::Shape::circle_filled(lens, lr, Color32::from_rgb(0x1B, 0x2A, 0x36)));
}
