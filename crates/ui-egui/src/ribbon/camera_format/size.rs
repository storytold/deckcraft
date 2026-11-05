//! Camera Format ▸ Size: Crop, the Cameo Height and Width spinners and Size and Position.
//!
//! A Cameo is a picture, so both controls reuse the picture commands: `picture.crop` writes the
//! `srcRect` crop and `shape.resize` writes the frame's `xfrm`. Neither touches the other, which
//! keeps the live bubble transform intact when the feed is cropped and vice versa. PowerPoint
//! writes a Cameo with `a:picLocks/@noChangeAspect`, so the height and width spinners keep the
//! frame's aspect ratio and the Size and Position dialog opens with "Lock aspect ratio" ticked.

use super::super::galleries::shapes_gallery;
use super::super::{Icon, SlideApp, Ui, big_button, enabled, group, json, rows, run, widgets};

/// Smallest frame side the spinners allow, in points: below this a Cameo cannot be grabbed again.
const MIN_SIDE: f64 = 1.0;

/// The first selected Cameo's effective frame: the tab also shows for a mixed selection, and the
/// spinners must show the same object they resize.
fn selected_xfrm(app: &SlideApp) -> Option<deckcraft_geom::Xfrm> {
    let st = app.session.active()?;
    let id = st.selected_shapes().iter().find(|s| s.is_cameo())?.id;
    let sh = st.shape(id)?;
    Some(deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh))
}

pub(super) fn size_section(app: &mut SlideApp, ui: &mut Ui) {
    let xfrm = selected_xfrm(app);
    let ids = super::cameo_ids(app);
    group(ui, |ui| {
        let r = big_button(ui, Icon::Crop, "Crop", enabled(app, "picture.crop"));
        egui::Popup::menu(&r).show(|ui| {
            ui.label("Crop to Shape");
            if let Some(p) = shapes_gallery(ui) {
                // The gallery lists the drawing tools too; `shape.change` only knows presets.
                super::styles::submit_camera_shape(app, p, &ids);
            }
            ui.separator();
            for (label, v) in [("Crop 10% each side", 0.1), ("Crop 20% each side", 0.2), ("Reset Crop", 0.0)] {
                if ui.button(label).clicked() {
                    run(app, "picture.crop", json!({"left": v, "top": v, "right": v, "bottom": v, "ids": ids}));
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
                    // A zero side would collapse the frame; keep it above the smallest usable one.
                    run(app, "shape.resize", json!({"h": (v * ppu).max(MIN_SIDE), "lockAspect": true, "ids": ids}));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Width: ");
                if let Some(v) = widgets::spinner(ui, "cw", "Width", x.w / ppu, 0.1, suffix, width) {
                    run(app, "shape.resize", json!({"w": (v * ppu).max(MIN_SIDE), "lockAspect": true, "ids": ids}));
                }
            });
            if ui.add_enabled(xfrm.is_some(), egui::Button::new("Size and Position…")).clicked() {
                // A Cameo's aspect is locked in the file; open the dialog that way.
                run(app, "shape.sizePosition", json!({"lockAspect": true, "ids": ids}));
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

    /// A blank deck with a text box selected first and a Cameo behind it, on the Camera Format tab:
    /// the tab shows for the Cameo, but the selection starts with another kind of shape.
    fn mixed_app(width: f32) -> (Harness<'static, SlideApp>, ShapeId, ShapeId) {
        let mut s = Session::with_new();
        s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
        let text = s.execute("insert.textBox", &json!({"rect": [500, 400, 200, 80], "text": "Hello"})).unwrap()["id"].as_u64().unwrap();
        let cameo = s.execute("insert.cameo", &json!({"rect": [200, 100, 162, 162]})).unwrap()["id"].as_u64().unwrap();
        s.execute("edit.select", &json!({"ids": [text, cameo]})).unwrap();
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
        (h, ShapeId(u32::try_from(text).unwrap()), ShapeId(u32::try_from(cameo).unwrap()))
    }

    #[test]
    fn a_mixed_selection_resizes_only_the_cameo() {
        let (mut h, text, cameo) = mixed_app(1600.0);
        let (text_before, cameo_before) = (xfrm(h.state(), text), xfrm(h.state(), cameo));
        h.get_by_label("Increase Width").click();
        h.run_steps(2);
        let (text_after, cameo_after) = (xfrm(h.state(), text), xfrm(h.state(), cameo));
        assert_eq!(text_after, text_before, "the text box is not a Cameo and keeps its frame");
        assert!(cameo_after.w > cameo_before.w, "the Cameo grew: {cameo_before:?} -> {cameo_after:?}");
    }

    #[test]
    fn size_and_position_edits_only_the_cameo_of_a_mixed_selection() {
        let (mut h, text, _) = mixed_app(1600.0);
        let text_before = xfrm(h.state(), text);
        h.get_by_label("Size and Position…").click();
        h.run_steps(2);
        let d = h.state().dialog.as_ref().expect("the dialog opened");
        assert_eq!(d.params.get("ids").and_then(|v| v.as_array()).map(|a| a.len()), Some(1), "the launcher names the Cameo");
        h.get_by_label("OK").click_accesskit();
        h.run_steps(3);
        assert!(h.state().dialog.is_none(), "OK closes the dialog");
        assert_eq!(xfrm(h.state(), text), text_before, "the text box is not one of the dialog's targets");
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
