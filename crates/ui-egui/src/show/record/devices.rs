//! Device controls for the Record view. A section agent owns this file; the shell only provides
//! the slots and the state it reads each frame.
//!
//! Nothing here invents a device: with no backend the state stays [`DeviceStatus::Unavailable`]
//! and the button reports why it cannot change anything.

use egui::{Color32, RichText, Ui};

use super::RecordView;

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
    pub(in crate::show) fn is_on(&self) -> bool {
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

/// Attach the presentation services' device hosts to the shell once per frame. The microphone
/// follows the capture service — absent, the control reads unavailable; present, it starts on,
/// as PowerPoint's narration capture does — and the take honours the user's toggle.
pub(super) fn connect(rec: &mut RecordView, services: &crate::Services) {
    let mic = &mut rec.devices.mic.status;
    if services.mic.is_none() {
        *mic = DeviceStatus::Unavailable;
    } else if *mic == DeviceStatus::Unavailable {
        *mic = DeviceStatus::On;
    }
}

pub(super) fn camera_button(ui: &mut Ui, rec: &mut RecordView, _now: f64) -> bool {
    toggle(ui, &mut rec.devices.camera, "Camera")
}

pub(super) fn mic_button(ui: &mut Ui, rec: &mut RecordView, _now: f64) -> bool {
    toggle(ui, &mut rec.devices.mic, "Microphone")
}

pub(super) fn screen_button(ui: &mut Ui, rec: &mut RecordView, _now: f64) -> bool {
    toggle(ui, &mut rec.devices.screen, "Screen recording")
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
