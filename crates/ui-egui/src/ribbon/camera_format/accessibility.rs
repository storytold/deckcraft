//! Camera Format ▸ Accessibility.

use super::super::{Icon, SlideApp, Ui, big, group, json};

pub(super) fn accessibility_section(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        big(app, ui, Icon::AltText, "Alt\nText", "app.dialog", json!({"id": "altText"}));
    });
}
