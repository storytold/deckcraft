//! Selection pane: every object on the slide, front-most first, groups expanded with their
//! children. Click to select (Ctrl toggles, Shift selects a run), drag to restack among
//! siblings, eye to show or hide, padlock to lock, double-click or F2 to rename, arrows to move
//! through the list. Everything it changes goes through engine commands.

use std::collections::HashSet;

use deckcraft_model::Shape;
use egui::{Align2, CornerRadius, Id, Rect, Role, Sense, Stroke, Ui, WidgetInfo, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

pub const ROW_H: f32 = 24.0;
const INDENT: f32 = 14.0;

/// Pane-only state (nothing here is part of the document).
#[derive(Debug, Default)]
pub struct State {
    /// Groups folded shut.
    pub collapsed: HashSet<u32>,
    /// Object being renamed and the text typed so far.
    pub renaming: Option<(u32, String)>,
    rename_started: bool,
    /// Last row clicked without Shift (the start of a Shift+click run).
    pub anchor: Option<u32>,
    /// Row being dragged and the drop it would make: (index among siblings, line y).
    pub drag: Option<(u32, Option<(usize, f32)>)>,
    /// Document (by uid) the state above belongs to.
    owner: Option<u64>,
}

impl State {
    /// Ties the pane state to a document: shape ids repeat between documents, so switching
    /// documents drops the pending rename, folds, anchor and drag.
    pub fn bind(&mut self, uid: u64) {
        if self.owner != Some(uid) {
            *self = State { owner: Some(uid), ..State::default() };
        }
    }
}

/// One line of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: u32,
    pub name: String,
    pub depth: usize,
    pub group: bool,
    pub hidden: bool,
    /// Inside a hidden group: drawn dimmed.
    pub dimmed: bool,
    pub locked: bool,
    pub parent: Option<u32>,
    /// The placeholder it is on the slide's layout ("Title", "Body text"…), shown after the name.
    pub tag: Option<String>,
    /// What kind of layer it is, as a small icon before the name.
    pub icon: crate::icons::Icon,
}

/// The icon a layer's row shows for its kind.
pub fn kind_icon(sh: &Shape) -> crate::icons::Icon {
    use crate::icons::Icon;
    use deckcraft_model::ShapeKind;
    match &sh.kind {
        ShapeKind::Group { .. } => Icon::Group,
        ShapeKind::Picture { .. } => Icon::Picture,
        ShapeKind::Table(_) => Icon::Table,
        ShapeKind::Chart(_) => Icon::Chart,
        ShapeKind::Media(m) if !m.video => Icon::Audio,
        ShapeKind::Media(_) => Icon::Video,
        ShapeKind::Connector { .. } => Icon::LayerLine,
        _ if sh.is_line() => Icon::LayerLine,
        ShapeKind::Shape if sh.text_box || sh.ph.is_some() => Icon::LayerText,
        _ => Icon::LayerShape,
    }
}

/// The rows as listed: front-most first, children under their group unless it's folded.
pub fn rows(shapes: &[Shape], collapsed: &HashSet<u32>) -> Vec<Row> {
    fn rec(v: &[Shape], depth: usize, parent: Option<u32>, dim: bool, collapsed: &HashSet<u32>, out: &mut Vec<Row>) {
        if depth > 64 {
            return;
        }
        for sh in v.iter().rev() {
            out.push(Row {
                id: sh.id.0,
                name: sh.name.clone(),
                depth,
                group: sh.is_group(),
                hidden: sh.hidden,
                dimmed: dim,
                locked: sh.locked,
                parent,
                tag: sh.ph_type().map(|k| crate::props::kind_name(k.xml(), false)),
                icon: kind_icon(sh),
            });
            if sh.is_group() && !collapsed.contains(&sh.id.0) {
                rec(sh.children(), depth + 1, Some(sh.id.0), dim || sh.hidden, collapsed, out);
            }
        }
    }
    let mut out = vec![];
    rec(shapes, 0, None, false, collapsed, &mut out);
    out
}

