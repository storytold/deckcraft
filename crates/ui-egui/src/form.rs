//! Dialog building blocks (after PhotoCraft's dialogs): the button row in the platform's order
//! with a primary default button, a label / field grid with right-aligned grey labels, section
//! headings, and grey fields like the canvas panel's (numbers take units, arithmetic and ↑ / ↓).

use egui::{Align2, Color32, CornerRadius, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use crate::theme::{self, Tokens};

/// A field row's height.
pub const ROW: f32 = 26.0;
/// The label column's width.
pub const LABEL_W: f32 = 118.0;

/// What a dialog button does, which decides where the platform's order puts it (from PhotoCraft).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonRole {
    /// The default answer (OK, Save, Insert): the primary button, and Enter.
    Default,
    /// Another answer that closes the dialog (Don't Save, Apply to one).
    Alternate,
    /// Closes the dialog without acting.
    Cancel,
    /// Acts and keeps the dialog open (Apply).
    Apply,
}

impl ButtonRole {
    /// Position from the left. macOS puts the default last, in the corner, Cancel beside it and
    /// other answers further left (Don't Save … Cancel Save); Windows and Linux put it first.
    fn slot(self, mac: bool) -> u8 {
        match (self, mac) {
            (Self::Default, false) | (Self::Alternate, true) => 0,
            (Self::Alternate, false) | (Self::Cancel, true) => 1,
            (Self::Cancel, false) | (Self::Apply, true) => 2,
            (Self::Apply, false) | (Self::Default, true) => 3,
        }
    }
}

/// One button of a [`buttons`] row.
#[derive(Clone, Copy)]
pub struct Button<'a> {
    pub role: ButtonRole,
    pub label: &'a str,
    pub enabled: bool,
}

impl<'a> Button<'a> {
    pub fn new(role: ButtonRole, label: &'a str) -> Self {
        Button { role, label, enabled: true }
    }
    pub fn enabled(self, enabled: bool) -> Self {
        Button { enabled, ..self }
    }
}

const BUTTON_MIN_W: f32 = 84.0;
const BUTTON_H: f32 = 28.0;

/// A dialog's buttons, right-aligned, in the platform's order. Returns the role clicked. With
/// `enter`, Enter presses the default button (dialogs typing several lines pass false).
pub fn buttons(ui: &mut Ui, list: &[Button], enter: bool) -> Option<ButtonRole> {
    let mac = ui.ctx().os() == egui::os::OperatingSystem::Mac || cfg!(target_os = "macos");
    ui.add_space(14.0);
    let mut hit = None;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        // Right to left: the highest slot first.
        for slot in (0..4).rev() {
            for b in list.iter().filter(|b| b.role.slot(mac) == slot) {
                let r = ui.add_enabled_ui(b.enabled, |ui| button(ui, b.label, b.role == ButtonRole::Default)).inner;
                if r.clicked() {
                    hit = Some(b.role);
                }
            }
        }
    });
    if hit.is_none() && enter && list.iter().any(|b| b.role == ButtonRole::Default && b.enabled) && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        hit = Some(ButtonRole::Default);
    }
    hit
}

/// A dialog button: the primary one filled with the accent, the others grey with a hairline.
pub fn button(ui: &mut Ui, label: &str, primary: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let fg = if primary { t.accent_text } else { t.text };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::medium(13.0), fg);
    let size = vec2((galley.size().x + 28.0).max(BUTTON_MIN_W), BUTTON_H);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    let down = resp.is_pointer_button_down_on();
    let bg = if primary { t.accent } else { t.field };
    let fill = if down {
        bg.gamma_multiply(0.85)
    } else if resp.hovered() {
        if primary { bg.gamma_multiply(0.92) } else { t.hover }
    } else {
        bg
    };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(6), if ui.is_enabled() { fill } else { fill.gamma_multiply(0.5) });
    if !primary {
        p.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, t.card_border), StrokeKind::Inside);
    }
    if resp.has_focus() {
        p.rect_stroke(rect.expand(2.0), CornerRadius::same(8), Stroke::new(2.0, t.focus), StrokeKind::Outside);
    }
    p.galley(rect.center() - galley.size() / 2.0, galley, if ui.is_enabled() { fg } else { t.text_faint });
    resp
}

/// A label / field grid: put [`label`] then a field on each row and `ui.end_row()`.
pub fn grid<R>(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Grid::new(("form", id)).num_columns(2).spacing([12.0, 8.0]).min_col_width(0.0).show(ui, add).inner
}

