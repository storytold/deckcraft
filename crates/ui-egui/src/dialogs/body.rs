use super::about::{about, credits_block};
use super::alt_text::alt_text_starts_decorative;
use super::columns::columns_dialog;
use super::date_time::{date_rows, date_time_dialog};
use super::equation::pretty_equation;
use super::export::gif_options;
use super::find::find_dialog;
use super::font::font_dialog;
use super::grid_options::grid_options;
use super::hyperlink::{hyperlink_command, hyperlink_seed_of, hyperlink_should_apply};
use super::language::selected_language;
use super::paragraph::paragraph_dialog;
use super::photo_album::photo_album_dialog;
use super::preferences::{preferences_snapshot, restore_preferences};
use super::size_position::size_position_dialog;
use super::slide_size::{put_slide_dim, slide_dim_points, unit_abbr};
use super::tabs::tabs_dialog;
use super::theme_colors::theme_colors_dialog;
use super::theme_fonts::theme_fonts_dialog;
use super::trim::{seconds_text_to_ms, trim_seed};
use super::zoom::zoom_dialog;
use super::{Dialog, SlideApp, Tokens, Ui, Value, buttons, check, field, field_of, json, num, theme, vec2};

/// Returns true when the dialog should close.
pub(super) fn body(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    match d.id.as_str() {
        "table" => dlg_table(app, ui, d),
        "slideSize" => dlg_slide_size(app, ui, d),
        "sizePosition" => size_position_dialog(app, ui, d),
        "headerFooter" => dlg_header_footer(app, ui, d),
        "hyperlink" => dlg_hyperlink(app, ui, d),
        "setupShow" => dlg_setup_show(app, ui, d),
        "quit" => dlg_quit(app, ui, d),
        "close" => dlg_close(app, ui, d),
        "zoom" => zoom_dialog(app, ui, d),
        "comment" => dlg_comment(app, ui, d),
        "bullets" => dlg_bullets(app, ui, d),
        "action" => dlg_action(app, ui, d),
        "altText" => dlg_alt_text(app, ui, d),
        "outline" => dlg_outline(app, ui, d),
        "reuseSlides" => dlg_reuse_slides(app, ui, d),
        "renameSection" | "renameShape" | "renameLayout" => dlg_rename_section(app, ui, d),
        "font" => font_dialog(app, ui, d),
        "paragraph" => paragraph_dialog(app, ui, d),
        "tabs" => tabs_dialog(app, ui, d),
        "export" => dlg_export(app, ui, d),
        "replaceFonts" => dlg_replace_fonts(app, ui, d),
        "symbol" => dlg_symbol(app, ui, d),
        "equation" => dlg_equation(app, ui, d),
        "pasteSpecial" => dlg_paste_special(app, ui, d),
        "find" => find_dialog(app, ui, d),
        "spelling" => dlg_spelling(app, ui, d),
        "accessibility" => dlg_accessibility(app, ui, d),
        "language" => dlg_language(app, ui, d),
        "customShow" => dlg_custom_show(app, ui, d),
        "chartData" => dlg_chart_data(app, ui, d),
        "trim" => dlg_trim(app, ui, d),
        "smartart" => dlg_smartart(app, ui, d),
        "about" => dlg_about(app, ui, d),
        "autocorrect" => dlg_autocorrect(app, ui, d),
        "gridOptions" => grid_options(app, ui, d),
        "columns" => columns_dialog(app, ui, d),
        "preferences" => dlg_preferences(app, ui, d),
        "photoAlbum" => photo_album_dialog(app, ui, d),
        "themeColors" => theme_colors_dialog(app, ui, d),
        "themeFonts" => theme_fonts_dialog(app, ui, d),
        "dateTime" => date_time_dialog(app, ui, d),
        _ => {
            ui.label(format!("“{}” is not available yet.", d.id));
            let (ok, cancel) = buttons(ui, "OK");
            ok || cancel
        }
    }
}

fn run(app: &mut SlideApp, id: &str, p: Value) {
    let _ = app.run(id, p);
}

fn dlg_table(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    field(ui, d, "Number of columns:", "cols", "5");
    field(ui, d, "Number of rows:", "rows", "2");
    let (ok, cancel) = buttons(ui, "Insert");
    if ok {
        let (r, c) = (num(d, "rows", 2.0) as usize, num(d, "cols", 5.0) as usize);
        run(app, "insert.table", json!({"rows": r, "cols": c}));
    }
    ok || cancel
}

fn dlg_slide_size(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let cur = app.session.active().map(|s| s.doc.slide_size).unwrap_or(deckcraft_model::defaults::WIDE);
    let len = app.session.prefs.units;
    let ppu = len.points_per_unit();
    // Each field keeps its exact size in points (`wPt`) beside the two decimals shown, so
    // a preset, Portrait or an untouched field is not nudged off by the rounding (13.33",
    // 33.87 cm read back would make the slide 959.76 or 960.09 pt wide).
    if !d.fields.contains_key("wPt") {
        put_slide_dim(d, "w", cur.width, ppu);
        put_slide_dim(d, "h", cur.height, ppu);
    }
    let label = ui.label("Slides sized for:");
    let preset = d.get("preset", "");
    let combo = craft_ui::ComboBox::from_id_salt("sizes").selected_text(if preset.is_empty() { "Custom" } else { &preset }).show_ui(ui, |ui| {
        for (label, w, h) in deckcraft_model::defaults::SLIDE_SIZES {
            if ui.selectable_label(preset == *label, *label).clicked() {
                d.fields.insert("preset".into(), label.to_string());
                put_slide_dim(d, "w", *w, ppu);
                put_slide_dim(d, "h", *h, ppu);
            }
        }
    });
    let _ = combo.response.labelled_by(label.id);
    let abbr = unit_abbr(len);
    let (w_seed, h_seed) = (field_of(d, "w"), field_of(d, "h"));
    field(ui, d, &format!("Width ({abbr}):"), "w", &w_seed);
    field(ui, d, &format!("Height ({abbr}):"), "h", &h_seed);
    let (w, h) = (slide_dim_points(d, "w", ppu, cur.width), slide_dim_points(d, "h", ppu, cur.height));
    ui.horizontal(|ui| {
        ui.label("Orientation:");
        let portrait = ui.radio(h > w, "Portrait").clicked();
        let landscape = ui.radio(w >= h, "Landscape").clicked();
        // Picking the other orientation swaps the width and height fields.
        if portrait && w > h || landscape && h > w {
            put_slide_dim(d, "w", h, ppu);
            put_slide_dim(d, "h", w, ppu);
        }
    });
    let maximize = check(ui, d, "Maximize content (else ensure fit)", "max", false);
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        let (w, h) = (slide_dim_points(d, "w", ppu, cur.width), slide_dim_points(d, "h", ppu, cur.height));
        run(app, "design.slideSize", json!({"w": w, "h": h, "scale": if maximize { "maximize" } else { "ensureFit" }}));
    }
    ok || cancel
}

