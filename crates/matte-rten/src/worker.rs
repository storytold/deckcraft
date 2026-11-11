//! One inference worker, one pending frame, one last matte.
//!
//! The model takes about two seconds per frame on CPU. The UI draws at 60 Hz. The worker therefore
//! owns the only thread that runs inference, keeps at most one waiting frame, and never blocks the
//! caller: [`MatteWorker::process`] composites with the latest matte and asks for a new one on the
//! caller's cadence. Reused frames are counted so the panel and the docs can be honest.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use deckcraft_matte::compositing::{CompositeOptions, composite_prepared, prepare_matte};
use deckcraft_matte::frame::RgbaFrame;
use deckcraft_matte::{Matte, MatteError, Segmenter, Timings};
use deckcraft_model::CameraBackgroundMode;

/// How the worker finds its segmenter. Loading is what reads, hashes and parses the 191 MB model,
/// so it runs on the worker thread: [`MatteWorker::with_loader`] returns immediately and the
/// caller gets `Ok(None)` until the matte is ready (finding: the synchronous load froze the UI).
type Loader = Box<dyn FnOnce() -> Result<Box<dyn Segmenter>, MatteError> + Send>;

struct State {
    segmenter: Option<Box<dyn Segmenter>>,
    /// Taken and run on the worker thread on its first tick; `None` once a load was attempted.
    loader: Option<Loader>,
    pending: Option<RgbaFrame>,
    latest: Option<Arc<Matte>>,
    error: Option<MatteError>,
    timings: Timings,
    last_submit: Option<std::time::Instant>,
    stop: bool,
}

/// The matte the worker last prepared for compositing: resizing and feathering are per-matte work,
/// so a cached matte must not pay for them on every frame. `source` identifies the matte it came
/// from (`Arc::ptr_eq`), so the cache invalidates when a new matte arrives.
struct Prepared {
    source: Arc<Matte>,
    feather: u32,
    width: u32,
    height: u32,
    matte: Matte,
}

/// The last replacement picture resized for the frame size, with the picture it came from. Exact
/// equality on the source is the invalidation: a caller that draws the same replacement repeatedly
/// resizes it once, and a new picture replaces it.
struct Replacement {
    source: RgbaFrame,
    resized: RgbaFrame,
}

/// Runs a [`Segmenter`] on its own thread. Drop asks it to stop and detaches: the thread exits at
/// its next check, after the inference it is running, and a new worker may overlap it briefly.
pub struct MatteWorker {
    shared: Arc<(Mutex<State>, Condvar)>,
    /// Minimum time between submissions.
    interval: Duration,
    prepared: Mutex<Option<Prepared>>,
    replacement: Mutex<Option<Replacement>>,
}

impl MatteWorker {
    /// A worker over an already-built segmenter. Returns `Err` when the inference thread cannot be
    /// started: a worker that silently never runs would make `process` return `Ok(None)` for ever
    /// with no reason shown.
    pub fn new(segmenter: Box<dyn Segmenter>, interval: Duration) -> Result<Self, MatteError> {
        Self::with_loader(Box::new(move || Ok(segmenter)), interval)
    }

    /// A worker that loads its segmenter on the worker thread. The caller returns to the UI
    /// immediately and [`MatteWorker::process`] answers `Ok(None)` until the first matte exists; a
    /// load failure is remembered and reported as an [`MatteError::Unavailable`].
    pub fn with_loader(loader: Loader, interval: Duration) -> Result<Self, MatteError> {
        let shared = Arc::new((
            Mutex::new(State {
                segmenter: None,
                loader: Some(loader),
                pending: None,
                latest: None,
                error: None,
                timings: Timings::default(),
                last_submit: None,
                stop: false,
            }),
            Condvar::new(),
        ));
        let thread_shared = Arc::clone(&shared);
        // The handle is not kept: joining on drop would block the caller (the UI thread) for the
        // rest of a running inference. The thread owns an `Arc` of the state and exits on `stop`.
        std::thread::Builder::new()
            .name("deckcraft-matte".into())
            .spawn(move || run(thread_shared))
            .map_err(|e| MatteError::Unavailable(format!("cannot start the camera background worker: {e}")))?;
        Ok(MatteWorker { shared, interval, prepared: Mutex::new(None), replacement: Mutex::new(None) })
    }