/// A checkbox: a rounded box with a hairline, filled with the accent and ticked when on, and
/// its label. Returns the response (clicked toggles `on`).
pub fn checkbox(ui: &mut Ui, on: &mut bool, text: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let enabled = ui.is_enabled();
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme::font(12.5), if enabled { t.text } else { t.text_faint });
    let size = vec2(16.0 + 8.0 + galley.size().x, ROW.min(22.0));
    let (r, mut resp) = ui.allocate_exact_size(size, Sense::click());
    let b = Rect::from_min_size(pos2(r.min.x, r.center().y - 8.0), vec2(16.0, 16.0));
    let p = ui.painter();
    if *on {
        p.rect_filled(b, CornerRadius::same(4), if enabled { t.accent } else { t.text_faint });
        let s = Stroke::new(1.8, t.accent_text);
        p.line_segment([pos2(b.min.x + 4.0, b.center().y), pos2(b.min.x + 7.0, b.max.y - 4.5)], s);
        p.line_segment([pos2(b.min.x + 7.0, b.max.y - 4.5), pos2(b.max.x - 3.5, b.min.y + 4.5)], s);
    } else {
        p.rect(b, CornerRadius::same(4), t.card, Stroke::new(1.0, if resp.hovered() { t.text_faint } else { t.border }), StrokeKind::Inside);
    }
    if resp.has_focus() {
        p.rect_stroke(b.expand(2.0), CornerRadius::same(6), Stroke::new(2.0, t.focus), StrokeKind::Outside);
    }
    p.galley(pos2(b.max.x + 8.0, r.center().y - galley.size().y / 2.0), galley, if enabled { t.text } else { t.text_faint });
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let value = *on;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, value, text));
    resp
}

/// A row of a choice list: left-aligned, the chosen one on a grey highlight in bold.
pub fn list_row(ui: &mut Ui, text: &str, selected: bool, width: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    if selected {
        ui.painter().rect_filled(r, CornerRadius::same(5), t.card);
        ui.painter().rect_stroke(r, CornerRadius::same(5), Stroke::new(1.0, t.card_border), StrokeKind::Inside);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
    }
    ui.painter().text(
        pos2(r.min.x + 10.0, r.center().y),
        Align2::LEFT_CENTER,
        text,
        if selected { theme::bold(12.5) } else { theme::font(12.5) },
        t.text,
    );
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, text));
    resp
}

/// A grid row's label: grey, right-aligned in the label column.
pub fn label(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.allocate_ui_with_layout(vec2(LABEL_W, ROW), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.add(egui::Label::new(egui::RichText::new(text).font(theme::font(12.5)).color(t.text_dim)).wrap_mode(egui::TextWrapMode::Truncate));
    });
}

/// An empty label cell (a row under a heading, or a checkbox row).
pub fn no_label(ui: &mut Ui) {
    ui.allocate_exact_size(vec2(LABEL_W, ROW), Sense::hover());
}

/// A heading over a group of rows.
pub fn section(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).font(theme::bold(12.5)).color(t.text));
    ui.add_space(2.0);
}

/// A note under a dialog's fields: grey, wrapping.
pub fn note(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add(egui::Label::new(egui::RichText::new(text).font(theme::font(12.0)).color(t.text_dim)).wrap());
}

fn field_frame(ui: &Ui, r: Rect, focused: bool) {
    let t = Tokens::get(ui.ctx());
    if focused {
        ui.painter().rect_stroke(r, CornerRadius::same(6), Stroke::new(1.5, t.focus), StrokeKind::Inside);
    } else if ui.rect_contains_pointer(r) {
        ui.painter().rect_stroke(r, CornerRadius::same(6), Stroke::new(1.0, t.field_hover_border), StrokeKind::Inside);
    }
}

/// A grey one-line text field `width` wide. Returns true when it changed.
pub fn text(ui: &mut Ui, value: &mut String, width: f32, hint: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(6), t.field);
    let inner = r.shrink2(vec2(8.0, 5.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let te = child.add_sized(
        inner.size(),
        egui::TextEdit::singleline(value).frame(egui::Frame::NONE).font(theme::font(12.5)).text_color(t.text).hint_text(hint),
    );
    let te = te.accessible_name(if hint.is_empty() { "Text" } else { hint });
    field_frame(ui, r, te.has_focus());
    te
}

/// A grey text box several lines tall.
pub fn text_area(ui: &mut Ui, value: &mut String, width: f32, rows: usize, hint: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let out = egui::Frame::NONE.fill(t.field).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.add(
            egui::TextEdit::multiline(value)
                .frame(egui::Frame::NONE)
                .font(theme::font(12.5))
                .text_color(t.text)
                .desired_rows(rows)
                .desired_width(width - 16.0)
                .hint_text(hint),
        )
        .accessible_name(if hint.is_empty() { "Text" } else { hint })
    });
    field_frame(ui, out.response.rect, out.inner.has_focus());
    out.inner
}

