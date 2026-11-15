//! The Library pane the Craft apps share (#958), in egui: a strip of tabs, then the items of the
//! documents in the user's Library folders (DeckCraft's slides) as thumbnails grouped by file, with
//! a folder drop-down (Add Folder…, Remove, Refresh) and a search box. Clicking an item inserts it;
//! its context menu can also put its shapes, or one named element, where the person is working.
//!
//! The pane holds no document and reads no file: it speaks only the `library.*` commands, through
//! the app's [`Host`]. The app also gives its words ([`Labels`], whose nouns are the commands'
//! keys) and colours ([`Palette`]), and may add tabs of its own beside the shared one ([`ITEMS`]),
//! which it draws in [`Host::tab`].
//!
//! Files are read a few per frame, so a large Library fills in without stalling the window, only
//! thumbnails in view are drawn, and at most [`MAX_SHOWN_FILES`] files are laid out at once.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use craft_library::Nouns;
use egui::{Color32, CornerRadius, Rect, RichText, Role, Sense, Stroke, StrokeKind, TextureOptions, Ui, WidgetInfo, vec2};
use serde_json::{Value, json};

/// The shared tab's id: the items of the Library's documents.
pub const ITEMS: &str = "items";

/// Time spent reading files per frame, in seconds.
const READ_BUDGET: f64 = 0.012;
/// Thumbnails drawn per frame.
const THUMBS_PER_FRAME: usize = 2;
/// Thumbnail textures kept.
const MAX_TEXTURES: usize = 256;
/// Search runs this long after the last key, in seconds.
const SEARCH_DELAY: f64 = 0.25;
/// Files laid out at once; past this the pane asks to choose a folder or search.
pub const MAX_SHOWN_FILES: usize = 300;
/// Elements listed in an item's Insert Element menu, and items whose lists are kept.
const MAX_MENU_ELEMENTS: usize = 40;
const MAX_ELEMENT_LISTS: usize = 64;
/// The largest thumbnail decoded, in pixels.
const MAX_THUMB_PIXELS: u64 = 512 * 2048;

/// What the pane needs from the app.
pub trait Host {
    /// Run a `library.*` command the person asked for (an insert, adding or removing a folder);
    /// the host shows a failure itself.
    fn act(&mut self, id: &str, params: Value) -> Result<Value, String>;
    /// Run a reading `library.*` command (list, search, thumbnail, shapes) quietly.
    fn ask(&mut self, id: &str, params: Value) -> Result<Value, String>;
    /// The Library folders, as the preferences keep them.
    fn folders(&self) -> Vec<String>;
    /// The host's folder picker: `None` when it has none (the pane then takes a typed path),
    /// `Some(None)` when the person cancelled it.
    fn pick_folder(&mut self) -> Option<Option<String>>;
    /// Is there somewhere to put shapes (a current slide, page or sheet)?
    fn can_place(&self) -> bool;
    /// Tell the person something (the status bar).
    fn report(&mut self, message: String);
    /// Draw the app's own tab `id` (one of [`Labels::tabs`] other than [`ITEMS`]).
    fn tab(&mut self, _ui: &mut Ui, _id: &str) {}
}

/// The app's words. `nouns` also name the commands' keys (`slide`, `slides`).
#[derive(Clone, Debug)]
pub struct Labels {
    pub nouns: Nouns,
    /// The tab strip, in order: `(id, label)`; [`ITEMS`] is the shared tab.
    pub tabs: &'static [(&'static str, &'static str)],
    /// An item's accessible name before its number: `Slide` gives "Slide 2: Agenda".
    pub item: &'static str,
    /// "Insert slide".
    pub insert_item: &'static str,
    /// "Insert shapes on current slide"; `None` when the app has no such insert.
    pub insert_shapes: Option<&'static str>,
    /// "Insert element"; `None` when the app has no named elements.
    pub insert_element: Option<&'static str>,
    pub search_hint: &'static str,
    /// The tooltip of Add Folder….
    pub add_folder_tip: &'static str,
    /// What the pane says with no folders yet.
    pub empty: &'static str,
    /// What the pane says in a browser build.
    pub not_in_browser: &'static str,
    /// An item's height over its width, before its thumbnail arrives.
    pub aspect: f32,
}

