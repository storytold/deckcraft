//! Internationalization (i18n) and localization (L10n) layer for DeckCraft UI.
//!
//! Architecture:
//! - English (`en`) is the canonical source key language; zero allocation when running in English.
//! - Additional languages are loaded from UTF-8 `.tsv` catalogs embedded at compile time
//!   (see [`catalog::VI_TSV`]), keeping the binary self-contained with zero runtime file I/O.
//! - Supports:
//!   - [`t`]: direct key lookup (`t("File") -> "Tệp"`).
//!   - [`tc`]: context-disambiguated lookup (`tc("menu", "View")` looks up `"menu::View"`,
//!     falling back to `"View"`).
//!   - [`tn`]: plural-aware lookup (`tn("Slide", "{n} slides", n)` looks up `"Slide#one"` /
//!     `"Slide#other"` and substitutes `{n}`).
//!   - [`tf`]: named placeholder formatting (`tf("Slide {cur} of {total}", &[("cur", "1"), ("total", "5")])`).
//!   - [`fmt`]: positional `{}` placeholder formatting (`fmt("Saved {}", &[path])`).
//!   - [`Lang::from_pref`]: resolves `"auto"` / `"system"` against OS locale (`DECKCRAFT_LOCALE`,
//!     `LC_ALL`, `LC_MESSAGES`, `LANG`, `LANGUAGE`, macOS `AppleLanguages`, Windows registry)
//!     with graceful fallback to English.

pub mod catalog;

use std::cell::Cell;
use std::sync::OnceLock;

pub use catalog::{Catalog, vi_catalog};

/// Supported UI display languages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Lang {
    /// English (United States) — canonical source language.
    #[default]
    EN,
    /// Vietnamese (`Tiếng Việt`).
    VI,
}

/// Ordered list of all compiled-in languages for settings dropdowns and introspection.
pub const ALL_LANGS: &[Lang] = &[Lang::EN, Lang::VI];

impl Lang {
    /// All supported languages in selector order.
    pub const ALL: [Self; 2] = [Self::EN, Self::VI];

    /// BCP-47 / ISO 639-1 language code (`"en"`, `"vi"`).
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::EN => "en",
            Self::VI => "vi",
        }
    }

    /// Native display name shown in the language selector (`"English"`, `"Tiếng Việt"`).
    #[must_use]
    pub const fn native_name(self) -> &'static str {
        match self {
            Self::EN => "English",
            Self::VI => "Tiếng Việt",
        }
    }

    /// Alias for [`Self::native_name`] used in UI selectors.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        self.native_name()
    }

    /// English display name (`"English"`, `"Vietnamese"`).
    #[must_use]
    pub const fn english_name(self) -> &'static str {
        match self {
            Self::EN => "English",
            Self::VI => "Vietnamese",
        }
    }

    /// Parse a user preference string (`"auto"`, `"en"`, `"vi"`, `"vi-VN"`, `"vi_VN.UTF-8"`, etc.).
    ///
    /// When `pref` is `""`, `"auto"`, `"system"`, or `"default"`, queries [`system_lang`].
    /// Unknown language tags fall back to [`Lang::EN`].
    #[must_use]
    pub fn from_pref(pref: &str) -> Self {
        let trimmed = pref.trim();
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("auto")
            || trimmed.eq_ignore_ascii_case("system")
            || trimmed.eq_ignore_ascii_case("default")
        {
            return system_lang();
        }
        lang_from_tag(trimmed).unwrap_or(Self::EN)
    }

    /// Normalize a preference string into one of `"auto"`, `"en"`, or `"vi"`.
    #[must_use]
    pub fn normalize_pref(pref: &str) -> &'static str {
        let trimmed = pref.trim();
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("auto")
            || trimmed.eq_ignore_ascii_case("system")
            || trimmed.eq_ignore_ascii_case("default")
        {
            "auto"
        } else {
            match lang_from_tag(trimmed) {
                Some(Self::VI) => "vi",
                Some(Self::EN) | None => "en",
            }
        }
    }
}

/// Parse an arbitrary locale tag (`"vi"`, `"vi_VN.UTF-8"`, `"vi-VN"`, `"Tiếng Việt"`, `"en_US"`)
/// into a supported [`Lang`], or `None` if unrecognized.
#[must_use]
pub fn lang_from_tag(tag: &str) -> Option<Lang> {
    let clean = tag.trim().trim_matches('"').trim_matches('\'');
    if clean.is_empty() {
        return None;
    }
    let lower = clean.to_lowercase();
    if lower == "tiếng việt" || lower == "tieng viet" || lower == "vietnamese" || lower == "vie" {
        return Some(Lang::VI);
    }
    if lower == "english" || lower == "eng" || lower == "c" || lower == "posix" {
        return Some(Lang::EN);
    }
    let without_encoding = lower.split('.').next().unwrap_or(lower.as_str());
    let without_modifier = without_encoding
        .split('@')
        .next()
        .unwrap_or(without_encoding);
    let primary = without_modifier
        .split(['_', '-'])
        .next()
        .unwrap_or(without_modifier);
    match primary {
        "vi" | "vie" => Some(Lang::VI),
        "en" | "eng" => Some(Lang::EN),
        _ => None,
    }
}

