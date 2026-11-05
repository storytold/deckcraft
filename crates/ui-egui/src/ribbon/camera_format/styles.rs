//! Camera Format ▸ Camera Styles: the nine preset frames (shape, outline, effects), Apply to
//! All Slides, Camera Shape, Camera Border, Camera Effects and Format Object. A style is plain
//! DrawingML on the picture, so every other picture command keeps working on a styled cameo.

use super::super::galleries::{effects_menu, outline_menu, shapes_gallery};
use super::super::{
    Color32, CornerRadius, Icon, Rect, Sense, SlideApp, Stroke, Tokens, Ui, enabled, group, json, pos2, rows, run, scheme, small, small_button_ex,
    vec2, widgets,
};
use deckcraft_engine::cmd::camera::{STYLES, style_of};

const TILE: egui::Vec2 = vec2(36.0, 34.0);

pub(super) fn styles_section(app: &mut SlideApp, ui: &mut Ui) {
    // Empty means "no preset tile matches", which covers both No Style and a hand-picked
    // border or effect: highlighting No Style for those would be wrong.
    let current = app.session.active().and_then(|d| d.selected_shapes().iter().find(|sh| sh.is_cameo()).and_then(|sh| style_of(sh))).unwrap_or("");
    let en = enabled(app, "camera.style");
    // A mixed selection keeps the tab: every command below addresses the Cameos only.
    let ids = super::cameo_ids(app);
    group(ui, |ui| {
        if let Some(id) = style_gallery(ui, current, en) {
            run(app, "camera.style", json!({"style": id, "ids": ids}));
        }
        rows(ui, |ui| {
            small(app, ui, Icon::ApplyToAll, "Apply to All Slides", "camera.applyToAll", json!({"ids": ids}));
            let r = widgets::small_button(ui, Icon::Shapes, "Camera Shape", en, false);
            egui::Popup::menu(&r).show(|ui| {
                if let Some(p) = shapes_gallery(ui) {
                    // The drawing tools' gallery also lists freeform shapes; a Cameo can only take
                    // a preset `shape.change` knows.
                    submit_camera_shape(app, p, &ids);
                }
            });
            camera_border(app, ui, en, &ids);
        });
        rows(ui, |ui| {
            let r = widgets::small_button(ui, Icon::ShapeEffects, "Camera Effects", en, false);
            egui::Popup::menu(&r).show(|ui| effects_menu(app, ui, &ids));
            small(app, ui, Icon::FormatBackground, "Format Object…", "view.pane", json!({"pane": "format"}));
        });
    });
}

/// The nine styles, each a named, selectable tile. Returns the style clicked this frame.
fn style_gallery(ui: &mut Ui, current: &str, enabled: bool) -> Option<&'static str> {
    let t = Tokens::get(ui.ctx());
    let mut picked = None;
    for (i, st) in STYLES.iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(TILE, if enabled { Sense::click() } else { Sense::hover() });
        let sel = st.id == current;
        let painter = ui.painter_at(rect.expand(2.0));
        if sel || resp.hovered() {
            painter.rect_filled(rect.expand(1.0), CornerRadius::same(4), if sel { t.selected } else { t.hover });
        }
        paint_style(&painter, rect.shrink(3.0), i);
        if sel {
            painter.rect_stroke(rect.expand(1.0), CornerRadius::same(4), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
        }
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, st.label));
        if resp.on_hover_text(st.label).clicked() && enabled {
            picked = Some(st.id);
        }
    }
    picked
}

