//! The canvas shell (`view.shell {shell: canvas}`): a canvas-first layout in place of the ribbon.
//! On the left, the presentation's name, the slide list and the current slide's layers; on the
//! right, an inspector with Design and Animate tabs; under the slide, a notes card and a
//! floating toolbar. It reuses the classic shell's pieces and, like them, acts only through
//! commands, so both layouts edit, save and open exactly the same presentations.

use deckcraft_engine::{Mods, Target, ToolKind};
use egui::{Align2, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};
use crate::widgets::icon_toggle;
use crate::{SlideApp, ViewMode};

const SIDEBAR_W: f32 = 260.0;
const INSPECTOR_W: f32 = 280.0;
const TOOLBAR_H: f32 = 64.0;
/// Zoom menu, in the canvas layout's pixel percentages (200% is the canvas's 4× limit).
const ZOOMS: [u32; 7] = [10, 25, 50, 75, 100, 150, 200];

pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    if app.session.active().is_none() {
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |_| {});
        return;
    }
    sidebar(app, ui);
    inspector(app, ui);
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| match app.ui.view {
        ViewMode::Sorter => crate::sorter::show(app, ui),
        ViewMode::NotesPage => crate::sorter::notes_page(app, ui),
        ViewMode::Normal | ViewMode::Outline | ViewMode::Reading => {
            toolbar(app, ui);
            if app.ui.notes {
                notes_card(app, ui);
            }
            crate::canvas::show(app, ui);
        }
    });
}

/// Single-key tools, as in canvas editors: V select, T text box, R rectangle, O oval, L line, and
/// Esc back to Select from a drawing tool. Not while text is being edited (the letters are
/// typed) or a freeform is being drawn. Returns whether the key was used.
pub fn tool_key(app: &mut SlideApp, key: egui::Key, mods: Mods) -> bool {
    if mods.cmd || mods.alt || mods.shift || app.session.tool.drawing_freeform() {
        return false;
    }
    if app.session.active().is_none_or(|d| d.selection.text.is_some()) {
        return false;
    }
    let shape = |p: &str| ToolKind::Shape { preset: p.to_string() };
    let kind = match key {
        egui::Key::V => ToolKind::Select,
        egui::Key::T => ToolKind::TextBox,
        egui::Key::R => shape("rect"),
        egui::Key::O => shape("ellipse"),
        egui::Key::L => shape("line"),
        egui::Key::Escape if app.session.tool.kind != ToolKind::Select => ToolKind::Select,
        _ => return false,
    };
    app.session.set_tool(kind);
    true
}

/// A panel edge: the sidebars are cards with a hairline against the canvas.
fn edge(ui: &Ui, x: f32, t: &Tokens) {
    let r = ui.max_rect();
    ui.painter().line_segment([pos2(x, r.min.y), pos2(x, r.max.y)], Stroke::new(1.0, t.separator));
}

fn section_title(ui: &mut Ui, title: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(title).font(theme::bold(12.5)).color(t.text));
}

// ----- Left: name, slides, layers ---------------------------------------------------------

fn sidebar(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let in_master = app.session.active().is_some_and(|d| d.selection.target != Target::Slides);
    egui::Panel::left("canvas_sidebar")
        .resizable(true)
        .default_size(app.ui.thumbs_width.max(SIDEBAR_W))
        .size_range(200.0..=420.0)
        .frame(egui::Frame::NONE.fill(t.card))
        .show(ui, |ui| {
            app.ui.thumbs_width = ui.available_width();
            edge(ui, ui.max_rect().max.x, &t);
            header(app, ui);
            ui.add_space(6.0);
            new_slide_row(app, ui);
            ui.add_space(6.0);
            ui.painter().hline(ui.max_rect().x_range(), ui.cursor().min.y, Stroke::new(1.0, t.separator));
            if !in_master {
                layers(app, ui);
            }
            egui::ScrollArea::vertical().id_salt("canvas_slides").auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(8.0);
                crate::thumbs::list(app, ui);
                ui.add_space(12.0);
            });
        });
}

