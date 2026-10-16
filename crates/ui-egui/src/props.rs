//! The canvas shell's property editor for a selection, laid out like a canvas editor's design
//! panel: a header naming the layer, then Position, Layout, Appearance, Typography, Fill, Stroke,
//! Effects and Export. Numbers sit in grey fields with a prefix (drag the prefix to scrub) and
//! are shown in pixels without a unit. Every control runs an existing command.

use deckcraft_model::{Fill, Geom, Shape, ShapeId, ShapeKind, Xfrm};
use egui::{Align2, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::icons::{self, Icon};
use crate::ribbon::{cref_param, enabled, fmt_state};
use crate::theme::{self, Tokens};
use crate::units;
use crate::{SlideApp, widgets};

pub(crate) const ROW_H: f32 = 30.0;
pub(crate) const GAP: f32 = 8.0;
/// The column at the right for a row's extra button (constrain proportions, details).
const SIDE: f32 = 30.0;

fn run(app: &mut SlideApp, id: &str, p: Value) {
    if let Err(e) = app.run(id, p) {
        app.set_status(e);
    }
}

pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let Some(st) = app.session.active() else { return };
    let shapes: Vec<Shape> = st.selected_shapes().into_iter().cloned().collect();
    let editing = st.selection.text.as_ref().filter(|t| !t.notes).map(|t| t.shape);
    let sh = match shapes.first().cloned().or_else(|| editing.and_then(|id| st.shape(id).cloned())) {
        Some(s) => s,
        None => return,
    };
    let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, &sh);
    // Every selected layer's box, for values shown as Mixed and spacing.
    let boxes: Vec<(deckcraft_model::ShapeId, deckcraft_model::Xfrm)> =
        shapes.iter().map(|s| (s.id, deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, s))).collect();
    let multi = shapes.len() > 1;
    let texty = editing.is_some() || shapes.iter().any(|s| s.text.is_some() || matches!(s.kind, ShapeKind::Shape));
    let picture = matches!(sh.kind, ShapeKind::Picture { .. });

    header_row(app, ui, &sh, multi, shapes.len());
    position(app, ui, &x, &boxes);
    layout(app, ui, &sh, &x, &boxes, texty && !multi);
    appearance(app, ui, &shapes, &boxes);
    if texty {
        typography(app, ui, &sh);
    }
    if !picture {
        fill(app, ui, &sh);
        stroke(app, ui, &sh);
    }
    effects(app, ui, &sh);
    export(app, ui);
    if ui.ctx().data(|d| d.get_temp::<bool>(more_id())).unwrap_or(false) {
        section(ui, "More options", false, &[]);
        crate::panes::format_shape(app, ui);
    }
}

fn more_id() -> egui::Id {
    egui::Id::new("props-more-options")
}

// ----- Building blocks ----------------------------------------------------------------------

/// A section: hairline above, title, buttons at the right. Returns the button clicked.
pub(crate) fn section(ui: &mut Ui, title: &str, empty: bool, actions: &[(Icon, &str)]) -> Option<usize> {
    section_buttons(ui, title, empty, actions).iter().position(|r| r.clicked())
}

/// `section`, returning the buttons (for a menu one of them opens).
fn section_buttons(ui: &mut Ui, title: &str, empty: bool, actions: &[(Icon, &str)]) -> Vec<egui::Response> {
    let t = Tokens::get(ui.ctx());
    ui.add_space(6.0);
    ui.painter().hline(ui.max_rect().x_range().expand(12.0), ui.cursor().min.y, Stroke::new(1.0, t.separator));
    ui.add_space(8.0);
    let mut out = vec![];
    ui.horizontal(|ui| {
        ui.set_min_height(26.0);
        ui.label(egui::RichText::new(title).font(theme::bold(13.0)).color(if empty { t.caption } else { t.text }));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (icon, tip) in actions.iter().rev() {
                out.push(icon_button(ui, *icon, tip, true));
            }
        });
    });
    out.reverse();
    ui.add_space(2.0);
    out
}

pub(crate) fn caption(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).font(theme::font(11.0)).color(t.caption));
}

pub(crate) fn icon_button(ui: &mut Ui, icon: Icon, tip: &str, en: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), if en { Sense::click() } else { Sense::hover() });
    if en && resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
    }
    icons::paint_mono(ui.painter(), r.shrink(5.0), icon, t.text, !en);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, en, tip));
    resp.on_hover_text(tip)
}

/// A row of buttons in one grey pill; the ones that are on show as raised. Returns the one clicked.
pub(crate) fn seg_group(ui: &mut Ui, items: &[(Icon, &str, bool)], width: f32, en: bool) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(6), t.field);
    let n = items.len().max(1) as f32;
    let cw = r.width() / n;
    let mut out = None;
    for (k, (icon, tip, on)) in items.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(r.min.x + k as f32 * cw, r.min.y), vec2(cw, r.height()));
        let resp = ui.interact(cell, ui.id().with(("seg", tip)), if en { Sense::click() } else { Sense::hover() });
        if *on {
            ui.painter().rect(cell.shrink(1.0), CornerRadius::same(5), t.card, Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
        } else if en && resp.hovered() {
            ui.painter().rect_filled(cell.shrink(1.0), CornerRadius::same(5), t.hover);
        }
        let s = (cell.height() - 14.0).min(cell.width() - 6.0);
        let ink = if *on { t.text } else { t.text_dim };
        icons::paint_mono(ui.painter(), Rect::from_center_size(cell.center(), vec2(s, s)), *icon, ink, !en);
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, en, *on, *tip));
        if resp.on_hover_text(*tip).clicked() {
            out = Some(k);
        }
    }
    out
}