/// Camera Border is a split button, as in PowerPoint: the button re-applies the colour last
/// picked, the arrow opens Outline ▸ colour, weight and dashes.
fn camera_border(app: &mut SlideApp, ui: &mut Ui, enabled: bool, ids: &[u32]) {
    let (main, arrow) = small_button_ex(ui, Icon::ShapeOutline, "Camera Border", enabled, false, true);
    if main.clicked() {
        let c = app.last_outline.clone();
        run(app, "shape.line", json!({"color": c, "ids": ids}));
    }
    let Some(arrow) = arrow else { return };
    let sc = scheme(app);
    widgets::arrow_color_popup(&main, &arrow).show(|ui| outline_menu(app, ui, &sc, ids));
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

/// Apply a gallery pick to a shape, ignoring the freeform tools the shared gallery also lists.
pub(super) fn submit_camera_shape(app: &mut SlideApp, preset: &str, ids: &[u32]) {
    let draw_tool = matches!(preset, "curve" | "freeform" | "scribble");
    if draw_tool {
        app.set_status("Pick a shape preset for a Cameo");
        return;
    }
    run(app, "shape.change", json!({"preset": preset, "ids": ids}));
}

#[cfg(test)]
mod tests {
    use deckcraft_engine::Session;
    use deckcraft_engine::cmd::camera::{STYLES, style_of};
    use deckcraft_model::ShapeId;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use crate::{Services, SlideApp};

    /// A deck with `slides` slides, one Cameo on each, selected, on the Camera Format tab.
    fn camera_format(width: f32, slides: usize) -> (Harness<'static, SlideApp>, ShapeId) {
        let mut s = Session::with_new();
        s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
        for _ in 1..slides {
            s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        }
        let id = s.execute("insert.cameo", &json!({"slide": 0, "all": true})).unwrap()["id"].as_u64().unwrap();
        s.execute("edit.select", &json!({"ids": [id]})).unwrap();
        let app = SlideApp::new(s, Services::default());
        let mut h = Harness::builder().with_size(egui::vec2(width, 900.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.state_mut().ui.tab = "Camera Format".into();
        h.run_steps(4);
        (h, ShapeId(u32::try_from(id).unwrap()))
    }

    fn cameos(app: &SlideApp) -> Vec<deckcraft_model::Shape> {
        let Some(doc) = app.session.active() else { return vec![] };
        doc.doc
            .slides
            .iter()
            .flat_map(|sl| {
                let mut v = vec![];
                deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
                    if sh.is_cameo() {
                        v.push(sh.clone());
                    }
                });
                v
            })
            .collect()
    }

    #[test]
    fn the_gallery_names_all_nine_presets_in_powerpoint_order() {
        let (h, _) = camera_format(1920.0, 1);
        let names: Vec<&str> = STYLES.iter().map(|s| s.label).collect();
        assert_eq!(
            names,
            [
                "No Style",
                "Simple Frame Rectangle",
                "Center Shadow Rectangle",
                "Soft Edge Rectangle",
                "Simple Frame Circle",
                "Center Shadow Circle",
                "Soft Edge Circle",
                "Center Shadow Diamond",
                "Center Shadow Hexagon",
            ]
        );
        for label in names {
            assert!(h.query_by_label(label).is_some(), "{label} is missing from Camera Styles");
        }
        for control in ["Apply to All Slides", "Camera Shape", "Camera Border", "Camera Effects", "Format Object…"] {
            assert!(h.query_by_label(control).is_some(), "{control} is missing from Camera Styles");
        }
        assert!(h.query_by_label("Remove Background").is_none(), "background segmentation is not part of Camera Styles");
    }

    #[test]
    fn apply_to_all_slides_copies_the_selected_cameos_formatting_to_every_slide() {
        let (mut h, _) = camera_format(1920.0, 3);
        h.get_by_label("Soft Edge Rectangle").click();
        h.run_steps(2);
        let undo = h.state().session.active().unwrap().history.undo.len();
        h.get_by_label("Apply to All Slides").click();
        h.run_steps(2);

        let app = h.state();
        for sh in cameos(app) {
            assert_eq!(sh.geom.preset_name(), Some("rect"));
            assert_eq!(sh.effects.as_ref().and_then(|e| e.soft_edge), Some(10.0));
            assert_eq!(style_of(&sh), Some("softEdgeRectangle"));
        }
        assert_eq!(h.state().session.active().unwrap().history.undo.len(), undo + 1, "one undo step for all slides");
    }

    #[test]
    fn the_camera_border_button_reapplies_the_last_outline_colour() {
        let (mut h, id) = camera_format(1920.0, 1);
        h.state_mut().last_outline = json!("#c00000");
        h.get_by_label("Camera Border").click();
        h.run_steps(2);
        let doc = h.state().session.active().unwrap();
        let sh = doc.shape(id).unwrap();
        assert!(sh.line.as_ref().and_then(|l| l.width).is_some(), "Camera Border gives the cameo a frame");
        assert_eq!(style_of(sh), None, "a hand-picked border is not one of the presets");
    }
}
