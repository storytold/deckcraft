use super::galleries::{Fl, kurbo_flatten, shapes_gallery};
use super::home::layout_menu;
use super::view::zoom_menu;
use super::{
    Color32, CornerRadius, Icon, Rect, Sense, SlideApp, Stroke, Tokens, Ui, big, big_button, big_button_ex, enabled, group, json, menu_item, pos2,
    run, tip, vec2,
};

/// Photo Album is a split button: the main click starts a new album, the arrow edits one.
fn photo_album_button(app: &mut SlideApp, ui: &mut Ui) {
    let en = enabled(app, "insert.photoAlbum");
    let (main, arrow) = big_button_ex(ui, Icon::Picture, "Photo\nAlbum", en, true, false);
    let _ = main.clone().on_hover_text(tip("insert.photoAlbum", "Photo Album…"));
    main.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, en, "Photo Album…"));
    if let Some(a) = &arrow {
        let _ = a.clone().on_hover_text("Photo Album options");
        a.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, en, "Photo Album options"));
    }
    if main.clicked() {
        run(app, "insert.photoAlbum", json!({}));
    }
    if let Some(a) = arrow {
        egui::Popup::menu(&a).show(|ui| {
            menu_item(app, ui, Icon::Picture, "New Photo Album…", "insert.photoAlbum", json!({}));
            menu_item(app, ui, Icon::Picture, "Edit Photo Album…", "insert.photoAlbumEdit", json!({}));
        });
    }
}

// ---------- Insert ----------

