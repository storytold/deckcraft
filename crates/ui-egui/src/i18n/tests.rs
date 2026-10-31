//! Unit and catalog integrity tests for DeckCraft i18n (`en` / `vi`).

use std::collections::HashSet;

use serde_json::json;

use super::catalog::{Catalog, VI_TSV, vi_catalog};
use super::*;

fn extract_placeholders(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut token = String::from("{");
            for nxt in chars.by_ref() {
                token.push(nxt);
                if nxt == '}' {
                    break;
                }
            }
            if token.ends_with('}') {
                out.push(token);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn locale_tags_resolve_to_supported_languages() {
    for tag in [
        "vi",
        "VI",
        "vi-VN",
        "vi_VN",
        "vi_VN.UTF-8",
        "vi_VN.UTF-8@anything",
        "vie",
        "Tiếng Việt",
        "tieng viet",
        "Vietnamese",
    ] {
        assert_eq!(lang_from_tag(tag), Some(Lang::VI), "tag {tag:?} should map to VI");
    }

    for tag in ["en", "EN", "en-US", "en_GB.UTF-8", "eng", "English", "C", "POSIX"] {
        assert_eq!(lang_from_tag(tag), Some(Lang::EN), "tag {tag:?} should map to EN");
    }

    for unsupported in ["", "   ", "qaa-QM", "fr-FR", "ja_JP.UTF-8"] {
        assert_eq!(lang_from_tag(unsupported), None, "tag {unsupported:?} should be None");
    }
}

#[test]
fn preferences_parse_and_normalize_with_fallback() {
    assert_eq!(Lang::from_pref("vi"), Lang::VI);
    assert_eq!(Lang::from_pref("VI-VN"), Lang::VI);
    assert_eq!(Lang::from_pref("Tiếng Việt"), Lang::VI);
    assert_eq!(Lang::from_pref("en"), Lang::EN);
    assert_eq!(Lang::from_pref("en-US"), Lang::EN);
    // Under #[cfg(test)], system_lang() is deterministic (Lang::EN).
    assert_eq!(Lang::from_pref("auto"), Lang::EN);
    assert_eq!(Lang::from_pref("system"), Lang::EN);
    assert_eq!(Lang::from_pref(""), Lang::EN);
    assert_eq!(Lang::from_pref("unknown-tag"), Lang::EN);

    assert_eq!(Lang::normalize_pref(""), "auto");
    assert_eq!(Lang::normalize_pref("AUTO"), "auto");
    assert_eq!(Lang::normalize_pref("system"), "auto");
    assert_eq!(Lang::normalize_pref("vi"), "vi");
    assert_eq!(Lang::normalize_pref("vi_VN.UTF-8"), "vi");
    assert_eq!(Lang::normalize_pref("en-GB"), "en");
    assert_eq!(Lang::normalize_pref("unknown"), "en");
}

#[test]
fn env_locale_detection_honors_priority_and_skips_c_locale() {
    let lang = detect_system_lang_from_env(|key| match key {
        "LC_ALL" => Some("C.UTF-8".into()),
        "LANG" => Some("vi_VN.UTF-8".into()),
        _ => None,
    });
    assert_eq!(lang, Some(Lang::VI));

    let colon_list = detect_system_lang_from_env(|key| match key {
        "LANGUAGE" => Some("qaa_QM:vi_VN:en_US".into()),
        _ => None,
    });
    assert_eq!(colon_list, Some(Lang::VI));

    let override_var = detect_system_lang_from_env(|key| match key {
        "DECKCRAFT_LOCALE" => Some("vi".into()),
        "LC_ALL" => Some("en_US.UTF-8".into()),
        _ => None,
    });
    assert_eq!(override_var, Some(Lang::VI));
}

#[test]
fn thread_local_language_is_isolated_per_thread() {
    set_current(Lang::VI);
    assert_eq!(current(), Lang::VI);
    assert_eq!(t("File"), "Tệp");
    assert_eq!(t("Slide Show"), "Trình chiếu");

    std::thread::spawn(|| {
        assert_eq!(current(), Lang::EN);
        assert_eq!(t("File"), "File");
    })
    .join()
    .unwrap();

    set_current(Lang::EN);
    assert_eq!(current(), Lang::EN);
    assert_eq!(t("File"), "File");
}

#[test]
fn context_plural_and_placeholder_helpers_work() {
    let custom_tsv = "\
# sample catalog
View\tXem
noun::View\tChế độ xem
Slide#one\t{n} trang chiếu
Slide#other\t{n} trang chiếu
Slide {cur} of {total}\tTrang chiếu {cur} / {total}
Exported {}\tĐã xuất {}
";
    let cat = Catalog::parse(custom_tsv);
    assert_eq!(cat.get("View"), Some("Xem"));
    assert_eq!(cat.get_ctx("noun", "View"), Some("Chế độ xem"));
    assert_eq!(cat.get_ctx("unknown_ctx", "View"), Some("Xem"));

    set_current(Lang::VI);
    assert_eq!(
        tf("Slide {i} of {n}", &[("i", "3"), ("n", "12")]),
        "Trang chiếu 3 / 12"
    );
    assert_eq!(
        tl!("Slide {i} of {n}", i = 3, n = 12),
        "Trang chiếu 3 / 12"
    );
    assert_eq!(
        tl!("Exported {}", "deck.pdf"),
        "Đã xuất deck.pdf"
    );
    assert_eq!(tn("{n} slide", "{n} slides", 1), "1 trang chiếu");
    assert_eq!(tn("{n} slide", "{n} slides", 5), "5 trang chiếu");

    set_current(Lang::EN);
    assert_eq!(tn("{n} slide", "{n} slides", 1), "1 slide");
    assert_eq!(tn("{n} slide", "{n} slides", 5), "5 slides");
}

#[test]
fn vietnamese_catalog_is_well_formed_and_preserves_placeholders() {
    let cat = vi_catalog();
    assert!(
        cat.len() >= 2000,
        "expected comprehensive Vietnamese catalog (>= 2000 entries), got {}",
        cat.len()
    );

    let mut seen_keys = HashSet::new();
    for (line_no, raw_line) in VI_TSV.lines().enumerate() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim_start_matches(' ');
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((raw_k, raw_v)) = line.split_once('\t') else {
            panic!("line {} in vi.tsv is missing a tab separator: {:?}", line_no + 1, line);
        };
        assert!(
            !raw_k.is_empty() && !raw_v.is_empty(),
            "line {} in vi.tsv has empty key or value",
            line_no + 1
        );
        assert!(
            seen_keys.insert(raw_k),
            "duplicate key on line {} in vi.tsv: {:?}",
            line_no + 1,
            raw_k
        );
    }

    for (k, v) in cat.iter() {
        assert_eq!(
            extract_placeholders(k),
            extract_placeholders(v),
            "placeholder mismatch for key {k:?} -> {v:?}"
        );
        if !k.contains("...") {
            assert!(
                !v.contains("..."),
                "use single-character ellipsis '…' instead of '...' in translation for {k:?}: {v:?}"
            );
        }
        if k.ends_with('…') {
            assert!(
                v.ends_with('…'),
                "translation for {k:?} must preserve trailing ellipsis '…': {v:?}"
            );
        }
    }
}

#[test]
fn vietnamese_covers_deckcraft_ribbon_backstage_views_and_dialogs() {
    for (en, expected_vi) in [
        ("File", "Tệp"),
        ("Home", "Trang đầu"),
        ("Insert", "Chèn"),
        ("Design", "Thiết kế"),
        ("Transitions", "Chuyển tiếp"),
        ("Animations", "Hoạt ảnh"),
        ("Slide Show", "Trình chiếu"),
        ("Review", "Rà soát"),
        ("View", "Xem"),
        ("Undo", "Hoàn tác"),
        ("Redo", "Làm lại"),
        ("Cut", "Cắt"),
        ("Copy", "Sao chép"),
        ("Paste", "Dán"),
        ("New Slide", "Trang chiếu mới"),
        ("Layout", "Bố cục"),
        ("Reset", "Đặt lại"),
        ("Section", "Phân mục"),
        ("Selection Pane", "Ngăn lựa chọn"),
        ("Animation Pane", "Ngăn hiệu ứng động"),
        ("Slide Sorter", "Sắp xếp trang chiếu"),
        ("Notes Page", "Trang ghi chú"),
        ("Reading View", "Chế độ đọc"),
        ("Presenter View", "Chế độ diễn giả"),
        ("Preferences", "Tùy chọn"),
        ("Language", "Ngôn ngữ"),
        ("System default", "Mặc định hệ thống"),
    ] {
        assert_eq!(tr(Lang::VI, en), expected_vi, "unexpected translation for {en:?}");
    }
}

#[test]
fn language_command_updates_ui_state_and_persists() {
    let mut app = crate::SlideApp::new(
        deckcraft_engine::Session::new(),
        crate::Services::default(),
    );
    assert_eq!(app.ui.language, "auto");

    let res = app
        .run("ui.language", json!({ "language": "vi-VN" }))
        .expect("ui.language command should succeed");
    assert_eq!(app.ui.language, "vi");
    assert_eq!(res["language"], "vi");
    assert_eq!(res["resolved"], "vi");
    assert_eq!(current(), Lang::VI);

    let serialized = serde_json::to_string(&app.ui).expect("serialize UiState");
    let restored: crate::UiState = serde_json::from_str(&serialized).expect("deserialize UiState");
    assert_eq!(restored.language, "vi");
    assert_eq!(Lang::from_pref(&restored.language), Lang::VI);

    // Legacy settings without `language` field default to `"auto"`.
    let legacy: crate::UiState = serde_json::from_str("{}").expect("deserialize legacy UiState");
    assert_eq!(legacy.language, "auto");

    // Reset back to English for subsequent tests.
    let _ = app.run("ui.language", json!({ "language": "en" }));
    assert_eq!(current(), Lang::EN);
}

#[test]
fn every_command_and_menu_path_is_translated() {
    let cat = vi_catalog();
    let mut missing = Vec::new();
    for spec in deckcraft_engine::cmd::command_specs() {
        if !spec.label.is_empty() && cat.get(spec.label).is_none() {
            missing.push(format!("engine command {} label {:?}", spec.id, spec.label));
        }
        for seg in spec.menu {
            if !seg.is_empty() && cat.get(seg).is_none() {
                missing.push(format!("engine command {} menu {:?}", spec.id, seg));
            }
        }
    }
    for (id, label, _, _) in crate::UI_COMMANDS {
        if !label.is_empty() && cat.get(label).is_none() {
            missing.push(format!("ui command {id} label {label:?}"));
        }
    }
    assert!(
        missing.is_empty(),
        "untranslated command labels or menu segments: {missing:#?}"
    );
}
