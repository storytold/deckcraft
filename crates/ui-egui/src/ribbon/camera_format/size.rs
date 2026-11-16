//! Camera Format ▸ Size: Crop (with Crop to Shape), Height, Width and Size and Position.

use super::super::arrange::size_group;
use super::super::galleries::shapes_gallery;
use super::super::{Icon, SlideApp, Ui, big_button, enabled, group, json, run};

pub(super) fn size_section(app: &mut SlideApp, ui: &mut Ui) {
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
    });
    size_group(app, ui);
}
