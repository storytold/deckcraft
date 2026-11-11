//! Camera Format ▸ Camera: the Preview split button and its More Options menu.
//!
//! This section owns the toggle, not the device. `cameo.preview` asks the UI host's camera source
//! to open or release a camera through `UiRequest::CameraPreview`, and `cameo.preview.state` reads
//! back what the source reported. Live frames are capture work; here the preview is started,
//! stopped, and every outcome (requesting, live, denied, no device, busy, lost, unsupported) is
//! named where the user can see it.

use deckcraft_media::camera::CameraStatus;

use super::super::{Icon, SlideApp, Ui, big_button, big_button_ex, enabled, group, json, run, widgets};

pub(super) fn camera_section(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let on = app.camera_preview.is_on();
        let (main, arrow) = big_button_ex(ui, Icon::Cameo, "Enable\nCamera\nPreview", enabled(app, "cameo.preview"), true, on);
        let main = main.on_hover_text(tooltip(app));
        if main.clicked() {
            run(app, "cameo.preview", json!({}));
        }
        if let Some(arrow) = arrow {
            let arrow = arrow.on_hover_text("Enable Camera Preview options");
            // The menu opens on a fresh device list, not the one from the previous opening.
            if arrow.clicked() {
                app.camera_preview.refresh_devices();
            }
            widgets::arrow_popup(&main, &arrow).show(|ui| preview_menu(app, ui));
        }
        let more = big_button(ui, Icon::Preview, "More\nOptions", true).on_hover_text("Camera preview options");
        if more.clicked() {
            app.camera_preview.refresh_devices();
        }
        egui::Popup::menu(&more).show(|ui| preview_menu(app, ui));
    });
}

/// The Preview dropdown: the toggle preview switch, then the cameras the source listed.
fn preview_menu(app: &mut SlideApp, ui: &mut Ui) {
    let status = app.camera_preview.status();
    ui.label(status_line(app));
    ui.separator();
    let on = app.camera_preview.is_on();
    // A running preview can always be stopped; starting one needs the command's own conditions.
    let usable = on || enabled(app, "cameo.preview");
    if ui.add_enabled(usable, egui::Button::new(if on { "Turn Off Camera Preview" } else { "Turn On Camera Preview" })).clicked() {
        run(app, "cameo.preview", json!({"on": !on}));
        ui.close();
    }
    ui.separator();
    ui.label("Camera");
    let picked = app.camera_preview.picked().map(str::to_string);
    let devices = app.camera_preview.devices().to_vec();
    if devices.is_empty() {
        ui.label(empty_list(status));
    } else {
        for device in devices {
            let selected = picked.as_deref() == Some(device.id.as_str()) || (picked.is_none() && device.default);
            let label = ui.add_enabled(usable, egui::Button::new(&device.name).selected(selected).frame(false));
            if label.clicked() {
                run(app, "cameo.preview", json!({"on": true, "device": device.id}));
                ui.close();
            }
        }
    }
    ui.separator();
    if ui.button("Refresh Camera List").clicked() {
        app.camera_preview.refresh_devices();
        ui.close();
    }
}

/// "Camera preview live — Integrated Camera", or the state on its own.
fn status_line(app: &SlideApp) -> String {
    let status = app.camera_preview.status();
    match app.camera_preview.device_name() {
        Some(name) if !name.is_empty() => format!("{} — {name}", status.message()),
        _ => status.message().to_string(),
    }
}

/// Why the camera list is empty; a build without capture is not the same as a machine without a
/// camera.
fn empty_list(status: CameraStatus) -> &'static str {
    if status == CameraStatus::Unsupported { "Camera preview is not available in this build" } else { "No camera found" }
}

fn tooltip(app: &SlideApp) -> String {
    let status = app.camera_preview.status();
    let device = app.camera_preview.device_name().map(|n| format!(" ({n})")).unwrap_or_default();
    match status {
        CameraStatus::Off => "Enable Camera Preview".to_string(),
        CameraStatus::Requesting | CameraStatus::Live => format!("Turn Off Camera Preview{device}"),
        _ => format!("Retry Camera Preview — {}{device}", status.message()),
    }
}
