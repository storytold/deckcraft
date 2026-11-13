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
    PtBr,
}

impl Language {
    pub const ALL: [Self; 4] = [Self::System, Self::En, Self::Uk, Self::PtBr];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => tr("System language"),
            Self::En => "English",
            Self::Uk => "Українська",
            Self::PtBr => "Português (Brasil)",
        }
    }

    /// The `app.language` code and the saved preference value.
    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::En => "en",
            Self::Uk => "uk",
            Self::PtBr => "pt-BR",
        }
    }

    pub fn from_locale(locale: &str) -> Self {
        let language = locale.split(['-', '_', '.', '@']).next().unwrap_or("");
        if language.eq_ignore_ascii_case("uk") {
            Self::Uk
        } else if language.eq_ignore_ascii_case("pt") {
            Self::PtBr
        } else {
            Self::En
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

const UK_SOURCE: &str = include_str!("i18n/uk.tsv");
const PT_BR_SOURCE: &str = include_str!("i18n/pt-br.tsv");
static UK: OnceLock<HashMap<String, String>> = OnceLock::new();
static PT_BR: OnceLock<HashMap<String, String>> = OnceLock::new();

fn parse(source: &str) -> HashMap<String, String> {
    source
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
        .map(|(source, text)| (source.replace("\\n", "\n"), text.replace("\\n", "\n")))
        .collect()
}

/// The catalog of a language that has one. English — and a `System` preference before it resolves —
/// has none, so its text stays as written.
pub fn catalog(language: Language) -> Option<&'static HashMap<String, String>> {
    match language {
        Language::Uk => Some(UK.get_or_init(|| parse(UK_SOURCE))),
        Language::PtBr => Some(PT_BR.get_or_init(|| parse(PT_BR_SOURCE))),
        _ => None,
    }
}

fn active_catalog() -> Option<&'static HashMap<String, String>> {
    catalog(CURRENT.get())
}

/// Exact lookup at an explicit interface boundary; unknown text falls back to English.
pub fn tr(text: &str) -> &str {
    active_catalog().and_then(|c| c.get(text)).map(String::as_str).unwrap_or(text)
}

/// Context-specific interface label with the original English source as fallback.
pub fn tr_context<'a>(source: &'a str, key: &str) -> &'a str {
    active_catalog().and_then(|c| c.get(key)).map(String::as_str).unwrap_or(source)
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

/// Integer count of a noun. `english` is the English form the caller already chose; `uk` carries the
/// three Ukrainian forms (`one|few|many`) and `pt` the two Portuguese forms (`one|other`), both
/// pipe-separated. A language without forms of its own falls back to English.
pub fn count(n: usize, english: &str, uk: &str, pt: &str) -> String {
    let selected = match CURRENT.get() {
        Language::Uk => Some((
            uk,
            match (n % 10, n % 100) {
                (1, last) if last != 11 => 0,
                (2..=4, last) if !(12..=14).contains(&last) => 1,
                _ => 2,
            },
        )),
        Language::PtBr => Some((pt, usize::from(n > 1))),
        _ => None,
    };
    let word = selected.and_then(|(forms, index)| forms.split('|').nth(index)).unwrap_or(english);
    format!("{n} {word}")
}

