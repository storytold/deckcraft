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

/// A Hebrew catalog for the tests, in craft-i18n's format. The `{total}` / `{n}` order is the
/// translator's, not the English order, so placeholder filling is what the assertion checks.
const HE: &str = "@id\tlanguage.name\tעברית
\tInsert\tהוספה
\tNew\\nSlide\tשקופית\\nחדשה
\tSlide {n} of {total}\t{total} מתוך {n}
";

/// Puts the interface language and the fixture catalog back, including when an assertion fails.
struct ResetLang;

impl ResetLang {
    fn arm() -> Self {
        set_current(Lang::EN);
        FIXTURE.set(None);
        Self
    }
}

impl Drop for ResetLang {
    fn drop(&mut self) {
        set_current(Lang::EN);
        FIXTURE.set(None);
    }
}

fn hebrew_catalog_embedded() -> bool {
    CRAFT_I18N.iter().any(|(code, ..)| code.eq_ignore_ascii_case("he"))
}

/// A build that embedded catalogs (`CRAFT_I18N_DIR`) has to embed Hebrew too. An English-only
/// build, with no catalogs at all, does not.
fn require_hebrew_when_catalogs_are_embedded() {
    assert!(
        CRAFT_I18N.is_empty() || hebrew_catalog_embedded(),
        "CRAFT_I18N_DIR embedded interface catalogs but not Hebrew (locales/he/deckcraft.tsv)"
    );
}

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
    // Hebrew is listed when its catalog is embedded. A checkout that embeds other languages and
    // omits `locales/he/deckcraft.tsv` fails below, after the German checks have run.
    let expected: Vec<&str> = ["en", "de", "he"].into_iter().filter(|c| *c != "he" || hebrew_catalog_embedded()).collect();
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
    require_hebrew_when_catalogs_are_embedded();
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

/// The Hebrew catalog translates core labels from both the shared and DeckCraft rows, and Hebrew
/// locale tags resolve to it. Skipped only in a build with no catalogs at all. A `CRAFT_I18N_DIR`
/// that embeds other languages and omits `locales/he/deckcraft.tsv` fails.
#[test]
fn the_hebrew_catalog_translates_core_labels() {
    if CRAFT_I18N.is_empty() {
        eprintln!("skipped: built without craft-i18n (CRAFT_I18N_DIR); the interface is English only");
        return;
    }
    // A fixture left by another test on this thread would answer `he` with its own wording.
    FIXTURE.set(None);
    require_hebrew_when_catalogs_are_embedded();
    let he = Lang::from_code("he").expect("Hebrew is embedded");
    assert_eq!(he.name(), "עברית");
    for tag in ["he", "HE", "He", "he-IL", "HE-il", "he_IL.UTF-8", "HE_il.utf-8"] {
        assert_eq!(lang_from_tag(tag), Some(he), "{tag}");
    }
    assert_eq!(lang_from_tag("fr-FR"), None);
    assert_eq!(first_supported(["fr-FR", "he-IL"]), Some(he));
    // A shared row and DeckCraft rows, one of them a two-line ribbon label.
    assert_eq!(tr(he, "Save"), "שמירה");
    assert_eq!(tr(he, "New Slide"), "שקופית חדשה");
    assert_eq!(tr(he, "New\nSlide"), "שקופית\nחדשה");
    assert_eq!(fmt_plain(tr(he, "Slide {n} of {total}"), &[("n", "2"), ("total", "9")]), "שקופית 2 מתוך 9");
    // The status bar fills the same string with `fmt`, which isolates each value.
    let filled = fmt(tr(he, "Slide {n} of {total}"), &[("n", "2"), ("total", "9")]);
    assert_eq!(fmt_plain(&filled, &[]), "שקופית 2 מתוך 9");
    assert_ne!(filled, "שקופית 2 מתוך 9", "right-to-left text isolates the values");
}

