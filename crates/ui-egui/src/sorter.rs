//! Slide Sorter, Notes Page and the Outline pane.

use std::collections::HashSet;

use deckcraft_model::{Presentation, SlideId};
use egui::{Align2, Color32, CornerRadius, Rect, Role, Sense, Stroke, Ui, WidgetInfo, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// Sorter-only state. Nothing here is part of the document.
#[derive(Debug, Default)]
pub struct State {
    /// Sections folded shut, keyed by (document uid, that section's first slide) so moving a
    /// section does not fold a different one. An empty section has nothing to hide.
    pub collapsed: HashSet<(u64, u32)>,
    /// Inline rename: (document uid, section index, text typed so far).
    pub renaming: Option<(u64, usize, String)>,
    rename_started: bool,
    /// Whole-section drag: (section index, insertion slot among the bars).
    pub drag: Option<(usize, usize)>,
}

/// Where a dragged section would land. `centers` are the header centre-y values, top to bottom;
/// `slot` is the gap (0 = before the first bar, `len` = after the last).
fn drop_slot(centers: &[f32], y: f32) -> usize {
    centers.iter().position(|cy| y < *cy).unwrap_or(centers.len())
}

/// `section.move`'s `to` for a drag from `from` released at `slot`. `None` when the section
/// would stay where it is (the gap before it, or the gap just after it).
fn move_dest(from: usize, slot: usize) -> Option<usize> {
    if slot == from || slot == from + 1 { None } else { Some(if slot > from { slot - 1 } else { slot }) }
}

/// One section's slides, in the section's order, then any slide that belongs to none.
fn groups(doc: &Presentation) -> Vec<(Option<usize>, Vec<usize>)> {
    if doc.sections.is_empty() {
        return vec![(None, (0..doc.slides.len()).collect())];
    }
    let mut seen = vec![false; doc.slides.len()];
    let mut out = Vec::with_capacity(doc.sections.len() + 1);
    for (si, sec) in doc.sections.iter().enumerate() {
        let mut slides = Vec::with_capacity(sec.slides.len());
        for id in &sec.slides {
            if let Some(i) = doc.slide_index(*id)
                && let Some(flag) = seen.get_mut(i)
                && !*flag
            {
                *flag = true;
                slides.push(i);
            }
        }
        out.push((Some(si), slides));
    }
    let rest: Vec<usize> = seen.iter().enumerate().filter(|(_, on)| !**on).map(|(i, _)| i).collect();
    if !rest.is_empty() {
        out.push((None, rest));
    }
    out
}

struct Header<'a> {
    uid: u64,
    index: usize,
    name: &'a str,
    count: usize,
    first: Option<u32>,
}

