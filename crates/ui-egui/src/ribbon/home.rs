use super::arrange::arrange_menu;
use super::galleries::{cref_param, effects_menu, outline_menu, quick_styles, shapes_gallery, smartart_gallery};
use super::{
    Color32, CornerRadius, Icon, Rect, Sense, SlideApp, Stroke, Tokens, Ui, Value, big, big_button, big_button_ex, color_split, dropdown, enabled,
    fmt_state, group, icon_toggle, json, pos2, rows, run, scheme, small, small_button, small_button_ex, small_button_named, toggle, vec2, widgets,
};
use crate::i18n;

// ---------- Home ----------

/// Draw supported translated clipboard controls and invoke their existing commands.
fn clipboard_group(app: &mut SlideApp, ui: &mut Ui) {
    let lang = app.lang();
    group(ui, |ui| {
        let (main, arrow) = big_button_ex(ui, Icon::Paste, i18n::text(lang, "ribbon.clipboard.paste"), enabled(app, "edit.paste"), true, false);
        if main.on_hover_text(i18n::text(lang, "ribbon.clipboard.paste_tip")).clicked() {
            run(app, "edit.paste", json!({}));
        }
        if let Some(a) = arrow {
            egui::Popup::menu(&a).show(|ui| {
                if ui.button(i18n::text(lang, "ribbon.clipboard.paste")).clicked() {
                    run(app, "edit.paste", json!({}));
                }
                if ui.button(i18n::text(lang, "ribbon.clipboard.paste_match")).clicked() {
                    run(app, "edit.pasteText", json!({}));
                }
                if ui.button("Paste Special…").clicked() {
                    run(app, "app.pasteSpecial", json!({}));
                }
                if ui.button(i18n::text(lang, "ribbon.clipboard.paste_format")).clicked() {
                    run(app, "format.painterApply", json!({}));
                }
            });
        }
        rows(ui, |ui| {
            small(app, ui, Icon::Cut, i18n::text(lang, "ribbon.clipboard.cut"), "edit.cut", json!({}));
            small(app, ui, Icon::Copy, i18n::text(lang, "ribbon.clipboard.copy"), "edit.copy", json!({}));
            let on = app.session.painter.is_some();
            let en = enabled(app, "format.painter");
            let r = small_button(ui, Icon::FormatPainter, i18n::text(lang, "ribbon.clipboard.format"), en, on)
                .on_hover_text(i18n::text(lang, "ribbon.clipboard.painter_tip"));
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

pub(super) fn layout_menu(app: &mut SlideApp, ui: &mut Ui, cmd: &str) {
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
                    let tex = crate::textures::upload_editor(ui.ctx(), format!("layout-{li}"), crate::textures::to_color_image(&img, false));
                    let at = crate::textures::device_pixel_rect(rect, ui.ctx().pixels_per_point());
                    ui.painter().image(tex.id(), at, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
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

/// Render the Home Slides group: New Slide, Layout, Reset, and Section controls.
fn slides_group(app: &mut SlideApp, ui: &mut Ui) {
    let lang = app.lang();
    group(ui, |ui| {
        let (main, arrow) = big_button_ex(ui, Icon::NewSlide, i18n::text(lang, "ribbon.slides.new"), enabled(app, "slide.new"), true, false);
        if main.on_hover_text(i18n::text(lang, "ribbon.slides.new_tip")).clicked() {
            run(app, "slide.new", json!({}));
        }
        if let Some(a) = arrow {
            egui::Popup::menu(&a).width(420.0).show(|ui| new_slide_menu(app, ui));
        }
        rows(ui, |ui| {
            let (main, arrow) =
                small_button_ex(ui, Icon::Layout, i18n::text(lang, "ribbon.slides.layout"), enabled(app, "slide.layout"), false, true);
            if let Some(arrow) = arrow {
                widgets::split_menu(&main, &arrow, |ui| layout_menu(app, ui, "slide.layout"));
            }
            small(app, ui, Icon::ResetSlide, i18n::text(lang, "ribbon.slides.reset"), "slide.reset", json!({}));
            let (main, arrow) =
                small_button_ex(ui, Icon::Section, i18n::text(lang, "ribbon.slides.section"), enabled(app, "section.add"), false, true);
            if let Some(arrow) = arrow {
                widgets::split_menu(&main, &arrow, |ui| section_menu(app, ui));
            }
        });
    });
}

/// The New Slide button's menu: layouts, duplication, outline import and slide reuse.
fn new_slide_menu(app: &mut SlideApp, ui: &mut Ui) {
    let lang = app.lang();
    layout_menu(app, ui, "slide.new");
    ui.separator();
    if ui.button(i18n::text(lang, "ribbon.slides.duplicate")).clicked() {
        run(app, "slide.duplicate", json!({}));
    }
    if ui.button(i18n::text(lang, "ribbon.slides.outline")).clicked() {
        app.dialog = Some(crate::dialogs::Dialog::new("outline"));
    }
    let desktop = app.services.pick_open.is_some();
    for (label, keep) in [("Reuse Slides…", false), ("Reuse Slides (Keep Source Formatting)…", true)] {
        let r = ui.add_enabled(desktop, egui::Button::new(label));
        if r.clicked() {
            run(app, "app.reuseSlidesDialog", json!({"keepFormatting": keep}));
        }
        r.on_disabled_hover_text("Choosing a presentation to reuse needs the desktop app");
    }
}

/// The Section split button's menu.
fn section_menu(app: &mut SlideApp, ui: &mut Ui) {
    let lang = app.lang();
    if ui.button(i18n::text(lang, "ribbon.slides.add_section")).clicked() {
        run(app, "section.add", json!({}));
    }
    if ui.button(i18n::text(lang, "ribbon.slides.remove_sections")).clicked() {
        run(app, "section.removeAll", json!({}));
    }
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

/// Draw Home's text-format controls with supported translated captions and tooltips.
fn font_group(app: &mut SlideApp, ui: &mut Ui, st: &Value) {
    let lang = app.lang();
    let en = enabled(app, "format.bold");
    group(ui, |ui| {
        rows(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let font = st.get("font").and_then(Value::as_str).unwrap_or("").to_string();
                let size = st.get("size").and_then(Value::as_f64);
                let fams = font_families(app);
                let theme_fonts: Vec<String> = fams.iter().take(2).cloned().collect();
                dropdown(ui, "Font", &font, 150.0, en, |ui| {
                    for (k, f) in fams.iter().enumerate() {
                        let label = if k == 0 && theme_fonts.len() == 2 {
                            i18n::fill(i18n::text(lang, "ribbon.font.headings"), &[("font", f)])
                        } else if k == 1 && theme_fonts.len() == 2 {
                            i18n::fill(i18n::text(lang, "ribbon.font.body"), &[("font", f)])
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
                    ui.separator();
                    // Fonts installed while DeckCraft runs (e.g. into ~/.local/share/fonts).
                    if ui.button("Rescan Installed Fonts").clicked() {
                        run(app, "font.rescan", json!({}));
                        ui.close();
                    }
                });
                let size_text = size.map(|s| widgets::fmt_num(s, "")).unwrap_or_default();
                dropdown(ui, "Font size", &size_text, 52.0, en, |ui| {
                    for s in deckcraft_engine::cmd::format::SIZES.iter().chain([72.0, 80.0, 88.0, 96.0].iter()) {
                        if ui.button(widgets::fmt_num(*s, "")).clicked() {
                            run(app, "format.size", json!({"size": s}));
                            ui.close();
                        }
                    }
                });
                toggle(app, ui, Icon::FontGrow, i18n::text(lang, "ribbon.font.grow"), "format.grow", false);
                toggle(app, ui, Icon::FontShrink, i18n::text(lang, "ribbon.font.shrink"), "format.shrink", false);
                toggle(app, ui, Icon::ClearFormatting, i18n::text(lang, "ribbon.font.clear"), "format.clear", false);
            });
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                let b = |k: &str| st.get(k).and_then(Value::as_bool).unwrap_or(false);
                toggle(app, ui, Icon::Bold, i18n::text(lang, "ribbon.font.bold"), "format.bold", b("bold"));
                toggle(app, ui, Icon::Italic, i18n::text(lang, "ribbon.font.italic"), "format.italic", b("italic"));
                toggle(app, ui, Icon::Underline, i18n::text(lang, "ribbon.font.underline"), "format.underline", b("underline"));
                toggle(app, ui, Icon::Strikethrough, i18n::text(lang, "ribbon.font.strike"), "format.strikethrough", b("strike"));
                toggle(app, ui, Icon::Superscript, i18n::text(lang, "ribbon.font.superscript"), "format.superscript", b("superscript"));
                toggle(app, ui, Icon::Subscript, i18n::text(lang, "ribbon.font.subscript"), "format.subscript", b("subscript"));
                let r = icon_toggle(ui, Icon::CharSpacing, i18n::text(lang, "ribbon.font.spacing"), en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for (label, pt) in [
                        (i18n::text(lang, "ribbon.font.spacing_very_tight"), -3.0),
                        (i18n::text(lang, "ribbon.font.spacing_tight"), -1.5),
                        (i18n::text(lang, "ribbon.font.spacing_normal"), 0.0),
                        (i18n::text(lang, "ribbon.font.spacing_loose"), 3.0),
                        (i18n::text(lang, "ribbon.font.spacing_very_loose"), 6.0),
                    ] {
                        if ui.button(label).clicked() {
                            run(app, "format.spacing", json!({"pt": pt}));
                        }
                    }
                });
                let r = icon_toggle(ui, Icon::ChangeCase, i18n::text(lang, "ribbon.font.case"), en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for (label, mode) in [
                        (i18n::text(lang, "ribbon.font.case_sentence"), "sentence"),
                        (i18n::text(lang, "ribbon.font.case_lower"), "lower"),
                        (i18n::text(lang, "ribbon.font.case_upper"), "upper"),
                        (i18n::text(lang, "ribbon.font.case_title"), "title"),
                        (i18n::text(lang, "ribbon.font.case_toggle"), "toggle"),
                    ] {
                        if ui.button(label).clicked() {
                            run(app, "format.case", json!({"mode": mode}));
                        }
                    }
                });
                let sc = scheme(app);
                let (m, a) = color_split(ui, Icon::Highlight, i18n::text(lang, "ribbon.font.highlight"), Color32::from_rgb(255, 230, 0), en);
                if m.clicked() {
                    run(app, "format.highlight", json!({"color": "#FFFF00"}));
                }
                widgets::color_popup(&a).show(|ui| {
                    if let Some(c) = widgets::color_grid(ui, &sc, Some(i18n::text(lang, "ribbon.font.no_color"))) {
                        run(app, "format.highlight", json!({"color": c.map(|c| serde_json::to_value(cref_param(&c)).unwrap_or_default())}));
                    }
                });
                let (m, a) = color_split(ui, Icon::FontColor, i18n::text(lang, "ribbon.font.color"), Color32::from_rgb(0xD8, 0x3F, 0x3F), en);
                if m.clicked() {
                    run(app, "format.color", json!({"color": "#C00000"}));
                }
                widgets::color_popup(&a).show(|ui| {
                    if let Some(Some(c)) = widgets::color_grid(ui, &sc, None) {
                        run(app, "format.color", json!({"color": cref_param(&c)}));
                    }
                });
                if ui.add_enabled(en, egui::Button::new("Font…").min_size(vec2(52.0, 22.0))).on_hover_text("Font").clicked() {
                    run(app, "format.fontDialog", json!({}));
                }
            });
        });
    });
}

/// Render paragraph formatting controls using the current paragraph state.
fn paragraph_group(app: &mut SlideApp, ui: &mut Ui, st: &Value) {
    let lang = app.lang();
    let en = enabled(app, "format.bullets");
    group(ui, |ui| {
        rows(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                let b = |k: &str| st.get(k).and_then(Value::as_bool).unwrap_or(false);
                let (m, a) = small_button_named(ui, Icon::Bullets, "", i18n::text(lang, "ribbon.paragraph.bullets"), en, b("bullets"), true);
                if m.on_hover_text(i18n::text(lang, "ribbon.paragraph.bullets")).clicked() {
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
                        ui.separator();
                        if ui.button("Bullets and Numbering…").clicked() {
                            let mut dlg = crate::dialogs::Dialog::new("bullets");
                            dlg.fields.insert("tab".into(), "bulleted".into());
                            app.dialog = Some(dlg);
                        }
                    });
                }
                let (m, a) = small_button_named(ui, Icon::Numbering, "", i18n::text(lang, "ribbon.paragraph.numbering"), en, b("numbering"), true);
                if m.on_hover_text(i18n::text(lang, "ribbon.paragraph.numbering")).clicked() {
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
                        ui.separator();
                        if ui.button("Bullets and Numbering…").clicked() {
                            let mut dlg = crate::dialogs::Dialog::new("bullets");
                            dlg.fields.insert("tab".into(), "numbered".into());
                            app.dialog = Some(dlg);
                        }
                    });
                }
                toggle(app, ui, Icon::DecreaseIndent, i18n::text(lang, "ribbon.paragraph.outdent"), "format.outdent", false);
                toggle(app, ui, Icon::IncreaseIndent, i18n::text(lang, "ribbon.paragraph.indent"), "format.indent", false);
                let r = icon_toggle(ui, Icon::LineSpacing, i18n::text(lang, "ribbon.paragraph.line_spacing"), en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for v in [1.0, 1.5, 2.0, 2.5, 3.0] {
                        if ui.button(format!("{v:.1}")).clicked() {
                            run(app, "format.lineSpacing", json!({"lines": v}));
                        }
                    }
                    if ui.button(i18n::text(lang, "ribbon.paragraph.line_spacing_options")).clicked() {
                        run(app, "format.paragraphDialog", json!({}));
                    }
                    if ui.button("Tabs…").clicked() {
                        app.dialog = Some(crate::dialogs::Dialog::new("tabs"));
                    }
                });
                let r = icon_toggle(ui, Icon::Columns, i18n::text(lang, "ribbon.paragraph.columns"), en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for n in 1..=3 {
                        if ui.button(format!("{n} Column{}", if n > 1 { "s" } else { "" })).clicked() {
                            run(app, "format.columns", json!({"count": n, "spacing": 18}));
                        }
                    }
                    if ui.button("More Columns…").clicked() {
                        run(app, "format.columns", json!({}));
                    }
                });
            });
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                let al = st.get("align").and_then(Value::as_str).unwrap_or("");
                toggle(app, ui, Icon::AlignLeft, i18n::text(lang, "ribbon.align.left"), "format.alignLeft", al == "l");
                toggle(app, ui, Icon::AlignCenter, i18n::text(lang, "ribbon.paragraph.center"), "format.alignCenter", al == "ctr");
                toggle(app, ui, Icon::AlignRight, i18n::text(lang, "ribbon.align.right"), "format.alignRight", al == "r");
                toggle(app, ui, Icon::Justify, i18n::text(lang, "ribbon.paragraph.justify"), "format.justify", al == "just");
                let r = icon_toggle(ui, Icon::TextDirection, i18n::text(lang, "ribbon.paragraph.direction"), en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for (l, d) in [
                        (i18n::text(lang, "ribbon.paragraph.horizontal"), "horizontal"),
                        (i18n::text(lang, "ribbon.paragraph.rotate_90"), "rotate90"),
                        (i18n::text(lang, "ribbon.paragraph.rotate_270"), "rotate270"),
                        (i18n::text(lang, "ribbon.paragraph.stacked"), "stacked"),
                    ] {
                        if ui.button(l).clicked() {
                            run(app, "format.direction", json!({"dir": d}));
                        }
                    }
                });
                let r = icon_toggle(ui, Icon::AlignText, i18n::text(lang, "ribbon.paragraph.align"), en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for (l, a) in [
                        (i18n::text(lang, "pane.format.top"), "top"),
                        (i18n::text(lang, "pane.format.middle"), "middle"),
                        (i18n::text(lang, "pane.format.bottom"), "bottom"),
                    ] {
                        if ui.button(l).clicked() {
                            run(app, "format.anchor", json!({"anchor": a}));
                        }
                    }
                });
                let r = icon_toggle(ui, Icon::SmartArt, "Convert to SmartArt", enabled(app, "smartArt.fromText"), false);
                egui::Popup::menu(&r).show(|ui| smartart_gallery(app, ui, "smartArt.fromText", json!({}), "", ("", ""), None));
                if ui.add_enabled(en, egui::Button::new("Paragraph…").min_size(vec2(88.0, 22.0))).on_hover_text("Paragraph").clicked() {
                    run(app, "format.paragraphDialog", json!({}));
                }
            });
        });
    });
}