fn dlg_header_footer(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let hf = app.session.active().map(|s| s.doc.header_footer.clone()).unwrap_or_default();
    let date = check(ui, d, "Date and time", "date", hf.date);
    field(ui, d, "  Fixed date (empty = update automatically):", "dateText", &hf.date_text);
    // The format applies to an automatic date; a fixed one is the text as typed.
    let automatic = d.get("dateText", &hf.date_text).is_empty();
    let fmt_now = if hf.date_format.is_empty() { "datetime1".to_string() } else { hf.date_format.clone() };
    // Not `d.get`: that stores the default as if the user had picked it.
    let fmt = d.fields.get("dateFormat").cloned().unwrap_or_else(|| fmt_now.clone());
    let rows = date_rows("en-US");
    let label = rows.iter().find(|(k, _)| *k == fmt).map(|(_, s)| s.clone()).unwrap_or_else(|| fmt.clone());
    ui.add_enabled_ui(date && automatic, |ui| {
        ui.horizontal(|ui| {
            ui.label("  Format:");
            craft_ui::ComboBox::from_id_salt("hf-date-format").selected_text(label).width(200.0).show_ui(ui, |ui| {
                for (kind, sample) in &rows {
                    if ui.selectable_label(fmt == *kind, sample).clicked() {
                        d.fields.insert("dateFormat".into(), (*kind).to_string());
                    }
                }
            });
        });
    });
    let num_on = check(ui, d, "Slide number", "num", hf.slide_number);
    let foot = check(ui, d, "Footer", "footer", hf.footer);
    field(ui, d, "  Footer text:", "footerText", &hf.footer_text);
    let hide = check(ui, d, "Don't show on title slide", "hide", hf.hide_on_title);
    ui.add_space(6.0);
    let mut apply_all = false;
    let mut apply = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        apply_all = ui.button("Apply to All").clicked();
        apply = ui.button("Apply").clicked();
        if ui.button("Cancel").clicked() {
            d.fields.insert("cancel".into(), "1".into());
        }
    });
    if apply || apply_all {
        let dt = d.get("dateText", "");
        let ft = d.get("footerText", "");
        let mut params = serde_json::Map::new();
        params.insert("date".into(), json!(date));
        params.insert("dateText".into(), json!(dt));
        // Only a format the user picked: sending the default would rewrite every slide's
        // existing date field (an imported datetime4, say) to datetime1.
        if d.fields.contains_key("dateFormat") {
            params.insert("dateFormat".into(), json!(d.get("dateFormat", &fmt_now)));
        }
        params.insert("slideNumber".into(), json!(num_on));
        params.insert("footer".into(), json!(foot));
        params.insert("footerText".into(), json!(ft));
        params.insert("hideOnTitle".into(), json!(hide));
        params.insert("all".into(), json!(apply_all));
        run(app, "design.headerFooter", serde_json::Value::Object(params));
    }
    apply || apply_all || d.fields.contains_key("cancel")
}

fn dlg_hyperlink(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let seed = hyperlink_seed_of(app.session.active());
    let slide_default = seed.slide.to_string();
    let mode = d.get("mode", seed.mode);
    ui.horizontal(|ui| {
        if ui.selectable_label(mode == "url", "Web Page or File").clicked() {
            d.fields.insert("mode".into(), "url".into());
        }
        if ui.selectable_label(mode == "slide", "This Document").clicked() {
            d.fields.insert("mode".into(), "slide".into());
        }
        if ui.selectable_label(mode == "email", "Email Address").clicked() {
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
            let sel = d.get("slide", &slide_default).parse::<usize>().unwrap_or(0);
            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                for (i, t) in titles.iter().enumerate() {
                    if ui.selectable_label(sel == i, t).clicked() {
                        d.fields.insert("slide".into(), i.to_string());
                    }
                }
            });
        }
        "email" => field(ui, d, "Email address:", "email", &seed.email),
        _ => field(ui, d, "Address:", "url", &seed.url),
    }
    field(ui, d, "ScreenTip:", "tip", &seed.tip);
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        let mode_now = d.get("mode", seed.mode);
        let url = d.get("url", &seed.url);
        let email = d.get("email", &seed.email);
        let slide = d.get("slide", &slide_default).parse::<usize>().unwrap_or(0);
        let tip = d.get("tip", &seed.tip);
        if hyperlink_should_apply(&seed, &mode_now, &url, &email, slide) {
            run(app, "insert.hyperlink", hyperlink_command(&mode_now, &url, &email, slide, &tip, seed.highlight));
        }
    }
    ok || cancel
}

fn dlg_setup_show(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let sh = app.session.active().map(|s| s.doc.show.clone()).unwrap_or_default();
    let ty = d.get("type", &sh.show_type);
    ui.label("Show type");
    for (l, v) in [
        ("Presented by a speaker (full screen)", "speaker"),
        ("Browsed by an individual (window)", "browsed"),
        ("Browsed at a kiosk (full screen)", "kiosk"),
    ] {
        if ui.radio(ty == v, l).clicked() {
            d.fields.insert("type".into(), v.into());
        }
    }
    let lp = check(ui, d, "Loop continuously until 'Esc'", "loop", sh.loop_until_esc);
    let nn = check(ui, d, "Show without narration", "nn", sh.without_narration);
    let na = check(ui, d, "Show without animation", "na", sh.without_animation);
    let ut = check(ui, d, "Use timings, if present", "ut", sh.use_timings);
    let kc = check(ui, d, "Keep mouse cursor visible", "kc", app.ui.show_keep_cursor);
    let names: Vec<String> = app.session.active().map(|s| s.doc.custom_shows.iter().map(|c| c.name.clone()).collect()).unwrap_or_default();
    let chosen = d.fields.entry("custom".into()).or_insert_with(|| sh.custom_show.clone().unwrap_or_default()).clone();
    let field = ui.label("Slides");
    let label = if chosen.is_empty() { "All slides".to_string() } else { chosen.clone() };
    let combo = craft_ui::ComboBox::from_id_salt("setup_custom").selected_text(label).show_ui(ui, |ui| {
        if ui.selectable_label(chosen.is_empty(), "All slides").clicked() {
            d.fields.insert("custom".into(), String::new());
        }
        for n in &names {
            if ui.selectable_label(&chosen == n, n).clicked() {
                d.fields.insert("custom".into(), n.clone());
            }
        }
    });
    let _ = combo.response.labelled_by(field.id);
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        let ty = d.get("type", "speaker");
        let custom = d.fields.get("custom").cloned().unwrap_or_default();
        run(app, "show.setup", json!({"type": ty, "loop": lp, "noNarration": nn, "noAnimation": na, "useTimings": ut, "custom": custom}));
        run(app, "show.keepCursor", json!({"on": kc}));
    }
    ok || cancel
}

