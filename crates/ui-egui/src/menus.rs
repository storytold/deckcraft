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
        (H, true, false, false) => "app.replace",
        _ => return false,
    };
    let _ = app.run(id, json!({}));
    true
}

/// The palette's entries for the lowercase query `q`. An engine command is hidden when a UI command has the same label: Open…, Save As…,
/// Export… and Audio/Video from File… need a `path` the palette can't give, so the one entry left opens the dialog instead of failing.
fn palette_items(session: &deckcraft_engine::Session, q: &str) -> Vec<(String, String, Option<&'static str>)> {
    // Search the English label (the catalog key) and the translated one, so a query typed in either
    // language finds the command; what is listed is the current interface language's label.
    let matched = |label: &str, id: &str| {
        q.is_empty() || label.to_lowercase().contains(q) || crate::i18n::t(label).to_lowercase().contains(q) || id.to_lowercase().contains(q)
    };
    let mut items: Vec<(String, String, Option<&'static str>)> = session
        .commands()
        .into_iter()
        // Commands that need a path or other value fail when the palette runs them with `{}`.
        .filter(|c| c.enabled && !crate::UI_COMMANDS.iter().any(|u| u.1 == c.label) && !deckcraft_engine::cmd::needs_arguments(c.params))
        .filter(|c| matched(c.label, c.id))
        .map(|c| (c.id.to_string(), tl!(c.label).to_string(), c.shortcut))
        .chain(
            crate::UI_COMMANDS
                .iter()
                .filter(|c| !deckcraft_engine::cmd::needs_arguments(c.3) && matched(c.1, c.0))
                .map(|c| (c.0.to_string(), tl!(c.1).to_string(), c.2)),
        )
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
            let r = ui
                .add(egui::TextEdit::singleline(&mut query).hint_text(tl!("Search commands…")).desired_width(f32::INFINITY).font(theme::font(15.0)));
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
            tl!("File"),
            vec![
                (tl!("New Presentation"), "file.new"),
                (tl!("Open…"), "app.openDialog"),
                ("-", ""),
                (tl!("Close"), "app.closePresentation"),
                (tl!("Save"), "file.save"),
                (tl!("Save As…"), "app.saveAsDialog"),
                (tl!("Export…"), "app.exportDialog"),
                (tl!("Print…"), "app.print"),
                ("-", ""),
                (tl!("Properties"), "file.properties"),
            ],
        ),
        (
            tl!("Edit"),
            vec![
                (tl!("Undo"), "edit.undo"),
                (tl!("Redo"), "edit.redo"),
                ("-", ""),
                (tl!("Cut"), "edit.cut"),
                (tl!("Copy"), "edit.copy"),
                (tl!("Paste"), "edit.paste"),
                (tl!("Paste and Match Formatting"), "edit.pasteText"),
                (tl!("Paste Special…"), "app.pasteSpecial"),
                ("-", ""),
                (tl!("Select All"), "edit.selectAll"),
                (tl!("Duplicate"), "edit.duplicate"),
                (tl!("Delete Slide"), "slide.delete"),
                ("-", ""),
                (tl!("Find…"), "app.find"),
                (tl!("Replace…"), "app.replace"),
            ],
        ),
        (
            tl!("View"),
            vec![
                (tl!("Normal"), "view.normal"),
                (tl!("Slide Sorter"), "view.sorter"),
                (tl!("Notes Page"), "view.notesPage"),
                (tl!("Outline View"), "view.outline"),
                (tl!("Reading View"), "view.reading"),
                (tl!("Slide Show"), "show.start"),
                ("-", ""),
                (tl!("Slide Master"), "view.slideMaster"),
                (tl!("Handout Master"), "view.handoutMaster"),
                (tl!("Notes Master"), "view.notesMaster"),
                ("-", ""),
                (tl!("Ruler"), "view.ruler"),
                (tl!("Gridlines"), "view.gridlines"),
                (tl!("Guides"), "view.guides"),
                (tl!("Grid Options…"), "view.gridOptions"),
                ("-", ""),
                (tl!("Zoom…"), "view.zoomDialog"),
                (tl!("Zoom In"), "view.zoomIn"),
                (tl!("Zoom Out"), "view.zoomOut"),
                (tl!("Fit to Window"), "view.fit"),
                ("-", ""),
                (tl!("New Window"), "view.newWindow"),
            ],
        ),
        (
            tl!("Insert"),
            vec![
                (tl!("New Slide"), "slide.new"),
                (tl!("Duplicate Slide"), "slide.duplicate"),
                ("-", ""),
                (tl!("Section"), "section.add"),
                (tl!("Text Box"), "insert.textBox"),
                (tl!("WordArt"), "insert.wordArt"),
                (tl!("Date and Time"), "insert.dateTime"),
                (tl!("Slide Number"), "insert.slideNumber"),
                ("-", ""),
                (tl!("Table…"), "insert.table"),
                (tl!("Chart"), "insert.chart"),
                (tl!("Picture from File…"), "app.insertPictureDialog"),
                (tl!("Photo Album…"), "insert.photoAlbum"),
                (tl!("Edit Photo Album…"), "insert.photoAlbumEdit"),
                (tl!("Audio from File…"), "app.insertAudioDialog"),
                (tl!("Video from File…"), "app.insertVideoDialog"),
            ],
        ),
        (
            tl!("Format"),
            vec![
                (tl!("Font…"), "format.fontDialog"),
                (tl!("Paragraph…"), "format.paragraphDialog"),
                (tl!("Bold"), "format.bold"),
                (tl!("Italic"), "format.italic"),
                (tl!("Underline"), "format.underline"),
                ("-", ""),
                (tl!("Align Left"), "format.alignLeft"),
                (tl!("Center"), "format.alignCenter"),
                (tl!("Align Right"), "format.alignRight"),
                (tl!("Justify"), "format.justify"),
                ("-", ""),
                (tl!("Size and Position…"), "shape.sizePosition"),
                (tl!("Right-to-Left Text Direction"), "format.rtl"),
            ],
        ),
        (
            tl!("Arrange"),
            vec![
                (tl!("Bring to Front"), "arrange.bringToFront"),
                (tl!("Send to Back"), "arrange.sendToBack"),
                (tl!("Bring Forward"), "arrange.bringForward"),
                (tl!("Send Backward"), "arrange.sendBackward"),
                ("-", ""),
                (tl!("Group"), "arrange.group"),
                (tl!("Ungroup"), "arrange.ungroup"),
                (tl!("Regroup"), "arrange.regroup"),
                ("-", ""),
                (tl!("Rotate Left 90°"), "arrange.rotateLeft"),
                (tl!("Rotate Right 90°"), "arrange.rotateRight"),
                (tl!("Flip Horizontal"), "arrange.flipHorizontal"),
                (tl!("Flip Vertical"), "arrange.flipVertical"),
            ],
        ),
        (
            tl!("Slide Show"),
            vec![
                (tl!("Play from Start"), "show.fromStart"),
                (tl!("Play from Current Slide"), "show.fromCurrent"),
                ("-", ""),
                (tl!("Hide Slide"), "slide.hide"),
                (tl!("Set Up Show…"), "show.setup"),
            ],
        ),
        (tl!("Help"), vec![(tl!("About DeckCraft"), "app.about"), (tl!("Command Palette"), "app.palette")]),
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
}
