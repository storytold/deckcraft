use super::{Icon, SlideApp, Ui, big, big_button, big_button_ex, group, json, rows, run};

// ---------- Slide Show / Record / Review / View ----------

pub(super) fn slide_show(app: &mut SlideApp, ui: &mut Ui) {
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
        if big_button(ui, Icon::Record, "Record", true)
            .on_hover_text("Records slide timings, the microphone and pen ink. Laser follows the pointer.")
            .clicked()
        {
            // The same start the Record tab's "From Beginning" uses: the first slide of the show
            // order, not always slide 0 (which may be hidden or outside a custom show).
            let _ = app.run("show.record", json!({"from": app.show_start_index()}));
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
    group(ui, |ui| {
        let recording = app.session.screen.is_recording();
        let r = big_button_ex(ui, Icon::ScreenRecording, "Screen\nRecording", true, false, recording).0;
        let r = r.on_hover_text("Record a display, a window or a region and insert it on the current slide");
        if r.clicked() {
            crate::screen::pick(app);
        }
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Cameo, "Cameo", false);
        let _ = r.on_hover_text("Camera capture is not available yet");
        if big_button(ui, Icon::PlayFromStart, "From\nBeginning", true).clicked() {
            let _ = app.run("show.record", json!({"from": app.show_start_index()}));
        }
        if big_button(ui, Icon::PlayFromCurrent, "From\nCurrent Slide", true).clicked() {
            let i = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            let _ = app.run("show.record", json!({"from": i}));
        }
    });
    group(ui, |ui| {
        if big_button(ui, Icon::Help, "Learn\nMore", true).clicked() {
            app.open_url(deckcraft_engine::links::APP_PAGE);
        }
    });
}