fn dlg_quit(app: &mut SlideApp, ui: &mut Ui, _d: &mut Dialog) -> bool {
    let names: Vec<String> = app.session.documents().iter().filter(|d| d.is_dirty()).map(|d| d.title()).collect();
    if names.is_empty() {
        app.quit_confirmed = true;
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        return true;
    }
    let what = if names.len() == 1 { format!("“{}”", names[0]) } else { format!("{} presentations", names.len()) };
    ui.label(egui::RichText::new(format!("Do you want to save the changes you made to {what}?")).strong());
    ui.label("Your changes will be lost if you don't save them.");
    ui.add_space(8.0);
    let mut done = false;
    ui.horizontal(|ui| {
        let t = Tokens::get(ui.ctx());
        if ui.add(egui::Button::new("Don't Save").min_size(vec2(84.0, 24.0))).clicked() {
            let _ = app.session.execute("file.recovery.discard", &json!({}));
            app.quit_confirmed = true;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            done = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let save = ui.add(egui::Button::new(egui::RichText::new("Save").color(t.accent_text)).fill(t.accent).min_size(vec2(72.0, 24.0)));
            if save.clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if app.save_all() {
                    app.quit_confirmed = true;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                done = true;
            }
            if ui.add(egui::Button::new("Cancel").min_size(vec2(72.0, 24.0))).clicked() {
                done = true;
            }
        });
    });
    done
}

fn dlg_close(app: &mut SlideApp, ui: &mut Ui, _d: &mut Dialog) -> bool {
    // File › Close and Ctrl+W on a presentation with unsaved changes.
    let Some(name) = app.session.active().map(|d| d.title()) else { return true };
    ui.label(egui::RichText::new(format!("Do you want to save the changes you made to “{name}”?")).strong());
    ui.label("Your changes will be lost if you don't save them.");
    ui.add_space(8.0);
    let mut done = false;
    ui.horizontal(|ui| {
        let t = Tokens::get(ui.ctx());
        if ui.add(egui::Button::new("Don't Save").min_size(vec2(84.0, 24.0))).clicked() {
            run(app, "file.close", json!({}));
            done = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let save = ui.add(egui::Button::new(egui::RichText::new("Save").color(t.accent_text)).fill(t.accent).min_size(vec2(72.0, 24.0)));
            if save.clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                app.save();
                // In the browser a save is a download: the presentation stays "unsaved".
                let web = app.services.pick_save.is_none() && app.services.download.is_some();
                if web || app.session.active().is_some_and(|d| !d.is_dirty()) {
                    run(app, "file.close", json!({}));
                }
                done = true;
            }
            if ui.add(egui::Button::new("Cancel").min_size(vec2(72.0, 24.0))).clicked() {
                done = true;
            }
        });
    });
    done
}

fn dlg_comment(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let mut v = d.get("text", "");
    ui.add(egui::TextEdit::multiline(&mut v).hint_text("Comment").desired_rows(4).desired_width(320.0));
    d.fields.insert("text".into(), v.clone());
    let (ok, cancel) = buttons(ui, "Post");
    if ok && !v.trim().is_empty() {
        run(app, "comment.add", json!({"text": v}));
        app.ui.pane = Some("comments".into());
    }
    ok || cancel
}

fn dlg_bullets(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    // The current paragraph's list, as the dialog's starting point.
    let cur = app.session.execute("format.state", &json!({})).ok().unwrap_or(Value::Null);
    let numbered = cur.get("numbering").and_then(Value::as_bool).unwrap_or(false);
    let tab = d.get("tab", if numbered { "numbered" } else { "bulleted" });
    ui.horizontal(|ui| {
        for (k, l) in [("bulleted", "Bulleted"), ("numbered", "Numbered")] {
            if ui.selectable_label(tab == k, l).clicked() {
                d.fields.insert("tab".into(), k.into());
            }
        }
    });
    ui.separator();
    let style_key = if tab == "numbered" { "scheme" } else { "char" };
    let current = d.get(style_key, if tab == "numbered" { "arabicPeriod" } else { "•" });
    let styles: &[(&str, &str)] = if tab == "numbered" {
        &[
            ("arabicPeriod", "1. 2. 3."),
            ("arabicParenR", "1) 2) 3)"),
            ("romanUcPeriod", "I. II. III."),
            ("romanLcPeriod", "i. ii. iii."),
            ("alphaUcPeriod", "A. B. C."),
            ("alphaLcParenR", "a) b) c)"),
            ("alphaLcPeriod", "a. b. c."),
        ]
    } else {
        &[("•", "•"), ("○", "○"), ("■", "■"), ("□", "□"), ("◆", "◆"), ("➢", "➢"), ("✓", "✓"), ("–", "–"), ("★", "★")]
    };
    egui::Grid::new("bullet-styles").show(ui, |ui| {
        for (i, (k, l)) in styles.iter().enumerate() {
            let r = ui.add_sized(
                vec2(if tab == "numbered" { 88.0 } else { 44.0 }, 40.0),
                egui::Button::selectable(current == *k, egui::RichText::new(*l).size(16.0)),
            );
            if r.clicked() {
                d.fields.insert(style_key.into(), (*k).into());
            }
            if (i + 1) % if tab == "numbered" { 4 } else { 5 } == 0 {
                ui.end_row();
            }
        }
    });
    if tab == "bulleted" {
        field(ui, d, "Custom character:", "custom", "");
    } else {
        field(ui, d, "Start at:", "start", "1");
    }
    field(ui, d, "Size (% of text):", "size", "100");
    let colors = [
        ("", "Automatic"),
        ("tx1", "Text"),
        ("accent1", "Accent 1"),
        ("accent2", "Accent 2"),
        ("accent3", "Accent 3"),
        ("accent4", "Accent 4"),
        ("accent5", "Accent 5"),
        ("accent6", "Accent 6"),
    ];
    let color = d.get("color", "");
    ui.horizontal(|ui| {
        let field = ui.add_sized(vec2(120.0, 20.0), egui::Label::new("Color:"));
        let combo = craft_ui::ComboBox::from_id_salt("bullet-color")
            .selected_text(colors.iter().find(|c| c.0 == color).map_or("Automatic", |c| c.1))
            .show_ui(ui, |ui| {
                for (k, l) in colors {
                    if ui.selectable_label(color == k, l).clicked() {
                        d.fields.insert("color".into(), k.into());
                    }
                }
            });
        let _ = combo.response.labelled_by(field.id);
    });
    ui.horizontal(|ui| {
        if ui.button("None").on_hover_text("Remove bullets and numbering").clicked() {
            d.fields.insert("none".into(), "1".into());
        }
    });
    let (ok, cancel) = buttons(ui, "OK");
    let none = d.fields.contains_key("none");
    if none {
        run(app, if tab == "numbered" { "format.numbering" } else { "format.bullets" }, json!({"on": false}));
    } else if ok {
        let mut p = json!({"on": true});
        if let Some(size) = d.get("size", "100").trim().trim_end_matches('%').parse::<f64>().ok().filter(|v| v.is_finite()) {
            p["size"] = json!(size.clamp(25.0, 400.0));
        }
        if !color.is_empty() {
            p["color"] = json!(color);
        }
        if tab == "numbered" {
            p["scheme"] = json!(d.get("scheme", "arabicPeriod"));
            p["start"] = json!(d.get("start", "1").trim().parse::<u64>().unwrap_or(1).clamp(1, 32767));
            run(app, "format.numbering", p);
        } else {
            let custom = d.get("custom", "");
            let ch = custom.chars().find(|c| !c.is_whitespace()).map(String::from).unwrap_or_else(|| d.get("char", "•"));
            p["char"] = json!(ch);
            run(app, "format.bullets", p);
        }
    }
    ok || cancel || none
}

