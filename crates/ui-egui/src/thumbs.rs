//! The slide thumbnail pane (Normal view): numbered thumbnails grouped by section, selection,
//! drag-to-reorder, and the slide context menu.

use deckcraft_engine::Target;
use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::theme::{self, Tokens};

/// Render the slide thumbnail pane, including section headers, selection, and scroll state.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let in_master = app.session.active().is_some_and(|d| d.selection.target != Target::Slides);
    // Handout Master view has one page and no thumbnails, as in PowerPoint.
    if app.session.active().is_some_and(|d| d.selection.target.is_page_master()) {
        return;
    }
    let panel = egui::Panel::left("thumbs")
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
            let out = egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(8.0);
                list(app, ui);
                ui.add_space(20.0);
            });
            // Where the pane is scrolled to, for the control channel and tests (#917).
            app.ui.thumbs_scroll = out.state.clamped_offset().y;
        });
    // Focus follows the last press: inside the pane keeps it, anywhere else (canvas, ribbon) drops it.
    if let Some(pos) = ui.input(|i| if i.pointer.primary_pressed() { i.pointer.interact_pos() } else { None }) {
        app.slides_focused = panel.response.rect.contains(pos);
    }
}

/// Render the scrollable thumbnail list, scrolling the current slide into view when it changes.
fn list(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let cur = st.selection.slide;
    let multi = st.selection.slides.clone();
    let width = ui.available_width();
    let tw = (width - 40.0).max(60.0);
    let th = tw * (doc.slide_size.height / doc.slide_size.width.max(1.0)) as f32;
    // Scroll the current slide into view when it changes, so arrow-key navigation follows
    // along in the pane (#917). The scroll itself is minimal, only when the thumbnail leaves
    // the view, as in PowerPoint, so the user's own scrolling stands; the first frame only
    // records the slide.
    let scroll_key = ui.scope_id().with("thumb_scrolled_to");
    let slide_changed = ui.data_mut(|d| {
        let last: Option<usize> = d.get_temp(scroll_key);
        d.insert_temp(scroll_key, cur);
        last.is_some_and(|last| last != cur)
    });
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
        let img = crate::textures::device_pixel_rect(Rect::from_min_size(pos2(row.min.x + 30.0, row.min.y + 6.0), vec2(tw, th)), ppp);
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
        let wpx = (img.width() * ppp).round() as u32;
        // Only rows in view (plus a little margin) ask for a thumbnail: with every row asking, a
        // deck bigger than the thumbnail cache evicts its own thumbnails and never settles (#969).
        let near = ui.clip_rect().expand2(vec2(0.0, 2.0 * row.height())).intersects(row);
        let tex = if near { app.textures.thumb(ui.ctx(), &doc, i, wpx, &mut budget) } else { None };
        match tex {
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
                need_more |= near;
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
                if shift {
                    // Shift: the slides between the current one and this one. Ctrl+Shift keeps what is selected.
                    let _ = app.run("slide.selectRange", json!({"to": i, "add": cmd}));
                } else if cmd {
                    let _ = app.run("slide.selectSlides", json!({"ids": [id.0], "add": true}));
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
    // Scroll the current slide's thumbnail into view when the slide changes (#917).
    if slide_changed && let Some((_, row_rect)) = rects.iter().find(|(i, _)| *i == cur) {
        ui.scroll_to_rect(*row_rect, None);
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
                let ppp = ui.ctx().pixels_per_point();
                let img = crate::textures::device_pixel_rect(Rect::from_min_size(pos2(row.min.x + indent, row.min.y + 4.0), vec2(tw, th)), ppp);
                let wpx = ((img.width() * ppp).round() as u32).max(1);
                let px = crate::textures::fit_size(
                    (wpx, (wpx as f64 * doc.slide_size.height / doc.slide_size.width.max(1.0)).clamp(1.0, 1e9) as u32),
                    crate::textures::max_side(ui.ctx()),
                );
                let pic = deckcraft_render::render_layout(
                    &doc,
                    mi,
                    li,
                    &deckcraft_render::RenderOpts {
                        scale: px.0 as f64 / doc.slide_size.width.max(1.0),
                        edit: true,
                        size: Some(px),
                        ..Default::default()
                    },
                );
                let tex = crate::textures::upload_editor(ui.ctx(), format!("ml-{mi}-{li:?}"), crate::textures::to_color_image(&pic, false));
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use egui_kittest::Harness;

    use super::*;
    use crate::Services;
    use deckcraft_engine::Session;

    fn harness() -> Harness<'static, SlideApp> {
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        h.run_steps(3);
        h
    }

    /// Vertical scroll offset of the thumbnail pane, as recorded each frame by `show`.
    fn thumbs_scroll(h: &Harness<'_, SlideApp>) -> f32 {
        h.state().ui.thumbs_scroll
    }

    /// #917: navigating to a slide below the fold scrolls the pane to reveal it; a neighbour
    /// that is already on screen does not move the pane.
    #[test]
    fn navigating_slides_keeps_the_current_thumbnail_visible() {
        let mut h = harness();
        for _ in 0..30 {
            h.state_mut().session.execute("slide.new", &json!({})).unwrap();
        }
        // slide.new selects each new slide; back to the first so the pane starts at the top.
        h.state_mut().session.execute("slide.go", &json!({"index": 0})).unwrap();
        h.run_steps(3);
        let last = h.state().session.active().unwrap().doc.slides.len() - 1;
        assert_eq!(thumbs_scroll(&h), 0.0, "the pane starts at the top");

        // Jump to the last slide: the pane follows and reveals it.
        h.state_mut().session.execute("slide.go", &json!({"index": last})).unwrap();
        h.run_steps(120); // the scroll eases in over several frames
        let jumped = thumbs_scroll(&h);
        assert!(jumped > 100.0, "the pane scrolled to the last slide, offset {jumped}");

        // One slide up was already on screen after that scroll: the pane holds still.
        h.state_mut().session.execute("slide.go", &json!({"index": last - 1})).unwrap();
        h.run_steps(120);
        let held = thumbs_scroll(&h);
        assert!((held - jumped).abs() < 1.0, "no re-scroll for a visible slide: {jumped} -> {held}");
    }
}
