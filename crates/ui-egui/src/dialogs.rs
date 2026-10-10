//! Dialogs (Insert Table, Slide Size, Header & Footer, Hyperlink, Set Up Show, Zoom, …), the start
//! screen and the About window.

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::theme::{self, Tokens};
use crate::{SlideApp, ribbon};

#[derive(Clone, Debug, serde::Serialize)]
pub struct Dialog {
    pub id: String,
    pub params: Value,
    /// Field values while the dialog is open.
    #[serde(skip)]
    pub fields: std::collections::HashMap<String, String>,
}

impl Dialog {
    pub fn new(id: &str) -> Self {
        Dialog { id: id.into(), params: Value::Null, fields: Default::default() }
    }
    pub fn modal(&self) -> bool {
        true
    }
    fn get(&mut self, k: &str, default: &str) -> String {
        self.fields.entry(k.into()).or_insert_with(|| default.to_string()).clone()
    }
}

fn title(id: &str) -> &'static str {
    match id {
        "table" => "Insert Table",
        "slideSize" => "Slide Size",
        "headerFooter" => "Header and Footer",
        "hyperlink" => "Insert Hyperlink",
        "setupShow" => "Set Up Slide Show",
        "zoom" => "Zoom",
        "comment" => "New Comment",
        "altText" => "Alt Text",
        "outline" => "Slides from Outline",
        "renameSection" => "Rename Section",
        "renameShape" => "Rename",
        "renameLayout" => "Rename Layout",
        "paragraph" => "Paragraph",
        "about" => "About DeckCraft",
        "preferences" => "Preferences",
        "export" => "Export",
        "symbol" => "Symbol",
        "equation" => "Equation",
        "spelling" => "Spelling",
        "accessibility" => "Accessibility Checker",
        "language" => "Language",
        "customShow" => "Custom Shows",
        "chartData" => "Chart Data",
        "trim" => "Trim Media",
        "smartart" => "Choose a SmartArt Graphic",
        "quit" => "DeckCraft",
        _ => "DeckCraft",
    }
}

pub fn show(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(mut d) = app.dialog.take() else { return };
    let mut open = true;
    let mut close = false;
    egui::Window::new(crate::i18n::tr(title(&d.id)))
        .id(egui::Id::new(("dialog", d.id.clone())))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, -40.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            close = body(app, ui, &mut d);
        });
    if open && !close && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.dialog = Some(d);
    }
}

fn buttons(ui: &mut Ui, ok_label: &str) -> (bool, bool) {
    let mut ok = false;
    let mut cancel = false;
    ui.add_space(8.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let t = Tokens::get(ui.ctx());
        if ui
            .add(egui::Button::new(egui::RichText::new(crate::i18n::tr(ok_label)).color(t.accent_text)).fill(t.accent).min_size(vec2(72.0, 24.0)))
            .clicked()
            || ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            ok = true;
        }
        if ui.add(egui::Button::new(crate::i18n::tr("Cancel")).min_size(vec2(72.0, 24.0))).clicked() {
            cancel = true;
        }
    });
    (ok, cancel)
}

fn field(ui: &mut Ui, d: &mut Dialog, label: &str, key: &str, default: &str) {
    let mut v = d.get(key, default);
    ui.horizontal(|ui| {
        ui.add_sized(vec2(120.0, 20.0), egui::Label::new(crate::i18n::tr(label)));
        ui.add(egui::TextEdit::singleline(&mut v).desired_width(180.0));
    });
    d.fields.insert(key.into(), v);
}

fn check(ui: &mut Ui, d: &mut Dialog, label: &str, key: &str, default: bool) -> bool {
    let mut v = d.get(key, if default { "1" } else { "0" }) == "1";
    ui.checkbox(&mut v, crate::i18n::tr(label));
    d.fields.insert(key.into(), if v { "1".into() } else { "0".into() });
    v
}

fn num(d: &mut Dialog, key: &str, default: f64) -> f64 {
    d.get(key, "").trim().trim_end_matches(['"', '%', ' ']).parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(default)
}

