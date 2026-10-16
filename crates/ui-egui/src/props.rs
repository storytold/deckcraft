//! The canvas shell's property editor for a selection, laid out like a canvas editor's design
//! panel: a header naming the layer, then Position, Layout, Appearance, Typography, Fill, Stroke,
//! Effects and Export. Numbers sit in grey fields with a prefix (drag the prefix to scrub) and
//! are shown in pixels without a unit. Every control runs an existing command.

use deckcraft_model::{Fill, Geom, Shape, ShapeKind};
use egui::{Align2, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::icons::{self, Icon};
use crate::ribbon::{cref_param, enabled, fmt_state};
use crate::theme::{self, Tokens};
use crate::units;
use crate::{SlideApp, widgets};

const ROW_H: f32 = 30.0;
const GAP: f32 = 8.0;
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
    let multi = shapes.len() > 1;
    let texty = editing.is_some() || shapes.iter().any(|s| s.text.is_some() || matches!(s.kind, ShapeKind::Shape));
    let picture = matches!(sh.kind, ShapeKind::Picture { .. });

    header_row(app, ui, &sh, multi, shapes.len());
    position(app, ui, &x);
    layout(app, ui, &sh, &x, texty && !multi);
    appearance(app, ui, &sh, &x, multi);
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
fn section(ui: &mut Ui, title: &str, empty: bool, actions: &[(Icon, &str)]) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    ui.add_space(6.0);
    ui.painter().hline(ui.max_rect().x_range().expand(12.0), ui.cursor().min.y, Stroke::new(1.0, t.separator));
    ui.add_space(8.0);
    let mut out = None;
    ui.horizontal(|ui| {
        ui.set_min_height(26.0);
        ui.label(egui::RichText::new(title).font(theme::bold(13.5)).color(if empty { t.text_dim } else { t.text }));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (k, (icon, tip)) in actions.iter().enumerate().rev() {
                if icon_button(ui, *icon, tip, true).clicked() {
                    out = Some(k);
                }
            }
        });
    });
    ui.add_space(2.0);
    out
}

fn caption(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).font(theme::font(11.5)).color(t.text_dim));
}

fn icon_button(ui: &mut Ui, icon: Icon, tip: &str, en: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), if en { Sense::click() } else { Sense::hover() });
    if en && resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
    }
    icons::paint(ui.painter(), r.shrink(5.0), icon, t.text, !en);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, en, tip));
    resp.on_hover_text(tip)
}

/// A row of buttons in one grey pill; the ones that are on show as raised. Returns the one clicked.
fn seg_group(ui: &mut Ui, items: &[(Icon, &str, bool)], width: f32, en: bool) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(6), t.hover);
    let n = items.len().max(1) as f32;
    let cw = r.width() / n;
    let mut out = None;
    for (k, (icon, tip, on)) in items.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(r.min.x + k as f32 * cw, r.min.y), vec2(cw, r.height()));
        let resp = ui.interact(cell, ui.id().with(("seg", tip)), if en { Sense::click() } else { Sense::hover() });
        if *on {
            ui.painter().rect(cell.shrink(1.5), CornerRadius::same(5), t.card, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        } else if en && resp.hovered() {
            ui.painter().rect_filled(cell.shrink(1.5), CornerRadius::same(5), t.pressed);
        }
        if k > 0 && !on && !items.get(k - 1).is_some_and(|i| i.2) {
            ui.painter().vline(cell.min.x, cell.y_range().shrink(7.0), Stroke::new(1.0, t.card));
        }
        let s = (cell.height() - 12.0).min(cell.width() - 6.0);
        icons::paint(ui.painter(), Rect::from_center_size(cell.center(), vec2(s, s)), *icon, t.text, !en);
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, en, *on, *tip));
        if resp.on_hover_text(*tip).clicked() {
            out = Some(k);
        }
    }
    out
}

