use super::{Icon, SlideApp, Ui, big, big_button, big_button_ex, group, json, rows, run, small, small_button, vec2};

// ---------- Slide Show / Record / Review / View ----------

pub(super) fn slide_show(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, "Start Slide Show", |ui| {
        if crate::ribbon_fit::group_level(ui.ctx()) == crate::ribbon_fit::STACKED {
            // Measured three rows between 600 and 1120 points.
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
                ui.horizontal(|ui| {
                    if small_button(ui, Icon::PlayFromStart, "Play from Start", true, false).clicked() {
                        app.start_show(app.show_start_index(), false);
                    }
                    if small_button(ui, Icon::PlayFromCurrent, "Play from Current Slide", true, false).clicked() {
                        let i = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
                        app.start_show(i, false);
                    }
                });
                ui.horizontal(|ui| {
                    let on = app.ui.presenter_view;
                    if small_button(ui, Icon::PresenterView, "Presenter View", true, on).clicked() {
                        app.ui.presenter_view = !on;
                    }
                });
                ui.horizontal(|ui| {
                    small(app, ui, Icon::CustomShow, "Custom Show", "app.dialog", json!({"id": "customShow"}));
                });
            });
        } else {
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
        }
    });
    group(ui, "Set Up", |ui| {
        big(app, ui, Icon::SetUpShow, "Set Up\nSlide Show", "app.dialog", json!({"id": "setupShow"}));
        let hidden = app.session.active().and_then(|d| d.current_slide().map(|s| s.hidden)).unwrap_or(false);
        if big_button_ex(ui, Icon::HideSlide, "Hide\nSlide", true, false, hidden).0.clicked() {
            run(app, "slide.hide", json!({}));
        }
    });
    group(ui, "Rehearse", |ui| {
        if big_button(ui, Icon::RehearseTimings, "Rehearse\nTimings", true).clicked() {
            app.start_show(app.show_start_index(), false);
            if let Some(s) = app.show.as_mut() {
                s.rehearse = true;
            }
        }
        let r = big_button(ui, Icon::Record, "Record", false);
        let _ = r.on_hover_text("Recording narration needs audio capture (coming soon)");
    });
    group(ui, "Timing", |ui| {
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
        });
    });
}

pub(super) fn record(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, "Record", |ui| {
        if crate::ribbon_fit::group_level(ui.ctx()) == crate::ribbon_fit::STACKED {
            // Measured three rows between 600 and 680 points.
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = vec2(1.0, 0.0);
                let r = small_button(ui, Icon::Cameo, "Cameo", false, false);
                let _ = r.on_hover_text("Camera capture is not available yet");
                if small_button(ui, Icon::PlayFromStart, "From Beginning", true, false).clicked() {
                    app.start_show(app.show_start_index(), false);
                    if let Some(s) = app.show.as_mut() {
                        s.rehearse = true;
                    }
                }
                if small_button(ui, Icon::PlayFromCurrent, "From Current Slide", true, false).clicked() {
                    let i = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
                    app.start_show(i, false);
                    if let Some(s) = app.show.as_mut() {
                        s.rehearse = true;
                    }
                }
            });
        } else {
            let r = big_button(ui, Icon::Cameo, "Cameo", false);
            let _ = r.on_hover_text("Camera capture is not available yet");
            if big_button(ui, Icon::PlayFromStart, "From\nBeginning", true).clicked() {
                app.start_show(app.show_start_index(), false);
                if let Some(s) = app.show.as_mut() {
                    s.rehearse = true;
                }
            }
            if big_button(ui, Icon::PlayFromCurrent, "From\nCurrent Slide", true).clicked() {
                let i = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
                app.start_show(i, false);
                if let Some(s) = app.show.as_mut() {
                    s.rehearse = true;
                }
            }
        }
    });
    group(ui, "Help", |ui| {
        if big_button(ui, Icon::Help, "Learn\nMore", true).clicked() {
            app.open_url(deckcraft_engine::links::APP_PAGE);
        }
    });
}
