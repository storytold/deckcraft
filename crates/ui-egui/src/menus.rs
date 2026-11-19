//! UI-level shortcuts, the command palette, and the menu structure (shared with the native macOS
//! menu bar in the desktop app).

use deckcraft_engine::Mods;
use serde_json::json;

use crate::SlideApp;
use crate::theme::{self, Tokens};

/// Shortcuts the UI handles itself (views, show, zoom, palette, file dialogs). True when consumed.
pub fn ui_shortcut(app: &mut SlideApp, key: egui::Key, m: Mods) -> bool {
    use egui::Key::*;
    if key == A && m.cmd && !m.shift && !m.alt && app.slides_list_focused() {
        // The engine can't see which pane has focus, so the UI says so through the scope param.
        let _ = app.run("edit.selectAll", json!({"scope": "slides"}));
        return true;
    }
    let id = match (key, m.cmd, m.shift, m.alt) {
        (F5, _, false, _) => {
            app.start_show(app.show_start_index(), false);
            return true;
        }
        (F5, _, true, _) | (Enter, true, false, false) => {
            let i = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            app.start_show(i, false);
            return true;
        }
        (Enter, true, true, _) => {
            app.start_show(app.show_start_index(), false);
            return true;
        }
        (S, true, false, false) => {
            app.save();
            return true;
        }
        (S, true, true, false) => "app.saveAsDialog",
        // The Mac's Redo, next to the registry's Ctrl+Y.
        (Z, true, true, false) => "edit.redo",
        // File tab: Alt+F opens it on Home with the keyboard in its side menu; Ctrl+P on Print.
        (F, false, false, true) => {
            app.open_backstage(crate::backstage::Page::Home);
            if let Some(b) = app.backstage.as_mut() {
                b.focus = Some(0);
            }
            return true;
        }
        (P, true, false, false) => {
            if app.session.active().is_none() {
                return false;
            }
            app.open_backstage(crate::backstage::Page::Print);
            return true;
        }
        (W, true, false, false) => {
            app.close_presentation();
            return true;
        }
        (O, true, false, false) => "app.openDialog",
        (V, true, false, true) if app.session.active().is_some() => {
            app.dialog = Some(crate::dialogs::Dialog::new("pasteSpecial"));
            return true;
        }
        (P, true, true, false) => "app.palette",
        (Comma, true, false, false) => "app.preferences",
        (Equals | Plus, true, false, false) => "view.zoomIn",
        (Minus, true, false, false) => "view.zoomOut",
        (Num0, true, false, false) => "view.fit",
        (F1, true, false, false) => "view.collapseRibbon",
        (F9, false, true, false) => "view.gridlines",
        (F9, false, false, true) => "view.guides",
        (K, true, false, false) => {
            app.dialog = Some(crate::dialogs::Dialog::new("hyperlink"));
            return true;
        }
        (F, true, false, false) => "app.find",
        // Ctrl+H is PowerPoint's (Cmd+H is macOS's Hide, see `keys.rs`); Option+Cmd+F is the Mac's.
        (H, true, false, false) | (F, true, false, true) => "app.replace",
        (Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9, false, false, true) => {
            if app.session.mac_keys || app.backstage.is_some() || app.session.active().is_some_and(|d| d.selection.text.is_some()) {
                return false;
            }
            let idx = match key {
                Num1 => 0,
                Num2 => 1,
                Num3 => 2,
                Num4 => 3,
                Num5 => 4,
                Num6 => 5,
                Num7 => 6,
                Num8 => 7,
                Num9 => 8,
                _ => return false,
            };
            return crate::qat::run_index(app, idx);
        }
        _ => return false,
    };
    let _ = app.run(id, json!({}));
    true
}

