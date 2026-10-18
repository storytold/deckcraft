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

fn run(app: &mut SlideApp, id: &str, p: Value) {
    if id.starts_with("format.") {
        // While typing with a bare caret, the panel formats the whole box.
        crate::textstyles::run_on_text(app, id, vec![(id, p)]);
        return;
    }
    let _ = app.run(id, p);
}

pub fn design(app: &mut SlideApp, ui: &mut Ui) {
    let Some(st) = app.session.active() else { return };
    let editing = st.selection.text.as_ref().is_some_and(|t| !t.notes);
    let shapes: Vec<deckcraft_model::Shape> = st.selected_shapes().into_iter().cloned().collect();
    if shapes.is_empty() && !editing {
        if st.selection.target == deckcraft_engine::Target::Slides {
            text_styles(app, ui);
            slide(app, ui);
        } else {
            template(app, ui);
            text_styles(app, ui);
        }
        return;
    }
    crate::props::show(app, ui);
}

// ----- Layout helpers ---------------------------------------------------------------------

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

/// Bold, italic, underline, strikethrough, lists and (when `color`) the text colour.
pub(crate) fn type_extras(app: &mut SlideApp, ui: &mut Ui, st: &Value, en: bool, color: bool) {
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
            ui.horizontal(|ui| {
                let update_w = 64.0;
                if ui.add(egui::Button::selectable(on, label).min_size(vec2(w - 20.0 - update_w, 0.0))).on_hover_text(tip).clicked() {
                    crate::textstyles::apply(app, s);
                    ui.close();
                }
                // Make the style match the selected text, which then follows it.
                let r = ui.add_sized(vec2(update_w, 22.0), egui::Button::new(egui::RichText::new("Update").font(theme::font(12.0))));
                if r.on_hover_text(format!("Update {} to match the selected text", s.name)).clicked() {
                    crate::textstyles::update_from_selection(app, s);
                    ui.close();
                }
            });
        }
    });
}

/// Nothing selected: the slide's layout, then its background, in the property panel's fields.
fn slide(app: &mut SlideApp, ui: &mut Ui) {
    use crate::props::{GAP, caption, cols, pick_wide};
    let Some(st) = app.session.active() else { return };
    let layout =
        st.current_slide().and_then(|s| st.doc.masters.first().and_then(|m| m.layouts.iter().find(|l| l.id == s.layout))).map(|l| l.name.clone());
    let hide = st.current_slide().map(|s| !s.show_master_shapes).unwrap_or(false);
    let own = st.current_slide().and_then(|s| s.background.clone());
    let full = cols(ui) * 2.0 + GAP;
    // Reset: the slide's placeholders back on their layout's positions, sizes and formatting.
    if crate::props::section(ui, "Slide", false, &[(Icon::Revert, "Reset slide to its layout")]) == Some(0) {
        run(app, "slide.reset", json!({}));
    }
    caption(ui, "Layout");
    pick_wide(ui, "Layout", &layout.unwrap_or_else(|| "Layout".into()), full, 420.0, true, |ui| crate::ribbon::layout_menu(app, ui, "slide.layout"));
    background_editor(app, ui, own, Some("Follow the layout"), true, Some(hide));
}

