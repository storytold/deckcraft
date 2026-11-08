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
    // copies the file in): re-read it here, not per frame of the canvas.
    if let Some(matte) = app.matte.as_mut() {
        matte.refresh();
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
            let usable = can_edit && (mode == CameraBackgroundMode::Normal || ready);
            let response = widgets::small_button(ui, icon, mode.label(), usable, current == mode);
            let response = if usable { response } else { response.on_hover_text(disabled_reason(&state, mode)) };
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
    if current == CameraBackgroundMode::Custom {
        group(ui, |ui| {
            if ui.button("Choose Replacement Image…").clicked() {
                choose_replacement(app);
            }
        });
    }
    group(ui, |ui| model_panel(app, ui, state));
}

/// Pick a PNG/JPEG through the host, read it off the UI thread's blocking path (the host's reader
/// is local), and send it inline so the picture and the mode are one undo step.
fn choose_replacement(app: &mut SlideApp) {
    let Some(path) = app.services.pick_open.as_mut().and_then(|pick| pick("picture")) else { return };
    let Some(read) = app.services.read.as_ref() else {
        app.set_status("This build cannot read files: the host has no file reader".to_string());
        return;
    };
    let bytes = match read(&path) {
        Ok(bytes) => bytes,
        Err(e) => {
            app.set_status(format!("Cannot read {path}: {e}"));
            return;
        }
    };
    let ids = cameo_ids(app);
    let data = deckcraft_engine::cmd::base64_encode(&bytes);
    let p = if ids.is_empty() { json!({"mode": "custom", "imageData": data}) } else { json!({"mode": "custom", "imageData": data, "ids": ids}) };
    run(app, "cameo.background", p);
}

fn current_mode(app: &SlideApp) -> CameraBackgroundMode {
    app.session
        .active()
        .and_then(|st| {
            st.selected_shapes().iter().find(|sh| sh.is_cameo()).and_then(|sh| sh.cameo.as_ref()).and_then(|c| c.background.as_ref()).map(|b| b.mode)
        })
        .unwrap_or_default()
}

fn disabled_reason(state: &Option<ModelState>, mode: CameraBackgroundMode) -> String {
    if mode == CameraBackgroundMode::Normal {
        return "Select a Cameo".into();
    }
    match state {
        Some(ModelState::Ready { .. }) => "Select a Cameo".into(),
        Some(other) => format!("{} — the background modes need the model", other.message()),
        None => "Camera backgrounds need the desktop app and its segmentation model".into(),
    }
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
        }
        ModelState::Missing { root, url, bytes, sha256, license } => {
            ui.label("Background model not installed. Camera background removal needs it.");
            ui.collapsing("Model details", |ui| {
                ui.label(format!("Licence: {license}"));
                ui.label(format!("Size: {:.1} MB", *bytes as f64 / 1_000_000.0));
                ui.label(format!("SHA-256: {sha256}"));
                ui.label(format!("Source: {url}"));
                ui.label(format!("Install directory: {root}"));
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

/// The consented download, offered wherever there is no usable model: missing, or present but
/// unusable (a failed download, a corrupt install).
fn download_button(app: &mut SlideApp, ui: &mut Ui, bytes: u64) {
    let download = ui.add_enabled(true, egui::Button::new("Download Model…"));
    if download.on_hover_text(format!("Downloads {:.1} MB after this click; nothing is fetched at startup", bytes as f64 / 1_000_000.0)).clicked() {
        match app.matte.as_mut().map(|m| m.start_download()) {
            Some(Ok(())) => app.set_status("Downloading the background model…"),
            Some(Err(e)) => app.set_status(e),
            None => {}
        }
    }
}

fn import_button(app: &mut SlideApp, ui: &mut Ui) {
    if !ui.button("Import Model…").clicked() {
        return;
    }
    let Some(path) = app.services.pick_open.as_mut().and_then(|pick| pick("model")) else { return };
    match app.matte.as_mut().map(|m| m.install(&path)) {
        Some(Ok(())) => app.set_status("Background model installed and verified"),
        Some(Err(e)) => app.set_status(format!("Cannot install the model: {e}")),
        None => {}
    }
}