#[derive(Clone, Copy)]
enum Prefix {
    Text(&'static str),
    Icon(Icon),
}

/// A grey number field. Dragging the prefix scrubs the value (`step` per point dragged); typing
/// a number and pressing Enter or leaving the field sets it. Returns the new value when one is set.
#[allow(clippy::too_many_arguments)]
fn number(ui: &mut Ui, id: &str, prefix: Prefix, value: f64, suffix: &str, width: f32, step: f64, en: bool) -> Option<f64> {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(6), t.hover);
    let pre = Rect::from_min_size(r.min, vec2(28.0, r.height()));
    let scrub = ui.interact(pre, ui.id().with((id, "scrub")), if en { Sense::drag() } else { Sense::hover() });
    if en && (scrub.hovered() || scrub.dragged()) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let skey = ui.id().with((id, "scrubbing"));
    let mut out = None;
    let mut shown = value;
    if scrub.dragged() {
        let acc = ui.data_mut(|d| d.get_temp::<f64>(skey)).unwrap_or(value) + scrub.drag_delta().x as f64 * step;
        ui.data_mut(|d| d.insert_temp(skey, acc));
        shown = acc;
    }
    if scrub.drag_stopped() {
        out = ui.data_mut(|d| d.remove_temp::<f64>(skey));
        if let Some(v) = out {
            shown = v;
        }
    }
    let ink = if en { t.text_dim } else { t.text_faint };
    match prefix {
        Prefix::Text(s) => {
            ui.painter().text(pre.center(), Align2::CENTER_CENTER, s, theme::font(12.5), ink);
        }
        Prefix::Icon(i) => icons::paint(ui.painter(), Rect::from_center_size(pre.center(), vec2(14.0, 14.0)), i, ink, !en),
    }
    let tkey = ui.id().with((id, "typing"));
    let mut text: String = ui.data_mut(|d| d.get_temp(tkey)).unwrap_or_else(|| format!("{}{suffix}", widgets::fmt_num(shown, "")));
    let er = Rect::from_min_max(pos2(pre.max.x, r.min.y), r.max).shrink2(vec2(2.0, 6.0));
    // In a child area, so the field doesn't move the row's cursor.
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(er).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let te = child.add_sized(
        er.size(),
        egui::TextEdit::singleline(&mut text).frame(egui::Frame::NONE).font(theme::font(13.0)).interactive(en).id(ui.id().with((id, "edit"))),
    );
    if te.has_focus() {
        ui.data_mut(|d| d.insert_temp(tkey, text.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(tkey));
    }
    if te.lost_focus() {
        let num: String = text.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
        if let Ok(v) = num.parse::<f64>()
            && (v - value).abs() > 1e-9
        {
            out = Some(v);
        }
    }
    out
}

/// Two half-width columns and the side column.
fn cols(ui: &Ui) -> f32 {
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
    let title = if multi { format!("{n} layers") } else { kind_label(sh) };
    ui.horizontal(|ui| {
        ui.set_min_height(30.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let more_on = ui.ctx().data(|d| d.get_temp::<bool>(more_id())).unwrap_or(false);
            if icon_button(ui, Icon::More, if more_on { "Hide more options" } else { "More options: effects, size, text box" }, true).clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(more_id(), !more_on));
            }
            let r = icon_button(ui, Icon::Link, "Link", enabled(app, "insert.hyperlink"));
            egui::Popup::menu(&r).width(260.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| link_field(app, ui));
            // The name takes the rest of the row, cut short with … when it's long.
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&title).font(theme::bold(15.0)).color(t.text)).truncate());
            });
        });
    });
}

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

