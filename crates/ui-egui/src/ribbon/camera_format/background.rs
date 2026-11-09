//! Camera Format ▸ Camera Background: the four modes and the model panel.
//!
//! The mode is a document edit (`cameo.background`, undoable and saved); segmentation needs the
//! user-installed `birefnet-lite-512` model. Without it every mode but Normal is disabled and the
//! panel names what is missing, its licence, size, hash and source. The web build has no native
//! backend, so the same panel says the desktop app is needed.
//!
//! Live capture is issue #1837: this tab sets and saves the background, and the pipeline behind it
//! is exercised by `cameo.backgroundPreview` (control channel and `deckcraft-cli
//! camera-background`) until the capture seam lands.

use deckcraft_matte::service::ModelState;
use deckcraft_model::CameraBackgroundMode;

use super::super::{Icon, SlideApp, Ui, enabled, group, json, run, widgets};
use super::cameo_ids;

pub(super) fn background_section(app: &mut SlideApp, ui: &mut Ui) {
    let current = current_mode(app);
    // The panel is the only place the model directory can change under a running app (the user
    // copies the file in). Re-read it when the tab opens and then at most once every few seconds:
    // `refresh()` probes every model root, and the ribbon redraws at animation speed.
    let due = app.camera_background_checked.is_none_or(|t| t.elapsed() >= MODEL_PROBE_INTERVAL);
    if due {
        app.camera_background_checked = Some(std::time::Instant::now());
        if let Some(matte) = app.matte.as_mut() {
            matte.refresh();
        }
    }
    let state = app.matte.as_mut().map(|m| m.state());
    let ready = state.as_ref().is_some_and(|s| s.is_ready());
    // The session loads the model by itself for `cameo.backgroundPreview` and
    // `cameo.background.state`; a model installed or removed since then must be retried, so tell
    // the session what the service above reports.
    app.session.sync_segmenter(ready);
    let can_edit = enabled(app, "cameo.background");
    group(ui, |ui| {
        for (mode, icon) in [
            (CameraBackgroundMode::Normal, Icon::Cameo),
            (CameraBackgroundMode::Removed, Icon::Eraser),
            (CameraBackgroundMode::Blur, Icon::Video),
            (CameraBackgroundMode::Custom, Icon::Picture),
        ] {
            let gate = mode_gate(can_edit, state.as_ref(), mode);
            let response = widgets::small_button(ui, icon, mode.label(), gate.is_ok(), current == Some(mode));
            let response = match &gate {
                Ok(()) => response,
                Err(reason) => response.on_hover_text(reason),
            };
            if response.clicked() {
                if mode == CameraBackgroundMode::Custom {
                    // A Custom background needs a replacement: ask for it first and send the mode
                    // and the picture as one edit, so the document never holds a Custom
                    // background with nothing behind it.
                    choose_replacement(app);
                } else {
                    let ids = cameo_ids(app);
                    let p = if ids.is_empty() { json!({"mode": mode.as_str()}) } else { json!({"mode": mode.as_str(), "ids": ids}) };
                    run(app, "cameo.background", p);
                }
            }
        }
    });
    // The replacement picker needs the same model gate as the Custom mode button: a picture with
    // no segmentation would only be stored, never shown.
    if current == Some(CameraBackgroundMode::Custom) {
        let gate = mode_gate(can_edit, state.as_ref(), CameraBackgroundMode::Custom);
        let picker = app.services.pick_open.is_some();
        group(ui, |ui| {
            let response = ui.add_enabled(gate.is_ok() && picker, egui::Button::new("Choose Replacement Image…"));
            let response = match (&gate, picker) {
                (Err(reason), _) => response.on_hover_text(reason),
                (Ok(()), false) => response.on_hover_text(NO_PICKER),
                (Ok(()), true) => response,
            };
            if response.clicked() {
                choose_replacement(app);
            }
        });
    }
    group(ui, |ui| model_panel(app, ui, state));
}

/// How often the panel re-probes the model directory while it is open.
const MODEL_PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
/// The status line when the host has no file picker: a disabled button alone would not say why.
const NO_PICKER: &str = "This build cannot pick files: the host has no file picker";

/// Whether a mode button can be pressed, and the sentence to show when it cannot. Pure, so the
/// gating is testable without a frame: every mode against every model state.
pub(super) fn mode_gate(can_edit: bool, state: Option<&ModelState>, mode: CameraBackgroundMode) -> Result<(), String> {
    if !can_edit {
        return Err("Select a Cameo".into());
    }
    if mode == CameraBackgroundMode::Normal {
        return Ok(());
    }
    match state {
        Some(state) if state.is_ready() => Ok(()),
        Some(other) => Err(format!("{} — the background modes need the model", other.message())),
        None => Err("Camera backgrounds need the desktop app and its segmentation model".into()),
    }
}

