//! Windows/macOS/Linux camera capture over `cameras` (pinned `=0.3.2`).

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError};
use std::thread;
use std::time::Duration;

use deckcraft_media::camera::{
    CameraDesc, CameraError, CameraEvent, CameraFormat, CameraFrame, CameraId, CameraIn, CameraInfo, CameraPosition, CameraRequest, CameraSession,
    PixelFormat,
};

/// Two buffered frames: enough that a slow paint does not stall the worker, small enough that the
/// preview cannot accumulate latency. A full channel drops the newest frame and the next one gets
/// through after the UI drains.
const EVENT_CAPACITY: usize = 2;
/// How long a frame read waits before the worker checks its command channel.
const FRAME_WAIT: Duration = Duration::from_millis(100);
/// A terminal event (`Failed`) may arrive while the UI has both slots full; the worker retries a
/// bounded number of times so the reason is not lost, without stalling shutdown for long.
const TERMINAL_TRIES: usize = 20;
const TERMINAL_WAIT: Duration = Duration::from_millis(50);

/// Told to the worker thread. `Start` always replaces any running camera first.
enum Command {
    Start(CameraRequest, SyncSender<CameraEvent>),
    Stop,
    Quit,
}

/// Camera capture through the platform API. Create it without a device; it does nothing until
/// [`CameraIn::start`].
#[derive(Default)]
pub struct SystemCamera {
    commands: Option<Sender<Command>>,
}

impl SystemCamera {
    /// The worker's command sender, spawning the worker on first use.
    fn ensure_worker(&mut self) -> Result<Sender<Command>, CameraError> {
        if let Some(commands) = &self.commands {
            return Ok(commands.clone());
        }
        let (commands, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("deckcraft-camera".into())
            .spawn(move || run(receiver))
            .map_err(|error| CameraError::Backend(error.to_string()))?;
        self.commands = Some(commands.clone());
        Ok(commands)
    }
}

/// Hand one `Start` to the worker and take the session's event channel. Fails only when the
/// worker thread has exited; the caller decides whether to respawn it.
fn send_start(commands: &Sender<Command>, request: &CameraRequest) -> Result<CameraSession, ()> {
    let (events, receiver) = mpsc::sync_channel(EVENT_CAPACITY);
    commands.send(Command::Start(request.clone(), events)).map_err(|_| ())?;
    Ok(CameraSession { started_at: web_time::Instant::now(), events: receiver })
}

impl CameraIn for SystemCamera {
    fn start(&mut self, request: &CameraRequest) -> Result<CameraSession, CameraError> {
        let commands = self.ensure_worker()?;
        // A failed send means the worker thread has exited (a panic in the capture backend):
        // drop the dead sender so the retry spawns a new worker instead of failing every later
        // `start` until DeckCraft restarts.
        match send_start(&commands, request) {
            Ok(session) => Ok(session),
            Err(()) => {
                self.commands = None;
                let commands = self.ensure_worker()?;
                send_start(&commands, request).map_err(|_| CameraError::Backend("the camera worker stopped".into()))
            }
        }
    }

    fn stop(&mut self) {
        if let Some(commands) = &self.commands {
            // Cooperatively; the worker drops the device when its current read returns.
            let _ = commands.send(Command::Stop);
        }
    }

    fn devices(&mut self) -> Receiver<Result<Vec<CameraDesc>, CameraError>> {
        probe_async(list_devices)
    }

    fn formats(&mut self, device: Option<&CameraId>) -> Receiver<Result<Vec<CameraFormat>, CameraError>> {
        let device = device.cloned();
        probe_async(move || list_formats(device.as_deref()))
    }
}

impl Drop for SystemCamera {
    fn drop(&mut self) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(Command::Quit);
        }
        // Deliberately not joined: the worker may be inside enumeration waiting on the macOS
        // permission sheet. The process is ending; the OS closes the handle.
    }
}

