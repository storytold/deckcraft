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
    let Some(mut d) = app.dialog.take() else { return };
    let mut open = true;
    let mut close = false;
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

/// Room the About credits need for their table; the About tab keeps its compact size.
const CREDITS_SIZE: egui::Vec2 = vec2(640.0, 380.0);

/// The Contributors / Models body in a block of its own fixed size, below the tab row. Sizing the
/// dialog's `ui` instead (`set_min_size` / `set_max_height`) squeezed the tab row and the first
/// credits row together, so the text was garbled and the tabs stopped answering (#843).
fn credits_block(ui: &mut Ui, models: bool) -> Rect {
    ui.allocate_ui(CREDITS_SIZE, |ui| {
        ui.set_min_size(CREDITS_SIZE);
        if models { crate::credits::models_ui(ui) } else { crate::credits::contributors_ui(ui) }
    })
    .response
    .rect
}

fn buttons(ui: &mut Ui, ok_label: &str) -> (bool, bool) {
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

/// What the Preferences dialog can change, as it was when the dialog opened.
fn preferences_snapshot(app: &SlideApp) -> Value {
    json!({
        "dark": app.ui.brightness == theme::Brightness::Dark,
        "uiScale": app.ui.ui_scale,
        "prefs": serde_json::to_value(&app.session.prefs).unwrap_or(Value::Null),
    })
}

/// Puts back the settings the Preferences dialog opened with (Cancel, Esc, the close button).
fn restore_preferences(app: &mut SlideApp, d: &Dialog) {
    let Some(before) = d.params.get("before") else { return };
    if let Some(dark) = before.get("dark").and_then(Value::as_bool)
        && dark != (app.ui.brightness == theme::Brightness::Dark)
    {
        let _ = app.run("view.dark", json!({ "on": dark }));
    }
    if let Some(scale) = before.get("uiScale").and_then(Value::as_f64) {
        app.ui.ui_scale = scale as f32;
    }
    if let Some(prefs) = before.get("prefs").and_then(|p| serde_json::from_value(p.clone()).ok()) {
        app.session.prefs = prefs;
    }
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

/// "Mark as decorative" starts checked when every selected shape is already decorative.
fn alt_text_starts_decorative(decorative: impl IntoIterator<Item = bool>) -> bool {
    let mut any = false;
    for on in decorative {
        if !on {
            return false;
        }
        any = true;
    }
    any
}

fn check(ui: &mut Ui, d: &mut Dialog, label: &str, key: &str, default: bool) -> bool {
    let mut v = d.get(key, if default { "1" } else { "0" }) == "1";
    ui.checkbox(&mut v, label);
    d.fields.insert(key.into(), if v { "1".into() } else { "0".into() });
    v
}

/// A Slide Size field set to `pt`: two decimals of the unit shown, the exact points kept beside.
fn put_slide_dim(d: &mut Dialog, key: &str, pt: f64, ppu: f64) {
    d.fields.insert(key.into(), format!("{:.2}", pt / ppu));
    d.fields.insert(format!("{key}Pt"), pt.to_string());
}

/// A Slide Size field in points: the exact value while the text is still its rendering, else
/// the number typed (`default` when it is not one).
fn slide_dim_points(d: &Dialog, key: &str, ppu: f64, default: f64) -> f64 {
    let text = field_of(d, key);
    if let Some(pt) = exact_of(d, key)
        && text == format!("{:.2}", pt / ppu)
    {
        return pt;
    }
    text.trim().trim_end_matches(['"', '%', ' ']).parse::<f64>().ok().filter(|v| v.is_finite()).map_or(default, |v| v * ppu)
}

/// The unit's abbreviation in a field label, "Width (cm):".
fn unit_abbr(unit: deckcraft_geom::Unit) -> &'static str {
    match unit {
        deckcraft_geom::Unit::Inches => "in",
        deckcraft_geom::Unit::Centimeters => "cm",
        deckcraft_geom::Unit::Points => "pt",
        deckcraft_geom::Unit::Pixels => "px",
    }
}

fn num(d: &mut Dialog, key: &str, default: f64) -> f64 {
    d.get(key, "").trim().trim_end_matches(['"', '%', ' ']).parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(default)
}

fn album_items(d: &Dialog) -> Option<Vec<Value>> {
    let raw = d.fields.get("items").map(String::as_str).unwrap_or("[]");
    serde_json::from_str::<Value>(raw).ok().and_then(|v| v.as_array().cloned())
}

fn store_album_items(d: &mut Dialog, items: &[Value]) {
    let raw = serde_json::to_string(items).unwrap_or_else(|_| "[]".into());
    d.fields.insert("items".into(), raw);
}

fn album_item_label(v: &Value) -> String {
    if let Some(text) = v.get("text").and_then(Value::as_str) {
        let text = text.trim();
        if text.is_empty() { "Text Box".to_string() } else { format!("Text: {text}") }
    } else {
        v.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("Picture").to_string()
    }
}

fn pick_album_picture(app: &mut SlideApp) -> Option<Value> {
    let path = if let Some(pick) = app.services.pick_open.as_mut() {
        pick("picture")
    } else {
        app.set_status("Pass pictures to insert.photoAlbum");
        None
    };
    let path = path?;
    let loaded = if let Some(read) = app.services.read.as_ref() {
        match read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                app.set_status(e);
                return None;
            }
        }
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) => {
                    app.set_status(format!("{path}: {e}"));
                    return None;
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            app.set_status("The web build inserts album pictures from data passed to insert.photoAlbum");
            return None;
        }
    };
    if loaded.is_empty() {
        app.set_status("That file is empty");
        return None;
    }
    if loaded.len() > 32 * 1024 * 1024 {
        app.set_status("That picture is larger than 32 MB");
        return None;
    }
    let name = std::path::Path::new(&path).file_name().and_then(std::ffi::OsStr::to_str).filter(|s| !s.is_empty()).unwrap_or("picture").to_string();
    Some(json!({"name": name, "data": deckcraft_engine::cmd::base64_encode(&loaded)}))
}

/// Draw an accessible album combo box, preserving its current/default selection and storing a
/// newly chosen key; the caller supplies the visible label.
fn album_choice(ui: &mut Ui, d: &mut Dialog, id: &str, name: &str, key: &str, default: &str, choices: &[(&str, &str)]) {
    let cur = d.get(key, default);
    let label = choices.iter().find(|(k, _)| *k == cur).map(|(_, l)| *l).unwrap_or(default);
    let opened = egui::ComboBox::from_id_salt(id).selected_text(label).width(220.0).show_ui(ui, |ui| {
        for (k, l) in choices {
            if ui.selectable_label(cur == *k, *l).clicked() {
                d.fields.insert(key.into(), (*k).to_string());
            }
        }
    });
    let access = name.to_string();
    let shown = label.to_string();
    opened.response.widget_info(move || {
        let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, true, access.clone());
        info.current_text_value = Some(shown.clone());
        info
    });
}

const ALBUM_THEMES: &[&str] = &["DeckCraft", "Harbor", "Ember", "Meadow", "Nocturne", "Paper", "Coral Reef", "Slate"];

fn current_theme(app: &SlideApp) -> String {
    app.session
        .active()
        .and_then(|d| d.doc.masters.first())
        .map(|m| m.theme.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "DeckCraft".into())
}

fn album_json_id(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// Copy a command's dialog params into fields once. A fresh album has no `items`.
fn seed_photo_album(app: &SlideApp, d: &mut Dialog) {
    if d.fields.contains_key("seeded") {
        return;
    }
    d.fields.insert("seeded".into(), "1".into());
    let p = d.params.clone();
    if let Some(items) = p.get("items").and_then(Value::as_array) {
        store_album_items(d, items);
        d.fields.insert("editing".into(), "1".into());
    }
    for key in ["layout", "frame", "theme"] {
        if let Some(v) = p.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()) {
            d.fields.insert(key.into(), v.to_string());
        }
    }
    for key in ["captions", "grayscale"] {
        if let Some(v) = p.get(key).and_then(Value::as_bool) {
            d.fields.insert(key.into(), if v { "1".into() } else { "0".into() });
        }
    }
    if let Some(id) = p.get("album").and_then(album_json_id).filter(|n| *n != 0) {
        d.fields.insert("album".into(), id.to_string());
    }
    let theme_was = d.fields.get("theme").cloned().filter(|s| !s.is_empty()).unwrap_or_else(|| current_theme(app));
    if !d.fields.contains_key("theme") {
        d.fields.insert("theme".into(), theme_was.clone());
    }
    d.fields.entry("themeWas".into()).or_insert(theme_was);
}

fn selected_album_picture(d: &Dialog, items: &[Value]) -> bool {
    let i = d.fields.get("sel").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    items.get(i).is_some_and(|item| item.get("data").is_some() || item.get("path").is_some())
}

fn with_selected_picture(d: &mut Dialog, f: impl FnOnce(&mut serde_json::Map<String, Value>)) {
    let Some(mut list) = album_items(d) else { return };
    let i = d.fields.get("sel").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    let Some(item) = list.get_mut(i) else { return };
    if item.get("data").is_none() && item.get("path").is_none() {
        return;
    }
    let Some(obj) = item.as_object_mut() else { return };
    f(obj);
    store_album_items(d, &list);
}

/// Photo Album. Returns true when the dialog should close.
fn photo_album_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    seed_photo_album(app, d);
    ui.set_min_width(440.0);
    egui::ScrollArea::vertical().max_height(520.0).id_salt("album_body").show(ui, |ui| {
        ui.set_min_width(420.0);
        ui.label("Pictures in album");
        let items = album_items(d).unwrap_or_default();
        let mut sel = d.get("sel", "0").parse::<usize>().unwrap_or(0);
        // Fixed height so adding pictures does not push Create past the window.
        egui::ScrollArea::vertical().max_height(120.0).min_scrolled_height(120.0).auto_shrink([false, false]).id_salt("album_list").show(ui, |ui| {
            if items.is_empty() {
                ui.label(egui::RichText::new("No pictures yet").color(Tokens::get(ui.ctx()).text_dim));
            }
            for (i, item) in items.iter().enumerate() {
                if ui.selectable_label(sel == i, album_item_label(item)).clicked() {
                    sel = i;
                }
            }
        });
        d.fields.insert("sel".into(), sel.to_string());
        ui.horizontal(|ui| {
            if ui.button("Add Pictures…").clicked()
                && let Some(item) = pick_album_picture(app)
            {
                match album_items(d) {
                    Some(mut list) if list.len() < 48 => {
                        list.push(item);
                        let next = list.len().saturating_sub(1);
                        store_album_items(d, &list);
                        d.fields.insert("sel".into(), next.to_string());
                    }
                    Some(_) => app.set_status("A photo album holds at most 48 pictures or text boxes"),
                    None => app.set_status("The picture list is invalid"),
                }
            }
            if ui.button("New Text Box").clicked() {
                let text = d.get("boxText", "Text Box");
                let text = text.trim();
                if text.is_empty() {
                    app.set_status("Type the text for the new text box");
                } else if text.chars().count() > 400 {
                    app.set_status("Text is longer than 400 characters");
                } else if let Some(mut list) = album_items(d) {
                    if list.len() >= 48 {
                        app.set_status("A photo album holds at most 48 pictures or text boxes");
                    } else {
                        list.push(json!({"text": text}));
                        let next = list.len().saturating_sub(1);
                        store_album_items(d, &list);
                        d.fields.insert("sel".into(), next.to_string());
                    }
                }
            }
            if ui.button("Remove").clicked()
                && let Some(mut list) = album_items(d)
                && let Some(i) = d.get("sel", "0").parse::<usize>().ok()
                && i < list.len()
            {
                list.remove(i);
                store_album_items(d, &list);
                let next = i.min(list.len().saturating_sub(1));
                d.fields.insert("sel".into(), next.to_string());
            }
        });
        field(ui, d, "Text box:", "boxText", "Text Box");
        let picture = selected_album_picture(d, &items);
        ui.label(album_picture_status(&items, sel));
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(picture, |ui| {
                if ui.button("Rotate Left").clicked() {
                    with_selected_picture(d, |obj| {
                        let cur = obj.get("rot").and_then(Value::as_f64).unwrap_or(0.0);
                        let next = ((cur - 90.0) % 360.0 + 360.0) % 360.0;
                        obj.insert("rot".into(), json!(next));
                    });
                }
                if ui.button("Rotate Right").clicked() {
                    with_selected_picture(d, |obj| {
                        let cur = obj.get("rot").and_then(Value::as_f64).unwrap_or(0.0);
                        let next = ((cur + 90.0) % 360.0 + 360.0) % 360.0;
                        obj.insert("rot".into(), json!(next));
                    });
                }
                if ui.button("More Brightness").clicked() {
                    nudge_picture(d, "brightness", 0.1);
                }
                if ui.button("Less Brightness").clicked() {
                    nudge_picture(d, "brightness", -0.1);
                }
                if ui.button("More Contrast").clicked() {
                    nudge_picture(d, "contrast", 0.1);
                }
                if ui.button("Less Contrast").clicked() {
                    nudge_picture(d, "contrast", -0.1);
                }
            });
        });
        ui.add_space(4.0);
        ui.label("Picture layout");
        album_choice(
            ui,
            d,
            "album_layout",
            "Picture layout",
            "layout",
            "1",
            &[
                ("fit", "Fit to slide"),
                ("1", "1 picture"),
                ("2", "2 pictures"),
                ("4", "4 pictures"),
                ("1title", "1 picture with title"),
                ("2title", "2 pictures with title"),
                ("4title", "4 pictures with title"),
            ],
        );
        ui.label("Frame shape");
        album_choice(
            ui,
            d,
            "album_frame",
            "Frame shape",
            "frame",
            "rect",
            &[("none", "None"), ("rect", "Rectangle"), ("rounded", "Rounded"), ("simple", "Simple, white")],
        );
        check(ui, d, "Captions under pictures", "captions", false);
        check(ui, d, "ALL pictures black and white", "grayscale", false);
        ui.label("Album theme");
        album_choice(ui, d, "album_theme", "Album theme", "theme", "DeckCraft", &ALBUM_THEMES.iter().map(|n| (*n, *n)).collect::<Vec<_>>());
    });
    let captions = d.fields.get("captions").is_some_and(|v| v == "1");
    let grayscale = d.fields.get("grayscale").is_some_and(|v| v == "1");
    let editing = d.fields.get("editing").is_some_and(|v| v == "1");
    let (ok, cancel) = buttons(ui, if editing { "Update" } else { "Create" });
    if ok {
        let Some(list) = album_items(d) else {
            app.set_status("The picture list is invalid");
            return false;
        };
        if list.is_empty() {
            app.set_status("Add a picture or a text box first");
            return false;
        }
        let layout = d.get("layout", "1");
        let frame = d.get("frame", "rect");
        let theme = d.get("theme", "DeckCraft");
        let theme_was = d.fields.get("themeWas").cloned().unwrap_or_default();
        let mut payload = json!({"pictures": list, "layout": layout, "frame": frame, "captions": captions, "grayscale": grayscale});
        if theme != theme_was
            && let Some(obj) = payload.as_object_mut()
        {
            obj.insert("theme".into(), json!(theme));
        }
        if editing {
            if let Some(album) = d.fields.get("album").filter(|s| !s.is_empty() && s.as_str() != "0")
                && let Ok(id) = album.parse::<u64>()
                && let Some(obj) = payload.as_object_mut()
            {
                obj.insert("album".into(), json!(id));
            }
            return app.run("insert.photoAlbumEdit", payload).is_ok();
        }
        return app.run("insert.photoAlbum", payload).is_ok();
    }
    cancel
}

fn album_picture_status(items: &[Value], sel: usize) -> String {
    let Some(item) = items.get(sel) else {
        return "Select a picture to rotate it or change brightness and contrast".into();
    };
    if item.get("data").is_none() && item.get("path").is_none() {
        return "Text boxes have no picture options".into();
    }
    let rot = item.get("rot").and_then(Value::as_f64).unwrap_or(0.0);
    let brightness = item.get("brightness").and_then(Value::as_f64).unwrap_or(0.0);
    let contrast = item.get("contrast").and_then(Value::as_f64).unwrap_or(0.0);
    format!("Rotation {rot:.0}°, brightness {brightness:.1}, contrast {contrast:.1}")
}

fn nudge_picture(d: &mut Dialog, key: &str, delta: f64) {
    with_selected_picture(d, |obj| {
        let cur = obj.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        let next = (((cur + delta) * 10.0).round() / 10.0).clamp(-1.0, 1.0);
        obj.insert(key.into(), json!(next));
    });
}

fn seed_theme_colors(d: &mut Dialog) {
    if d.fields.contains_key("seeded") {
        return;
    }
    let name = d.params.get("name").and_then(Value::as_str).unwrap_or("Custom");
    d.fields.insert("schemeName".into(), name.to_string());
    let obj = d.params.get("colors").and_then(Value::as_object);
    for slot in SchemeSlot::THEME {
        let hex = obj.and_then(|o| o.get(slot.xml_name())).and_then(Value::as_str).unwrap_or("000000");
        d.fields.insert(slot.xml_name().into(), hex.to_string());
    }
    d.fields.insert("seeded".into(), "1".into());
}

/// Draw one theme-colour row with its hex editor and store the value for the scheme slot.
fn theme_color_row(ui: &mut Ui, d: &mut Dialog, slot: SchemeSlot) {
    let key = slot.xml_name();
    let mut v = d.get(key, "000000");
    ui.horizontal(|ui| {
        ui.add_sized(vec2(188.0, 18.0), egui::Label::new(slot.label()));
        let fill = deckcraft_color::Rgba::from_hex(&v).map(theme::to_color32).unwrap_or_else(|| Color32::from_gray(200));
        let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        ui.painter().rect(rect, CornerRadius::same(2), fill, Stroke::new(1.0, Tokens::get(ui.ctx()).border), egui::StrokeKind::Inside);
        let resp = ui.add(egui::TextEdit::singleline(&mut v).desired_width(84.0).id_salt(("theme-hex", key)));
        let shown = v.clone();
        resp.widget_info(move || {
            let mut info = egui::WidgetInfo::labeled(egui::Role::TextInput, true, format!("{} hex", slot.label()));
            info.current_text_value = Some(shown.clone());
            info
        });
    });
    d.fields.insert(key.into(), v);
}

fn theme_color_payload(d: &mut Dialog) -> Result<Value, String> {
    let name = d.get("schemeName", "").trim().to_string();
    if name.is_empty() {
        return Err("Enter a scheme name.".into());
    }
    let mut colors = serde_json::Map::new();
    for slot in SchemeSlot::THEME {
        let raw = d.get(slot.xml_name(), "");
        let Some(rgb) = deckcraft_color::Rgba::from_hex(raw.trim()) else {
            return Err(format!("{} needs a colour like #RRGGBB.", slot.label()));
        };
        colors.insert(slot.xml_name().into(), Value::String(format!("#{}", rgb.hex())));
    }
    Ok(json!({"name": name, "colors": colors}))
}

