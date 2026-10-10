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
}

impl Language {
    pub const ALL: [Self; 3] = [Self::System, Self::En, Self::Uk];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => tr("System language"),
            Self::En => "English",
            Self::Uk => "Українська",
        }
    }

    pub fn from_locale(locale: &str) -> Self {
        let language = locale.split(['-', '_', '.', '@']).next().unwrap_or("");
        if language.eq_ignore_ascii_case("uk") { Self::Uk } else { Self::En }
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

const CATALOG: &str = include_str!("i18n/uk.tsv");
static UK: OnceLock<HashMap<String, String>> = OnceLock::new();

fn catalog() -> &'static HashMap<String, String> {
    UK.get_or_init(|| {
        CATALOG
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| line.split_once('\t'))
            .map(|(source, text)| (source.replace("\\n", "\n"), text.replace("\\n", "\n")))
            .collect()
    })
}

/// Exact lookup at an explicit interface boundary; unknown text falls back to English.
pub fn tr(text: &str) -> &str {
    if CURRENT.get() == Language::Uk { catalog().get(text).map(String::as_str).unwrap_or(text) } else { text }
}

/// Context-specific interface label with the original English source as fallback.
pub fn tr_context<'a>(source: &'a str, key: &str) -> &'a str {
    if CURRENT.get() == Language::Uk { catalog().get(key).map(String::as_str).unwrap_or(source) } else { source }
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

/// Ukrainian integer count forms: 1/21; 2–4/22–24; 0/5–19.
pub fn count(n: usize, english: &str, one: &str, few: &str, many: &str) -> String {
    let word = if CURRENT.get() == Language::Uk {
        match (n % 10, n % 100) {
            (1, last) if last != 11 => one,
            (2..=4, last) if !(12..=14).contains(&last) => few,
            _ => many,
        }
    } else {
        english
    };
    format!("{n} {word}")
}

/// Recovery notice includes accusative Ukrainian forms.
pub fn recovered(n: usize) -> String {
    let amount = count(
        n,
        if n == 1 { "unsaved presentation" } else { "unsaved presentations" },
        "незбережену презентацію",
        "незбережені презентації",
        "незбережених презентацій",
    );
    format("Recovered {} from the last session", &[amount])
}

#[cfg(test)]
mod tests {
    use super::*;

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
                assert!(catalog().contains_key(&label), "missing effect option: {option}: {label}");
            }
        }
        let missing: Vec<_> = required.into_iter().filter(|key| !catalog().contains_key(*key)).collect();
        assert!(missing.is_empty(), "missing visible metadata: {missing:?}");
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
                    assert!(catalog().contains_key(&key), "missing literal translation: {key}");
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
        set_current(Language::En);
    }

    #[test]
    fn bundled_ui_fonts_cover_the_ukrainian_alphabet_without_system_fonts() {
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
        let letters: std::collections::HashSet<_> =
            catalog().values().flat_map(|value| value.chars()).chain("ҐґЄєІіЇї".chars()).filter(|c| ('\u{0400}'..='\u{04ff}').contains(c)).collect();
        for c in letters {
            assert_ne!(face.glyph_for(c), 0, "missing document fallback glyph: {c}");
        }
    }

    // An explicit local visual check; the generated PNGs stay outside the repository.
    #[test]
    #[ignore = "writes translation review screenshots; run explicitly with DECKCRAFT_I18N_SHOTS"]
    fn ukrainian_interface_preview() {
        let dir = std::env::var("DECKCRAFT_I18N_SHOTS").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        for (code, language) in [("en", Language::En), ("uk", Language::Uk)] {
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
        for locale in ["uk", "uk-UA", "uk_UA.UTF-8", "UK_ua@euro"] {
            assert_eq!(Language::from_locale(locale), Language::Uk);
        }
        for locale in ["", "en-US", "ru-UA", "ukrainian"] {
            assert_eq!(Language::from_locale(locale), Language::En);
        }
        assert_eq!(Language::Uk.resolve(Some("en")), Language::Uk);
        let old: crate::UiState = serde_json::from_str("{}").unwrap();
        assert_eq!(old.interface_language, Language::System);
        let mut ui = old;
        ui.interface_language = Language::Uk;
        let round_trip: crate::UiState = serde_json::from_str(&serde_json::to_string(&ui).unwrap()).unwrap();
        assert_eq!(round_trip.interface_language, Language::Uk);
    }

    #[test]
    fn catalog_is_unique_complete_and_preserves_placeholders() {
        let mut seen = std::collections::HashSet::new();
        for (line_no, line) in CATALOG.lines().enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (source, text) = line.split_once('\t').unwrap();
            assert!(seen.insert(source), "duplicate on {}: {source}", line_no + 1);
            assert!(!text.trim().is_empty(), "empty translation: {source}");
            assert_eq!(placeholders(source), placeholders(text), "placeholder names/order/format: {source}");
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
            assert_eq!(count(n, "slides", "слайд", "слайди", "слайдів"), format!("{n} {word}"));
        }
        set_current(Language::En);
    }
}
