//! Title bar, ribbon tabs and the ribbon card with every tab's groups.

use deckcraft_engine::Target;
use deckcraft_model::{ShapeId, ShapeKind, anim::AnimClass};
use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::SlideApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};
use crate::widgets::{
    self, RIBBON_H, big_button, big_button_ex, color_split, dropdown, group, icon_toggle, rows, small_button, small_button_ex, small_button_named,
};

mod animations;
mod arrange;
mod chart;
mod design;
mod draw;
mod galleries;
mod home;
mod insert;
mod masters;
mod picture_format;
mod playback;
mod review;
mod shape_format;
mod slide_show;
mod smartart;
mod table;
mod title_bar;
mod transitions;
mod view;

use animations::animations;
pub use animations::paint_anim_tile;
#[cfg(test)]
use animations::{animation_effect_rows, marked_options, text_build_id};
pub use arrange::{align_menu, arrange_menu};
use chart::chart_design;
use design::design;
pub use design::paint_theme_tile;
use draw::draw;
pub use galleries::{cref_param, effects_menu, outline_menu, paint_preset, shapes_gallery};
pub use home::font_families;
use home::home;
use insert::insert;
pub use insert::table_grid_picker;
use masters::{handout_master, notes_master, slide_master};
use picture_format::picture_format;
use playback::playback;
use review::review;
use shape_format::shape_format;
use slide_show::{record, slide_show};
use smartart::smartart_design;
pub use table::paint_table_style;
use table::{table_design, table_layout};
pub use title_bar::{MAC_CONTENT_LEFT, MAC_TITLE_BAR, TITLE_H, title_bar};
#[cfg(test)]
use title_bar::{MAC_LIGHT, MAC_LIGHT_PITCH, MAC_LIGHTS_LEFT, title_geometry};
pub use transitions::paint_transition_tile;
#[cfg(test)]
use transitions::transition_effect_rows;
use transitions::transitions;
use view::view;

/// Is engine command `id` enabled right now?
/// Whether `id` names an engine or UI command.
pub fn known(id: &str) -> bool {
    deckcraft_engine::find_command(id).is_some() || crate::UI_COMMANDS.iter().any(|c| c.0 == id)
}

pub fn enabled(app: &SlideApp, id: &str) -> bool {
    match deckcraft_engine::find_command(id) {
        Some(c) => (c.enabled)(&app.session).is_ok(),
        None => crate::UI_COMMANDS.iter().any(|c| c.0 == id),
    }
}

fn run(app: &mut SlideApp, id: &str, p: Value) {
    let _ = app.run(id, p);
}

/// Which contextual tabs apply to the selection.
fn contextual(app: &SlideApp) -> Vec<&'static str> {
    let mut v = vec![];
    let Some(st) = app.session.active() else { return v };
    if st.selection.target == Target::Handout {
        v.push("Handout Master");
    } else if st.selection.target == Target::NotesMaster {
        v.push("Notes Master");
    } else if st.selection.target != Target::Slides {
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
    if deckcraft_engine::cmd::smartart::selected(st).is_some() {
        v.push("SmartArt Design");
    }
    if shapes.iter().any(|s| matches!(s.kind, ShapeKind::Chart(_))) {
        v.push("Chart Design");
    }
    if shapes.iter().any(|s| matches!(s.kind, ShapeKind::Media(_))) {
        v.push("Playback");
    }
    if app.equation.is_open() {
        v.push("Equation");
    }
    v
}

const TABS: [&str; 10] = ["Home", "Insert", "Draw", "Design", "Transitions", "Animations", "Slide Show", "Record", "Review", "View"];

