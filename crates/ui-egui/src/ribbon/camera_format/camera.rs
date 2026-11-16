//! Camera Format ▸ Camera: the live Cameo preview.
//!
//! The Preview toggle is the only thing that enumerates devices, so it is what asks the OS for
//! permission. It stays enabled whenever the host injected a capture backend; the tooltip says
//! what the session is doing (off, opening, live, or why it failed).

use super::super::{Icon, SlideApp, Ui, big_button, big_button_ex, group};

pub(super) fn camera_section(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let available = app.camera_available();
        let on = app.camera_preview_on();
        let (preview, _) = big_button_ex(ui, Icon::Cameo, "Enable\nCamera\nPreview", available, false, on);
        let preview = preview.on_hover_text(app.camera_status_text());
        if preview.clicked() {
            app.toggle_camera_preview();
        }
        big_button(ui, Icon::Preview, "More\nOptions", false).on_hover_text("Camera preview options are not available yet");
    });
}
