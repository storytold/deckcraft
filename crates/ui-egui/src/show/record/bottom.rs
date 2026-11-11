//! The Record view's bottom strip: previous/next, the slide count and progress bar, and the ink
//! tools (pen, highlighter, eraser) with PowerPoint's ten recording colours.
//!
//! The strip only paints and edits [`InkState`]; the section owns no capture. Every click that
//! changes the tool also updates the show's pointer through [`Show::set_ink_mode`] and
//! [`Show::set_ink_color`], so the canvas and the recording read one state.

use std::sync::Arc;

use deckcraft_model::Presentation;
use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};

use super::super::{InkMode, Show};
use super::{InkTool, Layout, RecordView};
use crate::theme;

/// The ten ink colours of PowerPoint's recording toolbar, in swatch order.
pub(super) const INK_COLORS: [Color32; 10] = [
    Color32::from_rgb(0, 0, 0),
    Color32::from_rgb(255, 255, 255),
    Color32::from_rgb(127, 127, 127),
    Color32::from_rgb(224, 58, 58),
    Color32::from_rgb(242, 140, 40),
    Color32::from_rgb(255, 212, 0),
    Color32::from_rgb(76, 175, 80),
    Color32::from_rgb(22, 168, 160),
    Color32::from_rgb(46, 111, 216),
    Color32::from_rgb(142, 68, 173),
];

/// Names for the swatches' accessible labels and hover text.
const INK_NAMES: [&str; 10] = ["Black", "White", "Grey", "Red", "Orange", "Yellow", "Green", "Teal", "Blue", "Purple"];

const NAV: f32 = 30.0;
const TOOL: f32 = 30.0;
const SWATCH: f32 = 16.0;
const GAP: f32 = 4.0;
const SWATCH_GAP: f32 = 3.0;
const ROW: f32 = 34.0;

/// The show's ink mode for a toolbar choice.
pub(super) fn ink_mode(tool: InkTool) -> InkMode {
    match tool {
        InkTool::Arrow => InkMode::Arrow,
        InkTool::Pen => InkMode::Pen,
        InkTool::Highlighter => InkMode::Highlighter,
        InkTool::Eraser => InkMode::Eraser,
    }
}

/// The toolbar choice for the show's ink mode.
#[cfg(test)]
pub(super) fn ink_tool(mode: InkMode) -> InkTool {
    match mode {
        InkMode::Arrow => InkTool::Arrow,
        InkMode::Pen => InkTool::Pen,
        InkMode::Highlighter => InkTool::Highlighter,
        InkMode::Eraser => InkTool::Eraser,
    }
}

/// How much of the deck the current slide covers, 1-based: slide 0 of 10 fills a tenth.
pub(super) fn progress_fraction(slide: usize, count: usize) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let done = slide.saturating_add(1).min(count);
    (done as f32 / count as f32).clamp(0.0, 1.0)
}

/// Previous is live after the first slide; next is live before the last.
pub(super) fn nav_enabled(slide: usize, count: usize) -> (bool, bool) {
    (slide > 0, slide.saturating_add(1) < count)
}

/// The slide count label, as PowerPoint reads it.
pub(super) fn slide_count_label(slide: usize, count: usize) -> String {
    if count == 0 { "No slides".to_owned() } else { format!("Slide {} of {}", slide.saturating_add(1).min(count), count) }
}

/// Choose a colour. A colour press in PowerPoint starts the pen (or keeps the highlighter);
/// the eraser and the arrow both give way to the pen so the choice is immediately usable.
pub(super) fn pick_color(rec: &mut RecordView, index: usize) {
    if index >= INK_COLORS.len() {
        return;
    }
    rec.ink.color = index;
    if !matches!(rec.ink.tool, InkTool::Pen | InkTool::Highlighter) {
        rec.ink.tool = InkTool::Pen;
    }
}

/// Pick a tool; pressing the selected pen or highlighter puts the pointer away again (a toggle),
/// as the pen button does in a show.
pub(super) fn pick_tool(rec: &mut RecordView, tool: InkTool) {
    rec.ink.tool = if rec.ink.tool == tool { InkTool::Arrow } else { tool };
    rec.ink.active = false;
}

/// Where each control sits in the bottom strip. The right cluster is reserved for the Views
/// dropdown and camera-preview buttons, which the shell draws on the lower row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rects {
    pub prev: Rect,
    pub next: Rect,
    pub count: Rect,
    pub progress: Rect,
    pub tools: Rect,
    pub swatches: Rect,
}

impl Rects {
    pub(super) fn tool(&self, i: usize) -> Rect {
        let x = self.tools.min.x + i as f32 * (TOOL + GAP);
        Rect::from_min_size(pos2(x, self.tools.min.y), vec2(TOOL, ROW))
    }

    pub(super) fn swatch(&self, i: usize) -> Rect {
        let x = self.swatches.min.x + i as f32 * (SWATCH + SWATCH_GAP);
        Rect::from_center_size(pos2(x + SWATCH * 0.5, self.swatches.center().y), vec2(SWATCH, SWATCH))
    }
}

