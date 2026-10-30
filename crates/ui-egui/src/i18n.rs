//! Interface languages: English (the strings in the code) and Korean (`i18n/ko.tsv`).
//!
//! The English text is the key: [`tr`] gives the current language's text for it, or the English
//! when the catalog has none, so an untranslated string still reads. Command ids, presentation
//! content, file and font names are never translated.
//!
//! The language is per thread: the interface runs on one thread, and tests on parallel threads
//! each keep their own.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Ko,
}

/// The values `UiState::language` and `app.language` take.
pub const SETTINGS: [&str; 3] = ["system", "en", "ko"];

thread_local! {
    static LANG: Cell<Lang> = const { Cell::new(Lang::En) };
}

pub fn current() -> Lang {
    LANG.with(Cell::get)
}

pub fn set(l: Lang) {
    LANG.with(|c| c.set(l));
}

/// The language a setting (`system`, `en` or `ko`) picks.
pub fn resolve(setting: &str) -> Lang {
    match setting {
        "ko" => Lang::Ko,
        "en" => Lang::En,
        _ => system(),
    }
}

/// The language a locale name (`ko-KR`, `ko_KR.UTF-8`, `en-US`) asks for.
pub fn for_locale(locale: &str) -> Lang {
    let l = locale.to_ascii_lowercase();
    if l == "ko" || l.starts_with("ko-") || l.starts_with("ko_") || l.starts_with("ko.") { Lang::Ko } else { Lang::En }
}

/// The system's language: English unless the user's locale is Korean. On the web the browser's
/// language isn't read; the user picks Korean in Preferences. Tests run in English whatever the
/// machine's language.
fn system() -> Lang {
    static SYS: OnceLock<Lang> = OnceLock::new();
    *SYS.get_or_init(|| {
        #[cfg(not(any(target_arch = "wasm32", test)))]
        if let Some(l) = sys_locale::get_locale() {
            return for_locale(&l);
        }
        Lang::En
    })
}

const KO_TSV: &str = include_str!("i18n/ko.tsv");

/// `\n` and `\t` in a catalog cell are a line break and a tab.
fn unescape(s: &str) -> String {
    s.replace("\\n", "\n").replace("\\t", "\t")
}

/// The Korean catalog: English → Korean. Lines are `English<TAB>Korean`; `#` starts a comment.
fn ko_catalog() -> &'static HashMap<String, String> {
    static KO: OnceLock<HashMap<String, String>> = OnceLock::new();
    KO.get_or_init(|| {
        KO_TSV
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| l.split_once('\t'))
            .map(|(en, ko)| (unescape(en), unescape(ko)))
            .collect()
    })
}

/// The Korean for `en`, whatever the current language.
pub fn ko(en: &str) -> Option<&'static str> {
    ko_catalog().get(en).map(String::as_str)
}

/// Each character of the Korean catalog once, for borrowing the interface faces that draw them.
pub fn ko_text() -> &'static str {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| ko_catalog().values().flat_map(|v| v.chars()).collect::<std::collections::BTreeSet<char>>().into_iter().collect())
}

/// The renderer's translation of placeholder prompts in the current language.
pub fn prompts() -> Option<fn(&str) -> Option<&'static str>> {
    match current() {
        Lang::En => None,
        Lang::Ko => Some(ko),
    }
}

/// `en` in the current language.
pub fn tr(en: &str) -> &str {
    match current() {
        Lang::En => en,
        Lang::Ko => ko(en).unwrap_or(en),
    }
}