/// Pick a PNG/JPEG through the host, check its size and header before reading it, and send it
/// inline so the picture and the mode are one undo step. A file over the engine's limit is refused
/// with the same sentence the command would give, before it is read or base64-encoded.
fn choose_replacement(app: &mut SlideApp) {
    let Some(pick) = app.services.pick_open.as_mut() else {
        app.set_status(NO_PICKER.to_string());
        return;
    };
    let Some(path) = pick("picture") else { return };
    let Some(read) = app.services.read.as_ref() else {
        app.set_status("This build cannot read files: the host has no file reader".to_string());
        return;
    };
    // The metadata check is the cheap bound: a 500 MB file is refused before it is read into
    // memory, and the length is checked again after the read in case it grew.
    let limit = deckcraft_engine::cmd::cameo_background::MAX_INLINE_IMAGE_BYTES as u64;
    if let Ok(meta) = std::fs::metadata(&path)
        && meta.len() > limit
    {
        app.set_status(format!("Cannot use {path}: the picture is larger than {} MB", limit / (1024 * 1024)));
        return;
    }
    let bytes = match read(&path) {
        Ok(bytes) => bytes,
        Err(e) => {
            app.set_status(format!("Cannot read {path}: {e}"));
            return;
        }
    };
    if bytes.len() as u64 > limit {
        app.set_status(format!("Cannot use {path}: the picture is larger than {} MB", limit / (1024 * 1024)));
        return;
    }
    let png = bytes.starts_with(&[0x89, b'P', b'N', b'G']);
    let jpeg = bytes.starts_with(&[0xFF, 0xD8]);
    if !png && !jpeg {
        app.set_status(format!("Cannot use {path}: choose a PNG or JPEG picture"));
        return;
    }
    let ids = cameo_ids(app);
    let data = deckcraft_engine::cmd::base64_encode(&bytes);
    let p = if ids.is_empty() { json!({"mode": "custom", "imageData": data}) } else { json!({"mode": "custom", "imageData": data, "ids": ids}) };
    run(app, "cameo.background", p);
    app.camera_background_checked = None;
}

/// The mode every selected Cameo shares, or `None` when the selection mixes modes (or holds no
/// Cameo). A single on-state for a mixed selection would claim one Cameo's mode is the others'.
fn current_mode(app: &SlideApp) -> Option<CameraBackgroundMode> {
    let state = app.session.active()?;
    let mut shared: Option<CameraBackgroundMode> = None;
    let mut cameos = 0usize;
    for shape in state.selected_shapes().iter().filter(|sh| sh.is_cameo()) {
        cameos += 1;
        let mode = shape.cameo.as_ref().and_then(|c| c.background.as_ref()).map(|b| b.mode).unwrap_or_default();
        match shared {
            None => shared = Some(mode),
            Some(seen) if seen != mode => return None,
            Some(_) => {}
        }
    }
    (cameos > 0).then_some(shared).flatten()
}

fn model_panel(app: &mut SlideApp, ui: &mut Ui, state: Option<ModelState>) {
    let Some(state) = state else {
        ui.label("Camera backgrounds need the desktop app: this build has no segmentation model.");
        return;
    };
    match &state {
        ModelState::Ready { bytes, .. } => {
            let timings = app.matte.as_ref().map(|m| m.timings()).unwrap_or_default();
            ui.label(format!("Background model ready ({:.1} MB)", *bytes as f64 / 1_000_000.0));
            if timings.runs > 0 {
                ui.label(format!("Last matte {} ms; {} reused frames", timings.last_ms, timings.reused));
            }
        }
        ModelState::Downloading { received, total } => {
            let fraction = if *total == 0 { 0.0 } else { *received as f32 / *total as f32 };
            ui.add(egui::ProgressBar::new(fraction.clamp(0.0, 1.0)).text(state.message()));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            // A trickling transfer can be stopped; the thread fails on its next read and removes
            // its partial file, and the panel offers the download again.
            if ui.button("Cancel Download").clicked() {
                if let Some(matte) = app.matte.as_mut() {
                    matte.cancel_download();
                }
                app.camera_background_checked = None;
                app.set_status("Download cancelled".to_string());
            }
        }
        ModelState::Missing { root, url, bytes, sha256, license } => {
            ui.label("Background model not installed. Camera background removal needs it.");
            ui.collapsing("Model details", |ui| {
                // Constrained and wrapping: a bare label for a 64-character hash and a long URL
                // widened the ribbon, and an unexpectedly wide path could not be copied.
                ui.set_max_width(ui.available_width().min(360.0));
                wrapping_value(ui, &format!("Licence: {license}"));
                wrapping_value(ui, &format!("Size: {:.1} MB", *bytes as f64 / 1_000_000.0));
                wrapping_value(ui, &format!("SHA-256: {sha256}"));
                wrapping_value(ui, &format!("Source: {url}"));
                wrapping_value(ui, &format!("Install directory: {root}"));
            });
            download_button(app, ui, *bytes);
            import_button(app, ui);
        }
        ModelState::Failed { detail } => {
            ui.label(format!("Background model unusable: {detail}"));
            // A failed download leaves no file, and `start_download` accepts a retry (the thread is
            // gone by then), so the same button belongs here; so does a corrupt install, which
            // `download_to` replaces after verifying the new file.
            download_button(app, ui, deckcraft_matte::manifest::MODEL_BYTES);
            import_button(app, ui);
        }
    }
    ui.label("Live capture arrives with issue #1837; backgrounds are saved and previewable now.");
}

