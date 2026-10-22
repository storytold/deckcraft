//! What the craft apps need on top of the collation: natural (number-aware) order for file names,
//! whole-letter search with the Turkish and Greek folds of the craft apps' search, and language tags.

use std::cmp::Ordering;

use deckcraft_collate::{Collation, compare, natural, search_contains, search_contains_folded, search_fold, text_key};

#[test]
fn natural_numbers_by_value_letters_by_collation() {
    let nfd10 = "Ba\u{301}o ca\u{301}o 10.pdf";
    let nfc2 = "Báo cáo 2.pdf";
    assert_eq!(natural(nfc2, nfd10, Collation::Root), Ordering::Less);
    assert_eq!(natural("scan2", "scan10", Collation::Root), Ordering::Less);
    assert_eq!(natural("a007", "a7", Collation::Root), Ordering::Equal); // like upstream: leading zeros are dropped
    assert_eq!(natural("Đà Nẵng.pdf", "Dung.pdf", Collation::Vi), Ordering::Greater);
    assert_eq!(natural("Đà Nẵng.pdf", "Dung.pdf", Collation::Root), Ordering::Less);
    let long = format!("x{}", "9".repeat(400));
    assert_eq!(natural(&long, "x10", Collation::Root), Ordering::Greater);
}

/// Upstream's character-by-character natural order (Combine file list), the oracle for names
/// without letters that differ in case or encoding.
fn upstream_natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        it.next();
                    }
                    digits.trim_start_matches('0').to_owned()
                };
                let (x, y) = (take(&mut a), take(&mut b));
                match x.len().cmp(&y.len()).then_with(|| x.cmp(&y)) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            (Some(x), Some(y)) => {
                match x.to_lowercase().cmp(y.to_lowercase()) {
                    Ordering::Equal => {}
                    other => return other,
                }
                a.next();
                b.next();
            }
        }
    }
}

#[test]
fn natural_keeps_a_name_before_its_numbered_copies() {
    for c in [Collation::Root, Collation::Vi] {
        assert_eq!(natural("scan.pdf", "scan1.pdf", c), Ordering::Less);
        assert_eq!(natural("dir/a/x.pdf", "dir/a1/x.pdf", c), Ordering::Less);
        assert_eq!(natural("a-1", "a1", c), Ordering::Less); // '-' 0x2D < '1', as upstream
        assert_eq!(natural("a-1", "a1", c), upstream_natural("a-1", "a1"));
        let mut names = vec!["scan10.pdf", "scan2.pdf", "scan.pdf", "scan1.pdf", "scan02.pdf"];
        names.sort_by(|a, b| natural(a, b, c));
        assert_eq!(names, ["scan.pdf", "scan1.pdf", "scan2.pdf", "scan02.pdf", "scan10.pdf"]);
    }
    assert_eq!(natural("Dung.pdf", "Đà Nẵng.pdf", Collation::Vi), Ordering::Less);
    assert_eq!(natural("Đà Nẵng.pdf", "Dung.pdf", Collation::Vi), Ordering::Greater);
}

#[test]
fn natural_agrees_with_upstream_on_plain_names() {
    let words = [
        "",
        "a",
        "a1",
        "a2",
        "a10",
        "a010",
        "a-1",
        "a_1",
        "a 1",
        "a.1",
        "a/1",
        "a1b",
        "a1b2",
        "a1-b",
        "a-b",
        "a.b",
        "a/b",
        "b",
        "1",
        "01",
        "10",
        "2",
        "a0",
        "a00",
        "x0y",
        "x00y",
        "x1y",
        "scan",
        "scan.pdf",
        "scan1.pdf",
        "scan2.pdf",
        "scan10.pdf",
        "dir/a/x.pdf",
        "dir/a1/x.pdf",
        "dir/a2/x.pdf",
        "dir/b/x.pdf",
        "9",
        "09",
        "99",
        "100",
        "a99",
        "a100",
        "ab12",
        "ab2c",
    ];
    for a in words {
        for b in words {
            for c in [Collation::Root, Collation::Vi] {
                assert_eq!(natural(a, b, c), upstream_natural(a, b), "{a:?} {b:?} {c:?}");
            }
        }
    }
}

#[test]
fn natural_digits_with_marks_and_cap() {
    // A mark after a number belongs to the number; a number is not split by it.
    for c in [Collation::Root, Collation::Vi] {
        assert_eq!(natural("a1\u{301}", "a1\u{301}", c), Ordering::Equal);
        assert_ne!(natural("a1\u{301}", "a1", c), Ordering::Equal);
        assert_eq!(natural("a1\u{301}2", "a01\u{301}002", c), Ordering::Equal);
        assert_eq!(natural("a0", "a000", c), Ordering::Equal);
        assert_eq!(natural("a0", "a1", c), Ordering::Less);
        assert_eq!(natural("a9", "a10", c), Ordering::Less);
        // A number longer than the primary cap still orders by value and is not quadratic.
        let long9 = format!("x{}", "9".repeat(50_000));
        let long9z = format!("x{}", "9".repeat(49_999)) + "8";
        assert_eq!(natural(&long9, &long9z, c), Ordering::Greater);
        assert_eq!(natural(&long9z, &long9, c), Ordering::Less);
        assert_eq!(natural(&long9, &long9, c), Ordering::Equal);
        assert_eq!(natural(&format!("{}1", "0".repeat(50_000)), "1", c), Ordering::Equal);
        // More than 16 384 elements: the rest still takes part through the text.
        let many = "a1".repeat(20_000);
        assert_eq!(natural(&many, &(many.clone() + "2"), c), Ordering::Less);
    }
}

