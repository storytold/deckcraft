//! Title bar, ribbon tabs and the ribbon card with every tab's groups.

use deckcraft_engine::Target;
use deckcraft_model::{ShapeKind, anim::AnimClass};
use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::SlideApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};
use crate::widgets::{self, RIBBON_H, big_button, big_button_ex, color_split, dropdown, group, icon_toggle, rows, small_button, small_button_ex};

pub const TITLE_H: f32 = 38.0;

/// Is engine command `id` enabled right now?
pub fn enabled(app: &SlideApp, id: &str) -> bool {
    match deckcraft_engine::find_command(id) {
        Some(c) => (c.enabled)(&app.session).is_ok(),
        None => crate::UI_COMMANDS.iter().any(|c| c.0 == id),
    }
}

fn run(app: &mut SlideApp, id: &str, p: Value) {
    let _ = app.run(id, p);
}

/// The window's title row: traffic-light space, quick access, centred title, actions.
pub fn title_bar(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::top("title_bar").exact_size(TITLE_H).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let rect = ui.max_rect();
        // Dragging the empty title bar moves the window.
        let drag = ui.interact(rect, ui.id().with("titledrag"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            let max = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        let left = if app.integrated_titlebar { 84.0 } else { 10.0 };
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(rect.min.x + left, rect.min.y), pos2(rect.min.x + 420.0, rect.max.y)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 2.0;
        let has_doc = app.session.active().is_some();
        let dirty = app.session.active().is_some_and(|d| d.is_dirty());
        if icon_toggle(&mut child, Icon::Home, "Home", true, false).clicked() {
            let _ = app.run("file.new", json!({}));
        }
        if icon_toggle(&mut child, Icon::Save, "Save (⌘S)", has_doc, false).clicked() {
            app.save();
        }
        if icon_toggle(&mut child, Icon::Undo, "Undo (⌘Z)", enabled(app, "edit.undo"), false).clicked() {
            run(app, "edit.undo", json!({}));
        }
        if icon_toggle(&mut child, Icon::Redo, "Redo (⌘Y)", enabled(app, "edit.redo"), false).clicked() {
            run(app, "edit.redo", json!({}));
        }
        if icon_toggle(&mut child, Icon::PlayFromStart, "Play from Start (F5)", has_doc, false).clicked() {
            app.start_show(0, false);
        }
        let title =
            app.session.active().map(|d| format!("{}{}", d.title(), if dirty { " — Edited" } else { "" })).unwrap_or_else(|| "DeckCraft".into());
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, title, theme::font(13.0), t.text);
        // Right side.
        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(rect.max.x - 320.0, rect.min.y), pos2(rect.max.x - 10.0, rect.max.y)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        right.spacing_mut().item_spacing.x = 6.0;
        if icon_toggle(&mut right, Icon::Search, "Search commands (⇧⌘P)", true, false).clicked() {
            app.palette = Some((String::new(), 0));
        }
        let share = right.add(
            egui::Button::new(egui::RichText::new("  Present  ").color(t.accent_text).font(theme::bold(13.0)))
                .fill(t.accent)
                .corner_radius(CornerRadius::same(13))
                .min_size(vec2(84.0, 26.0)),
        );
        if share.clicked() && has_doc {
            let from = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            app.start_show(from, false);
        }
        if icon_toggle(&mut right, Icon::CommentsBubble, "Comments", has_doc, app.ui.pane.as_deref() == Some("comments")).clicked() {
            let _ = app.run("view.pane", json!({"pane": "comments", "toggle": true}));
        }
        if icon_toggle(
            &mut right,
            if app.ui.brightness == theme::Brightness::Dark { Icon::Sparkle } else { Icon::Palette },
            "Light/Dark appearance",
            true,
            false,
        )
        .clicked()
        {
            let _ = app.run("view.dark", json!({}));
        }
    });
}

/// Which contextual tabs apply to the selection.
fn contextual(app: &SlideApp) -> Vec<&'static str> {
    let mut v = vec![];
    let Some(st) = app.session.active() else { return v };
    if st.selection.target != Target::Slides {
        v.push("Slide Master");
    }
    let shapes = st.selected_shapes();
    if shapes.iter().any(|s| matches!(s.kind, ShapeKind::Shape | ShapeKind::Group { .. } | ShapeKind::Connector { .. }))
        || st.selection.text.is_some()
    {
        v.push("Shape Format");
    }
    if shapes.iter().any(|s| matches!(s.kind, ShapeKind::Picture { .. })) {
        v.push("Picture Format");
    }
    if shapes.iter().any(|s| matches!(s.kind, ShapeKind::Table(_))) || st.selection.text.as_ref().is_some_and(|t| t.cell.is_some()) {
        v.push("Table Design");
        v.push("Layout");
    }
    if shapes.iter().any(|s| matches!(s.kind, ShapeKind::Chart(_))) {
        v.push("Chart Design");
    }
    if shapes.iter().any(|s| matches!(s.kind, ShapeKind::Media(_))) {
        v.push("Playback");
    }
    v
}

const TABS: [&str; 10] = ["Home", "Insert", "Draw", "Design", "Transitions", "Animations", "Slide Show", "Record", "Review", "View"];

/// Tab row and the ribbon card.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let ctx_tabs = contextual(app);
    if !TABS.contains(&app.ui.tab.as_str()) && !ctx_tabs.contains(&app.ui.tab.as_str()) {
        app.ui.tab = "Home".into();
    }
    let h = 28.0 + if app.ui.ribbon_collapsed { 4.0 } else { RIBBON_H + 20.0 };
    egui::Panel::top("ribbon").exact_size(h).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let rect = ui.max_rect();
        // Tabs.
        let mut x = rect.min.x + 12.0;
        let y = rect.min.y + 2.0;
        let all: Vec<(&str, bool)> = TABS.iter().map(|s| (*s, false)).chain(ctx_tabs.iter().map(|s| (*s, true))).collect();
        for (name, contextual) in all {
            let font = theme::font(13.0);
            let w = ui.fonts_mut(|f| f.layout_no_wrap(name.to_string(), font.clone(), t.text).size().x) + 18.0;
            let r = Rect::from_min_size(pos2(x, y), vec2(w, 24.0));
            let resp = ui.interact(r, ui.id().with(("tab", name)), Sense::click());
            let active = app.ui.tab == name;
            if resp.hovered() && !active {
                ui.painter().rect_filled(r.shrink2(vec2(2.0, 2.0)), CornerRadius::same(4), t.hover);
            }
            let col = if contextual { t.contextual } else { t.text };
            let f = if active { theme::bold(13.0) } else { font };
            ui.painter().text(r.center(), Align2::CENTER_CENTER, name, f, col);
            if active {
                let uw = (w - 20.0).max(16.0);
                ui.painter().rect_filled(Rect::from_center_size(pos2(r.center().x, r.max.y - 1.0), vec2(uw, 3.0)), CornerRadius::same(1), t.accent);
            }
            if resp.clicked() {
                if active {
                    app.ui.ribbon_collapsed = !app.ui.ribbon_collapsed;
                } else {
                    app.ui.tab = name.to_string();
                    app.ui.ribbon_collapsed = false;
                }
            }
            if resp.double_clicked() {
                app.ui.ribbon_collapsed = !app.ui.ribbon_collapsed;
            }
            x += w + 2.0;
        }
        if app.ui.ribbon_collapsed {
            return;
        }
        // The white card.
        let card = Rect::from_min_max(pos2(rect.min.x + 8.0, rect.min.y + 30.0), pos2(rect.max.x - 8.0, rect.min.y + 30.0 + RIBBON_H + 10.0));
        ui.painter().rect_filled(card.translate(vec2(0.0, 1.0)), CornerRadius::same(10), t.card_border);
        ui.painter().rect_filled(card, CornerRadius::same(10), t.card);
        let inner = Rect::from_min_size(pos2(card.min.x + 10.0, card.min.y + 5.0), vec2(card.width() - 20.0, RIBBON_H));
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Min)));
        cui.set_clip_rect(inner.expand2(vec2(0.0, 3.0)));
        cui.spacing_mut().item_spacing.x = 1.0;
        let ui = &mut cui;
        let tab = app.ui.tab.clone();
        match tab.as_str() {
            "Home" => home(app, ui),
            "Insert" => insert(app, ui),
            "Draw" => draw(app, ui),
            "Design" => design(app, ui),
            "Transitions" => transitions(app, ui),
            "Animations" => animations(app, ui),
            "Slide Show" => slide_show(app, ui),
            "Record" => record(app, ui),
            "Review" => review(app, ui),
            "View" => view(app, ui),
            "Shape Format" => shape_format(app, ui),
            "Picture Format" => picture_format(app, ui),
            "Table Design" => table_design(app, ui),
            "Layout" => table_layout(app, ui),
            "Chart Design" => chart_design(app, ui),
            "Playback" => playback(app, ui),
            "Slide Master" => slide_master(app, ui),
            _ => {}
        }
    });
}

fn big(app: &mut SlideApp, ui: &mut Ui, icon: Icon, label: &str, id: &str, p: Value) {
    let en = enabled(app, id);
    if big_button(ui, icon, label, en).on_hover_text(tip(id, label)).clicked() {
        run(app, id, p);
    }
}

fn small(app: &mut SlideApp, ui: &mut Ui, icon: Icon, label: &str, id: &str, p: Value) {
    let en = enabled(app, id);
    if small_button(ui, icon, label, en, false).on_hover_text(tip(id, label)).clicked() {
        run(app, id, p);
    }
}

fn toggle(app: &mut SlideApp, ui: &mut Ui, icon: Icon, label: &str, id: &str, on: bool) {
    let en = enabled(app, id);
    if icon_toggle(ui, icon, &tip(id, label), en, on).clicked() {
        run(app, id, json!({}));
    }
}

fn tip(id: &str, label: &str) -> String {
    let sc = deckcraft_engine::find_command(id).and_then(|c| c.shortcut).or_else(|| crate::UI_COMMANDS.iter().find(|c| c.0 == id).and_then(|c| c.2));
    match sc {
        Some(s) => format!("{label} ({})", pretty_shortcut(s)),
        None => label.to_string(),
    }
}

pub fn pretty_shortcut(s: &str) -> String {
    if cfg!(target_os = "macos") {
        s.replace("Cmd+", "⌘").replace("Shift+", "⇧").replace("Alt+", "⌥").replace("Ctrl+", "⌃")
    } else {
        s.replace("Cmd+", "Ctrl+")
    }
}

fn scheme(app: &SlideApp) -> deckcraft_color::ColorScheme {
    app.session.active().and_then(|d| d.doc.masters.first().map(|m| m.scheme())).unwrap_or_else(|| deckcraft_model::Theme::default().colors)
}

/// The formatting state at the selection (bold, size…).
pub fn fmt_state(app: &mut SlideApp) -> Value {
    if app.session.active().is_none_or(|d| d.selection.shapes.is_empty() && d.selection.text.is_none()) {
        return json!({});
    }
    app.session.execute("format.state", &json!({})).unwrap_or_default()
}

// ---------- Home ----------

fn clipboard_group(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let (main, arrow) = big_button_ex(ui, Icon::Paste, "Paste", enabled(app, "edit.paste"), true, false);
        if main.on_hover_text("Paste (⌘V)").clicked() {
            run(app, "edit.paste", json!({}));
        }
        if let Some(a) = arrow {
            egui::Popup::menu(&a).show(|ui| {
                if ui.button("Paste").clicked() {
                    run(app, "edit.paste", json!({}));
                }
                if ui.button("Paste and Match Formatting").clicked() {
                    run(app, "edit.pasteText", json!({}));
                }
                if ui.button("Paste Formatting").clicked() {
                    run(app, "format.painterApply", json!({}));
                }
            });
        }
        rows(ui, |ui| {
            small(app, ui, Icon::Cut, "Cut", "edit.cut", json!({}));
            small(app, ui, Icon::Copy, "Copy", "edit.copy", json!({}));
            let on = app.session.painter.is_some();
            let en = enabled(app, "format.painter");
            let r = small_button(ui, Icon::FormatPainter, "Format", en, on).on_hover_text("Format Painter (double-click to keep it on)");
            if r.double_clicked() {
                run(app, "format.painter", json!({"sticky": true}));
            } else if r.clicked() {
                if on {
                    app.session.painter = None;
                } else {
                    run(app, "format.painter", json!({}));
                }
            }
        });
    });
}

