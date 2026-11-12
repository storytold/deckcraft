//! Live captions in the slide show: the host side of `deckcraft-asr`.
//!
//! The provider owns a thread that captures the microphone and runs Whisper; this module only
//! starts and stops it, polls the events that arrived since the last frame, and keeps the text
//! the show draws. Nothing here waits for inference.
//!
//! Subtitles are per-user settings (`Prefs::subtitles`), not deck state. The modal dialog and
//! the ribbon group set them through the `show.subtitles` command, so the control channel, the
//! CLI and MCP can do the same.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread::JoinHandle;

use deckcraft_asr::caption::{self, LayoutInput};
use deckcraft_asr::{
    Accelerator, Event, ModelManifest, ModelState, ModelStatus, ProviderState, ProviderStatus, SubtitleSettings, Transcriber, model, whisper_tiny,
};
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, pos2};

use crate::SlideApp;
use crate::services::ModelDownloadFn;
use crate::theme;

/// A download in progress.
pub struct DownloadJob {
    rx: Receiver<DownloadEvent>,
    /// Set by Cancel: progress and the result are no longer delivered to the dialog.
    cancel: Arc<AtomicBool>,
    pub done: u64,
    pub total: u64,
}

enum DownloadEvent {
    Progress(u64, u64),
    Done(Result<(), String>),
}

