//! Camera capture: the platform-neutral interface the UI host drives for the Cameo live preview.
//!
//! Nothing here names a platform API or an OS crate, so this module builds for `wasm32` and keeps
//! `deckcraft-media` free of device dependencies. The host injects an implementation through
//! [`CameraIn`] — `SystemCamera` on the desktop, `None` on the web and in tests.
//!
//! The contract is one worker per session:
//!
//! - [`CameraIn::start`] returns immediately with a bounded event channel; enumerating devices,
//!   asking the OS for permission and opening the stream all happen on the worker.
//! - [`CameraEvent::Opened`] and then [`CameraEvent::Frame`]s arrive as the camera delivers them.
//! - A failure is terminal: [`CameraEvent::Failed`] then no more events. The host keeps running
//!   (the slide simply keeps its placeholder) and offers to start again.
//! - [`CameraIn::stop`] only signals the worker; it never waits for a device call to return, so
//!   the UI thread cannot block on a permission sheet or a stalled driver.
//!
//! Frames are delivered as tightly packed RGBA8 so the UI can upload them as a texture without a
//! second conversion. The preview channel is deliberately latest-wins: if the UI falls behind,
//! stale frames are dropped rather than queued, keeping live latency bounded.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use web_time::Instant;

/// A platform-assigned camera identifier (macOS `uniqueID`, a Windows symbolic link, `/dev/video*`
/// on Linux).
pub type CameraId = String;

/// A camera the platform can see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraDesc {
    pub id: CameraId,
    pub name: String,
    pub position: CameraPosition,
}

impl CameraDesc {
    /// The name to show in a menu. Cameras without a reported name still get a stable label.
    pub fn label(&self) -> &str {
        if self.name.trim().is_empty() { &self.id } else { &self.name }
    }
}

impl Default for CameraDesc {
    fn default() -> Self {
        CameraDesc { id: String::new(), name: String::new(), position: CameraPosition::Unknown }
    }
}

/// Where the camera physically sits, when the platform reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraPosition {
    #[default]
    Unknown,
    Front,
    Back,
    External,
}

/// A pixel layout a camera can deliver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Bgra8,
    Rgba8,
    Rgb8,
    Yuyv,
    Nv12,
    Mjpeg,
}

/// One format the Camera Format tab can offer. Mirrors the platform descriptor without naming it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraFormat {
    pub width: u32,
    pub height: u32,
    /// Highest frame rate of the device's reported range.
    pub fps: f32,
    pub pixel_format: PixelFormat,
}

impl Default for CameraFormat {
    fn default() -> Self {
        // The preview default: 720p at 30 fps. Backends fall back to the closest format they have.
        CameraFormat { width: 1280, height: 720, fps: 30.0, pixel_format: PixelFormat::Bgra8 }
    }
}

/// What to open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraRequest {
    /// `None` asks for the platform's first/default camera.
    pub device: Option<CameraId>,
    /// `None` asks for [`CameraFormat::default`] and lets the backend pick the closest format.
    pub format: Option<CameraFormat>,
}

impl CameraRequest {
    /// The requested format, or the preview default.
    pub fn format_or_default(&self) -> CameraFormat {
        self.format.unwrap_or_default()
    }
}

/// The stream actually opened (the first delivered frame has the final say on its size).
#[derive(Clone, Debug, PartialEq)]
pub struct CameraInfo {
    pub device: CameraDesc,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub source_format: PixelFormat,
}

impl Default for CameraInfo {
    fn default() -> Self {
        CameraInfo { device: CameraDesc::default(), width: 0, height: 0, fps: 0.0, source_format: PixelFormat::Bgra8 }
    }
}

/// One captured frame, converted for the screen.
#[derive(Clone, Debug)]
pub struct CameraFrame {
    /// Monotonic counter from the capturing worker; a new value means new pixels.
    pub seq: u64,
    /// Monotonic receipt time on the same clock as the show and audio playback.
    pub at: Instant,
    /// The backend's presentation time, when it gave one. The origin is platform-defined (Windows
    /// stream ticks, a macOS `CMTime`, Linux wall-clock delivery), so this is for diagnostics and
    /// offset calibration only — use [`CameraFrame::at`] for synchronisation.
    pub source_t: Option<Duration>,
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8, `width * height * 4` bytes. Shared so a preview and a future recorder
    /// can read the same frame without copying it.
    pub rgba: Arc<[u8]>,
}