fn layout_menu(app: &mut SlideApp, ui: &mut Ui, cmd: &str) {
    let layouts: Vec<(String, deckcraft_model::LayoutId)> = app
        .session
        .active()
        .and_then(|d| d.doc.masters.first().map(|m| m.layouts.iter().map(|l| (l.name.clone(), l.id)).collect()))
        .unwrap_or_default();
    egui::Grid::new(("layouts", cmd)).spacing(vec2(6.0, 6.0)).show(ui, |ui| {
        for (k, (name, id)) in layouts.iter().enumerate() {
            let idx = app.session.active().and_then(|d| d.doc.masters.first().and_then(|m| m.layouts.iter().position(|l| l.id == *id)));
            let resp = ui.vertical(|ui| {
                let (rect, resp) = ui.allocate_exact_size(vec2(96.0, 54.0), Sense::click());
                if let (Some(d), Some(li)) = (app.session.active(), idx) {
                    let img = deckcraft_render::render_layout(
                        &d.doc,
                        0,
                        Some(li),
                        &deckcraft_render::RenderOpts {
                            scale: 192.0 / d.doc.slide_size.width.max(1.0),
                            edit: true,
                            size: Some((192, 108)),
                            ..Default::default()
                        },
                    );
                    let tex =
                        ui.ctx().load_texture(format!("layout-{li}"), crate::textures::to_color_image(&img, false), egui::TextureOptions::LINEAR);
                    ui.painter().image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                let t = Tokens::get(ui.ctx());
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::ZERO,
                    Stroke::new(if resp.hovered() { 2.0 } else { 1.0 }, if resp.hovered() { t.accent } else { t.border }),
                    egui::StrokeKind::Outside,
                );
                ui.add_sized(vec2(96.0, 14.0), egui::Label::new(egui::RichText::new(name).size(10.5)).truncate());
                resp
            });
            if resp.inner.clicked() {
                run(app, cmd, json!({"layout": id.0}));
                ui.close();
            }
            if k % 4 == 3 {
                ui.end_row();
            }
        }
    });
}

fn slides_group(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let (main, arrow) = big_button_ex(ui, Icon::NewSlide, "New\nSlide", enabled(app, "slide.new"), true, false);
        if main.on_hover_text("New Slide (⇧⌘N)").clicked() {
            run(app, "slide.new", json!({}));
        }
        if let Some(a) = arrow {
            egui::Popup::menu(&a).width(420.0).show(|ui| {
                layout_menu(app, ui, "slide.new");
                ui.separator();
                if ui.button("Duplicate Selected Slides").clicked() {
                    run(app, "slide.duplicate", json!({}));
                }
                if ui.button("Slides from Outline…").clicked() {
                    app.dialog = Some(crate::dialogs::Dialog::new("outline"));
                }
            });
        }
        rows(ui, |ui| {
            let (r, _) = small_button_ex(ui, Icon::Layout, "Layout", enabled(app, "slide.layout"), false, true);
            egui::Popup::menu(&r).width(420.0).show(|ui| layout_menu(app, ui, "slide.layout"));
            small(app, ui, Icon::ResetSlide, "Reset", "slide.reset", json!({}));
            let (r, _) = small_button_ex(ui, Icon::Section, "Section", enabled(app, "section.add"), false, true);
            egui::Popup::menu(&r).show(|ui| {
                if ui.button("Add Section").clicked() {
                    run(app, "section.add", json!({}));
                }
                if ui.button("Remove All Sections").clicked() {
                    run(app, "section.removeAll", json!({}));
                }
            });
        });
    });
}

pub fn font_families(app: &SlideApp) -> Vec<String> {
    let mut v = vec![];
    if let Some(m) = app.session.active().and_then(|d| d.doc.masters.first()) {
        v.push(m.theme.fonts.major.latin.clone());
        v.push(m.theme.fonts.minor.latin.clone());
    }
    v.extend(deckcraft_fonts::FontDb::global().families());
    v.dedup();
    v
}

fn font_group(app: &mut SlideApp, ui: &mut Ui, st: &Value) {
    let en = enabled(app, "format.bold");
    group(ui, |ui| {
        rows(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let font = st.get("font").and_then(Value::as_str).unwrap_or("").to_string();
                let size = st.get("size").and_then(Value::as_f64);
                let fams = font_families(app);
                let theme_fonts: Vec<String> = fams.iter().take(2).cloned().collect();
                dropdown(ui, "font", &font, 150.0, en, |ui| {
                    for (k, f) in fams.iter().enumerate() {
                        let label = if k == 0 && theme_fonts.len() == 2 {
                            format!("{f} (Headings)")
                        } else if k == 1 && theme_fonts.len() == 2 {
                            format!("{f} (Body)")
                        } else {
                            f.clone()
                        };
                        if ui.button(egui::RichText::new(label)).clicked() {
                            run(app, "format.font", json!({"family": f}));
                            ui.close();
                        }
                        if k == 1 {
                            ui.separator();
                        }
                    }
                });
                let size_text = size.map(|s| widgets::fmt_num(s, "")).unwrap_or_default();
                dropdown(ui, "size", &size_text, 52.0, en, |ui| {
                    for s in deckcraft_engine::cmd::format::SIZES.iter().chain([72.0, 80.0, 88.0, 96.0].iter()) {
                        if ui.button(widgets::fmt_num(*s, "")).clicked() {
                            run(app, "format.size", json!({"size": s}));
                            ui.close();
                        }
                    }
                });
                toggle(app, ui, Icon::FontGrow, "Increase Font Size", "format.grow", false);
                toggle(app, ui, Icon::FontShrink, "Decrease Font Size", "format.shrink", false);
                toggle(app, ui, Icon::ClearFormatting, "Clear All Formatting", "format.clear", false);
            });
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                let b = |k: &str| st.get(k).and_then(Value::as_bool).unwrap_or(false);
                toggle(app, ui, Icon::Bold, "Bold", "format.bold", b("bold"));
                toggle(app, ui, Icon::Italic, "Italic", "format.italic", b("italic"));
                toggle(app, ui, Icon::Underline, "Underline", "format.underline", b("underline"));
                toggle(app, ui, Icon::Strikethrough, "Strikethrough", "format.strikethrough", b("strike"));
                toggle(app, ui, Icon::Superscript, "Superscript", "format.superscript", b("superscript"));
                toggle(app, ui, Icon::Subscript, "Subscript", "format.subscript", b("subscript"));
                let r = icon_toggle(ui, Icon::CharSpacing, "Character Spacing", en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for (label, pt) in [("Very Tight", -3.0), ("Tight", -1.5), ("Normal", 0.0), ("Loose", 3.0), ("Very Loose", 6.0)] {
                        if ui.button(label).clicked() {
                            run(app, "format.spacing", json!({"pt": pt}));
                        }
                    }
                });
                let r = icon_toggle(ui, Icon::ChangeCase, "Change Case", en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for (label, mode) in [
                        ("Sentence case.", "sentence"),
                        ("lowercase", "lower"),
                        ("UPPERCASE", "upper"),
                        ("Capitalize Each Word", "title"),
                        ("tOGGLE cASE", "toggle"),
                    ] {
                        if ui.button(label).clicked() {
                            run(app, "format.case", json!({"mode": mode}));
                        }
                    }
                });
                let sc = scheme(app);
                let (m, a) = color_split(ui, Icon::Highlight, "Text Highlight Color", Color32::from_rgb(255, 230, 0), en);
                if m.clicked() {
                    run(app, "format.highlight", json!({"color": "#FFFF00"}));
                }
                widgets::color_popup(&a).show(|ui| {
                    if let Some(c) = widgets::color_grid(ui, &sc, Some("No Color")) {
                        run(app, "format.highlight", json!({"color": c.map(|c| serde_json::to_value(cref_param(&c)).unwrap_or_default())}));
                    }
                });
                let (m, a) = color_split(ui, Icon::FontColor, "Font Color", Color32::from_rgb(0xD8, 0x3F, 0x3F), en);
                if m.clicked() {
                    run(app, "format.color", json!({"color": "#C00000"}));
                }
                widgets::color_popup(&a).show(|ui| {
                    if let Some(Some(c)) = widgets::color_grid(ui, &sc, None) {
                        run(app, "format.color", json!({"color": cref_param(&c)}));
                    }
                });
            });
        });
    });
}

/// A ColorRef as a command colour parameter.
pub fn cref_param(c: &deckcraft_model::ColorRef) -> Value {
    match &c.base {
        deckcraft_model::ColorBase::Scheme { slot } => {
            let mut o = serde_json::Map::new();
            o.insert("scheme".into(), json!(slot.xml_name()));
            for m in &c.mods {
                if let Some(v) = m.value() {
                    o.insert(m.xml_name().into(), json!(v));
                }
            }
            Value::Object(o)
        }
        deckcraft_model::ColorBase::Rgb { rgb } if c.mods.is_empty() => json!(format!("#{}", rgb.hex())),
        deckcraft_model::ColorBase::Rgb { rgb } => {
            let mut o = serde_json::Map::new();
            o.insert("rgb".into(), json!(format!("#{}", rgb.hex())));
            for m in &c.mods {
                if let Some(v) = m.value() {
                    o.insert(m.xml_name().into(), json!(v));
                }
            }
            Value::Object(o)
        }
        _ => json!("#000000"),
    }
}

fn paragraph_group(app: &mut SlideApp, ui: &mut Ui, st: &Value) {
    let en = enabled(app, "format.bullets");
    group(ui, |ui| {
        rows(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                let b = |k: &str| st.get(k).and_then(Value::as_bool).unwrap_or(false);
                let (m, a) = small_button_ex(ui, Icon::Bullets, "", en, b("bullets"), true);
                if m.on_hover_text("Bullets").clicked() {
                    run(app, "format.bullets", json!({}));
                }
                if let Some(a) = a {
                    egui::Popup::menu(&a).show(|ui| {
                        if ui.button("None").clicked() {
                            run(app, "format.bullets", json!({"on": false}));
                        }
                        for ch in ["•", "○", "■", "□", "◆", "➢", "✓", "–", "★"] {
                            if ui.button(egui::RichText::new(format!("{ch}  Bullet")).size(14.0)).clicked() {
                                run(app, "format.bullets", json!({"on": true, "char": ch}));
                            }
                        }
                    });
                }
                let (m, a) = small_button_ex(ui, Icon::Numbering, "", en, b("numbering"), true);
                if m.on_hover_text("Numbering").clicked() {
                    run(app, "format.numbering", json!({}));
                }
                if let Some(a) = a {
                    egui::Popup::menu(&a).show(|ui| {
                        if ui.button("None").clicked() {
                            run(app, "format.numbering", json!({"on": false}));
                        }
                        for (label, sch) in [
                            ("1. 2. 3.", "arabicPeriod"),
                            ("1) 2) 3)", "arabicParenR"),
                            ("I. II. III.", "romanUcPeriod"),
                            ("i. ii. iii.", "romanLcPeriod"),
                            ("A. B. C.", "alphaUcPeriod"),
                            ("a) b) c)", "alphaLcParenR"),
                            ("a. b. c.", "alphaLcPeriod"),
                        ] {
                            if ui.button(label).clicked() {
                                run(app, "format.numbering", json!({"on": true, "scheme": sch}));
                            }
                        }
                    });
                }
                toggle(app, ui, Icon::DecreaseIndent, "Decrease List Level", "format.outdent", false);
                toggle(app, ui, Icon::IncreaseIndent, "Increase List Level", "format.indent", false);
                let r = icon_toggle(ui, Icon::LineSpacing, "Line Spacing", en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for v in [1.0, 1.5, 2.0, 2.5, 3.0] {
                        if ui.button(format!("{v:.1}")).clicked() {
                            run(app, "format.lineSpacing", json!({"lines": v}));
                        }
                    }
                    if ui.button("Line Spacing Options…").clicked() {
                        app.dialog = Some(crate::dialogs::Dialog::new("paragraph"));
                    }
                });
                let r = icon_toggle(ui, Icon::Columns, "Columns", en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for n in 1..=3 {
                        if ui.button(format!("{n} Column{}", if n > 1 { "s" } else { "" })).clicked() {
                            run(app, "format.columns", json!({"count": n, "spacing": 18}));
                        }
                    }
                });
            });
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                let al = st.get("align").and_then(Value::as_str).unwrap_or("");
                toggle(app, ui, Icon::AlignLeft, "Align Left", "format.alignLeft", al == "l");
                toggle(app, ui, Icon::AlignCenter, "Center", "format.alignCenter", al == "ctr");
                toggle(app, ui, Icon::AlignRight, "Align Right", "format.alignRight", al == "r");
                toggle(app, ui, Icon::Justify, "Justify", "format.justify", al == "just");
                let r = icon_toggle(ui, Icon::TextDirection, "Text Direction", en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for (l, d) in [
                        ("Horizontal", "horizontal"),
                        ("Rotate all text 90°", "rotate90"),
                        ("Rotate all text 270°", "rotate270"),
                        ("Stacked", "stacked"),
                    ] {
                        if ui.button(l).clicked() {
                            run(app, "format.direction", json!({"dir": d}));
                        }
                    }
                });
                let r = icon_toggle(ui, Icon::AlignText, "Align Text", en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for (l, a) in [("Top", "top"), ("Middle", "middle"), ("Bottom", "bottom")] {
                        if ui.button(l).clicked() {
                            run(app, "format.anchor", json!({"anchor": a}));
                        }
                    }
                });
            });
        });
    });
}

