//! The Equation tab (shown while editing an equation) and the Insert › Equation gallery:
//! ready-made equations, input format and conversion, symbol sets, and the structure galleries
//! (fractions, scripts, radicals, integrals, large operators, brackets, functions, accents,
//! limits and logs, operators, matrices). Tiles and button pictures are rendered by the math
//! engine itself.

use deckcraft_engine::math_gallery::{BUILT_INS, STRUCTURES, SYMBOL_SETS, Template};
use deckcraft_model::math::parse_linear;
use egui::{Align2, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets::{RIBBON_H, big_button_ex, group, rows, small_button, small_button_ex, split_menu};

/// What each structure button shows.
pub(crate) fn button_picture(id: &str) -> &'static str {
    match id {
        "fraction" => "x/y",
        "script" => "e^x",
        "radical" => "√(n&x)",
        "integral" => "∫_(−x)^x",
        "largeOperator" => "∑_(i=0)^n",
        "bracket" => "{()}",
        "function" => "sin⁡θ",
        "accent" => "ä",
        "limitLog" => "lim┬(n→∞)",
        "operator" => "≜",
        "matrix" => "[■(1&0@0&1)]",
        _ => "x",
    }
}

/// A rendered equation drawn into `r` (centred), or its linear text while it renders.
struct MathPreview<'a> {
    rect: Rect,
    key: &'a str,
    linear: &'a str,
    nodes: Option<deckcraft_model::math::Arg>,
    display: bool,
    points: f32,
}

/// Draws a cached rasterized math preview, or falls back to placeholder text.
fn paint_math(app: &mut SlideApp, ui: &Ui, preview: MathPreview<'_>) {
    let MathPreview { rect: r, key, linear, nodes, display, points: pt } = preview;
    let nodes = nodes.unwrap_or_else(|| parse_linear(linear));
    match app.textures.math_preview(ui.ctx(), key, &nodes, display, pt, r.size()) {
        Some((tex, uv)) => {
            ui.painter().image(tex, r, uv, Tokens::get(ui.ctx()).text);
        }
        None => {
            let t = Tokens::get(ui.ctx());
            let placeholder = math_placeholder(ui, linear);
            ui.painter().with_clip_rect(r).text(r.center(), Align2::CENTER_CENTER, placeholder, theme::font(11.0), t.text_dim);
        }
    }
}

/// Generates a font-safe placeholder string for an equation.
fn math_placeholder(ui: &Ui, linear: &str) -> String {
    let font = theme::font(11.0);
    math_placeholder_with(linear, |c| ui.fonts_mut(|fonts| fonts.has_glyph(&font, c)))
}

/// Formats a linear math string as a placeholder preview, replacing glyphs not covered by the font with names or codes.
pub(crate) fn math_placeholder_with(linear: &str, covered: impl Fn(char) -> bool) -> String {
    let mut result = String::new();
    for c in linear.chars() {
        if c.is_whitespace() || covered(c) {
            result.push(c);
        } else if let Some(name) = deckcraft_model::math_symbols::name_of(c) {
            result.push('\\');
            result.push_str(name);
        } else {
            use std::fmt::Write as _;
            let _ = write!(result, "U+{:04X}", c as u32);
        }
    }
    result
}

/// The Equation tab's own big button picture.
const EQUATION_PICTURE: &str = "∑_(i=1)^n";

