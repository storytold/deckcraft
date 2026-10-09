//! The slide thumbnail pane (Normal view): numbered thumbnails grouped by section, selection,
//! drag-to-reorder, and the slide context menu.

use deckcraft_engine::Target;
use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::theme::{self, Tokens};

pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let in_master = app.session.active().is_some_and(|d| d.selection.target != Target::Slides);
    egui::Panel::left("thumbs")
        .resizable(true)
        .default_size(app.ui.thumbs_width)
        .size_range(150.0..=420.0)
        .frame(egui::Frame::NONE.fill(t.chrome))
        .show(ui, |ui| {
            app.ui.thumbs_width = ui.available_width();
            if in_master {
                master_list(app, ui);
                return;
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(8.0);
                list(app, ui);
                ui.add_space(20.0);
            });
        });
}

fn list(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let cur = st.selection.slide;
    let multi = st.selection.slides.clone();
    let width = ui.available_width();
    let tw = (width - 40.0).max(60.0);
    let th = tw * (doc.slide_size.height / doc.slide_size.width.max(1.0)) as f32;
    let ppp = ui.ctx().pixels_per_point();
    let mut budget = 3u32;
    let mut rects = vec![];
    let section_starts: Vec<(usize, usize)> =
        doc.sections.iter().enumerate().filter_map(|(si, s)| s.slides.first().and_then(|id| doc.slide_index(*id)).map(|i| (i, si))).collect();
    let mut need_more = false;
    for i in 0..doc.slides.len() {
        if let Some((_, si)) = section_starts.iter().find(|(start, _)| *start == i)
            && let Some(sec) = doc.sections.get(*si)
        {
            ui.add_space(4.0);
            let (r, resp) = ui.allocate_exact_size(vec2(width, 22.0), Sense::click());
            let p = ui.painter();
            crate::widgets::chevron(p, pos2(r.min.x + 14.0, r.center().y), 3.0, t.text_dim);
            p.text(
                pos2(r.min.x + 24.0, r.center().y),
                Align2::LEFT_CENTER,
                format!("{}  ({})", sec.name, sec.slides.len()),
                theme::font(12.0),
                t.text_dim,
            );
            resp.context_menu(|ui| {
                if ui.button(crate::i18n::tr("Rename Section…")).clicked() {
                    let mut d = crate::dialogs::Dialog::new("renameSection");
                    d.params = json!({"index": si, "name": sec.name});
                    app.dialog = Some(d);
                    ui.close();
                }
                if ui.button(crate::i18n::tr("Remove Section")).clicked() {
                    let _ = app.run("section.remove", json!({"index": si}));
                    ui.close();
                }
                if ui.button(crate::i18n::tr("Remove Section & Slides")).clicked() {
                    let _ = app.run("section.remove", json!({"index": si, "slides": true}));
                    ui.close();
                }
                if ui.button(crate::i18n::tr("Move Section Up")).clicked() {
                    let _ = app.run("section.move", json!({"index": si, "to": si.saturating_sub(1)}));
                    ui.close();
                }
                if ui.button(crate::i18n::tr("Move Section Down")).clicked() {
                    let _ = app.run("section.move", json!({"index": si, "to": si + 1}));
                    ui.close();
                }
            });
        }
        let (row, resp) = ui.allocate_exact_size(vec2(width, th + 16.0), Sense::click_and_drag());
        let img = Rect::from_min_size(pos2(row.min.x + 30.0, row.min.y + 6.0), vec2(tw, th));
        rects.push((i, row));
        let p = ui.painter();
        let id = doc.slides.get(i).map(|s| s.id);
        let selected = i == cur || id.is_some_and(|id| multi.contains(&id));
        let hidden = doc.slides.get(i).is_some_and(|s| s.hidden);
        p.text(
            pos2(row.min.x + 14.0, img.min.y + 2.0),
            Align2::CENTER_TOP,
            (i + 1).to_string(),
            theme::font(12.0),
            if selected { t.text } else { t.text_dim },
        );
        if hidden {
            p.line_segment([pos2(row.min.x + 8.0, img.min.y + 18.0), pos2(row.min.x + 20.0, img.min.y + 6.0)], Stroke::new(1.0, t.text_dim));
        }
        let has_anim = doc.slides.get(i).is_some_and(|s| !s.animations.is_empty() || s.transition.as_ref().is_some_and(|tr| tr.kind != "none"));
        if has_anim {
            p.text(pos2(row.min.x + 14.0, img.min.y + 20.0), Align2::CENTER_TOP, "★", theme::font(10.0), t.text_faint);
        }
        let wpx = (tw * ppp).round() as u32;
        match app.textures.thumb(ui.ctx(), &doc, i, wpx, &mut budget) {
            Some(tex) => {
                p.image(
                    tex.id(),
                    img,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    if hidden { Color32::from_white_alpha(140) } else { Color32::WHITE },
                );
            }
            None => {
                p.rect_filled(img, CornerRadius::ZERO, Color32::WHITE);
                need_more = true;
            }
        }
        if selected {
            p.rect_stroke(img.expand(2.0), CornerRadius::same(4), Stroke::new(3.0, t.accent), egui::StrokeKind::Outside);
        } else {
            p.rect_stroke(
                img,
                CornerRadius::ZERO,
                Stroke::new(if resp.hovered() { 2.0 } else { 1.0 }, if resp.hovered() { t.text_faint } else { t.border }),
                egui::StrokeKind::Outside,
            );
        }
        if resp.clicked() {
            let (shift, cmd) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
            if let Some(id) = id {
                if cmd {
                    let _ = app.run("slide.selectSlides", json!({"ids": [id.0], "add": true}));
                } else if shift {
                    let (a, b) = (cur.min(i), cur.max(i));
                    let ids: Vec<u32> = doc.slides.iter().skip(a).take(b - a + 1).map(|s| s.id.0).collect();
                    let _ = app.run("slide.selectSlides", json!({"ids": ids}));
                    let _ = app.run("slide.go", json!({"index": i}));
                    let _ = app.run("slide.selectSlides", json!({"ids": ids}));
                } else {
                    let _ = app.run("slide.go", json!({"index": i}));
                }
            }
        }
        if resp.double_clicked() {
            app.start_show(i, false);
        }
        if resp.drag_started() {
            app.thumb_drag = Some((i, i));
        }
        resp.context_menu(|ui| slide_menu(app, ui, i));
    }
    if need_more {
        ui.ctx().request_repaint();
    }
    // Drag reorder: insertion line.
    if let Some((from, _)) = app.thumb_drag {
        if let Some(pp) = ui.input(|i| i.pointer.interact_pos()) {
            let mut to = doc.slides.len();
            for (i, r) in &rects {
                if pp.y < r.center().y {
                    to = *i;
                    break;
                }
            }
            app.thumb_drag = Some((from, to));
            let y = rects.iter().find(|(i, _)| *i == to).map(|(_, r)| r.min.y + 2.0).or_else(|| rects.last().map(|(_, r)| r.max.y)).unwrap_or(0.0);
            ui.painter().line_segment([pos2(26.0, y), pos2(width, y)], Stroke::new(2.0, t.accent));
        }
        if ui.input(|i| i.pointer.any_released()) {
            if let Some((from, to)) = app.thumb_drag.take()
                && to != from
                && to != from + 1
            {
                let dest = if to > from { to - 1 } else { to };
                let _ = app.run("slide.move", json!({"from": from, "to": dest}));
            }
            app.thumb_drag = None;
        }
    }
    // Click on empty space below: new slide prompt.
    let (r, resp) = ui.allocate_exact_size(vec2(width, 40.0), Sense::click());
    if resp.hovered() {
        ui.painter().text(r.center(), Align2::CENTER_CENTER, crate::i18n::tr("+ New Slide"), theme::font(12.0), t.text_faint);
    }
    if resp.clicked() {
        let _ = app.run("slide.new", json!({}));
    }
}