/// Where a dragged row would land: among the rows sharing its parent (`rects`: each sibling's
/// block, the group's children included, top to bottom), the gap nearest `y`. Returns the index
/// among the siblings in the file's order (0 = back) and the y of the insertion line.
pub fn drop_target(dragged: u32, siblings: &[(u32, Rect)], y: f32) -> Option<(usize, f32)> {
    let others: Vec<&(u32, Rect)> = siblings.iter().filter(|(id, _)| *id != dragged).collect();
    let first = others.first()?;
    let mut gaps: Vec<f32> = vec![first.1.top()];
    gaps.extend(others.iter().map(|(_, r)| r.bottom()));
    let (g, gy) = gaps.iter().enumerate().min_by(|a, b| (a.1 - y).abs().total_cmp(&(b.1 - y).abs()))?;
    // `g` siblings sit above the line; the dragged object goes behind them.
    Some((others.len() - g, *gy))
}

fn run(app: &mut SlideApp, id: &str, p: serde_json::Value) {
    let _ = app.run(id, p);
}

/// Each row is a widget with an id of its own (keyboard focus sits on the selected row).
fn row_id(id: u32) -> Id {
    Id::unique(("selection_pane_row", id))
}

/// Is `focused` one of the Layers list's rows (for a layer on the current slide)?
pub fn is_row(app: &SlideApp, focused: Id) -> bool {
    let Some(st) = app.session.active() else { return false };
    let mut hit = false;
    deckcraft_model::walk(st.shapes(), &mut |sh, _| hit |= row_id(sh.id.0) == focused);
    hit
}

/// Draw a small labelled icon button and return its interaction response.
fn small_icon_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(4), t.hover);
    }
    ui.painter().rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    icons::paint(ui.painter(), rect.shrink(4.0), icon, t.text, !enabled);
    resp.on_hover_text(label)
}

fn toolbar(app: &mut SlideApp, ui: &mut Ui, t: &Tokens, has_sel: bool) {
    ui.horizontal(|ui| {
        if ui.add(egui::Button::new("Show All").stroke(Stroke::new(1.0, t.border))).clicked() {
            run(app, "selection.showAll", json!({}));
        }
        if ui.add(egui::Button::new("Hide All").stroke(Stroke::new(1.0, t.border))).clicked() {
            run(app, "selection.hideAll", json!({}));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if small_icon_button(ui, Icon::ChevronDown, "Send Backward", has_sel).clicked() {
                run(app, "arrange.sendBackward", json!({}));
            }
            if small_icon_button(ui, Icon::ChevronUp, "Bring Forward", has_sel).clicked() {
                run(app, "arrange.bringForward", json!({}));
            }
        });
    });
}

fn focus_lock(ui: &mut Ui, rows: &[Row]) -> Option<u32> {
    let focused_row = ui.memory(|m| m.focused()).and_then(|f| rows.iter().find(|r| row_id(r.id) == f).map(|r| r.id));
    if let Some(f) = focused_row {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(row_id(f), egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: true })
        });
    }
    focused_row
}

/// A row's background (selected or hovered) and its layer-kind icon before the name.
fn paint_row(painter: &egui::Painter, row: &Row, r: Rect, x0: f32, sel: bool, hovered: bool, t: &Tokens) {
    if sel {
        painter.rect_filled(r, CornerRadius::same(5), t.selected_row);
    } else if hovered {
        painter.rect_filled(r, CornerRadius::same(5), t.hover);
    }
    let kind_r = Rect::from_center_size(pos2(x0 + 24.0, r.center().y), vec2(14.0, 14.0));
    let kind_ink = if row.dimmed || row.hidden {
        t.text_faint
    } else if sel {
        t.text
    } else {
        t.text_dim
    };
    icons::paint_mono(painter, kind_r, row.icon, kind_ink, false);
}