/// Create New Theme Colors. Returns true when the dialog should close.
fn theme_colors_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    seed_theme_colors(d);
    // The window keeps the height of its contents and will not grow after the first frame, so the
    // rows stay short enough that Save is inside that rect.
    ui.style_mut().spacing.interact_size.y = 16.0;
    ui.style_mut().spacing.item_spacing.y = 2.0;
    ui.style_mut().spacing.button_padding.y = 2.0;
    let left = [SchemeSlot::Dk1, SchemeSlot::Lt1, SchemeSlot::Dk2, SchemeSlot::Lt2, SchemeSlot::Hlink, SchemeSlot::FolHlink];
    let right = [SchemeSlot::Accent1, SchemeSlot::Accent2, SchemeSlot::Accent3, SchemeSlot::Accent4, SchemeSlot::Accent5, SchemeSlot::Accent6];
    egui::Grid::new("theme-colors").spacing(vec2(18.0, 4.0)).show(ui, |ui| {
        for (a, b) in left.into_iter().zip(right) {
            theme_color_row(ui, d, a);
            theme_color_row(ui, d, b);
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    let mut name = d.get("schemeName", "");
    ui.horizontal(|ui| {
        ui.label("Name:");
        let resp = ui.add(egui::TextEdit::singleline(&mut name).desired_width(280.0).id_salt("schemeName"));
        let shown = name.clone();
        resp.widget_info(move || {
            let mut info = egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Scheme name");
            info.current_text_value = Some(shown.clone());
            info
        });
    });
    d.fields.insert("schemeName".into(), name);
    ui.label(egui::RichText::new("Applies to this presentation's theme colours.").size(11.0).color(Tokens::get(ui.ctx()).text_dim));
    let (ok, cancel) = buttons(ui, "Save");
    if ok {
        match theme_color_payload(d) {
            Ok(payload) => return app.run("design.colors", payload).is_ok(),
            Err(msg) => {
                app.set_status(msg);
                return false;
            }
        }
    }
    cancel
}

fn seed_theme_fonts(d: &mut Dialog) {
    if d.fields.contains_key("seeded") {
        return;
    }
    let name = d.params.get("name").and_then(Value::as_str).unwrap_or("Custom");
    let major = d.params.get("major").and_then(Value::as_str).unwrap_or("Inter");
    let minor = d.params.get("minor").and_then(Value::as_str).unwrap_or(major);
    d.fields.insert("schemeName".into(), name.to_string());
    d.fields.insert("major".into(), major.to_string());
    d.fields.insert("minor".into(), minor.to_string());
    d.fields.insert("seeded".into(), "1".into());
}

fn theme_font_choices(app: &SlideApp, selected: &str) -> Vec<String> {
    let mut v = crate::ribbon::font_families(app);
    for scheme in deckcraft_model::theme::builtin_font_schemes() {
        v.push(scheme.major.latin);
        v.push(scheme.minor.latin);
    }
    if !selected.is_empty() {
        v.push(selected.to_string());
    }
    let mut seen = std::collections::HashSet::new();
    v.retain(|face| !face.is_empty() && seen.insert(face.clone()));
    v
}

/// Draw an accessible font combo box and update the selected family; the caller supplies the
/// visible label.
fn font_combo(ui: &mut Ui, label: &str, choices: &[String], selected: &mut String) {
    let resp = egui::ComboBox::from_id_salt(label).selected_text(selected.as_str()).width(240.0).show_ui(ui, |ui| {
        for face in choices {
            if ui.selectable_label(selected == face, face).clicked() {
                *selected = face.clone();
            }
        }
    });
    let shown = selected.clone();
    let access = label.to_string();
    resp.response.widget_info(move || {
        let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, true, access.clone());
        info.current_text_value = Some(shown.clone());
        info
    });
}

fn theme_font_payload(d: &mut Dialog) -> Result<Value, String> {
    let name = d.get("schemeName", "").trim().to_string();
    if name.is_empty() {
        return Err("Enter a scheme name.".into());
    }
    let major = d.get("major", "").trim().to_string();
    if major.is_empty() {
        return Err("Choose a heading font.".into());
    }
    let minor = d.get("minor", "").trim().to_string();
    if minor.is_empty() {
        return Err("Choose a body font.".into());
    }
    Ok(json!({"name": name, "major": major, "minor": minor}))
}

/// Create New Theme Fonts. Returns true when the dialog should close.
fn theme_fonts_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    seed_theme_fonts(d);
    let major = d.get("major", "Inter");
    let minor = d.get("minor", "Inter");
    let headings = theme_font_choices(app, &major);
    let bodies = theme_font_choices(app, &minor);
    let mut major = major;
    let mut minor = minor;
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 18.0), egui::Label::new("Heading:"));
        font_combo(ui, "Heading font", &headings, &mut major);
    });
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 18.0), egui::Label::new("Body:"));
        font_combo(ui, "Body font", &bodies, &mut minor);
    });
    d.fields.insert("major".into(), major.clone());
    d.fields.insert("minor".into(), minor.clone());
    ui.add_space(4.0);
    // Theme faces are not egui families; naming an unbound family panics. The lines show the
    // chosen names at heading and body sizes.
    ui.label(egui::RichText::new(format!("Heading sample  {major}")).size(20.0));
    ui.label(egui::RichText::new(format!("Body sample  {minor}")).size(14.0));
    ui.add_space(4.0);
    let mut name = d.get("schemeName", "");
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 18.0), egui::Label::new("Name:"));
        let resp = ui.add(egui::TextEdit::singleline(&mut name).desired_width(240.0).id_salt("schemeName"));
        let shown = name.clone();
        resp.widget_info(move || {
            let mut info = egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Scheme name");
            info.current_text_value = Some(shown.clone());
            info
        });
    });
    d.fields.insert("schemeName".into(), name);
    ui.label(egui::RichText::new("Heading and body fonts for this presentation.").size(11.0).color(Tokens::get(ui.ctx()).text_dim));
    let (ok, cancel) = buttons(ui, "Save");
    if ok {
        match theme_font_payload(d) {
            Ok(payload) => return app.run("design.fonts", payload).is_ok(),
            Err(msg) => {
                app.set_status(msg);
                return false;
            }
        }
    }
    cancel
}

/// What Insert Hyperlink would edit on the current selection.
#[derive(Clone, Debug, PartialEq)]
enum CurrentLink {
    /// Nothing is linked. OK sets a new link (a caret at a word boundary inserts text).
    None,
    /// One link covers the text span or every selected shape.
    One(deckcraft_model::text::Hyperlink),
    /// The selection mixes different links. OK must not replace them until a new target is chosen.
    Mixed,
}

/// Fields the Insert Hyperlink dialog opens with.
#[derive(Clone, Debug, PartialEq)]
struct HyperlinkSeed {
    /// `url`, `email` or `slide`.
    mode: &'static str,
    url: String,
    email: String,
    slide: usize,
    tip: String,
    highlight: bool,
    /// The action is not a web address, email or slide this dialog edits (or the selection mixes
    /// links). OK leaves it in place until the user picks a different target.
    preserve: bool,
}

impl HyperlinkSeed {
    /// A new link: the address starts at `https://`, as the dialog always did.
    fn blank() -> Self {
        HyperlinkSeed { mode: "url", url: "https://".into(), email: String::new(), slide: 0, tip: String::new(), highlight: false, preserve: false }
    }
}

/// Turn one hyperlink into dialog fields. A slide that is no longer in the deck, and actions this
/// dialog does not edit (next slide, a program, …), are preserved so OK does not retarget them.
fn seed_of_link(link: &deckcraft_model::text::Hyperlink, slide_ids: &[deckcraft_model::SlideId]) -> HyperlinkSeed {
    use deckcraft_model::text::Action;
    let mut seed = HyperlinkSeed {
        mode: "url",
        url: String::new(),
        email: String::new(),
        slide: 0,
        tip: link.tooltip.clone(),
        highlight: link.highlight_click,
        preserve: true,
    };
    match &link.action {
        Action::Url { url } => {
            seed.preserve = false;
            if let Some(email) = url.strip_prefix("mailto:") {
                seed.mode = "email";
                seed.email = email.to_string();
                seed.url = "https://".into();
            } else {
                seed.url = url.clone();
            }
        }
        Action::Slide { slide } => {
            if let Some(i) = slide_ids.iter().position(|id| id == slide) {
                seed.preserve = false;
                seed.mode = "slide";
                seed.slide = i;
                seed.url = "https://".into();
            }
        }
        _ => {}
    }
    seed
}

fn seed_from(current: CurrentLink, slide_ids: &[deckcraft_model::SlideId]) -> HyperlinkSeed {
    match current {
        CurrentLink::None => HyperlinkSeed::blank(),
        CurrentLink::Mixed => HyperlinkSeed { preserve: true, url: String::new(), ..HyperlinkSeed::blank() },
        CurrentLink::One(link) => seed_of_link(&link, slide_ids),
    }
}

/// Links on the half-open span `[a, b)`. A point span is a new link: Insert Hyperlink inserts
/// there instead of editing a neighbouring run.
fn link_covering(body: &deckcraft_model::text::TextBody, a: (usize, usize), b: (usize, usize)) -> CurrentLink {
    if a == b {
        return CurrentLink::None;
    }
    let mut seen: Option<Option<deckcraft_model::text::Hyperlink>> = None;
    for (pi, para) in body.paragraphs.iter().enumerate() {
        if pi < a.0 || pi > b.0 {
            continue;
        }
        let from = if pi == a.0 { a.1 } else { 0 };
        let to = if pi == b.0 { b.1 } else { usize::MAX };
        if from >= to {
            continue;
        }
        let mut acc = 0usize;
        let mut covered = false;
        for run in &para.runs {
            let len = run.char_len();
            let end = acc + len;
            if len > 0 && end > from && acc < to {
                covered = true;
                match &seen {
                    None => seen = Some(run.props.link.clone()),
                    Some(prev) if prev == &run.props.link => {}
                    Some(_) => return CurrentLink::Mixed,
                }
            }
            acc = end;
        }
        if !covered && para.char_len() == 0 {
            match &seen {
                None => seen = Some(para.end_props.link.clone()),
                Some(prev) if prev == &para.end_props.link => {}
                Some(_) => return CurrentLink::Mixed,
            }
        }
    }
    match seen {
        Some(Some(link)) => CurrentLink::One(link),
        _ => CurrentLink::None,
    }
}

fn common_click(clicks: impl IntoIterator<Item = Option<deckcraft_model::text::Hyperlink>>) -> CurrentLink {
    let mut clicks = clicks.into_iter();
    let Some(first) = clicks.next() else { return CurrentLink::None };
    for next in clicks {
        if next != first {
            return CurrentLink::Mixed;
        }
    }
    match first {
        Some(link) => CurrentLink::One(link),
        None => CurrentLink::None,
    }
}

/// The link Insert Hyperlink would edit: text when a caret or selection is active, otherwise the
/// click action shared by the selected shapes.
fn hyperlink_seed_of(st: Option<&deckcraft_engine::DocState>) -> HyperlinkSeed {
    let Some(st) = st else { return HyperlinkSeed::blank() };
    let ids: Vec<deckcraft_model::SlideId> = st.doc.slides.iter().map(|s| s.id).collect();
    let current = if let Some(t) = &st.selection.text {
        match deckcraft_engine::cmd::text::body_of(st, t) {
            Some(body) => {
                let (a, b) = deckcraft_engine::cmd::text::format_range(st, t);
                link_covering(body, a, b)
            }
            None => CurrentLink::None,
        }
    } else {
        common_click(st.selected_shapes().into_iter().map(|sh| sh.click.clone()))
    };
    seed_from(current, &ids)
}

/// OK writes a link unless the dialog is holding an action it does not edit and the user has not
/// chosen a new target.
fn hyperlink_should_apply(seed: &HyperlinkSeed, mode: &str, url: &str, email: &str, slide: usize) -> bool {
    if !seed.preserve {
        return true;
    }
    if mode != seed.mode {
        return true;
    }
    match mode {
        "email" => email != seed.email,
        "slide" => slide != seed.slide,
        _ => url != seed.url,
    }
}

fn hyperlink_command(mode: &str, url: &str, email: &str, slide: usize, tip: &str, highlight: bool) -> Value {
    match mode {
        "slide" => json!({"slide": slide, "tooltip": tip, "highlightClick": highlight}),
        "email" => json!({"url": format!("mailto:{email}"), "tooltip": tip, "highlightClick": highlight}),
        _ => json!({"url": url, "tooltip": tip, "highlightClick": highlight}),
    }
}

/// Preset percents in the Zoom dialog, largest first.
const ZOOM_PRESETS: &[u32] = &[400, 200, 150, 100, 75, 66, 50, 33];

/// A typed zoom percent as a scale in `0.1..=4` (10%–400%). `None` when it is not a finite number.
pub fn parse_zoom_percent(text: &str) -> Option<f32> {
    let t = text.trim().trim_end_matches('%').trim();
    if t.is_empty() {
        return None;
    }
    let v = t.parse::<f64>().ok().filter(|n| n.is_finite())?;
    Some(((v / 100.0) as f32).clamp(0.1, 4.0))
}

/// Zoom dialog: a percent or Fit. Nothing changes until OK.
fn zoom_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    if !d.fields.contains_key("mode") {
        let shown = app.ui.zoom.unwrap_or(app.canvas_scale);
        let pct = if shown.is_finite() { (shown * 100.0).round().clamp(10.0, 400.0) } else { 100.0 };
        d.fields.insert("mode".into(), if app.ui.zoom.is_none() { "fit".into() } else { "percent".into() });
        d.fields.insert("percent".into(), format!("{pct:.0}"));
    }
    let mut mode = d.get("mode", "percent");
    let mut percent = d.get("percent", "100");
    ui.label("Zoom to");
    if ui.radio(mode == "fit", "Fit").clicked() {
        mode = "fit".into();
    }
    for pct in ZOOM_PRESETS {
        let on = mode == "percent" && percent == pct.to_string();
        if ui.radio(on, format!("{pct}%")).clicked() {
            mode = "percent".into();
            percent = pct.to_string();
        }
    }
    ui.horizontal(|ui| {
        let l = ui.label("Percent:");
        let edit = ui.add(egui::TextEdit::singleline(&mut percent).desired_width(72.0)).labelled_by(l.id);
        if edit.changed() {
            mode = "percent".into();
        }
        ui.label("%");
    });
    d.fields.insert("mode".into(), mode.clone());
    d.fields.insert("percent".into(), percent.clone());
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        if mode == "fit" {
            app.ui.zoom = None;
            return true;
        }
        match parse_zoom_percent(&percent) {
            Some(scale) => app.ui.zoom = Some(scale),
            None => {
                app.set_status("Enter a zoom percent");
                return false;
            }
        }
    }
    ok || cancel
}

/// Seconds for a Trim Media field. Three decimal places, trailing zeros dropped, so the text
/// round-trips the millisecond value the clip stores.
fn ms_as_seconds(ms: u32) -> String {
    let s = format!("{:.3}", f64::from(ms) / 1000.0);
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() { "0".to_string() } else { trimmed.to_string() }
}

/// Parse a seconds field back to milliseconds. Blank, junk and negatives become 0; the result is
/// rounded so `0.333` is 333 ms, not 332.
fn seconds_text_to_ms(text: &str) -> u32 {
    let v = text.trim().parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(0.0);
    let ms = (v.max(0.0) * 1000.0).round();
    if ms >= f64::from(u32::MAX) { u32::MAX } else { ms as u32 }
}

/// Start, end trim, fade in and fade out, as the Trim Media dialog shows them.
struct TrimSeed {
    start: String,
    end: String,
    fade_in: String,
    fade_out: String,
}

impl TrimSeed {
    fn zeros() -> Self {
        TrimSeed { start: "0".into(), end: "0".into(), fade_in: "0".into(), fade_out: "0".into() }
    }
    fn from_clip(m: &deckcraft_model::MediaClip) -> Self {
        TrimSeed {
            start: ms_as_seconds(m.trim_start_ms),
            end: ms_as_seconds(m.trim_end_ms),
            fade_in: ms_as_seconds(m.fade_in_ms),
            fade_out: ms_as_seconds(m.fade_out_ms),
        }
    }
}

/// The first selected audio or video. Other shapes in the selection are skipped. No media means
/// the old zeros, which OK then stores.
fn trim_seed(st: Option<&deckcraft_engine::DocState>) -> TrimSeed {
    let Some(st) = st else { return TrimSeed::zeros() };
    st.selected_shapes()
        .into_iter()
        .find_map(|sh| match &sh.kind {
            deckcraft_model::ShapeKind::Media(m) => Some(TrimSeed::from_clip(m)),
            _ => None,
        })
        .unwrap_or_else(TrimSeed::zeros)
}

