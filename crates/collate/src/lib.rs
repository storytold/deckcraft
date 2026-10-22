//! Text collation for the lists the craft apps sort and filter (canonical equivalence, Vietnamese
//! order, natural order, whole-letter search).
//!
//! - **Canonical equivalence.** A letter typed precomposed (`ế`, U+1EBF) and typed as base + marks
//!   (`e` U+0302 U+0301) is the same text, and so are marks of different combining classes typed
//!   in either order. [`canonical`] gives the one form the key builder reads; [`fold_key`] adds
//!   case; [`text_eq`] and [`compare`] agree with it: two strings are equal exactly when their
//!   [`fold_key`]s are. This holds for the Latin letters the tables cover (below): a precomposed
//!   letter of another script is not decomposed, so Cyrillic `й` and `и` + U+0306, or Greek `ά`
//!   and `α` + U+0301, stay different.
//! - **Order.** [`Collation::Root`] sorts accented letters with their base letter; [`Collation::Vi`]
//!   is the CLDR `vi` standard order (`a ă â b c d đ e ê …`, tones as a secondary difference).
//!   [`natural`] sorts digit runs by value on top of either.
//! - **Search.** [`search_contains`] ignores case and encoding and matches whole letters, with the
//!   Turkish `i` (`İ` in NFC or NFD) and Greek sigma folds of the craft apps' search
//!   ([`search_fold`]).
//!
//! Nothing here changes the user's text: it builds keys and answers comparisons only. The data
//! behind the key is in `ucd.rs`: the canonical decomposition of the Latin letters
//! U+00C0..=U+024F and U+1E00..=U+1EFF (497 characters) and the 111 combining marks of
//! U+0300..=U+036F with a nonzero combining class, four of which decompose to other marks (U+0340
//! and U+0341, the grave and acute tone marks, are U+0300 and U+0301; U+0343 is U+0313; U+0344 is
//! U+0308 U+0301). Anything outside them is left alone (a
//! starter without a decomposition): this is not a general NFD, it is the part of NFD that
//! Vietnamese and the Latin scripts need. The tables are generated from the Unicode Character
//! Database 17.0 (via ICU4X `icu_normalizer` =2.3.0, data generated with CLDR 48.2.1 and
//! ICU release-78.1rc, as stated in `icu_normalizer_data` 2.3.0) and verified against it by the
//! test `tables_match_icu_normalizer`. Canonical decompositions are frozen by Unicode's normalization
//! stability policy, so later Unicode versions do not change them.
//!
//! Nothing here panics: lookups use `get` and binary search, and the work is linear in the input
//! except for the sort of a mark run, which is capped at 32 marks.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod ucd;

use std::cmp::Ordering;

use ucd::{COMBINING_CLASS, DECOMPOSITION};

/// Longest run of combining marks that is reordered. A longer run is returned in input order:
/// it is not text, and sorting it would let one hostile string cost quadratic time.
const MAX_RUN: usize = 32;

/// First scalar of the first range in [`DECOMPOSITION`].
const DEC_FIRST: u32 = 0xc0;

/// The canonical combining class of `c` (0 for a starter and for every character outside the
/// table).
pub(crate) fn ccc(c: char) -> u8 {
    if !('\u{300}'..='\u{36f}').contains(&c) {
        return 0;
    }
    match COMBINING_CLASS.binary_search_by_key(&c, |&(k, _)| k) {
        Ok(i) => COMBINING_CLASS.get(i).map_or(0, |&(_, class)| class),
        Err(_) => 0,
    }
}

/// The full canonical decomposition of `c`, or `None` when it has none in the table.
fn decompose(c: char) -> Option<&'static str> {
    if (c as u32) < DEC_FIRST {
        return None;
    }
    let i = DECOMPOSITION.binary_search_by_key(&c, |&(k, _)| k).ok()?;
    DECOMPOSITION.get(i).map(|&(_, d)| d)
}

/// `s` with every character in the table fully decomposed and each run of combining marks put in
/// canonical order (NFC and NFD text give the same result). Case is kept
/// and characters outside the table are kept as they are. A run of more than 32 marks keeps its
/// input order. ASCII text comes back unchanged.
pub fn canonical(s: &str) -> String {
    if s.is_ascii() {
        return s.to_string();
    }
    let mut chars: Vec<char> = Vec::with_capacity(s.len());
    for c in s.chars() {
        match decompose(c) {
            Some(d) => chars.extend(d.chars()),
            None => chars.push(c),
        }
    }
    reorder_marks(&mut chars);
    chars.into_iter().collect()
}

