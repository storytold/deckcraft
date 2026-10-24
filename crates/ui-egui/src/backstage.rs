//! The File tab (Backstage, #76): a full-window view that replaces the ribbon, slides and status
//! bar until Esc or the back arrow returns to the presentation. With nothing open it is the start
//! screen: PowerPoint's start screen and File › Home are the same page.
//!
//! A side menu on the left picks a page (Home, New, Open, Info, Save As, Print, Export, Account);
//! Save, Close and Options act at once. Every action runs an existing command through
//! [`SlideApp::run`] or one of the app's file helpers ([`SlideApp::export_as`],
//! [`SlideApp::print_pdf`], [`SlideApp::save_as_dialog`]); the state here is UI-only and never
//! saved. Recent presentations are listed by path: nothing on disk is read until one is picked.
//!
//! Laid out after PowerPoint for Windows (observed black-box): a 160-point side menu darker than
//! the page, 40-point entries with an outline icon, the current page in the accent colour.

use egui::{Align2, Color32, CornerRadius, Key, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::SlideApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// Width of the side menu.
pub const NAV_W: f32 = 160.0;
/// Height of a side-menu entry (the back arrow's row is a little taller).
const ITEM_H: f32 = 40.0;
const BACK_H: f32 = 46.0;
/// The widest a page's content grows.
const CONTENT_W: f32 = 1180.0;

/// A page of the File tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Home,
    New,
    Open,
    Info,
    SaveAs,
    Print,
    Export,
    Account,
    /// Settings: interface themes, keyboard shortcuts (#127) and Reset, one tab each.
    Settings,
}

impl Page {
    pub const ALL: [Page; 9] =
        [Page::Home, Page::New, Page::Open, Page::Info, Page::SaveAs, Page::Print, Page::Export, Page::Account, Page::Settings];

    /// The id `app.backstage {page}` takes.
    pub fn id(self) -> &'static str {
        match self {
            Page::Home => "home",
            Page::New => "new",
            Page::Open => "open",
            Page::Info => "info",
            Page::SaveAs => "saveAs",
            Page::Print => "print",
            Page::Export => "export",
            Page::Account => "account",
            Page::Settings => "settings",
        }
    }

    pub fn parse(id: &str) -> Option<Page> {
        let id = id.trim();
        Page::ALL.into_iter().find(|p| p.id().eq_ignore_ascii_case(id))
    }

    /// Side-menu label and page heading.
    pub fn label(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::New => "New",
            Page::Open => "Open",
            Page::Info => "Info",
            Page::SaveAs => "Save As",
            Page::Print => "Print",
            Page::Export => "Export",
            Page::Account => "Account",
            Page::Settings => "Settings",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Page::Home => Icon::Home,
            Page::New => Icon::NewFile,
            Page::Open => Icon::OpenFolder,
            Page::Info => Icon::Info,
            Page::SaveAs => Icon::SaveAs,
            Page::Print => Icon::Print,
            Page::Export => Icon::Export,
            Page::Account => Icon::Account,
            Page::Settings => Icon::Settings,
        }
    }

    /// Pages about the open presentation (hidden on the start screen).
    pub fn needs_doc(self) -> bool {
        matches!(self, Page::Info | Page::SaveAs | Page::Print | Page::Export)
    }
}

/// A tab of File › Settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Themes,
    NewPresentations,
    Shortcuts,
    Backup,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 5] =
        [SettingsTab::General, SettingsTab::Themes, SettingsTab::NewPresentations, SettingsTab::Shortcuts, SettingsTab::Backup];

    /// The id `app.backstage {page: "settings", tab}` takes.
    pub fn id(self) -> &'static str {
        match self {
            SettingsTab::General => "general",
            SettingsTab::Themes => "themes",
            SettingsTab::NewPresentations => "new",
            SettingsTab::Shortcuts => "shortcuts",
            SettingsTab::Backup => "backup",
        }
    }

    pub fn parse(id: &str) -> Option<SettingsTab> {
        if id.eq_ignore_ascii_case("reset") {
            return Some(SettingsTab::Backup);
        }
        SettingsTab::ALL.into_iter().find(|t| t.id().eq_ignore_ascii_case(id))
    }

    pub fn label(self) -> &'static str {
        match self {
            SettingsTab::General => "General",
            SettingsTab::Themes => "Themes",
            SettingsTab::NewPresentations => "New Presentations",
            SettingsTab::Shortcuts => "Keyboard Shortcuts",
            SettingsTab::Backup => "Backup and Reset",
        }
    }

    fn icon(self) -> Icon {
        match self {
            SettingsTab::General => Icon::Settings,
            SettingsTab::Themes => Icon::Palette,
            SettingsTab::NewPresentations => Icon::NewFile,
            SettingsTab::Shortcuts => Icon::Keyboard,
            SettingsTab::Backup => Icon::Save,
        }
    }
}

/// What each Settings tab holds, for its search box: (tab, setting, more words that find it).
const SETTINGS_INDEX: &[(SettingsTab, &str, &str)] = &[
    (SettingsTab::General, "Interface size", "zoom scale bigger smaller larger text accessibility"),
    (SettingsTab::General, "Reduce motion", "animation sliding fading accessibility"),
    (SettingsTab::General, "When DeckCraft starts", "startup launch open last presentation blank start screen"),
    (SettingsTab::Themes, "Light or dark", "dark mode night appearance"),
    (SettingsTab::Themes, "Interface theme", "colour color high contrast lagoon sunset custom accent tint appearance"),
    (SettingsTab::NewPresentations, "Theme for new presentations", "design default template"),
    (SettingsTab::NewPresentations, "Slide size", "widescreen 16:9 standard 4:3 aspect"),
    (SettingsTab::NewPresentations, "Font for new presentations", "typeface default text"),
    (SettingsTab::NewPresentations, "Default shape", "set as default shape look style"),
    (SettingsTab::Shortcuts, "Keyboard shortcuts", "keys hotkeys keybindings customise customize"),
    (SettingsTab::Shortcuts, "Shortcut cheat sheet", "print save copy list"),
    (SettingsTab::Backup, "Back up settings", "export save file another computer"),
    (SettingsTab::Backup, "Restore settings", "import load file another computer"),
    (SettingsTab::Backup, "Reset all settings", "factory defaults"),
];

/// The settings whose name, tab or extra words contain every word of `query`.
pub fn settings_search(query: &str) -> Vec<(SettingsTab, &'static str)> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return vec![];
    }
    SETTINGS_INDEX
        .iter()
        .filter(|(tab, name, more)| {
            let hay = format!("{} {name} {more}", tab.label()).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .map(|(tab, name, _)| (*tab, *name))
        .collect()
}

/// A side-menu entry: a page, or an action that runs at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Page(Page),
    Save,
    Close,
    Options,
}

impl Nav {
    fn label(self) -> &'static str {
        match self {
            Nav::Page(p) => p.label(),
            Nav::Save => "Save",
            Nav::Close => "Close",
            Nav::Options => "Options",
        }
    }
    fn icon(self) -> Icon {
        match self {
            Nav::Page(p) => p.icon(),
            Nav::Save => Icon::Save,
            Nav::Close => Icon::File,
            Nav::Options => Icon::Settings,
        }
    }
}

/// The side menu, top group then bottom group, in keyboard order. PowerPoint's Share (cloud) is
/// left out; with nothing open only Home, New and Open stay at the top.
pub fn entries(has_doc: bool) -> (Vec<Nav>, Vec<Nav>) {
    let mut top = vec![Nav::Page(Page::Home), Nav::Page(Page::New), Nav::Page(Page::Open)];
    if has_doc {
        top.extend([Nav::Page(Page::Info), Nav::Save, Nav::Page(Page::SaveAs), Nav::Page(Page::Print), Nav::Page(Page::Export), Nav::Close]);
    }
    (top, vec![Nav::Page(Page::Account), Nav::Page(Page::Settings), Nav::Options])
}

/// Print page: which slides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrintScope {
    All,
    Current,
    Custom,
}

/// Export page: the chosen task on the left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportTask {
    Pdf,
    Handouts,
    FileType,
}

/// The File tab's state while it is open.
#[derive(Clone, Debug)]
pub struct Backstage {
    pub page: Page,
    /// The open presentations (count, active) when it opened: a change closes it.
    pub docs: (usize, Option<usize>),
    /// Keyboard focus in the side menu (an index into [`entries`], top then bottom).
    pub focus: Option<usize>,
    /// Search text on Home, New and Open.
    pub query: String,
    /// Info: property drafts while a field is being edited.
    props: Vec<String>,
    pub print_scope: PrintScope,
    pub print_range: String,
    /// `slides`, `notes`, or `h1`…`h9` (handouts with that many slides a page).
    pub print_layout: &'static str,
    pub print_hidden: bool,
    pub print_frame: bool,
    /// Notes and handouts: a paper id from [`deckcraft_engine::cmd::file::PAPERS`] and its orientation.
    pub print_paper: &'static str,
    pub print_landscape: bool,
    pub preview_page: usize,
    pub export: ExportTask,
    /// Export › Change File Type / Save As: the chosen type (an [`SAVE_TYPES`] id).
    pub file_type: &'static str,
    pub handouts_per_page: u8,
    pub pdf_hidden: bool,
    /// Settings: the open tab.
    pub settings_tab: SettingsTab,
    /// Settings: the search box above the tabs.
    pub settings_query: String,
    /// Keyboard Shortcuts: the slide-show list instead of the editor's.
    pub keys_show: bool,
    /// Keyboard Shortcuts: only actions that have a key.
    pub keys_bound_only: bool,
    /// Keyboard Shortcuts: the action whose next key press is being recorded. While it is, the
    /// app's own shortcuts are off.
    pub recording: Option<String>,
    /// Keyboard Shortcuts: a recorded key another action has (action, keys it would get, message).
    pub clash: Option<(String, Vec<String>, String)>,
    /// Keyboard Shortcuts: why the last recorded key was refused.
    pub key_note: Option<String>,
    /// Settings › Reset: Reset all settings asked to be confirmed.
    pub confirm_reset: bool,
    /// Export › Change File Type › PNG: leave the slide background out (#784).
    pub png_transparent: bool,
}