/// Returns true when the dialog should close.
fn body(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let run = |app: &mut SlideApp, id: &str, p: Value| {
        let _ = app.run(id, p);
    };
    match d.id.as_str() {
        "table" => {
            field(ui, d, "Number of columns:", "cols", "5");
            field(ui, d, "Number of rows:", "rows", "2");
            let (ok, cancel) = buttons(ui, "Insert");
            if ok {
                let (r, c) = (num(d, "rows", 2.0) as usize, num(d, "cols", 5.0) as usize);
                run(app, "insert.table", json!({"rows": r, "cols": c}));
            }
            ok || cancel
        }
        "slideSize" => {
            let cur = app.session.active().map(|s| s.doc.slide_size).unwrap_or(deckcraft_model::defaults::WIDE);
            let len = app.session.prefs.units;
            let ppu = len.points_per_unit();
            // Each field keeps its exact size in points (`wPt`) beside the two decimals shown, so
            // a preset, Portrait or an untouched field is not nudged off by the rounding (13.33",
            // 33.87 cm read back would make the slide 959.76 or 960.09 pt wide).
            if !d.fields.contains_key("wPt") {
                put_slide_dim(d, "w", cur.width, ppu);
                put_slide_dim(d, "h", cur.height, ppu);
            }
            let label = ui.label("Slides sized for:");
            let preset = d.get("preset", "");
            let combo = egui::ComboBox::from_id_salt("sizes").selected_text(if preset.is_empty() { "Custom" } else { &preset }).show_ui(ui, |ui| {
                for (label, w, h) in deckcraft_model::defaults::SLIDE_SIZES {
                    if ui.selectable_label(preset == *label, *label).clicked() {
                        d.fields.insert("preset".into(), label.to_string());
                        put_slide_dim(d, "w", *w, ppu);
                        put_slide_dim(d, "h", *h, ppu);
                    }
                }
            });
            let _ = combo.response.labelled_by(label.id);
            let abbr = unit_abbr(len);
            let (w_seed, h_seed) = (field_of(d, "w"), field_of(d, "h"));
            field(ui, d, &format!("Width ({abbr}):"), "w", &w_seed);
            field(ui, d, &format!("Height ({abbr}):"), "h", &h_seed);
            let (w, h) = (slide_dim_points(d, "w", ppu, cur.width), slide_dim_points(d, "h", ppu, cur.height));
            ui.horizontal(|ui| {
                ui.label("Orientation:");
                let portrait = ui.radio(h > w, "Portrait").clicked();
                let landscape = ui.radio(w >= h, "Landscape").clicked();
                // Picking the other orientation swaps the width and height fields.
                if portrait && w > h || landscape && h > w {
                    put_slide_dim(d, "w", h, ppu);
                    put_slide_dim(d, "h", w, ppu);
                }
            });
            let maximize = check(ui, d, "Maximize content (else ensure fit)", "max", false);
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                let (w, h) = (slide_dim_points(d, "w", ppu, cur.width), slide_dim_points(d, "h", ppu, cur.height));
                run(app, "design.slideSize", json!({"w": w, "h": h, "scale": if maximize { "maximize" } else { "ensureFit" }}));
            }
            ok || cancel
        }
        "sizePosition" => size_position_dialog(app, ui, d),
        "headerFooter" => {
            let hf = app.session.active().map(|s| s.doc.header_footer.clone()).unwrap_or_default();
            let date = check(ui, d, "Date and time", "date", hf.date);
            field(ui, d, "  Fixed date (empty = update automatically):", "dateText", &hf.date_text);
            // The format applies to an automatic date; a fixed one is the text as typed.
            let automatic = d.get("dateText", &hf.date_text).is_empty();
            let fmt_now = if hf.date_format.is_empty() { "datetime1".to_string() } else { hf.date_format.clone() };
            // Not `d.get`: that stores the default as if the user had picked it.
            let fmt = d.fields.get("dateFormat").cloned().unwrap_or_else(|| fmt_now.clone());
            let rows = date_rows("en-US");
            let label = rows.iter().find(|(k, _)| *k == fmt).map(|(_, s)| s.clone()).unwrap_or_else(|| fmt.clone());
            ui.add_enabled_ui(date && automatic, |ui| {
                ui.horizontal(|ui| {
                    ui.label("  Format:");
                    egui::ComboBox::from_id_salt("hf-date-format").selected_text(label).width(200.0).show_ui(ui, |ui| {
                        for (kind, sample) in &rows {
                            if ui.selectable_label(fmt == *kind, sample).clicked() {
                                d.fields.insert("dateFormat".into(), (*kind).to_string());
                            }
                        }
                    });
                });
            });
            let num_on = check(ui, d, "Slide number", "num", hf.slide_number);
            let foot = check(ui, d, "Footer", "footer", hf.footer);
            field(ui, d, "  Footer text:", "footerText", &hf.footer_text);
            let hide = check(ui, d, "Don't show on title slide", "hide", hf.hide_on_title);
            ui.add_space(6.0);
            let mut apply_all = false;
            let mut apply = false;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                apply_all = ui.button("Apply to All").clicked();
                apply = ui.button("Apply").clicked();
                if ui.button("Cancel").clicked() {
                    d.fields.insert("cancel".into(), "1".into());
                }
            });
            if apply || apply_all {
                let dt = d.get("dateText", "");
                let ft = d.get("footerText", "");
                let mut params = serde_json::Map::new();
                params.insert("date".into(), json!(date));
                params.insert("dateText".into(), json!(dt));
                // Only a format the user picked: sending the default would rewrite every slide's
                // existing date field (an imported datetime4, say) to datetime1.
                if d.fields.contains_key("dateFormat") {
                    params.insert("dateFormat".into(), json!(d.get("dateFormat", &fmt_now)));
                }
                params.insert("slideNumber".into(), json!(num_on));
                params.insert("footer".into(), json!(foot));
                params.insert("footerText".into(), json!(ft));
                params.insert("hideOnTitle".into(), json!(hide));
                params.insert("all".into(), json!(apply_all));
                run(app, "design.headerFooter", serde_json::Value::Object(params));
            }
            apply || apply_all || d.fields.contains_key("cancel")
        }
        "hyperlink" => {
            let seed = hyperlink_seed_of(app.session.active());
            let slide_default = seed.slide.to_string();
            let mode = d.get("mode", seed.mode);
            ui.horizontal(|ui| {
                if ui.selectable_label(mode == "url", "Web Page or File").clicked() {
                    d.fields.insert("mode".into(), "url".into());
                }
                if ui.selectable_label(mode == "slide", "This Document").clicked() {
                    d.fields.insert("mode".into(), "slide".into());
                }
                if ui.selectable_label(mode == "email", "Email Address").clicked() {
                    d.fields.insert("mode".into(), "email".into());
                }
            });
            match mode.as_str() {
                "slide" => {
                    let titles: Vec<String> = app
                        .session
                        .active()
                        .map(|s| s.doc.slides.iter().enumerate().map(|(i, sl)| format!("{}. {}", i + 1, sl.title())).collect())
                        .unwrap_or_default();
                    let sel = d.get("slide", &slide_default).parse::<usize>().unwrap_or(0);
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        for (i, t) in titles.iter().enumerate() {
                            if ui.selectable_label(sel == i, t).clicked() {
                                d.fields.insert("slide".into(), i.to_string());
                            }
                        }
                    });
                }
                "email" => field(ui, d, "Email address:", "email", &seed.email),
                _ => field(ui, d, "Address:", "url", &seed.url),
            }
            field(ui, d, "ScreenTip:", "tip", &seed.tip);
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                let mode_now = d.get("mode", seed.mode);
                let url = d.get("url", &seed.url);
                let email = d.get("email", &seed.email);
                let slide = d.get("slide", &slide_default).parse::<usize>().unwrap_or(0);
                let tip = d.get("tip", &seed.tip);
                if hyperlink_should_apply(&seed, &mode_now, &url, &email, slide) {
                    run(app, "insert.hyperlink", hyperlink_command(&mode_now, &url, &email, slide, &tip, seed.highlight));
                }
            }
            ok || cancel
        }
        "setupShow" => {
            let sh = app.session.active().map(|s| s.doc.show.clone()).unwrap_or_default();
            let ty = d.get("type", &sh.show_type);
            ui.label("Show type");
            for (l, v) in [
                ("Presented by a speaker (full screen)", "speaker"),
                ("Browsed by an individual (window)", "browsed"),
                ("Browsed at a kiosk (full screen)", "kiosk"),
            ] {
                if ui.radio(ty == v, l).clicked() {
                    d.fields.insert("type".into(), v.into());
                }
            }
            let lp = check(ui, d, "Loop continuously until 'Esc'", "loop", sh.loop_until_esc);
            let nn = check(ui, d, "Show without narration", "nn", sh.without_narration);
            let na = check(ui, d, "Show without animation", "na", sh.without_animation);
            let ut = check(ui, d, "Use timings, if present", "ut", sh.use_timings);
            let names: Vec<String> = app.session.active().map(|s| s.doc.custom_shows.iter().map(|c| c.name.clone()).collect()).unwrap_or_default();
            let chosen = d.fields.entry("custom".into()).or_insert_with(|| sh.custom_show.clone().unwrap_or_default()).clone();
            let field = ui.label("Slides");
            let label = if chosen.is_empty() { "All slides".to_string() } else { chosen.clone() };
            let combo = egui::ComboBox::from_id_salt("setup_custom").selected_text(label).show_ui(ui, |ui| {
                if ui.selectable_label(chosen.is_empty(), "All slides").clicked() {
                    d.fields.insert("custom".into(), String::new());
                }
                for n in &names {
                    if ui.selectable_label(&chosen == n, n).clicked() {
                        d.fields.insert("custom".into(), n.clone());
                    }
                }
            });
            let _ = combo.response.labelled_by(field.id);
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                let ty = d.get("type", "speaker");
                let custom = d.fields.get("custom").cloned().unwrap_or_default();
                run(app, "show.setup", json!({"type": ty, "loop": lp, "noNarration": nn, "noAnimation": na, "useTimings": ut, "custom": custom}));
            }
            ok || cancel
        }
        "quit" => {
            let names: Vec<String> = app.session.documents().iter().filter(|d| d.is_dirty()).map(|d| d.title()).collect();
            if names.is_empty() {
                app.quit_confirmed = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                return true;
            }
            let what = if names.len() == 1 { format!("“{}”", names[0]) } else { format!("{} presentations", names.len()) };
            ui.label(egui::RichText::new(format!("Do you want to save the changes you made to {what}?")).strong());
            ui.label("Your changes will be lost if you don't save them.");
            ui.add_space(8.0);
            let mut done = false;
            ui.horizontal(|ui| {
                let t = Tokens::get(ui.ctx());
                if ui.add(egui::Button::new("Don't Save").min_size(vec2(84.0, 24.0))).clicked() {
                    let _ = app.session.execute("file.recovery.discard", &json!({}));
                    app.quit_confirmed = true;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    done = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save = ui.add(egui::Button::new(egui::RichText::new("Save").color(t.accent_text)).fill(t.accent).min_size(vec2(72.0, 24.0)));
                    if save.clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if app.save_all() {
                            app.quit_confirmed = true;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        done = true;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(vec2(72.0, 24.0))).clicked() {
                        done = true;
                    }
                });
            });
            done
        }
        "close" => {
            // File › Close and Ctrl+W on a presentation with unsaved changes.
            let Some(name) = app.session.active().map(|d| d.title()) else { return true };
            ui.label(egui::RichText::new(format!("Do you want to save the changes you made to “{name}”?")).strong());
            ui.label("Your changes will be lost if you don't save them.");
            ui.add_space(8.0);
            let mut done = false;
            ui.horizontal(|ui| {
                let t = Tokens::get(ui.ctx());
                if ui.add(egui::Button::new("Don't Save").min_size(vec2(84.0, 24.0))).clicked() {
                    run(app, "file.close", json!({}));
                    done = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save = ui.add(egui::Button::new(egui::RichText::new("Save").color(t.accent_text)).fill(t.accent).min_size(vec2(72.0, 24.0)));
                    if save.clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        app.save();
                        // In the browser a save is a download: the presentation stays "unsaved".
                        let web = app.services.pick_save.is_none() && app.services.download.is_some();
                        if web || app.session.active().is_some_and(|d| !d.is_dirty()) {
                            run(app, "file.close", json!({}));
                        }
                        done = true;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(vec2(72.0, 24.0))).clicked() {
                        done = true;
                    }
                });
            });
            done
        }
        "zoom" => zoom_dialog(app, ui, d),
        "comment" => {
            let mut v = d.get("text", "");
            ui.add(egui::TextEdit::multiline(&mut v).hint_text("Comment").desired_rows(4).desired_width(320.0));
            d.fields.insert("text".into(), v.clone());
            let (ok, cancel) = buttons(ui, "Post");
            if ok && !v.trim().is_empty() {
                run(app, "comment.add", json!({"text": v}));
                app.ui.pane = Some("comments".into());
            }
            ok || cancel
        }
        "bullets" => {
            // The current paragraph's list, as the dialog's starting point.
            let cur = app.session.execute("format.state", &json!({})).ok().unwrap_or(Value::Null);
            let numbered = cur.get("numbering").and_then(Value::as_bool).unwrap_or(false);
            let tab = d.get("tab", if numbered { "numbered" } else { "bulleted" });
            ui.horizontal(|ui| {
                for (k, l) in [("bulleted", "Bulleted"), ("numbered", "Numbered")] {
                    if ui.selectable_label(tab == k, l).clicked() {
                        d.fields.insert("tab".into(), k.into());
                    }
                }
            });
            ui.separator();
            let style_key = if tab == "numbered" { "scheme" } else { "char" };
            let current = d.get(style_key, if tab == "numbered" { "arabicPeriod" } else { "•" });
            let styles: &[(&str, &str)] = if tab == "numbered" {
                &[
                    ("arabicPeriod", "1. 2. 3."),
                    ("arabicParenR", "1) 2) 3)"),
                    ("romanUcPeriod", "I. II. III."),
                    ("romanLcPeriod", "i. ii. iii."),
                    ("alphaUcPeriod", "A. B. C."),
                    ("alphaLcParenR", "a) b) c)"),
                    ("alphaLcPeriod", "a. b. c."),
                ]
            } else {
                &[("•", "•"), ("○", "○"), ("■", "■"), ("□", "□"), ("◆", "◆"), ("➢", "➢"), ("✓", "✓"), ("–", "–"), ("★", "★")]
            };
            egui::Grid::new("bullet-styles").show(ui, |ui| {
                for (i, (k, l)) in styles.iter().enumerate() {
                    let r = ui.add_sized(
                        vec2(if tab == "numbered" { 88.0 } else { 44.0 }, 40.0),
                        egui::Button::selectable(current == *k, egui::RichText::new(*l).size(16.0)),
                    );
                    if r.clicked() {
                        d.fields.insert(style_key.into(), (*k).into());
                    }
                    if (i + 1) % if tab == "numbered" { 4 } else { 5 } == 0 {
                        ui.end_row();
                    }
                }
            });
            if tab == "bulleted" {
                field(ui, d, "Custom character:", "custom", "");
            } else {
                field(ui, d, "Start at:", "start", "1");
            }
            field(ui, d, "Size (% of text):", "size", "100");
            let colors = [
                ("", "Automatic"),
                ("tx1", "Text"),
                ("accent1", "Accent 1"),
                ("accent2", "Accent 2"),
                ("accent3", "Accent 3"),
                ("accent4", "Accent 4"),
                ("accent5", "Accent 5"),
                ("accent6", "Accent 6"),
            ];
            let color = d.get("color", "");
            ui.horizontal(|ui| {
                let field = ui.add_sized(vec2(120.0, 20.0), egui::Label::new("Color:"));
                let combo = egui::ComboBox::from_id_salt("bullet-color")
                    .selected_text(colors.iter().find(|c| c.0 == color).map_or("Automatic", |c| c.1))
                    .show_ui(ui, |ui| {
                        for (k, l) in colors {
                            if ui.selectable_label(color == k, l).clicked() {
                                d.fields.insert("color".into(), k.into());
                            }
                        }
                    });
                let _ = combo.response.labelled_by(field.id);
            });
            ui.horizontal(|ui| {
                if ui.button("None").on_hover_text("Remove bullets and numbering").clicked() {
                    d.fields.insert("none".into(), "1".into());
                }
            });
            let (ok, cancel) = buttons(ui, "OK");
            let none = d.fields.contains_key("none");
            if none {
                run(app, if tab == "numbered" { "format.numbering" } else { "format.bullets" }, json!({"on": false}));
            } else if ok {
                let mut p = json!({"on": true});
                if let Some(size) = d.get("size", "100").trim().trim_end_matches('%').parse::<f64>().ok().filter(|v| v.is_finite()) {
                    p["size"] = json!(size.clamp(25.0, 400.0));
                }
                if !color.is_empty() {
                    p["color"] = json!(color);
                }
                if tab == "numbered" {
                    p["scheme"] = json!(d.get("scheme", "arabicPeriod"));
                    p["start"] = json!(d.get("start", "1").trim().parse::<u64>().unwrap_or(1).clamp(1, 32767));
                    run(app, "format.numbering", p);
                } else {
                    let custom = d.get("custom", "");
                    let ch = custom.chars().find(|c| !c.is_whitespace()).map(String::from).unwrap_or_else(|| d.get("char", "•"));
                    p["char"] = json!(ch);
                    run(app, "format.bullets", p);
                }
            }
            ok || cancel || none
        }
        "action" => {
            use deckcraft_model::text::Action;
            let tab = d.get("tab", "click");
            // Start from the selected shape's current setting for this tab.
            let (cur, slides, shows) = app
                .session
                .active()
                .map(|st| {
                    let sh = st.selected_shapes().first().map(|x| if tab == "hover" { x.hover.clone() } else { x.click.clone() }).unwrap_or(None);
                    let slides: Vec<(deckcraft_model::SlideId, String)> =
                        st.doc.slides.iter().enumerate().map(|(i, sl)| (sl.id, format!("{}. {}", i + 1, sl.title()))).collect();
                    let shows: Vec<String> = st.doc.custom_shows.iter().map(|c| c.name.clone()).collect();
                    (sh, slides, shows)
                })
                .unwrap_or_default();
            let (cur_kind, cur_arg) = match cur.as_ref().map(|h| &h.action) {
                None => ("none", String::new()),
                Some(Action::NextSlide) => ("next", String::new()),
                Some(Action::PreviousSlide) => ("previous", String::new()),
                Some(Action::FirstSlide) => ("first", String::new()),
                Some(Action::LastSlide) => ("last", String::new()),
                Some(Action::LastViewed) => ("lastViewed", String::new()),
                Some(Action::EndShow) => ("endShow", String::new()),
                Some(Action::PlayMedia) => ("playMedia", String::new()),
                Some(Action::Url { url }) => ("url", url.clone()),
                Some(Action::CustomShow { name, .. }) => ("customShow", name.clone()),
                Some(Action::Program { path }) => ("program", path.clone()),
                Some(Action::Slide { slide }) => ("slide", slides.iter().position(|(id, _)| id == slide).unwrap_or(0).to_string()),
            };
            ui.horizontal(|ui| {
                for (k, l) in [("click", "Mouse Click"), ("hover", "Mouse Over")] {
                    if ui.selectable_label(tab == k, l).clicked() && tab != k {
                        d.fields.clear();
                        d.fields.insert("tab".into(), k.into());
                    }
                }
            });
            ui.separator();
            ui.label(if tab == "hover" { "Action on mouse over" } else { "Action on click" });
            let kind = d.get("kind", cur_kind);
            let hyperlink = !matches!(kind.as_str(), "none" | "program" | "playMedia");
            let pick = |d: &mut Dialog, ui: &mut Ui, k: &str, l: &str| {
                if ui.radio(d.get("kind", cur_kind) == k, l).clicked() {
                    d.fields.insert("kind".into(), k.into());
                }
            };
            pick(d, ui, "none", "None");
            ui.horizontal(|ui| {
                let field = ui.radio(hyperlink, "Hyperlink to:");
                if field.clicked() && !hyperlink {
                    d.fields.insert("kind".into(), "next".into());
                }
                let targets = [
                    ("next", "Next Slide"),
                    ("previous", "Previous Slide"),
                    ("first", "First Slide"),
                    ("last", "Last Slide"),
                    ("lastViewed", "Last Slide Viewed"),
                    ("endShow", "End Show"),
                    ("customShow", "Custom Show…"),
                    ("slide", "Slide…"),
                    ("url", "URL…"),
                ];
                let shown = targets.iter().find(|t| t.0 == kind).map_or("Next Slide", |t| t.1);
                ui.add_enabled_ui(hyperlink, |ui| {
                    let combo = egui::ComboBox::from_id_salt("action-target").selected_text(shown).show_ui(ui, |ui| {
                        for (k, l) in targets {
                            if ui.selectable_label(kind == k, l).clicked() {
                                d.fields.insert("kind".into(), k.into());
                            }
                        }
                    });
                    let _ = combo.response.labelled_by(field.id);
                });
            });
            let kind = d.get("kind", cur_kind);
            match kind.as_str() {
                "url" => field(ui, d, "Address:", "arg", if cur_kind == "url" { &cur_arg } else { "https://" }),
                "slide" => {
                    let sel = d.get("arg", if cur_kind == "slide" { &cur_arg } else { "0" }).parse::<usize>().unwrap_or(0);
                    egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                        for (i, (_, t)) in slides.iter().enumerate() {
                            if ui.selectable_label(sel == i, t).clicked() {
                                d.fields.insert("arg".into(), i.to_string());
                            }
                        }
                    });
                }
                "customShow" => {
                    if shows.is_empty() {
                        ui.label("This presentation has no custom shows (Slide Show ▸ Custom Slide Show).");
                    }
                    let sel = d.get("arg", if cur_kind == "customShow" { &cur_arg } else { shows.first().map_or("", String::as_str) });
                    for n in &shows {
                        if ui.selectable_label(&sel == n, n).clicked() {
                            d.fields.insert("arg".into(), n.clone());
                        }
                    }
                }
                _ => {}
            }
            pick(d, ui, "program", "Run program:");
            if d.get("kind", cur_kind) == "program" {
                field(ui, d, "Program:", "arg", if cur_kind == "program" { &cur_arg } else { "" });
            }
            pick(d, ui, "playMedia", "Play the shape's media");
            let hl = check(ui, d, "Highlight click", "highlight", cur.as_ref().is_none_or(|h| h.highlight_click));
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                let kind = d.get("kind", cur_kind);
                let arg = d.get("arg", &cur_arg);
                let mut p = json!({"on": tab, "action": kind, "highlight": hl});
                match kind.as_str() {
                    "url" => p["url"] = json!(arg),
                    "slide" => p["slide"] = json!(arg.parse::<usize>().unwrap_or(0)),
                    "customShow" => p["show"] = json!(arg),
                    "program" => p["path"] = json!(arg),
                    _ => {}
                }
                run(app, "shape.action", p);
            }
            ok || cancel
        }
        "altText" => {
            let (cur, deco_now) = app
                .session
                .active()
                .map(|s| {
                    let shapes = s.selected_shapes();
                    let cur = shapes.first().map(|x| x.descr.clone()).unwrap_or_default();
                    let deco = alt_text_starts_decorative(shapes.iter().map(|x| x.decorative));
                    (cur, deco)
                })
                .unwrap_or_default();
            ui.label("How would you describe this object and its context to someone who is blind or has low vision?");
            let mut v = d.get("text", &cur);
            ui.add(egui::TextEdit::multiline(&mut v).hint_text("Alt text").desired_rows(4).desired_width(340.0));
            d.fields.insert("text".into(), v.clone());
            let deco = check(ui, d, "Mark as decorative", "deco", deco_now);
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                run(app, "shape.altText", json!({"text": v, "decorative": deco}));
            }
            ok || cancel
        }
        "outline" => {
            ui.label("Paste or type an outline. Unindented lines become slide titles; tab-indented lines become bullets.");
            let mut v = d.get("text", "");
            ui.add(egui::TextEdit::multiline(&mut v).hint_text("Outline").desired_rows(10).desired_width(380.0).code_editor());
            d.fields.insert("text".into(), v.clone());
            let (ok, cancel) = buttons(ui, "Insert");
            if ok {
                run(app, "slide.fromOutline", json!({"text": v}));
            }
            ok || cancel
        }
        "reuseSlides" => {
            let path = d.params.get("path").and_then(Value::as_str).unwrap_or("").to_string();
            let slides: Vec<(usize, String)> = d
                .params
                .get("slides")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| {
                            Some((usize::try_from(x.get("index")?.as_u64()?).ok()?, x.get("title").and_then(Value::as_str).unwrap_or("").to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(path.clone());
            ui.label(format!("Insert slides from {name} after the current slide:"));
            // Unticked slides are remembered by index; everything starts ticked.
            let off = |d: &Dialog, i: usize| d.fields.get(&format!("off{i}")).is_some_and(|v| v == "1");
            ui.horizontal(|ui| {
                if ui.small_button("Select All").clicked() {
                    for (i, _) in &slides {
                        d.fields.remove(&format!("off{i}"));
                    }
                }
                if ui.small_button("Select None").clicked() {
                    for (i, _) in &slides {
                        d.fields.insert(format!("off{i}"), "1".into());
                    }
                }
            });
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for (i, title) in &slides {
                    let mut on = !off(d, *i);
                    let label = if title.trim().is_empty() { format!("{}  (no title)", i + 1) } else { format!("{}  {}", i + 1, title.trim()) };
                    if ui.checkbox(&mut on, label).changed() {
                        if on {
                            d.fields.remove(&format!("off{i}"));
                        } else {
                            d.fields.insert(format!("off{i}"), "1".into());
                        }
                    }
                }
            });
            let mut keep = d.params.get("keep").and_then(Value::as_bool).unwrap_or(false);
            if ui.checkbox(&mut keep, "Keep source formatting").changed()
                && let Some(o) = d.params.as_object_mut()
            {
                o.insert("keep".into(), json!(keep));
            }
            let picked: Vec<usize> = slides.iter().map(|(i, _)| *i).filter(|i| !off(d, *i)).collect();
            let (ok, cancel) = buttons(ui, "Insert");
            if ok {
                if picked.is_empty() {
                    app.set_status("Tick at least one slide to insert".to_string());
                    return false;
                }
                // A failed insert keeps the dialog (and the ticked slides) open; the error is in the status bar.
                let source = d.params.get("source").cloned().unwrap_or_default();
                return app.reuse_slides(&source, keep, &picked);
            }
            cancel
        }
        "renameSection" | "renameShape" | "renameLayout" => {
            let cur = d.params.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            field(ui, d, "Name:", "name", &cur);
            let (ok, cancel) = buttons(ui, "Rename");
            if ok {
                let name = d.get("name", "");
                match d.id.as_str() {
                    "renameSection" => run(app, "section.rename", json!({"index": d.params.get("index").cloned().unwrap_or_default(), "name": name})),
                    "renameShape" => run(app, "shape.rename", json!({"id": d.params.get("id").cloned().unwrap_or_default(), "name": name})),
                    _ => run(app, "master.renameLayout", json!({"name": name})),
                }
            }
            ok || cancel
        }
        "font" => font_dialog(app, ui, d),
        "paragraph" => paragraph_dialog(app, ui, d),
        "tabs" => tabs_dialog(app, ui, d),
        "export" => {
            ui.label("Export the presentation as:");
            let fmt = d.get("fmt", "pdf");
            for (l, f) in [
                ("PDF document (.pdf)", "pdf"),
                ("PNG images (current slide)", "png"),
                ("PNG images (all slides)", "pngAll"),
                ("JPEG image (current slide)", "jpeg"),
                ("PowerPoint Presentation (.pptx)", "pptx"),
                ("Outline (.txt)", "outline"),
                ("Animated GIF (.gif)", "gif"),
                ("Outline/RTF (.rtf)", "rtf"),
            ] {
                if ui.radio(fmt == f, l).clicked() {
                    d.fields.insert("fmt".into(), f.into());
                }
            }
            let pdf_layout = d.get("pdfLayout", "slides");
            if fmt == "pdf" {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let field = ui.label("Print layout:");
                    let combo = egui::ComboBox::from_id_salt("pdf_layout")
                        .selected_text(match pdf_layout.as_str() {
                            "notes" => "Notes Pages",
                            "h1" => "Handouts (1 slide per page)",
                            "h2" => "Handouts (2 slides per page)",
                            "h3" => "Handouts (3 slides per page)",
                            "h4" => "Handouts (4 slides per page)",
                            "h6" => "Handouts (6 slides per page)",
                            "h9" => "Handouts (9 slides per page)",
                            _ => "Full Page Slides",
                        })
                        .show_ui(ui, |ui| {
                            for (k, l) in [
                                ("slides", "Full Page Slides"),
                                ("notes", "Notes Pages"),
                                ("h1", "Handouts (1 slide per page)"),
                                ("h2", "Handouts (2 slides per page)"),
                                ("h3", "Handouts (3 slides per page)"),
                                ("h4", "Handouts (4 slides per page)"),
                                ("h6", "Handouts (6 slides per page)"),
                                ("h9", "Handouts (9 slides per page)"),
                            ] {
                                if ui.selectable_label(pdf_layout == k, l).clicked() {
                                    d.fields.insert("pdfLayout".into(), k.into());
                                }
                            }
                        });
                    let _ = combo.response.labelled_by(field.id);
                });
                let mut hidden = d.get("pdfHidden", "false") == "true";
                if ui.checkbox(&mut hidden, "Include hidden slides").changed() {
                    d.fields.insert("pdfHidden".into(), hidden.to_string());
                }
            }
            let png = matches!(fmt.as_str(), "png" | "pngAll");
            if png {
                ui.add_space(4.0);
                let mut clear = d.get("pngTransparent", "false") == "true";
                if ui
                    .checkbox(&mut clear, "Transparent background")
                    .on_hover_text("Leave the slide background out: only what is on the slide is drawn")
                    .changed()
                {
                    d.fields.insert("pngTransparent".into(), clear.to_string());
                }
            }
            let gif_params = if fmt == "gif" { gif_options(app, ui, d) } else { Ok(Value::Null) };
            if let Err(e) = &gif_params {
                ui.label(egui::RichText::new(e).color(Tokens::get(ui.ctx()).text_dim));
            }
            let pdf_params = {
                let (layout, per) = match pdf_layout.as_str() {
                    "notes" => ("notes", 0),
                    l if l.starts_with('h') => ("handouts", l[1..].parse().unwrap_or(6)),
                    _ => ("slides", 0),
                };
                json!({"layout": layout, "perPage": per, "includeHidden": d.get("pdfHidden", "false") == "true"})
            };
            let (ok, cancel) = buttons(ui, "Export");
            if ok {
                let gif_params = match gif_params {
                    Ok(v) => v,
                    // Keep the dialog open so the range or width can be corrected.
                    Err(e) => {
                        app.set_status(e);
                        return false;
                    }
                };
                let extra = match fmt.as_str() {
                    "pdf" => pdf_params,
                    "gif" => gif_params,
                    _ if png && d.get("pngTransparent", "false") == "true" => json!({"transparentBackground": true}),
                    _ => json!({}),
                };
                app.export_as(&fmt, extra);
            }
            ok || cancel
        }
        "replaceFonts" => {
            let used: Vec<String> =
                app.session.execute("font.used", &json!({})).ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
            let mut from = d.get("from", used.first().map(String::as_str).unwrap_or(""));
            let mut to = d.get("to", "");
            egui::Grid::new("replace_fonts").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                let field = ui.label("Replace:");
                let combo = egui::ComboBox::from_id_salt("rf_from").width(240.0).selected_text(from.clone()).show_ui(ui, |ui| {
                    for f in &used {
                        ui.selectable_value(&mut from, f.clone(), f);
                    }
                });
                let _ = combo.response.labelled_by(field.id);
                ui.end_row();
                ui.label("With:");
                ui.vertical(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut to).desired_width(240.0).hint_text("Font name"));
                    let needle = to.trim().to_lowercase();
                    egui::ScrollArea::vertical().max_height(140.0).id_salt("rf_list").show(ui, |ui| {
                        for f in
                            crate::ribbon::font_families(app).iter().filter(|f| needle.is_empty() || f.to_lowercase().contains(&needle)).take(200)
                        {
                            if ui.selectable_label(f.eq_ignore_ascii_case(to.trim()), f).clicked() {
                                to = f.clone();
                            }
                        }
                    });
                });
                ui.end_row();
            });
            if let Some(msg) = d.fields.get("result") {
                ui.label(egui::RichText::new(msg.clone()).color(Tokens::get(ui.ctx()).text_dim));
            }
            d.fields.insert("from".into(), from.clone());
            d.fields.insert("to".into(), to.clone());
            let (ok, cancel) = buttons(ui, "Replace");
            if ok && !from.is_empty() && !to.trim().is_empty() {
                let n = app.run("font.replace", json!({"from": from, "to": to.trim()})).ok().and_then(|v| v["replaced"].as_u64()).unwrap_or(0);
                let msg =
                    if n == 0 { format!("“{from}” isn't used explicitly.") } else { format!("Replaced “{from}” with “{}”.", to.trim()) };
                d.fields.insert("result".into(), msg);
                d.fields.remove("from");
                // Stay open, like PowerPoint, so several fonts can be replaced in a row.
                return cancel;
            }
            cancel
        }
        "symbol" => {
            ui.label("Click a symbol to insert it at the insertion point.");
            let groups: [(&str, &str); 6] = [
                ("Punctuation", "–—‘’“”…•·§¶†‡‰′″‹›«»¡¿"),
                ("Currency", "$€£¥¢₹₩₽₺₿"),
                ("Math", "±×÷≠≈≤≥∞√∑∏∫∂∆∇∈∉∩∪⊂⊃∀∃¬∧∨⇒⇔°′"),
                ("Greek", "αβγδεζηθικλμνξοπρστυφχψωΓΔΘΛΞΠΣΦΨΩ"),
                ("Arrows", "←↑→↓↔↕⇐⇑⇒⇓↖↗↘↙⟵⟶"),
                ("Shapes", "■□▪▫▲△▶▷▼▽◀◁◆◇○●◎★☆✓✔✗✘♠♣♥♦"),
            ];
            for (g, chars) in groups {
                ui.label(egui::RichText::new(g).font(theme::bold(12.0)));
                ui.horizontal_wrapped(|ui| {
                    for c in chars.chars() {
                        if ui.add(egui::Button::new(egui::RichText::new(c.to_string()).size(16.0)).min_size(vec2(26.0, 26.0))).clicked() {
                            if app.session.active().is_some_and(|s| s.selection.text.is_none()) {
                                let _ = app.run("text.edit", json!({}));
                            }
                            let _ = app.run("insert.symbol", json!({"text": c.to_string()}));
                        }
                    }
                });
            }
            let (ok, cancel) = buttons(ui, "Close");
            ok || cancel
        }
        "equation" => {
            ui.label("Type an equation in linear form (e.g. a^2+b^2=c^2, x=(-b±√(b^2-4ac))/2a).");
            let mut v = d.get("eq", "");
            ui.add(egui::TextEdit::singleline(&mut v).hint_text("Equation").desired_width(340.0).font(theme::font(15.0)));
            d.fields.insert("eq".into(), v.clone());
            let (ok, cancel) = buttons(ui, "Insert");
            if ok && !v.is_empty() {
                let pretty = pretty_equation(&v);
                let size = app.session.active().map(|s| s.doc.slide_size).unwrap_or(deckcraft_model::defaults::WIDE);
                run(app, "insert.textBox", json!({"rect": [size.width / 2.0 - 150.0, size.height / 2.0 - 25.0, 300, 50], "text": pretty}));
                run(app, "text.exit", json!({}));
                run(app, "format.font", json!({"family": "Liberation Serif"}));
                run(app, "format.italic", json!({"on": true}));
                run(app, "format.size", json!({"size": 28}));
            }
            ok || cancel
        }
        "pasteSpecial" => {
            const OPTS: [(&str, &str); 4] = [
                ("destinationTheme", "Use Destination Theme"),
                ("keepSource", "Keep Source Formatting"),
                ("picture", "Picture (PNG)"),
                ("text", "Keep Text Only"),
            ];
            let has_shapes = !app.session.clipboard.shapes.is_empty();
            let mut cur = d.get("as", "destinationTheme");
            ui.label("As:");
            for (id, label) in OPTS {
                let ok = has_shapes || matches!(id, "destinationTheme" | "text");
                ui.add_enabled_ui(ok, |ui| ui.radio_value(&mut cur, id.to_string(), label));
            }
            d.fields.insert("as".into(), cur.clone());
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                run(app, "edit.pasteSpecial", json!({"as": cur}));
            }
            ok || cancel
        }
        "find" => find_dialog(app, ui, d),
        "spelling" => {
            // Words skipped with Ignore All while the dialog is open, one per line.
            let ignored = d.get("ignore", "");
            let ignore: Vec<&str> = ignored.lines().filter(|l| !l.is_empty()).collect();
            let v = app.session.execute("review.spelling", &json!({"ignore": ignore})).unwrap_or_default();
            let list = v.as_array().cloned().unwrap_or_default();
            let info = app.session.execute("review.dictionary", &json!({})).unwrap_or_default();
            let available = info.get("available").and_then(Value::as_bool).unwrap_or(false);
            match info.get("found").and_then(Value::as_str) {
                Some(f) if available => {
                    ui.label(egui::RichText::new(format!("Dictionary: {}", f.rsplit(['/', '\\']).next().unwrap_or(f))).small().weak())
                }
                _ if available => {
                    ui.label(egui::RichText::new(format!("Dictionary: {}", info.get("lang").and_then(Value::as_str).unwrap_or(""))).small().weak())
                }
                _ => ui.label(
                    egui::RichText::new(format!(
                        "No {} dictionary is installed, so only common misspellings are found. Put a Hunspell .aff/.dic pair in {}.",
                        info.get("lang").and_then(Value::as_str).unwrap_or("en-US"),
                        info.get("folders")
                            .and_then(Value::as_array)
                            .and_then(|a| a.get(1).or(a.first()))
                            .and_then(Value::as_str)
                            .unwrap_or("a dictionaries folder")
                    ))
                    .small()
                    .weak(),
                ),
            };
            if list.is_empty() {
                ui.label("The spelling check is complete. No issues found.");
            }
            let mut add_ignore = None;
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for item in &list {
                    let w = item.get("word").and_then(Value::as_str).unwrap_or("");
                    let slide = item.get("slide").cloned().unwrap_or_default();
                    let shape = item.get("shape").cloned().unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label(format!("Slide {}: “{w}”", slide.as_u64().unwrap_or(0) + 1));
                        if ui.small_button("Go to").clicked() {
                            let _ = app.run("slide.go", json!({"index": slide}));
                            let _ = app.run("edit.select", json!({"ids": [shape]}));
                        }
                        if ui.small_button("Ignore All").clicked() {
                            add_ignore = Some(w.to_string());
                        }
                        if ui.small_button("Add to Dictionary").clicked()
                            && let Err(e) = app.session.execute("review.addToDictionary", &json!({"word": w}))
                        {
                            app.set_status(e.to_string());
                        }
                    });
                    let sug: Vec<String> = item
                        .get("suggestions")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                        .unwrap_or_default();
                    if item.get("repeated").and_then(Value::as_bool).unwrap_or(false) && !item.get("notes").and_then(Value::as_bool).unwrap_or(false)
                    {
                        ui.horizontal(|ui| {
                            ui.add_space(16.0);
                            ui.label(egui::RichText::new("Repeated word").weak());
                            if ui.small_button("Delete Repeated Word").clicked() {
                                let from = item.get("deleteFrom").and_then(Value::as_u64).unwrap_or(0) as usize;
                                let end = item.get("end").and_then(Value::as_u64).unwrap_or(0) as usize;
                                let para = item.get("para").and_then(Value::as_u64).unwrap_or(0) as usize;
                                let text = app.session.active().and_then(|st| {
                                    let sh = st.shape(deckcraft_model::ShapeId(shape.as_u64()? as u32))?;
                                    let pp = match (item.get("row").and_then(Value::as_u64), item.get("col").and_then(Value::as_u64), &sh.kind) {
                                        (Some(r), Some(c), deckcraft_model::ShapeKind::Table(tb)) => {
                                            tb.cell(r as usize, c as usize)?.text.paragraphs.get(para)?.text()
                                        }
                                        _ => sh.text.as_ref()?.paragraphs.get(para)?.text(),
                                    };
                                    Some(pp.chars().skip(from).take(end.saturating_sub(from)).collect::<String>())
                                });
                                if let Some(gap) = text {
                                    let mut p = json!({"shape": shape, "para": para, "start": from, "word": gap, "to": ""});
                                    if let (Some(o), Some(r), Some(c)) = (p.as_object_mut(), item.get("row"), item.get("col")) {
                                        o.insert("row".into(), r.clone());
                                        o.insert("col".into(), c.clone());
                                    }
                                    run(app, "review.spellingChange", p);
                                }
                            }
                        });
                    }
                    if !sug.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(16.0);
                            ui.label(egui::RichText::new("Change to:").weak());
                            for to in &sug {
                                let r = ui.small_button(to.as_str()).on_hover_text("Click to change this one; Shift-click to change all");
                                if r.clicked() {
                                    let all = ui.input(|i| i.modifiers.shift);
                                    if item.get("notes").and_then(Value::as_bool).unwrap_or(false) {
                                        // A word in the notes: change that line of the slide's notes.
                                        let si = slide.as_u64().unwrap_or(0) as usize;
                                        let notes =
                                            app.session.active().and_then(|d| d.doc.slides.get(si)).map(|s| s.notes_text()).unwrap_or_default();
                                        let (pi, st) = (
                                            item.get("para").and_then(Value::as_u64).unwrap_or(0) as usize,
                                            item.get("start").and_then(Value::as_u64).unwrap_or(0) as usize,
                                        );
                                        let lines: Vec<String> = notes
                                            .split('\n')
                                            .enumerate()
                                            .map(|(i, l)| {
                                                if i != pi {
                                                    return l.to_string();
                                                }
                                                let c: Vec<char> = l.chars().collect();
                                                let n = w.chars().count();
                                                if c.get(st..st + n).is_some_and(|x| x.iter().collect::<String>() == w) {
                                                    c[..st].iter().chain(to.chars().collect::<Vec<_>>().iter()).chain(c[st + n..].iter()).collect()
                                                } else {
                                                    l.to_string()
                                                }
                                            })
                                            .collect();
                                        run(app, "slide.notes", json!({"text": lines.join("\n"), "index": si}));
                                    } else {
                                        let mut p = if all {
                                            json!({"word": w, "to": to, "all": true})
                                        } else {
                                            json!({"shape": shape, "word": w, "to": to, "para": item.get("para"), "start": item.get("start")})
                                        };
                                        if !all && let (Some(o), Some(r), Some(c)) = (p.as_object_mut(), item.get("row"), item.get("col")) {
                                            o.insert("row".into(), r.clone());
                                            o.insert("col".into(), c.clone());
                                        }
                                        run(app, "review.spellingChange", p);
                                    }
                                }
                            }
                        });
                    }
                }
            });
            if let Some(w) = add_ignore {
                d.fields.insert("ignore".into(), format!("{ignored}{w}\n"));
            }
            let (ok, cancel) = buttons(ui, "Close");
            ok || cancel
        }
        "accessibility" => {
            let v = app.session.execute("review.accessibility", &json!({})).unwrap_or_default();
            let list = v.as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                ui.label("No accessibility issues found. People with disabilities should not have difficulty reading this document.");
            }
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for item in &list {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "Slide {}: {}{}",
                            item.get("slide").and_then(Value::as_u64).unwrap_or(0) + 1,
                            item.get("issue").and_then(Value::as_str).unwrap_or(""),
                            item.get("name").and_then(Value::as_str).map(|n| format!(" — {n}")).unwrap_or_default()
                        ));
                        if ui.small_button("Go to").clicked() {
                            let _ = app.run("slide.go", json!({"index": item.get("slide").cloned().unwrap_or_default()}));
                            if let Some(id) = item.get("shape") {
                                let _ = app.run("edit.select", json!({"ids": [id]}));
                            }
                        }
                    });
                }
            });
            let (ok, cancel) = buttons(ui, "Close");
            ok || cancel
        }
        "language" => {
            // Starts from the selected text's language and Do Not Check Spelling.
            let (cur, np) = selected_language(app);
            let lang = d.get("lang", &cur);
            let mut no_proof = d.get("noProof", if np { "1" } else { "" }) == "1";
            // The selected text, or every text in the presentation (all there is with nothing selected).
            let has_sel = deckcraft_engine::cmd::has_text_or_shapes(&app.session).is_ok();
            let mut all = !has_sel || d.get("scope", "") == "all";
            ui.horizontal(|ui| {
                ui.label("Apply to:");
                if ui.add_enabled(has_sel, egui::RadioButton::new(!all, "Selected text")).clicked() {
                    d.fields.insert("scope".into(), "sel".into());
                    all = false;
                }
                if ui.radio(all, "Whole presentation").clicked() {
                    d.fields.insert("scope".into(), "all".into());
                    all = true;
                }
            });
            ui.label(if all { "Mark all text in the presentation as:" } else { "Mark selected text as:" });
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                // The selection's own language first when the list doesn't have it.
                let own = (!crate::squiggles::LANGS.iter().any(|(_, t)| t.eq_ignore_ascii_case(&cur))).then(|| (cur.clone(), cur.clone()));
                let all = own.into_iter().chain(crate::squiggles::LANGS.iter().map(|(n, t)| (n.to_string(), t.to_string())));
                for (_native, tag) in all {
                    // Native names are shown when the interface fonts can draw them; otherwise
                    // the tag is the explicit fallback (egui 0.37 aborts on a missing glyph).
                    let name = crate::squiggles::display_lang_name(ui.ctx(), &tag);
                    let installed = deckcraft_engine::spell::dictionary(&tag).is_some();
                    let label = if installed { format!("{name}  (dictionary)") } else { name };
                    if ui.selectable_label(lang.eq_ignore_ascii_case(&tag), label).clicked() {
                        d.fields.insert("lang".into(), tag);
                    }
                }
            });
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Spelling is checked for languages marked (dictionary).").small().weak());
            if ui.checkbox(&mut no_proof, "Do not check spelling or grammar").changed() {
                d.fields.insert("noProof".into(), if no_proof { "1".into() } else { String::new() });
            }
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                let lang = d.get("lang", &cur);
                run(app, if all { "format.languageAll" } else { "format.language" }, json!({"lang": lang, "noProof": no_proof}));
            }
            ok || cancel
        }
        "customShow" => {
            let shows = app.session.active().map(|s| s.doc.custom_shows.clone()).unwrap_or_default();
            let mut started = false;
            for s in &shows {
                ui.horizontal(|ui| {
                    ui.label(format!("{} ({} slides)", s.name, s.slides.len()));
                    if ui.small_button("Show").clicked() {
                        run(app, "show.start", json!({"custom": s.name}));
                        started = true;
                    }
                    if ui.small_button("Delete").clicked() {
                        let _ = app.run("show.customShow", json!({"name": s.name, "delete": true}));
                    }
                });
            }
            if started {
                return true;
            }
            field(ui, d, "New show name:", "name", "Custom Show 1");
            field(ui, d, "Slides (e.g. 1,3,5):", "slides", "1");
            let (ok, cancel) = buttons(ui, "Create");
            if ok {
                let idx: Vec<usize> =
                    d.get("slides", "").split(',').filter_map(|s| s.trim().parse::<usize>().ok()).filter(|v| *v > 0).map(|v| v - 1).collect();
                run(app, "show.customShow", json!({"name": d.get("name", "Custom Show"), "slides": idx}));
            }
            ok || cancel
        }
        "chartData" => {
            let ch = app.session.active().and_then(|s| {
                s.selected_shapes()
                    .into_iter()
                    .find_map(|x| if let deckcraft_model::ShapeKind::Chart(c) = &x.kind { Some(((**c).clone(), x.id)) } else { None })
            });
            let Some((c, id)) = ch else {
                ui.label("Select a chart first.");
                let (ok, cancel) = buttons(ui, "Close");
                return ok || cancel;
            };
            ui.label("Edit the data: first row = series names, first column = categories.");
            let mut grid: Vec<Vec<String>> = d.params.get("grid").and_then(|g| serde_json::from_value(g.clone()).ok()).unwrap_or_else(|| {
                let mut g = vec![std::iter::once(String::new()).chain(c.series.iter().map(|s| s.name.clone())).collect::<Vec<_>>()];
                for (i, cat) in c.categories.iter().enumerate() {
                    let mut row = vec![cat.clone()];
                    for s in &c.series {
                        row.push(s.values.get(i).copied().flatten().map(|v| v.to_string()).unwrap_or_default());
                    }
                    g.push(row);
                }
                g
            });
            egui::Grid::new("chartdata").striped(true).show(ui, |ui| {
                for row in grid.iter_mut() {
                    for cell in row.iter_mut() {
                        ui.add(egui::TextEdit::singleline(cell).hint_text("Chart cell").desired_width(70.0));
                    }
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                if ui.button("+ Row").clicked() {
                    let n = grid.first().map(Vec::len).unwrap_or(2);
                    grid.push(vec![String::new(); n]);
                }
                if ui.button("+ Series").clicked() {
                    for r in grid.iter_mut() {
                        r.push(String::new());
                    }
                }
            });
            d.params = json!({"grid": grid});
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                let names: Vec<String> = grid.first().map(|r| r.iter().skip(1).cloned().collect()).unwrap_or_default();
                let cats: Vec<String> = grid.iter().skip(1).map(|r| r.first().cloned().unwrap_or_default()).collect();
                let series: Vec<Value> = names.iter().enumerate().map(|(k, n)| json!({"name": n, "values": grid.iter().skip(1).map(|r| r.get(k + 1).and_then(|v| v.trim().parse::<f64>().ok())).collect::<Vec<_>>()})).collect();
                run(app, "chart.data", json!({"id": id.0, "categories": cats, "series": series}));
            }
            ok || cancel
        }
        "trim" => {
            let seed = trim_seed(app.session.active());
            field(ui, d, "Start time (s):", "start", &seed.start);
            field(ui, d, "End trim (s):", "end", &seed.end);
            field(ui, d, "Fade in (s):", "fin", &seed.fade_in);
            field(ui, d, "Fade out (s):", "fout", &seed.fade_out);
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                run(
                    app,
                    "media.options",
                    json!({
                        "trimStart": seconds_text_to_ms(&d.get("start", &seed.start)),
                        "trimEnd": seconds_text_to_ms(&d.get("end", &seed.end)),
                        "fadeIn": seconds_text_to_ms(&d.get("fin", &seed.fade_in)),
                        "fadeOut": seconds_text_to_ms(&d.get("fout", &seed.fade_out)),
                    }),
                );
            }
            ok || cancel
        }
        "smartart" => {
            ui.label("Pick a graphic; type one item per line.");
            let kind = d.get("kind", "process");
            ui.horizontal_wrapped(|ui| {
                for (k, l) in [
                    ("list", "Basic List"),
                    ("process", "Basic Process"),
                    ("cycle", "Basic Cycle"),
                    ("hierarchy", "Hierarchy"),
                    ("pyramid", "Pyramid"),
                    ("matrix", "Matrix"),
                ] {
                    if ui.selectable_label(kind == k, l).clicked() {
                        d.fields.insert("kind".into(), k.into());
                    }
                }
            });
            let mut v = d.get("text", "Plan\nBuild\nLaunch");
            ui.add(egui::TextEdit::multiline(&mut v).hint_text("SmartArt items").desired_rows(5).desired_width(320.0));
            d.fields.insert("text".into(), v.clone());
            let (ok, cancel) = buttons(ui, "Insert");
            if ok {
                run(app, "insert.smartArt", json!({"kind": kind, "items": v.lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>()}));
                app.ui.pane = Some("smartart".into());
            }
            ok || cancel
        }
        "about" => {
            let tab = d.get("tab", "about");
            ui.horizontal(|ui| {
                for (k, l) in [("about", "About"), ("contributors", "Contributors"), ("models", "Models")] {
                    if ui.selectable_label(tab == k, l).clicked() {
                        d.fields.insert("tab".into(), k.into());
                    }
                }
            });
            ui.separator();
            match tab.as_str() {
                "contributors" | "models" => {
                    credits_block(ui, tab == "models");
                }
                _ => about(app, ui),
            }
            let (ok, cancel) = buttons(ui, "Close");
            ok || cancel
        }
        "autocorrect" => {
            let pr = &mut app.session.prefs;
            ui.checkbox(&mut pr.ac_two_initials, "Correct TWo INitial CApitals");
            ui.checkbox(&mut pr.ac_sentence_caps, "Capitalize first letter of sentences");
            ui.checkbox(&mut pr.ac_days, "Capitalize names of days");
            ui.checkbox(&mut pr.ac_caps_lock, "Correct accidental use of cAPS LOCK key");
            ui.checkbox(&mut pr.ac_fractions, "Replace fractions (1/2) with fraction characters (½)");
            ui.checkbox(&mut pr.ac_ordinals, "Ordinals (1st) with superscript");
            ui.checkbox(&mut pr.ac_lists, "Automatic bulleted and numbered lists");
            ui.checkbox(&mut pr.autocorrect, "Replace text as you type");
            ui.add_space(4.0);
            let mut from = d.get("from", "");
            let mut to = d.get("to", "");
            ui.horizontal(|ui| {
                let l = ui.label("Replace");
                ui.add(egui::TextEdit::singleline(&mut from).desired_width(90.0)).labelled_by(l.id);
                let l = ui.label("With");
                ui.add(egui::TextEdit::singleline(&mut to).desired_width(130.0)).labelled_by(l.id);
                if ui.add_enabled(!from.trim().is_empty(), egui::Button::new("Add")).clicked() {
                    run(app, "text.autocorrect", json!({"add": [from.trim(), to]}));
                    from.clear();
                    to.clear();
                }
            });
            d.fields.insert("from".into(), from.clone());
            d.fields.insert("to".into(), to);
            let list = app.session.execute("text.autocorrect", &json!({})).ok().and_then(|v| v.get("entries").cloned());
            let entries: Vec<(String, String)> = list.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
            let q = from.trim().to_lowercase();
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                egui::Grid::new("ac-list").striped(true).show(ui, |ui| {
                    for (k, v) in entries.iter().filter(|(k, _)| q.is_empty() || k.to_lowercase().starts_with(&q)) {
                        ui.label(k);
                        ui.label(v);
                        if ui.small_button("Delete").clicked() {
                            run(app, "text.autocorrect", json!({"remove": k}));
                        }
                        ui.end_row();
                    }
                });
            });
            let (ok, cancel) = buttons(ui, "OK");
            ok || cancel
        }
        "gridOptions" => grid_options(app, ui, d),
        "columns" => columns_dialog(app, ui, d),
        "preferences" => {
            // The controls apply as they change, so the dialog remembers what it opened with and
            // Cancel (or Esc, or the close button: see `show`) puts that back.
            if d.params.get("before").is_none() {
                d.params = json!({ "before": preferences_snapshot(app) });
            }
            ui.label(egui::RichText::new("General").font(theme::bold(13.0)));
            let mut dark = app.ui.brightness == theme::Brightness::Dark;
            if ui.checkbox(&mut dark, "Dark appearance").changed() {
                let _ = app.run("view.dark", json!({"on": dark}));
            }
            // While the pointer holds the slider its value only waits in the dialog: applying it
            // would rescale the slider under the pointer and run it to an end (#767). It applies
            // on release, and at once from the keyboard.
            let pending = d.fields.get("uiScale").and_then(|v| v.parse::<f32>().ok());
            let mut scale = pending.unwrap_or(app.ui.ui_scale);
            let r = ui.add(egui::Slider::new(&mut scale, 0.75..=1.75));
            // No visible label here (the row is laid out to fit); the accessible name only.
            let enabled = r.enabled();
            r.widget_info(|| egui::WidgetInfo::slider(enabled, f64::from(scale), "Interface size"));
            if r.is_pointer_button_down_on() {
                if r.changed() || pending.is_some() {
                    d.fields.insert("uiScale".into(), scale.to_string());
                }
            } else {
                if r.changed() || pending.is_some() {
                    app.ui.ui_scale = scale;
                }
                d.fields.remove("uiScale");
            }
            // PowerPoint takes the unit from the system's measurement setting; DeckCraft asks here.
            ui.horizontal(|ui| {
                ui.label("Measurement units:");
                let units = &mut app.session.prefs.units;
                ui.radio_value(units, deckcraft_geom::Unit::Inches, "Inches");
                ui.radio_value(units, deckcraft_geom::Unit::Centimeters, "Centimeters");
            });
            ui.label(egui::RichText::new("Editing").font(theme::bold(13.0)));
            ui.checkbox(&mut app.session.prefs.smart_guides, "Show smart guides");
            ui.checkbox(&mut app.session.prefs.snap_to_grid, "Snap objects to grid");
            ui.checkbox(&mut app.session.prefs.smart_quotes, "Replace straight quotes with smart quotes");
            ui.horizontal(|ui| {
                ui.checkbox(&mut app.session.prefs.autocorrect, "AutoCorrect as you type");
                if ui.button("AutoCorrect Options…").clicked() {
                    *d = Dialog::new("autocorrect");
                }
            });
            if d.id != "preferences" {
                return false;
            }
            ui.horizontal(|ui| {
                let label = ui.label("Your name (comments):");
                // Name the field for assistive technology; a bare text edit next to a label is an
                // unnamed input in egui 0.37's accessibility tree.
                let _ = ui.add(egui::TextEdit::singleline(&mut app.session.prefs.author)).labelled_by(label.id);
            });
            let (ok, cancel) = buttons(ui, "Done");
            if cancel {
                restore_preferences(app, d);
            }
            ok || cancel
        }
        "photoAlbum" => photo_album_dialog(app, ui, d),
        "themeColors" => theme_colors_dialog(app, ui, d),
        "themeFonts" => theme_fonts_dialog(app, ui, d),
        "dateTime" => date_time_dialog(app, ui, d),
        _ => {
            ui.label(format!("“{}” is not available yet.", d.id));
            let (ok, cancel) = buttons(ui, "OK");
            ok || cancel
        }
    }
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