/// Render drawing controls for shapes, text, arrange, quick styles, fill, and outline.
fn drawing_group(app: &mut SlideApp, ui: &mut Ui) {
    let lang = app.lang();
    group(ui, |ui| {
        let en = enabled(app, "shape.insert");
        let r = big_button(ui, Icon::Shapes, i18n::text(lang, "ribbon.drawing.shapes"), en).on_hover_text(i18n::text(lang, "ribbon.drawing.shapes"));
        egui::Popup::menu(&r).show(|ui| {
            if let Some(p) = shapes_gallery(ui) {
                app.session.set_tool(deckcraft_engine::ToolKind::Shape { preset: p.to_string() });
            }
        });
        big(app, ui, Icon::TextBox, i18n::text(lang, "pane.format.text_box"), "insert.textBox", json!({}));
        let sel = enabled(app, "shape.fill");
        let r = big_button(ui, Icon::Arrange, i18n::text(lang, "ribbon.drawing.arrange"), sel);
        egui::Popup::menu(&r).show(|ui| arrange_menu(app, ui));
        let r = big_button(ui, Icon::QuickStyles, i18n::text(lang, "ribbon.drawing.quick_styles"), sel);
        egui::Popup::menu(&r).width(330.0).show(|ui| quick_styles(app, ui));
        rows(ui, |ui| {
            let sc = scheme(app);
            let (m, a) = small_button_ex(ui, Icon::ShapeFill, i18n::text(lang, "ribbon.drawing.fill"), sel, false, true);
            if m.clicked() {
                let c = app.last_fill.clone();
                run(app, "shape.fill", json!({"color": c}));
            }
            if let Some(a) = a {
                widgets::color_popup(&a).show(|ui| {
                    if let Some(c) = widgets::color_grid(ui, &sc, Some("No Fill")) {
                        match c {
                            Some(c) => {
                                app.last_fill = cref_param(&c);
                                run(app, "shape.fill", json!({"color": cref_param(&c)}));
                            }
                            None => run(app, "shape.fill", json!({"none": true})),
                        }
                    }
                });
            }
            let (m, a) = small_button_ex(ui, Icon::ShapeOutline, i18n::text(lang, "ribbon.drawing.outline"), sel, false, true);
            if m.clicked() {
                let c = app.last_outline.clone();
                run(app, "shape.line", json!({"color": c}));
            }
            if let Some(a) = a {
                widgets::color_popup(&a).show(|ui| outline_menu(app, ui, &sc));
            }
            let (main, arrow) = small_button_ex(ui, Icon::ShapeEffects, i18n::text(lang, "ribbon.drawing.effects"), sel, false, true);
            if let Some(arrow) = arrow {
                widgets::split_menu(&main, &arrow, |ui| effects_menu(app, ui));
            }
        });
    });
}

