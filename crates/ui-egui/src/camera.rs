//! Live camera preview state behind the Camera Format tab.
//!
//! The ribbon owns no device. It asks [`Preview`] to start or stop; [`Preview`] drives the
//! [`CameraSource`] the host injected through [`crate::Services::camera`], and the state the source
//! reports is published to the session so `cameo.preview.state` (and the control channel) can read
//! it. Capture itself lives outside this crate; [`deckcraft_media::camera::NoCamera`] is the
//! fallback until a host supplies a real source.

use deckcraft_media::camera::{CameraDevice, CameraSource, CameraStatus};

use crate::SlideApp;

/// The preview's state machine over one camera source.
pub struct Preview {
    source: Box<dyn CameraSource>,
    status: CameraStatus,
    /// The camera the user picked, kept across off/on so the menu can keep ticking it.
    picked: Option<String>,
    /// The camera currently open; `None` while the preview is off or failed.
    device: Option<String>,
    devices: Vec<CameraDevice>,
    /// Enumerated once, then again whenever the menu asks.
    enumerated: bool,
}

impl Preview {
    /// Wrap a source. A host without capture passes [`deckcraft_media::camera::NoCamera`].
    pub fn new(source: Box<dyn CameraSource>) -> Self {
        Preview { source, status: CameraStatus::Off, picked: None, device: None, devices: Vec::new(), enumerated: false }
    }

    pub fn status(&self) -> CameraStatus {
        self.status
    }

    /// The camera currently open.
    pub fn device(&self) -> Option<&str> {
        self.device.as_deref()
    }

    /// The camera the user picked, open or not.
    pub fn picked(&self) -> Option<&str> {
        self.picked.as_deref()
    }

    /// A name for the open (or picked) camera, for status text and tooltips.
    pub fn device_name(&self) -> Option<&str> {
        let id = self.device.as_deref().or(self.picked.as_deref())?;
        Some(self.devices.iter().find(|d| d.id == id).map_or(id, |d| d.name.as_str()))
    }

    /// Is the preview on (opening or live)? The ribbon button reads as pressed.
    pub fn is_on(&self) -> bool {
        self.status.is_on()
    }

    /// The cameras the source listed at the last enumeration.
    pub fn devices(&self) -> &[CameraDevice] {
        &self.devices
    }

    /// Ask the source for its cameras again (the More Options menu opens on a fresh list).
    pub fn refresh_devices(&mut self) -> &[CameraDevice] {
        self.devices = self.source.devices();
        self.enumerated = true;
        &self.devices
    }

    /// Enumerate the devices the first time they are needed, and never block a menu on later frames.
    pub fn ensure_devices(&mut self) {
        if !self.enumerated {
            self.refresh_devices();
        }
    }

    /// Toggle (`None`), start (`Some(true)`) or stop (`Some(false)`) the preview. `device` names
    /// the camera to open; without one the previously picked camera (or the source's default) is
    /// used.
    pub fn request(&mut self, on: Option<bool>, device: Option<String>) -> CameraStatus {
        match on {
            Some(false) => self.stop(),
            Some(true) => self.start(device.as_deref()),
            None => {
                if self.status.is_on() {
                    self.stop()
                } else {
                    self.start(device.as_deref())
                }
            }
        }
    }

    /// Ask the source to open a camera. The source reports `Requesting` while it opens; callers
    /// poll for `Live` or a failure.
    pub fn start(&mut self, device: Option<&str>) -> CameraStatus {
        if self.status.is_on() {
            self.source.stop();
        }
        if let Some(id) = device {
            self.picked = Some(id.to_string());
        }
        self.status = self.source.start(self.picked.as_deref());
        self.device =
            if self.status.is_on() { self.picked.clone().or_else(|| self.devices.iter().find(|d| d.default).map(|d| d.id.clone())) } else { None };
        self.status
    }

    /// Release the camera. The picked camera stays selected for the next start.
    pub fn stop(&mut self) -> CameraStatus {
        self.source.stop();
        self.status = CameraStatus::Off;
        self.device = None;
        CameraStatus::Off
    }

    /// Take the source's latest status while a preview is on. Returns the new status when it
    /// changed (the UI repaints and announces failures), `None` otherwise.
    pub fn poll(&mut self) -> Option<CameraStatus> {
        if !self.status.is_on() {
            return None;
        }
        let next = self.source.poll();
        if next == self.status {
            return None;
        }
        self.status = next;
        if !next.is_on() {
            self.device = None;
            if next.is_error() {
                // The device is gone or refused: release whatever the source still holds.
                self.source.stop();
            }
        }
        Some(next)
    }
}

impl SlideApp {
    /// Act on `cameo.preview`: start, stop or toggle the source the host injected.
    pub(crate) fn apply_camera_request(&mut self, on: Option<bool>, device: Option<String>) {
        self.camera_preview.ensure_devices();
        let status = self.camera_preview.request(on, device);
        self.publish_camera();
        if status.is_error() {
            self.set_status(camera_message(&self.camera_preview, status));
        }
    }

    /// Poll the source once per frame and report a status change or failure.
    pub(crate) fn poll_camera(&mut self, ctx: &egui::Context) {
        let Some(status) = self.camera_preview.poll() else {
            // A live preview can wait frames for the device: keep the frame clock running so a
            // later Lost or Ready is seen even when nothing else asks for a repaint.
            if self.camera_preview.is_on() {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            return;
        };
        self.publish_camera();
        if status.is_error() {
            self.set_status(camera_message(&self.camera_preview, status));
        }
        ctx.request_repaint();
    }

    /// Make the session's `cameo.preview.state` agree with the source.
    fn publish_camera(&mut self) {
        self.session.camera_status = self.camera_preview.status();
        self.session.camera_device = self.camera_preview.device().map(str::to_string);
    }
}

/// The status sentence, naming the camera when one is known.
fn camera_message(preview: &Preview, status: CameraStatus) -> String {
    match preview.device_name() {
        Some(name) if !name.is_empty() => format!("{} ({name})", status.message()),
        _ => status.message().to_string(),
    }
}
