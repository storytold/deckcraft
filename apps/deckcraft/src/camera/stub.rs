//! Camera capture on platforms with no backend (FreeBSD and other non-desktop targets): the
//! interface exists so `deckcraft-media` and the UI compile everywhere, and every start reports
//! `Unsupported` without touching a device.

use std::sync::mpsc::{self, Receiver};

use deckcraft_media::camera::{CameraError, CameraFormat, CameraId, CameraIn, CameraRequest, CameraSession};

/// A camera backend that is never available.
#[derive(Default)]
pub struct SystemCamera;

impl CameraIn for SystemCamera {
    fn start(&mut self, _request: &CameraRequest) -> Result<CameraSession, CameraError> {
        Err(CameraError::Unsupported)
    }

    fn stop(&mut self) {}

    fn devices(&mut self) -> Receiver<Result<Vec<deckcraft_media::camera::CameraDesc>, CameraError>> {
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(Err(CameraError::Unsupported));
        rx
    }

    fn formats(&mut self, _device: Option<&CameraId>) -> Receiver<Result<Vec<CameraFormat>, CameraError>> {
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(Err(CameraError::Unsupported));
        rx
    }
}
