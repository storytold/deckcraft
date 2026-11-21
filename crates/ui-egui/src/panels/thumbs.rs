//! The slide thumbnail pane (Normal view): numbered thumbnails grouped by section, selection,
//! drag-to-reorder, and the slide context menu.

use std::collections::HashSet;

use deckcraft_engine::{Mods, Target};
use deckcraft_model::SlideId;
use egui::{Align2, Color32, CornerRadius, Rect, Role, Sense, Stroke, Ui, WidgetInfo, pos2, vec2};
use serde_json::{Value, json};

use crate::SlideApp;
use crate::theme::{self, Tokens};

/// A slide drag in the thumbnail pane or the Slide Sorter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlideDrag {
    /// The presentation the drag started in; it is dropped, not applied, anywhere else.
    pub uid: u64,
    /// The dragged slides, in deck order.
    pub ids: Vec<SlideId>,
    /// The gap under the pointer: 0 is before the first slide, the slide count after the last.
    pub slot: usize,
}

/// Starts dragging slide `i`: the whole slide selection when `i` is part of it, else `i` alone,
/// which then becomes the selection, as in PowerPoint.
pub(crate) fn start_slide_drag(app: &mut SlideApp, i: usize) {
    let Some(st) = app.session.active() else { return };
    let Some(id) = st.doc.slides.get(i).map(|s| s.id) else { return };
    let selected: HashSet<SlideId> = st.selection.slides.iter().copied().collect();
    let ids: Vec<SlideId> =
        if selected.contains(&id) { st.doc.slides.iter().map(|s| s.id).filter(|x| selected.contains(x)).collect() } else { vec![id] };
    let uid = st.uid;
    if !selected.contains(&id) {
        let _ = app.run("slide.go", json!({"index": i}));
    }
    app.thumb_drag = Some(SlideDrag { uid, ids, slot: i });
}

/// Whether a slide drag is under way in the active presentation. `button` is whether the
/// primary button is held or was released this frame. A drag begun in another presentation, or
/// one whose release this view never saw (the view changed under it), is discarded, so it never
/// moves slides on some later, unrelated release.
pub(crate) fn slide_drag_live(app: &mut SlideApp, button: bool) -> bool {
    let uid = app.session.active().map(|st| st.uid);
    if !button || app.thumb_drag.as_ref().is_some_and(|d| Some(d.uid) != uid) {
        app.thumb_drag = None;
    }
    app.thumb_drag.is_some()
}

/// Whether the primary button is held or was released this frame: a slide drag is still alive.
pub(crate) fn drag_button(ui: &Ui) -> bool {
    ui.input(|i| i.pointer.primary_down() || i.pointer.any_released())
}

/// `slide.move`'s `to` for slides at `moving` (ascending indices) dropped at gap `slot`: the gap
/// counted without them. `None` when the drop leaves the deck as it is.
pub(crate) fn slides_move_dest(moving: &[usize], slot: usize) -> Option<usize> {
    let first = *moving.first()?;
    let dest = slot.saturating_sub(moving.iter().filter(|&&i| i < slot).count());
    let one_run = moving.iter().enumerate().all(|(k, &i)| first.checked_add(k) == Some(i));
    if one_run && dest == first { None } else { Some(dest) }
}

/// Ends the slide drag: one `slide.move` (one undo step) for all the dragged slides, unless the
/// drop leaves them where they are, the drag began in another presentation, or one of them is gone.
pub(crate) fn drop_slide_drag(app: &mut SlideApp) {
    let Some(drag) = app.thumb_drag.take() else { return };
    let Some(st) = app.session.active().filter(|st| st.uid == drag.uid) else { return };
    let dragged: HashSet<SlideId> = drag.ids.iter().copied().collect();
    let moving: Vec<usize> = st.doc.slides.iter().enumerate().filter(|(_, s)| dragged.contains(&s.id)).map(|(i, _)| i).collect();
    if moving.len() != dragged.len() {
        return;
    }
    if let Some(to) = slides_move_dest(&moving, drag.slot) {
        let ids: Vec<u32> = drag.ids.iter().map(|id| id.0).collect();
        let _ = app.run("slide.move", json!({"ids": ids, "to": to}));
    }
}

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