#[derive(Clone, Copy)]
pub(crate) enum Prefix {
    Text(&'static str),
    Icon(Icon),
    /// No prefix (and so no scrubbing): the value alone, as a type size is shown.
    None,
}

/// A grey number field. Dragging the prefix scrubs the value (`step` per point dragged); typing
/// a number and pressing Enter or leaving the field sets it. Returns the new value when one is set.
#[allow(clippy::too_many_arguments)]
pub(crate) fn number(ui: &mut Ui, id: &str, prefix: Prefix, value: f64, suffix: &str, width: f32, step: f64, range: Range, en: bool) -> Option<f64> {
    number_or_mixed(ui, id, prefix, Some(value), suffix, width, step, range, en)
}

/// The values a field takes (dragging, arrow keys and typing all stay inside): `(min, max)`.
pub(crate) type Range = (f64, f64);
const ANY: Range = (-1e6, 1e6);
const NOT_NEGATIVE: Range = (0.0, 1e6);
const PERCENT: Range = (0.0, 100.0);
/// Width and height: at least one unit.
const SIZE: Range = (1.0, 1e6);

/// The value the selected layers share, or None (shown as Mixed) when they differ.
fn common(values: impl IntoIterator<Item = f64>) -> Option<f64> {
    let mut it = values.into_iter();
    let first = it.next()?;
    it.all(|v| (v - first).abs() < 0.005).then_some(first)
}

/// A value as a field shows it: to two decimals (whole numbers from dragging stay whole; a
/// typed 10.5 stays 10.5).
fn two(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// `number` for a value the selected layers may not share (`None`: shown as Mixed, and typing a
/// number sets it for all of them; scrubbing needs a value to start from).
#[allow(clippy::too_many_arguments)]
fn number_or_mixed(
    ui: &mut Ui,
    id: &str,
    prefix: Prefix,
    value: Option<f64>,
    suffix: &str,
    width: f32,
    step: f64,
    (lo, hi): Range,
    en: bool,
) -> Option<f64> {
    let clamp = |v: f64| v.clamp(lo, hi.max(lo));
    let mixed = value.is_none();
    let value = value.unwrap_or(0.0);
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(6), t.field);
    let pre_w = if matches!(prefix, Prefix::None) { 8.0 } else { 28.0 };
    let pre = Rect::from_min_size(r.min, vec2(pre_w, r.height()));
    let scrub = ui.interact(pre, ui.id().with((id, "scrub")), if en && !mixed { Sense::drag() } else { Sense::hover() });
    if en && !mixed && (scrub.hovered() || scrub.dragged()) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let skey = ui.id().with((id, "scrubbing"));
    let mut out = None;
    let mut shown = value;
    if scrub.dragged() {
        let acc = ui.data_mut(|d| d.get_temp::<f64>(skey)).unwrap_or(value) + scrub.drag_delta().x as f64 * step;
        ui.data_mut(|d| d.insert_temp(skey, acc));
        // Scrubbing lands on whole numbers; a typed value may have decimals.
        shown = clamp(acc.round());
    }
    if scrub.drag_stopped() {
        out = ui.data_mut(|d| d.remove_temp::<f64>(skey)).map(|v| clamp(v.round()));
        if let Some(v) = out {
            shown = v;
        }
    }
    let ink = if en { t.caption } else { t.text_faint };
    match prefix {
        Prefix::Text(s) => {
            ui.painter().text(pre.center(), Align2::CENTER_CENTER, s, theme::font(11.5), ink);
        }
        Prefix::Icon(i) => icons::paint_mono(ui.painter(), Rect::from_center_size(pre.center(), vec2(14.0, 14.0)), i, ink, !en),
        Prefix::None => {}
    }
    let tkey = ui.id().with((id, "typing"));
    let edit_id = ui.id().with((id, "edit"));
    let mut text: String =
        ui.data_mut(|d| d.get_temp(tkey)).unwrap_or_else(|| if mixed { String::new() } else { format!("{}{suffix}", widgets::fmt_num(shown, "")) });
    // ↑ / ↓ (⇧: by ten) while typing: applied at once, and the field keeps the keyboard. A Mixed
    // field steps from a number typed into it.
    if en
        && let Some(d) = widgets::arrow_step(ui, edit_id, step)
        && let Some(base) = widgets::parse_num(&text).or((!mixed).then_some(value))
    {
        let v = clamp(base + d);
        text = format!("{}{suffix}", widgets::fmt_num(v, ""));
        out = Some(v);
    }
    let er = Rect::from_min_max(pos2(pre.max.x, r.min.y), r.max).shrink2(vec2(2.0, 6.0));
    // In a child area, so the field doesn't move the row's cursor.
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(er).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let te = child.add_sized(
        er.size(),
        egui::TextEdit::singleline(&mut text)
            .frame(egui::Frame::NONE)
            .font(theme::font(12.5))
            .text_color(t.text)
            .hint_text(if mixed { "Mixed" } else { "" })
            .interactive(en)
            .id(edit_id),
    );
    if te.has_focus() {
        ui.data_mut(|d| d.insert_temp(tkey, text.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(tkey));
    }
    if te.lost_focus()
        && out.is_none()
        && let Some(v) = widgets::parse_num(&text).map(clamp)
        && (mixed || (v - value).abs() > 1e-9)
    {
        out = Some(v);
    }
    out
}

/// A grey dropdown field like the number fields: `label`, a chevron, and a menu of `add`.
pub(crate) fn pick(ui: &mut Ui, id: &str, label: &str, width: f32, en: bool, add: impl FnOnce(&mut Ui)) -> egui::Response {
    pick_wide(ui, id, label, width, width.max(180.0), en, add)
}

/// `pick` with a menu `menu_w` wide (a gallery).
pub(crate) fn pick_wide(ui: &mut Ui, id: &str, label: &str, width: f32, menu_w: f32, en: bool, add: impl FnOnce(&mut Ui)) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(width, ROW_H), if en { Sense::click() } else { Sense::hover() });
    ui.painter().rect_filled(r, CornerRadius::same(6), if en && resp.hovered() { t.hover } else { t.field });
    let text_r = Rect::from_min_max(pos2(r.min.x + 8.0, r.min.y), pos2(r.max.x - 22.0, r.max.y));
    ui.painter().with_clip_rect(text_r).text(
        pos2(text_r.min.x, r.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::font(12.5),
        if en { t.text } else { t.text_faint },
    );
    icons::paint_mono(ui.painter(), Rect::from_center_size(pos2(r.max.x - 12.0, r.center().y), vec2(9.0, 9.0)), Icon::ChevronDown, t.caption, !en);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, en, format!("{id}: {label}")));
    egui::Popup::menu(&resp).width(menu_w).show(|ui| {
        egui::ScrollArea::vertical().max_height(360.0).show(ui, add);
    });
    resp
}

/// Two half-width columns and the side column.
pub(crate) fn cols(ui: &Ui) -> f32 {
    ((ui.available_width() - SIDE - GAP * 2.0) / 2.0).max(60.0)
}

fn scheme(app: &SlideApp) -> deckcraft_color::ColorScheme {
    app.session.active().and_then(|d| d.doc.masters.first().map(|m| m.scheme())).unwrap_or_else(|| deckcraft_model::Theme::default().colors)
}

fn hex(c: deckcraft_color::Rgba) -> String {
    format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

/// `FFF`, `#FFFFFF` or `ffffff` as `#RRGGBB`.
pub fn parse_hex(s: &str) -> Option<String> {
    let h: String = s.trim().trim_start_matches('#').chars().collect();
    let full = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 => h,
        _ => return None,
    };
    full.chars().all(|c| c.is_ascii_hexdigit()).then(|| format!("#{}", full.to_uppercase()))
}

// ----- Header ---------------------------------------------------------------------------------

/// What the layer is, as the header names it.
pub fn kind_label(sh: &Shape) -> String {
    match &sh.kind {
        ShapeKind::Picture { .. } => "Image".into(),
        ShapeKind::Group { .. } => "Group".into(),
        ShapeKind::Table(_) => "Table".into(),
        ShapeKind::Chart(_) => "Chart".into(),
        ShapeKind::Connector { .. } => "Line".into(),
        ShapeKind::Media(_) => "Media".into(),
        ShapeKind::Shape if sh.text_box || sh.ph.is_some() => "Text".into(),
        ShapeKind::Shape => match &sh.geom {
            Geom::Preset { name, .. } => deckcraft_geom::preset::info(name).map(|p| p.label.to_string()).unwrap_or_else(|| "Shape".into()),
            Geom::Custom { .. } => "Shape".into(),
        },
        _ => "Layer".into(),
    }
}

fn header_row(app: &mut SlideApp, ui: &mut Ui, sh: &Shape, multi: bool, n: usize) {
    let t = Tokens::get(ui.ctx());
    let on_slide = app.session.active().is_some_and(|d| d.selection.target == deckcraft_engine::Target::Slides);
    let title = match sh.ph_type() {
        _ if multi => format!("{n} selected"),
        None => kind_label(sh),
        Some(k) => {
            let name = kind_name(k.xml(), false).trim_end_matches(" text").to_string();
            // On a slide a placeholder is named by what it is ("Title", "Footer"); in Slide
            // Master view, where boxes are made placeholders, it says so.
            if on_slide { name } else { format!("{name} placeholder") }
        }
    };
    let target = app.session.active().map(|d| d.selection.target);
    ui.horizontal(|ui| {
        ui.set_min_height(30.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let more_on = ui.ctx().data(|d| d.get_temp::<bool>(more_id())).unwrap_or(false);
            if icon_button(ui, Icon::More, if more_on { "Hide more options" } else { "More options: effects, size, text box" }, true).clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(more_id(), !more_on));
            }
            if multi {
                let r = icon_button(ui, Icon::MergeShapes, "Merge shapes", enabled(app, "shape.merge"));
                egui::Popup::menu(&r).show(|ui| {
                    for (op, label) in MERGE_OPS {
                        if ui.button(label).clicked() {
                            run(app, "shape.merge", json!({"op": op}));
                        }
                    }
                });
                if icon_button(ui, Icon::Group, "Group (⌘G)", enabled(app, "arrange.group")).clicked() {
                    run(app, "arrange.group", json!({}));
                }
            } else {
                if matches!(sh.kind, ShapeKind::Group { .. }) && icon_button(ui, Icon::Ungroup, "Ungroup (⇧⌘G)", true).clicked() {
                    run(app, "arrange.ungroup", json!({}));
                }
                let r = icon_button(ui, Icon::Link, "Link", enabled(app, "insert.hyperlink"));
                egui::Popup::menu(&r).width(260.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| link_field(app, ui));
                // A slide's placeholder: back to its layout's position and size.
                if target == Some(deckcraft_engine::Target::Slides) && sh.ph.is_some() {
                    let moved = sh.xfrm.is_some();
                    let tip = if moved { "Reset to layout" } else { "Reset to layout (it's where its layout has it)" };
                    if icon_button(ui, Icon::Revert, tip, moved).clicked() {
                        run(app, "placeholder.reset", json!({"ids": [sh.id]}));
                    }
                }
                // Boxes are made placeholders in Slide Master view; slides take their layout's.
                if target.is_some_and(|t| t != deckcraft_engine::Target::Slides) && matches!(sh.kind, ShapeKind::Shape) {
                    let r = icon_button(ui, Icon::Tag, "Placeholder: what this box is on the slide's layout", true);
                    egui::Popup::menu(&r).width(220.0).show(|ui| placeholder_menu(app, ui, sh));
                }
            }
            // The name takes the rest of the row, cut short with … when it's long.
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&title).font(theme::bold(14.0)).color(t.text)).truncate());
            });
        });
    });
}