/// Recovery notice includes the Ukrainian accusative forms and the Portuguese ones.
pub fn recovered(n: usize) -> String {
    let amount = count(
        n,
        if n == 1 { "unsaved presentation" } else { "unsaved presentations" },
        "незбережену презентацію|незбережені презентації|незбережених презентацій",
        "apresentação não salva|apresentações não salvas",
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
        // Brazilian Portuguese is the maintained interface translation here, so it must be
        // complete; a newer upstream label that Ukrainian has not caught up with is reported but
        // does not fail the build (it shows in English until it is translated).
        let mut missing = vec![];
        for language in [Language::Uk, Language::PtBr] {
            let catalog = catalog(language).unwrap();
            for option in deckcraft_model::anim::TRANSITIONS
                .iter()
                .flat_map(|effect| effect.4)
                .chain(deckcraft_model::anim::ANIMATIONS.iter().flat_map(|effect| effect.5))
            {
                let label = crate::ribbon::option_label(option);
                // Directions on the small option tiles ("L", "Ru", "Hex L") are glyph-like codes,
                // not words: they stay as drawn.
                let code = label.chars().count() <= 3;
                if !label.is_empty() && !code && !label.chars().all(|c| c.is_ascii_digit()) && !catalog.contains_key(&label) {
                    missing.push((language, label));
                }
            }
            missing.extend(required.iter().filter(|key| !catalog.contains_key(**key)).map(|key| (language, (*key).to_string())));
        }
        let pt: Vec<_> = missing.iter().filter(|(language, _)| *language == Language::PtBr).map(|(_, key)| key.as_str()).collect();
        assert!(pt.is_empty(), "pt-BR: missing visible metadata: {pt:?}");
        let uk = missing.iter().filter(|(language, _)| *language == Language::Uk).count();
        if uk > 0 {
            eprintln!("uk: {uk} interface labels still untranslated (fall back to English)");
        }
    }

    /// Every literal passed to `i18n::tr` / `i18n::format` in the UI crate and the apps: the
    /// crate's own sources (all modules, tests excluded) and the two app crates beside it.
    fn literal_translation_keys() -> Vec<String> {
        fn sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    sources(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && !path.file_name().is_some_and(|n| n.to_string_lossy().contains("test"))
                {
                    out.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut paths = vec![];
        sources(&root.join("src"), &mut paths);
        for app in ["../../apps/deckcraft/src", "../../apps/deckcraft-web/src"] {
            sources(&root.join(app), &mut paths);
        }
        let mut keys = vec![];
        for path in paths {
            let Ok(source) = std::fs::read_to_string(&path) else { continue };
            let source = source.split("#[cfg(test)]").next().unwrap_or(&source).to_string();
            for prefix in ["i18n::tr(\"", "i18n::format(\""] {
                let mut rest = source.as_str();
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
                    keys.push(key);
                    rest = &tail[end + 1..];
                }
            }
        }
        keys
    }

    #[test]
    fn every_literal_translation_call_has_a_catalog_entry() {
        let keys = literal_translation_keys();
        let pt = catalog(Language::PtBr).unwrap();
        let missing: Vec<_> = keys.iter().filter(|key| !pt.contains_key(*key)).collect();
        assert!(missing.is_empty(), "pt-BR: missing literal translation: {missing:?}");
        // Ukrainian predates the labels upstream added since; an untranslated one shows in
        // English, so it is reported, not fatal.
        let uk = catalog(Language::Uk).unwrap();
        let lagging = keys.iter().filter(|key| !uk.contains_key(*key)).count();
        if lagging > 0 {
            eprintln!("uk: {lagging} interface literals still untranslated (fall back to English)");
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
        let letters: std::collections::HashSet<_> = catalog(Language::Uk)
            .unwrap()
            .values()
            .flat_map(|value| value.chars())
            .chain("ҐґЄєІіЇї".chars())
            .filter(|c| ('\u{0400}'..='\u{04ff}').contains(c))
            .collect();
        for c in letters {
            assert_ne!(face.glyph_for(c), 0, "missing document fallback glyph: {c}");
        }
    }

    #[test]
    fn brazilian_portuguese_lookup_counts_and_fallbacks() {
        set_current(Language::PtBr);
        assert_eq!(tr("File"), "Arquivo");
        assert_eq!(tr("Insert"), "Inserir");
        assert_eq!(tr_context("Insert", "@ribbon:Insert"), "Inserir");
        assert_eq!(tr("a user-defined slide title"), "a user-defined slide title");
        assert_eq!(count(1, "slide", "слайд|слайди|слайдів", "slide|slides"), "1 slide");
        assert_eq!(count(3, "slides", "слайд|слайди|слайдів", "slide|slides"), "3 slides");
        for (n, word) in
            [(0, "apresentação não salva"), (1, "apresentação não salva"), (2, "apresentações não salvas"), (11, "apresentações não salvas")]
        {
            let text = recovered(n);
            assert!(text.contains(word), "{n}: {text}");
        }
        set_current(Language::En);
        assert_eq!(tr("File"), "File");
    }

    #[test]
    fn bundled_document_font_covers_the_brazilian_portuguese_catalog() {
        let db = deckcraft_fonts::FontDb::with_font_dirs(vec![]);
        let face = db.face(deckcraft_fonts::LAST_RESORT_FAMILY, "Regular");
        let letters: std::collections::HashSet<_> =
            catalog(Language::PtBr).unwrap().values().flat_map(|value| value.chars()).filter(|c| ('\u{00c0}'..='\u{00ff}').contains(c)).collect();
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
        for (code, language) in [("en", Language::En), ("uk", Language::Uk), ("pt-BR", Language::PtBr)] {
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
    fn catalogs_are_unique_complete_and_preserve_placeholders() {
        for (code, source) in [("uk", UK_SOURCE), ("pt-BR", PT_BR_SOURCE)] {
            let mut seen = std::collections::HashSet::new();
            for (line_no, line) in source.lines().enumerate() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let (source, text) = line.split_once('\t').unwrap();
                assert!(seen.insert(source), "{code}: duplicate on {}: {source}", line_no + 1);
                assert!(!text.trim().is_empty(), "{code}: empty translation: {source}");
                assert_eq!(placeholders(source), placeholders(text), "{code}: placeholder names/order/format: {source}");
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
            assert_eq!(count(n, "slides", "слайд|слайди|слайдів", "slide|slides"), format!("{n} {word}"));
        }
        set_current(Language::En);
    }
}
