//! Interface localisation: the language setting and the string catalogs.
//!
//! English is the source language and the key space: a lookup takes the English text (or a
//! stable id — command, dialog and page ids), returns the translation for the current
//! language, and falls back to the key itself when anything is missing, so an untranslated
//! string is simply the English one and nothing can fail. The catalogs live in [`vi`].
//!
//! The engine keeps every stored string English (command labels, CLI and MCP output, file
//! contents); translation happens at the UI's draw sites via [`tr`] / [`tr_id`]. The current
//! language is a process-wide setting the desktop app applies at start-up
//! ([`apply`]) and when Preferences changes it; headless callers never touch it, so the
//! CLI and MCP stay English whatever the system locale is.

use std::borrow::Cow;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Deserializer, Serialize};

mod vi;

/// The interface languages DeckCraft ships.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Vi,
}

impl Language {
    /// Every language in the menu, in menu order.
    pub const ALL: [Language; 2] = [Language::En, Language::Vi];

    /// The BCP-47-ish code stored in `prefs.json`.
    pub fn code(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Vi => "vi",
        }
    }

    /// The language's own name, always shown in that language ("English", "Tiếng Việt").
    pub fn native_name(self) -> &'static str {
        match self {
            Language::En => "English",
            Language::Vi => "Tiếng Việt",
        }
    }

    /// A stored code (`"vi"`, `"vi-VN"`, `"vi_VN"`) to a language; unknown codes are `None`
    /// so a hand-edited `prefs.json` falls back to the system instead of failing.
    pub fn from_code(code: &str) -> Option<Language> {
        let base = code.split(['-', '_', '.', '@']).next().unwrap_or(code);
        match base.trim().to_ascii_lowercase().as_str() {
            "en" => Some(Language::En),
            "vi" => Some(Language::Vi),
            _ => None,
        }
    }

    /// A locale string (`"vi_VN.UTF-8"`, `"en"`) to a language, for following the system:
    /// only languages with a catalog jump out, anything else is English.
    pub fn from_locale(locale: &str) -> Language {
        Language::from_code(locale).filter(|l| *l != Language::En).unwrap_or(Language::En)
    }

    /// The system language for "follow the system": `LC_ALL`, `LC_MESSAGES` then `LANG`,
    /// as POSIX and macOS set them. Platforms that set none of them (Windows, WASM) get
    /// English until the language is chosen in Preferences.
    pub fn system() -> Language {
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(v) = std::env::var(var)
                && !v.is_empty()
                && v != "C"
                && v != "POSIX"
            {
                return Language::from_locale(&v);
            }
        }
        Language::En
    }
}

/// The current interface language: 0 = [`Language::En`], 1 = [`Language::Vi`].
static CURRENT: AtomicU8 = AtomicU8::new(0);

fn to_u8(l: Language) -> u8 {
    match l {
        Language::En => 0,
        Language::Vi => 1,
    }
}

fn from_u8(v: u8) -> Language {
    match v {
        1 => Language::Vi,
        _ => Language::En,
    }
}

/// The language every [`tr`] call uses from now on. The desktop app calls this at start-up
/// and when Preferences changes the setting; tests use the `*_in` functions instead.
pub fn set_language(language: Language) {
    CURRENT.store(to_u8(language), Ordering::Relaxed);
}

/// The current interface language.
pub fn language() -> Language {
    from_u8(CURRENT.load(Ordering::Relaxed))
}

/// Apply a preference: `None` means follow the system language (issue #15).
pub fn apply(preference: Option<Language>) {
    set_language(preference.unwrap_or_else(Language::system));
}

/// The translation of the English `key` in the current language (the key itself in
/// English or when no translation exists).
pub fn tr(key: &str) -> Cow<'_, str> {
    tr_in(language(), key)
}

/// [`tr`] for an explicit language; English is the identity, so this never fails.
pub fn tr_in(language: Language, key: &str) -> Cow<'_, str> {
    match language {
        Language::En => Cow::Borrowed(key),
        Language::Vi => vi::by_text(key).map_or(Cow::Borrowed(key), Cow::Borrowed),
    }
}

/// Like [`tr`], but keyed by a stable id (command, dialog or page id) with the English
/// text as the fallback. Ids are what the catalogs are checked against (see the tests
/// below and `crates/ui-egui` for the UI-side ones), so renaming an English label never
/// loses a translation.
pub fn tr_id<'k>(id: &str, english: &'k str) -> Cow<'k, str> {
    tr_id_in(language(), id, english)
}

/// [`tr_id`] for an explicit language.
pub fn tr_id_in<'k>(language: Language, id: &str, english: &'k str) -> Cow<'k, str> {
    match language {
        Language::En => Cow::Borrowed(english),
        Language::Vi => vi::by_id(id).map_or(Cow::Borrowed(english), Cow::Borrowed),
    }
}

/// Whether the catalogs have a translation for `id` (command, dialog or page). Used by the
/// tests that keep every new command translatable; English trivially has it all.
pub fn has_id_translation(id: &str) -> bool {
    vi::by_id(id).is_some()
}

/// Whether the catalogs have a translation for the English text `key`.
pub fn has_text_translation(key: &str) -> bool {
    vi::by_text(key).is_some()
}

