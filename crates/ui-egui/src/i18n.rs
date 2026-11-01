//! Interface languages (#15). Strings in the code stay English and are the lookup keys; the
//! craft-i18n catalogs map them to display text when the UI is drawn. Command ids, labels the
//! engine returns, presentation text, file names, the control channel, the CLI and MCP never see
//! translated text, so scripts and agents are unaffected. A string without a translation shows in
//! English, so coverage can grow one entry at a time, and a missing translation never fails a
//! build, a test or a release.
//!
//! The catalogs live in [storytold/craft-i18n](https://github.com/storytold/craft-i18n)
//! (`locales/<locale>/common.tsv` and `deckcraft.tsv`), not in this repository. `build.rs` embeds
//! them from `CRAFT_I18N_DIR` (CI and releases check out the SHA in `craft-i18n.rev`); without it
//! the build is English only. Lookup, fallback (`de-at` → `de` → English, app rows over shared
//! ones), plurals, language names and locale matching are the `craft-i18n` crate's (craft-libs).
//!
//! The `language` setting is `auto` by default: the UI follows the operating system's language
//! (Windows display language, macOS preferred languages, `LANG`/`LC_*` elsewhere, the browser on
//! the web) when there is a catalog for it, and English otherwise. Preferences › Interface language and the
//! `app.language` command pick one by hand.
//!
//! # Adding or changing a translation
//! In craft-i18n: add rows to `locales/<locale>/deckcraft.tsv` (its `docs/integration.md`); they
//! show here once `craft-i18n.rev` moves to a commit that has them. A feature PR here adds the
//! English only.
//!
//! # Adding a language
//! 1. craft-i18n gets `locales/<code>/deckcraft.tsv` (its `common.tsv` already names the language).
//! 2. Add its code to [`LANGUAGES`].
//!
//! The Preferences dropdown, `app.language` and the system-language match pick it up from there.
//!
//! # Looking strings up
//! - [`tl!`](crate::tl) / [`t`]: a string in the current language. [`tr`]: in a given one.
//! - [`tc`]: worded for a context, for an English word with two meanings.
//! - [`location`]: a ribbon location such as `Home › Font`, segment by segment.
//! - [`fmt`]: fill `{name}` placeholders after a lookup; translators may reorder them.

use std::cell::Cell;
use std::sync::{LazyLock, OnceLock};

use craft_i18n::I18n;

pub use craft_i18n::{fmt, fmt_plain};

include!(concat!(env!("OUT_DIR"), "/craft_i18n.rs"));

/// The embedded catalogs, offering exactly the languages in [`LANGUAGES`]. Built on first use;
/// a locale is parsed on its first lookup.
static I18N: LazyLock<I18n> = LazyLock::new(|| {
    let (i18n, problems) = I18n::new(CRAFT_I18N, &LANGUAGES);
    problems.iter().for_each(|p| log::warn!("interface catalogs: {p}"));
    i18n
});

/// The `language` setting that follows the system.
pub const AUTO: &str = "auto";

/// The languages DeckCraft offers, as lowercase BCP 47 codes (also the `language` setting's value
/// and the craft-i18n locale), when their catalogs are embedded. English first: it is the source
/// language and the fallback.
pub static LANGUAGES: [&str; 3] = [
    "en", // German, neutral across Germany, Austria and Switzerland; `de-DE`, `de-AT`, `de_CH.UTF-8` …
    // resolve here.
    "de", // Russian; `ru-RU`, `ru-SK`, `ru-UA` and `ru_RU.UTF-8` resolve here.
    "ru",
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

    fn i18n(self) -> &'static I18n {
        self.catalogs.unwrap_or_else(|| LazyLock::force(&I18N))
    }

    pub fn code(self) -> &'static str {
        self.i18n().code(self.lang)
    }

    /// The language's name in itself (its `@id language.name` row), shown in the Preferences list.
    pub fn name(self) -> &'static str {
        self.i18n().name(self.lang)
    }

    /// A language by its exact code (any case): one of [`LANGUAGES`] whose catalogs are embedded.
    pub fn from_code(code: &str) -> Option<Lang> {
        let code = LANGUAGES.iter().find(|l| l.eq_ignore_ascii_case(code.trim()))?;
        #[cfg(test)]
        if let Some(l) = FIXTURE.get().filter(|l| l.code() == *code) {
            return Some(l);
        }
        I18N.from_code(code).map(|lang| Lang { lang, catalogs: None })
    }

    /// Resolve the `language` setting: a language code, or [`AUTO`] (and anything unknown, e.g. a
    /// code from a newer version) to follow the system.
    pub fn from_pref(pref: &str) -> Lang {
        Lang::from_code(pref).unwrap_or_else(system_lang)
    }

    /// Every language the interface can be shown in: [`LANGUAGES`] in order, those with embedded
    /// catalogs (English only in a build without craft-i18n).
    pub fn all() -> impl Iterator<Item = Lang> {
        LANGUAGES.iter().filter_map(|l| Lang::from_code(l))
    }
}

