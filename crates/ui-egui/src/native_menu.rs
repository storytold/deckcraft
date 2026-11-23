//! The menu bar: File, Edit, Insert, Slide, Format, Arrange, View, Play, Window and Help, laid out
//! the way a Mac app's are (after PhotoCraft's `native_menu.rs`). On macOS the desktop app hands
//! it to the system menu bar (`apps/deckcraft/src/mac_menu.rs`, through muda); elsewhere, and on
//! the web, [`bar_ui`] draws the same menus in the window under the title bar.
//!
//! Pure and platform-free, so it is tested everywhere. What the layout does:
//! - an **app menu** named DeckCraft: About, Settings… (⌘,), Services, Hide, Hide Others, Show
//!   All and Quit (the app's own quit, so the save prompt runs);
//! - **Window** gets Minimize, Zoom, the open presentations and Bring All to Front.
//!
//! **Keys stay with egui.** AppKit runs a menu item's key equivalent before the window sees the
//! key, which would skip DeckCraft's key rules (text being typed keeps ⌘B, ⌘Z and the arrows,
//! dialogs are modal). So the backend reports a key equivalent as [`Event::Key`] and
//! [`NativeMenu::raw_input`] gives it to egui as the key press it was (⌘C, ⌘X and ⌘V as the copy,
//! cut and paste egui would have made of them). A click is [`Event::Click`] and runs the command.

use std::hash::{DefaultHasher, Hash, Hasher};

use egui::{Key, Modifiers};
use serde_json::json;

use crate::SlideApp;

pub const APP_NAME: &str = "DeckCraft";
/// Quit DeckCraft (the save prompt runs).
pub const QUIT: &str = "app.quit";
/// Hide DeckCraft (the backend calls AppKit's `hide:`).
pub const HIDE: &str = "app.hide";
/// Minimize the window.
pub const MINIMIZE: &str = "window.minimize";
/// `window.doc.<n>`: bring presentation n forward.
const DOC_PREFIX: &str = "window.doc.";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuBar {
    pub menus: Vec<Menu>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub title: String,
    pub role: MenuRole,
    pub children: Vec<Node>,
}

/// What a top-level menu is for. macOS treats the app, Window and Help menus specially.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MenuRole {
    #[default]
    Normal,
    App,
    Window,
    Help,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Item(Item),
    Separator,
    Submenu {
        label: String,
        children: Vec<Node>,
    },
    /// An item AppKit provides and runs itself (Services, Zoom, …).
    Standard(Standard),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// The command id (`file.save`), or [`QUIT`], [`HIDE`], [`MINIMIZE`].
    pub id: String,
    pub label: String,
    /// `Cmd+Shift+Z` form; `Ctrl` is the real Control key.
    pub shortcut: Option<String>,
    pub enabled: bool,
    /// `Some` for check items.
    pub checked: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Standard {
    Services,
    HideOthers,
    ShowAll,
    Zoom,
    BringAllToFront,
}

impl MenuBar {
    /// Every item, depth first.
    pub fn items(&self) -> Vec<&Item> {
        fn walk<'a>(nodes: &'a [Node], out: &mut Vec<&'a Item>) {
            for n in nodes {
                match n {
                    Node::Item(it) => out.push(it),
                    Node::Submenu { children, .. } => walk(children, out),
                    Node::Separator | Node::Standard(_) => {}
                }
            }
        }
        let mut out = Vec::new();
        for m in &self.menus {
            walk(&m.children, &mut out);
        }
        out
    }

    pub fn find(&self, id: &str) -> Option<&Item> {
        self.items().into_iter().find(|it| it.id == id)
    }
}

/// A hash of everything a native menu can only change by rebuilding: menus, item ids, kinds and
/// shortcuts. Labels, enabled and checked are updated in place.
pub fn structure_key(bar: &MenuBar) -> u64 {
    fn walk(nodes: &[Node], h: &mut DefaultHasher) {
        for n in nodes {
            match n {
                Node::Item(it) => ('i', &it.id, it.checked.is_some(), &it.shortcut).hash(h),
                Node::Separator => '-'.hash(h),
                Node::Submenu { label, children } => {
                    ('[', label).hash(h);
                    walk(children, h);
                    ']'.hash(h);
                }
                Node::Standard(s) => ('s', s).hash(h),
            }
        }
    }
    let mut h = DefaultHasher::new();
    for m in &bar.menus {
        (&m.title, m.role).hash(&mut h);
        walk(&m.children, &mut h);
    }
    h.finish()
}