/// The palette's entries for the lowercase query `q`. An engine command is hidden when a UI command has the same label: Open…, Save As…,
/// Export… and Audio/Video from File… need a `path` the palette can't give, so the one entry left opens the dialog instead of failing.
fn palette_items(session: &deckcraft_engine::Session, q: &str) -> Vec<(String, String, Option<&'static str>)> {
    let mut items: Vec<(String, String, Option<&'static str>)> = session
        .commands()
        .into_iter()
        // Commands that need a path or other value fail when the palette runs them with `{}`.
        .filter(|c| c.enabled && !crate::UI_COMMANDS.iter().any(|u| u.1 == c.label) && !deckcraft_engine::cmd::needs_arguments(c.params))
        .map(|c| (c.id.to_string(), c.label.to_string(), c.shortcut))
        .chain(crate::UI_COMMANDS.iter().filter(|c| !deckcraft_engine::cmd::needs_arguments(c.3)).map(|c| (c.0.to_string(), c.1.to_string(), c.2)))
        .filter(|(id, label, _)| q.is_empty() || label.to_lowercase().contains(q) || id.to_lowercase().contains(q))
        .collect();
    items.sort_by_key(|(_, l, _)| !l.to_lowercase().starts_with(q));
    items.truncate(60);
    items
}

/// ⇧⌘P: search every command by name and run it.
pub fn palette(app: &mut SlideApp, ctx: &egui::Context) {
    let Some((mut query, mut sel)) = app.palette.take() else { return };
    let t = Tokens::get(ctx);
    let mut close = false;
    let mut run: Option<String> = None;
    egui::Window::new("command_palette")
        .title_bar(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 90.0))
        .fixed_size(egui::vec2(520.0, 380.0))
        .show(ctx, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut query).hint_text("Search commands…").desired_width(f32::INFINITY).font(theme::font(15.0)));
            r.request_focus();
            let q = query.to_lowercase();
            let items = palette_items(&app.session, &q);
            let (down, up, enter, esc) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowDown),
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::Enter),
                    i.key_pressed(egui::Key::Escape),
                )
            });
            if down {
                sel = (sel + 1).min(items.len().saturating_sub(1));
            }
            if up {
                sel = sel.saturating_sub(1);
            }
            if esc {
                close = true;
            }
            if enter && let Some((id, ..)) = items.get(sel) {
                run = Some(id.clone());
            }
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for (k, (id, label, sc)) in items.iter().enumerate() {
                    let r = ui.horizontal(|ui| {
                        let resp = ui.selectable_label(k == sel, label);
                        if let Some(s) = sc {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(egui::RichText::new(crate::ribbon::pretty_shortcut(s)).color(t.text_faint));
                            });
                        }
                        resp
                    });
                    if r.inner.clicked() {
                        run = Some(id.clone());
                    }
                }
            });
        });
    if let Some(id) = run {
        let _ = app.run(&id, json!({}));
        close = true;
    }
    if !close {
        app.palette = Some((query, sel));
    }
}