/// The placeholders a box can become: on a slide, its layout's; on a layout or the master, the
/// kinds they may have. `(kind, name)`, one per name.
pub fn layout_places(app: &SlideApp) -> Vec<(String, String)> {
    let Some(st) = app.session.active() else { return vec![] };
    if st.selection.target != deckcraft_engine::Target::Slides {
        let master = matches!(st.selection.target, deckcraft_engine::Target::Master { .. });
        let mut out: Vec<(String, String)> = vec![];
        for k in deckcraft_engine::cmd::placeholder::template_kinds(master) {
            let name = template_kind_name(k.xml());
            if !out.iter().any(|(_, n)| *n == name) {
                out.push((k.xml().to_string(), name));
            }
        }
        return out;
    }
    let Some(slide) = st.doc.slides.get(st.selection.slide) else { return vec![] };
    let Some((_, Some(layout))) = st.doc.master_for(slide) else { return vec![] };
    let mut out: Vec<(String, String)> = vec![];
    for k in layout.shapes.iter().filter_map(Shape::ph_type) {
        let name = kind_name(k.xml(), false);
        if !out.iter().any(|(_, n)| *n == name) && !matches!(k, deckcraft_model::PhType::SlideImage) {
            out.push((k.xml().to_string(), name));
        }
    }
    out
}

/// A placeholder kind as the menu on a layout or master names it (content and centred titles
/// are kinds of their own there).
fn template_kind_name(xml: &str) -> String {
    match xml {
        "ctrTitle" => "Centered title".into(),
        "obj" => "Content".into(),
        "media" => "Media".into(),
        k => kind_name(k, false),
    }
}

/// Make the box one of its layout's placeholders, or a plain box again.
fn placeholder_menu(app: &mut SlideApp, ui: &mut Ui, sh: &Shape) {
    let on_slide = app.session.active().is_some_and(|d| d.selection.target == deckcraft_engine::Target::Slides);
    let name = |x: &str| if on_slide { kind_name(x, false) } else { template_kind_name(x) };
    let now = sh.ph_type().map(|k| name(k.xml()));
    let heading = match app.session.active().map(|d| d.selection.target) {
        Some(deckcraft_engine::Target::Master { .. }) => "Make this the master's",
        Some(deckcraft_engine::Target::Layout { .. }) => "Make this a placeholder",
        _ => "Make this the slide's",
    };
    ui.label(egui::RichText::new(heading).font(theme::bold(12.5)));
    let places = layout_places(app);
    if places.is_empty() {
        ui.label(egui::RichText::new("This layout has no placeholders").font(theme::font(12.0)).weak());
    }
    for (kind, name) in places {
        let on = now.as_deref() == Some(name.as_str());
        if ui.add(egui::Button::selectable(on, &name)).clicked() && !on {
            run(app, "placeholder.assign", json!({"id": sh.id, "kind": kind}));
            ui.close();
        }
    }
    ui.separator();
    let r = ui.add_enabled(sh.ph.is_some(), egui::Button::new("Not a placeholder"));
    if r.on_hover_text("A plain text box that looks the same, no longer following the layout").clicked() {
        run(app, "placeholder.assign", json!({"id": sh.id, "kind": "none"}));
        ui.close();
    }
}

/// `shape.merge` operations, as the Merge shapes menu lists them.
const MERGE_OPS: [(&str, &str); 5] =
    [("union", "Union"), ("combine", "Combine"), ("fragment", "Fragment"), ("intersect", "Intersect"), ("subtract", "Subtract")];

/// A URL or slide number, or a slide from the list.
fn link_field(app: &mut SlideApp, ui: &mut Ui) {
    let slides = app.session.active().map(|d| d.doc.slides.len()).unwrap_or(0);
    let id = ui.id().with("props-link");
    let mut url: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
    ui.label(egui::RichText::new("Link").font(theme::bold(12.5)));
    let r = ui.add(egui::TextEdit::singleline(&mut url).hint_text("Add URL or slide number").desired_width(240.0));
    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !url.trim().is_empty() {
        let v = url.trim().to_string();
        match v.parse::<usize>() {
            Ok(n) if (1..=slides).contains(&n) => run(app, "insert.hyperlink", json!({"slide": n - 1})),
            _ => run(app, "insert.hyperlink", json!({"url": v})),
        }
        url.clear();
        ui.close();
    }
    ui.data_mut(|d| d.insert_temp(id, url));
    egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
        for i in 0..slides {
            if ui.button(format!("Slide {}", i + 1)).clicked() {
                run(app, "insert.hyperlink", json!({"slide": i}));
                ui.close();
            }
        }
    });
}

// ----- Position -------------------------------------------------------------------------------

fn position(app: &mut SlideApp, ui: &mut Ui, x: &Xfrm, boxes: &[(ShapeId, Xfrm)]) {
    section(ui, "Position", false, &[]);
    let en = enabled(app, "arrange.align");
    let c = cols(ui);
    caption(ui, "Alignment");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let h = [
            (Icon::AlignObjLeft, "Align left", false),
            (Icon::AlignObjCenter, "Align horizontal centers", false),
            (Icon::AlignObjRight, "Align right", false),
        ];
        if let Some(k) = seg_group(ui, &h, c, en) {
            let edge = ["left", "center", "right"].get(k).copied().unwrap_or("left");
            run(app, "arrange.align", json!({"edge": edge}));
        }
        let v = [
            (Icon::AlignObjTop, "Align top", false),
            (Icon::AlignObjMiddle, "Align vertical centers", false),
            (Icon::AlignObjBottom, "Align bottom", false),
        ];
        if let Some(k) = seg_group(ui, &v, c, en) {
            let edge = ["top", "middle", "bottom"].get(k).copied().unwrap_or("top");
            run(app, "arrange.align", json!({"edge": edge}));
        }
        let r = icon_button(ui, Icon::Grid, "Distribute", en && boxes.len() > 2);
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("Distribute horizontal spacing").clicked() {
                run(app, "arrange.distribute", json!({"dir": "horizontal", "to": "selection"}));
            }
            if ui.button("Distribute vertical spacing").clicked() {
                run(app, "arrange.distribute", json!({"dir": "vertical", "to": "selection"}));
            }
        });
    });
    let u = units::length(app);
    let en = enabled(app, "shape.move");
    caption(ui, "Position");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let cx = common(boxes.iter().map(|b| two(u.show(b.1.x))));
        if let Some(v) = number_or_mixed(ui, "x", Prefix::Text("X"), cx, "", c, 1.0, ANY, en) {
            run(app, "shape.move", json!({"x": u.to_pt(v)}));
        }
        let cy = common(boxes.iter().map(|b| two(u.show(b.1.y))));
        if let Some(v) = number_or_mixed(ui, "y", Prefix::Text("Y"), cy, "", c, 1.0, ANY, en) {
            run(app, "shape.move", json!({"y": u.to_pt(v)}));
        }
    });
    caption(ui, "Rotation");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let rot = common(boxes.iter().map(|b| (b.1.rot * 10.0).round() / 10.0));
        if let Some(v) = number_or_mixed(ui, "rot", Prefix::Icon(Icon::Rotate), rot, "°", c, 1.0, (-360.0, 360.0), enabled(app, "shape.rotate")) {
            run(app, "shape.rotate", json!({"deg": v.rem_euclid(360.0)}));
        }
        let items =
            [(Icon::RotateRight, "Rotate 90° right", false), (Icon::FlipH, "Flip horizontal", x.flip_h), (Icon::FlipV, "Flip vertical", x.flip_v)];
        match seg_group(ui, &items, c, enabled(app, "arrange.flipHorizontal")) {
            Some(0) => run(app, "arrange.rotateRight", json!({})),
            Some(1) => run(app, "arrange.flipHorizontal", json!({})),
            Some(2) => run(app, "arrange.flipVertical", json!({})),
            _ => {}
        }
    });
}

