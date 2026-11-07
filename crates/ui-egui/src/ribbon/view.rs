use super::{Icon, SlideApp, Target, Ui, big_button, big_button_ex, group, json, rows, run, theme};

pub(super) fn view(app: &mut SlideApp, ui: &mut Ui) {
    use crate::ViewMode;
    group(ui, |ui| {
        for (icon, label, id, mode) in [
            (Icon::ViewNormal, "Normal", "view.normal", Some(ViewMode::Normal)),
            (Icon::ViewOutline, "Outline\nView", "view.outline", Some(ViewMode::Outline)),
            (Icon::ViewSorter, "Slide\nSorter", "view.sorter", Some(ViewMode::Sorter)),
            (Icon::ViewNotesPage, "Notes\nPage", "view.notesPage", Some(ViewMode::NotesPage)),
            (Icon::ViewReading, "Reading\nView", "view.reading", None),
        ] {
            let sel = mode.is_some_and(|m| app.ui.view == m);
            if big_button_ex(ui, icon, label, true, false, sel).0.clicked() {
                let _ = app.run(id, json!({}));
            }
        }
    });
    group(ui, |ui| {
        let in_master = app.session.active().is_some_and(|d| d.selection.target != Target::Slides);
        if big_button_ex(ui, Icon::SlideMaster, "Slide\nMaster", true, false, in_master).0.clicked() {
            let _ = app.run(if in_master { "view.closeMaster" } else { "view.slideMaster" }, json!({}));
        }
        let in_handout = app.session.active().is_some_and(|d| d.selection.target == Target::Handout);
        if big_button_ex(ui, Icon::HandoutMaster, "Handout\nMaster", true, false, in_handout).0.clicked() {
            let _ = app.run(if in_handout { "view.closeMaster" } else { "view.handoutMaster" }, json!({}));
        }
        let in_notes = app.session.active().is_some_and(|d| d.selection.target == Target::NotesMaster);
        if big_button_ex(ui, Icon::NotesMaster, "Notes\nMaster", true, false, in_notes).0.clicked() {
            let _ = app.run(if in_notes { "view.closeMaster" } else { "view.notesMaster" }, json!({}));
        }
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            let mut r = app.ui.ruler;
            if ui.checkbox(&mut r, crate::tl!("Ruler")).changed() {
                app.ui.ruler = r;
            }
            let mut g = app.ui.gridlines;
            if ui.checkbox(&mut g, crate::tl!("Gridlines")).changed() {
                app.ui.gridlines = g;
            }
            let mut gd = app.ui.guides;
            if ui.checkbox(&mut gd, crate::tl!("Guides")).changed() {
                app.ui.guides = gd;
            }
        });
        if big_button(ui, Icon::Gridlines, "Grid\nOptions", true).clicked() {
            let _ = app.run("view.gridOptions", json!({}));
        }
        let n = app.ui.notes;
        if big_button_ex(ui, Icon::Notes, "Notes", true, false, n).0.clicked() {
            app.ui.notes = !n;
        }
    });
    group(ui, |ui| {
        let (main, arrow) = big_button_ex(ui, Icon::ZoomGlass, "Zoom", true, true, false);
        if main.on_hover_text("Zoom").clicked() {
            let _ = app.run("view.zoomDialog", json!({}));
        }
        if let Some(a) = arrow {
            egui::Popup::menu(&a).show(|ui| {
                for pct in [400, 200, 150, 100, 75, 66, 50, 33] {
                    if ui.button(format!("{pct}%")).clicked() {
                        let _ = app.run("view.zoom", json!({"percent": pct}));
                        ui.close();
                    }
                }
                if ui.button(crate::tl!("Fit")).clicked() {
                    let _ = app.run("view.fit", json!({}));
                    ui.close();
                }
            });
        }
        if big_button(ui, Icon::FitToWindow, "Fit to\nWindow", true).clicked() {
            app.ui.zoom = None;
        }
    });
    group(ui, |ui| {
        let has = app.session.active().is_some();
        let on = app.doc_window.is_some();
        let resp = big_button_ex(ui, Icon::NewWindow, "New\nWindow", has, false, on).0;
        if resp.on_hover_text("Open another window on this presentation").clicked() {
            let _ = app.run("view.newWindow", json!({}));
        }
    });
    group(ui, |ui| {
        let gs = app.ui.grayscale;
        if big_button_ex(ui, Icon::Grayscale, "Grayscale", true, false, gs).0.clicked() {
            app.ui.grayscale = !gs;
        }
        let dark = app.ui.brightness == theme::Brightness::Dark;
        if big_button_ex(ui, Icon::Palette, "Dark\nMode", true, false, dark).0.clicked() {
            let _ = app.run("view.dark", json!({}));
        }
    });
}

/// Insert › Zoom: pick Summary, Section or Slide Zoom and what it zooms to, as PowerPoint's
/// Insert Zoom dialogs do (a checklist of slides or sections).
pub(super) fn zoom_menu(app: &mut SlideApp, ui: &mut Ui) {
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let id = egui::Id::unique("insert_zoom_menu");
    let (mut kind, mut picked): (u8, Vec<usize>) = ui.data_mut(|d| d.get_temp(id).unwrap_or((0, vec![])));
    ui.horizontal(|ui| {
        for (k, label) in [(0u8, crate::tl!("Slide Zoom")), (1, crate::tl!("Section Zoom")), (2, crate::tl!("Summary Zoom"))] {
            if ui.selectable_label(kind == k, label).clicked() && kind != k {
                kind = k;
                picked.clear();
            }
        }
    });
    ui.separator();
    let items: Vec<String> = if kind == 1 {
        doc.sections.iter().map(|s| s.name.clone()).collect()
    } else {
        doc.slides
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let t = s.title();
                format!("{}. {}", i + 1, t.lines().next().unwrap_or("").trim())
            })
            .collect()
    };
    if items.is_empty() {
        ui.label(if kind == 1 { crate::tl!("The presentation has no sections. Add one with Home › Section.") } else { crate::tl!("No slides.") });
    }
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        for (i, label) in items.iter().enumerate() {
            let mut on = picked.contains(&i);
            if ui.checkbox(&mut on, label).changed() {
                if on {
                    picked.push(i);
                } else {
                    picked.retain(|x| *x != i);
                }
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(id, (kind, picked.clone())));
    ui.separator();
    if ui.add_enabled(!picked.is_empty(), egui::Button::new(crate::tl!("Insert"))).clicked() {
        let mut v = picked.clone();
        v.sort_unstable();
        let p = match kind {
            1 => json!({"kind": "section", "sections": v}),
            2 => json!({"kind": "summary", "slides": v}),
            _ => json!({"kind": "slide", "slides": v}),
        };
        run(app, "insert.zoom", p);
        ui.data_mut(|d| d.remove::<(u8, Vec<usize>)>(id));
        ui.close();
    }
}