/// Lookup, tag resolution and `app.language` for Hebrew, from a fixture catalog, so they run in a
/// build with no `CRAFT_I18N_DIR`.
#[test]
fn hebrew_fixture_covers_lookup_tags_and_app_language() {
    let _reset = ResetLang::arm();
    let he = use_fixture("he", HE);
    assert_eq!(he.name(), "עברית");
    assert_eq!(tr(he, "Insert"), "הוספה");
    assert_eq!(tr(he, "New\nSlide"), "שקופית\nחדשה");
    assert_eq!(tr(he, "Not in the catalog"), "Not in the catalog");

    for tag in ["he", "HE", "He", "he-IL", "HE-il", "he_IL.UTF-8", "HE_il.utf-8"] {
        assert_eq!(lang_from_tag(tag), Some(he), "{tag}");
    }
    for tag in ["fr-FR", "fr", "ar", "xx", ""] {
        assert_eq!(lang_from_tag(tag), None, "{tag}");
    }
    assert_eq!(first_supported(["fr-FR", "he-IL"]), Some(he));
    assert_eq!(first_supported(["fr-FR", "ja-JP"]), None);

    set_current(he);
    // The Quick Access Toolbar stores the ribbon's line break as a space.
    assert_eq!(t_flat("New Slide"), "שקופית חדשה");
    assert_eq!(t_flat("Insert"), "הוספה");
    // The translator put `{total}` before `{n}`.
    assert_eq!(fmt_plain(t("Slide {n} of {total}"), &[("n", "2"), ("total", "9")]), "9 מתוך 2");
    let filled = fmt(t("Slide {n} of {total}"), &[("n", "2"), ("total", "9")]);
    assert_eq!(fmt_plain(&filled, &[]), "9 מתוך 2");
    assert!(filled.contains('\u{2068}'), "a right-to-left template isolates each value");

    let mut app = SlideApp::new(Session::with_new(), Services::default());
    let v = app.run("app.language", json!({"language": "he"})).unwrap();
    assert_eq!((v["language"].as_str(), v["current"].as_str()), (Some("he"), Some("he")));
    assert_eq!(app.ui.language, "he");
    let v = app.run("app.language", json!({"language": "HE"})).unwrap();
    assert_eq!(v["current"].as_str(), Some("he"));
    let names: Vec<&str> = v["available"].as_array().unwrap().iter().filter(|l| l["code"] == "he").filter_map(|l| l["name"].as_str()).collect();
    assert_eq!(names, ["עברית"]);
    // The setting is the language code. A regional tag is not a value of it.
    assert!(app.run("app.language", json!({"language": "he-IL"})).is_err());
    assert_eq!(app.ui.language, "he");
}

/// Directional marks turn around for Hebrew and stay put for a left-to-right language.
#[test]
fn right_to_left_languages_mirror_directional_symbols() {
    let _reset = ResetLang::arm();
    assert!(!is_rtl());
    for glyph in ["→", "▶", "◀", "‹", "›"] {
        assert_eq!(mirror_glyph(glyph), glyph);
    }
    set_current(use_fixture("de", DE));
    assert!(!is_rtl());
    assert_eq!(mirror_glyph("→"), "→");

    set_current(use_fixture("he", HE));
    assert!(is_rtl());
    assert_eq!(mirror_glyph("→"), "←");
    assert_eq!(mirror_glyph("▶"), "◀");
    assert_eq!(mirror_glyph("◀"), "▶");
    assert_eq!(mirror_glyph("Close"), "Close");
    // The left ribbon cue lowers the offset and the right cue raises it. The chevrons keep
    // pointing that way when the language is Hebrew.
    assert_eq!(mirror_glyph("‹"), "‹");
    assert_eq!(mirror_glyph("›"), "›");
    assert_eq!(crate::ribbon::scroll_cue_glyph(true), "‹");
    assert_eq!(crate::ribbon::scroll_cue_glyph(false), "›");
}

/// A selected shape's Width label uses the catalog key `Width:`. `Width: ` (the old key, with the
/// space inside it) has no row, so it would stay English. The stand-in is ASCII, and this fixture
/// translates nothing else, so the test does not need a Hebrew face.
#[test]
fn a_selected_shapes_width_label_uses_the_catalog_key() {
    let _reset = ResetLang::arm();
    let he = use_fixture("he", "\tWidth:\tWIDTH\n");
    assert_eq!(tr(he, "Width:"), "WIDTH");
    assert_eq!(tr(he, "Width: "), "Width: ", "a trailing space is a different catalog key");
    let mut session = Session::with_new();
    session.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = session.execute("shape.insert", &json!({"preset": "rect", "rect": [100.0, 100.0, 80.0, 60.0]})).unwrap()["id"].as_u64().unwrap();
    session.execute("selection.select", &json!({"ids": [id]})).unwrap();
    let mut app = SlideApp::new(session, Services::default());
    app.run("app.language", json!({"language": "he"})).unwrap();
    app.ui.tab = "Shape Format".into();
    let mut h = Harness::builder().with_size(egui::vec2(2200.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("WIDTH ");
    assert!(h.query_by_label("Width: ").is_none());
}
