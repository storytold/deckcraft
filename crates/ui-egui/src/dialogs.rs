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
        "smartart" => "Choose a SmartArt Graphic",
        "bullets" => "Bullets and Numbering",
        "action" => "Action Settings",
        "quit" => "DeckCraft",
        "themeColors" => "Create New Theme Colors",
        "themeFonts" => "Create New Theme Fonts",
        _ => "DeckCraft",
    }
}

/// Render the active dialog, clearing it when dismissed and preserving or restoring application
/// state as its close path requires.
pub fn show(app: &mut SlideApp, ctx: &egui::Context) {
    // The equation editor lives as long as its dialog, however that closes.
    if app.dialog.as_ref().is_none_or(|d| d.id != "equation") {
        app.equation.open = None;
    }
    let Some(mut d) = app.dialog.take() else { return };
    let mut open = true;
    let mut close = false;
    // The equation is edited where it sits on the slide, not in a window.
    if d.id == "equation" {
        let close = crate::equation_editor::in_place(app, ctx, &mut d);
        if close {
            app.equation.open = None;
        } else {
            app.dialog = Some(d);
        }
        return;
    }
    let mut window = egui::Window::new(title(&d.id))
        .id(egui::Id::unique(("dialog", d.id.clone())))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, -40.0))
        .open(&mut open);
    // Two columns of colour rows plus Save must fit the first frame (Theme Colors), as must Save
    // in Theme Fonts. A later resize clips the buttons (the window size sticks) and clicks miss them.
    if d.id == "themeColors" {
        window = window.default_size(vec2(760.0, 420.0)).min_width(720.0).min_height(420.0);
    } else if d.id == "themeFonts" {
        window = window.default_size(vec2(460.0, 320.0)).min_width(420.0).min_height(300.0);
    }
    window.show(ctx, |ui| {
        ui.set_min_width(match d.id.as_str() {
            "themeColors" => 700.0,
            "themeFonts" => 400.0,
            _ => 340.0,
        });
        if d.id == "themeColors" {
            ui.set_min_height(360.0);
        } else if d.id == "themeFonts" {
            ui.set_min_height(280.0);
        }
        close = body(app, ui, &mut d);
    });
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if open && !close && !escape {
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

pub(crate) fn buttons(ui: &mut Ui, ok_label: &str) -> (bool, bool) {
    let mut ok = false;
    let mut cancel = false;
    ui.add_space(8.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let t = Tokens::get(ui.ctx());
        if ui.add(egui::Button::new(egui::RichText::new(ok_label).color(t.accent_text)).fill(t.accent).min_size(vec2(72.0, 24.0))).clicked()
            || ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            ok = true;
        }
        if ui.add(egui::Button::new("Cancel").min_size(vec2(72.0, 24.0))).clicked() {
            cancel = true;
        }
    });
    (ok, cancel)
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

fn check(ui: &mut Ui, d: &mut Dialog, label: &str, key: &str, default: bool) -> bool {
    let mut v = d.get(key, if default { "1" } else { "0" }) == "1";
    ui.checkbox(&mut v, label);
    d.fields.insert(key.into(), if v { "1".into() } else { "0".into() });
    v
}

fn num(d: &mut Dialog, key: &str, default: f64) -> f64 {
    d.get(key, "").trim().trim_end_matches(['"', '%', ' ']).parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(default)
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
mod preferences_tests;
