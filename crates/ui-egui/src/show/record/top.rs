//! The floating command bar of the Record view: Edit, the capture clock, Reset/Undo,
//! Record/Stop, Pause/Resume, camera, microphone, screen recording, More and Export.
//!
//! Every button acts on [`RecordView`] or [`Show`] itself. Device controls call the
//! [`super::devices`] hooks; Export says that the video renderer is not wired yet, so nothing here
//! reports success it did not have.

use egui::{Align, Color32, CornerRadius, Layout as EguiLayout, Rect, RichText, Ui, UiBuilder, vec2};

use super::devices;
use super::{RecordStatus, RecordView, clock_total};
use crate::SlideApp;
use crate::show::Show;
use crate::theme;

pub(super) struct Out {
    pub close: bool,
}

const FG: Color32 = Color32::from_gray(226);
const DIM: Color32 = Color32::from_gray(150);
const RED: Color32 = Color32::from_rgb(224, 62, 62);

/// Draw the top bar. Returns `close` when Edit was pressed.
pub(super) fn ui(app: &mut SlideApp, ui: &mut Ui, show: &mut Show, rec: &mut RecordView, now: f64, layout: &super::Layout) -> Out {
    let rect = layout.top;
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::same(10), Color32::from_rgb(30, 30, 34));
    painter.rect_stroke(rect, CornerRadius::same(10), egui::Stroke::new(1.0, Color32::from_gray(64)), egui::StrokeKind::Inside);

    let mut out = Out { close: false };
    let inner = rect.shrink2(vec2(10.0, 6.0));
    let mut child = ui.new_child(UiBuilder::new().max_rect(inner).layout(EguiLayout::left_to_right(Align::Center)));
    child.spacing_mut().item_spacing.x = 6.0;
    child.visuals_mut().override_text_color = Some(FG);

    if child.add(egui::Button::new(RichText::new("‹ Edit").size(13.0).color(FG)).frame(false)).on_hover_text("Leave the Record view").clicked() {
        let _ = super::command(rec, show, "record.stop", now);
        // Only a take that actually captured something is saved when the view closes.
        show.record = rec.captured;
        out.close = true;
    }

    // The clock sits in the middle of the bar, whatever the groups on either side.
    let elapsed = rec.elapsed(now);
    let clock = clock_total(elapsed, rec.total_seconds());
    let status = match rec.status {
        RecordStatus::Idle => DIM,
        RecordStatus::Recording => RED,
        RecordStatus::Paused => Color32::from_rgb(230, 168, 58),
    };
    painter.circle_filled(egui::pos2(rect.center().x - 66.0, rect.center().y), 4.0, status);
    painter.text(egui::pos2(rect.center().x - 54.0, rect.center().y), egui::Align2::LEFT_CENTER, clock, theme::font(14.0), FG);

    // Right-hand group, laid out from the right edge inward.
    let right = Rect::from_min_max(egui::pos2(rect.center().x + 40.0, inner.min.y), inner.max);
    let mut right_ui = ui.new_child(UiBuilder::new().max_rect(right).layout(EguiLayout::right_to_left(Align::Center)));
    right_ui.spacing_mut().item_spacing.x = 6.0;
    right_ui.visuals_mut().override_text_color = Some(FG);

    // The clock owns the middle of the bar. The optional controls (Export, More and the device
    // labels) are dropped when the right-hand side cannot hold them all, so the transport controls
    // never run over the clock on a narrow window.
    let measured =
        |text: &str, size: f32| ui.ctx().fonts_mut(|f| f.layout_no_wrap(text.to_owned(), egui::FontId::proportional(size), FG).size().x) + 14.0;
    let required = ["Reset", "Undo", "Pause", "● Record"].iter().map(|t| measured(t, 13.0)).sum::<f32>();
    let extras = measured("Export ›", 13.0) + measured("…", 13.0);
    let devices = measured("Camera", 12.0) + measured("Mic", 12.0) + measured("Screen", 12.0);
    let room = right.width() - 6.0 * 4.0;
    let (show_extras, show_devices) = if room >= required + extras + devices { (true, true) } else { (false, room >= required + devices) };

    if show_extras {
        let export = button(&mut right_ui, "Export ›", true);
        if export.on_hover_text("Export the finished recording (not wired yet)").clicked() {
            // No video renderer is wired in this shell: say so rather than claim an export happened.
            app.set_status("Exporting the recording as a video is not wired yet.");
        }
    }
    if show_extras {
        let more = button(&mut right_ui, "…", true);
        more.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "More"));
        let more_resp = more.on_hover_text("More recording options");
        egui::Popup::menu(&more_resp).width(220.0).show(|ui| {
            ui.checkbox(&mut rec.notes_open, "Notes pane");
            ui.checkbox(&mut rec.camera_bubble, "Camera bubble");
            ui.checkbox(&mut rec.camera_preview, "Camera preview");
            ui.separator();
            ui.add_enabled(false, egui::Button::new("Recording settings…"));
        });
    }
    if show_devices {
        if devices::screen_button(&mut right_ui, rec, now) {
            // The screen section owns the recorder behind this flag.
        }
        let _ = devices::mic_button(&mut right_ui, rec, now);
        if devices::camera_button(&mut right_ui, rec, now) {
            rec.camera_bubble = rec.devices.camera.is_on();
        }
    }

    let pause_label = if rec.status == RecordStatus::Paused { "Resume" } else { "Pause" };
    let pause = right_ui.add_enabled(rec.status != RecordStatus::Idle, egui::Button::new(RichText::new(pause_label).size(13.0)).frame(false));
    if pause.clicked() {
        let id = if rec.status == RecordStatus::Paused { "record.resume" } else { "record.pause" };
        let _ = super::command(rec, show, id, now);
    }
    let taking = rec.status != RecordStatus::Idle;
    let (record_label, record_hint) = if taking { ("■ Stop", "Stop recording") } else { ("● Record", "Start recording") };
    let record = right_ui.add(egui::Button::new(RichText::new(record_label).size(13.0).color(RED)).frame(false));
    record.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, if taking { "Stop recording" } else { "Record" }));
    if record.on_hover_text(record_hint).clicked() {
        let _ = super::command(rec, show, "record.toggle", now);
    }
    let undo = right_ui.add_enabled(rec.undo_elapsed.is_some(), egui::Button::new(RichText::new("Undo").size(13.0)).frame(false));
    if undo.clicked() {
        let _ = super::command(rec, show, "record.undo", now);
    }
    if right_ui.add(egui::Button::new(RichText::new("Reset").size(13.0)).frame(false)).on_hover_text("Reset the capture clock").clicked() {
        let _ = super::command(rec, show, "record.reset", now);
    }

    painter.text(
        egui::pos2(inner.max.x - 2.0, inner.max.y - 8.0),
        egui::Align2::RIGHT_BOTTOM,
        match rec.status {
            RecordStatus::Idle => "Ready",
            RecordStatus::Recording => "Recording",
            RecordStatus::Paused => "Paused",
        },
        theme::font(10.0),
        status,
    );
    out
}

fn button(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(label).size(13.0)).frame(false))
}
