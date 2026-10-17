//! Interface language tests (localization).

use super::catalog::Catalog;
use super::*;

fn lang(code: &str) -> Lang {
    Lang::from_code(code).unwrap_or_else(|| panic!("{code} registered"))
}

/// A language tag no catalog will ever have: `qaa`–`qtz` are reserved for local use (ISO 639-2),
/// so these tests keep working as real languages land.
const UNSUPPORTED: &str = "qaa-QM";

#[test]
fn system_locales_map_to_languages() {
    assert_eq!(lang_from_tag("zh_CN.UTF-8"), Some(lang("zh-hans")));
    assert_eq!(lang_from_tag("zh-CN"), Some(lang("zh-hans")));
    assert_eq!(lang_from_tag("zh"), Some(lang("zh-hans")));
    assert_eq!(lang_from_tag("zh-SG"), Some(lang("zh-hans")));
    assert_eq!(lang_from_tag("zh-Hans-HK"), Some(lang("zh-hans")), "a script beats the region");
    assert_eq!(lang_from_tag("en-GB"), Some(Lang::EN));
    assert_eq!(lang_from_tag("C"), Some(Lang::EN));
    assert_eq!(lang_from_tag("POSIX"), Some(Lang::EN));
    assert_eq!(lang_from_tag(UNSUPPORTED), None);
    assert_eq!(lang_from_tag(""), None);
    assert_eq!(lang_from_tag("_"), None);
}

#[test]
fn the_first_supported_preferred_language_wins() {
    assert_eq!(first_supported([UNSUPPORTED, "zh-Hans", "en-US"]), Some(lang("zh-hans")));
    assert_eq!(first_supported([UNSUPPORTED, "en-US"]), Some(Lang::EN));
    assert_eq!(first_supported([UNSUPPORTED]), None);
}

#[test]
fn the_setting_resolves_auto_and_unknown_codes_to_a_supported_language() {
    assert_eq!(normalize_pref("auto"), Some(AUTO));
    assert_eq!(normalize_pref("AUTO"), Some(AUTO));
    assert_eq!(normalize_pref("zh-hans"), Some("zh-hans"));
    assert_eq!(normalize_pref("ZH-Hans"), Some("zh-hans"));
    assert_eq!(normalize_pref("klingon"), None);
    // `from_pref` never fails: an unknown code falls back to the system language.
    assert!(Lang::all().any(|l| l == Lang::from_pref("klingon")));
}

#[test]
fn every_catalog_entry_parses_and_none_is_malformed() {
    for lang in Lang::all() {
        let (catalog, errors) = Catalog::parse(lang.source_for_test());
        assert!(errors.is_empty(), "{:?} has catalog errors: {errors:?}", lang.code());
        if lang == Lang::EN {
            assert_eq!(catalog.sources().count(), 0, "English is the source language, not a catalog");
        }
    }
}