/// Everything a capture session reports.
#[derive(Clone, Debug)]
pub enum CameraEvent {
    /// The kernel stream is open. Sent once, before the first frame.
    Opened(CameraInfo),
    Frame(CameraFrame),
    /// The session ended and will send nothing else. Retry only after an explicit user action.
    Failed(CameraError),
    /// The worker stopped cleanly after `stop`.
    Closed,
}

/// Why a camera session could not start or continue.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CameraError {
    #[error("no camera was found")]
    NoDevice,
    #[error("camera access is denied")]
    PermissionDenied,
    #[error("the camera is in use by another application")]
    InUse,
    #[error("the camera does not support the requested format")]
    UnsupportedFormat,
    #[error("the camera was disconnected")]
    Lost,
    #[error("camera capture is not supported on this platform")]
    Unsupported,
    #[error("camera backend error: {0}")]
    Backend(String),
}

/// The preview's state, which the UI shows beside the Cameo controls.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum CameraStatus {
    /// No session: the preview is off, or the host has no camera backend.
    #[default]
    Off,
    /// A session was requested; the worker may still be enumerating or waiting for permission.
    Starting,
    /// Streaming.
    Live(CameraInfo),
    /// The session ended with an error and will not recover on its own.
    Failed(CameraError),
}

impl CameraStatus {
    pub fn is_live(&self) -> bool {
        matches!(self, CameraStatus::Live(_))
    }

    pub fn info(&self) -> Option<&CameraInfo> {
        match self {
            CameraStatus::Live(info) => Some(info),
            _ => None,
        }
    }

    pub fn error(&self) -> Option<&CameraError> {
        match self {
            CameraStatus::Failed(e) => Some(e),
            _ => None,
        }
    }

    /// The state as one of the eight names the Camera Format tab and the control channel use.
    pub fn state(&self) -> CameraState {
        match self {
            CameraStatus::Off => CameraState::Off,
            CameraStatus::Starting => CameraState::Requesting,
            CameraStatus::Live(_) => CameraState::Live,
            CameraStatus::Failed(error) => match error {
                CameraError::PermissionDenied => CameraState::Denied,
                CameraError::NoDevice => CameraState::NoDevice,
                CameraError::InUse => CameraState::Busy,
                CameraError::Lost => CameraState::Lost,
                CameraError::UnsupportedFormat | CameraError::Unsupported | CameraError::Backend(_) => CameraState::Unsupported,
            },
        }
    }
}

/// The preview's state, named as PowerPoint's Camera status reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraState {
    Off,
    Requesting,
    Live,
    Denied,
    NoDevice,
    Busy,
    Lost,
    Unsupported,
}

impl CameraState {
    /// The stable lower-camel name used in the control channel and the engine status.
    pub fn as_str(&self) -> &'static str {
        match self {
            CameraState::Off => "off",
            CameraState::Requesting => "requesting",
            CameraState::Live => "live",
            CameraState::Denied => "denied",
            CameraState::NoDevice => "noDevice",
            CameraState::Busy => "busy",
            CameraState::Lost => "lost",
            CameraState::Unsupported => "unsupported",
        }
    }
}

/// A running capture, handed back by [`CameraIn::start`].
pub struct CameraSession {
    /// Monotonic start, for mapping [`CameraFrame::at`] onto the show/recording clock.
    pub started_at: Instant,
    /// Bounded event channel owned by the worker. Drain with `try_recv`; never block the UI.
    pub events: Receiver<CameraEvent>,
}

/// A camera backend the host injects. Implementations must be safe to create without a device and
/// must do all device work on their own worker.
pub trait CameraIn: Send {
    /// Start on a worker and return immediately. `Opened`, `Frame`, or `Failed` arrives on
    /// `session.events`; this call must not wait for the OS permission sheet or device probes.
    fn start(&mut self, request: &CameraRequest) -> Result<CameraSession, CameraError>;

    /// Cooperative stop: signal the worker and return. The worker drops the OS camera when its
    /// current call returns. Never join the worker on the UI thread.
    fn stop(&mut self);

    /// Enumerate cameras on a worker. The default reports [`CameraError::Unsupported`].
    fn devices(&mut self) -> Receiver<Result<Vec<CameraDesc>, CameraError>> {
        unsupported_receiver()
    }