pub(super) fn insert(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let (main, arrow) = big_button_ex(ui, Icon::NewSlide, "New\nSlide", enabled(app, "slide.new"), true, false);
        if main.clicked() {
            run(app, "slide.new", json!({}));
        }
        if let Some(a) = arrow {
            egui::Popup::menu(&a).width(420.0).show(|ui| layout_menu(app, ui, "slide.new"));
        }
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Table, "Table", enabled(app, "insert.table"));
        egui::Popup::menu(&r).show(|ui| {
            if let Some((rr, cc)) = table_grid_picker(ui) {
                run(app, "insert.table", json!({"rows": rr, "cols": cc}));
                ui.close();
            }
            ui.separator();
            if ui.button(crate::tl!("Insert Table…")).clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("table"));
                ui.close();
            }
            if ui.button(crate::tl!("Draw Table")).clicked() {
                app.session.set_tool(deckcraft_engine::ToolKind::Shape { preset: deckcraft_engine::tools::DRAW_TABLE.into() });
                ui.close();
            }
        });
    });
    group(ui, |ui| {
        big(app, ui, Icon::Picture, "Pictures", "app.insertPictureDialog", json!({}));
        photo_album_button(app, ui);
        let r = big_button(ui, Icon::Screenshot, "Screenshot", false);
        let _ = r.on_hover_text("Screenshot (not available yet)");
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Shapes, "Shapes", enabled(app, "shape.insert"));
        egui::Popup::menu(&r).show(|ui| {
            if let Some(p) = shapes_gallery(ui) {
                app.session.set_tool(deckcraft_engine::ToolKind::Shape { preset: p.to_string() });
            }
        });
        let r = big_button(ui, Icon::Icons, "Icons", enabled(app, "insert.icon"));
        egui::Popup::menu(&r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).width(340.0).show(|ui| icon_library(app, ui));
        big(app, ui, Icon::SmartArt, "SmartArt", "app.dialog", json!({"id": "smartart"}));
        let r = big_button(ui, Icon::Chart, "Chart", enabled(app, "insert.chart"));
        egui::Popup::menu(&r).show(|ui| {
            for k in chart_menu_types() {
                if k.is_none() {
                    ui.separator();
                }
                if let Some(k) = k
                    && ui.button(k.label()).clicked()
                {
                    run(app, "insert.chart", json!({"type": format!("{k:?}")}));
                }
            }
        });
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Zoom, "Zoom", enabled(app, "insert.zoom"));
        egui::Popup::menu(&r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).width(300.0).show(|ui| zoom_menu(app, ui));
        big(app, ui, Icon::Link, "Link", "app.dialog", json!({"id": "hyperlink"}));
        let r = big_button(ui, Icon::Action, "Action", enabled(app, "insert.actionButton"));
        egui::Popup::menu(&r).show(|ui| {
            for k in ["back", "forward", "beginning", "end", "home", "information", "return", "movie", "document", "sound", "help", "custom"] {
                if ui.button(format!("Action Button: {k}")).clicked() {
                    run(app, "insert.actionButton", json!({"kind": k}));
                }
            }
            ui.separator();
            let has_sel = app.session.active().is_some_and(|s| !s.selection.shapes.is_empty());
            if ui.add_enabled(has_sel, egui::Button::new(crate::tl!("Action Settings…"))).clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("action"));
            }
        });
    });
    group(ui, |ui| {
        big(app, ui, Icon::Comment, "Comment", "app.dialog", json!({"id": "comment"}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::TextBox, "Text\nBox", "insert.textBox", json!({}));
        big(app, ui, Icon::HeaderFooter, "Header &\nFooter", "app.dialog", json!({"id": "headerFooter"}));
        let r = big_button(ui, Icon::WordArt, "WordArt", enabled(app, "insert.wordArt"));
        egui::Popup::menu(&r).show(|ui| {
            for i in 0..6 {
                if ui.button(format!("WordArt style {}", i + 1)).clicked() {
                    run(app, "insert.wordArt", json!({"style": i}));
                }
            }
        });
        big(app, ui, Icon::DateTime, "Date &\nTime", "insert.dateTime", json!({}));
        big(app, ui, Icon::SlideNumber, "Slide\nNumber", "insert.slideNumber", json!({}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::Equation, "Equation", "app.dialog", json!({"id": "equation"}));
        big(app, ui, Icon::Symbol, "Symbol", "app.dialog", json!({"id": "symbol"}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::Video, "Video", "app.insertVideoDialog", json!({}));
        big(app, ui, Icon::Audio, "Audio", "app.insertAudioDialog", json!({}));
    });
}

/// Hover-to-size table grid. Returns (rows, cols) when clicked.
pub fn table_grid_picker(ui: &mut Ui) -> Option<(usize, usize)> {
    let t = Tokens::get(ui.ctx());
    let id = ui.scope_id().with("tgrid");
    let mut hover: (usize, usize) = ui.data_mut(|d| d.get_temp(id).unwrap_or((0, 0)));
    let cell = 16.0;
    let label = if hover.0 > 0 { format!("{}x{} {}", hover.1, hover.0, crate::tl!("Table")) } else { crate::tl!("Insert Table").to_string() };
    ui.label(label);
    let (rect, resp) = ui.allocate_exact_size(vec2(10.0 * (cell + 2.0), 8.0 * (cell + 2.0)), Sense::click());
    if let Some(p) = resp.hover_pos() {
        let c = ((p.x - rect.min.x) / (cell + 2.0)).floor() as usize + 1;
        let r = ((p.y - rect.min.y) / (cell + 2.0)).floor() as usize + 1;
        hover = (r.min(8), c.min(10));
    }
    for r in 0..8 {
        for c in 0..10 {
            let cr = Rect::from_min_size(rect.min + vec2(c as f32 * (cell + 2.0), r as f32 * (cell + 2.0)), vec2(cell, cell));
            let on = r < hover.0 && c < hover.1;
            ui.painter().rect(
                cr,
                CornerRadius::ZERO,
                if on { t.accent.gamma_multiply(0.3) } else { t.card },
                Stroke::new(1.0, if on { t.accent } else { t.border }),
                egui::StrokeKind::Inside,
            );
        }
    }
    ui.data_mut(|d| d.insert_temp(id, hover));
    if resp.clicked() && hover.0 > 0 { Some(hover) } else { None }
}

fn render_icon_categories(app: &mut SlideApp, ui: &mut Ui, icons: &[deckcraft_engine::icons::Icon], color: Color32, hover: Color32) {
    let mut cats: Vec<&str> = Vec::new();
    for icon in icons {
        if !cats.contains(&icon.category) {
            cats.push(icon.category);
        }
    }
    for cat in cats {
        ui.label(egui::RichText::new(cat).small().strong());
        ui.horizontal_wrapped(|ui| {
            for icon in icons.iter().filter(|i| i.category == cat) {
                icon_cell(app, ui, icon, color, hover);
            }
        });
    }
}

fn render_filtered_icons(app: &mut SlideApp, ui: &mut Ui, icons: &[deckcraft_engine::icons::Icon], q: &str, color: Color32, hover: Color32) {
    let mut n = 0;
    ui.horizontal_wrapped(|ui| {
        for icon in icons {
            if icon.name.contains(q) || icon.label.to_lowercase().contains(q) {
                icon_cell(app, ui, icon, color, hover);
                n += 1;
            }
        }
    });
    if n == 0 {
        ui.label("No icons match.");
    }
}

/// Lucide stroke icons (Insert › Icons). Search filters the gallery; a click inserts the outline.
fn icon_library(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (color, hover) = (t.text, t.hover);
    let id = ui.scope_id().with("icon-query");
    let mut query = ui.data_mut(|d| d.get_temp::<String>(id).unwrap_or_default());
    ui.add(egui::TextEdit::singleline(&mut query).hint_text("Search icons").desired_width(ui.available_width()));
    let q = query.trim().to_lowercase();
    ui.data_mut(|d| d.insert_temp(id, query));
    let icons = deckcraft_engine::icons::icons();
    egui::ScrollArea::vertical().id_salt("icon-scroll").max_height(420.0).show(ui, |ui| {
        if q.is_empty() {
            render_icon_categories(app, ui, icons, color, hover);
        } else {
            render_filtered_icons(app, ui, icons, &q, color, hover);
        }
    });
}

fn icon_cell(app: &mut SlideApp, ui: &mut Ui, icon: &deckcraft_engine::icons::Icon, color: Color32, hover: Color32) {
    let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(4), hover);
    }
    paint_icon(ui.painter(), r.shrink(6.0), icon, color);
    if resp.on_hover_text(icon.label).clicked() {
        let _ = app.run("insert.icon", json!({"name": icon.name}));
        ui.close();
    }
}