/// Returns true when the dialog should close.
fn body(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let run = |app: &mut SlideApp, id: &str, p: Value| {
        let _ = app.run(id, p);
    };
    match d.id.as_str() {
        "table" => {
            field(ui, d, "Number of columns:", "cols", "5");
            field(ui, d, "Number of rows:", "rows", "2");
            let (ok, cancel) = buttons(ui, "Insert");
            if ok {
                let (r, c) = (num(d, "rows", 2.0) as usize, num(d, "cols", 5.0) as usize);
                run(app, "insert.table", json!({"rows": r, "cols": c}));
            }
            ok || cancel
        }
        "slideSize" => {
            let cur = app.session.active().map(|s| s.doc.slide_size).unwrap_or(deckcraft_model::defaults::WIDE);
            ui.label(crate::i18n::tr("Slides sized for:"));
            let preset = d.get("preset", "");
            egui::ComboBox::from_id_salt("sizes").selected_text(crate::i18n::tr(if preset.is_empty() { "Custom" } else { &preset })).show_ui(
                ui,
                |ui| {
                    for (label, w, h) in deckcraft_model::defaults::SLIDE_SIZES {
                        if ui.selectable_label(preset == *label, crate::i18n::tr(label)).clicked() {
                            d.fields.insert("preset".into(), label.to_string());
                            d.fields.insert("w".into(), format!("{:.2}", w / 72.0));
                            d.fields.insert("h".into(), format!("{:.2}", h / 72.0));
                        }
                    }
                },
            );
            field(ui, d, "Width (in):", "w", &format!("{:.2}", cur.width / 72.0));
            field(ui, d, "Height (in):", "h", &format!("{:.2}", cur.height / 72.0));
            let maximize = check(ui, d, "Maximize content (else ensure fit)", "max", false);
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                let (w, h) = (num(d, "w", cur.width / 72.0) * 72.0, num(d, "h", cur.height / 72.0) * 72.0);
                run(app, "design.slideSize", json!({"w": w, "h": h, "scale": if maximize { "maximize" } else { "ensureFit" }}));
            }
            ok || cancel
        }
        "headerFooter" => {
            let hf = app.session.active().map(|s| s.doc.header_footer.clone()).unwrap_or_default();
            let date = check(ui, d, "Date and time", "date", hf.date);
            field(ui, d, "  Fixed date (empty = update automatically):", "dateText", &hf.date_text);
            let num_on = check(ui, d, "Slide number", "num", hf.slide_number);
            let foot = check(ui, d, "Footer", "footer", hf.footer);
            field(ui, d, "  Footer text:", "footerText", &hf.footer_text);
            let hide = check(ui, d, "Don't show on title slide", "hide", hf.hide_on_title);
            ui.add_space(6.0);
            let mut apply_all = false;
            let mut apply = false;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                apply_all = ui.button(crate::i18n::tr("Apply to All")).clicked();
                apply = ui.button(crate::i18n::tr("Apply")).clicked();
                if ui.button(crate::i18n::tr("Cancel")).clicked() {
                    d.fields.insert("cancel".into(), "1".into());
                }
            });
            if apply || apply_all {
                let dt = d.get("dateText", "");
                let ft = d.get("footerText", "");
                run(
                    app,
                    "design.headerFooter",
                    json!({"date": date, "dateText": dt, "slideNumber": num_on, "footer": foot, "footerText": ft, "hideOnTitle": hide, "all": apply_all}),
                );
            }
            apply || apply_all || d.fields.contains_key("cancel")
        }
        "hyperlink" => {
            let mode = d.get("mode", "url");
            ui.horizontal(|ui| {
                if ui.selectable_label(mode == "url", crate::i18n::tr("Web Page or File")).clicked() {
                    d.fields.insert("mode".into(), "url".into());
                }
                if ui.selectable_label(mode == "slide", crate::i18n::tr("This Document")).clicked() {
                    d.fields.insert("mode".into(), "slide".into());
                }
                if ui.selectable_label(mode == "email", crate::i18n::tr("Email Address")).clicked() {
                    d.fields.insert("mode".into(), "email".into());
                }
            });
            match mode.as_str() {
                "slide" => {
                    let titles: Vec<String> = app
                        .session
                        .active()
                        .map(|s| s.doc.slides.iter().enumerate().map(|(i, sl)| format!("{}. {}", i + 1, sl.title())).collect())
                        .unwrap_or_default();
                    let sel = d.get("slide", "0").parse::<usize>().unwrap_or(0);
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        for (i, t) in titles.iter().enumerate() {
                            if ui.selectable_label(sel == i, t).clicked() {
                                d.fields.insert("slide".into(), i.to_string());
                            }
                        }
                    });
                }
                "email" => field(ui, d, "Email address:", "email", ""),
                _ => field(ui, d, "Address:", "url", "https://"),
            }
            field(ui, d, "ScreenTip:", "tip", "");
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                let tip = d.get("tip", "");
                let p = match mode.as_str() {
                    "slide" => json!({"slide": d.get("slide", "0").parse::<usize>().unwrap_or(0), "tooltip": tip}),
                    "email" => json!({"url": format!("mailto:{}", d.get("email", "")), "tooltip": tip}),
                    _ => json!({"url": d.get("url", ""), "tooltip": tip}),
                };
                run(app, "insert.hyperlink", p);
            }
            ok || cancel
        }
        "setupShow" => {
            let sh = app.session.active().map(|s| s.doc.show.clone()).unwrap_or_default();
            let ty = d.get("type", &sh.show_type);
            ui.label(crate::i18n::tr("Show type"));
            for (l, v) in [
                ("Presented by a speaker (full screen)", "speaker"),
                ("Browsed by an individual (window)", "browsed"),
                ("Browsed at a kiosk (full screen)", "kiosk"),
            ] {
                if ui.radio(ty == v, crate::i18n::tr(l)).clicked() {
                    d.fields.insert("type".into(), v.into());
                }
            }
            let lp = check(ui, d, "Loop continuously until 'Esc'", "loop", sh.loop_until_esc);
            let nn = check(ui, d, "Show without narration", "nn", sh.without_narration);
            let na = check(ui, d, "Show without animation", "na", sh.without_animation);
            let ut = check(ui, d, "Use timings, if present", "ut", sh.use_timings);
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                let ty = d.get("type", "speaker");
                run(app, "show.setup", json!({"type": ty, "loop": lp, "noNarration": nn, "noAnimation": na, "useTimings": ut}));
            }
            ok || cancel
        }
        "quit" => {
            let names: Vec<String> = app.session.documents().iter().filter(|d| d.is_dirty()).map(|d| d.title()).collect();
            if names.is_empty() {
                app.quit_confirmed = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                return true;
            }
            let what = if names.len() == 1 {
                format!("“{}”", names[0])
            } else {
                crate::i18n::count(names.len(), "presentations", "презентації", "презентацій", "презентацій")
            };
            ui.label(
                egui::RichText::new(crate::i18n::format("Do you want to save the changes you made to {what}?", std::slice::from_ref(&what))).strong(),
            );
            ui.label(crate::i18n::tr("Your changes will be lost if you don't save them."));
            ui.add_space(8.0);
            let mut done = false;
            ui.horizontal(|ui| {
                let t = Tokens::get(ui.ctx());
                if ui.add(egui::Button::new(crate::i18n::tr("Don't Save")).min_size(vec2(84.0, 24.0))).clicked() {
                    let _ = app.session.execute("file.recovery.discard", &json!({}));
                    app.quit_confirmed = true;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    done = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save = ui.add(
                        egui::Button::new(egui::RichText::new(crate::i18n::tr("Save")).color(t.accent_text))
                            .fill(t.accent)
                            .min_size(vec2(72.0, 24.0)),
                    );
                    if save.clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if app.save_all() {
                            app.quit_confirmed = true;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        done = true;
                    }
                    if ui.add(egui::Button::new(crate::i18n::tr("Cancel")).min_size(vec2(72.0, 24.0))).clicked() {
                        done = true;
                    }
                });
            });
            done
        }
        "zoom" => {
            for pct in [400, 200, 150, 100, 75, 66, 50, 33] {
                if ui.radio(false, format!("{pct}%")).clicked() {
                    let _ = app.run("view.zoom", json!({"percent": pct}));
                }
            }
            if ui.button(crate::i18n::tr("Fit")).clicked() {
                app.ui.zoom = None;
            }
            let (ok, cancel) = buttons(ui, "OK");
            ok || cancel
        }
        "comment" => {
            let mut v = d.get("text", "");
            ui.add(egui::TextEdit::multiline(&mut v).hint_text(crate::i18n::tr("Start a conversation")).desired_rows(4).desired_width(320.0));
            d.fields.insert("text".into(), v.clone());
            let (ok, cancel) = buttons(ui, "Post");
            if ok && !v.trim().is_empty() {
                run(app, "comment.add", json!({"text": v}));
                app.ui.pane = Some("comments".into());
            }
            ok || cancel
        }
        "altText" => {
            let cur = app.session.active().and_then(|s| s.selected_shapes().first().map(|x| x.descr.clone())).unwrap_or_default();
            ui.label(crate::i18n::tr("How would you describe this object and its context to someone who is blind or has low vision?"));
            let mut v = d.get("text", &cur);
            ui.add(egui::TextEdit::multiline(&mut v).desired_rows(4).desired_width(340.0));
            d.fields.insert("text".into(), v.clone());
            let deco = check(ui, d, "Mark as decorative", "deco", false);
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                run(app, "shape.altText", json!({"text": v, "decorative": deco}));
            }
            ok || cancel
        }
        "outline" => {
            ui.label(crate::i18n::tr("Paste or type an outline. Unindented lines become slide titles; tab-indented lines become bullets."));
            let mut v = d.get("text", "");
            ui.add(egui::TextEdit::multiline(&mut v).desired_rows(10).desired_width(380.0).code_editor());
            d.fields.insert("text".into(), v.clone());
            let (ok, cancel) = buttons(ui, "Insert");
            if ok {
                run(app, "slide.fromOutline", json!({"text": v}));
            }
            ok || cancel
        }
        "renameSection" | "renameShape" | "renameLayout" => {
            let cur = d.params.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            field(ui, d, "Name:", "name", &cur);
            let (ok, cancel) = buttons(ui, "Rename");
            if ok {
                let name = d.get("name", "");
                match d.id.as_str() {
                    "renameSection" => run(app, "section.rename", json!({"index": d.params.get("index").cloned().unwrap_or_default(), "name": name})),
                    "renameShape" => run(app, "shape.rename", json!({"id": d.params.get("id").cloned().unwrap_or_default(), "name": name})),
                    _ => run(app, "master.renameLayout", json!({"name": name})),
                }
            }
            ok || cancel
        }
        "paragraph" => {
            field(ui, d, "Before text (in):", "left", "0");
            field(ui, d, "Special — hanging (in):", "hang", "0");
            field(ui, d, "Spacing before (pt):", "before", "0");
            field(ui, d, "Spacing after (pt):", "after", "0");
            field(ui, d, "Line spacing (lines):", "lines", "1.0");
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                run(
                    app,
                    "format.paragraph",
                    json!({"indentLeft": num(d, "left", 0.0) * 72.0, "indentFirst": -num(d, "hang", 0.0) * 72.0, "spaceBefore": num(d, "before", 0.0), "spaceAfter": num(d, "after", 0.0), "lineSpacing": num(d, "lines", 1.0)}),
                );
            }
            ok || cancel
        }
        "export" => {
            ui.label(crate::i18n::tr("Export the presentation as:"));
            let fmt = d.get("fmt", "pdf");
            for (l, f) in [
                ("PDF document (.pdf)", "pdf"),
                ("PNG images (current slide)", "png"),
                ("PNG images (all slides)", "pngAll"),
                ("JPEG image (current slide)", "jpeg"),
                ("PowerPoint Presentation (.pptx)", "pptx"),
                ("Outline (.txt)", "outline"),
            ] {
                if ui.radio(fmt == f, crate::i18n::tr(l)).clicked() {
                    d.fields.insert("fmt".into(), f.into());
                }
            }
            let pdf_layout = d.get("pdfLayout", "slides");
            if fmt == "pdf" {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Print layout:"));
                    egui::ComboBox::from_id_salt("pdf_layout")
                        .selected_text(match pdf_layout.as_str() {
                            "notes" => "Notes Pages",
                            "h1" => "Handouts (1 slide per page)",
                            "h2" => "Handouts (2 slides per page)",
                            "h3" => "Handouts (3 slides per page)",
                            "h4" => "Handouts (4 slides per page)",
                            "h6" => "Handouts (6 slides per page)",
                            "h9" => "Handouts (9 slides per page)",
                            _ => "Full Page Slides",
                        })
                        .show_ui(ui, |ui| {
                            for (k, l) in [
                                ("slides", "Full Page Slides"),
                                ("notes", "Notes Pages"),
                                ("h1", "Handouts (1 slide per page)"),
                                ("h2", "Handouts (2 slides per page)"),
                                ("h3", "Handouts (3 slides per page)"),
                                ("h4", "Handouts (4 slides per page)"),
                                ("h6", "Handouts (6 slides per page)"),
                                ("h9", "Handouts (9 slides per page)"),
                            ] {
                                if ui.selectable_label(pdf_layout == k, crate::i18n::tr(l)).clicked() {
                                    d.fields.insert("pdfLayout".into(), k.into());
                                }
                            }
                        });
                });
                let mut hidden = d.get("pdfHidden", "false") == "true";
                if ui.checkbox(&mut hidden, crate::i18n::tr("Include hidden slides")).changed() {
                    d.fields.insert("pdfHidden".into(), hidden.to_string());
                }
            }
            let pdf_params = {
                let (layout, per) = match pdf_layout.as_str() {
                    "notes" => ("notes", 0),
                    l if l.starts_with('h') => ("handouts", l[1..].parse().unwrap_or(6)),
                    _ => ("slides", 0),
                };
                json!({"layout": layout, "perPage": per, "includeHidden": d.get("pdfHidden", "false") == "true"})
            };
            let (ok, cancel) = buttons(ui, "Export");
            if ok {
                let name = app.session.active().map(|s| s.title()).unwrap_or_else(|| "Presentation".into());
                let (ext, all) = match fmt.as_str() {
                    "pdf" => ("pdf", false),
                    "pngAll" => ("png", true),
                    "jpeg" => ("jpg", false),
                    "pptx" => ("pptx", false),
                    "outline" => ("txt", false),
                    _ => ("png", false),
                };
                let suggested = format!("{name}.{ext}");
                if let Some(pick) = app.services.pick_save.as_mut() {
                    if let Some(path) = pick(&suggested) {
                        let mut params = json!({"path": path, "all": all});
                        if ext == "pdf"
                            && let (Some(o), Some(extra)) = (params.as_object_mut(), pdf_params.as_object())
                        {
                            o.extend(extra.clone());
                        }
                        let r = app.session.execute("file.export", &params);
                        match r {
                            Ok(_) => app.set_status(crate::i18n::format("Exported {path}", std::slice::from_ref(&path))),
                            Err(e) => app.set_status(e.to_string()),
                        }
                    }
                } else if let (Some(dl), Some(s)) = (app.services.download.as_mut(), app.session.active()) {
                    let bytes = match ext {
                        "png" | "jpg" => {
                            let img = deckcraft_render::render_slide(
                                &s.doc,
                                s.selection.slide,
                                &deckcraft_render::RenderOpts { scale: 2.0, ..Default::default() },
                            );
                            if ext == "png" { img.to_png() } else { img.to_jpeg(92) }
                        }
                        "pdf" => deckcraft_pdf_bytes(&s.doc, &pdf_params),
                        other => deckcraft_engine::cmd::file::save_bytes(&s.doc, other).unwrap_or_default(),
                    };
                    dl(&suggested, &bytes);
                }
            }
            ok || cancel
        }
        "symbol" => {
            ui.label(crate::i18n::tr("Click a symbol to insert it at the insertion point."));
            let groups: [(&str, &str); 6] = [
                ("Punctuation", "–—‘’“”…•·§¶†‡‰′″‹›«»¡¿"),
                ("Currency", "$€£¥¢₹₩₽₺₿"),
                ("Math", "±×÷≠≈≤≥∞√∑∏∫∂∆∇∈∉∩∪⊂⊃∀∃¬∧∨⇒⇔°′"),
                ("Greek", "αβγδεζηθικλμνξοπρστυφχψωΓΔΘΛΞΠΣΦΨΩ"),
                ("Arrows", "←↑→↓↔↕⇐⇑⇒⇓↖↗↘↙⟵⟶"),
                ("Shapes", "■□▪▫▲△▶▷▼▽◀◁◆◇○●◎★☆✓✔✗✘♠♣♥♦"),
            ];
            for (g, chars) in groups {
                ui.label(egui::RichText::new(crate::i18n::tr(g)).font(theme::bold(12.0)));
                ui.horizontal_wrapped(|ui| {
                    for c in chars.chars() {
                        if ui.add(egui::Button::new(egui::RichText::new(c.to_string()).size(16.0)).min_size(vec2(26.0, 26.0))).clicked() {
                            if app.session.active().is_some_and(|s| s.selection.text.is_none()) {
                                let _ = app.run("text.edit", json!({}));
                            }
                            let _ = app.run("insert.symbol", json!({"text": c.to_string()}));
                        }
                    }
                });
            }
            let (ok, cancel) = buttons(ui, "Close");
            ok || cancel
        }
        "equation" => {
            ui.label(crate::i18n::tr("Type an equation in linear form (e.g. a^2+b^2=c^2, x=(-b±√(b^2-4ac))/2a)."));
            let mut v = d.get("eq", "");
            ui.add(egui::TextEdit::singleline(&mut v).desired_width(340.0).font(theme::font(15.0)));
            d.fields.insert("eq".into(), v.clone());
            let (ok, cancel) = buttons(ui, "Insert");
            if ok && !v.is_empty() {
                let pretty = pretty_equation(&v);
                let size = app.session.active().map(|s| s.doc.slide_size).unwrap_or(deckcraft_model::defaults::WIDE);
                run(app, "insert.textBox", json!({"rect": [size.width / 2.0 - 150.0, size.height / 2.0 - 25.0, 300, 50], "text": pretty}));
                run(app, "text.exit", json!({}));
                run(app, "format.font", json!({"family": "Liberation Serif"}));
                run(app, "format.italic", json!({"on": true}));
                run(app, "format.size", json!({"size": 28}));
            }
            ok || cancel
        }
        "spelling" => {
            let v = app.session.execute("review.spelling", &json!({})).unwrap_or_default();
            let list = v.as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                ui.label(crate::i18n::tr("The spelling check is complete. No issues found."));
            }
            for item in &list {
                ui.horizontal(|ui| {
                    let w = item.get("word").and_then(Value::as_str).unwrap_or("");
                    ui.label(crate::i18n::format(
                        "Slide {}: “{w}”",
                        &[(item.get("slide").and_then(Value::as_u64).unwrap_or(0) + 1).to_string(), (w).to_string()],
                    ));
                    if ui.small_button(crate::i18n::tr("Go to")).clicked() {
                        let _ = app.run("slide.go", json!({"index": item.get("slide").cloned().unwrap_or_default()}));
                    }
                });
            }
            let (ok, cancel) = buttons(ui, "Close");
            ok || cancel
        }
        "accessibility" => {
            let v = app.session.execute("review.accessibility", &json!({})).unwrap_or_default();
            let list = v.as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                ui.label(crate::i18n::tr(
                    "No accessibility issues found. People with disabilities should not have difficulty reading this document.",
                ));
            }
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for item in &list {
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::format(
                            "Slide {}: {}{}",
                            &[
                                (item.get("slide").and_then(Value::as_u64).unwrap_or(0) + 1).to_string(),
                                (item.get("issue").and_then(Value::as_str).unwrap_or("")).to_string(),
                                (item.get("name").and_then(Value::as_str).map(|n| format!(" — {n}")).unwrap_or_default()).to_string(),
                            ],
                        ));
                        if ui.small_button(crate::i18n::tr("Go to")).clicked() {
                            let _ = app.run("slide.go", json!({"index": item.get("slide").cloned().unwrap_or_default()}));
                            if let Some(id) = item.get("shape") {
                                let _ = app.run("edit.select", json!({"ids": [id]}));
                            }
                        }
                    });
                }
            });
            let (ok, cancel) = buttons(ui, "Close");
            ok || cancel
        }
        "language" => {
            ui.label("Українська");
            ui.label(crate::i18n::tr("Mark selected text as:"));
            for l in [
                "English (United States)",
                "English (United Kingdom)",
                "Español",
                "Français",
                "Deutsch",
                "Italiano",
                "Português",
                "日本語",
                "中文",
                "العربية",
            ] {
                let _ = ui.selectable_label(l.starts_with("English (United States)"), crate::i18n::tr(l));
            }
            let (ok, cancel) = buttons(ui, "OK");
            ok || cancel
        }
        "customShow" => {
            let shows = app.session.active().map(|s| s.doc.custom_shows.clone()).unwrap_or_default();
            for s in &shows {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{} ({})",
                        s.name,
                        crate::i18n::count(s.slides.len(), if s.slides.len() == 1 { "slide" } else { "slides" }, "слайд", "слайди", "слайдів")
                    ));
                    if ui.small_button(crate::i18n::tr("Delete")).clicked() {
                        let _ = app.run("show.customShow", json!({"name": s.name, "delete": true}));
                    }
                });
            }
            field(ui, d, "New show name:", "name", "Custom Show 1");
            field(ui, d, "Slides (e.g. 1,3,5):", "slides", "1");
            let (ok, cancel) = buttons(ui, "Create");
            if ok {
                let idx: Vec<usize> =
                    d.get("slides", "").split(',').filter_map(|s| s.trim().parse::<usize>().ok()).filter(|v| *v > 0).map(|v| v - 1).collect();
                run(app, "show.customShow", json!({"name": d.get("name", "Custom Show"), "slides": idx}));
            }
            ok || cancel
        }
        "chartData" => {
            let ch = app.session.active().and_then(|s| {
                s.selected_shapes()
                    .into_iter()
                    .find_map(|x| if let deckcraft_model::ShapeKind::Chart(c) = &x.kind { Some(((**c).clone(), x.id)) } else { None })
            });
            let Some((c, id)) = ch else {
                ui.label(crate::i18n::tr("Select a chart first."));
                let (ok, cancel) = buttons(ui, "Close");
                return ok || cancel;
            };
            ui.label(crate::i18n::tr("Edit the data: first row = series names, first column = categories."));
            let mut grid: Vec<Vec<String>> = d.params.get("grid").and_then(|g| serde_json::from_value(g.clone()).ok()).unwrap_or_else(|| {
                let mut g = vec![std::iter::once(String::new()).chain(c.series.iter().map(|s| s.name.clone())).collect::<Vec<_>>()];
                for (i, cat) in c.categories.iter().enumerate() {
                    let mut row = vec![cat.clone()];
                    for s in &c.series {
                        row.push(s.values.get(i).copied().flatten().map(|v| v.to_string()).unwrap_or_default());
                    }
                    g.push(row);
                }
                g
            });
            egui::Grid::new("chartdata").striped(true).show(ui, |ui| {
                for row in grid.iter_mut() {
                    for cell in row.iter_mut() {
                        ui.add(egui::TextEdit::singleline(cell).desired_width(70.0));
                    }
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("+ Row")).clicked() {
                    let n = grid.first().map(|r| r.len()).unwrap_or(2);
                    grid.push(vec![String::new(); n]);
                }
                if ui.button(crate::i18n::tr("+ Series")).clicked() {
                    for r in grid.iter_mut() {
                        r.push(String::new());
                    }
                }
            });
            d.params = json!({"grid": grid});
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                let names: Vec<String> = grid.first().map(|r| r.iter().skip(1).cloned().collect()).unwrap_or_default();
                let cats: Vec<String> = grid.iter().skip(1).map(|r| r.first().cloned().unwrap_or_default()).collect();
                let series: Vec<Value> = names.iter().enumerate().map(|(k, n)| json!({"name": n, "values": grid.iter().skip(1).map(|r| r.get(k + 1).and_then(|v| v.trim().parse::<f64>().ok())).collect::<Vec<_>>()})).collect();
                run(app, "chart.data", json!({"id": id.0, "categories": cats, "series": series}));
            }
            ok || cancel
        }
        "trim" => {
            field(ui, d, "Start time (s):", "start", "0");
            field(ui, d, "End trim (s):", "end", "0");
            field(ui, d, "Fade in (s):", "fin", "0");
            field(ui, d, "Fade out (s):", "fout", "0");
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                run(
                    app,
                    "media.options",
                    json!({"trimStart": (num(d, "start", 0.0) * 1000.0) as u64, "trimEnd": (num(d, "end", 0.0) * 1000.0) as u64, "fadeIn": (num(d, "fin", 0.0) * 1000.0) as u64, "fadeOut": (num(d, "fout", 0.0) * 1000.0) as u64}),
                );
            }
            ok || cancel
        }
        "smartart" => {
            ui.label(crate::i18n::tr("Pick a graphic; type one item per line."));
            let kind = d.get("kind", "process");
            ui.horizontal_wrapped(|ui| {
                for (k, l) in [
                    ("list", "Basic List"),
                    ("process", "Basic Process"),
                    ("cycle", "Basic Cycle"),
                    ("hierarchy", "Hierarchy"),
                    ("pyramid", "Pyramid"),
                    ("matrix", "Matrix"),
                ] {
                    if ui.selectable_label(kind == k, crate::i18n::tr(l)).clicked() {
                        d.fields.insert("kind".into(), k.into());
                    }
                }
            });
            let mut v = d.get("text", "Plan\nBuild\nLaunch");
            ui.add(egui::TextEdit::multiline(&mut v).desired_rows(5).desired_width(320.0));
            d.fields.insert("text".into(), v.clone());
            let (ok, cancel) = buttons(ui, "Insert");
            if ok {
                run(app, "insert.smartArt", json!({"kind": kind, "items": v.lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>()}));
            }
            ok || cancel
        }
        "about" => {
            let tab = d.get("tab", "about");
            ui.horizontal(|ui| {
                for (k, l) in [("about", "About"), ("contributors", "Contributors"), ("models", "Models")] {
                    if ui.selectable_label(tab == k, crate::i18n::tr(l)).clicked() {
                        d.fields.insert("tab".into(), k.into());
                    }
                }
            });
            ui.separator();
            match tab.as_str() {
                "contributors" | "models" => {
                    // The credits need room for the table; the About tab keeps its compact size.
                    ui.set_min_size(vec2(640.0, 380.0));
                    ui.set_max_height(380.0);
                    if tab == "models" { crate::credits::models_ui(ui) } else { crate::credits::contributors_ui(ui) }
                }
                _ => about(app, ui),
            }
            let (ok, _) = buttons(ui, "Close");
            ok
        }
        "preferences" => {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Interface language:"));
                egui::ComboBox::from_id_salt("interface_language").selected_text(app.ui.interface_language.label()).show_ui(ui, |ui| {
                    for language in crate::i18n::Language::ALL {
                        if ui.selectable_label(app.ui.interface_language == language, crate::i18n::tr(language.label())).clicked() {
                            let code = match language {
                                crate::i18n::Language::System => "system",
                                crate::i18n::Language::En => "en",
                                crate::i18n::Language::Uk => "uk",
                            };
                            let _ = app.run("app.language", json!({"language": code}));
                        }
                    }
                });
            });
            ui.label(egui::RichText::new(crate::i18n::tr("General")).font(theme::bold(13.0)));
            let mut dark = app.ui.brightness == theme::Brightness::Dark;
            if ui.checkbox(&mut dark, crate::i18n::tr("Dark appearance")).changed() {
                let _ = app.run("view.dark", json!({"on": dark}));
            }
            let mut scale = app.ui.ui_scale;
            if ui.add(egui::Slider::new(&mut scale, 0.75..=1.75).text(crate::i18n::tr("Interface size"))).changed() {
                app.ui.ui_scale = scale;
            }
            ui.label(egui::RichText::new(crate::i18n::tr("Editing")).font(theme::bold(13.0)));
            ui.checkbox(&mut app.session.prefs.smart_guides, crate::i18n::tr("Show smart guides"));
            ui.checkbox(&mut app.session.prefs.snap_to_grid, crate::i18n::tr("Snap objects to grid"));
            ui.checkbox(&mut app.session.prefs.smart_quotes, crate::i18n::tr("Replace straight quotes with smart quotes"));
            ui.checkbox(&mut app.session.prefs.autocorrect, crate::i18n::tr("AutoCorrect as you type"));
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Your name (comments):"));
                ui.text_edit_singleline(&mut app.session.prefs.author);
            });
            let (ok, _) = buttons(ui, "Done");
            ok
        }
        _ => {
            ui.label(crate::i18n::format("“{}” is not available yet.", std::slice::from_ref(&d.id)));
            let (ok, cancel) = buttons(ui, "OK");
            ok || cancel
        }
    }
}

