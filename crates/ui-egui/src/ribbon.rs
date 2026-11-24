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
mod doc_tabs;
mod draw;
pub(crate) mod fold;
mod galleries;
mod home;
mod insert;
mod masters;
mod maths;
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
pub use doc_tabs::doc_tabs;
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
    v
}

const TABS: [&str; 10] = ["Home", "Insert", "Draw", "Design", "Transitions", "Animations", "Slide Show", "Record", "Review", "View"];

/// Height of the tab row, the card's inner padding, its side margin, and the gap under it.
const TAB_ROW: f32 = 40.0;
const CARD_PAD: f32 = 12.0;
const CARD_MARGIN: f32 = 12.0;
const CARD_GAP: f32 = 8.0;

/// Tab row and the ribbon card.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let ctx_tabs = contextual(app);
    if !TABS.contains(&app.ui.tab.as_str()) && !ctx_tabs.contains(&app.ui.tab.as_str()) {
        app.ui.tab = "Home".into();
    }
    // The tab row, then a floating card with room around its contents (the design system's
    // "roomy ribbon": space between groups instead of rules, rounded, softly raised).
    let h = TAB_ROW + if app.ui.ribbon_collapsed { 6.0 } else { CARD_PAD * 2.0 + RIBBON_H + CARD_GAP };
    egui::Panel::top("ribbon").exact_size(h).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let rect = ui.max_rect();
        // Tabs: text in a row of pills, the current one a white raised pill with the accent
        // colour as its text. File comes first and opens the File tab's full-window view
        // (Backstage), as in PowerPoint.
        let mut x = rect.min.x + CARD_MARGIN;
        let y = rect.min.y + 5.0;
        let pill = |ui: &mut Ui, x: f32, label: &str, id: egui::Id, active: bool, ink: egui::Color32| -> (egui::Response, f32) {
            let font = if active { theme::bold(13.5) } else { theme::font(13.5) };
            let w = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), t.text).size().x) + 26.0;
            let r = Rect::from_min_size(pos2(x, y), vec2(w, 30.0));
            let resp = ui.interact(r, id, Sense::click());
            // Only File is named for screen readers and tests (as before): naming every tab
            // would make a tab and a dialog button of the same name ("Insert") ambiguous.
            if label == "File" {
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
            }
            if active {
                ui.painter().add(egui::epaint::Shadow { offset: [0, 1], blur: 6, spread: 0, color: t.shadow }.as_shape(r, CornerRadius::same(15)));
                ui.painter().rect_filled(r, CornerRadius::same(15), t.card);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(15), t.hover);
            }
            ui.painter().text(r.center(), Align2::CENTER_CENTER, label, font, ink);
            (resp, w)
        };
        {
            let (resp, w) = pill(ui, x, "File", ui.scope_id().with("tab_file"), false, t.text);
            if resp.on_hover_text("File (Alt+F)").clicked() {
                app.open_backstage(crate::backstage::Page::Home);
            }
            x += w + 2.0;
        }
        let all: Vec<(&str, bool)> = TABS.iter().map(|s| (*s, false)).chain(ctx_tabs.iter().map(|s| (*s, true))).collect();
        for (name, contextual) in all {
            let active = app.ui.tab == name;
            let ink = if active || contextual { if contextual { t.contextual } else { t.accent } } else { t.text_dim };
            let (resp, w) = pill(ui, x, name, ui.scope_id().with(("tab", name)), active, ink);
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
        // The card: white, 16 radius, a hairline and a soft shadow.
        let card = Rect::from_min_max(
            pos2(rect.min.x + CARD_MARGIN, rect.min.y + TAB_ROW),
            pos2(rect.max.x - CARD_MARGIN, rect.min.y + TAB_ROW + CARD_PAD * 2.0 + RIBBON_H),
        );
        let corner = CornerRadius::same(theme::radius::SURFACE);
        ui.painter().add(egui::epaint::Shadow { offset: [0, 4], blur: 18, spread: 0, color: t.shadow }.as_shape(card, corner));
        ui.painter().rect_filled(card, corner, t.card);
        ui.painter().rect_stroke(card, corner, egui::Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
        let inner =
            Rect::from_min_size(pos2(card.min.x + CARD_MARGIN - 2.0, card.min.y + CARD_PAD), vec2(card.width() - CARD_MARGIN * 2.0 + 4.0, RIBBON_H));
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Min)));
        cui.set_clip_rect(inner.expand2(vec2(0.0, 3.0)));
        cui.spacing_mut().item_spacing.x = 1.0;
        fold::responsive_tab(app, &mut cui, inner);
        // A gallery sized from this reads it on the next frame, so a change (a resize, a tab
        // shown for the first time) asks for one; within half a point it is the same layout.
        let tab = app.ui.tab.clone();
        let spare = inner.width() - fold::tab_width(app, &tab).unwrap_or_else(|| cui.min_rect().width());
        let moved = ui.ctx().data_mut(|d| {
            let was = d.get_temp::<f32>(spare_room_id(&tab));
            d.insert_temp(spare_room_id(&tab), spare);
            was.is_none_or(|was| (was - spare).abs() > 0.5)
        });
        if moved {
            ui.ctx().request_repaint();
        }
    });
}

fn spare_room_id(tab: &str) -> egui::Id {
    egui::Id::unique(("ribbon_spare_room", tab))
}

/// The width the tab's groups left free in the ribbon on the last frame it was drawn; negative
/// when they ran past its edge (folded into More included). A gallery that gives way to the
/// groups after it sizes itself from this.
pub(super) fn spare_room(ctx: &egui::Context, tab: &str) -> Option<f32> {
    ctx.data(|d| d.get_temp::<f32>(spare_room_id(tab)))
}

/// The groups of the current ribbon tab.
fn render_tab(app: &mut SlideApp, ui: &mut Ui) {
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
        "SmartArt Design" => smartart_design(app, ui),
        "Playback" => playback(app, ui),
        "Slide Master" => slide_master(app, ui),
        "Handout Master" => handout_master(app, ui),
        "Notes Master" => notes_master(app, ui),
        _ => {}
    }
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
mod add_animation_menu_tests;

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

#[cfg(test)]
mod picture_styles_tests;
