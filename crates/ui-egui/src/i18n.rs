//! Interface text only: presentation content, file names and command identifiers stay intact.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    #[default]
    System,
    En,
    Uk,
    Ru,
    Es,
    Fr,
    Pt,
    De,
}

impl Language {
    pub const ALL: [Self; 8] = [Self::System, Self::En, Self::Uk, Self::Ru, Self::Es, Self::Fr, Self::Pt, Self::De];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => tr("System language"),
            Self::En => "English",
            Self::Uk => "Українська",
            Self::Ru => "Русский",
            Self::Es => "Español",
            Self::Fr => "Français",
            Self::Pt => "Português",
            Self::De => "Deutsch",
        }
    }

    pub fn from_locale(locale: &str) -> Self {
        let language = locale.split(['-', '_', '.', '@']).next().unwrap_or("");
        match language.to_ascii_lowercase().as_str() {
            "uk" => Self::Uk,
            "ru" => Self::Ru,
            "es" => Self::Es,
            "fr" => Self::Fr,
            "pt" => Self::Pt,
            "de" => Self::De,
            _ => Self::En,
        }
    }

    pub fn resolve(self, locale: Option<&str>) -> Self {
        if self == Self::System { Self::from_locale(locale.unwrap_or("")) } else { self }
    }
}

thread_local! {
    static CURRENT: Cell<Language> = const { Cell::new(Language::En) };
}

pub fn current() -> Language {
    CURRENT.get()
}

pub fn set_current(language: Language) {
    CURRENT.set(language);
}

const UK_CATALOG: &str = include_str!("i18n/uk.tsv");
const RU_CATALOG: &str = include_str!("i18n/ru.tsv");
const ES_CATALOG: &str = include_str!("i18n/es.tsv");
const FR_CATALOG: &str = include_str!("i18n/fr.tsv");
const PT_CATALOG: &str = include_str!("i18n/pt.tsv");
const DE_CATALOG: &str = include_str!("i18n/de.tsv");

static UK: OnceLock<HashMap<String, String>> = OnceLock::new();
static RU: OnceLock<HashMap<String, String>> = OnceLock::new();
static ES: OnceLock<HashMap<String, String>> = OnceLock::new();
static FR: OnceLock<HashMap<String, String>> = OnceLock::new();
static PT: OnceLock<HashMap<String, String>> = OnceLock::new();
static DE: OnceLock<HashMap<String, String>> = OnceLock::new();

fn parse_catalog(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
        .map(|(source, text)| (source.replace("\\n", "\n"), text.replace("\\n", "\n")))
        .collect()
}

fn catalog(language: Language) -> Option<&'static HashMap<String, String>> {
    match language {
        Language::Uk => Some(UK.get_or_init(|| parse_catalog(UK_CATALOG))),
        Language::Ru => Some(RU.get_or_init(|| parse_catalog(RU_CATALOG))),
        Language::Es => Some(ES.get_or_init(|| parse_catalog(ES_CATALOG))),
        Language::Fr => Some(FR.get_or_init(|| parse_catalog(FR_CATALOG))),
        Language::Pt => Some(PT.get_or_init(|| parse_catalog(PT_CATALOG))),
        Language::De => Some(DE.get_or_init(|| parse_catalog(DE_CATALOG))),
        _ => None,
    }
}

/// Exact lookup at an explicit interface boundary; unknown text falls back to English.
pub fn tr(text: &str) -> &str {
    match catalog(CURRENT.get()) {
        Some(catalog) => catalog.get(text).map(String::as_str).unwrap_or(text),
        None => text,
    }
}

/// Context-specific interface label with the original English source as fallback.
pub fn tr_context<'a>(source: &'a str, key: &str) -> &'a str {
    match catalog(CURRENT.get()) {
        Some(catalog) => catalog.get(key).map(String::as_str).unwrap_or(source),
        None => source,
    }
}

pub fn placeholder_prompt(kind: deckcraft_model::PhType) -> &'static str {
    tr(kind.prompt())
}

