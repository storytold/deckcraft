use super::{Icon, SlideApp, Target, Ui, big, big_button, group, json, rows, run};

/// Preserve, and whether the open master or layout has a title or any footer placeholder.
fn master_checks(app: &SlideApp) -> (bool, bool, bool) {
    let Some(st) = app.session.active() else { return (false, false, false) };
    let preserve = match st.selection.target {
        Target::Master { master } => st.doc.masters.get(master).is_some_and(|m| m.preserve),
        Target::Layout { master, layout } => st.doc.masters.get(master).and_then(|m| m.layouts.get(layout)).is_some_and(|l| l.preserve),
        Target::Slides | Target::Handout | Target::NotesMaster => false,
    };
    let title = st.shapes().iter().any(|sh| sh.ph_type().is_some_and(|k| k.is_title()));
    let footers = st.shapes().iter().any(|sh| sh.ph_type().is_some_and(|k| k.is_footer_kind()));
    (preserve, title, footers)
}

pub(super) fn slide_master(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        big(app, ui, Icon::NewSlide, "Insert\nLayout", "master.insertLayout", json!({}));
        big(app, ui, Icon::DeleteSlide, "Delete", "master.deleteLayout", json!({}));
        big(app, ui, Icon::Layout, "Rename", "app.dialog", json!({"id": "renameLayout"}));
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            let (preserve, title, footers) = master_checks(app);
            let mut on = preserve;
            if ui.checkbox(&mut on, "Preserve").changed() {
                run(app, "master.preserve", json!({"preserve": on}));
            }
            let mut on = title;
            if ui.checkbox(&mut on, "Title").changed() {
                run(app, "master.placeholders", json!({"title": on}));
            }
            let mut on = footers;
            if ui.checkbox(&mut on, "Footers").changed() {
                run(app, "master.placeholders", json!({"footers": on}));
            }
        });
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Table, "Insert\nPlaceholder", true);
        egui::Popup::menu(&r).show(|ui| {
            for (l, k) in
                [("Content", "content"), ("Text", "text"), ("Picture", "picture"), ("Chart", "chart"), ("Table", "table"), ("Media", "media")]
            {
                if ui.button(l).clicked() {
                    run(app, "master.insertPlaceholder", json!({"kind": k}));
                }
            }
        });
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Themes, "Themes", true);
        egui::Popup::menu(&r).show(|ui| {
            for th in deckcraft_model::theme::builtin_themes() {
                if ui.button(&th.name).clicked() {
                    run(app, "design.theme", json!({"name": th.name}));
                }
            }
            ui.separator();
            if ui.button("Browse for Themes…").clicked() {
                run(app, "app.browseThemeDialog", json!({}));
            }
        });
        big(app, ui, Icon::FormatBackground, "Background\nStyles", "view.pane", json!({"pane": "background"}));
    });
    group(ui, |ui| {
        if big_button(ui, Icon::Close, "Close\nMaster View", true).clicked() {
            run(app, "view.closeMaster", json!({}));
        }
    });
}

pub(super) fn notes_master(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        use deckcraft_model::PhType;
        let shown = |app: &SlideApp, k: PhType| {
            app.session.active().and_then(|d| d.doc.notes_master.as_ref()).is_some_and(|m| m.shapes.iter().any(|s| s.ph_type() == Some(k)))
        };
        for col in [
            [("Header", "header", PhType::Header), ("Slide Image", "slideImage", PhType::SlideImage), ("Footer", "footer", PhType::Footer)],
            [("Date", "date", PhType::Date), ("Body", "body", PhType::Body), ("Page Number", "pageNumber", PhType::SlideNum)],
        ] {
            rows(ui, |ui| {
                for (label, key, k) in col {
                    let mut on = shown(app, k);
                    if ui.checkbox(&mut on, label).changed() {
                        run(app, "notes.placeholders", json!({ key: on }));
                    }
                }
            });
        }
    });
    group(ui, |ui| {
        big(app, ui, Icon::FormatBackground, "Background\nStyles", "view.pane", json!({"pane": "background"}));
    });
    group(ui, |ui| {
        if big_button(ui, Icon::Close, "Close\nMaster View", true).clicked() {
            run(app, "view.closeMaster", json!({}));
        }
    });
}

pub(super) fn handout_master(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let r = big_button(ui, Icon::Layout, &format!("Slides Per\nPage: {}", app.ui.handout_per_page), true);
        egui::Popup::menu(&r).show(|ui| {
            for n in [1u8, 2, 3, 4, 6, 9] {
                if ui.selectable_label(app.ui.handout_per_page == n, format!("{n} Slide{}", if n == 1 { "" } else { "s" })).clicked() {
                    app.ui.handout_per_page = n;
                }
            }
        });
    });
    group(ui, |ui| {
        let shown = |app: &SlideApp, k: deckcraft_model::PhType| {
            app.session.active().and_then(|d| d.doc.handout_master.as_ref()).is_some_and(|m| m.shapes.iter().any(|s| s.ph_type() == Some(k)))
        };
        rows(ui, |ui| {
            use deckcraft_model::PhType;
            for (label, key, k) in [
                ("Header", "header", PhType::Header),
                ("Date", "date", PhType::Date),
                ("Footer", "footer", PhType::Footer),
                ("Page Number", "pageNumber", PhType::SlideNum),
            ] {
                let mut on = shown(app, k);
                if ui.checkbox(&mut on, label).changed() {
                    run(app, "handout.placeholders", json!({ key: on }));
                }
            }
        });
    });
    group(ui, |ui| {
        big(app, ui, Icon::FormatBackground, "Background\nStyles", "view.pane", json!({"pane": "background"}));
    });
    group(ui, |ui| {
        if big_button(ui, Icon::Close, "Close\nMaster View", true).clicked() {
            run(app, "view.closeMaster", json!({}));
        }
    });
}
