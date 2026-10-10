//! File tab (Backstage): New, Open, Save, Save As, Print, Export.

use egui::{Align2, Sense, Ui, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::theme::{self, Tokens};

/// PowerPoint's brand red — fixed regardless of light/dark mode, same as the blue backstage
/// panel in WordCraft and the green one in GridCraft.
pub const APP_COLOR: egui::Color32 = egui::Color32::from_rgb(0xB7, 0x27, 0x2C);

const PAGES: [(&str, &str); 6] =
    [("new", "New"), ("open", "Open"), ("save", "Save"), ("saveAs", "Save As"), ("print", "Print"), ("export", "Export")];

pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::left("backstage_nav")
        .exact_size(200.0)
        .frame(egui::Frame::NONE.fill(APP_COLOR).inner_margin(egui::Margin { left: 0, right: 0, top: 12, bottom: 12 }))
        .show(ui, |ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(200.0, 40.0), Sense::click());
            ui.painter().text(pos2(r.min.x + 22.0, r.center().y), Align2::LEFT_CENTER, "←", theme::bold(16.0), egui::Color32::WHITE);
            if resp.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                app.ui.backstage = false;
            }
            ui.add_space(6.0);
            for (id, label) in PAGES {
                let (r, resp) = ui.allocate_exact_size(vec2(200.0, 38.0), Sense::click());
                let active = app.ui.backstage_page == id;
                if active {
                    ui.painter().rect_filled(r, 0.0, egui::Color32::from_white_alpha(46));
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, egui::Color32::from_white_alpha(26));
                }
                ui.painter().text(
                    pos2(r.min.x + 22.0, r.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    if active { theme::bold(14.0) } else { theme::font(14.0) },
                    egui::Color32::WHITE,
                );
                if resp.clicked() {
                    match id {
                        "save" => app.save(),
                        "saveAs" => app.save_as_dialog("deckcraft"),
                        "export" => {
                            app.dialog = Some(crate::dialogs::Dialog::new("export"));
                            app.ui.backstage = false;
                        }
                        _ => app.ui.backstage_page = id.into(),
                    }
                }
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                let (r, resp) = ui.allocate_exact_size(vec2(200.0, 32.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, egui::Color32::from_white_alpha(26));
                }
                ui.painter().text(pos2(r.min.x + 22.0, r.center().y), Align2::LEFT_CENTER, "About", theme::font(13.0), egui::Color32::WHITE);
                if resp.clicked() {
                    app.dialog = Some(crate::dialogs::Dialog::new("about"));
                    app.ui.backstage = false;
                }
            });
        });
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(40, 30))).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| match app.ui.backstage_page.as_str() {
            "open" => open_page(app, ui),
            "print" => print_page(app, ui),
            _ => new_page(app, ui),
        });
    });
}

fn heading(ui: &mut Ui, s: &str) {
    ui.label(egui::RichText::new(s).font(theme::bold(26.0)));
    ui.add_space(16.0);
}

fn recent_list(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new("Recent").font(theme::bold(16.0)));
    ui.add_space(6.0);
    if app.ui.recent.is_empty() {
        ui.label(egui::RichText::new("Presentations you open will show up here.").color(t.text_dim));
    }
    for p in app.ui.recent.clone() {
        let name = std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(p.clone());
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(700.0), 40.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 4.0, t.hover);
        }
        ui.painter().text(pos2(r.min.x + 6.0, r.min.y + 13.0), Align2::LEFT_CENTER, &name, theme::font(13.0), t.text);
        ui.painter().text(pos2(r.min.x + 6.0, r.min.y + 29.0), Align2::LEFT_CENTER, &p, theme::font(11.0), t.text_dim);
        if resp.clicked() {
            let _ = app.open_path(&p);
            app.ui.backstage = false;
        }
    }
}

fn new_page(app: &mut SlideApp, ui: &mut Ui) {
    heading(ui, "New");
    if ui.add(egui::Button::new(egui::RichText::new("Blank presentation").font(theme::font(14.0))).min_size(vec2(220.0, 34.0))).clicked() {
        let _ = app.run("file.new", json!({}));
        app.ui.backstage = false;
    }
    ui.add_space(28.0);
    recent_list(app, ui);
}

fn open_page(app: &mut SlideApp, ui: &mut Ui) {
    heading(ui, "Open");
    if ui.add(egui::Button::new(egui::RichText::new("Browse…").font(theme::font(14.0))).min_size(vec2(220.0, 34.0))).clicked() {
        let _ = app.run("app.openDialog", json!({}));
    }
    ui.add_space(18.0);
    recent_list(app, ui);
}

fn print_page(app: &mut SlideApp, ui: &mut Ui) {
    heading(ui, "Print");
    ui.label("Send the active presentation straight to a printer.");
    ui.add_space(12.0);
    if ui.add(egui::Button::new(egui::RichText::new("Print").font(theme::font(13.5))).min_size(vec2(320.0, 34.0))).clicked() {
        app.print_now();
    }
}
