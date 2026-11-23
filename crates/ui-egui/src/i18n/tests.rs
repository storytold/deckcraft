//! Interface language tests: catalog format, system-language detection, the setting's fallback,
//! switching the UI language and English fallback for strings without a translation.

use super::catalog::{Catalog, parse_entries, placeholders};
use super::*;

fn lang(code: &str) -> Lang {
    Lang::from_code(code).unwrap_or_else(|| panic!("{code} registered"))
}

/// A language tag no catalog will ever have: `qaa`–`qtz` are reserved for local use (ISO 639-2),
/// so these tests keep working as real languages land.
const UNSUPPORTED: &str = "qaa-QM";

#[test]
fn system_locales_map_to_languages() {
    assert_eq!(lang_from_tag("ja_JP.UTF-8"), Some(lang("ja")));
    assert_eq!(lang_from_tag("ja-JP"), Some(lang("ja")));
    assert_eq!(lang_from_tag("ja"), Some(lang("ja")));
    assert_eq!(lang_from_tag("JA-jp"), Some(lang("ja")));
    assert_eq!(lang_from_tag("en-GB"), Some(Lang::EN));
    assert_eq!(lang_from_tag("C"), Some(Lang::EN));
    assert_eq!(lang_from_tag("POSIX"), Some(Lang::EN));
    assert_eq!(lang_from_tag(UNSUPPORTED), None);
    assert_eq!(lang_from_tag(""), None);
    assert_eq!(lang_from_tag("_"), None);
}

#[test]
fn the_first_supported_preferred_language_wins() {
    // Windows and macOS list several preferred languages in order.
    assert_eq!(first_supported([UNSUPPORTED, "ja-JP", "en-US"]), Some(lang("ja")));
    assert_eq!(first_supported(["en-US", "ja-JP"]), Some(Lang::EN));
    assert_eq!(first_supported([UNSUPPORTED, "qtz"]), None);
    assert_eq!(first_supported(["qtz", "ja-JP"]), Some(lang("ja")));
    assert_eq!(first_supported([]), None);
}

#[test]
fn a_cjk_interface_asks_for_a_cjk_face() {
    assert!(lang("ja").uses_cjk(), "Japanese needs a CJK interface face");
    assert!(!Lang::EN.uses_cjk());
    assert!(lang("ja").catalog_text().contains("挿入"), "the catalog text is what the interface draws");
}

#[test]
fn settings_resolve_with_fallback() {
    assert_eq!(Lang::from_pref("ja"), lang("ja"));
    assert_eq!(Lang::from_pref("JA"), lang("ja"));
    assert_eq!(Lang::from_pref("en"), Lang::EN);
    // `auto` and unknown codes follow the system (English under test).
    assert_eq!(Lang::from_pref(AUTO), Lang::EN);
    assert_eq!(Lang::from_pref("xx-unknown"), Lang::EN);
    assert_eq!(normalize_pref("Ja"), Some("ja"));
    assert_eq!(normalize_pref("Auto"), Some(AUTO));
    assert_eq!(normalize_pref("xx"), None);
    assert_eq!(normalize_pref("jp"), None, "only exact codes are settings");
}

#[test]
fn lookups_fall_back_to_english() {
    let ja = lang("ja");
    assert_eq!(tr(ja, "File"), "ファイル");
    assert_eq!(tr(ja, "Undo"), "元に戻す");
    assert_eq!(tr(ja, "no such label"), "no such label");
    assert_eq!(tr(Lang::EN, "File"), "File");
    assert!(has(ja, "Save") && !has(Lang::EN, "Save"));
}

#[test]
fn switching_the_interface_language_changes_what_is_drawn() {
    let ja = lang("ja");
    set_current(ja);
    assert_eq!(t("Insert"), "挿入");
    assert_eq!(t("Slide Sorter"), "スライド一覧");
    assert_eq!(fmt(t("Slide {current} of {total}"), &[("current", "3"), ("total", "9")]), "スライド 3 / 9");
    // An untranslated string stays English: coverage can grow one entry at a time.
    assert_eq!(t("A label no catalog has"), "A label no catalog has");
    set_current(Lang::EN);
    assert_eq!(t("Insert"), "Insert");
}