impl DownloadJob {
    /// A cancel has been requested but the worker has not exited yet; the transfer cannot be
    /// aborted, so the job stays busy until it reports.
    pub fn is_cancelling(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// One background hash check. The cancellation flag stops the worker between blocks, and the
/// handle is joined by a reaper thread so closing the dialog never waits for 151 MB of hashing.
struct VerifyJob {
    stop: Arc<AtomicBool>,
    rx: Receiver<ModelStatus>,
    handle: Option<JoinHandle<()>>,
}

/// The subtitle host: the provider, the caption on screen and the model's state.
pub struct Subtitles {
    /// The line the show draws: the newest partial, or the last final line.
    pub caption: Option<String>,
    provider: Option<Box<dyn Transcriber>>,
    /// The settings the running provider was started with; a change restarts it, and a failure
    /// is not retried until they change again.
    applied: Option<SubtitleSettings>,
    status: ProviderStatus,
    /// The model directory in use (found, adopted by hand, or where a download goes).
    pub model_dir: PathBuf,
    /// Where a download installs: always the host's install root, never a folder the user just
    /// picked to check (a download must not write manifest names into an arbitrary folder).
    install_dir: PathBuf,
    /// The folder the user chose by hand for this session, when one was chosen.
    adopted: Option<PathBuf>,
    model: ModelStatus,
    checked: bool,
    pub download: Option<DownloadJob>,
    /// Why the last download failed; the dialog shows it until the next attempt.
    pub download_error: Option<String>,
    /// A full hash check running off the UI thread; its result replaces `model`.
    verify: Option<VerifyJob>,
    /// Why the last background hash check never ran; the dialog shows it until the next attempt.
    pub verify_error: Option<String>,
    /// The input devices, enumerated once (a device list is not free) and reused.
    microphones: Option<Vec<String>>,
    /// The layout the last frame drew, reused while its key is unchanged.
    pub caption_cache: RefCell<CaptionCache>,
}

impl Default for Subtitles {
    fn default() -> Self {
        Subtitles {
            caption: None,
            provider: None,
            applied: None,
            status: ProviderStatus::idle(),
            model_dir: PathBuf::new(),
            install_dir: PathBuf::new(),
            adopted: None,
            model: ModelStatus::default(),
            checked: false,
            download: None,
            download_error: None,
            verify: None,
            verify_error: None,
            microphones: None,
            caption_cache: RefCell::new(CaptionCache::default()),
        }
    }
}

impl Drop for Subtitles {
    fn drop(&mut self) {
        self.cancel_verification();
    }
}

impl Subtitles {
    /// The provider's last reported state.
    pub fn status(&self) -> ProviderStatus {
        self.status.clone()
    }

    /// Whether live subtitles can run: the model is installed and hash-verified (and, in the
    /// browser, never). Presence alone is not "installed" until the background check agrees.
    pub fn available(&mut self) -> bool {
        self.ensure_checked();
        self.poll_verification();
        cfg!(not(target_arch = "wasm32")) && self.model.state == ModelState::Ready && !self.is_verifying()
    }

    /// Look for the model once. Presence is cheap, but it is not proof: a present directory is
    /// hashed off the UI thread, and the state stays unverified until the result arrives.
    pub fn ensure_checked(&mut self) {
        if self.checked {
            return;
        }
        self.checked = true;
        self.locate();
        if self.model.state != ModelState::Absent {
            let dir = self.model.dir.clone().unwrap_or_else(|| self.install_dir.clone());
            self.verify_off_thread(dir);
        }
    }

    /// Look again (after a download, or the user placed files by hand). Presence first, then the
    /// full hash check off the UI thread: a present but damaged file must not report `Ready`.
    pub fn refresh(&mut self) {
        self.checked = true;
        self.locate();
        let dir = self.model.dir.clone().unwrap_or_else(|| self.install_dir.clone());
        if self.model.state != ModelState::Absent {
            self.verify_off_thread(dir);
        }
    }

    fn locate(&mut self) {
        let roots = model::default_roots();
        let manifest = whisper_tiny();
        let (status, dir) = model::discover_files(&roots, &manifest);
        self.install_dir = model::install_dir(&roots, &manifest);
        self.adopted = None;
        self.model = status;
        self.model_dir = dir;
        // A pending hash check belongs to the folder it was started for; this one replaced it.
        self.cancel_verification();
    }

    /// Use a directory the user chose by hand: any of the model files' folder. It is reported by
    /// presence at once and hashed off the UI thread, because the full check reads 151 MB. It does
    /// not become the download target: a download only ever installs under the host's root.
    pub fn adopt_dir(&mut self, dir: PathBuf) {
        self.adopted = Some(dir.clone());
        self.verify_off_thread(dir);
    }

    /// The folder the user chose by hand this session, if any. The dialog labels it as
    /// session-only: it is not searched for again.
    pub fn adopted_dir(&self) -> Option<&std::path::Path> {
        self.adopted.as_deref()
    }

    /// Check Again: re-check the folder in use (including one chosen by hand) without silently
    /// falling back to the searched roots.
    pub fn recheck(&mut self) {
        self.ensure_checked();
        let dir = if self.model_dir.as_os_str().is_empty() { self.install_dir.clone() } else { self.model_dir.clone() };
        self.verify_off_thread(dir);
    }

    /// Report `dir` by presence now and run the full hash check on a worker thread; the result
    /// replaces [`Self::model`] when it arrives.
    fn verify_off_thread(&mut self, dir: PathBuf) {
        self.cancel_verification();
        self.model = model::check_dir_files(&dir, &whisper_tiny());
        self.model_dir = dir.clone();
        self.checked = true;
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let spawned = std::thread::Builder::new().name("deckcraft-model-check".into()).spawn(move || {
            if let Some(status) = model::check_dir_until(&dir, &whisper_tiny(), &worker_stop) {
                let _ = tx.send(status);
            }
        });
        match spawned {
            Ok(handle) => {
                self.verify = Some(VerifyJob { stop, rx, handle: Some(handle) });
                self.verify_error = None;
            }
            Err(_) => self.verify_unavailable(),
        }
    }

    /// No thread was available for the background hash check: keep the presence-only state
    /// and report the failure instead of hashing 151 MB on the UI thread. The next check
    /// (Check Again, another download, another folder) retries off the UI thread.
    pub(crate) fn verify_unavailable(&mut self) {
        self.verify_error = Some("could not start the background check; the model state is presence-only".into());
        self.verify = None;
    }

    /// Whether a background hash check is still running (a repaint is due when it finishes).
    pub fn is_verifying(&self) -> bool {
        self.verify.is_some()
    }

    /// Stop the running check, if any. The worker sees the flag between hash blocks; its handle
    /// is joined by a reaper thread so the UI thread never waits for the hash.
    fn cancel_verification(&mut self) {
        let Some(job) = self.verify.take() else { return };
        job.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = job.handle {
            let _ = std::thread::Builder::new().name("deckcraft-model-check-reaper".into()).spawn(move || {
                let _ = handle.join();
            });
        }
    }

    /// Take the background check's result, if it has finished.
    fn poll_verification(&mut self) {
        let Some(mut job) = self.verify.take() else { return };
        match job.rx.try_recv() {
            Ok(status) => {
                self.model = status;
                self.verify_error = None;
                // A start that failed for a missing or damaged model is retried once the check
                // says the model is now usable. A healthy running stream is not restarted: the
                // check finishing must not drop the audio and reload the model mid-show.
                if self.provider.is_none() {
                    self.applied = None;
                }
                if let Some(handle) = job.handle.take() {
                    let _ = handle.join();
                }
            }
            Err(TryRecvError::Empty) => self.verify = Some(job),
            Err(TryRecvError::Disconnected) => {
                if let Some(handle) = job.handle.take() {
                    let _ = handle.join();
                }
            }
        }
    }

    /// The input devices, enumerated once and cached: enumerating audio devices is not free and
    /// the dialog asks every frame it is open.
    pub fn microphones(&mut self) -> &[String] {
        self.microphones.get_or_insert_with(|| {
            #[cfg(not(target_arch = "wasm32"))]
            {
                deckcraft_asr_candle::WhisperTranscriber::microphones()
            }
            #[cfg(target_arch = "wasm32")]
            {
                Vec::new()
            }
        })
    }

    /// The accelerators this build can use here (the CPU alone until an accelerator backend
    /// ships); the dialog lists every one so the fallback note can be reached.
    pub fn accelerators(&self) -> Vec<Accelerator> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            deckcraft_asr_candle::capabilities(Accelerator::Cpu).available
        }
        #[cfg(target_arch = "wasm32")]
        {
            vec![Accelerator::Cpu]
        }
    }