/// A big ribbon button whose picture is an equation; opens `menu`.
fn math_menu_button(ui: &mut Ui, app: &mut SlideApp, picture: &str, label: &str, menu: impl FnOnce(&mut Ui, &mut SlideApp)) {
    let t = Tokens::get(ui.ctx());
    let font = theme::font(11.0);
    let galley_w =
        label.split('\n').map(|l| ui.ctx().fonts_mut(|f| f.layout_no_wrap(l.to_string(), font.clone(), t.text).size().x)).fold(0.0, f32::max);
    let w = (galley_w + 22.0).max(46.0);
    let (r, resp) = ui.allocate_exact_size(vec2(w, RIBBON_H - 2.0), Sense::click());
    let spoken = label.replace('\n', " ");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &spoken));
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(4), t.hover);
    }
    let pic = Rect::from_center_size(pos2(r.center().x, r.min.y + 19.0), vec2(40.0, 32.0));
    ui.painter().rect_filled(pic, CornerRadius::same(3), t.field_bg);
    paint_math(app, ui, MathPreview { rect: pic, key: &format!("btn:{picture}"), linear: picture, nodes: None, display: false, points: 12.0 });
    let lines: Vec<&str> = label.split('\n').collect();
    let mut y = r.min.y + 39.0;
    for (i, l) in lines.iter().enumerate() {
        let lw = ui.fonts_mut(|f| f.layout_no_wrap(l.to_string(), font.clone(), t.text).size().x);
        let is_last = i + 1 == lines.len();
        let x = r.center().x - (lw + if is_last { 8.0 } else { 0.0 }) / 2.0;
        ui.painter().text(pos2(x, y), Align2::LEFT_TOP, l, font.clone(), t.text);
        if is_last {
            crate::widgets::chevron(ui.painter(), pos2(x + lw + 5.0, y + 6.5), 2.8, t.text_dim);
        }
        y += 12.5;
    }
    let resp = resp.on_hover_text(&spoken);
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(260.0);
        menu(ui, app);
    });
}

/// One gallery tile: the template rendered, its name as a tooltip.
fn tile(ui: &mut Ui, app: &mut SlideApp, tpl: &Template, size: egui::Vec2) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tpl.label));
    ui.painter().rect_filled(r, CornerRadius::same(2), t.field_bg);
    if resp.hovered() {
        ui.painter().rect_stroke(r, CornerRadius::same(2), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
    }
    let pt = if size.x > 120.0 { 13.0 } else { 14.0 };
    paint_math(
        app,
        ui,
        MathPreview { rect: r.shrink(2.0), key: &format!("tpl:{}", tpl.id), linear: tpl.linear, nodes: Some(tpl.nodes()), display: true, points: pt },
    );
    resp.on_hover_text(tpl.label).clicked()
}

/// Renders a section header label within a gallery popup.
fn section_heading(ui: &mut Ui, name: &str) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().max(240.0), 20.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::ZERO, t.hover);
    ui.painter().text(pos2(r.min.x + 6.0, r.center().y), Align2::LEFT_CENTER, name, theme::bold(11.5), t.text);
}

/// A structure gallery's menu.
fn structure_menu(ui: &mut Ui, app: &mut SlideApp, gallery_id: &str) {
    let Some(g) = STRUCTURES.iter().find(|g| g.id == gallery_id) else { return };
    egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
        ui.set_width(330.0);
        for (name, templates) in g.sections {
            section_heading(ui, name);
            let wide = name.starts_with("Common") || templates.iter().all(|t| !t.linear.contains('⬚')) && templates.len() <= 3;
            let size = if wide { vec2(158.0, 60.0) } else { vec2(60.0, 56.0) };
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for tpl in templates.iter() {
                    if tile(ui, app, tpl, size) {
                        let _ = app.run("equation.insertStructure", json!({"id": tpl.id}));
                        ui.close();
                    }
                }
            });
            ui.add_space(4.0);
        }
    });
}

/// The ready-made equations (Insert › Equation and Equation › Equation).
pub fn builtin_menu(ui: &mut Ui, app: &mut SlideApp) {
    egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
        ui.set_width(360.0);
        section_heading(ui, "Built-In");
        for (id, label, lin) in BUILT_INS {
            let t = Tokens::get(ui.ctx());
            ui.label(egui::RichText::new(*label).font(theme::bold(11.5)).color(t.text));
            let (r, resp) = ui.allocate_exact_size(vec2(350.0, 64.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, *label));
            ui.painter().rect_filled(r, CornerRadius::same(2), t.field_bg);
            if resp.hovered() {
                ui.painter().rect_stroke(r, CornerRadius::same(2), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
            }
            paint_math(
                app,
                ui,
                MathPreview { rect: r.shrink(2.0), key: &format!("builtin:{id}"), linear: lin, nodes: None, display: true, points: 11.0 },
            );
            if resp.clicked() {
                let _ = app.run("insert.equation", json!({"builtin": id}));
                app.ui.tab = "Equation".into();
                ui.close();
            }
            ui.add_space(4.0);
        }
    });
    ui.separator();
    if ui.button("Insert New Equation").clicked() {
        let _ = app.run("insert.equation", json!({}));
        app.ui.tab = "Equation".into();
        ui.close();
    }
}

