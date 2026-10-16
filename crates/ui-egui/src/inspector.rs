//! The canvas shell's Design tab. With a selection it shows the property editor (`props.rs`);
//! with nothing selected, the deck's text styles (click one to edit it for the whole deck) and the
//! slide's layout and background. Also the type details and style menu the property editor uses.
//! Every control runs an engine command, and each one writes something PPTX already stores: font
//! weights are offered only as names a font exposes (Regular and Bold, or a family name such as
//! "Inter SemiBold"), never as free axis values.

use deckcraft_fonts::FontDb;
use deckcraft_model::Fill;
use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::icons::{self, Icon};
use crate::ribbon::cref_param;
use crate::theme::{self, Tokens};
use crate::{SlideApp, widgets};

/// Letter spacing presets, in points (the values PowerPoint's Character Spacing menu uses).
const SPACING: [(&str, f64); 5] = [("Very tight", -3.0), ("Tight", -1.5), ("Normal", 0.0), ("Loose", 3.0), ("Very loose", 6.0)];

fn run(app: &mut SlideApp, id: &str, p: Value) {
    let _ = app.run(id, p);
}

pub fn design(app: &mut SlideApp, ui: &mut Ui) {
    let Some(st) = app.session.active() else { return };
    let editing = st.selection.text.as_ref().is_some_and(|t| !t.notes);
    let shapes: Vec<deckcraft_model::Shape> = st.selected_shapes().into_iter().cloned().collect();
    if shapes.is_empty() && !editing {
        text_styles(app, ui);
        section(ui, "Slide");
        slide(app, ui);
        return;
    }
    crate::props::show(app, ui);
}

// ----- Layout helpers ---------------------------------------------------------------------

/// A section: hairline above, bold title, optional icon at the right.
fn section(ui: &mut Ui, title: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(6.0);
    let y = ui.cursor().min.y;
    ui.painter().hline(ui.max_rect().x_range().expand(12.0), y, Stroke::new(1.0, t.separator));
    ui.add_space(10.0);
    ui.label(egui::RichText::new(title).font(theme::bold(13.0)).color(t.text));
    ui.add_space(4.0);
}

fn caption(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(text).font(theme::font(11.5)).color(t.text_dim));
}

/// A square toggle in a segmented row.
fn seg(ui: &mut Ui, icon: Icon, tip: &str, on: bool, en: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(30.0, 28.0), if en { Sense::click() } else { Sense::hover() });
    let p = ui.painter_at(r);
    if on {
        p.rect_filled(r, CornerRadius::same(5), t.accent.gamma_multiply(0.18));
    } else if resp.hovered() {
        p.rect_filled(r, CornerRadius::same(5), t.hover);
    }
    icons::paint(&p, r.shrink(7.0), icon, if on { t.accent } else { t.text }, !en);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, en, on, tip));
    resp.on_hover_text(tip)
}

