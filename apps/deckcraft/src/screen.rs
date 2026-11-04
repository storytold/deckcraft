//! Screen capture for the desktop host: `scap` on Windows, macOS and Linux, a stub elsewhere
//! (FreeBSD, wasm). The engine sees only [`ScreenIn`]; frames are BGRA pictures with the backend's
//! row stride.
//!
//! The capture device runs on its own thread because both `scap`'s `Capturer::build` (which can
//! wait on a system picker) and `get_next_frame` block. `start` only spawns that thread and returns;
//! the worker then publishes whether it started, or why it could not, into a shared slot that
//! [`ScreenIn::frame`] reads. Frames keep flowing until the recorder drops the receiver or sets the
//! stop flag, so the UI never blocks on the device.

use deckcraft_screen::ScreenIn;

/// The host's screen-capture device: `scap` where it exists, [`NoScreen`] elsewhere.
pub fn host() -> Option<Box<dyn ScreenIn>> {
    if std::env::var_os("DECKCRAFT_NO_SCREEN").is_some() {
        return None;
    }
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        Some(Box::new(ScapIn::default()))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Some(Box::new(deckcraft_screen::NoScreen))
    }
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
mod imp {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, TryRecvError, TrySendError, sync_channel};

    use deckcraft_screen::{CaptureOptions, CaptureStatus, CaptureTarget, ScreenError, ScreenFrame, ScreenIn, TargetKind};

    /// A frame or a capture error from the worker thread.
    type Item = Result<ScreenFrame, ScreenError>;

    /// What the worker reports about starting, before it delivers frames: `None` while the
    /// capturer is still being built, `Some(Ok(()))` once it captures, `Some(Err(_))` when it
    /// never will.
    type Outcome = Option<Result<(), ScreenError>>;

    /// The worker's [`Outcome`], shared with [`ScapIn::frame`].
    type Ready = Arc<Mutex<Outcome>>;

    /// Record the worker's starting state for [`ScapIn::frame`] to report.
    fn publish(ready: &Mutex<Outcome>, outcome: Outcome) {
        let mut slot = ready.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = outcome;
    }

    /// The `scap`-backed device. A new worker runs per `start`; its receiver and ready slot are
    /// replaced, so a stale worker's sends and reports are ignored and it exits.
    #[derive(Default)]
    pub struct ScapIn {
        frames: Option<Receiver<Item>>,
        stop: Option<Arc<AtomicBool>>,
        ready: Option<Ready>,
    }

    impl ScapIn {
        fn stop_worker(&mut self) {
            if let Some(stop) = self.stop.take() {
                stop.store(true, Ordering::Relaxed);
            }
            // Dropping the receiver makes the worker's next send fail, so it stops capturing even
            // when no frame arrived after the flag was set. Dropping the ready slot discards a
            // failure the finished take will never be asked about.
            self.frames = None;
            self.ready = None;
        }

        /// The worker's starting state, if it has reported one.
        fn reported(&self) -> Outcome {
            let ready = self.ready.as_ref()?;
            let slot = ready.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            slot.clone()
        }

        /// A device whose worker has reported `state` and then exited, with a frame channel
        /// nothing sends on. Exercises [`ScreenIn::frame`]'s starting-report path without a
        /// capture device.
        #[cfg(test)]
        pub fn with_report(state: Outcome) -> Self {
            let (tx, rx) = sync_channel::<Item>(1);
            drop(tx);
            ScapIn { frames: Some(rx), stop: None, ready: Some(Arc::new(Mutex::new(state))) }
        }
    }

    impl Drop for ScapIn {
        fn drop(&mut self) {
            self.stop_worker();
        }
    }

    fn build_error(e: scap::capturer::CapturerBuildError) -> ScreenError {
        match e {
            scap::capturer::CapturerBuildError::NotSupported => ScreenError::Unavailable,
            scap::capturer::CapturerBuildError::PermissionNotGranted => ScreenError::PermissionDenied,
        }
    }

    fn target(row: &scap::Target) -> CaptureTarget {
        let (id, title) = match row {
            scap::Target::Display(d) => (u64::from(d.id), d.title.clone()),
            scap::Target::Window(w) => (u64::from(w.id), w.title.clone()),
        };
        let kind = if matches!(row, scap::Target::Display(_)) { TargetKind::Display } else { TargetKind::Window };
        // `scap` 0.0.8 doesn't expose target sizes publicly; the picker asks for a region only
        // when the user chooses one.
        CaptureTarget { id, kind, title, width: 0, height: 0 }
    }

    fn convert(frame: scap::frame::Frame) -> Option<ScreenFrame> {
        match frame {
            scap::frame::Frame::BGRA(f) => {
                let (w, h) = (u32::try_from(f.width).ok()?, u32::try_from(f.height).ok()?);
                // Windows hands back a raw buffer whose rows may be padded; the pitch is whatever
                // the buffer's length says, never an assumption of `width * 4`.
                let tight = w as usize * 4;
                let stride = if h > 0 && f.data.len() % h as usize == 0 { f.data.len() / h as usize } else { tight };
                ScreenFrame::new(w, h, stride.max(tight), f.data)
            }
            // The other layouts are not requested; converting them would guess at stride and
            // channel order. Treat them as a frame we cannot use rather than a wrong picture.
            _ => None,
        }
    }

    impl ScreenIn for ScapIn {
        fn status(&mut self) -> CaptureStatus {
            if !scap::is_supported() {
                CaptureStatus::Unsupported
            } else if !scap::has_permission() {
                CaptureStatus::PermissionDenied
            } else {
                CaptureStatus::Ready
            }
        }

        fn targets(&mut self) -> Result<Vec<CaptureTarget>, ScreenError> {
            if !scap::is_supported() {
                return Err(ScreenError::Unavailable);
            }
            if !scap::has_permission() {
                return Err(ScreenError::PermissionDenied);
            }
            let mut out: Vec<CaptureTarget> = scap::get_all_targets().iter().map(target).collect();
            // Displays first: they are what a screen recording usually means.
            out.sort_by_key(|t| (t.kind != TargetKind::Display, t.id));
            if out.is_empty() { Err(ScreenError::NoDevice) } else { Ok(out) }
        }

        fn start(&mut self, options: &CaptureOptions) -> Result<(), ScreenError> {
            self.stop_worker();
            match self.status() {
                CaptureStatus::Ready => {}
                CaptureStatus::Unsupported => return Err(ScreenError::Unavailable),
                CaptureStatus::PermissionDenied => return Err(ScreenError::PermissionDenied),
            }
            // Only primitives cross the thread boundary: `scap::Target` holds platform handles
            // and is not guaranteed `Send`. The worker looks the target up itself.
            let wanted = options.target;
            let kind = options.kind;
            let fps = options.fps.clamp(1, 60);
            let cursor = options.show_cursor;
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            // Two frames at most: the recorder takes one per interval, so a slow consumer drops
            // capture instead of queueing full pictures (8 MB each at 1080p) without a limit.
            let (tx, rx) = sync_channel::<Item>(2);
            let ready: Ready = Arc::new(Mutex::new(None));
            let report = ready.clone();
            let spawned = std::thread::Builder::new().name("deckcraft-screen".into()).spawn(move || {
                // A target that disappeared since the picker listed it must fail here: `scap`
                // would record the primary display instead, a surface the user did not choose. The
                // kind picks the right variant when a display and a window share an id.
                let target = match wanted {
                    Some(id) => {
                        let matches: Vec<scap::Target> = scap::get_all_targets()
                            .into_iter()
                            .filter(|t| match (t, kind) {
                                (scap::Target::Display(d), None | Some(TargetKind::Display)) => u64::from(d.id) == id,
                                (scap::Target::Window(w), None | Some(TargetKind::Window)) => u64::from(w.id) == id,
                                _ => false,
                            })
                            .collect();
                        match matches.len() {
                            1 => matches.into_iter().next(),
                            // The picked surface is gone (a closed window, an unplugged display):
                            // recording the primary display instead would film something the user
                            // never chose.
                            0 => {
                                publish(&report, Some(Err(ScreenError::NoDevice)));
                                return;
                            }
                            // Without a kind the id is ambiguous: record nothing rather than a
                            // surface the user did not pick.
                            _ => {
                                publish(&report, Some(Err(ScreenError::Failed("that id is both a display and a window; pick one".into()))));
                                return;
                            }
                        }
                    }
                    None => None,
                };
                let opts = scap::capturer::Options {
                    fps,
                    show_cursor: cursor,
                    show_highlight: false,
                    target,
                    crop_area: None,
                    output_type: scap::frame::FrameType::BGRAFrame,
                    output_resolution: scap::capturer::Resolution::Captured,
                    excluded_targets: None,
                };
                let mut cap = match scap::capturer::Capturer::build(opts) {
                    Ok(c) => c,
                    Err(e) => {
                        publish(&report, Some(Err(build_error(e))));
                        return;
                    }
                };
                cap.start_capture();
                // A stopped or replaced capture has already set the flag, so the loop below is
                // skipped and this worker releases the device at once.
                publish(&report, Some(Ok(())));
                while !flag.load(Ordering::Relaxed) {
                    match cap.get_next_frame() {
                        Ok(f) => {
                            let Some(f) = convert(f) else { continue };
                            match tx.try_send(Ok(f)) {
                                // The consumer is behind: dropping this frame beats queueing it.
                                Ok(()) | Err(TrySendError::Full(_)) => {}
                                Err(TrySendError::Disconnected(_)) => break,
                            }
                        }
                        Err(_) => break,
                    }
                }
                cap.stop_capture();
            });
            if let Err(e) = spawned {
                return Err(ScreenError::Failed(format!("couldn't start the screen recorder: {e}")));
            }
            // Never wait for the worker's first report: `Capturer::build` blocks on the system
            // picker (on Wayland, a portal D-Bus call that can take minutes while the user
            // chooses), so waiting here would freeze the window's repainting. `frame` reports
            // the outcome instead, and a failure ends the take with its message.
            self.frames = Some(rx);
            self.stop = Some(stop);
            self.ready = Some(ready);
            Ok(())
        }

        fn frame(&mut self) -> Result<Option<ScreenFrame>, ScreenError> {
            // A worker that never gets to capture reports why here; until it reports success there
            // is nothing to read from the channel, not "no frame since the last call". Once the
            // slot is dropped, the frame channel says everything.
            if self.ready.is_some() {
                match self.reported() {
                    None => return Ok(None),
                    Some(Err(e)) => return Err(e),
                    Some(Ok(())) => self.ready = None,
                }
            }
            let Some(rx) = self.frames.as_ref() else { return Ok(None) };
            // The host returns the latest frame (see `ScreenIn::frame`): take everything queued and
            // keep the newest, so a slow consumer never replays a stale picture after a pause.
            let mut latest = None;
            loop {
                match rx.try_recv() {
                    Ok(Ok(f)) => latest = Some(f),
                    Ok(Err(e)) => return Err(e),
                    Err(TryRecvError::Empty) => return Ok(latest),
                    Err(TryRecvError::Disconnected) => {
                        return match latest {
                            Some(f) => Ok(Some(f)),
                            // The worker may have published its reason and exited after the check
                            // above: prefer that over the generic message.
                            None => match self.reported() {
                                Some(Err(e)) => Err(e),
                                _ => Err(ScreenError::Failed("the screen capture stopped".into())),
                            },
                        };
                    }
                }
            }
        }

        fn stop(&mut self) {
            self.stop_worker();
        }

        fn request_permission(&mut self) -> bool {
            scap::request_permission()
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub use imp::ScapIn;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_is_a_device_even_when_capture_is_unavailable() {
        let mut device = host().expect("a stub or a device");
        // On a machine without a screen (CI) the status is still one of the three, never a panic.
        let _ = device.status();
        let _ = device.targets();
    }

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    #[test]
    fn a_worker_that_could_not_start_reports_it_through_frame() {
        use deckcraft_screen::ScreenError;

        let mut device = ScapIn::with_report(Some(Err(ScreenError::NoDevice)));
        // `start` returns before the worker builds the capturer, so the reason it never will must
        // arrive from `frame`: that is what ends the take with the message.
        assert_eq!(device.frame(), Err(ScreenError::NoDevice));
        // A second poll keeps the reason instead of turning it into "no frame yet".
        assert_eq!(device.frame(), Err(ScreenError::NoDevice));
    }

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    #[test]
    fn a_worker_still_building_reports_no_frame_until_it_starts() {
        let mut device = ScapIn::with_report(None);
        // A Wayland picker can hold the worker here for minutes; that is not a failure.
        assert_eq!(device.frame(), Ok(None));
        assert_eq!(device.frame(), Ok(None));
    }
}