/// `template` in the current language with each `{}` replaced by the next of `args`, in order.
pub fn trf(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::new();
    let mut args = args.iter();
    let mut parts = tr(template).split("{}").peekable();
    while let Some(p) = parts.next() {
        out.push_str(p);
        if parts.peek().is_some()
            && let Some(a) = args.next()
        {
            out.push_str(&a.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_pick_korean_only_for_korean() {
        for l in ["ko", "ko-KR", "ko_KR.UTF-8", "KO-kr"] {
            assert_eq!(for_locale(l), Lang::Ko, "{l}");
        }
        for l in ["en-US", "ja-JP", "kok", "", "C"] {
            assert_eq!(for_locale(l), Lang::En, "{l}");
        }
        assert_eq!(resolve("ko"), Lang::Ko);
        assert_eq!(resolve("en"), Lang::En);
    }

    #[test]
    fn english_is_the_key_and_the_fallback() {
        set(Lang::En);
        assert_eq!(tr("Insert"), "Insert");
        set(Lang::Ko);
        assert_eq!(tr("Insert"), "삽입");
        assert_eq!(tr("Not in the catalog"), "Not in the catalog");
        assert_eq!(trf("Slide {} of {}", &[&2, &10]), "슬라이드 2/10");
        set(Lang::En);
        assert_eq!(trf("Slide {} of {}", &[&2, &10]), "Slide 2 of 10");
    }

    /// Every row has a key and a value, keys are unique, and each Korean keeps its English's
    /// `{}` count, line breaks and trailing ellipsis.
    #[test]
    fn catalog_rows_are_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for (n, l) in KO_TSV.lines().enumerate().filter(|(_, l)| !l.is_empty() && !l.starts_with('#')) {
            let Some((en, ko)) = l.split_once('\t') else { panic!("line {}: no tab: {l}", n + 1) };
            assert!(!en.is_empty() && !ko.trim().is_empty() && !ko.contains('\t'), "line {}: {l}", n + 1);
            assert!(seen.insert(en), "line {}: duplicate {en}", n + 1);
            assert_eq!(en.matches("{}").count(), ko.matches("{}").count(), "line {}: placeholders in {en}", n + 1);
            assert_eq!(en.ends_with('…'), ko.ends_with('…'), "line {}: ellipsis in {en}", n + 1);
        }
    }

    /// Every literal the interface passes to `tr`/`trf` has a Korean row.
    #[test]
    fn every_translated_literal_is_in_the_catalog() {
        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut missing = vec![];
        // The interface's modules, in their folders too (`ribbon/home.rs`, `dialogs/font.rs`).
        let mut dirs = vec![src_dir.clone()];
        while let Some(dir) = dirs.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if p.is_dir() {
                    if name != "tests" && name != "i18n" {
                        dirs.push(p);
                    }
                    continue;
                }
                if !name.ends_with(".rs") || name.ends_with("_tests.rs") || name == "tests.rs" || name == "i18n.rs" {
                    continue;
                }
                let rel = p.strip_prefix(&src_dir).unwrap_or(&p).display().to_string();
                let src = std::fs::read_to_string(&p).unwrap();
                for lit in literals(&src) {
                    if ko(&lit).is_none() {
                        missing.push(format!("{rel}: {lit:?}"));
                    }
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "{} strings have no Korean:\n{}", missing.len(), missing.join("\n"));
    }

    /// The string literals `src` passes to `tr(`/`trf(`, and the labels of the ribbon's buttons
    /// (`big`, `small`, `menu_item`, `toggle` translate their label themselves).
    fn literals(src: &str) -> Vec<String> {
        let mut out = vec![];
        for start in ["tr(", "trf(", "big(app, ui, ", "small(app, ui, ", "menu_item(app, ui, ", "toggle(app, ui, ", "v.push("] {
            let mut rest = src;
            while let Some(i) = rest.find(start) {
                let before = rest[..i].chars().last();
                rest = &rest[i + start.len()..];
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') || rest.starts_with("app, ") {
                    continue;
                }
                // The label is the first literal of the line (`tr(`: right at the start).
                let line = rest.split('\n').next().unwrap_or("");
                let Some(q) = line.find('"') else { continue };
                if start.ends_with('(') && q != 0 {
                    continue;
                }
                let mut lit = String::new();
                let mut chars = rest[q + 1..].chars();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => lit.push('\n'),
                            Some('t') => lit.push('\t'),
                            Some(c) => lit.push(c),
                            None => break,
                        },
                        c => lit.push(c),
                    }
                }
                if lit.starts_with(|c: char| c.is_ascii_uppercase()) {
                    out.push(lit);
                }
            }
        }
        out
    }

    /// The names the interface shows from tables: commands (palette, Quick Access Toolbar),
    /// ribbon tabs, dialog titles, the menu bar and placeholder prompts.
    #[test]
    fn every_interface_name_is_in_the_catalog() {
        let mut names: Vec<String> = deckcraft_engine::Session::with_new().commands().into_iter().map(|c| c.label.to_string()).collect();
        names.extend(crate::UI_COMMANDS.iter().map(|c| c.1.to_string()));
        names.extend(crate::ribbon::TABS.iter().map(|t| t.to_string()));
        names.extend(crate::backstage::Page::ALL.map(|p| p.label().to_string()));
        names.extend(["Save", "Close", "Options"].map(String::from));
        names.extend(deckcraft_model::anim::TRANSITIONS.iter().map(|t| t.1.to_string()));
        names.extend(deckcraft_model::anim::ANIMATIONS.iter().map(|a| a.1.to_string()));
        names.extend(crate::qat::common().into_iter().map(|q| q.label));
        for (menu, items) in crate::menus::menu_tree() {
            names.push(menu.into());
            names.extend(items.into_iter().filter(|i| i.0 != "-").map(|i| i.0.to_string()));
        }
        let dialogs = include_str!("dialogs.rs");
        let titles = dialogs.split("fn title(").nth(1).and_then(|t| t.split("\n}").next()).unwrap_or("");
        names.extend(titles.lines().filter_map(|l| l.split("=> \"").nth(1)).filter_map(|t| t.split('"').next()).map(String::from));
        use deckcraft_model::PhType::*;
        names.extend([Title, SubTitle, Body, Chart, Table, Diagram, Media, Picture, Date, Footer, Header].map(|k| k.prompt().to_string()));
        let mut missing: Vec<String> = names.into_iter().filter(|n| ko(n).is_none()).collect();
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "{} names have no Korean:\n{}", missing.len(), missing.join("\n"));
    }
}