/// Slide index -> the first section starting there, built once instead of searched per slide.
fn section_starts(doc: &deckcraft_model::Presentation) -> std::collections::HashMap<usize, usize> {
    let mut index_of = std::collections::HashMap::new();
    for (i, s) in doc.slides.iter().enumerate() {
        index_of.entry(s.id).or_insert(i);
    }
    let mut starts = std::collections::HashMap::new();
    for (si, s) in doc.sections.iter().enumerate() {
        if let Some(&i) = s.slides.first().and_then(|id| index_of.get(id)) {
            starts.entry(i).or_insert(si);
        }
    }
    starts
}

/// Render the scrollable thumbnail list, scrolling the current slide into view when it changes.
pub(crate) fn list(app: &mut SlideApp, ui: &mut Ui) {
    app.thumbs_rect = Some(ui.clip_rect());
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
    let section_starts = section_starts(&doc);
    let mut need_more = false;
    // The canvas shell lists a slide and its states (slides after it entering with Morph) once.
    let grouped = groups_states(app);
    // When the current slide changes (arrow keys, the canvas, a command), bring it into view.
    let seen_id = ui.scope_id().with("thumbs_current");
    let now = (app.session.active().map(|d| d.uid).unwrap_or(0), cur);
    let scroll_to_current = ui.data_mut(|d| d.get_temp::<(u64, usize)>(seen_id)) != Some(now);
    ui.data_mut(|d| d.insert_temp(seen_id, now));
    for i in 0..doc.slides.len() {
        if grouped && crate::states::is_state_step(&doc, i) {
            continue;
        }
        let run = if grouped { crate::states::run_of(&doc, i) } else { i..i + 1 };
        if let Some(si) = section_starts.get(&i)
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
                if ui.button("Rename Section…").clicked() {
                    let mut d = crate::dialogs::Dialog::new("renameSection");
                    d.params = json!({"index": si, "name": sec.name});
                    app.dialog = Some(d);
                    ui.close();
                }
                if ui.button("Remove Section").clicked() {
                    let _ = app.run("section.remove", json!({"index": si}));
                    ui.close();
                }
                if ui.button("Remove Section & Slides").clicked() {
                    let _ = app.run("section.remove", json!({"index": si, "slides": true}));
                    ui.close();
                }
                if ui.button("Move Section Up").clicked() {
                    let _ = app.run("section.move", json!({"index": si, "to": si.saturating_sub(1)}));
                    ui.close();
                }
                if ui.button("Move Section Down").clicked() {
                    let _ = app.run("section.move", json!({"index": si, "to": si + 1}));
                    ui.close();
                }
            });
        }
        let (row, resp) = ui.allocate_exact_size(vec2(width, th + 16.0), Sense::click_and_drag());
        let label = format!("Slide {}", i + 1);
        resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
        let img = crate::textures::device_pixel_rect(Rect::from_min_size(pos2(row.min.x + 30.0, row.min.y + 6.0), vec2(tw, th)), ppp);
        rects.push((i, row));
        if run.contains(&cur) && scroll_to_current {
            ui.scroll_to_rect(row, None);
        }
        let p = ui.painter();
        let selected = run.clone().any(|k| k == cur || doc.slides.get(k).is_some_and(|s| multi.contains(&s.id)));
        let hidden = doc.slides.get(i).is_some_and(|s| s.hidden);
        p.text(
            pos2(row.min.x + 14.0, img.min.y + 2.0),
            Align2::CENTER_TOP,
            format!("{}", i + 1),
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
        // While the list has the keyboard its selected rows are tinted, so it's clear where ⌘D and
        // Delete go.
        if selected && app.slides_focus {
            p.rect_filled(row.shrink2(vec2(4.0, 0.0)), CornerRadius::same(6), t.accent.gamma_multiply(0.14));
        }
        let wpx = (img.width() * ppp).round() as u32;
        // Only rows in view (plus a little margin) ask for a thumbnail: with every row asking, a
        // deck bigger than the thumbnail cache evicts its own thumbnails and never settles (#969).
        let near = ui.clip_rect().expand2(vec2(0.0, 2.0 * row.height())).intersects(row);
        let tex = if near { app.textures.thumb(ui.ctx(), &doc, i, wpx, app.ui.grayscale, &mut budget) } else { None };
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
        if run.len() > 1 {
            // "3 states" in a pill on the thumbnail's bottom right.
            let text = format!("{} states", run.len());
            let galley = ui.painter().layout_no_wrap(text, theme::bold(10.5), Color32::WHITE);
            let pill = Rect::from_min_size(
                pos2(img.max.x - galley.size().x - 14.0, img.max.y - galley.size().y - 10.0),
                vec2(galley.size().x + 10.0, galley.size().y + 4.0),
            );
            ui.painter().rect_filled(pill, CornerRadius::same(8), Color32::from_black_alpha(150));
            ui.painter().galley(pill.min + vec2(5.0, 2.0), galley, Color32::WHITE);
        }
        let p = ui.painter();
        if selected {
            // Accent while the list has the keyboard, grey when it doesn't (as macOS lists do).
            let c = if app.slides_focus { t.accent } else { t.text_faint };
            p.rect_stroke(img.expand(2.0), CornerRadius::same(4), Stroke::new(3.0, c), egui::StrokeKind::Outside);
        } else {
            p.rect_stroke(
                img,
                CornerRadius::ZERO,
                Stroke::new(if resp.hovered() { 2.0 } else { 1.0 }, if resp.hovered() { t.text_faint } else { t.border }),
                egui::StrokeKind::Outside,
            );
        }
        thumb_input(app, ui, &resp, &doc, i, &run, cur, grouped);
        resp.context_menu(|ui| slide_menu(app, ui, i));
    }
    if need_more {
        ui.ctx().request_repaint();
    }
    // Scroll the current slide's thumbnail into view when the slide changes (#917).
    if slide_changed && let Some((_, row_rect)) = rects.iter().find(|(i, _)| *i == cur) {
        ui.scroll_to_rect(*row_rect, None);
    }
    // Drag reorder: insertion line (for a drag started here, not in the grid).
    if !app.thumb_drag_in_grid && slide_drag_live(app, drag_button(ui)) {
        if let Some(pp) = ui.input(|i| i.pointer.interact_pos()) {
            let mut to = doc.slides.len();
            for (i, r) in &rects {
                if pp.y < r.center().y {
                    to = *i;
                    break;
                }
            }
            if let Some(d) = app.thumb_drag.as_mut() {
                d.slot = to;
            }
            let y = rects.iter().find(|(i, _)| *i == to).map(|(_, r)| r.min.y + 2.0).or_else(|| rects.last().map(|(_, r)| r.max.y)).unwrap_or(0.0);
            let x0 = ui.min_rect().left() + 26.0;
            crate::widgets::insertion_line(ui.painter(), x0, ui.min_rect().left() + width - 4.0, y, t.accent);
        }
        if ui.input(|i| i.pointer.any_released()) {
            drop_slide_drag(app);
        }
    }
    // Click on empty space below: new slide prompt.
    let (r, resp) = ui.allocate_exact_size(vec2(width, 40.0), Sense::click());
    if resp.hovered() {
        ui.painter().text(r.center(), Align2::CENTER_CENTER, "+ New Slide", theme::font(12.0), t.text_faint);
    }
    if resp.clicked() {
        let _ = app.run("slide.new", json!({}));
    }
}

