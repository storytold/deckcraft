//! The Record view's bottom strip: navigation, progress and the ink palette. The widget-level
//! clicks are exercised in the shell's own tests; these pin the state and geometry the strip
//! promises a section.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

/// The RGBA the strip sets on the show from a swatch.
fn rgba(c: egui::Color32) -> deckcraft_model::Rgba {
    deckcraft_model::Rgba::rgb(c.r(), c.g(), c.b())
}

fn record_view() -> RecordView {
    RecordView::default()
}

#[test]
fn the_progress_bar_counts_the_current_slide_in() {
    assert_eq!(progress_fraction(0, 0), 0.0);
    assert!((progress_fraction(0, 10) - 0.1).abs() < 1e-6);
    assert!((progress_fraction(4, 10) - 0.5).abs() < 1e-6);
    assert_eq!(progress_fraction(9, 10), 1.0);
    assert_eq!(progress_fraction(50, 10), 1.0, "a stale index cannot overfill the bar");
}

#[test]
fn previous_and_next_stop_at_the_ends() {
    assert_eq!(nav_enabled(0, 0), (false, false));
    assert_eq!(nav_enabled(0, 1), (false, false));
    assert_eq!(nav_enabled(0, 3), (false, true));
    assert_eq!(nav_enabled(1, 3), (true, true));
    assert_eq!(nav_enabled(2, 3), (true, false));
    assert_eq!(nav_enabled(9, 3), (true, false), "an index past the end never enables next");
}

#[test]
fn the_slide_count_is_one_based_and_bounded() {
    assert_eq!(slide_count_label(0, 3), "Slide 1 of 3");
    assert_eq!(slide_count_label(2, 3), "Slide 3 of 3");
    assert_eq!(slide_count_label(7, 3), "Slide 3 of 3");
    assert_eq!(slide_count_label(0, 0), "No slides");
}

#[test]
fn the_ink_tools_map_both_ways() {
    for tool in [InkTool::Arrow, InkTool::Pen, InkTool::Highlighter, InkTool::Eraser] {
        assert_eq!(ink_tool(ink_mode(tool)), tool);
    }
    assert_eq!(ink_mode(InkTool::Pen), InkMode::Pen);
    assert_eq!(ink_mode(InkTool::Highlighter), InkMode::Highlighter);
    assert_eq!(ink_mode(InkTool::Eraser), InkMode::Eraser);
    assert_eq!(ink_mode(InkTool::Arrow), InkMode::Arrow);
}

#[test]
fn pressing_the_selected_tool_puts_the_pointer_back() {
    let mut rec = record_view();
    pick_tool(&mut rec, InkTool::Pen);
    assert_eq!(rec.ink.tool, InkTool::Pen);
    pick_tool(&mut rec, InkTool::Pen);
    assert_eq!(rec.ink.tool, InkTool::Arrow, "the pen is a toggle");
    pick_tool(&mut rec, InkTool::Highlighter);
    assert_eq!(rec.ink.tool, InkTool::Highlighter);
    pick_tool(&mut rec, InkTool::Eraser);
    assert_eq!(rec.ink.tool, InkTool::Eraser, "a different tool replaces the highlighter");
}

#[test]
fn a_colour_starts_the_pen_but_keeps_the_highlighter() {
    let mut rec = record_view();
    assert_eq!(rec.ink.tool, InkTool::Arrow);
    pick_color(&mut rec, 3);
    assert_eq!(rec.ink.color, 3);
    assert_eq!(rec.ink.tool, InkTool::Pen, "a colour press with the arrow out starts the pen");

    pick_tool(&mut rec, InkTool::Highlighter);
    pick_color(&mut rec, 5);
    assert_eq!(rec.ink.tool, InkTool::Highlighter, "the highlighter keeps its tool");
    assert_eq!(rec.ink.color, 5);

    pick_tool(&mut rec, InkTool::Eraser);
    pick_color(&mut rec, 8);
    assert_eq!(rec.ink.tool, InkTool::Pen, "the eraser gives way to the chosen pen colour");
    assert_eq!(rec.ink.color, 8);
}

