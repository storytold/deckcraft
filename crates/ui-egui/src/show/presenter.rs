use super::keys::keyboard;
use super::render::{fit, next_rect, next_split_from_pointer};
use super::zoom::{Zoom, grid_indices, paint_grid, see_all_hit, slide_aspect, toggle_blank, zoom_frame};
use super::{Align2, Color32, CornerRadius, Presentation, Rect, RenderOpts, Sense, Show, Stroke, Ui, pos2, theme, vec2};

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

/// The Presenter View's preview of slide `n`, `size` pixels.
fn render_preview(doc: &Presentation, n: usize, size: (u32, u32), threads: u16) -> egui::ColorImage {
    let opts = RenderOpts { scale: size.0 as f64 / doc.slide_size.width.max(1.0), size: Some(size), threads, ..Default::default() };
    crate::textures::to_color_image(&deckcraft_render::render_slide(doc, n, &opts), false)
}

/// Render the presenter window and set `keep` false when the user ends or closes the show.
pub(super) fn presenter_window(ctx: &egui::Context, show: &mut Show, doc: &std::sync::Arc<Presentation>, now: f64, keep: &mut bool) {
    let cur = show.state.slide;
    let next = show.state.peek_next(doc);
    let cur_tex = show.tex.as_ref().map(|(_, t)| t.clone());
    // The preview wanted: slide `n` of this presentation (an edit replaces it).
    let is = |d: &std::sync::Arc<Presentation>, k: usize, n: usize| std::sync::Arc::ptr_eq(d, doc) && k == n;
    if let Some(n) = next
        && show.next_tex.as_ref().is_none_or(|(d, k, _)| !is(d, *k, n))
    {
        // A very tall slide shape would otherwise ask for a picture taller than the GPU's limit.
        let next_size = crate::textures::fit_size(
            (480, (480.0 * doc.slide_size.height / doc.slide_size.width.max(1.0)).clamp(1.0, 1e9) as u32),
            crate::textures::max_side(ctx),
        );
        // Rendered on another thread, so turning the page doesn't wait for the preview (#964). One
        // render at a time: paging faster than it renders asks for the newest preview once it is
        // done, and a finished one no longer wanted is dropped.
        if let Some((d, k, rx)) = show.next_job.take() {
            match rx.try_recv() {
                Ok(ci) if is(&d, k, n) => show.next_tex = Some((d, k, crate::textures::upload(ctx, "presenter-next", ci))),
                Ok(_) => {}
                Err(std::sync::mpsc::TryRecvError::Empty) => show.next_job = Some((d, k, rx)),
                // The worker died: render this one here, as before, rather than start another.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    show.next_tex = Some((doc.clone(), n, crate::textures::upload(ctx, "presenter-next", render_preview(doc, n, next_size, 0))));
                }
            }
        }
        if show.next_job.is_none() && show.next_tex.as_ref().is_none_or(|(d, k, _)| !is(d, *k, n)) {
            let (tx, rx) = std::sync::mpsc::channel();
            let worker_doc = doc.clone();
            // The web has no threads: render it here.
            let spawned = !cfg!(target_arch = "wasm32")
                && std::thread::Builder::new()
                    .name("show-next-preview".into())
                    .spawn(move || {
                        let _ = tx.send(render_preview(&worker_doc, n, next_size, 1));
                    })
                    .is_ok();
            if spawned {
                #[cfg(test)]
                {
                    show.preview_spawns += 1;
                }
                show.next_job = Some((doc.clone(), n, rx));
            } else {
                show.next_tex = Some((doc.clone(), n, crate::textures::upload(ctx, "presenter-next", render_preview(doc, n, next_size, 0))));
            }
        }
        if show.next_job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
    let next_tex = show.next_tex.as_ref().filter(|(d, k, _)| next.is_some_and(|n| is(d, *k, n))).map(|(_, _, t)| t.clone());
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
    // A fixed point size is scaled into physical pixels. At 125% that leaves a
    // 1100×700 window on a 1920×1200 monitor, so a notes screen gets no inner
    // size and is fitted to the monitor from inside the window.
    let mut viewport = egui::ViewportBuilder::default().with_title("Presenter View — DeckCraft");
    if show.notes_frame.is_some()
        && let Some(screen) = crate::screens::presenter_monitor(show.audience_screen, show.notes_screen)
    {
        viewport = viewport.with_monitor(screen).with_decorations(false);
    } else {
        viewport = viewport.with_inner_size([1100.0, 700.0]);
    }
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("deckcraft_presenter"), viewport, |ui, _| {
        if let Some(frame) = show.notes_frame {
            crate::screens::cover_monitor(ui.ctx(), frame);
        }
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
                    let halt = if show.halt_last { "Halt on last slide on" } else { "Halt on last slide" };
                    if button(ui, halt)
                        .on_hover_text("On the last slide, arrows and a clicker stay put so a live show does not end. End Show still leaves.")
                        .clicked()
                    {
                        show.halt_last = !show.halt_last;
                    }
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
            let right = slide_column(ui, show, doc, avail, &cur_tex);
            let notes_top = next_column(ui, show, doc, right, &next_tex, next.is_none());
            let notes_rect = Rect::from_min_max(pos2(right.min.x, notes_top), right.max);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(notes_rect));
            super::notes_edit::pane(&mut child, show, cur, notes, doc);
        });
        if ui.input(|i| i.viewport().close_requested()) {
            show.notes_editing = false;
            super::notes_edit::flush_draft(show);
            show.presenter = false;
            // A grid left open in the closed window would hold auto-advance for the rest of the show.
            show.presenter_grid = false;
        }
    });
    for a in actions {
        if !show.apply(a, doc, now) {
            *keep = false;
        }
    }
}