fn put_dim(d: &mut Dialog, key: &str, exact: f64, shown: String) {
    d.fields.insert(key.into(), shown.clone());
    d.fields.insert(format!("seed{key}"), shown.clone());
    d.fields.insert(format!("prev{key}"), shown);
    d.fields.insert(format!("{key}Pt"), exact.to_string());
}

fn publish_dim(d: &mut Dialog, key: &str, exact: f64, shown: String) {
    d.fields.insert(key.into(), shown.clone());
    d.fields.insert(format!("seed{key}"), shown);
    d.fields.insert(format!("{key}Pt"), exact.to_string());
}

fn edited(d: &Dialog, key: &str) -> bool {
    field_of(d, key) != field_of(d, &format!("seed{key}"))
}

fn exact_of(d: &Dialog, key: &str) -> Option<f64> {
    parse_shown(&field_of(d, &format!("{key}Pt")))
}

fn selection_xfrm(app: &SlideApp) -> Option<deckcraft_geom::Xfrm> {
    let st = app.session.active()?;
    let id = *st.selection.shapes.first()?;
    let sh = st.shape(id)?;
    Some(deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh))
}

fn seed_size_position(app: &SlideApp, d: &mut Dialog) {
    if d.fields.contains_key("baseW") {
        return;
    }
    let num = |k: &str| d.params.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
    let sel = selection_xfrm(app);
    let x = num("x").or_else(|| sel.map(|b| b.x)).unwrap_or(0.0);
    let y = num("y").or_else(|| sel.map(|b| b.y)).unwrap_or(0.0);
    let w = num("w").or_else(|| sel.map(|b| b.w)).unwrap_or(72.0);
    let h = num("h").or_else(|| sel.map(|b| b.h)).unwrap_or(72.0);
    let rot = num("rot").or_else(|| sel.map(|b| b.rot)).unwrap_or(0.0);
    let sw = num("scaleW").unwrap_or(100.0);
    let sh = num("scaleH").unwrap_or(100.0);
    let lock = d.params.get("lockAspect").and_then(Value::as_bool).unwrap_or(false);
    put_dim(d, "x", x, num_text(x / 72.0));
    put_dim(d, "y", y, num_text(y / 72.0));
    put_dim(d, "w", w, num_text(w / 72.0));
    put_dim(d, "h", h, num_text(h / 72.0));
    put_dim(d, "rot", rot, num_text(rot));
    put_dim(d, "scaleW", sw, num_text(sw));
    put_dim(d, "scaleH", sh, num_text(sh));
    d.fields.insert("baseW".into(), w.to_string());
    d.fields.insert("baseH".into(), h.to_string());
    let bit = if lock { "1" } else { "0" };
    d.fields.insert("lock".into(), bit.into());
    d.fields.insert("prevlock".into(), bit.into());
}