/// Render one slide-sorter section header, handle its collapse/drag interactions, and return the
/// occupied rectangle.
fn section_header(app: &mut SlideApp, ui: &mut Ui, h: &Header<'_>) -> Rect {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
    let resp = ui.interact(r, ui.scope_id().with(("sorter-section", h.uid, h.index)), Sense::click_and_drag());
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, h.name));
    let open = h.first.is_none_or(|id| !app.sorter.collapsed.contains(&(h.uid, id)));
    let dragging = app.sorter.drag.is_some_and(|(from, _)| from == h.index);
    let bg = if dragging {
        t.selected
    } else if resp.hovered() {
        t.hover
    } else {
        t.card
    };
    let painter = ui.painter_at(r);
    painter.rect_filled(r, CornerRadius::same(4), bg);
    let chev = Rect::from_center_size(pos2(r.min.x + 20.0, r.center().y), vec2(24.0, 24.0));
    let cresp = ui.interact(chev, ui.scope_id().with(("sorter-fold", h.uid, h.index)), Sense::click());
    let fold = format!("{} {}", if open { "Collapse" } else { "Expand" }, h.name);
    cresp.widget_info(|| WidgetInfo::labeled(Role::Button, true, &fold));
    icons::paint(&painter, chev.shrink(4.0), if open { Icon::ChevronDown } else { Icon::ChevronRight }, t.text_dim, h.first.is_none());
    let renaming = app.sorter.renaming.clone().filter(|(uid, index, _)| *uid == h.uid && *index == h.index).map(|(_, _, text)| text);
    let name_rect = Rect::from_min_max(pos2(r.min.x + 40.0, r.min.y + 4.0), pos2(r.max.x - 56.0, r.max.y - 4.0));
    if let Some(mut buf) = renaming {
        let edit_id = ui.scope_id().with(("sorter-rename", h.uid, h.index));
        let out =
            ui.put(name_rect, egui::TextEdit::singleline(&mut buf).id(edit_id).hint_text("Name").font(theme::bold(13.0)).margin(vec2(4.0, 2.0)));
        let already = app.sorter.rename_started;
        if !already {
            out.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), edit_id) {
                let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(buf.chars().count()));
                state.cursor.set_char_range(Some(all));
                state.store(ui.ctx(), edit_id);
            }
            app.sorter.rename_started = true;
        }
        let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
        // The frame that asks for focus has not had it yet; committing then would cancel the rename.
        if already && (out.lost_focus() || enter || esc) {
            app.sorter.renaming = None;
            app.sorter.rename_started = false;
            let trimmed = buf.trim();
            if !esc && !trimmed.is_empty() && trimmed != h.name {
                let _ = app.run("section.rename", json!({"index": h.index, "name": trimmed}));
            }
        } else if let Some((_, _, text)) = app.sorter.renaming.as_mut() {
            *text = buf;
        }
    } else {
        painter.text(pos2(name_rect.min.x, r.center().y), Align2::LEFT_CENTER, h.name, theme::bold(13.0), t.text);
    }
    painter.text(pos2(r.max.x - 16.0, r.center().y), Align2::RIGHT_CENTER, format!("({})", h.count), theme::font(12.0), t.text_dim);
    if cresp.clicked()
        && let Some(id) = h.first
    {
        let key = (h.uid, id);
        if open {
            app.sorter.collapsed.insert(key);
        } else {
            app.sorter.collapsed.remove(&key);
        }
    } else if resp.double_clicked() && app.sorter.renaming.is_none() {
        app.sorter.renaming = Some((h.uid, h.index, h.name.to_string()));
        app.sorter.rename_started = false;
    }
    let on_chevron = resp.interact_pointer_pos().is_some_and(|p| chev.contains(p));
    if resp.drag_started() && !on_chevron && app.sorter.renaming.is_none() {
        app.sorter.drag = Some((h.index, h.index));
        app.thumb_drag = None;
    }
    let index = h.index;
    let name = h.name.to_string();
    resp.context_menu(|ui| {
        if ui.button("Rename Section…").clicked() {
            let mut d = crate::dialogs::Dialog::new("renameSection");
            d.params = json!({"index": index, "name": name});
            app.dialog = Some(d);
            ui.close();
        }
        if ui.button("Add Section").clicked() {
            if let Some(at) = app.session.active().map(|st| st.selection.slide) {
                let _ = app.run("section.add", json!({"at": at}));
            }
            ui.close();
        }
        if ui.button("Remove Section").clicked() {
            let _ = app.run("section.remove", json!({"index": index}));
            ui.close();
        }
        if ui.button("Remove Section & Slides").clicked() {
            let _ = app.run("section.remove", json!({"index": index, "slides": true}));
            ui.close();
        }
        if ui.button("Move Section Up").clicked() {
            let _ = app.run("section.move", json!({"index": index, "to": index.saturating_sub(1)}));
            ui.close();
        }
        if ui.button("Move Section Down").clicked() {
            let _ = app.run("section.move", json!({"index": index, "to": index + 1}));
            ui.close();
        }
    });
    r
}

struct Grid<'a> {
    doc: &'a Presentation,
    indices: &'a [usize],
    cur: usize,
    multi: &'a [SlideId],
    tw: f32,
    th: f32,
}