fn position(app: &mut SlideApp, ui: &mut Ui, x: &deckcraft_model::Xfrm) {
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
        let r = icon_button(ui, Icon::More, "Distribute", en);
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("Distribute horizontally").clicked() {
                run(app, "arrange.distribute", json!({"dir": "horizontal"}));
            }
            if ui.button("Distribute vertically").clicked() {
                run(app, "arrange.distribute", json!({"dir": "vertical"}));
            }
        });
    });
    let u = units::length(app);
    let en = enabled(app, "shape.move");
    caption(ui, "Position");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        if let Some(v) = number(ui, "x", Prefix::Text("X"), u.show(x.x).round(), "", c, 1.0, en) {
            run(app, "shape.move", json!({"x": u.to_pt(v)}));
        }
        if let Some(v) = number(ui, "y", Prefix::Text("Y"), u.show(x.y).round(), "", c, 1.0, en) {
            run(app, "shape.move", json!({"y": u.to_pt(v)}));
        }
    });
    caption(ui, "Rotation");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        if let Some(v) = number(ui, "rot", Prefix::Icon(Icon::Rotate), (x.rot * 10.0).round() / 10.0, "°", c, 1.0, enabled(app, "shape.rotate")) {
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

fn layout(app: &mut SlideApp, ui: &mut Ui, sh: &Shape, x: &deckcraft_model::Xfrm, texty: bool) {
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
    let lock_id = egui::Id::new("props-constrain");
    let lock = ui.ctx().data(|d| d.get_temp::<bool>(lock_id)).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        if let Some(v) = number(ui, "w", Prefix::Text("W"), u.show(x.w).round(), "", c, 1.0, en) {
            run(app, "shape.resize", json!({"w": u.to_pt(v).max(1.0), "lockAspect": lock}));
        }
        if let Some(v) = number(ui, "h", Prefix::Text("H"), u.show(x.h).round(), "", c, 1.0, en) {
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
pub fn corner_radius(sh: &Shape, x: &deckcraft_model::Xfrm) -> Option<f64> {
    let Geom::Preset { name, adj } = &sh.geom else { return None };
    (name == "roundRect").then(|| adj.first().copied().unwrap_or(16667.0) / 100_000.0 * x.w.min(x.h))
}

fn appearance(app: &mut SlideApp, ui: &mut Ui, sh: &Shape, x: &deckcraft_model::Xfrm, multi: bool) {
    let visible = !sh.hidden;
    let clicked =
        section(ui, "Appearance", false, &[(if visible { Icon::Eye } else { Icon::EyeOff }, if visible { "Hide layer" } else { "Show layer" })]);
    if clicked == Some(0) {
        run(app, "shape.visible", json!({"visible": !visible}));
    }
    let c = cols(ui);
    let sc = scheme(app);
    let picture = matches!(sh.kind, ShapeKind::Picture { .. });
    let has_fill = matches!(sh.fill, Some(Fill::Solid { .. } | Fill::Gradient(_) | Fill::Picture(_)));
    let radius = if multi { None } else { corner_radius(sh, x) };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        ui.vertical(|ui| {
            ui.set_width(c);
            caption(ui, "Opacity");
            let en = picture || has_fill;
            let cur = (opacity_of(sh, &sc) * 100.0).round();
            if let Some(v) = number(ui, "opacity", Prefix::Icon(Icon::Transparency), cur, "%", c, 1.0, en) {
                let tr = 1.0 - v.clamp(0.0, 100.0) / 100.0;
                if picture {
                    run(app, "picture.adjust", json!({"transparency": tr}));
                } else {
                    run(app, "shape.fill", json!({"transparency": tr}));
                }
            }
        });
        ui.vertical(|ui| {
            caption(ui, "Corner radius");
            let u = units::small(app);
            let shown = radius.map(|r| u.show(r).round()).unwrap_or(0.0);
            if let Some(v) = number(ui, "radius", Prefix::Icon(Icon::FitSmall), shown, "", c, 1.0, radius.is_some())
                && let Some(min) = Some(x.w.min(x.h)).filter(|m| *m > 0.0)
            {
                let adj = (u.to_pt(v).max(0.0) / min * 100_000.0).clamp(0.0, 50_000.0);
                run(app, "shape.adjust", json!({"index": 0, "value": adj.round()}));
            }
        });
    });
}

// ----- Typography -----------------------------------------------------------------------------

fn typography(app: &mut SlideApp, ui: &mut Ui, sh: &Shape) {
    let details_id = egui::Id::new("props-type-details");
    let details = ui.ctx().data(|d| d.get_temp::<bool>(details_id)).unwrap_or(false);
    section(ui, "Typography", false, &[]);
    let st = fmt_state(app);
    let en = enabled(app, "format.bold");
    style_chip(app, ui, &st, en);
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
            run(app, id, json!({}));
        }
        let v = [
            (Icon::AlignObjTop, "Text at top", anchor.starts_with('t')),
            (Icon::AlignObjMiddle, "Text in the middle", anchor.starts_with("ctr") || anchor.starts_with('m')),
            (Icon::AlignObjBottom, "Text at bottom", anchor.starts_with('b')),
        ];
        if let Some(k) = seg_group(ui, &v, c, en) {
            let anchor = ["top", "middle", "bottom"].get(k).copied().unwrap_or("top");
            run(app, "format.anchor", json!({"anchor": anchor}));
        }
        let t = Tokens::get(ui.ctx());
        let r = icon_button(ui, Icon::Settings, if details { "Hide type details" } else { "Type details: font, weight, size, spacing" }, true);
        if details {
            ui.painter().rect_stroke(r.rect, CornerRadius::same(5), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        }
        if r.clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(details_id, !details));
        }
    });
    if details {
        ui.add_space(6.0);
        crate::inspector::text_details(app, ui, &st, en, !text_color_is_fill(sh));
    }
}

