use super::{
    Align2, Backstage, CONTENT_W, Color32, CornerRadius, Icon, Outcome, Page, Rect, RichText, Sense, SlideApp, Stroke, StrokeKind, Tokens, Ui, Value,
    arrow_link, dim, heading, icons, json, pos2, search, theme, vec2,
};

/// A new-presentation tile: a theme preview drawn in code (or a blank slide) over its name.
fn tile(ui: &mut Ui, label: &str, theme: Option<&deckcraft_model::Theme>, big: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let thumb_w = if big { 200.0 } else { 160.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(thumb_w + 16.0, thumb_w * 9.0 / 16.0 + 44.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(4), t.hover);
    }
    let thumb = Rect::from_min_size(rect.min + vec2(8.0, 10.0), vec2(thumb_w, thumb_w * 9.0 / 16.0));
    match theme {
        Some(th) => crate::ribbon::paint_theme_tile(p, thumb, th),
        None => {
            p.rect_filled(thumb, CornerRadius::same(2), Color32::WHITE);
            p.rect_stroke(thumb, CornerRadius::same(2), Stroke::new(1.0, Color32::from_gray(200)), StrokeKind::Inside);
        }
    }
    if resp.hovered() {
        p.rect_stroke(thumb, CornerRadius::same(2), Stroke::new(2.0, t.accent), StrokeKind::Outside);
    }
    let galley = p.layout(label.to_string(), theme::font(12.0), t.text, rect.width() - 8.0);
    p.galley(pos2(rect.center().x - galley.size().x / 2.0, thumb.bottom() + 8.0), galley, t.text);
    resp
}

/// The built-in themes and the sample deck, filtered by `query` (after a blank presentation when
/// `blank`); at most `limit` theme tiles. False when nothing matched.
fn new_tiles(app: &mut SlideApp, ui: &mut Ui, query: &str, limit: usize, blank: bool) -> bool {
    let q = query.trim().to_lowercase();
    let themes: Vec<deckcraft_model::Theme> =
        deckcraft_model::theme::builtin_themes().into_iter().filter(|th| q.is_empty() || th.name.to_lowercase().contains(&q)).collect();
    let sample = q.is_empty() || "sample deck northern lights".contains(&q);
    if themes.is_empty() && !sample {
        return false;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        if blank && tile(ui, crate::tl!("Blank Presentation"), None, false).clicked() {
            let _ = app.run("file.new", json!({}));
        }
        let mut shown = 0;
        for th in themes {
            if shown >= limit {
                break;
            }
            if tile(ui, &th.name, Some(&th), false).clicked() {
                let _ = app.run("file.new", json!({"theme": th.name}));
            }
            shown += 1;
        }
        if sample && shown < limit {
            let th = deckcraft_model::theme::builtin_themes().into_iter().find(|t| t.name == "DeckCraft");
            if tile(ui, crate::tl!("Sample deck: Northern Lights"), th.as_ref(), false).clicked()
                && let Err(e) = deckcraft_engine::sample::open_sample(&mut app.session)
            {
                app.set_status(e.to_string());
            }
        }
    });
    true
}

/// Starter decks whose name, summary, theme or a slide title matches `query`, at most `limit` tiles.
fn template_tiles(app: &mut SlideApp, ui: &mut Ui, query: &str, limit: usize) -> bool {
    let list = deckcraft_engine::templates::matching(query);
    if list.is_empty() {
        return false;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        for starter in list.into_iter().take(limit) {
            let theme = deckcraft_model::theme::builtin_themes().into_iter().find(|th| th.name == starter.theme);
            if tile(ui, starter.name, theme.as_ref(), false).on_hover_text(starter.summary).clicked() {
                let _ = app.run("file.new", json!({"template": starter.name}));
            }
        }
    });
    true
}

/// File name and folder of a recent path, split as text (nothing on disk is read).
pub fn split_path(path: &str) -> (String, String) {
    let p = std::path::Path::new(path);
    let name = p.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(path).to_string();
    let folder = p.parent().and_then(std::path::Path::to_str).unwrap_or("").to_string();
    (name, folder)
}