/// Shapes gallery popup body: every preset by category. Returns the chosen preset.
pub fn shapes_gallery(ui: &mut Ui) -> Option<&'static str> {
    let t = Tokens::get(ui.ctx());
    let mut out = None;
    ui.set_max_width(330.0);
    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        for cat in deckcraft_geom::preset::Category::ALL {
            ui.label(egui::RichText::new(cat.label()).font(theme::bold(12.0)));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
                for p in deckcraft_geom::preset::CATALOG.iter().filter(|p| p.category == cat) {
                    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, CornerRadius::same(3), t.hover);
                    }
                    paint_preset(ui.painter(), r.shrink(4.0), p.name, Color32::TRANSPARENT, t.text);
                    if resp.on_hover_text(p.label).clicked() {
                        out = Some(p.name);
                    }
                }
                if cat == deckcraft_geom::preset::Category::Lines {
                    for (name, label) in deckcraft_engine::tools::FREEFORM_TOOLS {
                        let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, CornerRadius::same(3), t.hover);
                        }
                        paint_freeform_icon(ui.painter(), r.shrink(5.0), name, t.text);
                        if resp.on_hover_text(label).clicked() {
                            out = Some(name);
                        }
                    }
                }
            });
            ui.add_space(4.0);
        }
    });
    if out.is_some() {
        ui.close();
    }
    out
}

/// Draw a preset's outline (gallery icons).
pub fn paint_preset(p: &egui::Painter, r: Rect, name: &str, fill: Color32, stroke: Color32) {
    let (w, h) = (r.width() as f64, r.height() as f64);
    let Some(g) = deckcraft_geom::preset::build(name, w, h, &[]) else { return };
    for sp in &g.paths {
        let mut pts: Vec<egui::Pos2> = vec![];
        let mut polys: Vec<(Vec<egui::Pos2>, bool)> = vec![];
        kurbo_flatten(&sp.path, |el| match el {
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
            if closed && fill != Color32::TRANSPARENT && sp.fill != deckcraft_geom::preset::FillMode::None {
                p.add(egui::Shape::Path(egui::epaint::PathShape { points: poly.clone(), closed: true, fill, stroke: Stroke::NONE.into() }));
            }
            if sp.stroke {
                p.add(egui::Shape::Path(egui::epaint::PathShape {
                    points: poly,
                    closed,
                    fill: Color32::TRANSPARENT,
                    stroke: Stroke::new(1.0, stroke).into(),
                }));
            }
        }
    }
}

enum Fl {
    Move(f64, f64),
    Line(f64, f64),
    Close,
}

fn kurbo_flatten(path: &deckcraft_geom::BezPath, mut f: impl FnMut(Fl)) {
    deckcraft_geom::preset_flatten(path, &mut |el| match el {
        deckcraft_geom::PathEl::MoveTo(p) => f(Fl::Move(p.x, p.y)),
        deckcraft_geom::PathEl::LineTo(p) => f(Fl::Line(p.x, p.y)),
        deckcraft_geom::PathEl::ClosePath => f(Fl::Close),
        _ => {}
    });
}

fn drawing_group(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let en = enabled(app, "shape.insert");
        let r = big_button(ui, Icon::Shapes, "Shapes", en).on_hover_text("Shapes");
        egui::Popup::menu(&r).show(|ui| {
            if let Some(p) = shapes_gallery(ui) {
                app.session.set_tool(deckcraft_engine::ToolKind::Shape { preset: p.to_string() });
            }
        });
        big(app, ui, Icon::TextBox, "Text Box", "insert.textBox", json!({}));
        let sel = enabled(app, "shape.fill");
        let r = big_button(ui, Icon::Arrange, "Arrange", sel);
        egui::Popup::menu(&r).show(|ui| arrange_menu(app, ui));
        let r = big_button(ui, Icon::QuickStyles, "Quick\nStyles", sel);
        egui::Popup::menu(&r).width(330.0).show(|ui| quick_styles(app, ui));
        rows(ui, |ui| {
            let sc = scheme(app);
            let (m, a) = small_button_ex(ui, Icon::ShapeFill, "Shape Fill", sel, false, true);
            if m.clicked() {
                run(app, "shape.fill", json!({"color": "accent1"}));
            }
            if let Some(a) = a {
                widgets::color_popup(&a).show(|ui| {
                    if let Some(c) = widgets::color_grid(ui, &sc, Some("No Fill")) {
                        match c {
                            Some(c) => run(app, "shape.fill", json!({"color": cref_param(&c)})),
                            None => run(app, "shape.fill", json!({"none": true})),
                        }
                    }
                });
            }
            let (m, a) = small_button_ex(ui, Icon::ShapeOutline, "Shape Outline", sel, false, true);
            if m.clicked() {
                run(app, "shape.line", json!({"color": "tx1", "width": 1}));
            }
            if let Some(a) = a {
                widgets::color_popup(&a).show(|ui| outline_menu(app, ui, &sc));
            }
            let (_, a) = small_button_ex(ui, Icon::ShapeEffects, "Shape Effects", sel, false, true);
            if let Some(a) = a {
                egui::Popup::menu(&a).show(|ui| effects_menu(app, ui));
            }
        });
    });
}

pub fn outline_menu(app: &mut SlideApp, ui: &mut Ui, sc: &deckcraft_color::ColorScheme) {
    if let Some(c) = widgets::color_grid(ui, sc, Some("No Outline")) {
        match c {
            Some(c) => run(app, "shape.line", json!({"color": cref_param(&c)})),
            None => run(app, "shape.line", json!({"none": true})),
        }
    }
    ui.menu_button("Weight", |ui| {
        for w in [0.25, 0.5, 0.75, 1.0, 1.5, 2.25, 3.0, 4.5, 6.0] {
            if ui.button(format!("{w} pt")).clicked() {
                run(app, "shape.line", json!({"width": w}));
                ui.close();
            }
        }
    });
    ui.menu_button("Dashes", |ui| {
        for (l, d) in [
            ("Solid", "solid"),
            ("Round Dot", "sysDot"),
            ("Square Dot", "sysDash"),
            ("Dash", "dash"),
            ("Dash Dot", "dashDot"),
            ("Long Dash", "lgDash"),
            ("Long Dash Dot", "lgDashDot"),
        ] {
            if ui.button(l).clicked() {
                run(app, "shape.line", json!({"dash": d}));
                ui.close();
            }
        }
    });
    ui.menu_button("Arrows", |ui| {
        for (l, h, tl) in [
            ("None", "none", "none"),
            ("Arrow at end", "none", "triangle"),
            ("Arrow at start", "triangle", "none"),
            ("Arrows at both ends", "triangle", "triangle"),
            ("Stealth end", "none", "stealth"),
            ("Oval ends", "oval", "oval"),
        ] {
            if ui.button(l).clicked() {
                run(app, "shape.line", json!({"head": h, "tail": tl}));
                ui.close();
            }
        }
    });
}

pub fn effects_menu(app: &mut SlideApp, ui: &mut Ui) {
    ui.menu_button("Shadow", |ui| {
        for (l, v) in [
            ("No Shadow", "none"),
            ("Outer", "outer"),
            ("Bottom", "bottom"),
            ("Centered", "center"),
            ("Inner", "inner"),
            ("Perspective", "perspective"),
        ] {
            if ui.button(l).clicked() {
                run(app, "shape.effects", json!({"shadow": v}));
            }
        }
    });
    ui.menu_button("Reflection", |ui| {
        for (l, v) in [("No Reflection", "none"), ("Tight", "tight"), ("Half", "half"), ("Full", "full")] {
            if ui.button(l).clicked() {
                run(app, "shape.effects", json!({"reflection": v}));
            }
        }
    });
    ui.menu_button("Glow", |ui| {
        if ui.button("No Glow").clicked() {
            run(app, "shape.effects", json!({"glow": null}));
        }
        for r in [5.0, 8.0, 11.0, 18.0] {
            if ui.button(format!("{r} pt glow")).clicked() {
                run(app, "shape.effects", json!({"glow": r}));
            }
        }
    });
    ui.menu_button("Soft Edges", |ui| {
        for r in [0.0, 1.0, 2.5, 5.0, 10.0, 25.0] {
            if ui.button(if r == 0.0 { "No Soft Edges".to_string() } else { format!("{r} pt") }).clicked() {
                run(app, "shape.effects", json!({"softEdges": r}));
            }
        }
    });
    ui.menu_button("Bevel", |ui| {
        for b in [
            "none",
            "circle",
            "relaxedInset",
            "cross",
            "coolSlant",
            "angle",
            "softRound",
            "convex",
            "slope",
            "divot",
            "riblet",
            "hardEdge",
            "artDeco",
        ] {
            if ui.button(b).clicked() {
                run(app, "shape.effects", json!({"bevel": b}));
            }
        }
    });
    if ui.button("Format Shape…").clicked() {
        let _ = app.run("view.pane", json!({"pane": "format", "tab": "effects"}));
    }
}

fn quick_styles(app: &mut SlideApp, ui: &mut Ui) {
    let sc = scheme(app);
    let slots = [
        deckcraft_color::SchemeSlot::Dk1,
        deckcraft_color::SchemeSlot::Accent1,
        deckcraft_color::SchemeSlot::Accent2,
        deckcraft_color::SchemeSlot::Accent3,
        deckcraft_color::SchemeSlot::Accent4,
        deckcraft_color::SchemeSlot::Accent5,
        deckcraft_color::SchemeSlot::Accent6,
    ];
    egui::Grid::new("quickstyles").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        for row in 0..6 {
            for (k, slot) in slots.iter().enumerate() {
                let c = theme::to_color32(sc.get(*slot));
                let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 26.0), Sense::click());
                let p = ui.painter();
                let (fill, stroke, text) = match row {
                    0 => (Color32::WHITE, c, Color32::BLACK),
                    2 => (c.gamma_multiply(0.35).to_opaque().lerp_to_gamma(Color32::WHITE, 0.6), Color32::TRANSPARENT, c),
                    5 => (c.lerp_to_gamma(Color32::BLACK, 0.25), Color32::WHITE, Color32::WHITE),
                    _ => (c, c.lerp_to_gamma(Color32::BLACK, 0.3), Color32::WHITE),
                };
                p.rect(rect.shrink(2.0), CornerRadius::same(3), fill, Stroke::new(1.5, stroke), egui::StrokeKind::Inside);
                p.text(rect.center(), Align2::CENTER_CENTER, "Abc", theme::font(11.0), text);
                if resp.hovered() {
                    p.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.5, Tokens::get(ui.ctx()).accent), egui::StrokeKind::Inside);
                }
                if resp.clicked() {
                    run(app, "shape.quickStyle", json!({"index": row * 7 + k}));
                    ui.close();
                }
            }
            ui.end_row();
        }
    });
}

