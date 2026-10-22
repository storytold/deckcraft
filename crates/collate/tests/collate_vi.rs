//! Vietnamese collation (CLDR `vi` standard) through the public API: tailored letters, tones as a
//! secondary difference, canonical equivalence and language-tag handling.
//!
//! Expected orders are fixed vectors for the `vi` collation, written out as literals: they were
//! first checked with ICU 77.1 / CLDR 47 `vi` and are re-checked against ICU4X 2.3.1 in
//! `collate_icu.rs`, both at secondary strength. The text order has no case level (lists of names
//! ignore case), so case-level vectors are not here.

use std::cmp::Ordering;

use deckcraft_collate::{Collation, compare, text_eq, text_key};

/// `xs` sorted (stably) by `compare(.., c)`.
fn sorted(xs: &[&str], c: Collation) -> Vec<String> {
    let mut v: Vec<String> = xs.iter().map(|s| s.to_string()).collect();
    v.sort_by(|a, b| compare(a, b, c));
    v
}

fn vi(a: &str, b: &str) -> Ordering {
    compare(a, b, Collation::Vi)
}

const V1_INPUT: &[&str] = &[
    "Yến", "Ưng", "Uyên", "Út", "Phan", "Phạm", "Ơn", "Ông", "Oanh", "Nguyễn", "Ngô", "Huỳnh", "Hồ", "Hoàng", "Ê-đê", "Em", "Đỗ", "Đinh", "Đặng",
    "Đào", "Dương", "Duy", "Dũng", "Bùi", "Bình", "Âu", "Ấn", "Ân", "Ăn", "Ắc", "Ánh", "Ảnh", "Anh", "An",
];
const V1_EXPECTED: &[&str] = &[
    "An", "Anh", "Ảnh", "Ánh", "Ắc", "Ăn", "Ân", "Ấn", "Âu", "Bình", "Bùi", "Dũng", "Duy", "Dương", "Đào", "Đặng", "Đinh", "Đỗ", "Em", "Ê-đê",
    "Hoàng", "Hồ", "Huỳnh", "Ngô", "Nguyễn", "Oanh", "Ông", "Ơn", "Phạm", "Phan", "Út", "Uyên", "Ưng", "Yến",
];

const V2_EXPECTED: &[&[&str]] =
    &[&["ma", "mà", "mả", "mã", "má", "mạ"], &["MA", "MÀ", "MẢ", "MÃ", "MÁ", "MẠ"], &["ă", "ằ", "ẳ", "ẵ", "ắ", "ặ"], &["ơ", "ờ", "ở", "ỡ", "ớ", "ợ"]];

const V3_INPUT: &[&str] = &[
    "Nguyễn Văn Ấn",
    "Nguyen Thi Ngoc Anh",
    "Nguyễn Thị Ngọc Ánh",
    "Nguyễn Thị Ngọc Anh",
    "Nguyễn Văn An",
    "Ngô Bảo Châu",
    "Nguyễn Thị Ngọc Ảnh",
    "Nguyễn Văn Ân",
    "Đắk Lắk",
    "Đà Nẵng",
    "Hà Nội",
    "Hải Phòng",
    "Huế",
    "Hà Giang",
    "Hòa Bình",
    "Hoà Bình",
    "Cần Thơ",
    "Bà Rịa Vũng Tàu",
];
const V3_EXPECTED: &[&str] = &[
    "Bà Rịa Vũng Tàu",
    "Cần Thơ",
    "Đà Nẵng",
    "Đắk Lắk",
    "Hà Giang",
    "Hà Nội",
    "Hải Phòng",
    "Hoà Bình",
    "Hòa Bình",
    "Huế",
    "Ngô Bảo Châu",
    "Nguyen Thi Ngoc Anh",
    "Nguyễn Thị Ngọc Anh",
    "Nguyễn Thị Ngọc Ảnh",
    "Nguyễn Thị Ngọc Ánh",
    "Nguyễn Văn An",
    "Nguyễn Văn Ân",
    "Nguyễn Văn Ấn",
];

const EQUAL_CI: &[(&str, &str)] = &[
    ("Nguy\u{1ec5}n V\u{103}n An", "nguy\u{1ec5}n v\u{103}n an"),
    ("\u{110}\u{e0} N\u{1eb5}ng", "\u{110}\u{c0} N\u{1eb4}NG"),
    ("\u{c1}nh", "\u{c1}NH"),
];