impl Backstage {
    pub fn new(page: Page, docs: (usize, Option<usize>)) -> Backstage {
        Backstage {
            page,
            docs,
            focus: None,
            query: String::new(),
            props: vec![String::new(); PROPERTIES.len()],
            print_scope: PrintScope::All,
            print_range: String::new(),
            print_layout: "slides",
            print_hidden: false,
            print_frame: true,
            print_paper: "letter",
            print_landscape: false,
            preview_page: 0,
            export: ExportTask::Pdf,
            file_type: "pptx",
            handouts_per_page: 3,
            pdf_hidden: false,
            settings_tab: SettingsTab::General,
            settings_query: String::new(),
            keys_show: false,
            keys_bound_only: true,
            recording: None,
            clash: None,
            key_note: None,
            confirm_reset: false,
            png_transparent: false,
        }
    }

    /// Show `page`, as a click in the side menu does.
    pub fn go(&mut self, page: Page) {
        if self.page != page {
            self.query.clear();
            self.preview_page = 0;
            self.recording = None;
            self.clash = None;
            self.key_note = None;
            self.confirm_reset = false;
        }
        self.page = page;
    }

    /// Show Settings' `tab`, as a click in its list does.
    pub fn open_tab(&mut self, tab: SettingsTab) {
        if self.settings_tab != tab {
            self.query.clear();
            self.recording = None;
            self.clash = None;
            self.key_note = None;
            self.confirm_reset = false;
        }
        self.settings_tab = tab;
    }
}

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
const PROPERTIES: [(&str, &str, &str); 7] = [
    ("title", "Title", "Add a title"),
    ("subject", "Subject", "Specify the subject"),
    ("keywords", "Tags", "Add a tag"),
    ("category", "Categories", "Add a category"),
    ("company", "Company", "Specify the company"),
    ("comments", "Comments", "Add comments"),
    ("author", "Author", "Add an author"),
];

/// What a click asked for, applied after drawing.
#[derive(Default)]
struct Outcome {
    close: bool,
    page: Option<Page>,
}

/// Colours of the side menu and the page, from the theme: the menu sits a step darker than the
/// page, as in PowerPoint.
fn colours(t: &Tokens) -> (Color32, Color32) {
    (t.chrome, t.card)
}

/// Runs a side-menu action. Returns the page to show, or None to close the view (Some(current)
/// leaves it as it is).
fn act(app: &mut SlideApp, nav: Nav, current: Page) -> Option<Page> {
    match nav {
        Nav::Page(p) => Some(p),
        Nav::Save => {
            let web = app.services.pick_save.is_none() && app.services.download.is_some();
            let has_path = app.session.active().is_some_and(|d| d.path.is_some());
            if !has_path && !web {
                // A presentation that was never saved goes to Save As first, as in PowerPoint.
                return Some(Page::SaveAs);
            }
            app.save();
            let saved = app.session.active().is_some_and(|d| !d.is_dirty());
            if saved || web { None } else { Some(current) }
        }
        Nav::Close => {
            app.close_presentation();
            Some(current)
        }
        Nav::Options => {
            let _ = app.run("app.preferences", json!({}));
            Some(current)
        }
    }
}

/// The File tab's own keys: Esc goes back to the presentation; Up and Down walk the side menu
/// (a page shows as soon as it has the focus, as in PowerPoint); Enter or Space runs the focused
/// action. True when the key was used.
pub fn key(app: &mut SlideApp, key: Key, mods: deckcraft_engine::Mods) -> bool {
    if mods.cmd || mods.alt || mods.shift {
        return false;
    }
    let has_doc = app.session.active().is_some();
    let (top, bottom) = entries(has_doc);
    let all: Vec<Nav> = top.into_iter().chain(bottom).collect();
    let Some(bs) = app.backstage.as_mut() else { return false };
    match key {
        Key::Escape => {
            app.close_backstage();
            true
        }
        Key::ArrowDown | Key::ArrowUp => {
            let cur = bs.focus.or_else(|| all.iter().position(|n| *n == Nav::Page(bs.page)));
            let n = all.len();
            let next = match (cur, key == Key::ArrowDown) {
                (None, _) => 0,
                (Some(i), true) => (i + 1) % n,
                (Some(i), false) => (i + n - 1) % n,
            };
            bs.focus = Some(next);
            if let Some(Nav::Page(p)) = all.get(next) {
                bs.go(*p);
            }
            true
        }
        Key::Enter | Key::Space => {
            let Some(nav) = bs.focus.and_then(|i| all.get(i)).copied() else { return false };
            let current = bs.page;
            match act(app, nav, current) {
                None => app.close_backstage(),
                Some(p) => {
                    if let Some(bs) = app.backstage.as_mut() {
                        bs.go(p);
                    }
                }
            }
            true
        }
        _ => false,
    }
}

/// Draws the File tab in place of the ribbon, slides and status bar.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let Some(mut bs) = app.backstage.take() else { return };
    let t = Tokens::get(ui.ctx());
    let has_doc = app.session.active().is_some();
    if !has_doc && bs.page.needs_doc() {
        bs.page = Page::Home;
    }
    let (nav_bg, page_bg) = colours(&t);
    let mut out = Outcome::default();
    let (top, bottom) = entries(has_doc);
    egui::Panel::left("backstage_nav").exact_size(NAV_W).resizable(false).frame(egui::Frame::NONE.fill(nav_bg)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        if has_doc {
            let (r, resp) = ui.allocate_exact_size(vec2(NAV_W, BACK_H), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Back"));
            let hovered = resp.hovered();
            if hovered {
                ui.painter().rect_filled(Rect::from_center_size(pos2(r.left() + 26.0, r.center().y), vec2(34.0, 34.0)), 17.0, t.hover);
            }
            icons::paint(
                ui.painter(),
                Rect::from_center_size(pos2(r.left() + 26.0, r.center().y), vec2(22.0, 22.0)),
                Icon::BackCircle,
                t.text,
                false,
            );
            if resp.on_hover_text("Back (Esc)").clicked() {
                out.close = true;
            }
        } else {
            ui.add_space(BACK_H);
        }
        let mut index = 0;
        for (k, nav) in top.iter().enumerate() {
            if k == 3 {
                separator(ui, &t);
            }
            if nav_item(ui, *nav, &bs, index, &t).clicked() {
                bs.focus = None;
                commit_props(app, &bs);
                match act(app, *nav, bs.page) {
                    None => out.close = true,
                    Some(p) => out.page = Some(p),
                }
            }
            index += 1;
        }
        let first_bottom = index;
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(8.0);
            // Bottom-up: draw the last entry first.
            for (k, nav) in bottom.iter().enumerate().rev() {
                if nav_item(ui, *nav, &bs, first_bottom + k, &t).clicked() {
                    bs.focus = None;
                    commit_props(app, &bs);
                    match act(app, *nav, bs.page) {
                        None => out.close = true,
                        Some(p) => out.page = Some(p),
                    }
                }
            }
            separator(ui, &t);
        });
    });
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(page_bg)).show(ui, |ui| {
        // Fields, boxes and buttons sit on the page's own colour: outline them, as PowerPoint does.
        ui.visuals_mut().text_edit_bg_color = Some(t.field_bg);
        let w = &mut ui.visuals_mut().widgets;
        w.inactive.bg_fill = t.field_bg;
        w.inactive.weak_bg_fill = t.field_bg;
        w.inactive.bg_stroke = Stroke::new(1.0, t.border);
        w.hovered.bg_stroke = Stroke::new(1.0, t.accent);
        w.active.bg_stroke = Stroke::new(1.0, t.accent);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::NONE.inner_margin(egui::Margin { left: 48, right: 36, top: 22, bottom: 32 }).show(ui, |ui| {
                ui.set_max_width(CONTENT_W);
                if bs.page != Page::Home {
                    ui.label(RichText::new(bs.page.label()).font(theme::bold(22.0)).color(t.text));
                    ui.add_space(16.0);
                }
                // Errors raised on a backstage page (a missing recent file, a failed open) would
                // otherwise vanish: the status bar only draws once the backstage is closed (#172).
                // A real label, not painted text, so screen readers see it too.
                if let Some(msg) = app.fresh_status() {
                    ui.label(RichText::new(&msg).font(theme::font(12.0)).color(t.accent));
                    ui.add_space(8.0);
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
                }
                match bs.page {
                    Page::Home => home(app, ui, &mut bs, &mut out),
                    Page::New => new_page(app, ui, &mut bs),
                    Page::Open => open_page(app, ui, &mut bs),
                    Page::Info => info(app, ui, &mut bs),
                    Page::SaveAs => save_as(app, ui, &mut bs, &mut out),
                    Page::Print => print(app, ui, &mut bs),
                    Page::Export => export(app, ui, &mut bs),
                    Page::Account => account(app, ui),
                    Page::Settings => settings(app, ui, &mut bs),
                }
            });
        });
    });
    if let Some(page) = out.page {
        bs.go(page);
    }
    // A command run from a page may have closed the view itself (a new presentation); keep that.
    if out.close && has_doc {
        app.backstage = None;
    } else if app.backstage.is_none() {
        app.backstage = Some(bs);
    }
}

// ---------------------------------------------------------------- side menu