/// Name and folder of the presentation, and the slide / grid view switch.
fn header(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let title = st.title();
    let place = st
        .path
        .as_deref()
        .and_then(|p| std::path::Path::new(p).parent())
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Not saved".into());
    let dirty = st.is_dirty();
    egui::Frame::NONE.inner_margin(egui::Margin { left: 14, right: 10, top: 12, bottom: 0 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let sorter = app.ui.view == ViewMode::Sorter;
                if icon_toggle(ui, Icon::Grid, "Grid view", true, sorter).clicked() {
                    let _ = app.run("view.sorter", json!({}));
                }
                if icon_toggle(ui, Icon::ViewNormalSmall, "Slide view", true, !sorter).clicked() {
                    let _ = app.run("view.normal", json!({}));
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let name = format!("{title}{}", if dirty { " •" } else { "" });
                    let r = ui.add(egui::Button::new(egui::RichText::new(name).font(theme::bold(15.0))).frame(false).truncate());
                    egui::Popup::menu(&r).show(|ui| {
                        for (label, page) in [("Info…", "info"), ("Save As…", "saveAs"), ("Export…", "export"), ("Print…", "print")] {
                            if ui.button(label).clicked() {
                                let _ = app.run("app.backstage", json!({"page": page}));
                            }
                        }
                        ui.separator();
                        if ui.button("Close").clicked() {
                            let _ = app.run("app.closePresentation", json!({}));
                        }
                    });
                });
            });
        });
        ui.label(egui::RichText::new(place).font(theme::font(12.0)).color(t.text_dim));
    });
}

/// "New slide ▾" (pick a layout) and "+" (the next slide's usual layout).
fn new_slide_row(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let en = crate::ribbon::enabled(app, "slide.new");
    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(10, 0)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let w = ui.available_width() - 34.0;
            let (r, resp) = ui.allocate_exact_size(vec2(w, 28.0), if en { Sense::click() } else { Sense::hover() });
            ui.painter().rect(
                r,
                CornerRadius::same(6),
                if resp.hovered() { t.hover } else { t.card },
                Stroke::new(1.0, t.border),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                r.center() - vec2(6.0, 0.0),
                Align2::CENTER_CENTER,
                "New slide",
                theme::font(13.0),
                if en { t.text } else { t.text_faint },
            );
            crate::widgets::chevron(ui.painter(), pos2(r.center().x + 34.0, r.center().y), 3.0, t.text_dim);
            egui::Popup::menu(&resp).width(420.0).show(|ui| {
                crate::ribbon::layout_menu(app, ui, "slide.new");
                ui.separator();
                if ui.button("Duplicate Selected Slides").clicked() {
                    let _ = app.run("slide.duplicate", json!({}));
                }
            });
            let (pr, presp) = ui.allocate_exact_size(vec2(28.0, 28.0), if en { Sense::click() } else { Sense::hover() });
            ui.painter().rect(
                pr,
                CornerRadius::same(6),
                if presp.hovered() { t.hover } else { t.card },
                Stroke::new(1.0, t.border),
                egui::StrokeKind::Inside,
            );
            icons::paint(ui.painter(), pr.shrink(7.0), Icon::Plus, t.text, !en);
            if presp.on_hover_text("New Slide (⇧⌘N)").clicked() {
                let _ = app.run("slide.new", json!({}));
            }
        });
    });
}

/// The current slide's objects (the Selection pane's list) in a resizable strip at the bottom.
fn layers(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::bottom("canvas_layers")
        .resizable(true)
        .default_size(220.0)
        .size_range(80.0..=600.0)
        .frame(egui::Frame::NONE.fill(t.card).inner_margin(egui::Margin { left: 10, right: 8, top: 10, bottom: 8 }))
        .show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().hline(r.x_range().expand(10.0), r.min.y - 10.0, Stroke::new(1.0, t.separator));
            section_title(ui, "Layers");
            ui.add_space(4.0);
            egui::ScrollArea::vertical().id_salt("canvas_layers_list").auto_shrink([false, false]).show(ui, |ui| {
                if app.session.active().is_some_and(|d| d.shapes().is_empty()) {
                    ui.label(egui::RichText::new("Nothing on this slide yet").font(theme::font(12.0)).color(t.text_faint));
                } else {
                    let marked = crate::states::changed_layers(app);
                    crate::selpane::list_marked(app, ui, &marked);
                }
            });
        });
}

// ----- Right: inspector -------------------------------------------------------------------

