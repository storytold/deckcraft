//! Interface language tests.

use super::catalog::{Catalog, parse_entries, placeholders};
use super::*;

fn lang(code: &str) -> Lang {
    Lang::from_code(code).unwrap_or_else(|| panic!("{code} registered"))
}

#[test]
fn system_locales_map_to_languages() {
    assert_eq!(lang_from_tag("en-GB"), Some(Lang::EN));
    assert_eq!(lang_from_tag("C"), Some(Lang::EN));
    assert_eq!(lang_from_tag("POSIX"), Some(Lang::EN));
    assert_eq!(lang_from_tag("fr-FR"), None);
    assert_eq!(lang_from_tag(""), None);
    assert_eq!(lang_from_tag("_"), None);
    assert_eq!(lang_from_tag("sr"), Some(lang("sr")), "Cyrillic is the default script");
    assert_eq!(lang_from_tag("sr-RS"), Some(lang("sr")));
    assert_eq!(lang_from_tag("sr_RS.UTF-8"), Some(lang("sr")));
    assert_eq!(lang_from_tag("sr-Cyrl-RS"), Some(lang("sr")));
    assert_eq!(lang_from_tag("sr-Latn-RS"), Some(lang("sr-latn")));
    assert_eq!(lang_from_tag("sr-Latn"), Some(lang("sr-latn")));
}

#[test]
fn the_first_supported_preferred_language_wins() {
    assert_eq!(first_supported(["fr-FR", "sr-Latn-RS", "en-US"]), Some(lang("sr-latn")));
    assert_eq!(first_supported(["en-US", "sr-RS"]), Some(Lang::EN));
    assert_eq!(first_supported(["fr-FR", "de-DE"]), None);
    assert_eq!(first_supported([]), None);
}

#[test]
fn settings_resolve_with_fallback() {
    assert_eq!(Lang::from_pref("sr"), lang("sr"));
    assert_eq!(Lang::from_pref("SR-LATN"), lang("sr-latn"));
    assert_eq!(Lang::from_pref("en"), Lang::EN);
    assert_eq!(Lang::from_pref(AUTO), Lang::EN);
    assert_eq!(Lang::from_pref("xx-unknown"), Lang::EN);
    assert_eq!(normalize_pref("Sr-Latn"), Some("sr-latn"));
    assert_eq!(normalize_pref("Auto"), Some(AUTO));
    assert_eq!(normalize_pref("xx"), None);
}

#[test]
fn lookups_fall_back_to_english() {
    let sr = lang("sr");
    assert_eq!(tr(sr, "no such label"), "no such label");
    assert_eq!(tr(Lang::EN, "Home"), "Home");
}

#[test]
fn current_language_is_per_thread() {
    set_current(lang("sr-latn"));
    assert_eq!(t("Home"), "Početak");
    std::thread::spawn(|| assert_eq!(current(), Lang::EN)).join().unwrap();
    set_current(Lang::EN);
    assert_eq!(t("Home"), "Home");
}

#[test]
fn placeholders_fill_without_reinterpreting_values() {
    let args = [("path", "a-{pages}.pptx"), ("pages", "2")];
    assert_eq!(fmt("{pages}: {path}; {unknown}", &args), "2: a-{pages}.pptx; {unknown}");
    assert_eq!(fmt("text {unfinished", &args), "text {unfinished");
}

#[test]
fn malformed_catalog_lines_are_skipped_not_fatal() {
    let (c, errors) = Catalog::parse(
        "# c\n\tHello\tZdravo\nctx\tA\tB\n\tOpen…\tOtvori\n\tPage {n}\tStrana {m}\nno tabs\n\tHello\tPonovljeno\n\tLine\\nTwo\tJedan\\nDva\n",
    );
    assert_eq!(errors.len(), 5, "{errors:?}");
    assert_eq!(c.plain("Hello"), Some("Zdravo"), "the first entry wins");
    assert_eq!(c.plain("Line\nTwo"), Some("Jedan\nDva"));
    assert_eq!(c.plain("Open…"), None);
}

