//! Bidirectional text (UAX #9) for paragraph layout.
//!
//! The pipeline is the one the algorithm prescribes: embedding levels are resolved once for the
//! whole logical paragraph (base level from `a:pPr/@rtl`); shaping and line breaking happen in
//! logical order; then each line is reordered on its own ([`line_levels`] applies rule L1,
//! [`visual_order`] rule L2). Rule L4 (mirrored brackets) is applied by the shaper, which draws
//! mirrorable characters in right-to-left items with their mirrored glyphs.

use unicode_bidi::BidiClass::{self, AL, AN, B, BN, FSI, LRE, LRI, LRO, PDF, PDI, R, RLE, RLI, RLO, S, WS};
use unicode_bidi::{BidiInfo, Level, bidi_class};

/// Does anything in the text need the full algorithm? (Strong right-to-left, Arabic numbers or
/// explicit embeddings.)
fn needs_bidi(chars: &[char]) -> bool {
    chars.iter().any(|c| matches!(bidi_class(*c), R | AL | AN | LRE | RLE | LRO | RLO | PDF | LRI | RLI | FSI | PDI))
}

/// Resolved embedding level of every character of a paragraph (before the line rules), with the
/// paragraph level forced to 1 when `rtl`.
pub(crate) fn levels(text: &str, chars: &[char], rtl: bool) -> Vec<u8> {
    let base = u8::from(rtl);
    if !rtl && !needs_bidi(chars) {
        return vec![0; chars.len()];
    }
    let info = BidiInfo::new(text, Some(if rtl { Level::rtl() } else { Level::ltr() }));
    // `levels` is per byte; take each character's first byte.
    text.char_indices().map(|(b, _)| info.levels.get(b).map(|l| l.number()).unwrap_or(base)).collect()
}

/// Whitespace-like for rule L1: reset to the paragraph level when trailing a line or preceding a
/// segment/paragraph separator.
fn l1_whitespace(c: BidiClass) -> bool {
    matches!(c, WS | FSI | LRI | RLI | PDI | BN | LRE | RLE | LRO | RLO | PDF)
}

/// Rule L1 for one line: segment and paragraph separators, any whitespace run before them, and
/// whitespace at the end of the line take the paragraph level.
pub(crate) fn line_levels(chars: &[char], levels: &[u8], para_level: u8) -> Vec<u8> {
    let mut out: Vec<u8> = levels.to_vec();
    out.resize(chars.len(), para_level);
    // Walk backwards: `trailing` is true while every character after this one (up to the line end
    // or a separator) is whitespace.
    let mut trailing = true;
    for (i, c) in chars.iter().enumerate().rev() {
        let class = bidi_class(*c);
        if matches!(class, S | B) {
            if let Some(l) = out.get_mut(i) {
                *l = para_level;
            }
            trailing = true;
        } else if l1_whitespace(class) {
            if trailing && let Some(l) = out.get_mut(i) {
                *l = para_level;
            }
        } else {
            trailing = false;
        }
    }
    out
}

/// Rule L2: the visual order of a line's characters (indices into `levels`, left to right). From
/// the highest level down to the lowest odd level, every maximal run at that level or higher is
/// reversed.
pub(crate) fn visual_order(levels: &[u8]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..levels.len()).collect();
    let Some(&max) = levels.iter().max() else { return order };
    let min_odd = levels.iter().copied().filter(|l| l % 2 == 1).min().unwrap_or(max.saturating_add(1));
    let mut lvl = max;
    while lvl >= min_odd && lvl > 0 {
        let mut i = 0;
        while i < order.len() {
            let at = |k: usize| order.get(k).and_then(|&j| levels.get(j)).copied().unwrap_or(0);
            if at(i) >= lvl {
                let start = i;
                while i < order.len() && at(i) >= lvl {
                    i += 1;
                }
                if let Some(run) = order.get_mut(start..i) {
                    run.reverse();
                }
            } else {
                i += 1;
            }
        }
        lvl -= 1;
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lv(s: &str, rtl: bool) -> Vec<u8> {
        let chars: Vec<char> = s.chars().collect();
        levels(s, &chars, rtl)
    }

    #[test]
    fn mixed_arabic_latin_numbers_and_brackets() {
        let s = "سنة 2024 (Retrait) فقط";
        let l = lv(s, true);
        let chars: Vec<char> = s.chars().collect();
        assert_eq!(l.len(), chars.len());
        // Arabic letters at the paragraph level 1; European digits at 2; Latin at 2.
        assert_eq!(l[0], 1);
        assert_eq!(&l[4..8], &[2, 2, 2, 2]);
        assert_eq!(&l[10..17], &[2; 7]);
        assert_eq!(l[19], 1);
        // Brackets around Latin inside an RTL paragraph resolve to the embedding direction (N0).
        assert_eq!(l[9] % 2, 1, "( is right-to-left");
        assert_eq!(l[17] % 2, 1, ") is right-to-left");
        // Visual order, left to right: the Arabic words reversed, Latin and digits kept, the
        // bracket characters swapped (their glyphs are mirrored back by the shaper, rule L4).
        let order = visual_order(&line_levels(&chars, &l, 1));
        let vis: String = order.iter().map(|&i| chars[i]).collect();
        assert_eq!(vis, "طقف )Retrait( 2024 ةنس");
    }

    #[test]
    fn ltr_paragraph_fast_path_and_embedded_rtl() {
        assert_eq!(lv("Hello 123", false), vec![0; 9]);
        let s = "ab سن cd";
        let chars: Vec<char> = s.chars().collect();
        let l = lv(s, false);
        assert_eq!(l, vec![0, 0, 0, 1, 1, 0, 0, 0]);
        let vis: String = visual_order(&l).iter().map(|&i| chars[i]).collect();
        assert_eq!(vis, "ab نس cd");
        // An RTL paragraph of Latin text: the text stays left to right inside it.
        assert_eq!(lv("ab", true), vec![2, 2]);
    }

    #[test]
    fn l1_resets_trailing_whitespace_and_tabs() {
        let chars: Vec<char> = "ab\tcd  ".chars().collect();
        let l = line_levels(&chars, &[2, 2, 2, 2, 2, 2, 2], 1);
        assert_eq!(l, vec![2, 2, 1, 2, 2, 1, 1]);
        // Short level slices never panic.
        assert_eq!(line_levels(&['a', 'b'], &[], 1), vec![1, 1]);
    }

    #[test]
    fn visual_order_edge_cases() {
        assert!(visual_order(&[]).is_empty());
        assert_eq!(visual_order(&[0, 0]), vec![0, 1]);
        assert_eq!(visual_order(&[1, 1, 1]), vec![2, 1, 0]);
        // Numbers (level 2) inside RTL keep their order.
        assert_eq!(visual_order(&[1, 2, 2, 1]), vec![3, 1, 2, 0]);
        assert_eq!(visual_order(&[125, 125]), vec![1, 0]);
    }
}
