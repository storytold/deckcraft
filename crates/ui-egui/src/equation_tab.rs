//! The Equation tab, laid out as WordCraft's is: Tools, Symbols, Structures, Equation Options.
//! Shown while the Equation dialog is open. The galleries, symbol sets, ready-made equations and
//! the pictures on the buttons are equaSHion's (WordCraft's, exported); everything inserts into
//! the dialog's editor at the caret, so the dialog stays where it is and the ribbon does the
//! choosing.
//!
//! Not in equaSHion's editor yet, so shown but disabled: Unicode / LaTeX input format, Convert,
//! Normal Text and equation numbers.

use egui::{Align2, Color32, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use equashion_egui::equashion_math::{GalleryTemplate, Math, galleries};
use equashion_egui::EquationEditor;

use crate::SlideApp;
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets::{RIBBON_H, group, rows};

const NOT_YET: &str = "Not available in DeckCraft's equation editor yet";

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
fn structure_menu(ui: &mut Ui, ed: &mut EquationEditor, gallery_id: &str) {
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
                        ed.insert_gallery_template(ui, &tpl.math);
                        ui.close();
                    }
                }
            });
            ui.add_space(4.0);
        }
    });
}

/// The ready-made equations (Equation › Equation).
fn builtin_menu(ui: &mut Ui, ed: &mut EquationEditor) {
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
                ed.replace_equation(ui, b.math.clone());
                ui.close();
            }
            ui.add_space(4.0);
        }
    });
    ui.separator();
    if ui.button("Insert New Equation").clicked() {
        ed.replace_equation(ui, Math::new(vec![]));
        ui.close();
    }
}

/// Insert › Equation: a split button. The top inserts a new equation; the arrow lists the
/// ready-made ones (WordCraft's Insert › Equation), each opening the editor started from it.
pub fn insert_button(app: &mut SlideApp, ui: &mut Ui) {
    let (main, arrow) = crate::widgets::big_button_ex(ui, Icon::Equation, "Equation", true, true, false);
    if main.on_hover_text("Insert a new equation").clicked() {
        let _ = app.run("app.dialog", serde_json::json!({"id": "equation"}));
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
        let mut d = crate::dialogs::Dialog::new("equation");
        if !id.is_empty() {
            d.params = serde_json::json!({"builtin": id});
        }
        app.dialog = Some(d);
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
fn symbols(ui: &mut Ui, ed: &mut EquationEditor) {
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
                        ed.insert_gallery_symbol(ui, *c);
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
                                ed.insert_gallery_symbol(ui, c);
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

/// A small ribbon button that is shown but not wired yet.
fn unavailable(ui: &mut Ui, icon: Icon, label: &str) {
    small_button(ui, icon, label, false, false).on_hover_text(NOT_YET);
}

pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let mut close = false;
    {
        let Some(ed) = app.equation.widget_mut() else { return };
        group(ui, |ui| {
            let g = galleries();
            let r = math_button(ui, ed, "tab/equation", &g.equation_picture, "Equation");
            egui::Popup::menu(&r).show(|ui| {
                ui.set_min_width(260.0);
                builtin_menu(ui, ed);
            });
            rows(ui, |ui| {
                unavailable(ui, Icon::Equation, "Unicode");
                unavailable(ui, Icon::Equation, "LaTeX");
                unavailable(ui, Icon::Section, "Convert");
            });
            rows(ui, |ui| {
                unavailable(ui, Icon::ChangeCase, "Normal Text");
            });
        });
        group(ui, |ui| symbols(ui, ed));
        group(ui, |ui| {
            for g in &galleries().structures {
                let r = math_button(ui, ed, &format!("tab/pic/{}", g.id), &g.picture, &g.label);
                egui::Popup::menu(&r).show(|ui| {
                    ui.set_min_width(260.0);
                    structure_menu(ui, ed, &g.id);
                });
            }
        });
        group(ui, |ui| {
            let display = ed.is_display();
            rows(ui, |ui| {
                if small_button(ui, Icon::AlignCenter, "Display", true, display).on_hover_text("Show the equation on a line of its own").clicked() {
                    ed.set_display(true);
                }
                if small_button(ui, Icon::AlignLeft, "Inline", true, !display).on_hover_text("Show the equation in the line of text").clicked() {
                    ed.set_display(false);
                }
                unavailable(ui, Icon::Numbering, "Number");
            });
            rows(ui, |ui| {
                let r = small_button(ui, Icon::Justify, "Justification", display, false).on_hover_text("Justification of a display equation");
                egui::Popup::menu(&r).show(|ui| {
                    for (label, jc) in [("Left", "left"), ("Right", "right"), ("Centered", "center"), ("Centered as Group", "centerGroup")] {
                        if ui.button(label).clicked() {
                            ed.set_justification(Some(jc));
                            ui.close();
                        }
                    }
                });
                if ui.button("Close Equation").on_hover_text("Apply the equation and leave it").clicked() {
                    close = true;
                }
            });
        });
    }
    if close {
        app.equation.commit_requested = true;
    }
}