/// The text style chip: "Ag Title · 120/108", which opens the style menu.
fn style_chip(app: &mut SlideApp, ui: &mut Ui, st: &Value, en: bool) {
    let t = Tokens::get(ui.ctx());
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let styles = crate::textstyles::styles(&doc);
    let current = crate::textstyles::current(app, &styles, st).unwrap_or_else(|| "Custom".into());
    let u = units::small(app);
    let size = st.get("size").and_then(Value::as_f64).unwrap_or(18.0);
    let lines = match st.get("lineSpacing") {
        Some(v) if v.get("kind").and_then(Value::as_str) == Some("pct") => v.get("v").and_then(Value::as_f64).unwrap_or(1.0),
        _ => 1.2,
    };
    let metrics = format!("{}/{}", widgets::fmt_num(u.show(size).round(), ""), widgets::fmt_num((u.show(size) * lines).round(), ""));
    let w = cols(ui) * 2.0 + GAP;
    let (r, resp) = ui.allocate_exact_size(vec2(w, 34.0), if en { Sense::click() } else { Sense::hover() });
    ui.painter().rect(r, CornerRadius::same(6), if resp.hovered() { t.hover } else { t.card }, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let ag = ui.painter().text(pos2(r.min.x + 8.0, r.center().y), Align2::LEFT_CENTER, "Ag", theme::font(17.0), t.text);
    let name = ui.painter().text(pos2(ag.max.x + 6.0, r.center().y), Align2::LEFT_CENTER, &current, theme::font(13.0), t.text);
    ui.painter().text(pos2(name.max.x + 4.0, r.center().y), Align2::LEFT_CENTER, format!("· {metrics}"), theme::font(13.0), t.text_dim);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, en, format!("Text style: {current}")));
    crate::inspector::style_menu(app, &resp, &current, w.max(220.0));
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
fn color_row(
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
    let op_w = if opacity.is_some() { 70.0 } else { 0.0 };
    let (r, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(6), t.hover);
    let sw = Rect::from_center_size(pos2(r.min.x + 16.0, r.center().y), vec2(16.0, 16.0));
    let sresp = ui.interact(sw.expand(3.0), ui.id().with((id, "swatch")), Sense::click());
    match rgba {
        Some(c) => {
            ui.painter().rect(
                sw,
                CornerRadius::same(3),
                theme::to_color32(deckcraft_color::Rgba { a: 255, ..c }),
                Stroke::new(1.0, t.border),
                egui::StrokeKind::Inside,
            );
        }
        None => {
            ui.painter().rect_stroke(sw, CornerRadius::same(3), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
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
        ui.painter().text(pos2(hex_rect.min.x, r.center().y), Align2::LEFT_CENTER, l, theme::font(13.0), t.text);
    } else if let Some(c) = rgba {
        let tkey = ui.id().with((id, "hex"));
        let mut text: String = ui.data_mut(|d| d.get_temp(tkey)).unwrap_or_else(|| hex(c));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(hex_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let te = child.add_sized(
            hex_rect.size(),
            egui::TextEdit::singleline(&mut text).frame(egui::Frame::NONE).font(theme::font(13.0)).id(ui.id().with((id, "hexedit"))),
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
        ui.painter().vline(orect.min.x, orect.y_range().shrink(6.0), Stroke::new(1.0, t.card));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(orect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        new_op = number(&mut child, &format!("{id}-op"), Prefix::Text(""), op.round(), "%", orect.width(), 1.0, true);
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
            run(app, "format.color", json!({"color": col}));
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
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let (picked, op) = color_row(app, ui, "Fill", rgba, label, if label.is_none() { opacity } else { None }, width);
        if let Some(col) = picked {
            run(app, "shape.fill", json!({"color": col}));
        }
        if let Some(v) = op {
            run(app, "shape.fill", json!({"transparency": 1.0 - v.clamp(0.0, 100.0) / 100.0}));
        }
        let r = icon_button(ui, Icon::More, "Fill type: solid, gradient, image", true);
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
            ui.separator();
            if ui.button("Remove fill").clicked() {
                run(app, "shape.fill", json!({"none": true}));
            }
        });
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
    let actions: Vec<(Icon, &str)> = if none { vec![(Icon::Plus, "Add stroke")] } else { vec![(Icon::Minus, "Remove stroke")] };
    match section(ui, "Stroke", none, &actions) {
        Some(0) if none => {
            run(app, "shape.line", json!({"color": "tx1", "width": 1}));
            return;
        }
        Some(0) => {
            run(app, "shape.line", json!({"none": true}));
            return;
        }
        _ => {}
    }
    if none {
        return;
    }
    let (rgba, label) = match line.and_then(|l| l.fill.as_ref()) {
        Some(Fill::Solid { color }) => (Some(color.resolve(&sc, None)), None),
        _ => (None, Some("Automatic")),
    };
    let c = cols(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let (picked, _) = color_row(app, ui, "Stroke", rgba, label, None, c);
        if let Some(col) = picked {
            run(app, "shape.line", json!({"color": col}));
        }
        let u = units::small(app);
        let w = line.and_then(|l| l.width).unwrap_or(0.75);
        if let Some(v) = number(ui, "stroke-w", Prefix::Icon(Icon::LineSpacing), (u.show(w) * 4.0).round() / 4.0, "", c, 0.25, true) {
            run(app, "shape.line", json!({"width": u.to_pt(v).max(0.0)}));
        }
    });
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
        ui.label(egui::RichText::new("Effects").font(theme::bold(13.5)).color(if rows.is_empty() { t.text_dim } else { t.text }));
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
        ui.label(egui::RichText::new("Export").font(theme::bold(13.5)).color(t.text_dim));
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