fn pretty_equation(s: &str) -> String {
    let sup = |c: char| match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        'n' => 'ⁿ',
        'i' => 'ⁱ',
        '+' => '⁺',
        '-' => '⁻',
        _ => c,
    };
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '^' => {
                while let Some(&n) = chars.peek() {
                    if n.is_ascii_alphanumeric() || n == '+' || n == '-' {
                        out.push(sup(n));
                        chars.next();
                        if !n.is_ascii_digit() {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            '*' => out.push('·'),
            _ => out.push(c),
        }
    }
    out.replace("sqrt", "√").replace("<=", "≤").replace(">=", "≥").replace("!=", "≠").replace("pi", "π").replace("+-", "±")
}

pub fn about(_app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.vertical_centered(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
        paint_logo(ui.painter(), r);
        ui.label(egui::RichText::new(crate::i18n::tr("DeckCraft")).font(theme::bold(22.0)));
        ui.label(egui::RichText::new(version_line()).color(t.text_dim));
        ui.add_space(6.0);
        ui.label(crate::i18n::tr("Presentations and slide shows, rebuilt from scratch in pure Rust."));
        ui.label(crate::i18n::tr("An open-source, clean-room project. Not affiliated with Microsoft."));
        ui.add_space(6.0);
        ui.hyperlink_to(crate::i18n::tr("Join our community on Discord"), deckcraft_engine::links::DISCORD);
        ui.hyperlink_to("getartcraft.com/apps/deckcraft", deckcraft_engine::links::APP_PAGE);
        ui.hyperlink_to(crate::i18n::tr("Source on GitHub"), deckcraft_engine::links::GITHUB);
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(crate::i18n::tr("MIT OR Apache-2.0 · © 2026 ArtCraft Team and the DeckCraft contributors"))
                .size(11.0)
                .color(t.text_faint),
        );
    });
}

