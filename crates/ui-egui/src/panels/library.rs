//! Library pane (Insert ▸ Library, #958): a side pane with a strip of tabs. Slides is the first
//! tab: the slides of the presentations in the user's Library folders, as thumbnails grouped by
//! file. Clicking a slide inserts it; its context menu can also put its shapes on the current
//! slide. Everything goes through the `library.*` commands; the pane only keeps what it shows.
//!
//! More tabs (Shapes, Icons, Diagrams, Charts and Pictures from #958, the objects already in the
//! deck from #909) each add a [`Tab`] and an arm in [`show`]; the tab strip and [`State`] stay as
//! they are, and each tab keeps its own state beside the Slides tab's.
//!
//! The folders are read a few files per frame, so a large Library fills in without stalling the
//! window, and only thumbnails in view are drawn.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use egui::{Color32, CornerRadius, Rect, RichText, Role, Sense, Stroke, StrokeKind, TextureOptions, Ui, WidgetInfo, vec2};
use serde_json::{Value, json};

use crate::SlideApp;
use crate::theme::{self, Tokens};

/// The Library pane's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    /// Slides and elements from the presentations in the Library folders.
    #[default]
    Slides,
}

impl Tab {
    /// In the order the strip shows them.
    pub const ALL: &'static [Tab] = &[Tab::Slides];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Slides => "Slides",
        }
    }
}

/// Time spent reading files per frame, in seconds.
const READ_BUDGET: f64 = 0.012;
/// Thumbnails drawn per frame.
const THUMBS_PER_FRAME: usize = 2;
/// Thumbnail textures kept.
const MAX_TEXTURES: usize = 256;
/// Search runs this long after the last key, in seconds.
const SEARCH_DELAY: f64 = 0.25;
/// Files drawn at once; past this the pane asks to choose a folder or search.
const MAX_SHOWN_FILES: usize = 300;
/// What the pane says in the browser build.
pub const NOT_IN_BROWSER: &str = "The Library shows the slides in folders of presentations on this computer, which the browser version can't read. Use the desktop app to browse a Library.";

/// One slide as the pane lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct SlideRow {
    pub index: usize,
    pub title: String,
    pub hidden: bool,
}

/// One presentation; `slides` is `None` until it has been read.
#[derive(Clone, Debug)]
pub struct FileRow {
    pub file: String,
    pub title: String,
    pub slides: Option<Arc<Vec<SlideRow>>>,
    pub error: Option<String>,
}

/// A group of slides under one file's heading.
#[derive(Clone, Debug)]
struct Group {
    file: String,
    title: String,
    error: Option<String>,
    slides: Arc<Vec<SlideRow>>,
}

enum Thumb {
    Ready(egui::TextureHandle),
    Failed,
}

type ThumbKey = (String, usize, u32);

/// Pane-only state: nothing here is part of a document or a preference.
#[derive(Default)]
pub struct State {
    pub tab: Tab,
    /// The folder shown; `None`: every folder.
    pub folder: Option<String>,
    pub query: String,
    /// A folder path typed in (when the host has no folder picker).
    pub new_folder: String,
    /// The path field is shown.
    pub adding: bool,
    /// The Library folders and the folder shown that `files` was listed for.
    listed_for: Option<(Vec<String>, Option<String>)>,
    pub files: Vec<FileRow>,
    /// How many of `files` have been read.
    read: usize,
    /// What `library.search` found for `searched`, grouped by file.
    hits: Option<Vec<Group>>,
    searched: String,
    typed_at: f64,
    thumbs: HashMap<ThumbKey, Thumb>,
    thumb_order: VecDeque<ThumbKey>,
}

impl State {
    /// List and read the folders again, and draw the thumbnails again (after files changed).
    pub fn refresh(&mut self) {
        self.listed_for = None;
        self.hits = None;
        self.searched.clear();
        self.thumbs.clear();
        self.thumb_order.clear();
    }

    /// Every file has been read.
    pub fn ready(&self) -> bool {
        self.listed_for.is_some() && self.read >= self.files.len()
    }