fn inspector(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::right("canvas_inspector")
        .resizable(true)
        .default_size(INSPECTOR_W)
        .size_range(240.0..=400.0)
        .frame(egui::Frame::NONE.fill(t.card).inner_margin(egui::Margin { left: 12, right: 12, top: 10, bottom: 0 }))
        .show(ui, |ui| {
            edge(ui, ui.max_rect().min.x - 12.0, &t);
            inspector_header(app, ui);
            ui.add_space(8.0);
            inspector_tabs(app, ui);
            ui.add_space(4.0);
            let r = ui.max_rect();
            ui.painter().hline(r.x_range().expand(12.0), ui.cursor().min.y, Stroke::new(1.0, t.separator));
            ui.add_space(8.0);
            let tab = app.ui.inspector_tab.clone();
            // The panel keeps the width it was given: wide content (the gradient editor) is
            // clipped rather than widening the panel frame after frame.
            let w = ui.available_width();
            egui::ScrollArea::both().id_salt("canvas_inspector_body").auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(w);
                // Slider tracks and fields stand out from the white card.
                ui.visuals_mut().widgets.inactive.bg_fill = t.pressed;
                match tab.as_str() {
                    "animate" => crate::states::animate(app, ui),
                    _ => crate::inspector::design(app, ui),
                }
            });
        });
}

/// Play from the current slide, and Export.
fn inspector_header(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let export = ui.add(
                egui::Button::new(egui::RichText::new("Export").color(t.accent_text).font(theme::bold(13.0)))
                    .fill(t.accent)
                    .corner_radius(CornerRadius::same(6))
                    .min_size(vec2(72.0, 28.0)),
            );
            if export.clicked() {
                let _ = app.run("app.backstage", json!({"page": "export"}));
            }
            let (r, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(6), t.hover);
            }
            icons::paint(ui.painter(), r.shrink(6.0), Icon::Play, t.text, false);
            if resp.on_hover_text("Present from this slide").clicked() {
                let from = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
                app.start_show(from, false);
            }
        });
    });
}

/// Design / Animate, and the zoom menu.
fn inspector_tabs(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        for (id, label) in [("design", "Design"), ("animate", "Animate")] {
            let on = app.ui.inspector_tab == id;
            let r = ui.add(
                egui::Button::new(egui::RichText::new(label).font(if on { theme::bold(13.0) } else { theme::font(13.0) }).color(if on {
                    t.text
                } else {
                    t.text_dim
                }))
                .fill(if on { t.selected } else { egui::Color32::TRANSPARENT })
                .corner_radius(CornerRadius::same(5)),
            );
            if r.clicked() {
                app.ui.inspector_tab = id.to_string();
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let pct = (crate::units::zoom_shown(app, app.ui.zoom.unwrap_or(app.canvas_scale)) * 100.0).round();
            let label = format!("{pct}%");
            let r = ui.add(egui::Button::new(egui::RichText::new(label).font(theme::font(13.0))).frame(false));
            egui::Popup::menu(&r).show(|ui| {
                if ui.button("Zoom In").clicked() {
                    let _ = app.run("view.zoomIn", json!({}));
                }
                if ui.button("Zoom Out").clicked() {
                    let _ = app.run("view.zoomOut", json!({}));
                }
                if ui.button("Fit").clicked() {
                    let _ = app.run("view.fit", json!({}));
                }
                ui.separator();
                for z in ZOOMS {
                    if ui.button(format!("{z}%")).clicked() {
                        let p = crate::units::zoom_percent(app, z as f64);
                        let _ = app.run("view.zoom", json!({"percent": p}));
                    }
                }
            });
        });
    });
}

// ----- Under the slide: notes and toolbar --------------------------------------------------

/// Presenter notes in a rounded card with a drag handle above it.
fn notes_card(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    if app.session.active().is_none_or(|d| d.selection.target != Target::Slides) {
        return;
    }
    egui::Panel::bottom("canvas_notes")
        .resizable(true)
        .show_separator_line(false)
        .default_size(app.ui.notes_height.max(120.0))
        .size_range(60.0..=420.0)
        .frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 20, right: 20, top: 14, bottom: 4 }))
        .show(ui, |ui| {
            app.ui.notes_height = ui.available_height() + 18.0;
            let r = ui.max_rect();
            // The grip says the card can be pulled up (the panel edge does the resizing).
            ui.painter()
                .line_segment([pos2(r.center().x - 22.0, r.min.y - 8.0), pos2(r.center().x + 22.0, r.min.y - 8.0)], Stroke::new(3.0, t.text_faint));
            egui::Frame::NONE.fill(t.hover).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                egui::ScrollArea::vertical().id_salt("canvas_notes_scroll").show(ui, |ui| {
                    crate::status::notes_editor(app, ui, "Add presenter notes…");
                });
            });
        });
}

