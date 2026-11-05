//! Device controls for the Record view. A section agent owns this file; the shell only provides
//! the slots and the state it reads each frame.
//!
//! Nothing here invents a device: with no backend the state stays [`DeviceStatus::Unavailable`]
//! and the button reports why it cannot change anything.

use egui::{Color32, RichText, Ui};

use deckcraft_screen::CaptureStatus;

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
pub(super) fn connect(rec: &mut RecordView, services: &crate::Services, show: &crate::show::Show, screen: (deckcraft_screen::CaptureStatus, bool)) {
    // The shell's own `rec` is out of `show` while this runs, so the take state comes from `rec`
    // and the microphone from what the show actually started.
    // A host that is installed but whose start failed (no device, refused) is unavailable, not
    // "off": the take is running without narration.
    let attempted = rec.status == RecordStatus::Recording && show.mic_started();
    rec.devices.mic.status = match (services.mic.is_some(), attempted, show.mic_live()) {
        (false, _, _) => DeviceStatus::Unavailable,
        (true, true, false) => DeviceStatus::Unavailable,
        (true, _, true) => DeviceStatus::On,
        (true, false, false) => DeviceStatus::Off,
    };
    // The capture state lives in the session (the host moved there at start-up): unsupported or a
    // denied permission is unavailable, not "off".
    let (capture, recording) = screen;
    rec.devices.screen.status = match (capture, recording) {
        (CaptureStatus::Ready, true) => DeviceStatus::On,
        (CaptureStatus::Ready, false) => DeviceStatus::Off,
        (CaptureStatus::Unsupported | CaptureStatus::PermissionDenied, _) => DeviceStatus::Unavailable,
    };
}

pub(super) fn camera_button(ui: &mut Ui, rec: &mut RecordView, _now: f64) -> bool {
    toggle(ui, &mut rec.devices.camera, "Camera", "Camera")
}

/// The microphone is driven by the take, not by this label: it reports whether narration is being
/// captured, and never pretends a click changed it.
pub(super) fn mic_button(ui: &mut Ui, rec: &mut RecordView, _now: f64) -> bool {
    status(ui, &rec.devices.mic, "Mic", "Microphone")
}

/// Screen capture is driven by the Record tab's picker: this label reports whether the host has a
/// capture device and whether the take is using it.
pub(super) fn screen_button(ui: &mut Ui, rec: &mut RecordView, _now: f64) -> bool {
    status(ui, &rec.devices.screen, "Screen", "Screen recording")
}

/// One status-only device label. Returns false: it never reports a state change.
fn status(ui: &mut Ui, state: &DeviceState, short: &str, name: &str) -> bool {
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
    let resp = ui.add(egui::Label::new(RichText::new(short).size(12.0).color(color)).sense(egui::Sense::hover()));
    // A label is never disabled, and it must say the state it shows: "Microphone on" / "— unavailable".
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &label));
    resp.on_hover_text(detail);
    false
}

/// One status-capable toggle. Returns whether the requested state changed; an unavailable,
/// denied or busy device never flips to a fake "on".
///
/// The visible text is the short name: the bar also carries the clock and the transport controls,
/// and a status sentence here would push them over the clock on a narrow window. The colour, the
/// tooltip and the accessible label carry the state.
fn toggle(ui: &mut Ui, state: &mut DeviceState, short: &str, name: &str) -> bool {
    let label = match state.status {
        DeviceStatus::On => format!("{name} on"),
        DeviceStatus::Off => format!("{name} off"),
        DeviceStatus::Unavailable => format!("{name} — unavailable"),
    };
    let color = if state.status == DeviceStatus::On { Color32::from_rgb(120, 200, 140) } else { Color32::from_gray(200) };
    let resp = ui.add_enabled(state.can_toggle(), egui::Button::new(RichText::new(short).size(12.0).color(color)).frame(false));
    // `enabled` says whether it can be clicked; the on/off state goes in `selected`.
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, state.can_toggle(), state.is_on(), &label));
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