fn nav_item(ui: &mut Ui, nav: Nav, bs: &Backstage, index: usize, t: &Tokens) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(NAV_W, ITEM_H), Sense::click());
    let selected = nav == Nav::Page(bs.page);
    let label = nav.label();
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label));
    let p = ui.painter();
    let row = rect.shrink2(vec2(6.0, 1.0));
    if resp.hovered() {
        p.rect_filled(row, CornerRadius::same(2), t.hover);
    }
    if bs.focus == Some(index) || selected {
        // The current page (and the keyboard focus) is outlined, as in PowerPoint.
        let ink = if bs.focus == Some(index) { t.text } else { t.border };
        p.rect_stroke(row, CornerRadius::same(2), Stroke::new(1.0, ink), StrokeKind::Inside);
    }
    let ink = if selected { t.accent } else { t.text };
    icons::paint(p, Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), vec2(16.0, 16.0)), nav.icon(), ink, false);
    p.text(pos2(rect.left() + 44.0, rect.center().y), Align2::LEFT_CENTER, label, theme::font(13.0), ink);
    resp
}

fn separator(ui: &mut Ui, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(NAV_W, 13.0), Sense::hover());
    let y = rect.center().y;
    ui.painter().line_segment([pos2(rect.left() + 20.0, y), pos2(rect.right() - 14.0, y)], Stroke::new(1.0, t.separator));
}

// ---------------------------------------------------------------- shared pieces

fn heading(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(text).font(theme::bold(16.0)).color(t.text));
    ui.add_space(8.0);
}

fn dim(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(text).font(theme::font(12.5)).color(t.text_dim));
}

/// A link with an arrow (More themes →): true when clicked.
fn arrow_link(ui: &mut Ui, text: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    ui.add(egui::Label::new(RichText::new(format!("{text}  →")).font(theme::font(12.5)).color(t.accent)).sense(Sense::click()))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// A search box; true when its text changed.
fn search(ui: &mut Ui, query: &mut String, hint: &str, width: f32) -> bool {
    ui.add(egui::TextEdit::singleline(query).hint_text(hint).desired_width(width)).changed()
}

/// A new-presentation tile: a theme preview drawn in code (or a blank slide) over its name.
fn tile(ui: &mut Ui, label: &str, theme: Option<&deckcraft_model::Theme>, big: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let thumb_w = if big { 200.0 } else { 160.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(thumb_w + 16.0, thumb_w * 9.0 / 16.0 + 44.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(4), t.hover);
    }
    let thumb = Rect::from_min_size(rect.min + vec2(8.0, 10.0), vec2(thumb_w, thumb_w * 9.0 / 16.0));
    match theme {
        Some(th) => crate::ribbon::paint_theme_tile(p, thumb, th),
        None => {
            p.rect_filled(thumb, CornerRadius::same(2), Color32::WHITE);
            p.rect_stroke(thumb, CornerRadius::same(2), Stroke::new(1.0, Color32::from_gray(200)), StrokeKind::Inside);
        }
    }
    if resp.hovered() {
        p.rect_stroke(thumb, CornerRadius::same(2), Stroke::new(2.0, t.accent), StrokeKind::Outside);
    }
    let galley = p.layout(label.to_string(), theme::font(12.0), t.text, rect.width() - 8.0);
    p.galley(pos2(rect.center().x - galley.size().x / 2.0, thumb.bottom() + 8.0), galley, t.text);
    resp
}

/// The built-in themes and the sample deck, filtered by `query` (after a blank presentation when
/// `blank`); at most `limit` theme tiles. False when nothing matched.
fn new_tiles(app: &mut SlideApp, ui: &mut Ui, query: &str, limit: usize, blank: bool) -> bool {
    let q = query.trim().to_lowercase();
    let themes: Vec<deckcraft_model::Theme> =
        deckcraft_model::theme::builtin_themes().into_iter().filter(|th| q.is_empty() || th.name.to_lowercase().contains(&q)).collect();
    let sample = q.is_empty() || "sample deck northern lights".contains(&q);
    if themes.is_empty() && !sample {
        return false;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        if blank && tile(ui, "Blank Presentation", None, false).clicked() {
            let _ = app.run("file.new", json!({}));
        }
        let mut shown = 0;
        for th in themes {
            if shown >= limit {
                break;
            }
            if tile(ui, &th.name, Some(&th), false).clicked() {
                let _ = app.run("file.new", json!({"theme": th.name}));
            }
            shown += 1;
        }
        if sample && shown < limit {
            let th = deckcraft_model::theme::builtin_themes().into_iter().find(|t| t.name == "DeckCraft");
            if tile(ui, "Sample deck: Northern Lights", th.as_ref(), false).clicked()
                && let Err(e) = deckcraft_engine::sample::open_sample(&mut app.session)
            {
                app.set_status(e.to_string());
            }
        }
    });
    true
}

/// Starter decks whose name, summary, theme or a slide title matches `query`, at most `limit` tiles.
fn template_tiles(app: &mut SlideApp, ui: &mut Ui, query: &str, limit: usize) -> bool {
    let list = deckcraft_engine::templates::matching(query);
    if list.is_empty() {
        return false;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        for starter in list.into_iter().take(limit) {
            let theme = deckcraft_model::theme::builtin_themes().into_iter().find(|th| th.name == starter.theme);
            if tile(ui, starter.name, theme.as_ref(), false).on_hover_text(starter.summary).clicked() {
                let _ = app.run("file.new", json!({"template": starter.name}));
            }
        }
    });
    true
}

/// File name and folder of a recent path, split as text (nothing on disk is read).
pub fn split_path(path: &str) -> (String, String) {
    let p = std::path::Path::new(path);
    let name = p.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(path).to_string();
    let folder = p.parent().and_then(std::path::Path::to_str).unwrap_or("").to_string();
    (name, folder)
}

/// Recent presentations whose name matches `query`, newest first, at most `limit`. Opening one is
/// the user's explicit choice.
fn recent_list(app: &mut SlideApp, ui: &mut Ui, query: &str, limit: usize) {
    let t = Tokens::get(ui.ctx());
    if app.ui.recent.is_empty() {
        dim(ui, "Presentations you open show up here. Use Open to browse for one.");
        return;
    }
    let q = query.trim().to_lowercase();
    let list: Vec<String> =
        app.ui.recent.iter().filter(|p| q.is_empty() || split_path(p).0.to_lowercase().contains(&q)).take(limit).cloned().collect();
    if list.is_empty() {
        dim(ui, &format!("No recent presentations match “{}”.", query.trim()));
        return;
    }
    let width = ui.available_width().min(CONTENT_W);
    for path in list {
        let missing = !app.recent_exists(&path);
        let (name, folder) = split_path(&path);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 44.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(rect, CornerRadius::same(3), t.hover);
        }
        let ink = if missing { t.text_dim } else { t.text };
        // The row is as wide as the page's content, which a narrow window clips: keep the mark
        // and the remove button inside the visible part of the row.
        let visible_right = rect.right().min(ui.clip_rect().right() - 6.0);
        {
            let p = ui.painter();
            icons::paint(p, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(22.0, 22.0)), Icon::Slide, ink, false);
            p.text(pos2(rect.left() + 44.0, rect.center().y - 8.0), Align2::LEFT_CENTER, &name, theme::font(13.0), ink);
            p.text(pos2(rect.left() + 44.0, rect.center().y + 9.0), Align2::LEFT_CENTER, &folder, theme::font(11.0), t.text_dim);
            // A missing file says so before it is clicked (#172).
            if missing {
                p.text(pos2(visible_right - 82.0, rect.center().y), Align2::RIGHT_CENTER, "(missing)", theme::font(11.0), t.text_dim);
            }
        }
        let resp = if missing { resp.on_hover_text(format!("{path} — this file can't be found")) } else { resp.on_hover_text(&path) };
        // Remove from the list: a real labelled widget on top of the row, with a code-drawn icon
        // (a text glyph would need a font that has it).
        let x = ui.allocate_rect(
            Rect::from_min_max(pos2(visible_right - 32.0, rect.top() + 5.0), pos2(visible_right, rect.bottom() - 5.0)),
            Sense::click(),
        );
        x.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Remove from Recent"));
        if x.hovered() {
            ui.painter().rect_filled(x.rect, CornerRadius::same(3), t.hover);
        }
        icons::paint(ui.painter(), Rect::from_center_size(x.rect.center(), vec2(14.0, 14.0)), Icon::Close, t.text_dim, false);
        if x.on_hover_text("Remove from Recent").clicked() {
            app.forget_recent(&path);
        }
        if resp.clicked() {
            app.open_path_in_background(&path);
        }
    }
}

/// A large square command button (Print, Create PDF, Check for Issues…).
fn square_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(88.0, 80.0), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(3), if resp.hovered() && enabled { t.hover } else { t.card });
    p.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, if resp.hovered() && enabled { t.accent } else { t.border }), StrokeKind::Inside);
    icons::paint(p, Rect::from_center_size(pos2(rect.center().x, rect.top() + 26.0), vec2(26.0, 26.0)), icon, t.text, !enabled);
    let ink = if enabled { t.text } else { t.text_faint };
    let galley = p.layout(label.to_string(), theme::font(12.0), ink, rect.width() - 8.0);
    p.galley(pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 46.0), galley, ink);
    resp
}

/// A row of a page's own list on the left (Open's places, Export's tasks).
fn place_row(ui: &mut Ui, icon: Icon, label: &str, selected: bool, width: f32) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 44.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label));
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CornerRadius::same(2), t.selected);
        p.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, t.border), StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(2), t.hover);
    }
    icons::paint(p, Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), vec2(24.0, 24.0)), icon, t.text, false);
    p.text(pos2(rect.left() + 46.0, rect.center().y), Align2::LEFT_CENTER, label, theme::font(13.5), t.text);
    resp
}

/// A square button with its title and description beside it (Info, Export).
fn task(ui: &mut Ui, icon: Icon, button: &str, title: &str, detail: &str, enabled: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut clicked = false;
    ui.horizontal_top(|ui| {
        clicked = square_button(ui, icon, button, enabled).clicked();
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(title).font(theme::font(15.0)).color(t.text));
            dim(ui, detail);
        });
    });
    ui.add_space(14.0);
    clicked
}