    /// Why the running backend is not on the requested accelerator, when it is not.
    pub fn runtime_note(&self) -> Option<String> {
        self.status.capabilities.as_ref().and_then(|c| c.note.clone())
    }

    /// The model's state, as the dialog shows it.
    pub fn model(&mut self) -> &ModelStatus {
        self.ensure_checked();
        self.poll_verification();
        &self.model
    }

    /// The manifest, for the dialog's licence, sizes and hashes.
    pub fn manifest(&self) -> ModelManifest {
        whisper_tiny()
    }

    /// Start downloading the model through the host's hook. The target is always the host's
    /// install root, never a folder the user picked just to check: a download must not write
    /// manifest names into an arbitrary directory.
    pub fn begin_download(&mut self, hook: ModelDownloadFn) {
        if self.download.is_some() {
            return;
        }
        self.ensure_checked();
        let manifest = whisper_tiny();
        let dir = if self.install_dir.as_os_str().is_empty() { self.model_dir.clone() } else { self.install_dir.clone() };
        let (tx, rx) = channel::<DownloadEvent>();
        let progress = tx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let report_cancel = cancel.clone();
        let mut report = move |done: u64, total: u64| {
            if !report_cancel.load(Ordering::Relaxed) {
                let _ = progress.send(DownloadEvent::Progress(done, total));
            }
        };
        let spawned = std::thread::Builder::new().name("deckcraft-model-download".into()).spawn(move || {
            let result = hook(&manifest, &dir, &mut report);
            // The completion signal is sent even after a cancel: the job stays busy until this
            // worker has exited, so a retry cannot share the same `.part` path with it.
            let _ = tx.send(DownloadEvent::Done(result));
        });
        match spawned {
            Ok(_) => {
                self.download_error = None;
                self.download = Some(DownloadJob { rx, cancel, done: 0, total: whisper_tiny().total_bytes() });
            }
            Err(e) => self.download_error = Some(format!("cannot start the download: {e}")),
        }
    }

