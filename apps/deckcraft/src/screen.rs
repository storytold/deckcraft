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
        /// The capture thread. `scap` 0.0.8 cannot cancel a blocked `get_next_frame`, so a stop
        /// hands the handle to a reaper that joins it once the device yields a frame.
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl ScapIn {
        fn stop_worker(&mut self) {
            if let Some(stop) = self.stop.take() {
                stop.store(true, Ordering::Relaxed);
            }
            // Dropping the receiver makes the worker's next send fail, so it stops capturing even
            // when no frame arrived after the flag was set. Dropping the ready slot discards a
            // failure the finished take will never be asked about.
            //
            // `scap` 0.0.8 has no cancellable receive: `get_next_frame` loops on a private
            // receiver, and `stop_capture` needs the `&mut Capturer` that the blocked worker holds,
            // so the device is released when the screen next produces a frame. Stopping itself
            // never waits: the flag and the dropped receiver are all the worker needs to end.
            self.frames = None;
            self.ready = None;
            if let Some(handle) = self.handle.take() {
                // Join off the UI thread: the worker can sit in `get_next_frame` until the screen
                // changes, and Stop must not freeze the window for that long. The reaper also
                // says so when the release was slow, which is the one thing a caller can act on.
                let started = std::time::Instant::now();
                let reaped = std::thread::Builder::new().name("deckcraft-screen-stop".into()).spawn(move || {
                    let _ = handle.join();
                    let waited = started.elapsed();
                    if waited > std::time::Duration::from_secs(2) {
                        log::warn!("screen recording: the capture device took {waited:?} to stop; it is released at the next captured frame");
                    }
                });
                if reaped.is_err() {
                    log::warn!("screen recording: no thread to join the capture device; it is released at the next captured frame");
                }
            }
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
            ScapIn { frames: Some(rx), stop: None, ready: Some(Arc::new(Mutex::new(state))), handle: None }
        }

        /// A device whose worker has reported `state` and is still running: the frame channel is
        /// open but nothing has been sent yet, as while the capturer is being built. The caller
        /// keeps the returned sender alive; dropping it marks the worker as ended.
        #[cfg(test)]
        pub fn with_open_report(state: Outcome) -> (Self, std::sync::mpsc::SyncSender<Item>) {
            let (tx, rx) = sync_channel::<Item>(1);
            (ScapIn { frames: Some(rx), stop: None, ready: Some(Arc::new(Mutex::new(state))), handle: None }, tx)
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

    /// What a `scap` target is to the picker: its id and kind (ids are only unique within a
    /// kind).
    fn identity(row: &scap::Target) -> (u64, TargetKind) {
        match row {
            scap::Target::Display(d) => (u64::from(d.id), TargetKind::Display),
            scap::Target::Window(w) => (u64::from(w.id), TargetKind::Window),
        }
    }

    /// The picker's order: displays before windows, then by id.
    fn order_key(kind: TargetKind, id: u64) -> (bool, u64) {
        (kind != TargetKind::Display, id)
    }

    /// The index of the one candidate `wanted` names. No match when the surface is gone (a closed
    /// window, an unplugged display) and an error when the id is both a display and a window and
    /// no kind disambiguates it. Kept free of `scap` types so the matching is testable.
    fn choose_target(wanted: u64, kind: Option<TargetKind>, candidates: &[(u64, TargetKind)]) -> Result<usize, ScreenError> {
        let found: Vec<usize> =
            candidates.iter().enumerate().filter(|(_, (id, k))| *id == wanted && kind.is_none_or(|want| want == *k)).map(|(i, _)| i).collect();
        match found.as_slice() {
            [i] => Ok(*i),
            // The picked surface is gone: recording the primary display instead would film
            // something the user never chose.
            [] => Err(ScreenError::NoDevice),
            _ => Err(ScreenError::Failed("that id is both a display and a window; pick one".into())),
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

    pub(super) fn convert(frame: scap::frame::Frame) -> Option<ScreenFrame> {
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
            out.sort_by_key(|t| order_key(t.kind, t.id));
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
            // The command validated the range; a wrong value is refused rather than silently
            // clamped, so two layers cannot disagree about the rate that was recorded.
            if !(1..=60).contains(&options.fps) {
                return Err(ScreenError::Failed(format!("the capture rate must be from 1 to 60 frames per second, not {}", options.fps)));
            }
            let fps = options.fps;
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
                        let all = scap::get_all_targets();
                        let candidates: Vec<(u64, TargetKind)> = all.iter().map(identity).collect();
                        match choose_target(id, kind, &candidates) {
                            Ok(i) => all.into_iter().nth(i),
                            Err(e) => {
                                publish(&report, Some(Err(e)));
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
                            // Stop was requested while this frame was on its way: drop it, so
                            // nothing recorded after Stop reaches the take, and release the
                            // device without another blocking read.
                            if flag.load(Ordering::Relaxed) {
                                break;
                            }
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
            let handle = match spawned {
                Ok(handle) => handle,
                Err(e) => return Err(ScreenError::Failed(format!("couldn't start the screen recorder: {e}"))),
            };
            // Never wait for the worker's first report: `Capturer::build` blocks on the system
            // picker (on Wayland, a portal D-Bus call that can take minutes while the user
            // chooses), so waiting here would freeze the window's repainting. `frame` reports
            // the outcome instead, and a failure ends the take with its message.
            self.frames = Some(rx);
            self.stop = Some(stop);
            self.ready = Some(ready);
            self.handle = Some(handle);
            Ok(())
        }

        fn frame(&mut self) -> Result<Option<ScreenFrame>, ScreenError> {
            // A worker that never gets to capture reports why here; until it reports success there
            // is nothing to read from the channel, not "no frame since the last call". Once the
            // slot is dropped, the frame channel says everything.
            if self.ready.is_some() {
                match self.reported() {
                    None => {
                        // The worker reports success before it sends any frame, so a frame channel
                        // whose sender is gone before that report means it ended early (a panic
                        // while building the capturer): say so instead of waiting forever.
                        let Some(rx) = self.frames.as_ref() else { return Ok(None) };
                        match rx.try_recv() {
                            Err(TryRecvError::Empty) => return Ok(None),
                            Err(TryRecvError::Disconnected) => {
                                return Err(ScreenError::Failed("the screen capture stopped before it started".into()));
                            }
                            // Only possible if the worker published between the read above and
                            // this poll; it is a real frame, so keep it.
                            Ok(item) => return item.map(Some),
                        }
                    }
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

    #[cfg(test)]
    mod unit {
        use super::*;

        fn bgra(w: i32, h: i32, data: Vec<u8>) -> scap::frame::Frame {
            scap::frame::Frame::BGRA(scap::frame::BGRAFrame { display_time: 0, width: w, height: h, data })
        }

        #[test]
        fn a_padded_windows_frame_keeps_its_pitch() {
            // 4×2 BGRA with 8 bytes of row padding: 24-byte rows.
            let mut data = vec![0u8; 48];
            for row in 0..2 {
                data[row * 24..row * 24 + 16].fill(0x7F);
            }
            let f = convert(bgra(4, 2, data)).expect("a BGRA frame converts");
            assert_eq!((f.width, f.height, f.stride), (4, 2, 24));
            assert_eq!(&f.data[..4], &[0x7F; 4]);
        }

        #[test]
        fn a_tight_frame_and_a_malformed_length_fall_back_to_width_times_four() {
            let f = convert(bgra(4, 2, vec![0; 32])).expect("tight");
            assert_eq!(f.stride, 16);
            // 33 bytes over two rows is not a whole number of rows: use the tight stride.
            let f = convert(bgra(4, 2, vec![0; 33])).expect("tight fallback");
            assert_eq!(f.stride, 16);
            // A buffer shorter than one tight row cannot be a picture.
            assert!(convert(bgra(4, 2, vec![0; 8])).is_none());
        }

        #[test]
        fn a_frame_in_another_layout_is_refused_rather_than_guessed() {
            let rgb = scap::frame::Frame::RGB(scap::frame::RGBFrame { display_time: 0, width: 4, height: 2, data: vec![0; 24] });
            assert!(convert(rgb).is_none());
        }

        #[test]
        fn displays_sort_before_windows_and_targets_are_matched_by_id_and_kind() {
            assert!(order_key(TargetKind::Display, 99) < order_key(TargetKind::Window, 1));
            let candidates = [(1, TargetKind::Display), (1, TargetKind::Window), (7, TargetKind::Window)];
            assert_eq!(choose_target(1, Some(TargetKind::Display), &candidates), Ok(0));
            assert_eq!(choose_target(1, Some(TargetKind::Window), &candidates), Ok(1));
            assert_eq!(choose_target(7, None, &candidates), Ok(2));
            // An id shared by a display and a window needs a kind.
            assert_eq!(choose_target(1, None, &candidates), Err(ScreenError::Failed("that id is both a display and a window; pick one".into())));
            // A surface that disappeared is never replaced by another one.
            assert_eq!(choose_target(9, None, &candidates), Err(ScreenError::NoDevice));
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
        use deckcraft_screen::{CaptureStatus, ScreenError};

        let mut device = host().expect("a stub or a device");
        // On a machine without a screen (CI) the status is still one of the three, never a panic
        // and never a fourth state.
        let status = device.status();
        assert!(matches!(status, CaptureStatus::Ready | CaptureStatus::Unsupported | CaptureStatus::PermissionDenied), "{status:?}");
        // Targets are a list, or the error that explains why there is none; a host that cannot
        // record does not report an empty success.
        match device.targets() {
            Ok(targets) => {
                assert_eq!(status, CaptureStatus::Ready, "a host that lists targets is ready");
                assert!(!targets.is_empty(), "the picker never gets an empty success");
            }
            Err(e) => {
                // A headless machine can be ready and still list nothing (NoDevice), so the error
                // says why rather than the list saying nothing.
                assert!(
                    matches!(e, ScreenError::Unavailable | ScreenError::PermissionDenied | ScreenError::NoDevice | ScreenError::Failed(_)),
                    "{e:?}"
                );
            }
        }
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
        // The sender stays alive: the worker exists, it just has no frame yet.
        let (mut device, _worker) = ScapIn::with_open_report(None);
        // A Wayland picker can hold the worker here for minutes; that is not a failure.
        assert_eq!(device.frame(), Ok(None));
        assert_eq!(device.frame(), Ok(None));
    }

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    #[test]
    fn a_worker_that_ended_before_starting_reports_it_through_frame() {
        // `with_report` drops the sender, so the worker is gone without ever publishing a
        // report: `frame` must say so rather than answer "no frame yet" forever.
        let mut device = ScapIn::with_report(None);
        assert_eq!(device.frame(), Err(deckcraft_screen::ScreenError::Failed("the screen capture stopped before it started".into())));
        // A second poll keeps the failure instead of going quiet.
        assert!(device.frame().is_err());
    }
}