// ---------------------------------------------------------------- pages

/// Home: DeckCraft's mark, new presentations (the first themes, with a link to New) and recent
/// presentations (with a link to Open). The same page is the start screen.
fn home(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
        crate::dialogs::paint_logo(ui.painter(), r);
        ui.label(RichText::new("DeckCraft").font(theme::bold(24.0)).color(t.text));
    });
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("New").font(theme::bold(16.0)).color(t.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if arrow_link(ui, "More themes") {
                out.page = Some(Page::New);
            }
        });
    });
    ui.add_space(6.0);
    new_tiles(app, ui, "", 5, true);
    ui.add_space(22.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Templates").font(theme::bold(16.0)).color(t.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if arrow_link(ui, "More templates") {
                out.page = Some(Page::New);
            }
        });
    });
    ui.add_space(6.0);
    template_tiles(app, ui, "", 3);
    ui.add_space(22.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Recent").font(theme::bold(16.0)).color(t.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            search(ui, &mut bs.query, "Search for a file", 280.0);
        });
    });
    ui.add_space(6.0);
    recent_list(app, ui, &bs.query.clone(), 8);
    if app.ui.recent.len() > 8 {
        ui.add_space(6.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            if arrow_link(ui, "More presentations") {
                out.page = Some(Page::Open);
            }
        });
    }
    ui.add_space(30.0);
    ui.hyperlink_to("Join the ArtCraft community on Discord", deckcraft_engine::links::DISCORD);
}

/// New: a blank presentation, then every theme and starter template, with a search box.
fn new_page(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
    let t = Tokens::get(ui.ctx());
    if tile(ui, "Blank Presentation", None, true).clicked() {
        let _ = app.run("file.new", json!({}));
    }
    ui.add_space(8.0);
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, t.separator));
    ui.add_space(14.0);
    search(ui, &mut bs.query, "Search for themes and templates", 380.0);
    ui.add_space(14.0);
    let q = bs.query.clone();
    let themes = new_tiles(app, ui, &q, usize::MAX, false);
    let templates = if q.trim().is_empty() || !deckcraft_engine::templates::matching(&q).is_empty() {
        ui.add_space(18.0);
        heading(ui, "Templates");
        template_tiles(app, ui, &q, usize::MAX)
    } else {
        false
    };
    if !themes && !templates {
        dim(ui, &format!("No themes or templates match “{}”.", q.trim()));
    }
}

/// Open: Recent and Browse on the left; the recent presentations (with a search box) on the right.
fn open_page(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(230.0);
            place_row(ui, Icon::Recent, "Recent", true, 220.0);
            if place_row(ui, Icon::OpenFolder, "Browse", false, 220.0).on_hover_text("Open a presentation from a folder (Ctrl+O)").clicked() {
                let _ = app.run("app.openDialog", json!({}));
            }
            if app.session.recovery_dir.is_some() {
                ui.add_space(24.0);
                if ui.button("Recover Unsaved Presentations").clicked() {
                    match app.run("file.recovery.open", json!({})) {
                        Ok(v) if v.get("opened").and_then(Value::as_array).is_some_and(|a| a.is_empty()) => {
                            app.set_status("There are no unsaved presentations to recover");
                        }
                        _ => {}
                    }
                }
            }
        });
        let h = ui.available_height().max(320.0);
        let (r, _) = ui.allocate_exact_size(vec2(1.0, h), Sense::hover());
        ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, t.separator));
        ui.add_space(24.0);
        ui.vertical(|ui| {
            search(ui, &mut bs.query, "Search for a file", 380.0);
            ui.add_space(12.0);
            heading(ui, "Presentations");
            let q = bs.query.clone();
            recent_list(app, ui, &q, usize::MAX);
        });
    });
}

/// Info: the presentation's name and location, Inspect and Manage on the left, its properties
/// (editable, through `file.properties`) on the right.
fn info(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
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
fn commit_props(app: &mut SlideApp, bs: &Backstage) {
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
fn save_as(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome) {
    let t = Tokens::get(ui.ctx());
    let (title, path) = app.session.active().map(|d| (d.title(), d.path.clone())).unwrap_or_default();
    ui.label(RichText::new(&title).font(theme::font(15.0)).color(t.text));
    dim(ui, path.as_deref().unwrap_or("Not saved yet"));
    ui.add_space(14.0);
    let current = SAVE_TYPES.iter().find(|s| s.0 == bs.file_type).map_or(SAVE_TYPES[0].1, |s| s.1);
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("backstage_save_type").width(340.0).selected_text(current).show_ui(ui, |ui| {
            for (id, label, _) in SAVE_TYPES {
                if ui.selectable_label(bs.file_type == id, label).clicked() {
                    bs.file_type = id;
                }
            }
        });
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

/// The slides the Print page prints, in order (0-based).
pub fn print_slides(doc: &deckcraft_model::Presentation, bs: &Backstage, current: usize) -> Result<Vec<usize>, String> {
    let n = doc.slides.len();
    let mut v: Vec<usize> = match bs.print_scope {
        PrintScope::All => (0..n).collect(),
        PrintScope::Current => vec![current.min(n.saturating_sub(1))],
        PrintScope::Custom => deckcraft_engine::cmd::file::parse_slide_range(&bs.print_range, n).map_err(|e| e.to_string())?,
    };
    if !bs.print_hidden && bs.print_scope != PrintScope::Current {
        v.retain(|i| doc.slides.get(*i).is_some_and(|s| !s.hidden));
    }
    Ok(v)
}

/// Handout slides per page for a `print_layout` id (`h3` → 3).
fn per_page(layout: &str) -> usize {
    layout.strip_prefix('h').and_then(|n| n.parse::<usize>().ok()).filter(|n| matches!(n, 1 | 2 | 3 | 4 | 6 | 9)).unwrap_or(1)
}

/// The `file.export` / `app.print` params for the Print page's settings.
pub fn print_params(bs: &Backstage, slides: &[usize]) -> Value {
    let (layout, per) = match bs.print_layout {
        "notes" => ("notes", 0),
        l if l.starts_with('h') => ("handouts", per_page(l)),
        _ => ("slides", 0),
    };
    // The list is already filtered for hidden slides, so the PDF takes it as it is.
    json!({
        "layout": layout,
        "perPage": per,
        "slides": slides,
        "includeHidden": true,
        "frame": bs.print_frame,
        "paper": bs.print_paper,
        "orientation": if bs.print_landscape { "landscape" } else { "portrait" },
    })
}

/// The printed page size in points: the slide size for full-page slides, else the chosen paper.
pub fn print_page_size(bs: &Backstage, doc: &deckcraft_model::Presentation) -> (f32, f32) {
    if bs.print_layout == "slides" {
        return (doc.slide_size.width as f32, doc.slide_size.height as f32);
    }
    let (w, h) = deckcraft_engine::cmd::file::paper_size(bs.print_paper, bs.print_landscape).unwrap_or((612.0, 792.0));
    (w as f32, h as f32)
}

const LAYOUTS: [(&str, &str); 8] = [
    ("slides", "Full Page Slides"),
    ("notes", "Notes Pages"),
    ("h1", "Handouts (1 slide per page)"),
    ("h2", "Handouts (2 slides per page)"),
    ("h3", "Handouts (3 slides per page)"),
    ("h4", "Handouts (4 slides per page)"),
    ("h6", "Handouts (6 slides per page)"),
    ("h9", "Handouts (9 slides per page)"),
];

/// Print: the Print button and settings on the left, a preview of each page on the right.
fn print(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
    let t = Tokens::get(ui.ctx());
    let Some((doc, current)) = app.session.active().map(|d| (d.doc.clone(), d.selection.slide)) else {
        dim(ui, "No presentation is open.");
        return;
    };
    let slides = print_slides(&doc, bs, current);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(250.0);
            let ok = slides.as_ref().is_ok_and(|s| !s.is_empty());
            if square_button(ui, Icon::Print, "Print", ok).clicked()
                && let Ok(s) = &slides
            {
                app.print_pdf(print_params(bs, s));
            }
            ui.add_space(6.0);
            dim(
                ui,
                if app.services.pick_save.is_some() {
                    "DeckCraft prints through a PDF: it opens in your PDF viewer, where you choose the printer."
                } else {
                    "DeckCraft prints through a PDF: it downloads, ready to print from your browser or PDF viewer."
                },
            );
            ui.add_space(14.0);
            heading(ui, "Settings");
            let scope = match bs.print_scope {
                PrintScope::All => "Print All Slides",
                PrintScope::Current => "Print Current Slide",
                PrintScope::Custom => "Custom Range",
            };
            egui::ComboBox::from_id_salt("backstage_print_scope").width(240.0).selected_text(scope).show_ui(ui, |ui| {
                for (s, label) in
                    [(PrintScope::All, "Print All Slides"), (PrintScope::Current, "Print Current Slide"), (PrintScope::Custom, "Custom Range")]
                {
                    if ui.selectable_label(bs.print_scope == s, label).clicked() {
                        bs.print_scope = s;
                        bs.preview_page = 0;
                    }
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let l = ui.label("Slides:");
                let r = ui.add(egui::TextEdit::singleline(&mut bs.print_range).hint_text("e.g. 1-3, 5").desired_width(176.0)).labelled_by(l.id);
                if r.changed() {
                    // Typing a range picks Custom Range, as in PowerPoint.
                    bs.print_scope = if bs.print_range.trim().is_empty() { PrintScope::All } else { PrintScope::Custom };
                    bs.preview_page = 0;
                }
            });
            ui.add_space(4.0);
            let layout = LAYOUTS.iter().find(|l| l.0 == bs.print_layout).map_or("Full Page Slides", |l| l.1);
            egui::ComboBox::from_id_salt("backstage_print_layout").width(240.0).selected_text(layout).show_ui(ui, |ui| {
                for (id, label) in LAYOUTS {
                    if ui.selectable_label(bs.print_layout == id, label).clicked() {
                        bs.print_layout = id;
                        bs.preview_page = 0;
                    }
                }
            });
            ui.add_space(4.0);
            // PowerPoint prints full-page slides on slide-shaped pages; paper and orientation are
            // for notes pages and handouts.
            ui.add_enabled_ui(bs.print_layout != "slides", |ui| {
                let paper = deckcraft_engine::cmd::file::PAPERS.iter().find(|p| p.0 == bs.print_paper).map_or("Letter", |p| p.1);
                egui::ComboBox::from_id_salt("backstage_print_paper").width(240.0).selected_text(paper).show_ui(ui, |ui| {
                    for (id, label, _) in deckcraft_engine::cmd::file::PAPERS {
                        if ui.selectable_label(bs.print_paper == id, label).clicked() {
                            bs.print_paper = id;
                        }
                    }
                });
                ui.add_space(4.0);
                let orient = if bs.print_landscape { "Landscape Orientation" } else { "Portrait Orientation" };
                egui::ComboBox::from_id_salt("backstage_print_orientation").width(240.0).selected_text(orient).show_ui(ui, |ui| {
                    for (land, label) in [(false, "Portrait Orientation"), (true, "Landscape Orientation")] {
                        if ui.selectable_label(bs.print_landscape == land, label).clicked() {
                            bs.print_landscape = land;
                        }
                    }
                });
            });
            ui.add_space(4.0);
            if ui.checkbox(&mut bs.print_hidden, "Print Hidden Slides").changed() {
                bs.preview_page = 0;
            }
            ui.checkbox(&mut bs.print_frame, "Frame Slides");
            ui.add_space(6.0);
            if ui.link("Edit Header & Footer").clicked() {
                let _ = app.run("app.dialog", json!({"id": "headerFooter"}));
            }
        });
        ui.add_space(20.0);
        let (r, _) = ui.allocate_exact_size(vec2(1.0, 560.0), Sense::hover());
        ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, t.separator));
        ui.add_space(20.0);
        ui.vertical(|ui| match &slides {
            Err(e) => dim(ui, e),
            Ok(s) if s.is_empty() => dim(ui, "Nothing to print: every slide in the range is hidden."),
            Ok(s) => preview(app, ui, bs, &doc, s),
        });
    });
}