/// Run `work` on a short-lived thread so a probe never blocks the UI. A thread that cannot start
/// answers with the reason instead of leaving the receiver to wait for a result that never comes.
fn probe_async<T, F>(work: F) -> Receiver<Result<T, CameraError>>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, CameraError> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    let spawned = thread::Builder::new().name("deckcraft-camera-probe".into()).spawn({
        let tx = tx.clone();
        move || {
            let _ = tx.send(work());
        }
    });
    if spawned.is_err() {
        let _ = tx.send(Err(CameraError::Backend("the camera probe thread could not start".into())));
    }
    rx
}

fn list_devices() -> Result<Vec<CameraDesc>, CameraError> {
    cameras::devices().map(|devices| devices.iter().map(desc).collect()).map_err(map_error)
}

fn list_formats(device: Option<&str>) -> Result<Vec<CameraFormat>, CameraError> {
    let devices = cameras::devices().map_err(map_error)?;
    let device = pick_device(&devices, device).ok_or(CameraError::NoDevice)?;
    let capabilities = cameras::probe(device).map_err(map_error)?;
    Ok(capabilities.formats.iter().map(camera_format).collect())
}

/// One open stream and its event channel.
struct Running {
    camera: cameras::Camera,
    events: SyncSender<CameraEvent>,
    device: CameraDesc,
    fps: f32,
    seq: u64,
    /// Whether `Opened` has been sent (it waits for the first frame, whose size is authoritative on
    /// macOS even when `Camera::config` echoes the request).
    opened: bool,
}

fn run(commands: Receiver<Command>) {
    let mut running: Option<Running> = None;
    loop {
        if let Some(current) = running.as_mut() {
            match cameras::next_frame(&current.camera, FRAME_WAIT) {
                Ok(frame) => {
                    if !deliver(current, frame) {
                        running = None;
                    }
                }
                Err(cameras::Error::Timeout) => {}
                Err(error) => {
                    send_terminal(&current.events, CameraEvent::Failed(map_error(error)));
                    running = None;
                }
            }
        }
        // Polled after every frame, not only after a timeout: a stream that never stalls would
        // otherwise keep `Stop` and `Quit` waiting for a timeout that does not come.
        let Some(command) = next_command(&commands, running.is_some()) else { continue };
        match command {
            Command::Start(request, events) => {
                // Drop the old camera before opening the new one: a device cannot be opened twice.
                drop(running.take());
                running = open(&request, events);
            }
            Command::Stop => running = None,
            Command::Quit => return,
        }
    }
}

/// The next command: blocking while no camera runs (the worker is idle), non-blocking while one
/// delivers frames. A gone host ends the worker like `Quit`, in both branches: an idle worker
/// whose sender was dropped would otherwise spin on a `recv` that never blocks again.
fn next_command(commands: &Receiver<Command>, running: bool) -> Option<Command> {
    match running {
        true => match commands.try_recv() {
            Ok(command) => Some(command),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Command::Quit),
        },
        false => match commands.recv() {
            Ok(command) => Some(command),
            Err(_) => Some(Command::Quit),
        },
    }
}

/// The reported stream rate: the request clamped to the format's range, or the request itself
/// when the device-reported bounds are malformed (inverted, non-finite) and `clamp` would panic.
fn clamp_fps(wanted: f64, min: f64, max: f64) -> f64 {
    if min.is_finite() && max.is_finite() && min <= max { wanted.clamp(min, max) } else { wanted }
}