    /// Probe a camera's formats on a worker, for the Camera Format tab. The default reports
    /// [`CameraError::Unsupported`].
    fn formats(&mut self, _device: Option<&CameraId>) -> Receiver<Result<Vec<CameraFormat>, CameraError>> {
        unsupported_receiver()
    }
}

/// A receiver that immediately reports `Unsupported`, for optional trait methods.
fn unsupported_receiver<T: Send + 'static>() -> Receiver<Result<T, CameraError>> {
    let (tx, rx) = mpsc::channel();
    let _ = tx.send(Err(CameraError::Unsupported));
    rx
}

/// A scripted camera for tests and headless builds: no device, no OS calls.
#[cfg(feature = "test-util")]
pub mod testing {
    use std::sync::mpsc::{self, SyncSender};
    use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

    use web_time::Instant;

    use super::{CameraDesc, CameraError, CameraEvent, CameraFrame, CameraIn, CameraInfo, CameraPosition, CameraRequest, CameraSession, PixelFormat};

    struct State {
        starts: usize,
        stops: usize,
        sender: Option<SyncSender<CameraEvent>>,
        /// Events delivered as soon as a session starts.
        script: Vec<CameraEvent>,
        /// Camera frames delivered as soon as a session starts.
        frames_on_start: u32,
        width: u32,
        height: u32,
        seq: u64,
        fail_start: Option<CameraError>,
        device: CameraDesc,
    }

    impl Default for State {
        fn default() -> Self {
            State {
                starts: 0,
                stops: 0,
                sender: None,
                script: Vec::new(),
                frames_on_start: 1,
                width: 320,
                height: 240,
                seq: 0,
                fail_start: None,
                device: CameraDesc { id: "fake-camera".into(), name: "Fake camera".into(), position: CameraPosition::Unknown },
            }
        }
    }

    /// A [`CameraIn`] whose frames and events the test writes by hand. Clone shares the same
    /// script, so the test can push frames while the host owns its own boxed handle.
    #[derive(Clone, Default)]
    pub struct FakeCamera {
        state: Arc<Mutex<State>>,
    }

    impl FakeCamera {
        pub fn new() -> FakeCamera {
            FakeCamera::default()
        }

        fn lock(&self) -> MutexGuard<'_, State> {
            self.state.lock().unwrap_or_else(PoisonError::into_inner)
        }

        /// Set the frame size the fake delivers.
        pub fn sized(self, width: u32, height: u32) -> FakeCamera {
            {
                let mut st = self.lock();
                st.width = width.max(1);
                st.height = height.max(1);
            }
            self
        }

        /// How many frames a newly started session delivers immediately (default one).
        pub fn frames_on_start(self, frames: u32) -> FakeCamera {
            self.lock().frames_on_start = frames;
            self
        }

        /// Make the next [`CameraIn::start`] fail with `error` (one shot: a retry tries again).
        pub fn fail_start(self, error: CameraError) -> FakeCamera {
            self.lock().fail_start = Some(error);
            self
        }

        /// Queue an event for the next session start.
        pub fn queue(&self, event: CameraEvent) {
            self.lock().script.push(event);
        }

        /// Deliver an event to the running session, if any. A full channel drops it, exactly as
        /// the real preview does.
        pub fn emit(&self, event: CameraEvent) {
            let sender = self.lock().sender.clone();
            if let Some(tx) = sender {
                let _ = tx.try_send(event);
            }
        }

        /// Deliver one RGBA frame to the running session. Returns it so a test can compare.
        pub fn push_frame(&self) -> CameraFrame {
            let (frame, sender) = {
                let mut st = self.lock();
                let seq = st.seq;
                st.seq = st.seq.wrapping_add(1);
                (make_frame(seq, st.width, st.height), st.sender.clone())
            };
            if let Some(tx) = sender {
                let _ = tx.try_send(CameraEvent::Frame(frame.clone()));
            }
            frame
        }

        /// Report a terminal failure and end the session.
        pub fn fail(&self, error: CameraError) {
            let mut st = self.lock();
            if let Some(tx) = st.sender.take() {
                let _ = tx.try_send(CameraEvent::Failed(error));
            }
        }

        /// End the session cleanly.
        pub fn finish(&self) {
            let mut st = self.lock();
            if let Some(tx) = st.sender.take() {
                let _ = tx.try_send(CameraEvent::Closed);
            }
        }