/// One printed page in miniature, with a pager under it.
fn preview(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage, doc: &deckcraft_model::Presentation, slides: &[usize]) {
    let t = Tokens::get(ui.ctx());
    let per = match bs.print_layout {
        "slides" | "notes" => 1,
        l => per_page(l),
    };
    let pages: Vec<&[usize]> = slides.chunks(per.max(1)).collect();
    let last = pages.len().saturating_sub(1);
    bs.preview_page = bs.preview_page.min(last);
    let Some(on_page) = pages.get(bs.preview_page).copied() else { return };
    // Full-page slides print on slide-shaped pages; notes and handouts on the chosen paper.
    let (pw, ph) = print_page_size(bs, doc);
    let avail_w = ui.available_width().clamp(200.0, 760.0);
    let scale = (avail_w / pw.max(1.0)).min(520.0 / ph.max(1.0));
    let (area, _) = ui.allocate_exact_size(vec2(avail_w, ph * scale + 24.0), Sense::hover());
    ui.painter().rect_filled(area, CornerRadius::same(2), t.canvas);
    let page = Rect::from_center_size(area.center(), vec2(pw * scale, ph * scale));
    let p = ui.painter();
    p.rect_filled(page.translate(vec2(2.0, 3.0)), 0.0, t.shadow);
    p.rect_filled(page, 0.0, Color32::WHITE);
    let aspect = (doc.slide_size.height / doc.slide_size.width.max(1.0)) as f32;
    let mut budget = 4u32;
    let mut draw_slide = |ui: &mut Ui, i: usize, r: Rect| {
        let px = (r.width() * ui.ctx().pixels_per_point()).round().clamp(16.0, 1600.0) as u32;
        match app.textures.thumb(ui.ctx(), doc, i, px, &mut budget) {
            Some(tex) => {
                ui.painter().image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => ui.ctx().request_repaint(),
        }
        if bs.print_frame || bs.print_layout != "slides" {
            ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(120)), StrokeKind::Outside);
        }
    };
    match bs.print_layout {
        "slides" => {
            if let Some(i) = on_page.first() {
                draw_slide(ui, *i, page);
            }
        }
        "notes" => {
            if let Some(i) = on_page.first() {
                let w = page.width() * 0.75;
                let r = Rect::from_min_size(pos2(page.center().x - w / 2.0, page.top() + page.height() * 0.08), vec2(w, w * aspect));
                draw_slide(ui, *i, r);
                let notes = doc.slides.get(*i).map(|s| s.notes_text()).unwrap_or_default();
                let font = theme::font((11.0 * scale).clamp(5.0, 13.0));
                let text_rect = Rect::from_min_max(
                    pos2(page.left() + page.width() * 0.12, r.bottom() + 20.0 * scale),
                    page.max - vec2(page.width() * 0.12, 30.0 * scale),
                );
                let galley = ui.painter().layout(notes, font, Color32::from_gray(30), text_rect.width());
                ui.painter().with_clip_rect(text_rect).galley(text_rect.min, galley, Color32::from_gray(30));
            }
        }
        l => {
            let n = per_page(l);
            let (cols, rows) = match n {
                1 => (1, 1),
                2 => (1, 2),
                3 => (1, 3),
                4 => (2, 2),
                6 => (2, 3),
                _ => (3, 3),
            };
            let inner = page.shrink2(vec2(page.width() * 0.08, page.height() * 0.07));
            // Three a page leaves lines for notes beside each slide, as PowerPoint's handouts do.
            let slide_area = if n == 3 { Rect::from_min_size(inner.min, vec2(inner.width() * 0.5, inner.height())) } else { inner };
            let cell = vec2(slide_area.width() / cols as f32, slide_area.height() / rows as f32);
            for (k, i) in on_page.iter().enumerate() {
                let (c, r) = ((k % cols) as f32, (k / cols) as f32);
                let cell_rect = Rect::from_min_size(slide_area.min + vec2(c * cell.x, r * cell.y), cell).shrink(cell.x.min(cell.y) * 0.08);
                let w = cell_rect.width().min(cell_rect.height() / aspect.max(0.01));
                let sr = Rect::from_center_size(cell_rect.center(), vec2(w, w * aspect));
                draw_slide(ui, *i, sr);
                if n == 3 {
                    for line in 1..=5 {
                        let y = sr.top() + sr.height() * line as f32 / 6.0;
                        ui.painter()
                            .line_segment([pos2(inner.center().x + 10.0, y), pos2(inner.right(), y)], Stroke::new(0.8, Color32::from_gray(150)));
                    }
                }
            }
        }
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let prev = icon_button(ui, Icon::ChevronLeft, "Previous Page", bs.preview_page > 0);
        if prev {
            bs.preview_page = bs.preview_page.saturating_sub(1);
        }
        ui.label(format!("{} of {}", bs.preview_page + 1, pages.len()));
        if icon_button(ui, Icon::ChevronRight, "Next Page", bs.preview_page < last) {
            bs.preview_page = (bs.preview_page + 1).min(last);
        }
    });
}

fn icon_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if resp.hovered() && enabled {
        ui.painter().rect_filled(r, CornerRadius::same(3), t.hover);
    }
    icons::paint(ui.painter(), r.shrink(3.0), icon, t.text, !enabled);
    resp.on_hover_text(label).clicked()
}

/// Export: tasks on the left (Create PDF Document, Create Handouts, Change File Type), the chosen
/// one's description and button on the right. PowerPoint's video, animated GIF and CD package
/// exports aren't offered.
fn export(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(250.0);
            for (task, icon, label) in [
                (ExportTask::Pdf, Icon::File, "Create PDF Document"),
                (ExportTask::Handouts, Icon::Notes, "Create Handouts"),
                (ExportTask::FileType, Icon::Export, "Change File Type"),
            ] {
                if place_row(ui, icon, label, bs.export == task, 244.0).clicked() {
                    bs.export = task;
                }
            }
        });
        let (r, _) = ui.allocate_exact_size(vec2(1.0, 420.0), Sense::hover());
        ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, t.separator));
        ui.add_space(24.0);
        ui.vertical(|ui| {
            ui.set_max_width(560.0);
            match bs.export {
                ExportTask::Pdf => {
                    ui.label(RichText::new("Create a PDF Document").font(theme::font(16.0)).color(t.text));
                    bullets(
                        ui,
                        &["Keeps the layout, fonts and pictures", "Opens on any device with a PDF viewer", "The presentation keeps its own file"],
                    );
                    ui.checkbox(&mut bs.pdf_hidden, "Include hidden slides");
                    ui.add_space(10.0);
                    if square_button(ui, Icon::File, "Create PDF", true).clicked() {
                        app.export_as("pdf", json!({"includeHidden": bs.pdf_hidden}));
                    }
                }
                ExportTask::Handouts => {
                    ui.label(RichText::new("Create Handouts").font(theme::font(16.0)).color(t.text));
                    bullets(ui, &["Several slides on each page of a PDF", "Three a page leaves lines for notes", "Print them for your audience"]);
                    egui::ComboBox::from_id_salt("backstage_handouts")
                        .width(240.0)
                        .selected_text(format!("{} slides per page", bs.handouts_per_page))
                        .show_ui(ui, |ui| {
                            for n in [1u8, 2, 3, 4, 6, 9] {
                                if ui.selectable_label(bs.handouts_per_page == n, format!("{n} slides per page")).clicked() {
                                    bs.handouts_per_page = n;
                                }
                            }
                        });
                    ui.add_space(10.0);
                    if square_button(ui, Icon::Notes, "Create Handouts", true).clicked() {
                        app.export_as("pdf", json!({"layout": "handouts", "perPage": bs.handouts_per_page}));
                    }
                }
                ExportTask::FileType => {
                    ui.label(RichText::new("Change File Type").font(theme::font(16.0)).color(t.text));
                    ui.add_space(8.0);
                    for (id, label, detail) in SAVE_TYPES.iter().filter(|s| s.0 != "pdf") {
                        let selected = bs.file_type == *id;
                        let (rect, resp) = ui.allocate_exact_size(vec2(520.0, 46.0), Sense::click());
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, *label));
                        let p = ui.painter();
                        if selected {
                            p.rect_filled(rect, CornerRadius::same(2), t.selected);
                        } else if resp.hovered() {
                            p.rect_filled(rect, CornerRadius::same(2), t.hover);
                        }
                        p.text(pos2(rect.left() + 12.0, rect.center().y - 8.0), Align2::LEFT_CENTER, *label, theme::font(13.0), t.text);
                        p.text(pos2(rect.left() + 12.0, rect.center().y + 9.0), Align2::LEFT_CENTER, *detail, theme::font(11.5), t.text_dim);
                        if resp.clicked() {
                            bs.file_type = id;
                        }
                    }
                    ui.add_space(12.0);
                    let png = bs.file_type == "png";
                    if png {
                        ui.checkbox(&mut bs.png_transparent, "Transparent background")
                            .on_hover_text("Leave the slide background out: only what is on the slide is drawn");
                        ui.add_space(8.0);
                    }
                    if square_button(ui, Icon::SaveAs, "Save As", true).clicked() {
                        let extra = if png && bs.png_transparent { json!({"transparentBackground": true}) } else { json!({}) };
                        app.export_as(bs.file_type, extra);
                    }
                }
            }
        });
    });
}

