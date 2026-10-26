use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh-TW", alias = "zh-tw", alias = "zh-Hant")]
    TraditionalChinese,
}

impl Language {
    pub fn label_in(self, interface: Self) -> &'static str {
        if interface == Self::English && self == Self::TraditionalChinese { "Traditional Chinese (Taiwan)" } else { self.label() }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::TraditionalChinese => "繁體中文",
        }
    }
}

type Catalog = HashMap<String, String>;

fn traditional_chinese_catalog() -> Option<&'static Catalog> {
    static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(include_str!("../locales/zh-TW.json")).map_err(|error| {
                log::error!("could not load the zh-TW UI catalog: {error}");
                error.to_string()
            })
        })
        .as_ref()
        .ok()
}

pub fn translate<'a>(language: Language, source: &'a str) -> Cow<'a, str> {
    if language == Language::TraditionalChinese
        && let Some(translated) = traditional_chinese_catalog().and_then(|catalog| catalog.get(source))
    {
        return Cow::Borrowed(translated);
    }
    Cow::Borrowed(source)
}

pub fn format_text(language: Language, source: &str, replacements: &[(&str, String)]) -> String {
    let mut result = translate(language, source).into_owned();
    for (name, value) in replacements {
        result = result.replace(&format!("{{{name}}}"), value);
    }
    result
}

fn context_id() -> egui::Id {
    egui::Id::unique("deckcraft.ui.language")
}

pub fn set_context_language(ctx: &egui::Context, language: Language) {
    ctx.data_mut(|data| data.insert_temp(context_id(), language));
}

pub fn context_language(ctx: &egui::Context) -> Language {
    ctx.data(|data| data.get_temp::<Language>(context_id())).unwrap_or_default()
}

/// Discover glyphs before painting translated chrome, through the existing system-font policy.
pub fn prepare_fonts(ctx: &egui::Context, language: Language, fonts: &mut crate::uifonts::UiFonts) -> bool {
    let id = egui::Id::unique("deckcraft.ui.language.fonts");
    let previous = ctx.data(|data| data.get_temp::<Language>(id));
    if previous == Some(language) {
        return false;
    }
    ctx.data_mut(|data| data.insert_temp(id, language));
    if language == Language::English {
        return previous.is_some();
    }
    if language == Language::TraditionalChinese {
        fonts.prefer_taiwan();
    }
    fonts.note(ctx, include_str!("../locales/zh-TW.json"));
    true
}