/// Lay the strip out left to right: navigation, count and progress on the left, the ink cluster
/// right-aligned before the reserved Views slot. A narrow window drops the progress bar rather
/// than overlapping controls.
pub(super) fn rects(bottom: Rect, reserve_right: f32) -> Rects {
    let y = bottom.center().y - ROW * 0.5;
    let left = bottom.min.x;
    let right = (bottom.max.x - reserve_right).max(left + 260.0);
    let prev = Rect::from_min_size(pos2(left, y), vec2(NAV, ROW));
    let next = Rect::from_min_size(pos2(left + NAV + GAP, y), vec2(NAV, ROW));
    let count_w = 108.0;
    let count = Rect::from_min_size(pos2(left + 2.0 * (NAV + GAP), y), vec2(count_w, ROW));
    let tools_w = 3.0 * TOOL + 2.0 * GAP;
    let swatches_w = INK_COLORS.len() as f32 * SWATCH + (INK_COLORS.len() as f32 - 1.0) * SWATCH_GAP;
    let cluster_w = tools_w + 12.0 + swatches_w;
    let cluster_left = (right - cluster_w).max(left + 2.0 * (NAV + GAP) + count_w + 8.0);
    let progress_left = count.max.x + 8.0;
    let progress_right = (cluster_left - 12.0).max(progress_left);
    // Below a readable width the bar would be a sliver: collapse it and keep the controls apart.
    let progress_w = if progress_right - progress_left >= 16.0 { progress_right - progress_left } else { 0.0 };
    let progress = Rect::from_min_size(pos2(progress_left, bottom.center().y - 3.0), vec2(progress_w, 6.0));
    let tools = Rect::from_min_size(pos2(cluster_left, y), vec2(tools_w, ROW));
    let swatches = Rect::from_min_size(pos2(cluster_left + tools_w + 12.0, y), vec2(swatches_w, ROW));
    Rects { prev, next, count, progress, tools, swatches }
}

/// Draw the strip and handle its clicks. `doc` is the live deck; the strip never captures the
/// pointer, so a press outside a control falls through to the canvas.
pub(super) fn ui(
    mic: &mut super::super::MicSlot,
    ui: &mut Ui,
    show: &mut Show,
    rec: &mut RecordView,
    doc: &Arc<Presentation>,
    now: f64,
    layout: &Layout,
) {
    let bottom = layout.bottom;
    paint_strip(ui, bottom);
    // Keep the Views/camera-preview cluster's lower-right corner clear.
    let reserve = (bottom.max.x - layout.camera_preview.min.x).max(0.0) + 16.0;
    let r = rects(bottom, reserve);
    let count = doc.slides.len();
    let (prev_on, next_on) = nav_enabled(show.state.slide, count);

    let prev = ui.interact(r.prev, ui.scope_id().with("record_bottom_prev"), click_sense(prev_on));
    prev.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, prev_on, "Previous slide"));
    let next = ui.interact(r.next, ui.scope_id().with("record_bottom_next"), click_sense(next_on));
    next.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, next_on, "Next slide"));

    // All three tools stay live: the eraser does nothing until there is ink, as in PowerPoint.
    let mut tools = Vec::with_capacity(3);
    for (i, tool) in [InkTool::Pen, InkTool::Highlighter, InkTool::Eraser].into_iter().enumerate() {
        let resp = ui.interact(r.tool(i), ui.scope_id().with(("record_bottom_tool", i)), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, rec.ink.tool == tool, tool_name(tool)));
        if tool == InkTool::Eraser {
            let hint = if show.has_ink() { eraser_hint(show.stroke_count()) } else { "Eraser: nothing to erase yet".to_owned() };
            let _ = resp.clone().on_hover_text(hint);
        }
        tools.push((tool, resp));
    }

    let mut swatches = Vec::with_capacity(INK_COLORS.len());
    for (i, ink_name) in INK_NAMES.iter().enumerate() {
        let name = format!("Ink colour {}", i + 1);
        let resp = ui.interact(r.swatch(i), ui.scope_id().with(("record_bottom_swatch", i)), Sense::click());
        let selected = rec.ink.color == i;
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
        let _ = resp.clone().on_hover_text(format!("{name} {ink_name}"));
        swatches.push(resp);
    }

    // Clicks first, so the count, progress and swatch rings show the new state this frame.
    if prev.clicked() && prev_on {
        let action = show.state.prev(doc);
        // The chevrons are the Record view's main navigation: the take ends here, on the slide
        // being left, exactly as the keyboard and the reading bar do it.
        super::super::before_slide_change_with(mic, show, &action);
        show.apply(action, doc, now);
    }
    if next.clicked() && next_on {
        let action = show.state.next(doc);
        super::super::before_slide_change_with(mic, show, &action);
        show.apply(action, doc, now);
    }
    for (tool, resp) in &tools {
        if resp.clicked() {
            pick_tool(rec, *tool);
        }
    }
    for (i, resp) in swatches.iter().enumerate() {
        if resp.clicked() {
            pick_color(rec, i);
        }
    }

    // One source of truth for the canvas and the recording.
    let mode = ink_mode(rec.ink.tool);
    if show.ink_mode() != mode {
        show.set_ink_mode(mode);
    }
    let color = INK_COLORS[rec.ink.color.min(INK_COLORS.len() - 1)];
    let rgba = deckcraft_model::Rgba::rgb(color.r(), color.g(), color.b());
    if show.ink_color() != rgba {
        show.set_ink_color(rgba);
    }
    rec.ink.active = show.ink_active();

    let slide = show.state.slide;
    paint_nav(ui, &prev, "◀", prev_on);
    paint_nav(ui, &next, "▶", next_on);
    ui.painter().text(r.count.left_center(), Align2::LEFT_CENTER, slide_count_label(slide, count), theme::font(12.0), Color32::from_gray(210));
    paint_progress(ui, r.progress, progress_fraction(slide, count));

    // Tools.
    for (i, icon) in [crate::icons::Icon::Pen, crate::icons::Icon::HighlighterPen, crate::icons::Icon::Eraser].into_iter().enumerate() {
        let (tool, resp) = &tools[i];
        paint_tool(ui, r.tool(i), icon, resp, true, rec.ink.tool == *tool);
    }
    // A hairline separates the tools from the colours.
    let sep_x = r.swatches.min.x - 6.0;
    ui.painter().line_segment([pos2(sep_x, r.tools.min.y + 6.0), pos2(sep_x, r.tools.max.y - 6.0)], Stroke::new(1.0, Color32::from_gray(64)));

    // Colours.
    for (i, resp) in swatches.iter().enumerate() {
        paint_swatch(ui, r.swatch(i), INK_COLORS[i], rec.ink.color == i, resp.hovered());
    }
}