fn dlg_action(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    use deckcraft_model::text::Action;
    let tab = d.get("tab", "click");
    // Start from the selected shape's current setting for this tab.
    let (cur, slides, shows) = app
        .session
        .active()
        .map(|st| {
            let sh = st.selected_shapes().first().map(|x| if tab == "hover" { x.hover.clone() } else { x.click.clone() }).unwrap_or(None);
            let slides: Vec<(deckcraft_model::SlideId, String)> =
                st.doc.slides.iter().enumerate().map(|(i, sl)| (sl.id, format!("{}. {}", i + 1, sl.title()))).collect();
            let shows: Vec<String> = st.doc.custom_shows.iter().map(|c| c.name.clone()).collect();
            (sh, slides, shows)
        })
        .unwrap_or_default();
    let (cur_kind, cur_arg) = match cur.as_ref().map(|h| &h.action) {
        None => ("none", String::new()),
        Some(Action::NextSlide) => ("next", String::new()),
        Some(Action::PreviousSlide) => ("previous", String::new()),
        Some(Action::FirstSlide) => ("first", String::new()),
        Some(Action::LastSlide) => ("last", String::new()),
        Some(Action::LastViewed) => ("lastViewed", String::new()),
        Some(Action::EndShow) => ("endShow", String::new()),
        Some(Action::PlayMedia) => ("playMedia", String::new()),
        Some(Action::Url { url }) => ("url", url.clone()),
        Some(Action::CustomShow { name, .. }) => ("customShow", name.clone()),
        Some(Action::Program { path }) => ("program", path.clone()),
        Some(Action::Slide { slide }) => ("slide", slides.iter().position(|(id, _)| id == slide).unwrap_or(0).to_string()),
    };
    ui.horizontal(|ui| {
        for (k, l) in [("click", "Mouse Click"), ("hover", "Mouse Over")] {
            if ui.selectable_label(tab == k, l).clicked() && tab != k {
                d.fields.clear();
                d.fields.insert("tab".into(), k.into());
            }
        }
    });
    ui.separator();
    ui.label(if tab == "hover" { "Action on mouse over" } else { "Action on click" });
    let kind = d.get("kind", cur_kind);
    let hyperlink = !matches!(kind.as_str(), "none" | "program" | "playMedia");
    let pick = |d: &mut Dialog, ui: &mut Ui, k: &str, l: &str| {
        if ui.radio(d.get("kind", cur_kind) == k, l).clicked() {
            d.fields.insert("kind".into(), k.into());
        }
    };
    pick(d, ui, "none", "None");
    ui.horizontal(|ui| {
        let field = ui.radio(hyperlink, "Hyperlink to:");
        if field.clicked() && !hyperlink {
            d.fields.insert("kind".into(), "next".into());
        }
        let targets = [
            ("next", "Next Slide"),
            ("previous", "Previous Slide"),
            ("first", "First Slide"),
            ("last", "Last Slide"),
            ("lastViewed", "Last Slide Viewed"),
            ("endShow", "End Show"),
            ("customShow", "Custom Show…"),
            ("slide", "Slide…"),
            ("url", "URL…"),
        ];
        let shown = targets.iter().find(|t| t.0 == kind).map_or("Next Slide", |t| t.1);
        ui.add_enabled_ui(hyperlink, |ui| {
            let combo = craft_ui::ComboBox::from_id_salt("action-target").selected_text(shown).show_ui(ui, |ui| {
                for (k, l) in targets {
                    if ui.selectable_label(kind == k, l).clicked() {
                        d.fields.insert("kind".into(), k.into());
                    }
                }
            });
            let _ = combo.response.labelled_by(field.id);
        });
    });
    let kind = d.get("kind", cur_kind);
    match kind.as_str() {
        "url" => field(ui, d, "Address:", "arg", if cur_kind == "url" { &cur_arg } else { "https://" }),
        "slide" => {
            let sel = d.get("arg", if cur_kind == "slide" { &cur_arg } else { "0" }).parse::<usize>().unwrap_or(0);
            egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                for (i, (_, t)) in slides.iter().enumerate() {
                    if ui.selectable_label(sel == i, t).clicked() {
                        d.fields.insert("arg".into(), i.to_string());
                    }
                }
            });
        }
        "customShow" => {
            if shows.is_empty() {
                ui.label("This presentation has no custom shows (Slide Show ▸ Custom Slide Show).");
            }
            let sel = d.get("arg", if cur_kind == "customShow" { &cur_arg } else { shows.first().map_or("", String::as_str) });
            for n in &shows {
                if ui.selectable_label(&sel == n, n).clicked() {
                    d.fields.insert("arg".into(), n.clone());
                }
            }
        }
        _ => {}
    }
    pick(d, ui, "program", "Run program:");
    if d.get("kind", cur_kind) == "program" {
        field(ui, d, "Program:", "arg", if cur_kind == "program" { &cur_arg } else { "" });
    }
    pick(d, ui, "playMedia", "Play the shape's media");
    let hl = check(ui, d, "Highlight click", "highlight", cur.as_ref().is_none_or(|h| h.highlight_click));
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        let kind = d.get("kind", cur_kind);
        let arg = d.get("arg", &cur_arg);
        let mut p = json!({"on": tab, "action": kind, "highlight": hl});
        match kind.as_str() {
            "url" => p["url"] = json!(arg),
            "slide" => p["slide"] = json!(arg.parse::<usize>().unwrap_or(0)),
            "customShow" => p["show"] = json!(arg),
            "program" => p["path"] = json!(arg),
            _ => {}
        }
        run(app, "shape.action", p);
    }
    ok || cancel
}

fn dlg_alt_text(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let (cur, deco_now) = app
        .session
        .active()
        .map(|s| {
            let shapes = s.selected_shapes();
            let cur = shapes.first().map(|x| x.descr.clone()).unwrap_or_default();
            let deco = alt_text_starts_decorative(shapes.iter().map(|x| x.decorative));
            (cur, deco)
        })
        .unwrap_or_default();
    ui.label("How would you describe this object and its context to someone who is blind or has low vision?");
    let mut v = d.get("text", &cur);
    ui.add(egui::TextEdit::multiline(&mut v).hint_text("Alt text").desired_rows(4).desired_width(340.0));
    d.fields.insert("text".into(), v.clone());
    let deco = check(ui, d, "Mark as decorative", "deco", deco_now);
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        run(app, "shape.altText", json!({"text": v, "decorative": deco}));
    }
    ok || cancel
}

