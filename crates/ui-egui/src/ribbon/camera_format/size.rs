//! Camera Format ▸ Size: Crop, the Cameo Height and Width spinners and Size and Position.
//!
//! A Cameo is a picture, so both controls reuse the picture commands: `picture.crop` writes the
//! `srcRect` crop and `shape.resize` writes the frame's `xfrm`. Neither touches the other, which
//! keeps the live bubble transform intact when the feed is cropped and vice versa. PowerPoint
//! writes a Cameo with `a:picLocks/@noChangeAspect`, so the height and width spinners keep the
//! frame's aspect ratio and the Size and Position dialog opens with "Lock aspect ratio" ticked.

use super::super::galleries::shapes_gallery;
use super::super::{Icon, SlideApp, Ui, big_button, enabled, group, json, rows, run, widgets};

/// The selected shape's effective frame, if there is a selection.
fn selected_xfrm(app: &SlideApp) -> Option<deckcraft_geom::Xfrm> {
    let st = app.session.active()?;
    let id = *st.selection.shapes.first()?;
    let sh = st.shape(id)?;
    Some(deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh))
}

pub(super) fn size_section(app: &mut SlideApp, ui: &mut Ui) {
    let xfrm = selected_xfrm(app);
    group(ui, |ui| {
        let r = big_button(ui, Icon::Crop, "Crop", enabled(app, "picture.crop"));
        egui::Popup::menu(&r).show(|ui| {
            ui.label("Crop to Shape");
            if let Some(p) = shapes_gallery(ui) {
                run(app, "shape.change", json!({"preset": p}));
            }
            ui.separator();
            for (label, v) in [("Crop 10% each side", 0.1), ("Crop 20% each side", 0.2), ("Reset Crop", 0.0)] {
                if ui.button(label).clicked() {
                    run(app, "picture.crop", json!({"left": v, "top": v, "right": v, "bottom": v}));
                }
            }
        });
        rows(ui, |ui| {
            let Some(x) = xfrm else {
                ui.label("Height:");
                ui.label("Width:");
                return;
            };
            let len = app.session.prefs.units;
            let (ppu, suffix) = (len.points_per_unit(), len.suffix());
            // " cm" needs more room than the inch mark.
            let width = if len == deckcraft_geom::Unit::Inches { 54.0 } else { 66.0 };
            ui.horizontal(|ui| {
                ui.label("Height:");
                if let Some(v) = widgets::spinner(ui, "ch", "Height", x.h / ppu, 0.1, suffix, width) {
                    run(app, "shape.resize", json!({"h": v.max(0.0) * ppu, "lockAspect": true}));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Width: ");
                if let Some(v) = widgets::spinner(ui, "cw", "Width", x.w / ppu, 0.1, suffix, width) {
                    run(app, "shape.resize", json!({"w": v.max(0.0) * ppu, "lockAspect": true}));
                }
            });
            if ui.add_enabled(xfrm.is_some(), egui::Button::new("Size and Position…")).clicked() {
                // A Cameo's aspect is locked in the file; open the dialog that way.
                run(app, "shape.sizePosition", json!({"lockAspect": true}));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use deckcraft_engine::Session;
    use deckcraft_model::ShapeId;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use crate::{Services, SlideApp};

    /// A blank deck with one selected Cameo, on the Camera Format tab.
    fn cameo_app(width: f32) -> (Harness<'static, SlideApp>, ShapeId) {
        let mut s = Session::with_new();
        s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
        let id = s.execute("insert.cameo", &json!({"rect": [200, 100, 162, 162]})).unwrap()["id"].as_u64().unwrap();
        s.execute("edit.select", &json!({"ids": [id]})).unwrap();
        let mut app = SlideApp::new(s, Services::default());
        app.ui.tab = "Camera Format".into();
        let mut h = Harness::builder().with_size(egui::vec2(width, 900.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        (h, ShapeId(u32::try_from(id).unwrap()))
    }

    fn xfrm(app: &SlideApp, id: ShapeId) -> deckcraft_geom::Xfrm {
        app.session.active().and_then(|st| st.shape(id)).and_then(|s| s.xfrm).unwrap()
    }

    #[test]
    fn the_size_group_has_crop_height_width_and_size_and_position() {
        let (h, _) = cameo_app(1600.0);
        for label in ["Crop", "Height value", "Width value", "Size and Position…"] {
            assert!(h.query_by_label(label).is_some(), "{label} missing");
        }
    }

    #[test]
    fn the_height_spinner_keeps_the_cameo_aspect_ratio() {
        let (mut h, id) = cameo_app(1600.0);
        let before = xfrm(h.state(), id);
        h.get_by_label("Increase Height").click();
        h.run_steps(2);
        let after = xfrm(h.state(), id);
        assert!(after.h > before.h, "height grew: {before:?} -> {after:?}");
        assert!((after.w - after.h).abs() < 1e-6, "a square Cameo stays square, got {} x {}", after.w, after.h);
        assert_eq!((after.x, after.y), (before.x, before.y), "the live bubble keeps its place");
    }

    #[test]
    fn the_width_spinner_keeps_the_cameo_aspect_ratio() {
        let (mut h, id) = cameo_app(1600.0);
        let before = xfrm(h.state(), id);
        h.get_by_label("Increase Width").click();
        h.run_steps(2);
        let after = xfrm(h.state(), id);
        assert!((after.w - after.h).abs() < 1e-6, "a square Cameo stays square, got {} x {}", after.w, after.h);
        assert_eq!((after.x, after.y), (before.x, before.y));
    }

    #[test]
    fn size_and_position_opens_with_the_aspect_locked() {
        let (mut h, _) = cameo_app(1600.0);
        h.get_by_label("Size and Position…").click();
        h.run_steps(2);
        let d = h.state().dialog.as_ref().expect("the dialog opened");
        assert_eq!(d.id, "sizePosition");
        assert_eq!(d.fields.get("lock").map(String::as_str), Some("1"));
    }
}
