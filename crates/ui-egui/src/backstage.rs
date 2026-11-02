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

mod files;
mod info;
mod print;

pub use files::split_path;
use files::{home, new_page, open_page};
use info::{PROPERTIES, account, commit_props, info, save_as};
pub use info::{SAVE_TYPES, format_bytes};
use print::{export, print};
pub use print::{print_page_size, print_params, print_slides};

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
    pub docs: (usize, Option<u64>),
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
    /// Construct the Backstage view at a page with the current document identity, so a replaced deck
    /// can be detected.
    pub fn new(page: Page, docs: (usize, Option<u64>)) -> Backstage {
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
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Back"));
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
                // set_max_width can also widen: in a window narrower than CONTENT_W it would push the
                // page past the right margin and clip the tiles (#1421).
                ui.set_max_width(ui.available_width().min(CONTENT_W));
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

/// Draw one clickable Backstage navigation row and return its interaction response.
fn nav_item(ui: &mut Ui, nav: Nav, bs: &Backstage, index: usize, t: &Tokens) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(NAV_W, ITEM_H), Sense::click());
    let selected = nav == Nav::Page(bs.page);
    let label = nav.label();
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
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

/// Draw a labelled icon button and return whether it was clicked while enabled.
fn icon_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if resp.hovered() && enabled {
        ui.painter().rect_filled(r, CornerRadius::same(3), t.hover);
    }
    icons::paint(ui.painter(), r.shrink(3.0), icon, t.text, !enabled);
    resp.on_hover_text(label).clicked()
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

#[cfg(test)]
mod tests;