fn dlg_outline(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    ui.label("Paste or type an outline. Unindented lines become slide titles; tab-indented lines become bullets.");
    let mut v = d.get("text", "");
    ui.add(egui::TextEdit::multiline(&mut v).hint_text("Outline").desired_rows(10).desired_width(380.0).code_editor());
    d.fields.insert("text".into(), v.clone());
    let (ok, cancel) = buttons(ui, "Insert");
    if ok {
        run(app, "slide.fromOutline", json!({"text": v}));
    }
    ok || cancel
}

fn dlg_reuse_slides(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let path = d.params.get("path").and_then(Value::as_str).unwrap_or("").to_string();
    let slides: Vec<(usize, String)> = d
        .params
        .get("slides")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some((usize::try_from(x.get("index")?.as_u64()?).ok()?, x.get("title").and_then(Value::as_str).unwrap_or("").to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(path.clone());
    ui.label(format!("Insert slides from {name} after the current slide:"));
    // Unticked slides are remembered by index; everything starts ticked.
    let off = |d: &Dialog, i: usize| d.fields.get(&format!("off{i}")).is_some_and(|v| v == "1");
    ui.horizontal(|ui| {
        if ui.small_button("Select All").clicked() {
            for (i, _) in &slides {
                d.fields.remove(&format!("off{i}"));
            }
        }
        if ui.small_button("Select None").clicked() {
            for (i, _) in &slides {
                d.fields.insert(format!("off{i}"), "1".into());
            }
        }
    });
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        for (i, title) in &slides {
            let mut on = !off(d, *i);
            let label = if title.trim().is_empty() { format!("{}  (no title)", i + 1) } else { format!("{}  {}", i + 1, title.trim()) };
            if ui.checkbox(&mut on, label).changed() {
                if on {
                    d.fields.remove(&format!("off{i}"));
                } else {
                    d.fields.insert(format!("off{i}"), "1".into());
                }
            }
        }
    });
    let mut keep = d.params.get("keep").and_then(Value::as_bool).unwrap_or(false);
    if ui.checkbox(&mut keep, "Keep source formatting").changed()
        && let Some(o) = d.params.as_object_mut()
    {
        o.insert("keep".into(), json!(keep));
    }
    let picked: Vec<usize> = slides.iter().map(|(i, _)| *i).filter(|i| !off(d, *i)).collect();
    let (ok, cancel) = buttons(ui, "Insert");
    if ok {
        if picked.is_empty() {
            app.set_status("Tick at least one slide to insert".to_string());
            return false;
        }
        // A failed insert keeps the dialog (and the ticked slides) open; the error is in the status bar.
        let source = d.params.get("source").cloned().unwrap_or_default();
        return app.reuse_slides(&source, keep, &picked);
    }
    cancel
}

fn dlg_rename_section(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
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

fn dlg_export(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    ui.label("Export the presentation as:");
    let fmt = d.get("fmt", "pdf");
    for (l, f) in [
        ("PDF document (.pdf)", "pdf"),
        ("PNG images (current slide)", "png"),
        ("PNG images (all slides)", "pngAll"),
        ("JPEG image (current slide)", "jpeg"),
        ("PowerPoint Presentation (.pptx)", "pptx"),
        ("Outline (.txt)", "outline"),
        ("Animated GIF (.gif)", "gif"),
        ("Outline/RTF (.rtf)", "rtf"),
    ] {
        if ui.radio(fmt == f, l).clicked() {
            d.fields.insert("fmt".into(), f.into());
        }
    }
    let pdf_layout = d.get("pdfLayout", "slides");
    if fmt == "pdf" {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let field = ui.label("Print layout:");
            let combo = craft_ui::ComboBox::from_id_salt("pdf_layout")
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
                        if ui.selectable_label(pdf_layout == k, l).clicked() {
                            d.fields.insert("pdfLayout".into(), k.into());
                        }
                    }
                });
            let _ = combo.response.labelled_by(field.id);
        });
        let mut hidden = d.get("pdfHidden", "false") == "true";
        if ui.checkbox(&mut hidden, "Include hidden slides").changed() {
            d.fields.insert("pdfHidden".into(), hidden.to_string());
        }
    }
    let png = matches!(fmt.as_str(), "png" | "pngAll");
    if png {
        ui.add_space(4.0);
        let mut clear = d.get("pngTransparent", "false") == "true";
        if ui
            .checkbox(&mut clear, "Transparent background")
            .on_hover_text("Leave the slide background out: only what is on the slide is drawn")
            .changed()
        {
            d.fields.insert("pngTransparent".into(), clear.to_string());
        }
    }
    let gif_params = if fmt == "gif" { gif_options(app, ui, d) } else { Ok(Value::Null) };
    if let Err(e) = &gif_params {
        ui.label(egui::RichText::new(e).color(Tokens::get(ui.ctx()).text_dim));
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
        let gif_params = match gif_params {
            Ok(v) => v,
            // Keep the dialog open so the range or width can be corrected.
            Err(e) => {
                app.set_status(e);
                return false;
            }
        };
        let extra = match fmt.as_str() {
            "pdf" => pdf_params,
            "gif" => gif_params,
            _ if png && d.get("pngTransparent", "false") == "true" => json!({"transparentBackground": true}),
            _ => json!({}),
        };
        app.export_as(&fmt, extra);
    }
    ok || cancel
}

fn dlg_replace_fonts(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let used: Vec<String> = app.session.execute("font.used", &json!({})).ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let mut from = d.get("from", used.first().map(String::as_str).unwrap_or(""));
    let mut to = d.get("to", "");
    egui::Grid::new("replace_fonts").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
        let field = ui.label("Replace:");
        let combo = craft_ui::ComboBox::from_id_salt("rf_from").width(240.0).selected_text(from.clone()).show_ui(ui, |ui| {
            for f in &used {
                ui.selectable_value(&mut from, f.clone(), f);
            }
        });
        let _ = combo.response.labelled_by(field.id);
        ui.end_row();
        ui.label("With:");
        ui.vertical(|ui| {
            ui.add(egui::TextEdit::singleline(&mut to).desired_width(240.0).hint_text("Font name"));
            let needle = to.trim().to_lowercase();
            egui::ScrollArea::vertical().max_height(140.0).id_salt("rf_list").show(ui, |ui| {
                for f in crate::ribbon::font_families(app).iter().filter(|f| needle.is_empty() || f.to_lowercase().contains(&needle)).take(200) {
                    if ui.selectable_label(f.eq_ignore_ascii_case(to.trim()), f).clicked() {
                        to = f.clone();
                    }
                }
            });
        });
        ui.end_row();
    });
    if let Some(msg) = d.fields.get("result") {
        ui.label(egui::RichText::new(msg.clone()).color(Tokens::get(ui.ctx()).text_dim));
    }
    d.fields.insert("from".into(), from.clone());
    d.fields.insert("to".into(), to.clone());
    let (ok, cancel) = buttons(ui, "Replace");
    if ok && !from.is_empty() && !to.trim().is_empty() {
        let n = app.run("font.replace", json!({"from": from, "to": to.trim()})).ok().and_then(|v| v["replaced"].as_u64()).unwrap_or(0);
        let msg = if n == 0 { format!("“{from}” isn't used explicitly.") } else { format!("Replaced “{from}” with “{}”.", to.trim()) };
        d.fields.insert("result".into(), msg);
        d.fields.remove("from");
        // Stay open, like PowerPoint, so several fonts can be replaced in a row.
        return cancel;
    }
    cancel
}