/// The background of what's being edited (slide, layout or master): what it shows, how it's
/// filled, and a colour row that always works (picking a colour gives it a solid background).
/// `follow`: the label for having none of its own (None on the master, which always has one);
/// `apply_all`: offer giving every slide this background; `hide`: the Hide background graphics
/// box and its state.
fn background_editor(
    app: &mut SlideApp,
    ui: &mut Ui,
    own: Option<deckcraft_model::Background>,
    follow: Option<&str>,
    apply_all: bool,
    hide: Option<bool>,
) {
    use crate::props::{GAP, caption, cols, pick};
    let full = cols(ui) * 2.0 + GAP;
    let shown = shown_background(app);
    crate::props::section(ui, "Background", false, &[]);
    let sc = scheme(app);
    let fill = match &own {
        Some(deckcraft_model::Background::Fill { fill }) => Some(fill.clone()),
        _ => None,
    };
    let kind = match (&own, &fill) {
        (_, Some(Fill::Solid { .. })) => "solid",
        (_, Some(Fill::Gradient(_))) => "gradient",
        (_, Some(Fill::Picture(_))) => "picture",
        (_, Some(Fill::Pattern(_))) => "pattern",
        (Some(deckcraft_model::Background::Ref { .. }), _) => "theme",
        _ => "auto",
    };
    let mut kinds: Vec<(&str, &str)> = vec![];
    match follow {
        Some(f) => kinds.push((f, "auto")),
        None => kinds.push(("Theme background", "auto")),
    }
    if kind == "theme" {
        kinds.push(("Theme background style", "theme"));
    }
    kinds.extend([("Solid", "solid"), ("Gradient", "gradient"), ("Picture…", "picture"), ("Pattern", "pattern")]);
    let label = kinds.iter().find(|k| k.1 == kind).map(|k| k.0.trim_end_matches('…')).unwrap_or("Background");
    let mut chose = None;
    pick(ui, "Background", label, full, true, |ui| {
        for (l, k) in &kinds {
            if ui.selectable_label(*k == kind, *l).clicked() {
                chose = Some(*k);
                ui.close();
            }
        }
    });
    match chose.filter(|k| *k != kind || *k == "picture") {
        Some("solid") => run(app, "design.background", json!({"color": format!("#{}", shown.hex())})),
        Some("gradient") => {
            let mut g = crate::fillui::default_gradient(deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Bg2), 90.0);
            g.rotate_with_shape = false;
            run(app, "design.background", json!({"gradient": crate::fillui::gradient_json(&g)}));
        }
        Some("picture") => {
            if let Some(pick) = app.services.pick_open.as_mut()
                && let Some(path) = pick("picture")
                && let Some(Ok(bytes)) = app.services.read.as_ref().map(|r| r(&path))
            {
                run(app, "design.background", json!({"picture": deckcraft_engine::cmd::base64_encode(&bytes)}));
            }
        }
        Some("pattern") => run(app, "design.background", json!({"pattern": {"preset": "pct20", "fg": "accent1", "bg": "bg1"}})),
        Some("theme") => {}
        Some(_) => run(app, "design.background", json!({"reset": true})),
        None => {}
    }
    // What Apply to All re-applies: this background as command params.
    let mut current: Option<Value> = None;
    ui.add_space(6.0);
    match &fill {
        Some(Fill::Gradient(g)) => {
            current = Some(json!({"gradient": crate::fillui::gradient_json(g)}));
            if let Some(ng) = crate::fillui::gradient_editor(ui, "bg-grad", g, &sc) {
                run(app, "design.background", json!({"gradient": crate::fillui::gradient_json(&ng)}));
            }
        }
        Some(Fill::Pattern(pt)) => {
            current = Some(json!({"pattern": {"preset": pt.preset, "fg": cref_param(&pt.fg), "bg": cref_param(&pt.bg)}}));
            caption(ui, "Pattern");
            pick(ui, "Pattern", &pt.preset, full, true, |ui| {
                for preset in ["pct5", "pct20", "pct50", "ltHorz", "ltVert", "smGrid", "dkDnDiag", "wave", "dotGrid", "weave"] {
                    if ui.selectable_label(pt.preset == preset, preset).clicked() {
                        run(app, "design.background", json!({"pattern": {"preset": preset, "fg": cref_param(&pt.fg), "bg": cref_param(&pt.bg)}}));
                        ui.close();
                    }
                }
            });
            for (label, fg) in [("Foreground", true), ("Background", false)] {
                caption(ui, label);
                let c = if fg { &pt.fg } else { &pt.bg };
                let (picked, _) = crate::props::color_row(app, ui, label, Some(c.resolve(&sc, None)), None, None, full);
                if let Some(v) = picked {
                    let (f, b) = if fg { (v, cref_param(&pt.bg)) } else { (cref_param(&pt.fg), v) };
                    run(app, "design.background", json!({"pattern": {"preset": pt.preset, "fg": f, "bg": b}}));
                }
            }
        }
        Some(Fill::Picture(_)) => {
            if ui.button("Replace picture…").clicked()
                && let Some(pick) = app.services.pick_open.as_mut()
                && let Some(path) = pick("picture")
                && let Some(Ok(bytes)) = app.services.read.as_ref().map(|r| r(&path))
            {
                run(app, "design.background", json!({"picture": deckcraft_engine::cmd::base64_encode(&bytes)}));
            }
        }
        other => {
            // Solid, or following the layout / master / theme: the colour shown, and picking one
            // gives this its own solid background.
            if let Some(Fill::Solid { color }) = other {
                current = Some(json!({"color": cref_param(color)}));
            }
            caption(ui, "Color");
            let (picked, _) = crate::props::color_row(app, ui, "Background", Some(shown), None, None, full);
            if let Some(c) = picked {
                run(app, "design.background", json!({"color": c}));
            }
        }
    }
    if let Some(hide) = hide {
        ui.add_space(6.0);
        let mut h = hide;
        if ui.checkbox(&mut h, "Hide background graphics").on_hover_text("Leave out the pictures and shapes on the layout and master").changed() {
            run(app, "design.hideBackgroundGraphics", json!({"hide": h}));
        }
    }
    ui.add_space(6.0);
    let c = cols(ui);
    let t = Tokens::get(ui.ctx());
    let button = |ui: &mut Ui, label: &str, en: bool| {
        let b =
            egui::Button::new(egui::RichText::new(label).font(theme::font(12.5))).fill(t.field).corner_radius(egui::CornerRadius::same(6)).truncate();
        ui.add_enabled_ui(en, |ui| ui.add_sized(vec2(c, crate::props::ROW_H), b)).inner
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        if apply_all
            && button(ui, "Apply to all", current.is_some()).on_hover_text("Give every slide this background").clicked()
            && let Some(Value::Object(mut o)) = current.clone()
        {
            o.insert("all".into(), json!(true));
            run(app, "design.background", Value::Object(o));
        }
        let tip = match follow {
            Some(f) => format!("{f} again"),
            None => "Back to the theme's background".into(),
        };
        if button(ui, "Reset", own.is_some()).on_hover_text(tip).clicked() {
            run(app, "design.background", json!({"reset": true}));
        }
    });
}