const CANONICAL_EQUAL: &[(&str, &str)] = &[
    ("\u{1ead}", "a\u{302}\u{323}"),
    ("\u{1eb7}", "a\u{306}\u{323}"),
    ("\u{1ec7}", "e\u{302}\u{323}"),
    ("\u{1ed9}", "o\u{302}\u{323}"),
    ("\u{1ee3}", "o\u{323}\u{31b}"),
    ("\u{1ef1}", "u\u{323}\u{31b}"),
    ("Nguy\u{1ec5}n", "Nguye\u{302}\u{303}n"),
    ("\u{c1}nh", "A\u{301}nh"),
];

/// Both marks have combining class 230: no reordering, so e + tilde + circumflex is not ễ.
const CANONICAL_NOT_EQUAL: &[(&str, &str)] = &[("\u{1ec5}", "e\u{303}\u{302}")];

const ALL_DISTINCT_VI: &[&str] = &["a", "à", "ả", "ã", "á", "ạ", "ă", "ằ", "ẳ", "ẵ", "ắ", "ặ", "â", "ầ", "ẩ", "ẫ", "ấ", "ậ", "ä", "å", "ā", "ą", "ǎ"];

/// ICU `vi` orders these differently; the hand-written key keeps code-point order for
/// punctuation and does not claim parity there.
const PUNCTUATION_CODEPOINT_ORDER: &[&str] = &[" a", "'a", ",a", "-a", ".a", "_a"];

fn assert_strictly_increasing(xs: &[&str], cmp: impl Fn(&str, &str) -> Ordering) {
    for w in xs.windows(2) {
        if let [a, b] = w {
            assert_eq!(cmp(a, b), Ordering::Less, "{a} < {b}");
            assert_eq!(cmp(b, a), Ordering::Greater, "{b} > {a}");
        }
    }
}

#[test]
fn v1_single_syllable_names() {
    assert_eq!(sorted(V1_INPUT, Collation::Vi), V1_EXPECTED);
}

#[test]
fn v2_tones_are_secondary() {
    for xs in V2_EXPECTED {
        assert_strictly_increasing(xs, vi);
    }
}

#[test]
fn v3_multi_word_names_and_places() {
    assert_eq!(sorted(V3_INPUT, Collation::Vi), V3_EXPECTED);
    assert_eq!(vi("Hoà Bình", "Hòa Bình"), Ordering::Less);
}

#[test]
fn equal_ci_ties_keep_input_order() {
    assert_eq!(sorted(&["AN", "an", "An"], Collation::Vi), ["AN", "an", "An"]);
    for (a, b) in EQUAL_CI {
        assert_eq!(vi(a, b), Ordering::Equal, "{a} = {b}");
    }
    // NFC and NFD spellings of one name are a tie: either input order is kept.
    let (nfc, nfd) = ("Nguy\u{1ec5}n", "Nguye\u{302}\u{303}n");
    assert_eq!(sorted(&[nfc, nfd], Collation::Vi), [nfc, nfd]);
    assert_eq!(sorted(&[nfd, nfc], Collation::Vi), [nfd, nfc]);
}

#[test]
fn canonical_equivalence_in_vi() {
    for (a, b) in CANONICAL_EQUAL {
        assert_eq!(vi(a, b), Ordering::Equal, "{a} = {b:?}");
        assert_eq!(text_key(a, Collation::Vi), text_key(b, Collation::Vi), "{a} = {b:?}");
    }
    for (a, b) in CANONICAL_NOT_EQUAL {
        assert_ne!(vi(a, b), Ordering::Equal, "{a} != {b:?}");
    }
}

#[test]
fn vi_letters_beat_foreign_diacritics() {
    // â is a letter of its own in `vi`, so ä (an a with a foreign mark) sorts before ấ; the Root
    // order sorts both with a and lets the marks decide.
    assert_eq!(vi("ä", "ấ"), Ordering::Less);
    assert_eq!(vi("ấ", "ä"), Ordering::Greater);
    assert_eq!(compare("ấ", "ä", Collation::Root), Ordering::Less);
    for (i, a) in ALL_DISTINCT_VI.iter().enumerate() {
        for b in ALL_DISTINCT_VI.iter().skip(i + 1) {
            assert_ne!(vi(a, b), Ordering::Equal, "{a} != {b}");
        }
    }
    // Latin letters outside the alphabet follow their base letter under `vi`: `ä` and `ö` by their
    // marks, `ø` (no decomposition) by its base `o`. Under Root `ø` would sort after `z`.
    assert_eq!(sorted(&["zebra", "ö", "øre", "oz", "ä", "b"], Collation::Vi), ["ä", "b", "ö", "øre", "oz", "zebra"]);
}