// ------------------------------------------------------------------------------------- shortcuts

/// A shortcut as DeckCraft writes it (`Cmd+Shift+Z`). `Cmd` is ⌘ and `Ctrl` the Control key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// The key as written: `Z`, `1`, `[`, `F5`, `Enter`.
    pub key: String,
}

impl Chord {
    pub fn parse(s: &str) -> Option<Chord> {
        let mut c = Chord { cmd: false, ctrl: false, alt: false, shift: false, key: String::new() };
        for part in s.split('+') {
            match part {
                "Cmd" => c.cmd = true,
                "Ctrl" => c.ctrl = true,
                "Alt" => c.alt = true,
                "Shift" => c.shift = true,
                "" => c.key = "+".into(),
                k if c.key.is_empty() => c.key = k.into(),
                _ => return None,
            }
        }
        (!c.key.is_empty()).then_some(c)
    }

    pub fn is_function_key(&self) -> bool {
        self.key.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=24).contains(&n))
    }

    /// May the native menu make this a key equivalent? Only with ⌘ or ⌃, or a function key: as
    /// key equivalents bare letters would be taken from text being typed.
    pub fn native_ok(&self) -> bool {
        self.cmd || self.ctrl || self.is_function_key()
    }

    /// As a Mac menu shows it: ⌃⌥⇧⌘ then the key.
    pub fn symbols(&self) -> String {
        let mut s = String::new();
        for (on, sym) in [(self.ctrl, "⌃"), (self.alt, "⌥"), (self.shift, "⇧"), (self.cmd, "⌘")] {
            if on {
                s.push_str(sym);
            }
        }
        s.push_str(match self.key.as_str() {
            "Enter" => "↩",
            "Delete" | "Backspace" => "⌫",
            "Escape" => "⎋",
            "Tab" => "⇥",
            "Space" => "Space",
            k => k,
        });
        s
    }

    /// The key press egui would have had for this chord.
    pub fn key_event(&self, pressed: bool) -> Option<egui::Event> {
        let key = match self.key.as_str() {
            "+" => Key::Plus,
            "'" => Key::Quote,
            ";" => Key::Semicolon,
            "=" => Key::Equals,
            "-" => Key::Minus,
            "[" => Key::OpenBracket,
            "]" => Key::CloseBracket,
            "\\" => Key::Backslash,
            "/" => Key::Slash,
            "," => Key::Comma,
            "." => Key::Period,
            "`" => Key::Backtick,
            k => Key::from_name(k)?,
        };
        let modifiers = Modifiers { alt: self.alt, ctrl: self.ctrl, shift: self.shift, mac_cmd: self.cmd, command: self.cmd };
        Some(egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers })
    }
}

/// The shortcut a menu shows for command `id`: the Mac keys DeckCraft adds (⌘G, ⇧⌘Z…), else the
/// command's own.
pub fn shortcut_of(id: &str) -> Option<String> {
    let mac = match id {
        QUIT => Some("Cmd+Q"),
        HIDE => Some("Cmd+H"),
        MINIMIZE => Some("Cmd+M"),
        "app.openDialog" => Some("Cmd+O"),
        "file.save" => Some("Cmd+S"),
        "app.saveAsDialog" => Some("Cmd+Shift+S"),
        "edit.redo" => Some("Cmd+Shift+Z"),
        "edit.copy" => Some("Cmd+C"),
        "edit.cut" => Some("Cmd+X"),
        "edit.paste" => Some("Cmd+V"),
        "edit.pasteText" => Some("Cmd+Alt+Shift+V"),
        "arrange.group" => Some("Cmd+G"),
        "arrange.ungroup" => Some("Cmd+Shift+G"),
        "arrange.bringToFront" => Some("Cmd+Alt+]"),
        "arrange.sendToBack" => Some("Cmd+Alt+["),
        "view.fit" => Some("Cmd+0"),
        "app.find" => Some("Cmd+F"),
        "app.lockToggle" => Some("Cmd+L"),
        "app.unlockAll" => Some("Cmd+Shift+L"),
        _ => None,
    };
    // ⌘L locks layers in the canvas layout; Align Left keeps it in the ribbon.
    if id == "format.alignLeft" {
        return None;
    }
    mac.map(str::to_string)
        .or_else(|| deckcraft_engine::find_command(id).and_then(|c| c.shortcut).map(str::to_string))
        .or_else(|| crate::UI_COMMANDS.iter().find(|c| c.0 == id).and_then(|c| c.2).map(str::to_string))
}