/// The colour the background being edited shows (its first colour for a gradient or pattern).
fn shown_background(app: &SlideApp) -> deckcraft_color::Rgba {
    use deckcraft_engine::Target;
    use deckcraft_model::resolve::{self, Ctx};
    let white = deckcraft_color::Rgba::rgb(255, 255, 255);
    let Some(st) = app.session.active() else { return white };
    let doc = &st.doc;
    let (ctx, slide) = match st.selection.target {
        Target::Slides => match doc.slides.get(st.selection.slide) {
            Some(sl) => (Ctx::for_slide(doc, sl), Some(sl.as_ref())),
            None => (None, None),
        },
        Target::Master { master } => (doc.masters.get(master).map(|m| Ctx::for_master(doc, m)), None),
        Target::Layout { master, layout } => (doc.masters.get(master).and_then(|m| m.layouts.get(layout).map(|l| Ctx::for_layout(doc, m, l))), None),
        Target::Handout | Target::NotesMaster => (None, None),
    };
    let Some(ctx) = ctx else { return white };
    let (fill, ph) = resolve::background(&ctx, slide);
    match fill {
        Fill::Solid { color } => ctx.color(&color, ph),
        Fill::Gradient(g) => g.stops.first().map(|s| ctx.color(&s.color, ph)).unwrap_or(white),
        Fill::Pattern(p) => ctx.color(&p.bg, ph),
        _ => white,
    }
}

// ----- Slide master and layouts (Slide Master view) ----------------------------------------