#[test]
fn punctuation_keeps_code_point_order() {
    let reversed: Vec<&str> = PUNCTUATION_CODEPOINT_ORDER.iter().rev().copied().collect();
    assert_eq!(sorted(&reversed, Collation::Vi), PUNCTUATION_CODEPOINT_ORDER);
}

#[test]
fn language_tag_variants() {
    for tag in ["vi", "vi-VN", "VI_vn", "vi-"] {
        let c = Collation::from_tag(tag);
        assert_eq!(c, Collation::Vi, "{tag}");
        assert_eq!(sorted(V1_INPUT, c), V1_EXPECTED, "{tag}");
        assert_eq!(compare("Đào", "Dương", c), Ordering::Greater, "{tag}");
    }
    // The Root order (ours, the language-neutral one, not CLDR root) treats Đ as D with a secondary
    // difference.
    for tag in ["", "-", "zz", "v", "vie", "en-VN", "_vi"] {
        let c = Collation::from_tag(tag);
        assert_eq!(c, Collation::Root, "{tag:?}");
        assert_eq!(compare("Đào", "Dương", c), Ordering::Less, "{tag:?}");
    }
    assert_eq!(Collation::default(), Collation::Root);
    assert_eq!(Collation::Root.tag(), "root");
    assert_eq!(Collation::Vi.tag(), "vi");
    for c in [Collation::Root, Collation::Vi] {
        assert_eq!(Collation::from_tag(c.tag()), c);
    }
}

#[test]
fn equality_does_not_depend_on_collation() {
    let mut words: Vec<&str> = V1_INPUT.iter().chain(V3_INPUT).chain(ALL_DISTINCT_VI).copied().collect();
    for (a, b) in CANONICAL_EQUAL.iter().chain(EQUAL_CI).chain(CANONICAL_NOT_EQUAL) {
        words.push(a);
        words.push(b);
    }
    words.extend(["", "đ", "Đ", "d", "\u{301}a", "a", "ß", "ss", "ø", "o\u{338}", "İ", "i\u{307}"]);
    for a in &words {
        for b in &words {
            let eq = text_eq(a, b);
            for c in [Collation::Root, Collation::Vi] {
                assert_eq!(compare(a, b, c) == Ordering::Equal, eq, "{a:?} {b:?} {c:?}");
                assert_eq!(text_key(a, c) == text_key(b, c), eq, "{a:?} {b:?} {c:?}");
            }
        }
    }
}

#[test]
fn ascii_text_sorts_the_same_in_both() {
    let words = ["", " ", "-a", "a", "A", "ab", "aB", "b", "Ba", "z", "Z1", "z0", "~", "_x", "[y", "abc def", "ABC", "0", "10", "9"];
    for a in words {
        for b in words {
            assert_eq!(compare(a, b, Collation::Vi), compare(a, b, Collation::Root), "{a:?} {b:?}");
        }
    }
}

#[test]
fn hostile_text_never_panics_in_vi() {
    let marks = "a".to_string() + &"\u{323}\u{302}".repeat(5_000);
    let leading = "\u{301}\u{323}\u{302}a".to_string();
    let big = "Nguyễn Văn ".repeat(100_000);
    let quality = "o".to_string() + &"\u{31b}\u{302}\u{306}".repeat(3_000);
    let cases: [&str; 13] = [
        "",
        "\u{301}",
        "a\u{301}\u{301}\u{301}",
        "\u{10FFFF}",
        "İ",
        "ẞ",
        "\u{212b}",
        &marks,
        &leading,
        &big,
        "đ\u{323}\u{302}",
        &quality,
        "ßæœĳŀłøŧħıð",
    ];
    for a in cases {
        for b in cases {
            let o = vi(a, b);
            assert_eq!(o, vi(b, a).reverse());
            assert_eq!(o == Ordering::Equal, text_eq(a, b));
            assert_eq!(text_key(a, Collation::Vi).cmp(&text_key(b, Collation::Vi)), o);
        }
    }
    let almost = format!("{big}x");
    assert_ne!(vi(&big, &almost), Ordering::Equal);
    assert_eq!(vi(&big, &almost), Ordering::Less);
}