    /// Offer a frame. At most one waits; a newer frame replaces an older one. `process` only calls
    /// this when nothing is waiting and the interval has elapsed, so the queue never grows.
    pub fn submit(&self, frame: RgbaFrame) {
        if let Ok(mut s) = self.shared.0.lock() {
            s.pending = Some(frame);
            s.last_submit = Some(std::time::Instant::now());
            self.shared.1.notify_one();
        }
    }

    /// The last completed matte, if any. Shared, not cloned: a caller that keeps the reference for
    /// one frame must not copy the whole 512x512 (or frame-sized) mask.
    pub fn latest(&self) -> Option<Arc<Matte>> {
        self.shared.0.lock().ok().and_then(|s| s.latest.clone())
    }

    /// The last failure, if the worker has not produced a matte since it.
    pub fn error(&self) -> Option<MatteError> {
        self.shared.0.lock().ok().and_then(|s| s.error.clone())
    }

    pub fn timings(&self) -> Timings {
        self.shared.0.lock().map(|s| s.timings).unwrap_or_default()
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Composite `frame` with the latest matte. Returns `None` until one exists; a missing matte
    /// submits the frame (when `submit` is true). The worker never runs on this thread.
    pub fn process(
        &self,
        frame: &RgbaFrame,
        options: &CompositeOptions,
        replacement: Option<&RgbaFrame>,
        submit: bool,
    ) -> Result<Option<RgbaFrame>, MatteError> {
        let latest = self.latest();
        let mut submitted = false;
        // One gate for every submission, first or later: a frame already waiting is not replaced,
        // and a segmenter that fails persistently is retried on the interval instead of on every
        // call. `last_submit` is `None` before the first submit, so the first frame goes now.
        if submit
            && let Ok(s) = self.shared.0.lock()
            && s.pending.is_none()
            && s.last_submit.is_none_or(|t| t.elapsed() >= self.interval)
        {
            drop(s);
            self.submit(frame.clone());
            submitted = true;
        }
        if let Some(m) = latest {
            let out = self.composite_cached(frame, &m, options, replacement)?;
            if !submitted && let Ok(mut s) = self.shared.0.lock() {
                s.timings.reused = s.timings.reused.saturating_add(1);
            }
            return Ok(Some(out));
        }
        match self.error() {
            Some(e) => Err(e),
            None => Ok(None),
        }
    }

    /// Composite with the cached matte and replacement. The matte is borrowed from shared state
    /// and its resized/feathered form is reused until a new matte arrives, so a cached matte costs
    /// one lerp over the frame rather than a clone, a resize and a feather on every call.
    fn composite_cached(
        &self,
        frame: &RgbaFrame,
        matte: &Arc<Matte>,
        options: &CompositeOptions,
        replacement: Option<&RgbaFrame>,
    ) -> Result<RgbaFrame, MatteError> {
        if options.mode == CameraBackgroundMode::Normal {
            return Ok(frame.clone());
        }
        let (width, height) = (frame.width(), frame.height());
        let mut cache = self.prepared.lock().unwrap_or_else(|e| e.into_inner());
        let stale =
            cache.as_ref().is_none_or(|p| !Arc::ptr_eq(&p.source, matte) || p.feather != options.feather || p.width != width || p.height != height);
        if stale {
            let prepared = prepare_matte(matte, width, height, options.feather)?;
            *cache = Some(Prepared { source: Arc::clone(matte), feather: options.feather, width, height, matte: prepared });
        }
        let prepared = match cache.as_ref() {
            Some(p) => &p.matte,
            None => return Err(MatteError::InvalidImage("the compositor has no prepared matte".into())),
        };
        let mut resized = self.replacement.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(r) = replacement
            && (r.width(), r.height()) != (width, height)
            && resized.as_ref().is_none_or(|c| c.source != *r)
        {
            *resized = Some(Replacement { source: r.clone(), resized: r.resize(width, height)? });
        }
        let replacement = match replacement {
            Some(r) if (r.width(), r.height()) == (width, height) => Some(r),
            Some(_) => resized.as_ref().map(|c| &c.resized),
            None => None,
        };
        composite_prepared(frame, prepared, options, replacement)
    }
}

impl Drop for MatteWorker {
    fn drop(&mut self) {
        if let Ok(mut s) = self.shared.0.lock() {
            s.stop = true;
            s.pending = None;
            self.shared.1.notify_one();
        }
    }
}

fn run(shared: Arc<(Mutex<State>, Condvar)>) {
    // Load before the first frame. The 191 MB verify and the graph parse happen here, on the
    // worker, so the caller's `with_loader` (the UI thread) returns at once and `process` answers
    // `Ok(None)` until a matte exists. A failure is kept as the worker's error, which `process`
    // reports instead of returning "no matte yet" for ever.
    if let Ok(Some(loader)) = shared.0.lock().map(|mut s| s.loader.take()) {
        let loaded = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(loader)) {
            Ok(result) => result,
            Err(payload) => Err(MatteError::Unavailable(format!("the segmenter panicked while loading: {}", panic_text(&*payload)))),
        };
        if let Ok(mut s) = shared.0.lock() {
            match loaded {
                Ok(segmenter) => s.segmenter = Some(segmenter),
                Err(e) => {
                    s.error = Some(e);
                    s.pending = None;
                }
            }
        }
    }
    loop {
        let job = {
            let Ok(mut s) = shared.0.lock() else { return };
            loop {
                if s.stop {
                    return;
                }
                if s.pending.is_some() {
                    break;
                }
                let Ok(next) = shared.1.wait(s) else { return };
                s = next;
            }
            let frame = s.pending.take();
            let segmenter = s.segmenter.take();
            (frame, segmenter)
        };
        if let (Some(frame), Some(mut segmenter)) = job {
            let start = std::time::Instant::now();
            // A backend that panics must not take the thread with it: the state would keep a
            // pending frame that nobody consumes, `process` would return `Ok(None)` forever and
            // nothing would report it. The panic becomes an `Inference` error, which the caller
            // already shows, and the interval gate retries.
            let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| segmenter.matte(&frame))) {
                Ok(result) => result,
                Err(payload) => Err(MatteError::Inference(format!("the segmenter panicked: {}", panic_text(&*payload)))),
            };
            let ms = start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
            if let Ok(mut s) = shared.0.lock() {
                s.segmenter = Some(segmenter);
                match result {
                    Ok(m) => {
                        s.latest = Some(Arc::new(m));
                        s.error = None;
                    }
                    Err(e) => s.error = Some(e),
                }
                s.timings.last_ms = ms;
                s.timings.runs = s.timings.runs.saturating_add(1);
            }
        }
    }
}

