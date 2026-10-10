//! Ribbon widgets: large and small buttons, split buttons, groups, dropdowns, spinners, the colour
//! picker and galleries.

use deckcraft_color::{ColorScheme, Rgba, SchemeSlot};
use deckcraft_model::ColorRef;
use egui::{Align2, Color32, CornerRadius, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};

use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// Height of the ribbon's content area.
pub const RIBBON_H: f32 = 66.0;

fn bg(ui: &Ui, r: &Response, selected: bool, t: &Tokens) -> Option<Color32> {
    if !r.sense.senses_click() && !r.sense.senses_drag() {
        return None;
    }
    if selected {
        Some(t.selected)
    } else if r.is_pointer_button_down_on() {
        Some(t.pressed)
    } else if r.hovered() && ui.is_enabled() {
        Some(t.hover)
    } else {
        None
    }
}

/// A large ribbon button: 32 px icon over a one- or two-line label.
pub fn big_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    big_button_ex(ui, icon, label, enabled, false, false).0
}

/// Large button; with `split`, the label row is a separate dropdown target (returns its response).
pub fn big_button_ex(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, split: bool, selected: bool) -> (Response, Option<Response>) {
    let label = crate::i18n::tr(label);
    let t = Tokens::get(ui.ctx());
    let font = theme::font(11.0);
    let lines: Vec<&str> = wrap_label(label);
    let text_w = lines.iter().map(|l| ui.fonts_mut(|f| f.layout_no_wrap(l.to_string(), font.clone(), t.text).size().x)).fold(0.0f32, f32::max);
    let w = (text_w + 10.0 + if split && lines.len() == 1 { 10.0 } else { 0.0 }).max(40.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, RIBBON_H - 2.0), if enabled { Sense::click() } else { Sense::hover() });
    let painter = ui.painter_at(rect.expand(1.0));
    let (main_rect, arrow_rect) = if split {
        (Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + 38.0)), Some(Rect::from_min_max(pos2(rect.min.x, rect.min.y + 38.0), rect.max)))
    } else {
        (rect, None)
    };
    let main_resp =
        if split { ui.interact(main_rect, resp.id.with("main"), if enabled { Sense::click() } else { Sense::hover() }) } else { resp.clone() };
    let arrow_resp = arrow_rect.map(|r| ui.interact(r, resp.id.with("arrow"), if enabled { Sense::click() } else { Sense::hover() }));
    if let Some(c) = bg(ui, &main_resp, selected, &t) {
        painter.rect_filled(if split { main_rect } else { rect }, CornerRadius::same(4), c);
    }
    if let (Some(ar), Some(a)) = (arrow_rect, &arrow_resp)
        && let Some(c) = bg(ui, a, false, &t)
    {
        painter.rect_filled(ar, CornerRadius::same(4), c);
    }
    let icon_rect = Rect::from_center_size(pos2(rect.center().x, rect.min.y + 19.0), vec2(30.0, 30.0));
    icons::paint(&painter, icon_rect, icon, t.text, !enabled);
    let col = if enabled { t.text } else { t.text_faint };
    let mut y = rect.min.y + 39.0;
    for (i, l) in lines.iter().enumerate() {
        let last = i + 1 == lines.len();
        let label_w = ui.fonts_mut(|f| f.layout_no_wrap(l.to_string(), font.clone(), col).size().x);
        let extra = if split && last { 9.0 } else { 0.0 };
        let x = rect.center().x - (label_w + extra) / 2.0;
        painter.text(pos2(x, y), Align2::LEFT_TOP, crate::i18n::tr(l), font.clone(), col);
        if split && last {
            chevron(&painter, pos2(x + label_w + 5.0, y + 7.0), 3.0, col);
        }
        y += 12.5;
    }
    (if split { main_resp } else { resp }, arrow_resp)
}

fn wrap_label(label: &str) -> Vec<&str> {
    if let Some((a, b)) = label.split_once('\n') {
        return vec![a, b];
    }
    let words: Vec<&str> = label.split(' ').collect();
    if words.len() >= 2 && label.chars().count() > 8 {
        // Break near the middle.
        let mut best = 1;
        let mut best_diff = usize::MAX;
        for k in 1..words.len() {
            let a: usize = words.iter().take(k).map(|w| w.chars().count() + 1).sum();
            let b: usize = words.iter().skip(k).map(|w| w.chars().count() + 1).sum();
            let d = a.abs_diff(b);
            if d < best_diff {
                best_diff = d;
                best = k;
            }
        }
        let split_at: usize = words.iter().take(best).map(|w| w.len() + 1).sum::<usize>().saturating_sub(1);
        if let (Some(a), Some(b)) = (label.get(..split_at), label.get(split_at + 1..)) {
            return vec![a, b];
        }
    }
    vec![label]
}

