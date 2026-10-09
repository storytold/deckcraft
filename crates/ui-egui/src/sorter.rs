//! Slide Sorter, Notes Page and the Outline pane.

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::theme::{self, Tokens};

/// Grid of slides; click selects, double-click opens in Normal view, drag reorders.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let cur = st.selection.slide;
    let multi = st.selection.slides.clone();
    let zoom = app.ui.sorter_zoom.clamp(0.5, 3.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(16.0);
        let tw = 200.0 * zoom;
        let th = tw * (doc.slide_size.height / doc.slide_size.width.max(1.0)) as f32;
        let ppp = ui.ctx().pixels_per_point();
        let cols = ((ui.available_width() - 32.0) / (tw + 36.0)).floor().max(1.0) as usize;
        let mut budget = 4u32;
        let mut rects = vec![];
        let mut need_more = false;
        for row in 0..doc.slides.len().div_ceil(cols) {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                for c in 0..cols {
                    let i = row * cols + c;
                    if i >= doc.slides.len() {
                        break;
                    }
                    let (cell, resp) = ui.allocate_exact_size(vec2(tw + 36.0, th + 36.0), Sense::click_and_drag());
                    let img = Rect::from_min_size(cell.min + vec2(18.0, 8.0), vec2(tw, th));
                    rects.push((i, cell));
                    let p = ui.painter();
                    match app.textures.thumb(ui.ctx(), &doc, i, (tw * ppp) as u32, &mut budget) {
                        Some(tex) => {
                            p.image(tex.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                        }
                        None => {
                            p.rect_filled(img, CornerRadius::ZERO, Color32::WHITE);
                            need_more = true;
                        }
                    }
                    let id = doc.slides.get(i).map(|s| s.id);
                    let selected = i == cur || id.is_some_and(|id| multi.contains(&id));
                    p.rect_stroke(
                        img.expand(if selected { 2.0 } else { 0.0 }),
                        CornerRadius::same(3),
                        Stroke::new(if selected { 3.0 } else { 1.0 }, if selected { t.accent } else { t.border }),
                        egui::StrokeKind::Outside,
                    );
                    p.text(pos2(img.min.x, img.max.y + 12.0), Align2::LEFT_CENTER, (i + 1).to_string(), theme::font(12.0), t.text_dim);
                    if let Some(tr) = doc.slides.get(i).and_then(|s| s.transition.as_ref()).filter(|t| t.kind != "none") {
                        p.text(pos2(img.max.x, img.max.y + 12.0), Align2::RIGHT_CENTER, format!("★ {}", tr.kind), theme::font(10.5), t.text_faint);
                    }
                    if resp.clicked() {
                        let add = ui.input(|i| i.modifiers.command);
                        if add {
                            if let Some(id) = id {
                                let _ = app.run("slide.selectSlides", json!({"ids": [id.0], "add": true}));
                            }
                        } else {
                            let _ = app.run("slide.go", json!({"index": i}));
                        }
                    }
                    if resp.double_clicked() {
                        let _ = app.run("slide.go", json!({"index": i}));
                        app.ui.view = crate::ViewMode::Normal;
                    }
                    if resp.drag_started() {
                        app.thumb_drag = Some((i, i));
                    }
                    resp.context_menu(|ui| crate::thumbs::slide_menu(app, ui, i));
                }
            });
        }
        if need_more {
            ui.ctx().request_repaint();
        }
        if let Some((from, _)) = app.thumb_drag {
            if let Some(pp) = ui.input(|i| i.pointer.interact_pos()) {
                let to =
                    rects.iter().find(|(_, r)| r.contains(pp)).map(|(i, r)| if pp.x > r.center().x { i + 1 } else { *i }).unwrap_or(doc.slides.len());
                app.thumb_drag = Some((from, to));
                if let Some((_, r)) = rects.iter().find(|(i, _)| *i == to).or(rects.last()) {
                    let x = if to >= doc.slides.len() { r.max.x - 6.0 } else { r.min.x + 6.0 };
                    ui.painter().line_segment([pos2(x, r.min.y + 6.0), pos2(x, r.max.y - 20.0)], Stroke::new(3.0, t.accent));
                }
            }
            if ui.input(|i| i.pointer.any_released()) {
                if let Some((from, to)) = app.thumb_drag.take()
                    && to != from
                    && to != from + 1
                {
                    let _ = app.run("slide.move", json!({"from": from, "to": if to > from { to - 1 } else { to }}));
                }
                app.thumb_drag = None;
            }
        }
        // Ctrl/⌘ + wheel zooms the sorter.
        let (zd, cmd) = ui.input(|i| (i.zoom_delta(), i.modifiers.command));
        if (zd - 1.0).abs() > 1e-3 || cmd {
            app.ui.sorter_zoom = (zoom * zd).clamp(0.5, 3.0);
        }
    });
}

