//! The Record view's Views dropdown: PowerPoint's three record layouts, plus the camera preview
//! toggle. A section agent owns the menu's contents; the shell only calls it and reads
//! [`ViewKind`] for the layout.

use egui::{Color32, CornerRadius, Sense, Ui, pos2, vec2};

use super::{Layout, RecordView};
use crate::theme;

/// How the Record view splits the screen between the script and the slide.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ViewKind {
    /// A large script with the slide below it.
    #[default]
    Teleprompter,
    /// The slide with a compact script and a next-slide strip.
    Presenter,
    /// The slide alone.
    Slide,
}

impl ViewKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            ViewKind::Teleprompter => "Teleprompter",
            ViewKind::Presenter => "Presenter View",
            ViewKind::Slide => "Slide",
        }
    }
}

/// Draw the Views dropdown in the bottom-right slot and the camera-preview icon beside it.
pub(super) fn ui(ui: &mut Ui, rec: &mut RecordView, layout: &Layout) {
    let mut changed = false;
    let painter = ui.painter().clone();
    // Camera preview icon: a filled camera body when the preview is up.
    let cp = layout.camera_preview;
    // The Record view has no camera surface yet: hovering explains that instead of offering a
    // click that toggles state nothing draws.
    let cp_resp = ui.interact(cp, ui.scope_id().with("record_camera_preview"), Sense::hover());
    let cp_bg = if cp_resp.hovered() { Color32::from_gray(58) } else { Color32::from_gray(38) };
    painter.rect_filled(cp, CornerRadius::same(5), cp_bg);
    let body = cp.shrink(6.0);
    painter.rect_filled(body, CornerRadius::same(2), if rec.camera_preview { Color32::from_rgb(90, 170, 230) } else { Color32::from_gray(190) });
    painter.rect_filled(
        egui::Rect::from_min_size(pos2(body.max.x - 1.0, body.center().y - 3.0), vec2(4.0, 6.0)),
        CornerRadius::ZERO,
        if rec.camera_preview { Color32::from_rgb(90, 170, 230) } else { Color32::from_gray(190) },
    );
    cp_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::CheckBox, false, "Camera preview"));
    cp_resp.on_hover_text("The Record view's camera preview arrives with the camera section");

    let r = layout.views;
    let resp = ui.interact(r, ui.scope_id().with("record_views"), Sense::click());
    painter.rect_filled(r, CornerRadius::same(5), if resp.hovered() { Color32::from_gray(58) } else { Color32::from_gray(38) });
    painter.text(
        pos2(r.min.x + 10.0, r.center().y),
        egui::Align2::LEFT_CENTER,
        format!("Views: {}", rec.view.label()),
        theme::font(12.0),
        Color32::from_gray(220),
    );
    painter.text(pos2(r.max.x - 10.0, r.center().y), egui::Align2::RIGHT_CENTER, "▾", theme::font(12.0), Color32::from_gray(200));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Views"));
    let popup = egui::Popup::menu(&resp).width(190.0);
    popup.show(|ui| {
        for view in [ViewKind::Teleprompter, ViewKind::Presenter, ViewKind::Slide] {
            let picked = ui.selectable_label(rec.view == view, view.label()).clicked();
            if picked {
                rec.view = view;
                rec.notes_open = view != ViewKind::Slide;
                changed = true;
                ui.close();
            }
        }
        ui.separator();
        // The shell has no camera surface yet: the switch is offered, but reads as unavailable
        // rather than toggling a preview nothing draws.
        let camera = ui.add_enabled(false, egui::Checkbox::new(&mut rec.camera_preview, "Camera preview"));
        let _ = camera.on_hover_text("The Record view's camera preview arrives with the camera section");
        // Slide view hides the pane, so its switch has nothing to show.
        let notes = ui.add_enabled(rec.view != ViewKind::Slide, egui::Checkbox::new(&mut rec.notes_open, "Notes pane"));
        let changed_notes = notes.changed();
        let _ = notes.on_hover_text("Slide view hides the script");
        if changed_notes {
            changed = true;
        }
    });
    let _ = (MENU_BG, changed);
}

/// The bottom-right Views dropdown background colour, shared with section agents.
pub(super) const MENU_BG: Color32 = Color32::from_rgb(38, 38, 42);