    /// How many slides the last search found, once it has run.
    pub fn hits(&self) -> Option<usize> {
        self.hits.as_ref().map(|g| g.iter().map(|g| g.slides.len()).sum())
    }
}

pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        for &tab in Tab::ALL {
            let r = ui.selectable_label(app.library.tab == tab, tab.label());
            if r.clicked() {
                app.library.tab = tab;
            }
        }
    });
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, t.separator);
    ui.add_space(6.0);
    match app.library.tab {
        Tab::Slides => slides(app, ui),
    }
}

/// The last part of a folder's path, which the drop-down shows.
fn folder_name(path: &str) -> &str {
    path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().filter(|n| !n.is_empty()).unwrap_or(path)
}

fn dim(t: &Tokens, text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(t.text_dim)
}

/// Add Folder…: the host's folder picker, else a field to type the path in.
fn add_folder(app: &mut SlideApp) {
    let picked = app.services.pick_open.as_mut().map(|pick| pick("folder"));
    match picked {
        Some(Some(path)) => {
            if let Ok(v) = app.run("library.addFolder", json!({"path": path})) {
                app.library.folder = v["path"].as_str().map(String::from);
            }
        }
        Some(None) => {}
        None => app.library.adding = true,
    }
}

fn folder_controls(app: &mut SlideApp, ui: &mut Ui) {
    let folders = app.session.prefs.library_folders.clone();
    if app.library.folder.as_ref().is_some_and(|f| !folders.contains(f)) {
        app.library.folder = None;
    }
    let shown = app.library.folder.as_deref().map_or("All folders", folder_name).to_string();
    let mut chosen = app.library.folder.clone();
    let combo = craft_ui::ComboBox::from_id_salt("library_folder").selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
        ui.selectable_value(&mut chosen, None, "All folders");
        for f in &folders {
            ui.selectable_value(&mut chosen, Some(f.clone()), folder_name(f)).on_hover_text(f);
        }
    });
    combo.response.widget_info(|| WidgetInfo::labeled(Role::ComboBox, true, "Library folder"));
    app.library.folder = chosen;
    ui.horizontal(|ui| {
        let full = folders.len() >= deckcraft_engine::library::MAX_FOLDERS;
        let add = ui.add_enabled(!full, egui::Button::new("Add Folder…"));
        if add.on_hover_text("Show the slides of a folder of .pptx, .potx or .deckcraft files").clicked() {
            add_folder(app);
        }
        let remove = ui.add_enabled(app.library.folder.is_some(), egui::Button::new("Remove"));
        if remove.on_hover_text("Stop showing this folder in the Library (its files stay where they are)").clicked()
            && let Some(f) = app.library.folder.take()
        {
            let _ = app.run("library.removeFolder", json!({"path": f}));
        }
        if ui.button("Refresh").on_hover_text("Read the folders again").clicked() {
            app.library.refresh();
        }
    });
    if app.library.adding {
        ui.horizontal(|ui| {
            let field =
                ui.add(egui::TextEdit::singleline(&mut app.library.new_folder).hint_text("Folder path").desired_width(ui.available_width() - 60.0));
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Add").clicked() || enter) && !app.library.new_folder.trim().is_empty() {
                let path = app.library.new_folder.trim().to_string();
                if let Ok(v) = app.run("library.addFolder", json!({"path": path})) {
                    app.library.folder = v["path"].as_str().map(String::from);
                    app.library.new_folder.clear();
                    app.library.adding = false;
                }
            }
        });
    }
}