/// A thumbnail's clicks and drag: the selection (a slide with its states in the canvas list),
/// the keyboard for the list, the show on a double-click, and a slide drag.
#[allow(clippy::too_many_arguments)]
fn thumb_input(
    app: &mut SlideApp,
    ui: &Ui,
    resp: &egui::Response,
    doc: &deckcraft_model::Presentation,
    i: usize,
    run: &std::ops::Range<usize>,
    cur: usize,
    grouped: bool,
) {
    let id = doc.slides.get(i).map(|s| s.id);
    if resp.clicked() || resp.drag_started() {
        app.slides_focus = true;
        app.states_focus = false;
    }
    if resp.clicked() {
        let (shift, cmd) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        if !shift {
            app.slide_anchor = Some(i);
        }
        app.slide_cursor = Some(i);
        if id.is_some() {
            let ids_in = |r: std::ops::Range<usize>| -> Vec<u32> { doc.slides.get(r).unwrap_or(&[]).iter().map(|s| s.id.0).collect() };
            if shift && grouped {
                // A slide's states go with it.
                let (a, b) = (cur.min(i), cur.max(i));
                let ids = ids_in(crate::states::run_of(doc, a).start..crate::states::run_of(doc, b).end);
                let _ = app.run("slide.selectSlides", json!({"ids": ids}));
                let _ = app.run("slide.go", json!({"index": i}));
                let _ = app.run("slide.selectSlides", json!({"ids": ids}));
            } else if shift {
                // Shift: the slides between the current one and this one. Ctrl+Shift keeps what is selected.
                let _ = app.run("slide.selectRange", json!({"to": i, "add": cmd}));
            } else if cmd {
                let _ = app.run("slide.selectSlides", json!({"ids": ids_in(run.clone()), "add": true}));
            } else {
                // A run's entry keeps the state being edited current.
                let to = if run.contains(&cur) { cur } else { i };
                let _ = app.run("slide.go", json!({"index": to}));
            }
        }
    }
    if resp.double_clicked() {
        app.start_show(i, false);
    }
    if resp.drag_started() {
        start_slide_drag(app, i);
        app.thumb_drag_in_grid = false;
        // A slide drags with its states.
        if grouped
            && run.len() > 1
            && let Some(d) = app.thumb_drag.as_mut()
        {
            let have: HashSet<SlideId> = d.ids.iter().copied().collect();
            let mut ids: Vec<SlideId> = Vec::new();
            for (k, s) in doc.slides.iter().enumerate() {
                if have.contains(&s.id) || run.contains(&k) {
                    ids.push(s.id);
                }
            }
            d.ids = ids;
        }
    }
}