fn dlg_symbol(app: &mut SlideApp, ui: &mut Ui, _d: &mut Dialog) -> bool {
    ui.label("Click a symbol to insert it at the insertion point.");
    let groups: [(&str, &str); 6] = [
        ("Punctuation", "–—‘’“”…•·§¶†‡‰′″‹›«»¡¿"),
        ("Currency", "$€£¥¢₹₩₽₺₿"),
        ("Math", "±×÷≠≈≤≥∞√∑∏∫∂∆∇∈∉∩∪⊂⊃∀∃¬∧∨⇒⇔°′"),
        ("Greek", "αβγδεζηθικλμνξοπρστυφχψωΓΔΘΛΞΠΣΦΨΩ"),
        ("Arrows", "←↑→↓↔↕⇐⇑⇒⇓↖↗↘↙⟵⟶"),
        ("Shapes", "■□▪▫▲△▶▷▼▽◀◁◆◇○●◎★☆✓✔✗✘♠♣♥♦"),
    ];
    for (g, chars) in groups {
        ui.label(egui::RichText::new(g).font(theme::bold(12.0)));
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

fn dlg_equation(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    ui.label("Type an equation in linear form (e.g. a^2+b^2=c^2, x=(-b±√(b^2-4ac))/2a).");
    let mut v = d.get("eq", "");
    ui.add(egui::TextEdit::singleline(&mut v).hint_text("Equation").desired_width(340.0).font(theme::font(15.0)));
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

fn dlg_paste_special(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    const OPTS: [(&str, &str); 4] = [
        ("destinationTheme", "Use Destination Theme"),
        ("keepSource", "Keep Source Formatting"),
        ("picture", "Picture (PNG)"),
        ("text", "Keep Text Only"),
    ];
    let has_shapes = !app.session.clipboard.shapes.is_empty();
    let mut cur = d.get("as", "destinationTheme");
    ui.label("As:");
    for (id, label) in OPTS {
        let ok = has_shapes || matches!(id, "destinationTheme" | "text");
        ui.add_enabled_ui(ok, |ui| ui.radio_value(&mut cur, id.to_string(), label));
    }
    d.fields.insert("as".into(), cur.clone());
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        run(app, "edit.pasteSpecial", json!({"as": cur}));
    }
    ok || cancel
}

fn dlg_spelling(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    // Words skipped with Ignore All while the dialog is open, one per line.
    let ignored = d.get("ignore", "");
    let ignore: Vec<&str> = ignored.lines().filter(|l| !l.is_empty()).collect();
    let v = app.session.execute("review.spelling", &json!({"ignore": ignore})).unwrap_or_default();
    let list = v.as_array().cloned().unwrap_or_default();
    let info = app.session.execute("review.dictionary", &json!({})).unwrap_or_default();
    let available = info.get("available").and_then(Value::as_bool).unwrap_or(false);
    match info.get("found").and_then(Value::as_str) {
        Some(f) if available => ui.label(egui::RichText::new(format!("Dictionary: {}", f.rsplit(['/', '\\']).next().unwrap_or(f))).small().weak()),
        _ if available => {
            ui.label(egui::RichText::new(format!("Dictionary: {}", info.get("lang").and_then(Value::as_str).unwrap_or(""))).small().weak())
        }
        _ => ui.label(
            egui::RichText::new(format!(
                "No {} dictionary is installed, so only common misspellings are found. Put a Hunspell .aff/.dic pair in {}.",
                info.get("lang").and_then(Value::as_str).unwrap_or("en-US"),
                info.get("folders")
                    .and_then(Value::as_array)
                    .and_then(|a| a.get(1).or(a.first()))
                    .and_then(Value::as_str)
                    .unwrap_or("a dictionaries folder")
            ))
            .small()
            .weak(),
        ),
    };
    if list.is_empty() {
        ui.label("The spelling check is complete. No issues found.");
    }
    let mut add_ignore = None;
    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
        for item in &list {
            let w = item.get("word").and_then(Value::as_str).unwrap_or("");
            let slide = item.get("slide").cloned().unwrap_or_default();
            let shape = item.get("shape").cloned().unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label(format!("Slide {}: “{w}”", slide.as_u64().unwrap_or(0) + 1));
                if ui.small_button("Go to").clicked() {
                    let _ = app.run("slide.go", json!({"index": slide}));
                    let _ = app.run("edit.select", json!({"ids": [shape]}));
                }
                if ui.small_button("Ignore All").clicked() {
                    add_ignore = Some(w.to_string());
                }
                if ui.small_button("Add to Dictionary").clicked()
                    && let Err(e) = app.session.execute("review.addToDictionary", &json!({"word": w}))
                {
                    app.set_status(e.to_string());
                }
            });
            let sug: Vec<String> = item
                .get("suggestions")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            if item.get("repeated").and_then(Value::as_bool).unwrap_or(false) && !item.get("notes").and_then(Value::as_bool).unwrap_or(false) {
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new("Repeated word").weak());
                    if ui.small_button("Delete Repeated Word").clicked() {
                        let from = item.get("deleteFrom").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let end = item.get("end").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let para = item.get("para").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let text = app.session.active().and_then(|st| {
                            let sh = st.shape(deckcraft_model::ShapeId(shape.as_u64()? as u32))?;
                            let pp = match (item.get("row").and_then(Value::as_u64), item.get("col").and_then(Value::as_u64), &sh.kind) {
                                (Some(r), Some(c), deckcraft_model::ShapeKind::Table(tb)) => {
                                    tb.cell(r as usize, c as usize)?.text.paragraphs.get(para)?.text()
                                }
                                _ => sh.text.as_ref()?.paragraphs.get(para)?.text(),
                            };
                            Some(pp.chars().skip(from).take(end.saturating_sub(from)).collect::<String>())
                        });
                        if let Some(gap) = text {
                            let mut p = json!({"shape": shape, "para": para, "start": from, "word": gap, "to": ""});
                            if let (Some(o), Some(r), Some(c)) = (p.as_object_mut(), item.get("row"), item.get("col")) {
                                o.insert("row".into(), r.clone());
                                o.insert("col".into(), c.clone());
                            }
                            run(app, "review.spellingChange", p);
                        }
                    }
                });
            }
            if !sug.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new("Change to:").weak());
                    for to in &sug {
                        let r = ui.small_button(to.as_str()).on_hover_text("Click to change this one; Shift-click to change all");
                        if r.clicked() {
                            let all = ui.input(|i| i.modifiers.shift);
                            if item.get("notes").and_then(Value::as_bool).unwrap_or(false) {
                                // A word in the notes: change that line of the slide's notes.
                                let si = slide.as_u64().unwrap_or(0) as usize;
                                let notes = app.session.active().and_then(|d| d.doc.slides.get(si)).map(|s| s.notes_text()).unwrap_or_default();
                                let (pi, st) = (
                                    item.get("para").and_then(Value::as_u64).unwrap_or(0) as usize,
                                    item.get("start").and_then(Value::as_u64).unwrap_or(0) as usize,
                                );
                                let lines: Vec<String> = notes
                                    .split('\n')
                                    .enumerate()
                                    .map(|(i, l)| {
                                        if i != pi {
                                            return l.to_string();
                                        }
                                        let c: Vec<char> = l.chars().collect();
                                        let n = w.chars().count();
                                        if c.get(st..st + n).is_some_and(|x| x.iter().collect::<String>() == w) {
                                            c[..st].iter().chain(to.chars().collect::<Vec<_>>().iter()).chain(c[st + n..].iter()).collect()
                                        } else {
                                            l.to_string()
                                        }
                                    })
                                    .collect();
                                run(app, "slide.notes", json!({"text": lines.join("\n"), "index": si}));
                            } else {
                                let mut p = if all {
                                    json!({"word": w, "to": to, "all": true})
                                } else {
                                    json!({"shape": shape, "word": w, "to": to, "para": item.get("para"), "start": item.get("start")})
                                };
                                if !all && let (Some(o), Some(r), Some(c)) = (p.as_object_mut(), item.get("row"), item.get("col")) {
                                    o.insert("row".into(), r.clone());
                                    o.insert("col".into(), c.clone());
                                }
                                run(app, "review.spellingChange", p);
                            }
                        }
                    }
                });
            }
        }
    });
    if let Some(w) = add_ignore {
        d.fields.insert("ignore".into(), format!("{ignored}{w}\n"));
    }
    let (ok, cancel) = buttons(ui, "Close");
    ok || cancel
}

