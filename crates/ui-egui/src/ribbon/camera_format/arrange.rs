//! Camera Format ▸ Arrange: the picture/shape ordering, align, group and rotate controls.

use super::super::arrange::arrange_group;
use super::super::{SlideApp, Ui};

pub(super) fn arrange_section(app: &mut SlideApp, ui: &mut Ui) {
    arrange_group(app, ui);
}