pub fn arrange_menu(app: &mut SlideApp, ui: &mut Ui) {
    ui.label(egui::RichText::new("Order Objects").font(theme::bold(12.0)));
    for (l, id, ic) in [
        ("Bring to Front", "arrange.bringToFront", Icon::BringToFront),
        ("Send to Back", "arrange.sendToBack", Icon::SendToBack),
        ("Bring Forward", "arrange.bringForward", Icon::BringForward),
        ("Send Backward", "arrange.sendBackward", Icon::SendBackward),
    ] {
        menu_item(app, ui, ic, l, id, json!({}));
    }
    ui.separator();
    ui.label(egui::RichText::new("Group Objects").font(theme::bold(12.0)));
    menu_item(app, ui, Icon::Group, "Group", "arrange.group", json!({}));
    menu_item(app, ui, Icon::Ungroup, "Ungroup", "arrange.ungroup", json!({}));
    menu_item(app, ui, Icon::Group, "Regroup", "arrange.regroup", json!({}));
    ui.separator();
    ui.label(egui::RichText::new("Position Objects").font(theme::bold(12.0)));
    ui.menu_button("Align", |ui| align_menu(app, ui));
    ui.menu_button("Rotate", |ui| {
        menu_item(app, ui, Icon::RotateRight, "Rotate Right 90°", "arrange.rotateRight", json!({}));
        menu_item(app, ui, Icon::RotateLeft, "Rotate Left 90°", "arrange.rotateLeft", json!({}));
        menu_item(app, ui, Icon::FlipV, "Flip Vertical", "arrange.flipVertical", json!({}));
        menu_item(app, ui, Icon::FlipH, "Flip Horizontal", "arrange.flipHorizontal", json!({}));
    });
    if ui.button("Selection Pane…").clicked() {
        let _ = app.run("view.pane", json!({"pane": "selection"}));
        ui.close();
    }
}

pub fn align_menu(app: &mut SlideApp, ui: &mut Ui) {
    for (l, e, ic) in [
        ("Align Left", "left", Icon::AlignObjLeft),
        ("Align Center", "center", Icon::AlignObjCenter),
        ("Align Right", "right", Icon::AlignObjRight),
        ("Align Top", "top", Icon::AlignObjTop),
        ("Align Middle", "middle", Icon::AlignObjMiddle),
        ("Align Bottom", "bottom", Icon::AlignObjBottom),
    ] {
        menu_item(app, ui, ic, l, "arrange.align", json!({"edge": e}));
    }
    ui.separator();
    menu_item(app, ui, Icon::DistributeH, "Distribute Horizontally", "arrange.distribute", json!({"dir": "horizontal"}));
    menu_item(app, ui, Icon::DistributeV, "Distribute Vertically", "arrange.distribute", json!({"dir": "vertical"}));
}

pub fn menu_item(app: &mut SlideApp, ui: &mut Ui, icon: Icon, label: &str, id: &str, p: Value) {
    let en = enabled(app, id);
    let r = small_button(ui, icon, label, en, false);
    if r.clicked() {
        run(app, id, p);
        ui.close();
    }
}

fn home(app: &mut SlideApp, ui: &mut Ui) {
    let st = fmt_state(app);
    clipboard_group(app, ui);
    slides_group(app, ui);
    font_group(app, ui, &st);
    paragraph_group(app, ui, &st);
    group(ui, |ui| {
        big(app, ui, Icon::Picture, "Picture", "app.insertPictureDialog", json!({}));
    });
    drawing_group(app, ui);
    group(ui, |ui| {
        big(app, ui, Icon::DesignIdeas, "Design\nIdeas", "view.pane", json!({"pane": "designer"}));
    });
}

// ---------- Insert ----------

fn insert(app: &mut SlideApp, ui: &mut Ui) {
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
            if ui.button("Insert Table…").clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("table"));
                ui.close();
            }
        });
    });
    group(ui, |ui| {
        big(app, ui, Icon::Picture, "Pictures", "app.insertPictureDialog", json!({}));
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
        let r = big_button(ui, Icon::Icons, "Icons", enabled(app, "shape.insert"));
        egui::Popup::menu(&r).width(260.0).show(|ui| icon_library(app, ui));
        big(app, ui, Icon::SmartArt, "SmartArt", "app.dialog", json!({"id": "smartart"}));
        let r = big_button(ui, Icon::Chart, "Chart", enabled(app, "insert.chart"));
        egui::Popup::menu(&r).show(|ui| {
            for k in deckcraft_model::chart::ChartType::MAIN {
                if ui.button(k.label()).clicked() {
                    run(app, "insert.chart", json!({"type": format!("{k:?}")}));
                }
            }
        });
    });
    group(ui, |ui| {
        big(app, ui, Icon::Link, "Link", "app.dialog", json!({"id": "hyperlink"}));
        let r = big_button(ui, Icon::Action, "Action", enabled(app, "insert.actionButton"));
        egui::Popup::menu(&r).show(|ui| {
            for k in ["back", "forward", "beginning", "end", "home", "information", "return", "movie", "document", "sound", "help", "custom"] {
                if ui.button(format!("Action Button: {k}")).clicked() {
                    run(app, "insert.actionButton", json!({"kind": k}));
                }
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
    let id = ui.id().with("tgrid");
    let mut hover: (usize, usize) = ui.data_mut(|d| d.get_temp(id).unwrap_or((0, 0)));
    let cell = 16.0;
    let label = if hover.0 > 0 { format!("{}x{} Table", hover.1, hover.0) } else { "Insert Table".into() };
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

/// A small built-in icon library: our own simple pictograms built from preset shapes.
fn icon_library(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let items: [(&str, &str); 24] = [
        ("star5", "Star"),
        ("heart", "Heart"),
        ("lightningBolt", "Lightning"),
        ("sun", "Sun"),
        ("moon", "Moon"),
        ("cloud", "Cloud"),
        ("smileyFace", "Smile"),
        ("noSmoking", "Not allowed"),
        ("mathPlus", "Plus"),
        ("mathMultiply", "Multiply"),
        ("mathEqual", "Equals"),
        ("chevron", "Chevron"),
        ("rightArrow", "Arrow"),
        ("quadArrow", "Move"),
        ("uturnArrow", "Return"),
        ("flowChartDecision", "Decision"),
        ("flowChartMagneticDisk", "Database"),
        ("flowChartDocument", "Document"),
        ("wedgeRoundRectCallout", "Speech"),
        ("cloudCallout", "Thought"),
        ("teardrop", "Pin"),
        ("donut", "Ring"),
        ("irregularSeal1", "Burst"),
        ("ribbon2", "Banner"),
    ];
    ui.horizontal_wrapped(|ui| {
        for (preset, label) in items {
            let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(4), t.hover);
            }
            paint_preset(ui.painter(), r.shrink(6.0), preset, t.text, t.text);
            if resp.on_hover_text(label).clicked() {
                let size = app.session.active().map(|d| d.doc.slide_size).unwrap_or(deckcraft_model::defaults::WIDE);
                let _ = app.run("shape.insert", json!({"preset": preset, "rect": [size.width / 2.0 - 48.0, size.height / 2.0 - 48.0, 96, 96]}));
                let _ = app.run("shape.fill", json!({"color": "tx1"}));
                let _ = app.run("shape.line", json!({"none": true}));
                ui.close();
            }
        }
    });
}

// ---------- Draw ----------

fn draw(app: &mut SlideApp, ui: &mut Ui) {
    use deckcraft_engine::ToolKind;
    let cur = app.session.tool.kind.clone();
    group(ui, |ui| {
        let sel = matches!(cur, ToolKind::Select);
        if big_button_ex(ui, Icon::LassoSelect, "Select", true, false, sel).0.clicked() {
            app.session.set_tool(ToolKind::Select);
        }
        let er = matches!(&cur, ToolKind::Ink { mode, .. } if mode == "eraser");
        if big_button_ex(ui, Icon::Eraser, "Eraser", true, false, er).0.clicked() {
            app.session.set_tool(ToolKind::Ink { mode: "eraser".into(), color: deckcraft_color::Rgba::BLACK, width: 2.0 });
        }
    });
    group(ui, |ui| {
        let pens: [(Icon, &str, &str, deckcraft_color::Rgba, f64); 5] = [
            (Icon::Pen, "Black Pen", "pen", deckcraft_color::Rgba::rgb(20, 20, 20), 1.5),
            (Icon::Pen, "Red Pen", "pen", deckcraft_color::Rgba::rgb(0xD8, 0x3F, 0x3F), 1.5),
            (Icon::Pen, "Blue Pen", "pen", deckcraft_color::Rgba::rgb(0x2E, 0x6F, 0xD8), 1.5),
            (Icon::Pencil, "Pencil", "pen", deckcraft_color::Rgba::rgb(90, 90, 90), 1.0),
            (Icon::HighlighterPen, "Highlighter", "highlighter", deckcraft_color::Rgba::rgb(255, 230, 0), 10.0),
        ];
        for (icon, label, mode, color, width) in pens {
            let on = matches!(&cur, ToolKind::Ink { mode: m, color: c, .. } if m == mode && *c == color);
            if big_button_ex(ui, icon, label, true, false, on).0.clicked() {
                app.session.set_tool(ToolKind::Ink { mode: mode.into(), color, width });
            }
        }
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::InkToShape, "Ink to\nShape", false);
        let _ = r.on_hover_text("Ink to Shape (coming soon)");
        let r = big_button(ui, Icon::InkToText, "Ink to\nText", false);
        let _ = r.on_hover_text("Ink to Text (coming soon)");
        let r = big_button(ui, Icon::InkToMath, "Ink to\nMath", false);
        let _ = r.on_hover_text("Ink to Math (coming soon)");
    });
}

// ---------- Design ----------

fn design(app: &mut SlideApp, ui: &mut Ui) {
    let themes = deckcraft_model::theme::builtin_themes();
    let cur = app.session.active().and_then(|d| d.doc.masters.first().map(|m| m.theme.name.clone())).unwrap_or_default();
    group(ui, |ui| {
        let sel = themes.iter().position(|th| th.name == cur);
        let names: Vec<String> = themes.iter().map(|t| t.name.clone()).collect();
        if let Some(i) = widgets::gallery(
            ui,
            themes.len(),
            vec2(84.0, 56.0),
            sel,
            |p, r, i| {
                if let Some(th) = themes.get(i) {
                    paint_theme_tile(p, r, th);
                }
            },
            |i| names.get(i).cloned().unwrap_or_default(),
        ) && let Some(n) = names.get(i)
        {
            run(app, "design.theme", json!({"name": n}));
        }
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Colors, "Colors", true);
        egui::Popup::menu(&r).width(240.0).show(|ui| {
            for cs in deckcraft_model::theme::builtin_color_schemes() {
                ui.horizontal(|ui| {
                    let (rect, resp) = ui.allocate_exact_size(vec2(220.0, 20.0), Sense::click());
                    let p = ui.painter();
                    for (k, c) in cs.colors.iter().take(10).skip(2).enumerate() {
                        p.rect_filled(
                            Rect::from_min_size(rect.min + vec2(k as f32 * 12.0, 3.0), vec2(11.0, 14.0)),
                            CornerRadius::ZERO,
                            theme::to_color32(*c),
                        );
                    }
                    p.text(rect.min + vec2(104.0, 10.0), Align2::LEFT_CENTER, &cs.name, theme::font(12.0), Tokens::get(ui.ctx()).text);
                    if resp.clicked() {
                        run(app, "design.colors", json!({"name": cs.name}));
                        ui.close();
                    }
                });
            }
        });
        let r = big_button(ui, Icon::Fonts, "Fonts", true);
        egui::Popup::menu(&r).width(220.0).show(|ui| {
            for fs in deckcraft_model::theme::builtin_font_schemes() {
                if ui.button(format!("{}\n   {} / {}", fs.name, fs.major.latin, fs.minor.latin)).clicked() {
                    run(app, "design.fonts", json!({"name": fs.name}));
                }
            }
        });
        let r = big_button(ui, Icon::BackgroundStyles, "Background\nStyles", true);
        egui::Popup::menu(&r).show(|ui| {
            egui::Grid::new("bgstyles").show(ui, |ui| {
                let sc = scheme(app);
                for i in 1..=12 {
                    let slots = [
                        deckcraft_color::SchemeSlot::Lt1,
                        deckcraft_color::SchemeSlot::Lt2,
                        deckcraft_color::SchemeSlot::Dk2,
                        deckcraft_color::SchemeSlot::Dk1,
                    ];
                    let base = theme::to_color32(sc.get(slots[(i - 1) % 4]));
                    let shade = match (i - 1) / 4 {
                        0 => base,
                        1 => base.lerp_to_gamma(Color32::GRAY, 0.15),
                        _ => base.lerp_to_gamma(Color32::BLACK, 0.12),
                    };
                    let (rect, resp) = ui.allocate_exact_size(vec2(48.0, 28.0), Sense::click());
                    ui.painter().rect(rect, CornerRadius::same(2), shade, Stroke::new(1.0, Tokens::get(ui.ctx()).border), egui::StrokeKind::Inside);
                    if resp.on_hover_text(format!("Style {i}")).clicked() {
                        run(app, "design.background", json!({"style": i, "all": true}));
                        ui.close();
                    }
                    if i % 4 == 0 {
                        ui.end_row();
                    }
                }
            });
            if ui.button("Reset Slide Background").clicked() {
                run(app, "design.background", json!({"reset": true}));
            }
        });
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::SlideSize, "Slide\nSize", true);
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("Standard (4:3)").clicked() {
                run(app, "design.slideSize", json!({"preset": "standard"}));
            }
            if ui.button("Widescreen (16:9)").clicked() {
                run(app, "design.slideSize", json!({"preset": "widescreen"}));
            }
            if ui.button("Page Setup…").clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("slideSize"));
            }
        });
        big(app, ui, Icon::FormatBackground, "Format\nBackground", "view.pane", json!({"pane": "background"}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::DesignIdeas, "Design\nIdeas", "view.pane", json!({"pane": "designer"}));
    });
}

