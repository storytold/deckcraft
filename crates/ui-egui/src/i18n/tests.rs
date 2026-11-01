#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use super::*;
use crate::{Services, SlideApp, UiState};

/// A German catalog for the tests, in craft-i18n's format (context, English, translation).
const DE: &str = "@id\tlanguage.name\tDeutsch
\tHome\tStart
\tInsert\tEinfügen
\tFile\tDatei
\tNew\\nSlide\tNeue\\nFolie
\tSlide {n} of {total}\tFolie {n} von {total}
\tNotes\tNotizen
\t{name} options\tOptionen für {name}
\tSystem language\tSystemsprache
";

fn harness(app: SlideApp) -> Harness<'static, SlideApp> {
    Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    )
}

#[test]
fn a_string_without_a_translation_stays_english() {
    let de = fixture_lang("de", DE);
    assert_eq!(tr(de, "Insert"), "Einfügen");
    assert_eq!(tr(de, "New\nSlide"), "Neue\nFolie", "`\\n` in a catalog is a line break");
    assert_eq!(tr(de, "Not in the catalog"), "Not in the catalog");
    assert_eq!(tr(Lang::EN, "Insert"), "Insert");
    assert!(has(de, "Insert") && !has(Lang::EN, "Insert"));
    assert_eq!(de.name(), "Deutsch");
    set_current(de);
    assert_eq!(fmt(t("Slide {n} of {total}"), &[("n", "2"), ("total", "9")]), "Folie 2 von 9");
    assert_eq!(location("Home › Insert"), "Start › Einfügen");
    // A Quick Access Toolbar label is a ribbon caption with its line break saved as a space.
    assert_eq!(t_flat("New Slide"), "Neue Folie");
    assert_eq!(t_flat("Insert"), "Einfügen");
    assert_eq!(t_flat("Not in the catalog"), "Not in the catalog");
    set_current(Lang::EN);
}

#[test]
fn the_language_setting_is_auto_or_an_offered_code() {
    assert_eq!(normalize_pref("auto"), Some(AUTO));
    assert_eq!(normalize_pref(" AUTO "), Some(AUTO));
    assert_eq!(normalize_pref("EN"), Some("en"));
    for bad in ["", "xx", "en-US-x-whatever", "Deutsch"] {
        assert_eq!(normalize_pref(bad), None, "{bad:?}");
    }
    // Tests always resolve the system language to English, so labels can be queried in English.
    assert_eq!(Lang::from_pref(AUTO), Lang::EN);
    assert_eq!(Lang::from_pref("a code from a newer version"), Lang::EN);
    assert_eq!(Lang::all().next(), Some(Lang::EN), "English is first and always offered");
    // A damaged or hand-edited `ui.json` follows the system.
    for bad in ["klingon", "", "d", "de-", "<script>"] {
        assert_eq!(UiState { language: bad.into(), ..Default::default() }.sanitized().language, AUTO, "{bad:?}");
    }
    assert_eq!(UiState { language: "EN".into(), ..Default::default() }.sanitized().language, "en");
    // A code this build doesn't offer (German saved by a release, read by a build without the
    // catalogs) is kept, and the interface follows the system meanwhile.
    for kept in ["fr", "PT-br", "zh_Hans"] {
        let ui = UiState { language: kept.into(), ..Default::default() }.sanitized();
        assert_eq!(ui.language, kept.to_ascii_lowercase());
        assert_eq!(Lang::from_pref(&ui.language), Lang::EN);
    }
}

#[test]
fn app_language_reports_and_changes_the_setting() {
    use_fixture("de", DE);
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    let v = app.run("app.language", json!({})).unwrap();
    assert_eq!((v["language"].as_str(), v["current"].as_str()), (Some(AUTO), Some("en")));
    let codes: Vec<&str> = v["available"].as_array().unwrap().iter().filter_map(|l| l["code"].as_str()).collect();
    let mut expected = vec!["en", "de"];
    if CRAFT_I18N.iter().any(|(code, ..)| *code == "ru") {
        expected.push("ru");
    }
    assert_eq!(codes, expected);
    let v = app.run("app.language", json!({"language": "DE"})).unwrap();
    assert_eq!((v["language"].as_str(), v["current"].as_str()), (Some("de"), Some("de")));
    assert_eq!(app.ui.language, "de");
    for bad in [json!({"language": "xx"}), json!({"language": 7}), json!({"language": null})] {
        assert!(app.run("app.language", bad.clone()).is_err(), "{bad}");
        assert_eq!(app.ui.language, "de", "a refused value keeps the setting");
    }
    app.run("app.language", json!({"language": "auto"})).unwrap();
    assert_eq!(app.ui.language, AUTO);
}