// ------------------------------------------------------------------------------------ the menus

/// One row of the menu definition: a command (id, label), a separator, a submenu, or AppKit's own.
enum Row {
    Cmd(&'static str, &'static str),
    Sep,
    Sub(&'static str, Vec<Row>),
    Std(Standard),
}

use Row::{Cmd, Sep, Std, Sub};

fn definition() -> Vec<(&'static str, MenuRole, Vec<Row>)> {
    vec![
        (
            APP_NAME,
            MenuRole::App,
            vec![
                Cmd("app.about", "About DeckCraft"),
                Sep,
                Cmd("app.preferences", "Settings…"),
                Sep,
                Std(Standard::Services),
                Sep,
                Cmd(HIDE, "Hide DeckCraft"),
                Std(Standard::HideOthers),
                Std(Standard::ShowAll),
                Sep,
                Cmd(QUIT, "Quit DeckCraft"),
            ],
        ),
        (
            "File",
            MenuRole::Normal,
            vec![
                Cmd("file.new", "New Presentation"),
                Cmd("app.openDialog", "Open…"),
                Sep,
                Cmd("app.closePresentation", "Close"),
                Cmd("file.save", "Save"),
                Cmd("app.saveAsDialog", "Save As…"),
                Sep,
                Cmd("app.exportDialog", "Export…"),
                Cmd("app.print", "Print…"),
                Sep,
                Cmd("file.properties", "Properties"),
            ],
        ),
        (
            "Edit",
            MenuRole::Normal,
            vec![
                Cmd("edit.undo", "Undo"),
                Cmd("edit.redo", "Redo"),
                Sep,
                Cmd("edit.cut", "Cut"),
                Cmd("edit.copy", "Copy"),
                Cmd("edit.paste", "Paste"),
                Cmd("edit.pasteText", "Paste and Match Style"),
                Cmd("edit.pasteToReplace", "Paste to Replace"),
                Cmd("edit.duplicate", "Duplicate"),
                Sep,
                Cmd("edit.selectAll", "Select All"),
                Cmd("edit.deselect", "Deselect"),
                Sep,
                Cmd("app.find", "Find…"),
            ],
        ),
        (
            "Insert",
            MenuRole::Normal,
            vec![
                Cmd("slide.new", "New Slide"),
                Sep,
                Cmd("insert.textBox", "Text Box"),
                Cmd("app.insertPictureDialog", "Picture…"),
                Cmd("insert.table", "Table"),
                Cmd("insert.chart", "Chart"),
                Sub("Media", vec![Cmd("app.insertVideoDialog", "Video…"), Cmd("app.insertAudioDialog", "Audio…")]),
                Sep,
                Cmd("insert.wordArt", "Decorative Text"),
                Cmd("insert.dateTime", "Date and Time"),
                Cmd("insert.slideNumber", "Slide Number"),
            ],
        ),
        (
            "Slide",
            MenuRole::Normal,
            vec![
                Cmd("slide.new", "New Slide"),
                Cmd("slide.duplicate", "Duplicate Slide"),
                Cmd("slide.delete", "Delete Slide"),
                Sep,
                Cmd("slide.hide", "Skip Slide"),
                Cmd("slide.reset", "Reset Slide"),
                Sep,
                Cmd("section.add", "Add Section"),
            ],
        ),
        (
            "Format",
            MenuRole::Normal,
            vec![
                Sub("Font", vec![Cmd("format.bold", "Bold"), Cmd("format.italic", "Italic"), Cmd("format.underline", "Underline")]),
                Sub(
                    "Text",
                    vec![
                        Cmd("format.alignLeft", "Align Left"),
                        Cmd("format.alignCenter", "Center"),
                        Cmd("format.alignRight", "Align Right"),
                        Cmd("format.justify", "Justify"),
                        Sep,
                        Cmd("format.rtl", "Right-to-Left"),
                    ],
                ),
                Sep,
                Cmd("app.paragraphDialog", "Paragraph…"),
            ],
        ),
        (
            "Arrange",
            MenuRole::Normal,
            vec![
                Cmd("arrange.bringToFront", "Bring to Front"),
                Cmd("arrange.bringForward", "Bring Forward"),
                Cmd("arrange.sendBackward", "Send Backward"),
                Cmd("arrange.sendToBack", "Send to Back"),
                Sep,
                Cmd("arrange.group", "Group"),
                Cmd("arrange.ungroup", "Ungroup"),
                Sep,
                Cmd("app.lockToggle", "Lock / Unlock"),
                Cmd("app.unlockAll", "Unlock All"),
                Sep,
                Cmd("arrange.flipHorizontal", "Flip Horizontally"),
                Cmd("arrange.flipVertical", "Flip Vertically"),
                Cmd("arrange.rotateLeft", "Rotate Left"),
                Cmd("arrange.rotateRight", "Rotate Right"),
            ],
        ),
        (
            "View",
            MenuRole::Normal,
            vec![
                Cmd("view.normal", "Slides"),
                Cmd("view.sorter", "Grid"),
                Cmd("view.outline", "Outline"),
                Sep,
                Cmd("view.slideMaster", "Edit Slide Masters"),
                Sep,
                Cmd("view.notes", "Presenter Notes"),
                Cmd("view.ruler", "Ruler"),
                Cmd("view.gridlines", "Gridlines"),
                Cmd("view.guides", "Guides"),
                Sep,
                Cmd("view.panels", "Show/Hide Side Panels"),
                Cmd("view.leftPanel", "Show/Hide Left Panel"),
                Cmd("view.collapseLayers", "Collapse Layers"),
                Sep,
                Cmd("view.zoomIn", "Zoom In"),
                Cmd("view.zoomOut", "Zoom Out"),
                Cmd("view.fit", "Zoom to Fit"),
                Sep,
                Cmd("view.dark", "Dark Appearance"),
            ],
        ),
        (
            "Play",
            MenuRole::Normal,
            vec![Cmd("show.fromStart", "Play from Start"), Cmd("show.fromCurrent", "Play from This Slide"), Sep, Cmd("show.setup", "Set Up Show…")],
        ),
        ("Window", MenuRole::Window, vec![Cmd(MINIMIZE, "Minimize"), Std(Standard::Zoom), Sep, Std(Standard::BringAllToFront)]),
        (
            "Help",
            MenuRole::Help,
            vec![Cmd("app.palette", "Command Palette"), Cmd("app.links", "Community and Source"), Sep, Cmd("app.about", "About DeckCraft")],
        ),
    ]
}

/// Commands the menus run themselves (not engine or UI commands).
fn own(id: &str) -> bool {
    matches!(id, QUIT | HIDE | MINIMIZE | "app.find" | "app.paragraphDialog" | "app.lockToggle" | "app.unlockAll") || id.starts_with(DOC_PREFIX)
}

/// The check mark an item shows, for the settings it toggles.
fn checked(app: &SlideApp, id: &str) -> Option<bool> {
    Some(match id {
        "view.notes" => app.ui.notes,
        "view.ruler" => app.ui.ruler,
        "view.gridlines" => app.ui.gridlines,
        "view.guides" => app.ui.guides,
        "view.dark" => app.ui.brightness == crate::theme::Brightness::Dark,
        "view.normal" => app.ui.view == crate::ViewMode::Normal,
        "view.sorter" => app.ui.view == crate::ViewMode::Sorter,
        "view.outline" => app.ui.view == crate::ViewMode::Outline,
        _ => return None,
    })
}

/// The menus as they stand: each item's shortcut, whether it can run now, and its check mark.
pub fn menu_bar(app: &SlideApp) -> MenuBar {
    fn nodes(app: &SlideApp, rows: Vec<Row>) -> Vec<Node> {
        rows.into_iter()
            .map(|r| match r {
                Cmd(id, label) => Node::Item(Item {
                    id: id.into(),
                    label: label.into(),
                    shortcut: shortcut_of(id),
                    enabled: id == QUIT || (app.dialog.is_none() && (own(id) || crate::ribbon::enabled(app, id))),
                    checked: checked(app, id),
                }),
                Sep => Node::Separator,
                Sub(label, children) => Node::Submenu { label: label.into(), children: nodes(app, children) },
                Std(s) => Node::Standard(s),
            })
            .collect()
    }
    let mut menus: Vec<Menu> =
        definition().into_iter().map(|(title, role, rows)| Menu { title: title.into(), role, children: nodes(app, rows) }).collect();
    // Window: the open presentations, the current one checked.
    if let Some(w) = menus.iter_mut().find(|m| m.role == MenuRole::Window) {
        let docs: Vec<String> = app.session.documents().iter().map(|d| d.title()).collect();
        let active = app.session.active_index();
        let at = w.children.len().saturating_sub(1);
        let mut list = vec![Node::Separator];
        for (i, title) in docs.into_iter().enumerate() {
            list.push(Node::Item(Item {
                id: format!("{DOC_PREFIX}{i}"),
                label: title,
                shortcut: None,
                enabled: true,
                checked: Some(active == Some(i)),
            }));
        }
        w.children.splice(at..at, list);
    }
    MenuBar { menus }
}

/// Run a menu item's command, as a click on it does.
pub fn invoke(app: &mut SlideApp, ctx: &egui::Context, id: &str) {
    match id {
        QUIT => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        MINIMIZE => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
        HIDE => {}
        "app.find" => crate::find::open(app),
        "app.paragraphDialog" => app.dialog = Some(crate::dialogs::Dialog::new("paragraph")),
        "app.lockToggle" | "app.unlockAll" => {
            let shift = id == "app.unlockAll";
            crate::menus::mac_shortcut(app, egui::Key::L, deckcraft_engine::Mods { cmd: true, shift, alt: false });
        }
        _ => {
            if let Some(i) = id.strip_prefix(DOC_PREFIX).and_then(|n| n.parse::<usize>().ok()) {
                let _ = app.run("window.next", json!({"index": i}));
                return;
            }
            // Reset Slide in the canvas layout keeps the slide's layers (nothing added).
            let p = if id == "slide.reset" && app.ui.shell == crate::Shell::Canvas { json!({"adopt": true}) } else { json!({}) };
            if let Err(e) = app.run(id, p) {
                app.set_status(e);
            }
        }
    }
}

// ------------------------------------------------------------------------------- native backend

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// An item was chosen with the mouse (or the keyboard inside an open menu).
    Click(String),
    /// A key equivalent was pressed: the shortcut, to be handled as the key press it was.
    Key(Chord),
}