/// A text box's insets in points (left, top, right, bottom), inherited and defaulted as drawn.
pub fn insets(app: &SlideApp, sh: &Shape) -> [f64; 4] {
    use deckcraft_model::resolve::Ctx;
    let b = app
        .session
        .active()
        .and_then(|st| {
            let doc = &st.doc;
            let ctx = match st.selection.target {
                deckcraft_engine::Target::Slides => doc.slides.get(st.selection.slide).and_then(|s| Ctx::for_slide(doc, s)),
                deckcraft_engine::Target::Master { master } => doc.masters.get(master).map(|m| Ctx::for_master(doc, m)),
                deckcraft_engine::Target::Layout { master, layout } => {
                    doc.masters.get(master).and_then(|m| m.layouts.get(layout).map(|l| Ctx::for_layout(doc, m, l)))
                }
                // Notes-size master pages: the default insets.
                deckcraft_engine::Target::Handout | deckcraft_engine::Target::NotesMaster => None,
            }?;
            Some(deckcraft_model::resolve::body(&ctx, sh))
        })
        .unwrap_or_default();
    [b.inset_l.unwrap_or(7.2), b.inset_t.unwrap_or(3.6), b.inset_r.unwrap_or(7.2), b.inset_b.unwrap_or(3.6)]
}

/// Padding: horizontal (left and right) and vertical (top and bottom), or each side.
fn padding(app: &mut SlideApp, ui: &mut Ui, sh: &Shape) {
    let u = units::small(app);
    let [l, t, r, b] = insets(app, sh).map(|v| two(u.show(v)));
    let c = cols(ui);
    let en = enabled(app, "format.margins");
    let sides_id = egui::Id::new("props-padding-sides");
    // Each side when asked for, or when the sides already differ.
    let sides = ui.ctx().data(|d| d.get_temp::<bool>(sides_id)).unwrap_or(l != r || t != b);
    caption(ui, "Padding");
    let set = |app: &mut SlideApp, keys: &[&str], v: f64| {
        let mut p = serde_json::Map::new();
        for k in keys {
            p.insert((*k).to_string(), json!(u.to_pt(v).max(0.0)));
        }
        run(app, "format.margins", Value::Object(p));
    };
    let toggle = |ui: &mut Ui| {
        let tk = Tokens::get(ui.ctx());
        let resp = icon_button(ui, Icon::PadSides, if sides { "Horizontal and vertical padding" } else { "Padding for each side" }, en);
        if sides {
            ui.painter().rect_stroke(resp.rect, CornerRadius::same(5), Stroke::new(1.0, tk.border), egui::StrokeKind::Inside);
        }
        if resp.clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(sides_id, !sides));
        }
    };
    if !sides {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            if let Some(v) = number_or_mixed(ui, "pad-h", Prefix::Icon(Icon::PadH), common([l, r]), "", c, 1.0, NOT_NEGATIVE, en) {
                set(app, &["left", "right"], v);
            }
            if let Some(v) = number_or_mixed(ui, "pad-v", Prefix::Icon(Icon::PadV), common([t, b]), "", c, 1.0, NOT_NEGATIVE, en) {
                set(app, &["top", "bottom"], v);
            }
            toggle(ui);
        });
        return;
    }
    for (row, pair) in
        [[("pad-l", "L", "left", l), ("pad-t", "T", "top", t)], [("pad-r", "R", "right", r), ("pad-b", "B", "bottom", b)]].into_iter().enumerate()
    {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (id, label, key, v) in pair {
                if let Some(nv) = number(ui, id, Prefix::Text(label), v, "", c, 1.0, NOT_NEGATIVE, en) {
                    set(app, &[key], nv);
                }
            }
            if row == 0 {
                toggle(ui);
            }
        });
    }
}

/// What a placeholder kind is called, plural for the panel's "every slide's" row.
pub fn kind_name(xml: &str, plural: bool) -> String {
    let (one, many) = match xml {
        "title" | "ctrTitle" => ("Title", "titles"),
        "subTitle" => ("Subtitle", "subtitles"),
        "body" | "obj" => ("Body text", "body text boxes"),
        "dt" => ("Date", "dates"),
        "ftr" => ("Footer", "footers"),
        "sldNum" => ("Slide number", "slide numbers"),
        "pic" => ("Picture", "pictures"),
        "tbl" => ("Table", "tables"),
        "chart" => ("Chart", "charts"),
        _ => ("Placeholder", "placeholders"),
    };
    if plural { many.into() } else { one.into() }
}

/// The gaps between neighbouring layers left to right (`horizontal`) or top to bottom, in points.
pub fn gaps(boxes: &[(ShapeId, Xfrm)], horizontal: bool) -> Vec<f64> {
    let sorted = sorted_boxes(boxes, horizontal);
    sorted
        .windows(2)
        .filter_map(|w| match w {
            [a, b] if horizontal => Some(b.1.x - (a.1.x + a.1.w)),
            [a, b] => Some(b.1.y - (a.1.y + a.1.h)),
            _ => None,
        })
        .collect()
}

fn sorted_boxes(boxes: &[(ShapeId, Xfrm)], horizontal: bool) -> Vec<(ShapeId, Xfrm)> {
    let mut v = boxes.to_vec();
    let key = |b: &(ShapeId, Xfrm)| if horizontal { b.1.x } else { b.1.y };
    v.sort_by(|a, b| key(a).total_cmp(&key(b)));
    v
}

/// Where each layer after the first goes for `gap` points between neighbours: `(id, x or y)`.
pub fn spaced(boxes: &[(ShapeId, Xfrm)], horizontal: bool, gap: f64) -> Vec<(ShapeId, f64)> {
    let sorted = sorted_boxes(boxes, horizontal);
    let Some(first) = sorted.first() else { return vec![] };
    let mut edge = if horizontal { first.1.x + first.1.w } else { first.1.y + first.1.h };
    sorted
        .iter()
        .skip(1)
        .map(|(id, x)| {
            let at = edge + gap;
            edge = at + if horizontal { x.w } else { x.h };
            (*id, at)
        })
        .collect()
}

// ----- Layout ---------------------------------------------------------------------------------

/// How a text box sizes itself, as the Resizing buttons show it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resizing {
    /// Grows sideways with its text (no wrapping, shape fits the text).
    AutoWidth,
    /// Wraps at its width and grows downwards.
    AutoHeight,
    /// Keeps its size.
    Fixed,
}

pub fn resizing_of(sh: &Shape) -> Resizing {
    let b = sh.text.as_ref().map(|t| &t.body);
    let fits = b.and_then(|b| b.autofit.as_ref()).is_some_and(|a| matches!(a, deckcraft_model::text::AutoFit::Shape));
    let wraps = b.and_then(|b| b.wrap).unwrap_or(true);
    match (fits, wraps) {
        (true, false) => Resizing::AutoWidth,
        (true, true) => Resizing::AutoHeight,
        _ => Resizing::Fixed,
    }
}

fn layout(app: &mut SlideApp, ui: &mut Ui, sh: &Shape, x: &Xfrm, boxes: &[(ShapeId, Xfrm)], texty: bool) {
    section(ui, "Layout", false, &[]);
    let c = cols(ui);
    if texty && sh.text.is_some() {
        caption(ui, "Resizing");
        let cur = resizing_of(sh);
        let items = [
            (Icon::DistributeH, "Auto width", cur == Resizing::AutoWidth),
            (Icon::DistributeV, "Auto height", cur == Resizing::AutoHeight),
            (Icon::TextBox, "Fixed size", cur == Resizing::Fixed),
        ];
        match seg_group(ui, &items, c * 2.0 + GAP, enabled(app, "format.autofit")) {
            Some(0) => app.run_as_one("Auto Width", vec![("format.wrap", json!({"on": false})), ("format.autofit", json!({"mode": "resize"}))]),
            Some(1) => app.run_as_one("Auto Height", vec![("format.wrap", json!({"on": true})), ("format.autofit", json!({"mode": "resize"}))]),
            Some(2) => run(app, "format.autofit", json!({"mode": "none"})),
            _ => {}
        }
    }
    caption(ui, "Dimensions");
    let u = units::length(app);
    let en = enabled(app, "shape.resize");
    // Pictures keep their proportions unless asked not to (as their corner handles do); other
    // layers don't. Each kind remembers its own setting.
    let picture = matches!(sh.kind, ShapeKind::Picture { .. }) && boxes.len() < 2;
    let lock_id = egui::Id::new(("props-constrain", picture));
    let lock = ui.ctx().data(|d| d.get_temp::<bool>(lock_id)).unwrap_or(picture);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let w = if boxes.len() > 1 { common(boxes.iter().map(|b| two(u.show(b.1.w)))) } else { Some(two(u.show(x.w))) };
        if let Some(v) = number_or_mixed(ui, "w", Prefix::Text("W"), w, "", c, 1.0, SIZE, en) {
            run(app, "shape.resize", json!({"w": u.to_pt(v).max(1.0), "lockAspect": lock}));
        }
        let h = if boxes.len() > 1 { common(boxes.iter().map(|b| two(u.show(b.1.h)))) } else { Some(two(u.show(x.h))) };
        if let Some(v) = number_or_mixed(ui, "h", Prefix::Text("H"), h, "", c, 1.0, SIZE, en) {
            run(app, "shape.resize", json!({"h": u.to_pt(v).max(1.0), "lockAspect": lock}));
        }
        let t = Tokens::get(ui.ctx());
        let r = icon_button(ui, if lock { Icon::Lock } else { Icon::Unlock }, "Constrain proportions", en);
        if lock {
            ui.painter().rect_stroke(r.rect, CornerRadius::same(5), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        }
        if r.clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(lock_id, !lock));
        }
    });
    if texty && sh.text.is_some() {
        padding(app, ui, sh);
    }
    if boxes.len() > 1 {
        caption(ui, "Spacing");
        let en = enabled(app, "shape.move");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (horizontal, id, icon, label) in
                [(true, "gap-x", Icon::DistributeH, "Horizontal Spacing"), (false, "gap-y", Icon::DistributeV, "Vertical Spacing")]
            {
                let cur = common(gaps(boxes, horizontal).into_iter().map(|g| two(u.show(g))));
                if let Some(v) = number_or_mixed(ui, id, Prefix::Icon(icon), cur, "", c, 1.0, ANY, en) {
                    let key = if horizontal { "x" } else { "y" };
                    let moves = spaced(boxes, horizontal, u.to_pt(v));
                    app.run_as_one(label, moves.into_iter().map(|(id, at)| ("shape.move", json!({"ids": [id], key: at}))).collect());
                }
            }
        });
    }
}

