use super::keys::keyboard;
use super::render::{fit, next_rect, render_rail_thumb};
use super::zoom::{Zoom, grid_slide_hit, grid_tab_hit, paint_grid, scroll_grid, slide_aspect, toggle_blank, unit_uv, zoom_frame};
use super::{Align2, Color32, CornerRadius, Presentation, Rect, RenderOpts, Sense, Show, ShowAction, Stroke, Ui, pos2, theme, vec2};
use crate::icons::{self, Icon};
use crate::sections::{self, GridFilter};

pub(super) const NOTES_DEFAULT: f32 = 18.0;

pub(super) const NOTES_MIN: f32 = 12.0;

pub(super) const NOTES_MAX: f32 = 48.0;

pub(super) const NOTES_STEP: f32 = 4.0;

/// Presenter-view elapsed time. Pausing freezes the number; the show keeps playing.
#[derive(Clone, Copy)]
pub(super) struct PresenterTimer {
    origin: f64,
    frozen: f64,
    paused: bool,
}

impl PresenterTimer {
    pub(super) fn start(now: f64) -> Self {
        PresenterTimer { origin: finite_time(now), frozen: 0.0, paused: false }
    }

    pub(super) fn elapsed(self, now: f64) -> f64 {
        if self.paused {
            return finite_secs(self.frozen);
        }
        let dt = now - self.origin;
        finite_secs(self.frozen + if dt.is_finite() && dt > 0.0 { dt } else { 0.0 })
    }

    pub(super) fn paused(self) -> bool {
        self.paused
    }

    pub(super) fn toggle(&mut self, now: f64) {
        if self.paused {
            self.origin = finite_time(now);
            self.paused = false;
        } else {
            self.frozen = self.elapsed(now);
            self.paused = true;
        }
    }

    /// Zero the elapsed time. A paused timer stays paused at zero.
    pub(super) fn reset(&mut self, now: f64) {
        self.frozen = 0.0;
        self.origin = finite_time(now);
    }
}

fn finite_time(now: f64) -> f64 {
    if now.is_finite() { now } else { 0.0 }
}

fn finite_secs(n: f64) -> f64 {
    if n.is_finite() && n >= 0.0 { n } else { 0.0 }
}