#[test]
fn a_colour_outside_the_palette_changes_nothing() {
    let mut rec = record_view();
    rec.ink.color = 4;
    pick_color(&mut rec, INK_COLORS.len());
    assert_eq!(rec.ink.color, 4);
    assert_eq!(rec.ink.tool, InkTool::Arrow);
}

#[test]
fn the_palette_has_ten_distinct_opaque_colours() {
    assert_eq!(INK_COLORS.len(), 10);
    assert_eq!(INK_NAMES.len(), 10);
    for (i, swatch) in INK_COLORS.iter().enumerate() {
        assert_eq!(swatch.a(), 255, "swatches paint solid");
        for (j, other) in INK_COLORS.iter().enumerate().skip(i + 1) {
            assert_ne!(swatch, other, "swatch {i} and {j} are the same colour");
        }
    }
}

#[test]
fn the_strip_keeps_every_control_inside_its_slot() {
    let bottom = Rect::from_min_size(pos2(0.0, 900.0), vec2(1280.0, 96.0));
    let r = rects(bottom, 200.0);
    for rect in [r.prev, r.next, r.count, r.progress, r.tools, r.swatches] {
        assert!(bottom.contains_rect(rect), "{rect:?} escapes the bottom strip {bottom:?}");
    }
    assert!(r.prev.max.x <= r.next.min.x, "previous and next do not overlap");
    assert!(r.next.max.x <= r.count.min.x);
    assert!(r.count.max.x <= r.progress.min.x, "the count clears the progress bar");
    assert!(r.progress.max.x <= r.tools.min.x, "the progress bar clears the ink tools");
    assert!(r.tools.max.x < r.swatches.min.x, "tools and colours are separated");
    assert!(r.swatches.max.x <= bottom.max.x - 200.0, "the Views slot stays clear");
    let last = r.swatch(INK_COLORS.len() - 1);
    assert!(last.max.x <= r.swatches.max.x + 0.5, "the tenth swatch fits the colour run");
    assert!(r.tool(0).min.x >= r.tools.min.x);
    assert!(r.tool(2).max.x <= r.tools.max.x + 0.5);
}

#[test]
fn a_narrow_window_drops_the_progress_bar_instead_of_overlapping() {
    let bottom = Rect::from_min_size(pos2(0.0, 500.0), vec2(700.0, 96.0));
    let r = rects(bottom, 200.0);
    assert_eq!(r.progress.width(), 0.0, "no room left: the bar collapses");
    assert!(r.swatches.max.x <= bottom.max.x - 200.0 + 0.5);
    assert!(r.next.max.x <= r.count.min.x);
}

#[test]
fn the_strip_stays_centred_when_the_strip_is_taller() {
    let bottom = Rect::from_min_size(pos2(10.0, 700.0), vec2(1200.0, 120.0));
    let r = rects(bottom, 220.0);
    assert!((r.prev.center().y - bottom.center().y).abs() < 0.5);
    assert!(r.prev.min.x >= bottom.min.x);
}

// ---------- Widget-level clicks ----------

use super::super::super::{InkMode, InkStroke, Show};
use super::super::layout as record_layout;
use super::super::{InkTool, Layout, RecordView};
use egui::accesskit::Toggled;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

type State = (Show, RecordView, std::sync::Arc<deckcraft_model::Presentation>, Layout, crate::show::MicSlot);

fn harness() -> Harness<'static, State> {
    let mut session = deckcraft_engine::Session::with_new();
    session.execute("slide.new", &serde_json::json!({})).unwrap();
    let doc = session.active().unwrap().doc.clone();
    let show = Show::new(&doc, 0, false, false);
    let rec = RecordView::default();
    let layout = record_layout(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 700.0)), &doc, &rec);
    let mut h = Harness::builder().with_size(vec2(1280.0, 700.0)).build_ui_state(
        |ui, st: &mut State| {
            let (show, rec, doc, layout, mic) = st;
            bottom_ui(mic, ui, show, rec, doc, 0.0, layout);
        },
        (show, rec, doc, layout, None),
    );
    h.run_steps(2);
    h
}

/// `ui` is the section entry point; naming it keeps the closure readable and avoids a name
/// clash with the `ui` parameter of the tests.
use super::ui as bottom_ui;

