//! Camera Format ▸ Camera: the preview controls. Live capture lands with the capture section, so
//! both buttons stay disabled here and say why.

use super::super::{Icon, SlideApp, Ui, big_button, group};

pub(super) fn camera_section(_app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        big_button(ui, Icon::Cameo, "Enable\nCamera\nPreview", false).on_hover_text("Live camera preview arrives with camera capture");
        big_button(ui, Icon::Preview, "More\nOptions", false).on_hover_text("Camera preview options arrive with camera capture");
    });
}