#[test]
fn current_language_is_per_thread() {
    set_current(lang("ja"));
    assert_eq!(t("Edit"), "編集");
    std::thread::spawn(|| assert_eq!(current(), Lang::EN)).join().unwrap();
    set_current(Lang::EN);
    assert_eq!(t("Edit"), "Edit");
}

#[test]
fn placeholders_fill_without_reinterpreting_values() {
    let args = [("path", "a-{pages}.pptx"), ("pages", "2")];
    assert_eq!(fmt("{pages}: {path}; {unknown}", &args), "2: a-{pages}.pptx; {unknown}");
    assert_eq!(fmt("text {unfinished", &args), "text {unfinished");
    assert_eq!(fmt(tr(lang("ja"), "Slide {current} of {total}"), &[("current", "3"), ("total", "9")]), "スライド 3 / 9");
}

#[test]
fn malformed_catalog_lines_are_skipped_not_fatal() {
    let (c, errors) = Catalog::parse("# c\nHello\tこんにちは\nOpen\t開く\nPage {n}\tページ\nno tabs\nHello\t重複\nLine\\nTwo\t一\\n二\n");
    // `\tHello\t...` has an empty source column; `Page {n}` keeps a different placeholder.
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert_eq!(c.plain("Hello"), Some("こんにちは"), "the first entry wins");
    assert_eq!(c.plain("Line\nTwo"), Some("一\n二"));
    assert_eq!(c.plain("Page {n}"), None, "a different placeholder set is rejected");
    assert_eq!(c.plain("Open"), Some("開く"));
}

/// Every bundled catalog parses without errors, and its entries keep the English placeholders.
#[test]
fn bundled_catalogs_are_well_formed() {
    let mut codes = std::collections::HashSet::new();
    for l in &LANGUAGES {
        assert!(codes.insert(l.code), "duplicate code {}", l.code);
        if l.code == "en" {
            assert!(l.source.is_empty(), "English is the source language, not a catalog");
            continue;
        }
        let (entries, errors) = parse_entries(l.source);
        assert!(errors.is_empty(), "{}: {errors:?}", l.code);
        assert!(entries.len() > 100, "{}: {} entries", l.code, entries.len());
        for e in &entries {
            assert!(!e.source.trim().is_empty() && !e.translation.trim().is_empty(), "{}: empty entry", l.code);
            assert_eq!(placeholders(&e.source), placeholders(&e.translation), "{}: {:?}", l.code, e.source);
        }
    }
}

/// The first step's coverage: the Japanese catalog translates the menus, the ribbon's tabs and
/// buttons and the status bar, not just a handful of strings.
#[test]
fn japanese_covers_the_menus_ribbon_and_status_bar() {
    let ja = lang("ja");
    for (english, japanese) in [
        ("File", "ファイル"),
        ("Edit", "編集"),
        ("View", "表示"),
        ("Insert", "挿入"),
        ("Home", "ホーム"),
        ("Design", "デザイン"),
        ("Transitions", "画面切り替え"),
        ("Animations", "アニメーション"),
        ("Slide Show", "スライドショー"),
        ("Review", "校閲"),
        ("Undo", "元に戻す"),
        ("Redo", "やり直し"),
        ("Paste", "貼り付け"),
        ("Bold", "太字"),
        ("Italic", "斜体"),
        ("Align Left", "左揃え"),
        ("Paragraph", "段落"),
        ("Slide Sorter", "スライド一覧"),
        ("Fit slide to current window", "スライドをウィンドウに合わせる"),
        ("Interface language:", "表示言語:"),
        ("Cancel", "キャンセル"),
    ] {
        assert_eq!(tr(ja, english), japanese, "{english}");
    }
    assert_eq!(ja.translated_strings(), parse_entries(ja.0.source).0.len());
}