fn scale_of(pt: f64, base: f64) -> f64 {
    if base > 0.0 { pt / base * 100.0 } else { 100.0 }
}

fn current_points(d: &Dialog, key: &str) -> Option<f64> {
    if edited(d, key) { parse_shown(&field_of(d, key)).map(|inches| inches * 72.0) } else { exact_of(d, key) }
}

/// Keep scale tied to size, and the other side tied when aspect is locked.
fn reconcile_size_fields(d: &mut Dialog) {
    let lock = field_of(d, "lock") == "1";
    let was_locked = field_of(d, "prevlock") == "1";
    let base_w = parse_shown(&field_of(d, "baseW")).unwrap_or(1.0);
    let base_h = parse_shown(&field_of(d, "baseH")).unwrap_or(1.0);
    let ar = if base_h > 0.0 { base_w / base_h } else { 1.0 };
    let (w_s, h_s, sw_s, sh_s) = (field_of(d, "w"), field_of(d, "h"), field_of(d, "scaleW"), field_of(d, "scaleH"));
    let (prev_w, prev_h, prev_sw, prev_sh) = (field_of(d, "prevw"), field_of(d, "prevh"), field_of(d, "prevscaleW"), field_of(d, "prevscaleH"));
    if w_s != prev_w {
        if let Some(inches) = parse_shown(&w_s) {
            let w_pt = inches * 72.0;
            d.fields.insert("wPt".into(), w_pt.to_string());
            let pct = scale_of(w_pt, base_w);
            publish_dim(d, "scaleW", pct, num_text(pct));
            if lock {
                let h_pt = if ar > 0.0 { w_pt / ar } else { w_pt };
                publish_dim(d, "h", h_pt, num_text(h_pt / 72.0));
                let hp = scale_of(h_pt, base_h);
                publish_dim(d, "scaleH", hp, num_text(hp));
            }
        }
    } else if h_s != prev_h {
        if let Some(inches) = parse_shown(&h_s) {
            let h_pt = inches * 72.0;
            d.fields.insert("hPt".into(), h_pt.to_string());
            let pct = scale_of(h_pt, base_h);
            publish_dim(d, "scaleH", pct, num_text(pct));
            if lock {
                let w_pt = h_pt * ar;
                publish_dim(d, "w", w_pt, num_text(w_pt / 72.0));
                let wp = scale_of(w_pt, base_w);
                publish_dim(d, "scaleW", wp, num_text(wp));
            }
        }
    } else if sw_s != prev_sw {
        if let Some(pct) = parse_shown(&sw_s) {
            let w_pt = base_w * pct / 100.0;
            d.fields.insert("scaleWPt".into(), pct.to_string());
            publish_dim(d, "w", w_pt, num_text(w_pt / 72.0));
            if lock {
                let h_pt = if ar > 0.0 { w_pt / ar } else { w_pt };
                publish_dim(d, "h", h_pt, num_text(h_pt / 72.0));
                publish_dim(d, "scaleH", pct, num_text(pct));
            }
        }
    } else if sh_s != prev_sh {
        if let Some(pct) = parse_shown(&sh_s) {
            let h_pt = base_h * pct / 100.0;
            d.fields.insert("scaleHPt".into(), pct.to_string());
            publish_dim(d, "h", h_pt, num_text(h_pt / 72.0));
            if lock {
                let w_pt = h_pt * ar;
                publish_dim(d, "w", w_pt, num_text(w_pt / 72.0));
                publish_dim(d, "scaleW", pct, num_text(pct));
            }
        }
    } else if lock
        && !was_locked
        && let Some(w_pt) = current_points(d, "w")
    {
        let h_pt = if ar > 0.0 { w_pt / ar } else { w_pt };
        publish_dim(d, "h", h_pt, num_text(h_pt / 72.0));
        let hp = scale_of(h_pt, base_h);
        publish_dim(d, "scaleH", hp, num_text(hp));
        let wp = scale_of(w_pt, base_w);
        publish_dim(d, "scaleW", wp, num_text(wp));
    }
    for key in ["x", "y", "w", "h", "rot", "scaleW", "scaleH"] {
        let v = field_of(d, key);
        d.fields.insert(format!("prev{key}"), v);
    }
    d.fields.insert("prevlock".into(), if lock { "1".into() } else { "0".into() });
}

