//! Camera Format: the contextual tab of a Cameo (a picture with [`deckcraft_model::Shape::cameo`]
//! set). The shell owns the tab's sections; each section lives in its own module so the capture,
//! style, accessibility, arrange and size work can land independently.

mod accessibility;
mod arrange;
mod camera;
mod size;
mod styles;
use super::{SlideApp, Ui};

/// The Camera Format tab, in PowerPoint's group order.
pub(super) fn camera_format(app: &mut SlideApp, ui: &mut Ui) {
    camera::camera_section(app, ui);
    styles::styles_section(app, ui);
    accessibility::accessibility_section(app, ui);
    arrange::arrange_section(app, ui);
    size::size_section(app, ui);
}
