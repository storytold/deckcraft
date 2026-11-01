//! Interface languages. Strings in the code stay English and are the lookup keys; the craft-i18n
//! catalogs map them to display text when the UI is drawn. Command ids, document text, file names,
//! the control channel, the CLI and MCP never see translated text, so scripts and agents are
//! unaffected. A string without a translation shows in English, so coverage can grow one entry at
//! a time, and a missing translation never fails a build, a test or a release.
//!
//! The catalogs live in [storytold/craft-i18n](https://github.com/storytold/craft-i18n)
//! (`locales/<locale>/common.tsv` and `deckcraft.tsv`), not in this repository. `build.rs` embeds
//! them from `CRAFT_I18N_DIR` (the checkout at the SHA in `craft-i18n.rev`); without it the build is
//! English only. Only a locale that has a `deckcraft.tsv` is embedded, so a language appears in the
//! picker once its DeckCraft catalog exists. Lookup, fallback (`pt-br` → `pt` → English, app rows
//! over shared ones), plurals, language names and locale matching are the `craft-i18n` crate's
//! (craft-libs).
//!
//! The `language` setting is [`AUTO`] by default: the UI follows the operating system's language
//! (Windows display language, macOS preferred languages, `LANG`/`LC_*` elsewhere, the browser on
//! the web) when there is a catalog for it, and English otherwise. Preferences picks one by hand.
//!
//! # Adding or changing a translation
//! In craft-i18n: add rows to `locales/<locale>/deckcraft.tsv` (its `docs/integration.md`); they
//! show here once `craft-i18n.rev` moves to a commit that has them. A feature PR here adds the
//! English only.
//!
//! # Adding a language
//! 1. craft-i18n gets `locales/<code>/deckcraft.tsv` and the language's `@id language.name` row.
//! 2. Add one row to [`LANGUAGES`].
//!
//! The Preferences list and the system-language match pick it up from there.
//!
//! # Looking strings up
//! - [`tl!`](crate::tl) / [`t`]: a string in the current language. [`tr`]: in a given one.
//! - [`tc`] / [`trc`]: worded for a context, for text that must fit a tight spot.
//! - [`get`]: one context's translation only, never the plain row.
//! - [`fmt`]: fill `{name}` placeholders after a lookup; translators may reorder them.
//! - [`fmt_plain`]: [`fmt`] without bidi isolate marks, for text that goes into the document.

use std::cell::Cell;
use std::sync::{LazyLock, OnceLock};

use craft_i18n::I18n;

pub use craft_i18n::{fmt, fmt_plain};

include!(concat!(env!("OUT_DIR"), "/craft_i18n.rs"));

/// The embedded catalogs, offering exactly the languages in [`LANGUAGES`]. Built on first use;
/// a locale is parsed on its first lookup.
static I18N: LazyLock<I18n> = LazyLock::new(|| {
    let offered: Vec<&str> = LANGUAGES.iter().map(|l| l.code).collect();
    let (i18n, problems) = I18n::new(CRAFT_I18N, &offered);
    problems.iter().for_each(|p| log::warn!("interface catalogs: {p}"));
    i18n
});

/// The `language` setting that follows the system.
pub const AUTO: &str = "auto";

/// One interface language DeckCraft offers: what the app needs beyond the catalogs. Its name and
/// translations come from craft-i18n.
pub struct LangInfo {
    /// BCP 47 code, lowercase (`ja`, `zh-hans`). Also the `language` setting's value and the
    /// craft-i18n locale.
    pub code: &'static str,
}

/// The languages DeckCraft offers, when their catalogs are embedded. English first: it is the
/// fallback and the source language. Regional tags resolve to these by the crate's matching
/// (`es-MX` → `es`, `zh-TW` → `zh-hant`, `sr-Latn-*` → `sr-latn`).
pub static LANGUAGES: [LangInfo; 27] = [
    LangInfo { code: "en" },
    LangInfo { code: "zh-hans" },
    LangInfo { code: "zh-hant" },
    LangInfo { code: "ja" },
    LangInfo { code: "ko" },
    LangInfo { code: "uk" },
    LangInfo { code: "es" },
    // Brazilian Portuguese. Plain `pt` and `pt-PT` stay English ([`lang_from_tag`]): the
    // European vocabulary differs.
    LangInfo { code: "pt-br" },
    LangInfo { code: "sr" },
    LangInfo { code: "sr-latn" },
    LangInfo { code: "pl" },
    LangInfo { code: "cs" },
    LangInfo { code: "et" },
    LangInfo { code: "de" },
    LangInfo { code: "ru" },
    LangInfo { code: "tr" },
    LangInfo { code: "it" },
    LangInfo { code: "hu" },
    LangInfo { code: "fr" },
    LangInfo { code: "bg" },
    LangInfo { code: "vi" },
    LangInfo { code: "sv" },
    LangInfo { code: "hr" },
    LangInfo { code: "nl" },
    LangInfo { code: "da" },
    LangInfo { code: "ro" },
    LangInfo { code: "sl" },
];

