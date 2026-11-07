use super::{Icon, SlideApp, Ui, big, big_button, big_button_ex, group, json, rows, run};

// ---------- Slide Show / Record / Review / View ----------

pub(super) fn slide_show(app: &mut SlideApp, ui: &mut Ui) {
    // One microphone: a Record ▸ Audio take in progress blocks starting a show that records.
    let recording = app.audio_recording();
    let busy = if recording { "A Record ▸ Audio take owns the microphone: stop or cancel it first" } else { "" };
    group(ui, |ui| {
        if big_button(ui, Icon::PlayFromStart, "Play from\nStart", true).clicked() {
            app.start_show(app.show_start_index(), false);
        }
        if big_button(ui, Icon::PlayFromCurrent, "Play from\nCurrent Slide", true).clicked() {
            let i = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            app.start_show(i, false);
        }
        let on = app.ui.presenter_view;
        if big_button_ex(ui, Icon::PresenterView, "Presenter\nView", true, false, on).0.clicked() {
            app.ui.presenter_view = !on;
        }
        big(app, ui, Icon::CustomShow, "Custom\nShow", "app.dialog", json!({"id": "customShow"}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::SetUpShow, "Set Up\nSlide Show", "app.dialog", json!({"id": "setupShow"}));
        let hidden = app.session.active().and_then(|d| d.current_slide().map(|s| s.hidden)).unwrap_or(false);
        if big_button_ex(ui, Icon::HideSlide, "Hide\nSlide", true, false, hidden).0.clicked() {
            run(app, "slide.hide", json!({}));
        }
    });
    group(ui, |ui| {
        if big_button(ui, Icon::RehearseTimings, "Rehearse\nTimings", true).clicked() {
            app.start_show(app.show_start_index(), false);
            if let Some(s) = app.show.as_mut() {
                s.rehearse = true;
            }
        }
        let r = big_button(ui, Icon::Record, "Record", !recording)
            .on_hover_text("Records slide timings, the microphone and pen ink. Laser follows the pointer.");
        // Attached only while it applies: an empty tooltip still draws a box under the description.
        if (if recording { r.on_hover_text(busy) } else { r }).clicked() {
            let _ = app.run("show.record", json!({}));
        }
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            let mut use_t = app.session.active().is_none_or(|d| d.doc.show.use_timings);
            if ui.checkbox(&mut use_t, "Use Timings").changed() {
                run(app, "show.setup", json!({"useTimings": use_t}));
            }
            let mut narr = app.session.active().is_none_or(|d| !d.doc.show.without_narration);
            if ui.checkbox(&mut narr, "Play Narrations").changed() {
                run(app, "show.setup", json!({"noNarration": !narr}));
            }
            let mut mc = app.session.active().is_none_or(|d| d.doc.show.show_media_controls);
            if ui.checkbox(&mut mc, "Show Media Controls").changed() {
                run(app, "show.setup", json!({"showMediaControls": mc}));
            }
            let clear = ui.button("Clear").on_hover_text("Clear Timing or Clear Narration");
            egui::Popup::menu(&clear).show(|ui| {
                if ui.button("Clear Timing on Current Slide").clicked() {
                    run(app, "show.clearTiming", json!({}));
                    ui.close();
                }
                if ui.button("Clear Timing on All Slides").clicked() {
                    run(app, "show.clearTimingAll", json!({}));
                    ui.close();
                }
                if ui.button("Clear Narration on Current Slide").clicked() {
                    run(app, "show.clearNarration", json!({}));
                    ui.close();
                }
                if ui.button("Clear Narration on All Slides").clicked() {
                    run(app, "show.clearNarrationAll", json!({}));
                    ui.close();
                }
            });
        });
    });
}