fn open(request: &CameraRequest, events: SyncSender<CameraEvent>) -> Option<Running> {
    let devices = match cameras::devices() {
        Ok(devices) => devices,
        Err(error) => {
            send_terminal(&events, CameraEvent::Failed(map_error(error)));
            return None;
        }
    };
    let Some(device) = pick_device(&devices, request.device.as_deref()) else {
        send_terminal(&events, CameraEvent::Failed(CameraError::NoDevice));
        return None;
    };

    let capabilities = match cameras::probe(device) {
        Ok(capabilities) => capabilities,
        Err(error) => {
            send_terminal(&events, CameraEvent::Failed(map_error(error)));
            return None;
        }
    };
    let wanted = stream_config(request);
    // The closest format the device really has; no format list at all leaves the request as is and
    // lets the backend report `FormatNotSupported`.
    let (config, fps) = match cameras::best_format(&capabilities, &wanted) {
        Some(found) => (
            cameras::StreamConfig { resolution: found.resolution, framerate: wanted.framerate, pixel_format: found.pixel_format },
            // The stream runs at the requested rate, not the format's maximum: report it clamped
            // to the range the device really produces, so the status text matches the stream. A
            // malformed inverted range would make `clamp` panic, so it falls back to the request.
            clamp_fps(wanted.framerate as f64, found.framerate_range.min, found.framerate_range.max) as f32,
        ),
        None => (wanted, wanted.framerate as f32),
    };
    let camera = match cameras::open(device, config) {
        Ok(camera) => camera,
        Err(error) => {
            send_terminal(&events, CameraEvent::Failed(map_error(error)));
            return None;
        }
    };
    Some(Running { camera, events, device: desc(device), fps, seq: 0, opened: false })
}

/// The largest frame this preview accepts, in bytes (`4 * width * height`): a device report is not
/// trusted to be small, and the conversion allocates from it.
const MAX_FRAME_BYTES: usize = 256 << 20;

/// The pixel count of a device-reported frame, or `None` when its width and height cannot make a
/// picture: zero, or larger than [`MAX_FRAME_BYTES`]. Checked before the conversion allocates.
fn frame_pixels(width: u32, height: u32) -> Option<usize> {
    let pixels = (width as usize).checked_mul(height as usize)?;
    let bytes = pixels.checked_mul(4)?;
    (bytes > 0 && bytes <= MAX_FRAME_BYTES).then_some(pixels)
}

/// Convert and forward one frame. Returns false when the UI dropped the session.
fn deliver(running: &mut Running, frame: cameras::Frame) -> bool {
    let (width, height) = (frame.width, frame.height);
    let source_format = pixel_format(frame.pixel_format);
    let source_t = (!frame.timestamp.is_zero()).then_some(frame.timestamp);
    // The device's own numbers: validate them before the conversion allocates a picture from them.
    let Some(pixels) = frame_pixels(width, height) else { return true };
    // A corrupt compressed frame (MJPEG recovery) is skipped; the stream keeps going.
    let Ok(rgba) = cameras::to_rgba8(&frame) else { return true };
    if rgba.len() != pixels * 4 {
        return true;
    }
    if !running.opened {
        running.opened = true;
        let info = CameraInfo { device: running.device.clone(), width, height, fps: running.fps, source_format };
        if !send(&running.events, CameraEvent::Opened(info)) {
            return false;
        }
    }
    let event = CameraEvent::Frame(CameraFrame {
        seq: running.seq,
        at: web_time::Instant::now(),
        source_t,
        width,
        height,
        rgba: Arc::from(rgba.into_boxed_slice()),
    });
    running.seq = running.seq.wrapping_add(1);
    send(&running.events, event)
}

/// Send unless the UI is gone. A disconnected receiver means the session was abandoned.
fn send(events: &SyncSender<CameraEvent>, event: CameraEvent) -> bool {
    !matches!(events.try_send(event), Err(TrySendError::Disconnected(_)))
}

/// Deliver the last event of a session. The channel holds two events, so a `Failed` sent into a
/// full one would be dropped and the UI would only see `Disconnected` (reported as a plain "off").
/// Retry briefly while it is full; give up when the receiver is gone.
fn send_terminal(events: &SyncSender<CameraEvent>, mut event: CameraEvent) {
    for _ in 0..TERMINAL_TRIES {
        match events.try_send(event) {
            Err(TrySendError::Full(back)) => {
                event = back;
                thread::sleep(TERMINAL_WAIT);
            }
            _ => return,
        }
    }
}

fn stream_config(request: &CameraRequest) -> cameras::StreamConfig {
    let format = request.format_or_default();
    cameras::StreamConfig {
        resolution: cameras::Resolution { width: format.width.max(1), height: format.height.max(1) },
        framerate: if format.fps.is_finite() { format.fps.clamp(1.0, 240.0).round() as u32 } else { 30 },
        pixel_format: cameras_pixel(format.pixel_format),
    }
}

