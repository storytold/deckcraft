//! Equation galleries. Every choice edits the native math tree through an engine command.
use egui::{Align2, Color32, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use equashion_egui::EquationEditor;
use equashion_egui::equashion_math::{GalleryTemplate, Math, galleries};

use crate::SlideApp;
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets::{RIBBON_H, group};

/// A rendered equation drawn into `r` (centred, shrunk to fit), black on its white tile.
fn paint_math(ed: &mut EquationEditor, ui: &Ui, r: Rect, key: &str, math: &Math, pt: f64) {
    if let Some((tex, size)) = ed.face(ui, key, math, pt) {
        let k = (r.width() / size.x.max(1.0)).min(r.height() / size.y.max(1.0)).min(1.0);
        let at = Rect::from_center_size(r.center(), size * k);
        ui.painter().image(tex, at, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::BLACK);
    }
}

/// A big ribbon button whose picture is an equation. Returns its response (the menu hangs on it).
fn math_button(ui: &mut Ui, ed: &mut EquationEditor, key: &str, picture: &Math, label: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let font = theme::font(11.5);
    let lines: Vec<&str> = label.split('\n').collect();
    let galley_w = lines.iter().map(|l| ui.fonts_mut(|f| f.layout_no_wrap(l.to_string(), font.clone(), t.text).size().x)).fold(0.0f32, f32::max);
    let w = (galley_w + 22.0).max(46.0);
    let (r, resp) = ui.allocate_exact_size(vec2(w, RIBBON_H - 2.0), Sense::click());
    let name = label.replace('\n', " ");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    if resp.hovered() {
        ui.painter().rect_filled(r, 4.0, t.hover);
    }
    let pic = Rect::from_center_size(pos2(r.center().x, r.min.y + 20.0), vec2(40.0, 34.0));
    ui.painter().rect_filled(pic, 3.0, Color32::WHITE);
    paint_math(ed, ui, pic, key, picture, 12.0);
    let mut y = r.min.y + 43.0;
    for (i, l) in lines.iter().enumerate() {
        // The UI font has no \u{25BE}: the dropdown chevron is painted after the last line.
        let last = i + 1 == lines.len();
        let w = ui.fonts_mut(|f| f.layout_no_wrap(l.to_string(), font.clone(), t.text).size().x);
        let x = r.center().x - (w + if last { 9.0 } else { 0.0 }) / 2.0;
        ui.painter().text(pos2(x, y), Align2::LEFT_TOP, *l, font.clone(), t.text);
        if last {
            crate::widgets::chevron(ui.painter(), pos2(x + w + 5.0, y + 7.0), 3.0, t.text);
        }
        y += 13.0;
    }
    resp.on_hover_text(name)
}

/// One gallery tile: the template rendered, its name as a tooltip.
fn tile(ui: &mut Ui, ed: &mut EquationEditor, tpl: &GalleryTemplate, size: egui::Vec2) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &tpl.label));
    ui.painter().rect_filled(r, 2.0, Color32::WHITE);
    if resp.hovered() {
        ui.painter().rect_stroke(r, 2.0, Stroke::new(1.5, t.accent), StrokeKind::Inside);
    }
    // The template with its empty places drawn as boxes.
    let face = equashion_egui::equashion_math::Editor::new(tpl.math.clone()).display_math();
    let pt = if size.x > 120.0 { 13.0 } else { 14.0 };
    paint_math(ed, ui, r.shrink(2.0), &format!("tab/{}", tpl.id), &face, pt);
    resp.on_hover_text(&tpl.label).clicked()
}

fn section_heading(ui: &mut Ui, name: &str) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().max(240.0), 20.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, t.hover);
    ui.painter().text(pos2(r.min.x + 6.0, r.center().y), Align2::LEFT_CENTER, name, theme::bold(11.5), t.text);
}