/// The current slide and the drag handle on its right. Returns the column left for the next
/// slide and the notes. Hidden (`current_split` near 0) leaves a button that brings the slide back.
fn slide_column(ui: &mut Ui, show: &mut Show, doc: &Presentation, avail: Rect, cur_tex: &Option<egui::TextureHandle>) -> Rect {
    let hidden = !show.current_split.is_finite() || show.current_split < 0.04;
    let show_btn = |ui: &mut Ui, label: &str| {
        ui.add(egui::Button::new(egui::RichText::new(label).size(13.0).color(Color32::WHITE)).fill(Color32::from_gray(58)).min_size(vec2(0.0, 26.0)))
    };
    if hidden {
        let bar = Rect::from_min_size(avail.min, vec2(avail.width().max(1.0), 28.0));
        let mut header = ui.new_child(egui::UiBuilder::new().max_rect(bar));
        if show_btn(&mut header, "Show current slide").on_hover_text("Show the current slide beside the notes").clicked() {
            show.current_split = crate::state::PRESENTER_CURRENT_DEFAULT;
        }
        return Rect::from_min_max(pos2(avail.min.x, (bar.max.y + 8.0).min(avail.max.y)), avail.max);
    }
    let cols = presenter_columns(avail, show.current_split);
    let header = Rect::from_min_size(cols.slide.min, vec2(cols.slide.width().max(1.0), 22.0));
    let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(header));
    bar.horizontal(|ui| {
        ui.label(egui::RichText::new("Slide").size(13.0).color(Color32::from_gray(170)));
        if show_btn(ui, "Hide").on_hover_text("Hide the current slide and give the room to the notes").clicked() {
            show.current_split = 0.0;
        }
    });
    let image_area = Rect::from_min_max(pos2(cols.slide.min.x, header.max.y), cols.slide.max);
    let cr = if image_area.width() > 8.0 && image_area.height() > 8.0 {
        let fitted = fit(image_area, doc);
        Rect::from_min_size(image_area.min, fitted.size())
    } else {
        Rect::from_min_size(image_area.min, vec2(0.0, 0.0))
    };
    if cr.width() > 1.0 && cr.height() > 1.0 {
        if let Some(t) = cur_tex {
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
    }
    let x = (avail.min.x + avail.width() * show.current_split.clamp(0.04, 0.85)).clamp(avail.min.x + 36.0, avail.max.x - 48.0);
    let handle = Rect::from_min_max(pos2(x - 4.0, avail.min.y), pos2(x + 4.0, avail.max.y));
    ui.painter().vline(x, handle.y_range(), Stroke::new(2.0, Color32::from_gray(70)));
    let hit = ui.interact(handle, ui.scope_id().with("presenter_current_split"), Sense::click_and_drag());
    let hit = hit.on_hover_cursor(egui::CursorIcon::ResizeHorizontal).on_hover_text("Drag to resize the current slide. Double-click to hide it.");
    if hit.double_clicked() {
        show.current_split = 0.0;
    } else if hit.dragged()
        && let Some(p) = ui.input(|i| i.pointer.interact_pos())
    {
        show.current_split = current_split_from_pointer(avail, p.x);
    }
    cols.notes
}