/// The app's colours.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub separator: Color32,
    /// An item's background before its thumbnail arrives.
    pub control: Color32,
    pub border: Color32,
    pub accent: Color32,
}

impl Palette {
    /// Colours from egui's own visuals, for an app without theme tokens.
    pub fn from_visuals(v: &egui::Visuals) -> Self {
        Palette {
            text_dim: v.weak_text_color(),
            text_faint: v.weak_text_color().gamma_multiply(0.7),
            separator: v.widgets.noninteractive.bg_stroke.color,
            control: v.faint_bg_color,
            border: v.widgets.noninteractive.bg_stroke.color,
            accent: v.selection.stroke.color,
        }
    }
}

/// One item as the pane lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemRow {
    pub index: usize,
    pub title: String,
    pub hidden: bool,
}

/// One document; `items` is `None` until it has been read.
#[derive(Clone, Debug)]
pub struct FileRow {
    pub file: String,
    pub title: String,
    pub items: Option<Arc<Vec<ItemRow>>>,
    pub error: Option<String>,
}

/// Items under one file's heading.
#[derive(Clone, Debug)]
struct Group {
    file: String,
    title: String,
    error: Option<String>,
    items: Arc<Vec<ItemRow>>,
}

enum Thumb {
    Ready(egui::TextureHandle),
    Failed,
}

type ThumbKey = (String, usize, u32);

/// Pane-only state: nothing here is part of a document or a preference.
#[derive(Default)]
pub struct State {
    /// The tab shown (empty: the first).
    pub tab: String,
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
    /// An item's top-level shape names and how many shapes carry each, for Insert Element.
    elements: HashMap<(String, usize), Vec<(String, usize)>>,
}

impl State {
    /// List and read the folders again, and draw the thumbnails again (after files changed).
    pub fn refresh(&mut self) {
        self.listed_for = None;
        self.hits = None;
        self.searched.clear();
        self.thumbs.clear();
        self.thumb_order.clear();
        self.elements.clear();
    }

    /// Every file has been read.
    pub fn ready(&self) -> bool {
        self.listed_for.is_some() && self.read >= self.files.len()
    }

    /// How many items the last search found, once it has run.
    pub fn hits(&self) -> Option<usize> {
        self.hits.as_ref().map(|g| g.iter().map(|g| g.items.len()).sum())
    }
}

/// Draw the pane.
pub fn show(ui: &mut Ui, st: &mut State, host: &mut dyn Host, labels: &Labels, pal: &Palette) {
    if !labels.tabs.iter().any(|(id, _)| *id == st.tab) {
        st.tab = labels.tabs.first().map(|(id, _)| (*id).to_string()).unwrap_or_else(|| ITEMS.to_string());
    }
    ui.horizontal(|ui| {
        for (id, label) in labels.tabs {
            if ui.selectable_label(st.tab == *id, *label).clicked() {
                st.tab = (*id).to_string();
            }
        }
    });
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, pal.separator);
    ui.add_space(6.0);
    if st.tab == ITEMS {
        items(ui, st, host, labels, pal);
    } else {
        let tab = st.tab.clone();
        host.tab(ui, &tab);
    }
}

/// The last part of a folder's path, which the drop-down shows.
fn folder_name(path: &str) -> &str {
    path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().filter(|n| !n.is_empty()).unwrap_or(path)
}

fn dim(pal: &Palette, text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(pal.text_dim)
}

/// Add Folder…: the host's folder picker, else a field to type the path in.
fn add_folder(st: &mut State, host: &mut dyn Host) {
    match host.pick_folder() {
        Some(Some(path)) => {
            if let Ok(v) = host.act("library.addFolder", json!({"path": path})) {
                st.folder = v["path"].as_str().map(String::from);
            }
        }
        Some(None) => {}
        None => st.adding = true,
    }
}

