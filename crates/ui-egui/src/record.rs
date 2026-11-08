//! Record ▸ Audio: capture the default microphone and insert the recording on the current slide.
//!
//! Capture itself lives behind [`deckcraft_media::MicIn`] (cpal on desktop, absent in the browser
//! and in tests without a device). This module drains that microphone on the UI clock, validates
//! the take and hands a 16-bit mono WAV to `insert.audio`, so the slide gets an ordinary audio
//! `ShapeKind::Media` and undo/PPTX need no special case.

use std::time::Duration;

use deckcraft_media::MicIn;

use crate::SlideApp;
use crate::clock::monotonic_secs;
use crate::theme;

/// A take shorter than this is almost always an accidental click, not a recording.
pub(crate) const MIN_SECONDS: f64 = 0.2;
/// Longest recording kept on one slide.
pub(crate) const MAX_SECONDS: f64 = 10.0 * 60.0;
/// Longest 16-bit mono WAV a recording may produce (64 MiB: about 58 minutes at 48 kHz).
pub(crate) const MAX_WAV_BYTES: usize = 64 * 1024 * 1024;

/// Most samples a take may keep at `rate`: the duration limit or the byte limit, whichever bites
/// first. The microphone callback and the UI drain both stop here.
pub(crate) fn max_samples(rate: u32) -> usize {
    let by_time = (rate as usize).saturating_mul(MAX_SECONDS as usize);
    // `wav_mono` writes at most ten minutes at the take's own rate; the byte cap can bite first.
    by_time.min((MAX_WAV_BYTES - 44) / 2)
}

/// Why a take cannot become a slide's audio, or `Ok` when it can.
pub(crate) fn check(rate: u32, len: usize) -> Result<(), String> {
    if rate == 0 {
        return Err("The microphone didn't record anything".into());
    }
    let duration = len as f64 / rate as f64;
    if duration < MIN_SECONDS {
        return Err(format!("A recording must be at least {MIN_SECONDS} s long"));
    }
    if len > max_samples(rate) || duration > MAX_SECONDS {
        return Err(format!("A recording can be at most {} minutes long", (MAX_SECONDS / 60.0) as u32));
    }
    let bytes = 44usize
        .checked_add(len.checked_mul(2).ok_or_else(|| "The recording is too large".to_string())?)
        .ok_or_else(|| "The recording is too large".to_string())?;
    if bytes > MAX_WAV_BYTES {
        return Err("The recording is too large".into());
    }
    Ok(())
}

/// A validated 16-bit mono WAV of `rate` and `samples`.
pub(crate) fn encode(rate: u32, samples: &[f32]) -> Result<Vec<u8>, String> {
    check(rate, samples.len())?;
    deckcraft_media::wav_mono(rate, samples)
}

/// Minutes, seconds and tenths for the record bar and status messages.
pub(crate) fn elapsed_text(seconds: f64) -> String {
    let t = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
    let minutes = (t / 60.0).floor() as u64;
    let secs = t - minutes as f64 * 60.0;
    format!("{minutes}:{secs:04.1}")
}

/// The take in progress: its rate (set by the first captured buffer) and mono samples.
struct Take {
    rate: u32,
    samples: Vec<f32>,
    started: f64,
    /// The duration or byte cap stopped further capture.
    full: bool,
    /// The presentation and slide that were in front when the take started: the recording lands
    /// there even if the user browsed elsewhere meanwhile.
    target: crate::FileTarget,
}

/// Record ▸ Audio state.
#[derive(Default)]
pub(crate) struct Recorder {
    take: Option<Take>,
}

impl Recorder {
    pub(crate) fn is_recording(&self) -> bool {
        self.take.is_some()
    }

    /// Seconds captured so far.
    pub(crate) fn elapsed(&self) -> f64 {
        match &self.take {
            Some(t) if t.rate > 0 => t.samples.len() as f64 / t.rate as f64,
            _ => 0.0,
        }
    }

    /// Whether the take reached its length cap.
    pub(crate) fn full(&self) -> bool {
        self.take.as_ref().is_some_and(|t| t.full)
    }

    /// Wall-clock time since Start, so the bar moves even while the first buffer is pending.
    pub(crate) fn started_ago(&self, now: f64) -> f64 {
        self.take.as_ref().map(|t| (now - t.started).max(0.0)).unwrap_or(0.0)
    }