pub fn chevron(p: &egui::Painter, c: egui::Pos2, s: f32, col: Color32) {
    p.line_segment([pos2(c.x - s, c.y - s * 0.5), pos2(c.x, c.y + s * 0.5)], Stroke::new(1.2, col));
    p.line_segment([pos2(c.x, c.y + s * 0.5), pos2(c.x + s, c.y - s * 0.5)], Stroke::new(1.2, col));
}

/// A small ribbon button: 16 px icon + label in one row (label may be empty).
pub fn small_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, selected: bool) -> Response {
    small_button_ex(ui, icon, label, enabled, selected, false).0
}

pub fn small_button_ex(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, selected: bool, split: bool) -> (Response, Option<Response>) {
    let label = crate::i18n::tr(label);
    let t = Tokens::get(ui.ctx());
    let font = theme::font(12.0);
    let tw = if label.is_empty() { 0.0 } else { ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), t.text).size().x) + 5.0 };
    let arrow_w = if split { 12.0 } else { 0.0 };
    let w = 24.0 + tw + arrow_w;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    let painter = ui.painter_at(rect);
    let main_rect = Rect::from_min_max(rect.min, pos2(rect.max.x - arrow_w, rect.max.y));
    let main_resp =
        if split { ui.interact(main_rect, resp.id.with("m"), if enabled { Sense::click() } else { Sense::hover() }) } else { resp.clone() };
    let arrow_resp = split.then(|| {
        ui.interact(
            Rect::from_min_max(pos2(rect.max.x - arrow_w, rect.min.y), rect.max),
            resp.id.with("a"),
            if enabled { Sense::click() } else { Sense::hover() },
        )
    });
    if let Some(c) = bg(ui, &main_resp, selected, &t) {
        painter.rect_filled(main_rect, CornerRadius::same(4), c);
    }
    if let Some(a) = &arrow_resp
        && let Some(c) = bg(ui, a, false, &t)
    {
        painter.rect_filled(Rect::from_min_max(pos2(rect.max.x - arrow_w, rect.min.y), rect.max), CornerRadius::same(4), c);
    }
    icons::paint(&painter, Rect::from_center_size(pos2(rect.min.x + 12.0, rect.center().y), vec2(17.0, 17.0)), icon, t.text, !enabled);
    if !label.is_empty() {
        painter.text(
            pos2(rect.min.x + 24.0, rect.center().y),
            Align2::LEFT_CENTER,
            crate::i18n::tr(label),
            font,
            if enabled { t.text } else { t.text_faint },
        );
    }
    if split {
        chevron(&painter, pos2(rect.max.x - 6.0, rect.center().y), 2.8, t.text_dim);
    }
    (if split { main_resp } else { resp }, arrow_resp)
}

/// A square icon toggle (B, I, U, alignment…).
pub fn icon_toggle(ui: &mut Ui, icon: Icon, tip: &str, enabled: bool, on: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    let painter = ui.painter_at(rect);
    if let Some(c) = bg(ui, &resp, on, &t) {
        painter.rect_filled(rect, CornerRadius::same(4), c);
    }
    icons::paint(&painter, rect.shrink(3.0), icon, t.text, !enabled);
    resp.on_hover_text(crate::i18n::tr(tip))
}