/// Substitute already-formatted values without interpreting their contents as interface text.
pub fn format(source: &str, values: &[String]) -> String {
    let text = tr(source);
    let mut out = String::new();
    let mut rest = text;
    let mut index = 0;
    while let Some(start) = rest.find('{') {
        out.push_str(rest.get(..start).unwrap_or(""));
        let tail = rest.get(start + 1..).unwrap_or("");
        if let Some(after) = tail.strip_prefix('{') {
            out.push('{');
            rest = after;
        } else if let Some(end) = tail.find('}') {
            out.push_str(values.get(index).map(String::as_str).unwrap_or(""));
            index += 1;
            rest = tail.get(end + 1..).unwrap_or("");
        } else {
            out.push_str(rest);
            return out;
        }
    }
    out.push_str(&rest.replace("}}", "}"));
    out
}

/// "{n} {word}" with the word inflected for the interface language: English (one, other),
/// Ukrainian and Russian (one, few, many), Spanish, French, Portuguese and German (one, other;
/// French keeps the singular for 0).
#[allow(clippy::too_many_arguments)]
pub fn count(
    n: usize,
    english: (&str, &str),
    ukrainian: [&str; 3],
    russian: [&str; 3],
    spanish: (&str, &str),
    french: (&str, &str),
    portuguese: (&str, &str),
    german: (&str, &str),
) -> String {
    let word = match CURRENT.get() {
        Language::Uk => slavic(n, ukrainian),
        Language::Ru => slavic(n, russian),
        Language::Es => one_other(n, spanish),
        Language::Fr => if n <= 1 { french.0 } else { french.1 },
        Language::Pt => one_other(n, portuguese),
        Language::De => one_other(n, german),
        Language::En | Language::System => if n == 1 { english.0 } else { english.1 },
    };
    format!("{n} {word}")
}

/// Ukrainian/Russian integer forms: 1/21; 2–4/22–24; 0/5–19.
fn slavic<'a>(n: usize, forms: [&'a str; 3]) -> &'a str {
    match (n % 10, n % 100) {
        (1, last) if last != 11 => forms[0],
        (2..=4, last) if !(12..=14).contains(&last) => forms[1],
        _ => forms[2],
    }
}

fn one_other<'a>(n: usize, forms: (&'a str, &'a str)) -> &'a str {
    if n == 1 { forms.0 } else { forms.1 }
}