/// Does the slide list show a slide's states as one entry (the canvas shell's list; the grid
/// shows every slide)?
pub fn groups_states(app: &SlideApp) -> bool {
    app.ui.shell == crate::Shell::Canvas && app.ui.view != crate::ViewMode::Sorter
}

/// The `slide.move` steps that move the slides in `block` to just before slide `to` (`to` past
/// the last slide: to the end), keeping their order. Nothing when `to` is inside the block or
/// right after it.
pub fn block_moves(block: std::ops::Range<usize>, to: usize) -> Vec<(usize, usize)> {
    let n = block.len();
    if to <= block.start {
        (0..n).map(|k| (block.start + k, to + k)).filter(|(f, t)| f != t).collect()
    } else if to > block.end {
        (0..n).map(|_| (block.start, to - 1)).collect()
    } else {
        vec![]
    }
}

/// Grouped list: widen the slide selection to whole runs of states (the current slide stays
/// current), so Delete, Duplicate, Cut and Copy take a slide's states with it.
pub fn select_runs(app: &mut SlideApp) {
    if !groups_states(app) {
        return;
    }
    let Some(st) = app.session.active() else { return };
    let doc = &st.doc;
    let cur = st.selection.slide;
    let mut picked: Vec<usize> = st.selection.slides.iter().filter_map(|id| doc.slide_index(*id)).collect();
    picked.push(cur);
    let mut all: Vec<usize> = picked.iter().flat_map(|i| crate::states::run_of(doc, *i)).collect();
    all.sort_unstable();
    all.dedup();
    if all.len() <= picked.len() && all.len() <= 1 {
        return;
    }
    let mut ids: Vec<u32> = all.iter().filter(|i| **i != cur).filter_map(|i| doc.slides.get(*i)).map(|s| s.id.0).collect();
    ids.extend(doc.slides.get(cur).map(|s| s.id.0));
    let _ = app.run("slide.selectSlides", json!({"ids": ids}));
}