/// A row's name, a placeholder's kind as a small pill after it, and the accent dot of a layer
/// that changes from the previous state.
fn paint_name(painter: &egui::Painter, row: &Row, r: Rect, name_x: f32, name_rect: Rect, marked: bool, t: &Tokens) {
    let color = if row.dimmed || row.hidden { t.text_faint } else { t.text };
    let name = painter.text(pos2(name_x, r.center().y), Align2::LEFT_CENTER, &row.name, theme::font(12.5), color);
    if let Some(tag) = &row.tag {
        let galley = painter.layout_no_wrap(tag.clone(), theme::font(10.5), t.text_dim);
        let pill = Rect::from_min_size(pos2(name.max.x + 6.0, r.center().y - 8.0), vec2(galley.size().x + 10.0, 16.0));
        if pill.max.x < name_rect.max.x {
            painter.rect_filled(pill, CornerRadius::same(8), t.hover);
            painter.galley(pos2(pill.min.x + 5.0, pill.center().y - galley.size().y / 2.0), galley, t.text_dim);
        }
    }
    if marked {
        painter.circle_filled(pos2(name_x - 4.0, r.center().y), 2.5, t.accent);
    }
}

fn group_chevron(app: &mut SlideApp, ui: &mut Ui, row: &Row, painter: &egui::Painter, x0: f32, r: Rect, t: &Tokens) {
    let open = !app.selpane.collapsed.contains(&row.id);
    let cr = Rect::from_center_size(pos2(x0 + 6.0, r.center().y), vec2(14.0, 14.0));
    let cresp = ui.interact(cr, ui.scope_id().with(("fold", row.id)), Sense::click());
    let label = format!("{} {}", if open { "Collapse" } else { "Expand" }, row.name);
    cresp.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
    icons::paint(painter, cr, if open { Icon::ChevronDown } else { Icon::ChevronRight }, t.text_dim, false);
    if cresp.clicked() {
        // ⌥-click: the groups inside it too, as in canvas editors.
        let mut ids = vec![row.id];
        if ui.input(|i| i.modifiers.alt)
            && let Some(st) = app.session.active()
            && let Some(g) = deckcraft_model::find_shape(st.shapes(), deckcraft_model::ShapeId(row.id))
        {
            nested_groups(g.children(), &mut ids, 0);
        }
        for id in ids {
            if open {
                app.selpane.collapsed.insert(id);
            } else {
                app.selpane.collapsed.remove(&id);
            }
        }
    }
}

/// Render the Selection Pane and route visibility, order, rename, and selection actions through
/// engine commands.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let has_sel = !st.selection.shapes.is_empty();

    toolbar(app, ui, &t, has_sel);
    ui.add_space(6.0);
    list(app, ui);
}

/// The rows alone, without the Show All / Hide All header.
pub fn list(app: &mut SlideApp, ui: &mut Ui) {
    list_marked(app, ui, &HashSet::new());
}