/// Recovery notice includes object forms per language.
pub fn recovered(n: usize) -> String {
    let amount = count(
        n,
        ("unsaved presentation", "unsaved presentations"),
        ["незбережену презентацію", "незбережені презентації", "незбережених презентацій"],
        ["несохраненную презентацию", "несохраненные презентации", "несохраненных презентаций"],
        ("presentación sin guardar", "presentaciones sin guardar"),
        ("présentation non enregistrée", "présentations non enregistrées"),
        ("apresentação não salva", "apresentações não salvas"),
        ("nicht gespeicherte Präsentation", "nicht gespeicherte Präsentationen"),
    );
    format("Recovered {} from the last session", &[amount])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The languages that ship a catalog.
    const TRANSLATED: [Language; 6] = [Language::Uk, Language::Ru, Language::Es, Language::Fr, Language::Pt, Language::De];

    fn placeholders(text: &str) -> Vec<&str> {
        let mut out = vec![];
        let mut rest = text;
        while let Some((_, tail)) = rest.split_once('{') {
            if let Some(tail) = tail.strip_prefix('{') {
                rest = tail;
                continue;
            }
            if let Some((field, tail)) = tail.split_once('}') {
                out.push(field);
                rest = tail;
            } else {
                break;
            }
        }
        out
    }

    #[test]
    fn all_command_menu_and_gallery_labels_are_translated() {
        for language in TRANSLATED {
            let catalog = catalog(language).unwrap();
            let mut required = vec![];
            required.extend(crate::UI_COMMANDS.iter().map(|c| c.1));
            required.extend(deckcraft_engine::Session::new().commands().into_iter().map(|c| c.label));
            for (menu, items) in crate::menus::menu_tree() {
                required.push(menu);
                required.extend(items.into_iter().filter(|(label, _)| *label != "-").map(|(label, _)| label));
            }
            required.extend(deckcraft_geom::preset::CATALOG.iter().map(|shape| shape.label));
            required.extend(deckcraft_geom::preset::Category::ALL.iter().map(|category| category.label()));
            required.extend(deckcraft_model::anim::ANIMATIONS.iter().map(|effect| effect.1));
            required.extend(deckcraft_model::anim::TRANSITIONS.iter().flat_map(|effect| [effect.1, effect.2]));
            required.extend(deckcraft_model::chart::ChartType::MAIN.iter().map(|kind| kind.label()));
            required.extend(deckcraft_model::defaults::SLIDE_SIZES.iter().map(|size| size.0));
            required.extend(deckcraft_color::STANDARD_COLORS.iter().map(|color| color.1));
            required.extend(deckcraft_color::SchemeSlot::THEME.iter().map(|slot| slot.label()));
            required.extend(["@ribbon:Home", "@ribbon:Insert"]);
            for option in deckcraft_model::anim::TRANSITIONS
                .iter()
                .flat_map(|effect| effect.4)
                .chain(deckcraft_model::anim::ANIMATIONS.iter().flat_map(|effect| effect.5))
            {
                let label = crate::ribbon::option_label(option);
                if !label.is_empty() && !label.chars().all(|c| c.is_ascii_digit()) {
                    assert!(catalog.contains_key(&label), "missing effect option: {option}: {label}");
                }
            }
            let missing: Vec<_> = required.into_iter().filter(|key| !catalog.contains_key(*key)).collect();
            assert!(missing.is_empty(), "missing visible metadata in {language:?}: {missing:?}");
        }
    }

    #[test]
    fn every_literal_translation_call_has_a_catalog_entry() {
        for source in [
            include_str!("canvas.rs"),
            include_str!("credits.rs"),
            include_str!("dialogs.rs"),
            include_str!("fillui.rs"),
            include_str!("lib.rs"),
            include_str!("media.rs"),
            include_str!("menus.rs"),
            include_str!("panes.rs"),
            include_str!("ribbon.rs"),
            include_str!("show.rs"),
            include_str!("sorter.rs"),
            include_str!("status.rs"),
            include_str!("thumbs.rs"),
            include_str!("widgets.rs"),
            include_str!("../../../apps/deckcraft/src/main.rs"),
            include_str!("../../../apps/deckcraft-web/src/web.rs"),
        ] {
            for prefix in ["i18n::tr(\"", "i18n::format(\""] {
                let mut rest = source.split("#[cfg(test)]").next().unwrap_or(source);
                while let Some((_, tail)) = rest.split_once(prefix) {
                    let mut escaped = false;
                    let end = tail
                        .char_indices()
                        .find_map(|(index, c)| {
                            if escaped {
                                escaped = false;
                                None
                            } else if c == '\\' {
                                escaped = true;
                                None
                            } else if c == '\"' {
                                Some(index)
                            } else {
                                None
                            }
                        })
                        .unwrap();
                    let key: String = serde_json::from_str(&format!("\"{}\"", &tail[..end])).unwrap();
                    for language in TRANSLATED {
                        assert!(
                            catalog(language).unwrap().contains_key(&key),
                            "missing literal translation in {language:?}: {key}"
                        );
                    }
                    rest = &tail[end + 1..];
                }
            }
        }
    }

    #[test]
    fn language_command_keeps_document_data_and_identifiers_intact() {
        let mut app = crate::SlideApp::new(deckcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", serde_json::json!({})).unwrap();
        app.run("insert.textBox", serde_json::json!({"text": "File {customer} / Звіт", "rect": [1, 2, 100, 40]})).unwrap();
        let before = serde_json::to_value(&app.session.active().unwrap().doc).unwrap();
        app.run("app.language", serde_json::json!({"language": "uk"})).unwrap();
        assert_eq!(app.ui.interface_language, Language::Uk);
        assert_eq!(tr("File"), "Файл");
        assert_eq!(before, serde_json::to_value(&app.session.active().unwrap().doc).unwrap());
        assert!(app.run("app.language", serde_json::json!({"language": "invalid"})).is_err());
        assert_eq!(app.ui.interface_language, Language::Uk);
        app.run("app.language", serde_json::json!({"language": "ru"})).unwrap();
        assert_eq!(app.ui.interface_language, Language::Ru);
        assert_eq!(tr("File"), "Файл");
        set_current(Language::En);
    }

    #[test]
    fn bundled_ui_fonts_cover_the_interface_alphabets_without_system_fonts() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Name("bold".into()), egui::FontFamily::Monospace] {
                // egui 0.36 has_glyph() can report a valid fallback face as a replacement.
                // Check the configured fallback and its actual font charmap below instead.
                let fallback_installed = ui.ctx().fonts_mut(|fonts| {
                    fonts.definitions().families.get(&family).is_some_and(|names| names.iter().any(|name| name == "Ubuntu-Light"))
                });
                assert!(fallback_installed, "missing bundled Ukrainian fallback in {family:?}");
            }
        });
        output.textures_delta.clear();
        let db = deckcraft_fonts::FontDb::with_font_dirs(vec![]);
        let face = db.face(deckcraft_fonts::LAST_RESORT_FAMILY, "Regular");
        let letters: std::collections::HashSet<_> = TRANSLATED
            .into_iter()
            .filter_map(catalog)
            .flat_map(|catalog| catalog.values().flat_map(|value| value.chars()))
            .chain("ҐґЄєІіЇї".chars())
            .filter(|c| ('\u{0400}'..='\u{04ff}').contains(c))
            .collect();
        for c in letters {
            assert_ne!(face.glyph_for(c), 0, "missing document fallback glyph: {c}");
        }
    }

    // An explicit local visual check; the generated PNGs stay outside the repository.
    #[test]
    #[ignore = "writes translation review screenshots; run explicitly with DECKCRAFT_I18N_SHOTS"]
    fn translated_interface_preview() {
        let dir = std::env::var("DECKCRAFT_I18N_SHOTS").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        for (code, language) in
            [("en", Language::En), ("uk", Language::Uk), ("ru", Language::Ru), ("es", Language::Es), ("fr", Language::Fr), ("pt", Language::Pt), ("de", Language::De)]
        {
            for (name, dialog) in [("editor", None), ("preferences", Some("preferences")), ("table", Some("table"))] {
                let mut app = crate::SlideApp::new(deckcraft_engine::Session::new(), crate::Services::default());
                app.run("file.new", serde_json::json!({})).unwrap();
                app.ui.interface_language = language;
                app.dialog = dialog.map(crate::dialogs::Dialog::new);
                let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui(|ui| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                });
                harness.run_steps(6);
                harness.render().unwrap().save(format!("{dir}/{code}-{name}.png")).unwrap();
            }
        }
        set_current(Language::En);
    }

    #[test]
    fn locales_and_preferences_are_backward_compatible() {
        for (locale, expected) in [
            ("uk", Language::Uk),
            ("uk-UA", Language::Uk),
            ("uk_UA.UTF-8", Language::Uk),
            ("UK_ua@euro", Language::Uk),
            ("ru", Language::Ru),
            ("ru-RU", Language::Ru),
            ("es", Language::Es),
            ("es-MX", Language::Es),
            ("fr", Language::Fr),
            ("fr_FR.UTF-8", Language::Fr),
            ("pt", Language::Pt),
            ("pt-BR", Language::Pt),
            ("de", Language::De),
            ("de-AT", Language::De),
        ] {
            assert_eq!(Language::from_locale(locale), expected);
        }
        for locale in ["", "en-US", "ukrainian"] {
            assert_eq!(Language::from_locale(locale), Language::En);
        }
        assert_eq!(Language::Uk.resolve(Some("en")), Language::Uk);
        assert_eq!(Language::Ru.resolve(Some("uk")), Language::Ru);
        let old: crate::UiState = serde_json::from_str("{}").unwrap();
        assert_eq!(old.interface_language, Language::System);
        let mut ui = old;
        ui.interface_language = Language::Ru;
        let round_trip: crate::UiState = serde_json::from_str(&serde_json::to_string(&ui).unwrap()).unwrap();
        assert_eq!(round_trip.interface_language, Language::Ru);
    }

    #[test]
    fn catalog_is_unique_complete_and_preserves_placeholders() {
        let source_count = |text: &str| text.lines().filter(|line| !line.is_empty() && !line.starts_with('#')).count();
        let expected = source_count(UK_CATALOG);
        for (constant, text) in [
            (UK_CATALOG, "uk"),
            (RU_CATALOG, "ru"),
            (ES_CATALOG, "es"),
            (FR_CATALOG, "fr"),
            (PT_CATALOG, "pt"),
            (DE_CATALOG, "de"),
        ] {
            assert_eq!(source_count(constant), expected, "{text} catalog row count");
            let mut seen = std::collections::HashSet::new();
            for (line_no, line) in constant.lines().enumerate() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let (source, text) = line.split_once('\t').unwrap();
                assert!(seen.insert(source), "{text}: duplicate on {}: {source}", line_no + 1);
                assert!(!text.trim().is_empty(), "{text}: empty translation: {source}");
                assert_eq!(placeholders(source), placeholders(text), "{text}: placeholder names/order/format: {source}");
            }
        }
    }

    #[test]
    fn lookup_and_format_preserve_user_values_and_fallbacks() {
        set_current(Language::Uk);
        assert_eq!(tr("File"), "Файл");
        assert_eq!(tr_context("Insert", "@ribbon:Insert"), "Вставлення");
        assert_eq!(tr("Insert"), "Вставити");
        assert_eq!(tr("a user-defined slide title"), "a user-defined slide title");
        let user = "File {user} / Звіт.pptx".to_string();
        assert!(format("Saved {path}", std::slice::from_ref(&user)).ends_with(&user));
        set_current(Language::Ru);
        assert_eq!(tr("File"), "Файл");
        set_current(Language::En);
        assert_eq!(tr("File"), "File");
    }

    #[test]
    fn ukrainian_integer_counts_cover_teens_and_large_values() {
        set_current(Language::Uk);
        for (n, word) in [
            (0, "слайдів"),
            (1, "слайд"),
            (2, "слайди"),
            (4, "слайди"),
            (5, "слайдів"),
            (11, "слайдів"),
            (12, "слайдів"),
            (14, "слайдів"),
            (21, "слайд"),
            (24, "слайди"),
            (111, "слайдів"),
        ] {
            assert_eq!(
                count(n, ("slide", "slides"), ["слайд", "слайди", "слайдів"], ["слайд", "слайда", "слайдов"], ("diapositiva", "diapositivas"), ("diapositive", "diapositives"), ("slide", "slides"), ("Folie", "Folien")),
                format!("{n} {word}")
            );
        }
        set_current(Language::En);
    }

    #[test]
    fn russian_and_western_counts_use_their_own_forms() {
        let forms = |n| count(n, ("slide", "slides"), ["слайд", "слайди", "слайдів"], ["слайд", "слайда", "слайдов"], ("diapositiva", "diapositivas"), ("diapositive", "diapositives"), ("slide", "slides"), ("Folie", "Folien"));
        set_current(Language::Ru);
        for (n, word) in [
            (0, "слайдов"),
            (1, "слайд"),
            (2, "слайда"),
            (4, "слайда"),
            (5, "слайдов"),
            (11, "слайдов"),
            (12, "слайдов"),
            (14, "слайдов"),
            (21, "слайд"),
            (22, "слайда"),
            (24, "слайда"),
            (25, "слайдов"),
            (111, "слайдов"),
        ] {
            assert_eq!(forms(n), format!("{n} {word}"));
        }
        set_current(Language::De);
        assert_eq!(forms(0), "0 Folien");
        assert_eq!(forms(1), "1 Folie");
        assert_eq!(forms(3), "3 Folien");
        set_current(Language::Fr);
        assert_eq!(forms(0), "0 diapositive");
        assert_eq!(forms(1), "1 diapositive");
        assert_eq!(forms(3), "3 diapositives");
        set_current(Language::Es);
        assert_eq!(forms(0), "0 diapositivas");
        assert_eq!(forms(1), "1 diapositiva");
        set_current(Language::En);
    }
}