fn bullets(ui: &mut Ui, items: &[&str]) {
    ui.add_space(4.0);
    for b in items {
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(format!("•  {b}")).font(theme::font(12.5)));
        });
    }
    ui.add_space(10.0);
}

/// Account: what DeckCraft is. DeckCraft has no accounts, so there is nothing to sign in to.
fn account(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(340.0);
            heading(ui, "Appearance");
            let dark = app.ui.brightness == theme::Brightness::Dark;
            egui::ComboBox::from_id_salt("backstage_appearance").width(260.0).selected_text(if dark { "Dark" } else { "Light" }).show_ui(ui, |ui| {
                for (on, label) in [(false, "Light"), (true, "Dark")] {
                    if ui.selectable_label(dark == on, label).clicked() {
                        let _ = app.run("view.dark", json!({"on": on}));
                    }
                }
            });
            ui.add_space(6.0);
            if arrow_link(ui, "Themes, keyboard shortcuts and more in Settings") {
                bs_settings(app);
            }
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

/// Account's link to Settings › Themes.
fn bs_settings(app: &mut SlideApp) {
    let _ = app.run("app.backstage", json!({"page": "settings", "tab": "themes"}));
}

/// Settings: a search box and a list of tabs on the left, the chosen tab on the right.
fn settings(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
    let t = Tokens::get(ui.ctx());
    let top = ui.cursor().top();
    ui.horizontal_top(|ui| {
        let tabs = ui
            .vertical(|ui| {
                ui.set_width(TAB_W);
                let field = egui::TextEdit::singleline(&mut bs.settings_query).hint_text("Search settings").desired_width(TAB_W - 8.0);
                let field = ui.add(field);
                // The hint isn't a name: screen readers get one.
                field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search settings"));
                let typed = field.changed();
                ui.add_space(8.0);
                let hits = settings_search(&bs.settings_query);
                let searching = !bs.settings_query.trim().is_empty();
                // Typing jumps to the first tab with a match, unless the open one has one.
                if typed
                    && let Some(first) = hits.first()
                    && !hits.iter().any(|h| h.0 == bs.settings_tab)
                {
                    bs.open_tab(first.0);
                }
                if searching && hits.is_empty() {
                    dim(ui, "No settings match.");
                }
                for tab in SettingsTab::ALL {
                    let found: Vec<&str> = hits.iter().filter(|h| h.0 == tab).map(|h| h.1).collect();
                    if searching && found.is_empty() {
                        continue;
                    }
                    if settings_tab(ui, tab, bs.settings_tab == tab, &t).clicked() {
                        bs.open_tab(tab);
                    }
                    for name in found {
                        ui.horizontal(|ui| {
                            ui.add_space(42.0);
                            let l = egui::Label::new(RichText::new(name).font(theme::font(12.0)).color(t.text_dim)).sense(Sense::click());
                            if ui.add(l).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                bs.open_tab(tab);
                            }
                        });
                    }
                }
            })
            .response;
        let (gap, _) = ui.allocate_exact_size(vec2(33.0, 1.0), Sense::hover());
        let content = ui
            .vertical(|ui| {
                let tab = bs.settings_tab;
                ui.label(RichText::new(tab.label()).font(theme::bold(16.0)).color(t.text));
                ui.add_space(8.0);
                match tab {
                    SettingsTab::General => general_settings(app, ui, &t),
                    SettingsTab::Themes => {
                        ui.set_max_width(THEMES_W);
                        appearance(app, ui, &t);
                    }
                    SettingsTab::NewPresentations => new_presentation_settings(app, ui, &t),
                    SettingsTab::Shortcuts => shortcuts(app, ui, bs),
                    SettingsTab::Backup => backup_and_reset(app, ui, bs),
                }
            })
            .response;
        // The line between the tabs and the tab's content runs as far down as either goes.
        let bottom = tabs.rect.bottom().max(content.rect.bottom());
        let x = gap.center().x;
        ui.painter().line_segment([pos2(x, top), pos2(x, bottom)], Stroke::new(1.0, t.separator));
    });
}

/// A tab in Settings' list: an icon and a name; the open one has an accent bar and a shaded row.
fn settings_tab(ui: &mut Ui, tab: SettingsTab, selected: bool, t: &Tokens) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(TAB_W, 38.0), Sense::click());
    let label = tab.label();
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label));
    let p = ui.painter();
    if selected || resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(4), t.hover);
    }
    if selected {
        let bar = Rect::from_min_size(rect.min + vec2(0.0, 7.0), vec2(3.0, rect.height() - 14.0));
        p.rect_filled(bar, CornerRadius::same(1), t.accent);
    }
    if resp.has_focus() {
        p.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, t.text), StrokeKind::Inside);
    }
    let ink = if selected { t.accent } else { t.text };
    icons::paint(p, Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), vec2(16.0, 16.0)), tab.icon(), ink, false);
    let font = if selected { theme::bold(13.5) } else { theme::font(13.5) };
    p.text(pos2(rect.left() + 42.0, rect.center().y), Align2::LEFT_CENTER, label, font, t.text);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A setting's name inside a Settings tab.
fn sub(ui: &mut Ui, text: &str, t: &Tokens) {
    ui.add_space(14.0);
    ui.label(RichText::new(text).font(theme::bold(13.5)).color(t.text));
    ui.add_space(2.0);
}

/// Settings › General: interface size, reduce motion and what opens at start.
fn general_settings(app: &mut SlideApp, ui: &mut Ui, t: &Tokens) {
    use crate::{MAX_UI_SCALE, MIN_UI_SCALE, Startup};
    ui.set_max_width(640.0);
    sub(ui, "Interface size", t);
    dim(ui, "Makes all of DeckCraft bigger or smaller: menus, ribbon, panes and their text. Slides have their own zoom.");
    ui.add_space(6.0);
    let scale = app.ui.ui_scale;
    ui.horizontal(|ui| {
        for (s, label) in [(0.9, "90%"), (1.0, "100%"), (1.25, "125%"), (1.5, "150%"), (1.75, "175%")] {
            if ui.add(egui::Button::selectable((scale - s).abs() < 0.005, label).min_size(vec2(56.0, 26.0))).clicked() {
                let _ = app.run("view.interfaceSize", json!({"scale": s}));
            }
        }
        ui.add_space(14.0);
        // Five-percent steps from the nearest one.
        let step = |d: f32| ((scale * 20.0).round() + d) / 20.0;
        let smaller = egui::Button::new("−").min_size(vec2(26.0, 26.0));
        if ui.add_enabled(scale > MIN_UI_SCALE + 0.001, smaller).on_hover_text("Smaller").clicked() {
            let _ = app.run("view.interfaceSize", json!({"scale": step(-1.0)}));
        }
        ui.label(RichText::new(format!("{:.0}%", scale * 100.0)).color(t.text));
        let larger = egui::Button::new("+").min_size(vec2(26.0, 26.0));
        if ui.add_enabled(scale < MAX_UI_SCALE - 0.001, larger).on_hover_text("Larger").clicked() {
            let _ = app.run("view.interfaceSize", json!({"scale": step(1.0)}));
        }
    });

    sub(ui, "Motion", t);
    let mut calm = app.ui.reduce_motion;
    if ui.checkbox(&mut calm, "Reduce motion").changed() {
        let _ = app.run("view.reduceMotion", json!({"on": calm}));
    }
    dim(ui, "Turns off the sliding and fading of DeckCraft's own menus, panes and scrolling. Transitions and animations in your slides still play.");

    sub(ui, "When DeckCraft starts", t);
    for s in Startup::ALL {
        if ui.radio(app.ui.startup == s, s.label()).clicked() {
            let _ = app.run("app.startup", json!({"open": s.id()}));
        }
    }
    dim(ui, "A presentation you open with DeckCraft (by double-clicking it, say) always opens, whatever you pick here.");
}