/// "Version X.Y.Z" plus, for release builds, the short commit and build date. The version comes
/// from `[workspace.package] version` (the single source of truth, `cargo xtask version`); the
/// release workflow sets `DECKCRAFT_BUILD_SHA` / `DECKCRAFT_BUILD_DATE` at build time.
pub fn version_line() -> String {
    let mut s = crate::i18n::format("Version {}", &[(env!("CARGO_PKG_VERSION")).to_string()]);
    let sha = option_env!("DECKCRAFT_BUILD_SHA").map(|s| s.get(..9).unwrap_or(s)).filter(|s| !s.is_empty());
    let date = option_env!("DECKCRAFT_BUILD_DATE").filter(|s| !s.is_empty());
    match (sha, date) {
        (Some(sha), Some(date)) => s.push_str(&format!(" ({sha}, {date})")),
        (Some(x), None) | (None, Some(x)) => s.push_str(&format!(" ({x})")),
        (None, None) => {}
    }
    s
}

/// DeckCraft's mark, drawn in code: a slide card with a play triangle in our orange.
pub fn paint_logo(p: &egui::Painter, r: Rect) {
    let orange = Color32::from_rgb(0xF2, 0x6B, 0x1D);
    let deep = Color32::from_rgb(0xC2, 0x4E, 0x14);
    let card = r.shrink(r.width() * 0.08);
    p.rect_filled(card.translate(vec2(r.width() * 0.04, r.width() * 0.05)), CornerRadius::same((r.width() * 0.14) as u8), deep);
    p.rect_filled(card, CornerRadius::same((r.width() * 0.14) as u8), orange);
    let c = card.center();
    let s = card.width() * 0.22;
    p.add(egui::Shape::convex_polygon(
        vec![pos2(c.x - s * 0.7, c.y - s), pos2(c.x + s, c.y), pos2(c.x - s * 0.7, c.y + s)],
        Color32::WHITE,
        Stroke::NONE,
    ));
}