/// A language the UI can be shown in: English, or an offered language whose catalogs are embedded.
#[derive(Clone, Copy)]
pub struct Lang {
    lang: craft_i18n::Lang,
    /// The catalogs it is looked up in: the embedded ones ([`I18N`]), or a test fixture's.
    catalogs: Option<&'static I18n>,
}

impl std::fmt::Debug for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Lang({})", self.code())
    }
}

impl PartialEq for Lang {
    fn eq(&self, other: &Self) -> bool {
        self.code() == other.code()
    }
}

impl Eq for Lang {}

impl Lang {
    pub const EN: Lang = Lang { lang: craft_i18n::Lang::EN, catalogs: None };

    /// The internal catalogs handle for this language.
    fn i18n(self) -> &'static I18n {
        self.catalogs.unwrap_or_else(|| LazyLock::force(&I18N))
    }

    /// Return the BCP 47 code for this language (e.g. "en", "ko", "zh-hans").
    pub fn code(self) -> &'static str {
        self.i18n().code(self.lang)
    }

    /// The language's name in itself (its `@id language.name` row), shown in the Preferences list.
    pub fn name(self) -> &'static str {
        self.i18n().name(self.lang)
    }

    /// Check whether this language prefers Simplified Chinese fonts.
    pub fn prefers_hans(self) -> bool {
        self.code() == "zh-hans"
    }

    /// Check whether this language prefers Korean fonts.
    pub fn prefers_korean(self) -> bool {
        self.code() == "ko"
    }

    /// Is the interface written in CJK script, so it needs a CJK face?
    pub fn uses_cjk(self) -> bool {
        self.name().chars().any(|c| ('\u{2E80}'..='\u{9FFF}').contains(&c) || ('\u{AC00}'..='\u{D7AF}').contains(&c))
    }

    /// A language by its exact code (any case): one of [`LANGUAGES`] whose catalogs are embedded.
    pub fn from_code(code: &str) -> Option<Lang> {
        let info = LANGUAGES.iter().find(|l| l.code.eq_ignore_ascii_case(code))?;
        I18N.from_code(info.code).map(|lang| Lang { lang, catalogs: None })
    }

    /// Resolve the `language` setting: a language code, or [`AUTO`] (and anything unknown, e.g. a
    /// code from a newer version) to follow the system.
    pub fn from_pref(pref: &str) -> Lang {
        Lang::from_code(pref).unwrap_or_else(system_lang)
    }

    /// Every language the interface can be shown in: [`LANGUAGES`] in order, those with embedded
    /// catalogs (English only in a build without craft-i18n).
    pub fn all() -> impl Iterator<Item = Lang> {
        LANGUAGES.iter().filter_map(|l| Lang::from_code(l.code))
    }

    /// Every registered language in the order a picker lists them: English first (the source
    /// language and the fallback), then the rest by their own name with accents folded, so
    /// Čeština sits under C rather than after Z. Names in other scripts follow the Latin ones in
    /// code point order: Greek, Cyrillic, Arabic, then the CJK languages.
    pub fn for_picker() -> Vec<Lang> {
        let mut rest: Vec<Lang> = Lang::all().filter(|l| l.code() != Lang::EN.code()).collect();
        rest.sort_by_cached_key(|l| name_sort_key(l.name()));
        std::iter::once(Lang::EN).chain(rest).collect()
    }

    /// The embedded catalog texts for this language (shared and DeckCraft's own).
    pub fn catalog_text(self) -> Vec<&'static str> {
        let code = self.code();
        CRAFT_I18N
            .iter()
            .filter(|(c, ..)| c.eq_ignore_ascii_case(code))
            .flat_map(|(_, shared, app)| [*shared, *app])
            .filter(|s| !s.is_empty())
            .collect()
    }
}