/// Settings › New Presentations: the theme, slide size and font new presentations start with,
/// and the default shape.
fn new_presentation_settings(app: &mut SlideApp, ui: &mut Ui, t: &Tokens) {
    dim(ui, "What every new presentation starts with. A theme you pick on the New page wins over the one here.");
    ui.add_space(12.0);
    let prefs = app.session.prefs.clone();
    let themes = deckcraft_model::theme::builtin_themes();
    let chosen = themes.iter().find(|th| th.name.eq_ignore_ascii_case(&prefs.new_theme)).cloned().unwrap_or_default();
    let standard = prefs.new_size == "standard";
    ui.horizontal_top(|ui| {
        // A preview: the theme, its font and the slide's shape.
        let mut preview = chosen.clone();
        if !prefs.new_font.is_empty() {
            preview.fonts.major.latin = prefs.new_font.clone();
            preview.fonts.minor.latin = prefs.new_font.clone();
        }
        let size = if standard { vec2(144.0, 108.0) } else { vec2(192.0, 108.0) };
        let (slot, _) = ui.allocate_exact_size(vec2(192.0, 108.0), Sense::hover());
        let r = Rect::from_center_size(slot.center(), size);
        crate::ribbon::paint_theme_tile(ui.painter(), r, &preview);
        ui.painter().rect_stroke(r, CornerRadius::same(2), Stroke::new(1.0, t.border), StrokeKind::Outside);
        ui.add_space(20.0);
        egui::Grid::new("new_presentation_defaults").num_columns(2).spacing(vec2(14.0, 10.0)).show(ui, |ui| {
            ui.label("Theme");
            egui::ComboBox::from_id_salt("new_presentation_theme").width(240.0).selected_text(chosen.name.clone()).show_ui(ui, |ui| {
                for th in &themes {
                    if ui.selectable_label(th.name == chosen.name, th.name.as_str()).clicked() {
                        let _ = app.run("settings.newPresentation", json!({"theme": th.name}));
                    }
                }
            });
            ui.end_row();
            ui.label("Slide size");
            ui.horizontal(|ui| {
                for (id, label, on) in [("widescreen", "Widescreen (16:9)", !standard), ("standard", "Standard (4:3)", standard)] {
                    if ui.add(egui::Button::selectable(on, label).min_size(vec2(0.0, 26.0))).clicked() {
                        let _ = app.run("settings.newPresentation", json!({"size": id}));
                    }
                }
            });
            ui.end_row();
            ui.label("Font");
            const THEMES_FONTS: &str = "The theme's fonts";
            let shown = if prefs.new_font.is_empty() { THEMES_FONTS.to_string() } else { prefs.new_font.clone() };
            egui::ComboBox::from_id_salt("new_presentation_font").width(240.0).height(360.0).selected_text(shown).show_ui(ui, |ui| {
                if ui.selectable_label(prefs.new_font.is_empty(), THEMES_FONTS).clicked() {
                    let _ = app.run("settings.newPresentation", json!({"font": ""}));
                }
                for f in deckcraft_fonts::FontDb::global().families() {
                    if ui.selectable_label(prefs.new_font == f, f.as_str()).clicked() {
                        let _ = app.run("settings.newPresentation", json!({"font": f}));
                    }
                }
            });
            ui.end_row();
        });
    });

    sub(ui, "Default shape", t);
    if prefs.default_shape.is_some() {
        dim(ui, "New shapes look like the one you picked with Set as Default Shape, now and the next time you open DeckCraft.");
        ui.add_space(6.0);
        if ui.button("Forget the default shape").clicked() {
            let _ = app.run("settings.newPresentation", json!({"forgetShape": true}));
        }
    } else {
        dim(ui, "Right-click a shape and pick Set as Default Shape: new shapes then look like it, and DeckCraft remembers it next time.");
    }
    ui.add_space(18.0);
    if ui.button("New presentation with these settings").clicked() {
        let _ = app.run("file.new", json!({}));
    }
}

/// Settings › Backup and Reset: settings to and from a file, and Reset all settings.
fn backup_and_reset(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
    let t = Tokens::get(ui.ctx());
    ui.set_max_width(640.0);
    sub(ui, "Back up and restore", &t);
    dim(
        ui,
        "Keep your theme, keyboard shortcuts, editing options and the rest in one file, to save or to bring to another computer. Recent files aren't included.",
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("Back up settings…").clicked() {
            app.backup_settings();
        }
        if ui.button("Restore from a backup…").clicked() {
            app.restore_settings();
        }
    });
    sub(ui, "Reset", &t);
    reset_settings(app, ui, bs);
}

/// Settings › Backup and Reset: Reset all settings, with a confirmation in place.
fn reset_settings(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
    if !bs.confirm_reset {
        dim(ui, "Put the theme, editing options, keyboard shortcuts and window layout back to how DeckCraft ships.");
        dim(ui, "Your presentations and recent files are not touched.");
        ui.add_space(6.0);
        if ui.button("Reset all settings…").clicked() {
            bs.confirm_reset = true;
        }
        return;
    }
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new("Reset every setting? Your presentations and recent files stay; this can't be undone.").color(t.text));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.add(egui::Button::new(RichText::new("Yes, reset everything").color(t.accent_text)).fill(t.accent)).clicked() {
            let _ = app.run("app.resetSettings", json!({}));
            bs.confirm_reset = false;
            app.set_status("All settings are back to their defaults");
        }
        if ui.button("Cancel").clicked() {
            bs.confirm_reset = false;
        }
    });
}

/// Keyboard Shortcuts (#127): every command and slide-show action with its keys. Each key can be
/// removed, a new one recorded with +, and an action or everything reset. A recorded key another
/// action has asks before it moves.
fn shortcuts(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
    use crate::keymap::{Combo, Scope};
    let t = Tokens::get(ui.ctx());
    record_key(app, ui, bs);
    dim(
        ui,
        "Give any command your own keyboard keys. Custom keys are checked first; an action's built-in keys stop working once you change its keys.",
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("Save cheat sheet…").on_hover_text("A page with all your shortcuts, to print or keep").clicked() {
            app.save_cheat_sheet();
        }
        if ui.button("Copy as text").clicked() {
            let (text, _) = app.ui.keymap.cheat_sheet();
            ui.ctx().copy_text(text);
            app.set_status("Copied all your shortcuts as text");
        }
    });
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        for (show, label) in [(false, "Editor"), (true, "Slide Show")] {
            if ui.add(egui::Button::selectable(bs.keys_show == show, label).min_size(vec2(96.0, 26.0))).clicked() {
                bs.keys_show = show;
                bs.recording = None;
                bs.clash = None;
            }
        }
        ui.add_space(16.0);
        ui.add(egui::TextEdit::singleline(&mut bs.query).hint_text("Search commands or keys").desired_width(240.0));
        if !bs.keys_show {
            ui.checkbox(&mut bs.keys_bound_only, "Only commands with keys");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let any = !app.ui.keymap.0.is_empty();
            if ui.add_enabled(any, egui::Button::new("Reset all shortcuts")).clicked() {
                let _ = app.run("keys.reset", json!({}));
                bs.clash = None;
            }
        });
    });
    if !bs.keys_show {
        dim(ui, "Editor shortcuts need Ctrl, Alt or a function key, so typing stays typing.");
    }
    if let Some(note) = &bs.key_note {
        ui.label(RichText::new(note.as_str()).color(t.accent));
    }
    if let Some((id, keys, msg)) = bs.clash.clone() {
        ui.add_space(6.0);
        egui::Frame::NONE.fill(t.card).stroke(Stroke::new(1.0, t.accent)).corner_radius(CornerRadius::same(6)).inner_margin(10).show(ui, |ui| {
            ui.label(RichText::new(msg).color(t.text));
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new("Move it here").color(t.accent_text)).fill(t.accent)).clicked() {
                    let _ = app.run("keys.set", json!({"action": id, "keys": keys, "replace": true}));
                    bs.clash = None;
                }
                if ui.button("Cancel").clicked() {
                    bs.clash = None;
                }
            });
        });
    }
    ui.add_space(10.0);
    let scope = if bs.keys_show { Scope::Show } else { Scope::Editor };
    let q = bs.query.trim().to_lowercase();
    let mut rows: Vec<_> = crate::keymap::actions().into_iter().filter(|a| a.scope == scope).collect();
    rows.sort_by(|a, b| a.group.cmp(&b.group).then(a.label.cmp(&b.label)));
    let km = app.ui.keymap.clone();
    let mut edits: Vec<(String, Vec<String>)> = vec![];
    let mut resets: Vec<String> = vec![];
    egui::Grid::new("shortcut_rows").num_columns(3).striped(true).spacing(vec2(18.0, 6.0)).min_col_width(80.0).show(ui, |ui| {
        for a in &rows {
            let keys = km.keys_of(a);
            let hay = format!("{} {} {} {}", a.label, a.id, a.group, keys.iter().map(Combo::display).collect::<Vec<_>>().join(" ")).to_lowercase();
            if (!q.is_empty() && !hay.contains(&q))
                || (scope == Scope::Editor && bs.keys_bound_only && q.is_empty() && keys.is_empty() && !km.is_custom(&a.id))
            {
                continue;
            }
            ui.vertical(|ui| {
                ui.set_min_width(300.0);
                ui.label(RichText::new(&a.label).color(t.text));
                ui.label(RichText::new(format!("{} · {}", a.group, a.id)).font(theme::font(11.0)).color(t.text_faint));
            });
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(420.0);
                for k in &keys {
                    let chip = ui.add(egui::Button::new(format!("{}  ×", k.display())).corner_radius(CornerRadius::same(10)));
                    chip.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Remove {} from {}", k.display(), a.label))
                    });
                    if chip.on_hover_text("Remove this key").clicked() {
                        edits.push((a.id.clone(), keys.iter().filter(|x| *x != k).map(Combo::spec).collect()));
                    }
                }
                if bs.recording.as_deref() == Some(a.id.as_str()) {
                    ui.label(RichText::new("Press the new keys… (Esc cancels)").color(t.accent));
                } else if keys.len() < crate::keymap::MAX_KEYS {
                    let add = ui.button("+").on_hover_text("Add a key");
                    add.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Add a key to {}", a.label)));
                    if add.clicked() {
                        add.surrender_focus();
                        bs.recording = Some(a.id.clone());
                        bs.clash = None;
                        bs.key_note = None;
                    }
                }
            });
            if km.is_custom(&a.id) {
                if ui
                    .link("Reset")
                    .on_hover_text(format!(
                        "Back to {}",
                        if a.defaults.is_empty() {
                            "no key".to_string()
                        } else {
                            a.defaults.iter().map(Combo::display).collect::<Vec<_>>().join(", ")
                        }
                    ))
                    .clicked()
                {
                    resets.push(a.id.clone());
                }
            } else {
                ui.label("");
            }
            ui.end_row();
        }
    });
    for (id, keys) in edits {
        let _ = app.run("keys.set", json!({"action": id, "keys": keys}));
    }
    for id in resets {
        let _ = app.run("keys.reset", json!({"action": id}));
    }
}