pub(super) fn format_elapsed(secs: f64) -> String {
    let s = finite_secs(secs) as u64;
    let s = s.min(99 * 3600 + 59 * 60 + 59);
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// Local wall clock for presenter view. `--:--` where there is no clock (wasm).
pub(super) fn wall_clock() -> String {
    match deckcraft_text::datetime::DateTime::now_local() {
        Some(t) if t.hour < 24 && t.minute < 60 => format!("{:02}:{:02}", t.hour, t.minute),
        _ => "--:--".to_string(),
    }
}

/// The widest the Presenter View section rail may get: a third of the window, but never so wide
/// that the slide and notes drop below their minimum room.
pub(super) fn section_rail_max(window_width: f32) -> f32 {
    if !window_width.is_finite() {
        return 0.0;
    }
    ((window_width - RAIL_GAP - MIN_CONTENT_W).max(0.0)).min(window_width / 3.0)
}

/// The rail's width for this frame: never more than a third of the window, and zero when there
/// are no sections to show.
pub(super) fn section_rail_width(preferred: f32, tabs: usize, window_width: f32) -> f32 {
    if tabs == 0 {
        return 0.0;
    }
    preferred.clamp(0.0, section_rail_max(window_width))
}

pub(super) const DEFAULT_RAIL_W: f32 = 208.0;

const RAIL_GAP: f32 = 12.0;
const MIN_CONTENT_W: f32 = 320.0;
/// Below this the rail has no room for its header and previews; dragging further hides it.
const RAIL_DRAW_MIN: f32 = 160.0;

/// How many previews the rail uploads per frame. A long section requests a repaint until the
/// rows already on screen are drawn, so opening it never renders the whole deck at once.
const RAIL_PREVIEW_BUDGET: usize = 8;

const RAIL_LABEL_H: f32 = 16.0;

/// One clickable slide preview in an unfolded section of the Presenter View rail.
fn rail_slide_preview(ui: &mut Ui, show: &mut Show, doc: &Presentation, i: usize, w: f32, made: &mut usize) -> egui::Response {
    let thumb_h = (w / slide_aspect(doc)).max(1.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, thumb_h + RAIL_LABEL_H), Sense::click());
    let title = doc.slides.get(i).map(|s| s.title().split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_default();
    let name = if title.is_empty() { format!("Slide {}", i + 1) } else { format!("Slide {}: {title}", i + 1) };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
    if rect.intersects(ui.clip_rect()) {
        let thumb = Rect::from_min_size(rect.min, vec2(w, thumb_h));
        let painter = ui.painter_at(rect);
        painter.rect_filled(thumb, CornerRadius::same(4), Color32::from_gray(28));
        let tex = match show.rail_thumbs.iter().find(|(k, _)| *k == i).map(|(_, t)| t.clone()) {
            Some(t) => Some(t),
            None if *made < RAIL_PREVIEW_BUDGET => {
                *made += 1;
                render_rail_thumb(ui.ctx(), show, doc, i)
            }
            None => {
                ui.ctx().request_repaint();
                None
            }
        };
        if let Some(tex) = tex {
            painter.image(tex.id(), thumb, unit_uv(), Color32::WHITE);
        }
        let current = i == show.state.slide;
        if resp.hovered() {
            painter.rect_stroke(thumb, CornerRadius::same(4), Stroke::new(1.0, Color32::from_gray(160)), egui::StrokeKind::Inside);
        }
        painter.rect_stroke(
            thumb,
            CornerRadius::same(4),
            Stroke::new(if current { 3.0 } else { 1.0 }, if current { Color32::WHITE } else { Color32::from_gray(70) }),
            egui::StrokeKind::Inside,
        );
        let hidden = doc.slides.get(i).is_some_and(|s| s.hidden);
        let label = if hidden { format!("{}  Hidden", i + 1) } else { format!("{}", i + 1) };
        painter.text(
            pos2(thumb.center().x, thumb.max.y + RAIL_LABEL_H * 0.5),
            Align2::CENTER_CENTER,
            label,
            theme::font(11.0),
            if current { Color32::WHITE } else { Color32::from_gray(180) },
        );
    }
    resp.on_hover_text(name)
}

/// Render the presenter window and set `keep` false when the user ends or closes the show.
pub(super) fn presenter_window(ctx: &egui::Context, show: &mut Show, doc: &Presentation, now: f64, keep: &mut bool) {
    let cur = show.state.slide;
    let next = show.state.peek_next(doc);
    let cur_tex = show.tex.as_ref().map(|(_, t)| t.clone());
    if let Some(n) = next
        && show.next_tex.as_ref().is_none_or(|(k, _)| *k != n)
    {
        // A very tall slide shape would otherwise ask for a picture taller than the GPU's limit.
        let next_size = crate::textures::fit_size(
            (480, (480.0 * doc.slide_size.height / doc.slide_size.width.max(1.0)).clamp(1.0, 1e9) as u32),
            crate::textures::max_side(ctx),
        );
        let img = deckcraft_render::render_slide(
            doc,
            n,
            &RenderOpts { scale: next_size.0 as f64 / doc.slide_size.width.max(1.0), size: Some(next_size), ..Default::default() },
        );
        show.next_tex = Some((n, crate::textures::upload(ctx, "presenter-next", crate::textures::to_color_image(&img, false))));
    }
    let next_tex = show.next_tex.as_ref().filter(|(k, _)| Some(*k) == next).map(|(_, t)| t.clone());
    let no_notes = deckcraft_model::text::TextBody::default();
    let notes = doc.slides.get(cur).map_or(&no_notes, |s| &s.notes);
    let clock = wall_clock();
    // The presenter clock only moves when this window is drawn again.
    ctx.request_repaint_after(std::time::Duration::from_millis(500));
    let mut actions = vec![];
    // The presenter window is dark; default buttons draw dark text on it (#221).
    let button = |ui: &mut Ui, s: &str| {
        ui.add(egui::Button::new(egui::RichText::new(s).size(15.0).color(Color32::WHITE)).fill(Color32::from_gray(60)).min_size(vec2(0.0, 30.0)))
    };
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("deckcraft_presenter"),
        egui::ViewportBuilder::default().with_title("Presenter View — DeckCraft").with_inner_size([1100.0, 700.0]),
        |ui, _| {
            if !keyboard(ui.ctx(), show, doc, now, true) {
                *keep = false;
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            let elapsed = show.timer.elapsed(now);
            let paused = show.timer.paused();
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::from_gray(24)).inner_margin(egui::Margin::same(16))).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format_elapsed(elapsed)).size(22.0).color(if paused {
                        Color32::from_gray(160)
                    } else {
                        Color32::WHITE
                    }));
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new(clock.clone()).size(16.0).color(Color32::from_gray(200)));
                    ui.add_space(20.0);
                    ui.label(egui::RichText::new(format!("Slide {} of {}", cur + 1, doc.slides.len())).size(16.0).color(Color32::from_gray(200)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if button(ui, "End Show").clicked() {
                            *keep = false;
                        }
                        if button(ui, "Next ▶").clicked() {
                            actions.push(show.state.next(doc));
                        }
                        if button(ui, "◀ Previous").clicked() {
                            actions.push(show.state.prev(doc));
                        }
                    });
                });
                ui.horizontal(|ui| {
                    if button(ui, if paused { "Resume" } else { "Pause" }).clicked() {
                        show.timer.toggle(now);
                    }
                    if button(ui, "Reset").clicked() {
                        show.timer.reset(now);
                    }
                    if button(ui, if show.blank == Some(Color32::BLACK) { "Black on" } else { "Black" }).clicked() {
                        toggle_blank(show, Color32::BLACK);
                    }
                    if button(ui, if show.blank == Some(Color32::WHITE) { "White on" } else { "White" }).clicked() {
                        toggle_blank(show, Color32::WHITE);
                    }
                    if button(ui, if show.presenter_grid { "All slides on" } else { "All slides" }).clicked() {
                        if show.presenter_grid {
                            show.presenter_grid = false;
                        } else {
                            show.open_grid(true, doc);
                        }
                    }
                    let mut stay = show.presenter_grid_stay;
                    let stay_resp =
                        ui.add(egui::Checkbox::new(&mut stay, egui::RichText::new("Keep All slides open").size(13.0).color(Color32::WHITE)));
                    if stay_resp.changed() {
                        show.presenter_grid_stay = stay;
                        // Turning the option off is also the way back to the classic presenter view.
                        if !stay {
                            show.presenter_grid = false;
                        }
                    }
                    let _ = stay_resp.on_hover_text("Keep the grid open after choosing a slide; turn it off to return to Presenter View");
                    if button(ui, show.zoom_label()).clicked() {
                        show.zoom_key();
                    }
                });
                ui.add_space(8.0);
                let avail = ui.available_rect_before_wrap();
                // A resizable section rail: hidden, a narrow strip, or up to a third of the window.
                let rail_tabs: Vec<(GridFilter, String, usize)> =
                    sections::tabs(doc).into_iter().filter(|(filter, _, count)| !matches!(filter, GridFilter::All) && *count > 0).collect();
                let content = sections_rail(ui, show, doc, cur, avail, &rail_tabs, &mut actions);
                if show.presenter_grid {
                    // The grid fills this window; the audience keeps seeing the current slide.
                    let delta = ui.input(|i| i.smooth_scroll_delta.y);
                    if delta != 0.0 {
                        scroll_grid(show, -delta, content, doc);
                    }
                    paint_grid(ui.painter(), ui.ctx(), show, doc, content);
                    let hit = ui.interact(content, ui.scope_id().with("presenter_grid"), Sense::click());
                    if hit.clicked()
                        && let Some(p) = hit.interact_pointer_pos()
                    {
                        if let Some(filter) = grid_tab_hit(content, doc, p) {
                            if filter != show.grid_filter {
                                show.grid_filter = filter;
                                show.grid_scroll = 0.0;
                                ui.ctx().request_repaint();
                            }
                        } else if let Some(i) = grid_slide_hit(content, doc, show.grid_filter, show.grid_scroll, p) {
                            pick_presenter_slide(show);
                            actions.push(show.state.goto(doc, i));
                        }
                    }
                    return;
                }
                let left = Rect::from_min_max(content.min, pos2(content.min.x + content.width() * 0.62, content.max.y));
                let right = Rect::from_min_max(pos2(left.max.x + 16.0, content.min.y), content.max);
                let cr = fit(Rect::from_min_max(left.min, pos2(left.max.x, left.min.y + left.height() * 0.8)), doc);
                let cr = Rect::from_min_size(left.min, cr.size());
                if let Some(t) = &cur_tex {
                    ui.painter().image(t.id(), cr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                if let Some(z) = show.zoom {
                    let mark = zoom_frame(cr, z.uv());
                    ui.painter().rect_stroke(mark, CornerRadius::ZERO, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Inside);
                }
                if show.zoom_arm {
                    let hit = ui.interact(cr, ui.scope_id().with("presenter_zoom"), Sense::click());
                    hit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Zoom"));
                    if hit.clicked()
                        && let Some(p) = hit.interact_pointer_pos()
                        && cr.width() > 1.0
                        && cr.height() > 1.0
                    {
                        show.zoom = Some(Zoom::at((p.x - cr.min.x) / cr.width(), (p.y - cr.min.y) / cr.height()));
                        show.zoom_arm = false;
                    }
                }
                if let Some(c) = show.blank {
                    ui.painter().rect_filled(cr, CornerRadius::ZERO, c);
                }
                let nr = next_rect(right, doc);
                ui.painter().text(right.min, Align2::LEFT_TOP, "Next", theme::font(13.0), Color32::from_gray(170));
                match &next_tex {
                    Some(t) => {
                        ui.painter().image(t.id(), nr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        ui.painter().rect_filled(nr, CornerRadius::ZERO, Color32::from_gray(40));
                        ui.painter().text(nr.center(), Align2::CENTER_CENTER, "End of slide show", theme::font(13.0), Color32::from_gray(170));
                    }
                }
                let notes_head = Rect::from_min_size(pos2(right.min.x, nr.max.y + 12.0), vec2(right.width(), 28.0));
                let mut head = ui.new_child(egui::UiBuilder::new().max_rect(notes_head));
                head.horizontal(|ui| {
                    ui.label(egui::RichText::new("Notes").size(13.0).color(Color32::from_gray(170)));
                    if ui.button("A-").clicked() {
                        show.nudge_notes(-NOTES_STEP);
                    }
                    ui.label(egui::RichText::new(format!("{}", show.notes_px as i32)).size(13.0).color(Color32::from_gray(200)));
                    if ui.button("A+").clicked() {
                        show.nudge_notes(NOTES_STEP);
                    }
                });
                let notes_rect = Rect::from_min_max(pos2(right.min.x, notes_head.max.y + 4.0), right.max);
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(notes_rect));
                egui::ScrollArea::vertical().show(&mut child, |ui| {
                    presenter_notes(ui, notes, show.notes_px);
                });
            });
            if ui.input(|i| i.viewport().close_requested()) {
                show.presenter = false;
                // A grid left open in the closed window would hold auto-advance for the rest of the show.
                show.presenter_grid = false;
            }
        },
    );
    for a in actions {
        if !show.apply(a, doc, now) {
            *keep = false;
        }
    }
}