/// Compose the Home ribbon groups using the resolved interface language.
pub(super) fn home(app: &mut SlideApp, ui: &mut Ui) {
    let lang = app.lang();
    let st = fmt_state(app);
    clipboard_group(app, ui);
    slides_group(app, ui);
    font_group(app, ui, &st);
    paragraph_group(app, ui, &st);
    group(ui, |ui| {
        big(app, ui, Icon::Picture, "Picture", "app.insertPictureDialog", json!({}));
    });
    drawing_group(app, ui);
    // Editing, as in PowerPoint: Find, Replace and Replace Fonts, then the Selection Pane.
    group(ui, |ui| {
        ui.vertical(|ui| {
            small(app, ui, Icon::Search, "Find", "app.find", json!({}));
            small(app, ui, Icon::Spelling, "Replace", "app.replace", json!({}));
            if small_button(ui, Icon::Fonts, "Replace Fonts…", enabled(app, "font.replace"), false).clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("replaceFonts"));
            }
        });
        // Also under Arrange, but that is disabled until something is selected (#132).
        big(app, ui, Icon::SelectionPane, "Selection\nPane", "view.pane", json!({"pane": "selection"}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::DesignIdeas, i18n::text(lang, "ribbon.home.design_ideas"), "view.pane", json!({"pane": "designer"}));
    });
}