/// A colour swatch button that opens the theme colour grid.
fn color_field(
    app: &SlideApp,
    ui: &mut Ui,
    label: &str,
    swatch: Option<Color32>,
    none_label: Option<&str>,
) -> Option<Option<deckcraft_model::ColorRef>> {
    let t = Tokens::get(ui.ctx());
    let sc = scheme(app);
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
    ui.painter().rect(
        r,
        CornerRadius::same(5),
        if resp.hovered() { t.hover } else { t.field_bg },
        Stroke::new(1.0, t.border),
        egui::StrokeKind::Inside,
    );
    let sw = Rect::from_center_size(pos2(r.min.x + 16.0, r.center().y), vec2(14.0, 14.0));
    match swatch {
        Some(c) => {
            ui.painter().rect(sw, CornerRadius::same(3), c, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        }
        None => {
            ui.painter().rect_stroke(sw, CornerRadius::same(3), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
            ui.painter().line_segment([sw.left_bottom(), sw.right_top()], Stroke::new(1.0, Color32::from_rgb(0xD0, 0x30, 0x30)));
        }
    }
    ui.painter().text(pos2(r.min.x + 32.0, r.center().y), Align2::LEFT_CENTER, label, theme::font(12.5), t.text);
    widgets::chevron(ui.painter(), pos2(r.max.x - 12.0, r.center().y), 3.0, t.text_dim);
    let mut out = None;
    widgets::color_popup(&resp).show(|ui| {
        out = widgets::color_grid(ui, &sc, none_label);
    });
    out
}

fn t_auto(ui: &Ui) -> Color32 {
    Tokens::get(ui.ctx()).text_faint
}

fn scheme(app: &SlideApp) -> deckcraft_color::ColorScheme {
    app.session.active().and_then(|d| d.doc.masters.first().map(|m| m.scheme())).unwrap_or_else(|| deckcraft_model::Theme::default().colors)
}

// ----- Text -------------------------------------------------------------------------------

/// The font's base family, the weights it can be set to and the current one.
///
/// A weight is offered when PPTX can name it: Regular and Bold (the bold flag), or a style whose
/// font has a family name of its own ("Inter SemiBold"), which is how PowerPoint stores it.
pub fn weights(font: &str, bold: bool) -> (String, Vec<String>, String) {
    let db = FontDb::global();
    let plain = || (font.to_string(), vec!["Regular".to_string(), "Bold".to_string()], if bold { "Bold" } else { "Regular" }.to_string());
    if font.is_empty() || !db.has_family(font) {
        return plain();
    }
    let base = db.face(font, "Regular").family.clone();
    let current = match font.get(base.len()..) {
        Some(rest) if font.len() > base.len() && font[..base.len()].eq_ignore_ascii_case(&base) => rest.trim().to_string(),
        _ if bold => "Bold".into(),
        _ => "Regular".into(),
    };
    let mut styles: Vec<String> = db
        .styles(&base)
        .into_iter()
        .filter(|s| !s.to_lowercase().contains("italic") && !s.to_lowercase().contains("oblique"))
        .filter(|s| s == "Regular" || s == "Bold" || db.has_family(&format!("{base} {s}")))
        .collect();
    for must in ["Regular", "Bold"] {
        if !styles.iter().any(|s| s == must) {
            styles.push(must.into());
        }
    }
    (base, styles, current)
}

/// The commands that set a weight (see [`weights`]).
pub fn weight_commands(base: &str, style: &str) -> Vec<(&'static str, Value)> {
    match style {
        "Regular" => vec![("format.font", json!({"family": base})), ("format.bold", json!({"on": false}))],
        "Bold" => vec![("format.font", json!({"family": base})), ("format.bold", json!({"on": true}))],
        s => vec![("format.font", json!({"family": format!("{base} {s}")})), ("format.bold", json!({"on": false}))],
    }
}

/// Type details: font, weight, size, line height, letter spacing, bold / italic / underline /
/// strikethrough, lists and (when `color`) the text colour.
pub(crate) fn text_details(app: &mut SlideApp, ui: &mut Ui, st: &Value, en: bool, color: bool) {
    let font = st.get("font").and_then(Value::as_str).unwrap_or("").to_string();
    let bold = st.get("bold").and_then(Value::as_bool).unwrap_or(false);
    let (base, styles, current) = weights(&font, bold);
    let w = ui.available_width();
    let families = crate::ribbon::font_families(app);
    widgets::dropdown(ui, "insp-font", if base.is_empty() { "Font" } else { &base }, w, en, |ui| {
        for f in families {
            if ui.selectable_label(f == base, &f).clicked() {
                run(app, "format.font", json!({"family": f}));
                ui.close();
            }
        }
    });
    ui.add_space(6.0);
    let half = (w - 8.0) / 2.0;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(half);
            caption(ui, "Weight");
            widgets::dropdown(ui, "insp-weight", &current, half, en, |ui| {
                for s in &styles {
                    if ui.selectable_label(*s == current, s).clicked() {
                        for (id, p) in weight_commands(&base, s) {
                            run(app, id, p);
                        }
                        ui.close();
                    }
                }
            });
        });
        ui.vertical(|ui| {
            caption(ui, "Size");
            let u = crate::units::small(app);
            let size = st.get("size").and_then(Value::as_f64).unwrap_or(18.0);
            if let Some(v) = widgets::spinner(ui, "insp-size", u.show(size), 1.0, u.suffix, half - 14.0) {
                run(app, "format.size", json!({"size": u.to_pt(v).clamp(1.0, 4000.0)}));
            }
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(half);
            caption(ui, "Line height");
            let pct = match st.get("lineSpacing") {
                Some(v) if v.get("kind").and_then(Value::as_str) == Some("pct") => v.get("v").and_then(Value::as_f64).unwrap_or(1.0) * 100.0,
                _ => 100.0,
            };
            if let Some(v) = widgets::spinner(ui, "insp-lh", pct.round(), 5.0, "%", half - 14.0) {
                run(app, "format.lineSpacing", json!({"lines": (v / 100.0).clamp(0.1, 9.99)}));
            }
        });
        ui.vertical(|ui| {
            caption(ui, "Letter spacing");
            widgets::dropdown(ui, "insp-spacing", "Spacing", half, en, |ui| {
                for (l, pt) in SPACING {
                    if ui.button(l).clicked() {
                        run(app, "format.spacing", json!({"pt": pt}));
                        ui.close();
                    }
                }
            });
        });
    });
    ui.add_space(6.0);
    let flag = |k: &str| st.get(k).and_then(Value::as_bool).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        for (icon, tip, id, key) in [
            (Icon::Bold, "Bold (⌘B)", "format.bold", "bold"),
            (Icon::Italic, "Italic (⌘I)", "format.italic", "italic"),
            (Icon::Underline, "Underline (⌘U)", "format.underline", "underline"),
            (Icon::Strikethrough, "Strikethrough", "format.strikethrough", "strike"),
        ] {
            if seg(ui, icon, tip, flag(key), en).clicked() {
                run(app, id, json!({"on": !flag(key)}));
            }
        }
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        if seg(ui, Icon::Bullets, "Bullets", flag("bullets"), en).clicked() {
            run(app, "format.bullets", json!({"on": !flag("bullets")}));
        }
        if seg(ui, Icon::Numbering, "Numbering", flag("numbering"), en).clicked() {
            run(app, "format.numbering", json!({"on": !flag("numbering")}));
        }
    });
    if color {
        ui.add_space(6.0);
        caption(ui, "Color");
        if let Some(Some(c)) = color_field(app, ui, "Text color", Some(t_auto(ui)), None) {
            run(app, "format.color", json!({"color": cref_param(&c)}));
        }
    }
}

