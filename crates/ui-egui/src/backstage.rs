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
}

impl Page {
    pub const ALL: [Page; 8] = [Page::Home, Page::New, Page::Open, Page::Info, Page::SaveAs, Page::Print, Page::Export, Page::Account];

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
        }
    }

    /// Pages about the open presentation (hidden on the start screen).
    pub fn needs_doc(self) -> bool {
        matches!(self, Page::Info | Page::SaveAs | Page::Print | Page::Export)
    }
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
    (top, vec![Nav::Page(Page::Account), Nav::Options])
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
            png_transparent: false,
        }
    }

    /// Show `page`, as a click in the side menu does.
    pub fn go(&mut self, page: Page) {
        if self.page != page {
            self.query.clear();
            self.preview_page = 0;
        }
        self.page = page;
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

/// Account: the interface's appearance and what DeckCraft is. DeckCraft has no accounts, so
/// there is nothing to sign in to.
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

#[cfg(test)]
#[path = "backstage_tests.rs"]
mod tests;
