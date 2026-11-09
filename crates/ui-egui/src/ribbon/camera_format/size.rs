//! Camera Format ▸ Size: Crop (with Crop to Shape), Height, Width and Size and Position.

use super::super::arrange::size_group;
use super::super::galleries::mask_gallery;
use super::super::{Icon, SlideApp, Ui, big_button, enabled, group, json, run};

pub(super) fn size_section(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let r = big_button(ui, Icon::Crop, "Crop", enabled(app, "picture.crop"));
        egui::Popup::menu(&r).show(|ui| {
            ui.label("Crop to Shape");
            // A mask applies to the selection: only offer it while the Cameo is the selection.
            let picked = ui.add_enabled_ui(enabled(app, "camera.style"), mask_gallery).inner;
            if let Some(p) = picked {
                run(app, "shape.change", json!({"preset": p}));
                ui.close();
            }
            ui.separator();
            ui.add_enabled_ui(enabled(app, "picture.crop"), |ui| {
                for (label, v) in [("Crop 10% each side", 0.1), ("Crop 20% each side", 0.2), ("Reset Crop", 0.0)] {
                    if ui.button(label).clicked() {
                        run(app, "picture.crop", json!({"left": v, "top": v, "right": v, "bottom": v}));
                        ui.close();
                    }
                }
            });
        });
    });
    size_group(app, ui);
}