/// The text style menu, opened by `button`: the deck's styles as previews; picking one applies it
/// (a link where the box can follow the style, else a copy).
pub(crate) fn style_menu(app: &mut SlideApp, button: &egui::Response, current: &str, w: f32) {
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let styles = crate::textstyles::styles(&doc);
    egui::Popup::menu(button).width(w).show(|ui| {
        for s in &styles {
            // A preview at a readable size: the style's own weight, its size scaled down.
            let size = (s.size() as f32 * 0.45).clamp(12.0, 26.0);
            let font = if s.bold() { theme::bold(size) } else { theme::font(size) };
            let label = egui::RichText::new(s.name).font(font);
            let tip = if crate::textstyles::links(app, s) {
                "Follows the style: editing the style changes this text"
            } else {
                "This box can't follow the style in PPTX: its values are copied"
            };
            let on = current == s.name || current.strip_suffix(" (copy)") == Some(s.name);
            if ui.add(egui::Button::selectable(on, label).min_size(vec2(w - 12.0, 0.0))).on_hover_text(tip).clicked() {
                crate::textstyles::apply(app, s);
                ui.close();
            }
        }
    });
}

/// Nothing selected: the slide's layout, then its background.
fn slide(app: &mut SlideApp, ui: &mut Ui) {
    let Some(st) = app.session.active() else { return };
    let layout =
        st.current_slide().and_then(|s| st.doc.masters.first().and_then(|m| m.layouts.iter().find(|l| l.id == s.layout))).map(|l| l.name.clone());
    let t = Tokens::get(ui.ctx());
    caption(ui, "Layout");
    let w = ui.available_width();
    let r =
        ui.add_sized(vec2(w, 26.0), egui::Button::new(layout.unwrap_or_else(|| "Layout".into())).fill(t.field_bg).stroke(Stroke::new(1.0, t.border)));
    egui::Popup::menu(&r).width(420.0).show(|ui| crate::ribbon::layout_menu(app, ui, "slide.layout"));
    section(ui, "Background");
    crate::panes::background(app, ui);
}

// ----- Text styles (nothing selected) ------------------------------------------------------

/// Which text style the panel is editing (an index into `textstyles::styles`), if any.
fn editing_style_id() -> egui::Id {
    egui::Id::new("canvas-inspector-editing-style")
}

pub fn edit_text_style(ctx: &egui::Context, index: Option<usize>) {
    ctx.data_mut(|d| d.insert_temp(editing_style_id(), index));
}

