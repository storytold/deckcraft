//! Dialogs (Insert Table, Slide Size, Font, Paragraph, Tabs, Header & Footer, Hyperlink, Set Up Show, Zoom, …) and the
//! About window. The start screen is File › Home (`backstage.rs`).

use deckcraft_color::SchemeSlot;
use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::SlideApp;
use crate::theme::{self, Tokens};

#[derive(Clone, Debug, serde::Serialize)]
pub struct Dialog {
    pub id: String,
    pub params: Value,
    /// Field values while the dialog is open.
    #[serde(skip)]
    pub fields: std::collections::HashMap<String, String>,
}

impl Dialog {
    pub fn new(id: &str) -> Self {
        Dialog { id: id.into(), params: Value::Null, fields: Default::default() }
    }
    pub fn modal(&self) -> bool {
        true
    }
    fn get(&mut self, k: &str, default: &str) -> String {
        self.fields.entry(k.into()).or_insert_with(|| default.to_string()).clone()
    }
}

fn title(id: &str) -> &'static str {
    match id {
        "table" => "Insert Table",
        "slideSize" => "Slide Size",
        "sizePosition" => "Size and Position",
        "headerFooter" => "Header and Footer",
        "hyperlink" => "Insert Hyperlink",
        "setupShow" => "Set Up Slide Show",
        "zoom" => "Zoom",
        "comment" => "New Comment",
        "altText" => "Alt Text",
        "outline" => "Slides from Outline",
        "renameSection" => "Rename Section",
        "reuseSlides" => "Reuse Slides",
        "renameShape" => "Rename",
        "renameLayout" => "Rename Layout",
        "paragraph" => "Paragraph",
        "tabs" => "Tabs",
        "font" => "Font",
        "about" => "About DeckCraft",
        "preferences" => "Preferences",
        "autocorrect" => "AutoCorrect",
        "gridOptions" => "Grid Options",
        "columns" => "Columns",
        "export" => "Export",
        "symbol" => "Symbol",
        "photoAlbum" => "Photo Album",
        "replaceFonts" => "Replace Font",
        "equation" => "Equation",
        "dateTime" => "Date and Time",
        "spelling" => "Spelling",
        "pasteSpecial" => "Paste Special",
        "find" => "Find and Replace",
        "accessibility" => "Accessibility Checker",
        "language" => "Language",
        "customShow" => "Custom Shows",
        "chartData" => "Chart Data",
        "trim" => "Trim Media",
        "smartart" => "Choose a Diagram Graphic",
        "bullets" => "Bullets and Numbering",
        "action" => "Action Settings",
        "quit" => "DeckCraft",
        "themeColors" => "Create New Theme Colors",
        "themeFonts" => "Create New Theme Fonts",
        _ => "DeckCraft",
    }
}

/// The width a dialog's body takes: (at least, at most).
fn widths(id: &str) -> (f32, f32) {
    match id {
        "about" => (640.0, 700.0),
        "export" => (580.0, 600.0),
        "chartData" => (420.0, 720.0),
        // Two columns of colour rows (Theme Colors).
        "themeColors" => (720.0, 760.0),
        "themeFonts" => (420.0, 460.0),
        "symbol" | "smartart" | "outline" | "accessibility" | "spelling" => (440.0, 520.0),
        _ => (380.0, 440.0),
    }
}