/// A structure gallery's menu.
fn structure_menu(ui: &mut Ui, ed: &mut EquationEditor, gallery_id: &str, actions: &mut Vec<serde_json::Value>) {
    let Some(g) = galleries().structure(gallery_id) else { return };
    egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
        ui.set_width(330.0);
        for s in &g.sections {
            section_heading(ui, &s.name);
            let wide = s.name.starts_with("Common") || s.templates.iter().all(|t| !t.linear.contains('\u{2B1A}')) && s.templates.len() <= 3;
            let size = if wide { vec2(158.0, 60.0) } else { vec2(60.0, 56.0) };
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for tpl in &s.templates {
                    if tile(ui, ed, tpl, size) {
                        actions.push(serde_json::json!({"template": tpl.id}));
                        ui.close();
                    }
                }
            });
            ui.add_space(4.0);
        }
    });
}

/// The ready-made equations (Equation › Equation).
fn builtin_menu(ui: &mut Ui, ed: &mut EquationEditor, actions: &mut Vec<serde_json::Value>) {
    egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
        ui.set_width(360.0);
        section_heading(ui, "Built-In");
        for b in &galleries().builtins {
            let t = Tokens::get(ui.ctx());
            ui.label(egui::RichText::new(&b.label).font(theme::bold(11.5)).color(t.text));
            let (r, resp) = ui.allocate_exact_size(vec2(350.0, 64.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &b.label));
            ui.painter().rect_filled(r, 2.0, Color32::WHITE);
            if resp.hovered() {
                ui.painter().rect_stroke(r, 2.0, Stroke::new(1.5, t.accent), StrokeKind::Inside);
            }
            paint_math(ed, ui, r.shrink(2.0), &format!("tab/builtin/{}", b.id), &b.math, 11.0);
            if resp.clicked() {
                actions.push(serde_json::json!({"builtin": b.id}));
                ui.close();
            }
            ui.add_space(4.0);
        }
    });
    ui.separator();
    if ui.button("Insert New Equation").clicked() {
        actions.push(serde_json::json!({"blank": true}));
        ui.close();
    }
}

/// Insert › Equation: a split button. The top inserts a new equation; the arrow lists the
/// ready-made ones (WordCraft's Insert › Equation), each opening the editor started from it.
pub fn insert_button(app: &mut SlideApp, ui: &mut Ui) {
    let (main, arrow) = crate::widgets::big_button_ex(ui, Icon::Equation, "Equation", true, true, false);
    if main.on_hover_text("Insert a new equation").clicked() {
        let _ = app.run("insert.equation", serde_json::json!({}));
    }
    let Some(a) = arrow else { return };
    let mut chosen: Option<String> = None;
    egui::Popup::menu(&a).show(|ui| {
        ui.set_min_width(260.0);
        let ed = app.equation.preview.get_or_insert_with(|| EquationEditor::new().id_salt("deckcraft-equation-preview"));
        egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
            ui.set_width(360.0);
            section_heading(ui, "Built-In");
            for b in &galleries().builtins {
                let t = Tokens::get(ui.ctx());
                ui.label(egui::RichText::new(&b.label).font(theme::bold(11.5)).color(t.text));
                let (r, resp) = ui.allocate_exact_size(vec2(350.0, 64.0), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &b.label));
                ui.painter().rect_filled(r, 2.0, Color32::WHITE);
                if resp.hovered() {
                    ui.painter().rect_stroke(r, 2.0, Stroke::new(1.5, t.accent), StrokeKind::Inside);
                }
                paint_math(ed, ui, r.shrink(2.0), &format!("insert/builtin/{}", b.id), &b.math, 11.0);
                if resp.clicked() {
                    chosen = Some(b.id.clone());
                    ui.close();
                }
                ui.add_space(4.0);
            }
        });
        ui.separator();
        if ui.button("Insert New Equation").clicked() {
            chosen = Some(String::new());
            ui.close();
        }
    });
    if let Some(id) = chosen {
        let p = if id.is_empty() { serde_json::json!({"blank": true}) } else { serde_json::json!({"builtin": id}) };
        let _ = app.run("equation.template", p);
    }
}

fn symbol_button(ui: &mut Ui, ed: &mut EquationEditor, c: char) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
    let s = c.to_string();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &s));
    ui.painter().rect_filled(r, 2.0, Color32::WHITE);
    if resp.hovered() {
        ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0, t.accent), StrokeKind::Inside);
    }
    let face = Math::new(vec![equashion_egui::equashion_math::Node::Text(s.clone())]);
    paint_math(ed, ui, r, &format!("tab/sym/{}", c as u32), &face, 11.0);
    let tip = galleries().symbol_name(c).map_or_else(|| format!("{s}  U+{:04X}", c as u32), |n| format!("{s}  \\{n}"));
    resp.on_hover_text(tip).clicked()
}