/// A thumbnail picked in the Presenter View grid: the slide changes; the grid stays when the
/// presenter asked it to, otherwise it returns to the classic current/next/notes layout.
/// Draw the section rail and its resize handle in `avail`; returns the rectangle left for the
/// slide and notes.
fn sections_rail(
    ui: &mut Ui,
    show: &mut Show,
    doc: &Presentation,
    cur: usize,
    avail: Rect,
    rail_tabs: &[(GridFilter, String, usize)],
    actions: &mut Vec<ShowAction>,
) -> Rect {
    let max_rail = section_rail_max(avail.width());
    let rail_w = section_rail_width(show.presenter_rail_w, rail_tabs.len(), avail.width());
    let content = if rail_tabs.is_empty() { avail } else { Rect::from_min_max(pos2(avail.min.x + rail_w + RAIL_GAP, avail.min.y), avail.max) };
    let rail = Rect::from_min_max(avail.min, pos2(avail.min.x + rail_w, avail.max.y));
    if rail_w > 0.0 {
        ui.painter().rect_filled(rail, CornerRadius::same(6), Color32::from_gray(32));
    }
    if rail_w >= RAIL_DRAW_MIN {
        sections_rail_contents(ui, show, doc, cur, rail_tabs, rail, actions);
    } else {
        // A narrow strip has no room for the header or previews; free their textures.
        show.rail_thumbs.clear();
    }
    if !rail_tabs.is_empty() && max_rail > 0.0 {
        sections_rail_handle(ui, show, avail, rail_w, max_rail);
    }
    content
}