/// The rows, with an accent dot before the objects in `marked` (the canvas shell's Layers list
/// marks the layers that change from the previous state).
pub fn list_marked(app: &mut SlideApp, ui: &mut Ui, marked: &HashSet<u32>) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    app.selpane.bind(st.uid);
    let rows = rows(st.shapes(), &app.selpane.collapsed);
    let selected: Vec<u32> = st.selection.shapes.iter().map(|i| i.0).collect();

    // The list has the keyboard while one of its rows has focus.
    let focused_row = focus_lock(ui, &rows);
    let mut rects: Vec<Rect> = Vec::with_capacity(rows.len());
    let width = ui.available_width();
    for (k, row) in rows.iter().enumerate() {
        let (r, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
        let resp = ui.interact(r, row_id(row.id), Sense::click_and_drag());
        rects.push(r);
        let sel = selected.contains(&row.id);
        resp.widget_info(|| WidgetInfo::selected(Role::Button, true, sel, &row.name));
        let painter = ui.painter_at(r);
        // The whole row, its eye and lock included, counts as hovered.
        let hovered = ui.rect_contains_pointer(r);
        let x0 = r.min.x + 4.0 + INDENT * row.depth as f32;
        paint_row(&painter, row, r, x0, sel, hovered, &t);
        // Group chevron.
        if row.group {
            group_chevron(app, ui, row, &painter, x0, r, &t);
        }
        let name_x = x0 + 38.0;
        // As in canvas editors: the eye at the right end, the lock before it.
        let eye = Rect::from_center_size(pos2(r.max.x - 14.0, r.center().y), vec2(16.0, 16.0));
        let lock = Rect::from_center_size(pos2(r.max.x - 38.0, r.center().y), vec2(16.0, 16.0));
        let name_rect = Rect::from_min_max(pos2(name_x, r.min.y + 2.0), pos2(lock.min.x - 6.0, r.max.y - 2.0));
        let renaming = app.selpane.renaming.as_ref().filter(|(id, _)| *id == row.id).map(|(_, s)| s.clone());
        if let Some(mut buf) = renaming {
            let edit_id = ui.scope_id().with(("rename", row.id));
            let out = ui
                .put(name_rect, egui::TextEdit::singleline(&mut buf).id(edit_id).hint_text("Name").font(theme::font(12.5)).margin(vec2(2.0, 1.0)))
                .accessible_name("Name");
            if !app.selpane.rename_started {
                out.request_focus();
                if let Some(mut s) = egui::TextEdit::load_state(ui.ctx(), edit_id) {
                    let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(buf.chars().count()));
                    s.cursor.set_char_range(Some(all));
                    s.store(ui.ctx(), edit_id);
                }
                app.selpane.rename_started = true;
            }
            let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
            if out.lost_focus() || enter || esc {
                app.selpane.renaming = None;
                app.selpane.rename_started = false;
                if !esc && buf.trim() != row.name && !buf.trim().is_empty() {
                    run(app, "shape.rename", json!({"id": row.id, "name": buf.trim()}));
                }
                ui.memory_mut(|m| m.request_focus(row_id(row.id)));
            } else if let Some((_, s)) = app.selpane.renaming.as_mut() {
                *s = buf;
            }
        } else {
            paint_name(&painter, row, r, name_x, name_rect, marked.contains(&row.id), &t);
        }
        // Eye and padlock: shown while the row is hovered, and always when hidden or locked.
        let ink = if row.dimmed { t.text_faint } else { t.text };
        if hovered || row.hidden {
            icons::paint_mono(&painter, eye, if row.hidden { Icon::EyeOff } else { Icon::Eye }, ink, false);
        }
        if hovered || row.locked {
            icons::paint_mono(&painter, lock, if row.locked { Icon::Lock } else { Icon::Unlock }, ink, false);
        }
        let eye_resp = ui.interact(eye.expand(3.0), ui.scope_id().with(("eye", row.id)), Sense::click());
        let eye_label = format!("{} {}", if row.hidden { "Show" } else { "Hide" }, row.name);
        eye_resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, &eye_label));
        let lock_resp = ui.interact(lock.expand(3.0), ui.scope_id().with(("lock", row.id)), Sense::click());
        let lock_label = format!("{} {}", if row.locked { "Unlock" } else { "Lock" }, row.name);
        lock_resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, &lock_label));
        if eye_resp.clicked() {
            run(app, "shape.visible", json!({"id": row.id, "visible": row.hidden}));
        } else if lock_resp.clicked() {
            run(app, "shape.lock", json!({"id": row.id}));
        } else if resp.double_clicked() {
            app.selpane.renaming = Some((row.id, row.name.clone()));
            app.selpane.rename_started = false;
        } else if resp.clicked() {
            let m = ui.input(|i| i.modifiers);
            click(app, &rows, k, m.command || m.ctrl, m.shift);
            ui.memory_mut(|mem| mem.request_focus(row_id(row.id)));
        }
        if resp.drag_started() {
            if !sel {
                run(app, "selection.select", json!({"id": row.id}));
            }
            app.selpane.drag = Some((row.id, None));
        }
        context_menu(app, &resp, row);
    }

    // Dragging: the insertion line among the dragged row's siblings.
    if let Some((dragged, _)) = app.selpane.drag {
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let released = ui.input(|i| i.pointer.any_released() || !i.pointer.any_down());
        let target = pointer.and_then(|p| {
            let parent = rows.iter().find(|r| r.id == dragged).map(|r| r.parent)?;
            let blocks = sibling_blocks(&rows, &rects, parent);
            drop_target(dragged, &blocks, p.y)
        });
        app.selpane.drag = Some((dragged, target));
        if let Some((_, y)) = target {
            let (x0, x1) = (ui.min_rect().left(), ui.min_rect().left() + width);
            crate::widgets::insertion_line(ui.painter(), x0, x1, y, t.accent);
        }
        if released {
            app.selpane.drag = None;
            if let Some((index, _)) = target {
                run(app, "arrange.reorder", json!({"id": dragged, "index": index}));
            }
        }
    }

    // Keys while the list has focus.
    if let Some(f) = focused_row
        && app.selpane.renaming.is_none()
    {
        keys(app, ui, &rows, &selected, f);
    }
    // Clicking the empty part of the list doesn't change anything; keep the pane's height.
    let rest = ui.available_size();
    if rest.y > 0.0 {
        ui.allocate_exact_size(vec2(width, rest.y.min(400.0)), Sense::hover());
    }
}