        /// Sessions started so far.
        pub fn starts(&self) -> usize {
            self.lock().starts
        }

        /// Stops requested so far.
        pub fn stops(&self) -> usize {
            self.lock().stops
        }

        /// Whether a session is currently open.
        pub fn running(&self) -> bool {
            self.lock().sender.is_some()
        }

        /// The device reported in `Opened`.
        pub fn info(&self) -> CameraInfo {
            let st = self.lock();
            info_for(&st.device, st.width, st.height)
        }
    }

    fn info_for(device: &CameraDesc, width: u32, height: u32) -> CameraInfo {
        CameraInfo { device: device.clone(), width, height, fps: 30.0, source_format: PixelFormat::Bgra8 }
    }

    /// One deterministic frame: the red channel varies with `seq`, so a test can tell frames apart
    /// without decoding anything.
    fn make_frame(seq: u64, width: u32, height: u32) -> CameraFrame {
        let width = width.max(1);
        let height = height.max(1);
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..width as usize * height as usize {
            rgba.extend_from_slice(&[(seq % 251) as u8, (seq / 251 % 251) as u8, 0x40, 0xFF]);
        }
        CameraFrame { seq, at: Instant::now(), source_t: None, width, height, rgba: Arc::from(rgba.into_boxed_slice()) }
    }

    impl CameraIn for FakeCamera {
        fn start(&mut self, request: &CameraRequest) -> Result<CameraSession, CameraError> {
            let (width, height, device, frames, script, mut seq) = {
                let mut st = self.lock();
                st.starts = st.starts.saturating_add(1);
                if let Some(error) = st.fail_start.take() {
                    return Err(error);
                }
                if let Some(f) = request.format {
                    st.width = f.width.max(1);
                    st.height = f.height.max(1);
                }
                (st.width, st.height, st.device.clone(), st.frames_on_start, std::mem::take(&mut st.script), st.seq)
            };
            // The initial burst is `Opened` plus the queued script and frames: size the channel
            // for all of it, so a test's configured start is never silently short, keeping the
            // usual capacity of two as the floor. Later `push_frame`/`emit` calls stay bounded
            // and latest-wins, as before.
            let (tx, rx) = mpsc::sync_channel((1 + script.len() + frames as usize).max(2));
            self.lock().sender = Some(tx.clone());
            let _ = tx.try_send(CameraEvent::Opened(info_for(&device, width, height)));
            for event in script {
                let _ = tx.try_send(event);
            }
            for _ in 0..frames {
                let _ = tx.try_send(CameraEvent::Frame(make_frame(seq, width, height)));
                seq = seq.wrapping_add(1);
            }
            self.lock().seq = seq;
            Ok(CameraSession { started_at: Instant::now(), events: rx })
        }

        fn stop(&mut self) {
            let mut st = self.lock();
            st.stops = st.stops.saturating_add(1);
            st.sender = None;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// A backend that only implements the required methods.
    struct Minimal;

    impl CameraIn for Minimal {
        fn start(&mut self, _request: &CameraRequest) -> Result<CameraSession, CameraError> {
            Err(CameraError::Unsupported)
        }

        fn stop(&mut self) {}
    }

    #[test]
    fn optional_probes_report_unsupported_instead_of_panicking() {
        let mut camera = Minimal;
        let devices = camera.devices().recv().expect("a result");
        assert_eq!(devices, Err(CameraError::Unsupported));
        let formats = camera.formats(None).recv().expect("a result");
        assert_eq!(formats, Err(CameraError::Unsupported));
    }

    #[test]
    fn the_default_request_is_720p_at_30() {
        let request = CameraRequest::default();
        assert_eq!(request.device, None);
        let format = request.format_or_default();
        assert_eq!((format.width, format.height), (1280, 720));
        assert_eq!(format.fps, 30.0);
        assert_eq!(format.pixel_format, PixelFormat::Bgra8);
    }

    #[test]
    fn status_helpers_see_live_and_failed() {
        assert!(!CameraStatus::Off.is_live());
        assert!(CameraStatus::Starting.info().is_none());
        let info = CameraInfo::default();
        assert_eq!(CameraStatus::Live(info.clone()).info(), Some(&info));
        assert_eq!(CameraStatus::Failed(CameraError::Lost).error(), Some(&CameraError::Lost));
    }
}