// ----- Appearance -----------------------------------------------------------------------------

/// The selection's opacity, 0–1: a picture's, or a solid fill's colour alpha.
pub fn opacity_of(sh: &Shape, sc: &deckcraft_color::ColorScheme) -> f64 {
    match (&sh.kind, &sh.fill) {
        (ShapeKind::Picture { fill }, _) => fill.alpha.unwrap_or(1.0),
        (_, Some(Fill::Solid { color })) => color.resolve(sc, None).a as f64 / 255.0,
        _ => 1.0,
    }
}

/// The corner radius of a rounded rectangle in points (None for other shapes).
pub fn corner_radius(sh: &Shape, x: &Xfrm) -> Option<f64> {
    let Geom::Preset { name, adj } = &sh.geom else { return None };
    (name == "roundRect").then(|| adj.first().copied().unwrap_or(16667.0) / 100_000.0 * x.w.min(x.h))
}

/// A rectangle's corner radii in points, clockwise from the top left, when PPTX can round it:
/// a rectangle (square corners) or one of the rounded rectangles (`roundRect`, `round1Rect`,
/// `round2SameRect`, `round2DiagRect`). Text boxes and placeholders have none.
pub fn corners(sh: &Shape, x: &Xfrm) -> Option<[f64; 4]> {
    if !matches!(sh.kind, ShapeKind::Shape) || sh.text_box || sh.ph.is_some() {
        return None;
    }
    let Geom::Preset { name, adj } = &sh.geom else { return None };
    let min = x.w.min(x.h);
    let a = |i: usize, default: f64| adj.get(i).copied().unwrap_or(default).clamp(0.0, 50_000.0) / 100_000.0 * min;
    Some(match name.as_str() {
        "rect" => [0.0; 4],
        "roundRect" => [a(0, 16_667.0); 4],
        "round1Rect" => [0.0, a(0, 16_667.0), 0.0, 0.0],
        "round2SameRect" => {
            let (top, bottom) = (a(0, 16_667.0), a(1, 0.0));
            [top, top, bottom, bottom]
        }
        "round2DiagRect" => {
            let (p, q) = (a(0, 16_667.0), a(1, 0.0));
            [p, q, p, q]
        }
        _ => return None,
    })
}

/// The preset and adjustments (file units) that round a rectangle's corners as `r` (points,
/// clockwise from the top left) when its shorter side is `min`, if PPTX has one: all four equal,
/// the top pair and the bottom pair, the diagonals, or the top-right corner alone.
pub fn corner_preset(r: [f64; 4], min: f64) -> Option<(&'static str, Vec<f64>)> {
    let adj = |v: f64| if min > 0.0 { (v / min * 100_000.0).clamp(0.0, 50_000.0).round() } else { 0.0 };
    let eq = |a: f64, b: f64| (a - b).abs() < 0.01;
    let [tl, tr, br, bl] = r;
    if r.iter().all(|v| eq(*v, 0.0)) {
        Some(("rect", vec![]))
    } else if eq(tl, tr) && eq(tr, br) && eq(br, bl) {
        Some(("roundRect", vec![adj(tl)]))
    } else if eq(tl, tr) && eq(br, bl) {
        Some(("round2SameRect", vec![adj(tl), adj(br)]))
    } else if eq(tl, br) && eq(tr, bl) {
        Some(("round2DiagRect", vec![adj(tl), adj(tr)]))
    } else if eq(tl, 0.0) && eq(br, 0.0) && eq(bl, 0.0) {
        Some(("round1Rect", vec![adj(tr)]))
    } else {
        None
    }
}

/// The commands that round shape `id` (preset `now`) as `r`, or why PPTX can't.
fn corner_commands(id: ShapeId, now: &str, r: [f64; 4], min: f64) -> Result<Vec<(&'static str, Value)>, String> {
    let (preset, adj) = corner_preset(r, min).ok_or_else(|| {
        "PowerPoint can't round those corners: make all four equal, the top and bottom pairs, the diagonal pairs, or only the top right".to_string()
    })?;
    let mut cmds = vec![];
    if preset != now {
        cmds.push(("shape.change", json!({"preset": preset, "ids": [id]})));
    }
    for (i, v) in adj.into_iter().enumerate() {
        cmds.push(("shape.adjust", json!({"index": i, "value": v, "ids": [id]})));
    }
    Ok(cmds)
}