/// One section's thumbnails. Returns whether a thumbnail is still being drawn.
fn place_slides(app: &mut SlideApp, ui: &mut Ui, g: &Grid<'_>, budget: &mut u32, rects: &mut Vec<(usize, Rect)>) -> bool {
    let t = Tokens::get(ui.ctx());
    let ppp = ui.ctx().pixels_per_point();
    let cols = ((ui.available_width() - 32.0) / (g.tw + 36.0)).floor().max(1.0) as usize;
    let mut need_more = false;
    let rows = g.indices.len().div_ceil(cols);
    for row in 0..rows {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            for c in 0..cols {
                let Some(&i) = g.indices.get(row * cols + c) else { break };
                let (cell, resp) = ui.allocate_exact_size(vec2(g.tw + 36.0, g.th + 36.0), Sense::click_and_drag());
                let label = format!("Slide {}", i + 1);
                resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
                let img = crate::textures::device_pixel_rect(Rect::from_min_size(cell.min + vec2(18.0, 8.0), vec2(g.tw, g.th)), ppp);
                rects.push((i, cell));
                let p = ui.painter();
                // Only cells in view (plus a little margin) ask for a thumbnail, so a deck bigger
                // than the thumbnail cache does not evict its own thumbnails forever (#969).
                let near = ui.clip_rect().expand2(vec2(0.0, 2.0 * cell.height())).intersects(cell);
                let tex = if near { app.textures.thumb(ui.ctx(), g.doc, i, (img.width() * ppp).round() as u32, budget) } else { None };
                match tex {
                    Some(tex) => {
                        p.image(tex.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        p.rect_filled(img, CornerRadius::ZERO, Color32::WHITE);
                        need_more |= near;
                    }
                }
                let id = g.doc.slides.get(i).map(|s| s.id);
                let selected = i == g.cur || id.is_some_and(|id| g.multi.contains(&id));
                p.rect_stroke(
                    img.expand(if selected { 2.0 } else { 0.0 }),
                    CornerRadius::same(3),
                    Stroke::new(if selected { 3.0 } else { 1.0 }, if selected { t.accent } else { t.border }),
                    egui::StrokeKind::Outside,
                );
                p.text(pos2(img.min.x, img.max.y + 12.0), Align2::LEFT_CENTER, format!("{}", i + 1), theme::font(12.0), t.text_dim);
                if let Some(tr) = g.doc.slides.get(i).and_then(|s| s.transition.as_ref()).filter(|tr| tr.kind != "none") {
                    p.text(pos2(img.max.x, img.max.y + 12.0), Align2::RIGHT_CENTER, format!("★ {}", tr.kind), theme::font(10.5), t.text_faint);
                }
                if resp.clicked() {
                    let (shift, add) = ui.input(|inp| (inp.modifiers.shift, inp.modifiers.command));
                    if shift {
                        let _ = app.run("slide.selectRange", json!({"to": i, "add": add}));
                    } else if add {
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
                if resp.drag_started() && app.sorter.drag.is_none() {
                    app.thumb_drag = Some((i, i));
                }
                resp.context_menu(|ui| crate::thumbs::slide_menu(app, ui, i));
            }
        });
    }
    need_more
}

/// Grid of slides; click selects, double-click opens in Normal view, drag reorders.
/// With sections, each one is a bar: add, rename, collapse, and drag the whole section.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else {
        app.sorter.drag = None;
        app.sorter.renaming = None;
        app.sorter.rename_started = false;
        return;
    };
    let uid = st.uid;
    if app.sorter.renaming.as_ref().is_some_and(|(u, i, _)| *u != uid || *i >= st.doc.sections.len()) {
        app.sorter.renaming = None;
        app.sorter.rename_started = false;
    }
    let doc = st.doc.clone();
    let cur = st.selection.slide;
    let multi = st.selection.slides.clone();
    let zoom = app.ui.sorter_zoom.clamp(0.5, 3.0);
    let grouped = groups(&doc);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            if ui.add(egui::Button::new("Add Section").stroke(Stroke::new(1.0, t.border))).clicked() {
                let _ = app.run("section.add", json!({"at": cur}));
            }
        });
        ui.add_space(8.0);
        let tw = 200.0 * zoom;
        let th = tw * (doc.slide_size.height / doc.slide_size.width.max(1.0)) as f32;
        let mut budget = 4u32;
        let mut rects = vec![];
        let mut headers: Vec<Rect> = Vec::new();
        let mut need_more = false;
        for (si, slides) in &grouped {
            if let Some(si) = *si
                && let Some(sec) = doc.sections.get(si)
            {
                let first = sec.slides.first().map(|id| id.0);
                let rect = section_header(app, ui, &Header { uid, index: si, name: &sec.name, count: sec.slides.len(), first });
                headers.push(rect);
                let folded = first.is_some_and(|id| app.sorter.collapsed.contains(&(uid, id)));
                if folded {
                    ui.add_space(8.0);
                    continue;
                }
            }
            let grid = Grid { doc: &doc, indices: slides, cur, multi: &multi, tw, th };
            if place_slides(app, ui, &grid, &mut budget, &mut rects) {
                need_more = true;
            }
            ui.add_space(8.0);
        }
        if need_more {
            ui.ctx().request_repaint();
        }
        if let Some((from, _)) = app.sorter.drag {
            if let Some(pp) = ui.input(|i| i.pointer.interact_pos()) {
                let centers: Vec<f32> = headers.iter().map(|r| r.center().y).collect();
                let slot = drop_slot(&centers, pp.y);
                app.sorter.drag = Some((from, slot));
                let y = headers.get(slot).map(|r| r.min.y).or_else(|| headers.last().map(|r| r.max.y + 2.0));
                if let Some(y) = y {
                    ui.painter().line_segment([pos2(16.0, y), pos2(ui.available_width(), y)], Stroke::new(2.0, t.accent));
                }
            }
            if ui.input(|i| i.pointer.any_released()) {
                if let Some((from, slot)) = app.sorter.drag.take()
                    && let Some(to) = move_dest(from, slot)
                {
                    let _ = app.run("section.move", json!({"index": from, "to": to}));
                }
                app.sorter.drag = None;
            }
        }
        if app.sorter.drag.is_none()
            && let Some((from, _)) = app.thumb_drag
        {
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
    // The notes master places the slide, the notes and the header and footer text (in Letter
    // points, as the PDF notes pages do).
    let paper = (612.0, 792.0);
    let k = page_w / paper.0 as f32;
    let to_screen = |(x0, y0, x1, y1): (f64, f64, f64, f64)| {
        Rect::from_min_max(page.min + vec2(x0 as f32 * k, y0 as f32 * k), page.min + vec2(x1 as f32 * k, y1 as f32 * k))
    };
    let (slide_box, notes_box) = deckcraft_engine::pdf::notes_frames(&doc, paper);
    // On the physical pixel grid, so the thumbnail is one texel per device pixel (#184).
    let srect = crate::textures::device_pixel_rect(to_screen((slide_box.x0, slide_box.y0, slide_box.x1, slide_box.y1)), ui.ctx().pixels_per_point());
    if let Some(m) = doc.notes_master.as_deref() {
        // Laid out by the text engine as on the PDF page: wrapped, each run in its size and colour.
        for piece in deckcraft_engine::pdf::master_text_pieces(&doc, m, paper, idx + 1) {
            let size = (piece.size as f32 * k).max(4.0);
            let at = page.min + vec2(piece.at.0 as f32 * k, piece.at.1 as f32 * k - size * 0.8);
            let c = Color32::from_rgba_unmultiplied(piece.color.r, piece.color.g, piece.color.b, piece.color.a);
            ui.painter().text(at, egui::Align2::LEFT_TOP, &piece.text, theme::font(size), c);
        }
    }
    let ppp = ui.ctx().pixels_per_point();
    let mut budget = 2;
    if let Some(tex) = app.textures.thumb(ui.ctx(), &doc, idx, (srect.width() * ppp).round() as u32, &mut budget) {
        ui.painter().image(tex.id(), srect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    ui.painter().rect_stroke(srect, CornerRadius::ZERO, Stroke::new(1.0, Color32::GRAY), egui::StrokeKind::Outside);
    let notes_rect = to_screen((notes_box.x0, notes_box.y0, notes_box.x1, notes_box.y1));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(notes_rect));
    // A draft belongs to this document and to the notes it started from (#413): another document,
    // a reorder or an undo that changes them drops it.
    let id = egui::Id::unique(("notespage", st.uid, idx));
    let edit_id = egui::Id::unique(("notespage-edit", idx));
    let focused = child.ctx().memory(|m| m.has_focus(edit_id));
    let notes_of = |app: &SlideApp| app.session.active().and_then(|st| st.doc.slides.get(idx)).map(|s| s.notes.clone()).unwrap_or_default();
    let notes = notes_of(app);
    let stored: Option<(deckcraft_model::text::TextBody, crate::notes::Draft)> = child.data_mut(|d| d.get_temp(id));
    let mut draft = stored.filter(|(base, _)| *base == notes).map(|(_, d)| d);
    // Save an edited draft once the field has lost focus, before anything reloads it.
    if !focused && let Some(d) = draft.as_mut().filter(|d| d.edited) {
        d.edited = false;
        let _ = app.run("slide.notes", json!({"body": d.body, "index": idx}));
    }
    let notes = notes_of(app);
    let mut draft = draft.filter(|d| focused || d.body == notes).unwrap_or_else(|| crate::notes::Draft::new(&notes));
    let look = crate::notes::Look { size: 14.0, color: Some(Color32::from_gray(30)) };
    let width = notes_rect.width();
    let r = crate::notes::editor(&mut child, &mut draft, &look, |e| {
        e.id(edit_id).hint_text(crate::i18n::tr("Click to add text")).desired_width(width).desired_rows(12).frame(egui::Frame::NONE)
    })
    .response
    .response;
    if r.lost_focus() && draft.edited {
        draft.edited = false;
        let _ = app.run("slide.notes", json!({"body": draft.body, "index": idx}));
    }
    let base = notes_of(app);
    child.data_mut(|d| d.insert_temp(id, (base, draft)));
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
            // Only the slides in view (and the current one, whose editor may have the focus) are
            // laid out; the rest take their last measured height (estimated until first shown).
            // Laying out every slide's editors each frame made a 2000-slide outline crawl.
            egui::ScrollArea::vertical().auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
                let origin = ui.cursor().top();
                let margin = 300.0;
                for (i, s) in doc.slides.iter().enumerate() {
                    let hid = egui::Id::unique(("ol-height", s.id.0));
                    let known: Option<f32> = ui.data(|d| d.get_temp(hid));
                    let h = known.unwrap_or_else(|| outline_estimate(s));
                    let y = ui.cursor().top() - origin;
                    if i == cur || (y + h >= viewport.min.y - margin && y <= viewport.max.y + margin) {
                        outline_slide(app, ui, &t, i, s, cur);
                        let real = ui.cursor().top() - origin - y;
                        ui.data_mut(|d| d.insert_temp(hid, real));
                    } else {
                        ui.allocate_exact_size(vec2(ui.available_width(), (h - ui.spacing().item_spacing.y).max(0.0)), Sense::hover());
                    }
                }
            });
        },
    );
}