    /// Open the microphone and begin a take. A failed start leaves the recorder idle.
    pub(crate) fn start(&mut self, mic: Option<&mut (dyn MicIn + '_)>, now: f64, target: crate::FileTarget) -> Result<(), String> {
        if self.is_recording() {
            return Err("A recording is already in progress".into());
        }
        let mic = mic.ok_or_else(|| "Audio recording isn't available here".to_string())?;
        mic.stop();
        mic.start().map_err(|e| format!("Couldn't start recording: {e}"))?;
        self.take = Some(Take { rate: 0, samples: Vec::new(), started: now, full: false, target });
        Ok(())
    }

    /// Drain captured samples into the take. Called every frame while recording.
    pub(crate) fn tick(&mut self, mic: Option<&mut (dyn MicIn + '_)>) {
        let Some(mic) = mic else { return };
        let Some(take) = &mut self.take else { return };
        let (rate, samples) = mic.take();
        if take.rate == 0 && rate != 0 {
            take.rate = rate;
        }
        if samples.is_empty() || take.rate == 0 || rate != 0 && rate != take.rate {
            return;
        }
        let cap = max_samples(take.rate);
        let room = cap.saturating_sub(take.samples.len());
        let n = samples.len().min(room);
        take.samples.extend_from_slice(&samples[..n]);
        if n < samples.len() || take.samples.len() >= cap {
            take.full = true;
        }
    }

    /// Stop the microphone, drain what is left and return the WAV. A failed stop keeps the take, so
    /// the caller can retry or discard it, and a rejected take keeps the microphone running: no
    /// error path here ends the recording state without the caller choosing it.
    pub(crate) fn finish(&mut self, mut mic: Option<&mut (dyn MicIn + '_)>) -> Result<(Vec<u8>, crate::FileTarget), String> {
        if let Some(mic) = mic.as_mut() {
            self.tick(Some(&mut **mic));
        }
        // The device may have gone away mid-take: say so instead of saving a truncated clip. The
        // take stays so the caller can discard it explicitly.
        if let Some(e) = mic.as_ref().and_then(|mic| mic.failure()) {
            if let Some(mic) = mic.as_mut() {
                mic.stop();
            }
            return Err(format!("The microphone stopped: {e}"));
        }
        // A rejected take keeps its microphone running: the user can record more and stop again
        // instead of being stuck with a stopped device and a frozen clock.
        if let Some(take) = &self.take
            && let Err(e) = check(take.rate, take.samples.len())
        {
            return Err(e);
        }
        let Some(take) = self.take.take() else { return Err("No recording is in progress".into()) };
        if let Some(mic) = mic {
            mic.stop();
        }
        match encode(take.rate, &take.samples) {
            Ok(wav) => Ok((wav, take.target)),
            Err(e) => {
                self.take = Some(take);
                Err(e)
            }
        }
    }

    /// Stop the microphone and throw the take away.
    pub(crate) fn cancel(&mut self, mic: Option<&mut (dyn MicIn + '_)>) {
        self.take = None;
        if let Some(mic) = mic {
            mic.stop();
        }
    }
}

impl SlideApp {
    /// Whether the host supplies a microphone (false in the browser and with `DECKCRAFT_NO_AUDIO`).
    pub(crate) fn has_mic(&self) -> bool {
        self.services.mic.is_some()
    }

    pub(crate) fn audio_recording(&self) -> bool {
        self.record.is_recording()
    }

    /// Record ▸ Audio ▸ Audio: open the microphone and show the record bar.
    pub(crate) fn start_audio_recording(&mut self) -> Result<(), String> {
        if self.session.active().is_none() {
            self.set_status("Open a presentation first");
            return Err("Open a presentation first".into());
        }
        let now = monotonic_secs();
        // The take belongs to the slide it starts on, wherever the user browses meanwhile.
        let target = self.capture_file_target();
        let r = self.record.start(self.services.mic.as_deref_mut(), now, target);
        if let Err(e) = &r {
            self.set_status(e.clone());
        }
        r
    }

    /// Drain the microphone on the UI clock (from `logic`, so a minimized window keeps taking).
    pub(crate) fn tick_audio_recording(&mut self) {
        if !self.record.is_recording() {
            return;
        }
        // The device opens asynchronously: a refused permission or a lost microphone shows up only
        // here, and the take must not keep a running clock over a stream that recorded nothing.
        if let Some(e) = self.services.mic.as_ref().and_then(|m| m.failure()) {
            self.cancel_audio_recording();
            let text = format!("The microphone stopped: {e}. The take was cancelled.");
            // The status bar expires and the File tab hides it: keep a notice up until dismissed.
            self.toast = Some(crate::toast::Toast { text: text.clone(), at: crate::now_ms(), sticky: true });
            self.set_status(text);
            return;
        }
        let mic = self.services.mic.as_deref_mut();
        self.record.tick(mic);
    }

    /// Stop the take, validate it and insert it as audio on the current slide (one undo step).
    pub(crate) fn stop_audio_recording(&mut self) -> Result<(), String> {
        let (wav, target) = match self.record.finish(self.services.mic.as_deref_mut()) {
            Ok(done) => done,
            Err(e) => {
                self.set_status(e.clone());
                return Err(e);
            }
        };
        let name = self.next_recording_name();
        let params = serde_json::json!({"name": name, "data": deckcraft_engine::cmd::base64_encode(&wav)});
        // Inserted where the take started, not where the user happens to be now. When that slide
        // or its presentation has gone the audio is still real, so keep the take on the slide in
        // view; any other failure keeps its own error.
        match self.execute_on_target(&target, "The recording", "insert.audio", &params) {
            Ok(()) => self.set_status("Recording inserted on the slide it was made for"),
            Err(e) if target_gone(&e) => {
                if let Err(again) = self.session.execute("insert.audio", &params).map(|_| ()) {
                    let text = format!("Couldn't insert the recording: {again}");
                    self.set_status(text.clone());
                    return Err(text);
                }
                self.set_status("Recording inserted on the current slide: the one it was made for is gone");
            }
            Err(e) => {
                self.set_status(format!("Couldn't insert the recording: {e}"));
                return Err(e);
            }
        }
        Ok(())
    }

    /// Cancel the take: stop the microphone and insert nothing.
    pub(crate) fn cancel_audio_recording(&mut self) {
        self.record.cancel(self.services.mic.as_deref_mut());
        self.set_status("Recording cancelled");
    }

    /// `Recording 1.wav`, `Recording 2.wav`… avoiding the names already in the presentation.
    fn next_recording_name(&self) -> String {
        let mut n = 1u32;
        loop {
            let name = format!("Recording {n}.wav");
            let taken = self.session.active().is_some_and(|d| d.doc.media.iter().any(|m| m.name == name));
            if !taken || n == u32::MAX {
                return name;
            }
            n += 1;
        }
    }
}

/// The floating record bar: elapsed time, Stop and Cancel.
pub(crate) fn bar(app: &mut SlideApp, ctx: &egui::Context) {
    if !app.record.is_recording() {
        return;
    }
    ctx.request_repaint_after(Duration::from_millis(100));
    let t = theme::Tokens::get(ctx);
    egui::Area::new(egui::Id::unique("record_audio_bar")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -56.0]).order(egui::Order::Foreground).show(
        ctx,
        |ui| {
            egui::Frame::NONE
                .fill(t.card)
                .stroke(egui::Stroke::new(1.0, t.border))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let dot = egui::RichText::new("●").color(egui::Color32::from_rgb(0xE0, 0x3B, 0x3B));
                        ui.label(dot);
                        ui.label("Recording audio");
                        let seconds = if app.record.elapsed() > 0.0 { app.record.elapsed() } else { app.record.started_ago(monotonic_secs()) };
                        ui.monospace(elapsed_text(seconds));
                        if app.record.full() {
                            ui.label("maximum length reached");
                        }
                        if ui.button("Stop").clicked() {
                            let _ = app.stop_audio_recording();
                        }
                        if ui.button("Cancel").clicked() {
                            app.cancel_audio_recording();
                        }
                    });
                });
        },
    );
}

/// `execute_on_target`'s refusals for a target that no longer exists: the presentation was closed,
/// or the slide (the place) it points at is gone. A recording that took either still has real
/// audio, so it is inserted in view rather than lost.
fn target_gone(e: &str) -> bool {
    e.contains("the presentation it was chosen for is closed") || e.contains("was not applied:")
}