fn cameras_pixel(format: PixelFormat) -> cameras::PixelFormat {
    match format {
        PixelFormat::Bgra8 => cameras::PixelFormat::Bgra8,
        PixelFormat::Rgba8 => cameras::PixelFormat::Rgba8,
        PixelFormat::Rgb8 => cameras::PixelFormat::Rgb8,
        PixelFormat::Yuyv => cameras::PixelFormat::Yuyv,
        PixelFormat::Nv12 => cameras::PixelFormat::Nv12,
        PixelFormat::Mjpeg => cameras::PixelFormat::Mjpeg,
    }
}

fn pick_device<'a>(devices: &'a [cameras::Device], wanted: Option<&str>) -> Option<&'a cameras::Device> {
    match wanted {
        Some(id) => devices.iter().find(|device| device.id.0.as_str() == id),
        None => devices.first(),
    }
}

fn desc(device: &cameras::Device) -> CameraDesc {
    CameraDesc {
        id: device.id.0.clone(),
        name: device.name.clone(),
        position: match device.position {
            cameras::Position::Front => CameraPosition::Front,
            cameras::Position::Back => CameraPosition::Back,
            cameras::Position::External => CameraPosition::External,
            _ => CameraPosition::Unknown,
        },
    }
}

fn camera_format(found: &cameras::FormatDescriptor) -> CameraFormat {
    CameraFormat {
        width: found.resolution.width,
        height: found.resolution.height,
        fps: found.framerate_range.max as f32,
        pixel_format: pixel_format(found.pixel_format),
    }
}

fn pixel_format(format: cameras::PixelFormat) -> PixelFormat {
    match format {
        cameras::PixelFormat::Bgra8 => PixelFormat::Bgra8,
        cameras::PixelFormat::Rgba8 => PixelFormat::Rgba8,
        cameras::PixelFormat::Rgb8 => PixelFormat::Rgb8,
        cameras::PixelFormat::Yuyv => PixelFormat::Yuyv,
        cameras::PixelFormat::Nv12 => PixelFormat::Nv12,
        cameras::PixelFormat::Mjpeg => PixelFormat::Mjpeg,
        _ => PixelFormat::Rgba8,
    }
}

