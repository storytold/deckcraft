use super::{Icon, SlideApp, Ui, big, big_button, big_button_ex, group, json, rows, run};

// ---------- Slide Show / Record / Review / View ----------

pub(super) fn slide_show(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        if big_button(ui, Icon::PlayFromStart, "처음부터", true).clicked() {
            app.start_show(app.show_start_index(), false);
        }
        if big_button(ui, Icon::PlayFromCurrent, "현재 슬라이드\n부터", true).clicked() {
            let i = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            app.start_show(i, false);
        }
        let on = app.ui.presenter_view;
        if big_button_ex(ui, Icon::PresenterView, "발표자\n보기", true, false, on).0.clicked() {
            app.ui.presenter_view = !on;
        }
        big(app, ui, Icon::CustomShow, "슬라이드 쇼\n재구성", "app.dialog", json!({"id": "customShow"}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::SetUpShow, "슬라이드 쇼\n설정", "app.dialog", json!({"id": "setupShow"}));
        let hidden = app.session.active().and_then(|d| d.current_slide().map(|s| s.hidden)).unwrap_or(false);
        if big_button_ex(ui, Icon::HideSlide, "슬라이드\n숨기기", true, false, hidden).0.clicked() {
            run(app, "slide.hide", json!({}));
        }
    });
    group(ui, |ui| {
        if big_button(ui, Icon::RehearseTimings, "예행연습", true).clicked() {
            app.start_show(app.show_start_index(), false);
            if let Some(s) = app.show.as_mut() {
                s.rehearse = true;
            }
        }
        let r = big_button(ui, Icon::Record, "기록", false);
        let _ = r.on_hover_text("설명 녹음에는 오디오 캡처가 필요합니다 (준비 중)");
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            let mut use_t = app.session.active().is_none_or(|d| d.doc.show.use_timings);
            if ui.checkbox(&mut use_t, "설명 타이밍 사용").changed() {
                run(app, "show.setup", json!({"useTimings": use_t}));
            }
            let mut narr = app.session.active().is_none_or(|d| !d.doc.show.without_narration);
            if ui.checkbox(&mut narr, "설명 재생").changed() {
                run(app, "show.setup", json!({"noNarration": !narr}));
            }
            let mut mc = app.session.active().is_none_or(|d| d.doc.show.show_media_controls);
            if ui.checkbox(&mut mc, "미디어 컨트롤 표시").changed() {
                run(app, "show.setup", json!({"showMediaControls": mc}));
            }
        });
    });
}

pub(super) fn record(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let r = big_button(ui, Icon::Cameo, "카메오", false);
        let _ = r.on_hover_text("카메라 캡처는 아직 지원되지 않습니다");
        if big_button(ui, Icon::PlayFromStart, "처음부터", true).clicked() {
            app.start_show(app.show_start_index(), false);
            if let Some(s) = app.show.as_mut() {
                s.rehearse = true;
            }
        }
        if big_button(ui, Icon::PlayFromCurrent, "현재 슬라이드\n부터", true).clicked() {
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