/// The Symbols group: a strip of the current set's symbols, and a menu with every set.
fn symbols(ui: &mut Ui, ed: &mut EquationEditor, actions: &mut Vec<serde_json::Value>) {
    let sets = &galleries().symbols;
    let id = egui::Id::new("deck_eq_symbol_set");
    let set = ui.data(|d| d.get_temp::<usize>(id)).unwrap_or(0).min(sets.len().saturating_sub(1));
    let Some(current) = sets.get(set) else { return };
    let start_id = egui::Id::new("deck_eq_symbol_start");
    let per_row = 10usize;
    let n_rows = 3usize;
    let all: Vec<char> = current.chars.chars().collect();
    let start = ui.data(|d| d.get_temp::<usize>(start_id)).unwrap_or(0).min(all.len().saturating_sub(1));
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
        for row in 0..n_rows {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
                for col in 0..per_row {
                    let Some(c) = all.get(start + row * per_row + col) else {
                        ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                        continue;
                    };
                    if symbol_button(ui, ed, *c) {
                        actions.push(serde_json::json!({"symbol": c.to_string()}));
                    }
                }
            });
        }
    });
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 1.0);
        let page = per_row * n_rows;
        if ui.add_enabled(start > 0, egui::Button::new("\u{25B2}").min_size(vec2(16.0, 22.0))).clicked() {
            ui.data_mut(|d| d.insert_temp(start_id, start.saturating_sub(per_row)));
        }
        if ui.add_enabled(start + page < all.len(), egui::Button::new("\u{25BC}").min_size(vec2(16.0, 22.0))).clicked() {
            ui.data_mut(|d| d.insert_temp(start_id, start + per_row));
        }
        let resp = ui.add(egui::Button::new("\u{2026}").min_size(vec2(16.0, 22.0))).on_hover_text("All symbol sets");
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_width(420.0);
            ui.horizontal_wrapped(|ui| {
                for (i, s) in sets.iter().enumerate() {
                    if ui.selectable_label(i == set, &s.name).clicked() {
                        ui.data_mut(|d| {
                            d.insert_temp(id, i);
                            d.insert_temp(start_id, 0usize);
                        });
                    }
                }
            });
            ui.separator();
            let set = ui.data(|d| d.get_temp::<usize>(id)).unwrap_or(0);
            if let Some(s) = sets.get(set) {
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
                        for c in s.chars.chars() {
                            if symbol_button(ui, ed, c) {
                                actions.push(serde_json::json!({"symbol": c.to_string()}));
                            }
                        }
                    });
                });
            }
        });
    });
}

/// The ribbon's small button, with its label for assistive technology.
fn small_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, selected: bool) -> egui::Response {
    let r = crate::widgets::small_button(ui, icon, label, enabled, selected);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    r
}

pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let mut actions = Vec::new();
    let mut close = false;
    let ed = app.equation.preview.get_or_insert_with(|| EquationEditor::new().id_salt("deckcraft-equation-preview"));
    group(ui, |ui| {
        let g = galleries();
        let r = math_button(ui, ed, "tab/equation", &g.equation_picture, "Equation");
        egui::Popup::menu(&r).show(|ui| builtin_menu(ui, ed, &mut actions));
    });
    group(ui, |ui| symbols(ui, ed, &mut actions));
    group(ui, |ui| {
        for g in &galleries().structures {
            let r = math_button(ui, ed, &format!("tab/pic/{}", g.id), &g.picture, &g.label);
            egui::Popup::menu(&r).show(|ui| structure_menu(ui, ed, &g.id, &mut actions));
        }
    });
    group(ui, |ui| {
        close = small_button(ui, Icon::Close, "Close Equation", true, false).clicked();
    });
    for p in actions {
        let _ = app.run("equation.template", p);
    }
    if close {
        let _ = app.run("equation.leave", serde_json::json!({}));
    }
}