/// Tab row and the ribbon card.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let ctx_tabs = contextual(app);
    // Editing an equation brings up the Equation tab once; leaving it goes back (below).
    let editing = ctx_tabs.contains(&"Equation");
    if editing && !app.equation.tab_shown {
        app.ui.tab = "Equation".into();
        app.ui.ribbon_collapsed = false;
    }
    app.equation.tab_shown = editing;
    if !TABS.contains(&app.ui.tab.as_str()) && !ctx_tabs.contains(&app.ui.tab.as_str()) {
        app.ui.tab = "Home".into();
    }
    let h = 28.0 + if app.ui.ribbon_collapsed { 4.0 } else { RIBBON_H + 20.0 };
    egui::Panel::top("ribbon").exact_size(h).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let rect = ui.max_rect();
        // Tabs. File comes first and opens the File tab's full-window view (Backstage), as in
        // PowerPoint.
        let mut x = rect.min.x + 12.0;
        let y = rect.min.y + 2.0;
        {
            let w = ui.fonts_mut(|f| f.layout_no_wrap("File".to_string(), theme::font(13.0), t.text).size().x) + 18.0;
            let r = Rect::from_min_size(pos2(x, y), vec2(w, 24.0));
            let resp = ui.interact(r, ui.scope_id().with("tab_file"), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "File"));
            if resp.hovered() {
                ui.painter().rect_filled(r.shrink2(vec2(2.0, 2.0)), CornerRadius::same(4), t.hover);
            }
            ui.painter().text(r.center(), Align2::CENTER_CENTER, "File", theme::font(13.0), t.text);
            if resp.on_hover_text("File (Alt+F)").clicked() {
                app.open_backstage(crate::backstage::Page::Home);
            }
            x += w + 2.0;
        }
        let all: Vec<(&str, bool)> = TABS.iter().map(|s| (*s, false)).chain(ctx_tabs.iter().map(|s| (*s, true))).collect();
        for (name, contextual) in all {
            let font = theme::font(13.0);
            let w = ui.fonts_mut(|f| f.layout_no_wrap(name.to_string(), font.clone(), t.text).size().x) + 18.0;
            let r = Rect::from_min_size(pos2(x, y), vec2(w, 24.0));
            let resp = ui.interact(r, ui.scope_id().with(("tab", name)), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
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
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        cui.set_clip_rect(inner.expand2(vec2(0.0, 3.0)));
        // Reserve a gutter on both sides for the overflow chevrons. They are painted and
        // interacted in that gutter, never on top of the first/last visible button: the bar they
        // reveal must not steal the click they were meant to make reachable.
        let edge = 18.0_f32.min(inner.width() * 0.25);
        let scroll_rect = inner.shrink2(vec2(edge, 0.0));
        let mut sui = ui.new_child(egui::UiBuilder::new().max_rect(scroll_rect));
        sui.set_clip_rect(scroll_rect.expand2(vec2(0.0, 3.0)));
        let tab = app.ui.tab.clone();
        // The card used to clip a too-wide group row with no way to reach the rest. A horizontal
        // scroll area plus edge chevrons keeps every group reachable at the default 1440 px
        // window, including Home's Editing group (Replace Fonts…). The bar is hidden; the
        // chevrons are the visible overflow control and are accessible buttons of their own.
        let request_id = cui.make_persistent_id(("ribbon_scroll_request", tab.as_str()));
        let requested = cui.ctx().data_mut(|d| d.remove_temp::<f32>(request_id));
        let mut area = egui::ScrollArea::horizontal().id_salt(("ribbon_scroll", tab.as_str()));
        if let Some(offset) = requested {
            area = area.horizontal_scroll_offset(offset);
        }
        let scroll =
            area.scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).auto_shrink([false, false]).show(&mut sui, |ui| {
                ui.set_min_height(RIBBON_H);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    match tab.as_str() {
                        "Equation" => crate::equation_tab::show(app, ui),
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
                        "SmartArt Design" => smartart_design(app, ui),
                        "Playback" => playback(app, ui),
                        "Slide Master" => slide_master(app, ui),
                        "Handout Master" => handout_master(app, ui),
                        "Notes Master" => notes_master(app, ui),
                        _ => {}
                    }
                });
            });
        let r = scroll.inner_rect;
        let overflow = (scroll.content_size.x - r.width()).max(0.0);
        let offset = scroll.state.clamped_offset().x;
        // Record the rendered offset for `InputTarget`: the same window point hits a different
        // button when the ribbon scrolls between queueing and delivering a control click.
        app.ribbon_scroll.insert(tab.clone(), offset.to_bits());
        let step = 160.0;
        let mut next = None;
        if offset > 1.0 {
            let left = Rect::from_min_max(pos2((r.left() - edge).max(inner.left()), r.top()), pos2(r.left(), r.bottom()));
            if edge_cue(&mut cui, left, "‹", "Scroll ribbon left", &t) {
                next = Some(offset - step);
            }
        }
        if overflow - offset > 1.0 {
            let right = Rect::from_min_max(pos2(r.right(), r.top()), pos2((r.right() + edge).min(inner.right()), r.bottom()));
            if edge_cue(&mut cui, right, "›", "Scroll ribbon right", &t) {
                next = Some(offset + step);
            }
        }
        if let Some(x) = next {
            ui.ctx().data_mut(|d| d.insert_temp(request_id, x.clamp(0.0, overflow)));
            ui.ctx().request_repaint();
        }
    });
}

/// A chevron over a ribbon edge that scrolls it; `true` on the frame it is clicked.
fn edge_cue(ui: &mut Ui, rect: Rect, glyph: &str, hint: &str, t: &Tokens) -> bool {
    let resp = ui.interact(rect, ui.scope_id().with(glyph), Sense::click()).on_hover_text(hint);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, hint));
    let ink = if resp.hovered() { t.text } else { t.text_dim };
    ui.painter().rect_filled(rect, CornerRadius::ZERO, t.card);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, egui::FontId::proportional(18.0), ink);
    resp.clicked()
}