/// The open dialog, as PhotoCraft's are: a modal card with its title and a hairline, the window
/// behind barely dimmed. It opens centred and stays where it's dragged (by its title); Esc
/// cancels it (the topmost, with no menu open), a click outside does nothing.
pub fn show(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(mut d) = app.dialog.take() else { return };
    let id = egui::Id::unique(("dialog", d.id.clone()));
    let pinned: Option<egui::Vec2> = ctx.data(|m| m.get_temp(id));
    let area = match pinned {
        Some(offset) => egui::Modal::default_area(id).anchor(Align2::LEFT_TOP, offset),
        None => egui::Modal::default_area(id),
    };
    let mut close = false;
    let mut dismissed = false;
    let mut drag = egui::Vec2::ZERO;
    let mut sizing = false;
    let prompt = matches!(d.id.as_str(), "quit" | "close");
    let modal = egui::Modal::new(id).area(area).backdrop_color(Color32::from_black_alpha(28)).frame(crate::form::frame(ctx)).show(ctx, |ui| {
        sizing = ui.is_sizing_pass();
        let (min, max) = widths(&d.id);
        ui.set_min_width(min);
        ui.set_max_width(max);
        ui.spacing_mut().item_spacing.y = 8.0;
        if !prompt {
            // Painted, not a label: the dialog itself carries its title's name (as egui's windows
            // do), so finding it by name gives the whole card.
            let galley = ui.painter().layout_no_wrap(title(&d.id).to_string(), theme::bold(15.0), Tokens::get(ui.ctx()).text);
            let (tr, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
            ui.painter().galley(tr.min, galley, Tokens::get(ui.ctx()).text);
            let bar = Rect::from_min_max(tr.min, pos2(ui.max_rect().right(), tr.bottom()));
            // The close button at the title's right end: the same as Cancel (or Esc).
            let xr = Rect::from_center_size(pos2(bar.right() - 9.0, bar.center().y), vec2(18.0, 18.0));
            let xresp = ui.interact(xr, id.with("close"), Sense::click());
            xresp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Close window"));
            let tk = Tokens::get(ui.ctx());
            if xresp.hovered() {
                ui.painter().rect_filled(xr, CornerRadius::same(4), tk.hover);
            }
            crate::icons::paint_mono(ui.painter(), xr.shrink(4.0), crate::icons::Icon::Close, tk.text_dim, false);
            dismissed = xresp.on_hover_text("Close").clicked();
            let bar = Rect::from_min_max(bar.min, pos2(xr.left() - 4.0, bar.bottom()));
            drag = ui.interact(bar, id.with("title"), Sense::drag()).drag_delta();
            ui.add_space(2.0);
            crate::form::hairline(ui);
            ui.add_space(4.0);
        }
        close = body(app, ui, &mut d);
    });
    let name = title(&d.id).to_string();
    modal.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Window, true, &name));
    if !sizing && (pinned.is_none() || drag != egui::Vec2::ZERO) {
        let screen = ctx.content_rect();
        let room = (screen.size() - modal.response.rect.size()).max(egui::Vec2::ZERO);
        let offset = pinned.unwrap_or(modal.response.rect.min - screen.min) + drag;
        ctx.data_mut(|m| m.insert_temp(id, offset.clamp(egui::Vec2::ZERO, room)));
    }
    // Esc cancels (Preferences then puts its settings back).
    let escaped = dismissed
        // One dialog is open at a time, so Esc is its own from the first frame (a new modal is not
        // yet the top one there); a menu open in it takes the Esc instead.
        || !close && !modal.any_popup_open && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    if close || escaped {
        ctx.data_mut(|m| m.remove::<egui::Vec2>(id));
    }
    if !close && !escaped {
        app.dialog = Some(d);
    } else if !close && d.id == "preferences" {
        // Esc or the window's close button: the same as Cancel.
        restore_preferences(app, &d);
    } else if app.dialog.is_none()
        && let Some(ret) = d.params.get("returnParams").filter(|v| v.is_object()).cloned()
    {
        // Tabs… opened from Paragraph: closing it (OK, Cancel, Esc or the window) returns there.
        let id = d.params.get("returnId").and_then(Value::as_str).unwrap_or("paragraph");
        let mut back = Dialog::new(id);
        back.params = ret;
        app.dialog = Some(back);
    }
}

/// OK (labelled `ok_label`) and Cancel; Enter is OK.
fn buttons(ui: &mut Ui, ok_label: &str) -> (bool, bool) {
    buttons_ex(ui, ok_label, true)
}

/// [`buttons`] for a dialog typing several lines: Enter is a new line there, ⌘Enter is OK.
fn buttons_multiline(ui: &mut Ui, ok_label: &str) -> (bool, bool) {
    // ⌘ as the Enter press carried it.
    let cmd = ui.input(|i| {
        i.modifiers.command
            || i.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. } if modifiers.command))
    });
    buttons_ex(ui, ok_label, cmd)
}