/// One entry of the menu bar. `action` is a command id, optionally followed by a space and its JSON
/// params; `tool:<preset>` arms the shape tool, and "" is a feature DeckCraft doesn't have yet (shown
/// dimmed). `keys` is the shortcut DeckCraft already binds to the action, shown in the menu.
pub enum Entry {
    Item { label: &'static str, action: &'static str, keys: Option<&'static str> },
    Sub(&'static str, Vec<Entry>),
    Sep,
}

fn it(label: &'static str, action: &'static str) -> Entry {
    Entry::Item { label, action, keys: None }
}

fn key(label: &'static str, action: &'static str, keys: &'static str) -> Entry {
    Entry::Item { label, action, keys: Some(keys) }
}

fn sub(label: &'static str, entries: Vec<Entry>) -> Entry {
    Entry::Sub(label, entries)
}

/// The menu bar below the application menu: (menu, entries). Used by the native macOS menu bar and
/// as the parity catalogue's live side.
pub fn menu_tree() -> Vec<(&'static str, Vec<Entry>)> {
    use Entry::Sep;
    vec![
        (
            "File",
            vec![
                key("New Presentation", "file.new", "Cmd+N"),
                it("New from Template…", r#"app.backstage {"page":"new"}"#),
                key("Open…", "app.openDialog", "Cmd+O"),
                it("Open Recent", "app.backstage"),
                Sep,
                key("Close", "app.closePresentation", "Cmd+W"),
                key("Save", "file.save", "Cmd+S"),
                key("Save As…", "app.saveAsDialog", "Cmd+Shift+S"),
                it("Export…", "app.exportDialog"),
                Sep,
                it("Compress Pictures…", "picture.compress"),
                Sep,
                it("Page Setup…", r#"app.dialog {"id":"slideSize"}"#),
                key("Print…", "app.print", "Cmd+P"),
                Sep,
                it("Properties", r#"app.backstage {"page":"info"}"#),
            ],
        ),
        (
            "Edit",
            vec![
                key("Undo", "edit.undo", "Cmd+Z"),
                // macOS redoes with ⇧⌘Z; Windows and Linux with Ctrl+Y.
                key("Redo", "edit.redo", if cfg!(target_os = "macos") { "Cmd+Shift+Z" } else { "Cmd+Y" }),
                Sep,
                key("Cut", "edit.cut", "Cmd+X"),
                key("Copy", "edit.copy", "Cmd+C"),
                Sep,
                key("Paste", "edit.paste", "Cmd+V"),
                key("Paste Special…", "app.pasteSpecial", "Cmd+Alt+V"),
                key("Paste and Match Formatting", "edit.pasteText", "Cmd+Shift+V"),
                Sep,
                it("Clear", "edit.delete"),
                key("Select All", "edit.selectAll", "Cmd+A"),
                key("Duplicate", "edit.duplicate", "Cmd+D"),
                it("Delete Slide", "slide.delete"),
                Sep,
                sub(
                    "Find",
                    vec![
                        key("Find…", "app.find", "Cmd+F"),
                        key("Find and Replace…", "app.replace", "Cmd+Alt+F"),
                        it("Replace Fonts…", r#"app.dialog {"id":"replaceFonts"}"#),
                    ],
                ),
            ],
        ),
        (
            "View",
            vec![
                it("Normal", "view.normal"),
                it("Slide Sorter", "view.sorter"),
                it("Notes Page", "view.notesPage"),
                it("Outline View", "view.outline"),
                it("Reading View", "view.reading"),
                it("Presenter View", "show.presenter"),
                key("Slide Show", "show.fromStart", "Cmd+Shift+Enter"),
                Sep,
                sub(
                    "Master",
                    vec![it("Slide Master", "view.slideMaster"), it("Handout Master", "view.handoutMaster"), it("Notes Master", "view.notesMaster")],
                ),
                Sep,
                key("Ribbon", "view.collapseRibbon", "Cmd+F1"),
                Sep,
                it("Header and Footer…", r#"app.dialog {"id":"headerFooter"}"#),
                Sep,
                it("Ruler", "view.ruler"),
                sub(
                    "Grid and Guides",
                    vec![
                        key("Gridlines", "view.gridlines", "Shift+F9"),
                        key("Guides", "view.guides", "Alt+F9"),
                        Sep,
                        it("Grid Options…", "view.gridOptions"),
                    ],
                ),
                sub(
                    "Zoom",
                    vec![
                        it("Zoom…", "view.zoomDialog"),
                        key("Zoom In", "view.zoomIn", "Cmd+="),
                        key("Zoom Out", "view.zoomOut", "Cmd+-"),
                        key("Fit to Window", "view.fit", "Cmd+0"),
                    ],
                ),
            ],
        ),
        (
            "Insert",
            vec![
                key("New Slide", "slide.new", "Cmd+Shift+N"),
                it("Duplicate Slide", "slide.duplicate"),
                sub("Slides from", vec![it("Reuse Slides…", "app.reuseSlidesDialog"), it("Outline…", r#"app.dialog {"id":"outline"}"#)]),
                Sep,
                it("Section", "section.add"),
                it("Comment", r#"app.dialog {"id":"comment"}"#),
                Sep,
                it("Text Box", "insert.textBox"),
                it("Decorative Text", "insert.wordArt"),
                it("Header and Footer…", r#"app.dialog {"id":"headerFooter"}"#),
                it("Date and Time", "insert.dateTime"),
                it("Slide Number", "insert.slideNumber"),
                Sep,
                it("Table…", r#"app.dialog {"id":"table"}"#),
                sub(
                    "Chart",
                    vec![
                        it("Column", r#"insert.chart {"type":"Column"}"#),
                        it("Bar", r#"insert.chart {"type":"Bar"}"#),
                        it("Line", r#"insert.chart {"type":"Line"}"#),
                        it("Area", r#"insert.chart {"type":"Area"}"#),
                        it("Pie", r#"insert.chart {"type":"Pie"}"#),
                        it("Treemap", r#"insert.chart {"type":"Treemap"}"#),
                        it("Sunburst", r#"insert.chart {"type":"Sunburst"}"#),
                        it("Histogram", r#"insert.chart {"type":"Histogram"}"#),
                        it("Pareto", r#"insert.chart {"type":"Pareto"}"#),
                        it("Box and Whisker", r#"insert.chart {"type":"BoxWhisker"}"#),
                        it("X Y (Scatter)", r#"insert.chart {"type":"Scatter"}"#),
                        it("Waterfall", r#"insert.chart {"type":"Waterfall"}"#),
                        it("Funnel", r#"insert.chart {"type":"Funnel"}"#),
                        it("Stock", r#"insert.chart {"type":"Stock"}"#),
                        it("Surface", r#"insert.chart {"type":"Surface"}"#),
                        it("Radar", r#"insert.chart {"type":"Radar"}"#),
                        it("Filled Map", r#"insert.chart {"type":"RegionMap"}"#),
                    ],
                ),
                sub(
                    "Diagram",
                    vec![
                        it("List", r#"insert.smartArt {"kind":"list"}"#),
                        it("Process", r#"insert.smartArt {"kind":"process"}"#),
                        it("Cycle", r#"insert.smartArt {"kind":"cycle"}"#),
                        it("Hierarchy", r#"insert.smartArt {"kind":"hierarchy"}"#),
                        it("Matrix", r#"insert.smartArt {"kind":"matrix"}"#),
                        it("Pyramid", r#"insert.smartArt {"kind":"pyramid"}"#),
                        Sep,
                        it("More Diagrams…", r#"app.dialog {"id":"smartart"}"#),
                    ],
                ),
                Sep,
                sub(
                    "Picture",
                    vec![
                        it("Picture from File…", "app.insertPictureDialog"),
                        it("Photo Album…", "insert.photoAlbum"),
                        it("Edit Photo Album…", "insert.photoAlbumEdit"),
                    ],
                ),
                sub("Audio", vec![it("Audio from File…", "app.insertAudioDialog")]),
                sub("Video", vec![it("Video from File…", "app.insertVideoDialog")]),
                it("Equation", r#"app.dialog {"id":"equation"}"#),
                it("Symbol…", r#"app.dialog {"id":"symbol"}"#),
                sub(
                    "Shape",
                    vec![
                        it("Rectangle", "tool:rect"),
                        it("Rounded Rectangle", "tool:roundRect"),
                        it("Triangle", "tool:triangle"),
                        it("Oval", "tool:ellipse"),
                        it("Line", "tool:line"),
                    ],
                ),
                Sep,
                it("Icons…", "insert.icons"),
                Sep,
                sub(
                    "Zoom",
                    vec![
                        it("Summary Zoom", r#"insert.zoom {"kind":"summary"}"#),
                        it("Section Zoom", r#"insert.zoom {"kind":"section"}"#),
                        it("Slide Zoom", r#"insert.zoom {"kind":"slide"}"#),
                    ],
                ),
                Sep,
                sub(
                    "Action Buttons",
                    vec![
                        it("Back or Previous", r#"insert.actionButton {"kind":"back"}"#),
                        it("Forward or Next", r#"insert.actionButton {"kind":"forward"}"#),
                        it("Beginning", r#"insert.actionButton {"kind":"beginning"}"#),
                        it("End", r#"insert.actionButton {"kind":"end"}"#),
                        it("Home", r#"insert.actionButton {"kind":"home"}"#),
                        it("Information", r#"insert.actionButton {"kind":"information"}"#),
                        it("Return", r#"insert.actionButton {"kind":"return"}"#),
                        it("Video", r#"insert.actionButton {"kind":"movie"}"#),
                        it("Document", r#"insert.actionButton {"kind":"document"}"#),
                        it("Sound", r#"insert.actionButton {"kind":"sound"}"#),
                        it("Help", r#"insert.actionButton {"kind":"help"}"#),
                        it("Custom", r#"insert.actionButton {"kind":"custom"}"#),
                    ],
                ),
                it("Action Settings…", r#"app.dialog {"id":"action"}"#),
                Sep,
                key("Link…", r#"app.dialog {"id":"hyperlink"}"#, "Cmd+K"),
            ],
        ),
        (
            "Format",
            vec![
                key("Font…", "format.fontDialog", "Cmd+T"),
                it("Paragraph…", "format.paragraphDialog"),
                Sep,
                it("Bullets and Numbering…", r#"app.dialog {"id":"bullets"}"#),
                Sep,
                it("Columns…", "format.columns"),
                sub(
                    "Alignment",
                    vec![
                        key("Align Left", "format.alignLeft", "Cmd+L"),
                        key("Center", "format.alignCenter", "Cmd+E"),
                        key("Align Right", "format.alignRight", "Cmd+R"),
                        key("Justify", "format.justify", "Cmd+J"),
                    ],
                ),
                it("Size and Position…", "shape.sizePosition"),
                it("Right-to-Left Text Direction", "format.rtl"),
                Sep,
                key("Pick Up Object Style", "format.painter", "Cmd+Shift+C"),
                it("Animation Painter", "animation.painter"),
                Sep,
                it("Replace Fonts…", r#"app.dialog {"id":"replaceFonts"}"#),
                Sep,
                it("Theme Colors…", "design.colors"),
                it("Slide Background…", r#"view.pane {"pane":"background"}"#),
                Sep,
                it("Crop", "picture.crop"),
                Sep,
                it("Format Object…", r#"view.pane {"pane":"format"}"#),
            ],
        ),
        (
            "Arrange",
            vec![
                key("Bring to Front", "arrange.bringToFront", "Cmd+Shift+]"),
                key("Send to Back", "arrange.sendToBack", "Cmd+Shift+["),
                key("Bring Forward", "arrange.bringForward", "Cmd+]"),
                key("Send Backward", "arrange.sendBackward", "Cmd+["),
                Sep,
                key("Group", "arrange.group", "Cmd+Alt+G"),
                key("Ungroup", "arrange.ungroup", "Cmd+Alt+Shift+G"),
                key("Regroup", "arrange.regroup", "Cmd+Alt+J"),
                Sep,
                sub(
                    "Rotate or Flip",
                    vec![
                        it("Rotate Left 90°", "arrange.rotateLeft"),
                        it("Rotate Right 90°", "arrange.rotateRight"),
                        it("Flip Horizontal", "arrange.flipHorizontal"),
                        it("Flip Vertical", "arrange.flipVertical"),
                    ],
                ),
                sub(
                    "Align or Distribute",
                    vec![
                        it("Align Left", r#"arrange.align {"edge":"left"}"#),
                        it("Align Center", r#"arrange.align {"edge":"center"}"#),
                        it("Align Right", r#"arrange.align {"edge":"right"}"#),
                        it("Align Top", r#"arrange.align {"edge":"top"}"#),
                        it("Align Middle", r#"arrange.align {"edge":"middle"}"#),
                        it("Align Bottom", r#"arrange.align {"edge":"bottom"}"#),
                        Sep,
                        it("Distribute Horizontally", r#"arrange.distribute {"dir":"horizontal"}"#),
                        it("Distribute Vertically", r#"arrange.distribute {"dir":"vertical"}"#),
                    ],
                ),
                Sep,
                it("Selection Pane…", r#"view.pane {"pane":"selection"}"#),
            ],
        ),
        (
            "Tools",
            vec![
                key("Spelling…", "review.spelling", "F7"),
                key("Thesaurus…", "review.thesaurusPane", "Shift+F7"),
                it("Set Proofing Language…", r#"app.dialog {"id":"language"}"#),
                it("AutoCorrect Options…", r#"app.dialog {"id":"autocorrect"}"#),
                Sep,
                it("Check Accessibility", "review.accessibility"),
                Sep,
                it("Add-ins…", ""),
            ],
        ),
        (
            "Slide Show",
            vec![
                key("Play from Start", "show.fromStart", "Cmd+Shift+Enter"),
                key("Play from Current Slide", "show.fromCurrent", "Cmd+Enter"),
                Sep,
                it("Custom Slide Show…", r#"app.dialog {"id":"customShow"}"#),
                Sep,
                it("Presenter View", "show.presenter"),
                Sep,
                it("Hide Slide", "slide.hide"),
                it("Set Up Show…", r#"app.dialog {"id":"setupShow"}"#),
            ],
        ),
        ("Window", vec![it("New Window", "view.newWindow"), key("Next Window", "window.next", "Cmd+`")]),
        (
            "Help",
            vec![
                it("About DeckCraft", "app.about"),
                it("Check for Updates…", "app.checkUpdates"),
                key("Command Palette", "app.palette", "Cmd+Shift+P"),
            ],
        ),
    ]
}

/// An entry's command id and params.
pub fn split_action(action: &str) -> (&str, serde_json::Value) {
    match action.split_once(' ') {
        Some((id, p)) => (id, serde_json::from_str(p).unwrap_or_default()),
        None => (action, json!({})),
    }
}

/// `Cmd+Shift+S` → the key and modifiers it stands for.
pub fn parse_keys(keys: &str) -> Option<(egui::Key, Mods)> {
    let (mods, k) = keys.rsplit_once('+').unwrap_or(("", keys));
    let has = |m: &str| mods.split('+').any(|x| x == m);
    Some((egui::Key::from_name(k)?, Mods { cmd: has("Cmd"), shift: has("Shift"), alt: has("Alt") }))
}

/// Which command's own check dims a dialog or pane entry.
const GATES: &[(&str, &str)] = &[
    ("comment", "comment.add"),
    ("hyperlink", "insert.hyperlink"),
    ("action", "shape.action"),
    ("symbol", "insert.symbol"),
    ("equation", "insert.textBox"),
    ("table", "insert.table"),
    ("headerFooter", "design.headerFooter"),
    ("replaceFonts", "font.replace"),
    ("customShow", "show.customShow"),
    ("setupShow", "show.setup"),
    ("language", "format.language"),
    ("bullets", "format.bullets"),
    ("autocorrect", "text.autocorrect"),
    ("outline", "slide.fromOutline"),
    ("smartart", "insert.smartArt"),
    ("format", "shape.sizePosition"),
];

/// UI commands that work with no presentation open.
const DOCLESS: &[&str] = &["app.openDialog", "app.about", "app.checkUpdates", "app.palette", "app.preferences", "app.home"];

/// Whether a menu entry can run now; the menu bar dims it otherwise. `text_field`: a text field
/// has the keyboard, so the clipboard and undo entries go to it.
pub fn menu_enabled(app: &SlideApp, action: &str, text_field: bool) -> bool {
    let (id, p) = split_action(action);
    // Also a dialog's or the palette's field.
    if text_field && matches!(id, "edit.undo" | "edit.redo" | "edit.cut" | "edit.copy" | "edit.paste" | "edit.selectAll") {
        return true;
    }
    if action.is_empty() || app.dialog.is_some() || app.palette.is_some() || app.show.is_some() {
        return false;
    }
    let gate = p.get("id").or_else(|| p.get("pane")).and_then(|v| v.as_str()).and_then(|g| GATES.iter().find(|x| x.0 == g)).map(|x| x.1);
    let id = if action.starts_with("tool:") { "shape.insert" } else { gate.unwrap_or(id) };
    if id == "app.backstage" {
        let page = p.get("page").and_then(|v| v.as_str()).unwrap_or("home");
        return crate::backstage::Page::parse(page).is_some_and(|pg| !pg.needs_doc() || app.session.active().is_some());
    }
    match deckcraft_engine::find_command(id) {
        // The File tab hides the presentation: nothing edits it from there, as with the keys.
        Some(c) => !(c.undoable && app.backstage.is_some()) && (c.enabled)(&app.session).is_ok(),
        None => crate::UI_COMMANDS.iter().any(|c| c.0 == id) && (DOCLESS.contains(&id) || app.session.active().is_some()),
    }
}

/// The check mark of a view entry: `None` for entries without one.
pub fn menu_checked(app: &SlideApp, action: &str) -> Option<bool> {
    use crate::state::ViewMode;
    let doc = app.session.active().is_some();
    Some(match action {
        "view.normal" => doc && app.ui.view == ViewMode::Normal,
        "view.sorter" => doc && app.ui.view == ViewMode::Sorter,
        "view.notesPage" => doc && app.ui.view == ViewMode::NotesPage,
        "view.outline" => doc && app.ui.view == ViewMode::Outline,
        "view.reading" => doc && app.ui.view == ViewMode::Reading,
        "view.ruler" => app.ui.ruler,
        "view.gridlines" => app.ui.gridlines,
        "view.guides" => app.ui.guides,
        "view.collapseRibbon" => !app.ui.ribbon_collapsed,
        _ => return None,
    })
}

/// Runs a menu entry: an entry with a shortcut does what that key does, the rest run their command.
pub fn run_menu_entry(app: &mut SlideApp, action: &str, keys: Option<&str>) {
    if let Some(preset) = action.strip_prefix("tool:") {
        app.session.set_tool(deckcraft_engine::ToolKind::Shape { preset: preset.to_string() });
        return;
    }
    if let Some((k, m)) = keys.and_then(parse_keys)
        && ui_shortcut(app, k, m)
    {
        return;
    }
    let (id, p) = split_action(action);
    // `run` puts a failure in the status bar.
    let _ = app.run(id, p);
}

#[cfg(test)]
mod tests {
    use super::palette_items;

    /// #79: the palette listed two "Export…" entries and the first (`file.export`) failed with "missing `path`".
    #[test]
    fn palette_has_one_export_entry_that_opens_the_dialog() {
        let session = deckcraft_engine::Session::with_new();
        for query in ["exp", "export"] {
            let exports: Vec<_> = palette_items(&session, query).into_iter().filter(|(_, label, _)| label == "Export…").collect();
            assert_eq!(exports.len(), 1, "query {query}");
            assert_eq!(exports[0].0, "app.exportDialog");
        }
        // Open…, Save As…, Audio/Video from File… and Notes had the same duplicates: every UI command is the only entry with its label.
        // Commands that need a value (Set Zoom's percent) are left out of the palette.
        for (id, label, _, params) in crate::UI_COMMANDS {
            if deckcraft_engine::cmd::needs_arguments(params) {
                continue;
            }
            let same: Vec<_> = palette_items(&session, &label.to_lowercase()).into_iter().filter(|(_, l, _)| l == label).collect();
            assert_eq!(same.len(), 1, "{label}");
            assert_eq!(same[0].0, *id, "{label}");
        }
    }

    fn items(entries: &[super::Entry], out: &mut Vec<(&'static str, &'static str, Option<&'static str>)>) {
        for e in entries {
            match e {
                super::Entry::Item { label, action, keys } => out.push((label, action, *keys)),
                super::Entry::Sub(_, sub) => items(sub, out),
                super::Entry::Sep => {}
            }
        }
    }

    fn all_items() -> Vec<(&'static str, &'static str, Option<&'static str>)> {
        let mut out = Vec::new();
        for (_, entries) in super::menu_tree() {
            items(&entries, &mut out);
        }
        out
    }

    fn app(session: deckcraft_engine::Session) -> crate::SlideApp {
        crate::SlideApp::new(session, crate::Services::default())
    }

    /// Every entry names a real command with valid params, and its shortcut is the one DeckCraft already binds to it.
    #[test]
    fn menu_entries_are_commands_with_their_own_shortcuts() {
        for (label, action, keys) in all_items() {
            if action.is_empty() {
                assert_eq!(label, "Add-ins…");
                continue;
            }
            if action.starts_with("tool:") {
                continue;
            }
            if let Some((_, p)) = action.split_once(' ') {
                assert!(serde_json::from_str::<serde_json::Value>(p).is_ok_and(|v| v.is_object()), "{label}: {p}");
            }
            let (id, _) = super::split_action(action);
            assert!(crate::ribbon::known(id), "{label}: unknown command {id}");
            let Some(keys) = keys else { continue };
            let parsed = super::parse_keys(keys);
            assert!(parsed.is_some(), "{label}: {keys}");
            let registered = deckcraft_engine::find_command(id)
                .and_then(|c| c.shortcut)
                .or_else(|| crate::UI_COMMANDS.iter().find(|c| c.0 == id).and_then(|c| c.2));
            if registered.and_then(super::parse_keys) == parsed {
                continue;
            }
            // Bound in `ui_shortcut` rather than the registry.
            let (k, m) = parsed.unwrap_or((egui::Key::Escape, Default::default()));
            assert!(super::ui_shortcut(&mut app(deckcraft_engine::Session::with_new()), k, m), "{label}: {keys} isn't DeckCraft's shortcut for {id}");
        }
    }

    #[test]
    fn entries_are_dimmed_when_they_cannot_run() {
        let on = |app: &crate::SlideApp, action| super::menu_enabled(app, action, false);
        let mut a = app(deckcraft_engine::Session::new());
        for action in ["file.new", "app.openDialog", "app.backstage", r#"app.backstage {"page":"new"}"#, "app.about"] {
            assert!(on(&a, action), "{action} with no presentation");
        }
        for action in [
            "file.save",
            "app.print",
            "app.saveAsDialog",
            r#"app.dialog {"id":"table"}"#,
            "view.ruler",
            "tool:rect",
            r#"app.backstage {"page":"info"}"#,
            "",
        ] {
            assert!(!on(&a, action), "{action} with no presentation");
        }
        a = app(deckcraft_engine::Session::with_new());
        for action in ["file.save", r#"app.dialog {"id":"table"}"#, "tool:rect", r#"insert.chart {"type":"Column"}"#] {
            assert!(on(&a, action), "{action}");
        }
        // Nothing is selected and there is nothing to undo.
        for action in ["arrange.group", "edit.undo", r#"app.dialog {"id":"action"}"#, r#"view.pane {"pane":"format"}"#, "picture.crop", ""] {
            assert!(!on(&a, action), "{action}");
        }
        // A text field takes Undo and the clipboard.
        assert!(super::menu_enabled(&a, "edit.undo", true));
        // An open dialog takes the keyboard and the mouse, but its own text fields keep the clipboard.
        a.dialog = Some(crate::dialogs::Dialog::new("about"));
        assert!(!on(&a, "file.save"));
        assert!(!on(&a, "window.next"), "the dialog's values would go to the other presentation");
        assert!(super::menu_enabled(&a, "edit.paste", true));
        // The File tab hides the presentation: nothing edits it, but Save still works.
        a.dialog = None;
        a.run("app.backstage", serde_json::json!({})).unwrap();
        assert!(!on(&a, "slide.delete") && !on(&a, "slide.new"));
        assert!(on(&a, "file.save"));
    }

    #[test]
    fn view_entries_show_the_current_view() {
        let mut a = app(deckcraft_engine::Session::with_new());
        assert_eq!(super::menu_checked(&a, "view.normal"), Some(true));
        assert_eq!(super::menu_checked(&a, "view.collapseRibbon"), Some(true));
        super::run_menu_entry(&mut a, "view.sorter", None);
        assert_eq!(super::menu_checked(&a, "view.normal"), Some(false));
        assert_eq!(super::menu_checked(&a, "view.sorter"), Some(true));
        assert_eq!(super::menu_checked(&a, "file.save"), None);
    }

    #[test]
    fn running_entries() {
        let mut a = app(deckcraft_engine::Session::with_new());
        // Open Recent is the start screen with its recent presentations.
        super::run_menu_entry(&mut a, "app.backstage", None);
        assert_eq!(a.backstage.as_ref().map(|b| b.page), Some(crate::backstage::Page::Home));
        a.backstage = None;
        let shapes = |a: &crate::SlideApp| a.session.active().map_or(0, |d| d.shapes().len());
        let before = shapes(&a);
        super::run_menu_entry(&mut a, r#"insert.chart {"type":"Column"}"#, None);
        assert_eq!(shapes(&a), before + 1);
        super::run_menu_entry(&mut a, "tool:ellipse", None);
        assert_eq!(a.session.tool.kind, deckcraft_engine::ToolKind::Shape { preset: "ellipse".into() });
        // An entry with a shortcut does what the key does: Cmd+Shift+N adds a slide.
        let slides = |a: &crate::SlideApp| a.session.active().map_or(0, |d| d.doc.slides.len());
        let n = slides(&a);
        super::run_menu_entry(&mut a, "slide.new", Some("Cmd+Shift+N"));
        assert_eq!(slides(&a), n + 1);
    }
}