/// Put each maximal run of marks with a nonzero combining class into ascending class order,
/// keeping the order of marks of equal class (a stable sort). Runs longer than [`MAX_RUN`] stay.
fn reorder_marks(chars: &mut [char]) {
    let mut i = 0;
    while i < chars.len() {
        if chars.get(i).is_none_or(|&c| ccc(c) == 0) {
            i += 1;
            continue;
        }
        let start = i;
        while chars.get(i).is_some_and(|&c| ccc(c) != 0) {
            i += 1;
        }
        if i.saturating_sub(start) <= MAX_RUN
            && let Some(run) = chars.get_mut(start..i)
        {
            run.sort_by_key(|&c| ccc(c));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The Root text order.
// ---------------------------------------------------------------------------------------------

/// Most primary elements a key holds: a megabyte paste does not make a megabyte key. The rest of
/// a longer text only takes part through the final tie-break on [`fold_key`], which always holds
/// the whole text.
///
/// A digit run of a natural key ([`natural`]) is one element however long it is, so such a key can
/// exceed this by the digits of the run that crosses the cap (the count and the digits are pushed
/// together); the work stays linear in the text.
const MAX_PRIMARY: usize = 16_384;

/// Secondary code of the stroke of `đ` (primary `d`). Mark codes start at 32, so it never equals
/// one.
const STROKE: u16 = 11;

/// Lowercase one char to exactly one char (no full case folding).
fn lower_char(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Whether `c` is a combining mark the key attaches to the element before it.
fn is_mark(c: char) -> bool {
    ('\u{300}'..='\u{36f}').contains(&c)
}

/// The secondary code of combining mark `c`: `32 + (cp - 0x300)`.
fn mark_code(c: char) -> u16 {
    u16::try_from((c as u32).saturating_sub(0x300).saturating_add(32)).unwrap_or(u16::MAX)
}

/// Append the lowercase, fully decomposed form of `c` to `out` (marks are not yet reordered).
///
/// A character the table decomposes is decomposed first and the pieces lowercased, so `İ`
/// (U+0130, whose one-to-one lowercase would lose its dot) equals `I` + U+0307. Any other
/// character is lowercased first, so one whose lowercase is a precomposed letter (the Angstrom
/// sign U+212B, lowercase `å`) is decomposed too. Either way NFC and NFD text give the same chars.
fn push_folded(c: char, out: &mut Vec<char>) {
    fold_char(c, |x| out.push(x));
}

/// [`push_folded`] without a buffer: hands each char of the folded form of `c` to `emit`.
fn fold_char(c: char, mut emit: impl FnMut(char)) {
    if let Some(d) = decompose(c) {
        d.chars().map(lower_char).for_each(emit);
        return;
    }
    let lowered = lower_char(c);
    match decompose(lowered) {
        Some(d) => d.chars().for_each(emit),
        None => emit(lowered),
    }
}

/// The form two strings must share to be equal as text: lowercased one char to one char, fully
/// decomposed, each run of combining marks in canonical order. Two strings are equal by
/// [`compare`] and [`text_eq`] exactly when their `fold_key`s are equal, so it is the key to
/// group, deduplicate and look up by (`HashMap`, `BTreeMap`). It never changes the user's text.
pub fn fold_key(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_lowercase();
    }
    let mut chars: Vec<char> = Vec::with_capacity(s.len());
    for c in s.chars() {
        push_folded(c, &mut chars);
    }
    reorder_marks(&mut chars);
    chars.into_iter().collect()
}

/// Whether `a` and `b` are the same text, ignoring case and encoding (`é` precomposed equals `e`
/// + U+0301; `Ánh` equals `ÁNH`). The same relation as `compare(a, b, c) == Equal` for every `c`.
pub fn text_eq(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(b);
    }
    fold_key(a) == fold_key(b)
}

// ---------------------------------------------------------------------------------------------
// Collations: the Vietnamese order (CLDR `vi` standard) next to the Root order.
// ---------------------------------------------------------------------------------------------

/// Vietnamese letters with a quality mark: letters of their own, after their base letter
/// (`(letter, base, rank)`, weight `(base << 4) + rank`). Tones are not letters: they are a
/// secondary difference ([`TONES`]).
const VI: &[(char, char, u32)] = &[('ă', 'a', 1), ('â', 'a', 2), ('đ', 'd', 1), ('ê', 'e', 1), ('ô', 'o', 1), ('ơ', 'o', 2), ('ư', 'u', 1)];

/// A base letter and the quality mark that make a Vietnamese letter of their own (the first step
/// of the Vietnamese key, in `Key::vi`).
const QUALITY: &[(char, char, char)] =
    &[('a', '\u{306}', 'ă'), ('a', '\u{302}', 'â'), ('e', '\u{302}', 'ê'), ('o', '\u{302}', 'ô'), ('o', '\u{31b}', 'ơ'), ('u', '\u{31b}', 'ư')];

/// The Vietnamese tone marks and their secondary weights, in `vi` order: (no tone 0), huyền,
/// hỏi, ngã, sắc, nặng.
const TONES: &[(char, u16)] = &[('\u{300}', 1), ('\u{309}', 2), ('\u{303}', 3), ('\u{301}', 4), ('\u{323}', 5)];

/// Lowercase Latin letters that have no canonical decomposition, so [`fold_key`] leaves them
/// whole, but that sort with their base letters in `vi`: `(letter, base letters, code)`. The
/// code is a secondary difference below every mark code: 10 dot, 11 stroke, 16 ligature,
/// 17 dotless, 18 eth.
const VI_FOREIGN: &[(char, &str, u16)] = &[
    ('æ', "ae", 16),
    ('ð', "d", 18),
    ('ħ', "h", 11),
    ('ı', "i", 17),
    ('ĳ', "ij", 16),
    ('ŀ', "l", 10),
    ('ł', "l", 11),
    ('ø', "o", 11),
    ('œ', "oe", 16),
    ('ß', "ss", 16),
    ('ŧ', "t", 11),
];

/// The primary weight of letter `c` at `rank` places after it.
fn weight(c: char, rank: u32) -> u32 {
    (c as u32).saturating_mul(16).saturating_add(rank)
}

/// The tone weight of mark `m` ([`TONES`]), 0 when it is not a tone mark.
fn tone(m: char) -> u16 {
    TONES.iter().find(|(t, _)| *t == m).map_or(0, |(_, w)| *w)
}

/// Rule V1 of [`Key::vi`]: `base` merged with each quality mark among `marks` (the marks after it,
/// in canonical order) that is not blocked by an earlier mark kept with a combining class >= its
/// own. Every mark that does not merge goes to `keep`, in order.
fn vi_merge(mut base: char, marks: impl Iterator<Item = char>, mut keep: impl FnMut(char)) -> char {
    let mut top = 0u8;
    for m in marks {
        let class = ccc(m);
        match QUALITY.iter().find(|(b, q, _)| *b == base && *q == m) {
            Some((_, _, merged)) if top < class => base = *merged,
            _ => {
                keep(m);
                top = top.max(class);
            }
        }
    }
    base
}

/// The weights of a (merged) base letter in `vi`: its first primary weight, a second one for a
/// ligature (`æ` is `a` `e`), and its [`VI_FOREIGN`] code (0 for every other letter).
fn vi_letter(c: char) -> (u32, Option<u32>, u16) {
    if let Some((_, base, rank)) = VI.iter().find(|(x, _, _)| *x == c) {
        return (weight(*base, *rank), None, 0);
    }
    if let Some((_, bases, code)) = VI_FOREIGN.iter().find(|(x, _, _)| *x == c) {
        let mut b = bases.chars();
        let first = b.next().map_or_else(|| weight(c, 0), |x| weight(x, 0));
        return (first, b.next().map(|x| weight(x, 0)), *code);
    }
    (weight(c, 0), None, 0)
}

/// Which text order to sort and compare by. Equality never depends on it: two strings compare
/// equal in every collation exactly when [`text_eq`] holds; only the order of unequal text
/// changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Collation {
    /// The language-neutral order: accented letters sort with their base letter (`Anh` < `Ánh` <
    /// `Ảnh`), `đ` with `d`, everything else by code point. That includes the Latin letters with no
    /// decomposition (`ø`, `ł`, `æ`, `ß`, `ð`, `ħ`, `ı`…), which sort after `z`; [`Collation::Vi`]
    /// puts them with their base letter.
    #[default]
    Root,
    /// Vietnamese, the CLDR `vi` standard order: `a ă â b c d đ e ê … o ô ơ … u ư …`, tones as a
    /// secondary difference (none, huyền, hỏi, ngã, sắc, nặng).
    Vi,
}

impl Collation {
    /// The collation a BCP 47 language tag asks for, by its primary language subtag: after
    /// trimming, `_` read as `-` and lowercasing, `vi` and anything that starts with `vi-` (`vi-VN`,
    /// `VI_vn`) give [`Collation::Vi`]; anything else (`""`, `"-"`, `"zz"`, `"vie"`) gives
    /// [`Collation::Root`].
    pub fn from_tag(tag: &str) -> Collation {
        let t = tag.trim().replace('_', "-").to_lowercase();
        if t == "vi" || t.starts_with("vi-") { Collation::Vi } else { Collation::Root }
    }

    /// The name of the collation: `"root"` or `"vi"`. [`Collation::from_tag`] reads it back.
    pub fn tag(self) -> &'static str {
        match self {
            Collation::Root => "root",
            Collation::Vi => "vi",
        }
    }
}

/// `s` (folded text) with the leading zeros of every run of ASCII digits dropped; a run of
/// zeros only becomes one `0`. Linear.
fn drop_leading_zeros(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut run = false; // inside a run of digits
    let mut zeros_only = false; // ... that has had nothing but zeros so far
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !run {
                (run, zeros_only) = (true, true);
            }
            if zeros_only && c == '0' {
                continue;
            }
            zeros_only = false;
        } else if run {
            if zeros_only {
                out.push('0');
            }
            run = false;
        }
        out.push(c);
    }
    if run && zeros_only {
        out.push('0');
    }
    out
}

/// Push the primary weights of the number whose run of ASCII digits starts with `first` and goes
/// on in `chars` (the run has no leading zero, except the single `0`): `zero` (the weight of the
/// digit `0`, so a number sorts where the digits sort), the count of its digits (`0` for the
/// number zero), then the digits. A longer number is larger; equal lengths compare digit by
/// digit; no other element has the weight `zero`, so the element ends where both runs say. Linear
/// in the run, whatever its length.
fn push_number(first: char, chars: &mut std::iter::Peekable<std::str::Chars<'_>>, zero: u32, out: &mut Vec<u32>) {
    let at = out.len();
    out.push(zero);
    out.push(0); // the count, set below
    let digit = |c: char| c.to_digit(10).unwrap_or(0);
    out.push(digit(first));
    while let Some(c) = chars.next_if(char::is_ascii_digit) {
        out.push(digit(c));
    }
    let digits = out.len().saturating_sub(at.saturating_add(2));
    if digits == 1 && first == '0' {
        out.pop(); // the number zero has no digits
    }
    let count = if digits == 1 && first == '0' { 0 } else { u32::try_from(digits).unwrap_or(u32::MAX) };
    if let Some(slot) = out.get_mut(at.saturating_add(1)) {
        *slot = count;
    }
}

/// The weights `compare` compares, in order.
///
/// - `primary`: one entry per element of the folded text. Root: the code point of a base
///   character (`đ` counts as `d`). Combining marks U+0300..=U+036F are not elements.
/// - `secondary`: for each element, its codes, then a `0` ending the element. Root: the stroke
///   code (`đ`) and the codes of the marks after it (`32 + (cp - 0x300)`, in canonical order). A
///   list per element, not a sum, so different mark sets never collide (`ấ` is `[34, 33]`, `ä`
///   is `[40]`). A mark with no element before it is dropped here and survives in `text`. Vi:
///   see [`Key::vi`].
/// - `text`: the whole [`fold_key`], so only equal text compares equal, whatever the collation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    primary: Vec<u32>,
    secondary: Vec<u16>,
    text: String,
}