fn map_error(error: cameras::Error) -> CameraError {
    match error {
        cameras::Error::PermissionDenied => CameraError::PermissionDenied,
        cameras::Error::DeviceNotFound(_) => CameraError::NoDevice,
        cameras::Error::DeviceInUse => CameraError::InUse,
        cameras::Error::FormatNotSupported => CameraError::UnsupportedFormat,
        cameras::Error::StreamEnded => CameraError::Lost,
        cameras::Error::BackendNotImplemented { .. } | cameras::Error::Unsupported { .. } => CameraError::Unsupported,
        // Windows Media Foundation reports the privacy-toggle denial as E_ACCESSDENIED; `cameras`
        // wraps it as `Backend`, so map it here, in one place.
        cameras::Error::Backend { ref message, .. }
            if message.contains("0x80070005") || message.to_ascii_lowercase().contains("access is denied") =>
        {
            CameraError::PermissionDenied
        }
        cameras::Error::Timeout => CameraError::Backend("timed out waiting for a frame".into()),
        other => CameraError::Backend(other.to_string()),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn a_terminal_failure_survives_a_full_channel() {
        let (tx, rx) = mpsc::sync_channel(EVENT_CAPACITY);
        assert!(tx.try_send(CameraEvent::Closed).is_ok());
        assert!(tx.try_send(CameraEvent::Closed).is_ok());
        let worker = thread::spawn(move || send_terminal(&tx, CameraEvent::Failed(CameraError::Lost)));
        // The UI drains the channel on a later pass, as it does while painting.
        thread::sleep(Duration::from_millis(120));
        assert!(matches!(rx.recv(), Ok(CameraEvent::Closed)));
        assert!(matches!(rx.recv(), Ok(CameraEvent::Closed)));
        assert!(matches!(rx.recv(), Ok(CameraEvent::Failed(CameraError::Lost))), "the reason reached the UI");
        worker.join().expect("worker thread");
    }

    #[test]
    fn a_terminal_failure_does_not_linger_over_a_gone_ui() {
        let (tx, rx) = mpsc::sync_channel(EVENT_CAPACITY);
        drop(rx);
        let start = std::time::Instant::now();
        send_terminal(&tx, CameraEvent::Failed(CameraError::Lost));
        assert!(start.elapsed() < TERMINAL_WAIT, "gives up as soon as the receiver is gone");
    }

    #[test]
    fn a_device_reported_frame_size_is_checked_before_the_conversion_allocates() {
        assert_eq!(frame_pixels(640, 480), Some(640 * 480));
        assert_eq!(frame_pixels(8192, 8192), Some(8192 * 8192), "exactly the largest frame accepted");
        assert_eq!(frame_pixels(0, 480), None);
        assert_eq!(frame_pixels(640, 0), None);
        assert_eq!(frame_pixels(8192, 8193), None, "over the bound");
        assert_eq!(frame_pixels(u32::MAX, u32::MAX), None, "no overflow");
    }

    #[test]
    fn a_malformed_framerate_range_falls_back_to_the_request() {
        assert_eq!(clamp_fps(30.0, 15.0, 60.0), 30.0);
        assert_eq!(clamp_fps(120.0, 15.0, 60.0), 60.0, "clamped to the range");
        assert_eq!(clamp_fps(5.0, 15.0, 60.0), 15.0, "clamped to the range");
        assert_eq!(clamp_fps(30.0, 60.0, 15.0), 30.0, "inverted bounds: no panic, the request");
        assert_eq!(clamp_fps(30.0, f64::NAN, 60.0), 30.0, "non-finite bounds: the request");
        assert_eq!(clamp_fps(30.0, 15.0, f64::INFINITY), 30.0, "non-finite bounds: the request");
    }

    #[test]
    fn commands_are_polled_without_waiting_while_a_camera_delivers_frames() {
        let (tx, rx) = mpsc::channel();
        assert!(next_command(&rx, true).is_none(), "nothing queued: the worker keeps reading frames");
        tx.send(Command::Stop).expect("send");
        assert!(matches!(next_command(&rx, true), Some(Command::Stop)), "Stop is taken on the next frame");
        drop(tx);
        assert!(matches!(next_command(&rx, true), Some(Command::Quit)), "a gone host ends the worker");
        let (tx, rx) = mpsc::channel();
        drop(tx);
        assert!(matches!(next_command(&rx, false), Some(Command::Quit)), "an idle worker ends too, instead of spinning");
    }

    #[test]
    fn a_dead_worker_is_replaced_without_touching_a_camera() {
        // The failed send is reported without starting the platform camera: the retry path only
        // respawns the worker, whose `run` loop blocks on commands until a `Start` arrives.
        let (dead, receiver) = mpsc::channel::<Command>();
        drop(receiver);
        assert!(send_start(&dead, &CameraRequest::default()).is_err(), "a dead worker reports its send failure");
        let mut camera = SystemCamera::default();
        let commands = camera.ensure_worker().expect("a worker spawns");
        assert!(commands.send(Command::Stop).is_ok(), "the respawned worker drains commands");
        camera.commands = Some(commands);
    }

    #[test]
    fn start_with_a_dead_sender_respawns_its_worker() {
        // `start` itself drops a dead sender and retries: one call recovers from an exited
        // worker without opening the platform camera (the new `run` loop only blocks on commands
        // until a `Start` arrives; the session's event channel is dropped unread).
        let mut camera = SystemCamera::default();
        let (dead, receiver) = mpsc::channel::<Command>();
        drop(receiver);
        camera.commands = Some(dead);
        let session = camera.start(&CameraRequest::default()).expect("the retry spawns a new worker");
        drop(session);
        camera.stop();
    }
}