/// Keys for the slide list while it has the keyboard (`SlideApp::slides_focus`), as in Keynote
/// and PowerPoint: ⌘D duplicates the selected slides, Delete removes them, ⇧⌘H hides or shows
/// them (⌘H is the Mac's Hide app), ⌘↑ / ⌘↓ (or ⌘← / ⌘→) move them one place and ⇧⌘↑ / ⇧⌘↓ to
/// the start or end, ↑ / ↓ go to the previous or next slide and ⇧↑ / ⇧↓ extend the selection,
/// ⌘A selects every slide. True when consumed.
pub fn slide_key(app: &mut SlideApp, key: egui::Key, m: Mods) -> bool {
    use egui::Key::*;
    let Some(st) = app.session.active() else { return false };
    let n = st.doc.slides.len();
    let cur = st.selection.slide;
    let ids: Vec<u32> = st.doc.slides.iter().map(|s| s.id.0).collect();
    let mut selected: Vec<usize> = st.selection.slides.iter().filter_map(|id| st.doc.slide_index(*id)).collect();
    if selected.is_empty() {
        selected.push(cur);
    }
    selected.sort_unstable();
    let run = |app: &mut SlideApp, id: &str, p: Value| {
        if let Err(e) = app.run(id, p) {
            app.set_status(e);
        }
    };
    let back = matches!(key, ArrowUp | ArrowLeft);
    let forth = matches!(key, ArrowDown | ArrowRight);
    if groups_states(app) && grouped_key(app, m, back, forth) {
        return true;
    }
    match (key, m.cmd, m.shift, m.alt) {
        (D, true, false, false) => {
            select_runs(app);
            run(app, "slide.duplicate", json!({}));
        }
        (Delete | Backspace, false, false, false) => {
            select_runs(app);
            run(app, "slide.delete", json!({}));
        }
        (H, true, true, false) => run(app, "slide.hide", json!({})),
        (A, true, false, false) => run(app, "slide.selectSlides", json!({"ids": ids})),
        (_, true, shift, false) if back || forth => {
            let order = moves(&selected, n, forth, shift);
            if order.is_empty() {
                return true;
            }
            let keep: Vec<u32> = selected.iter().filter_map(|i| ids.get(*i).copied()).collect();
            let cmds = order.into_iter().map(|(from, to)| ("slide.move", json!({"from": from, "to": to}))).collect();
            app.run_as_one(if keep.len() > 1 { "Move Slides" } else { "Move Slide" }, cmds);
            // The moved slides stay selected, the current one last.
            let current = st_id(app, cur, &ids);
            let mut sel: Vec<u32> = keep.into_iter().filter(|id| Some(*id) != current).collect();
            sel.extend(current);
            run(app, "slide.selectSlides", json!({"ids": sel}));
        }
        (_, false, true, false) if back || forth => {
            let cursor = app.slide_cursor.filter(|c| *c < n).unwrap_or(cur);
            let anchor = app.slide_anchor.filter(|a| *a < n).unwrap_or(cursor);
            let to = if back { cursor.saturating_sub(1) } else { (cursor + 1).min(n.saturating_sub(1)) };
            let (a, b) = (anchor.min(to), anchor.max(to));
            // The cursor's slide last: it becomes the current one.
            let mut sel: Vec<u32> = (a..=b).filter(|i| *i != to).filter_map(|i| ids.get(i).copied()).collect();
            sel.extend(ids.get(to));
            run(app, "slide.selectSlides", json!({"ids": sel}));
            app.slide_anchor = Some(anchor);
            app.slide_cursor = Some(to);
        }
        (_, false, false, false) if back || forth || matches!(key, Home | End) => {
            let to = match key {
                Home => 0,
                End => n.saturating_sub(1),
                _ if back => cur.saturating_sub(1),
                _ => (cur + 1).min(n.saturating_sub(1)),
            };
            run(app, "slide.go", json!({"index": to}));
            app.slide_anchor = Some(to);
            app.slide_cursor = Some(to);
        }
        _ => return false,
    }
    true
}