fn shown_points(d: &Dialog, key: &str, label: &str) -> Result<f64, String> {
    if edited(d, key) {
        let n = parse_shown(&field_of(d, key)).ok_or_else(|| format!("{label} must be a number"))?;
        return Ok(n * 72.0);
    }
    exact_of(d, key).ok_or_else(|| format!("{label} must be a number"))
}

fn shown_number(d: &Dialog, key: &str, label: &str) -> Result<f64, String> {
    if edited(d, key) {
        return parse_shown(&field_of(d, key)).ok_or_else(|| format!("{label} must be a number"));
    }
    exact_of(d, key).ok_or_else(|| format!("{label} must be a number"))
}

fn size_position_value(d: &Dialog) -> Result<Value, String> {
    let x = shown_points(d, "x", "Horizontal")?;
    let y = shown_points(d, "y", "Vertical")?;
    let rot = shown_number(d, "rot", "Rotation")?;
    let base_w = parse_shown(&field_of(d, "baseW")).unwrap_or(1.0);
    let base_h = parse_shown(&field_of(d, "baseH")).unwrap_or(1.0);
    let mut w = if edited(d, "w") {
        shown_points(d, "w", "Width")?
    } else if edited(d, "scaleW") {
        base_w * shown_number(d, "scaleW", "Scale width")? / 100.0
    } else {
        shown_points(d, "w", "Width")?
    };
    let mut h = if edited(d, "h") {
        shown_points(d, "h", "Height")?
    } else if edited(d, "scaleH") {
        base_h * shown_number(d, "scaleH", "Scale height")? / 100.0
    } else {
        shown_points(d, "h", "Height")?
    };
    if field_of(d, "lock") == "1" {
        let ar = if base_h > 0.0 { base_w / base_h } else { 1.0 };
        let width_drives = edited(d, "w") || edited(d, "scaleW") || (!edited(d, "h") && !edited(d, "scaleH"));
        if width_drives {
            h = if ar > 0.0 { w / ar } else { h };
        } else {
            w = h * ar;
        }
    }
    Ok(json!({"x": x, "y": y, "w": w, "h": h, "rot": rot, "lockAspect": field_of(d, "lock") == "1"}))
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

fn size_position_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    seed_size_position(app, d);
    ui.label(egui::RichText::new("Size").strong());
    text_row(ui, d, "Height (in):", "h");
    text_row(ui, d, "Width (in):", "w");
    text_row(ui, d, "Rotation (°):", "rot");
    text_row(ui, d, "Scale height (%):", "scaleH");
    text_row(ui, d, "Scale width (%):", "scaleW");
    check(ui, d, "Lock aspect ratio", "lock", false);
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Position").strong());
    ui.label("From the top-left corner of the slide");
    text_row(ui, d, "Horizontal (in):", "x");
    text_row(ui, d, "Vertical (in):", "y");
    reconcile_size_fields(d);
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        match size_position_value(d) {
            Ok(p) => return app.run("shape.sizePosition", p).is_ok(),
            Err(msg) => {
                app.set_status(msg);
                return false;
            }
        }
    }
    cancel
}

const DATE_LANGS: &[(&str, &str)] = &[
    ("en-US", "English (United States)"),
    ("en-GB", "English (United Kingdom)"),
    ("fr-FR", "French (France)"),
    ("ja-JP", "Japanese"),
    ("sv-SE", "Swedish"),
];

use deckcraft_text::datetime::DATE_KINDS;

/// Samples `field_text` can compute for `lang`. Named months and AM/PM exist for English only;
/// other languages keep the numeric date and the 24-hour times.
fn date_rows(lang: &str) -> Vec<(&'static str, String)> {
    let now = deckcraft_text::datetime::DateTime::now_local();
    let mut rows = Vec::new();
    for kind in DATE_KINDS {
        if let Some(now) = now
            && let Some(sample) = deckcraft_text::datetime::field_text(kind, Some(lang), now)
        {
            rows.push((kind, sample));
        }
    }
    if rows.is_empty() {
        rows.push(("datetime1", "datetime1".into()));
    }
    rows
}

/// Render language and format controls; OK applies `insert.dateTime`, and the return value says
/// whether the dialog should close.
fn date_time_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    ui.set_min_width(440.0);
    ui.label("Available formats:");
    let lang = d.get("language", "en-US");
    let rows = date_rows(&lang);
    if !rows.iter().any(|(k, _)| *k == d.get("format", "datetime1"))
        && let Some((k, _)) = rows.first()
    {
        d.fields.insert("format".into(), (*k).to_string());
    }
    let format = d.get("format", "datetime1");
    egui::ScrollArea::vertical().max_height(280.0).min_scrolled_height(220.0).auto_shrink([false, false]).show(ui, |ui| {
        for (kind, sample) in &rows {
            if ui.selectable_label(format == *kind, sample).clicked() {
                d.fields.insert("format".into(), (*kind).to_string());
            }
        }
    });
    let lang_label = DATE_LANGS.iter().find(|(k, _)| *k == lang).map(|(_, l)| *l).unwrap_or("English (United States)");
    ui.horizontal(|ui| {
        ui.label("Language:");
        let opened = egui::ComboBox::from_id_salt("date-lang").selected_text(lang_label).width(240.0).show_ui(ui, |ui| {
            for (k, l) in DATE_LANGS {
                if ui.selectable_label(lang == *k, *l).clicked() {
                    d.fields.insert("language".into(), (*k).to_string());
                }
            }
        });
        let shown = lang_label.to_string();
        opened.response.widget_info(move || {
            let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Language");
            info.current_text_value = Some(shown.clone());
            info
        });
    });
    ui.label(
        egui::RichText::new("Month names and AM/PM are listed for English. Other languages list the numeric date and 24-hour time.")
            .size(11.0)
            .color(Tokens::get(ui.ctx()).text_dim),
    );
    let update = check(ui, d, "Update automatically", "update", true);
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        let lang = d.get("language", "en-US");
        let mut format = d.get("format", "datetime1");
        let probe = deckcraft_text::datetime::DateTime::new(2026, 10, 9, 20, 5, 7);
        if deckcraft_text::datetime::field_text(&format, Some(&lang), probe).is_none() {
            format = "datetime1".into();
        }
        return app.run("insert.dateTime", json!({"format": format, "update": update, "language": lang})).is_ok();
    }
    cancel
}

fn format_spacing(v: f64) -> String {
    if !v.is_finite() {
        return "6".into();
    }
    if (v - v.round()).abs() < 1e-6 && v.abs() < 1.0e6 { format!("{}", v.round() as i64) } else { format!("{v:.2}") }
}

/// Grid Options: spacing, snap and whether the grid and guides are drawn. OK applies one
/// `view.gridOptions` call; Cancel leaves the presentation as it was.
fn grid_options(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    ui.label(egui::RichText::new("Snap").font(theme::bold(13.0)));
    let snap = check(ui, d, "Snap objects to grid", "snap", app.session.prefs.snap_to_grid);
    let smart = check(ui, d, "Snap objects to other objects", "smart", app.session.prefs.smart_guides);
    let spacing_default = format_spacing(app.session.prefs.grid_spacing);
    field(ui, d, "Spacing (pt):", "spacing", &spacing_default);
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Display").font(theme::bold(13.0)));
    let gridlines = check(ui, d, "Display grid on screen", "gridlines", app.ui.gridlines);
    let guides = check(ui, d, "Display drawing guides on screen", "guides", app.ui.guides);
    if let Some(err) = d.fields.get("error").cloned().filter(|s| !s.is_empty()) {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(err).color(Color32::from_rgb(0xB0, 0x30, 0x30)));
    }
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        let spacing = num(d, "spacing", app.session.prefs.grid_spacing);
        match app.run("view.gridOptions", json!({"spacing": spacing, "snap": snap, "smartGuides": smart, "gridlines": gridlines, "guides": guides})) {
            Ok(_) => return true,
            Err(e) => {
                d.fields.insert("error".into(), e);
                return false;
            }
        }
    }
    cancel
}

/// The Columns dialog's changed fields as `format.columns` params: Number and Spacing (inches,
/// sent in points). A field left as the dialog showed it is left out, so each selected text box
/// keeps its own value; `None` when neither field changed.
fn columns_value(d: &Dialog, shown_count: &str, shown_spacing: &str) -> Result<Option<Value>, String> {
    let mut p = serde_json::Map::new();
    let count = field_of(d, "count");
    if count != shown_count {
        let n = count.trim().parse::<u32>().ok().filter(|n| (1..=16).contains(n)).ok_or("Number must be a whole number from 1 to 16")?;
        p.insert("count".into(), json!(n));
    }
    let spacing = field_of(d, "spacing");
    if spacing != shown_spacing {
        let inches = spacing
            .trim()
            .trim_end_matches("in")
            .trim_end_matches('"')
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && (0.0..=22.0).contains(v))
            .ok_or("Spacing must be from 0 to 22 inches")?;
        p.insert("spacing".into(), json!(inches * 72.0));
    }
    Ok((!p.is_empty()).then_some(Value::Object(p)))
}

/// Columns: the number of columns in the text box and the space between them. OK applies one
/// `format.columns` call with the fields that changed, or none when neither did; Cancel leaves
/// the text box as it was.
fn columns_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let count = d.params.get("count").and_then(Value::as_u64).unwrap_or(1).to_string();
    let spacing = num_text(d.params.get("spacing").and_then(Value::as_f64).unwrap_or(0.0) / 72.0);
    field(ui, d, "Number:", "count", &count);
    field(ui, d, "Spacing (in):", "spacing", &spacing);
    if let Some(err) = d.fields.get("error").cloned().filter(|s| !s.is_empty()) {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(err).color(Color32::from_rgb(0xB0, 0x30, 0x30)));
    }
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        let applied = columns_value(d, &count, &spacing).and_then(|p| match p {
            Some(p) => app.run("format.columns", p).map(|_| ()),
            None => Ok(()),
        });
        match applied {
            Ok(()) => return true,
            Err(e) => {
                d.fields.insert("error".into(), e);
                return false;
            }
        }
    }
    cancel
}

fn pretty_equation(s: &str) -> String {
    let sup = |c: char| match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        'n' => 'ⁿ',
        'i' => 'ⁱ',
        '+' => '⁺',
        '-' => '⁻',
        _ => c,
    };
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '^' => {
                while let Some(&n) = chars.peek() {
                    if n.is_ascii_alphanumeric() || n == '+' || n == '-' {
                        out.push(sup(n));
                        chars.next();
                        if !n.is_ascii_digit() {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            '*' => out.push('·'),
            _ => out.push(c),
        }
    }
    out.replace("sqrt", "√").replace("<=", "≤").replace(">=", "≥").replace("!=", "≠").replace("pi", "π").replace("+-", "±")
}

pub fn about(_app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.vertical_centered(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
        paint_logo(ui.painter(), r);
        ui.label(egui::RichText::new("DeckCraft").font(theme::bold(22.0)));
        ui.label(egui::RichText::new(version_line()).color(t.text_dim));
        ui.add_space(6.0);
        ui.label("Presentations and slide shows, rebuilt from scratch in pure Rust.");
        ui.label("An open-source, clean-room project. Not affiliated with Microsoft.");
        ui.add_space(6.0);
        ui.hyperlink_to("Join our community on Discord", deckcraft_engine::links::DISCORD);
        ui.hyperlink_to("getartcraft.com/apps/deckcraft", deckcraft_engine::links::APP_PAGE);
        ui.hyperlink_to("Source on GitHub", deckcraft_engine::links::GITHUB);
        ui.add_space(6.0);
        ui.label(egui::RichText::new("MIT OR Apache-2.0 · © 2026 ArtCraft Team and the DeckCraft contributors").size(11.0).color(t.text_faint));
    });
}

/// "Version X.Y.Z" plus, for release builds, the short commit and build date. The version comes
/// from `[workspace.package] version` (the single source of truth, `cargo xtask version`); the
/// release workflow sets `DECKCRAFT_BUILD_SHA` / `DECKCRAFT_BUILD_DATE` at build time.
pub fn version_line() -> String {
    let mut s = format!("Version {}", env!("CARGO_PKG_VERSION"));
    let sha = option_env!("DECKCRAFT_BUILD_SHA").map(|s| s.get(..9).unwrap_or(s)).filter(|s| !s.is_empty());
    let date = option_env!("DECKCRAFT_BUILD_DATE").filter(|s| !s.is_empty());
    match (sha, date) {
        (Some(sha), Some(date)) => s.push_str(&format!(" ({sha}, {date})")),
        (Some(x), None) | (None, Some(x)) => s.push_str(&format!(" ({x})")),
        (None, None) => {}
    }
    s
}

/// DeckCraft's mark, drawn in code: a slide card with a play triangle in our orange.
pub fn paint_logo(p: &egui::Painter, r: Rect) {
    let orange = Color32::from_rgb(0xF2, 0x6B, 0x1D);
    let deep = Color32::from_rgb(0xC2, 0x4E, 0x14);
    let card = r.shrink(r.width() * 0.08);
    p.rect_filled(card.translate(vec2(r.width() * 0.04, r.width() * 0.05)), CornerRadius::same((r.width() * 0.14) as u8), deep);
    p.rect_filled(card, CornerRadius::same((r.width() * 0.14) as u8), orange);
    let c = card.center();
    let s = card.width() * 0.22;
    p.add(egui::Shape::convex_polygon(
        vec![pos2(c.x - s * 0.7, c.y - s), pos2(c.x + s, c.y), pos2(c.x - s * 0.7, c.y + s)],
        Color32::WHITE,
        Stroke::NONE,
    ));
}

/// The selection's proofing language and whether it's marked Do Not Check Spelling: the run at
/// the caret, else the first run of the first selected shape; US English when none says.
pub fn selected_language(app: &SlideApp) -> (String, bool) {
    let Some(st) = app.session.active() else { return ("en-US".into(), false) };
    let run = |sh: &deckcraft_model::Shape, at: Option<(usize, usize)>| -> Option<deckcraft_model::RunProps> {
        let t = sh.text.as_ref()?;
        if let Some((p, c)) = at {
            let para = t.paragraphs.get(p)?;
            let mut n = 0;
            for r in &para.runs {
                n += r.char_len();
                if c <= n {
                    return Some(r.props.clone());
                }
            }
            return para.runs.last().map(|r| r.props.clone());
        }
        t.paragraphs.iter().flat_map(|p| &p.runs).next().map(|r| r.props.clone())
    };
    let props = match &st.selection.text {
        Some(t) if !t.notes && t.cell.is_none() => st.shape(t.shape).and_then(|sh| run(sh, Some(t.caret))),
        _ => st.selection.shapes.first().and_then(|id| st.shape(*id)).and_then(|sh| run(sh, None)),
    };
    let props = props.unwrap_or_default();
    (props.lang.unwrap_or_else(|| "en-US".into()), props.no_proof == Some(true))
}

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

fn seed_font_fields(d: &mut Dialog) {
    seed_once(d, "family", json_text(d.params.get("family"), ""));
    seed_once(d, "size", json_text(d.params.get("size"), "18"));
    seed_once(d, "bold", json_text(d.params.get("bold"), "0"));
    seed_once(d, "italic", json_text(d.params.get("italic"), "0"));
    seed_once(d, "underline", json_text(d.params.get("underline"), "none"));
    seed_once(d, "strike", json_text(d.params.get("strike"), "none"));
    seed_once(d, "baseline", json_text(d.params.get("baseline"), "none"));
    seed_once(d, "caps", json_text(d.params.get("caps"), "none"));
    seed_once(d, "spacing", json_text(d.params.get("spacing"), "0"));
    let shown = d.params.get("color").map(color_label).unwrap_or_default();
    seed_once(d, "color", shown.clone());
    seed_once(d, "colorSeed", shown);
}

/// Draw a uniquely identified accessible choice combo box and store the selected key;
/// the caller supplies the visible label.
fn choice(ui: &mut Ui, d: &mut Dialog, salt: &str, name: &str, key: &str, choices: &[(&str, &str)]) {
    let cur = d.get(key, "");
    let label = choices.iter().find(|(v, _)| *v == cur).map(|(_, l)| *l).unwrap_or(cur.as_str());
    let combo = egui::ComboBox::from_id_salt(salt).selected_text(label).width(140.0).show_ui(ui, |ui| {
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

/// Home › Font › Font…, the shape menu and Cmd+T. One OK applies every field together.
fn font_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    ui.set_min_width(460.0);
    seed_font_fields(d);
    let mut fams = crate::ribbon::font_families(app);
    let family = d.get("family", "");
    if !family.is_empty() && !fams.iter().any(|f| f == &family) {
        fams.insert(0, family.clone());
    }
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(88.0, 20.0), egui::Label::new("Font"));
        let combo = egui::ComboBox::from_id_salt("font_dialog_family")
            .selected_text(if family.is_empty() { " " } else { family.as_str() })
            .width(300.0)
            .show_ui(ui, |ui| {
                egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                    for f in &fams {
                        if ui.selectable_label(family == *f, f).clicked() {
                            d.fields.insert("family".into(), f.clone());
                        }
                    }
                });
            });
        let _ = combo.response.labelled_by(field.id);
    });
    ui.horizontal(|ui| {
        let _ = labeled_text(ui, d, "Size", "size", 64.0);
        ui.add_space(16.0);
        check(ui, d, "Bold", "bold", false);
        check(ui, d, "Italic", "italic", false);
    });
    ui.horizontal(|ui| {
        ui.add_sized(vec2(88.0, 20.0), egui::Label::new("Underline"));
        choice(
            ui,
            d,
            "font_dialog_underline",
            "Underline",
            "underline",
            &[("none", "None"), ("sng", "Single"), ("dbl", "Double"), ("heavy", "Heavy"), ("dotted", "Dotted"), ("dash", "Dash"), ("wavy", "Wavy")],
        );
        ui.add_space(8.0);
        let _ = labeled_text(ui, d, "Color", "color", 110.0);
    });
    ui.horizontal(|ui| {
        ui.add_sized(vec2(88.0, 20.0), egui::Label::new("Strikethrough"));
        choice(ui, d, "font_dialog_strike", "Strikethrough", "strike", &[("none", "None"), ("sng", "Single"), ("dbl", "Double")]);
    });
    ui.horizontal(|ui| {
        ui.add_sized(vec2(88.0, 20.0), egui::Label::new("Baseline"));
        choice(ui, d, "font_dialog_baseline", "Baseline", "baseline", &[("none", "None"), ("super", "Superscript"), ("sub", "Subscript")]);
        ui.add_space(8.0);
        ui.label("Caps");
        choice(ui, d, "font_dialog_caps", "Caps", "caps", &[("none", "None"), ("small", "Small caps"), ("all", "All caps")]);
    });
    ui.horizontal(|ui| {
        ui.add_sized(vec2(88.0, 20.0), egui::Label::new("Spacing"));
        let mut spacing = d.get("spacing", "0");
        let edited = ui.add(egui::TextEdit::singleline(&mut spacing).desired_width(64.0).hint_text("pt"));
        let mut picked = None;
        let combo = egui::ComboBox::from_id_salt("font_dialog_spacing")
            .selected_text(match spacing.as_str() {
                "-3" => "Very tight",
                "-1.5" => "Tight",
                "0" => "Normal",
                "3" => "Loose",
                "6" => "Very loose",
                other => other,
            })
            .width(120.0)
            .show_ui(ui, |ui| {
                for (v, l) in [("-3", "Very tight"), ("-1.5", "Tight"), ("0", "Normal"), ("3", "Loose"), ("6", "Very loose")] {
                    if ui.selectable_label(spacing == v, l).clicked() {
                        picked = Some(v.to_string());
                    }
                }
            });
        combo.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Spacing"));
        let _ = edited;
        if let Some(v) = picked {
            d.fields.insert("spacing".into(), v);
        } else {
            d.fields.insert("spacing".into(), spacing);
        }
    });
    let bold = d.get("bold", "0") == "1";
    let italic = d.get("italic", "0") == "1";
    let underline = d.get("underline", "none");
    let strike = d.get("strike", "none");
    let shown = finite_pt(&d.get("size", "18")).unwrap_or(18.0).clamp(12.0, 28.0) as f32;
    let mut sample = egui::RichText::new("AaBbCcDd").size(shown);
    if bold {
        sample = sample.strong();
    }
    if italic {
        sample = sample.italics();
    }
    if underline != "none" {
        sample = sample.underline();
    }
    if strike != "none" {
        sample = sample.strikethrough();
    }
    ui.add_space(6.0);
    ui.label(sample);
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        let family = d.get("family", "");
        let family = family.trim();
        if family.is_empty() {
            app.set_status("Choose a font");
            return false;
        }
        let Some(size) = finite_pt(&d.get("size", "")) else {
            app.set_status("Font size must be a number");
            return false;
        };
        let Some(spacing) = finite_pt(&d.get("spacing", "0")) else {
            app.set_status("Character spacing must be a number");
            return false;
        };
        let mut params = json!({
            "family": family,
            "size": size,
            "bold": d.get("bold", "0") == "1",
            "italic": d.get("italic", "0") == "1",
            "underline": d.get("underline", "none"),
            "strike": d.get("strike", "none"),
            "baseline": d.get("baseline", "none"),
            "caps": d.get("caps", "none"),
            "spacing": spacing,
        });
        let color = d.get("color", "");
        let seed = d.get("colorSeed", "");
        if color == seed {
            if let Some(c) = d.params.get("color").filter(|v| !v.is_null()) {
                params["color"] = c.clone();
            }
        } else if !color.trim().is_empty() {
            params["color"] = json!(color.trim());
        }
        let _ = app.run("format.fontDialog", params);
    }
    ok || cancel
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