/// Detect the operating system's preferred UI language, cached after first call.
///
/// Note: Under `cfg!(test)`, `system_lang()` always returns [`Lang::EN`] so that existing
/// English UI snapshot and accessibility tests are unaffected by the developer's host locale.
/// Locale detection helpers are tested directly via [`detect_system_lang_from_env`].
#[must_use]
pub fn system_lang() -> Lang {
    if cfg!(test) {
        return Lang::EN;
    }
    static DETECTED: OnceLock<Lang> = OnceLock::new();
    *DETECTED.get_or_init(detect_system_lang)
}

fn detect_system_lang() -> Lang {
    if let Some(lang) = detect_system_lang_from_env(|k| std::env::var(k).ok()) {
        return lang;
    }
    #[cfg(target_os = "macos")]
    if let Some(lang) = detect_macos_apple_languages() {
        return lang;
    }
    #[cfg(target_os = "windows")]
    if let Some(lang) = detect_windows_locale() {
        return lang;
    }
    Lang::EN
}

/// Inspect standard locale environment variables (`DECKCRAFT_LOCALE`, `DECKCRAFT_LANGUAGE`,
/// `LC_ALL`, `LC_MESSAGES`, `LANG`, `LANGUAGE`) using a caller-supplied lookup closure.
#[must_use]
pub fn detect_system_lang_from_env<F>(mut get_env: F) -> Option<Lang>
where
    F: FnMut(&str) -> Option<String>,
{
    for var in [
        "DECKCRAFT_LOCALE",
        "DECKCRAFT_LANGUAGE",
        "LC_ALL",
        "LC_MESSAGES",
        "LANG",
        "LANGUAGE",
    ] {
        if let Some(val) = get_env(var) {
            for part in val.split(':') {
                let trimmed = part.trim();
                if trimmed.is_empty()
                    || trimmed.eq_ignore_ascii_case("c")
                    || trimmed.eq_ignore_ascii_case("posix")
                    || trimmed.eq_ignore_ascii_case("c.utf-8")
                {
                    continue;
                }
                if let Some(lang) = lang_from_tag(trimmed) {
                    return Some(lang);
                }
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn detect_macos_apple_languages() -> Option<Lang> {
    let output = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let cleaned = line.trim().trim_matches(&['(', ')', ',', '"', '\''][..]);
        if cleaned.is_empty() {
            continue;
        }
        if let Some(lang) = lang_from_tag(cleaned) {
            return Some(lang);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn detect_windows_locale() -> Option<Lang> {
    let sys_root = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"));
    let reg_exe = sys_root.join("System32").join("reg.exe");
    let output = std::process::Command::new(reg_exe)
        .args(["query", r"HKCU\Control Panel\International", "/v", "LocaleName"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for token in text.split_whitespace().rev() {
        if let Some(lang) = lang_from_tag(token) {
            return Some(lang);
        }
    }
    None
}

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::EN) };
}

/// Set the active UI language for the current thread.
pub fn set_current(lang: Lang) {
    CURRENT.with(|c| c.set(lang));
}

/// Get the active UI language for the current thread.
#[must_use]
pub fn current() -> Lang {
    CURRENT.with(Cell::get)
}

/// Translate an English UI key into `lang`, returning a borrowed `'static` slice when possible.
///
/// Falls back to returning `key` unchanged if no translation entry exists.
#[must_use]
pub fn tr<'a>(lang: Lang, key: &'a str) -> &'a str {
    match lang {
        Lang::EN => key,
        Lang::VI => vi_catalog().get(key).unwrap_or(key),
    }
}

/// Translate an English UI key using the current thread's active language.
#[must_use]
pub fn t(key: &str) -> &str {
    tr(current(), key)
}

/// Translate an English UI key with a disambiguating context (`ctx`), e.g.
/// `tc("menu", "View")` or `tc("noun", "Slide")`.
///
/// Looks up `"<ctx>::<key>"` first, then falls back to `<key>`, and finally returns `key`.
#[must_use]
pub fn tr_ctx<'a>(lang: Lang, ctx: &str, key: &'a str) -> &'a str {
    match lang {
        Lang::EN => key,
        Lang::VI => vi_catalog().get_ctx(ctx, key).unwrap_or(key),
    }
}

/// Context-disambiguated translation using the current thread's active language.
#[must_use]
pub fn tc<'a>(ctx: &str, key: &'a str) -> &'a str {
    tr_ctx(current(), ctx, key)
}

