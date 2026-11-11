//! Camera devices behind the Camera Format tab's Preview control.
//!
//! The preview itself is a UI concern: the ribbon asks a source to open a device and then reports
//! what the source says. Platform capture (Media Foundation, AVFoundation, V4L2 or the browser's
//! `MediaDevices`) arrives as an implementation of [`CameraSource`]; this module fixes the seam so
//! the tab, its tests and every backend agree on device enumeration, the lifecycle, and the way a
//! failure is named. No backend lives here, and [`NoCamera`] is what the app uses until one does.

/// What the Preview control last saw.
///
/// The states are the ones a live preview can be in, including the failures PowerPoint reports:
/// a refused permission ([`CameraStatus::Denied`]), no device at all
/// ([`CameraStatus::NoDevice`]), a device another application holds ([`CameraStatus::Busy`]), a
/// device unplugged while it streamed ([`CameraStatus::Lost`]) and a build without capture
/// ([`CameraStatus::Unsupported`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraStatus {
    /// The user has not asked for a preview, or asked for it to stop.
    #[default]
    Off,
    /// The source is opening the device. The first frame has not arrived.
    Requesting,
    /// Frames are arriving (or the source is ready to deliver them).
    Live,
    /// The system refused camera access for this application.
    Denied,
    /// No camera is attached.
    NoDevice,
    /// Another application holds the camera.
    Busy,
    /// The device that was streaming went away.
    Lost,
    /// Capture is not built for this target (for example a web build without camera support).
    Unsupported,
}

impl CameraStatus {
    /// Stable lowercase name, used by `cameo.preview.state` and the control channel.
    pub const fn as_str(self) -> &'static str {
        match self {
            CameraStatus::Off => "off",
            CameraStatus::Requesting => "requesting",
            CameraStatus::Live => "live",
            CameraStatus::Denied => "denied",
            CameraStatus::NoDevice => "noDevice",
            CameraStatus::Busy => "busy",
            CameraStatus::Lost => "lost",
            CameraStatus::Unsupported => "unsupported",
        }
    }

    /// A sentence for the status bar or a tooltip.
    pub const fn message(self) -> &'static str {
        match self {
            CameraStatus::Off => "Camera preview off",
            CameraStatus::Requesting => "Starting the camera…",
            CameraStatus::Live => "Camera preview live",
            CameraStatus::Denied => "Camera access is blocked — allow DeckCraft to use the camera in system settings",
            CameraStatus::NoDevice => "No camera found",
            CameraStatus::Busy => "The camera is in use by another application",
            CameraStatus::Lost => "The camera was disconnected",
            CameraStatus::Unsupported => "This build has no camera preview",
        }
    }

    /// The user asked for a preview and it has not failed: the button reads as pressed.
    pub const fn is_on(self) -> bool {
        matches!(self, CameraStatus::Requesting | CameraStatus::Live)
    }

    /// A state that ended a preview the user asked for.
    pub const fn is_error(self) -> bool {
        matches!(self, CameraStatus::Denied | CameraStatus::NoDevice | CameraStatus::Busy | CameraStatus::Lost | CameraStatus::Unsupported)
    }
}

/// One camera the source can open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CameraDevice {
    /// Opaque, source-specific identity passed back to [`CameraSource::start`].
    pub id: String,
    /// What the device list shows.
    pub name: String,
    /// The source would pick this device when `start` is called without one.
    pub default: bool,
}

/// A camera backend the UI can drive without blocking a frame.
///
/// `start` must return before the device opens: it reports [`CameraStatus::Requesting`] and the
/// caller polls [`CameraSource::poll`] for the outcome. A backend runs its capture on its own
/// worker thread and only publishes status and frames here.
pub trait CameraSource: Send {
    /// The cameras attached right now, in preference order.
    fn devices(&self) -> Vec<CameraDevice>;
    /// Ask for a live feed from `device` (or the source's default when `None`). Never blocks on
    /// the device: the result of opening it arrives through [`CameraSource::poll`].
    fn start(&mut self, device: Option<&str>) -> CameraStatus;
    /// Release the device and stop delivering frames.
    fn stop(&mut self);
    /// The current status after [`CameraSource::start`] or a device event.
    fn poll(&mut self) -> CameraStatus;
}

/// The source a build without camera capture uses: it reports no devices and never opens one.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoCamera;

impl CameraSource for NoCamera {
    fn devices(&self) -> Vec<CameraDevice> {
        Vec::new()
    }

    fn start(&mut self, _device: Option<&str>) -> CameraStatus {
        CameraStatus::Unsupported
    }

    fn stop(&mut self) {}

    fn poll(&mut self) -> CameraStatus {
        CameraStatus::Unsupported
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_has_a_stable_name_and_a_sentence() {
        let all = [
            CameraStatus::Off,
            CameraStatus::Requesting,
            CameraStatus::Live,
            CameraStatus::Denied,
            CameraStatus::NoDevice,
            CameraStatus::Busy,
            CameraStatus::Lost,
            CameraStatus::Unsupported,
        ];
        let names: Vec<&str> = all.iter().map(|s| s.as_str()).collect();
        assert_eq!(names, ["off", "requesting", "live", "denied", "noDevice", "busy", "lost", "unsupported"]);
        for st in all {
            assert!(!st.message().is_empty(), "{st:?} has no message");
        }
        assert_eq!(CameraStatus::default(), CameraStatus::Off);
    }

    #[test]
    fn only_requesting_and_live_read_as_on_and_failures_are_errors() {
        assert!(CameraStatus::Requesting.is_on());
        assert!(CameraStatus::Live.is_on());
        for st in [CameraStatus::Off, CameraStatus::Denied, CameraStatus::NoDevice, CameraStatus::Busy, CameraStatus::Lost, CameraStatus::Unsupported]
        {
            assert!(!st.is_on(), "{st:?}");
        }
        for st in [CameraStatus::Denied, CameraStatus::NoDevice, CameraStatus::Busy, CameraStatus::Lost, CameraStatus::Unsupported] {
            assert!(st.is_error(), "{st:?}");
        }
        assert!(!CameraStatus::Off.is_error());
        assert!(!CameraStatus::Live.is_error());
    }

    #[test]
    fn no_camera_reports_no_devices_and_no_preview() {
        let mut source = NoCamera;
        assert!(source.devices().is_empty());
        assert_eq!(source.start(None), CameraStatus::Unsupported);
        assert_eq!(source.poll(), CameraStatus::Unsupported);
        source.stop();
    }
}