pub fn paint_theme_tile(p: &egui::Painter, r: Rect, th: &deckcraft_model::Theme) {
    let bg = theme::to_color32(th.colors.get(deckcraft_color::SchemeSlot::Lt1));
    let fg = theme::to_color32(th.colors.get(deckcraft_color::SchemeSlot::Dk1));
    p.rect(r, CornerRadius::same(2), bg, Stroke::new(1.0, Color32::from_gray(200)), egui::StrokeKind::Inside);
    p.text(r.min + vec2(8.0, r.height() * 0.42), Align2::LEFT_CENTER, "Aa", theme::font(r.height() * 0.38), fg);
    let w = (r.width() - 16.0) / 6.0;
    for k in 0..6 {
        let c = theme::to_color32(th.colors.colors.get(4 + k).copied().unwrap_or(deckcraft_color::Rgba::BLACK));
        p.rect_filled(Rect::from_min_size(pos2(r.min.x + 8.0 + k as f32 * w, r.max.y - 11.0), vec2(w - 1.5, 5.0)), CornerRadius::ZERO, c);
    }
}

// ---------- Transitions ----------

fn transitions(app: &mut SlideApp, ui: &mut Ui) {
    let cur = app.session.active().and_then(|d| d.current_slide().and_then(|s| s.transition.clone()));
    let kind = cur.as_ref().map(|t| t.kind.clone()).unwrap_or_else(|| "none".into());
    group(ui, |ui| {
        if big_button(ui, Icon::Preview, "Preview", true).clicked() {
            let from = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            app.start_show(from, true);
            if let Some(s) = app.show.as_mut() {
                s.preview_only = true;
            }
        }
    });
    group(ui, |ui| {
        let list = deckcraft_model::anim::TRANSITIONS;
        let shown: Vec<_> = list.iter().take(12).collect();
        let sel = shown.iter().position(|x| x.0 == kind);
        if let Some(i) = widgets::gallery(
            ui,
            shown.len(),
            vec2(54.0, 56.0),
            sel,
            |p, r, i| paint_transition_tile(p, r, shown.get(i).map(|x| (x.0, x.1)).unwrap_or(("", ""))),
            |i| shown.get(i).map(|x| x.1.to_string()).unwrap_or_default(),
        ) && let Some(x) = shown.get(i)
        {
            run(app, "transition.set", json!({"kind": x.0}));
        }
        let more = widgets::drop_button(ui, "", vec2(14.0, 52.0), true);
        egui::Popup::menu(&more).width(480.0).show(|ui| {
            for cat in ["Subtle", "Exciting", "Dynamic Content"] {
                ui.label(egui::RichText::new(cat).font(theme::bold(12.0)));
                ui.horizontal_wrapped(|ui| {
                    for x in list.iter().filter(|x| x.2 == cat) {
                        let (r, resp) = ui.allocate_exact_size(vec2(56.0, 58.0), Sense::click());
                        if resp.hovered() || x.0 == kind {
                            ui.painter().rect_filled(r, CornerRadius::same(4), Tokens::get(ui.ctx()).hover);
                        }
                        paint_transition_tile(ui.painter(), r.shrink(3.0), (x.0, x.1));
                        if resp.clicked() {
                            run(app, "transition.set", json!({"kind": x.0}));
                            ui.close();
                        }
                    }
                });
            }
        });
    });
    group(ui, |ui| {
        let opts: &[&str] = deckcraft_model::anim::TRANSITIONS.iter().find(|x| x.0 == kind).map(|x| x.4).unwrap_or(&[]);
        let r = big_button(ui, Icon::EffectOptions, "Effect\nOptions", !opts.is_empty());
        egui::Popup::menu(&r).show(|ui| {
            for o in opts {
                if ui.button(option_label(o)).clicked() {
                    run(app, "transition.options", json!({"option": o}));
                }
            }
        });
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Duration:");
                let d = cur.as_ref().map(|t| t.duration_ms as f64 / 1000.0).unwrap_or(1.0);
                if let Some(v) = widgets::spinner(ui, "tdur", d, 0.25, "", 46.0) {
                    run(app, "transition.timing", json!({"duration": (v.max(0.0) * 1000.0).round() as u64}));
                }
            });
            ui.horizontal(|ui| {
                let mut click = cur.as_ref().is_none_or(|t| t.advance_on_click);
                if ui.checkbox(&mut click, "On Mouse Click").changed() {
                    run(app, "transition.timing", json!({"onClick": click}));
                }
            });
            ui.horizontal(|ui| {
                let after = cur.as_ref().and_then(|t| t.advance_after_ms);
                let mut on = after.is_some();
                if ui.checkbox(&mut on, "After:").changed() {
                    run(app, "transition.timing", json!({"after": if on { json!(after.unwrap_or(0)) } else { Value::Null }}));
                }
                if let Some(v) = widgets::spinner(ui, "tafter", after.unwrap_or(0) as f64 / 1000.0, 1.0, "", 46.0) {
                    run(app, "transition.timing", json!({"after": (v.max(0.0) * 1000.0).round() as u64}));
                }
            });
        });
        big(app, ui, Icon::ApplyToAll, "Apply\nTo All", "transition.applyAll", json!({}));
    });
}

fn option_label(o: &str) -> String {
    match o {
        "l" => "From Left".into(),
        "r" => "From Right".into(),
        "u" => "From Top".into(),
        "d" => "From Bottom".into(),
        "horz" => "Horizontal".into(),
        "vert" => "Vertical".into(),
        "in" => "In".into(),
        "out" => "Out".into(),
        other => {
            let mut s = String::new();
            for (i, c) in other.chars().enumerate() {
                if i == 0 {
                    s.extend(c.to_uppercase());
                } else if c.is_uppercase() {
                    s.push(' ');
                    s.push(c);
                } else {
                    s.push(c);
                }
            }
            s
        }
    }
}

pub fn paint_transition_tile(p: &egui::Painter, r: Rect, (id, label): (&str, &str)) {
    let t = Tokens::light();
    let pic = Rect::from_min_size(r.min + vec2(4.0, 2.0), vec2(r.width() - 8.0, r.height() - 18.0));
    let blue = Color32::from_rgb(0x2E, 0x6F, 0xD8);
    let light = Color32::from_rgb(0xBF, 0xD4, 0xF5);
    p.rect_filled(pic, CornerRadius::same(2), light);
    match id {
        "none" => {
            p.rect_filled(pic, CornerRadius::same(2), Color32::from_rgb(0xE6, 0xEC, 0xF5));
        }
        "fade" | "dissolve" | "flash" => {
            p.rect_filled(pic, CornerRadius::same(2), blue.gamma_multiply(0.5));
        }
        "push" | "cover" | "uncover" | "pan" | "conveyor" => {
            p.rect_filled(Rect::from_min_max(pos2(pic.center().x, pic.min.y), pic.max), CornerRadius::same(2), blue);
            p.arrow(pos2(pic.center().x + 10.0, pic.center().y), vec2(-12.0, 0.0), Stroke::new(1.5, Color32::WHITE));
        }
        "wipe" | "reveal" => {
            p.rect_filled(Rect::from_min_max(pic.min, pos2(pic.center().x, pic.max.y)), CornerRadius::same(2), blue);
        }
        "split" | "doors" | "window" | "curtains" => {
            p.rect_filled(
                Rect::from_min_max(pos2(pic.min.x + pic.width() * 0.3, pic.min.y), pos2(pic.max.x - pic.width() * 0.3, pic.max.y)),
                CornerRadius::ZERO,
                blue,
            );
        }
        "randomBar" | "blinds" | "comb" | "shred" => {
            for k in 0..5 {
                let x = pic.min.x + k as f32 * pic.width() / 5.0;
                p.rect_filled(Rect::from_min_size(pos2(x, pic.min.y), vec2(pic.width() / 10.0, pic.height())), CornerRadius::ZERO, blue);
            }
        }
        "checker" | "honeycomb" | "glitter" | "fracture" => {
            for k in 0..12 {
                if k % 2 == (k / 4) % 2 {
                    let x = pic.min.x + (k % 4) as f32 * pic.width() / 4.0;
                    let y = pic.min.y + (k / 4) as f32 * pic.height() / 3.0;
                    p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(pic.width() / 4.0, pic.height() / 3.0)), CornerRadius::ZERO, blue);
                }
            }
        }
        "shape" | "zoom" | "ripple" | "vortex" | "clock" => {
            p.circle_filled(pic.center(), pic.height() * 0.38, blue);
        }
        "morph" => {
            p.circle_filled(pos2(pic.min.x + pic.width() * 0.3, pic.center().y), pic.height() * 0.25, blue);
            p.rect_filled(
                Rect::from_center_size(pos2(pic.max.x - pic.width() * 0.3, pic.center().y), vec2(pic.height() * 0.45, pic.height() * 0.45)),
                CornerRadius::same(2),
                blue.gamma_multiply(0.7),
            );
        }
        _ => {
            // Fake 3-D: a tilted card.
            let pts = vec![
                pos2(pic.min.x + 6.0, pic.min.y + 4.0),
                pos2(pic.max.x - 3.0, pic.min.y + 1.0),
                pos2(pic.max.x - 3.0, pic.max.y - 1.0),
                pos2(pic.min.x + 6.0, pic.max.y - 4.0),
            ];
            p.add(egui::Shape::convex_polygon(pts, blue, Stroke::NONE));
        }
    }
    p.rect_stroke(pic, CornerRadius::same(2), Stroke::new(1.0, Color32::from_rgb(0x9B, 0xB4, 0xDB)), egui::StrokeKind::Inside);
    p.text(pos2(r.center().x, r.max.y - 7.0), Align2::CENTER_CENTER, label, theme::font(10.0), t.text);
}

// ---------- Animations ----------