fn sections_rail_contents(
    ui: &mut Ui,
    show: &mut Show,
    doc: &Presentation,
    cur: usize,
    rail_tabs: &[(GridFilter, String, usize)],
    rail: Rect,
    actions: &mut Vec<ShowAction>,
) {
    let t = theme::Tokens::get(ui.ctx());
    let mut rail_ui = ui.new_child(egui::UiBuilder::new().max_rect(rail.shrink2(vec2(8.0, 8.0))));
    rail_ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Sections").size(13.0).color(Color32::from_gray(170)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let cols = show.presenter_section_cols.clamp(1, 2);
                let label = if cols == 2 { "2 per row" } else { "1 per row" };
                let tip = if cols == 2 { "Show one slide per row" } else { "Show two slides per row" };
                let resp =
                    ui.add(egui::Button::new(egui::RichText::new(label).size(10.5).color(t.accent_text)).fill(t.accent).min_size(vec2(0.0, 20.0)));
                if resp.on_hover_text(tip).clicked() {
                    show.presenter_section_cols = if cols == 2 { 1 } else { 2 };
                }
            });
        });
        ui.add_space(4.0);
        let current = sections::current_filter(doc, cur);
        // Previews of folded sections (or of a closed show) must not keep textures alive.
        if show.presenter_sections_open.is_empty() {
            show.rail_thumbs.clear();
        } else {
            let open: Vec<usize> = show.presenter_sections_open.iter().flat_map(|f| sections::grid_slides(doc, *f)).collect();
            show.rail_thumbs.retain(|(k, _)| open.contains(k));
        }
        egui::ScrollArea::vertical().id_salt("presenter_sections").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let mut made = 0;
            for (filter, name, count) in rail_tabs {
                let selected = *filter == current;
                let open = show.presenter_sections_open.contains(filter);
                let arrow = if open { "-" } else { "+" };
                let resp = ui.add(
                    egui::Button::new(egui::RichText::new(format!("{arrow} {name} ({count})")).size(13.0).color(if selected {
                        t.accent_text
                    } else {
                        Color32::WHITE
                    }))
                    .fill(if selected { t.accent } else { Color32::from_gray(48) })
                    .min_size(vec2(ui.available_width(), 30.0))
                    .truncate(),
                );
                let resp = resp.on_hover_text(format!("{} the slides in {name}", if open { "Hide" } else { "Show" }));
                if resp.clicked() {
                    // The section row only unfolds its slides; the audience keeps the current
                    // slide until the presenter picks one.
                    if open {
                        show.presenter_sections_open.retain(|f| f != filter);
                    } else {
                        show.presenter_sections_open.push(*filter);
                        resp.scroll_to_me(Some(egui::Align::TOP));
                    }
                }
                if open {
                    let slides = sections::grid_slides(doc, *filter);
                    let inner_w = ui.available_width();
                    let cols = show.presenter_section_cols.clamp(1, 2);
                    let gap = 6.0;
                    let cell_w = if cols == 1 { inner_w } else { ((inner_w - gap) / 2.0).max(1.0) };
                    for row in slides.chunks(cols) {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            for &i in row {
                                if rail_slide_preview(ui, show, doc, i, cell_w, &mut made).clicked() {
                                    pick_presenter_slide(show);
                                    actions.push(show.state.goto(doc, i));
                                }
                            }
                        });
                    }
                }
            }
        });
    });
}