/// Insert › Symbols › Equation: a split button (insert a new equation, or pick a ready-made one).
pub fn insert_button(app: &mut SlideApp, ui: &mut Ui) {
    let en = super::enabled(app, "insert.equation");
    let (main, arrow) = big_button_ex(ui, Icon::Equation, "Equation", en, true, false);
    let main = main.on_hover_text(super::tip("insert.equation", "Equation"));
    if main.clicked() {
        let _ = app.run("insert.equation", json!({}));
        app.ui.tab = "Equation".into();
    }
    if let Some(a) = arrow {
        egui::Popup::menu(&a).show(|ui| builtin_menu(ui, app));
    }
    crate::qat::ribbon_menu(app, &main, Icon::Equation, "Equation", "insert.equation", &json!({}));
}

/// The Symbols group: a strip of the current set's symbols, and a menu with every set.
fn symbols(ui: &mut Ui, app: &mut SlideApp) {
    let id = egui::Id::unique("eq_symbol_set");
    let set = ui.data(|d| d.get_temp::<usize>(id)).unwrap_or(0).min(SYMBOL_SETS.len().saturating_sub(1));
    let Some((_, chars)) = SYMBOL_SETS.get(set) else { return };
    let start_id = egui::Id::unique("eq_symbol_start");
    let per_row = 10usize;
    let rows_count = 3usize;
    let all: Vec<char> = chars.chars().collect();
    let start = ui.data(|d| d.get_temp::<usize>(start_id)).unwrap_or(0).min(all.len().saturating_sub(1));
    let t = Tokens::get(ui.ctx());
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
        for row in 0..rows_count {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
                for col in 0..per_row {
                    let k = start + row * per_row + col;
                    let Some(c) = all.get(k) else {
                        ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                        continue;
                    };
                    if symbol_button(ui, app, *c, &t) {
                        let _ = app.run("equation.insertSymbol", json!({"char": c.to_string()}));
                    }
                }
            });
        }
    });
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 1.0);
        let page = per_row * rows_count;
        if ui.add_enabled(start > 0, egui::Button::new("▲").min_size(vec2(16.0, 20.0))).clicked() {
            ui.data_mut(|d| d.insert_temp(start_id, start.saturating_sub(per_row)));
        }
        if ui.add_enabled(start + page < all.len(), egui::Button::new("▼").min_size(vec2(16.0, 20.0))).clicked() {
            ui.data_mut(|d| d.insert_temp(start_id, start + per_row));
        }
        let resp = ui.add(egui::Button::new("…").min_size(vec2(16.0, 20.0))).on_hover_text("All symbol sets");
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_width(420.0);
            ui.horizontal_wrapped(|ui| {
                for (i, (name, _)) in SYMBOL_SETS.iter().enumerate() {
                    if ui.selectable_label(i == set, *name).clicked() {
                        ui.data_mut(|d| {
                            d.insert_temp(id, i);
                            d.insert_temp(start_id, 0usize);
                        });
                    }
                }
            });
            ui.separator();
            let set = ui.data(|d| d.get_temp::<usize>(id)).unwrap_or(0);
            if let Some((_, chars)) = SYMBOL_SETS.get(set) {
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
                        for c in chars.chars() {
                            if symbol_button(ui, app, c, &t) {
                                let _ = app.run("equation.insertSymbol", json!({"char": c.to_string()}));
                            }
                        }
                    });
                });
            }
        });
    });
}

/// Renders a single clickable math symbol tile in the symbols strip.
fn symbol_button(ui: &mut Ui, app: &mut SlideApp, c: char, t: &Tokens) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
    let s = c.to_string();
    let display = math_placeholder(ui, &s);
    let tooltip = symbol_tooltip(c, &display);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &tooltip));
    ui.painter().rect_filled(r, CornerRadius::same(2), t.field_bg);
    if resp.hovered() {
        ui.painter().rect_stroke(r, CornerRadius::same(2), Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    }
    let nodes = vec![deckcraft_model::math::MNode::Run(deckcraft_model::math::MRun { text: s.clone(), lit: true, ..Default::default() })];
    paint_math(app, ui, MathPreview { rect: r, key: &format!("sym:{s}"), linear: &s, nodes: Some(nodes), display: false, points: 11.0 });
    resp.on_hover_text(tooltip).clicked()
}

