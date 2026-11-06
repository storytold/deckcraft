use super::{Dialog, SlideApp, Tokens, Ui, Value, buttons, check, field, json};

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
    let opened = craft_ui::ComboBox::from_id_salt(id).selected_text(label).width(220.0).show_ui(ui, |ui| {
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
pub(super) fn photo_album_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
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
