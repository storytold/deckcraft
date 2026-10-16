//! Layout and render tests for the title bar.
//!
//! The core invariant: the left and right child panels must never overlap at any
//! viewport width.  The geometry is extracted into `title_bar_panel_bounds` so
//! the tests exercise the exact formula that ships, not a copy of it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use egui_kittest::kittest::Queryable as _;

use super::title_bar_panel_bounds;
use crate::{Services, SlideApp};

// ---------------------------------------------------------------- geometry (pure unit tests)

/// At every plausible viewport width the left panel's right edge must be ≤ the right
/// panel's left edge.  These are offsets relative to `rect.min.x`.
#[test]
fn title_bar_panels_never_overlap_standard_titlebar() {
    let left_offset = 10.0_f32; // integrated_titlebar = false
    for &width in &[200.0_f32, 300.0, 400.0, 472.0, 500.0, 600.0, 720.0, 800.0, 900.0, 1024.0, 1280.0, 1400.0, 1920.0] {
        let (left_max, right_min) = title_bar_panel_bounds(width, left_offset);
        assert!(left_max <= right_min, "panels overlap at width={width}: left ends at {left_max:.1}, right starts at {right_min:.1}");
        // Both panels must fit within the bar.
        assert!(left_max >= left_offset, "left panel underflows at width={width}");
        assert!(right_min <= width - 10.0, "right panel overflows at width={width}");
    }
}

/// Same invariant holds for the macOS integrated title bar (traffic-light offset = 84 px).
#[test]
fn title_bar_panels_never_overlap_integrated_titlebar() {
    let left_offset = 84.0_f32; // integrated_titlebar = true
    for &width in &[300.0_f32, 472.0, 600.0, 800.0, 1024.0, 1400.0] {
        let (left_max, right_min) = title_bar_panel_bounds(width, left_offset);
        assert!(left_max <= right_min, "panels overlap at width={width} (integrated): left ends at {left_max:.1}, right starts at {right_min:.1}");
    }
}

/// At desktop widths (≥ ~840 px) the geometry matches the original fixed values so that
/// the desktop appearance is unchanged.
#[test]
fn title_bar_panels_match_original_at_desktop_width() {
    // At 1400 px the centre stop does not engage on either side, so the results must be
    // exactly equal to the old hard-coded values.
    let (left_max, right_min) = title_bar_panel_bounds(1400.0, 10.0);
    assert_eq!(left_max, 420.0, "left panel keeps its 420-px reach at desktop width");
    assert_eq!(right_min, 1400.0 - 560.0, "right panel keeps its 560-px reach at desktop width");
}

// ---------------------------------------------------------------- render / integration tests

fn build_harness(width: f32) -> egui_kittest::Harness<'static, SlideApp> {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(width, 600.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    h.run_steps(4);
    h
}

/// The title bar renders at a narrow (472 px) viewport without panicking, and the
/// Present button — which is always essential — remains in the accessibility tree.
#[test]
fn title_bar_narrow_renders_without_panic() {
    let h = build_harness(472.0);
    // The Present button is an `egui::Button` whose text becomes its accessible label.
    // Whitespace is part of the label as egui stores it.
    assert!(h.query_by_label("  Present  ").is_some(), "the Present button must remain accessible at 472 px");
}

/// The title bar also renders cleanly at a very narrow (280 px) viewport — smaller than
/// any realistic browser tab, confirming there is no lower bound at which it panics.
#[test]
fn title_bar_very_narrow_renders_without_panic() {
    let _h = build_harness(280.0);
    // No assertion needed beyond "did not panic".
}