/// The ids of the groups among `shapes`, at every depth.
/// Every group among `shapes`, at any depth.
pub fn all_groups(shapes: &[deckcraft_model::Shape]) -> Vec<u32> {
    let mut out = vec![];
    nested_groups(shapes, &mut out, 0);
    out
}

/// Is any group in the Layers list open? (The Collapse layers button shows only then.)
pub fn any_open(app: &SlideApp) -> bool {
    app.session.active().is_some_and(|d| all_groups(d.shapes()).iter().any(|g| !app.selpane.collapsed.contains(g)))
}

fn nested_groups(shapes: &[deckcraft_model::Shape], out: &mut Vec<u32>, depth: usize) {
    if depth > 64 {
        return;
    }
    for s in shapes.iter().filter(|s| s.is_group()) {
        out.push(s.id.0);
        nested_groups(s.children(), out, depth + 1);
    }
}

/// Each sibling of `parent`'s block (its row plus its shown children), top to bottom.
fn sibling_blocks(rows: &[Row], rects: &[Rect], parent: Option<u32>) -> Vec<(u32, Rect)> {
    let mut out: Vec<(u32, Rect)> = vec![];
    let depth_of = |id: Option<u32>| id.and_then(|p| rows.iter().find(|r| r.id == p)).map(|r| r.depth + 1).unwrap_or(0);
    let depth = depth_of(parent);
    let mut current: Option<(u32, Rect)> = None;
    for (row, rect) in rows.iter().zip(rects) {
        if row.parent == parent {
            if let Some(c) = current.take() {
                out.push(c);
            }
            current = Some((row.id, *rect));
        } else if row.depth > depth
            && let Some((_, r)) = current.as_mut()
        {
            *r = r.union(*rect);
        } else if let Some(c) = current.take() {
            out.push(c);
        }
    }
    if let Some(c) = current {
        out.push(c);
    }
    out
}

fn click(app: &mut SlideApp, rows: &[Row], k: usize, ctrl: bool, shift: bool) {
    let Some(row) = rows.get(k) else { return };
    if shift && let Some(a) = app.selpane.anchor.and_then(|a| rows.iter().position(|r| r.id == a)) {
        // A run of rows of the same kind (top level, or inside groups) between the anchor and here.
        let (lo, hi) = if a <= k { (a, k) } else { (k, a) };
        let top = row.parent.is_none();
        let ids: Vec<u32> = rows.get(lo..=hi).unwrap_or(&[]).iter().filter(|r| r.parent.is_none() == top).map(|r| r.id).collect();
        run(app, "selection.select", json!({"ids": ids, "add": ctrl}));
        return;
    }
    run(app, "selection.select", json!({"id": row.id, "toggle": ctrl}));
    app.selpane.anchor = Some(row.id);
}

