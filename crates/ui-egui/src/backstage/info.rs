use super::files::task;
use super::{Backstage, Icon, Outcome, Page, RichText, Sense, SlideApp, Tokens, Ui, Value, dim, heading, json, theme, vec2};

/// Save As and Change File Type: (id for [`SlideApp::export_as`], label, detail).
pub const SAVE_TYPES: [(&str, &str, &str); 7] = [
    ("pptx", "PowerPoint Presentation (*.pptx)", "Opens in PowerPoint and other presentation apps"),
    ("deckcraft", "DeckCraft Presentation (*.deckcraft)", "DeckCraft's own format: everything is kept"),
    ("pdf", "PDF (*.pdf)", "A fixed copy that keeps its layout on any device"),
    ("outline", "Outline (*.txt)", "The slide titles and bullet text as plain text"),
    ("rtf", "Outline/RTF (*.rtf)", "The slide titles and bullet text for word processors"),
    ("png", "PNG Picture (*.png)", "The current slide as a picture"),
    ("jpeg", "JPEG Picture (*.jpg)", "The current slide as a compressed picture"),
];

/// Info page properties: (`file.properties` key, label, hint).
pub(super) const PROPERTIES: [(&str, &str, &str); 7] = [
    ("title", "Title", "Add a title"),
    ("subject", "Subject", "Specify the subject"),
    ("keywords", "Tags", "Add a tag"),
    ("category", "Categories", "Add a category"),
    ("company", "Company", "Specify the company"),
    ("comments", "Comments", "Add comments"),
    ("author", "Author", "Add an author"),
];

/// Info: the presentation's name and location, Inspect and Manage on the left, its properties
/// (editable, through `file.properties`) on the right.
pub(super) fn info(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
    let t = Tokens::get(ui.ctx());
    let Ok(v) = app.session.execute("file.info", &json!({})) else {
        dim(ui, "No presentation is open.");
        return;
    };
    let path = v.get("path").and_then(Value::as_str).map(str::to_string);
    let title = v.get("title").and_then(Value::as_str).unwrap_or("").to_string();
    let dirty = v.get("dirty").and_then(Value::as_bool).unwrap_or(false);
    ui.label(RichText::new(&title).font(theme::font(24.0)).color(t.accent));
    match &path {
        Some(p) => dim(ui, p),
        None => dim(ui, "Not saved yet"),
    }
    if let Some(p) = &path {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Copy path").clicked() {
                ui.ctx().copy_text(p.clone());
                app.set_status("Copied the path");
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(dir) = std::path::Path::new(p).parent().and_then(std::path::Path::to_str)
                && ui.button("Open file location").clicked()
            {
                app.open_url(&format!("file:///{}", dir.replace('\\', "/").trim_start_matches('/')));
            }
        });
    }
    ui.add_space(20.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(440.0);
            if task(
                ui,
                Icon::Accessibility,
                "Check for Issues",
                "Inspect Presentation",
                "Before you share it, check the presentation for things people with disabilities might find hard to read.",
                true,
            ) {
                let _ = app.run("app.dialog", json!({"id": "accessibility"}));
            }
            let (detail, can_revert) = match (dirty, path.is_some()) {
                (false, _) => ("There are no unsaved changes.", false),
                (true, true) => ("This presentation has unsaved changes. Revert goes back to the saved file.", true),
                (true, false) => ("This presentation has never been saved.", false),
            };
            if task(ui, Icon::ResetSlide, "Revert", "Manage Presentation", detail, can_revert) {
                let _ = app.run("file.revert", json!({}));
            }
        });
        ui.add_space(30.0);
        ui.vertical(|ui| {
            heading(ui, "Properties");
            let props = v.get("properties").cloned().unwrap_or_default();
            let size = v.get("bytes").and_then(Value::as_u64).map(format_bytes);
            egui::Grid::new("backstage_props").num_columns(2).spacing(vec2(18.0, 7.0)).show(ui, |ui| {
                let row = |ui: &mut Ui, k: &str, val: String| {
                    ui.label(RichText::new(k).color(t.text_dim));
                    ui.label(val);
                    ui.end_row();
                };
                if let Some(s) = size {
                    row(ui, "Size", s);
                }
                row(ui, "Slides", v.get("slides").and_then(Value::as_u64).unwrap_or(0).to_string());
                row(ui, "Hidden slides", v.get("hiddenSlides").and_then(Value::as_u64).unwrap_or(0).to_string());
                for (i, (key, label, hint)) in PROPERTIES.iter().enumerate() {
                    let saved = props.get(*key).and_then(Value::as_str).unwrap_or("").to_string();
                    let Some(draft) = bs.props.get_mut(i) else { continue };
                    let l = ui.label(RichText::new(*label).color(t.text_dim));
                    let id = ui.make_persistent_id(("backstage_prop", *key));
                    let resp = ui.add(egui::TextEdit::singleline(draft).id(id).hint_text(*hint).desired_width(300.0)).labelled_by(l.id);
                    if resp.lost_focus() && *draft != saved {
                        // Commit first: a click on another field takes the focus before this one is drawn.
                        let mut p = serde_json::Map::new();
                        p.insert((*key).to_string(), json!(draft.clone()));
                        let _ = app.run("file.properties", Value::Object(p));
                    } else if !resp.has_focus() && *draft != saved {
                        // Show the presentation's value unless the field is being edited (undo...).
                        draft.clone_from(&saved);
                        ui.ctx().request_repaint();
                    }
                    ui.end_row();
                }
                row(ui, "Location", path.clone().unwrap_or_else(|| "Not saved yet".into()));
            });
        });
    });
}

