//! UI-level shortcuts, the command palette, and the menu structure (shared with the native macOS
//! menu bar in the desktop app).

use deckcraft_engine::Mods;
use serde_json::json;

use crate::SlideApp;
use crate::theme::{self, Tokens};

/// Shortcuts the UI handles itself (views, show, zoom, palette, file dialogs). True when consumed.
pub fn ui_shortcut(app: &mut SlideApp, key: egui::Key, m: Mods) -> bool {
    use egui::Key::*;
    let id = match (key, m.cmd, m.shift, m.alt) {
        (F5, _, false, _) => {
            app.start_show(0, false);
            return true;
        }
        (F5, _, true, _) | (Enter, true, false, false) => {
            let i = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            app.start_show(i, false);
            return true;
        }
        (Enter, true, true, _) => {
            app.start_show(0, false);
            return true;
        }
        (S, true, false, false) => {
            app.save();
            return true;
        }
        (S, true, true, false) => "app.saveAsDialog",
        (O, true, false, false) => "app.openDialog",
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
        (F, true, false, false) => {
            app.palette = Some((String::new(), 0));
            return true;
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
        .filter(|c| c.enabled && !crate::UI_COMMANDS.iter().any(|u| u.1 == c.label))
        .map(|c| (c.id.to_string(), c.label.to_string(), c.shortcut))
        .chain(crate::UI_COMMANDS.iter().map(|c| (c.0.to_string(), c.1.to_string(), c.2)))
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

/// The application menu tree: (menu, [(label, command id, shortcut)]), "-" = separator. Used by the
/// native macOS menu bar and as the parity catalogue's live side.
pub fn menu_tree() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        (
            "File",
            vec![
                ("New Presentation", "file.new"),
                ("Open…", "app.openDialog"),
                ("-", ""),
                ("Close", "file.close"),
                ("Save", "file.save"),
                ("Save As…", "app.saveAsDialog"),
                ("Export…", "app.exportDialog"),
                ("-", ""),
                ("Properties", "file.properties"),
            ],
        ),
        (
            "Edit",
            vec![
                ("Undo", "edit.undo"),
                ("Redo", "edit.redo"),
                ("-", ""),
                ("Cut", "edit.cut"),
                ("Copy", "edit.copy"),
                ("Paste", "edit.paste"),
                ("Paste and Match Formatting", "edit.pasteText"),
                ("-", ""),
                ("Select All", "edit.selectAll"),
                ("Duplicate", "edit.duplicate"),
                ("Delete Slide", "slide.delete"),
                ("-", ""),
                ("Find…", "app.palette"),
            ],
        ),
        (
            "View",
            vec![
                ("Normal", "view.normal"),
                ("Slide Sorter", "view.sorter"),
                ("Notes Page", "view.notesPage"),
                ("Outline View", "view.outline"),
                ("Reading View", "view.reading"),
                ("Slide Show", "show.start"),
                ("-", ""),
                ("Slide Master", "view.slideMaster"),
                ("-", ""),
                ("Ruler", "view.ruler"),
                ("Gridlines", "view.gridlines"),
                ("Guides", "view.guides"),
                ("-", ""),
                ("Zoom In", "view.zoomIn"),
                ("Zoom Out", "view.zoomOut"),
                ("Fit to Window", "view.fit"),
            ],
        ),
        (
            "Insert",
            vec![
                ("New Slide", "slide.new"),
                ("Duplicate Slide", "slide.duplicate"),
                ("-", ""),
                ("Section", "section.add"),
                ("Text Box", "insert.textBox"),
                ("WordArt", "insert.wordArt"),
                ("Date and Time", "insert.dateTime"),
                ("Slide Number", "insert.slideNumber"),
                ("-", ""),
                ("Table…", "insert.table"),
                ("Chart", "insert.chart"),
                ("Picture from File…", "app.insertPictureDialog"),
                ("Audio from File…", "app.insertAudioDialog"),
                ("Video from File…", "app.insertVideoDialog"),
            ],
        ),
        (
            "Format",
            vec![
                ("Bold", "format.bold"),
                ("Italic", "format.italic"),
                ("Underline", "format.underline"),
                ("-", ""),
                ("Align Left", "format.alignLeft"),
                ("Center", "format.alignCenter"),
                ("Align Right", "format.alignRight"),
                ("Justify", "format.justify"),
                ("-", ""),
                ("Right-to-Left Text Direction", "format.rtl"),
            ],
        ),
        (
            "Arrange",
            vec![
                ("Bring to Front", "arrange.bringToFront"),
                ("Send to Back", "arrange.sendToBack"),
                ("Bring Forward", "arrange.bringForward"),
                ("Send Backward", "arrange.sendBackward"),
                ("-", ""),
                ("Group", "arrange.group"),
                ("Ungroup", "arrange.ungroup"),
                ("Regroup", "arrange.regroup"),
                ("-", ""),
                ("Rotate Left 90°", "arrange.rotateLeft"),
                ("Rotate Right 90°", "arrange.rotateRight"),
                ("Flip Horizontal", "arrange.flipHorizontal"),
                ("Flip Vertical", "arrange.flipVertical"),
            ],
        ),
        (
            "Slide Show",
            vec![
                ("Play from Start", "show.fromStart"),
                ("Play from Current Slide", "show.fromCurrent"),
                ("-", ""),
                ("Hide Slide", "slide.hide"),
                ("Set Up Show…", "show.setup"),
            ],
        ),
        ("Help", vec![("About DeckCraft", "app.about"), ("Command Palette", "app.palette")]),
    ]
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
        for (id, label, _, _) in crate::UI_COMMANDS {
            let same: Vec<_> = palette_items(&session, &label.to_lowercase()).into_iter().filter(|(_, l, _)| l == label).collect();
            assert_eq!(same.len(), 1, "{label}");
            assert_eq!(same[0].0, *id, "{label}");
        }
    }
}
