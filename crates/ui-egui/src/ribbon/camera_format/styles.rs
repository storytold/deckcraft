//! Camera Format ▸ Camera Styles: the nine preset frames (shape, outline, effects), Camera Shape,
//! Camera Border, Camera Effects and Format Object. A style is plain DrawingML on the picture, so
//! every other picture command keeps working on a styled cameo.

use super::super::galleries::{effects_menu, outline_menu, shapes_gallery};
use super::super::{Color32, CornerRadius, Icon, Rect, SlideApp, Stroke, Ui, group, json, pos2, rows, run, scheme, small, vec2, widgets};
use deckcraft_engine::cmd::camera::{STYLES, style_of};

const TILE: egui::Vec2 = vec2(34.0, 34.0);

pub(super) fn styles_section(app: &mut SlideApp, ui: &mut Ui) {
    // `style_of` is `None` when Camera Border or Camera Effects made the Cameo match no preset:
    // leave the gallery unselected then, instead of lighting up the "No Style" tile.
    let current = app.session.active().and_then(|d| d.selected_shapes().first().and_then(|sh| style_of(sh)));
    let selected = current.and_then(|id| STYLES.iter().position(|s| s.id == id));
    group(ui, |ui| {
        let labels: Vec<&str> = STYLES.iter().map(|s| s.label).collect();
        if let Some(i) = widgets::gallery(ui, STYLES.len(), TILE, selected, paint_style, |i| labels.get(i).copied().unwrap_or_default().to_string())
            && let Some(st) = STYLES.get(i)
        {
            run(app, "camera.style", json!({"style": st.id}));
        }
        rows(ui, |ui| {
            small(app, ui, Icon::ApplyToAll, "Apply to All Slides", "camera.applyToAll", json!({}));
            let r = widgets::small_button(ui, Icon::Shapes, "Camera Shape", true, false);
            egui::Popup::menu(&r).show(|ui| {
                if let Some(p) = shapes_gallery(ui) {
                    run(app, "shape.change", json!({"preset": p}));
                }
            });
            let r = widgets::small_button(ui, Icon::ShapeOutline, "Camera Border", true, false);
            egui::Popup::menu(&r).show(|ui| {
                let sc = scheme(app);
                outline_menu(app, ui, &sc);
            });
        });
        rows(ui, |ui| {
            let r = widgets::small_button(ui, Icon::ShapeEffects, "Camera Effects", true, false);
            egui::Popup::menu(&r).show(|ui| effects_menu(app, ui));
            small(app, ui, Icon::FormatBackground, "Format Object…", "view.pane", json!({"pane": "format"}));
        });
    });
}

/// A gallery tile: the style's shape, frame and shadow over a camera-blue body.
fn paint_style(p: &egui::Painter, r: Rect, i: usize) {
    let Some(st) = STYLES.get(i) else { return };
    let r = r.shrink(5.0);
    let body = Color32::from_rgb(0x6F, 0xB8, 0xDD);
    let stroke = st.frame.map_or(Stroke::new(0.8, Color32::from_gray(130)), |w| Stroke::new((w as f32 / 3.0).clamp(1.0, 3.0), Color32::WHITE));
    if st.shadow {
        p.rect_filled(r.translate(vec2(1.5, 1.5)), CornerRadius::same(2), Color32::from_black_alpha(55));
    }
    if st.soft_edge.is_some() {
        p.rect_filled(r.expand(2.0), CornerRadius::same(6), body.gamma_multiply(0.25));
    }
    match st.preset {
        "ellipse" => {
            p.add(egui::Shape::ellipse_filled(r.center(), r.size() / 2.0, body));
            p.add(egui::Shape::ellipse_stroke(r.center(), r.size() / 2.0, stroke));
        }
        "diamond" => {
            let c = r.center();
            let pts = vec![pos2(c.x, r.top()), pos2(r.right(), c.y), pos2(c.x, r.bottom()), pos2(r.left(), c.y)];
            p.add(egui::Shape::convex_polygon(pts, body, stroke));
        }
        "hexagon" => {
            let c = r.center();
            let rad = r.width().min(r.height()) / 2.0;
            let pts = (0..6)
                .map(|k| {
                    let a = std::f32::consts::TAU * k as f32 / 6.0;
                    pos2(c.x + rad * a.cos(), c.y + rad * a.sin())
                })
                .collect();
            p.add(egui::Shape::convex_polygon(pts, body, stroke));
        }
        _ => {
            p.rect_filled(r, CornerRadius::same(2), body);
            p.rect_stroke(r, CornerRadius::same(2), stroke, egui::StrokeKind::Middle);
        }
    }
    // The lens, so the tile reads as a camera rather than a bare shape.
    let lens = r.center();
    let lr = (r.width().min(r.height()) * 0.18).max(1.5);
    p.add(egui::Shape::circle_filled(lens, lr, Color32::from_rgb(0x1B, 0x2A, 0x36)));
}