/// Every bundled catalog parses without errors and keeps the English placeholders.
#[test]
fn bundled_catalogs_are_well_formed() {
    let mut codes = std::collections::HashSet::new();
    for l in &LANGUAGES {
        assert!(codes.insert(l.code), "duplicate code {}", l.code);
        assert_eq!(l.code, l.code.to_ascii_lowercase());
        let (entries, errors) = parse_entries(l.source);
        assert!(errors.is_empty(), "{}: {errors:?}", l.code);
        for e in &entries {
            assert_eq!(placeholders(&e.source), placeholders(&e.translation), "{}: {:?}", l.code, e.source);
            // Leading/trailing whitespace is sometimes intentional (indentation styling on a
            // field label); what matters is that the translation matches the source's pattern.
            assert_eq!(
                (e.source.starts_with(' '), e.source.ends_with(' ')),
                (e.translation.starts_with(' '), e.translation.ends_with(' ')),
                "{}: whitespace pattern differs from the English source in {:?}",
                l.code,
                e.translation
            );
            assert!(!e.translation.contains("..."), "{}: use … rather than three dots: {:?}", l.code, e.translation);
        }
    }
}

#[test]
fn serbian_covers_a_shared_key_set() {
    use std::collections::HashSet;
    let keys = |source| parse_entries(source).0.into_iter().map(|e| e.source).collect::<HashSet<_>>();
    let latn = keys(lang("sr-latn").0.source);
    let cyr = keys(lang("sr").0.source);
    assert_eq!(latn, cyr, "sr and sr-latn must translate exactly the same set of strings");
    assert!(latn.len() > 50, "only {} entries so far", latn.len());
}

#[test]
fn bundled_default_fonts_cover_serbian_cyrillic() {
    // DeckCraft's own fonts (deckcraft-fonts) only embed real files when CRAFT_FONTS_DIR was set
    // at build time; without it (the web build in this session), it falls through to egui's own
    // `default_fonts` (Ubuntu-Light), which does cover Cyrillic — confirmed against the actual
    // font bytes, not assumed.
    let defs = egui::FontDefinitions::default();
    let Some(bytes) = defs.font_data.get("Ubuntu-Light").map(|fd| fd.font.clone()) else {
        panic!(
            "egui's default proportional font (Ubuntu-Light) isn't registered under that name any more; update this test and re-check Cyrillic coverage"
        );
    };
    let face = ttf_parser::Face::parse(&bytes, 0).unwrap_or_else(|e| panic!("{e:?}"));
    let (entries, errors) = parse_entries(lang("sr").0.source);
    assert!(errors.is_empty());
    for ch in entries.iter().flat_map(|e| e.translation.chars()).filter(|c| ('\u{0400}'..='\u{04ff}').contains(c)) {
        assert!(face.glyph_index(ch).is_some(), "Ubuntu-Light lacks {ch}");
    }
}

#[test]
fn bundled_default_fonts_cover_serbian_latin() {
    let defs = egui::FontDefinitions::default();
    let Some(bytes) = defs.font_data.get("Ubuntu-Light").map(|fd| fd.font.clone()) else {
        panic!("egui's default proportional font (Ubuntu-Light) isn't registered under that name any more; update this test");
    };
    let face = ttf_parser::Face::parse(&bytes, 0).unwrap_or_else(|e| panic!("{e:?}"));
    // Serbian Latin's letters beyond plain ASCII: š č ć ž đ (and their capitals).
    let extra = ['š', 'č', 'ć', 'ž', 'đ', 'Š', 'Č', 'Ć', 'Ž', 'Đ'];
    let chars: std::collections::HashSet<char> = extra.into_iter().collect();
    for ch in &chars {
        assert!(face.glyph_index(*ch).is_some(), "Ubuntu-Light lacks {ch}");
    }
}