/// A platform's native menu bar (the desktop app implements it for macOS with muda).
pub trait Backend {
    /// Bring the menu in step with `bar`: update labels, enabled and checked in place, or
    /// rebuild when its structure changed.
    fn sync(&mut self, bar: &MenuBar);
    /// Events since the last call. Platform items (Services, Zoom, …) are handled by the backend.
    fn drain(&mut self) -> Vec<Event>;
}

/// Reads the clipboard's text (for ⌘V, which AppKit takes before egui could read it).
pub type ClipboardText = Box<dyn Fn() -> Option<String>>;

/// The native menu bar, while DeckCraft has one (the in-window bar is hidden then).
pub struct NativeMenu {
    backend: Box<dyn Backend>,
    clipboard: ClipboardText,
    /// What the menus were last built from.
    state: Option<u64>,
    clicks: Vec<String>,
    dirty: bool,
}

impl NativeMenu {
    pub fn new(backend: Box<dyn Backend>, clipboard: ClipboardText) -> NativeMenu {
        NativeMenu { backend, clipboard, state: None, clicks: Vec::new(), dirty: true }
    }

    /// Before egui sees this frame's input: key equivalents become the key presses they were
    /// (⌘C / ⌘X / ⌘V the copy, cut and paste egui makes of them), clicks wait for [`run`].
    ///
    /// AppKit takes only the key-down; winit still delivers the key-up, so a key equivalent adds
    /// just its press, ahead of its key-up when that is already queued.
    pub fn raw_input(&mut self, raw: &mut egui::RawInput) {
        for e in self.backend.drain() {
            match e {
                Event::Key(chord) => {
                    let clipboard_key = chord.cmd && !chord.ctrl && !chord.alt;
                    let event = match chord.key.as_str() {
                        "C" if clipboard_key && !chord.shift => Some(egui::Event::Copy),
                        "X" if clipboard_key && !chord.shift => Some(egui::Event::Cut),
                        // A paste with no text (an image) is the ⌘V release winit still sends.
                        "V" if chord.cmd && !chord.ctrl => (self.clipboard)().filter(|t| !t.is_empty()).map(egui::Event::Paste),
                        _ => chord.key_event(true),
                    };
                    let Some(event) = event else { continue };
                    let pressed_key = match &event {
                        egui::Event::Key { key, .. } => Some(*key),
                        _ => None,
                    };
                    let at = pressed_key
                        .and_then(|k| raw.events.iter().position(|e| matches!(e, egui::Event::Key { key, pressed: false, .. } if *key == k)))
                        .unwrap_or(raw.events.len());
                    raw.events.insert(at, event);
                }
                Event::Click(id) => self.clicks.push(id),
            }
            self.dirty = true;
        }
    }
}