/// The embedded catalog texts for the currently active interface language.
pub fn current_catalog_text() -> Vec<&'static str> {
    current().catalog_text()
}

/// The canonical `language` setting for user input: [`AUTO`] or a registered code (any case).
/// `None` for anything else, so callers keep the current setting.
pub fn normalize_pref(pref: &str) -> Option<&'static str> {
    if pref.eq_ignore_ascii_case(AUTO) {
        return Some(AUTO);
    }
    Lang::from_code(pref).map(Lang::code)
}

/// Portuguese other than Brazil's (`pt`, `pt-PT`, `pt_AO.UTF-8`): there is no catalog for it (the
/// European vocabulary differs), so it stays English rather than falling to `pt-br` as the crate's
/// matching would.
fn non_brazilian_portuguese(tag: &str) -> bool {
    let tag = tag.trim().to_ascii_lowercase();
    let mut parts = tag.split(['.', '@']).next().unwrap_or("").split(['-', '_']);
    parts.next() == Some("pt") && !parts.any(|p| p == "br")
}

/// The registered language for a locale tag such as `ja_JP.UTF-8` or `zh-Hant-TW`; `None` when
/// there is no catalog for it. `C`/`POSIX` mean English.
pub fn lang_from_tag(tag: &str) -> Option<Lang> {
    if non_brazilian_portuguese(tag) {
        return None;
    }
    I18N.match_tag(tag).and_then(|lang| Lang::from_code(I18N.code(lang)))
}

/// The first of the system's preferred languages DeckCraft has, in the user's order: someone who
/// prefers French, then Japanese, gets Japanese rather than English.
pub fn first_supported<'a>(tags: impl IntoIterator<Item = &'a str>) -> Option<Lang> {
    tags.into_iter().find_map(lang_from_tag)
}

/// The system language (looked up once). English when it can't be determined or isn't supported.
pub fn system_lang() -> Lang {
    // Tests drive the UI by its English labels whatever the machine's language is.
    if cfg!(test) {
        return Lang::EN;
    }
    static SYSTEM: OnceLock<Lang> = OnceLock::new();
    *SYSTEM.get_or_init(detect_system_lang)
}

/// Detect the system language from the `DECKCRAFT_LANGUAGE` override or platform locale settings.
fn detect_system_lang() -> Lang {
    // An explicit override, then the platform's preferred UI languages.
    if let Some(l) = std::env::var("DECKCRAFT_LANGUAGE").ok().and_then(|v| lang_from_tag(&v)) {
        return l;
    }
    let tags: Vec<String> = sys_locale::get_locales().take(16).collect();
    first_supported(tags.iter().map(String::as_str)).unwrap_or(Lang::EN)
}

thread_local! {
    // Per thread: parallel test harnesses must not change each other's language.
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::EN) };
}

/// Set the language the UI is drawn in (the app does this every frame from its setting).
pub fn set_current(lang: Lang) {
    CURRENT.set(lang);
}

/// The language the UI is drawn in.
pub fn current() -> Lang {
    CURRENT.get()
}

/// Does `lang` have a translation for this string? (English never does: it is the source.)
pub fn has(lang: Lang, s: &str) -> bool {
    lang.i18n().get(lang.lang, "", s).is_some()
}

/// `s` in the current language ([`tr`] with [`current`]).
pub fn t(s: &str) -> &str {
    tr(current(), s)
}

/// `s` in `lang`; strings without a translation come back unchanged.
pub fn tr(lang: Lang, s: &str) -> &str {
    lang.i18n().t(lang.lang, s)
}

/// `s` in the current language as worded for `context`, falling back to the plain translation and
/// then to English.
pub fn tc<'a>(context: &str, s: &'a str) -> &'a str {
    trc(current(), context, s)
}

/// [`tc`] in a given language.
pub fn trc<'a>(lang: Lang, context: &str, s: &'a str) -> &'a str {
    lang.i18n().tc(lang.lang, context, s)
}

/// `s`'s translation for `context` in the current language, with no fallback to the plain row or
/// English: for a word whose plain translation means something else in that place.
pub fn get<'a>(context: &str, s: &'a str) -> Option<&'a str> {
    let lang = current();
    lang.i18n().get(lang.lang, context, s)
}

