use super::{Hit, SlideApp, json};

pub(super) fn context_menu(app: &mut SlideApp, resp: &egui::Response) {
    // Right-clicking a vertex in Edit Points picks it for the point menu.
    if resp.secondary_clicked()
        && let Some(Hit::Vertex { path, index, .. }) = app.session.tool.hover.clone()
    {
        app.session.tool.edit_vertex = Some((path, index));
    }
    resp.context_menu(|ui| {
        let editing = app.session.active().and_then(|d| app.session.tool.editing_points(d).and_then(|id| d.shape(id).cloned()));
        if let Some(sh) = editing {
            edit_points_menu(app, ui, &sh);
            return;
        }
        // A misspelt word under the pointer: its suggestions, then Ignore All and Add to Dictionary.
        if let Some(w) = app.squiggles.menu_word.clone() {
            spelling_menu(app, ui, &w);
        }
        let has_sel = app.session.active().is_some_and(|d| !d.selection.shapes.is_empty());
        for (l, id) in [("Cut", "edit.cut"), ("Copy", "edit.copy"), ("Paste", "edit.paste")] {
            if ui.add_enabled(crate::ribbon::enabled(app, id), egui::Button::new(l)).clicked() {
                let _ = app.run(id, json!({}));
                ui.close();
            }
        }
        ui.separator();
        // Right-clicking the text being edited keeps it (`PointerKind::ContextClick`): text items.
        let editing = app.session.active().is_some_and(|d| d.selection.text.as_ref().is_some_and(|t| !t.notes));
        if editing {
            text_items(app, ui);
        } else if has_sel {
            shape_items(app, ui);
        } else {
            slide_items(app, ui);
        }
    });
}

fn edit_points_menu(app: &mut SlideApp, ui: &mut egui::Ui, sh: &deckcraft_model::Shape) {
    if let Some((path, index)) = app.session.tool.edit_segment {
        let cur = deckcraft_engine::cmd::points::segment_kind_of(sh, path, index);
        for (l, k) in [("Straight Segment", "straight"), ("Curved Segment", "curved")] {
            if ui.radio(cur == Some(k), l).clicked() {
                let _ = app.run("shape.segmentType", json!({"path": path, "segment": index, "type": k}));
                ui.close();
            }
        }
        // Each of these renumbers the points, so the picked segment is dropped.
        if ui.button("Add Point").clicked() {
            let t = app.session.tool.edit_segment_t.unwrap_or(0.5);
            let _ = app.run("shape.addPoint", json!({"path": path, "segment": index, "t": t}));
            app.session.tool.edit_segment = None;
            ui.close();
        }
        let closed = deckcraft_engine::cmd::points::closed_at(sh, path, index);
        if ui.add_enabled(closed == Some(true), egui::Button::new("Delete Segment")).clicked() {
            let _ = app.run("shape.deleteSegment", json!({"path": path, "segment": index}));
            app.session.tool.edit_segment = None;
            ui.close();
        }
        // Open Path opens the outline at the segment's start point.
        let (label, id, at) = match closed {
            Some(false) => ("Close Path", "shape.closePath", index),
            _ => ("Open Path", "shape.openPath", deckcraft_engine::cmd::points::segment_start_of(sh, path, index).unwrap_or(index)),
        };
        if ui.button(label).clicked() {
            let _ = app.run(id, json!({"path": path, "index": at}));
            app.session.tool.edit_segment = None;
            ui.close();
        }
        ui.separator();
    } else if let Some((path, index)) = app.session.tool.edit_vertex {
        let cur = deckcraft_engine::cmd::points::kind_of(sh, path, index);
        for (l, k) in [("Smooth Point", "smooth"), ("Straight Point", "straight"), ("Corner Point", "corner")] {
            if ui.radio(cur == Some(k), l).clicked() {
                let _ = app.run("shape.pointType", json!({"path": path, "index": index, "type": k}));
                ui.close();
            }
        }
        if ui.button("Delete Point").clicked() {
            let _ = app.run("shape.deletePoint", json!({"path": path, "index": index}));
            app.session.tool.edit_vertex = None;
            ui.close();
        }
        let (label, id) = match deckcraft_engine::cmd::points::closed_at(sh, path, index) {
            Some(false) => ("Close Path", "shape.closePath"),
            _ => ("Open Path", "shape.openPath"),
        };
        if ui.button(label).clicked() {
            let _ = app.run(id, json!({"path": path, "index": index}));
            // The points are numbered from the new start now.
            app.session.tool.edit_vertex = None;
            ui.close();
        }
        ui.separator();
    }
    if ui.button("Exit Edit Points").clicked() {
        let _ = app.run("shape.editPoints", json!({"on": false}));
        ui.close();
    }
}