/// Switching the language redraws the ribbon, status bar and accessible names in German, while
/// the tab ids the control channel and `view.tab` use stay English.
#[test]
fn switching_to_german_translates_the_interface_and_back() {
    use_fixture("de", DE);
    let mut h = harness(SlideApp::new(Session::with_new(), Services::default()));
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "Insert");
    h.get_by_value("Slide 1 of 1");

    h.state_mut().run("app.language", json!({"language": "de"})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::Button, "Insert").is_none(), "the English tab name is gone");
    h.get_by_role_and_label(Role::Button, "Einfügen").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.tab, "Insert", "the tab id stays English");
    h.state_mut().run("view.tab", json!({"tab": "Home"})).unwrap();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "Neue Folie");
    h.get_by_role_and_label(Role::Button, "Optionen für Neue Folie");
    h.get_by_role_and_label(Role::Button, "Notizen");
    h.get_by_value("Folie 1 von 1");
    h.get_by_role_and_label(Role::Button, "Datei");
    // A string the catalog lacks shows in English.
    h.get_by_role_and_label(Role::Button, "Design");

    h.state_mut().run("app.language", json!({"language": "en"})).unwrap();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "Insert");
    h.get_by_value("Slide 1 of 1");
}

/// Cancel in Preferences puts the language back, like the other settings there.
#[test]
fn preferences_cancel_restores_the_language() {
    use_fixture("de", DE);
    let mut h = harness(SlideApp::new(Session::with_new(), Services::default()));
    h.state_mut().run("app.preferences", json!({})).unwrap();
    h.run_steps(3);
    h.state_mut().run("app.language", json!({"language": "de"})).unwrap();
    h.run_steps(2);
    h.get_by_role_and_label(Role::Button, "Cancel").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert_eq!(h.state().ui.language, AUTO);
}

/// The catalogs `build.rs` embedded parse without a problem, and every embedded locale is offered.
/// Skipped in a build without craft-i18n (`CRAFT_I18N_DIR`), which is English only.
#[test]
fn embedded_catalogs_parse_cleanly() {
    if CRAFT_I18N.is_empty() {
        eprintln!("skipped: built without craft-i18n (CRAFT_I18N_DIR); the interface is English only");
        return;
    }
    let (i18n, problems) = I18n::new(CRAFT_I18N, &LANGUAGES);
    assert!(problems.is_empty(), "{problems:?}");
    let rows = i18n.check_all();
    assert!(rows.is_empty(), "{rows:#?}");
    for (code, ..) in CRAFT_I18N {
        assert!(LANGUAGES.contains(code), "{code} is embedded but not offered");
    }
}

/// The Russian catalogs reached the strings the UI draws, including the shared `common.tsv`
/// rows, a two-line ribbon label and a named-placeholder status line. Skipped without
/// `CRAFT_I18N_DIR`.
#[test]
fn the_russian_catalog_translates_core_labels() {
    let Some(russian) = Lang::from_code("ru") else {
        eprintln!("skipped: the ru catalogs are not embedded");
        return;
    };
    assert_eq!(tr(russian, "Insert"), "Вставка");
    assert_eq!(tr(russian, "Cancel"), "Отмена");
    assert_eq!(tr(russian, "Slide Show"), "Слайд-шоу");
    assert_eq!(tr(russian, "New\nSlide"), "Создать\nслайд");
    assert_eq!(fmt(tr(russian, "Slide {n} of {total}"), &[("n", "1"), ("total", "8")]), "Слайд 1 из 8");
    assert_eq!(lang_from_tag("ru-SK"), Some(russian));
    // Russian has three plural forms: 1 and 21 take one, 2 to 4 another, 5 to 20 a third.
    let count = |n| russian.i18n().trn(russian.lang, n, "{n} presentation", "{n} presentations").replace("{n}", &n.to_string());
    assert_eq!(count(1), "1 презентация");
    assert_eq!(count(2), "2 презентации");
    assert_eq!(count(5), "5 презентаций");
    assert_eq!(count(21), "21 презентация");
}