/// Icon with a colour bar under it and a dropdown arrow (Font Color, Highlight, Shape Fill…).
pub fn color_split(ui: &mut Ui, icon: Icon, tip: &str, color: Color32, enabled: bool) -> (Response, Response) {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 22.0), Sense::hover());
    let main = ui.interact(Rect::from_min_size(rect.min, vec2(23.0, 22.0)), resp.id.with("m"), if enabled { Sense::click() } else { Sense::hover() });
    let arrow = ui.interact(
        Rect::from_min_max(pos2(rect.min.x + 23.0, rect.min.y), rect.max),
        resp.id.with("a"),
        if enabled { Sense::click() } else { Sense::hover() },
    );
    let painter = ui.painter_at(rect);
    if let Some(c) = bg(ui, &main, false, &t) {
        painter.rect_filled(main.rect, CornerRadius::same(4), c);
    }
    if let Some(c) = bg(ui, &arrow, false, &t) {
        painter.rect_filled(arrow.rect, CornerRadius::same(4), c);
    }
    icons::paint(&painter, Rect::from_min_size(rect.min + vec2(3.0, 1.0), vec2(17.0, 15.0)), icon, t.text, !enabled);
    painter.rect_filled(
        Rect::from_min_size(rect.min + vec2(4.0, 17.0), vec2(15.0, 3.5)),
        CornerRadius::ZERO,
        if enabled { color } else { t.text_faint },
    );
    chevron(&painter, pos2(rect.max.x - 5.5, rect.center().y), 2.6, t.text_dim);
    (main.on_hover_text(crate::i18n::tr(tip)), arrow)
}

/// A ribbon group: contents, then a thin vertical separator.
pub fn group<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let r = ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
        add(ui)
    });
    separator(ui);
    r.inner
}

pub fn separator(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(13.0, RIBBON_H - 6.0), Sense::hover());
    ui.painter().line_segment([pos2(rect.center().x, rect.min.y + 6.0), pos2(rect.center().x, rect.max.y - 4.0)], Stroke::new(1.0, t.separator));
}

/// Two or three rows of small controls stacked in one ribbon column.
pub fn rows<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(1.0, 0.0);
        ui.add_space(0.0);
        add(ui)
    })
    .inner
}

/// A text dropdown field (font name, size): shows `current`, opens a popup with `add_items`.
pub fn dropdown(ui: &mut Ui, id: &str, current: &str, width: f32, enabled: bool, add_items: impl FnOnce(&mut Ui)) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        CornerRadius::same(4),
        t.field_bg,
        Stroke::new(1.0, if resp.hovered() { t.text_faint } else { t.border }),
        egui::StrokeKind::Inside,
    );
    let clip = rect.shrink2(vec2(6.0, 0.0));
    painter.with_clip_rect(Rect::from_min_max(clip.min, pos2(clip.max.x - 12.0, clip.max.y))).text(
        pos2(rect.min.x + 6.0, rect.center().y),
        Align2::LEFT_CENTER,
        current,
        theme::font(12.0),
        if enabled { t.text } else { t.text_faint },
    );
    chevron(&painter, pos2(rect.max.x - 9.0, rect.center().y), 3.0, t.text_dim);
    let _ = id;
    egui::Popup::menu(&resp).width(width.max(140.0)).show(|ui| {
        egui::ScrollArea::vertical().max_height(360.0).show(ui, add_items);
    });
    resp
}

/// The drop-down for a `color_grid`: a menu that stays open when its hex field is clicked
/// (egui menus close on any click inside). Picking a colour closes it (`color_grid` calls
/// `ui.close()`); other buttons in it must close it themselves.
pub fn color_popup(button: &Response) -> egui::Popup<'_> {
    egui::Popup::menu(button).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
}