fn spelling_menu(app: &mut SlideApp, ui: &mut egui::Ui, w: &crate::squiggles::MenuWord) {
    if let Some(from) = w.delete_from {
        if ui.button(egui::RichText::new("Delete Repeated Word").strong()).clicked() {
            let cut: String = format!("{}{}", " ".repeat(w.start - from), w.word);
            // The gap is spaces; take it from the text so tabs or no-break spaces match.
            let gap = app
                .session
                .active()
                .and_then(|st| match w.tag {
                    crate::squiggles::Tag::Shape { id, para } => st.shape(id)?.text.as_ref()?.paragraphs.get(para).map(|p| p.text()),
                    crate::squiggles::Tag::Cell { id, row, col, para } => match &st.shape(id)?.kind {
                        deckcraft_model::ShapeKind::Table(tb) => tb.cell(row, col)?.text.paragraphs.get(para).map(|p| p.text()),
                        _ => None,
                    },
                    crate::squiggles::Tag::Notes { .. } => None,
                })
                .map(|t| t.chars().skip(from).take(w.start + w.word.chars().count() - from).collect::<String>())
                .unwrap_or(cut);
            let mut p = json!({"para": 0, "start": from, "word": gap, "to": ""});
            if let Some(o) = p.as_object_mut() {
                match w.tag {
                    crate::squiggles::Tag::Shape { id, para } => {
                        o.insert("shape".into(), json!(id));
                        o.insert("para".into(), json!(para));
                    }
                    crate::squiggles::Tag::Cell { id, row, col, para } => {
                        o.insert("shape".into(), json!(id));
                        o.insert("row".into(), json!(row));
                        o.insert("col".into(), json!(col));
                        o.insert("para".into(), json!(para));
                    }
                    crate::squiggles::Tag::Notes { .. } => {}
                }
            }
            let _ = app.run("review.spellingChange", p);
            app.squiggles.menu_word = None;
            ui.close();
        }
    } else if w.suggestions.is_empty() {
        ui.label(egui::RichText::new("(no spelling suggestions)").weak());
    }
    for to in &w.suggestions {
        if ui.button(egui::RichText::new(to).strong()).clicked() {
            let p = match w.tag {
                crate::squiggles::Tag::Shape { id, para } => json!({"shape": id, "para": para, "start": w.start, "word": w.word, "to": to}),
                crate::squiggles::Tag::Cell { id, row, col, para } => {
                    json!({"shape": id, "row": row, "col": col, "para": para, "start": w.start, "word": w.word, "to": to})
                }
                crate::squiggles::Tag::Notes { .. } => json!({}),
            };
            let _ = app.run("review.spellingChange", p);
            app.squiggles.menu_word = None;
            ui.close();
        }
    }
    if ui.button("Ignore All").clicked() {
        app.squiggles.ignore(&w.word);
        app.squiggles.menu_word = None;
        ui.close();
    }
    if ui.button("Add to Dictionary").clicked() {
        if let Err(e) = app.session.execute("review.addToDictionary", &json!({"word": w.word})) {
            app.set_status(e.to_string());
        }
        app.squiggles.menu_word = None;
        ui.close();
    }
    ui.separator();
}

fn text_items(app: &mut SlideApp, ui: &mut egui::Ui) {
    if ui.button("Paragraph…").clicked() {
        app.dialog = Some(crate::dialogs::Dialog::new("paragraph"));
        ui.close();
    }
    for (l, id) in [("Bullets", "format.bullets"), ("Numbering", "format.numbering")] {
        if ui.add_enabled(crate::ribbon::enabled(app, id), egui::Button::new(l)).clicked() {
            let _ = app.run(id, json!({}));
            ui.close();
        }
    }
    if ui.button("Link…").clicked() {
        app.dialog = Some(crate::dialogs::Dialog::new("hyperlink"));
        ui.close();
    }
    ui.separator();
    if ui.button("Format Shape…").clicked() {
        let _ = app.run("view.pane", json!({"pane": "format"}));
        ui.close();
    }
}