/// Formats tooltip text for a math symbol showing glyph and LaTeX command name.
fn symbol_tooltip(c: char, display: &str) -> String {
    if display != c.to_string() {
        return display.to_owned();
    }
    deckcraft_model::math_symbols::name_of(c).map(|n| format!("{display}  \\{n}")).unwrap_or_else(|| format!("{display}  U+{:04X}", c as u32))
}

/// The Equation tab.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        math_menu_button(ui, app, EQUATION_PICTURE, "Equation", builtin_menu);
        rows(ui, |ui| {
            let latex = app.session.math_latex;
            let en = super::enabled(app, "equation.inputFormat");
            if small_button(ui, Icon::Equation, "Unicode", en, !latex).on_hover_text("Type equations in the linear format (UnicodeMath)").clicked() {
                let _ = app.run("equation.inputFormat", json!({"format": "unicode"}));
            }
            if small_button(ui, Icon::InkToMath, "LaTeX", en, latex).on_hover_text("Type equations in LaTeX").clicked() {
                let _ = app.run("equation.inputFormat", json!({"format": "latex"}));
            }
            let conv_en = super::enabled(app, "equation.convert");
            let (main, arrow) = small_button_ex(ui, Icon::ResetSlide, "Convert", conv_en, false, true);
            let main = main.on_hover_text("Convert between built-up and linear form");
            if let Some(a) = arrow {
                split_menu(&main, &a, |ui| {
                    for (label, to, all) in [
                        ("Current - Professional", "professional", false),
                        ("Current - Linear", "linear", false),
                        ("All - Professional", "professional", true),
                        ("All - Linear", "linear", true),
                    ] {
                        if ui.button(label).clicked() {
                            let _ = app.run("equation.convert", json!({"to": to, "all": all}));
                            ui.close();
                        }
                    }
                });
            }
        });
        rows(ui, |ui| {
            let normal = app.session.math_normal_text;
            let en = super::enabled(app, "equation.normalText");
            if small_button(ui, Icon::ChangeCase, "Normal Text", en, normal).on_hover_text("Type normal (non-math) text in the equation").clicked() {
                let _ = app.run("equation.normalText", json!({}));
            }
        });
    });
    group(ui, |ui| symbols(ui, app));
    group(ui, |ui| {
        for g in STRUCTURES {
            let id = g.id;
            math_menu_button(ui, app, button_picture(id), g.label, |ui, app| structure_menu(ui, app, id));
        }
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            let info = app.session.execute("equation.get", &json!({})).ok();
            let display = info.as_ref().and_then(|v| v.get("display")).and_then(|v| v.as_bool()).unwrap_or(false);
            let numbered = info.as_ref().and_then(|v| v.get("linear")).and_then(|v| v.as_str()).is_some_and(|l| l.contains('#'));
            let en_disp = super::enabled(app, "equation.display");
            if small_button(ui, Icon::AlignCenter, "Display", en_disp, display).on_hover_text("Show the equation on a line of its own").clicked() {
                let _ = app.run("equation.display", json!({"value": true}));
            }
            if small_button(ui, Icon::AlignLeft, "Inline", en_disp, !display).on_hover_text("Show the equation in the line of text").clicked() {
                let _ = app.run("equation.display", json!({"value": false}));
            }
            let en_num = super::enabled(app, "equation.number");
            if small_button(ui, Icon::Numbering, "Number", en_num, numbered)
                .on_hover_text("Number the equation; the number sits at the right margin (or type # then the number)")
                .clicked()
            {
                let _ = app.run("equation.number", json!({}));
            }
        });
        rows(ui, |ui| {
            let en_jc = super::enabled(app, "equation.justify");
            let (main, arrow) = small_button_ex(ui, Icon::Justify, "Justification", en_jc, false, true);
            let main = main.on_hover_text("Justification of a display equation");
            if let Some(a) = arrow {
                split_menu(&main, &a, |ui| {
                    for (label, jc) in [("Left", "left"), ("Right", "right"), ("Centered", "center"), ("Centered as Group", "centerGroup")] {
                        if ui.button(label).clicked() {
                            let _ = app.run("equation.justify", json!({"jc": jc}));
                            ui.close();
                        }
                    }
                });
            }
            if ui.button("Close Equation").on_hover_text("Leave the equation (Esc)").clicked() {
                let _ = app.run("equation.exit", json!({}));
            }
        });
    });
}
