//! Subtitle Settings: spoken and subtitle language, microphone, position, text size, and the
//! Whisper model (source, size, licence, hashes, download and manual import).

use super::{Dialog, SlideApp, Ui, buttons, json, theme};
use deckcraft_asr::caption::{MAX_FONT_PT, MAX_LINES_CAP, MIN_FONT_PT};
use deckcraft_asr::settings::{SPOKEN_LANGUAGES, SUBTITLE_LANGUAGES};
use deckcraft_asr::{Accelerator, ModelState, Position, SubtitleSettings};

/// Where the settings the dialog opened with are kept, so Cancel can put them back.
const BASELINE: &str = "subtitles.baseline";

/// The languages menu label for a tag; an unknown tag shows as itself.
fn language_label(list: &[(&str, &str)], tag: &str) -> String {
    list.iter().find(|(t, _)| *t == tag).map(|(_, l)| (*l).to_string()).unwrap_or_else(|| tag.to_string())
}

fn size_label(bytes: u64) -> String {
    if bytes >= 1_000_000 { format!("{:.1} MB", bytes as f64 / 1_000_000.0) } else { format!("{bytes} bytes") }
}

/// A value the user may want to copy (a hash, a revision): full, wrapping and selectable.
fn selectable(ui: &mut Ui, text: impl Into<String>) {
    ui.add(egui::Label::new(text.into()).selectable(true));
}