/// Nothing selected in Slide Master view: the master or layout's name, its background, and the
/// theme's colours and fonts (the master's, shared by its layouts and slides).
fn template(app: &mut SlideApp, ui: &mut Ui) {
    use crate::props::{GAP, caption, cols};
    use deckcraft_engine::Target;
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let target = st.selection.target;
    let (mi, li) = match target {
        Target::Master { master } => (master, None),
        Target::Layout { master, layout } => (master, Some(layout)),
        Target::Slides | Target::Handout | Target::NotesMaster => return,
    };
    let Some(m) = doc.masters.get(mi) else { return };
    let full = cols(ui) * 2.0 + GAP;
    match li.and_then(|l| m.layouts.get(l)) {
        Some(l) => {
            ui.label(egui::RichText::new("Layout").font(theme::bold(13.0)).color(t.text));
            ui.add_space(4.0);
            caption(ui, "Name");
            let key = ui.id().with(("layout-name", mi, li));
            let mut name: String = ui.data_mut(|d| d.get_temp(key)).unwrap_or_else(|| l.name.clone());
            let r = egui::Frame::NONE.fill(t.field).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                ui.add(egui::TextEdit::singleline(&mut name).frame(egui::Frame::NONE).font(theme::font(12.5)).desired_width(full - 16.0))
            });
            let te = r.inner;
            if te.has_focus() {
                ui.data_mut(|d| d.insert_temp(key, name.clone()));
            } else {
                ui.data_mut(|d| d.remove::<String>(key));
            }
            if te.lost_focus() && !name.trim().is_empty() && name.trim() != l.name {
                let _ = app.run("master.renameLayout", json!({"name": name.trim()}));
            }
            let used = doc.slides.iter().filter(|s| s.layout == l.id).count();
            ui.add_space(4.0);
            let note = match used {
                0 => "No slides use this layout yet".to_string(),
                1 => "1 slide uses this layout".to_string(),
                n => format!("{n} slides use this layout"),
            };
            ui.label(egui::RichText::new(note).font(theme::font(11.5)).color(t.caption));
            let own = l.background.clone();
            let hide = !l.show_master_shapes;
            background_editor(app, ui, own, Some("Follow the master"), false, Some(hide));
        }
        None => {
            ui.label(egui::RichText::new(format!("{} Slide Master", m.name)).font(theme::bold(13.0)).color(t.text));
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("What every layout and slide starts from: background, theme colours and fonts, text styles.")
                    .font(theme::font(11.5))
                    .color(t.caption),
            );
            background_editor(app, ui, m.background.clone(), None, false, None);
        }
    }
    theme_colors(app, ui, m);
    theme_fonts(app, ui, m);
}

/// The theme's twelve colours: a scheme to start from, and each colour's own swatch and hex.
fn theme_colors(app: &mut SlideApp, ui: &mut Ui, m: &deckcraft_model::Master) {
    use crate::props::{GAP, caption, cols, pick};
    use deckcraft_color::SchemeSlot as S;
    let c = cols(ui);
    crate::props::section(ui, "Theme colors", false, &[]);
    let colors = &m.theme.colors;
    pick(ui, "Color scheme", &colors.name, c * 2.0 + GAP, true, |ui| {
        for sc in deckcraft_model::theme::builtin_color_schemes() {
            if ui.selectable_label(sc.name == colors.name, &sc.name).clicked() {
                let _ = app.run("design.colors", json!({"name": sc.name}));
                ui.close();
            }
        }
    });
    ui.add_space(4.0);
    let slots = [
        (S::Dk1, "dk1", "Text dark"),
        (S::Lt1, "lt1", "Background light"),
        (S::Dk2, "dk2", "Text dark 2"),
        (S::Lt2, "lt2", "Background light 2"),
        (S::Accent1, "accent1", "Accent 1"),
        (S::Accent2, "accent2", "Accent 2"),
        (S::Accent3, "accent3", "Accent 3"),
        (S::Accent4, "accent4", "Accent 4"),
        (S::Accent5, "accent5", "Accent 5"),
        (S::Accent6, "accent6", "Accent 6"),
        (S::Hlink, "hlink", "Link"),
        (S::FolHlink, "folHlink", "Visited link"),
    ];
    for pair in slots.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (slot, xml, label) in pair {
                ui.vertical(|ui| {
                    ui.set_width(c);
                    caption(ui, label);
                    if let Some(hex) = slot_color(ui, xml, colors.get(*slot), c) {
                        let _ = app.run("design.colors", json!({"colors": {*xml: hex}}));
                    }
                });
            }
        });
    }
}