fn buttons_ex(ui: &mut Ui, ok_label: &str, enter: bool) -> (bool, bool) {
    use crate::form::{Button, ButtonRole};
    match crate::form::buttons(ui, &[Button::new(ButtonRole::Default, ok_label), Button::new(ButtonRole::Cancel, "Cancel")], enter) {
        Some(ButtonRole::Default) => (true, false),
        Some(_) => (false, true),
        None => (false, false),
    }
}

/// A closing button alone (Close, Done).
fn close_button(ui: &mut Ui, label: &str) -> bool {
    use crate::form::{Button, ButtonRole};
    crate::form::buttons(ui, &[Button::new(ButtonRole::Default, label)], true).is_some()
}

/// Draw a labelled single-line field bound to `key`, falling back to `default`, and store the
/// edited value in the dialog.
fn field(ui: &mut Ui, d: &mut Dialog, label: &str, key: &str, default: &str) {
    let mut v = d.get(key, default);
    ui.horizontal(|ui| {
        let l = ui.add_sized(vec2(120.0, 20.0), egui::Label::new(label));
        ui.add(egui::TextEdit::singleline(&mut v).desired_width(180.0)).labelled_by(l.id);
    });
    d.fields.insert(key.into(), v);
}

/// A labelled number field row (with its unit, ↑ / ↓ and arithmetic).
#[allow(clippy::too_many_arguments)]
fn number(ui: &mut Ui, d: &mut Dialog, label: &str, key: &str, default: f64, unit: &str, step: f64, range: (f64, f64)) -> f64 {
    let v = num(d, key, default);
    let mut out = v;
    ui.horizontal(|ui| {
        crate::form::label(ui, label);
        if let Some(n) = crate::form::number(ui, key, v, unit, step, range, 110.0) {
            out = n;
        }
    });
    d.fields.insert(key.into(), crate::widgets::fmt_num(out, ""));
    out
}

/// A labelled dropdown row over `options` (value, label); returns the value chosen.
fn dropdown_row(ui: &mut Ui, d: &mut Dialog, label: &str, key: &str, default: &str, options: &[(&str, &str)]) -> String {
    let cur = d.get(key, default);
    let shown = options.iter().find(|o| o.0 == cur).map(|o| o.1).unwrap_or(cur.as_str()).to_string();
    let mut chose = None;
    ui.horizontal(|ui| {
        crate::form::label(ui, label);
        let w = crate::form::field_width(ui);
        crate::form::dropdown(ui, label, &shown, w, |ui| {
            for (v, l) in options {
                if ui.selectable_label(*v == cur, *l).clicked() {
                    chose = Some(v.to_string());
                    ui.close();
                }
            }
        });
    });
    let v = chose.unwrap_or(cur);
    d.fields.insert(key.into(), v.clone());
    v
}

/// A checkbox row under the label column.
fn check(ui: &mut Ui, d: &mut Dialog, label: &str, key: &str, default: bool) -> bool {
    let mut v = d.get(key, if default { "1" } else { "0" }) == "1";
    ui.horizontal(|ui| {
        crate::form::no_label(ui);
        crate::form::checkbox(ui, &mut v, label);
    });
    d.fields.insert(key.into(), if v { "1".into() } else { "0".into() });
    v
}

fn num(d: &mut Dialog, key: &str, default: f64) -> f64 {
    crate::widgets::parse_num(&d.get(key, "")).filter(|v| v.is_finite()).unwrap_or(default)
}

fn num_text(v: f64) -> String {
    crate::widgets::fmt_num(v, "")
}

fn field_of(d: &Dialog, key: &str) -> String {
    d.fields.get(key).cloned().unwrap_or_default()
}

fn parse_shown(s: &str) -> Option<f64> {
    let t = s.trim().trim_end_matches(['%', '"']).trim();
    if t.is_empty() {
        return None;
    }
    t.parse().ok().filter(|v: &f64| v.is_finite())
}

fn edited(d: &Dialog, key: &str) -> bool {
    field_of(d, key) != field_of(d, &format!("seed{key}"))
}

fn exact_of(d: &Dialog, key: &str) -> Option<f64> {
    parse_shown(&field_of(d, &format!("{key}Pt")))
}