pub fn slide_menu(app: &mut SlideApp, ui: &mut Ui, i: usize) {
    if app.session.active().is_some_and(|d| d.selection.slide != i && !d.selection.slides.iter().any(|id| d.doc.slide_index(*id) == Some(i))) {
        let _ = app.run("slide.go", json!({"index": i}));
    }
    for (l, id, p) in [
        ("Cut", "edit.cut", json!({"scope": "slides"})),
        ("Copy", "edit.copy", json!({"scope": "slides"})),
        ("Paste", "edit.paste", json!({"scope": "slides"})),
    ] {
        if ui.button(crate::i18n::tr(l)).clicked() {
            let _ = app.run(id, p);
            ui.close();
        }
    }
    ui.separator();
    if ui.button(crate::i18n::tr("New Slide")).clicked() {
        let _ = app.run("slide.new", json!({}));
        ui.close();
    }
    if ui.button(crate::i18n::tr("Duplicate Slide")).clicked() {
        let _ = app.run("slide.duplicate", json!({}));
        ui.close();
    }
    if ui.button(crate::i18n::tr("Delete Slide")).clicked() {
        let _ = app.run("slide.delete", json!({}));
        ui.close();
    }
    ui.separator();
    if ui.button(crate::i18n::tr("Add Section")).clicked() {
        let _ = app.run("section.add", json!({"at": i}));
        ui.close();
    }
    ui.menu_button(crate::i18n::tr("Layout"), |ui| {
        let layouts: Vec<(String, u32)> = app
            .session
            .active()
            .and_then(|d| d.doc.masters.first().map(|m| m.layouts.iter().map(|l| (l.name.clone(), l.id.0)).collect()))
            .unwrap_or_default();
        for (n, id) in layouts {
            if ui.button(n).clicked() {
                let _ = app.run("slide.layout", json!({"layout": id}));
                ui.close();
            }
        }
    });
    if ui.button(crate::i18n::tr("Reset Slide")).clicked() {
        let _ = app.run("slide.reset", json!({}));
        ui.close();
    }
    if ui.button(crate::i18n::tr("Format Background…")).clicked() {
        let _ = app.run("view.pane", json!({"pane": "background"}));
        ui.close();
    }
    let hidden = app.session.active().and_then(|d| d.doc.slides.get(i).map(|s| s.hidden)).unwrap_or(false);
    if ui.button(if hidden { "Show Slide" } else { "Hide Slide" }).clicked() {
        let _ = app.run("slide.hide", json!({"index": i}));
        ui.close();
    }
}