pub fn text<'a>(ctx: &egui::Context, source: &'a str) -> Cow<'a, str> {
    translate(context_language(ctx), source)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UI_SOURCES: &[&str] = &[
        include_str!("backstage.rs"),
        include_str!("canvas.rs"),
        include_str!("credits.rs"),
        include_str!("dialogs.rs"),
        include_str!("fillui.rs"),
        include_str!("menus.rs"),
        include_str!("panes.rs"),
        include_str!("ribbon.rs"),
        include_str!("selpane.rs"),
        include_str!("show.rs"),
        include_str!("sorter.rs"),
        include_str!("status.rs"),
        include_str!("thumbs.rs"),
        include_str!("toast.rs"),
        include_str!("widgets.rs"),
    ];

    fn static_call_argument(source: &str, call_start: usize, name: &str, index: usize) -> Option<String> {
        let open = call_start + name.len();
        let mut depth = 0usize;
        let mut quoted = false;
        let mut escaped = false;
        let mut start = open + 1;
        let mut arguments = Vec::new();
        for (offset, ch) in source[open + 1..].char_indices() {
            let position = open + 1 + offset;
            if quoted {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    quoted = false;
                }
            } else {
                match ch {
                    '"' => quoted = true,
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' if depth > 0 => depth -= 1,
                    ',' if depth == 0 => {
                        arguments.push(source[start..position].trim());
                        start = position + 1;
                    }
                    ')' => {
                        arguments.push(source[start..position].trim());
                        break;
                    }
                    _ => {}
                }
            }
        }
        serde_json::from_str::<String>(arguments.get(index)?).ok()
    }

    #[test]
    fn traditional_chinese_catalog_has_translated_values() {
        let catalog = traditional_chinese_catalog().expect("catalog is valid JSON");
        assert!(!catalog.is_empty());
        assert!(catalog.values().all(|value| !value.trim().is_empty()));
        assert_eq!(translate(Language::TraditionalChinese, "File"), "檔案");
    }

    #[test]
    fn english_and_unknown_strings_fall_back_to_source() {
        assert_eq!(translate(Language::English, "File"), "File");
        assert_eq!(translate(Language::TraditionalChinese, "Uncatalogued"), "Uncatalogued");
    }

    #[test]
    fn language_codes_round_trip() {
        assert_eq!(serde_json::to_string(&Language::TraditionalChinese).ok().as_deref(), Some("\"zh-TW\""));
        assert_eq!(serde_json::from_str::<Language>("\"zh-TW\"").ok(), Some(Language::TraditionalChinese));
        assert_eq!(serde_json::from_str::<Language>("\"zh-Hant\"").ok(), Some(Language::TraditionalChinese));
    }

    #[test]
    fn language_survives_ui_state_persistence() {
        let state = crate::UiState { language: Language::TraditionalChinese, ..Default::default() };
        let saved = serde_json::to_value(state).unwrap();
        let loaded: crate::UiState = serde_json::from_value(saved).unwrap();
        assert_eq!(loaded.language, Language::TraditionalChinese);
        let old: crate::UiState = serde_json::from_str("{}").unwrap();
        assert_eq!(old.language, Language::English);
    }

    #[test]
    fn catalog_covers_every_engine_and_ui_command_label() {
        let catalog = traditional_chinese_catalog().expect("catalog is valid JSON");
        let mut missing = Vec::new();
        for command in deckcraft_engine::Session::new().commands() {
            if !catalog.contains_key(command.label) {
                missing.push(command.label.to_owned());
            }
        }
        for (_, label, _, _) in crate::UI_COMMANDS {
            if !catalog.contains_key(*label) {
                missing.push((*label).to_owned());
            }
        }
        assert!(missing.is_empty(), "missing zh-TW command labels: {missing:?}");
    }

    #[test]
    fn catalog_covers_every_static_ui_translation_call() {
        let catalog = traditional_chinese_catalog().expect("catalog is valid JSON");
        for source in UI_SOURCES {
            for (start, _) in source.match_indices("locale::text(") {
                if let Some(key) = static_call_argument(source, start, "locale::text", 1) {
                    if key.trim().is_empty() || key == "DeckCraft" || key == "RRGGBB" || !key.chars().any(char::is_alphanumeric) {
                        continue;
                    }
                    assert!(catalog.contains_key(key.as_str()), "missing zh-TW UI translation: {key}");
                }
            }
        }
        let backstage = include_str!("backstage.rs");
        for (name, indices) in [
            ("heading", &[1usize][..]),
            ("dim", &[1]),
            ("arrow_link", &[1]),
            ("search", &[2]),
            ("tile", &[1]),
            ("square_button", &[2]),
            ("place_row", &[2]),
            ("task", &[2, 3, 4]),
        ] {
            for (start, _) in backstage.match_indices(&format!("{name}(")) {
                for index in indices {
                    if let Some(key) = static_call_argument(backstage, start, name, *index)
                        && key.trim().chars().any(char::is_alphanumeric)
                        && key != "DeckCraft"
                    {
                        assert!(catalog.contains_key(key.as_str()), "missing zh-TW Backstage translation: {key}");
                    }
                }
            }
        }
    }

    #[test]
    fn catalog_has_no_duplicate_keys() {
        let mut keys = std::collections::HashSet::new();
        for line in include_str!("../locales/zh-TW.json").lines() {
            // A key may itself contain a colon or an escaped quotation mark.
            let line = line.trim();
            if !line.starts_with('"') {
                continue;
            }
            let mut stream = serde_json::Deserializer::from_str(line).into_iter::<String>();
            let key = stream.next().expect("key exists").expect("valid JSON key");
            assert!(keys.insert(key.clone()), "duplicate zh-TW catalog key: {key}");
        }
    }

    #[test]
    fn format_text_uses_localized_template_and_named_values() {
        assert_eq!(
            format_text(Language::TraditionalChinese, "Slide {number} of {total}", &[("number", "2".into()), ("total", "8".into())]),
            "第 2 張投影片，共 8 張"
        );
    }
}