/// The current slide's column and the column for the next slide and notes.
/// A split below `0.04` hides the slide and gives the whole area to the notes.
pub(super) fn presenter_columns(avail: Rect, split: f32) -> PresenterColumns {
    if !split.is_finite() || split < 0.04 || avail.width() < 8.0 || avail.height() < 8.0 {
        return PresenterColumns { slide: Rect::from_min_size(avail.min, vec2(0.0, 0.0)), notes: avail };
    }
    let split = split.clamp(0.04, 0.85);
    let boundary = avail.min.x + avail.width() * split;
    let slide = Rect::from_min_max(avail.min, pos2((boundary - 8.0).max(avail.min.x), avail.max.y));
    let notes = Rect::from_min_max(pos2((boundary + 8.0).min(avail.max.x), avail.min.y), avail.max);
    PresenterColumns { slide, notes }
}

/// Current-slide and notes columns of the presenter window.
pub(super) struct PresenterColumns {
    pub slide: Rect,
    pub notes: Rect,
}

/// The current-slide splitter as a fraction of `area`, from a drag at `x`. Hiding is the Hide
/// button, not a drag.
pub(super) fn current_split_from_pointer(area: Rect, x: f32) -> f32 {
    let w = area.width();
    if !x.is_finite() || w < 1.0 {
        return crate::state::PRESENTER_CURRENT_DEFAULT;
    }
    let min = (180.0 / w).clamp(0.15, 0.45);
    let max = ((w - 220.0) / w).clamp(min, 0.85);
    ((x - area.min.x) / w).clamp(min, max)
}

/// The next-slide preview and the drag handle under it. Returns where the notes start.
/// Hidden (`next_split` near 0) leaves only a button that brings the preview back.
fn next_column(ui: &mut Ui, show: &mut Show, doc: &Presentation, right: Rect, next_tex: &Option<egui::TextureHandle>, at_end: bool) -> f32 {
    let hidden = !show.next_split.is_finite() || show.next_split < 0.04;
    let show_btn = |ui: &mut Ui, label: &str| {
        ui.add(egui::Button::new(egui::RichText::new(label).size(13.0).color(Color32::WHITE)).fill(Color32::from_gray(58)).min_size(vec2(0.0, 26.0)))
    };
    if hidden {
        let bar = Rect::from_min_size(right.min, vec2(right.width().max(1.0), 28.0));
        let mut header = ui.new_child(egui::UiBuilder::new().max_rect(bar));
        if show_btn(&mut header, "Show next slide").on_hover_text("Show the next slide beside the notes").clicked() {
            show.next_split = crate::state::PRESENTER_NEXT_DEFAULT;
        }
        return (bar.max.y + 8.0).min(right.max.y);
    }
    let split = show.next_split.clamp(0.04, 0.85);
    let header = Rect::from_min_size(right.min, vec2(right.width().max(1.0), 22.0));
    let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(header));
    bar.horizontal(|ui| {
        ui.label(egui::RichText::new("Next").size(13.0).color(Color32::from_gray(170)));
        if show_btn(ui, "Hide").on_hover_text("Hide the next slide and give the room to the notes").clicked() {
            show.next_split = 0.0;
        }
    });
    let nr = next_rect(right, doc, split);
    if nr.width() > 1.0 && nr.height() > 1.0 {
        match next_tex {
            Some(t) => {
                ui.painter().image(t.id(), nr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                ui.painter().rect_filled(nr, CornerRadius::ZERO, Color32::from_gray(40));
                // A next slide whose preview is still on its way is not the end.
                if at_end {
                    ui.painter().text(nr.center(), Align2::CENTER_CENTER, "End of slide show", theme::font(13.0), Color32::from_gray(170));
                }
            }
        }
    }
    let y = (right.min.y + right.height() * split).clamp(right.min.y + 36.0, right.max.y - 48.0);
    let handle = Rect::from_min_max(pos2(right.min.x, y - 4.0), pos2(right.max.x, y + 4.0));
    ui.painter().hline(handle.x_range(), y, Stroke::new(2.0, Color32::from_gray(70)));
    let hit = ui.interact(handle, ui.scope_id().with("presenter_next_split"), Sense::click_and_drag());
    let hit = hit.on_hover_cursor(egui::CursorIcon::ResizeVertical).on_hover_text("Drag to resize the next slide. Double-click to hide it.");
    if hit.double_clicked() {
        show.next_split = 0.0;
    } else if hit.dragged()
        && let Some(p) = ui.input(|i| i.pointer.interact_pos())
    {
        show.next_split = next_split_from_pointer(right, p.y);
    }
    (y + 8.0).min(right.max.y)
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