/// Recent presentations whose name matches `query`, newest first, at most `limit`. Opening one is
/// the user's explicit choice.
fn recent_list(app: &mut SlideApp, ui: &mut Ui, query: &str, limit: usize) {
    let t = Tokens::get(ui.ctx());
    if app.ui.recent.is_empty() {
        dim(ui, crate::tl!("Presentations you open show up here. Use Open to browse for one."));
        return;
    }
    let q = query.trim().to_lowercase();
    let list: Vec<String> =
        app.ui.recent.iter().filter(|p| q.is_empty() || split_path(p).0.to_lowercase().contains(&q)).take(limit).cloned().collect();
    if list.is_empty() {
        dim(ui, &format!("No recent presentations match “{}”.", query.trim()));
        return;
    }
    let width = ui.available_width().min(CONTENT_W);
    for path in list {
        let missing = !app.recent_exists(&path);
        let (name, folder) = split_path(&path);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 44.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(rect, CornerRadius::same(3), t.hover);
        }
        let ink = if missing { t.text_dim } else { t.text };
        // The row is as wide as the page's content, which a narrow window clips: keep the mark
        // and the remove button inside the visible part of the row.
        let visible_right = rect.right().min(ui.clip_rect().right() - 6.0);
        {
            let p = ui.painter();
            icons::paint(p, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(22.0, 22.0)), Icon::Slide, ink, false);
            p.text(pos2(rect.left() + 44.0, rect.center().y - 8.0), Align2::LEFT_CENTER, &name, theme::font(13.0), ink);
            p.text(pos2(rect.left() + 44.0, rect.center().y + 9.0), Align2::LEFT_CENTER, &folder, theme::font(11.0), t.text_dim);
            // A missing file says so before it is clicked (#172).
            if missing {
                p.text(pos2(visible_right - 82.0, rect.center().y), Align2::RIGHT_CENTER, "(missing)", theme::font(11.0), t.text_dim);
            }
        }
        let resp = if missing { resp.on_hover_text(format!("{path} — this file can't be found")) } else { resp.on_hover_text(&path) };
        // Remove from the list: a real labelled widget on top of the row, with a code-drawn icon
        // (a text glyph would need a font that has it).
        let x = ui.allocate_rect(
            Rect::from_min_max(pos2(visible_right - 32.0, rect.top() + 5.0), pos2(visible_right, rect.bottom() - 5.0)),
            Sense::click(),
        );
        x.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, crate::tl!("Remove from Recent")));
        if x.hovered() {
            ui.painter().rect_filled(x.rect, CornerRadius::same(3), t.hover);
        }
        icons::paint(ui.painter(), Rect::from_center_size(x.rect.center(), vec2(14.0, 14.0)), Icon::Close, t.text_dim, false);
        if x.on_hover_text(crate::tl!("Remove from Recent")).clicked() {
            app.forget_recent(&path);
        }
        if resp.clicked() {
            app.open_path_in_background(&path);
        }
    }
}

/// A large square command button (Print, Create PDF, Check for Issues…).
pub(super) fn square_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(88.0, 80.0), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(3), if resp.hovered() && enabled { t.hover } else { t.card });
    p.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, if resp.hovered() && enabled { t.accent } else { t.border }), StrokeKind::Inside);
    icons::paint(p, Rect::from_center_size(pos2(rect.center().x, rect.top() + 26.0), vec2(26.0, 26.0)), icon, t.text, !enabled);
    let ink = if enabled { t.text } else { t.text_faint };
    let galley = p.layout(label.to_string(), theme::font(12.0), ink, rect.width() - 8.0);
    p.galley(pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 46.0), galley, ink);
    resp
}

/// A row of a page's own list on the left (Open's places, Export's tasks).
pub(super) fn place_row(ui: &mut Ui, icon: Icon, label: &str, selected: bool, width: f32) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 44.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CornerRadius::same(2), t.selected);
        p.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, t.border), StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(2), t.hover);
    }
    icons::paint(p, Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), vec2(24.0, 24.0)), icon, t.text, false);
    p.text(pos2(rect.left() + 46.0, rect.center().y), Align2::LEFT_CENTER, label, theme::font(13.5), t.text);
    resp
}

/// A square button with its title and description beside it (Info, Export).
pub(super) fn task(ui: &mut Ui, icon: Icon, button: &str, title: &str, detail: &str, enabled: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut clicked = false;
    ui.horizontal_top(|ui| {
        clicked = square_button(ui, icon, button, enabled).clicked();
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(title).font(theme::font(15.0)).color(t.text));
            dim(ui, detail);
        });
    });
    ui.add_space(14.0);
    clicked
}