pub(super) fn record(app: &mut SlideApp, ui: &mut Ui) {
    // Record Slide Show and Record ▸ Audio share one microphone: a take in progress owns it, and
    // a show take would split the captured audio and cancel the clip when it stops.
    let recording = app.audio_recording();
    let busy = if recording { "A Record ▸ Audio take owns the microphone: stop or cancel it first" } else { "" };
    group(ui, |ui| {
        let r = big_button(ui, Icon::Cameo, "Cameo", false);
        let _ = r.on_hover_text("Camera capture is not available yet");
        let has_mic = app.has_mic();
        let r = big_button(ui, Icon::Audio, "Audio", has_mic && !recording);
        let tip = if !has_mic {
            "Audio recording isn't available here"
        } else if recording {
            "A recording is in progress: use Stop or Cancel in the record bar"
        } else {
            "Record audio and insert it on this slide"
        };
        if r.on_hover_text(tip).clicked() {
            let _ = app.start_audio_recording();
        }
        let from_beginning = big_button(ui, Icon::PlayFromStart, "From\nBeginning", !recording);
        // An empty tooltip still draws a box, so the reason is attached only while it applies.
        if (if recording { from_beginning.on_hover_text(busy) } else { from_beginning }).clicked() {
            let _ = app.run("show.record", json!({"from": app.show_start_index()}));
        }
        let from_current = big_button(ui, Icon::PlayFromCurrent, "From\nCurrent Slide", !recording);
        if (if recording { from_current.on_hover_text(busy) } else { from_current }).clicked() {
            let i = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            let _ = app.run("show.record", json!({"from": i}));
        }
        let preview_hint = if recording { busy } else { "Play the recording with its timings and narration. Nothing is saved or changed." };
        if big_button(ui, Icon::Preview, "Preview", !recording).on_hover_text(preview_hint).clicked() {
            let _ = app.run("show.preview", json!({"from": app.show_start_index()}));
        }
    });
    group(ui, |ui| {
        let label = "Save as\nShow";
        let hint = "Save a copy as a PowerPoint Show (.ppsx) that starts the slide show when opened";
        if big_button(ui, Icon::SaveAs, label, true).on_hover_text(hint).clicked() {
            save_as_show(app);
        }
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            let clear_on = super::enabled(app, "show.clearRecording");
            let clear = ui
                .add_enabled(clear_on, egui::Button::new("Clear Recording"))
                .on_hover_text("Remove recorded narration, camera and screen media, ink, laser and advance timings");
            egui::Popup::menu(&clear).show(|ui| {
                for (label, scope) in [("Clear Recording on Current Slide", "current"), ("Clear Recording on All Slides", "all")] {
                    if ui.add_enabled(clear_on, egui::Button::new(label)).clicked() {
                        run(app, "show.clearRecording", json!({"scope": scope}));
                        ui.close();
                    }
                }
            });
            let reset_on = super::enabled(app, "show.resetCameo");
            let reset = ui
                .add_enabled(reset_on, egui::Button::new("Reset to Cameo"))
                .on_hover_text("Drop the recorded camera clip, return the live feed and restore the default Cameo placement and size");
            egui::Popup::menu(&reset).show(|ui| {
                for (label, scope) in [("Reset to Cameo on Current Slide", "current"), ("Reset to Cameo on All Slides", "all")] {
                    if ui.add_enabled(reset_on, egui::Button::new(label)).clicked() {
                        run(app, "show.resetCameo", json!({"scope": scope}));
                        ui.close();
                    }
                }
            });
        });
    });
    group(ui, |ui| {
        if big_button(ui, Icon::Help, "Learn\nMore", true).clicked() {
            app.open_url(deckcraft_engine::links::APP_PAGE);
        }
    });
}

/// Record ▸ Export ▸ Save as Show: write a `.ppsx` copy of the active presentation through the
/// normal Save dialog, as PowerPoint does. The copy starts the slide show when opened; the
/// presentation keeps its own file. The status bar names the copy, or why it wasn't written.
fn save_as_show(app: &mut SlideApp) {
    app.export_as("ppsx", json!({}));
}