fn animations(app: &mut SlideApp, ui: &mut Ui) {
    let has_sel = app.session.active().is_some_and(|d| !d.selection.shapes.is_empty());
    let cur: Option<deckcraft_model::Animation> = app.session.active().and_then(|d| {
        let s = d.current_slide()?;
        let id = d.selection.shapes.first()?;
        s.animations.iter().find(|a| a.shape == *id).cloned()
    });
    group(ui, |ui| {
        if big_button(ui, Icon::Preview, "Preview", true).clicked() {
            let from = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            app.start_show(from, true);
            if let Some(s) = app.show.as_mut() {
                s.preview_only = true;
                s.autoplay = true;
            }
        }
    });
    group(ui, |ui| {
        let quick: Vec<_> = deckcraft_model::anim::ANIMATIONS
            .iter()
            .filter(|a| {
                matches!(
                    a.0,
                    "appear"
                        | "fade"
                        | "fly"
                        | "float"
                        | "split"
                        | "wipe"
                        | "zoom"
                        | "pulse"
                        | "spin"
                        | "growShrink"
                        | "fadeOut"
                        | "flyOut"
                        | "lines"
                )
            })
            .collect();
        let sel = cur.as_ref().and_then(|c| quick.iter().position(|a| a.0 == c.effect));
        if let Some(i) = widgets::gallery(
            ui,
            quick.len(),
            vec2(50.0, 56.0),
            sel,
            |p, r, i| {
                if let Some(a) = quick.get(i) {
                    paint_anim_tile(p, r, a.1, a.2, has_sel);
                }
            },
            |i| quick.get(i).map(|a| a.1.to_string()).unwrap_or_default(),
        ) && let Some(a) = quick.get(i)
        {
            run(app, "animation.set", json!({"effect": a.0, "class": a.2.xml()}));
        }
        let more = widgets::drop_button(ui, "", vec2(14.0, 52.0), has_sel);
        egui::Popup::menu(&more).width(420.0).show(|ui| {
            for (cls, label) in
                [(AnimClass::Entrance, "Entrance"), (AnimClass::Emphasis, "Emphasis"), (AnimClass::Exit, "Exit"), (AnimClass::Path, "Motion Paths")]
            {
                ui.label(egui::RichText::new(label).font(theme::bold(12.0)));
                ui.horizontal_wrapped(|ui| {
                    for a in deckcraft_model::anim::ANIMATIONS.iter().filter(|a| a.2 == cls) {
                        let (r, resp) = ui.allocate_exact_size(vec2(56.0, 54.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, CornerRadius::same(4), Tokens::get(ui.ctx()).hover);
                        }
                        paint_anim_tile(ui.painter(), r.shrink(2.0), a.1, a.2, true);
                        if resp.clicked() {
                            run(app, "animation.set", json!({"effect": a.0, "class": a.2.xml()}));
                            ui.close();
                        }
                    }
                });
            }
        });
        let opts: &[&str] = cur.as_ref().and_then(|c| deckcraft_model::anim::animation_info(&c.effect, c.class)).map(|x| x.5).unwrap_or(&[]);
        let r = big_button(ui, Icon::EffectOptions, "Effect\nOptions", !opts.is_empty());
        egui::Popup::menu(&r).show(|ui| {
            for o in opts {
                if ui.button(option_label(o)).clicked() {
                    run(app, "animation.options", json!({"option": o}));
                }
            }
            ui.separator();
            for (l, b) in [("As One Object", "asOne"), ("By Paragraph", "byParagraph")] {
                if ui.button(l).clicked() {
                    run(app, "animation.options", json!({"textBuild": b}));
                }
            }
        });
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::AddAnimation, "Add\nAnimation", has_sel);
        egui::Popup::menu(&r).width(260.0).show(|ui| {
            for a in deckcraft_model::anim::ANIMATIONS.iter().filter(|a| a.2 != AnimClass::Media) {
                if ui
                    .button(format!(
                        "{} — {}",
                        a.1,
                        match a.2 {
                            AnimClass::Entrance => "Entrance",
                            AnimClass::Emphasis => "Emphasis",
                            AnimClass::Exit => "Exit",
                            _ => "Motion",
                        }
                    ))
                    .clicked()
                {
                    run(app, "animation.add", json!({"effect": a.0, "class": a.2.xml()}));
                }
            }
        });
        big(app, ui, Icon::AnimationPane, "Animation\nPane", "view.pane", json!({"pane": "animation"}));
        let r = big_button(ui, Icon::Trigger, "Trigger", cur.is_some());
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("On Click of… (none)").clicked() {
                run(app, "animation.timing", json!({"trigger": null}));
            }
            let shapes: Vec<(u32, String)> =
                app.session.active().map(|d| d.shapes().iter().map(|s| (s.id.0, s.name.clone())).collect()).unwrap_or_default();
            for (id, name) in shapes {
                if ui.button(format!("On Click of {name}")).clicked() {
                    run(app, "animation.timing", json!({"trigger": id}));
                }
            }
        });
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Start:");
                let s = cur.as_ref().map(|c| match c.start {
                    deckcraft_model::AnimStart::OnClick => "On Click",
                    deckcraft_model::AnimStart::WithPrevious => "With Previous",
                    deckcraft_model::AnimStart::AfterPrevious => "After Previous",
                });
                dropdown(ui, "astart", s.unwrap_or(""), 112.0, cur.is_some(), |ui| {
                    for (l, v) in [("On Click", "onClick"), ("With Previous", "withPrevious"), ("After Previous", "afterPrevious")] {
                        if ui.button(l).clicked() {
                            run(app, "animation.timing", json!({"start": v}));
                            ui.close();
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Duration:");
                if let Some(v) = widgets::spinner(ui, "adur", cur.as_ref().map(|c| c.duration_ms as f64 / 1000.0).unwrap_or(0.5), 0.25, "", 46.0) {
                    run(app, "animation.timing", json!({"duration": (v.max(0.0) * 1000.0).round() as u64}));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Delay:");
                if let Some(v) = widgets::spinner(ui, "adelay", cur.as_ref().map(|c| c.delay_ms as f64 / 1000.0).unwrap_or(0.0), 0.25, "", 46.0) {
                    run(app, "animation.timing", json!({"delay": (v.max(0.0) * 1000.0).round() as u64}));
                }
            });
        });
    });
}

pub fn paint_anim_tile(p: &egui::Painter, r: Rect, label: &str, class: AnimClass, enabled: bool) {
    let t = Tokens::light();
    let icon = match class {
        AnimClass::Entrance => Icon::AnimEntrance,
        AnimClass::Emphasis => Icon::AnimEmphasis,
        AnimClass::Exit => Icon::AnimExit,
        _ => Icon::AnimPath,
    };
    icons::paint(p, Rect::from_center_size(pos2(r.center().x, r.min.y + 18.0), vec2(28.0, 28.0)), icon, t.text, !enabled);
    p.text(pos2(r.center().x, r.max.y - 8.0), Align2::CENTER_CENTER, label, theme::font(10.0), if enabled { t.text } else { t.text_faint });
}

// ---------- Slide Show / Record / Review / View ----------

fn slide_show(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        if big_button(ui, Icon::PlayFromStart, "Play from\nStart", true).clicked() {
            app.start_show(0, false);
        }
        if big_button(ui, Icon::PlayFromCurrent, "Play from\nCurrent Slide", true).clicked() {
            let i = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            app.start_show(i, false);
        }
        let on = app.ui.presenter_view;
        if big_button_ex(ui, Icon::PresenterView, "Presenter\nView", true, false, on).0.clicked() {
            app.ui.presenter_view = !on;
        }
        big(app, ui, Icon::CustomShow, "Custom\nShow", "app.dialog", json!({"id": "customShow"}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::SetUpShow, "Set Up\nSlide Show", "app.dialog", json!({"id": "setupShow"}));
        let hidden = app.session.active().and_then(|d| d.current_slide().map(|s| s.hidden)).unwrap_or(false);
        if big_button_ex(ui, Icon::HideSlide, "Hide\nSlide", true, false, hidden).0.clicked() {
            run(app, "slide.hide", json!({}));
        }
    });
    group(ui, |ui| {
        if big_button(ui, Icon::RehearseTimings, "Rehearse\nTimings", true).clicked() {
            app.start_show(0, false);
            if let Some(s) = app.show.as_mut() {
                s.rehearse = true;
            }
        }
        let r = big_button(ui, Icon::Record, "Record", false);
        let _ = r.on_hover_text("Recording narration needs audio capture (coming soon)");
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            let mut use_t = app.session.active().is_none_or(|d| d.doc.show.use_timings);
            if ui.checkbox(&mut use_t, "Use Timings").changed() {
                run(app, "show.setup", json!({"useTimings": use_t}));
            }
            let mut narr = app.session.active().is_none_or(|d| !d.doc.show.without_narration);
            if ui.checkbox(&mut narr, "Play Narrations").changed() {
                run(app, "show.setup", json!({"noNarration": !narr}));
            }
            let mut mc = app.session.active().is_none_or(|d| d.doc.show.show_media_controls);
            let _ = ui.checkbox(&mut mc, "Show Media Controls");
        });
    });
}

fn record(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let r = big_button(ui, Icon::Cameo, "Cameo", false);
        let _ = r.on_hover_text("Camera capture is not available yet");
        if big_button(ui, Icon::PlayFromStart, "From\nBeginning", true).clicked() {
            app.start_show(0, false);
            if let Some(s) = app.show.as_mut() {
                s.rehearse = true;
            }
        }
        if big_button(ui, Icon::PlayFromCurrent, "From\nCurrent Slide", true).clicked() {
            let i = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            app.start_show(i, false);
            if let Some(s) = app.show.as_mut() {
                s.rehearse = true;
            }
        }
    });
    group(ui, |ui| {
        if big_button(ui, Icon::Help, "Learn\nMore", true).clicked() {
            app.open_url(deckcraft_engine::links::APP_PAGE);
        }
    });
}

fn review(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        big(app, ui, Icon::Spelling, "Spelling", "app.dialog", json!({"id": "spelling"}));
        let r = big_button(ui, Icon::Thesaurus, "Thesaurus", false);
        let _ = r.on_hover_text("Thesaurus (coming soon)");
    });
    group(ui, |ui| {
        big(app, ui, Icon::Accessibility, "Check\nAccessibility", "app.dialog", json!({"id": "accessibility"}));
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Translate, "Translate", false);
        let _ = r.on_hover_text("Translation needs a language model (not bundled)");
        big(app, ui, Icon::Language, "Language", "app.dialog", json!({"id": "language"}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::NewComment, "New\nComment", "app.dialog", json!({"id": "comment"}));
        big(app, ui, Icon::DeleteComment, "Delete", "comment.delete", json!({}));
        let show = app.ui.pane.as_deref() == Some("comments");
        if big_button_ex(ui, Icon::ShowComments, "Show\nComments", true, false, show).0.clicked() {
            let _ = app.run("view.pane", json!({"pane": "comments", "toggle": true}));
        }
    });
}

fn view(app: &mut SlideApp, ui: &mut Ui) {
    use crate::ViewMode;
    group(ui, |ui| {
        for (icon, label, id, mode) in [
            (Icon::ViewNormal, "Normal", "view.normal", Some(ViewMode::Normal)),
            (Icon::ViewOutline, "Outline\nView", "view.outline", Some(ViewMode::Outline)),
            (Icon::ViewSorter, "Slide\nSorter", "view.sorter", Some(ViewMode::Sorter)),
            (Icon::ViewNotesPage, "Notes\nPage", "view.notesPage", Some(ViewMode::NotesPage)),
            (Icon::ViewReading, "Reading\nView", "view.reading", None),
        ] {
            let sel = mode.is_some_and(|m| app.ui.view == m);
            if big_button_ex(ui, icon, label, true, false, sel).0.clicked() {
                let _ = app.run(id, json!({}));
            }
        }
    });
    group(ui, |ui| {
        let in_master = app.session.active().is_some_and(|d| d.selection.target != Target::Slides);
        if big_button_ex(ui, Icon::SlideMaster, "Slide\nMaster", true, false, in_master).0.clicked() {
            let _ = app.run(if in_master { "view.closeMaster" } else { "view.slideMaster" }, json!({}));
        }
        let r = big_button(ui, Icon::HandoutMaster, "Handout\nMaster", false);
        let _ = r.on_hover_text("Handout Master (coming soon)");
        let r = big_button(ui, Icon::NotesMaster, "Notes\nMaster", false);
        let _ = r.on_hover_text("Notes Master (coming soon)");
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            let mut r = app.ui.ruler;
            if ui.checkbox(&mut r, "Ruler").changed() {
                app.ui.ruler = r;
            }
            let mut g = app.ui.gridlines;
            if ui.checkbox(&mut g, "Gridlines").changed() {
                app.ui.gridlines = g;
            }
            let mut gd = app.ui.guides;
            if ui.checkbox(&mut gd, "Guides").changed() {
                app.ui.guides = gd;
            }
        });
        let n = app.ui.notes;
        if big_button_ex(ui, Icon::Notes, "Notes", true, false, n).0.clicked() {
            app.ui.notes = !n;
        }
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::ZoomGlass, "Zoom", true);
        egui::Popup::menu(&r).show(|ui| {
            for pct in [400, 200, 150, 100, 75, 66, 50, 33] {
                if ui.button(format!("{pct}%")).clicked() {
                    let _ = app.run("view.zoom", json!({"percent": pct}));
                }
            }
        });
        if big_button(ui, Icon::FitToWindow, "Fit to\nWindow", true).clicked() {
            app.ui.zoom = None;
        }
    });
    group(ui, |ui| {
        let gs = app.ui.grayscale;
        if big_button_ex(ui, Icon::Grayscale, "Grayscale", true, false, gs).0.clicked() {
            app.ui.grayscale = !gs;
        }
        let dark = app.ui.brightness == theme::Brightness::Dark;
        if big_button_ex(ui, Icon::Palette, "Dark\nMode", true, false, dark).0.clicked() {
            let _ = app.run("view.dark", json!({}));
        }
    });
}