// ---------------------------------------------------------------- pages

/// Home: DeckCraft's mark, new presentations (the first themes, with a link to New) and recent
/// presentations (with a link to Open). The same page is the start screen.
pub(super) fn home(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
        crate::dialogs::paint_logo(ui.painter(), r);
        ui.label(RichText::new("DeckCraft").font(theme::bold(24.0)).color(t.text));
    });
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::tl!("New")).font(theme::bold(16.0)).color(t.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if arrow_link(ui, crate::tl!("More themes")) {
                out.page = Some(Page::New);
            }
        });
    });
    ui.add_space(6.0);
    new_tiles(app, ui, "", 5, true);
    ui.add_space(22.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::tl!("Templates")).font(theme::bold(16.0)).color(t.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if arrow_link(ui, crate::tl!("More templates")) {
                out.page = Some(Page::New);
            }
        });
    });
    ui.add_space(6.0);
    template_tiles(app, ui, "", 3);
    ui.add_space(22.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::tl!("Recent")).font(theme::bold(16.0)).color(t.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            search(ui, &mut bs.query, crate::tl!("Search for a file"), 280.0);
        });
    });
    ui.add_space(6.0);
    recent_list(app, ui, &bs.query.clone(), 8);
    if app.ui.recent.len() > 8 {
        ui.add_space(6.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            if arrow_link(ui, crate::tl!("More presentations")) {
                out.page = Some(Page::Open);
            }
        });
    }
    ui.add_space(30.0);
    ui.hyperlink_to(crate::tl!("Join the ArtCraft community on Discord"), deckcraft_engine::links::DISCORD);
}

/// New: a blank presentation, then every theme and starter template, with a search box.
pub(super) fn new_page(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
    let t = Tokens::get(ui.ctx());
    if tile(ui, crate::tl!("Blank Presentation"), None, true).clicked() {
        let _ = app.run("file.new", json!({}));
    }
    ui.add_space(8.0);
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, t.separator));
    ui.add_space(14.0);
    search(ui, &mut bs.query, crate::tl!("Search for themes and templates"), 380.0);
    ui.add_space(14.0);
    let q = bs.query.clone();
    let themes = new_tiles(app, ui, &q, usize::MAX, false);
    let templates = if q.trim().is_empty() || !deckcraft_engine::templates::matching(&q).is_empty() {
        ui.add_space(18.0);
        heading(ui, crate::tl!("Templates"));
        template_tiles(app, ui, &q, usize::MAX)
    } else {
        false
    };
    if !themes && !templates {
        dim(ui, &format!("No themes or templates match “{}”.", q.trim()));
    }
}

/// Open: Recent and Browse on the left; the recent presentations (with a search box) on the right.
pub(super) fn open_page(app: &mut SlideApp, ui: &mut Ui, bs: &mut Backstage) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(230.0);
            place_row(ui, Icon::Recent, crate::tl!("Recent"), true, 220.0);
            if place_row(ui, Icon::OpenFolder, crate::tl!("Browse"), false, 220.0)
                .on_hover_text(crate::tl!("Open a presentation from a folder (Ctrl+O)"))
                .clicked()
            {
                let _ = app.run("app.openDialog", json!({}));
            }
            if app.session.recovery_dir.is_some() {
                ui.add_space(24.0);
                if ui.button(crate::tl!("Recover Unsaved Presentations")).clicked() {
                    match app.run("file.recovery.open", json!({})) {
                        Ok(v) if v.get("opened").and_then(Value::as_array).is_some_and(|a| a.is_empty()) => {
                            app.set_status(crate::tl!("There are no unsaved presentations to recover"));
                        }
                        _ => {}
                    }
                }
            }
        });
        let h = ui.available_height().max(320.0);
        let (r, _) = ui.allocate_exact_size(vec2(1.0, h), Sense::hover());
        ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, t.separator));
        ui.add_space(24.0);
        ui.vertical(|ui| {
            search(ui, &mut bs.query, crate::tl!("Search for a file"), 380.0);
            ui.add_space(12.0);
            heading(ui, crate::tl!("Presentations"));
            let q = bs.query.clone();
            recent_list(app, ui, &q, usize::MAX);
        });
    });
}