/// The Subtitle Settings dialog. Changes apply as they are made (through `show.subtitles`), so
/// the show and the status line follow without an OK step; Cancel restores the settings the
/// dialog opened with, and Close keeps them.
pub(super) fn subtitles_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let current = app.session.prefs.subtitles.clone();
    if !d.fields.contains_key(BASELINE) {
        let baseline = serde_json::to_string(&current).unwrap_or_default();
        d.fields.insert(BASELINE.to_string(), baseline);
    }
    let mut next = current.clone();
    let mut changed = false;

    ui.label(egui::RichText::new("Captions").font(theme::bold(13.0)));
    changed |= ui.checkbox(&mut next.on, "Always use subtitles in the slide show").changed();

    ui.horizontal(|ui| {
        ui.label("Spoken language:");
        craft_ui::ComboBox::from_id_salt("subs_spoken").selected_text(language_label(SPOKEN_LANGUAGES, &next.spoken)).show_ui(ui, |ui| {
            for (tag, label) in SPOKEN_LANGUAGES {
                if ui.selectable_label(next.spoken == *tag, *label).clicked() {
                    next.spoken = (*tag).to_string();
                    changed = true;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Subtitle language:");
        // Translation is not implemented, so the choice has no effect yet: showing it enabled
        // would promise a behaviour the build does not have.
        ui.add_enabled_ui(false, |ui| {
            craft_ui::ComboBox::from_id_salt("subs_subtitle").selected_text(language_label(SUBTITLE_LANGUAGES, &next.subtitle)).show_ui(ui, |ui| {
                for (tag, label) in SUBTITLE_LANGUAGES {
                    if ui.selectable_label(next.subtitle == *tag, *label).clicked() {
                        next.subtitle = (*tag).to_string();
                        changed = true;
                    }
                }
            });
        });
    });
    ui.label(egui::RichText::new("Captions are shown in the spoken language; translating them is not implemented yet.").weak());

    let devices = app.subtitles.microphones().to_vec();
    ui.horizontal(|ui| {
        ui.label("Microphone:");
        let selected = match &next.microphone {
            None => "System default".to_string(),
            Some(name) if devices.iter().any(|d| d == name) => name.clone(),
            Some(name) => format!("{name} (not available)"),
        };
        craft_ui::ComboBox::from_id_salt("subs_mic").selected_text(selected).show_ui(ui, |ui| {
            if ui.selectable_label(next.microphone.is_none(), "System default").clicked() {
                next.microphone = None;
                changed = true;
            }
            if devices.is_empty() {
                // The selector stays visible with an explanation rather than vanishing.
                ui.add_enabled(false, egui::Label::new("No microphones found"));
            }
            for name in &devices {
                if ui.selectable_label(next.microphone.as_deref() == Some(name.as_str()), name).clicked() {
                    next.microphone = Some(name.clone());
                    changed = true;
                }
            }
        });
    });
    if let Some(name) = &next.microphone
        && !devices.iter().any(|d| d == name)
    {
        let warn = theme::Tokens::get(ui.ctx()).warning;
        ui.label(egui::RichText::new(format!("`{name}` is not available; choose System default or connect it.")).color(warn));
    }
    ui.horizontal(|ui| {
        ui.label("Position:");
        craft_ui::ComboBox::from_id_salt("subs_position").selected_text(next.position.label()).show_ui(ui, |ui| {
            for p in Position::ALL {
                if ui.selectable_label(next.position == p, p.label()).clicked() {
                    next.position = p;
                    changed = true;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Processor:");
        craft_ui::ComboBox::from_id_salt("subs_accelerator").selected_text(next.accelerator.label()).show_ui(ui, |ui| {
            for a in Accelerator::ALL {
                if ui.selectable_label(next.accelerator == a, a.label()).clicked() {
                    next.accelerator = a;
                    changed = true;
                }
            }
        });
    });
    let tokens = theme::Tokens::get(ui.ctx());
    if let Some(note) = app.subtitles.runtime_note() {
        ui.label(egui::RichText::new(note).color(tokens.warning));
    } else if !app.subtitles.accelerators().contains(&next.accelerator) {
        ui.label(egui::RichText::new(format!("runs on CPU; {} not available", next.accelerator.label())).color(tokens.warning));
    }
    ui.horizontal(|ui| {
        changed |= ui.add(egui::Slider::new(&mut next.font_pt, MIN_FONT_PT..=MAX_FONT_PT).text("Text size (pt)")).changed();
        ui.label("  Lines:");
        craft_ui::ComboBox::from_id_salt("subs_lines").selected_text(next.max_lines.to_string()).show_ui(ui, |ui| {
            for n in 1..=MAX_LINES_CAP {
                if ui.selectable_label(next.max_lines == n, n.to_string()).clicked() {
                    next.max_lines = n;
                    changed = true;
                }
            }
        });
    });

    if changed {
        apply(app, &next);
    }

    ui.separator();
    model_block(app, ui);

    let (ok, cancel) = buttons(ui, "Close");
    if cancel {
        // Cancel undoes what this dialog applied: the user asked for it explicitly.
        if let Some(baseline) = d.fields.get(BASELINE).and_then(|raw| serde_json::from_str::<SubtitleSettings>(raw).ok()) {
            apply(app, &baseline);
        }
    }
    if ok || cancel {
        d.fields.remove(BASELINE);
    }
    ok || cancel
}

fn apply(app: &mut SlideApp, settings: &SubtitleSettings) {
    let p = json!({
        "on": settings.on,
        "spoken": settings.spoken,
        "subtitle": settings.subtitle,
        "position": settings.position.as_str(),
        "microphone": settings.microphone,
        "fontSizePt": settings.font_pt,
        "maxLines": settings.max_lines,
        "accelerator": settings.accelerator.as_str(),
    });
    if let Err(e) = app.run("show.subtitles", p) {
        app.set_status(e);
    }
}

/// The model: what it is, what it costs, where it goes, and how to get it.
fn model_block(app: &mut SlideApp, ui: &mut Ui) {
    let tokens = theme::Tokens::get(ui.ctx());
    let manifest = app.subtitles.manifest();
    let status = app.subtitles.model().clone();
    let verifying = app.subtitles.is_verifying();
    ui.label(egui::RichText::new("Speech model").font(theme::bold(13.0)));
    ui.label(format!("{} · {} · {} · {} files", manifest.name, manifest.license, size_label(manifest.total_bytes()), manifest.files.len()));
    ui.label(format!("Runtime: {}", manifest.runtime));
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Source: {} @", manifest.repo));
        selectable(ui, manifest.revision.as_str());
    });
    for file in &manifest.files {
        ui.label(format!("{} · {}", file.name, size_label(file.bytes)));
        selectable(ui, format!("SHA-256 {}", file.sha256));
    }
    ui.hyperlink_to("Model card and licence", &manifest.license_url);
    let colour = if status.state == ModelState::Ready { tokens.text } else { tokens.warning };
    ui.label(egui::RichText::new(status.summary()).color(colour));
    if verifying {
        // The hash result has not arrived yet: presence is not "installed".
        ui.label(egui::RichText::new("Checking the files…").color(tokens.warning));
    }
    if let Some(error) = &app.subtitles.verify_error {
        ui.label(egui::RichText::new(error).color(tokens.error));
    }
    let can_download = app.services.download_model.is_some();
    let can_pick = app.services.pick_open.is_some();
    let folder = !app.subtitles.model_dir.as_os_str().is_empty();
    if let Some(adopted) = app.subtitles.adopted_dir() {
        ui.label(format!("Chosen folder (this session only): {}", adopted.display()));
    } else if folder {
        ui.label(format!("Folder: {}", app.subtitles.model_dir.display()));
    }
    if let Some(job) = &app.subtitles.download {
        let fraction = if job.total == 0 { 0.0 } else { job.done as f32 / job.total as f32 };
        ui.add(egui::ProgressBar::new(fraction.clamp(0.0, 1.0)).text(format!("{} of {}", size_label(job.done), size_label(job.total))));
    }
    if let Some(error) = &app.subtitles.download_error {
        ui.label(egui::RichText::new(error).color(tokens.error));
    }
    ui.horizontal(|ui| {
        let hook = app.services.download_model.clone();
        let busy = app.subtitles.download.is_some();
        // Presence alone is not "installed": while the hash check runs, a corrupt directory can
        // still be replaced by a download.
        let ready = status.state == ModelState::Ready && !verifying;
        if ui.add_enabled(!busy && hook.is_some() && !ready, egui::Button::new("Download")).clicked()
            && let Some(hook) = hook
        {
            app.subtitles.begin_download(hook);
        }
        let cancelling = app.subtitles.download.as_ref().is_some_and(|job| job.is_cancelling());
        if busy {
            let label = if cancelling { "Cancelling…" } else { "Cancel download" };
            if ui.add_enabled(!cancelling, egui::Button::new(label)).clicked() {
                app.subtitles.cancel_download();
            }
        }
        if ui.add_enabled(!busy && can_pick, egui::Button::new("Choose Files…")).clicked()
            && let Some(pick) = app.services.pick_open.as_mut()
            && let Some(path) = pick("model")
        {
            // The manifest lists plain file names: the folder holding them is the model.
            let dir = std::path::Path::new(&path).parent().map(std::path::Path::to_path_buf);
            if let Some(dir) = dir {
                app.subtitles.adopt_dir(dir);
            }
        }
        if ui.add_enabled(!busy, egui::Button::new("Check Again")).on_hover_text("Re-check the folder in use; a folder you chose is kept").clicked() {
            app.subtitles.recheck();
        }
    });
    if !can_download {
        if can_pick {
            ui.label("Downloading is not available here; choose the files you placed in the model folder.");
        } else {
            ui.label("Subtitles are not available here: this host can neither download the model nor open a file picker.");
        }
    }
}
