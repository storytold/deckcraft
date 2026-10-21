//! Display-only translations. Commands, document data and user text keep their values.
use std::{cell::Cell, collections::HashMap, sync::OnceLock};
use serde::{Deserialize, Serialize};

mod ko;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "ko")]
    Korean,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::English, Self::Korean];
    pub fn code(self) -> &'static str { match self { Self::English => "en", Self::Korean => "ko" } }
    pub fn name(self) -> &'static str { match self { Self::English => "English", Self::Korean => "한국어" } }
    pub fn from_code(code: &str) -> Option<Self> {
        match code { "en" => Some(Self::English), "ko" => Some(Self::Korean), _ => None }
    }
}

thread_local! { static CURRENT: Cell<Language> = const { Cell::new(Language::English) }; }
pub fn set_current(language: Language) { CURRENT.set(language); }
pub fn is_korean() -> bool { CURRENT.get() == Language::Korean }

fn catalog() -> &'static HashMap<&'static str, &'static str> {
    static KO: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    KO.get_or_init(|| ko::TEXT.iter().copied().collect())
}

pub fn tr(source: &str) -> &str {
    if is_korean() { catalog().get(source).copied().unwrap_or(source) } else { source }
}

/// Look up a command's label without changing the command identifier.
pub fn command<'a>(id: &str, english: &'a str) -> &'a str {
    if is_korean() {
        ko::COMMANDS.iter().find(|(key, _, _)| *key == id).map_or_else(|| tr(english), |(_, _, label)| *label)
    } else { english }
}

pub fn path(source: &str) -> String {
    let separator = if source.contains(" > ") { " > " } else { " › " };
    source.split(separator).map(tr).collect::<Vec<_>>().join(separator)
}

/// Render preformatted values in one pass, so braces inside a user value stay literal.
/// The caller uses Rust's original format! in English and formats each field in Korean.
pub fn format_fields(source: &str, args: &[(&str, String)]) -> String {
    let template = tr(source);
    let mut output = String::new();
    let mut rest = template;
    let mut anonymous = 0;
    while let Some(at) = rest.find(['{', '}']) {
        output.push_str(&rest[..at]);
        rest = &rest[at..];
        if rest.starts_with("{{") { output.push('{'); rest = &rest[2..]; continue; }
        if rest.starts_with("}}") { output.push('}'); rest = &rest[2..]; continue; }
        if rest.starts_with('{') && let Some(end) = rest.find('}') {
            let key = &rest[1..end];
            let value = if key.is_empty() || key.starts_with(':') {
                let value = args.iter().filter(|(k, _)| k.is_empty() || k.starts_with(':')).nth(anonymous);
                anonymous += 1;
                value
            } else { args.iter().find(|(k, _)| *k == key) };
            if let Some((_, value)) = value { output.push_str(value); }
            else { output.push_str(&rest[..=end]); }
            rest = &rest[end + 1..];
        } else { output.push_str(&rest[..1]); rest = &rest[1..]; }
    }
    output.push_str(rest);
    output
}

pub fn korean_sample() -> &'static str {
    static SAMPLE: OnceLock<String> = OnceLock::new();
    SAMPLE.get_or_init(|| ko::TEXT.iter().map(|(_, value)| *value).collect::<Vec<_>>().join(""))
}

pub fn english_ending(value: &str) -> &str { if is_korean() { "" } else { value } }

pub fn chooser(ui: &mut egui::Ui, language: Language) -> Option<Language> {
    let mut selected = language;
    egui::ComboBox::from_id_salt("interface_language")
        .selected_text(language.name())
        .show_ui(ui, |ui| {
            for option in Language::ALL { ui.selectable_value(&mut selected, option, option.name()); }
        });
    (selected != language).then_some(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_round_trips_without_touching_user_values() {
        assert_eq!(serde_json::from_str::<Language>("\"ko\"").unwrap(), Language::Korean);
        assert_eq!(serde_json::to_string(&Language::English).unwrap(), "\"en\"");
        assert!(Language::from_code("unknown").is_none());
        set_current(Language::English);
        assert_eq!(tr("File"), "File");
        set_current(Language::Korean);
        assert_eq!(tr("File"), "파일");
        assert_eq!(tr("user {name}.txt"), "user {name}.txt");
        assert_eq!(format_fields("{} contributors · {} commits", &[("", "A {x}".into()), ("", "2".into())]), "기여자 A {x}명 · 커밋 2개");
        assert_eq!(english_ending("s"), "");
        set_current(Language::English);
        assert_eq!(english_ending("s"), "s");
    }
    #[test]
    fn reviewed_catalog_has_unique_keys() {
        let mut seen = std::collections::HashSet::new();
        for (source, translated) in ko::TEXT {
            assert!(seen.insert(source), "duplicate source {source}");
            assert!(!translated.trim().is_empty());
        }
    }
}
