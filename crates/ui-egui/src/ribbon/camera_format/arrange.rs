//! Camera Format ▸ Arrange: the picture/shape ordering, align, group and rotate controls.
//!
//! A Cameo is an ordinary picture shape with a live-feed marker, so the section is the same
//! Arrange group as Picture Format's (PowerPoint shows the same group on both tabs) and runs the
//! shared `arrange.*` commands on the current selection. A multi-selection that holds a Cameo
//! arranges every selected object, as PowerPoint does.

use super::super::arrange::arrange_group;
use super::super::{SlideApp, Ui};

pub(super) fn arrange_section(app: &mut SlideApp, ui: &mut Ui) {
    arrange_group(app, ui);
}

#[cfg(test)]
mod tests;