fn seed_paragraph_fields(d: &mut Dialog) {
    seed_once(d, "align", json_text(d.params.get("align"), "left"));
    seed_once(d, "before", inches_text(d.params.get("indentLeft")));
    seed_once(d, "after", inches_text(d.params.get("indentRight")));
    seed_once(d, "special", json_text(d.params.get("special"), "none"));
    seed_once(d, "by", inches_text(d.params.get("specialBy")));
    seed_once(d, "spaceBefore", pt_text(d.params.get("spaceBefore"), 0.0));
    seed_once(d, "spaceBeforeSeed", pt_text(d.params.get("spaceBefore"), 0.0));
    seed_once(d, "spaceAfter", pt_text(d.params.get("spaceAfter"), 0.0));
    seed_once(d, "spaceAfterSeed", pt_text(d.params.get("spaceAfter"), 0.0));
    seed_once(d, "lineKind", json_text(d.params.get("lineKind"), "multiple"));
    seed_once(d, "lineValue", pt_text(d.params.get("lineValue"), 1.0));
}

fn paragraph_return_params(d: &mut Dialog) -> Value {
    let before = finite_pt(&d.get("before", "0")).unwrap_or(0.0) * 72.0;
    let after = finite_pt(&d.get("after", "0")).unwrap_or(0.0) * 72.0;
    let by = finite_pt(&d.get("by", "0")).unwrap_or(0.0) * 72.0;
    let space_before = finite_pt(&d.get("spaceBefore", "0")).unwrap_or(0.0);
    let space_after = finite_pt(&d.get("spaceAfter", "0")).unwrap_or(0.0);
    let line_value = finite_pt(&d.get("lineValue", "1")).unwrap_or(1.0);
    let before_same = d.get("spaceBefore", "") == d.get("spaceBeforeSeed", "");
    let after_same = d.get("spaceAfter", "") == d.get("spaceAfterSeed", "");
    json!({
        "align": d.get("align", "left"),
        "indentLeft": before,
        "indentRight": after,
        "special": d.get("special", "none"),
        "specialBy": by,
        "spaceBefore": space_before,
        "spaceBeforeKind": if before_same { d.params.get("spaceBeforeKind").and_then(Value::as_str).unwrap_or("pts") } else { "pts" },
        "spaceAfter": space_after,
        "spaceAfterKind": if after_same { d.params.get("spaceAfterKind").and_then(Value::as_str).unwrap_or("pts") } else { "pts" },
        "lineKind": d.get("lineKind", "multiple"),
        "lineValue": line_value,
        "tabs": d.params.get("tabs").cloned().unwrap_or_else(|| json!([])),
        "defaultTab": d.params.get("defaultTab").cloned().unwrap_or_else(|| json!(72.0)),
    })
}

fn line_preset_label(kind: &str, value: &str) -> &'static str {
    if kind == "exactly" {
        "Exactly"
    } else {
        match value {
            "1" => "Single",
            "1.5" => "1.5 lines",
            "2" => "Double",
            _ => "Multiple",
        }
    }
}

/// Home › Paragraph launcher. One OK applies alignment, indentation and spacing together.
fn paragraph_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    ui.set_min_width(460.0);
    seed_paragraph_fields(d);
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 20.0), egui::Label::new("Alignment"));
        choice(
            ui,
            d,
            "paragraph_align",
            "Alignment",
            "align",
            &[
                ("left", "Left"),
                ("center", "Center"),
                ("right", "Right"),
                ("justify", "Justify"),
                ("justLow", "Justify low"),
                ("distributed", "Distributed"),
            ],
        );
    });
    ui.label(egui::RichText::new("Indentation").strong());
    ui.horizontal(|ui| {
        let _ = labeled_text(ui, d, "Before text", "before", 64.0);
        ui.label("in");
        ui.add_space(12.0);
        let _ = labeled_text(ui, d, "After text", "after", 64.0);
        ui.label("in");
    });
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 20.0), egui::Label::new("Special"));
        choice(ui, d, "paragraph_special", "Special", "special", &[("none", "None"), ("first", "First line"), ("hanging", "Hanging")]);
        ui.add_space(8.0);
        let _ = labeled_text(ui, d, "By", "by", 64.0);
        ui.label("in");
    });
    ui.label(egui::RichText::new("Spacing").strong());
    ui.horizontal(|ui| {
        let _ = labeled_text(ui, d, "Spacing before", "spaceBefore", 64.0);
        ui.label("pt");
        ui.add_space(12.0);
        let _ = labeled_text(ui, d, "Spacing after", "spaceAfter", 64.0);
        ui.label("pt");
    });
    let mut picked: Option<(&'static str, Option<&'static str>)> = None;
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(110.0, 20.0), egui::Label::new("Line spacing"));
        let kind = d.get("lineKind", "multiple");
        let value = d.get("lineValue", "1");
        let combo = egui::ComboBox::from_id_salt("paragraph_line").selected_text(line_preset_label(&kind, &value)).width(140.0).show_ui(ui, |ui| {
            for (label, k, v) in [
                ("Single", "multiple", Some("1")),
                ("1.5 lines", "multiple", Some("1.5")),
                ("Double", "multiple", Some("2")),
                ("Multiple", "multiple", None),
                ("Exactly", "exactly", None),
            ] {
                if ui.selectable_label(line_preset_label(&kind, &value) == label, label).clicked() {
                    picked = Some((k, v));
                }
            }
        });
        let _ = combo.response.labelled_by(field.id);
        ui.add_space(8.0);
        let _ = labeled_text(ui, d, "At", "lineValue", 64.0);
    });
    if let Some((kind, value)) = picked {
        d.fields.insert("lineKind".into(), kind.into());
        if let Some(v) = value {
            d.fields.insert("lineValue".into(), v.into());
        } else if kind == "exactly" && finite_pt(&d.get("lineValue", "1")).is_some_and(|n| n <= 9.99) {
            d.fields.insert("lineValue".into(), "18".into());
        } else if kind == "multiple" && finite_pt(&d.get("lineValue", "1")).is_some_and(|n| n > 9.99) {
            d.fields.insert("lineValue".into(), "1".into());
        }
    }
    let mut open_tabs = false;
    ui.horizontal(|ui| {
        if ui.button("Tabs…").clicked() {
            open_tabs = true;
        }
    });
    let (ok, cancel) = buttons(ui, "OK");
    if open_tabs {
        let mut tabs = Dialog::new("tabs");
        tabs.params = json!({
            "tabs": d.params.get("tabs").cloned().unwrap_or_else(|| json!([])),
            "defaultTab": d.params.get("defaultTab").cloned().unwrap_or_else(|| json!(72.0)),
            "returnId": "paragraph",
            "returnParams": paragraph_return_params(d),
        });
        app.dialog = Some(tabs);
        return true;
    }
    if ok {
        let Some(before_in) = finite_pt(&d.get("before", "")) else {
            app.set_status("Indent before text must be a number of inches");
            return false;
        };
        let Some(after_in) = finite_pt(&d.get("after", "")) else {
            app.set_status("Indent after text must be a number of inches");
            return false;
        };
        let Some(by_in) = finite_pt(&d.get("by", "")) else {
            app.set_status("Special indent must be a number of inches");
            return false;
        };
        let Some(space_before) = finite_pt(&d.get("spaceBefore", "")) else {
            app.set_status("Spacing before must be a number");
            return false;
        };
        let Some(space_after) = finite_pt(&d.get("spaceAfter", "")) else {
            app.set_status("Spacing after must be a number");
            return false;
        };
        let Some(line_value) = finite_pt(&d.get("lineValue", "")) else {
            app.set_status("Line spacing must be a number");
            return false;
        };
        let mut params = json!({
            "align": d.get("align", "left"),
            "indentLeft": before_in * 72.0,
            "indentRight": after_in * 72.0,
            "special": d.get("special", "none"),
            "specialBy": by_in * 72.0,
            "spaceBefore": space_before,
            "spaceAfter": space_after,
            "spaceBeforeKind": if d.get("spaceBefore", "") == d.get("spaceBeforeSeed", "") { d.params.get("spaceBeforeKind").and_then(Value::as_str).unwrap_or("pts") } else { "pts" },
            "spaceAfterKind": if d.get("spaceAfter", "") == d.get("spaceAfterSeed", "") { d.params.get("spaceAfterKind").and_then(Value::as_str).unwrap_or("pts") } else { "pts" },
        });
        if d.get("lineKind", "multiple") == "exactly" {
            params["lineSpacingPt"] = json!(line_value);
        } else {
            params["lineSpacing"] = json!(line_value);
        }
        if app.run("format.paragraphDialog", params).is_err() {
            return false;
        }
    }
    ok || cancel
}

const TAB_KINDS: [(&str, &str); 4] = [("l", "Left"), ("ctr", "Center"), ("r", "Right"), ("dec", "Decimal")];

/// Format › Tabs: the default spacing and the tab stops of the selected paragraphs, edited as a
/// list (inches, as PowerPoint's dialog) and applied together with OK.
fn tabs_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    if !d.fields.contains_key("stops") {
        // Opened from Paragraph (or by `format.tabs` with no fields) the stops come with the
        // dialog; otherwise they are read from the caret's paragraph.
        let r = if d.params.get("tabs").is_some() { d.params.clone() } else { app.session.execute("format.ruler", &json!({})).unwrap_or_default() };
        let stops: Vec<String> = r["tabs"]
            .as_array()
            .map(|a| a.iter().filter_map(|t| Some(format!("{}:{}", t["pos"].as_f64()? / 72.0, t["align"].as_str().unwrap_or("l")))).collect())
            .unwrap_or_default();
        d.fields.insert("stops".into(), stops.join("|"));
        d.fields.insert("default".into(), format!("{}", r["defaultTab"].as_f64().unwrap_or(72.0) / 72.0));
    }
    let mut stops: Vec<(f64, String)> = d
        .get("stops", "")
        .split('|')
        .filter_map(|x| {
            let (p, a) = x.split_once(':')?;
            Some((p.parse::<f64>().ok().filter(|v| v.is_finite())?, a.to_string()))
        })
        .collect();
    field(ui, d, "Default tab stops (in):", "default", "1");
    ui.add_space(6.0);
    ui.label("Tab stops:");
    let mut remove = None;
    egui::Grid::new("tab_stops").num_columns(3).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
        if stops.is_empty() {
            ui.label("(none)");
            ui.end_row();
        }
        for (i, (pos, align)) in stops.iter().enumerate() {
            ui.label(format!("{pos:.2}\""));
            ui.label(TAB_KINDS.iter().find(|k| k.0 == align).map(|k| k.1).unwrap_or("Left"));
            if ui.small_button("Clear").on_hover_text(format!("Clear the tab at {pos:.2}\"")).clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        stops.remove(i);
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let l = ui.label("Tab stop position (in):");
        let mut v = d.get("newpos", "");
        ui.add(egui::TextEdit::singleline(&mut v).desired_width(60.0)).labelled_by(l.id);
        d.fields.insert("newpos".into(), v);
    });
    let cur = d.get("newalign", "l");
    ui.horizontal(|ui| {
        ui.label("Alignment:");
        for (k, label) in TAB_KINDS {
            if ui.radio(cur == k, label).clicked() {
                d.fields.insert("newalign".into(), k.into());
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Set").clicked()
            && let Some(p) = Some(num(d, "newpos", f64::NAN)).filter(|v| v.is_finite() && *v >= 0.0 && *v <= 56.0)
        {
            stops.retain(|(q, _)| (q - p).abs() > 0.005);
            stops.push((p, d.get("newalign", "l")));
            stops.sort_by(|a, b| a.0.total_cmp(&b.0));
            d.fields.insert("newpos".into(), String::new());
        }
        if ui.button("Clear All").clicked() {
            stops.clear();
        }
    });
    d.fields.insert("stops".into(), stops.iter().map(|(p, a)| format!("{p}:{a}")).collect::<Vec<_>>().join("|"));
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        let set: Vec<Value> = stops.iter().map(|(p, a)| json!({"pos": p * 72.0, "align": a})).collect();
        let dt = num(d, "default", 1.0).clamp(0.02, 56.0) * 72.0;
        if let Err(e) = app.run("format.tabs", json!({"set": set, "defaultTab": dt})) {
            app.set_status(e);
        } else if let Some(ret) = d.params.get_mut("returnParams").and_then(Value::as_object_mut) {
            // Back in Paragraph, its Tabs… shows what was just applied.
            ret.insert("tabs".into(), json!(set));
            ret.insert("defaultTab".into(), json!(dt));
        }
    }
    ok || cancel
}

/// Home › Find / Replace: step through matches (slide text, table cells, notes) or replace them.
fn find_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let replace_mode = d.get("replace", "0") == "1";
    // "Find what:" has the keyboard when the dialog opens, so Ctrl+F and typing finds (#951).
    let mut what = d.get("text", "");
    ui.horizontal(|ui| {
        ui.add_sized(vec2(120.0, 20.0), egui::Label::new("Find what:"));
        let r = ui.add(egui::TextEdit::singleline(&mut what).desired_width(180.0));
        // Enter finds the next match and leaves the keyboard here, so the search can be changed.
        if !d.fields.contains_key("focused") || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
            r.request_focus();
            d.fields.insert("focused".into(), "1".into());
        }
    });
    d.fields.insert("text".into(), what);
    if replace_mode {
        field(ui, d, "Replace with:", "with", "");
    }
    let case = check(ui, d, "Match case", "matchCase", false);
    let whole = check(ui, d, "Find whole words only", "wholeWords", false);
    let text = d.get("text", "");
    let q = json!({"text": text, "matchCase": case, "wholeWords": whole});
    let mut close = false;
    let mut find_next = false;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let has = !text.is_empty();
        if ui.add_enabled(has, egui::Button::new("Find Next")).clicked() || (has && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
            find_next = true;
        }
        if replace_mode {
            if ui.add_enabled(has, egui::Button::new("Replace")).clicked() {
                let cur: Option<Value> = serde_json::from_str(&d.get("cur", "")).ok();
                if let Some(at) = cur {
                    let mut p = q.clone();
                    p["with"] = json!(d.get("with", ""));
                    p["at"] = at;
                    if app.session.execute("text.replace", &p).is_ok() {
                        let n = d.get("next", "1").parse::<usize>().unwrap_or(1);
                        d.fields.insert("next".into(), n.saturating_sub(1).to_string());
                    }
                }
                find_next = true;
            }
            if ui.add_enabled(has, egui::Button::new("Replace All")).clicked() {
                let mut p = q.clone();
                p["with"] = json!(d.get("with", ""));
                let msg = match app.session.execute("text.replace", &p) {
                    Ok(v) => format!("Replaced {} occurrence(s).", v.get("replaced").and_then(Value::as_u64).unwrap_or(0)),
                    Err(_) => format!("DeckCraft can't find “{text}”."),
                };
                d.fields.insert("status".into(), msg);
                d.fields.remove("cur");
            }
        } else if ui.button("Replace…").clicked() {
            d.fields.insert("replace".into(), "1".into());
        }
        if ui.button("Close").clicked() {
            close = true;
        }
    });
    if find_next {
        let list = app.session.execute("text.find", &q).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        if list.is_empty() {
            d.fields.insert("status".into(), format!("DeckCraft can't find “{text}”."));
            d.fields.remove("cur");
        } else {
            let n = d.get("next", "0").parse::<usize>().unwrap_or(0) % list.len();
            let m = &list[n];
            let _ = app.run("slide.go", json!({"index": m.get("slide").cloned().unwrap_or_default()}));
            if let Some(id) = m.get("shape").filter(|v| !v.is_null()) {
                let _ = app.run("edit.select", json!({"ids": [id]}));
            }
            d.fields.insert("cur".into(), m.to_string());
            d.fields.insert("next".into(), (n + 1).to_string());
            let where_ = if m.get("shape").is_some_and(Value::is_null) { " (notes)" } else { "" };
            d.fields.insert(
                "status".into(),
                format!("Match {} of {} on slide {}{where_}.", n + 1, list.len(), m.get("slide").and_then(Value::as_u64).unwrap_or(0) + 1),
            );
        }
    }
    let status = d.get("status", "");
    if !status.is_empty() {
        ui.add_space(4.0);
        ui.label(status);
    }
    close
}

#[cfg(test)]
#[path = "font_dialog_tests.rs"]
mod font_dialog_tests;

#[cfg(test)]
#[path = "size_position_tests.rs"]
mod size_position_tests;

#[cfg(test)]
#[path = "columns_dialog_tests.rs"]
mod columns_dialog_tests;

#[cfg(test)]
#[path = "photo_album_tests.rs"]
mod photo_album_tests;