#[test]
fn natural_edge_cases() {
    for c in [Collation::Root, Collation::Vi] {
        assert_eq!(natural("", "", c), Ordering::Equal);
        assert_eq!(natural("", "a", c), Ordering::Less);
        assert_eq!(natural("a", "", c), Ordering::Greater);
        assert_eq!(natural("a", "a1", c), Ordering::Less); // the string that ends first is smaller
        assert_eq!(natural("a1", "a", c), Ordering::Greater);
        assert_eq!(natural("1", "a", c), Ordering::Less); // a digit run against a letter run: digits first
        assert_eq!(natural("file2b", "file2a", c), Ordering::Greater);
        assert_eq!(natural("file02b", "file2a", c), Ordering::Greater);
        assert_eq!(natural("a0", "a00", c), Ordering::Equal);
        assert_eq!(natural("a0", "a1", c), Ordering::Less);
        // Case and encoding do not matter in the letter runs.
        assert_eq!(natural("Báo 2", "BA\u{301}O 2", c), Ordering::Equal);
        // Only ASCII digits are number runs.
        assert_eq!(natural("a\u{661}", "a\u{662}", c), compare("a\u{661}", "a\u{662}", c));
    }
}

#[test]
fn natural_is_a_consistent_order() {
    let words = [
        "",
        "a",
        "a1",
        "a2",
        "a10",
        "a010",
        "A1",
        "Á1",
        "A\u{301}1",
        "a1b",
        "a1b2",
        "b",
        "scan9",
        "scan10",
        "scan010",
        "Đà 2",
        "Da 10",
        "1",
        "01",
        "10",
    ];
    for c in [Collation::Root, Collation::Vi] {
        for a in words {
            for b in words {
                assert_eq!(natural(a, b, c), natural(b, a, c).reverse(), "{a:?} {b:?} {c:?}");
                for d in words {
                    if natural(a, b, c) != Ordering::Greater && natural(b, d, c) != Ordering::Greater {
                        assert_ne!(natural(a, d, c), Ordering::Greater, "{a:?} {b:?} {d:?} {c:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn search_keeps_turkish_and_sigma_and_whole_letters() {
    assert!(search_contains("İSTANBUL ŞUBESİ", "istanbul"));
    assert!(search_contains("Kış dönemi", "KIŞ"));
    assert!(search_contains("ΟΔΟΣ", "οδος"));
    assert!(search_contains("Chu\u{31b}o\u{31b}ng 1", "Chương"));
    assert!(search_contains("NGUYE\u{302}\u{303}N", "nguyễn"));
    assert!(!search_contains("Nguyễn", "e"));
    assert!(search_contains("anything", ""));
}

#[test]
fn search_fold_folds_the_key_and_maps_i_and_sigma() {
    // The case fold the craft apps' search uses: I i İ ı -> i (the dot of İ goes in NFC and NFD),
    // Σ σ ς -> σ.
    assert_eq!(search_fold("I\u{130}\u{131}i"), "iiii");
    assert_eq!(search_fold("I\u{130}\u{131}i"), search_fold("II\u{307}\u{131}i"));
    assert_eq!(search_fold("Σσς"), "σσσ");
    // On the key: lowercase, decomposed, marks in canonical order.
    assert_eq!(search_fold("NGUYỄN"), search_fold("nguye\u{302}\u{303}n"));
    assert_eq!(search_fold("Ánh"), "a\u{301}nh");
    // The folded form searches without folding again; overlapping occurrences are all tried.
    assert!(search_contains_folded(&search_fold("ễ e"), &search_fold("e")));
    assert!(!search_contains_folded(&search_fold("ễ"), &search_fold("e")));
    assert!(search_contains_folded("anything", ""));
    assert!(search_contains_folded("", ""));
    assert!(!search_contains_folded("", "a"));
}

#[test]
fn tags_map_to_collations() {
    for t in ["vi", "vi-VN", "VI_vn", " vi "] {
        assert_eq!(Collation::from_tag(t), Collation::Vi, "{t:?}");
    }
    for t in ["", "-", "zz", "en-US", "vie", "v"] {
        assert_eq!(Collation::from_tag(t), Collation::Root, "{t:?}");
    }
    assert_eq!(Collation::Root.tag(), "root");
    assert_eq!(Collation::Vi.tag(), "vi");
}

#[test]
fn hostile_text_never_panics() {
    let marks = "a".to_string() + &"\u{323}\u{302}".repeat(5_000); // 10 000 combining marks
    let leading = "\u{301}\u{323}\u{302}a".to_string(); // marks first
    let big = "ệ".repeat(30_000); // 90 KB: well past the 16 384-element key cap
    let digits = format!("x{}y{}", "7".repeat(30_000), "9".repeat(30_000));
    let cases: [&str; 7] = ["", "\u{10FFFF}", &marks, &leading, &big, &digits, "\u{301}"];
    for c in [Collation::Root, Collation::Vi] {
        for a in cases {
            let ka = text_key(a, c);
            for b in cases {
                let o = compare(a, b, c);
                assert_eq!(o, compare(b, a, c).reverse());
                assert_eq!(ka.cmp(&text_key(b, c)), o);
                assert_eq!(natural(a, b, c), natural(b, a, c).reverse());
                let _ = search_contains(a, b);
            }
        }
    }
}