/// Notes Page: the slide on top and its notes underneath, on a page.
pub fn notes_page(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let avail = ui.available_rect_before_wrap();
    ui.painter().rect_filled(avail, CornerRadius::ZERO, t.canvas);
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let idx = st.selection.slide;
    let page_h = avail.height() - 40.0;
    let page_w = page_h * 8.5 / 11.0;
    let page = Rect::from_center_size(avail.center(), vec2(page_w, page_h));
    ui.painter().rect_filled(page.translate(vec2(2.0, 2.0)), CornerRadius::ZERO, t.shadow);
    ui.painter().rect_filled(page, CornerRadius::ZERO, Color32::WHITE);
    let sw = page_w * 0.78;
    let sh = sw * (doc.slide_size.height / doc.slide_size.width.max(1.0)) as f32;
    let srect = Rect::from_min_size(pos2(page.center().x - sw / 2.0, page.min.y + page_h * 0.08), vec2(sw, sh));
    let ppp = ui.ctx().pixels_per_point();
    let mut budget = 2;
    if let Some(tex) = app.textures.thumb(ui.ctx(), &doc, idx, (sw * ppp) as u32, &mut budget) {
        ui.painter().image(tex.id(), srect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    ui.painter().rect_stroke(srect, CornerRadius::ZERO, Stroke::new(1.0, Color32::GRAY), egui::StrokeKind::Outside);
    let notes_rect = Rect::from_min_max(pos2(srect.min.x, srect.max.y + 24.0), pos2(srect.max.x, page.max.y - page_h * 0.06));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(notes_rect));
    let current = st.current_slide().map(|s| s.notes_text()).unwrap_or_default();
    let id = egui::Id::new(("notespage", idx));
    let mut buf: String = child.data_mut(|d| d.get_temp(id).unwrap_or_else(|| current.clone()));
    let r = child.add(
        egui::TextEdit::multiline(&mut buf)
            .hint_text(crate::i18n::tr("Click to add text"))
            .desired_width(notes_rect.width())
            .desired_rows(12)
            .frame(egui::Frame::NONE)
            .text_color(Color32::from_gray(30))
            .font(theme::font(14.0)),
    );
    if r.lost_focus() && buf != current {
        let _ = app.run("slide.notes", json!({"text": buf, "index": idx}));
    }
    child.data_mut(|d| d.insert_temp(id, buf));
    let nav = ui.input(|i| (i.key_pressed(egui::Key::PageDown), i.key_pressed(egui::Key::PageUp)));
    if nav.0 {
        let _ = app.run("slide.next", json!({}));
    }
    if nav.1 {
        let _ = app.run("slide.previous", json!({}));
    }
}

/// Outline view: slide titles and body text as an editable outline.
pub fn outline_pane(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::left("outline").resizable(true).default_size(360.0).frame(egui::Frame::NONE.fill(t.card).inner_margin(egui::Margin::same(10))).show(
        ui,
        |ui| {
            let Some(st) = app.session.active() else { return };
            let doc = st.doc.clone();
            let cur = st.selection.slide;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (i, s) in doc.slides.iter().enumerate() {
                    let title_shape = s.shapes.iter().find(|x| x.ph_type().is_some_and(|k| k.is_title())).map(|x| x.id.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new((i + 1).to_string()).color(t.text_dim));
                        let (r, _) = ui.allocate_exact_size(vec2(14.0, 10.0), Sense::hover());
                        ui.painter().rect_stroke(
                            r,
                            CornerRadius::same(1),
                            Stroke::new(1.0, if i == cur { t.accent } else { t.text_dim }),
                            egui::StrokeKind::Inside,
                        );
                        let id = egui::Id::new(("ol-title", s.id.0));
                        let cur_title = s.title();
                        let mut v: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_else(|| cur_title.clone()));
                        let resp =
                            ui.add(egui::TextEdit::singleline(&mut v).font(theme::bold(14.0)).frame(egui::Frame::NONE).desired_width(f32::INFINITY));
                        if resp.gained_focus() {
                            let _ = app.run("slide.go", json!({"index": i}));
                        }
                        if resp.lost_focus()
                            && v != cur_title
                            && let Some(tid) = title_shape
                        {
                            let _ = app.run("text.set", json!({"id": tid, "text": v}));
                        }
                        if !resp.has_focus() {
                            ui.data_mut(|d| d.remove::<String>(id));
                        } else {
                            ui.data_mut(|d| d.insert_temp(id, v));
                        }
                    });
                    for sh in s.shapes.iter().filter(|x| !x.ph_type().is_some_and(|k| k.is_title())) {
                        let Some(tb) = &sh.text else { continue };
                        if tb.is_empty() {
                            continue;
                        }
                        let id = egui::Id::new(("ol-body", s.id.0, sh.id.0));
                        let cur_text: String =
                            tb.paragraphs.iter().map(|p| format!("{}{}", "\t".repeat(p.level as usize), p.text())).collect::<Vec<_>>().join("\n");
                        let mut v: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_else(|| cur_text.clone()));
                        ui.horizontal(|ui| {
                            ui.add_space(36.0);
                            let resp =
                                ui.add(egui::TextEdit::multiline(&mut v).frame(egui::Frame::NONE).desired_width(f32::INFINITY).desired_rows(1));
                            if resp.gained_focus() {
                                let _ = app.run("slide.go", json!({"index": i}));
                            }
                            if resp.lost_focus() && v != cur_text {
                                let _ = app.run("text.set", json!({"id": sh.id.0, "text": v}));
                            }
                            if !resp.has_focus() {
                                ui.data_mut(|d| d.remove::<String>(id));
                            } else {
                                ui.data_mut(|d| d.insert_temp(id, v.clone()));
                            }
                        });
                    }
                    ui.add_space(6.0);
                }
            });
        },
    );
}