/// One tool button in the floating toolbar: the current tool is filled with the accent.
fn tool(ui: &mut Ui, icon: Icon, tip: &str, on: bool, enabled: bool) -> egui::Response {
    tool_button(ui, icon, tip, on, enabled, true)
}

/// A view toggle in the toolbar: shown on in grey, so it isn't mistaken for the current tool.
fn toggle(ui: &mut Ui, icon: Icon, tip: &str, on: bool) -> egui::Response {
    tool_button(ui, icon, tip, on, true, false)
}

fn tool_button(ui: &mut Ui, icon: Icon, tip: &str, on: bool, enabled: bool, accent: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter_at(r);
    let filled = on && accent;
    if filled {
        p.rect_filled(r, CornerRadius::same(8), t.accent);
    } else if on {
        p.rect_filled(r, CornerRadius::same(8), t.selected);
    } else if resp.hovered() {
        p.rect_filled(r, CornerRadius::same(8), t.hover);
    }
    icons::paint(&p, r.shrink(9.0), icon, if filled { t.accent_text } else { t.text }, !enabled);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, tip));
    resp.on_hover_text(tip)
}

/// The floating toolbar: drawing tools (select, text, shape) and inserts.
fn toolbar(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::bottom("canvas_toolbar").exact_size(TOOLBAR_H).show_separator_line(false).frame(egui::Frame::NONE).show(ui, |ui| {
        let area = ui.max_rect();
        let w = 7.0 * 38.0 + 1.0 + 2.0 * 38.0 + 16.0;
        let bar = Rect::from_center_size(pos2(area.center().x, area.center().y - 4.0), vec2(w, 48.0));
        let shadow = egui::epaint::Shadow { offset: [0, 2], blur: 10, spread: 0, color: t.shadow };
        ui.painter().add(shadow.as_shape(bar, CornerRadius::same(12)));
        ui.painter().rect(bar, CornerRadius::same(12), t.card, Stroke::new(1.0, t.card_border), egui::StrokeKind::Outside);
        let mut row =
            ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(8.0, 6.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        row.spacing_mut().item_spacing.x = 2.0;
        let ui = &mut row;
        let kind = app.session.tool.kind.clone();
        let has_slide = app.session.active().is_some_and(|d| !d.doc.slides.is_empty() || d.selection.target != Target::Slides);
        if tool(ui, Icon::Pointer, "Select (V)", kind == ToolKind::Select, true).clicked() {
            app.session.set_tool(ToolKind::Select);
        }
        if tool(ui, Icon::TextBox, "Text box (T)", kind == ToolKind::TextBox, has_slide).clicked() {
            app.session.set_tool(ToolKind::TextBox);
        }
        if tool(ui, Icon::Picture, "Picture…", false, has_slide).clicked() {
            let _ = app.run("app.insertPictureDialog", json!({}));
        }
        let r = tool(ui, Icon::Shapes, "Shape (R rectangle, O oval, L line)", matches!(kind, ToolKind::Shape { .. }), has_slide);
        egui::Popup::menu(&r).show(|ui| {
            if let Some(p) = crate::ribbon::shapes_gallery(ui) {
                app.session.set_tool(ToolKind::Shape { preset: p.to_string() });
            }
        });
        let r = tool(ui, Icon::Table, "Table", false, crate::ribbon::enabled(app, "insert.table"));
        egui::Popup::menu(&r).show(|ui| {
            if let Some((rows, cols)) = crate::ribbon::table_grid_picker(ui) {
                let _ = app.run("insert.table", json!({"rows": rows, "cols": cols}));
                ui.close();
            }
        });
        if tool(ui, Icon::Chart, "Chart", false, crate::ribbon::enabled(app, "insert.chart")).clicked() {
            let _ = app.run("insert.chart", json!({}));
        }
        if tool(ui, Icon::Comment, "Comment", false, has_slide).clicked() {
            app.dialog = Some(crate::dialogs::Dialog::new("comment"));
        }
        let sep = ui.allocate_exact_size(vec2(9.0, 28.0), Sense::hover()).0;
        ui.painter().vline(sep.center().x, sep.y_range(), Stroke::new(1.0, t.separator));
        if toggle(ui, Icon::NotesSmall, "Presenter notes", app.ui.notes).clicked() {
            let _ = app.run("view.notes", json!({}));
        }
        if toggle(ui, Icon::Search, "Search commands (⇧⌘P)", false).clicked() {
            app.palette = Some((String::new(), 0));
        }
    });
}

#[cfg(test)]
#[path = "canvas_shell_tests.rs"]
mod tests;