/// The deck's text styles: click one to edit it for the whole deck.
fn text_styles(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let styles = crate::textstyles::styles(&doc);
    let editing: Option<usize> = ui.ctx().data(|d| d.get_temp(editing_style_id())).flatten();
    if let Some(s) = editing.and_then(|k| styles.get(k)) {
        style_editor(app, ui, &doc, s);
        return;
    }
    ui.label(egui::RichText::new("Text styles").font(theme::bold(13.0)).color(t.text));
    ui.add_space(4.0);
    let w = ui.available_width();
    for (k, s) in styles.iter().enumerate() {
        let size = (s.size() as f32 * 0.42).clamp(12.0, 22.0);
        let (r, resp) = ui.allocate_exact_size(vec2(w, size + 24.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
        }
        let font = if s.bold() { theme::bold(size) } else { theme::font(size) };
        ui.painter().text(pos2(r.min.x + 8.0, r.min.y + 4.0), Align2::LEFT_TOP, s.name, font, t.text);
        let u = crate::units::small(app);
        let detail = format!("{} · {}{}{}", s.family(&doc), widgets::fmt_num(u.show(s.size()), ""), u.suffix, if s.bold() { " · Bold" } else { "" });
        ui.painter().text(pos2(r.min.x + 8.0, r.max.y - 4.0), Align2::LEFT_BOTTOM, detail, theme::font(11.0), t.text_dim);
        widgets::chevron(ui.painter(), pos2(r.max.x - 12.0, r.center().y), 3.0, t.text_faint);
        let label = format!("Edit text style {}", s.name);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        if resp.on_hover_text("Edit this style for the whole presentation").clicked() {
            edit_text_style(ui.ctx(), Some(k));
        }
    }
}

/// Edit one style; every change applies to the whole presentation.
fn style_editor(app: &mut SlideApp, ui: &mut Ui, doc: &deckcraft_model::Presentation, s: &crate::textstyles::TextStyle) {
    use crate::textstyles::Edit;
    let t = Tokens::get(ui.ctx());
    if ui.add(egui::Button::new("‹ Text styles").frame(false)).clicked() {
        edit_text_style(ui.ctx(), None);
    }
    ui.add_space(2.0);
    let size = (s.size() as f32 * 0.5).clamp(14.0, 28.0);
    ui.label(egui::RichText::new(s.name).font(if s.bold() { theme::bold(size) } else { theme::font(size) }).color(t.text));
    let n = crate::textstyles::copies(doc, s).len();
    let reach = match n {
        0 => format!("Changes all {} text in this presentation.", s.name),
        1 => format!("Changes all {} text, and 1 copy of it.", s.name),
        n => format!("Changes all {} text, and {n} copies of it.", s.name),
    };
    caption(ui, &reach);
    ui.add_space(6.0);
    let family = s.family(doc);
    let w = ui.available_width();
    caption(ui, "Font");
    let families = crate::ribbon::font_families(app);
    let mut edits: Vec<Edit> = vec![];
    widgets::dropdown(ui, "style-font", &family, w, true, |ui| {
        for f in families {
            if ui.selectable_label(f == family, &f).clicked() {
                edits.push(Edit::Font(f));
                ui.close();
            }
        }
    });
    match s.theme_font() {
        Some(true) => caption(ui, "The theme's heading font: every style and shape in it changes too."),
        Some(false) => caption(ui, "The theme's body font: every style and shape in it changes too."),
        None => {}
    }
    ui.add_space(4.0);
    let half = (w - 8.0) / 2.0;
    let (base, weights, cur) = weights(&family, s.bold());
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(half);
            caption(ui, "Weight");
            widgets::dropdown(ui, "style-weight", &cur, half, true, |ui| {
                for wname in &weights {
                    if ui.selectable_label(*wname == cur, wname).clicked() {
                        match wname.as_str() {
                            "Regular" | "Bold" => {
                                // From a named weight family back to the base family.
                                if family != base {
                                    edits.push(Edit::Font(base.clone()));
                                }
                                edits.push(Edit::Bold(wname == "Bold"));
                            }
                            other => {
                                edits.push(Edit::Font(format!("{base} {other}")));
                                edits.push(Edit::Bold(false));
                            }
                        }
                        ui.close();
                    }
                }
            });
        });
        ui.vertical(|ui| {
            caption(ui, "Size");
            let u = crate::units::small(app);
            if let Some(v) = widgets::spinner(ui, "style-size", u.show(s.size()), 1.0, u.suffix, half - 14.0) {
                edits.push(Edit::Size(u.to_pt(v).clamp(1.0, 4000.0)));
            }
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(half);
            caption(ui, "Line height");
            let pct = match s.line_spacing {
                Some(deckcraft_model::text::Spacing::Pct(v)) => v * 100.0,
                _ => 100.0,
            };
            if let Some(v) = widgets::spinner(ui, "style-lh", pct.round(), 5.0, "%", half - 14.0) {
                edits.push(Edit::LineSpacing((v / 100.0).clamp(0.1, 9.99)));
            }
        });
        ui.vertical(|ui| {
            caption(ui, "Style");
            if seg(ui, Icon::Italic, "Italic", s.italic(), true).clicked() {
                edits.push(Edit::Italic(!s.italic()));
            }
        });
    });
    ui.add_space(4.0);
    caption(ui, "Color");
    let sc = scheme(app);
    let swatch = match &s.run.fill {
        Some(Fill::Solid { color }) => Some(theme::to_color32(color.resolve(&sc, None))),
        _ => Some(t_auto(ui)),
    };
    if let Some(Some(c)) = color_field(app, ui, "Text color", swatch, None) {
        edits.push(Edit::Color(c));
    }
    if !edits.is_empty() {
        let mut cmds = vec![];
        for e in &edits {
            cmds.extend(crate::textstyles::edit_commands(app, s, e));
        }
        app.run_as_one(&format!("Edit Text Style: {}", s.name), cmds);
    }
}

#[cfg(test)]
#[path = "inspector_tests.rs"]
mod tests;