/// Commits what the Info page's property fields hold and the presentation does not: a side-menu
/// action such as Save runs before the field that is being left is drawn again (#422).
pub(super) fn commit_props(app: &mut SlideApp, bs: &Backstage) {
    if bs.page != Page::Info {
        return;
    }
    let Ok(v) = app.session.execute("file.info", &json!({})) else { return };
    let mut p = serde_json::Map::new();
    for ((key, _, _), draft) in PROPERTIES.iter().zip(&bs.props) {
        if v["properties"].get(*key).and_then(Value::as_str).unwrap_or("") != *draft {
            p.insert((*key).to_string(), json!(draft));
        }
    }
    if !p.is_empty() {
        let _ = app.run("file.properties", Value::Object(p));
    }
}

/// `1234567` → `1.2 MB`.
pub fn format_bytes(b: u64) -> String {
    match b {
        0..1024 => format!("{b} bytes"),
        1024..1_048_576 => format!("{:.1} KB", b as f64 / 1024.0),
        _ => format!("{:.1} MB", b as f64 / 1_048_576.0),
    }
}

/// Save As: the file type, then the system Save dialog (desktop) or a download (web). A
/// presentation saved as .pptx or .deckcraft takes the new file; the other types are copies.
pub(super) fn save_as(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome) {
    let t = Tokens::get(ui.ctx());
    let (title, path) = app.session.active().map(|d| (d.title(), d.path.clone())).unwrap_or_default();
    ui.label(RichText::new(&title).font(theme::font(15.0)).color(t.text));
    dim(ui, path.as_deref().unwrap_or("Not saved yet"));
    ui.add_space(14.0);
    let current = SAVE_TYPES.iter().find(|s| s.0 == bs.file_type).map_or(SAVE_TYPES[0].1, |s| s.1);
    ui.horizontal(|ui| {
        craft_ui::ComboBox::from_id_salt("backstage_save_type")
            .width(340.0)
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (id, label, _) in SAVE_TYPES {
                    if ui.selectable_label(bs.file_type == id, label).clicked() {
                        bs.file_type = id;
                    }
                }
            })
            .response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Save as type"));
        let web = app.services.pick_save.is_none();
        let label = if web { "Download" } else { "Save" };
        let save = ui.add(egui::Button::new(RichText::new(label).color(t.accent_text)).fill(t.accent).min_size(vec2(84.0, 26.0)));
        if save.on_hover_text(if web { "Download the file" } else { "Choose where to save" }).clicked() {
            let fmt = bs.file_type;
            let done = if matches!(fmt, "pptx" | "deckcraft") {
                let before = app.session.active().map(|d| (d.path.clone(), d.is_dirty()));
                app.save_as_dialog(fmt);
                web || app.session.active().map(|d| (d.path.clone(), d.is_dirty())) != before
            } else {
                app.export_as(fmt, json!({}))
            };
            if done {
                out.close = true;
            }
        }
    });
    ui.add_space(8.0);
    if let Some((_, _, detail)) = SAVE_TYPES.iter().find(|s| s.0 == bs.file_type) {
        dim(ui, detail);
    }
}

/// Account: the interface's appearance and what DeckCraft is. DeckCraft has no accounts, so
/// there is nothing to sign in to.
pub(super) fn account(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(340.0);
            heading(ui, "Appearance");
            let dark = app.ui.brightness == theme::Brightness::Dark;
            craft_ui::ComboBox::from_id_salt("backstage_appearance")
                .width(260.0)
                .selected_text(if dark { "Dark" } else { "Light" })
                .show_ui(ui, |ui| {
                    for (on, label) in [(false, "Light"), (true, "Dark")] {
                        if ui.selectable_label(dark == on, label).clicked() {
                            let _ = app.run("view.dark", json!({"on": on}));
                        }
                    }
                })
                .response
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Appearance"));
            ui.add_space(16.0);
            heading(ui, "User Information");
            dim(ui, "DeckCraft has no accounts: there is nothing to sign in to, and your presentations stay on your device.");
        });
        ui.add_space(40.0);
        ui.vertical(|ui| {
            heading(ui, "Product Information");
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                crate::dialogs::paint_logo(ui.painter(), r);
                ui.label(RichText::new("DeckCraft").font(theme::bold(24.0)).color(t.text));
            });
            dim(ui, &crate::dialogs::version_line());
            dim(ui, "Open source under MIT OR Apache-2.0. Not affiliated with Microsoft.");
            ui.add_space(14.0);
            if task(
                ui,
                Icon::Help,
                "About DeckCraft",
                "About DeckCraft",
                "The version, the people and models who built it, and where to find us.",
                true,
            ) {
                let _ = app.run("app.about", json!({}));
            }
            ui.hyperlink_to("Join our community on Discord", deckcraft_engine::links::DISCORD);
            ui.hyperlink_to("Source on GitHub", deckcraft_engine::links::GITHUB);
        });
    });
}