/// The start screen (no presentation open): new from themes, open, recent files.
pub fn start_screen(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(24.0);
        ui.horizontal(|ui| {
            ui.add_space(40.0);
            let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
            paint_logo(ui.painter(), r);
            ui.label(egui::RichText::new(crate::i18n::tr("DeckCraft")).font(theme::bold(26.0)));
        });
        if app.home_open() {
            let title = app.session.active().map(|d| d.title()).unwrap_or_default();
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(40.0);
                if ui.button(format!("← Back to {title}")).on_hover_text("Esc").clicked() {
                    let _ = app.run("app.home", json!({"on": false}));
                }
            });
        }
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.add_space(40.0);
            ui.label(egui::RichText::new(crate::i18n::tr("New presentation")).font(theme::bold(16.0)));
        });
        ui.add_space(8.0);
        inset_wrapped(ui, |ui| {
            for th in deckcraft_model::theme::builtin_themes() {
                let (r, resp) = ui.allocate_exact_size(vec2(180.0, 128.0), Sense::click());
                let tile = Rect::from_min_size(r.min, vec2(180.0, 101.0));
                ribbon::paint_theme_tile(ui.painter(), tile, &th);
                if resp.hovered() {
                    ui.painter().rect_stroke(tile, CornerRadius::same(2), Stroke::new(2.0, t.accent), egui::StrokeKind::Outside);
                }
                ui.painter().text(pos2(r.min.x, r.max.y - 10.0), Align2::LEFT_CENTER, &th.name, theme::font(12.0), t.text);
                if resp.clicked() {
                    let _ = app.run("file.new", json!({"theme": th.name}));
                }
            }
        });
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.add_space(40.0);
            if ui.button(crate::i18n::tr("Open…")).clicked() {
                let _ = app.run("app.openDialog", json!({}));
            }
            if ui.button(crate::i18n::tr("Open the sample deck")).clicked()
                && let Err(e) = deckcraft_engine::sample::open_sample(&mut app.session)
            {
                app.set_status(e.to_string());
            }
        });
        if !app.ui.recent.is_empty() {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(40.0);
                ui.label(egui::RichText::new(crate::i18n::tr("Recent")).font(theme::bold(16.0)));
            });
            for p in app.ui.recent.clone() {
                ui.horizontal(|ui| {
                    ui.add_space(40.0);
                    if ui.link(&p).clicked()
                        && let Err(e) = app.open_path(&p)
                    {
                        app.set_status(e);
                    }
                });
            }
        }
        ui.add_space(30.0);
        ui.horizontal(|ui| {
            ui.add_space(40.0);
            ui.hyperlink_to(crate::i18n::tr("Join the ArtCraft community on Discord"), deckcraft_engine::links::DISCORD);
        });
    });
}