/// Draw a large ribbon command button, honour its enabled state, run the command on click, and
/// attach its menu.
fn big(app: &mut SlideApp, ui: &mut Ui, icon: Icon, label: &str, id: &str, p: Value) {
    let en = enabled(app, id);
    let r = big_button(ui, icon, label, en);
    if id == "insert.dateTime" {
        let name = label.replace('\n', " ");
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, en, &name));
    }
    let r = r.on_hover_text(tip(id, label));
    if r.clicked() {
        run(app, id, p.clone());
    }
    crate::qat::ribbon_menu(app, &r, icon, label, id, &p);
}

fn small(app: &mut SlideApp, ui: &mut Ui, icon: Icon, label: &str, id: &str, p: Value) {
    let en = enabled(app, id);
    let r = small_button(ui, icon, label, en, false).on_hover_text(tip(id, label));
    if r.clicked() {
        run(app, id, p.clone());
    }
    crate::qat::ribbon_menu(app, &r, icon, label, id, &p);
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

pub fn menu_item(app: &mut SlideApp, ui: &mut Ui, icon: Icon, label: &str, id: &str, p: Value) {
    let en = enabled(app, id);
    let r = small_button(ui, icon, label, en, false);
    if r.clicked() {
        run(app, id, p);
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::anim::TextBuild;

    /// Paints one transition tile and one animation tile and returns the colour of every label.
    fn tile_label_colours(dark: bool) -> Vec<Color32> {
        let ctx = egui::Context::default();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(80.0, 60.0));
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint_transition_tile(ui.painter(), r, ("fade", "Fade"));
            paint_anim_tile(ui.painter(), r, "Appear", AnimClass::Entrance, true);
        });
        out.textures_delta.clear();
        out.shapes
            .iter()
            .flat_map(|c| match &c.shape {
                egui::Shape::Text(s) => s.galley.job.sections.iter().map(|x| x.format.color).collect(),
                _ => Vec::new(),
            })
            .collect()
    }

    #[test]
    fn ribbon_tile_labels_follow_the_theme() {
        // #159: the transition and animation tiles were always painted with the light palette,
        // so their labels were dark grey on the dark ribbon.
        let dark = tile_label_colours(true);
        assert_eq!(dark, vec![Tokens::dark().text; 2]);
        let light = tile_label_colours(false);
        assert_eq!(light, vec![Tokens::light().text; 2]);
    }

    fn marked(rows: Vec<(&str, bool)>) -> Vec<&str> {
        rows.into_iter().filter(|(_, on)| *on).map(|(id, _)| id).collect()
    }

    #[test]
    fn effect_options_mark_only_the_stored_choice() {
        assert_eq!(marked(transition_effect_rows("push", Some("l"))), vec!["l"]);
        assert_eq!(transition_effect_rows("push", Some("l")).into_iter().map(|(_, on)| on).collect::<Vec<_>>(), vec![false, false, true, false]);
        assert_eq!(marked(transition_effect_rows("fade", Some("throughBlack"))), vec!["throughBlack"]);
        assert!(marked(transition_effect_rows("push", Some("missing"))).is_empty());
        assert!(transition_effect_rows("none", None).is_empty());
        assert_eq!(marked(transition_effect_rows("cut", Some(""))), vec![""]);
        assert_eq!(marked(animation_effect_rows("fly", AnimClass::Entrance, Some("l"))), vec!["l"]);
        assert!(marked(animation_effect_rows("fly", AnimClass::Entrance, Some("nope"))).is_empty());
        assert!(animation_effect_rows("appear", AnimClass::Entrance, Some("l")).is_empty());
        assert_eq!(marked(marked_options(&["asOne", "byParagraph"], Some(text_build_id(TextBuild::ByParagraph)))), vec!["byParagraph"]);
        assert_eq!(marked(marked_options(&["asOne", "byParagraph"], Some(text_build_id(TextBuild::AsOne)))), vec!["asOne"]);
        assert!(marked(marked_options(&["asOne", "byParagraph"], Some(text_build_id(TextBuild::ByWord)))).is_empty());
    }
}

#[cfg(test)]
mod effects_tests;
pub mod qat;

#[cfg(test)]
mod master_layout_tests;

#[cfg(test)]
mod icons_popup_tests;

#[cfg(test)]
mod align_tests;

#[cfg(test)]
mod title_bar_tests {
    use super::*;

    #[test]
    fn mac_title_bar_centres_and_clears_the_traffic_lights() {
        let (h, left, control) = title_geometry(true);
        // AppKit centres the lights in its own band, so the bar is that band.
        assert_eq!(h, MAC_TITLE_BAR);
        // Content keeps the same gap after the lights as the lights keep from the top.
        let lights_end = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT;
        assert_eq!(left - lights_end, (h - MAC_LIGHT) / 2.0);
        // The tallest controls keep at least 4 pt above and below.
        assert!(control <= h - 8.0);
        // Other platforms keep their system frame and the original bar.
        assert_eq!(title_geometry(false), (TITLE_H, 10.0, 26.0));
    }
}

#[cfg(test)]
mod drawtable_tests;

#[cfg(test)]
mod inktoshape_tests;
