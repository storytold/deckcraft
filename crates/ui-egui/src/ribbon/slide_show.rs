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
        let r = big_button(ui, Icon::Record, "Record", false);
        let _ = r.on_hover_text(crate::i18n::tr("Recording narration needs audio capture (coming soon)"));
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            let mut use_t = app.session.active().is_none_or(|d| d.doc.show.use_timings);
            if ui.checkbox(&mut use_t, crate::i18n::tr("Use Timings")).changed() {
                run(app, "show.setup", json!({"useTimings": use_t}));
            }
            let mut narr = app.session.active().is_none_or(|d| !d.doc.show.without_narration);
            if ui.checkbox(&mut narr, crate::i18n::tr("Play Narrations")).changed() {
                run(app, "show.setup", json!({"noNarration": !narr}));
            }
            let mut mc = app.session.active().is_none_or(|d| d.doc.show.show_media_controls);
            if ui.checkbox(&mut mc, crate::i18n::tr("Show Media Controls")).changed() {
                run(app, "show.setup", json!({"showMediaControls": mc}));
            }
        });
    });
}

pub(super) fn record(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let r = big_button(ui, Icon::Cameo, "Cameo", false);
        let _ = r.on_hover_text(crate::i18n::tr("Camera capture is not available yet"));
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
    });
    group(ui, |ui| {
        if big_button(ui, Icon::Help, "Learn\nMore", true).clicked() {
            app.open_url(deckcraft_engine::links::APP_PAGE);
        }
    });
}
