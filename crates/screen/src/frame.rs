//! The neutral capture types: a frame, a target, a region, and the host trait.

use std::fmt;

/// A region to record, in captured-frame pixels. Coordinates start at the frame's top-left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// The smallest region a recording may select (a whole screen is usually much larger).
pub const MIN_REGION: u32 = 64;

impl CaptureRegion {
    /// A region at (`x`, `y`) of `width` × `height`. Negative origins and sides under
    /// [`MIN_REGION`] are refused, so a bad picker selection can never reach the capture host.
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Result<Self, ScreenError> {
        if x < 0 || y < 0 || width < MIN_REGION || height < MIN_REGION {
            return Err(ScreenError::Failed(format!("a recording region must be at least {MIN_REGION}×{MIN_REGION} pixels wide inside the screen")));
        }
        Ok(CaptureRegion { x, y, width, height })
    }

    /// The region as the command protocol's `{x, y, width, height}` object (the engine builds the
    /// JSON; this crate has no serde dependency).
    pub fn to_object(self) -> (i64, i64, u32, u32) {
        (i64::from(self.x), i64::from(self.y), self.width, self.height)
    }
}

/// What kind of surface a capture target is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Display,
    Window,
}

impl TargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TargetKind::Display => "display",
            TargetKind::Window => "window",
        }
    }
}

/// One capturable display or window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureTarget {
    /// Stable for the lifetime of the host; what [`CaptureOptions::target`] names.
    pub id: u64,
    pub kind: TargetKind,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

/// What the host can say about capture right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureStatus {
    /// Capture can start.
    Ready,
    /// The platform or this build has no screen capture.
    Unsupported,
    /// The system refuses capture until the user grants permission (macOS, Wayland portals).
    PermissionDenied,
}

/// Why capture failed. Every variant has one user-facing sentence; the UI shows it as it is.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ScreenError {
    #[error("screen recording isn't available on this platform")]
    Unavailable,
    #[error("screen recording needs permission: allow DeckCraft to record the screen, then try again")]
    PermissionDenied,
    #[error("a screen recording is already in progress")]
    Busy,
    #[error("no screen recording is in progress")]
    NotRecording,
    #[error("there is no screen to record")]
    NoDevice,
    #[error("{0}")]
    Failed(String),
}

impl ScreenError {
    /// The short status code used in command results and tests.
    pub fn code(&self) -> &'static str {
        match self {
            ScreenError::Unavailable => "unavailable",
            ScreenError::PermissionDenied => "permissionDenied",
            ScreenError::Busy => "busy",
            ScreenError::NotRecording => "notRecording",
            ScreenError::NoDevice => "noDevice",
            ScreenError::Failed(_) => "failed",
        }
    }
}

/// An uncompressed captured picture: BGRA, `stride` bytes per row (the backend's row padding is
/// kept; [`ScreenRecorder`](crate::ScreenRecorder) strips it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenFrame {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub data: Vec<u8>,
}

impl ScreenFrame {
    /// A frame of `width` × `height` BGRA pixels with `stride` bytes per row. `None` when the
    /// dimensions or the buffer length can't describe a picture (hostile backend data).
    pub fn new(width: u32, height: u32, stride: usize, data: Vec<u8>) -> Option<Self> {
        let need = stride.checked_mul(height as usize)?;
        if width == 0 || height == 0 || stride < (width as usize).checked_mul(4)? || data.len() < need {
            return None;
        }
        Some(ScreenFrame { width, height, stride, data })
    }

    /// A solid opaque picture, for tests and the no-device placeholder.
    pub fn solid(width: u32, height: u32, bgra: [u8; 4]) -> Self {
        let stride = width as usize * 4;
        let mut data = vec![0; stride * height as usize];
        for px in data.as_chunks_mut::<4>().0 {
            *px = bgra;
        }
        ScreenFrame { width, height, stride, data }
    }
}

/// Options for one recording, as the picker chose them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureOptions {
    /// A target from [`ScreenIn::targets`]; `None` is the primary display.
    pub target: Option<u64>,
    /// What kind of surface `target` is. Ids are only unique within a kind (a display and a window
    /// can share one), so the host matches this when it is set.
    pub kind: Option<TargetKind>,
    /// The part of the target to record; `None` records all of it.
    pub region: Option<CaptureRegion>,
    pub fps: u32,
    /// Draw the pointer in the captured frames.
    pub show_cursor: bool,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        CaptureOptions { target: None, kind: None, region: None, fps: 30, show_cursor: false }
    }
}