/// While an action is recording: the next key press becomes one of its keys (Esc cancels).
fn record_key(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
    use crate::keymap::Combo;
    let Some(id) = bs.recording.clone() else { return };
    // egui-winit sends Ctrl+C, Ctrl+X and Ctrl+V as Copy, Cut and Paste events, with no key press.
    let presses: Vec<(egui::Key, egui::Modifiers)> = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                egui::Event::Copy => Some((egui::Key::C, egui::Modifiers::COMMAND)),
                egui::Event::Cut => Some((egui::Key::X, egui::Modifiers::COMMAND)),
                egui::Event::Paste(_) => Some((egui::Key::V, egui::Modifiers::COMMAND)),
                _ => None,
            })
            .collect()
    });
    let Some((key, m)) = presses.into_iter().next() else { return };
    bs.recording = None;
    if key == egui::Key::Escape && m.is_none() {
        return;
    }
    let c = Combo::from_egui(key, m);
    let Some(a) = crate::keymap::actions().into_iter().find(|a| a.id == id) else { return };
    if a.scope == crate::keymap::Scope::Editor && !c.ok_for_editor() {
        bs.key_note = Some(format!("{} can't be an editor shortcut: add Ctrl, Alt or use a function key.", c.display()));
        return;
    }
    let mut keys: Vec<String> = app.ui.keymap.keys_of(&a).iter().map(Combo::spec).collect();
    if keys.contains(&c.spec()) {
        return;
    }
    keys.push(c.spec());
    match app.run("keys.set", json!({"action": id, "keys": keys})) {
        Ok(_) => bs.key_note = None,
        Err(e) if e.contains("replace: true") => {
            let msg = e.split(';').next().unwrap_or(&e).to_string();
            bs.clash = Some((id, keys, format!("{msg}. Move it to {}?", a.label)));
        }
        Err(e) => bs.key_note = Some(e),
    }
}

/// Width of Settings' tab list.
const TAB_W: f32 = 210.0;
/// Width of Settings › Themes: three theme cards side by side.
const THEMES_W: f32 = 3.0 * THEME_CARD.x + 2.0 * 12.0;
const THEME_CARD: egui::Vec2 = vec2(170.0, 102.0);

/// Light or dark, and the interface theme (PowerPoint's Office Theme): a preview of every theme in
/// the current mode, and the Custom theme's colours.
fn appearance(app: &mut SlideApp, ui: &mut Ui, t: &Tokens) {
    dim(ui, "Colours DeckCraft's own windows, ribbon and panes. Your slides keep their own colours.");
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        for (b, label) in [(theme::Brightness::Light, "Light"), (theme::Brightness::Dark, "Dark")] {
            if ui.add(egui::Button::selectable(app.ui.brightness == b, label).min_size(vec2(84.0, 26.0))).clicked() {
                let _ = app.run("view.dark", json!({"on": b == theme::Brightness::Dark}));
            }
        }
    });
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
        for th in theme::UiTheme::ALL {
            let (r, resp) = ui.allocate_exact_size(THEME_CARD + vec2(0.0, 24.0), Sense::click());
            let card = Rect::from_min_size(r.min, THEME_CARD);
            paint_theme_preview(ui.painter(), card, &Tokens::build(th, app.ui.brightness, &app.ui.custom_theme));
            let selected = app.ui.ui_theme == th;
            if selected || resp.hovered() || resp.has_focus() {
                ui.painter().rect_stroke(card, CornerRadius::same(6), Stroke::new(if selected { 2.5 } else { 1.5 }, t.accent), StrokeKind::Outside);
            }
            let font = if selected { theme::bold(13.0) } else { theme::font(13.0) };
            ui.painter().text(pos2(r.min.x + 2.0, r.max.y - 10.0), Align2::LEFT_CENTER, th.label(), font, t.text);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, th.label()));
            if resp.on_hover_text(th.description()).clicked() {
                let _ = app.run("view.uiTheme", json!({"theme": th.id()}));
            }
        }
    });
    dim(ui, app.ui.ui_theme.description());
    if app.ui.ui_theme == theme::UiTheme::Custom {
        ui.add_space(12.0);
        let mut c = app.ui.custom_theme;
        let mut changed = false;
        egui::Grid::new("custom_theme").num_columns(2).spacing(vec2(16.0, 8.0)).show(ui, |ui| {
            ui.label("Window tint");
            changed |= ui.color_edit_button_srgb(&mut c.tint).changed();
            ui.end_row();
            ui.label("Accent");
            changed |= ui.color_edit_button_srgb(&mut c.accent).changed();
            ui.end_row();
            ui.label("Tint strength");
            changed |= ui.add(egui::Slider::new(&mut c.strength, 0.0..=1.0).show_value(false)).changed();
            ui.end_row();
        });
        if changed {
            let _ = app.run("view.uiTheme", json!({"tint": theme::hex(c.tint), "accent": theme::hex(c.accent), "strength": c.strength}));
        }
        let ratio = [t.chrome, t.card, t.canvas].iter().map(|b| theme::contrast(t.text, *b)).fold(f32::INFINITY, f32::min);
        let verdict = if ratio >= 7.0 { "very easy to read (WCAG AAA)" } else { "easy to read (WCAG AA)" };
        dim(ui, &format!("Text contrast {ratio:.1}:1, {verdict}. Text colours adjust automatically to stay readable."));
    }
}

/// A miniature DeckCraft window in the theme `tk`: title bar, ribbon, thumbnails and a slide.
pub fn paint_theme_preview(p: &egui::Painter, r: Rect, tk: &Tokens) {
    p.rect(r, CornerRadius::same(6), tk.canvas, Stroke::new(1.0, tk.card_border), StrokeKind::Inside);
    let inner = r.shrink(1.0);
    // Title bar with the Present button.
    let title = Rect::from_min_size(inner.min, vec2(inner.width(), 16.0));
    p.rect_filled(title, CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, tk.chrome);
    p.rect_filled(Rect::from_min_size(pos2(title.max.x - 30.0, title.min.y + 4.0), vec2(24.0, 8.0)), CornerRadius::same(4), tk.accent);
    p.text(pos2(title.min.x + 6.0, title.center().y), Align2::LEFT_CENTER, "Aa", theme::bold(10.0), tk.text);
    // Ribbon card with a selected tab underline and a few buttons.
    let ribbon = Rect::from_min_size(pos2(inner.min.x + 4.0, title.max.y + 2.0), vec2(inner.width() - 8.0, 20.0));
    p.rect(ribbon, CornerRadius::same(3), tk.card, Stroke::new(if tk.high_contrast { 1.0 } else { 0.0 }, tk.card_border), StrokeKind::Inside);
    for i in 0..5 {
        let b = Rect::from_min_size(pos2(ribbon.min.x + 6.0 + i as f32 * 14.0, ribbon.min.y + 6.0), vec2(9.0, 9.0));
        p.rect_filled(b, CornerRadius::same(2), if i == 1 { tk.selected } else { tk.text_dim.gamma_multiply(0.85) });
    }
    p.line_segment([pos2(ribbon.min.x + 6.0, title.max.y), pos2(ribbon.min.x + 22.0, title.max.y)], Stroke::new(2.0, tk.accent));
    // Thumbnails pane.
    let side = Rect::from_min_max(pos2(inner.min.x, ribbon.max.y + 3.0), pos2(inner.min.x + 34.0, inner.max.y));
    p.rect_filled(side, CornerRadius { nw: 0, ne: 0, sw: 5, se: 0 }, tk.chrome);
    for i in 0..3 {
        let th = Rect::from_min_size(pos2(side.min.x + 6.0, side.min.y + 4.0 + i as f32 * 17.0), vec2(22.0, 13.0));
        let (w, c) = if i == 0 { (1.5, tk.accent) } else { (0.5, tk.border) };
        p.rect(th, CornerRadius::same(1), Color32::WHITE, Stroke::new(w, c), StrokeKind::Outside);
    }
    // The slide on the canvas.
    let area = Rect::from_min_max(pos2(side.max.x + 8.0, side.min.y + 6.0), pos2(inner.max.x - 8.0, inner.max.y - 8.0));
    let h = (area.width() * 9.0 / 16.0).min(area.height());
    let slide = Rect::from_center_size(area.center(), vec2(h * 16.0 / 9.0, h));
    p.rect_filled(slide, CornerRadius::same(1), Color32::WHITE);
    p.rect_filled(
        Rect::from_min_size(slide.min + vec2(8.0, h * 0.3), vec2(slide.width() * 0.55, 4.0)),
        CornerRadius::same(1),
        Color32::from_gray(0x40),
    );
    p.rect_filled(
        Rect::from_min_size(slide.min + vec2(8.0, h * 0.3 + 8.0), vec2(slide.width() * 0.4, 3.0)),
        CornerRadius::same(1),
        Color32::from_gray(0x9A),
    );
}

#[cfg(test)]
#[path = "backstage_tests.rs"]
mod tests;
