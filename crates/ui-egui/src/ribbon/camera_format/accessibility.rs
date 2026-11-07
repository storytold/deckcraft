//! Camera Format ▸ Accessibility: the Alt Text button opens the pane where the cameo's
//! description is written. The description is the picture's `descr`, so it round trips and is
//! read by screen readers like any other object's.

use super::super::{Icon, SlideApp, Ui, big, group, json};

pub(super) fn accessibility_section(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        // PowerPoint's button opens the pane beside the slide; a second press closes it.
        big(app, ui, Icon::AltText, "Alt\nText", "view.pane", json!({"pane": "altText", "toggle": true}));
    });
}
