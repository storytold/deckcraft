use super::files::{place_row, square_button};
use super::info::SAVE_TYPES;
use super::{
    Align2, Backstage, Color32, CornerRadius, ExportTask, Icon, PrintScope, Rect, RichText, Sense, SlideApp, Stroke, StrokeKind, Tokens, Ui, Value,
    bullets, dim, heading, icon_button, json, pos2, theme, vec2,
};

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
pub(super) fn print(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
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
            let combo = craft_ui::ComboBox::from_id_salt("backstage_print_scope").width(240.0).selected_text(scope).show_ui(ui, |ui| {
                for (s, label) in
                    [(PrintScope::All, "Print All Slides"), (PrintScope::Current, "Print Current Slide"), (PrintScope::Custom, "Custom Range")]
                {
                    if ui.selectable_label(bs.print_scope == s, label).clicked() {
                        bs.print_scope = s;
                        bs.preview_page = 0;
                    }
                }
            });
            combo.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Print range"));
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
            let combo = craft_ui::ComboBox::from_id_salt("backstage_print_layout").width(240.0).selected_text(layout).show_ui(ui, |ui| {
                for (id, label) in LAYOUTS {
                    if ui.selectable_label(bs.print_layout == id, label).clicked() {
                        bs.print_layout = id;
                        bs.preview_page = 0;
                    }
                }
            });
            combo.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Print layout"));
            ui.add_space(4.0);
            // PowerPoint prints full-page slides on slide-shaped pages; paper and orientation are
            // for notes pages and handouts.
            ui.add_enabled_ui(bs.print_layout != "slides", |ui| {
                let paper = deckcraft_engine::cmd::file::PAPERS.iter().find(|p| p.0 == bs.print_paper).map_or("Letter", |p| p.1);
                craft_ui::ComboBox::from_id_salt("backstage_print_paper")
                    .width(240.0)
                    .selected_text(paper)
                    .show_ui(ui, |ui| {
                        for (id, label, _) in deckcraft_engine::cmd::file::PAPERS {
                            if ui.selectable_label(bs.print_paper == id, label).clicked() {
                                bs.print_paper = id;
                            }
                        }
                    })
                    .response
                    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Paper size"));
                ui.add_space(4.0);
                let orient = if bs.print_landscape { "Landscape Orientation" } else { "Portrait Orientation" };
                craft_ui::ComboBox::from_id_salt("backstage_print_orientation")
                    .width(240.0)
                    .selected_text(orient)
                    .show_ui(ui, |ui| {
                        for (land, label) in [(false, "Portrait Orientation"), (true, "Landscape Orientation")] {
                            if ui.selectable_label(bs.print_landscape == land, label).clicked() {
                                bs.print_landscape = land;
                            }
                        }
                    })
                    .response
                    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Orientation"));
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

/// Export: tasks on the left (Create PDF Document, Create Handouts, Change File Type), the chosen
/// one's description and button on the right. PowerPoint's video, animated GIF and CD package
/// exports aren't offered.
pub(super) fn export(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
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
                    let combo = craft_ui::ComboBox::from_id_salt("backstage_handouts")
                        .width(240.0)
                        .selected_text(format!("{} slides per page", bs.handouts_per_page))
                        .show_ui(ui, |ui| {
                            for n in [1u8, 2, 3, 4, 6, 9] {
                                if ui.selectable_label(bs.handouts_per_page == n, format!("{n} slides per page")).clicked() {
                                    bs.handouts_per_page = n;
                                }
                            }
                        });
                    combo.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Slides per page"));
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
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, *label));
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