fn dlg_accessibility(app: &mut SlideApp, ui: &mut Ui, _d: &mut Dialog) -> bool {
    let v = app.session.execute("review.accessibility", &json!({})).unwrap_or_default();
    let list = v.as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        ui.label("No accessibility issues found. People with disabilities should not have difficulty reading this document.");
    }
    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        for item in &list {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Slide {}: {}{}",
                    item.get("slide").and_then(Value::as_u64).unwrap_or(0) + 1,
                    item.get("issue").and_then(Value::as_str).unwrap_or(""),
                    item.get("name").and_then(Value::as_str).map(|n| format!(" — {n}")).unwrap_or_default()
                ));
                if ui.small_button("Go to").clicked() {
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

fn dlg_language(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    // Starts from the selected text's language and Do Not Check Spelling.
    let (cur, np) = selected_language(app);
    let lang = d.get("lang", &cur);
    let mut no_proof = d.get("noProof", if np { "1" } else { "" }) == "1";
    // The selected text, or every text in the presentation (all there is with nothing selected).
    let has_sel = deckcraft_engine::cmd::has_text_or_shapes(&app.session).is_ok();
    let mut all = !has_sel || d.get("scope", "") == "all";
    ui.horizontal(|ui| {
        ui.label("Apply to:");
        if ui.add_enabled(has_sel, egui::RadioButton::new(!all, "Selected text")).clicked() {
            d.fields.insert("scope".into(), "sel".into());
            all = false;
        }
        if ui.radio(all, "Whole presentation").clicked() {
            d.fields.insert("scope".into(), "all".into());
            all = true;
        }
    });
    ui.label(if all { "Mark all text in the presentation as:" } else { "Mark selected text as:" });
    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
        // The selection's own language first when the list doesn't have it.
        let own = (!crate::squiggles::LANGS.iter().any(|(_, t)| t.eq_ignore_ascii_case(&cur))).then(|| (cur.clone(), cur.clone()));
        let all = own.into_iter().chain(crate::squiggles::LANGS.iter().map(|(n, t)| (n.to_string(), t.to_string())));
        for (_native, tag) in all {
            // Native names are shown when the interface fonts can draw them; otherwise
            // the tag is the explicit fallback (egui 0.37 aborts on a missing glyph).
            let name = crate::squiggles::display_lang_name(ui.ctx(), &tag);
            let installed = deckcraft_engine::spell::dictionary(&tag).is_some();
            let label = if installed { format!("{name}  (dictionary)") } else { name };
            if ui.selectable_label(lang.eq_ignore_ascii_case(&tag), label).clicked() {
                d.fields.insert("lang".into(), tag);
            }
        }
    });
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Spelling is checked for languages marked (dictionary).").small().weak());
    if ui.checkbox(&mut no_proof, "Do not check spelling or grammar").changed() {
        d.fields.insert("noProof".into(), if no_proof { "1".into() } else { String::new() });
    }
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        let lang = d.get("lang", &cur);
        run(app, if all { "format.languageAll" } else { "format.language" }, json!({"lang": lang, "noProof": no_proof}));
    }
    ok || cancel
}

fn dlg_custom_show(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let shows = app.session.active().map(|s| s.doc.custom_shows.clone()).unwrap_or_default();
    let mut started = false;
    for s in &shows {
        ui.horizontal(|ui| {
            ui.label(format!("{} ({} slides)", s.name, s.slides.len()));
            if ui.small_button("Show").clicked() {
                run(app, "show.start", json!({"custom": s.name}));
                started = true;
            }
            if ui.small_button("Delete").clicked() {
                let _ = app.run("show.customShow", json!({"name": s.name, "delete": true}));
            }
        });
    }
    if started {
        return true;
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

fn dlg_chart_data(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let ch = app.session.active().and_then(|s| {
        s.selected_shapes()
            .into_iter()
            .find_map(|x| if let deckcraft_model::ShapeKind::Chart(c) = &x.kind { Some(((**c).clone(), x.id)) } else { None })
    });
    let Some((c, id)) = ch else {
        ui.label("Select a chart first.");
        let (ok, cancel) = buttons(ui, "Close");
        return ok || cancel;
    };
    ui.label("Edit the data: first row = series names, first column = categories.");
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
                ui.add(egui::TextEdit::singleline(cell).hint_text("Chart cell").desired_width(70.0));
            }
            ui.end_row();
        }
    });
    ui.horizontal(|ui| {
        if ui.button("+ Row").clicked() {
            let n = grid.first().map(Vec::len).unwrap_or(2);
            grid.push(vec![String::new(); n]);
        }
        if ui.button("+ Series").clicked() {
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

fn dlg_trim(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let seed = trim_seed(app.session.active());
    field(ui, d, "Start time (s):", "start", &seed.start);
    field(ui, d, "End trim (s):", "end", &seed.end);
    field(ui, d, "Fade in (s):", "fin", &seed.fade_in);
    field(ui, d, "Fade out (s):", "fout", &seed.fade_out);
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        run(
            app,
            "media.options",
            json!({
                "trimStart": seconds_text_to_ms(&d.get("start", &seed.start)),
                "trimEnd": seconds_text_to_ms(&d.get("end", &seed.end)),
                "fadeIn": seconds_text_to_ms(&d.get("fin", &seed.fade_in)),
                "fadeOut": seconds_text_to_ms(&d.get("fout", &seed.fade_out)),
            }),
        );
    }
    ok || cancel
}

fn dlg_smartart(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    ui.label("Pick a graphic; type one item per line.");
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
            if ui.selectable_label(kind == k, l).clicked() {
                d.fields.insert("kind".into(), k.into());
            }
        }
    });
    let mut v = d.get("text", "Plan\nBuild\nLaunch");
    ui.add(egui::TextEdit::multiline(&mut v).hint_text("SmartArt items").desired_rows(5).desired_width(320.0));
    d.fields.insert("text".into(), v.clone());
    let (ok, cancel) = buttons(ui, "Insert");
    if ok {
        run(app, "insert.smartArt", json!({"kind": kind, "items": v.lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>()}));
        app.ui.pane = Some("smartart".into());
    }
    ok || cancel
}