/// The strip's own dark surface and the hairline that separates it from the canvas.
fn paint_strip(ui: &Ui, rect: Rect) {
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(23, 23, 27));
    p.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, Color32::from_gray(52)));
}

/// The eraser's hover text tells the user whether there is anything to remove.
fn eraser_hint(strokes: usize) -> String {
    match strokes {
        0 => "Eraser: nothing to erase yet".to_owned(),
        1 => "Eraser: 1 stroke".to_owned(),
        n => format!("Eraser: {n} strokes"),
    }
}

fn click_sense(enabled: bool) -> Sense {
    if enabled { Sense::click() } else { Sense::hover() }
}

fn tool_name(tool: InkTool) -> &'static str {
    match tool {
        InkTool::Arrow => "Pointer",
        InkTool::Pen => "Pen",
        InkTool::Highlighter => "Highlighter",
        InkTool::Eraser => "Eraser",
    }
}

fn paint_nav(ui: &Ui, resp: &egui::Response, glyph: &str, enabled: bool) {
    let rect = resp.rect;
    if resp.hovered() && enabled {
        ui.painter().rect_filled(rect, CornerRadius::same(6), Color32::from_gray(48));
    }
    let col = if enabled { Color32::from_gray(230) } else { Color32::from_gray(96) };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, theme::font(15.0), col);
}

fn paint_progress(ui: &Ui, rect: Rect, fraction: f32) {
    if rect.width() < 1.0 {
        return;
    }
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(3), Color32::from_gray(55));
    let w = (rect.width() * fraction.clamp(0.0, 1.0)).max(0.0);
    if w > 0.5 {
        let fill = Rect::from_min_size(rect.min, vec2(w, rect.height()));
        p.rect_filled(fill, CornerRadius::same(3), Color32::from_rgb(90, 170, 230));
    }
}

fn paint_tool(ui: &Ui, rect: Rect, icon: crate::icons::Icon, resp: &egui::Response, enabled: bool, on: bool) {
    let p = ui.painter();
    let bg = if on {
        Color32::from_rgb(46, 78, 112)
    } else if resp.hovered() && enabled {
        Color32::from_gray(48)
    } else {
        Color32::TRANSPARENT
    };
    if bg != Color32::TRANSPARENT {
        p.rect_filled(rect, CornerRadius::same(6), bg);
    }
    let ink = if on { Color32::WHITE } else { Color32::from_gray(220) };
    crate::icons::paint(p, rect.shrink(7.0), icon, ink, !enabled);
}

fn paint_swatch(ui: &Ui, rect: Rect, color: Color32, selected: bool, hovered: bool) {
    let p = ui.painter();
    let c = rect.center();
    let radius = rect.width() * 0.5;
    if selected {
        p.circle_stroke(c, radius + 2.5, Stroke::new(2.0, Color32::WHITE));
    }
    if hovered {
        p.circle_stroke(c, radius + 1.0, Stroke::new(1.0, Color32::from_gray(180)));
    }
    p.circle_filled(c, radius, color);
    // A dark swatch on the dark bar needs an edge; white and yellow read without one.
    p.circle_stroke(c, radius, Stroke::new(1.0, Color32::from_gray(120)));
}

#[cfg(test)]
#[path = "bottom_tests.rs"]
mod bottom_tests;