#[test]
fn next_and_previous_move_the_show_and_stop_at_the_ends() {
    let mut h = harness();
    assert_eq!(h.state().0.state.slide, 0);
    assert!(h.get_by_label("Previous slide").accesskit_node().is_disabled(), "no previous slide at the start");
    h.get_by_label("Next slide").click();
    h.run();
    assert_eq!(h.state().0.state.slide, 1, "Next advanced the show");
    h.get_by_label("Next slide").click();
    h.run();
    assert_eq!(h.state().0.state.slide, 1, "Next is disabled on the last slide");
    assert!(h.get_by_label("Next slide").accesskit_node().is_disabled());
    h.get_by_label("Previous slide").click();
    h.run();
    assert_eq!(h.state().0.state.slide, 0, "Previous went back");
}

#[test]
fn the_pen_and_highlighter_are_toggles_and_drive_the_show() {
    let mut h = harness();
    assert_eq!(h.state().0.ink_mode(), InkMode::Arrow);
    h.get_by_label("Pen").click();
    h.run();
    assert_eq!(h.state().1.ink.tool, InkTool::Pen);
    assert_eq!(h.state().0.ink_mode(), InkMode::Pen, "the canvas and recording see the pen");
    assert_eq!(h.get_by_label("Pen").accesskit_node().toggled(), Some(Toggled::True), "the pen reads as selected");
    h.get_by_label("Highlighter").click();
    h.run();
    assert_eq!(h.state().1.ink.tool, InkTool::Highlighter);
    assert_eq!(h.state().0.ink_mode(), InkMode::Highlighter);
    assert_eq!(h.get_by_label("Pen").accesskit_node().toggled(), Some(Toggled::False), "only one tool is active");
    h.get_by_label("Highlighter").click();
    h.run();
    assert_eq!(h.state().1.ink.tool, InkTool::Arrow, "pressing the active highlighter puts it away");
}

#[test]
fn the_eraser_is_selectable_with_or_without_ink() {
    let mut h = harness();
    assert!(!h.get_by_label("Eraser").accesskit_node().is_disabled(), "the eraser is always available");
    h.get_by_label("Eraser").click();
    h.run();
    assert_eq!(h.state().1.ink.tool, InkTool::Eraser);
    assert_eq!(h.state().0.ink_mode(), InkMode::Eraser);
    assert!(!h.state().0.pen, "the eraser leaves the pen capture off");
    assert!(!h.state().0.has_ink(), "there was nothing to erase");
    h.state_mut().0.strokes.push(InkStroke {
        points: vec![pos2(0.2, 0.2), pos2(0.3, 0.2)],
        mode: InkMode::Pen,
        color: deckcraft_model::Rgba::BLACK,
        width: 2.25,
        in_take: true,
    });
    h.run();
    assert!(h.state().0.has_ink(), "a stroke is available to the eraser");
}

#[test]
fn a_colour_click_selects_the_swatch_and_starts_the_pen() {
    let mut h = harness();
    h.get_by_label("Ink colour 4").click();
    h.run();
    assert_eq!(h.state().1.ink.color, 3);
    assert_eq!(h.state().1.ink.tool, InkTool::Pen, "the arrow gives way to the chosen pen");
    assert_eq!(h.state().0.ink_color(), rgba(INK_COLORS[3]));
    assert_eq!(h.get_by_label("Ink colour 4").accesskit_node().toggled(), Some(Toggled::True));
    assert_eq!(h.get_by_label("Ink colour 9").accesskit_node().toggled(), Some(Toggled::False));
    h.get_by_label("Ink colour 9").click();
    h.run();
    assert_eq!(h.state().1.ink.color, 8);
    assert_eq!(h.get_by_label("Ink colour 4").accesskit_node().toggled(), Some(Toggled::False));
    assert_eq!(h.get_by_label("Ink colour 9").accesskit_node().toggled(), Some(Toggled::True));
}

#[test]
fn a_colour_keeps_the_highlighter_selected() {
    let mut h = harness();
    h.get_by_label("Highlighter").click();
    h.run();
    h.get_by_label("Ink colour 6").click();
    h.run();
    assert_eq!(h.state().1.ink.tool, InkTool::Highlighter);
    assert_eq!(h.state().1.ink.color, 5);
    assert_eq!(h.state().0.ink_color(), rgba(INK_COLORS[5]));
}
