//! Device controls for the Record view. A section agent owns this file; the shell only provides
//! the slots and the state it reads each frame.
//!
//! Nothing here invents a device: with no backend the state stays [`DeviceStatus::Unavailable`]
//! and the button reports why it cannot change anything.

use egui::{Color32, RichText, Ui};

use super::{RecordStatus, RecordView};

/// What a device control knows about its backend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::show) enum DeviceStatus {
    /// No backend or no device was found.
    #[default]
    Unavailable,
    /// A device is present and switched off.
    Off,
    /// A device is present and capturing.
    On,
}

/// One selectable device and its current state.
#[derive(Clone, Debug, Default)]
pub(in crate::show) struct DeviceState {
    pub status: DeviceStatus,
}

impl DeviceState {
    pub(super) fn is_on(&self) -> bool {
        self.status == DeviceStatus::On
    }

    pub(super) fn can_toggle(&self) -> bool {
        matches!(self.status, DeviceStatus::Off | DeviceStatus::On)
    }
}

/// Camera, microphone and screen capture as the shell sees them.
#[derive(Clone, Debug, Default)]
pub(in crate::show) struct Devices {
    pub camera: DeviceState,
    pub mic: DeviceState,
    pub screen: DeviceState,
}

/// Show what the presentation services and the take actually have, once per frame. A missing host
/// leaves the device [`DeviceStatus::Unavailable`]; a microphone the take is recording from is
/// [`DeviceStatus::On`], and the buttons only report that state (no backend is wired to them).
pub(super) fn connect(rec: &mut RecordView, services: &crate::Services, show: &crate::show::Show, screen: (bool, bool)) {
    // The shell's own `rec` is out of `show` while this runs, so the take state comes from `rec`
    // and the microphone from what the show actually started.
    let mic = services.mic.is_some();
    rec.devices.mic.status = match (mic, rec.status == RecordStatus::Recording && show.mic_live()) {
        (false, _) => DeviceStatus::Unavailable,
        (true, true) => DeviceStatus::On,
        (true, false) => DeviceStatus::Off,
    };
    // The capture host lives in the session (it was moved there at start-up), not in the service.
    let (host, recording) = screen;
    rec.devices.screen.status = match (host, recording) {
        (false, _) => DeviceStatus::Unavailable,
        (true, true) => DeviceStatus::On,
        (true, false) => DeviceStatus::Off,
    };
}

pub(super) fn camera_button(ui: &mut Ui, rec: &mut RecordView, _now: f64) -> bool {
    toggle(ui, &mut rec.devices.camera, "Camera")
}

/// The microphone is driven by the take, not by this label: it reports whether narration is being
/// captured, and never pretends a click changed it.
pub(super) fn mic_button(ui: &mut Ui, rec: &mut RecordView, _now: f64) -> bool {
    status(ui, &rec.devices.mic, "Microphone")
}

/// Screen capture is driven by the Record tab's picker: this label reports whether the host has a
/// capture device and whether the take is using it.
pub(super) fn screen_button(ui: &mut Ui, rec: &mut RecordView, _now: f64) -> bool {
    status(ui, &rec.devices.screen, "Screen recording")
}

/// One status-only device label. Returns false: it never reports a state change.
fn status(ui: &mut Ui, state: &DeviceState, name: &str) -> bool {
    let label = match state.status {
        DeviceStatus::On => format!("{name} on"),
        DeviceStatus::Off => format!("{name} off"),
        DeviceStatus::Unavailable => format!("{name} — unavailable"),
    };
    let color = if state.status == DeviceStatus::On { Color32::from_rgb(120, 200, 140) } else { Color32::from_gray(200) };
    let detail = match state.status {
        DeviceStatus::Unavailable => "No device found",
        DeviceStatus::On | DeviceStatus::Off => "Available",
    };
    let resp = ui.add(egui::Label::new(RichText::new(label).size(12.0).color(color)).sense(egui::Sense::hover()));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, state.is_on(), name));
    resp.on_hover_text(detail);
    false
}

/// One status-capable toggle. Returns whether the requested state changed; an unavailable,
/// denied or busy device never flips to a fake "on".
fn toggle(ui: &mut Ui, state: &mut DeviceState, name: &str) -> bool {
    let label = match state.status {
        DeviceStatus::On => format!("{name} on"),
        DeviceStatus::Off => format!("{name} off"),
        DeviceStatus::Unavailable => format!("{name} — unavailable"),
    };
    let color = if state.status == DeviceStatus::On { Color32::from_rgb(120, 200, 140) } else { Color32::from_gray(200) };
    let resp = ui.add_enabled(state.can_toggle(), egui::Button::new(RichText::new(label).size(12.0).color(color)).frame(false));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::CheckBox, state.is_on(), name));
    let detail = match state.status {
        DeviceStatus::Unavailable => "No device found",
        DeviceStatus::On | DeviceStatus::Off => "Device is available",
    };
    let resp = resp.on_hover_text(detail);
    if resp.clicked() {
        state.status = if state.status == DeviceStatus::On { DeviceStatus::Off } else { DeviceStatus::On };
        return true;
    }
    false
}