impl Key {
    fn new(s: &str, c: Collation) -> Key {
        match c {
            Collation::Root => Key::root(s),
            Collation::Vi => Key::vi(s),
        }
    }

    /// The key of `natural`: the key of `s` in `c`, except that each maximal run of ASCII digits
    /// of the folded text is one element, weighed at the place of `0` and ordered by value
    /// ([`push_number`]), and `text` is the folded text with the leading zeros of every run
    /// dropped (so `a007` and `a7` give equal keys).
    fn natural(s: &str, c: Collation) -> Key {
        let text = drop_leading_zeros(&fold_key(s));
        match c {
            Collation::Root => Key::root_of(text, true),
            Collation::Vi => Key::vi_of(text, true),
        }
    }

    /// The Vietnamese key (rule V of [`Collation::Vi`]): weights from the clusters of the folded
    /// text, each a base letter and the marks after it.
    ///
    /// - V1. A quality mark merges with the base into a letter of its own (`ă â ê ô ơ ư`)
    ///   wherever it stands in the cluster, unless it is blocked: a mark before it that did not
    ///   merge has a combining class >= its own. (Canonical order puts the dot below before the
    ///   circumflex, `ậ` is `a` U+0323 U+0302, so only looking at the first mark would miss it.)
    /// - V2. Letters weigh `(base << 4) + rank`: `a ă â b c d đ e ê … o ô ơ … u ư …` ([`VI`]).
    ///   Letters with a stroke or ligature ([`VI_FOREIGN`]) weigh as their base letters; other
    ///   foreign letters arrive decomposed, so they weigh as their base letter too.
    /// - V3. Each element's secondary codes start with its tone: 0 none, 1 huyền, 2 hỏi, 3 ngã,
    ///   4 sắc, 5 nặng (the first tone mark among the marks that did not merge); then the code of
    ///   a [`VI_FOREIGN`] letter; then the codes of the other marks (`32 + (cp - 0x300)`),
    ///   leaving out every repeat of the tone mark; then the `0` ending the element. Only the
    ///   tone can be `0` and it always comes first, so the flat list orders like a list of lists.
    ///   So tones are a secondary difference (`ma < mà < mả < mã < má < mạ`) and `Hoà` (tone on
    ///   `a`) sorts before `Hòa` (tone on `o`).
    fn vi(s: &str) -> Key {
        Key::vi_of(fold_key(s), false)
    }

    /// [`Key::vi`] of the folded `text`; with `numeric` a run of ASCII digits is one element.
    fn vi_of(text: String, numeric: bool) -> Key {
        let mut primary: Vec<u32> = Vec::new();
        let mut secondary: Vec<u16> = Vec::new();
        let mut chars = text.chars().peekable();
        let mut rest: Vec<char> = Vec::new();
        while primary.len() < MAX_PRIMARY {
            let Some(c) = chars.next() else { break };
            if is_mark(c) {
                continue; // a mark with no letter before it leaves the weights alone
            }
            // V1: merge the quality marks that are not blocked; keep the others in `rest`.
            rest.clear();
            let number = numeric && c.is_ascii_digit();
            if number {
                push_number(c, &mut chars, weight('0', 0), &mut primary);
            }
            let base = vi_merge(c, std::iter::from_fn(|| chars.next_if(|m| is_mark(*m))), |m| rest.push(m));
            let (first, second, letter) = if number { (0, None, 0) } else { vi_letter(base) };
            if !number {
                primary.push(first);
            }
            let tone_mark = rest.iter().copied().find(|m| tone(*m) != 0);
            secondary.push(tone_mark.map_or(0, tone));
            if letter != 0 {
                secondary.push(letter);
            }
            secondary.extend(rest.iter().filter(|m| Some(**m) != tone_mark).map(|m| mark_code(*m)));
            secondary.push(0);
            if let Some(w) = second
                && primary.len() < MAX_PRIMARY
            {
                primary.push(w);
                secondary.extend([0, 0]); // no tone, end of element
            }
        }
        Key { primary, secondary, text }
    }

    /// The Root key.
    fn root(s: &str) -> Key {
        Key::root_of(fold_key(s), false)
    }

    /// [`Key::root`] of the folded `text`; with `numeric` a run of ASCII digits is one element.
    fn root_of(text: String, numeric: bool) -> Key {
        let mut primary: Vec<u32> = Vec::new();
        let mut secondary: Vec<u16> = Vec::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if is_mark(c) {
                if !primary.is_empty() {
                    secondary.push(mark_code(c));
                }
                continue;
            }
            if primary.len() >= MAX_PRIMARY {
                break;
            }
            if !primary.is_empty() {
                secondary.push(0);
            }
            if numeric && c.is_ascii_digit() {
                push_number(c, &mut chars, '0' as u32, &mut primary);
            } else if c == 'đ' {
                primary.push('d' as u32);
                secondary.push(STROKE);
            } else {
                primary.push(c as u32);
            }
        }
        if !primary.is_empty() {
            secondary.push(0);
        }
        Key { primary, secondary, text }
    }
}

impl Ord for Key {
    fn cmp(&self, other: &Key) -> Ordering {
        self.primary.cmp(&other.primary).then_with(|| self.secondary.cmp(&other.secondary)).then_with(|| self.text.cmp(&other.text))
    }
}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Key) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Case-insensitive, encoding-insensitive text order in collation `c`. It compares equal exactly
/// when [`text_eq`] holds, whatever `c` is. Two ASCII strings compare by their lowercase bytes in
/// every collation (no ASCII text holds a Vietnamese letter or mark).
///
/// In [`Collation::Root`] letters compare by their base letter first, so `Ánh` sorts next to
/// `Anh` and not after `z`; accents break ties, ordered by the code point of the mark (`Anh` <
/// `Ánh` < `Ảnh`: acute U+0301 before hook above U+0309), and `đ` counts as `d` first and follows
/// it as the tie-break. Characters outside the Latin tables keep code point order, including the
/// Latin letters that have no decomposition (such as `ø`, `ł`, `æ`, `ß`, `ð`, `ħ`, `ı`), which
/// therefore sort after `z`.
pub fn compare(a: &str, b: &str, c: Collation) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        return a.bytes().map(|x| x.to_ascii_lowercase()).cmp(b.bytes().map(|x| x.to_ascii_lowercase()));
    }
    Key::new(a, c).cmp(&Key::new(b, c))
}

/// A precomputed sort key for text: `text_key(a, c).cmp(&text_key(b, c)) == compare(a, b, c)` for every pair, and two keys of one collation are equal exactly when [`text_eq`] holds.
/// Build it once per string and compare keys (`sort_by_cached_key`, a field of a sorted item)
/// instead of calling [`compare`], which builds both keys on every comparison. Only
/// keys built with the same collation are meant to be compared. Opaque: what it holds may change,
/// only its order is the contract.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextKey(Key);

/// The [`TextKey`] of `s` in collation `c` (the order of [`compare`]).
pub fn text_key(s: &str, c: Collation) -> TextKey {
    TextKey(Key::new(s, c))
}

// ---------------------------------------------------------------------------------------------
// Natural order: numbers by value.
// ---------------------------------------------------------------------------------------------

/// File names in the order people expect: numbers by value (`scan2` before `scan10`), everything
/// else by the collation `c` (case and NFC/NFD do not matter).
///
/// It is [`compare`]'s key with one change: each maximal run of ASCII digits is a single element,
/// weighed at the place of `0` and ordered by value (leading zeros dropped, a longer number is
/// larger, equal lengths digit by digit), so `scan.pdf` < `scan1.pdf` < `scan2.pdf` <
/// `scan10.pdf` and `dir/a/x` < `dir/a1/x`, as a character-by-character natural order gives.
/// The keys are compared like [`text_key`]'s: all the elements first, then the marks. It is a
/// total preorder (a key comparison); strings that differ only in leading zeros, case or
/// encoding compare equal (`a007` = `a7`). Linear, whatever the length of a digit run.
pub fn natural(a: &str, b: &str, c: Collation) -> Ordering {
    Key::natural(a, c).cmp(&Key::natural(b, c))
}