// ---------- contextual tabs ----------

fn shape_format(app: &mut SlideApp, ui: &mut Ui) {
    let sc = scheme(app);
    let sel = enabled(app, "shape.fill");
    group(ui, |ui| {
        let r = big_button(ui, Icon::Shapes, "Shapes", true);
        egui::Popup::menu(&r).show(|ui| {
            if let Some(p) = shapes_gallery(ui) {
                app.session.set_tool(deckcraft_engine::ToolKind::Shape { preset: p.to_string() });
            }
        });
        big(app, ui, Icon::TextBox, "Text\nBox", "insert.textBox", json!({}));
        let r = big_button(ui, Icon::EditShape, "Edit\nShape", sel);
        egui::Popup::menu(&r).show(|ui| {
            ui.label("Change Shape");
            if let Some(p) = shapes_gallery(ui) {
                run(app, "shape.change", json!({"preset": p}));
            }
        });
        let can_merge = enabled(app, "shape.merge");
        let r = big_button(ui, Icon::MergeShapes, "Merge\nShapes", can_merge).on_hover_text("Merge Shapes");
        egui::Popup::menu(&r).show(|ui| {
            for (op, label) in deckcraft_engine::cmd::merge::OPS {
                if ui.button(label).clicked() {
                    run(app, "shape.merge", json!({"op": op}));
                    ui.close();
                }
            }
        });
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::QuickStyles, "Shape\nStyles", sel);
        egui::Popup::menu(&r).width(330.0).show(|ui| quick_styles(app, ui));
        rows(ui, |ui| {
            let (m, a) = small_button_ex(ui, Icon::ShapeFill, "Shape Fill", sel, false, true);
            if m.clicked() {
                run(app, "shape.fill", json!({"color": "accent1"}));
            }
            if let Some(a) = a {
                widgets::color_popup(&a).show(|ui| {
                    if let Some(c) = widgets::color_grid(ui, &sc, Some("No Fill")) {
                        match c {
                            Some(c) => run(app, "shape.fill", json!({"color": cref_param(&c)})),
                            None => run(app, "shape.fill", json!({"none": true})),
                        }
                    }
                    if ui.button("Gradient…").clicked() {
                        let _ = app.run("view.pane", json!({"pane": "format", "tab": "fill"}));
                        ui.close();
                    }
                });
            }
            let (_, a) = small_button_ex(ui, Icon::ShapeOutline, "Shape Outline", sel, false, true);
            if let Some(a) = a {
                widgets::color_popup(&a).show(|ui| outline_menu(app, ui, &sc));
            }
            let (_, a) = small_button_ex(ui, Icon::ShapeEffects, "Shape Effects", sel, false, true);
            if let Some(a) = a {
                egui::Popup::menu(&a).show(|ui| effects_menu(app, ui));
            }
        });
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            let (_, a) = small_button_ex(ui, Icon::TextFill, "Text Fill", sel, false, true);
            if let Some(a) = a {
                widgets::color_popup(&a).show(|ui| {
                    if let Some(Some(c)) = widgets::color_grid(ui, &sc, None) {
                        run(app, "format.color", json!({"color": cref_param(&c)}));
                    }
                });
            }
            let (_, a) = small_button_ex(ui, Icon::TextOutline, "Text Outline", sel, false, true);
            if let Some(a) = a {
                widgets::color_popup(&a).show(|ui| {
                    if let Some(c) = widgets::color_grid(ui, &sc, Some("No Outline")) {
                        run(app, "format.textOutline", json!({"color": c.map(|c| cref_param(&c))}));
                    }
                });
            }
            let r = small_button(ui, Icon::TextEffects, "Text Effects", sel, false);
            egui::Popup::menu(&r).show(|ui| {
                if ui.button("Shadow").clicked() {
                    run(app, "format.textShadow", json!({}));
                }
            });
        });
    });
    group(ui, |ui| {
        big(app, ui, Icon::AltText, "Alt\nText", "app.dialog", json!({"id": "altText"}));
    });
    arrange_group(app, ui);
    size_group(app, ui);
    group(ui, |ui| {
        big(app, ui, Icon::FormatBackground, "Format\nPane", "view.pane", json!({"pane": "format"}));
    });
}

fn arrange_group(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let sel = enabled(app, "arrange.bringForward");
        let (m, a) = big_button_ex(ui, Icon::BringForward, "Bring\nForward", sel, true, false);
        if m.clicked() {
            run(app, "arrange.bringForward", json!({}));
        }
        if let Some(a) = a {
            egui::Popup::menu(&a).show(|ui| {
                menu_item(app, ui, Icon::BringForward, "Bring Forward", "arrange.bringForward", json!({}));
                menu_item(app, ui, Icon::BringToFront, "Bring to Front", "arrange.bringToFront", json!({}));
            });
        }
        let (m, a) = big_button_ex(ui, Icon::SendBackward, "Send\nBackward", sel, true, false);
        if m.clicked() {
            run(app, "arrange.sendBackward", json!({}));
        }
        if let Some(a) = a {
            egui::Popup::menu(&a).show(|ui| {
                menu_item(app, ui, Icon::SendBackward, "Send Backward", "arrange.sendBackward", json!({}));
                menu_item(app, ui, Icon::SendToBack, "Send to Back", "arrange.sendToBack", json!({}));
            });
        }
        big(app, ui, Icon::SelectionPane, "Selection\nPane", "view.pane", json!({"pane": "selection"}));
        let r = big_button(ui, Icon::AlignObjects, "Align", sel);
        egui::Popup::menu(&r).show(|ui| align_menu(app, ui));
        rows(ui, |ui| {
            let (_, a) = small_button_ex(ui, Icon::Group, "Group", sel, false, true);
            if let Some(a) = a {
                egui::Popup::menu(&a).show(|ui| {
                    menu_item(app, ui, Icon::Group, "Group", "arrange.group", json!({}));
                    menu_item(app, ui, Icon::Ungroup, "Ungroup", "arrange.ungroup", json!({}));
                    menu_item(app, ui, Icon::Group, "Regroup", "arrange.regroup", json!({}));
                });
            }
            let (_, a) = small_button_ex(ui, Icon::Rotate, "Rotate", sel, false, true);
            if let Some(a) = a {
                egui::Popup::menu(&a).show(|ui| {
                    menu_item(app, ui, Icon::RotateRight, "Rotate Right 90°", "arrange.rotateRight", json!({}));
                    menu_item(app, ui, Icon::RotateLeft, "Rotate Left 90°", "arrange.rotateLeft", json!({}));
                    menu_item(app, ui, Icon::FlipV, "Flip Vertical", "arrange.flipVertical", json!({}));
                    menu_item(app, ui, Icon::FlipH, "Flip Horizontal", "arrange.flipHorizontal", json!({}));
                });
            }
        });
    });
}

fn size_group(app: &mut SlideApp, ui: &mut Ui) {
    let bx = app
        .session
        .active()
        .and_then(|d| d.selection.shapes.first().and_then(|id| d.shape(*id).map(|s| deckcraft_engine::cmd::xfrm_of(&d.doc, &d.selection, s))));
    group(ui, |ui| {
        rows(ui, |ui| {
            let Some(x) = bx else {
                ui.label("Height:");
                ui.label("Width:");
                return;
            };
            ui.horizontal(|ui| {
                ui.label("Height:");
                if let Some(v) = widgets::spinner(ui, "sh", x.h / 72.0, 0.1, "\"", 54.0) {
                    run(app, "shape.resize", json!({"h": v.max(0.0) * 72.0}));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Width: ");
                if let Some(v) = widgets::spinner(ui, "sw", x.w / 72.0, 0.1, "\"", 54.0) {
                    run(app, "shape.resize", json!({"w": v.max(0.0) * 72.0}));
                }
            });
        });
    });
}

fn picture_format(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let r = big_button(ui, Icon::RemoveBackground, "Remove\nBackground", false);
        let _ = r.on_hover_text("Remove Background (coming soon)");
        let r = big_button(ui, Icon::Corrections, "Corrections", true);
        egui::Popup::menu(&r).show(|ui| {
            for b in [-0.4, -0.2, 0.0, 0.2, 0.4] {
                if ui.button(format!("Brightness {:+}%", (b * 100.0) as i32)).clicked() {
                    run(app, "picture.adjust", json!({"brightness": b}));
                }
            }
            ui.separator();
            for c in [-0.4, -0.2, 0.0, 0.2, 0.4] {
                if ui.button(format!("Contrast {:+}%", (c * 100.0) as i32)).clicked() {
                    run(app, "picture.adjust", json!({"contrast": c}));
                }
            }
        });
        let r = big_button(ui, Icon::ColorAdjust, "Color", true);
        egui::Popup::menu(&r).show(|ui| {
            for s in [0.0, 0.33, 0.66, 1.0, 1.5, 2.0, 3.0] {
                if ui.button(format!("Saturation {}%", (s * 100.0) as i32)).clicked() {
                    run(app, "picture.adjust", json!({"saturation": s}));
                }
            }
            if ui.button("Grayscale").clicked() {
                run(app, "picture.adjust", json!({"grayscale": true}));
            }
        });
        let r = big_button(ui, Icon::Transparency, "Transparency", true);
        egui::Popup::menu(&r).show(|ui| {
            for tr in [0.0, 0.15, 0.3, 0.5, 0.65, 0.8, 0.95] {
                if ui.button(format!("{}%", (tr * 100.0) as i32)).clicked() {
                    run(app, "picture.adjust", json!({"transparency": tr}));
                }
            }
        });
        rows(ui, |ui| {
            small(app, ui, Icon::ChangePicture, "Change Picture", "app.insertPictureDialog", json!({}));
            small(app, ui, Icon::ResetPicture, "Reset Picture", "picture.reset", json!({}));
        });
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::QuickStyles, "Picture\nStyles", true);
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("Simple Frame, White").clicked() {
                run(app, "shape.line", json!({"color": "#FFFFFF", "width": 6}));
                run(app, "shape.effects", json!({"shadow": "outer"}));
            }
            if ui.button("Drop Shadow Rectangle").clicked() {
                run(app, "shape.effects", json!({"shadow": "bottom"}));
            }
            if ui.button("Rounded Diagonal Corner").clicked() {
                run(app, "shape.change", json!({"preset": "round2DiagRect"}));
            }
            if ui.button("Soft Edge Rectangle").clicked() {
                run(app, "shape.effects", json!({"softEdges": 10}));
            }
            if ui.button("Reflected Rounded Rectangle").clicked() {
                run(app, "shape.change", json!({"preset": "roundRect"}));
                run(app, "shape.effects", json!({"reflection": "half"}));
            }
            if ui.button("Oval").clicked() {
                run(app, "shape.change", json!({"preset": "ellipse"}));
            }
        });
        big(app, ui, Icon::AltText, "Alt\nText", "app.dialog", json!({"id": "altText"}));
    });
    arrange_group(app, ui);
    group(ui, |ui| {
        let r = big_button(ui, Icon::Crop, "Crop", true);
        egui::Popup::menu(&r).show(|ui| {
            ui.label("Crop to Shape");
            if let Some(p) = shapes_gallery(ui) {
                run(app, "shape.change", json!({"preset": p}));
            }
            ui.separator();
            for (l, v) in [("Crop 10% each side", 0.1), ("Crop 20% each side", 0.2), ("Reset Crop", 0.0)] {
                if ui.button(l).clicked() {
                    run(app, "picture.crop", json!({"left": v, "top": v, "right": v, "bottom": v}));
                }
            }
        });
    });
    size_group(app, ui);
}