/// Wrapped rows that all start 40 points in from the left, in line with the start screen's
/// headings. The inset is a margin on the container: an `add_space` inside `horizontal_wrapped`
/// would indent only the first row.
fn inset_wrapped(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    egui::Frame::NONE.inner_margin(egui::Margin { left: 40, ..Default::default() }).show(ui, |ui| {
        ui.horizontal_wrapped(add_contents);
    });
}

/// PDF bytes with the export dialog's options (web: downloaded instead of written).
fn deckcraft_pdf_bytes(doc: &deckcraft_model::Presentation, params: &serde_json::Value) -> Vec<u8> {
    deckcraft_engine::cmd::file::pdf_bytes(doc, params).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// Start screen theme tiles that wrap onto a second row keep the 40-point inset (#17).
    #[test]
    fn inset_wrapped_indents_every_row() {
        let left = Cell::new(0.0_f32);
        let tiles = RefCell::new(Vec::<Rect>::new());
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(500.0, 400.0)).build_ui(|ui| {
            left.set(ui.max_rect().left());
            tiles.borrow_mut().clear();
            inset_wrapped(ui, |ui| {
                for _ in 0..4 {
                    let (r, _) = ui.allocate_exact_size(vec2(180.0, 128.0), Sense::click());
                    tiles.borrow_mut().push(r);
                }
            });
        });
        harness.run();
        let tiles = tiles.borrow().clone();
        let first_top = tiles.first().map_or(0.0, |r| r.top());
        assert!(tiles.iter().any(|r| r.top() > first_top), "tiles should wrap onto a second row: {tiles:?}");
        let mut row_top = f32::NEG_INFINITY;
        for r in &tiles {
            if r.top() > row_top {
                row_top = r.top();
                assert!((r.left() - (left.get() + 40.0)).abs() < 0.5, "row starts at {} instead of {}", r.left(), left.get() + 40.0);
            }
        }
    }
}