/// Plural-aware translation with `{n}` (and `{}` / `{count}`) substitution.
///
/// - In `Lang::EN`, selects `singular` when `n == 1` and `plural` otherwise.
/// - In `Lang::VI`, looks up `"<singular>#other"` (or `"<singular>#one"` when `n == 1`),
///   falling back to translating `plural` (or `singular` when `n == 1`).
#[must_use]
pub fn tr_plural(lang: Lang, singular: &str, plural: &str, n: usize) -> String {
    let template = match lang {
        Lang::EN => {
            if n == 1 {
                singular
            } else {
                plural
            }
        }
        Lang::VI => {
            let cat = vi_catalog();
            let specific_key = if n == 1 {
                format!("{singular}#one")
            } else {
                format!("{singular}#other")
            };
            cat.get(&specific_key)
                .or_else(|| {
                    let other_key = format!("{singular}#other");
                    cat.get(&other_key)
                })
                .or_else(|| {
                    if n == 1 {
                        cat.get(singular)
                    } else {
                        cat.get(plural)
                    }
                })
                .unwrap_or(if n == 1 { singular } else { plural })
        }
    };
    let n_str = n.to_string();
    template
        .replace("{n}", &n_str)
        .replace("{count}", &n_str)
        .replace("{}", &n_str)
}

/// Plural-aware translation using the current thread's active language.
#[must_use]
pub fn tn(singular: &str, plural: &str, n: usize) -> String {
    tr_plural(current(), singular, plural, n)
}

/// Translate `key` in `lang` and substitute named placeholders (`{name}` -> value).
#[must_use]
pub fn tr_fmt(lang: Lang, key: &str, args: &[(&str, &str)]) -> String {
    let base = tr(lang, key);
    if args.is_empty() {
        return base.to_owned();
    }
    let mut out = base.to_owned();
    for (name, val) in args {
        let mut token = String::with_capacity(name.len() + 2);
        token.push('{');
        token.push_str(name);
        token.push('}');
        out = out.replace(&token, val);
    }
    out
}

/// Translate `key` using the current thread's active language and substitute named placeholders.
#[must_use]
pub fn tf(key: &str, args: &[(&str, &str)]) -> String {
    tr_fmt(current(), key, args)
}

/// Translate `key` using the current thread's active language and substitute positional `{}`
/// (or `{0}`, `{1}`, …) placeholders in order.
#[must_use]
pub fn fmt(key: &str, args: &[&str]) -> String {
    fmt_lang(current(), key, args)
}

/// Translate `key` in `lang` and substitute positional `{}` (or `{0}`, `{1}`, …) placeholders.
#[must_use]
pub fn fmt_lang(lang: Lang, key: &str, args: &[&str]) -> String {
    let tpl = tr(lang, key);
    if args.is_empty() {
        return tpl.to_owned();
    }
    let mut indexed = tpl.to_owned();
    let mut used_indexed = false;
    for (idx, val) in args.iter().enumerate() {
        let token = format!("{{{idx}}}");
        if indexed.contains(&token) {
            indexed = indexed.replace(&token, val);
            used_indexed = true;
        }
    }
    if used_indexed && !indexed.contains("{}") {
        return indexed;
    }
    let mut out = String::with_capacity(indexed.len() + 16);
    let mut parts = indexed.split("{}");
    if let Some(first) = parts.next() {
        out.push_str(first);
    }
    for (idx, part) in parts.enumerate() {
        if let Some(val) = args.get(idx) {
            out.push_str(val);
        } else {
            out.push_str("{}");
        }
        out.push_str(part);
    }
    out
}

/// Convenience macro for translating static or formatted UI labels.
///
/// - `tl!("File")` -> `crate::i18n::t("File")` (`&str`)
/// - `tl!("Slide {} of {}", cur, total)` -> `crate::i18n::fmt("Slide {} of {}", &[...])` (`String`)
#[macro_export]
macro_rules! tl {
    ($key:expr) => {
        $crate::i18n::t($key)
    };
    ($key:expr, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::i18n::tf($key, &[ $((stringify!($name), &format!("{}", $val))),+ ])
    };
    ($key:expr, $($arg:expr),+ $(,)?) => {
        $crate::i18n::fmt($key, &[$(&format!("{}", $arg)),+])
    };
}

#[cfg(test)]
mod tests;