/// The canonical `language` setting for user input: [`AUTO`] or an offered code (any case).
/// `None` for anything else, so callers keep the current setting.
pub fn normalize_pref(pref: &str) -> Option<&'static str> {
    if pref.trim().eq_ignore_ascii_case(AUTO) {
        return Some(AUTO);
    }
    Lang::from_code(pref).map(Lang::code)
}

/// The offered language for a locale tag such as `de_DE.UTF-8` or `de-AT`; `None` when there is
/// no catalog for it. `C`/`POSIX` mean English.
pub fn lang_from_tag(tag: &str) -> Option<Lang> {
    I18N.match_tag(tag).and_then(|lang| Lang::from_code(I18N.code(lang)))
}

/// The first of the system's preferred languages DeckCraft has, in the user's order: someone
/// who prefers French, then German, gets German rather than English.
pub fn first_supported<'a>(tags: impl IntoIterator<Item = &'a str>) -> Option<Lang> {
    tags.into_iter().find_map(lang_from_tag)
}

/// The system language (looked up once). English when it can't be determined or isn't offered.
pub fn system_lang() -> Lang {
    // Tests drive the UI by its English labels whatever the machine's language is.
    if cfg!(test) {
        return Lang::EN;
    }
    static SYSTEM: OnceLock<Lang> = OnceLock::new();
    *SYSTEM.get_or_init(detect_system_lang)
}

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
    let lang = current();
    lang.i18n().tc(lang.lang, context, s)
}

/// `one` or `other` (English forms with `{n}`) for `n` in the current language's plural form.
pub fn tn(n: u64, one: &str, other: &str) -> String {
    let lang = current();
    lang.i18n().trn(lang.lang, n, one, other)
}

/// A ribbon caption whose line break was saved as a space (a Quick Access Toolbar label such as
/// `New Slide`, from `New\nSlide`) in the current language, on one line.
pub fn t_flat(s: &str) -> String {
    let lang = current();
    if has(lang, s) {
        return t(s).replace('\n', " ");
    }
    // Try the break at each space: captions have one at most.
    let found = s.match_indices(' ').map(|(i, _)| format!("{}\n{}", &s[..i], &s[i + 1..])).find(|k| has(lang, k));
    found.map_or_else(|| s.to_string(), |k| t(&k).replace('\n', " "))
}

/// Whether `code` is shaped like a language code (`fr`, `pt-br`, `zh_Hans`), offered or not.
/// A saved setting of that shape is kept when this build doesn't offer it, so switching between
/// a release and a build without catalogs doesn't lose the choice.
pub fn well_formed_code(code: &str) -> bool {
    let mut parts = code.split(['-', '_']);
    let primary = parts.next().unwrap_or("");
    (2..=3).contains(&primary.len())
        && primary.bytes().all(|b| b.is_ascii_alphabetic())
        && parts.all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// A ribbon location (`Home › Font`) in the current language, segment by segment.
pub fn location(loc: &str) -> String {
    loc.split(" › ").map(t).collect::<Vec<_>>().join(" › ")
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

#[cfg(test)]
thread_local! {
    // A fixture language standing in for the embedded catalogs of its code (see `use_fixture`).
    static FIXTURE: Cell<Option<Lang>> = const { Cell::new(None) };
}

/// Make `code` (one of [`LANGUAGES`]) resolve to a fixture with `source` as its catalog on this
/// thread, so app tests can switch the interface whether or not craft-i18n was embedded.
#[cfg(test)]
pub(crate) fn use_fixture(code: &'static str, source: &'static str) -> Lang {
    let lang = fixture_lang(code, source);
    FIXTURE.set(Some(lang));
    lang
}

#[cfg(test)]
mod tests;