/// A theme colour: a swatch that opens a colour picker, and its hex. Returns `#RRGGBB` when changed.
fn slot_color(ui: &mut Ui, id: &str, c: deckcraft_color::Rgba, width: f32) -> Option<String> {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(width, crate::props::ROW_H), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(6), t.field);
    let sw = Rect::from_center_size(pos2(r.min.x + 15.0, r.center().y), vec2(14.0, 14.0));
    let sresp = ui.interact(sw.expand(3.0), ui.id().with(("theme-slot", id)), Sense::click());
    ui.painter().rect(sw, CornerRadius::same(2), theme::to_color32(c), Stroke::new(1.0, Color32::from_black_alpha(25)), egui::StrokeKind::Inside);
    sresp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Theme color {id}")));
    let mut out = None;
    widgets::color_popup(&sresp).show(|ui| {
        let pid = ui.id().with(("theme-slot-pick", id));
        let (mut col, mut pending): (Color32, bool) = ui.data_mut(|d| d.get_temp(pid)).unwrap_or((theme::to_color32(c), false));
        if egui::widgets::color_picker::color_picker_color32(ui, &mut col, egui::widgets::color_picker::Alpha::Opaque) {
            pending = true;
        }
        if pending && !ui.input(|i| i.pointer.any_down()) {
            pending = false;
            out = Some(format!("#{:02X}{:02X}{:02X}", col.r(), col.g(), col.b()));
        }
        ui.data_mut(|d| d.insert_temp(pid, (col, pending)));
    });
    let hex_rect = Rect::from_min_max(pos2(sw.max.x + 8.0, r.min.y), r.max).shrink2(vec2(0.0, 6.0));
    let tkey = ui.id().with(("theme-slot-hex", id));
    let now = format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b);
    let mut text: String = ui.data_mut(|d| d.get_temp(tkey)).unwrap_or_else(|| now.clone());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(hex_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let te = child.add_sized(
        hex_rect.size(),
        egui::TextEdit::singleline(&mut text)
            .frame(egui::Frame::NONE)
            .font(theme::font(12.5))
            .text_color(t.text)
            .id(ui.id().with(("theme-slot-edit", id))),
    );
    if te.has_focus() {
        ui.data_mut(|d| d.insert_temp(tkey, text.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(tkey));
    }
    if te.lost_focus()
        && let Some(h) = crate::props::parse_hex(&text)
        && h[1..] != now
    {
        out = Some(h);
    }
    out
}

/// The theme's heading and body fonts.
fn theme_fonts(app: &mut SlideApp, ui: &mut Ui, m: &deckcraft_model::Master) {
    use crate::props::{GAP, caption, cols, pick};
    let c = cols(ui);
    crate::props::section(ui, "Theme fonts", false, &[]);
    let (major, minor) = (m.theme.fonts.major.latin.clone(), m.theme.fonts.minor.latin.clone());
    let families = crate::ribbon::font_families(app);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for (label, heading) in [("Headings", true), ("Body", false)] {
            ui.vertical(|ui| {
                ui.set_width(c);
                caption(ui, label);
                let now = if heading { &major } else { &minor };
                pick(ui, label, now, c, true, |ui| {
                    for f in &families {
                        if ui.selectable_label(f == now, f).clicked() {
                            let (a, b) = if heading { (f.clone(), minor.clone()) } else { (major.clone(), f.clone()) };
                            let _ = app.run("design.fonts", json!({"major": a, "minor": b}));
                            ui.close();
                        }
                    }
                });
            });
        }
    });
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
    use crate::props::{GAP, Prefix, caption, cols, number, pick, seg_group};
    use crate::textstyles::Edit;
    let t = Tokens::get(ui.ctx());
    // Header: back to the list, then the style's name in its own weight.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if crate::props::icon_button(ui, Icon::ChevronLeft, "Back to text styles", true).clicked() {
            edit_text_style(ui.ctx(), None);
        }
        let font = if s.bold() { theme::bold(17.0) } else { theme::font(17.0) };
        ui.label(egui::RichText::new(s.name).font(font).color(t.text));
    });
    let n = crate::textstyles::copies(doc, s).len();
    let reach = match n {
        0 => format!("Changes all {} text in this presentation.", s.name),
        1 => format!("Changes all {} text, and 1 copy of it.", s.name),
        n => format!("Changes all {} text, and {n} copies of it.", s.name),
    };
    note(ui, &reach);
    ui.add_space(8.0);

    let mut edits: Vec<Edit> = vec![];
    let family = s.family(doc);
    let c = cols(ui);
    let full = c * 2.0 + GAP;
    caption(ui, "Font");
    let families = crate::ribbon::font_families(app);
    pick(ui, "Font", &family, full, true, |ui| {
        for f in families {
            if ui.selectable_label(f == family, &f).clicked() {
                edits.push(Edit::Font(f));
                ui.close();
            }
        }
    });
    match s.theme_font() {
        Some(true) => note(ui, "The theme's heading font: every style and shape using it changes too."),
        Some(false) => note(ui, "The theme's body font: every style and shape using it changes too."),
        None => {}
    }
    ui.add_space(4.0);
    let (base, weights, cur) = weights(&family, s.bold());
    let u = crate::units::small(app);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        ui.vertical(|ui| {
            ui.set_width(c);
            caption(ui, "Weight");
            pick(ui, "Weight", &cur, c, true, |ui| {
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
            if let Some(v) =
                number(ui, "style-size", Prefix::Text("Aa"), (u.show(s.size()) * 100.0).round() / 100.0, "", c, 1.0, (1.0, u.show(4000.0)), true)
            {
                edits.push(Edit::Size(u.to_pt(v).clamp(1.0, 4000.0)));
            }
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        ui.vertical(|ui| {
            ui.set_width(c);
            caption(ui, "Line height");
            let pct = match s.line_spacing {
                Some(deckcraft_model::text::Spacing::Pct(v)) => v * 100.0,
                _ => 100.0,
            };
            if let Some(v) = number(ui, "style-lh", Prefix::Icon(Icon::LineSpacing), pct.round(), "%", c, 1.0, (10.0, 999.0), true) {
                edits.push(Edit::LineSpacing((v / 100.0).clamp(0.1, 9.99)));
            }
        });
        ui.vertical(|ui| {
            caption(ui, "Style");
            if seg_group(ui, &[(Icon::Italic, "Italic", s.italic())], c, true).is_some() {
                edits.push(Edit::Italic(!s.italic()));
            }
        });
    });
    ui.add_space(4.0);
    caption(ui, "Color");
    let sc = scheme(app);
    let rgba = match &s.run.fill {
        Some(Fill::Solid { color }) => color.resolve(&sc, None),
        _ => deckcraft_color::Rgba::BLACK,
    };
    let (picked, _) = crate::props::color_row(app, ui, "Style", Some(rgba), None, None, full);
    if let Some(col) = picked {
        edits.push(Edit::Color(col));
    }
    if !edits.is_empty() {
        let mut cmds = vec![];
        for e in &edits {
            cmds.extend(crate::textstyles::edit_commands(app, s, e));
        }
        app.run_as_one(&format!("Edit Text Style: {}", s.name), cmds);
    }
}

/// A dim note under a field, wrapped with room between its lines.
fn note(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(2.0);
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat { font_id: theme::font(11.5), color: t.text_dim, line_height: Some(15.0), ..Default::default() },
    );
    job.wrap.max_width = ui.available_width();
    ui.label(job);
}

#[cfg(test)]
#[path = "inspector_tests.rs"]
mod tests;
