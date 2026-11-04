//! Scrolling of a long notes script.
//!
//! The pane keeps one vertical offset so a long script survives the pane being resized or the
//! header being redrawn. The offset is clamped to the content whenever the script gets shorter
//! (a slide change, an edit that deletes text) so the view cannot be left scrolled past the end.

use egui::scroll_area::ScrollBarVisibility;
use egui::{Rect, Ui};

/// Sanitize a stored scroll offset.
///
/// A missing, corrupt or negative offset reads as the top of the script.
pub(super) fn sanitize(offset: f32) -> f32 {
    if offset.is_finite() && offset > 0.0 { offset } else { 0.0 }
}

/// Keep an offset inside the script: never above the content, and `0` when it all fits.
pub(super) fn clamp_offset(offset: f32, content: f32, viewport: f32) -> f32 {
    let offset = sanitize(offset);
    let max = (content - viewport).max(0.0);
    offset.min(max)
}

/// Draw the script in the pane's scroll area and return the offset that was really shown.
///
/// `offset` is the offset from the previous frame, `viewport` the body rect the pane has for the
/// script. The caller stores the returned value so the next frame resumes where this one ended.
pub(super) fn pane(ui: &mut Ui, offset: f32, viewport: Rect, add: impl FnOnce(&mut Ui)) -> f32 {
    let height = viewport.height().max(0.0);
    let out = egui::ScrollArea::vertical()
        .id_salt("record_notes_scroll")
        .max_height(height)
        .auto_shrink([false, false])
        .scroll_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded)
        .vertical_scroll_offset(sanitize(offset))
        .show(ui, add);
    clamp_offset(out.state.unclamped_offset().y, out.content_size.y, height)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use egui::{Rect, pos2, vec2};

    use super::{clamp_offset, pane, sanitize};

    #[test]
    fn sanitize_drops_invalid_offsets() {
        assert_eq!(sanitize(f32::NAN), 0.0);
        assert_eq!(sanitize(f32::INFINITY), 0.0);
        assert_eq!(sanitize(f32::NEG_INFINITY), 0.0);
        assert_eq!(sanitize(-12.0), 0.0);
        assert_eq!(sanitize(-0.0), 0.0);
        assert_eq!(sanitize(0.0), 0.0);
        assert_eq!(sanitize(42.5), 42.5);
    }

    #[test]
    fn clamp_offset_stays_inside_the_script() {
        assert_eq!(clamp_offset(30.0, 800.0, 200.0), 30.0, "a reachable offset is kept");
        assert_eq!(clamp_offset(900.0, 800.0, 200.0), 600.0, "past the end is pulled back to the last screen");
        assert_eq!(clamp_offset(-5.0, 800.0, 200.0), 0.0, "negative offsets read as the top");
        assert_eq!(clamp_offset(f32::NAN, 800.0, 200.0), 0.0, "a corrupt offset reads as the top");
        assert_eq!(clamp_offset(30.0, 120.0, 200.0), 0.0, "a script that fits is never scrolled");
        assert_eq!(clamp_offset(30.0, 200.0, 200.0), 0.0, "a script exactly the viewport's height is never scrolled");
    }

    #[test]
    fn a_long_script_keeps_the_offset_between_frames() {
        let ctx = egui::Context::default();
        let viewport = Rect::from_min_size(pos2(0.0, 0.0), vec2(240.0, 120.0));
        let mut stored = None;
        ctx.run_ui(Default::default(), |ui| {
            let mut body = ui.new_child(egui::UiBuilder::new().max_rect(viewport));
            stored = Some(pane(&mut body, 30.0, viewport, |ui| {
                for line in 0..40 {
                    ui.label(format!("line {line}"));
                }
            }));
        })
        .drop_without_applying_deltas();
        let stored = stored.expect("pane ran");
        assert!(stored > 0.0, "the requested offset is kept: {stored}");
        assert!(stored <= 30.0 + 0.01, "the offset is not moved forward: {stored}");
    }

    #[test]
    fn a_short_script_reports_no_offset() {
        let ctx = egui::Context::default();
        let viewport = Rect::from_min_size(pos2(0.0, 0.0), vec2(240.0, 300.0));
        let mut stored = None;
        ctx.run_ui(Default::default(), |ui| {
            let mut body = ui.new_child(egui::UiBuilder::new().max_rect(viewport));
            stored = Some(pane(&mut body, 50.0, viewport, |ui| {
                ui.label("one line");
                ui.label("two lines");
            }));
        })
        .drop_without_applying_deltas();
        assert_eq!(stored.expect("pane ran"), 0.0);
    }
}