/// Draw a labelled single-line text row bound to a dialog key and store the edited value.
fn text_row(ui: &mut Ui, d: &mut Dialog, label: &str, key: &str) {
    let mut v = field_of(d, key);
    ui.horizontal(|ui| {
        let l = ui.add_sized(vec2(148.0, 20.0), egui::Label::new(label));
        ui.add(egui::TextEdit::singleline(&mut v).desired_width(96.0)).labelled_by(l.id);
    });
    d.fields.insert(key.into(), v);
}

use deckcraft_text::datetime::DATE_KINDS;

mod about;
mod alt_text;
mod body;
mod columns;
mod date_time;
mod equation;
mod export;
mod find;
mod font;
mod grid_options;
mod hyperlink;
mod language;
mod paragraph;
mod photo_album;
mod preferences;
mod size_position;
mod slide_size;
mod tabs;
mod theme_colors;
mod theme_fonts;
mod trim;
mod zoom;

pub use about::{about, paint_logo, version_line};
use body::body;
#[cfg(test)]
use hyperlink::{
    CurrentLink, HyperlinkSeed, common_click, hyperlink_command, hyperlink_seed_of, hyperlink_should_apply, link_covering, seed_from, seed_of_link,
};
pub use language::selected_language;
use preferences::restore_preferences;
pub use zoom::parse_zoom_percent;

fn seed_once(d: &mut Dialog, key: &str, val: String) {
    d.fields.entry(key.to_string()).or_insert(val);
}

fn json_text(v: Option<&Value>, default: &str) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.as_f64().map(|x| crate::widgets::fmt_num(x, "")).unwrap_or_else(|| default.to_string()),
        Some(Value::Bool(true)) => "1".into(),
        Some(Value::Bool(false)) => "0".into(),
        _ => default.to_string(),
    }
}

fn color_label(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(o) => o.get("scheme").or_else(|| o.get("rgb")).and_then(Value::as_str).unwrap_or("").to_string(),
        _ => String::new(),
    }
}

/// Draw a uniquely identified accessible choice combo box and store the selected key;
/// the caller supplies the visible label.
fn choice(ui: &mut Ui, d: &mut Dialog, salt: &str, name: &str, key: &str, choices: &[(&str, &str)]) {
    let cur = d.get(key, "");
    let label = choices.iter().find(|(v, _)| *v == cur).map(|(_, l)| *l).unwrap_or(cur.as_str());
    let combo = craft_ui::ComboBox::from_id_salt(salt).selected_text(label).width(140.0).show_ui(ui, |ui| {
        for (v, l) in choices {
            if ui.selectable_label(cur == *v, *l).clicked() {
                d.fields.insert((*key).into(), (*v).into());
            }
        }
    });
    combo.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, name));
}

fn labeled_text(ui: &mut Ui, d: &mut Dialog, label: &str, key: &str, width: f32) -> String {
    let mut v = d.get(key, "");
    let l = ui.label(label);
    ui.add(egui::TextEdit::singleline(&mut v).desired_width(width)).labelled_by(l.id);
    d.fields.insert(key.into(), v.clone());
    v
}

fn finite_pt(text: &str) -> Option<f64> {
    text.trim().trim_end_matches("pt").trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

fn pt_text(v: Option<&Value>, default: f64) -> String {
    match v.and_then(Value::as_f64).filter(|n| n.is_finite()) {
        Some(n) => crate::widgets::fmt_num(n, ""),
        None => crate::widgets::fmt_num(default, ""),
    }
}

fn inches_text(v: Option<&Value>) -> String {
    crate::widgets::fmt_num(v.and_then(Value::as_f64).filter(|n| n.is_finite()).unwrap_or(0.0) / 72.0, "")
}

pub mod credits;
#[cfg(test)]
mod font_dialog_tests;
#[cfg(not(target_arch = "wasm32"))]
pub(super) mod updates_ui;

#[cfg(test)]
mod size_position_tests;

#[cfg(test)]
mod columns_dialog_tests;

#[cfg(test)]
mod photo_album_tests;

#[cfg(test)]
mod theme_colors_tests;

#[cfg(test)]
mod theme_fonts_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod spelling_tests;

#[cfg(test)]
mod paragraph_dialog_tests;

#[cfg(test)]
#[path = "dialogs_tests.rs"]
mod modal_tests;

#[cfg(test)]
mod preferences_tests;