/// Stroke a Lucide outline into a gallery cell. The line is the design stroke scaled to the cell.
fn paint_icon(p: &egui::Painter, r: Rect, icon: &deckcraft_engine::icons::Icon, color: Color32) {
    let Some(src) = deckcraft_engine::icons::outline(icon.name) else { return };
    let mut bp = src.clone();
    let (w, h) = (r.width().max(1.0) as f64, r.height().max(1.0) as f64);
    bp.apply_affine(deckcraft_geom::Affine::scale_non_uniform(w / deckcraft_engine::icons::VIEW, h / deckcraft_engine::icons::VIEW));
    let sw = ((r.width() * icon.stroke as f32) / deckcraft_engine::icons::VIEW as f32).clamp(1.0, 3.5);
    let mut pts: Vec<egui::Pos2> = vec![];
    let mut polys: Vec<(Vec<egui::Pos2>, bool)> = vec![];
    kurbo_flatten(&bp, |el| match el {
        Fl::Move(x, y) => {
            if pts.len() > 1 {
                polys.push((std::mem::take(&mut pts), false));
            }
            pts.clear();
            pts.push(pos2(r.min.x + x as f32, r.min.y + y as f32));
        }
        Fl::Line(x, y) => pts.push(pos2(r.min.x + x as f32, r.min.y + y as f32)),
        Fl::Close => {
            if pts.len() > 1 {
                polys.push((std::mem::take(&mut pts), true));
            }
        }
    });
    if pts.len() > 1 {
        polys.push((pts, false));
    }
    for (poly, closed) in polys {
        p.add(egui::Shape::Path(egui::epaint::PathShape {
            points: poly,
            closed,
            fill: Color32::TRANSPARENT,
            stroke: egui::epaint::PathStroke::new(sw, color),
        }));
    }
}

/// The chart types in the Insert Chart and Change Chart Type menus: the main ones, a separator
/// (`None`), then the Office 2016 types not already listed.
pub(super) fn chart_menu_types() -> Vec<Option<deckcraft_model::chart::ChartType>> {
    use deckcraft_model::chart::ChartType;
    let mut v: Vec<Option<ChartType>> = ChartType::MAIN.iter().copied().map(Some).collect();
    v.push(None);
    v.extend(ChartType::OFFICE_2016.iter().copied().filter(|k| !ChartType::MAIN.contains(k)).map(Some));
    v
}