/// Keys that work on whole runs of states in the grouped list: ↑ / ↓ go to the previous or next
/// entry, ⌘↑ / ⌘↓ move the current one past its neighbour (⇧⌘ to the start or end).
fn grouped_key(app: &mut SlideApp, m: Mods, back: bool, forth: bool) -> bool {
    use crate::states::run_of;
    let Some(st) = app.session.active() else { return false };
    let doc = st.doc.clone();
    let n = doc.slides.len();
    let cur = st.selection.slide;
    let here = run_of(&doc, cur);
    if !(back || forth) || m.alt || n == 0 {
        return false;
    }
    match (m.cmd, m.shift) {
        (false, false) => {
            let to = if back { run_of(&doc, here.start.saturating_sub(1)).start } else { here.end.min(n - 1) };
            let to = if forth && here.end >= n { here.start } else { to };
            let _ = app.run("slide.go", json!({"index": to}));
            app.slide_anchor = Some(to);
            app.slide_cursor = Some(to);
            true
        }
        (true, shift) => {
            let to = match (back, shift) {
                (true, true) => 0,
                (true, false) if here.start > 0 => run_of(&doc, here.start - 1).start,
                (false, true) => n,
                (false, false) if here.end < n => run_of(&doc, here.end).end,
                _ => return true,
            };
            let cmds: Vec<(&str, Value)> =
                block_moves(here.clone(), to).into_iter().map(|(f, t)| ("slide.move", json!({"from": f, "to": t}))).collect();
            if cmds.is_empty() {
                return true;
            }
            let id = doc.slides.get(cur).map(|s| s.id.0);
            app.run_as_one(if here.len() > 1 { "Move Slides" } else { "Move Slide" }, cmds);
            if let Some(id) = id {
                let _ = app.run("slide.go", json!({"id": id}));
            }
            true
        }
        // ⇧↑ / ⇧↓ extend the selection as in the plain list.
        (false, true) => false,
    }
}

/// The current slide's id after slides moved (it moved with them).
fn st_id(app: &SlideApp, cur: usize, before: &[u32]) -> Option<u32> {
    let id = before.get(cur).copied()?;
    app.session.active()?.doc.slides.iter().any(|s| s.id.0 == id).then_some(id)
}