/// `Prefs::language` from JSON: a code, `null`, or anything else (→ `None`, follow the
/// system). A damaged value must not discard the rest of `prefs.json`.
pub fn de_language<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Language>, D::Error> {
    struct V;
    impl<'de> serde::de::Visitor<'de> for V {
        type Value = Option<Language>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a language code such as \"vi\", or null")
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(Language::from_code(v))
        }
        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        // A number, bool, list or object is not a language; keep the preferences usable.
        fn visit_bool<E: serde::de::Error>(self, _v: bool) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_u64<E: serde::de::Error>(self, _v: u64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_i64<E: serde::de::Error>(self, _v: i64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_f64<E: serde::de::Error>(self, _v: f64) -> Result<Self::Value, E> {
            Ok(None)
        }
        // Drained to the end: a half-consumed value would leave the outer reader mid-stream.
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut v: A) -> Result<Self::Value, A::Error> {
            while v.next_element::<serde::de::IgnoredAny>()?.is_some() {}
            Ok(None)
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(self, mut v: A) -> Result<Self::Value, A::Error> {
            while v.next_key::<serde::de::IgnoredAny>()?.is_some() {
                v.next_value::<serde::de::IgnoredAny>()?;
            }
            Ok(None)
        }
    }
    d.deserialize_any(V)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// The catalog every registered command can be translated — the check that a new
    /// command lands with its Vietnamese label (and, later, with the other languages').
    #[test]
    fn every_command_label_is_translated() {
        let session = crate::Session::with_new();
        let commands = session.commands();
        assert!(commands.len() >= 300, "registry looks thin: {}", commands.len());
        for c in &commands {
            let vi = vi::by_id(c.id).unwrap_or_else(|| panic!("no Vietnamese translation for command {}", c.id));
            assert!(!vi.trim().is_empty(), "empty Vietnamese translation for {}", c.id);
        }
    }

    /// A stored code parses; junk stays a usable preference instead of an error.
    #[test]
    fn language_codes_parse_leniently() {
        assert_eq!(Language::from_code("vi"), Some(Language::Vi));
        assert_eq!(Language::from_code("vi-VN"), Some(Language::Vi));
        assert_eq!(Language::from_code("vi_VN.UTF-8"), Some(Language::Vi));
        assert_eq!(Language::from_code("en_GB"), Some(Language::En));
        assert_eq!(Language::from_code("xx"), None);
        assert_eq!(Language::from_code(""), None);
        assert_eq!(Language::from_locale("vi_VN.UTF-8"), Language::Vi);
        assert_eq!(Language::from_locale("en_US.UTF-8"), Language::En);
        assert_eq!(Language::from_locale("ja_JP.UTF-8"), Language::En, "no catalog yet, so English");
        assert_eq!(Language::native_name(Language::Vi), "Tiếng Việt");
        assert_eq!(Language::code(Language::Vi), "vi");
        assert_eq!(Language::ALL, [Language::En, Language::Vi]);
    }

    /// English is the identity; Vietnamese falls back to the English key rather than
    /// failing, and unknown ids/keys pass through untouched.
    #[test]
    fn lookups_fall_back_to_english() {
        assert_eq!(tr_in(Language::En, "OK"), "OK");
        assert_eq!(tr_in(Language::Vi, "OK"), "Đồng ý");
        assert_eq!(tr_in(Language::Vi, "Not in any catalog"), "Not in any catalog");
        assert_eq!(tr_id_in(Language::En, "format.bold", "Bold"), "Bold");
        assert_eq!(tr_id_in(Language::Vi, "format.bold", "Bold"), "Đậm");
        assert_eq!(tr_id_in(Language::Vi, "no.such.id", "Zork"), "Zork");
        // Two commands share an English label but not an id, so they translate apart.
        assert_eq!(tr_id_in(Language::Vi, "format.align", "Align"), "Căn lề");
        assert_eq!(tr_id_in(Language::Vi, "arrange.align", "Align"), "Căn chỉnh");
        assert!(has_id_translation("format.bold"));
        assert!(!has_id_translation("no.such.id"));
        assert!(has_text_translation("OK"));
        assert!(!has_text_translation("Not in any catalog"));
    }

    /// The process-wide language only changes when the app says so.
    #[test]
    fn set_and_read_the_current_language() {
        let before = language();
        set_language(Language::Vi);
        assert_eq!(language(), Language::Vi);
        set_language(Language::En);
        assert_eq!(language(), Language::En);
        set_language(before);
        assert_eq!(language(), before);
    }

    /// `prefs.json` carries the language; every damaged value is `None` (follow the
    /// system) and leaves the other preferences standing.
    #[test]
    fn prefs_tolerate_any_language_value() {
        let p: crate::Prefs = serde_json::from_str(r#"{"language": "vi", "gridSpacing": 9.0}"#).unwrap();
        assert_eq!(p.language, Some(Language::Vi));
        assert_eq!(p.grid_spacing, 9.0);
        for junk in [r#"{"language": "xx"}"#, r#"{"language": null}"#, r#"{"language": 42}"#, r#"{"language": {"a": 1}}"#, r#"{"language": ["vi"]}"#]
        {
            let p: crate::Prefs = serde_json::from_str(junk).expect(junk);
            assert_eq!(p.language, None, "{junk}");
            assert_eq!(p.grid_spacing, crate::Prefs::default().grid_spacing, "{junk}");
        }
        let p: crate::Prefs = serde_json::from_str("{}").unwrap();
        assert_eq!(p.language, None);
        // The written form stays a plain code.
        let p = crate::Prefs { language: Some(Language::Vi), ..crate::Prefs::default() };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""language":"vi""#), "{json}");
    }
}