/// Run the native menu clicks that arrived since the last frame, and bring its items up to date
/// when anything they show may have changed.
pub fn run(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(menu) = app.native_menu.as_mut() else { return };
    let clicks = std::mem::take(&mut menu.clicks);
    for id in clicks {
        if app.dialog.is_none() || id == QUIT {
            invoke(app, ctx, &id);
        }
    }
    let input = ctx
        .input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. } | egui::Event::PointerButton { pressed: false, .. })));
    let state = state_hash(app, input);
    let Some(menu) = app.native_menu.as_ref() else { return };
    if menu.state == Some(state) && !menu.dirty {
        return;
    }
    let bar = menu_bar(app);
    if let Some(menu) = app.native_menu.as_mut() {
        menu.backend.sync(&bar);
        menu.state = Some(state);
        menu.dirty = false;
    }
}

/// What the menus depend on: the presentation's revision and selection, the open dialog, the
/// view settings, and (each frame with a click or key) the frame itself.
fn state_hash(app: &SlideApp, input: bool) -> u64 {
    let mut h = DefaultHasher::new();
    if let Some(d) = app.session.active() {
        (d.uid, d.revision, d.selection.shapes.len(), d.selection.text.is_some(), format!("{:?}", d.selection.target)).hash(&mut h);
    }
    (app.session.documents().len(), app.session.active_index(), app.dialog.is_some()).hash(&mut h);
    (app.ui.notes, app.ui.ruler, app.ui.gridlines, app.ui.guides, format!("{:?}{:?}", app.ui.view, app.ui.brightness)).hash(&mut h);
    if input {
        crate::now_ms().to_bits().hash(&mut h);
    }
    h.finish()
}

