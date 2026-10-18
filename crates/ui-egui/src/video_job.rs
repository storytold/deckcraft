//! File › Export › Create a Video in the background: the export runs on a thread of its own
//! (on the desktop) while the window shows its progress and can cancel it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Receiver;

use deckcraft_engine::video::VideoOpts;
use deckcraft_model::Presentation;

/// A video export under way.
pub struct VideoJob {
    pub path: String,
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    rx: Receiver<Result<String, String>>,
}

impl VideoJob {
    /// Frames written and frames in all (0 until the plan is known).
    pub fn progress(&self) -> (u64, u64) {
        (self.done.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed))
    }
    /// Ask the export to stop; it does after the frame it is on.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn cancelling(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    /// The outcome once the export has finished: a status line, or why it failed.
    pub fn poll(&self) -> Option<Result<String, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("the video export stopped unexpectedly".into())),
        }
    }
}

/// Encode `doc` as `format` (`mp4`/`mov`) with `opts` and write it to `path`, calling `progress`
/// after each frame (it returns `false` to stop). The status line for the window.
#[cfg(not(target_arch = "wasm32"))]
pub fn export(doc: &Presentation, path: &str, format: &str, opts: &VideoOpts, progress: &mut dyn FnMut(u64, u64) -> bool) -> Result<String, String> {
    let v = deckcraft_engine::video::encode_with(doc, format, opts, progress)?;
    std::fs::write(path, &v.bytes).map_err(|e| format!("couldn't write {path}: {e}"))?;
    let secs = v.frames as f64 / f64::from(opts.fps.clamp(1, 60));
    let mut msg = format!("Exported {path} ({secs:.1} s)");
    if let Some(w) = v.warnings.first() {
        msg.push_str(&format!(" — {w}"));
    }
    Ok(msg)
}

/// Start exporting on a thread of its own.
#[cfg(not(target_arch = "wasm32"))]
pub fn start(doc: Arc<Presentation>, path: String, format: String, opts: VideoOpts) -> Result<VideoJob, String> {
    let (done, total, cancel) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)));
    let (tx, rx) = std::sync::mpsc::channel();
    let (d, t, c, p) = (done.clone(), total.clone(), cancel.clone(), path.clone());
    std::thread::Builder::new()
        .name("video-export".into())
        .spawn(move || {
            let r = export(&doc, &p, &format, &opts, &mut |n, all| {
                d.store(n, Ordering::Relaxed);
                t.store(all, Ordering::Relaxed);
                !c.load(Ordering::Relaxed)
            });
            let _ = tx.send(r);
        })
        .map_err(|e| format!("couldn't start the video export: {e}"))?;
    Ok(VideoJob { path, done, total, cancel, rx })
}
