//! The type-ahead filter for the font name combobox (#933).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::ribbon::{filter_families, filter_sizes};

fn fams() -> Vec<String> {
    ["Arial".to_string(), "Times New Roman".to_string(), "Calibri".to_string(), "Charcoal".to_string(), "Verdana".to_string()].to_vec()
}

/// An empty query keeps every family, in its original order (the two theme fonts stay on top).
#[test]
fn an_empty_query_keeps_the_whole_list() {
    let v = filter_families(&fams(), "");
    assert_eq!(v.len(), 5);
    assert_eq!(v[0], "Arial");
    assert_eq!(v[1], "Times New Roman");
}

/// Typing filters case-insensitively — "CaL" finds "Calibri" (a case-blind match would find
/// nothing, so the single name below proves the query is folded).
#[test]
fn the_query_matches_without_regard_to_case() {
    let v = filter_families(&fams(), "CaL");
    assert_eq!(v, vec!["Calibri".to_string()]);
}

/// Prefixes come before later matches, so the name being typed rises to the top.
#[test]
fn a_prefix_ranks_before_a_match_in_the_middle() {
    let v = filter_families(&fams(), "cha");
    assert_eq!(v, vec!["Charcoal".to_string()]);
    let v = filter_families(&fams(), "ar");
    assert_eq!(v, vec!["Arial".to_string(), "Charcoal".to_string()]);
}

/// Pasted or padded text still filters: the query is trimmed.
#[test]
fn the_query_is_trimmed_before_matching() {
    let v = filter_families(&fams(), "  ver  ");
    assert_eq!(v, vec!["Verdana".to_string()]);
}

/// Nothing matches: the list is empty, not the whole list and not a panic.
#[test]
fn no_match_gives_an_empty_list() {
    assert!(filter_families(&fams(), "zzz").is_empty());
}

/// The size combobox filters on what is written there, not on the number's binary value.
#[test]
fn the_sizes_filter_on_their_text() {
    let sizes = [10.0_f64, 18.0, 20.0, 24.0, 96.0];
    assert_eq!(filter_sizes(&sizes, "2"), vec![20.0, 24.0]);
    assert_eq!(filter_sizes(&sizes, "8"), vec![18.0]);
    assert_eq!(filter_sizes(&sizes, ""), sizes.to_vec());
}