/// The colour grid popup body: theme colours with tints/shades, standard colours, extra entries.
/// Returns the picked colour (`Some(None)` = "No Fill"/"Automatic" when `none_label` is given).
pub fn color_grid(ui: &mut Ui, scheme: &ColorScheme, none_label: Option<&str>) -> Option<Option<ColorRef>> {
    let t = Tokens::get(ui.ctx());
    let mut out = None;
    ui.set_min_width(220.0);
    ui.label(egui::RichText::new(crate::i18n::tr("Theme Colors")).font(theme::bold(12.0)));
    let slots = [
        SchemeSlot::Lt1,
        SchemeSlot::Dk1,
        SchemeSlot::Lt2,
        SchemeSlot::Dk2,
        SchemeSlot::Accent1,
        SchemeSlot::Accent2,
        SchemeSlot::Accent3,
        SchemeSlot::Accent4,
        SchemeSlot::Accent5,
        SchemeSlot::Accent6,
    ];
    let cell = 18.0;
    let sw = |ui: &mut Ui, c: Rgba, tip: &str| -> bool {
        let (r, resp) = ui.allocate_exact_size(vec2(cell, cell), Sense::click());
        ui.painter().rect(
            r.shrink(1.0),
            CornerRadius::same(2),
            theme::to_color32(c),
            Stroke::new(if resp.hovered() { 2.0 } else { 1.0 }, if resp.hovered() { t.accent } else { t.border }),
            egui::StrokeKind::Inside,
        );
        resp.on_hover_text(crate::i18n::tr(tip)).clicked()
    };
    egui::Grid::new("theme_colors").spacing(vec2(3.0, 1.0)).show(ui, |ui| {
        for slot in slots {
            let alias = match slot {
                SchemeSlot::Lt1 => SchemeSlot::Bg1,
                SchemeSlot::Dk1 => SchemeSlot::Tx1,
                SchemeSlot::Lt2 => SchemeSlot::Bg2,
                SchemeSlot::Dk2 => SchemeSlot::Tx2,
                s => s,
            };
            if sw(ui, scheme.get(slot), slot.label()) {
                out = Some(Some(ColorRef::scheme(alias)));
            }
        }
        ui.end_row();
        for row in 0..5 {
            for slot in slots {
                let base = scheme.get(slot);
                let vars = deckcraft_color::theme_variants(base);
                let Some((mods, label)) = vars.get(row) else { continue };
                let c = deckcraft_color::apply(base, mods);
                if sw(ui, c, label) {
                    let alias = match slot {
                        SchemeSlot::Lt1 => SchemeSlot::Bg1,
                        SchemeSlot::Dk1 => SchemeSlot::Tx1,
                        SchemeSlot::Lt2 => SchemeSlot::Bg2,
                        SchemeSlot::Dk2 => SchemeSlot::Tx2,
                        s => s,
                    };
                    let mut r = ColorRef::scheme(alias);
                    r.mods = mods.clone();
                    out = Some(Some(r));
                }
            }
            ui.end_row();
        }
    });
    ui.add_space(4.0);
    ui.label(egui::RichText::new(crate::i18n::tr("Standard Colors")).font(theme::bold(12.0)));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (c, name) in deckcraft_color::STANDARD_COLORS {
            if sw(ui, c, name) {
                out = Some(Some(ColorRef::rgb(c)));
            }
        }
    });
    ui.separator();
    if let Some(n) = none_label
        && ui.button(crate::i18n::tr(n)).clicked()
    {
        out = Some(None);
    }
    // "More Colors…": a hex field.
    let id = ui.id().with("hex");
    let mut hex: String = ui.data_mut(|d| d.get_temp::<String>(id).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("More Colors… #"));
        let r = ui.add(egui::TextEdit::singleline(&mut hex).desired_width(70.0).hint_text("RRGGBB"));
        if r.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some(c) = Rgba::from_hex(&hex)
        {
            out = Some(Some(ColorRef::rgb(c)));
        }
    });
    ui.data_mut(|d| d.insert_temp(id, hex));
    if out.is_some() {
        ui.close();
    }
    out
}

