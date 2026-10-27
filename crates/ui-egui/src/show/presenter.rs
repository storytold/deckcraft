use super::keys::keyboard;
use super::render::{fit, next_rect};
use super::zoom::{Zoom, grid_indices, paint_grid, see_all_hit, slide_aspect, toggle_blank, zoom_frame};
use super::*;

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
                        show.presenter_grid = !show.presenter_grid;
                    }
                    if button(ui, show.zoom_label()).clicked() {
                        show.zoom_key();
                    }
                });
                ui.add_space(8.0);
                let avail = ui.available_rect_before_wrap();
                if show.presenter_grid {
                    // The grid fills this window; the audience keeps seeing the current slide.
                    paint_grid(ui.painter(), ui.ctx(), show, doc, avail);
                    let hit = ui.interact(avail, ui.scope_id().with("presenter_grid"), Sense::click());
                    if hit.clicked()
                        && let Some(p) = hit.interact_pointer_pos()
                        && let Some(n) = see_all_hit(avail, grid_indices(doc.slides.len()).len(), slide_aspect(doc), p)
                        && let Some(&i) = grid_indices(doc.slides.len()).get(n)
                    {
                        show.presenter_grid = false;
                        show.blank = None;
                        show.zoom = None;
                        show.zoom_arm = false;
                        actions.push(show.state.goto(doc, i));
                    }
                    return;
                }
                let left = Rect::from_min_max(avail.min, pos2(avail.min.x + avail.width() * 0.62, avail.max.y));
                let right = Rect::from_min_max(pos2(left.max.x + 16.0, avail.min.y), avail.max);
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

/// The current slide's notes in the presenter's notes pane, `px` for 12 pt text.
pub(super) fn presenter_notes(ui: &mut Ui, notes: &deckcraft_model::text::TextBody, px: f32) -> egui::Response {
    let color = Color32::from_gray(230);
    if notes.text().is_empty() {
        ui.label(egui::RichText::new("No notes.").size(px).color(color))
    } else {
        crate::notes::view(ui, notes, &crate::notes::Look { size: px, color: Some(color) })
    }
}