fn keys(app: &mut SlideApp, ui: &Ui, rows: &[Row], selected: &[u32], focused: u32) {
    let (up, down, home, end, f2, esc) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Home),
            i.key_pressed(egui::Key::End),
            i.key_pressed(egui::Key::F2),
            i.key_pressed(egui::Key::Escape),
        )
    });
    let cur = selected.last().and_then(|id| rows.iter().position(|r| r.id == *id));
    let to = if up {
        Some(cur.map(|c| c.saturating_sub(1)).unwrap_or(0))
    } else if down {
        Some(cur.map(|c| (c + 1).min(rows.len().saturating_sub(1))).unwrap_or(0))
    } else if home {
        Some(0)
    } else if end {
        Some(rows.len().saturating_sub(1))
    } else {
        None
    };
    if let Some(k) = to
        && let Some(r) = rows.get(k)
        && Some(k) != cur
    {
        run(app, "selection.select", json!({"id": r.id}));
        app.selpane.anchor = Some(r.id);
        ui.memory_mut(|m| m.request_focus(row_id(r.id)));
    }
    if f2
        && let [one] = selected
        && let Some(r) = rows.iter().find(|r| r.id == *one)
    {
        app.selpane.renaming = Some((r.id, r.name.clone()));
        app.selpane.rename_started = false;
    }
    if esc {
        ui.memory_mut(|m| m.surrender_focus(row_id(focused)));
    }
}

fn context_menu(app: &mut SlideApp, resp: &egui::Response, row: &Row) {
    resp.context_menu(|ui| {
        for (label, cmd) in [
            ("Bring to Front", "arrange.bringToFront"),
            ("Bring Forward", "arrange.bringForward"),
            ("Send Backward", "arrange.sendBackward"),
            ("Send to Back", "arrange.sendToBack"),
        ] {
            if ui.button(label).clicked() {
                run(app, cmd, json!({"ids": [row.id]}));
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Rename").clicked() {
            app.selpane.renaming = Some((row.id, row.name.clone()));
            app.selpane.rename_started = false;
            ui.close();
        }
        if ui.button(if row.hidden { "Show" } else { "Hide" }).clicked() {
            run(app, "shape.visible", json!({"id": row.id, "visible": row.hidden}));
            ui.close();
        }
        if ui.button(if row.locked { "Unlock" } else { "Lock" }).clicked() {
            run(app, "shape.lock", json!({"id": row.id}));
            ui.close();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_document_drops_a_pending_rename() {
        let mut s = State::default();
        s.bind(1);
        s.renaming = Some((5, "draft".to_string()));
        s.collapsed.insert(2);
        s.bind(1);
        assert!(s.renaming.is_some(), "same document keeps the draft");
        s.bind(2);
        assert!(s.renaming.is_none());
        assert!(s.collapsed.is_empty());
    }

    fn r(id: u32, top: f32) -> (u32, Rect) {
        (id, Rect::from_min_size(pos2(0.0, top), vec2(100.0, ROW_H)))
    }

    #[test]
    fn drop_target_picks_the_nearest_gap() {
        // Rows top to bottom: 4 (front), 3, 2, 1 (back); dragging 1.
        let sib = [r(4, 0.0), r(3, 24.0), r(2, 48.0), r(1, 72.0)];
        // Above everything: in front of all three others.
        assert_eq!(drop_target(1, &sib, -5.0), Some((3, 0.0)));
        // Between 4 and 3: one sibling above → index 2.
        assert_eq!(drop_target(1, &sib, 22.0), Some((2, 24.0)));
        // Below the last other row: the back.
        assert_eq!(drop_target(1, &sib, 500.0), Some((0, 72.0)));
        // Nothing else to drop among.
        assert_eq!(drop_target(1, &[r(1, 0.0)], 10.0), None);
    }
}

#[cfg(test)]
mod ui_tests;
