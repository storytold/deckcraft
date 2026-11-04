//! Camera Format: the contextual tab of a Cameo (a picture with [`deckcraft_model::Shape::cameo`]
//! set). The shell owns the tab's sections; each section lives in its own module so the capture,
//! style, accessibility, arrange and size work can land independently.

mod accessibility;
mod arrange;
mod camera;
mod size;
mod styles;
use super::{SlideApp, Ui};

/// The Cameos in the selection, in selection order. The tab also shows for a mixed selection, so a
/// section that formats Cameos addresses these ids instead of every selected shape.
pub(super) fn cameo_ids(app: &SlideApp) -> Vec<u32> {
    app.session.active().map(|st| st.selected_shapes().iter().filter(|s| s.is_cameo()).map(|s| s.id.0).collect()).unwrap_or_default()
}

/// The Camera Format tab, in PowerPoint's group order.
pub(super) fn camera_format(app: &mut SlideApp, ui: &mut Ui) {
    camera::camera_section(app, ui);
    styles::styles_section(app, ui);
    accessibility::accessibility_section(app, ui);
    arrange::arrange_section(app, ui);
    size::size_section(app, ui);
}