fn master_list(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let target = st.selection.target;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(8.0);
        let width = ui.available_width();
        for (mi, m) in doc.masters.iter().enumerate() {
            let items: Vec<Option<usize>> = std::iter::once(None).chain((0..m.layouts.len()).map(Some)).collect();
            for li in items {
                let indent = if li.is_some() { 30.0 } else { 14.0 };
                let tw = (width - indent - 14.0).max(40.0) * if li.is_some() { 0.8 } else { 1.0 };
                let th = tw * (doc.slide_size.height / doc.slide_size.width.max(1.0)) as f32;
                let (row, resp) = ui.allocate_exact_size(vec2(width, th + 12.0), Sense::click());
                let img = Rect::from_min_size(pos2(row.min.x + indent, row.min.y + 4.0), vec2(tw, th));
                let ppp = ui.ctx().pixels_per_point();
                let px = ((tw * ppp) as u32).max(1);
                let pic = deckcraft_render::render_layout(
                    &doc,
                    mi,
                    li,
                    &deckcraft_render::RenderOpts {
                        scale: px as f64 / doc.slide_size.width.max(1.0),
                        edit: true,
                        size: Some((px, (px as f64 * doc.slide_size.height / doc.slide_size.width.max(1.0)) as u32)),
                        ..Default::default()
                    },
                );
                let tex =
                    ui.ctx().load_texture(format!("ml-{mi}-{li:?}"), crate::textures::to_color_image(&pic, false), egui::TextureOptions::LINEAR);
                ui.painter().image(tex.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                let selected = match (target, li) {
                    (Target::Master { master }, None) => master == mi,
                    (Target::Layout { master, layout }, Some(l)) => master == mi && layout == l,
                    _ => false,
                };
                ui.painter().rect_stroke(
                    img.expand(if selected { 2.0 } else { 0.0 }),
                    CornerRadius::same(2),
                    Stroke::new(if selected { 3.0 } else { 1.0 }, if selected { t.accent } else { t.border }),
                    egui::StrokeKind::Outside,
                );
                let name = match li {
                    Some(l) => m.layouts.get(l).map(|x| x.name.clone()).unwrap_or_default(),
                    None => crate::i18n::format("{} Slide Master", std::slice::from_ref(&m.name)),
                };
                let resp = resp.on_hover_text(name);
                if resp.clicked() {
                    let p = match li {
                        Some(l) => json!({"master": mi, "layout": l}),
                        None => json!({"master": mi}),
                    };
                    let _ = app.run("view.slideMaster", p);
                }
            }
        }
    });
}