/// The host's screen-capture device. Implementations live in the app (`scap` on Windows, macOS
/// and Linux) and never in the engine; the engine only sees frames.
///
/// The methods run on the UI thread. [`frame`](ScreenIn::frame) must not block: the host pushes
/// captured frames from its own thread and returns the latest one, or `None` when none has
/// arrived since the last call.
pub trait ScreenIn: Send {
    /// Whether capture is possible, and whether permission is missing.
    fn status(&mut self) -> CaptureStatus;

    /// The displays and windows this host can capture.
    fn targets(&mut self) -> Result<Vec<CaptureTarget>, ScreenError>;

    /// Start delivering frames for `options`. Replacing a running capture is allowed.
    fn start(&mut self, options: &CaptureOptions) -> Result<(), ScreenError>;

    /// The next captured frame since the last call, if one arrived.
    fn frame(&mut self) -> Result<Option<ScreenFrame>, ScreenError>;

    /// Stop capturing and release the device. Already-delivered frames stay readable.
    fn stop(&mut self);

    /// Ask the system for permission (macOS shows its prompt). `true` when it is now granted.
    fn request_permission(&mut self) -> bool {
        false
    }
}

/// A host with no screen capture: the FreeBSD and wasm builds, and the headless test host.
#[derive(Debug, Default)]
pub struct NoScreen;

impl ScreenIn for NoScreen {
    fn status(&mut self) -> CaptureStatus {
        CaptureStatus::Unsupported
    }
    fn targets(&mut self) -> Result<Vec<CaptureTarget>, ScreenError> {
        Err(ScreenError::Unavailable)
    }
    fn start(&mut self, _options: &CaptureOptions) -> Result<(), ScreenError> {
        Err(ScreenError::Unavailable)
    }
    fn frame(&mut self) -> Result<Option<ScreenFrame>, ScreenError> {
        Ok(None)
    }
    fn stop(&mut self) {}
}

impl fmt::Display for CaptureRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}×{} at {},{}", self.width, self.height, self.x, self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_refuses_negative_origins_and_sides_under_the_minimum() {
        assert_eq!(CaptureRegion::new(0, 0, 64, 64), Ok(CaptureRegion { x: 0, y: 0, width: 64, height: 64 }));
        assert_eq!(
            CaptureRegion::new(-1, 0, 64, 64),
            Err(ScreenError::Failed("a recording region must be at least 64×64 pixels wide inside the screen".into()))
        );
        assert!(CaptureRegion::new(0, -1, 64, 64).is_err());
        assert!(CaptureRegion::new(0, 0, 63, 64).is_err());
        assert!(CaptureRegion::new(0, 0, 64, 63).is_err());
        assert_eq!(CaptureRegion::new(0, 0, 64, 64).unwrap().to_object(), (0, 0, 64, 64));
    }

    #[test]
    fn a_frame_refuses_zero_sides_a_tight_stride_and_a_short_buffer() {
        // A valid 4×4 picture: 16 bytes a row, 4 rows.
        assert!(ScreenFrame::new(4, 4, 16, vec![0; 64]).is_some());
        // A padded stride is valid and kept (the recorder strips it).
        let padded = ScreenFrame::new(4, 4, 24, vec![0; 96]).expect("padded stride");
        assert_eq!(padded.stride, 24);
        assert!(ScreenFrame::new(0, 4, 0, vec![]).is_none(), "no width");
        assert!(ScreenFrame::new(4, 0, 16, vec![]).is_none(), "no height");
        assert!(ScreenFrame::new(4, 4, 15, vec![0; 96]).is_none(), "a stride below width*4 cannot hold a row");
        assert!(ScreenFrame::new(4, 4, 16, vec![0; 63]).is_none(), "a buffer shorter than stride*height");
        // A width whose stride would overflow is refused rather than wrapping.
        assert!(ScreenFrame::new(u32::MAX, 2, 4, vec![0; 8]).is_none());
    }

    #[test]
    fn a_solid_frame_is_tight_and_padded_frames_keep_their_stride() {
        let f = ScreenFrame::solid(7, 3, [1, 2, 3, 255]);
        assert_eq!(f.stride, 7 * 4);
        assert_eq!(f.data.len(), f.stride * 3);
        assert_eq!(&f.data[..4], &[1, 2, 3, 255]);
    }
}