fn dlg_about(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let tab = d.get("tab", "about");
    ui.horizontal(|ui| {
        for (k, l) in [("about", "About"), ("contributors", "Contributors"), ("models", "Models")] {
            if ui.selectable_label(tab == k, l).clicked() {
                d.fields.insert("tab".into(), k.into());
            }
        }
    });
    ui.separator();
    match tab.as_str() {
        "contributors" | "models" => {
            credits_block(ui, tab == "models");
        }
        _ => about(app, ui),
    }
    let (ok, cancel) = buttons(ui, "Close");
    ok || cancel
}

fn dlg_autocorrect(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let pr = &mut app.session.prefs;
    ui.checkbox(&mut pr.ac_two_initials, "Correct TWo INitial CApitals");
    ui.checkbox(&mut pr.ac_sentence_caps, "Capitalize first letter of sentences");
    ui.checkbox(&mut pr.ac_days, "Capitalize names of days");
    ui.checkbox(&mut pr.ac_caps_lock, "Correct accidental use of cAPS LOCK key");
    ui.checkbox(&mut pr.ac_fractions, "Replace fractions (1/2) with fraction characters (½)");
    ui.checkbox(&mut pr.ac_ordinals, "Ordinals (1st) with superscript");
    ui.checkbox(&mut pr.ac_lists, "Automatic bulleted and numbered lists");
    ui.checkbox(&mut pr.autocorrect, "Replace text as you type");
    ui.add_space(4.0);
    let mut from = d.get("from", "");
    let mut to = d.get("to", "");
    ui.horizontal(|ui| {
        let l = ui.label("Replace");
        ui.add(egui::TextEdit::singleline(&mut from).desired_width(90.0)).labelled_by(l.id);
        let l = ui.label("With");
        ui.add(egui::TextEdit::singleline(&mut to).desired_width(130.0)).labelled_by(l.id);
        if ui.add_enabled(!from.trim().is_empty(), egui::Button::new("Add")).clicked() {
            run(app, "text.autocorrect", json!({"add": [from.trim(), to]}));
            from.clear();
            to.clear();
        }
    });
    d.fields.insert("from".into(), from.clone());
    d.fields.insert("to".into(), to);
    let list = app.session.execute("text.autocorrect", &json!({})).ok().and_then(|v| v.get("entries").cloned());
    let entries: Vec<(String, String)> = list.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let q = from.trim().to_lowercase();
    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
        egui::Grid::new("ac-list").striped(true).show(ui, |ui| {
            for (k, v) in entries.iter().filter(|(k, _)| q.is_empty() || k.to_lowercase().starts_with(&q)) {
                ui.label(k);
                ui.label(v);
                if ui.small_button("Delete").clicked() {
                    run(app, "text.autocorrect", json!({"remove": k}));
                }
                ui.end_row();
            }
        });
    });
    let (ok, cancel) = buttons(ui, "OK");
    ok || cancel
}

fn dlg_preferences(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    // The controls apply as they change, so the dialog remembers what it opened with and
    // Cancel (or Esc, or the close button: see `show`) puts that back.
    if d.params.get("before").is_none() {
        d.params = json!({ "before": preferences_snapshot(app) });
    }
    ui.label(egui::RichText::new("General").font(theme::bold(13.0)));
    let mut dark = app.ui.brightness == theme::Brightness::Dark;
    if ui.checkbox(&mut dark, "Dark appearance").changed() {
        let _ = app.run("view.dark", json!({"on": dark}));
    }
    // While the pointer holds the slider its value only waits in the dialog: applying it
    // would rescale the slider under the pointer and run it to an end (#767). It applies
    // on release, and at once from the keyboard.
    let pending = d.fields.get("uiScale").and_then(|v| v.parse::<f32>().ok());
    let mut scale = pending.unwrap_or(app.ui.ui_scale);
    let r = ui.add(egui::Slider::new(&mut scale, 0.75..=1.75));
    // No visible label here (the row is laid out to fit); the accessible name only.
    let enabled = r.enabled();
    r.widget_info(|| egui::WidgetInfo::slider(enabled, f64::from(scale), "Interface size"));
    if r.is_pointer_button_down_on() {
        if r.changed() || pending.is_some() {
            d.fields.insert("uiScale".into(), scale.to_string());
        }
    } else {
        if r.changed() || pending.is_some() {
            app.ui.ui_scale = scale;
        }
        d.fields.remove("uiScale");
    }
    // PowerPoint takes the unit from the system's measurement setting; DeckCraft asks here.
    ui.horizontal(|ui| {
        ui.label("Measurement units:");
        let units = &mut app.session.prefs.units;
        ui.radio_value(units, deckcraft_geom::Unit::Inches, "Inches");
        ui.radio_value(units, deckcraft_geom::Unit::Centimeters, "Centimeters");
    });
    ui.label(egui::RichText::new("Editing").font(theme::bold(13.0)));
    ui.checkbox(&mut app.session.prefs.smart_guides, "Show smart guides");
    ui.checkbox(&mut app.session.prefs.snap_to_grid, "Snap objects to grid");
    ui.checkbox(&mut app.session.prefs.smart_quotes, "Replace straight quotes with smart quotes");
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.session.prefs.autocorrect, "AutoCorrect as you type");
        if ui.button("AutoCorrect Options…").clicked() {
            *d = Dialog::new("autocorrect");
        }
    });
    if d.id != "preferences" {
        return false;
    }
    ui.horizontal(|ui| {
        let label = ui.label("Your name (comments):");
        // Name the field for assistive technology; a bare text edit next to a label is an
        // unnamed input in egui 0.37's accessibility tree.
        let _ = ui.add(egui::TextEdit::singleline(&mut app.session.prefs.author)).labelled_by(label.id);
    });
    let (ok, cancel) = buttons(ui, "Done");
    if cancel {
        restore_preferences(app, d);
    }
    ok || cancel
}