fn shape_items(app: &mut SlideApp, ui: &mut egui::Ui) {
    if ui.button("Edit Text").clicked() {
        let _ = app.run("text.edit", json!({}));
        ui.close();
    }
    if crate::ribbon::enabled(app, "shape.editPoints")
        && app.session.active().is_some_and(|d| d.selection.shapes.len() == 1 && d.selected_shapes().iter().all(|s| !s.is_line()))
        && ui.button("Edit Points").clicked()
    {
        let _ = app.run("shape.editPoints", json!({"on": true}));
        ui.close();
    }
    if ui.button("Font…").clicked() {
        let _ = app.run("format.fontDialog", json!({}));
        ui.close();
    }
    if ui.button("Paragraph…").clicked() {
        let _ = app.run("format.paragraphDialog", json!({}));
        ui.close();
    }
    ui.menu_button("Bring to Front", |ui| {
        crate::ribbon::menu_item(app, ui, crate::icons::Icon::BringToFront, "Bring to Front", "arrange.bringToFront", json!({}));
        crate::ribbon::menu_item(app, ui, crate::icons::Icon::BringForward, "Bring Forward", "arrange.bringForward", json!({}));
    });
    ui.menu_button("Send to Back", |ui| {
        crate::ribbon::menu_item(app, ui, crate::icons::Icon::SendToBack, "Send to Back", "arrange.sendToBack", json!({}));
        crate::ribbon::menu_item(app, ui, crate::icons::Icon::SendBackward, "Send Backward", "arrange.sendBackward", json!({}));
    });
    ui.menu_button("Group", |ui| {
        crate::ribbon::menu_item(app, ui, crate::icons::Icon::Group, "Group", "arrange.group", json!({}));
        crate::ribbon::menu_item(app, ui, crate::icons::Icon::Ungroup, "Ungroup", "arrange.ungroup", json!({}));
    });
    let locked = app.session.active().is_some_and(|d| d.selected_shapes().iter().all(|s| s.locked));
    if ui.button(if locked { "Unlock" } else { "Lock" }).clicked() {
        let _ = app.run("shape.lock", json!({"locked": !locked}));
        ui.close();
    }
    let picture =
        app.session.active().is_some_and(|d| d.selected_shapes().iter().any(|s| matches!(s.kind, deckcraft_model::ShapeKind::Picture { .. })));
    if picture && ui.button("Change Picture…").clicked() {
        let _ = app.run("app.changePictureDialog", json!({}));
        ui.close();
    }
    if ui.button("Link…").clicked() {
        app.dialog = Some(crate::dialogs::Dialog::new("hyperlink"));
        ui.close();
    }
    if ui.button("Edit Alt Text…").clicked() {
        app.dialog = Some(crate::dialogs::Dialog::new("altText"));
        ui.close();
    }
    if ui.button("Set as Default Shape").clicked() {
        let _ = app.run("shape.setDefault", json!({}));
        ui.close();
    }
    if ui.button("Size and Position…").clicked() {
        let _ = app.run("shape.sizePosition", json!({}));
        ui.close();
    }
    if ui.button("Format Shape…").clicked() {
        let _ = app.run("view.pane", json!({"pane": "format"}));
        ui.close();
    }
}

fn slide_items(app: &mut SlideApp, ui: &mut egui::Ui) {
    ui.menu_button("Layout", |ui| {
        let layouts: Vec<(String, u32)> = app
            .session
            .active()
            .and_then(|d| d.doc.masters.first().map(|m| m.layouts.iter().map(|l| (l.name.clone(), l.id.0)).collect()))
            .unwrap_or_default();
        for (n, id) in layouts {
            if ui.button(n).clicked() {
                let _ = app.run("slide.layout", json!({"layout": id}));
                ui.close();
            }
        }
    });
    ui.menu_button("Grid and Guides", |ui| {
        let mut on = app.ui.guides;
        if ui.checkbox(&mut on, "Guides").clicked() {
            let _ = app.run("view.guides", json!({"on": on}));
        }
        for (label, vertical) in [("Add Vertical Guide", true), ("Add Horizontal Guide", false)] {
            if ui.button(label).clicked() {
                let _ = app.run("guide.add", json!({"vertical": vertical}));
                let _ = app.run("view.guides", json!({"on": true}));
                ui.close();
            }
        }
    });
    if ui.button("Reset Slide").clicked() {
        let _ = app.run("slide.reset", json!({}));
        ui.close();
    }
    if ui.button("Format Background…").clicked() {
        let _ = app.run("view.pane", json!({"pane": "background"}));
        ui.close();
    }
    if ui.button("New Comment").clicked() {
        app.dialog = Some(crate::dialogs::Dialog::new("comment"));
        ui.close();
    }
}
