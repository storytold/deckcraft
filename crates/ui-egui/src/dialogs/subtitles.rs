//! Subtitle Settings: spoken and subtitle language, microphone, position, text size, and the
//! Whisper model (source, size, licence, hashes, download and manual import).

use super::{Color32, Dialog, SlideApp, Ui, buttons, json, theme};
use deckcraft_asr::caption::{MAX_FONT_PT, MAX_LINES_CAP, MIN_FONT_PT};
use deckcraft_asr::settings::{SPOKEN_LANGUAGES, SUBTITLE_LANGUAGES};
use deckcraft_asr::{Accelerator, ModelState, Position, SubtitleSettings};

/// The languages menu label for a tag; an unknown tag shows as itself.
fn language_label(list: &[(&str, &str)], tag: &str) -> String {
    list.iter().find(|(t, _)| *t == tag).map(|(_, l)| (*l).to_string()).unwrap_or_else(|| tag.to_string())
}

fn size_label(bytes: u64) -> String {
    if bytes >= 1_000_000 { format!("{:.1} MB", bytes as f64 / 1_000_000.0) } else { format!("{bytes} bytes") }
}

fn short_hash(hash: &str) -> String {
    hash.chars().take(16).collect::<String>() + "…"
}

#[cfg(not(target_arch = "wasm32"))]
fn microphones() -> Vec<String> {
    deckcraft_asr_candle::WhisperTranscriber::microphones()
}

#[cfg(target_arch = "wasm32")]
fn microphones() -> Vec<String> {
    Vec::new()
}

/// The Subtitle Settings dialog. Changes apply as they are made (through `show.subtitles`), so the
/// show and the status line follow without an OK step; Cancel does not undo them.
pub(super) fn subtitles_dialog(app: &mut SlideApp, ui: &mut Ui, _d: &mut Dialog) -> bool {
    let current = app.session.prefs.subtitles.clone();
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
        craft_ui::ComboBox::from_id_salt("subs_subtitle").selected_text(language_label(SUBTITLE_LANGUAGES, &next.subtitle)).show_ui(ui, |ui| {
            for (tag, label) in SUBTITLE_LANGUAGES {
                if ui.selectable_label(next.subtitle == *tag, *label).clicked() {
                    next.subtitle = (*tag).to_string();
                    changed = true;
                }
            }
        });
    });
    let devices = microphones();
    if !devices.is_empty() {
        ui.horizontal(|ui| {
            ui.label("Microphone:");
            let selected = next.microphone.clone().unwrap_or_else(|| "System default".to_string());
            craft_ui::ComboBox::from_id_salt("subs_mic").selected_text(selected).show_ui(ui, |ui| {
                if ui.selectable_label(next.microphone.is_none(), "System default").clicked() {
                    next.microphone = None;
                    changed = true;
                }
                for name in &devices {
                    if ui.selectable_label(next.microphone.as_deref() == Some(name.as_str()), name).clicked() {
                        next.microphone = Some(name.clone());
                        changed = true;
                    }
                }
            });
        });
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
    if let Some(note) = app.subtitles.runtime_note() {
        ui.label(egui::RichText::new(note).color(Color32::from_rgb(0xB0, 0x60, 0x10)));
    } else if !app.subtitles.accelerators().contains(&next.accelerator) {
        ui.label(egui::RichText::new(format!("runs on CPU; {} not available", next.accelerator.label())).color(Color32::from_rgb(0xB0, 0x60, 0x10)));
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
    let manifest = app.subtitles.manifest();
    let status = app.subtitles.model().clone();
    ui.label(egui::RichText::new("Speech model").font(theme::bold(13.0)));
    ui.label(format!("{} · {} · {} · {} files", manifest.name, manifest.license, size_label(manifest.total_bytes()), manifest.files.len()));
    ui.label(format!("Runtime: {}", manifest.runtime));
    ui.label(format!("Source: {} @ {}", manifest.repo, short_hash(&manifest.revision)));
    for file in &manifest.files {
        ui.label(format!("  {} · {} · SHA-256 {}", file.name, size_label(file.bytes), short_hash(&file.sha256)));
    }
    ui.hyperlink_to("Model card and licence", &manifest.license_url);
    let colour = if status.state == ModelState::Ready { theme::Tokens::get(ui.ctx()).text } else { Color32::from_rgb(0xB0, 0x60, 0x10) };
    ui.label(egui::RichText::new(status.summary()).color(colour));
    let can_download = app.services.download_model.is_some();
    let can_pick = app.services.pick_open.is_some();
    let folder = !app.subtitles.model_dir.as_os_str().is_empty();
    if folder {
        ui.label(format!("Folder: {}", app.subtitles.model_dir.display()));
    }
    if let Some(job) = &app.subtitles.download {
        let fraction = if job.total == 0 { 0.0 } else { job.done as f32 / job.total as f32 };
        ui.add(egui::ProgressBar::new(fraction.clamp(0.0, 1.0)).text(format!("{} of {}", size_label(job.done), size_label(job.total))));
    }
    if let Some(error) = &app.subtitles.download_error {
        ui.label(egui::RichText::new(error).color(Color32::from_rgb(0xB0, 0x30, 0x30)));
    }
    ui.horizontal(|ui| {
        let hook = app.services.download_model.clone();
        let busy = app.subtitles.download.is_some();
        let ready = status.state == ModelState::Ready;
        if ui.add_enabled(!busy && hook.is_some() && !ready, egui::Button::new("Download")).clicked()
            && let Some(hook) = hook
        {
            app.subtitles.begin_download(hook);
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
        if ui.add_enabled(!busy, egui::Button::new("Check Again")).clicked() {
            app.subtitles.refresh();
        }
    });
    if !can_download {
        if folder {
            ui.label("Downloading is not available here; place the files in the folder above.");
        } else if !can_pick {
            ui.label("Subtitles are not available here: this host can neither download the model nor open a file picker.");
        }
    }
}