/// A row of gallery tiles drawn by `paint_tile`; returns the clicked index.
pub fn gallery(
    ui: &mut Ui,
    n: usize,
    tile: Vec2,
    selected: Option<usize>,
    mut paint_tile: impl FnMut(&egui::Painter, Rect, usize),
    mut tip: impl FnMut(usize) -> String,
) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    let mut clicked = None;
    for i in 0..n {
        let (rect, resp) = ui.allocate_exact_size(tile, Sense::click());
        let painter = ui.painter_at(rect.expand(2.0));
        let sel = selected == Some(i);
        if sel || resp.hovered() {
            painter.rect_filled(rect.expand(1.0), CornerRadius::same(4), if sel { t.selected } else { t.hover });
        }
        paint_tile(&painter, rect.shrink(3.0), i);
        if sel {
            painter.rect_stroke(rect.expand(1.0), CornerRadius::same(4), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
        }
        let caption = tip(i);
        if resp.on_hover_text(crate::i18n::tr(&caption)).clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// A labelled number field with up/down steppers. Returns the new value when committed.
pub fn spinner(ui: &mut Ui, id: &str, value: f64, step: f64, suffix: &str, width: f32) -> Option<f64> {
    let t = Tokens::get(ui.ctx());
    let key = ui.id().with(id);
    let mut text: String = ui.data_mut(|d| d.get_temp::<String>(key)).unwrap_or_else(|| fmt_num(value, suffix));
    let mut out = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(width).font(theme::font(12.0)));
        if r.has_focus() {
            ui.data_mut(|d| d.insert_temp(key, text.clone()));
        } else {
            ui.data_mut(|d| d.remove::<String>(key));
        }
        if r.lost_focus() {
            let num: String = text.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
            if let Ok(v) = num.parse::<f64>() {
                out = Some(v);
            }
        }
        let (rect, _) = ui.allocate_exact_size(vec2(12.0, 22.0), Sense::hover());
        let up = ui.interact(Rect::from_min_max(rect.min, pos2(rect.max.x, rect.center().y)), key.with("up"), Sense::click());
        let down = ui.interact(Rect::from_min_max(pos2(rect.min.x, rect.center().y), rect.max), key.with("dn"), Sense::click());
        let p = ui.painter();
        let c = rect.center();
        p.line_segment([pos2(c.x - 3.0, c.y - 3.0), pos2(c.x, c.y - 6.0)], Stroke::new(1.0, t.text_dim));
        p.line_segment([pos2(c.x, c.y - 6.0), pos2(c.x + 3.0, c.y - 3.0)], Stroke::new(1.0, t.text_dim));
        p.line_segment([pos2(c.x - 3.0, c.y + 3.0), pos2(c.x, c.y + 6.0)], Stroke::new(1.0, t.text_dim));
        p.line_segment([pos2(c.x, c.y + 6.0), pos2(c.x + 3.0, c.y + 3.0)], Stroke::new(1.0, t.text_dim));
        if up.clicked() {
            out = Some(value + step);
        }
        if down.clicked() {
            out = Some(value - step);
        }
    });
    out
}

pub fn fmt_num(v: f64, suffix: &str) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    format!("{s}{suffix}")
}

/// Section header inside panes.
pub fn section(ui: &mut Ui, title: &str, open: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    let p = ui.painter();
    let c = pos2(rect.min.x + 8.0, rect.center().y);
    if open {
        chevron(p, c, 3.5, t.text);
    } else {
        p.line_segment([pos2(c.x - 2.0, c.y - 4.0), pos2(c.x + 2.0, c.y)], Stroke::new(1.2, t.text));
        p.line_segment([pos2(c.x + 2.0, c.y), pos2(c.x - 2.0, c.y + 4.0)], Stroke::new(1.2, t.text));
    }
    p.text(pos2(rect.min.x + 18.0, rect.center().y), Align2::LEFT_CENTER, title, theme::bold(13.0), t.text);
    if resp.clicked() { !open } else { open }
}

/// A button with an optional label and a painted dropdown chevron (the UI font has no ▾).
pub fn drop_button(ui: &mut Ui, label: &str, size: egui::Vec2, enabled: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let r = ui.add_enabled(enabled, egui::Button::new(if label.is_empty() { String::new() } else { format!("{label}   ") }).min_size(size));
    let col = if enabled { t.text_dim } else { t.text_faint };
    chevron(ui.painter(), egui::pos2(r.rect.max.x - 8.0, r.rect.center().y), 3.0, col);
    r
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::cell::RefCell;

    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;

    /// #32: clicking the "More Colors" hex field closed the colour menu before anything could be
    /// typed. It stays open now; Enter picks the colour and closes it.
    #[test]
    fn the_hex_field_keeps_the_color_menu_open() {
        let picked = RefCell::new(None);
        let scheme = deckcraft_model::Theme::default().colors;
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(400.0, 500.0)).build_ui(|ui| {
            theme::install_fonts(ui.ctx());
            let r = ui.button("Pick");
            color_popup(&r).show(|ui| {
                if let Some(c) = color_grid(ui, &scheme, None) {
                    *picked.borrow_mut() = Some(c);
                }
            });
        });
        harness.get_by_label("Pick").click();
        harness.run();
        harness.get_by_role(Role::TextInput).click();
        harness.run();
        assert!(harness.query_by_role(Role::TextInput).is_some(), "the menu is still open after clicking the hex field");
        harness.get_by_role(Role::TextInput).type_text("00FF00");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(*picked.borrow(), Some(Some(ColorRef::rgb(Rgba::from_hex("00FF00").unwrap()))));
        assert!(harness.query_by_role(Role::TextInput).is_none(), "picking a colour closes the menu");
    }
}