/// The divider grip: drag to resize, click to hide or restore the rail.
fn sections_rail_handle(ui: &mut Ui, show: &mut Show, avail: Rect, rail_w: f32, max_rail: f32) {
    let handle = Rect::from_min_max(pos2(avail.min.x + rail_w, avail.min.y), pos2(avail.min.x + rail_w + RAIL_GAP, avail.max.y));
    let resp = ui.interact(handle, ui.scope_id().with("presenter_rail_handle"), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Resize sections panel"));
    if resp.dragged() {
        let delta = resp.drag_delta().x;
        if delta.is_finite() {
            let next = (rail_w + delta).clamp(0.0, max_rail);
            show.presenter_rail_w = next;
            if next >= RAIL_DRAW_MIN {
                show.presenter_rail_last_w = next;
            }
            ui.ctx().request_repaint();
        }
    }
    if resp.clicked() {
        if rail_w > 0.0 {
            if rail_w >= RAIL_DRAW_MIN {
                show.presenter_rail_last_w = rail_w;
            }
            show.presenter_rail_w = 0.0;
        } else {
            let open = show.presenter_rail_last_w.clamp(0.0, max_rail);
            show.presenter_rail_w = if open > 0.0 { open } else { DEFAULT_RAIL_W.min(max_rail) };
        }
        ui.ctx().request_repaint();
    }
    let t = theme::Tokens::get(ui.ctx());
    let hot = resp.hovered() || resp.dragged();
    let painter = ui.painter_at(handle);
    painter.line_segment(
        [pos2(handle.center().x, handle.min.y), pos2(handle.center().x, handle.max.y)],
        Stroke::new(if hot { 2.0 } else { 1.0 }, if hot { t.accent } else { Color32::from_gray(48) }),
    );
    let tab = Rect::from_center_size(pos2(handle.center().x, handle.center().y), vec2(12.0, 52.0));
    painter.rect_filled(tab, CornerRadius::same(3), if hot { t.accent } else { Color32::from_gray(52) });
    icons::paint(
        &painter,
        tab.shrink(1.0),
        if rail_w > 0.0 { Icon::ChevronLeft } else { Icon::ChevronRight },
        if hot { t.accent_text } else { Color32::from_gray(200) },
        false,
    );
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let _ = resp.on_hover_text(if rail_w > 0.0 { "Drag to resize; click to hide Sections" } else { "Drag to resize; click to show Sections" });
}

pub(super) fn pick_presenter_slide(show: &mut Show) {
    if !show.presenter_grid_stay {
        show.presenter_grid = false;
    }
    show.blank = None;
    show.zoom = None;
    show.zoom_arm = false;
}

/// The current slide's notes in the presenter's notes pane, `px` for 12 pt text.
pub(super) fn presenter_notes(ui: &mut Ui, notes: &deckcraft_model::text::TextBody, px: f32) -> egui::Response {
    let color = Color32::from_gray(230);
    if notes.text().is_empty() {
        ui.label(egui::RichText::new("No notes.").size(px).color(color))
    } else {
        crate::notes::view(ui, notes, &crate::notes::Look { size: px, color: Some(color) })
    }
}