fn appearance(app: &mut SlideApp, ui: &mut Ui, shapes: &[Shape], boxes: &[(ShapeId, Xfrm)]) {
    // Any shown layer: the eye hides them all.
    let visible = shapes.iter().any(|s| !s.hidden);
    let clicked =
        section(ui, "Appearance", false, &[(if visible { Icon::Eye } else { Icon::EyeOff }, if visible { "Hide layer" } else { "Show layer" })]);
    if clicked == Some(0) {
        run(app, "shape.visible", json!({"visible": !visible}));
    }
    let c = cols(ui);
    let sc = scheme(app);
    let picture = |sh: &Shape| matches!(sh.kind, ShapeKind::Picture { .. });
    let has_opacity = |sh: &Shape| picture(sh) || matches!(sh.fill, Some(Fill::Solid { .. } | Fill::Gradient(_) | Fill::Picture(_)));
    let pictures: Vec<ShapeId> = shapes.iter().filter(|s| picture(s)).map(|s| s.id).collect();
    let filled: Vec<ShapeId> = shapes.iter().filter(|s| !picture(s) && has_opacity(s)).map(|s| s.id).collect();
    // Rectangles, with their preset, corner radii in points and shorter side.
    let radii: Vec<(ShapeId, String, [f64; 4], f64)> = shapes
        .iter()
        .zip(boxes)
        .filter_map(|(sh, (_, x))| {
            let name = match &sh.geom {
                Geom::Preset { name, .. } => name.clone(),
                Geom::Custom { .. } => return None,
            };
            corners(sh, x).map(|r| (sh.id, name, r, x.w.min(x.h)))
        })
        .collect();
    let u = units::small(app);
    // The largest radius that fits: half the shorter side (of the smallest, for several).
    let max_radius = radii.iter().map(|r| two(u.show(r.3 / 2.0))).fold(f64::INFINITY, f64::min).min(1e6);
    let each_id = egui::Id::new("props-corners-each");
    // Each corner on its own when asked for, or when one shape's corners already differ.
    let uneven = radii.len() == 1 && radii.first().is_some_and(|r| common(r.2).is_none());
    let each = ui.ctx().data(|d| d.get_temp::<bool>(each_id)).unwrap_or(uneven) && radii.len() == 1;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        ui.vertical(|ui| {
            ui.set_width(c);
            caption(ui, "Opacity");
            let cur = common(shapes.iter().filter(|s| has_opacity(s)).map(|s| (opacity_of(s, &sc) * 100.0).round()));
            let en = !pictures.is_empty() || !filled.is_empty();
            if let Some(v) =
                number_or_mixed(ui, "opacity", Prefix::Icon(Icon::Transparency), if en { cur } else { Some(100.0) }, "%", c, 1.0, PERCENT, en)
            {
                let tr = 1.0 - v.clamp(0.0, 100.0) / 100.0;
                let mut cmds = vec![];
                if !pictures.is_empty() {
                    cmds.push(("picture.adjust", json!({"transparency": tr, "ids": pictures})));
                }
                if !filled.is_empty() {
                    cmds.push(("shape.fill", json!({"transparency": tr, "ids": filled})));
                }
                app.run_as_one("Opacity", cmds);
            }
        });
        ui.vertical(|ui| {
            caption(ui, "Corner radius");
            let shown = if radii.is_empty() { Some(0.0) } else { common(radii.iter().flat_map(|r| r.2).map(|v| two(u.show(v)))) };
            if let Some(v) =
                number_or_mixed(ui, "radius", Prefix::Icon(Icon::FitSmall), shown, "", c, 1.0, (0.0, max_radius), !radii.is_empty() && !each)
            {
                let r = u.to_pt(v).max(0.0);
                let mut cmds = vec![];
                for (id, now, _, min) in &radii {
                    cmds.extend(corner_commands(*id, now, [r; 4], *min).unwrap_or_default());
                }
                app.run_as_one("Corner Radius", cmds);
            }
        });
        if radii.len() == 1 {
            let tk = Tokens::get(ui.ctx());
            let resp = icon_button(ui, Icon::Corners, if each { "One radius for every corner" } else { "Each corner on its own" }, true);
            if each {
                ui.painter().rect_stroke(resp.rect, CornerRadius::same(5), Stroke::new(1.0, tk.border), egui::StrokeKind::Inside);
            }
            if resp.clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(each_id, !each));
            }
        }
    });
    // Each corner: top left, top right / bottom left, bottom right, as in canvas editors.
    if let (true, Some((id, now, r, min))) = (each, radii.first()) {
        let mut set = None;
        for row in [
            [(0, Icon::CornerTl, "Top left"), (1, Icon::CornerTr, "Top right")],
            [(3, Icon::CornerBl, "Bottom left"), (2, Icon::CornerBr, "Bottom right")],
        ] {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                for (k, icon, label) in row {
                    let v = r.get(k).copied().unwrap_or(0.0);
                    if let Some(nv) =
                        number(ui, &format!("corner-{k}"), Prefix::Icon(icon), two(u.show(v)), "", c, 1.0, (0.0, two(u.show(min / 2.0))), true)
                    {
                        let mut nr = *r;
                        if let Some(slot) = nr.get_mut(k) {
                            *slot = u.to_pt(nv).max(0.0);
                        }
                        set = Some((nr, label));
                    }
                }
            });
        }
        if let Some((nr, _)) = set {
            match corner_commands(*id, now, nr, *min) {
                Ok(cmds) => app.run_as_one("Corner Radius", cmds),
                Err(e) => app.set_status(e),
            }
        }
    }
}

// ----- Typography -----------------------------------------------------------------------------

fn typography(app: &mut SlideApp, ui: &mut Ui, sh: &Shape) {
    let st = fmt_state(app);
    let en = enabled(app, "format.bold");
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let styles = crate::textstyles::styles(&doc);
    // Text with a style shows the style; text without one, the type controls and a styles button.
    let shown = crate::textstyles::shown(app, &styles, &st);
    let actions: &[(Icon, &str)] = if shown.is_none() { &[(Icon::Styles, "Text styles")] } else { &[] };
    let buttons = section_buttons(ui, "Typography", false, actions);
    if let Some(b) = buttons.first() {
        crate::inspector::style_menu(app, b, "", 240.0);
    }
    match &shown {
        Some(name) => style_chip(app, ui, &st, en, name, &styles),
        None => {
            let w = cols(ui) * 2.0 + GAP;
            ui.scope(|ui| {
                ui.set_max_width(w);
                type_fields(app, ui, &st, en);
            });
        }
    }
    let extras_id = egui::Id::new("props-type-extras");
    let extras = ui.ctx().data(|d| d.get_temp::<bool>(extras_id)).unwrap_or(false);
    caption(ui, "Alignment");
    let c = cols(ui);
    let al = st.get("align").and_then(Value::as_str).unwrap_or("l").to_string();
    let anchor = sh.text.as_ref().and_then(|t| t.body.anchor).map(|a| format!("{a:?}").to_lowercase());
    let anchor = anchor.as_deref().unwrap_or("t");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let h = [
            (Icon::AlignLeft, "Align text left", al == "l"),
            (Icon::AlignCenter, "Center text", al == "ctr"),
            (Icon::AlignRight, "Align text right", al == "r"),
        ];
        if let Some(k) = seg_group(ui, &h, c, en) {
            let id = ["format.alignLeft", "format.alignCenter", "format.alignRight"].get(k).copied().unwrap_or("format.alignLeft");
            crate::textstyles::run_on_text(app, id, vec![(id, json!({}))]);
        }
        let v = [
            (Icon::TextTop, "Text at top", anchor.starts_with('t')),
            (Icon::TextMiddle, "Text in the middle", anchor.starts_with("ctr") || anchor.starts_with('m')),
            (Icon::TextBottom, "Text at bottom", anchor.starts_with('b')),
        ];
        if let Some(k) = seg_group(ui, &v, c, en) {
            let anchor = ["top", "middle", "bottom"].get(k).copied().unwrap_or("top");
            run(app, "format.anchor", json!({"anchor": anchor}));
        }
        let t = Tokens::get(ui.ctx());
        let tip = if extras { "Hide type details" } else { "Type details: bold, italic, underline, lists" };
        let r = icon_button(ui, Icon::Sliders, tip, true);
        if extras {
            ui.painter().rect_stroke(r.rect, CornerRadius::same(5), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        }
        if r.clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(extras_id, !extras));
        }
    });
    if extras {
        ui.add_space(6.0);
        crate::inspector::type_extras(app, ui, &st, en, false);
    }
    // A text box's colour is its Fill; a shape's text has its own row here.
    if !text_color_is_fill(sh) {
        caption(ui, "Color");
        let c = text_color(app, sh);
        let (picked, _) = color_row(app, ui, "Text", Some(c), None, None, cols(ui) * 2.0 + GAP);
        if let Some(col) = picked {
            crate::textstyles::run_on_text(app, "Font Color", vec![("format.color", json!({"color": col}))]);
        }
    }
}

/// Font, weight, size, line height and letter spacing in the panel's grey fields.
fn type_fields(app: &mut SlideApp, ui: &mut Ui, st: &Value, en: bool) {
    let font = st.get("font").and_then(Value::as_str).unwrap_or("").to_string();
    let bold = st.get("bold").and_then(Value::as_bool).unwrap_or(false);
    let (base, styles, current) = crate::inspector::weights(&font, bold);
    // The caller limits the width to the two field columns.
    let c = ((ui.available_width() - GAP) / 2.0).max(60.0);
    let families = crate::ribbon::font_families(app);
    pick(ui, "Font", if base.is_empty() { "Font" } else { &base }, c * 2.0 + GAP, en, |ui| {
        for f in families {
            if ui.selectable_label(f == base, &f).clicked() {
                crate::textstyles::run_on_text(app, "format.font", vec![("format.font", json!({"family": f}))]);
                ui.close();
            }
        }
    });
    ui.add_space(GAP);
    let u = units::small(app);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        pick(ui, "Weight", &current, c, en, |ui| {
            for s in &styles {
                if ui.selectable_label(*s == current, s).clicked() {
                    crate::textstyles::run_on_text(app, "Weight", crate::inspector::weight_commands(&base, s));
                    ui.close();
                }
            }
        });
        let size = st.get("size").and_then(Value::as_f64).unwrap_or(18.0);
        if let Some(v) = number(ui, "type-size", Prefix::None, two(u.show(size)), "", c, 1.0, (1.0, u.show(4000.0)), en) {
            crate::textstyles::run_on_text(app, "format.size", vec![("format.size", json!({"size": u.to_pt(v).clamp(1.0, 4000.0)}))]);
        }
    });
    ui.add_space(GAP);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let pct = match st.get("lineSpacing") {
            Some(v) if v.get("kind").and_then(Value::as_str) == Some("pct") => v.get("v").and_then(Value::as_f64).unwrap_or(1.0) * 100.0,
            _ => 100.0,
        };
        let lh = ui.scope(|ui| number(ui, "type-lh", Prefix::Icon(Icon::LineSpacing), pct.round(), "%", c, 5.0, (10.0, 999.0), en));
        if let Some(v) = lh.inner {
            crate::textstyles::run_on_text(app, "format.lineSpacing", vec![("format.lineSpacing", json!({"lines": (v / 100.0).clamp(0.1, 9.99)}))]);
        }
        lh.response.on_hover_text("Line height");
        let sp = st.get("spacing").and_then(Value::as_f64).unwrap_or(0.0);
        let ls = ui.scope(|ui| number(ui, "type-ls", Prefix::Icon(Icon::CharSpacing), two(sp), "", c, 0.1, (-100.0, 1584.0), en));
        if let Some(v) = ls.inner {
            crate::textstyles::run_on_text(app, "format.spacing", vec![("format.spacing", json!({"pt": v}))]);
        }
        ls.response.on_hover_text("Letter spacing (points)");
    });
}

