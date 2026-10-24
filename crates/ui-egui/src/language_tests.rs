//! Interface language: switching draws the ribbon and a dialog in Japanese, an untranslated label
//! stays in English, and a damaged `language` setting falls back to following the system.

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

fn harness() -> Harness<'static, SlideApp> {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.ui.tab = "Home".into();
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

#[test]
fn switching_to_japanese_redraws_the_ribbons_buttons_in_japanese() {
    let mut h = harness();
    // English before the switch (`auto` picks English under test). The ribbon tabs are painted,
    // so the buttons are what the accessibility tree (and this test) can see.
    assert!(h.query_all_by_label("Paste").next().is_some(), "the English Paste button is drawn");
    assert!(h.query_all_by_label("貼り付け").next().is_none());
    h.state_mut().run("ui.language", json!({"value": "ja"})).unwrap();
    h.run_steps(2);
    for japanese in ["貼り付け", "レイアウト", "リセット", "セクション"] {
        assert!(h.query_all_by_label(japanese).next().is_some(), "the Japanese label {japanese} is drawn");
    }
    assert!(h.query_all_by_label("Paste").next().is_none(), "the English button label is gone");
    // Back to English: the same buttons draw in English again.
    h.state_mut().run("ui.language", json!({"value": "en"})).unwrap();
    h.run_steps(2);
    assert!(h.query_all_by_label("Paste").next().is_some());
    assert!(h.query_all_by_label("貼り付け").next().is_none());
}

#[test]
fn the_preferences_picker_switches_the_interface_language() {
    let mut h = harness();
    h.state_mut().run("app.preferences", json!({})).unwrap();
    h.run_steps(2);
    // The dialog opens in English; its Interface language list follows the system by default.
    assert!(h.query_all_by_label("Interface language:").next().is_some(), "the language section is shown");
    h.get_by_label("Automatic (English)").click_accesskit();
    h.run_steps(2);
    h.get_by_label("日本語").click();
    h.run_steps(2);
    assert!(h.query_all_by_label("表示言語:").next().is_some(), "the dialog itself is redrawn in Japanese");
    assert_eq!(h.state().ui.language, "ja");
}

#[test]
fn interface_language_setting_takes_codes_or_auto_and_rejects_junk() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    assert_eq!(app.ui.language, "auto", "the default follows the system");
    assert!(app.run("ui.language", json!({"value": "klingon"})).is_err());
    assert_eq!(app.ui.language, "auto", "a hostile value changes nothing");
    assert!(app.run("ui.language", json!({"value": 1})).is_err());
    app.run("ui.language", json!({"value": "JA"})).unwrap();
    assert_eq!(app.ui.language, "ja", "a code is stored in its canonical form");
    app.run("ui.language", json!({"value": "auto"})).unwrap();
    assert_eq!(app.ui.language, "auto");
    // A string without a translation is drawn in English, never blank.
    assert_eq!(crate::i18n::t("A label no catalog has"), "A label no catalog has");
}

#[test]
fn a_damaged_saved_language_setting_falls_back_to_automatic() {
    let junk = crate::UiState { language: "klingon".into(), ..Default::default() }.sanitized();
    assert_eq!(junk.language, crate::i18n::AUTO);
    let code = crate::UiState { language: "JA".into(), ..Default::default() }.sanitized();
    assert_eq!(code.language, "ja");
}