fn table_design(app: &mut SlideApp, ui: &mut Ui) {
    let tbl = app
        .session
        .active()
        .and_then(|d| d.selected_shapes().into_iter().find_map(|s| if let ShapeKind::Table(t) = &s.kind { Some(t.clone()) } else { None }));
    group(ui, |ui| {
        rows(ui, |ui| {
            let flags = tbl.as_ref().map(|t| (t.first_row, t.last_row, t.band_row, t.first_col, t.last_col, t.band_col)).unwrap_or_default();
            ui.horizontal(|ui| {
                let mut a = flags.0;
                if ui.checkbox(&mut a, "Header Row").changed() {
                    run(app, "table.options", json!({"headerRow": a}));
                }
                let mut d = flags.3;
                if ui.checkbox(&mut d, "First Column").changed() {
                    run(app, "table.options", json!({"firstColumn": d}));
                }
            });
            ui.horizontal(|ui| {
                let mut b = flags.1;
                if ui.checkbox(&mut b, "Total Row").changed() {
                    run(app, "table.options", json!({"totalRow": b}));
                }
                let mut e = flags.4;
                if ui.checkbox(&mut e, "Last Column").changed() {
                    run(app, "table.options", json!({"lastColumn": e}));
                }
            });
            ui.horizontal(|ui| {
                let mut c = flags.2;
                if ui.checkbox(&mut c, "Banded Rows").changed() {
                    run(app, "table.options", json!({"bandedRows": c}));
                }
                let mut f = flags.5;
                if ui.checkbox(&mut f, "Banded Columns").changed() {
                    run(app, "table.options", json!({"bandedColumns": f}));
                }
            });
        });
    });
    group(ui, |ui| {
        let styles = deckcraft_model::table::table_styles();
        let quick: Vec<_> = styles.iter().filter(|(id, _)| id.starts_with("medium2") || id == "none" || id == "grid").take(8).collect();
        let cur = tbl.as_ref().map(|t| t.style.clone()).unwrap_or_default();
        let sel = quick.iter().position(|(id, _)| *id == cur);
        let sc = scheme(app);
        if let Some(i) = widgets::gallery(
            ui,
            quick.len(),
            vec2(48.0, 40.0),
            sel,
            |p, r, i| {
                if let Some((id, _)) = quick.get(i) {
                    paint_table_style(p, r, id, &sc);
                }
            },
            |i| quick.get(i).map(|x| x.1.clone()).unwrap_or_default(),
        ) && let Some((id, _)) = quick.get(i)
        {
            run(app, "table.style", json!({"style": id}));
        }
        let more = widgets::drop_button(ui, "", vec2(14.0, 40.0), true);
        egui::Popup::menu(&more).width(360.0).show(|ui| {
            ui.horizontal_wrapped(|ui| {
                for (id, label) in &styles {
                    let (r, resp) = ui.allocate_exact_size(vec2(48.0, 36.0), Sense::click());
                    paint_table_style(ui.painter(), r.shrink(2.0), id, &sc);
                    if resp.on_hover_text(label).clicked() {
                        run(app, "table.style", json!({"style": id}));
                        ui.close();
                    }
                }
            });
        });
    });
    group(ui, |ui| {
        let sc = scheme(app);
        let (m, a) = color_split(ui, Icon::Shading, "Shading", Color32::from_rgb(0x2E, 0x6F, 0xD8), true);
        if m.clicked() {
            run(app, "table.cellFill", json!({"color": "accent1"}));
        }
        widgets::color_popup(&a).show(|ui| {
            if let Some(c) = widgets::color_grid(ui, &sc, Some("No Fill")) {
                match c {
                    Some(c) => run(app, "table.cellFill", json!({"color": cref_param(&c)})),
                    None => run(app, "table.cellFill", json!({"none": true})),
                }
            }
        });
    });
}

pub fn paint_table_style(p: &egui::Painter, r: Rect, id: &str, sc: &deckcraft_color::ColorScheme) {
    let slot = match id.rsplit('-').next().unwrap_or("accent1") {
        "accent2" => deckcraft_color::SchemeSlot::Accent2,
        "accent3" => deckcraft_color::SchemeSlot::Accent3,
        "accent4" => deckcraft_color::SchemeSlot::Accent4,
        "accent5" => deckcraft_color::SchemeSlot::Accent5,
        "accent6" => deckcraft_color::SchemeSlot::Accent6,
        "tx1" => deckcraft_color::SchemeSlot::Dk1,
        _ => deckcraft_color::SchemeSlot::Accent1,
    };
    let a = theme::to_color32(sc.get(slot));
    let kind = id.split('-').next().unwrap_or("");
    for row in 0..4 {
        let y = r.min.y + row as f32 * r.height() / 4.0;
        let rr = Rect::from_min_size(pos2(r.min.x, y), vec2(r.width(), r.height() / 4.0));
        let c = match (kind, row) {
            ("none", _) | ("grid", _) => Color32::WHITE,
            ("light1", 0) | ("light2", _) => Color32::WHITE,
            ("dark1", 0) => Color32::BLACK,
            ("dark1", _) => a.lerp_to_gamma(Color32::BLACK, 0.3),
            (_, 0) => a,
            (_, k) if k % 2 == 1 => a.lerp_to_gamma(Color32::WHITE, 0.6),
            _ => a.lerp_to_gamma(Color32::WHITE, 0.8),
        };
        p.rect_filled(rr, CornerRadius::ZERO, c);
        p.line_segment([rr.left_bottom(), rr.right_bottom()], Stroke::new(0.5, if kind == "grid" { Color32::GRAY } else { Color32::WHITE }));
    }
    if kind == "grid" || kind == "light2" {
        p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(0.8, if kind == "grid" { Color32::GRAY } else { a }), egui::StrokeKind::Inside);
    }
}

fn table_layout(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let r = big_button(ui, Icon::DeleteTable, "Delete", true);
        egui::Popup::menu(&r).show(|ui| {
            menu_item(app, ui, Icon::DeleteTable, "Delete Columns", "table.deleteColumn", json!({}));
            menu_item(app, ui, Icon::DeleteTable, "Delete Rows", "table.deleteRow", json!({}));
            menu_item(app, ui, Icon::DeleteTable, "Delete Table", "edit.delete", json!({}));
        });
        big(app, ui, Icon::InsertAbove, "Insert\nAbove", "table.insertRowAbove", json!({}));
        big(app, ui, Icon::InsertBelow, "Insert\nBelow", "table.insertRowBelow", json!({}));
        big(app, ui, Icon::InsertLeft, "Insert\nLeft", "table.insertColumnLeft", json!({}));
        big(app, ui, Icon::InsertRight, "Insert\nRight", "table.insertColumnRight", json!({}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::MergeCells, "Merge\nCells", "table.merge", json!({}));
        big(app, ui, Icon::SplitCells, "Split\nCells", "table.split", json!({}));
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            small(app, ui, Icon::DistributeV, "Distribute Rows", "table.distributeRows", json!({}));
            small(app, ui, Icon::DistributeH, "Distribute Columns", "table.distributeColumns", json!({}));
        });
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            ui.horizontal(|ui| {
                toggle(app, ui, Icon::AlignLeft, "Align Left", "format.alignLeft", false);
                toggle(app, ui, Icon::AlignCenter, "Center", "format.alignCenter", false);
                toggle(app, ui, Icon::AlignRight, "Align Right", "format.alignRight", false);
            });
            ui.horizontal(|ui| {
                for (l, a) in [("Top", "top"), ("Center", "middle"), ("Bottom", "bottom")] {
                    if ui.small_button(l).clicked() {
                        run(app, "format.anchor", json!({"anchor": a}));
                    }
                }
            });
        });
    });
    arrange_group(app, ui);
}

fn chart_design(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let r = big_button(ui, Icon::ChangeChartType, "Change\nChart Type", true);
        egui::Popup::menu(&r).show(|ui| {
            for k in deckcraft_model::chart::ChartType::MAIN {
                if ui.button(k.label()).clicked() {
                    // Replace the chart's type keeping its data.
                    if let Some(id) = app.session.active().and_then(|d| d.selection.shapes.first().copied()) {
                        let _ = app.run("chart.type", json!({"id": id.0, "type": format!("{k:?}")}));
                    }
                }
            }
        });
        big(app, ui, Icon::EditData, "Edit\nData", "app.dialog", json!({"id": "chartData"}));
    });
    arrange_group(app, ui);
}

fn playback(app: &mut SlideApp, ui: &mut Ui) {
    let m = app
        .session
        .active()
        .and_then(|d| d.selected_shapes().into_iter().find_map(|s| if let ShapeKind::Media(m) = &s.kind { Some(m.clone()) } else { None }));
    group(ui, |ui| {
        let playing = app.session.active().and_then(|d| d.selection.shapes.first().copied()).is_some_and(|id| app.media.is_playing(id));
        if big_button(ui, if playing { Icon::Pause } else { Icon::Play }, if playing { "Pause" } else { "Play" }, m.is_some()).clicked() {
            run(app, "media.toggle", json!({}));
        }
        big(app, ui, Icon::TrimMedia, "Trim", "app.dialog", json!({"id": "trim"}));
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            let mut auto = m.as_ref().is_some_and(|x| x.autoplay);
            if ui.checkbox(&mut auto, "Start Automatically").changed() {
                run(app, "media.options", json!({"autoplay": auto}));
            }
            let mut across = m.as_ref().is_some_and(|x| x.play_across_slides);
            if ui.checkbox(&mut across, "Play Across Slides").changed() {
                run(app, "media.options", json!({"acrossSlides": across}));
            }
            let mut lp = m.as_ref().is_some_and(|x| x.loop_play);
            if ui.checkbox(&mut lp, "Loop until Stopped").changed() {
                run(app, "media.options", json!({"loop": lp}));
            }
        });
        rows(ui, |ui| {
            let mut hide = m.as_ref().is_some_and(|x| x.hide_while_not_playing);
            if ui.checkbox(&mut hide, "Hide During Show").changed() {
                run(app, "media.options", json!({"hide": hide}));
            }
            let mut rw = m.as_ref().is_some_and(|x| x.rewind);
            if ui.checkbox(&mut rw, "Rewind after Playing").changed() {
                run(app, "media.options", json!({"rewind": rw}));
            }
            let mut fs = m.as_ref().is_some_and(|x| x.full_screen);
            if ui.checkbox(&mut fs, "Play Full Screen").changed() {
                run(app, "media.options", json!({"fullScreen": fs}));
            }
        });
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Volume, "Volume", m.is_some());
        egui::Popup::menu(&r).show(|ui| {
            for (l, v) in [("Low", 0.25), ("Medium", 0.5), ("High", 1.0), ("Mute", 0.0)] {
                if ui.button(l).clicked() {
                    run(app, "media.options", json!({"volume": v}));
                }
            }
        });
    });
}

fn slide_master(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        big(app, ui, Icon::NewSlide, "Insert\nLayout", "master.insertLayout", json!({}));
        big(app, ui, Icon::DeleteSlide, "Delete", "master.deleteLayout", json!({}));
        big(app, ui, Icon::Layout, "Rename", "app.dialog", json!({"id": "renameLayout"}));
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Table, "Insert\nPlaceholder", true);
        egui::Popup::menu(&r).show(|ui| {
            for (l, k) in
                [("Content", "content"), ("Text", "text"), ("Picture", "picture"), ("Chart", "chart"), ("Table", "table"), ("Media", "media")]
            {
                if ui.button(l).clicked() {
                    run(app, "master.insertPlaceholder", json!({"kind": k}));
                }
            }
        });
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Themes, "Themes", true);
        egui::Popup::menu(&r).show(|ui| {
            for th in deckcraft_model::theme::builtin_themes() {
                if ui.button(&th.name).clicked() {
                    run(app, "design.theme", json!({"name": th.name}));
                }
            }
        });
        big(app, ui, Icon::FormatBackground, "Background\nStyles", "view.pane", json!({"pane": "background"}));
    });
    group(ui, |ui| {
        if big_button(ui, Icon::Close, "Close\nMaster View", true).clicked() {
            run(app, "view.closeMaster", json!({}));
        }
    });
}

/// Gallery glyphs for the freeform tools: a wave (curve), a polygon (freeform), a squiggle (scribble).
fn paint_freeform_icon(p: &egui::Painter, r: Rect, name: &str, ink: Color32) {
    let at = |x: f32, y: f32| egui::pos2(r.min.x + x * r.width(), r.min.y + y * r.height());
    let pts: Vec<egui::Pos2> = match name {
        "curve" => (0..=24).map(|i| i as f32 / 24.0).map(|u| at(u, 0.5 - 0.38 * (u * std::f32::consts::TAU).sin())).collect(),
        "freeform" => {
            [(0.0, 0.85), (0.15, 0.2), (0.55, 0.45), (0.8, 0.05), (1.0, 0.75), (0.45, 1.0), (0.0, 0.85)].iter().map(|(x, y)| at(*x, *y)).collect()
        }
        _ => (0..=40).map(|i| i as f32 / 40.0).map(|u| at(u, 0.5 + 0.35 * (u * 19.0).sin() * (1.0 - u * 0.4))).collect(),
    };
    p.add(egui::Shape::line(pts, egui::Stroke::new(1.3, ink)));
}