/// The rows `library.list` answered.
fn rows(v: &Value) -> Vec<FileRow> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|f| FileRow {
                    file: f["file"].as_str().unwrap_or_default().to_string(),
                    title: f["title"].as_str().unwrap_or_default().to_string(),
                    slides: f["slides"].as_array().map(|s| {
                        Arc::new(
                            s.iter()
                                .map(|x| SlideRow {
                                    index: x["index"].as_u64().and_then(|i| usize::try_from(i).ok()).unwrap_or(0),
                                    title: x["title"].as_str().unwrap_or_default().to_string(),
                                    hidden: x["hidden"].as_bool().unwrap_or(false),
                                })
                                .collect(),
                        )
                    }),
                    error: f["error"].as_str().map(String::from),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// List the files of the folder shown (without opening them); they are read a few per frame.
fn list_files(app: &mut SlideApp, key: (Vec<String>, Option<String>)) {
    let mut p = json!({"slides": false});
    if let Some(f) = &key.1 {
        p["folder"] = json!(f);
    }
    let mut files = match app.session.execute("library.list", &p) {
        Ok(v) => rows(&v),
        Err(e) => {
            app.set_status(e.to_string());
            vec![]
        }
    };
    // A file listed with an error (too large) is not opened.
    for f in &mut files {
        if f.error.is_some() {
            f.slides = Some(Arc::default());
        }
    }
    let lib = &mut app.library;
    lib.files = files;
    lib.read = 0;
    lib.listed_for = Some(key);
    lib.hits = None;
    lib.searched.clear();
}

/// Read files until this frame's budget is spent.
fn read_some(app: &mut SlideApp) {
    let start = web_time::Instant::now();
    while app.library.read < app.library.files.len() && start.elapsed().as_secs_f64() < READ_BUDGET {
        let i = app.library.read;
        app.library.read += 1;
        let Some(file) = app.library.files.get(i).filter(|f| f.slides.is_none()).map(|f| f.file.clone()) else { continue };
        let answer = app.session.execute("library.list", &json!({"file": file}));
        let Some(row) = app.library.files.get_mut(i) else { continue };
        match answer.map(|v| rows(&v)) {
            Ok(read) => match read.into_iter().next() {
                Some(r) => {
                    row.title = r.title;
                    row.slides = Some(r.slides.unwrap_or_default());
                    row.error = r.error;
                }
                None => row.slides = Some(Arc::default()),
            },
            Err(e) => {
                row.slides = Some(Arc::default());
                row.error = Some(e.to_string());
            }
        }
    }
}

fn search(app: &mut SlideApp, query: &str) {
    let mut p = json!({"query": query, "limit": 500});
    if let Some(f) = &app.library.folder {
        p["folder"] = json!(f);
    }
    let hits: Vec<(String, usize, String)> = match app.session.execute("library.search", &p) {
        Ok(v) => v
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|h| {
                        let slide = h["slide"].as_u64().and_then(|i| usize::try_from(i).ok())?;
                        Some((h["file"].as_str()?.to_string(), slide, h["title"].as_str().unwrap_or_default().to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Err(e) => {
            app.set_status(e.to_string());
            vec![]
        }
    };
    let groups = hit_groups(&app.library.files, &hits);
    app.library.hits = Some(groups);
    app.library.searched = query.to_string();
}

/// The search hits grouped by file, files in the order they were first found.
fn hit_groups(files: &[FileRow], hits: &[(String, usize, String)]) -> Vec<Group> {
    let rows: HashMap<&str, &FileRow> = files.iter().map(|f| (f.file.as_str(), f)).collect();
    let mut groups: Vec<(Group, Vec<SlideRow>)> = vec![];
    let mut at: HashMap<&str, usize> = HashMap::new();
    for (file, slide, title) in hits {
        let row = rows.get(file.as_str()).copied();
        let hidden = row.and_then(|r| r.slides.as_ref()).and_then(|s| s.iter().find(|x| x.index == *slide)).is_some_and(|x| x.hidden);
        let entry = SlideRow { index: *slide, title: title.clone(), hidden };
        match at.get(file.as_str()).and_then(|i| groups.get_mut(*i)) {
            Some((_, slides)) => slides.push(entry),
            None => {
                at.insert(file.as_str(), groups.len());
                let title = row.map(|r| r.title.clone()).unwrap_or_else(|| file.rsplit(['/', '\\']).next().unwrap_or(file).to_string());
                groups.push((Group { file: file.clone(), title, error: None, slides: Arc::default() }, vec![entry]));
            }
        }
    }
    groups.into_iter().map(|(g, slides)| Group { slides: Arc::new(slides), ..g }).collect()
}

fn slides(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    if cfg!(target_arch = "wasm32") {
        ui.label(dim(&t, NOT_IN_BROWSER));
        return;
    }
    folder_controls(app, ui);
    let folders = app.session.prefs.library_folders.clone();
    if folders.is_empty() {
        ui.add_space(6.0);
        ui.label(dim(&t, "Add a folder of .pptx, .potx or .deckcraft presentations to see their slides here. Click a slide to insert it."));
        return;
    }
    ui.add_space(4.0);
    let field = ui.add(egui::TextEdit::singleline(&mut app.library.query).hint_text("Search slides").desired_width(f32::INFINITY));
    if field.changed() {
        app.library.typed_at = ui.input(|i| i.time);
    }
    let key = (folders, app.library.folder.clone());
    if app.library.listed_for.as_ref() != Some(&key) {
        list_files(app, key);
    }
    read_some(app);
    let reading = !app.library.ready();
    if reading {
        let (n, total) = (app.library.read, app.library.files.len());
        ui.label(dim(&t, format!("Reading {n} of {total} files…")));
        ui.ctx().request_repaint();
    }
    let query = app.library.query.trim().to_string();
    let groups: Vec<Group> = if query.is_empty() {
        app.library
            .files
            .iter()
            .filter_map(|f| {
                let slides = f.slides.clone()?;
                Some(Group { file: f.file.clone(), title: f.title.clone(), error: f.error.clone(), slides })
            })
            .take(MAX_SHOWN_FILES)
            .collect()
    } else {
        if !reading && query != app.library.searched {
            let waited = ui.input(|i| i.time) - app.library.typed_at;
            if waited >= SEARCH_DELAY {
                search(app, &query);
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(SEARCH_DELAY - waited));
            }
        }
        match app.library.hits.as_ref().filter(|_| app.library.searched == query) {
            Some(groups) => {
                if groups.is_empty() {
                    ui.label(dim(&t, format!("No slides match \"{query}\".")));
                }
                groups.clone()
            }
            None => vec![],
        }
    };
    if !reading && query.is_empty() && groups.is_empty() {
        ui.label(dim(&t, "No presentations in this folder."));
    }
    if query.is_empty() && app.library.files.len() > MAX_SHOWN_FILES {
        let n = app.library.files.len();
        ui.label(dim(&t, format!("Showing the first {MAX_SHOWN_FILES} of {n} files: choose a folder or search to see the others.")));
    }
    let mut budget = THUMBS_PER_FRAME;
    for g in &groups {
        group(app, ui, g, &mut budget);
    }
    if budget == 0 {
        ui.ctx().request_repaint();
    }
}

fn group(app: &mut SlideApp, ui: &mut Ui, g: &Group, budget: &mut usize) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(6.0);
    ui.label(RichText::new(&g.title).font(theme::bold(13.0))).on_hover_text(&g.file);
    if let Some(e) = &g.error {
        ui.label(RichText::new(e).color(t.text_dim).small());
        return;
    }
    if g.slides.is_empty() {
        ui.label(dim(&t, "No slides."));
        return;
    }
    let gap = 6.0;
    let w = ((ui.available_width() - gap) / 2.0).floor().max(48.0);
    // Pixels for the screen, in steps of 32 so nearby pane widths share thumbnails.
    let px = ((w * ui.ctx().pixels_per_point()) / 32.0).ceil() as u32 * 32;
    let px = px.clamp(*deckcraft_engine::library::THUMB_WIDTHS.start(), *deckcraft_engine::library::THUMB_WIDTHS.end());
    for pair in g.slides.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for s in pair {
                cell(app, ui, &g.file, s, w, px, budget);
            }
        });
    }
}