/// A wrapping, selectable value: the hash, URL and directory can be read and copied, and a long
/// one does not widen the ribbon.
fn wrapping_value(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).monospace()).wrap().selectable(true));
}

/// The consented download, offered wherever there is no usable model: missing, or present but
/// unusable (a failed download, a corrupt install).
fn download_button(app: &mut SlideApp, ui: &mut Ui, bytes: u64) {
    let download = ui.add_enabled(true, egui::Button::new("Download Model…"));
    if download.on_hover_text(format!("Downloads {:.1} MB after this click; nothing is fetched at startup", bytes as f64 / 1_000_000.0)).clicked() {
        match app.matte.as_mut().map(|m| m.start_download()) {
            Some(Ok(())) => {
                app.camera_background_checked = None;
                app.set_status("Downloading the background model…");
            }
            Some(Err(e)) => app.set_status(e),
            None => {}
        }
    }
}

fn import_button(app: &mut SlideApp, ui: &mut Ui) {
    let picker = app.services.pick_open.is_some();
    let button = ui.add_enabled(picker, egui::Button::new("Import Model…"));
    let button = if picker { button } else { button.on_hover_text(NO_PICKER) };
    if !button.clicked() {
        return;
    }
    let Some(path) = app.services.pick_open.as_mut().and_then(|pick| pick("model")) else { return };
    match app.matte.as_mut().map(|m| m.install(&path)) {
        Some(Ok(())) => {
            app.camera_background_checked = None;
            app.set_status("Background model installed and verified");
        }
        Some(Err(e)) => app.set_status(format!("Cannot install the model: {e}")),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states() -> Vec<(&'static str, Option<ModelState>)> {
        vec![
            ("no host", None),
            ("ready", Some(ModelState::Ready { path: "model.onnx".into(), bytes: 1 })),
            (
                "missing",
                Some(ModelState::Missing {
                    root: "models".into(),
                    url: "https://example.invalid/model.onnx".into(),
                    bytes: 1,
                    sha256: "0".repeat(64),
                    license: "MIT".into(),
                }),
            ),
            ("downloading", Some(ModelState::Downloading { received: 1, total: 2 })),
            ("failed", Some(ModelState::Failed { detail: "hash mismatch".into() })),
        ]
    }

    /// Every mode against every model state: Normal needs no model, the other three need a ready
    /// one, and nothing is enabled without an editable selection.
    #[test]
    fn the_mode_gate_covers_every_mode_and_state() {
        for (label, state) in states() {
            for mode in [CameraBackgroundMode::Normal, CameraBackgroundMode::Removed, CameraBackgroundMode::Blur, CameraBackgroundMode::Custom] {
                let gate = mode_gate(true, state.as_ref(), mode);
                let ready = state.as_ref().is_some_and(ModelState::is_ready);
                if mode == CameraBackgroundMode::Normal || ready {
                    assert!(gate.is_ok(), "{label}/{mode:?} should be usable: {gate:?}");
                } else {
                    let reason = gate.expect_err(&format!("{label}/{mode:?} needs the model"));
                    assert!(reason.contains("model") || reason.contains("desktop"), "{label}/{mode:?}: {reason}");
                }
                if let Some(state) = state.as_ref()
                    && !state.is_ready()
                {
                    assert!(mode_gate(false, Some(state), mode).is_err(), "{label}/{mode:?} without an editable selection");
                }
            }
        }
    }

    #[test]
    fn the_gate_names_the_missing_model() {
        let missing = ModelState::Missing { root: "models".into(), url: String::new(), bytes: 1, sha256: String::new(), license: "MIT".into() };
        let reason = mode_gate(true, Some(&missing), CameraBackgroundMode::Blur).expect_err("needs the model");
        assert!(reason.contains("needs the model"), "{reason}");
        let reason = mode_gate(true, None, CameraBackgroundMode::Blur).expect_err("needs the desktop app");
        assert!(reason.contains("desktop app"), "{reason}");
        assert!(mode_gate(true, None, CameraBackgroundMode::Normal).is_ok(), "Normal never needs a model");
    }
}