/// A grey number field: the value, a grey unit after it, ↑ / ↓ steps (⇧: ten), and typed
/// arithmetic or relative edits (`+1`, `*2`). Returns a new value when one is set.
#[allow(clippy::too_many_arguments)]
pub fn number(ui: &mut Ui, id: &str, value: f64, unit: &str, step: f64, (lo, hi): (f64, f64), width: f32) -> Option<f64> {
    let t = Tokens::get(ui.ctx());
    let clamp = |v: f64| v.clamp(lo, hi.max(lo));
    let (r, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(6), t.field);
    let unit_w = if unit.is_empty() { 0.0 } else { ui.painter().layout_no_wrap(unit.to_string(), theme::font(12.0), t.text_dim).size().x + 8.0 };
    if !unit.is_empty() {
        ui.painter().text(pos2(r.max.x - 8.0, r.center().y), Align2::RIGHT_CENTER, unit, theme::font(12.0), t.text_dim);
    }
    let key = ui.scope_id().with((id, "typing"));
    let edit = ui.scope_id().with((id, "edit"));
    let mut text: String = ui.data(|d| d.get_temp(key)).unwrap_or_else(|| crate::widgets::fmt_num(value, ""));
    let mut out = None;
    if let Some(d) = crate::widgets::arrow_step(ui, edit, step) {
        let v = clamp(crate::widgets::parse_num(&text).unwrap_or(value) + d);
        text = crate::widgets::fmt_num(v, "");
        out = Some(v);
    }
    let inner = Rect::from_min_max(pos2(r.min.x + 8.0, r.min.y + 5.0), pos2(r.max.x - 6.0 - unit_w, r.max.y - 5.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let te = child
        .add_sized(inner.size(), egui::TextEdit::singleline(&mut text).frame(egui::Frame::NONE).font(theme::font(12.5)).text_color(t.text).id(edit))
        .accessible_name(id);
    te.widget_info(|| egui::WidgetInfo::text_edit(true, "", &text, id));
    field_frame(ui, r, te.has_focus());
    if te.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, text.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(key));
    }
    let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if te.lost_focus()
        && out.is_none()
        && !escaped
        && let Some(v) = crate::widgets::typed_value(&text, Some(value), unit.trim() == "%").map(clamp)
        && (v - value).abs() > 1e-9
    {
        out = Some(v);
    }
    out
}

/// A grey dropdown showing `current`; `add` fills its menu.
pub fn dropdown(ui: &mut Ui, id: &str, current: &str, width: f32, add: impl FnOnce(&mut Ui)) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
    ui.painter().rect_filled(r, CornerRadius::same(6), t.field);
    field_frame(ui, r, false);
    let text_r = Rect::from_min_max(pos2(r.min.x + 8.0, r.min.y), pos2(r.max.x - 22.0, r.max.y));
    ui.painter().with_clip_rect(text_r).text(pos2(text_r.min.x, r.center().y), Align2::LEFT_CENTER, current, theme::font(12.5), t.text);
    crate::widgets::chevron(ui.painter(), pos2(r.max.x - 12.0, r.center().y), 3.0, t.text_dim);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, format!("{id}: {current}")));
    egui::Popup::menu(&resp).width(width.max(160.0)).show(|ui| {
        egui::ScrollArea::vertical().max_height(320.0).show(ui, add);
    });
    resp
}

/// Pill tabs across a dialog (Web Page · This Document · Email). Returns the one clicked.
pub fn tabs(ui: &mut Ui, items: &[&str], current: usize) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    let mut out = None;
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(7), t.field);
    let n = items.len().max(1) as f32;
    for (k, l) in items.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(r.min.x + k as f32 * w / n, r.min.y), vec2(w / n, r.height()));
        let resp = ui.interact(cell, ui.scope_id().with(("form-tab", k)), Sense::click());
        let on = k == current;
        if on {
            ui.painter().rect(cell.shrink(2.0), CornerRadius::same(6), t.card, Stroke::new(1.0, t.card_border), StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(cell.shrink(2.0), CornerRadius::same(6), t.hover);
        }
        ui.painter().text(
            cell.center(),
            Align2::CENTER_CENTER,
            *l,
            if on { theme::bold(12.5) } else { theme::font(12.5) },
            if on { t.text } else { t.text_dim },
        );
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, *l));
        if resp.clicked() {
            out = Some(k);
        }
    }
    ui.add_space(6.0);
    out
}

/// The space a dialog's body has for fields beside the label column.
pub fn field_width(ui: &Ui) -> f32 {
    (ui.available_width() - LABEL_W - 12.0).clamp(120.0, 300.0)
}

/// A hairline across the dialog.
pub fn hairline(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, t.separator));
}

/// The dialog's frame colour and margin.
pub fn frame(ctx: &egui::Context) -> egui::Frame {
    let t = Tokens::get(ctx);
    egui::Frame::popup(&ctx.global_style())
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.card_border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(18))
        .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(if t.dark { 120 } else { 45 }) })
}

/// A size as a `Vec2` (for callers that set a dialog's width).
pub fn width(w: f32) -> Vec2 {
    vec2(w, 0.0)
}

#[cfg(test)]
mod tests {
    use super::ButtonRole;

    #[test]
    fn buttons_follow_the_platforms_order() {
        let order = |mac: bool| {
            let mut v = [ButtonRole::Default, ButtonRole::Cancel, ButtonRole::Alternate, ButtonRole::Apply];
            v.sort_by_key(|r| r.slot(mac));
            v
        };
        // macOS: Don't Save … Cancel, Apply, then the default in the corner.
        assert_eq!(order(true), [ButtonRole::Alternate, ButtonRole::Cancel, ButtonRole::Apply, ButtonRole::Default]);
        // Windows and Linux: the default first.
        assert_eq!(order(false), [ButtonRole::Default, ButtonRole::Alternate, ButtonRole::Cancel, ButtonRole::Apply]);
    }
}