// ------------------------------------------------------------------------------- in-window bar

/// The menus in the window, under the title bar (where there's no native menu bar: the web,
/// Windows, Linux).
pub fn bar_ui(app: &mut SlideApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let bar = menu_bar(app);
    let mac = ui.ctx().os() == egui::os::OperatingSystem::Mac;
    let mut chosen = None;
    egui::Panel::top("menu_bar").exact_size(26.0).frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(8, 0))).show(
        ui,
        |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                for m in bar.menus.iter().filter(|m| m.role != MenuRole::App) {
                    ui.menu_button(egui::RichText::new(&m.title).font(crate::theme::font(13.0)), |ui| {
                        ui.set_min_width(220.0);
                        draw_nodes(ui, &m.children, mac, &mut chosen);
                    });
                }
            });
        },
    );
    if let Some(id) = chosen {
        let ctx = ui.ctx().clone();
        invoke(app, &ctx, &id);
    }
}

fn draw_nodes(ui: &mut egui::Ui, nodes: &[Node], mac: bool, chosen: &mut Option<String>) {
    for n in nodes {
        match n {
            Node::Separator => {
                ui.separator();
            }
            Node::Submenu { label, children } => {
                ui.menu_button(label, |ui| draw_nodes(ui, children, mac, chosen));
            }
            Node::Standard(_) => {}
            Node::Item(it) => {
                let keys =
                    it.shortcut.as_deref().and_then(Chord::parse).map(|c| if mac { c.symbols() } else { c.symbols_portable() }).unwrap_or_default();
                let label = match it.checked {
                    Some(true) => format!("✓ {}", it.label),
                    Some(false) => format!("   {}", it.label),
                    None => it.label.clone(),
                };
                if ui.add_enabled(it.enabled, egui::Button::new(label).shortcut_text(keys)).clicked() {
                    *chosen = Some(it.id.clone());
                    ui.close();
                }
            }
        }
    }
}

impl Chord {
    /// As Windows and Linux menus show it: `Ctrl+Shift+Z`.
    pub fn symbols_portable(&self) -> String {
        let mut parts = Vec::new();
        for (on, name) in [(self.ctrl || self.cmd, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift")] {
            if on {
                parts.push(name);
            }
        }
        parts.push(&self.key);
        parts.join("+")
    }
}

#[cfg(test)]
#[path = "native_menu_tests.rs"]
mod tests;