    /// Ask the download to stop. The host's transfer is not interrupted (its hook has no cancel
    /// seam), so the job stays in `download` until the worker exits and reports; the dialog shows
    /// it as cancelling and Download stays disabled until then.
    pub fn cancel_download(&mut self) {
        if let Some(job) = &self.download {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Install a provider as the start path would: `start` runs, the applied settings are
    /// recorded, and the status is the provider's own.
    #[cfg(test)]
    pub(crate) fn install(&mut self, mut provider: Box<dyn Transcriber>, settings: &SubtitleSettings) {
        let _ = provider.start();
        self.status = provider.status();
        self.provider = Some(provider);
        self.applied = Some(settings.clone());
    }

    #[cfg(test)]
    pub(crate) fn set_model_dir(&mut self, dir: PathBuf) {
        self.model_dir = dir;
        self.checked = true;
    }

    #[cfg(test)]
    pub(crate) fn is_running(&self) -> bool {
        self.provider.is_some()
    }

    /// The status as it stands without taking a finished background check, so a test can see the
    /// presence-only state before the worker reports.
    #[cfg(test)]
    pub(crate) fn presence_state(&self) -> ModelState {
        self.model.state
    }

    fn poll_download(&mut self) {
        let mut finished: Option<(bool, Result<(), String>)> = None;
        if let Some(job) = &mut self.download {
            loop {
                match job.rx.try_recv() {
                    Ok(DownloadEvent::Progress(done, total)) => {
                        job.done = done;
                        job.total = total.max(job.total);
                    }
                    Ok(DownloadEvent::Done(result)) => {
                        finished = Some((job.cancel.load(Ordering::Relaxed), result));
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished = Some((job.cancel.load(Ordering::Relaxed), Err("the download stopped before it finished".into())));
                        break;
                    }
                }
            }
        }
        if let Some((cancelled, result)) = finished {
            // The worker has exited: only now is the job no longer busy, so a retry cannot share
            // its `.part` path with it.
            self.download = None;
            if cancelled {
                // A cancelled attempt is not verified, is not an error, and does not restart a
                // stream.
                return;
            }
            match result {
                Ok(()) => {
                    // The downloader verified each file; hash again to be sure nothing else
                    // touched the directory. That reads 151 MB, so it runs off the UI thread.
                    let dir = if self.install_dir.as_os_str().is_empty() { self.model_dir.clone() } else { self.install_dir.clone() };
                    // A start that failed for the missing model is retried with the same show;
                    // a running stream is not restarted for it.
                    if self.provider.is_none() {
                        self.applied = None;
                    }
                    self.verify_off_thread(dir);
                }
                Err(e) => self.download_error = Some(e),
            }
        }
    }

    /// Stop the provider (the show ended, or subtitles were turned off). Idempotent, and it does
    /// not block: the provider hands the join of its worker to a background thread.
    pub fn stop(&mut self) {
        if let Some(mut provider) = self.provider.take() {
            provider.stop();
        }
        // The host reports the provider's state to the engine: leaving it `Running` after the
        // show ends would tell every caller captions are still live. A failure stays visible.
        if self.status.state.is_active() {
            self.status = ProviderStatus::stopped();
        }
    }

    /// Reconcile the provider with the settings and the show, and take the events that arrived.
    /// Returns a message to show once (a failure), if any.
    pub fn sync(&mut self, settings: &SubtitleSettings, show_active: bool) -> Option<String> {
        self.poll_download();
        self.poll_verification();
        if !(settings.on && show_active) {
            self.stop();
            self.caption = None;
            // The stream is not wanted. Forget what it was started with, so the next show
            // starts from the settings then in force, and a failed start is retried there
            // rather than every frame of this one.
            self.applied = None;
            if !settings.on {
                if self.status.state != ProviderState::Idle {
                    self.status = ProviderStatus::idle();
                }
            } else {
                self.status = ProviderStatus::stopped();
            }
            return None;
        }
        // Start when there is no provider, and restart when the settings changed while one ran:
        // `applied` is set before starting, so a failure is not retried until they change again.
        let applied = self.applied.as_ref().is_some_and(|applied| applied.provider_equivalent(settings));
        if !applied {
            self.stop();
            // The line belongs to the old stream (and possibly the old language); do not leave it
            // on screen looking live while the replacement starts or when it fails to.
            self.caption = None;
            self.applied = Some(settings.clone());
            if let Some(message) = self.start(settings) {
                return Some(message);
            }
        }
        let mut message = None;
        let events = match &mut self.provider {
            Some(provider) => provider.poll(),
            None => vec![],
        };
        for event in events {
            match event {
                Event::Partial { text } | Event::Final { text } => self.caption = Some(text),
                Event::Error { message: why } => {
                    self.stop();
                    self.caption = None;
                    self.status = ProviderStatus::failed(why.clone());
                    message = Some(format!("Subtitles stopped: {why}"));
                    break;
                }
                Event::Stopped => {
                    // The device went away: the last line must not stay on screen looking live.
                    self.stop();
                    self.caption = None;
                    self.status = ProviderStatus::stopped();
                }
            }
        }
        if let Some(provider) = &self.provider {
            self.status = provider.status();
        }
        message
    }

    fn start(&mut self, settings: &SubtitleSettings) -> Option<String> {
        self.ensure_checked();
        match self.start_native(settings) {
            Ok(()) => None,
            Err(reason) => {
                let message = format!("Subtitles unavailable: {reason}");
                self.status = ProviderStatus::failed(reason);
                Some(message)
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_native(&mut self, settings: &SubtitleSettings) -> Result<(), String> {
        let manifest = whisper_tiny();
        let found = model::check_dir_files(&self.model_dir, &manifest);
        if found.state != ModelState::Ready {
            return Err(format!("{} ({})", found.summary(), self.model_dir.display()));
        }
        let mut provider = deckcraft_asr_candle::WhisperTranscriber::new(self.model_dir.clone(), settings.clone());
        provider.start().map_err(|e| e.message().to_string())?;
        self.status = provider.status();
        self.provider = Some(Box::new(provider));
        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    fn start_native(&mut self, _settings: &SubtitleSettings) -> Result<(), String> {
        Err("live subtitles are not available in the browser".to_string())
    }
}

impl SlideApp {
    /// Keep the provider in step with the settings and the show, and hand its status to the
    /// engine. Called once per frame, before the show draws.
    pub(crate) fn sync_subtitles(&mut self, ctx: &egui::Context) {
        let settings = self.session.prefs.subtitles.clone();
        // A preview from the Transitions or Animations tab is not a presentation: it must not
        // open the microphone or start the model.
        let show_active = self.show.as_ref().is_some_and(|show| !show.preview_only);
        let message = self.subtitles.sync(&settings, show_active);
        if let Some(text) = message {
            self.set_status(text);
        }
        let status = self.subtitles.status();
        if status.state.is_active() || self.subtitles.download.is_some() || self.subtitles.is_verifying() {
            // Partial lines, download progress and the hash result must appear without waiting for
            // the pointer.
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
        self.session.subtitle_status = status;
    }
}

/// The caption text the show may draw now: nothing when captions are off, the show ended, the
/// grid or a blank screen is up, or the line is blank.
pub fn caption_line(on: bool, ended: bool, grid: bool, blank: bool, caption: Option<&str>) -> Option<&str> {
    if !on || ended || grid || blank {
        return None;
    }
    caption.map(str::trim).filter(|t| !t.is_empty())
}

/// The cache key: everything that changes the layout. Text and sizes are compared, not the
/// transcript's identity, so an unchanged frame reuses the wrapped lines.
#[derive(Clone, PartialEq, Eq)]
struct CaptionKey {
    text: String,
    font_bits: u64,
    max_lines: usize,
    position: deckcraft_asr::Position,
    screen: [u64; 4],
    slide: [u64; 4],
}

fn layout_bits(r: caption::Rect) -> [u64; 4] {
    [r.x.to_bits(), r.y.to_bits(), r.w.to_bits(), r.h.to_bits()]
}

/// The last laid-out caption, reused while [`CaptionKey`] is unchanged.
#[derive(Default)]
pub struct CaptionCache {
    key: Option<CaptionKey>,
    layout: Option<caption::CaptionLayout>,
}

impl CaptionCache {
    pub(crate) fn layout(&mut self, input: &LayoutInput) -> &caption::CaptionLayout {
        let key = CaptionKey {
            text: input.text.to_string(),
            font_bits: input.font_pt.to_bits(),
            max_lines: input.max_lines,
            position: input.position,
            screen: layout_bits(input.screen),
            slide: layout_bits(input.slide),
        };
        if self.key.as_ref() != Some(&key) {
            self.layout = Some(caption::layout(input));
            self.key = Some(key);
        }
        // The key was just set (or already matched), so the layout exists.
        self.layout.get_or_insert_with(|| caption::layout(input))
    }
}

/// How many laid-out lines fit the capped box: the layout caps the box to the screen, so a large
/// text size or a short screen can hold fewer lines than were wrapped.
pub(crate) fn visible_lines(laid: &caption::CaptionLayout) -> usize {
    if laid.line_height <= 0.0 {
        return 0;
    }
    ((laid.rect.h / laid.line_height).floor().max(0.0) as usize).min(laid.lines.len())
}

/// Draw the caption over the show. Called after the slide (and its media controls) so the text
/// sits above both; pen strokes and bars are drawn later and stay on top.
pub fn draw(painter: &egui::Painter, screen: Rect, slide: Rect, settings: &SubtitleSettings, caption: Option<&str>, cache: &RefCell<CaptionCache>) {
    let Some(text) = caption.map(str::trim).filter(|t| !t.is_empty()) else { return };
    let input = LayoutInput {
        screen: to_points(screen),
        slide: to_points(slide),
        position: settings.position,
        text,
        font_pt: settings.font_pt,
        max_lines: settings.max_lines,
        margin: caption::DEFAULT_MARGIN,
        width_fraction: caption::DEFAULT_WIDTH_FRACTION,
    };
    let mut cache = cache.borrow_mut();
    let laid = cache.layout(&input);
    if laid.is_empty() {
        return;
    }
    let fit = visible_lines(laid);
    if fit == 0 {
        return;
    }
    let line_height = laid.line_height as f32;
    let height = fit as f32 * line_height;
    let rect = Rect::from_min_size(pos2(laid.rect.x as f32, laid.rect.y as f32), egui::vec2(laid.rect.w as f32, laid.rect.h as f32));
    // Keep the box anchored where the layout put it: the bottom positions grow upwards, the top
    // ones downwards. Without this, hiding the lines that do not fit would move the text off the
    // edge the presenter expects.
    let top = match settings.position {
        deckcraft_asr::Position::BottomOverlay | deckcraft_asr::Position::BelowSlide => rect.bottom() - height,
        deckcraft_asr::Position::TopOverlay | deckcraft_asr::Position::AboveSlide => rect.top(),
    };
    let bg = Rect::from_min_size(pos2(rect.left(), top), egui::vec2(rect.width(), height)).intersect(screen);
    painter.rect_filled(bg, CornerRadius::same(6), Color32::from_black_alpha(150));
    // Horizontal padding: the text never touches the background's edge, and it is clipped to the
    // box, so a wrapped line cannot spill past it.
    const PAD: f32 = 8.0;
    let text_area =
        if bg.width() > 2.0 * PAD { Rect::from_min_max(pos2(bg.left() + PAD, bg.top()), pos2(bg.right() - PAD, bg.bottom())) } else { bg };
    let text_painter = painter.with_clip_rect(text_area);
    for (i, line) in laid.lines.iter().take(fit).enumerate() {
        let center: Pos2 = pos2(bg.center().x, bg.min.y + (i as f32 + 0.5) * line_height);
        text_painter.text(center, Align2::CENTER_CENTER, line, theme::font(laid.font_pt as f32), Color32::WHITE);
    }
}

fn to_points(r: Rect) -> caption::Rect {
    caption::Rect::new(f64::from(r.min.x), f64::from(r.min.y), f64::from(r.width()), f64::from(r.height()))
}
