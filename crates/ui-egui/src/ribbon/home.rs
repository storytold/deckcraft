use super::arrange::arrange_menu;
use super::galleries::{cref_param, effects_menu, outline_menu, quick_styles, shapes_gallery, smartart_gallery};
use super::{
    Color32, CornerRadius, Icon, Rect, Sense, SlideApp, Stroke, Tokens, Ui, Value, big, big_button, big_button_ex, color_split, dropdown, enabled,
    fmt_state, group, icon_toggle, json, pos2, rows, run, scheme, small, small_button, small_button_ex, small_button_named, toggle, vec2, widgets,
};

// ---------- Home ----------

fn clipboard_group(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let (main, arrow) = big_button_ex(ui, Icon::Paste, "Paste", enabled(app, "edit.paste"), true, false);
        if main.on_hover_text("Paste (⌘V)").clicked() {
            run(app, "edit.paste", json!({}));
        }
        if let Some(a) = arrow {
            egui::Popup::menu(&a).show(|ui| {
                if ui.button(crate::tl!("Paste")).clicked() {
                    run(app, "edit.paste", json!({}));
                }
                if ui.button(crate::tl!("Paste and Match Formatting")).clicked() {
                    run(app, "edit.pasteText", json!({}));
                }
                if ui.button(crate::tl!("Paste Special…")).clicked() {
                    run(app, "app.pasteSpecial", json!({}));
                }
                if ui.button(crate::tl!("Paste Formatting")).clicked() {
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
    group(ui, |ui| {
        let (main, arrow) = big_button_ex(ui, Icon::NewSlide, "New\nSlide", enabled(app, "slide.new"), true, false);
        if main.on_hover_text("New Slide (⇧⌘N)").clicked() {
            run(app, "slide.new", json!({}));
        }
        if let Some(a) = arrow {
            egui::Popup::menu(&a).width(420.0).show(|ui| new_slide_menu(app, ui));
        }
        rows(ui, |ui| {
            let (main, arrow) = small_button_ex(ui, Icon::Layout, "Layout", enabled(app, "slide.layout"), false, true);
            if let Some(arrow) = arrow {
                widgets::split_menu(&main, &arrow, |ui| layout_menu(app, ui, "slide.layout"));
            }
            small(app, ui, Icon::ResetSlide, "Reset", "slide.reset", json!({}));
            let (main, arrow) = small_button_ex(ui, Icon::Section, "Section", enabled(app, "section.add"), false, true);
            if let Some(arrow) = arrow {
                widgets::split_menu(&main, &arrow, |ui| section_menu(app, ui));
            }
        });
    });
}

/// The New Slide button's menu: layouts, duplication, outline import and slide reuse.
fn new_slide_menu(app: &mut SlideApp, ui: &mut Ui) {
    layout_menu(app, ui, "slide.new");
    ui.separator();
    if ui.button(crate::tl!("Duplicate Selected Slides")).clicked() {
        run(app, "slide.duplicate", json!({}));
    }
    if ui.button(crate::tl!("Slides from Outline…")).clicked() {
        app.dialog = Some(crate::dialogs::Dialog::new("outline"));
    }
    let desktop = app.services.pick_open.is_some();
    for (label, keep) in [(crate::tl!("Reuse Slides…"), false), (crate::tl!("Reuse Slides (Keep Source Formatting)…"), true)] {
        let r = ui.add_enabled(desktop, egui::Button::new(label));
        if r.clicked() {
            run(app, "app.reuseSlidesDialog", json!({"keepFormatting": keep}));
        }
        r.on_disabled_hover_text("Choosing a presentation to reuse needs the desktop app");
    }
}

/// The Section split button's menu.
fn section_menu(app: &mut SlideApp, ui: &mut Ui) {
    if ui.button(crate::tl!("Add Section")).clicked() {
        run(app, "section.add", json!({}));
    }
    if ui.button(crate::tl!("Remove All Sections")).clicked() {
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
                            format!("{f} ({})", crate::tl!("Headings"))
                        } else if k == 1 && theme_fonts.len() == 2 {
                            format!("{f} ({})", crate::tl!("Body"))
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
                    if ui.button(crate::tl!("Rescan Installed Fonts")).clicked() {
                        run(app, "font.rescan", json!({}));
                        ui.close();
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
                    for (label, pt) in [
                        (crate::tl!("Very Tight"), -3.0),
                        (crate::tl!("Tight"), -1.5),
                        (crate::tl!("Normal"), 0.0),
                        (crate::tl!("Loose"), 3.0),
                        (crate::tl!("Very Loose"), 6.0),
                    ] {
                        if ui.button(label).clicked() {
                            run(app, "format.spacing", json!({"pt": pt}));
                        }
                    }
                });
                let r = icon_toggle(ui, Icon::ChangeCase, "Change Case", en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for (label, mode) in [
                        (crate::tl!("Sentence case."), "sentence"),
                        (crate::tl!("lowercase"), "lower"),
                        (crate::tl!("UPPERCASE"), "upper"),
                        (crate::tl!("Capitalize Each Word"), "title"),
                        (crate::tl!("tOGGLE cASE"), "toggle"),
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
                    if let Some(c) = widgets::color_grid(ui, &sc, Some(crate::tl!("No Color"))) {
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
                if ui.add_enabled(en, egui::Button::new(crate::tl!("Font…")).min_size(vec2(52.0, 22.0))).on_hover_text("Font").clicked() {
                    run(app, "format.fontDialog", json!({}));
                }
            });
        });
    });
}

/// Render paragraph formatting controls using the current paragraph state.
fn paragraph_group(app: &mut SlideApp, ui: &mut Ui, st: &Value) {
    let en = enabled(app, "format.bullets");
    group(ui, |ui| {
        rows(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                let b = |k: &str| st.get(k).and_then(Value::as_bool).unwrap_or(false);
                let (m, a) = small_button_named(ui, Icon::Bullets, "", "Bullets", en, b("bullets"), true);
                if m.on_hover_text("Bullets").clicked() {
                    run(app, "format.bullets", json!({}));
                }
                if let Some(a) = a {
                    egui::Popup::menu(&a).show(|ui| {
                        if ui.button(crate::tl!("None")).clicked() {
                            run(app, "format.bullets", json!({"on": false}));
                        }
                        for ch in ["•", "○", "■", "□", "◆", "➢", "✓", "–", "★"] {
                            if ui.button(egui::RichText::new(format!("{ch}  Bullet")).size(14.0)).clicked() {
                                run(app, "format.bullets", json!({"on": true, "char": ch}));
                            }
                        }
                        ui.separator();
                        if ui.button(crate::tl!("Bullets and Numbering…")).clicked() {
                            let mut dlg = crate::dialogs::Dialog::new("bullets");
                            dlg.fields.insert("tab".into(), "bulleted".into());
                            app.dialog = Some(dlg);
                        }
                    });
                }
                let (m, a) = small_button_named(ui, Icon::Numbering, "", "Numbering", en, b("numbering"), true);
                if m.on_hover_text("Numbering").clicked() {
                    run(app, "format.numbering", json!({}));
                }
                if let Some(a) = a {
                    egui::Popup::menu(&a).show(|ui| {
                        if ui.button(crate::tl!("None")).clicked() {
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
                        if ui.button(crate::tl!("Bullets and Numbering…")).clicked() {
                            let mut dlg = crate::dialogs::Dialog::new("bullets");
                            dlg.fields.insert("tab".into(), "numbered".into());
                            app.dialog = Some(dlg);
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
                    if ui.button(crate::tl!("Line Spacing Options…")).clicked() {
                        run(app, "format.paragraphDialog", json!({}));
                    }
                    if ui.button(crate::tl!("Tabs…")).clicked() {
                        app.dialog = Some(crate::dialogs::Dialog::new("tabs"));
                    }
                });
                let r = icon_toggle(ui, Icon::Columns, "Columns", en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for n in 1..=3 {
                        if ui.button(format!("{n} Column{}", if n > 1 { "s" } else { "" })).clicked() {
                            run(app, "format.columns", json!({"count": n, "spacing": 18}));
                        }
                    }
                    if ui.button(crate::tl!("More Columns…")).clicked() {
                        run(app, "format.columns", json!({}));
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
                        (crate::tl!("Horizontal"), "horizontal"),
                        (crate::tl!("Rotate all text 90°"), "rotate90"),
                        (crate::tl!("Rotate all text 270°"), "rotate270"),
                        (crate::tl!("Stacked"), "stacked"),
                    ] {
                        if ui.button(l).clicked() {
                            run(app, "format.direction", json!({"dir": d}));
                        }
                    }
                });
                let r = icon_toggle(ui, Icon::AlignText, "Align Text", en, false);
                egui::Popup::menu(&r).show(|ui| {
                    for (l, a) in [(crate::tl!("Top"), "top"), (crate::tl!("Middle"), "middle"), (crate::tl!("Bottom"), "bottom")] {
                        if ui.button(l).clicked() {
                            run(app, "format.anchor", json!({"anchor": a}));
                        }
                    }
                });
                let r = icon_toggle(ui, Icon::SmartArt, "Convert to SmartArt", enabled(app, "smartArt.fromText"), false);
                egui::Popup::menu(&r).show(|ui| smartart_gallery(app, ui, "smartArt.fromText", json!({}), "", ("", ""), None));
                if ui.add_enabled(en, egui::Button::new(crate::tl!("Paragraph…")).min_size(vec2(88.0, 22.0))).on_hover_text("Paragraph").clicked() {
                    run(app, "format.paragraphDialog", json!({}));
                }
            });
        });
    });
}

/// Render drawing controls for shapes, text, arrange, quick styles, fill, and outline.
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
                let c = app.last_fill.clone();
                run(app, "shape.fill", json!({"color": c}));
            }
            if let Some(a) = a {
                widgets::color_popup(&a).show(|ui| {
                    if let Some(c) = widgets::color_grid(ui, &sc, Some(crate::tl!("No Fill"))) {
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
            let (m, a) = small_button_ex(ui, Icon::ShapeOutline, "Shape Outline", sel, false, true);
            if m.clicked() {
                let c = app.last_outline.clone();
                run(app, "shape.line", json!({"color": c}));
            }
            if let Some(a) = a {
                widgets::color_popup(&a).show(|ui| outline_menu(app, ui, &sc));
            }
            let (main, arrow) = small_button_ex(ui, Icon::ShapeEffects, "Shape Effects", sel, false, true);
            if let Some(arrow) = arrow {
                widgets::split_menu(&main, &arrow, |ui| effects_menu(app, ui));
            }
        });
    });
}

pub(super) fn home(app: &mut SlideApp, ui: &mut Ui) {
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
        big(app, ui, Icon::DesignIdeas, "Design\nIdeas", "view.pane", json!({"pane": "designer"}));
    });
}