fn folder_controls(ui: &mut Ui, st: &mut State, host: &mut dyn Host, labels: &Labels) {
    let folders = host.folders();
    if st.folder.as_ref().is_some_and(|f| !folders.contains(f)) {
        st.folder = None;
    }
    let shown = st.folder.as_deref().map_or("All folders", folder_name).to_string();
    let mut chosen = st.folder.clone();
    let combo = craft_ui::ComboBox::from_id_salt("library_folder").selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
        ui.selectable_value(&mut chosen, None, "All folders");
        for f in &folders {
            ui.selectable_value(&mut chosen, Some(f.clone()), folder_name(f)).on_hover_text(f);
        }
    });
    combo.response.widget_info(|| WidgetInfo::labeled(Role::ComboBox, true, "Library folder"));
    st.folder = chosen;
    ui.horizontal(|ui| {
        let full = folders.len() >= craft_library::MAX_FOLDERS;
        let add = ui.add_enabled(!full, egui::Button::new("Add Folder…"));
        if add.on_hover_text(labels.add_folder_tip).clicked() {
            add_folder(st, host);
        }
        let remove = ui.add_enabled(st.folder.is_some(), egui::Button::new("Remove"));
        if remove.on_hover_text("Stop showing this folder in the Library (its files stay where they are)").clicked()
            && let Some(f) = st.folder.take()
        {
            let _ = host.act("library.removeFolder", json!({"path": f}));
        }
        if ui.button("Refresh").on_hover_text("Read the folders again").clicked() {
            st.refresh();
        }
    });
    if st.adding {
        ui.horizontal(|ui| {
            let field = ui.add(egui::TextEdit::singleline(&mut st.new_folder).hint_text("Folder path").desired_width(ui.available_width() - 60.0));
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Add").clicked() || enter) && !st.new_folder.trim().is_empty() {
                let path = st.new_folder.trim().to_string();
                if let Ok(v) = host.act("library.addFolder", json!({"path": path})) {
                    st.folder = v["path"].as_str().map(String::from);
                    st.new_folder.clear();
                    st.adding = false;
                }
            }
        });
    }
}