/// The text style chip: "Ag Title · 120/108", which opens the style menu, with a button at its
/// right end that detaches the text from the style.
fn style_chip(app: &mut SlideApp, ui: &mut Ui, st: &Value, en: bool, current: &str, styles: &[crate::textstyles::TextStyle]) {
    let t = Tokens::get(ui.ctx());
    let u = units::small(app);
    let size = st.get("size").and_then(Value::as_f64).unwrap_or(18.0);
    let lines = match st.get("lineSpacing") {
        Some(v) if v.get("kind").and_then(Value::as_str) == Some("pct") => v.get("v").and_then(Value::as_f64).unwrap_or(1.0),
        _ => 1.2,
    };
    let metrics = format!("{}/{}", widgets::fmt_num(two(u.show(size)), ""), widgets::fmt_num((u.show(size) * lines).round(), ""));
    let w = cols(ui) * 2.0 + GAP;
    let (r, _) = ui.allocate_exact_size(vec2(w, 34.0), Sense::hover());
    let detach_r = Rect::from_center_size(pos2(r.max.x - 18.0, r.center().y), vec2(24.0, 24.0));
    let body = Rect::from_min_max(r.min, pos2(detach_r.min.x - 2.0, r.max.y));
    let resp = ui.interact(body, ui.id().with("style-chip"), if en { Sense::click() } else { Sense::hover() });
    let hovered = resp.hovered() || ui.rect_contains_pointer(r);
    ui.painter().rect(
        r,
        CornerRadius::same(6),
        if resp.hovered() { t.field } else { t.card },
        Stroke::new(1.0, t.card_border),
        egui::StrokeKind::Inside,
    );
    let ag = ui.painter().text(pos2(r.min.x + 8.0, r.center().y), Align2::LEFT_CENTER, "Ag", theme::bold(15.0), t.text);
    let name = ui.painter().text(pos2(ag.max.x + 6.0, r.center().y), Align2::LEFT_CENTER, current, theme::font(12.5), t.text);
    ui.painter().text(pos2(name.max.x + 3.0, r.center().y), Align2::LEFT_CENTER, format!("· {metrics}"), theme::font(12.5), t.caption);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, en, format!("Text style: {current}")));
    crate::inspector::style_menu(app, &resp, current, w.max(220.0));
    // Detach: shown while the pointer is over the chip, as in canvas editors.
    let dresp = ui.interact(detach_r, ui.id().with("style-detach"), if en { Sense::click() } else { Sense::hover() });
    dresp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, en, "Detach style"));
    if hovered || dresp.hovered() {
        if dresp.hovered() {
            ui.painter().rect_filled(detach_r, CornerRadius::same(5), t.pressed);
        }
        icons::paint_mono(ui.painter(), detach_r.shrink(4.0), Icon::Detach, t.text, !en);
    }
    let base = current.strip_suffix(" (copy)").unwrap_or(current);
    if dresp.on_hover_text("Detach style").clicked()
        && let Some(style) = styles.iter().find(|s| s.name == base)
    {
        crate::textstyles::detach(app, style);
    }
}

// ----- Fill -----------------------------------------------------------------------------------

/// Text boxes and placeholders: their Fill is the text's colour, as in canvas editors.
fn text_color_is_fill(sh: &Shape) -> bool {
    matches!(sh.kind, ShapeKind::Shape)
        && (sh.text_box || sh.ph.is_some())
        && !matches!(sh.fill, Some(Fill::Solid { .. } | Fill::Gradient(_) | Fill::Picture(_)))
}

/// The colour of a shape's first run of text: its own, else its text style's, else black.
fn text_color(app: &SlideApp, sh: &Shape) -> deckcraft_color::Rgba {
    let sc = scheme(app);
    let own = sh.text.as_ref().and_then(|t| t.paragraphs.iter().flat_map(|p| p.runs.iter()).find_map(|r| r.props.fill.clone()));
    let styled = || {
        let doc = &app.session.active()?.doc;
        let group = crate::textstyles::group_of(sh)?;
        crate::textstyles::styles(doc).into_iter().find(|s| s.group == group).and_then(|s| s.run.fill)
    };
    match own.or_else(styled) {
        Some(Fill::Solid { color }) => color.resolve(&sc, None),
        _ => deckcraft_color::Rgba::BLACK,
    }
}

/// A colour row: swatch (opens the colour grid), hex field, and an optional opacity field.
/// Returns a picked colour (`#RRGGBB` or a theme colour) and/or a new opacity percentage.
pub(crate) fn color_row(
    app: &SlideApp,
    ui: &mut Ui,
    id: &str,
    rgba: Option<deckcraft_color::Rgba>,
    label: Option<&str>,
    opacity: Option<f64>,
    width: f32,
) -> (Option<Value>, Option<f64>) {
    let t = Tokens::get(ui.ctx());
    let sc = scheme(app);
    let op_w = if opacity.is_some() { 64.0 } else { 0.0 };
    let (r, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(6), t.field);
    let sw = Rect::from_center_size(pos2(r.min.x + 15.0, r.center().y), vec2(14.0, 14.0));
    let sresp = ui.interact(sw.expand(3.0), ui.id().with((id, "swatch")), Sense::click());
    match rgba {
        Some(c) => {
            ui.painter().rect(
                sw,
                CornerRadius::same(2),
                theme::to_color32(deckcraft_color::Rgba { a: 255, ..c }),
                Stroke::new(1.0, egui::Color32::from_black_alpha(25)),
                egui::StrokeKind::Inside,
            );
        }
        None => {
            ui.painter().rect_stroke(sw, CornerRadius::same(2), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        }
    }
    sresp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{id} color")));
    let mut picked = None;
    widgets::color_popup(&sresp).show(|ui| {
        if let Some(Some(c)) = widgets::color_grid(ui, &sc, None) {
            picked = Some(cref_param(&c));
        }
    });
    let hex_rect = Rect::from_min_max(pos2(sw.max.x + 8.0, r.min.y), pos2(r.max.x - op_w, r.max.y)).shrink2(vec2(0.0, 6.0));
    if let Some(l) = label {
        ui.painter().text(pos2(hex_rect.min.x, r.center().y), Align2::LEFT_CENTER, l, theme::font(12.5), t.text);
    } else if let Some(c) = rgba {
        let tkey = ui.id().with((id, "hex"));
        let mut text: String = ui.data_mut(|d| d.get_temp(tkey)).unwrap_or_else(|| hex(c));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(hex_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let te = child.add_sized(
            hex_rect.size(),
            egui::TextEdit::singleline(&mut text)
                .frame(egui::Frame::NONE)
                .font(theme::font(12.5))
                .text_color(t.text)
                .id(ui.id().with((id, "hexedit"))),
        );
        if te.has_focus() {
            ui.data_mut(|d| d.insert_temp(tkey, text.clone()));
        } else {
            ui.data_mut(|d| d.remove::<String>(tkey));
        }
        if te.lost_focus()
            && let Some(h) = parse_hex(&text)
            && h[1..] != hex(c)
        {
            picked = Some(json!(h));
        }
    }
    let mut new_op = None;
    if let Some(op) = opacity {
        let orect = Rect::from_min_max(pos2(r.max.x - op_w, r.min.y), r.max);
        ui.painter().vline(orect.min.x, orect.y_range(), Stroke::new(1.0, t.card));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(orect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        new_op = number(&mut child, &format!("{id}-op"), Prefix::None, op.round(), " %", orect.width(), 1.0, PERCENT, true);
    }
    (picked, new_op)
}

fn fill(app: &mut SlideApp, ui: &mut Ui, sh: &Shape) {
    let sc = scheme(app);
    let width = cols(ui) * 2.0 + GAP;
    if text_color_is_fill(sh) {
        section(ui, "Fill", false, &[]);
        let c = text_color(app, sh);
        let (picked, _) = color_row(app, ui, "Fill", Some(c), None, None, width);
        if let Some(col) = picked {
            crate::textstyles::run_on_text(app, "Font Color", vec![("format.color", json!({"color": col}))]);
        }
        return;
    }
    let none = matches!(sh.fill, Some(Fill::None));
    let mut actions: Vec<(Icon, &str)> = vec![];
    if none {
        actions.push((Icon::Plus, "Add fill"));
    }
    if section(ui, "Fill", none, &actions) == Some(0) && none {
        run(app, "shape.fill", json!({"color": "accent1"}));
        return;
    }
    if none {
        return;
    }
    let opacity = Some(opacity_of(sh, &sc) * 100.0);
    let (rgba, label) = match &sh.fill {
        Some(Fill::Solid { color }) => (Some(color.resolve(&sc, None)), None),
        Some(Fill::Gradient(_)) => (None, Some("Gradient")),
        Some(Fill::Picture(_)) => (None, Some("Image")),
        Some(Fill::Pattern(_)) => (None, Some("Pattern")),
        _ => (None, Some("Automatic")),
    };
    // As in canvas editors: the colour field, then the row's buttons (fill type, remove).
    let field_w = (ui.available_width() - 2.0 * 26.0 - GAP - 4.0).max(120.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let (picked, op) = color_row(app, ui, "Fill", rgba, label, if label.is_none() { opacity } else { None }, field_w);
        if let Some(col) = picked {
            run(app, "shape.fill", json!({"color": col}));
        }
        if let Some(v) = op {
            run(app, "shape.fill", json!({"transparency": 1.0 - v.clamp(0.0, 100.0) / 100.0}));
        }
        ui.spacing_mut().item_spacing.x = 4.0;
        let r = icon_button(ui, Icon::Styles, "Fill type: solid, gradient, image", true);
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("Solid").clicked() {
                run(app, "shape.fill", json!({"color": "accent1"}));
            }
            if ui.button("Gradient").clicked() {
                let g = crate::fillui::default_gradient(deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Accent1), 90.0);
                run(app, "shape.fill", json!({"gradient": crate::fillui::gradient_json(&g)}));
            }
            if ui.button("Image…").clicked()
                && let Some(pick) = app.services.pick_open.as_mut()
                && let Some(path) = pick("picture")
                && let Some(Ok(bytes)) = app.services.read.as_ref().map(|r| r(&path))
            {
                run(app, "shape.fill", json!({"picture": deckcraft_engine::cmd::base64_encode(&bytes)}));
            }
        });
        if icon_button(ui, Icon::Minus, "Remove fill", true).clicked() {
            run(app, "shape.fill", json!({"none": true}));
        }
    });
    if let Some(Fill::Gradient(g)) = &sh.fill {
        ui.add_space(4.0);
        if let Some(ng) = crate::fillui::gradient_editor(ui, "props-grad", g, &sc) {
            run(app, "shape.fill", json!({"gradient": crate::fillui::gradient_json(&ng)}));
        }
    }
}

