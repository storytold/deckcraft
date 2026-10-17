//! File › Options: settings panes on the left (General, Display, Editing, Save, Advanced,
//! Agents), the chosen pane's settings on the right.
//!
//! Adapted from WordCraft for DeckCraft (presentations).

use egui::{RichText, Ui, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::theme::{self, Tokens};

/// The options panes, in display order.
pub const PANES: [&str; 6] = ["general", "display", "editing", "save", "advanced", "agents"];

fn pane_label(id: &str) -> &'static str {
    match id {
        "display" => "Display",
        "editing" => "Editing",
        "save" => "Save",
        "advanced" => "Advanced",
        "agents" => "Agents",
        _ => "General",
    }
}

/// The Options page.
pub fn page(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new("Options").font(theme::bold(22.0)).color(t.text));
    ui.add_space(16.0);
    let current = if PANES.contains(&app.ui.options_pane.as_str()) { app.ui.options_pane.clone() } else { "general".to_string() };
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(170.0);
            for id in PANES {
                let label = pane_label(id);
                let active = current == id;
                let (r, resp) = ui.allocate_exact_size(vec2(170.0, 32.0), egui::Sense::click());
                resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, active, label));
                if active {
                    ui.painter().rect_filled(r, 4.0, t.selected);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 4.0, t.hover);
                }
                let font = if active { theme::bold(14.0) } else { theme::font(14.0) };
                ui.painter().text(egui::pos2(r.min.x + 12.0, r.center().y), egui::Align2::LEFT_CENTER, label, font, t.text);
                if resp.clicked() {
                    app.ui.options_pane = id.to_string();
                }
            }
        });
        ui.add_space(12.0);
        let (line, _) = ui.allocate_exact_size(vec2(1.0, 440.0), egui::Sense::hover());
        ui.painter().rect_filled(line, 0.0, t.border);
        ui.add_space(20.0);
        ui.vertical(|ui| {
            ui.set_max_width(640.0);
            match current.as_str() {
                "display" => display(app, ui),
                "editing" => editing(app, ui),
                "save" => save(app, ui),
                "advanced" => advanced(app, ui),
                "agents" => agents(ui),
                _ => general(app, ui),
            }
        });
    });
}

fn section(ui: &mut Ui, title: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(6.0);
    ui.label(RichText::new(title).font(theme::bold(15.0)).color(t.text));
    ui.add_space(2.0);
}

fn note(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(text).font(theme::font(12.0)).color(t.text_dim));
}

fn general(app: &mut SlideApp, ui: &mut Ui) {
    section(ui, "Interface");
    ui.horizontal(|ui| {
        ui.label("Interface theme:");
        let dark = app.ui.brightness == theme::Brightness::Dark;
        egui::ComboBox::from_id_salt("opt_theme").width(200.0).selected_text(if dark { "Dark" } else { "Light" }).show_ui(ui, |ui| {
            for (on, label) in [(false, "Light"), (true, "Dark")] {
                if ui.selectable_label(dark == on, label).clicked() {
                    let _ = app.run("view.dark", json!({"on": on}));
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Interface scale:");
        let mut scale = app.ui.ui_scale;
        if ui.add(egui::Slider::new(&mut scale, 0.75..=1.75).step_by(0.05).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))).changed() {
            app.ui.ui_scale = scale;
        }
    });
    ui.add_space(8.0);
    section(ui, "Start-up");
    ui.checkbox(&mut app.ui.start_screen, "Show the Start screen when DeckCraft starts");
    note(ui, "On: DeckCraft opens on File › Home with themes and recent presentations. Off: it opens a blank presentation directly.");
    ui.add_space(8.0);
    section(ui, "Personalize");
    egui::Grid::new("opt_user").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        ui.label("User name:");
        ui.add(egui::TextEdit::singleline(&mut app.session.prefs.author).desired_width(260.0));
        ui.end_row();
    });
    note(ui, "Comments and notes added to slides are recorded under this name.");
}

fn display(app: &mut SlideApp, ui: &mut Ui) {
    section(ui, "Slide display");
    ui.checkbox(&mut app.ui.ruler, "Show rulers");
    ui.checkbox(&mut app.ui.gridlines, "Show gridlines");
    ui.checkbox(&mut app.ui.guides, "Show drawing guides");
    ui.checkbox(&mut app.ui.grayscale, "Grayscale view");
    ui.checkbox(&mut app.ui.notes, "Show notes pane by default");
    note(ui, "These view options control the default canvas display for presentations.");
}

fn editing(app: &mut SlideApp, ui: &mut Ui) {
    section(ui, "Guides & Snapping");
    ui.checkbox(&mut app.session.prefs.smart_guides, "Show smart guides");
    note(ui, "Displays dynamic alignment guides when moving and resizing shapes.");
    ui.checkbox(&mut app.session.prefs.snap_to_grid, "Snap objects to grid");
    ui.horizontal(|ui| {
        ui.label("Grid spacing (pt):");
        let mut spacing = app.session.prefs.grid_spacing;
        if ui.add(egui::DragValue::new(&mut spacing).range(1.0..=72.0).speed(0.5)).changed() {
            app.session.prefs.grid_spacing = spacing.clamp(1.0, 72.0);
        }
    });
    ui.add_space(8.0);
    section(ui, "Typing");
    ui.checkbox(&mut app.session.prefs.smart_quotes, "Replace straight quotes with smart quotes");
    ui.checkbox(&mut app.session.prefs.autocorrect, "AutoCorrect as you type");
}

fn save(app: &mut SlideApp, ui: &mut Ui) {
    section(ui, "Save presentations");
    ui.horizontal(|ui| {
        ui.label("Save AutoRecover information every:");
        let mut mins = app.session.prefs.recovery_minutes;
        if ui.add(egui::DragValue::new(&mut mins).range(0.5..=60.0).speed(0.5)).changed() {
            app.session.prefs.recovery_minutes = mins.clamp(0.5, 60.0);
        }
        ui.label("minutes");
    });
    note(ui, "DeckCraft saves recovery drafts of unsaved presentations to prevent data loss.");
}

fn advanced(app: &mut SlideApp, ui: &mut Ui) {
    section(ui, "Slide Show");
    ui.checkbox(&mut app.ui.presenter_view, "Use Presenter View by default");
    note(ui, "Presenter View displays current slide, next slide, timer and speaker notes on a secondary screen.");
    ui.add_space(8.0);
    section(ui, "Editing");
    ui.horizontal(|ui| {
        ui.label("Duplicate offset (pt):");
        let mut offset = app.session.prefs.duplicate_offset;
        if ui.add(egui::DragValue::new(&mut offset).range(0.0..=72.0).speed(1.0)).changed() {
            app.session.prefs.duplicate_offset = offset.clamp(0.0, 72.0);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Maximum undo steps:");
        let mut undo = app.session.prefs.undo_limit;
        if ui.add(egui::DragValue::new(&mut undo).range(10..=1000).speed(5.0)).changed() {
            app.session.prefs.undo_limit = undo.clamp(10, 1000);
        }
    });
}

fn agents(ui: &mut Ui) {
    section(ui, "Agents & Automation");
    ui.label(
        "Every command is available to scripts and AI agents: run `deckcraft-cli mcp` for an MCP server, \
        or start the app with `--control <port>` for the JSON control channel.",
    );
    ui.add_space(10.0);
    ui.hyperlink_to("Join our community on Discord", deckcraft_engine::links::DISCORD);
    ui.hyperlink_to("Source on GitHub", deckcraft_engine::links::GITHUB);
}