#[cfg(test)]
#[path = "sorter_tests.rs"]
mod ui_tests;

/// A slide's outline block height before it has been laid out: title row plus its text lines.
fn outline_estimate(s: &deckcraft_model::Slide) -> f32 {
    let lines: usize = s
        .shapes
        .iter()
        .filter(|x| !x.ph_type().is_some_and(|k| k.is_title()))
        .filter_map(|x| x.text.as_ref())
        .filter(|tb| !tb.is_empty())
        .map(|tb| tb.paragraphs.len().max(1))
        .sum();
    30.0 + lines as f32 * 19.0
}

/// One slide in the outline: number, title editor, then its other text.
fn outline_slide(app: &mut SlideApp, ui: &mut Ui, t: &Tokens, i: usize, s: &deckcraft_model::Slide, cur: usize) {
    let title_shape = s.shapes.iter().find(|x| x.ph_type().is_some_and(|k| k.is_title())).map(|x| x.id.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{}", i + 1)).color(t.text_dim));
        let (r, _) = ui.allocate_exact_size(vec2(14.0, 10.0), Sense::hover());
        ui.painter().rect_stroke(r, CornerRadius::same(1), Stroke::new(1.0, if i == cur { t.accent } else { t.text_dim }), egui::StrokeKind::Inside);
        let id = egui::Id::unique(("ol-title", s.id.0));
        let cur_title = s.title();
        let mut v: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_else(|| cur_title.clone()));
        let resp = ui.add(
            egui::TextEdit::singleline(&mut v).hint_text("Slide title").font(theme::bold(14.0)).frame(egui::Frame::NONE).desired_width(f32::INFINITY),
        );
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
        let id = egui::Id::unique(("ol-body", s.id.0, sh.id.0));
        let cur_text: String = tb.paragraphs.iter().map(|p| format!("{}{}", "\t".repeat(p.level as usize), p.text())).collect::<Vec<_>>().join("\n");
        let mut v: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_else(|| cur_text.clone()));
        ui.horizontal(|ui| {
            ui.add_space(36.0);
            let resp = ui
                .add(egui::TextEdit::multiline(&mut v).hint_text("Slide text").frame(egui::Frame::NONE).desired_width(f32::INFINITY).desired_rows(1));
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