// ----- Stroke ---------------------------------------------------------------------------------

fn stroke(app: &mut SlideApp, ui: &mut Ui, sh: &Shape) {
    let sc = scheme(app);
    let line = sh.line.as_ref();
    let none = match line.and_then(|l| l.fill.as_ref()) {
        Some(Fill::None) => true,
        // Text boxes draw no outline unless they're given one.
        None => line.is_none() && (sh.text_box || sh.ph.is_some()),
        _ => false,
    };
    let actions: Vec<(Icon, &str)> = if none { vec![(Icon::Plus, "Add stroke")] } else { vec![] };
    if section(ui, "Stroke", none, &actions) == Some(0) && none {
        run(app, "shape.line", json!({"color": "tx1", "width": 1}));
        return;
    }
    if none {
        return;
    }
    let (rgba, label) = match line.and_then(|l| l.fill.as_ref()) {
        Some(Fill::Solid { color }) => (Some(color.resolve(&sc, None)), None),
        _ => (None, Some("Automatic")),
    };
    let c = cols(ui);
    let field_w = (ui.available_width() - 2.0 * 26.0 - GAP - 4.0).max(120.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let (picked, _) = color_row(app, ui, "Stroke", rgba, label, None, field_w);
        if let Some(col) = picked {
            run(app, "shape.line", json!({"color": col}));
        }
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(26.0);
        if icon_button(ui, Icon::Minus, "Remove stroke", true).clicked() {
            run(app, "shape.line", json!({"none": true}));
        }
    });
    ui.add_space(GAP);
    let u = units::small(app);
    let w = line.and_then(|l| l.width).unwrap_or(0.75);
    let r = ui.scope(|ui| number(ui, "stroke-w", Prefix::Icon(Icon::ShapeOutline), two(u.show(w)), "", c, 0.25, (0.0, u.show(1584.0)), true));
    if let Some(v) = r.inner {
        run(app, "shape.line", json!({"width": u.to_pt(v).max(0.0)}));
    }
    r.response.on_hover_text("Stroke weight");
}

// ----- Effects --------------------------------------------------------------------------------

/// The effects a shape has, as (label, the `shape.effects` params that remove it).
pub fn effect_rows(sh: &Shape) -> Vec<(&'static str, Value)> {
    let Some(e) = &sh.effects else { return vec![] };
    let mut v = vec![];
    if e.outer_shadow.is_some() {
        v.push(("Drop shadow", json!({"shadow": "none"})));
    }
    if e.inner_shadow.is_some() {
        v.push(("Inner shadow", json!({"shadow": "none"})));
    }
    if e.glow.is_some() {
        v.push(("Glow", json!({"glow": null})));
    }
    if e.soft_edge.is_some() {
        v.push(("Soft edges", json!({"softEdges": 0})));
    }
    if e.reflection.is_some() {
        v.push(("Reflection", json!({"reflection": "none"})));
    }
    v
}

fn effects(app: &mut SlideApp, ui: &mut Ui, sh: &Shape) {
    let t = Tokens::get(ui.ctx());
    let rows = effect_rows(sh);
    let en = enabled(app, "shape.effects");
    // The + is a menu, drawn here so it can anchor a popup.
    ui.add_space(6.0);
    ui.painter().hline(ui.max_rect().x_range().expand(12.0), ui.cursor().min.y, Stroke::new(1.0, t.separator));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.set_min_height(26.0);
        ui.label(egui::RichText::new("Effects").font(theme::bold(13.0)).color(if rows.is_empty() { t.caption } else { t.text }));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = icon_button(ui, Icon::Plus, "Add effect", en);
            egui::Popup::menu(&r).show(|ui| {
                for (l, p) in [
                    ("Drop shadow", json!({"shadow": "outer"})),
                    ("Inner shadow", json!({"shadow": "inner"})),
                    ("Glow", json!({"glow": {"radius": 8}})),
                    ("Soft edges", json!({"softEdges": 5})),
                    ("Reflection", json!({"reflection": "half"})),
                ] {
                    if ui.button(l).clicked() {
                        run(app, "shape.effects", p);
                    }
                }
            });
        });
    });
    let width = cols(ui) * 2.0 + GAP;
    for (label, remove) in rows {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            let (r, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
            ui.painter().rect_filled(r, CornerRadius::same(6), t.hover);
            ui.painter().text(pos2(r.min.x + 10.0, r.center().y), Align2::LEFT_CENTER, label, theme::font(13.0), t.text);
            if icon_button(ui, Icon::Minus, &format!("Remove {}", label.to_lowercase()), en).clicked() {
                run(app, "shape.effects", remove);
            }
        });
    }
}

// ----- Export ---------------------------------------------------------------------------------

fn export(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(6.0);
    ui.painter().hline(ui.max_rect().x_range().expand(12.0), ui.cursor().min.y, Stroke::new(1.0, t.separator));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.set_min_height(26.0);
        ui.label(egui::RichText::new("Export").font(theme::bold(13.0)).color(t.caption));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = icon_button(ui, Icon::Plus, "Export", true);
            egui::Popup::menu(&r).show(|ui| {
                for (l, f) in [("This slide as PNG…", "png"), ("This slide as JPEG…", "jpeg"), ("Presentation as PDF…", "pdf")] {
                    if ui.button(l).clicked() {
                        app.export_as(f, json!({}));
                    }
                }
            });
        });
    });
}

#[cfg(test)]
#[path = "props_tests.rs"]
mod tests;