// ---------------------------------------------------------------------------------------------
// Searching: whole letters, ignoring case and encoding.
// ---------------------------------------------------------------------------------------------

/// `s` prepared for searching: [`fold_key`], then the craft apps' search folds.
///
/// [`fold_key`] lowercases one character to one character, decomposes and puts marks in canonical
/// order, so NFC and NFD text give the same result. The folds are the ones the craft apps' search
/// uses: the Latin `I` and `i`, the dotted `İ` (U+0130) and the dotless `ı` (U+0131) all become
/// `i` (Turkish and Azerbaijani text can be found by typing `istanbul` or `ırmak`; search cannot
/// know the language, so the four are one letter), and Greek `Σ`, `σ` and the word-final `ς`
/// become `σ`.
///
/// `İ` decomposes to `I` + U+0307 (and a full lowercase of it gives `i` + U+0307), so the dot of
/// `İ` is dropped in every spelling: after `I`, `i` or `ı`, a U+0307 that is the first mark above
/// (combining class 230) of the letter's marks in canonical order is removed. `İSTANBUL`,
/// `I` + U+0307 + `STANBUL` and `i` + U+0307 + `stanbul` all give `istanbul`. Only that one dot
/// goes: a second U+0307, or one after another mark above (`Í` + U+0307), stays, and other marks
/// keep their letter apart as before (`Í`, `Ï` and `Ì` are not `i`).
pub fn search_fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    // After an `i`, until its first mark above: a U+0307 there is the dot of `İ`.
    let mut dot_pending = false;
    for c in fold_key(s).chars() {
        let class = ccc(c);
        if class == 0 {
            dot_pending = matches!(c, 'i' | '\u{131}');
            out.push(match c {
                '\u{131}' => 'i',
                'ς' => 'σ',
                other => other,
            });
            continue;
        }
        if dot_pending && class == 230 {
            dot_pending = false;
            if c == '\u{307}' {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Whether the char at byte offset `at` of `s` is a combining mark.
fn mark_at(s: &str, at: usize) -> bool {
    s.get(at..).and_then(|t| t.chars().next()).is_some_and(is_mark)
}

/// [`search_contains`] with both sides already folded by [`search_fold`], so a list of texts
/// searched for one needle folds the needle once. Matches whole letters: an occurrence followed
/// by a combining mark U+0300..=U+036F is not a match, so `e` is not found in `Nguyễn` while `ễ`
/// and `nguyễn` are. Every occurrence is tried, overlapping ones too, so a rejected one never
/// hides a later match. A needle that starts with a mark is taken as given. An empty needle is
/// always found. The cost is O(n + k·m) for a haystack of n bytes, a needle of m chars and k
/// occurrences rejected because a mark follows them.
pub fn search_contains_folded(folded_hay: &str, folded_needle: &str) -> bool {
    if folded_needle.is_empty() {
        return true;
    }
    let mut start = 0;
    while let Some(rest) = folded_hay.get(start..) {
        let Some(at) = rest.find(folded_needle) else { return false };
        let at = start.saturating_add(at);
        if !mark_at(folded_hay, at.saturating_add(folded_needle.len())) {
            return true;
        }
        // Resume one character after this occurrence's start.
        start = at.saturating_add(folded_hay.get(at..).and_then(|t| t.chars().next()).map_or(1, char::len_utf8));
    }
    false
}

/// Whether `hay` contains `needle` as text: case, NFC versus NFD and the Turkish `i` and Greek
/// sigma variants are ignored ([`search_fold`]), and only whole letters match
/// ([`search_contains_folded`]). An empty needle is always found.
pub fn search_contains(hay: &str, needle: &str) -> bool {
    search_contains_folded(&search_fold(hay), &search_fold(needle))
}

#[cfg(test)]
mod tables {
    use super::*;
    use icu_normalizer::DecomposingNormalizerBorrowed;
    use icu_normalizer::properties::CanonicalCombiningClassMapBorrowed;

    /// The scalar ranges the table covers: Latin-1 Supplement letters to Latin Extended-B, the
    /// combining marks, and Latin Extended Additional.
    fn covered() -> impl Iterator<Item = char> {
        ('\u{c0}'..='\u{24f}').chain('\u{300}'..='\u{36f}').chain('\u{1e00}'..='\u{1eff}')
    }

    /// The marks of U+0300..=U+036F that have a canonical decomposition of their own.
    const MARKS_THAT_DECOMPOSE: [char; 4] = ['\u{340}', '\u{341}', '\u{343}', '\u{344}'];

    #[test]
    fn tables_match_icu_normalizer() {
        let nfd = DecomposingNormalizerBorrowed::new_nfd();
        for c in covered() {
            let s = c.to_string();
            assert_eq!(canonical(&s), nfd.normalize(&s), "U+{:04X}", c as u32);
        }
        let map = CanonicalCombiningClassMapBorrowed::new();
        for c in '\u{300}'..='\u{36f}' {
            assert_eq!(ccc(c), map.get_u8(c), "ccc U+{:04X}", c as u32);
        }
        assert_eq!(ccc('a'), 0);
        assert_eq!(ccc('\u{10ffff}'), 0);
        // The marks that decompose, after a letter and next to marks of other classes, in either
        // order: decomposed, then ordered with their neighbours.
        for m in MARKS_THAT_DECOMPOSE {
            for base in ["", "a", "U", "\u{1b0}"] {
                for other in ["", "\u{323}", "\u{31b}", "\u{302}", "\u{345}"] {
                    for s in [format!("{base}{m}{other}"), format!("{base}{other}{m}")] {
                        assert_eq!(canonical(&s), nfd.normalize(&s), "{s:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn marks_that_decompose_equal_their_decomposition() {
        // U+0340 and U+0341 (the grave and acute tone marks) are U+0300 and U+0301; U+0343 is
        // U+0313; U+0344 is U+0308 U+0301.
        assert_eq!(canonical("\u{340}\u{341}\u{343}"), "\u{300}\u{301}\u{313}");
        assert_eq!(canonical("\u{344}"), "\u{308}\u{301}");
        assert_eq!(canonical("a\u{344}\u{323}"), "a\u{323}\u{308}\u{301}");
        assert_eq!(fold_key("\u{341}"), "\u{301}");
        assert!(text_eq("a\u{341}", "á"));
        assert!(text_eq("a\u{340}", "à"));
        assert!(text_eq("U\u{344}", "ǘ"));
        assert!(text_eq("Ma\u{341}", "MÁ"));
        assert!(search_contains("Ma\u{341} Hoa\u{340}", "má hoà"));
        assert!(search_contains("Má Hoà", "ma\u{341}"));
        assert_eq!(natural("scan2 ma\u{341}", "scan02 má", Collation::Root), Ordering::Equal);
        for c in [Collation::Root, Collation::Vi] {
            for (single, usual) in
                [("ma\u{341}", "má"), ("ma\u{340}", "mà"), ("Ă\u{341}", "Ắ"), ("Hoa\u{340}", "Hoà"), ("a\u{344}", "a\u{308}\u{301}")]
            {
                assert_eq!(compare(single, usual, c), Ordering::Equal, "{single:?} {c:?}");
                assert_eq!(text_key(single, c), text_key(usual, c), "{single:?} {c:?}");
            }
        }
    }

    #[test]
    fn vi_tone_marks_typed_as_their_singletons_are_the_tones() {
        // U+0340 and U+0341 decompose to U+0300 (huyền) and U+0301 (sắc).
        assert_eq!(Key::vi("ma\u{340}").secondary.get(2), Some(&1));
        assert_eq!(Key::vi("ma\u{341}").secondary.get(2), Some(&4));
        // They sort exactly where `mà` and `má` do.
        let row = ["ma", "ma\u{340}", "mả", "mã", "ma\u{341}", "mạ"];
        let mut v = row;
        v.reverse();
        v.sort_by(|a, b| compare(a, b, Collation::Vi));
        assert_eq!(v, row);
    }

    #[test]
    fn fold_key_gives_nfc_and_nfd_the_same_key_for_every_covered_letter() {
        let nfd = DecomposingNormalizerBorrowed::new_nfd();
        for c in covered().chain(['\u{212a}', '\u{212b}']) {
            let s = c.to_string();
            let d = nfd.normalize(&s);
            assert_eq!(fold_key(&s), fold_key(&d), "U+{:04X}", c as u32);
            assert_eq!(compare(&s, &d, Collation::Root), Ordering::Equal, "U+{:04X}", c as u32);
            let upper: String = s.chars().flat_map(char::to_uppercase).collect();
            if upper.chars().count() == 1 && upper.chars().all(|u| lower_char(u) == c) {
                assert_eq!(fold_key(&upper), fold_key(&s), "case U+{:04X}", c as u32);
            }
        }
    }

    #[test]
    fn reorders_marks_by_combining_class_stably() {
        assert_eq!(canonical("a\u{302}\u{323}"), "a\u{323}\u{302}"); // ậ typed in the wrong order
        assert_eq!(canonical("ậ"), "a\u{323}\u{302}");
        assert_eq!(canonical("o\u{323}\u{31b}"), "o\u{31b}\u{323}"); // ợ: horn (216) before dot below (220)
        assert_eq!(canonical("ợ"), "o\u{31b}\u{323}");
        assert_eq!(canonical("e\u{303}\u{302}"), "e\u{303}\u{302}"); // same class 230: order kept
        assert_ne!(canonical("e\u{303}\u{302}"), canonical("ễ"));
        assert_eq!(canonical("Nguyễn"), canonical("Nguye\u{302}\u{303}n"));
        assert_eq!(canonical("ABC xyz"), "ABC xyz");
        assert_eq!(canonical("Ấ"), "A\u{302}\u{301}"); // case kept
    }

    #[test]
    fn hostile_marks_never_panic_and_long_clusters_stay_put() {
        // 10 000 alternating marks of different classes: reordered or not, never a panic.
        let long: String = "a".to_string() + &"\u{323}\u{302}".repeat(5_000);
        let _ = canonical(&long);
        // A leading mark, the last scalar, and the dotted/capital specials.
        assert_eq!(canonical("\u{323}\u{302}"), "\u{323}\u{302}");
        assert_eq!(canonical("\u{302}\u{323}"), "\u{323}\u{302}");
        assert_eq!(canonical("\u{10FFFF}"), "\u{10FFFF}");
        assert_eq!(canonical("İ"), "I\u{307}");
        assert_eq!(canonical("ẞ"), "ẞ");
        // A megabyte of text.
        let big = "Nguyễn Văn ".repeat(100_000);
        assert_eq!(canonical(&big), "Nguye\u{302}\u{303}n Va\u{306}n ".repeat(100_000));
        // A cluster of 33 marks is longer than the cap: returned in input order.
        let marks: String = "\u{302}\u{323}".repeat(16) + "\u{302}";
        assert_eq!(marks.chars().count(), 33);
        let input = format!("a{marks}");
        assert_eq!(canonical(&input), input);
        // 32 marks are still reordered.
        let marks32: String = "\u{302}\u{323}".repeat(16);
        let out = canonical(&format!("a{marks32}"));
        assert_eq!(out, format!("a{}{}", "\u{323}".repeat(16), "\u{302}".repeat(16)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Root order.
    fn compare_text(a: &str, b: &str) -> Ordering {
        compare(a, b, Collation::Root)
    }

    /// Whole-letter containment of the plain [`fold_key`]s (no Turkish or sigma mapping).
    fn contains_folded(h: &str, n: &str) -> bool {
        search_contains_folded(&fold_key(h), &fold_key(n))
    }

    /// Small deterministic generator (xorshift64*): the repo has no proptest.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
    }

    /// A plain case-insensitive char comparison, the oracle for ASCII text.
    fn old_compare_text(a: &str, b: &str) -> Ordering {
        let mut ai = a.chars().flat_map(char::to_lowercase);
        let mut bi = b.chars().flat_map(char::to_lowercase);
        loop {
            match (ai.next(), bi.next()) {
                (None, None) => return Ordering::Equal,
                (None, Some(_)) => return Ordering::Less,
                (Some(_), None) => return Ordering::Greater,
                (Some(x), Some(y)) if x != y => return x.cmp(&y),
                _ => {}
            }
        }
    }

    /// Pairs of equal text: a precomposed letter versus its base letter + marks.
    const CANONICAL_EQUAL: [(&str, &str); 8] = [
        ("ậ", "a\u{302}\u{323}"),
        ("ặ", "a\u{306}\u{323}"),
        ("ệ", "e\u{302}\u{323}"),
        ("ộ", "o\u{302}\u{323}"),
        ("ợ", "o\u{323}\u{31b}"),
        ("ự", "u\u{323}\u{31b}"),
        ("Nguyễn", "Nguye\u{302}\u{303}n"),
        ("Ánh", "A\u{301}nh"),
    ];

    fn sorted(words: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = words.iter().map(|w| (*w).to_string()).collect();
        v.sort_by(|a, b| compare_text(a, b));
        v
    }

    #[test]
    fn nfc_and_nfd_compare_equal() {
        for (a, b) in CANONICAL_EQUAL {
            assert_eq!(compare_text(a, b), Ordering::Equal, "{a:?} {b:?}");
            assert_eq!(compare_text(b, a), Ordering::Equal, "{b:?} {a:?}");
            assert!(text_eq(a, b), "{a:?} {b:?}");
            assert_eq!(fold_key(a), fold_key(b), "{a:?} {b:?}");
            assert_eq!(canonical(a), canonical(b), "{a:?} {b:?}");
        }
        // The same text typed with the marks in the other order.
        assert!(text_eq("a\u{302}\u{323}", "a\u{323}\u{302}"));
        // Case and encoding together.
        assert!(text_eq("NGUYỄN", "nguye\u{302}\u{303}n"));
        assert!(text_eq("Đà Nẵng", "đa\u{300} NA\u{306}\u{303}NG"));
        // canonical_not_equal: tilde and circumflex share a combining class, so their order counts.
        assert_ne!(compare_text("ễ", "e\u{303}\u{302}"), Ordering::Equal);
        assert!(!text_eq("ễ", "e\u{303}\u{302}"));
        assert_ne!(fold_key("ễ"), fold_key("e\u{303}\u{302}"));
        // Characters whose lowercase is, or decomposes to, a letter with marks.
        assert!(text_eq("\u{212b}", "a\u{30a}")); // Angstrom sign
        assert!(text_eq("\u{212b}", "\u{c5}"));
        assert!(text_eq("\u{130}", "I\u{307}")); // I with dot above keeps its dot in both forms
        assert!(!text_eq("\u{130}", "i"));
    }

    #[test]
    fn accented_letters_sort_next_to_their_base() {
        assert_eq!(sorted(&["Zebra", "ấm", "bàn", "Ánh", "Anh", "Ấn"]), ["ấm", "Ấn", "Anh", "Ánh", "bàn", "Zebra"]);
        assert_eq!(compare_text("ấ", "ä"), Ordering::Less); // different marks stay different
        assert_eq!(compare_text("ä", "ấ"), Ordering::Greater);
        // Accents break ties between the same base letters; the unaccented letter comes first.
        // (The Root order ranks marks by code point: acute U+0301 before hook above U+0309.)
        assert_eq!(sorted(&["Ảnh", "Ánh", "Anh"]), ["Anh", "Ánh", "Ảnh"]);
        assert_eq!(sorted(&["Huỳnh", "Hoàng", "Hồ"]), ["Hồ", "Hoàng", "Huỳnh"]);
        // đ sorts with d, after it when the rest is equal: its primary is the d.
        assert_eq!(compare_text("Đặng", "Em"), Ordering::Less);
        assert_eq!(compare_text("Đặng", "Dung"), Ordering::Less); // a < u at the primary level
        assert_eq!(compare_text("Dũng", "Đào"), Ordering::Greater); // u > a
        assert_eq!(compare_text("đ", "d"), Ordering::Greater);
        assert_eq!(compare_text("đ", "e"), Ordering::Less);
        assert_eq!(compare_text("Đ", "đ"), Ordering::Equal);
        // These 23 strings differ pairwise, and the
        // weights alone (not the text tie-break) tell them apart.
        let words = ["a", "à", "ả", "ã", "á", "ạ", "ă", "ằ", "ẳ", "ẵ", "ắ", "ặ", "â", "ầ", "ẩ", "ẫ", "ấ", "ậ", "ä", "å", "ā", "ą", "ǎ"];
        assert_eq!(words.len(), 23);
        let weights: std::collections::HashSet<(Vec<u32>, Vec<u16>)> = words
            .iter()
            .map(|w| {
                let k = Key::root(w);
                (k.primary, k.secondary)
            })
            .collect();
        assert_eq!(weights.len(), words.len());
        for (i, a) in words.iter().enumerate() {
            for b in &words[i + 1..] {
                assert_ne!(compare_text(a, b), Ordering::Equal, "{a} {b}");
            }
            // Each still sorts with `a`: between `a` and `b`.
            assert_ne!(compare_text(a, "a"), Ordering::Less, "{a}");
            assert_eq!(compare_text(a, "b"), Ordering::Less, "{a}");
        }
    }

    #[test]
    fn ascii_order_is_unchanged() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let alphabet: Vec<char> = " -_.,'0123456789abcXYZ~[]ABCdefgzZ".chars().collect();
        let mut words: Vec<String> = Vec::new();
        for _ in 0..200 {
            let len = rng.below(8);
            words.push((0..len).map(|_| alphabet[rng.below(alphabet.len())]).collect());
        }
        let mut new = words.clone();
        let mut old = words.clone();
        new.sort_by(|a, b| compare_text(a, b));
        old.sort_by(|a, b| old_compare_text(a, b));
        assert_eq!(new, old);
        for a in &words {
            for b in words.iter().take(40) {
                assert_eq!(compare_text(a, b), old_compare_text(a, b), "{a:?} {b:?}");
                assert_eq!(text_eq(a, b), old_compare_text(a, b) == Ordering::Equal, "{a:?} {b:?}");
            }
        }
    }

    /// A string from the pieces the Root key has to get right.
    fn random_text(rng: &mut Rng) -> String {
        const PIECES: [&str; 40] = [
            "a",
            "A",
            "b",
            "n",
            "g",
            "d",
            "Đ",
            "đ",
            "e",
            "o",
            "u",
            "y",
            "Z",
            " ",
            "-",
            "1",
            "ấ",
            "Ấ",
            "ậ",
            "ặ",
            "ễ",
            "ợ",
            "ự",
            "Ánh",
            "Nguyễn",
            "ä",
            "å",
            "ø",
            "İ",
            "ß",
            "ẞ",
            "\u{212b}",
            "\u{10ffff}",
            "й",
            "Ω",
            "z",
            "\u{302}",
            "\u{323}",
            "\u{301}",
            "\u{31b}",
        ];
        let mut s = String::new();
        for _ in 0..rng.below(6) {
            match rng.below(8) {
                0 => {
                    // a loose mark anywhere in U+0300..=U+036F
                    s.push(char::from_u32(0x300 + rng.below(0x70) as u32).unwrap_or('a'));
                }
                1 => {
                    // any scalar
                    s.push(char::from_u32(rng.below(0x3000) as u32).unwrap_or('a'));
                }
                _ => s.push_str(PIECES[rng.below(PIECES.len())]),
            }
        }
        s
    }

    /// `s` re-encoded: decomposed, with some letters upper-cased where that keeps the same
    /// lowercase (so `ß` stays `ß`: its upper case `SS` is another text).
    fn reencode(rng: &mut Rng, s: &str) -> String {
        let d = canonical(s);
        d.chars()
            .map(|c| {
                let mut up = c.to_uppercase();
                match (up.next(), up.next()) {
                    (Some(u), None) if rng.below(2) == 0 && lower_char(u) == lower_char(c) => u,
                    _ => c,
                }
            })
            .collect()
    }

    #[test]
    fn total_order_and_equality_invariants() {
        let mut rng = Rng(0x0123_4567_89ab_cdef);
        for _ in 0..2_000 {
            let a = random_text(&mut rng);
            let b = if rng.below(3) == 0 { reencode(&mut rng, &a) } else { random_text(&mut rng) };
            let c = if rng.below(3) == 0 { reencode(&mut rng, &b) } else { random_text(&mut rng) };
            let (ab, ba) = (compare_text(&a, &b), compare_text(&b, &a));
            assert_eq!(ab, ba.reverse(), "antisymmetry {a:?} {b:?}");
            assert_eq!(compare_text(&a, &a), Ordering::Equal, "reflexive {a:?}");
            let (bc, ac) = (compare_text(&b, &c), compare_text(&a, &c));
            if ab != Ordering::Greater && bc != Ordering::Greater {
                assert_ne!(ac, Ordering::Greater, "transitivity {a:?} {b:?} {c:?}");
            }
            if ab == Ordering::Less && bc == Ordering::Less {
                assert_eq!(ac, Ordering::Less, "transitivity {a:?} {b:?} {c:?}");
            }
            if ab == Ordering::Equal && bc == Ordering::Equal {
                assert_eq!(ac, Ordering::Equal, "equality transitivity {a:?} {b:?} {c:?}");
            }
            for (x, y, o) in [(&a, &b, ab), (&b, &c, bc), (&a, &c, ac)] {
                let eq = o == Ordering::Equal;
                assert_eq!(eq, text_eq(x, y), "text_eq {x:?} {y:?}");
                assert_eq!(eq, fold_key(x) == fold_key(y), "fold_key {x:?} {y:?}");
            }
        }
    }

    #[test]
    fn re_encoding_never_changes_the_order() {
        let mut rng = Rng(0xdead_beef_cafe_f00d);
        for _ in 0..1_000 {
            let a = random_text(&mut rng);
            let b = random_text(&mut rng);
            let (ra, rb) = (reencode(&mut rng, &a), reencode(&mut rng, &b));
            assert_eq!(compare_text(&a, &b), compare_text(&ra, &rb), "{a:?} {b:?}");
            assert_eq!(compare_text(&a, &ra), Ordering::Equal, "{a:?} {ra:?}");
            assert_eq!(fold_key(&fold_key(&a)), fold_key(&a), "idempotent {a:?}");
        }
    }

    #[test]
    fn hostile_text_never_panics() {
        let marks = "a".to_string() + &"\u{323}\u{302}".repeat(5_000);
        let leading = "\u{301}\u{323}\u{302}a".to_string();
        let big = "Nguyễn Văn ".repeat(100_000);
        assert!(big.len() >= 1_000_000);
        let cases: [&str; 11] =
            ["", "\u{301}", "a\u{301}\u{301}\u{301}", "\u{10FFFF}", "İ", "ẞ", "\u{212b}", &marks, &leading, &big, "đ\u{323}\u{302}"];
        for a in cases {
            let _ = fold_key(a);
            let _ = canonical(a);
            for b in cases {
                let o = compare_text(a, b);
                assert_eq!(o, compare_text(b, a).reverse());
                assert_eq!(o == Ordering::Equal, text_eq(a, b));
            }
        }
        // The key holds at most 16 384 elements; the rest takes part through the text.
        let k = Key::root(&big);
        assert_eq!(k.primary.len(), MAX_PRIMARY);
        assert!(k.text.len() >= 1_000_000, "the text keeps the whole string");
        let almost = format!("{big}x");
        assert_ne!(compare_text(&big, &almost), Ordering::Equal);
        // A mark with nothing before it is dropped from the weights and kept in the text.
        let k = Key::root("\u{301}a");
        assert_eq!((k.primary, k.secondary), (Key::root("a").primary, Key::root("a").secondary));
        assert_ne!(compare_text("\u{301}a", "a"), Ordering::Equal);
        // A run of marks longer than the cap keeps its input order, so it is not canonical-equal
        // to the other order; it must still compare consistently.
        let m33 = format!("a{}\u{302}", "\u{302}\u{323}".repeat(16));
        let m33b = format!("a\u{323}{}", "\u{302}\u{323}".repeat(16));
        assert_eq!(compare_text(&m33, &m33b), compare_text(&m33b, &m33).reverse());
    }

    #[test]
    fn contains_folded_matches_whole_letters() {
        // A base letter or a letter with fewer marks never matches inside a marked letter.
        assert!(!contains_folded("Nguyễn", "e"));
        assert!(contains_folded("Nguyễn", "ễ"));
        assert!(contains_folded("Nguye\u{302}\u{303}n", "nguyễn"));
        assert!(contains_folded("NGUYE\u{302}\u{303}N", "Nguyễn"));
        assert!(contains_folded("Nguyễn", "NGUYE\u{302}\u{303}N"));
        assert!(!contains_folded("Ánh", "a"));
        assert!(contains_folded("Ánh", "á"));
        assert!(contains_folded("A\u{301}nh", "Á"));
        assert!(!contains_folded("ế", "é"));
        assert!(!contains_folded("ễ", "ê"));
        assert!(contains_folded("Hà Nội và Hà Giang", "giang"));
        assert!(contains_folded("ậ", "a\u{302}\u{323}")); // marks typed in the other order
        assert!(contains_folded("abc", ""));
        assert!(contains_folded("", ""));
        assert!(!contains_folded("", "a"));
        // A rejected occurrence does not hide a later whole-letter one.
        assert!(contains_folded("ễ e", "e"));
        assert!(contains_folded("Ánh Anh", "anh"));
        // A needle that starts with a mark is taken as given.
        assert!(contains_folded("ễ", "\u{303}"));
        assert!(!contains_folded("abc", "abcd"));
        // An overlapping later occurrence is found after a rejected one.
        assert!(contains_folded("e\u{302}e\u{302}e", "e\u{302}e"));
        // ASCII is plain case-insensitive substring search.
        assert!(contains_folded("Hello World", "O W"));
        assert!(!contains_folded("Hello", "xyz"));
    }

    #[test]
    fn folded_search_is_encoding_blind_and_never_panics() {
        let marks = "a".to_string() + &"\u{323}\u{302}".repeat(5_000);
        let big = "Nguyễn Văn ".repeat(100_000);
        let cases: [&str; 10] = ["", "\u{301}", "a\u{301}\u{301}", "\u{10FFFF}", "İ", "ẞ", "ss", &marks, &big, "e\u{302}"];
        for h in cases {
            for n in cases {
                let _ = (contains_folded(h, n), search_contains(h, n));
            }
        }
        assert!(contains_folded(&big, "VĂN NGUYỄN"));
        assert!(!contains_folded(&big, "van"));
        assert!(!contains_folded(&marks, "\u{323}\u{302}a"));
        // NFC and NFD haystacks and needles give the same answers.
        let cases = [
            ("Nguyễn", "ễ"),
            ("Nguyễn", "e"),
            ("Ánh", "á"),
            ("Đà Nẵng", "nẵng"),
            ("Đà Nẵng", "nang"),
            ("İSTANBUL", "istanbul"),
            ("istanbul", "İSTANBUL"),
            ("Íslenska", "islenska"),
        ];
        for (h, n) in cases {
            let (hd, nd) = (canonical(h), canonical(n));
            for (a, b) in [(h, n), (hd.as_str(), n), (h, nd.as_str()), (hd.as_str(), nd.as_str())] {
                assert_eq!(contains_folded(a, b), contains_folded(h, n), "{a:?} {b:?}");
                assert_eq!(search_contains(a, b), search_contains(h, n), "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn canonical_equivalence_covers_the_latin_tables_only() {
        // What the crate and app docs promise: precomposed Latin letters equal their NFD spelling,
        // precomposed letters of other scripts do not (they are not in the tables).
        assert!(text_eq("Ánh", "A\u{301}nh"));
        for (nfc, nfd) in [("й", "и\u{306}"), ("ά", "α\u{301}"), ("Й", "И\u{306}")] {
            assert!(!text_eq(nfc, nfd), "{nfc:?}");
            assert_ne!(fold_key(nfc), fold_key(nfd), "{nfc:?}");
            assert!(!search_contains(nfc, nfd), "{nfc:?}");
        }
    }

    #[test]
    fn search_fold_takes_the_dot_of_i_in_nfc_and_nfd_alike() {
        // `İ` (U+0130) is `I` + U+0307 in NFD, and a full lowercase of it is `i` + U+0307.
        for s in ["İSTANBUL", "I\u{307}STANBUL", "i\u{307}stanbul", "ı\u{307}stanbul", "istanbul", "ISTANBUL", "ıstanbul"] {
            assert_eq!(search_fold(s), "istanbul", "{s:?}");
            assert!(search_contains(s, "istanbul"), "{s:?}");
            assert!(search_contains("Đi İstanbul", &s.to_lowercase()), "{s:?}");
            assert!(search_contains("Đi I\u{307}stanbul", s), "{s:?}");
        }
        // One dot belongs to the `i`; a second one is a mark like any other, in either encoding.
        assert_eq!(search_fold("İ\u{307}"), "i\u{307}");
        assert_eq!(search_fold("I\u{307}\u{307}"), "i\u{307}");
        // The dot is the `i`'s only as its first mark above (class 230): after an acute it is not.
        assert_eq!(search_fold("I\u{301}\u{307}"), "i\u{301}\u{307}");
        assert_eq!(search_fold("İ\u{301}"), "i\u{301}");
        // Marks of other classes may come first in any order: these four are canonically equivalent.
        for s in ["İ\u{323}", "Ị\u{307}", "I\u{323}\u{307}", "I\u{307}\u{323}"] {
            assert_eq!(search_fold(s), "i\u{323}", "{s:?}");
        }
        // Other marks on I stay letters of their own, as before: `í` and `ï` are not `i`.
        for (s, folded) in [("Í", "i\u{301}"), ("I\u{301}", "i\u{301}"), ("Ï", "i\u{308}"), ("ï", "i\u{308}"), ("Ì", "i\u{300}")] {
            assert_eq!(search_fold(s), folded, "{s:?}");
            assert!(!search_contains(s, "i"), "{s:?}");
        }
        assert!(!search_contains("Íslenska", "islenska"));
        assert!(search_contains("ÍSLENSKA", "i\u{301}slenska"));
        // Sigma folds as before.
        assert_eq!(search_fold("ΣΟΦΟΣ"), "σοφοσ");
        assert_eq!(search_fold("σοφος"), "σοφοσ");
        // Total on hostile input: dots and marks without a base, long runs.
        let dots = "I".to_string() + &"\u{307}".repeat(10_000);
        for s in ["\u{307}", "\u{307}I", "ı", "İ", &dots, "\u{10FFFF}\u{307}"] {
            let _ = (search_fold(s), search_contains(s, s), search_contains(s, "i"));
        }
    }

    #[test]
    fn text_key_orders_exactly_like_compare_text() {
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        let alphabet: Vec<char> = " -_.,'0123456789abcXYZ~[]ABCdefgzZ".chars().collect();
        let ascii = |rng: &mut Rng| -> String { (0..rng.below(8)).map(|_| alphabet[rng.below(alphabet.len())]).collect() };
        for i in 0..20_000 {
            // ASCII against ASCII (compare_text's fast path), mixed, and fully random text.
            let (a, b) = match i % 3 {
                0 => (ascii(&mut rng), ascii(&mut rng)),
                1 => (ascii(&mut rng), random_text(&mut rng)),
                _ => {
                    let a = random_text(&mut rng);
                    let b = if rng.below(4) == 0 { reencode(&mut rng, &a) } else { random_text(&mut rng) };
                    (a, b)
                }
            };
            let (ka, kb) = (text_key(&a, Collation::Root), text_key(&b, Collation::Root));
            assert_eq!(ka.cmp(&kb), compare_text(&a, &b), "{a:?} {b:?}");
            assert_eq!(ka == kb, text_eq(&a, &b), "{a:?} {b:?}");
        }
        // Case and encoding give equal keys; the hostile strings still order consistently.
        assert_eq!(text_key("NGUYỄN", Collation::Root), text_key("nguye\u{302}\u{303}n", Collation::Root));
        let big = "Nguyễn Văn ".repeat(100_000);
        let cases = ["", "\u{301}", "\u{10FFFF}", "İ", "ẞ", "Zebra", "ấm", &big];
        for a in cases {
            for b in cases {
                assert_eq!(text_key(a, Collation::Root).cmp(&text_key(b, Collation::Root)), compare_text(a, b), "{a:.20?} {b:.20?}");
            }
        }
        let mut words = vec!["Zebra", "ấm", "bàn", "Ánh", "Anh", "Ấn"];
        words.sort_by_cached_key(|w| text_key(w, Collation::Root));
        assert_eq!(words, ["ấm", "Ấn", "Anh", "Ánh", "bàn", "Zebra"]);
    }

    #[test]
    fn text_key_orders_exactly_like_compare_vi() {
        let mut rng = Rng(0x7f4a_7c15_9e37_79b9);
        let alphabet: Vec<char> = " -_.,'0123456789abcXYZ~[]ABCdefgzZ".chars().collect();
        let ascii = |rng: &mut Rng| -> String { (0..rng.below(8)).map(|_| alphabet[rng.below(alphabet.len())]).collect() };
        for i in 0..20_000 {
            let (a, b) = match i % 3 {
                0 => (ascii(&mut rng), ascii(&mut rng)),
                1 => (ascii(&mut rng), random_text(&mut rng)),
                _ => {
                    let a = random_text(&mut rng);
                    let b = if rng.below(4) == 0 { reencode(&mut rng, &a) } else { random_text(&mut rng) };
                    (a, b)
                }
            };
            let o = compare(&a, &b, Collation::Vi);
            let (ka, kb) = (text_key(&a, Collation::Vi), text_key(&b, Collation::Vi));
            assert_eq!(ka.cmp(&kb), o, "{a:?} {b:?}");
            assert_eq!(ka == kb, text_eq(&a, &b), "{a:?} {b:?}");
            // Equality never depends on the collation.
            assert_eq!(o == Ordering::Equal, compare_text(&a, &b) == Ordering::Equal, "{a:?} {b:?}");
        }
        let mut words = vec!["Zebra", "ấm", "bàn", "Ánh", "Anh", "Ấn", "ăn", "Đào", "Dương"];
        words.sort_by_cached_key(|w| text_key(w, Collation::Vi));
        assert_eq!(words, ["Anh", "Ánh", "ăn", "ấm", "Ấn", "bàn", "Dương", "Đào", "Zebra"]);
    }

    #[test]
    fn vi_total_order_and_equality_invariants() {
        let mut rng = Rng(0x0fed_cba9_8765_4321);
        let vi = |a: &str, b: &str| compare(a, b, Collation::Vi);
        for _ in 0..2_000 {
            let a = random_text(&mut rng);
            let b = if rng.below(3) == 0 { reencode(&mut rng, &a) } else { random_text(&mut rng) };
            let c = if rng.below(3) == 0 { reencode(&mut rng, &b) } else { random_text(&mut rng) };
            let (ab, bc, ac) = (vi(&a, &b), vi(&b, &c), vi(&a, &c));
            assert_eq!(ab, vi(&b, &a).reverse(), "antisymmetry {a:?} {b:?}");
            assert_eq!(vi(&a, &a), Ordering::Equal, "reflexive {a:?}");
            if ab != Ordering::Greater && bc != Ordering::Greater {
                assert_ne!(ac, Ordering::Greater, "transitivity {a:?} {b:?} {c:?}");
            }
            for (x, y, o) in [(&a, &b, ab), (&b, &c, bc), (&a, &c, ac)] {
                assert_eq!(o == Ordering::Equal, text_eq(x, y), "text_eq {x:?} {y:?}");
            }
            // Re-encoding (case, NFC/NFD, mark order) never changes the order.
            let (ra, rb) = (reencode(&mut rng, &a), reencode(&mut rng, &b));
            assert_eq!(vi(&ra, &rb), ab, "{a:?} {b:?}");
        }
    }

    #[test]
    fn vi_key_structure() {
        // One terminated secondary list per primary weight, each starting with the tone.
        for s in ["Nguyễn Thị Ánh", "ậ", "Đắk Lắk", "ßæ", "\u{301}a\u{301}\u{301}", "", "ơ\u{323}\u{31b}"] {
            let k = Key::vi(s);
            // Parse `[tone, codes…, 0]` per element: the tone is 0..=5, the codes are not 0.
            let mut it = k.secondary.iter().copied();
            let mut elements = 0;
            while let Some(t) = it.next() {
                assert!(t <= 5, "{s:?}");
                assert!(it.by_ref().any(|x| x == 0), "{s:?}: unterminated");
                elements += 1;
            }
            assert_eq!(elements, k.primary.len(), "{s:?}");
        }
        // Tones are the first code of an element: ma < mà < mả < mã < má < mạ.
        let tones: Vec<u16> = ["ma", "mà", "mả", "mã", "má", "mạ"].iter().map(|w| Key::vi(w).secondary.get(2).copied().unwrap_or(99)).collect();
        assert_eq!(tones, [0, 1, 2, 3, 4, 5]);
        // A quality mark merges wherever it stands unless blocked: ậ typed in either order is
        // â + nặng; e + tilde + circumflex (same class) stays e with two marks, not ê.
        assert_eq!(Key::vi("ậ").primary, Key::vi("â").primary);
        assert_eq!(Key::vi("a\u{302}\u{323}"), Key::vi("ậ"));
        assert_eq!(Key::vi("ễ").primary, Key::vi("ê").primary);
        assert_eq!(Key::vi("e\u{303}\u{302}").primary, Key::vi("e").primary);
        assert_ne!(compare("ễ", "e\u{303}\u{302}", Collation::Vi), Ordering::Equal);
        // Every repeat of the tone mark is left out of the other codes; other marks stay.
        assert_eq!(Key::vi("a\u{301}\u{301}").secondary, [4, 0]);
        assert_eq!(Key::vi("a\u{301}\u{300}").secondary, [4, mark_code('\u{300}'), 0]);
        // A foreign letter weighs as its base letter(s), told apart by its code.
        assert_eq!(Key::vi("ø").primary, Key::vi("o").primary);
        assert_eq!(Key::vi("ø").secondary, [0, 11, 0]);
        assert_eq!(Key::vi("ß").primary, Key::vi("ss").primary);
        assert_eq!(Key::vi("ß").secondary, [0, 16, 0, 0, 0]);
        assert_eq!(compare("ss", "ß", Collation::Vi), Ordering::Less);
        assert_eq!(compare("ß", "st", Collation::Vi), Ordering::Less);
        // These 23 strings differ pairwise: the weights alone tell them
        // apart in `vi` too.
        let words = ["a", "à", "ả", "ã", "á", "ạ", "ă", "ằ", "ẳ", "ẵ", "ắ", "ặ", "â", "ầ", "ẩ", "ẫ", "ấ", "ậ", "ä", "å", "ā", "ą", "ǎ"];
        let weights: std::collections::HashSet<(Vec<u32>, Vec<u16>)> = words
            .iter()
            .map(|w| {
                let k = Key::vi(w);
                (k.primary, k.secondary)
            })
            .collect();
        assert_eq!(weights.len(), words.len());
    }

    #[test]
    fn vi_tables_name_whole_lowercase_letters() {
        // fold_key leaves every letter the tables name whole and lowercase, so a key can find it.
        for (x, _, _) in VI {
            assert_eq!(fold_key(&x.to_string()).chars().count(), if *x == 'đ' { 1 } else { 2 }, "{x}");
        }
        for (_, _, merged) in QUALITY {
            assert!(VI.iter().any(|(x, _, _)| x == merged), "{merged}");
            assert_eq!(fold_key(&merged.to_string()), canonical(&merged.to_string()));
        }
        for (x, bases, code) in VI_FOREIGN {
            assert_eq!(fold_key(&x.to_string()), x.to_string(), "{x}");
            assert!(!bases.is_empty() && bases.chars().count() <= 2 && *code > 0 && *code < 32, "{x}");
        }
        for (m, w) in TONES {
            assert!(is_mark(*m) && (1..=5).contains(w));
        }
    }

    #[test]
    fn vi_key_caps_long_text() {
        let big = "Nguyễn Văn ".repeat(100_000);
        let k = Key::vi(&big);
        assert_eq!(k.primary.len(), MAX_PRIMARY);
        assert!(k.text.len() >= 1_000_000, "the text keeps the whole string");
        // Ligatures at the cap do not go past it.
        let lig = "ß".repeat(MAX_PRIMARY);
        assert_eq!(Key::vi(&lig).primary.len(), MAX_PRIMARY);
        let marks = "a".to_string() + &"\u{323}\u{302}".repeat(5_000);
        let k = Key::vi(&marks);
        assert_eq!(k.primary.len(), 1);
    }
}