/// The rows `library.list` answered.
fn rows(v: &Value, nouns: Nouns) -> Vec<FileRow> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|f| FileRow {
                    file: f["file"].as_str().unwrap_or_default().to_string(),
                    title: f["title"].as_str().unwrap_or_default().to_string(),
                    items: f[nouns.many].as_array().map(|s| {
                        Arc::new(
                            s.iter()
                                .map(|x| ItemRow {
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
fn list_files(st: &mut State, host: &mut dyn Host, nouns: Nouns, key: (Vec<String>, Option<String>)) {
    let mut p = json!({});
    p[nouns.many] = json!(false);
    if let Some(f) = &key.1 {
        p["folder"] = json!(f);
    }
    let mut files = match host.ask("library.list", p) {
        Ok(v) => rows(&v, nouns),
        Err(e) => {
            host.report(e);
            vec![]
        }
    };
    // A file listed with an error (too large) is not opened.
    for f in &mut files {
        if f.error.is_some() {
            f.items = Some(Arc::default());
        }
    }
    st.files = files;
    st.read = 0;
    st.listed_for = Some(key);
    st.hits = None;
    st.searched.clear();
}

/// Read files until this frame's budget is spent.
fn read_some(st: &mut State, host: &mut dyn Host, nouns: Nouns) {
    let start = web_time::Instant::now();
    while st.read < st.files.len() && start.elapsed().as_secs_f64() < READ_BUDGET {
        let i = st.read;
        st.read += 1;
        let Some(file) = st.files.get(i).filter(|f| f.items.is_none()).map(|f| f.file.clone()) else { continue };
        let answer = host.ask("library.list", json!({"file": file}));
        let Some(row) = st.files.get_mut(i) else { continue };
        match answer.map(|v| rows(&v, nouns)) {
            Ok(read) => match read.into_iter().next() {
                Some(r) => {
                    row.title = r.title;
                    row.items = Some(r.items.unwrap_or_default());
                    row.error = r.error;
                }
                None => row.items = Some(Arc::default()),
            },
            Err(e) => {
                row.items = Some(Arc::default());
                row.error = Some(e);
            }
        }
    }
}

fn search(st: &mut State, host: &mut dyn Host, nouns: Nouns, query: &str) {
    let mut p = json!({"query": query, "limit": 500});
    if let Some(f) = &st.folder {
        p["folder"] = json!(f);
    }
    let hits: Vec<(String, usize, String)> = match host.ask("library.search", p) {
        Ok(v) => v
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|h| {
                        let item = h[nouns.one].as_u64().and_then(|i| usize::try_from(i).ok())?;
                        Some((h["file"].as_str()?.to_string(), item, h["title"].as_str().unwrap_or_default().to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Err(e) => {
            host.report(e);
            vec![]
        }
    };
    st.hits = Some(hit_groups(&st.files, &hits));
    st.searched = query.to_string();
}

/// The search hits grouped by file, files in the order they were first found.
fn hit_groups(files: &[FileRow], hits: &[(String, usize, String)]) -> Vec<Group> {
    let rows: HashMap<&str, &FileRow> = files.iter().map(|f| (f.file.as_str(), f)).collect();
    let mut groups: Vec<(Group, Vec<ItemRow>)> = vec![];
    let mut at: HashMap<&str, usize> = HashMap::new();
    for (file, item, title) in hits {
        let row = rows.get(file.as_str()).copied();
        let hidden = row.and_then(|r| r.items.as_ref()).and_then(|s| s.iter().find(|x| x.index == *item)).is_some_and(|x| x.hidden);
        let entry = ItemRow { index: *item, title: title.clone(), hidden };
        match at.get(file.as_str()).and_then(|i| groups.get_mut(*i)) {
            Some((_, items)) => items.push(entry),
            None => {
                at.insert(file.as_str(), groups.len());
                let title = row.map(|r| r.title.clone()).unwrap_or_else(|| file.rsplit(['/', '\\']).next().unwrap_or(file).to_string());
                groups.push((Group { file: file.clone(), title, error: None, items: Arc::default() }, vec![entry]));
            }
        }
    }
    groups.into_iter().map(|(g, items)| Group { items: Arc::new(items), ..g }).collect()
}

fn items(ui: &mut Ui, st: &mut State, host: &mut dyn Host, labels: &Labels, pal: &Palette) {
    if cfg!(target_arch = "wasm32") {
        ui.label(dim(pal, labels.not_in_browser));
        return;
    }
    folder_controls(ui, st, host, labels);
    let folders = host.folders();
    if folders.is_empty() {
        ui.add_space(6.0);
        ui.label(dim(pal, labels.empty));
        return;
    }
    ui.add_space(4.0);
    let field = ui.add(egui::TextEdit::singleline(&mut st.query).hint_text(labels.search_hint).desired_width(f32::INFINITY));
    if field.changed() {
        st.typed_at = ui.input(|i| i.time);
    }
    let key = (folders, st.folder.clone());
    if st.listed_for.as_ref() != Some(&key) {
        list_files(st, host, labels.nouns, key);
    }
    read_some(st, host, labels.nouns);
    let reading = !st.ready();
    if reading {
        ui.label(dim(pal, format!("Reading {} of {} files…", st.read, st.files.len())));
        ui.ctx().request_repaint();
    }
    let query = st.query.trim().to_string();
    let groups: Vec<Group> = if query.is_empty() {
        st.files
            .iter()
            .filter_map(|f| Some(Group { file: f.file.clone(), title: f.title.clone(), error: f.error.clone(), items: f.items.clone()? }))
            .take(MAX_SHOWN_FILES)
            .collect()
    } else {
        if !reading && query != st.searched {
            let waited = ui.input(|i| i.time) - st.typed_at;
            if waited >= SEARCH_DELAY {
                search(st, host, labels.nouns, &query);
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(SEARCH_DELAY - waited));
            }
        }
        match st.hits.as_ref().filter(|_| st.searched == query) {
            Some(groups) => {
                if groups.is_empty() {
                    ui.label(dim(pal, format!("No {} match \"{query}\".", labels.nouns.many)));
                }
                groups.clone()
            }
            None => vec![],
        }
    };
    if !reading && query.is_empty() && groups.is_empty() {
        ui.label(dim(pal, "Nothing in this folder yet."));
    }
    if query.is_empty() && st.files.len() > MAX_SHOWN_FILES {
        let n = st.files.len();
        ui.label(dim(pal, format!("Showing the first {MAX_SHOWN_FILES} of {n} files: choose a folder or search to see the others.")));
    }
    let mut budget = THUMBS_PER_FRAME;
    for g in &groups {
        group(ui, st, host, labels, pal, g, &mut budget);
    }
    if budget == 0 {
        ui.ctx().request_repaint();
    }
}

fn group(ui: &mut Ui, st: &mut State, host: &mut dyn Host, labels: &Labels, pal: &Palette, g: &Group, budget: &mut usize) {
    ui.add_space(6.0);
    ui.label(RichText::new(&g.title).strong()).on_hover_text(&g.file);
    if let Some(e) = &g.error {
        ui.label(RichText::new(e).color(pal.text_dim).small());
        return;
    }
    if g.items.is_empty() {
        ui.label(dim(pal, format!("No {}.", labels.nouns.many)));
        return;
    }
    let gap = 6.0;
    let w = ((ui.available_width() - gap) / 2.0).floor().max(48.0);
    // Pixels for the screen, in steps of 32 so nearby pane widths share thumbnails.
    let px = ((w * ui.ctx().pixels_per_point()) / 32.0).ceil() as u32 * 32;
    let px = px.clamp(*craft_library::THUMB_WIDTHS.start(), *craft_library::THUMB_WIDTHS.end());
    for pair in g.items.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for it in pair {
                cell(ui, st, host, labels, pal, &g.file, it, w, px, budget);
            }
        });
    }
}

/// The accessible name of an item's cell: "Slide 2: Agenda".
pub fn cell_label(item: &str, row: &ItemRow) -> String {
    if row.title.is_empty() { format!("{item} {}", row.index + 1) } else { format!("{item} {}: {}", row.index + 1, row.title) }
}

#[allow(clippy::too_many_arguments)]
fn cell(
    ui: &mut Ui,
    st: &mut State,
    host: &mut dyn Host,
    labels: &Labels,
    pal: &Palette,
    file: &str,
    row: &ItemRow,
    w: f32,
    px: u32,
    budget: &mut usize,
) {
    let key: ThumbKey = (file.to_string(), row.index, px);
    let aspect = match st.thumbs.get(&key) {
        Some(Thumb::Ready(tex)) => {
            let size = tex.size_vec2();
            if size.x > 0.0 { (size.y / size.x).clamp(0.1, 4.0) } else { labels.aspect }
        }
        _ => labels.aspect,
    };
    let h = w * aspect;
    let label_h = 16.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h + label_h), Sense::click());
    let label = cell_label(labels.item, row);
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
    let img = Rect::from_min_size(rect.min, vec2(w, h));
    if ui.is_rect_visible(rect) {
        if !st.thumbs.contains_key(&key) && *budget > 0 {
            *budget -= 1;
            load_thumb(st, host, ui.ctx(), labels.nouns, &key);
        }
        let p = ui.painter();
        let tint = if row.hidden { Color32::from_white_alpha(110) } else { Color32::WHITE };
        match st.thumbs.get(&key) {
            Some(Thumb::Ready(tex)) => {
                p.image(tex.id(), img, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
            }
            Some(Thumb::Failed) => {
                p.rect_filled(img, CornerRadius::same(2), pal.control);
                p.text(img.center(), egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(16.0), pal.text_dim);
            }
            None => {
                p.rect_filled(img, CornerRadius::same(2), pal.control);
            }
        }
        let edge = if resp.hovered() { Stroke::new(2.0, pal.accent) } else { Stroke::new(1.0, pal.border) };
        p.rect_stroke(img, CornerRadius::same(2), edge, StrokeKind::Inside);
        let text = if row.title.is_empty() { format!("{}", row.index + 1) } else { format!("{}  {}", row.index + 1, row.title) };
        let galley = p.layout_no_wrap(text, egui::FontId::proportional(11.0), if row.hidden { pal.text_faint } else { pal.text_dim });
        let at = egui::pos2(rect.left(), img.bottom() + 2.0);
        p.with_clip_rect(Rect::from_min_max(at, rect.max)).galley(at, galley, pal.text_dim);
    }
    let hint = if row.hidden { format!("{label} (hidden)\nClick to insert") } else { format!("{label}\nClick to insert") };
    let resp = resp.on_hover_text(hint);
    let one = labels.nouns.one;
    let target = |extra: Value| {
        let mut p = json!({"file": file});
        p[one] = json!(row.index);
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        p
    };
    if resp.clicked() {
        let _ = host.act("library.insert", target(json!({})));
    }
    let place = host.can_place();
    resp.context_menu(|ui| {
        if ui.button(labels.insert_item).clicked() {
            let _ = host.act("library.insert", target(json!({})));
            ui.close();
        }
        if let Some(shapes) = labels.insert_shapes
            && ui.add_enabled(place, egui::Button::new(shapes)).clicked()
        {
            let _ = host.act("library.insert", target(json!({"as": "shapes"})));
            ui.close();
        }
        if let Some(element) = labels.insert_element {
            let names = element_names(st, host, labels.nouns, file, row.index);
            ui.add_enabled_ui(place && !names.is_empty(), |ui| {
                ui.menu_button(element, |ui| {
                    for (name, count) in names.iter().take(MAX_MENU_ELEMENTS) {
                        let shown = if *count > 1 { format!("{name} (×{count})") } else { name.clone() };
                        if ui.button(shown).on_hover_text("Put this element where it is on this one").clicked() {
                            let _ = host.act("library.insert", target(json!({"names": [name]})));
                            ui.close();
                        }
                    }
                    if names.len() > MAX_MENU_ELEMENTS {
                        ui.label(dim(pal, format!("and {} more", names.len() - MAX_MENU_ELEMENTS)));
                    }
                });
            });
        }
    });
}

/// The names of an item's top-level shapes that Insert Element offers (not empty placeholders),
/// each with how many shapes carry it; asked for once per item.
fn element_names(st: &mut State, host: &mut dyn Host, nouns: Nouns, file: &str, index: usize) -> Vec<(String, usize)> {
    let key = (file.to_string(), index);
    if let Some(names) = st.elements.get(&key) {
        return names.clone();
    }
    let mut p = json!({"file": file});
    p[nouns.one] = json!(index);
    let mut names: Vec<(String, usize)> = vec![];
    if let Ok(v) = host.ask("library.shapes", p) {
        for sh in v["shapes"].as_array().into_iter().flatten() {
            let name = sh["name"].as_str().unwrap_or_default().trim();
            if name.is_empty() || sh.get("parent").is_some() || sh["empty"].as_bool() == Some(true) {
                continue;
            }
            match names.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                Some((_, c)) => *c += 1,
                None => names.push((name.to_string(), 1)),
            }
        }
    }
    if st.elements.len() >= MAX_ELEMENT_LISTS {
        st.elements.clear();
    }
    st.elements.insert(key, names.clone());
    names
}

/// The PNG of a thumbnail as an image egui can draw, when it is a sane size.
fn decode(png: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_THUMB_PIXELS {
        return None;
    }
    Some(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw()))
}

/// The base64 in a `library.thumbnail` answer, decoded.
fn unbase64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

/// Draw a thumbnail through `library.thumbnail` into a texture; one that can't be drawn is
/// remembered as failed, so it isn't asked for every frame.
fn load_thumb(st: &mut State, host: &mut dyn Host, ctx: &egui::Context, nouns: Nouns, key: &ThumbKey) {
    let mut p = json!({"file": key.0, "width": key.2});
    p[nouns.one] = json!(key.1);
    let thumb = host
        .ask("library.thumbnail", p)
        .ok()
        .and_then(|v| v["png"].as_str().and_then(unbase64))
        .and_then(|png| decode(&png))
        .map(|image| Thumb::Ready(ctx.load_texture(format!("library {} {} {}", key.0, key.1, key.2), image, TextureOptions::LINEAR)))
        .unwrap_or(Thumb::Failed);
    st.thumbs.insert(key.clone(), thumb);
    st.thumb_order.push_back(key.clone());
    while st.thumb_order.len() > MAX_TEXTURES {
        if let Some(old) = st.thumb_order.pop_front() {
            st.thumbs.remove(&old);
        }
    }
}

#[cfg(test)]
mod tests;