/// The message a caught panic payload carries, for the error the caller sees.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> &str {
    payload.downcast_ref::<&str>().copied().or_else(|| payload.downcast_ref::<String>().map(String::as_str)).unwrap_or("no message")
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_matte::compositing::CompositeOptions;
    use deckcraft_matte::{EllipseSegmenter, MatteError};
    use deckcraft_model::CameraBackgroundMode;

    fn frame() -> RgbaFrame {
        let mut px = Vec::new();
        for y in 0..16 {
            for x in 0..16 {
                let v = ((x + y) * 8) as u8;
                px.extend_from_slice(&[v, 255 - v, 90, 255]);
            }
        }
        RgbaFrame::new(16, 16, px).expect("frame")
    }

    /// A segmenter that takes long enough for a second `process` call to land while it runs.
    struct SlowSegmenter;

    impl Segmenter for SlowSegmenter {
        fn matte(&mut self, frame: &RgbaFrame) -> Result<Matte, MatteError> {
            std::thread::sleep(Duration::from_millis(200));
            EllipseSegmenter::default().matte(frame)
        }

        fn name(&self) -> &'static str {
            "slow"
        }
    }

    /// The first submission runs on the interval like every other one: during the first inference
    /// a caller drawing at 60 Hz must not queue a second frame, and a segmenter that keeps failing
    /// must not be retried on every call.
    #[test]
    fn the_first_submission_honours_the_interval() {
        // The interval is far larger than anything this test waits for, so the assertion is about
        // the gate (a completed first inference must not start a second) and not about how fast
        // this machine is: with a 2 s interval a slow CI run could legitimately reach the next
        // submission.
        let w = MatteWorker::new(Box::new(SlowSegmenter), Duration::from_secs(3_600)).expect("worker");
        let f = frame();
        let opts = CompositeOptions::new(CameraBackgroundMode::Blur);
        for _ in 0..20 {
            let _ = w.process(&f, &opts, None, true);
            std::thread::sleep(Duration::from_millis(5));
        }
        // Wait for the first inference to land, bounded so a loaded machine only delays the
        // assertion.
        for _ in 0..400 {
            if w.timings().runs >= 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(w.timings().runs >= 1, "the first inference ran");
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(w.timings().runs, 1, "calls after the first inference must not queue another before the interval");
    }

    /// A load that fails is the worker's error, not a silent "no matte yet" forever, and it was
    /// attempted on the worker thread (the loader runs there, not in `with_loader`).
    #[test]
    fn a_failed_load_is_reported_and_not_retried() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let attempts = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&attempts);
        let w = MatteWorker::with_loader(
            Box::new(move || {
                counter.fetch_add(1, Ordering::Relaxed);
                Err(MatteError::Unavailable("the model is missing".into()))
            }),
            Duration::from_millis(0),
        )
        .expect("worker");
        assert_eq!(attempts.load(Ordering::Relaxed), 0, "the loader did not run on the caller's thread");
        let f = frame();
        let opts = CompositeOptions::new(CameraBackgroundMode::Blur);
        let mut error = None;
        for _ in 0..200 {
            match w.process(&f, &opts, None, true) {
                Err(e) => {
                    error = Some(e);
                    break;
                }
                Ok(_) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        assert!(matches!(error, Some(MatteError::Unavailable(_))), "{error:?}");
        let _ = w.process(&f, &opts, None, false);
        assert_eq!(attempts.load(Ordering::Relaxed), 1, "a failed load is not retried per call");
    }

    #[test]
    fn a_frame_is_composited_only_after_a_matte_exists() {
        let w = MatteWorker::new(Box::new(EllipseSegmenter::default()), Duration::from_millis(0)).expect("worker");
        let f = frame();
        let opts = CompositeOptions::new(CameraBackgroundMode::Removed);
        assert!(w.process(&f, &opts, None, true).expect("first").is_none(), "no matte yet");
        // Wait for the worker without polling in production code: a bounded spin in a test is
        // fine, and the worker signals completion through the shared state.
        for _ in 0..200 {
            if w.latest().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let out = w.process(&f, &opts, None, true).expect("second").expect("composited with the matte");
        assert_eq!(out.at(8, 8)[3], 255, "the subject is kept");
        let t = w.timings();
        assert!(t.runs >= 1);
        assert!(t.last_ms <= 5_000);
    }

    /// A backend that panics must be reported, not allowed to end the thread and strand the
    /// pending frame (which would make `process` return `Ok(None)` forever).
    struct PanicSegmenter;

    impl Segmenter for PanicSegmenter {
        fn matte(&mut self, _frame: &RgbaFrame) -> Result<Matte, MatteError> {
            panic!("the model exploded");
        }

        fn name(&self) -> &'static str {
            "panicking"
        }
    }

    #[test]
    fn a_panicking_segmenter_is_an_error_and_the_worker_keeps_running() {
        let w = MatteWorker::new(Box::new(PanicSegmenter), Duration::from_millis(0)).expect("worker");
        let f = frame();
        let opts = CompositeOptions::new(CameraBackgroundMode::Blur);
        let _ = w.process(&f, &opts, None, true);
        let mut error = None;
        for _ in 0..200 {
            error = w.error();
            if error.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        match &error {
            Some(MatteError::Inference(message)) => assert!(message.contains("the model exploded"), "{message}"),
            other => panic!("{other:?}"),
        }
        // The thread survived: a later submission is consumed and runs again.
        let _ = w.process(&f, &opts, None, true);
        for _ in 0..200 {
            if w.timings().runs >= 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(w.timings().runs >= 2, "the worker still runs after the panic");
    }

    #[test]
    fn an_unavailable_segmenter_reports_its_reason() {
        let w = MatteWorker::new(Box::new(deckcraft_matte::NoSegmenter::default()), Duration::from_millis(0)).expect("worker");
        let f = frame();
        // The first call returns `None` or the worker's error, depending on whether the worker has
        // already failed it: both are valid for the caller, so the assertion is on the error below.
        let _ = w.process(&f, &CompositeOptions::new(CameraBackgroundMode::Blur), None, true);
        let mut error = None;
        for _ in 0..200 {
            error = w.error();
            if error.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(error, Some(MatteError::Unavailable(_))), "{error:?}");
        assert!(w.process(&f, &CompositeOptions::new(CameraBackgroundMode::Blur), None, false).is_err());
    }
}
