//! Live captions in the slide show: the host side of `deckcraft-asr`.
//!
//! The provider owns a thread that captures the microphone and runs Whisper; this module only
//! starts and stops it, polls the events that arrived since the last frame, and keeps the text
//! the show draws. Nothing here waits for inference.
//!
//! Subtitles are per-user settings (`Prefs::subtitles`), not deck state. The modal dialog and
//! the ribbon group set them through the `show.subtitles` command, so the control channel, the
//! CLI and MCP can do the same.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

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
    pub done: u64,
    pub total: u64,
}

enum DownloadEvent {
    Progress(u64, u64),
    Done(Result<(), String>),
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
    /// The model directory in use (found, or where a download goes).
    pub model_dir: PathBuf,
    model: ModelStatus,
    checked: bool,
    pub download: Option<DownloadJob>,
    /// Why the last download failed; the dialog shows it until the next attempt.
    pub download_error: Option<String>,
}

impl Default for Subtitles {
    fn default() -> Self {
        Subtitles {
            caption: None,
            provider: None,
            applied: None,
            status: ProviderStatus::idle(),
            model_dir: PathBuf::new(),
            model: ModelStatus::default(),
            checked: false,
            download: None,
            download_error: None,
        }
    }
}

impl Subtitles {
    /// The provider's last reported state.
    pub fn status(&self) -> ProviderStatus {
        self.status.clone()
    }

    /// Whether live subtitles can run: the model is installed (and, in the browser, never).
    pub fn available(&mut self) -> bool {
        self.ensure_checked();
        cfg!(not(target_arch = "wasm32")) && self.model.state == ModelState::Ready
    }

    /// Look for the model once. Presence and size only, so this is cheap enough for a frame.
    pub fn ensure_checked(&mut self) {
        if self.checked {
            return;
        }
        self.checked = true;
        self.locate();
    }

    /// Look again (after a download, or the user placed files by hand).
    pub fn refresh(&mut self) {
        self.checked = true;
        self.locate();
    }

    fn locate(&mut self) {
        let (status, dir) = model::discover_files(&model::default_roots(), &whisper_tiny());
        self.model = status;
        self.model_dir = dir;
    }

    /// Use a directory the user chose by hand: any of the model files' folder.
    pub fn adopt_dir(&mut self, dir: PathBuf) {
        self.model = model::check_dir(&dir, &whisper_tiny());
        self.model_dir = dir;
        self.checked = true;
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
        &self.model
    }

    /// The manifest, for the dialog's licence, sizes and hashes.
    pub fn manifest(&self) -> ModelManifest {
        whisper_tiny()
    }

    /// Start downloading the model through the host's hook.
    pub fn begin_download(&mut self, hook: ModelDownloadFn) {
        if self.download.is_some() {
            return;
        }
        self.ensure_checked();
        let manifest = whisper_tiny();
        let dir = self.model_dir.clone();
        let (tx, rx) = channel::<DownloadEvent>();
        let progress = tx.clone();
        let mut report = move |done: u64, total: u64| {
            let _ = progress.send(DownloadEvent::Progress(done, total));
        };
        let spawned = std::thread::Builder::new().name("deckcraft-model-download".into()).spawn(move || {
            let result = hook(&manifest, &dir, &mut report);
            let _ = tx.send(DownloadEvent::Done(result));
        });
        match spawned {
            Ok(_) => {
                self.download_error = None;
                self.download = Some(DownloadJob { rx, done: 0, total: whisper_tiny().total_bytes() });
            }
            Err(e) => self.download_error = Some(format!("cannot start the download: {e}")),
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

    fn poll_download(&mut self) {
        let mut finished: Option<Result<(), String>> = None;
        if let Some(job) = &mut self.download {
            loop {
                match job.rx.try_recv() {
                    Ok(DownloadEvent::Progress(done, total)) => {
                        job.done = done;
                        job.total = total.max(job.total);
                    }
                    Ok(DownloadEvent::Done(result)) => {
                        finished = Some(result);
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished = Some(Err("the download stopped before it finished".into()));
                        break;
                    }
                }
            }
        }
        if let Some(result) = finished {
            self.download = None;
            match result {
                Ok(()) => {
                    // The downloader verified each file; hash again to be sure nothing else
                    // touched the directory, then look for the model.
                    self.model = model::check_dir(&self.model_dir, &whisper_tiny());
                    self.checked = true;
                }
                Err(e) => self.download_error = Some(e),
            }
        }
    }

    /// Stop the provider (the show ended, or subtitles were turned off). Idempotent.
    pub fn stop(&mut self) {
        if let Some(mut provider) = self.provider.take() {
            provider.stop();
        }
    }

    /// Reconcile the provider with the settings and the show, and take the events that arrived.
    /// Returns a message to show once (a failure), if any.
    pub fn sync(&mut self, settings: &SubtitleSettings, show_active: bool) -> Option<String> {
        self.poll_download();
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
                    self.stop();
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
        let show_active = self.show.is_some();
        let message = self.subtitles.sync(&settings, show_active);
        if let Some(text) = message {
            self.set_status(text);
        }
        let status = self.subtitles.status();
        if status.state.is_active() || self.subtitles.download.is_some() {
            // Partial lines and download progress must appear without waiting for the pointer.
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
        self.session.subtitle_status = status;
    }
}

/// Draw the caption over the show. Called after the slide (and its media controls) so the text
/// sits above both; pen strokes and bars are drawn later and stay on top.
pub fn draw(painter: &egui::Painter, screen: Rect, slide: Rect, settings: &SubtitleSettings, caption: Option<&str>) {
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
    let laid = caption::layout(&input);
    if laid.is_empty() {
        return;
    }
    let rect = Rect::from_min_size(pos2(laid.rect.x as f32, laid.rect.y as f32), egui::vec2(laid.rect.w as f32, laid.rect.h as f32));
    painter.rect_filled(rect, CornerRadius::same(6), Color32::from_black_alpha(150));
    for (i, line) in laid.lines.iter().enumerate() {
        let center: Pos2 = pos2(rect.center().x, rect.min.y + (i as f32 + 0.5) * laid.line_height as f32);
        painter.text(center, Align2::CENTER_CENTER, line, theme::font(laid.font_pt as f32), Color32::WHITE);
    }
}

fn to_points(r: Rect) -> caption::Rect {
    caption::Rect::new(f64::from(r.min.x), f64::from(r.min.y), f64::from(r.width()), f64::from(r.height()))
}
