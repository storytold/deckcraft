//! Camera Format: the contextual tab of a Cameo (a picture with
//! [`deckcraft_model::Shape::cameo`] set). This PR owns Camera Background; capture and the other
//! Camera sections are separate work.

mod background;

use super::{SlideApp, Ui};

/// The Cameos in the selection, in selection order. The tab also shows for a mixed selection, so
/// each command addresses these ids instead of every selected shape.
pub(super) fn cameo_ids(app: &SlideApp) -> Vec<u32> {
    app.session.active().map(|st| st.selected_shapes().iter().filter(|s| s.is_cameo()).map(|s| s.id.0).collect()).unwrap_or_default()
}

/// The Camera Format tab.
pub(super) fn camera_format(app: &mut SlideApp, ui: &mut Ui) {
    background::background_section(app, ui);
}