/// The accessible name of a slide's cell.
pub fn cell_label(s: &SlideRow) -> String {
    if s.title.is_empty() { format!("Slide {}", s.index + 1) } else { format!("Slide {}: {}", s.index + 1, s.title) }
}

fn cell(app: &mut SlideApp, ui: &mut Ui, file: &str, s: &SlideRow, w: f32, px: u32, budget: &mut usize) {
    let t = Tokens::get(ui.ctx());
    let key: ThumbKey = (file.to_string(), s.index, px);
    let aspect = match app.library.thumbs.get(&key) {
        Some(Thumb::Ready(tex)) => {
            let size = tex.size_vec2();
            if size.x > 0.0 { (size.y / size.x).clamp(0.1, 4.0) } else { 0.5625 }
        }
        _ => 0.5625,
    };
    let h = w * aspect;
    let label_h = 16.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h + label_h), Sense::click());
    let label = cell_label(s);
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
    let img = Rect::from_min_size(rect.min, vec2(w, h));
    if ui.is_rect_visible(rect) {
        if !app.library.thumbs.contains_key(&key) && *budget > 0 {
            *budget -= 1;
            load_thumb(app, ui.ctx(), &key);
        }
        let p = ui.painter();
        let tint = if s.hidden { Color32::from_white_alpha(110) } else { Color32::WHITE };
        match app.library.thumbs.get(&key) {
            Some(Thumb::Ready(tex)) => {
                p.image(tex.id(), img, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
            }
            Some(Thumb::Failed) => {
                p.rect_filled(img, CornerRadius::same(2), t.control);
                p.text(img.center(), egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(16.0), t.text_dim);
            }
            None => {
                p.rect_filled(img, CornerRadius::same(2), t.control);
            }
        }
        let edge = if resp.hovered() { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.border) };
        p.rect_stroke(img, CornerRadius::same(2), edge, StrokeKind::Inside);
        let text = if s.title.is_empty() { format!("{}", s.index + 1) } else { format!("{}  {}", s.index + 1, s.title) };
        let galley = p.layout_no_wrap(text, egui::FontId::proportional(11.0), if s.hidden { t.text_faint } else { t.text_dim });
        let at = egui::pos2(rect.left(), img.bottom() + 2.0);
        p.with_clip_rect(Rect::from_min_max(at, rect.max)).galley(at, galley, t.text_dim);
    }
    let hint = if s.hidden { format!("{label} (hidden)\nClick to insert") } else { format!("{label}\nClick to insert") };
    let resp = resp.on_hover_text(hint);
    if resp.clicked() {
        let _ = app.run("library.insert", json!({"file": file, "slide": s.index}));
    }
    let on_slide = app.session.active().is_some_and(|d| d.current_slide().is_some());
    resp.context_menu(|ui| {
        if ui.button("Insert slide").clicked() {
            let _ = app.run("library.insert", json!({"file": file, "slide": s.index}));
            ui.close();
        }
        if ui.add_enabled(on_slide, egui::Button::new("Insert shapes on current slide")).clicked() {
            let _ = app.run("library.insert", json!({"file": file, "slide": s.index, "as": "shapes"}));
            ui.close();
        }
    });
}

/// Draw a thumbnail through `library.thumbnail` into a texture; one that can't be drawn is
/// remembered as failed, so it isn't asked for every frame.
fn load_thumb(app: &mut SlideApp, ctx: &egui::Context, key: &ThumbKey) {
    let answer = app.session.execute("library.thumbnail", &json!({"file": key.0, "slide": key.1, "width": key.2}));
    let thumb = answer
        .ok()
        .and_then(|v| v["png"].as_str().and_then(deckcraft_engine::cmd::base64_decode))
        .and_then(|png| deckcraft_render::decode_image(&png))
        .map(|img| {
            let (w, h) = img.dimensions();
            let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
            Thumb::Ready(ctx.load_texture(format!("library {} {} {}", key.0, key.1, key.2), image, TextureOptions::LINEAR))
        })
        .unwrap_or(Thumb::Failed);
    let lib = &mut app.library;
    lib.thumbs.insert(key.clone(), thumb);
    lib.thumb_order.push_back(key.clone());
    while lib.thumb_order.len() > MAX_TEXTURES {
        if let Some(old) = lib.thumb_order.pop_front() {
            lib.thumbs.remove(&old);
        }
    }
}

#[cfg(test)]
mod ui_tests;