/// The export dialog's Animated GIF options (size, seconds per slide, loop, slide range, hidden
/// slides) as `file.export` params; a slide range that doesn't fit the deck is an error message.
fn gif_options(app: &SlideApp, ui: &mut Ui, d: &mut Dialog) -> Result<Value, String> {
    use deckcraft_engine::gif_export::{MAX_SECONDS, MAX_WIDTH, MIN_SECONDS, MIN_WIDTH, SIZE_PRESETS};
    let n = app.session.active().map_or(0, |s| s.doc.slides.len());
    let size = d.get("gifSize", "medium");
    let size_label = |k: &str| match k {
        "small" => "Small (640 px wide)",
        "large" => "Large (1280 px wide)",
        "extraLarge" => "Extra Large (1920 px wide)",
        "custom" => "Custom width",
        _ => "Medium (960 px wide)",
    };
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(120.0, 20.0), egui::Label::new("Size:"));
        let combo = egui::ComboBox::from_id_salt("gif_size").selected_text(size_label(&size)).show_ui(ui, |ui| {
            for k in SIZE_PRESETS.iter().map(|(k, _)| *k).chain(["custom"]) {
                if ui.selectable_label(size == k, size_label(k)).clicked() {
                    d.fields.insert("gifSize".into(), k.into());
                }
            }
        });
        let _ = combo.response.labelled_by(field.id);
    });
    if size == "custom" {
        field(ui, d, "Width (pixels):", "gifWidth", "960");
    }
    field(ui, d, "Seconds per slide:", "gifSeconds", "5");
    let n_text = n.max(1).to_string();
    field(ui, d, "From slide:", "gifFrom", "1");
    field(ui, d, "To slide:", "gifTo", &n_text);
    let forever = check(ui, d, "Loop forever", "gifLoop", true);
    let hidden = check(ui, d, "Include hidden slides", "gifHidden", false);

    let width = if size == "custom" {
        let w = num(d, "gifWidth", 960.0).round();
        if !(f64::from(MIN_WIDTH)..=f64::from(MAX_WIDTH)).contains(&w) {
            return Err(format!("Width: enter {MIN_WIDTH} to {MAX_WIDTH} pixels"));
        }
        json!(w as u32)
    } else {
        json!(size)
    };
    let seconds = num(d, "gifSeconds", f64::NAN);
    if deckcraft_engine::gif_export::check_seconds(seconds).is_err() {
        return Err(format!("Seconds per slide: enter {MIN_SECONDS} to {MAX_SECONDS}"));
    }
    let mut p = json!({"width": width, "secondsPerSlide": seconds, "loop": forever, "includeHidden": hidden});
    let (from, to) = (num(d, "gifFrom", 1.0).round(), num(d, "gifTo", n as f64).round());
    if !(from >= 1.0 && from <= to && to <= n as f64) {
        return Err(format!("Slides: choose a range from 1 to {n}"));
    }
    if from > 1.0 || to < n as f64 {
        p["slides"] = json!(((from as usize - 1)..(to as usize)).collect::<Vec<_>>());
    }
    Ok(p)
}

#[cfg(test)]
#[path = "theme_colors_tests.rs"]
mod theme_colors_tests;

#[cfg(test)]
#[path = "theme_fonts_tests.rs"]
mod theme_fonts_tests;

#[cfg(test)]
mod alt_text_tests {
    use super::alt_text_starts_decorative;

    #[test]
    fn decorative_starts_checked_only_when_every_shape_is() {
        assert!(!alt_text_starts_decorative([]));
        assert!(!alt_text_starts_decorative([false]));
        assert!(alt_text_starts_decorative([true]));
        assert!(alt_text_starts_decorative([true, true]));
        assert!(!alt_text_starts_decorative([true, false]));
    }
}

#[cfg(test)]
mod tests {
    use deckcraft_model::text::{Action, Hyperlink, Run, TextBody};
    use deckcraft_model::{Presentation, SlideId};

    use super::{
        CurrentLink, HyperlinkSeed, common_click, hyperlink_command, hyperlink_seed_of, hyperlink_should_apply, link_covering, seed_from,
        seed_of_link,
    };

    fn link(action: Action, tip: &str) -> Hyperlink {
        Hyperlink { action, tooltip: tip.into(), highlight_click: false, sound: None, stop_sound: false, zoom_return: None }
    }

    fn url(address: &str, tip: &str) -> Hyperlink {
        let mut h = link(Action::Url { url: address.into() }, tip);
        h.highlight_click = tip == "glow";
        h
    }

    /// Header & Footer ▸ Apply without picking a format leaves a slide's existing date field in
    /// its own format instead of rewriting it to the default one.
    #[test]
    fn header_footer_apply_keeps_an_existing_date_format() {
        use std::sync::Arc;

        use egui::vec2;
        use egui_kittest::kittest::Queryable as _;
        use serde_json::json;

        use crate::SlideApp;
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("design.headerFooter", &json!({"date": true, "dateText": "", "dateFormat": "datetime4", "all": true})).unwrap();
        // A file whose document-level format is unset, as an imported deck's is.
        Arc::make_mut(&mut session.doc_mut().unwrap().doc).header_footer.date_format = String::new();
        let date_fields = |app: &SlideApp| -> Vec<String> {
            app.session
                .doc()
                .unwrap()
                .doc
                .slides
                .iter()
                .flat_map(|sl| sl.shapes.iter())
                .filter(|sh| sh.ph_type() == Some(deckcraft_model::PhType::Date))
                .flat_map(|sh| sh.text.iter())
                .flat_map(|t| t.paragraphs.iter())
                .flat_map(|p| p.runs.iter())
                .filter_map(|r| match &r.kind {
                    deckcraft_model::text::RunKind::Field { field } => Some(field.clone()),
                    _ => None,
                })
                .collect()
        };
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(1200.0, 900.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(session, crate::Services::default()),
        );
        harness.run_steps(2);
        assert_eq!(date_fields(harness.state()), vec!["datetime4".to_string()]);
        harness.state_mut().run("app.dialog", json!({"id": "headerFooter"})).unwrap();
        harness.run_steps(2);
        harness.get_by_label("Apply").click_accesskit();
        harness.run_steps(3);
        assert_eq!(date_fields(harness.state()), vec!["datetime4".to_string()], "the dialog left the field's format alone");
    }

    /// The Export… dialog (Animated GIF) shows the GIF options and exports through `file.export` (#171).
    #[test]
    fn export_dialog_writes_an_animated_gif() {
        use std::cell::RefCell;

        use egui::vec2;
        use egui_kittest::kittest::Queryable as _;
        use serde_json::json;

        use crate::SlideApp;
        let dir = std::env::temp_dir().join(format!("deckcraft-ui-gif-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("deck.gif");
        let picked = path.to_string_lossy().to_string();
        let suggested = std::rc::Rc::new(RefCell::new(String::new()));
        let seen = suggested.clone();
        let services = crate::Services {
            pick_save: Some(Box::new(move |s: &str| {
                *seen.borrow_mut() = s.to_string();
                Some(picked.clone())
            })),
            ..Default::default()
        };
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("slide.new", &json!({})).unwrap();
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(1200.0, 900.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(session, services),
        );
        harness.run_steps(2);
        harness.state_mut().run("app.exportDialog", json!({})).unwrap();
        harness.run_steps(2);
        harness.get_by_label("Animated GIF (.gif)").click_accesskit();
        harness.run_steps(2);
        assert_eq!(harness.state().dialog.as_ref().and_then(|d| d.fields.get("fmt")).map(String::as_str), Some("gif"));
        harness.get_by_label("Seconds per slide:");
        harness.get_by_label("Loop forever").click_accesskit();
        harness.run_steps(2);
        // A range outside the deck keeps the dialog open with a message, and nothing is written.
        harness.state_mut().dialog.as_mut().unwrap().fields.insert("gifTo".into(), "9".into());
        harness.run_steps(2);
        harness.key_press(egui::Key::Enter);
        harness.run_steps(2);
        assert!(harness.state().dialog.is_some());
        assert!(harness.state().status.as_ref().is_some_and(|(m, _)| m.contains("Slides")), "{:?}", harness.state().status);
        assert!(!path.exists());
        harness.state_mut().dialog.as_mut().unwrap().fields.insert("gifTo".into(), "2".into());
        harness.run_steps(2);
        harness.key_press(egui::Key::Enter);
        harness.run_steps(2);
        assert!(harness.state().dialog.is_none(), "the dialog closes after exporting");
        assert!(suggested.borrow().ends_with(".gif"), "{}", suggested.borrow());
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"GIF89a"));
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 960, "medium by default");
        assert!(!bytes.windows(11).any(|w| w == b"NETSCAPE2.0"), "Loop forever unticked: plays once");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The Export… dialog offers "Transparent background" for PNG only, and the picture it
    /// writes then leaves the slide background out (#784).
    #[test]
    fn export_dialog_writes_a_png_with_a_transparent_background() {
        use egui::vec2;
        use egui_kittest::kittest::Queryable as _;
        use serde_json::json;

        use crate::SlideApp;
        let dir = std::env::temp_dir().join(format!("deckcraft-ui-png-clear-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("slide.png");
        let picked = path.to_string_lossy().to_string();
        let services = crate::Services { pick_save: Some(Box::new(move |_: &str| Some(picked.clone()))), ..Default::default() };
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(1200.0, 900.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(deckcraft_engine::Session::with_new(), services),
        );
        harness.run_steps(2);
        harness.state_mut().run("app.exportDialog", json!({})).unwrap();
        harness.run_steps(2);
        assert!(harness.query_by_label("Transparent background").is_none(), "PDF has no transparent background");
        harness.get_by_label("JPEG image (current slide)").click_accesskit();
        harness.run_steps(2);
        assert!(harness.query_by_label("Transparent background").is_none(), "nor has JPEG");
        harness.get_by_label("PNG images (current slide)").click_accesskit();
        harness.run_steps(2);
        harness.get_by_label("Transparent background").click_accesskit();
        harness.run_steps(2);
        assert_eq!(harness.state().dialog.as_ref().and_then(|d| d.fields.get("pngTransparent")).map(String::as_str), Some("true"));
        harness.key_press(egui::Key::Enter);
        harness.run_steps(2);
        assert!(harness.state().dialog.is_none(), "the dialog closes after exporting");
        let img = deckcraft_render::decode_image(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(img.get_pixel(2, 2).0[3], 0, "the background is left out");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_link_opens_as_a_new_web_address() {
        let seed = seed_from(CurrentLink::None, &[]);
        assert_eq!(seed, HyperlinkSeed::blank());
        assert!(hyperlink_should_apply(&seed, "url", "https://", "", 0));
    }

    #[test]
    fn web_email_and_slide_links_fill_the_dialog() {
        let ids = [SlideId(7), SlideId(9)];
        let web = seed_of_link(&url("https://example.org", "See"), &ids);
        assert_eq!(web.mode, "url");
        assert_eq!(web.url, "https://example.org");
        assert_eq!(web.tip, "See");
        assert!(!web.preserve);
        assert!(hyperlink_should_apply(&web, "url", "https://example.org", "", 0));

        let mail = seed_of_link(&url("mailto:a@b.org", "Write"), &ids);
        assert_eq!(mail.mode, "email");
        assert_eq!(mail.email, "a@b.org");
        assert_eq!(mail.tip, "Write");
        assert_eq!(hyperlink_command("email", "", "a@b.org", 0, "Write", false)["url"], "mailto:a@b.org");

        let slide = seed_of_link(&link(Action::Slide { slide: SlideId(9) }, "There"), &ids);
        assert_eq!(slide.mode, "slide");
        assert_eq!(slide.slide, 1);
        assert_eq!(slide.tip, "There");
        assert!(!slide.preserve);

        let glow = url("https://example.org", "glow");
        assert!(glow.highlight_click);
        assert!(seed_of_link(&glow, &ids).highlight);
        assert_eq!(hyperlink_command("url", "https://example.org", "", 0, "glow", true)["highlightClick"], true);
    }

    #[test]
    fn actions_this_dialog_does_not_edit_survive_ok() {
        let next = seed_of_link(&link(Action::NextSlide, "On"), &[]);
        assert!(next.preserve);
        assert_eq!(next.tip, "On");
        assert_eq!(next.url, "");
        assert!(!hyperlink_should_apply(&next, "url", "", "", 0));
        assert!(hyperlink_should_apply(&next, "url", "https://example.org", "", 0));

        let gone = seed_of_link(&link(Action::Slide { slide: SlideId(99) }, ""), &[SlideId(1)]);
        assert!(gone.preserve, "a missing slide is not retargeted to the first slide");
        assert!(!hyperlink_should_apply(&gone, "url", "", "", 0));

        let mixed = seed_from(CurrentLink::Mixed, &[]);
        assert!(mixed.preserve);
        assert!(!hyperlink_should_apply(&mixed, "url", "", "", 0));
        assert!(hyperlink_should_apply(&mixed, "email", "", "a@b.org", 0));
    }

    #[test]
    fn a_text_span_reports_the_link_it_covers() {
        let mut body = TextBody::from_text("Hello world");
        body.paragraphs[0].runs.clear();
        body.paragraphs[0].runs.push(Run::new("Hello "));
        let mut world = Run::new("world");
        world.props.link = Some(url("https://example.org", "tip"));
        body.paragraphs[0].runs.push(world);

        assert_eq!(link_covering(&body, (0, 6), (0, 6)), CurrentLink::None, "a caret inserts; it does not edit the run beside it");
        match link_covering(&body, (0, 6), (0, 11)) {
            CurrentLink::One(h) => assert_eq!(h.tooltip, "tip"),
            other => panic!("linked word, got {other:?}"),
        }
        assert_eq!(link_covering(&body, (0, 0), (0, 11)), CurrentLink::Mixed);

        let mut other = url("https://example.org", "tip");
        other.tooltip = "other".into();
        assert_eq!(common_click([None, None]), CurrentLink::None);
        assert!(matches!(common_click([Some(url("https://example.org", "tip"))]), CurrentLink::One(_)));
        assert_eq!(common_click([Some(url("https://example.org", "tip")), Some(other)]), CurrentLink::Mixed);
        assert_eq!(common_click(std::iter::empty()), CurrentLink::None);
    }

    #[test]
    fn the_open_dialog_reads_the_caret_word_and_the_shape_click() {
        let mut doc = Presentation::default();
        let shape_id = {
            let slide = std::sync::Arc::make_mut(&mut doc.slides[0]);
            let mut body = TextBody::from_text("Hello world");
            body.paragraphs[0].runs.clear();
            body.paragraphs[0].runs.push(Run::new("Hello "));
            let mut world = Run::new("world");
            world.props.link = Some(url("https://example.org", "tip"));
            body.paragraphs[0].runs.push(world);
            slide.shapes[0].text = Some(body);
            slide.shapes[0].id
        };
        let mut st = deckcraft_engine::DocState::new(doc, None, "t".into());
        st.selection.text = Some(deckcraft_engine::TextSel { shape: shape_id, anchor: (0, 8), caret: (0, 8), ..Default::default() });
        let seed = hyperlink_seed_of(Some(&st));
        assert_eq!(seed.mode, "url");
        assert_eq!(seed.url, "https://example.org");
        assert_eq!(seed.tip, "tip");
        assert!(!seed.preserve);

        st.selection.text = None;
        st.selection.shapes = vec![shape_id];
        {
            let doc = std::sync::Arc::make_mut(&mut st.doc);
            let slide = std::sync::Arc::make_mut(&mut doc.slides[0]);
            let shape = slide.shapes.iter_mut().find(|sh| sh.id == shape_id).unwrap();
            let mut click = url("https://shape.example", "shape tip");
            click.highlight_click = true;
            shape.click = Some(click);
        }
        let seed = hyperlink_seed_of(Some(&st));
        assert_eq!(seed.url, "https://shape.example");
        assert_eq!(seed.tip, "shape tip");
        assert!(seed.highlight);

        st.selection.text = Some(deckcraft_engine::TextSel { shape: shape_id, anchor: (0, 11), caret: (0, 11), ..Default::default() });
        let seed = hyperlink_seed_of(Some(&st));
        assert_eq!(seed, HyperlinkSeed::blank(), "a caret at the end of the word inserts a new link");
    }
}

#[cfg(test)]
mod zoom_tests {
    use super::parse_zoom_percent;

    fn near(v: f32, want: f32) -> bool {
        (v - want).abs() < 1.0e-4
    }

    #[test]
    fn zoom_percent_accepts_a_number_and_clamps() {
        assert!(parse_zoom_percent("150").is_some_and(|v| near(v, 1.5)));
        assert!(parse_zoom_percent("150%").is_some_and(|v| near(v, 1.5)));
        assert!(parse_zoom_percent("  100 ").is_some_and(|v| near(v, 1.0)));
        assert!(parse_zoom_percent("1").is_some_and(|v| near(v, 0.1)));
        assert!(parse_zoom_percent("900").is_some_and(|v| near(v, 4.0)));
        assert!(parse_zoom_percent("").is_none());
        assert!(parse_zoom_percent("fit").is_none());
        assert!(parse_zoom_percent("nan").is_none());
        assert!(parse_zoom_percent("inf").is_none());
    }
}

#[cfg(test)]
mod trim_media_tests {
    use std::sync::Arc;

    use deckcraft_model::{MediaClip, Presentation, ShapeId, ShapeKind};

    use super::{ms_as_seconds, seconds_text_to_ms, trim_seed};

    #[test]
    fn trim_seconds_round_trip_the_stored_milliseconds() {
        for ms in [0, 1, 250, 333, 1000, 1500, 60_000] {
            let text = ms_as_seconds(ms);
            assert_eq!(seconds_text_to_ms(&text), ms, "{text}");
        }
        assert_eq!(seconds_text_to_ms(""), 0);
        assert_eq!(seconds_text_to_ms("nope"), 0);
        assert_eq!(seconds_text_to_ms("-2"), 0);
        assert_eq!(seconds_text_to_ms("1.5"), 1500);
        assert_eq!(seconds_text_to_ms("999999999"), u32::MAX);
    }

    #[test]
    fn trim_dialog_reads_the_selected_clip() {
        assert_eq!(trim_seed(None).start, "0");
        let mut doc = Presentation::default();
        let media_id = {
            let slide = Arc::make_mut(&mut doc.slides[0]);
            let title = slide.shapes[0].id;
            let clip = MediaClip { trim_start_ms: 1500, trim_end_ms: 250, fade_in_ms: 333, fade_out_ms: 1000, ..Default::default() };
            let mut media = slide.shapes[0].clone();
            media.id = ShapeId(900);
            media.kind = ShapeKind::Media(clip);
            slide.shapes.push(media);
            (title, ShapeId(900))
        };
        let mut st = deckcraft_engine::DocState::new(doc, None, "t".into());
        st.selection.shapes = vec![media_id.0];
        assert_eq!(trim_seed(Some(&st)).start, "0", "a title shape is not a clip");
        st.selection.shapes = vec![media_id.0, media_id.1];
        let seed = trim_seed(Some(&st));
        assert_eq!(seed.start, "1.5");
        assert_eq!(seed.end, "0.25");
        assert_eq!(seed.fade_in, "0.333");
        assert_eq!(seed.fade_out, "1");
    }
}

#[cfg(test)]
#[path = "spelling_tests.rs"]
mod spelling_tests;

#[cfg(test)]
#[path = "paragraph_dialog_tests.rs"]
mod paragraph_dialog_tests;

#[cfg(test)]
mod about_credits_tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::{CREDITS_SIZE, Dialog, credits_block};
    use crate::{Services, SlideApp};

    fn tab_of(h: &Harness<'_, SlideApp>) -> String {
        let d = h.state().dialog.as_ref().expect("the About dialog is open");
        d.fields.get("tab").cloned().unwrap_or_else(|| "about".into())
    }

    /// The reported symptom (#843): in the real dialog the tabs stopped answering once Contributors
    /// or Models was open. Click through Contributors, Models and back to About.
    #[test]
    fn about_dialog_tabs_keep_responding() {
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), Services::default());
        app.dialog = Some(Dialog::new("about"));
        let mut h = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        assert_eq!(tab_of(&h), "about");
        for label in ["Contributors", "Models", "About", "Models", "Contributors"] {
            h.get_by_label(label).click();
            h.run_steps(3);
            assert_eq!(tab_of(&h), label.to_lowercase(), "clicking {label} switches the tab");
        }
    }

    #[test]
    fn credits_get_their_own_block_below_the_tab_row() {
        for models in [false, true] {
            let ctx = egui::Context::default();
            let mut tabs = egui::Rect::NOTHING;
            let mut block = egui::Rect::NOTHING;
            let mut whole = egui::Rect::NOTHING;
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                tabs = ui.label("About").rect;
                block = credits_block(ui, models);
                whole = ui.min_rect();
            });
            out.textures_delta.clear();
            assert_eq!(block.size(), CREDITS_SIZE, "models={models}");
            assert!(block.min.y >= tabs.max.y, "the credits start below the tab row (models={models})");
            assert!(whole.max.y >= block.max.y, "the dialog grows to hold the block (models={models})");
            assert!(whole.height() > CREDITS_SIZE.y, "the tab row is not squeezed into the credits height (models={models})");
        }
    }
}

#[cfg(test)]
#[path = "preferences_tests.rs"]
mod preferences_tests;