/// The `slide.move` steps that move the `selected` slides (sorted indices) one place back or
/// forth, or (`to_end`) to the start or end, keeping their order. Each step's indices hold after
/// the steps before it.
pub fn moves(selected: &[usize], n: usize, forth: bool, to_end: bool) -> Vec<(usize, usize)> {
    let mut out = vec![];
    if !forth {
        let mut limit = 0;
        for (k, &i) in selected.iter().enumerate() {
            let to = if to_end { k } else { i.saturating_sub(1).max(limit) };
            if to != i {
                out.push((i, to));
            }
            limit = to + 1;
        }
    } else {
        let mut limit = n.saturating_sub(1);
        for (k, &i) in selected.iter().rev().enumerate() {
            let to = if to_end { n.saturating_sub(1 + k) } else { (i + 1).min(limit) };
            if to != i {
                out.push((i, to));
            }
            limit = to.saturating_sub(1);
        }
    }
    out
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
        if ui.button(l).clicked() {
            if id != "edit.paste" {
                select_runs(app);
            }
            let _ = app.run(id, p);
            ui.close();
        }
    }
    ui.separator();
    if ui.button("New Slide").clicked() {
        let _ = app.run("slide.new", json!({}));
        ui.close();
    }
    if ui.button("Duplicate Slide").clicked() {
        select_runs(app);
        let _ = app.run("slide.duplicate", json!({}));
        ui.close();
    }
    if ui.button("Delete Slide").clicked() {
        select_runs(app);
        let _ = app.run("slide.delete", json!({}));
        ui.close();
    }
    ui.separator();
    if ui.button("Add Section").clicked() {
        let _ = app.run("section.add", json!({"at": i}));
        ui.close();
    }
    ui.menu_button("Layout", |ui| {
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
    if ui.button("Reset Slide").clicked() {
        // The canvas layout keeps the slide's layers (no empty placeholders added).
        let adopt = app.ui.shell == crate::Shell::Canvas;
        let _ = app.run("slide.reset", json!({"adopt": adopt}));
        ui.close();
    }
    if ui.button("Format Background…").clicked() {
        let _ = app.run("view.pane", json!({"pane": "background"}));
        ui.close();
    }
    let hidden = app.session.active().and_then(|d| d.doc.slides.get(i).map(|s| s.hidden)).unwrap_or(false);
    if ui.button(if hidden { "Show Slide" } else { "Hide Slide" }).clicked() {
        let _ = app.run("slide.hide", json!({"index": i}));
        ui.close();
    }
}

pub(crate) fn master_list(app: &mut SlideApp, ui: &mut Ui) {
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
                    None => format!("{} Slide Master", m.name),
                };
                resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
                let resp = resp.on_hover_text(&name);
                let pick = |app: &mut SlideApp| match li {
                    Some(l) => {
                        let _ = app.run("view.slideMaster", json!({"master": mi, "layout": l}));
                    }
                    // `layout: null` picks the master itself (no layout opens the current slide's).
                    None => {
                        let _ = app.run("view.slideMaster", json!({"master": mi, "layout": null}));
                    }
                };
                if resp.clicked() {
                    pick(app);
                }
                if li.is_some() {
                    resp.context_menu(|ui| {
                        pick(app);
                        if ui.button("Rename Layout…").clicked() {
                            app.dialog = Some(crate::dialogs::Dialog::new("renameLayout"));
                            ui.close();
                        }
                        if ui.button("New Layout").clicked() {
                            let _ = app.run("master.insertLayout", json!({}));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Delete Layout").clicked() {
                            let _ = app.run("master.deleteLayout", json!({}));
                            ui.close();
                        }
                    });
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

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

    #[test]
    fn section_starts_map_each_first_slide_to_its_first_section() {
        let mut s = Session::with_new();
        for _ in 0..3 {
            s.execute("slide.new", &json!({})).unwrap();
        }
        let mut doc = (*s.active().unwrap().doc).clone();
        let ids: Vec<_> = doc.slides.iter().map(|sl| sl.id).collect();
        doc.sections = vec![
            deckcraft_model::Section { name: "a".into(), slides: vec![ids[0], ids[1]] },
            deckcraft_model::Section { name: "dup".into(), slides: vec![ids[0]] },
            deckcraft_model::Section { name: "c".into(), slides: vec![ids[2]] },
            deckcraft_model::Section { name: "empty".into(), slides: vec![] },
        ];
        let starts = section_starts(&doc);
        assert_eq!(starts.get(&0), Some(&0), "the first section starting at a slide wins");
        assert_eq!(starts.get(&2), Some(&2));
        assert_eq!(starts.len(), 2);
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

    /// A tall window, so every thumbnail of a small deck is on screen.
    fn tall_harness(slides: usize) -> (Harness<'static, SlideApp>, Vec<u32>) {
        let mut s = Session::with_new();
        while s.doc().unwrap().doc.slides.len() < slides {
            s.execute("slide.new", &json!({})).unwrap();
        }
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 1600.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(s, Services::default()),
        );
        h.run_steps(3);
        let ids = order(&h);
        (h, ids)
    }

    fn order(h: &Harness<'_, SlideApp>) -> Vec<u32> {
        h.state().session.active().unwrap().doc.slides.iter().map(|s| s.id.0).collect()
    }

    #[test]
    fn slides_move_dest_counts_without_the_moved_slides() {
        // One slide: the gaps on either side of it leave it where it is.
        assert_eq!(slides_move_dest(&[1], 1), None);
        assert_eq!(slides_move_dest(&[1], 2), None);
        assert_eq!(slides_move_dest(&[1], 0), Some(0));
        assert_eq!(slides_move_dest(&[1], 4), Some(3));
        // A run: any gap inside or around it is a no-op.
        for slot in 2..=5 {
            assert_eq!(slides_move_dest(&[2, 3, 4], slot), None, "slot {slot}");
        }
        assert_eq!(slides_move_dest(&[2, 3, 4], 0), Some(0));
        assert_eq!(slides_move_dest(&[2, 3, 4], 6), Some(3));
        // Scattered slides always gather, even at a gap between them.
        assert_eq!(slides_move_dest(&[0, 2], 1), Some(0));
        assert_eq!(slides_move_dest(&[0, 2], 0), Some(0));
        assert_eq!(slides_move_dest(&[1, 3], 5), Some(3));
        assert_eq!(slides_move_dest(&[], 3), None);
        assert_eq!(slides_move_dest(&[usize::MAX], usize::MAX), None);
    }

    #[test]
    fn dragging_selected_thumbnails_moves_them_together() {
        let (mut h, ids) = tall_harness(4);
        h.state_mut().run("slide.selectSlides", json!({"ids": [ids[0], ids[2]]})).unwrap();
        h.run_steps(2);
        let (first, last) = (h.get_by_label("Slide 1").rect(), h.get_by_label("Slide 4").rect());
        // Below the middle of the last thumbnail: the gap after it.
        crate::sorter::ui_tests::drag(&mut h, first.center(), egui::pos2(last.center().x, last.bottom() - 4.0));
        assert_eq!(order(&h), [ids[1], ids[3], ids[0], ids[2]]);
        let sel: Vec<u32> = h.state().session.active().unwrap().selection.slides.iter().map(|s| s.0).collect();
        assert_eq!(sel, [ids[0], ids[2]], "the moved slides stay selected");
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(order(&h), ids, "one undo puts both back");
    }

    #[test]
    fn dragging_an_unselected_thumbnail_moves_only_it() {
        let (mut h, ids) = tall_harness(4);
        h.state_mut().run("slide.selectSlides", json!({"ids": [ids[0], ids[2]]})).unwrap();
        h.run_steps(2);
        let (second, first) = (h.get_by_label("Slide 2").rect(), h.get_by_label("Slide 1").rect());
        crate::sorter::ui_tests::drag(&mut h, second.center(), egui::pos2(first.center().x, first.top() + 4.0));
        assert_eq!(order(&h), [ids[1], ids[0], ids[2], ids[3]]);
        let sel: Vec<u32> = h.state().session.active().unwrap().selection.slides.iter().map(|s| s.0).collect();
        assert_eq!(sel, [ids[1]]);
    }

    /// A drag whose release no view saw (a double-click in the Slide Sorter opens Normal view
    /// mid-gesture) is dropped, not left to move slides on the next unrelated release.
    #[test]
    fn a_slide_drag_without_a_held_button_is_dropped() {
        let (mut h, ids) = tall_harness(3);
        let app = h.state_mut();
        app.run("slide.selectSlides", json!({"ids": [ids[0], ids[1]]})).unwrap();
        start_slide_drag(app, 0);
        if let Some(d) = app.thumb_drag.as_mut() {
            d.slot = 3;
        }
        h.run_steps(2);
        assert!(h.state().thumb_drag.is_none(), "no button held: the drag is over");
        let last = h.get_by_label("Slide 3").rect();
        h.hover_at(last.center());
        h.run_steps(1);
        h.event(egui::Event::PointerButton {
            pos: last.center(),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(2);
        assert_eq!(order(&h), ids, "a later release moves nothing");
    }

    /// A drag begun in one presentation never moves slides in another (switching mid-drag).
    #[test]
    fn a_slide_drag_does_not_follow_into_another_presentation() {
        let (mut h, ids) = tall_harness(3);
        let app = h.state_mut();
        app.run("slide.selectSlides", json!({"ids": [ids[0], ids[1]]})).unwrap();
        start_slide_drag(app, 0);
        let drag = app.thumb_drag.clone().unwrap();
        assert_eq!(drag.ids.iter().map(|s| s.0).collect::<Vec<_>>(), [ids[0], ids[1]]);
        app.thumb_drag = Some(SlideDrag { slot: 3, ..drag.clone() });
        app.run("file.new", json!({})).unwrap();
        while app.session.active().unwrap().doc.slides.len() < 3 {
            app.run("slide.new", json!({})).unwrap();
        }
        let other = app.session.active().unwrap().doc.slides.iter().map(|s| s.id.0).collect::<Vec<_>>();
        assert!(!slide_drag_live(app, true), "the drag belongs to the first presentation");
        app.thumb_drag = Some(SlideDrag { slot: 3, ..drag });
        drop_slide_drag(app);
        assert!(app.thumb_drag.is_none());
        assert_eq!(app.session.active().unwrap().doc.slides.iter().map(|s| s.id.0).collect::<Vec<_>>(), other);
        app.session.set_active(0);
        assert_eq!(order(&h), ids, "and the first presentation is untouched");
    }
}