/// A test language whose DeckCraft catalog is `source` (catalog format), so tests of lookup code
/// don't depend on the embedded catalogs' wording.
#[cfg(test)]
pub(crate) fn fixture_lang(code: &'static str, source: &'static str) -> Lang {
    let embedded: &'static craft_i18n::Embedded = Box::leak(Box::new([(code, "", source)]));
    let (i18n, _) = I18n::new(embedded, &[code]);
    let i18n: &'static I18n = Box::leak(Box::new(i18n));
    let lang = i18n.from_code(code).unwrap_or(craft_i18n::Lang::EN);
    Lang { lang, catalogs: Some(i18n) }
}

/// `name` lowercased with the Latin accents that occur in language names folded away, so a
/// byte-wise sort reads alphabetically. Other scripts are left as they are.
fn name_sort_key(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'è'..='ë' | 'ē' | 'ě' | 'ę' | 'ế' | 'ệ' => 'e',
            'ì'..='ï' | 'ī' => 'i',
            'ñ' | 'ń' | 'ň' => 'n',
            'ò'..='ö' | 'ø' | 'ő' => 'o',
            'ś' | 'š' => 's',
            'ù'..='ü' | 'ū' | 'ů' | 'ű' => 'u',
            'ý' | 'ÿ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABELS: &str = "\tHome\tHome-FX\n\tFile\tFile-FX\nribbon\tReview\tReview-ctx-FX\n";

    #[test]
    fn a_missing_translation_shows_the_english_source() {
        set_current(fixture_lang("x-fixture", LABELS));
        assert_eq!(t("Home"), "Home-FX");
        assert_eq!(t("Transitions"), "Transitions");
        set_current(Lang::EN);
        assert_eq!(t("Home"), "Home");
    }

    #[test]
    fn context_rows_are_used_only_for_their_context() {
        let lang = fixture_lang("x-fixture", LABELS);
        assert_eq!(trc(lang, "ribbon", "Review"), "Review-ctx-FX");
        assert_eq!(trc(lang, "other", "Review"), "Review");
        set_current(lang);
        assert_eq!(get("ribbon", "Review"), Some("Review-ctx-FX"));
        assert_eq!(get("other", "Review"), None);
        set_current(Lang::EN);
    }

    #[test]
    fn the_macro_reads_the_current_language() {
        set_current(fixture_lang("x-fixture", LABELS));
        assert_eq!(crate::tl!("File"), "File-FX");
        set_current(Lang::EN);
        assert_eq!(crate::tl!("File"), "File");
    }

    #[test]
    fn the_setting_resolves_to_a_language_and_unknown_values_follow_the_system() {
        // Tests run in English whatever the machine's language is.
        assert_eq!(Lang::from_pref(AUTO), Lang::EN);
        assert_eq!(Lang::from_pref("nonsense"), Lang::EN);
        assert_eq!(normalize_pref("AUTO"), Some(AUTO));
        assert_eq!(normalize_pref("nonsense"), None);
        assert_eq!(normalize_pref("en"), Some("en"));
    }

    #[test]
    fn european_portuguese_stays_english() {
        for tag in ["pt", "pt-PT", "pt_AO.UTF-8"] {
            assert!(non_brazilian_portuguese(tag), "{tag}");
        }
        for tag in ["pt-BR", "pt_BR.UTF-8", "pt-Latn-BR", "en-US"] {
            assert!(!non_brazilian_portuguese(tag), "{tag}");
        }
    }

    #[test]
    fn the_picker_lists_english_first_then_names_alphabetically() {
        let order = Lang::for_picker();
        assert_eq!(order.first().map(|l| l.code()), Some(Lang::EN.code()));
        assert_eq!(order.len(), Lang::all().count());
        let keys: Vec<String> = order[1..].iter().map(|l| name_sort_key(l.name())).collect();
        assert!(keys.windows(2).all(|w| w[0] <= w[1]), "{keys:?}");
        assert_eq!(name_sort_key("Čeština"), "cestina");
        assert_eq!(name_sort_key("Türkçe"), "turkce");
    }

    #[test]
    fn a_build_without_catalogs_offers_english_only() {
        if CRAFT_I18N.is_empty() {
            assert_eq!(Lang::all().map(Lang::code).collect::<Vec<_>>(), vec!["en"]);
        }
    }
}
