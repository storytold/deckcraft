//! One inference worker, one pending frame, one last matte.
//!
//! The model takes about two seconds per frame on CPU. The UI draws at 60 Hz. The worker therefore
//! owns the only thread that runs inference, keeps at most one waiting frame, and never blocks the
//! caller: [`MatteWorker::process`] composites with the latest matte and asks for a new one on the
//! caller's cadence. Reused frames are counted so the panel and the docs can be honest.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use deckcraft_matte::compositing::CompositeOptions;
use deckcraft_matte::frame::RgbaFrame;
use deckcraft_matte::{Matte, MatteError, Segmenter, Timings};

struct State {
    segmenter: Option<Box<dyn Segmenter>>,
    pending: Option<RgbaFrame>,
    latest: Option<Matte>,
    error: Option<MatteError>,
    timings: Timings,
    last_submit: Option<std::time::Instant>,
    stop: bool,
}

/// Runs a [`Segmenter`] on its own thread. Drop asks it to stop and detaches: the thread exits at
/// its next check, after the inference it is running, and a new worker may overlap it briefly.
pub struct MatteWorker {
    shared: Arc<(Mutex<State>, Condvar)>,
    /// Minimum time between submissions.
    interval: Duration,
}

impl MatteWorker {
    pub fn new(segmenter: Box<dyn Segmenter>, interval: Duration) -> Self {
        let shared = Arc::new((
            Mutex::new(State {
                segmenter: Some(segmenter),
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
        let _ = std::thread::Builder::new().name("deckcraft-matte".into()).spawn(move || run(thread_shared));
        MatteWorker { shared, interval }
    }

    /// Offer a frame. At most one waits; a newer frame replaces an older one.
    pub fn submit(&self, frame: RgbaFrame) {
        if let Ok(mut s) = self.shared.0.lock() {
            s.pending = Some(frame);
            s.last_submit = Some(std::time::Instant::now());
            self.shared.1.notify_one();
        }
    }

    /// The last completed matte, if any.
    pub fn latest(&self) -> Option<Matte> {
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
            let out = deckcraft_matte::compositing::composite(frame, Some(&m), options, replacement)?;
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
            let result = segmenter.matte(&frame);
            let ms = start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
            if let Ok(mut s) = shared.0.lock() {
                s.segmenter = Some(segmenter);
                match result {
                    Ok(m) => {
                        s.latest = Some(m);
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
        let w = MatteWorker::new(Box::new(SlowSegmenter), Duration::from_millis(2_000));
        let f = frame();
        let opts = CompositeOptions::new(CameraBackgroundMode::Blur);
        for _ in 0..20 {
            let _ = w.process(&f, &opts, None, true);
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(w.timings().runs, 1, "100 ms of calls while the first inference ran must not queue another");
    }

    #[test]
    fn a_frame_is_composited_only_after_a_matte_exists() {
        let w = MatteWorker::new(Box::new(EllipseSegmenter::default()), Duration::from_millis(0));
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

    #[test]
    fn an_unavailable_segmenter_reports_its_reason() {
        let w = MatteWorker::new(Box::new(deckcraft_matte::NoSegmenter::default()), Duration::from_millis(0));
        let f = frame();
        assert!(w.process(&f, &CompositeOptions::new(CameraBackgroundMode::Blur), None, true).expect("first").is_none());
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
