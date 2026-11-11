//! Caption geometry: which box the live caption fills, how the text wraps, and how much of it
//! fits. Pure arithmetic in points so the slide show can draw the same layout at any size.
//!
//! The four positions are the ones PowerPoint offers: over the bottom or the top of the slide,
//! or in the letterboxed band below or above it.

use serde::{Deserialize, Serialize};

/// The caption's position on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Position {
    /// Over the bottom edge of the slide.
    #[default]
    BottomOverlay,
    /// Over the top edge of the slide.
    TopOverlay,
    /// In the band under the slide (the letterbox area).
    BelowSlide,
    /// In the band above the slide.
    AboveSlide,
}

impl Position {
    pub const ALL: [Position; 4] = [Position::BottomOverlay, Position::TopOverlay, Position::BelowSlide, Position::AboveSlide];

    pub fn as_str(self) -> &'static str {
        match self {
            Position::BottomOverlay => "bottomOverlay",
            Position::TopOverlay => "topOverlay",
            Position::BelowSlide => "belowSlide",
            Position::AboveSlide => "aboveSlide",
        }
    }

    /// The label the Subtitle Settings dialog shows.
    pub fn label(self) -> &'static str {
        match self {
            Position::BottomOverlay => "Bottom of slide",
            Position::TopOverlay => "Top of slide",
            Position::BelowSlide => "Below slide",
            Position::AboveSlide => "Above slide",
        }
    }

    /// Parse the spelling commands and `prefs.json` use. Also takes the lower-case two-word
    /// spelling (`below slide`) a hand-edited file may hold.
    pub fn parse(s: &str) -> Option<Position> {
        let key: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        Position::ALL.into_iter().find(|p| p.as_str().to_ascii_lowercase() == key)
    }

    /// Whether the caption is drawn on the slide rather than in the surrounding band.
    pub fn on_slide(self) -> bool {
        matches!(self, Position::BottomOverlay | Position::TopOverlay)
    }
}

/// A rectangle in points, with its top-left corner at `(x, y)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const EMPTY: Rect = Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };

    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x: sane(x), y: sane(y), w: sane(w).max(0.0), h: sane(h).max(0.0) }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn center_x(&self) -> f64 {
        self.x + self.w / 2.0
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
    }
}

fn sane(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

/// Default caption text size in points.
pub const DEFAULT_FONT_PT: f64 = 24.0;
/// Smallest and largest caption text size the layout accepts.
pub const MIN_FONT_PT: f64 = 8.0;
pub const MAX_FONT_PT: f64 = 96.0;
/// Default line cap.
pub const DEFAULT_MAX_LINES: usize = 3;
/// Largest line cap; anything more would not fit a slide.
pub const MAX_LINES_CAP: usize = 8;
/// Default margin between the caption and the slide edge, in points.
pub const DEFAULT_MARGIN: f64 = 18.0;
/// Fraction of the slide width a caption may use.
pub const DEFAULT_WIDTH_FRACTION: f64 = 0.9;
/// Longest text a single layout considers; a transcript line longer than this is truncated.
pub const MAX_TEXT_CHARS: usize = 640;
/// Average glyph advance as a fraction of the font size, for wrapping without a font. This is
/// one wrapping unit: [wrap] charges an average Latin glyph one unit and a wide one two.
pub const ADVANCE_RATIO: f64 = 0.52;

/// What to lay out.
#[derive(Clone, Debug)]
pub struct LayoutInput<'a> {
    /// The whole viewport, in points.
    pub screen: Rect,
    /// The rendered slide inside the viewport (its letterbox is `screen` minus this).
    pub slide: Rect,
    pub position: Position,
    pub text: &'a str,
    /// Text size in points; out-of-range values take their nearest usable value.
    pub font_pt: f64,
    /// How many wrapped lines may show.
    pub max_lines: usize,
    /// Space between the caption and the slide's edge, in points.
    pub margin: f64,
    /// Fraction of the slide width the caption may use (`0.2..=1.0`).
    pub width_fraction: f64,
}

impl<'a> LayoutInput<'a> {
    /// The caption at the defaults for a slide box.
    pub fn new(screen: Rect, slide: Rect, position: Position, text: &'a str) -> Self {
        LayoutInput {
            screen,
            slide,
            position,
            text,
            font_pt: DEFAULT_FONT_PT,
            max_lines: DEFAULT_MAX_LINES,
            margin: DEFAULT_MARGIN,
            width_fraction: DEFAULT_WIDTH_FRACTION,
        }
    }
}

/// The caption box and the lines that fit in it.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptionLayout {
    pub rect: Rect,
    pub lines: Vec<String>,
    pub position: Position,
    pub font_pt: f64,
    /// Baseline-to-baseline distance, in points.
    pub line_height: f64,
    /// Text was dropped to fit `max_lines` (or [`MAX_TEXT_CHARS`]); the newest words are kept.
    pub clipped: bool,
    /// There was no room outside the slide, so the caption sits over it (Below/Above Slide).
    pub overlaps_slide: bool,
}

impl CaptionLayout {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() || self.rect.is_empty()
    }
}

/// Lay out the caption for `input`. Never returns a box outside `screen` and never returns
/// lines when the slide or screen has no size.
pub fn layout(input: &LayoutInput) -> CaptionLayout {
    let font_pt = finite_or(input.font_pt, DEFAULT_FONT_PT).clamp(MIN_FONT_PT, MAX_FONT_PT);
    let line_height = font_pt * 1.25;
    let max_lines = input.max_lines.clamp(1, MAX_LINES_CAP);
    let margin = finite_or(input.margin, DEFAULT_MARGIN).clamp(0.0, 200.0);
    let fraction = finite_or(input.width_fraction, DEFAULT_WIDTH_FRACTION).clamp(0.2, 1.0);
    let screen = input.screen;
    let slide = input.slide;
    let mut out =
        CaptionLayout { rect: Rect::EMPTY, lines: vec![], position: input.position, font_pt, line_height, clipped: false, overlaps_slide: false };
    if screen.is_empty() || slide.is_empty() {
        return out;
    }
    let usable = (slide.w * fraction).min(screen.w - 2.0 * margin);
    if usable <= 0.0 {
        return out;
    }
    let advance = (font_pt * ADVANCE_RATIO).max(1.0);
    // A line's capacity in wrapping units, so a wide glyph takes the room of two average ones.
    let per_line = ((usable / advance).floor() as usize).clamp(1, MAX_TEXT_CHARS);
    let (lines, clipped) = wrap_tail(input.text, per_line, max_lines);
    out.clipped = clipped;
    if lines.is_empty() {
        return out;
    }
    let h = (lines.len() as f64 * line_height).min(screen.h - 2.0 * margin).max(0.0);
    let w = usable.min(screen.w);
    let x = clamp_axis(slide.center_x() - w / 2.0, screen.x + margin, screen.right() - margin - w);
    let (y, overlaps) = match input.position {
        Position::BottomOverlay => (slide.bottom() - margin - h, false),
        Position::TopOverlay => (slide.y + margin, false),
        Position::BelowSlide => {
            let y = slide.bottom() + margin;
            match fit_axis(y, h, screen.y + margin, screen.bottom() - margin) {
                Some(y) => (y, false),
                None => (screen.bottom() - margin - h, true),
            }
        }
        Position::AboveSlide => {
            let y = slide.y - margin - h;
            match fit_axis(y, h, screen.y + margin, screen.bottom() - margin) {
                Some(y) => (y, false),
                None => (screen.y + margin, true),
            }
        }
    };
    let y = clamp_axis(y, screen.y + margin, screen.bottom() - margin - h);
    out.overlaps_slide = overlaps;
    out.rect = Rect::new(x, y, w, h);
    out.lines = lines;
    out
}

/// `v` when it is finite, else `default`.
fn finite_or(v: f64, default: f64) -> f64 {
    if v.is_finite() { v } else { default }
}

/// Keep a top edge between `lo` and `hi` (when the box is larger than that, `lo` wins).
fn clamp_axis(v: f64, lo: f64, hi: f64) -> f64 {
    if hi < lo { lo } else { v.clamp(lo, hi) }
}

/// `Some(y)` when a box of height `h` at `y` stays within `lo..=hi`.
fn fit_axis(y: f64, h: f64, lo: f64, hi: f64) -> Option<f64> {
    (y >= lo && y + h <= hi).then_some(y)
}

/// Wrap `text` to at most `per_line` wrapping units and `max_lines` lines. Words are kept whole
/// where they fit; a word longer than a line is split. Returns the lines and whether anything
/// was dropped, along with a flag cut off by the line cap.
///
/// A unit is one average Latin advance ([`ADVANCE_RATIO`] em); see [`glyph_units`].
pub fn wrap(text: &str, per_line: usize, max_lines: usize) -> (Vec<String>, bool) {
    let per_line = per_line.clamp(1, MAX_TEXT_CHARS);
    let max_lines = max_lines.clamp(1, MAX_LINES_CAP);
    let mut clipped = text.chars().count() > MAX_TEXT_CHARS;
    let text: String = text.chars().take(MAX_TEXT_CHARS).collect();
    let mut lines: Vec<String> = vec![];
    let mut current = String::new();
    for word in text.split_whitespace() {
        if lines.len() >= max_lines {
            clipped = true;
            break;
        }
        let sep = usize::from(!current.is_empty());
        if units_of(&current) + sep + units_of(word) <= per_line {
            if sep == 1 {
                current.push(' ');
            }
            current.push_str(word);
            continue;
        }
        if !current.is_empty() {
            if lines.len() + 1 >= max_lines {
                // This is the last line: keep what fits of the word and drop the rest.
                let room = per_line.saturating_sub(units_of(&current) + 1);
                // `take_units` keeps at least one character, which can be a wide glyph that needs
                // two units: append only what actually fits, so the line never exceeds `per_line`.
                let part = take_units(word, room);
                if !part.is_empty() && units_of(part) <= room {
                    current.push(' ');
                    current.push_str(part);
                }
                lines.push(std::mem::take(&mut current));
                return (lines, true);
            }
            lines.push(std::mem::take(&mut current));
        }
        let mut rest: &str = word;
        loop {
            if units_of(rest) <= per_line {
                current.push_str(rest);
                break;
            }
            if lines.len() + 1 >= max_lines {
                lines.push(take_units(rest, per_line).to_string());
                return (lines, true);
            }
            let cut = units_cut(rest, per_line);
            lines.push(rest.get(..cut).unwrap_or_default().to_string());
            rest = rest.get(cut..).unwrap_or_default();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        clipped = true;
    }
    (lines, clipped)
}

/// Wrap `text` keeping its newest words. When the whole text does not fit `max_lines` lines of
/// `per_line` units, leading words are dropped so the tail — what is being said now — is what
/// the caption shows; `clipped` then reports that text was dropped. A short text wraps exactly
/// as [`wrap`] does.
///
/// Captions are read while they are still being spoken, so the beginning of a long transcript
/// scrolls off rather than the newest words.
pub fn wrap_tail(text: &str, per_line: usize, max_lines: usize) -> (Vec<String>, bool) {
    let per_line = per_line.clamp(1, MAX_TEXT_CHARS);
    let max_lines = max_lines.clamp(1, MAX_LINES_CAP);
    // A transcript longer than the layout considers keeps its end, like the wrapping does.
    let total_chars = text.chars().count();
    let text: String = if total_chars > MAX_TEXT_CHARS { text.chars().skip(total_chars - MAX_TEXT_CHARS).collect() } else { text.to_string() };
    // Keep at most the newest characters that can fit by units — a run with no spaces (CJK)
    // has no word boundary to cut on — then start at a word boundary where there is one.
    let capacity = per_line.saturating_mul(max_lines);
    let tail = tail_by_units(&text, capacity);
    let tail = if tail.len() < text.len() {
        let before = text.get(..text.len() - tail.len()).and_then(|head| head.chars().next_back());
        match before {
            // The cut fell inside a word: start at the next boundary where there is one. A cut
            // that lands right after whitespace already starts at a word, so it is kept whole.
            Some(c) if !c.is_whitespace() => tail.find(char::is_whitespace).map_or(tail, |i| tail.get(i..).unwrap_or_default().trim_start()),
            _ => tail,
        }
    } else {
        tail
    };
    // The greedy wrap can still leave a line partly empty; drop one more leading word per pass,
    // never below the last word, which [`wrap`] splits instead.
    let words: Vec<&str> = tail.split_whitespace().collect();
    let mut dropped = total_chars > MAX_TEXT_CHARS || tail.len() < text.len();
    let mut start = 0;
    loop {
        let (lines, over) = wrap(&words.get(start..).unwrap_or_default().join(" "), per_line, max_lines);
        if !over || start + 1 >= words.len() {
            return (lines, dropped || over);
        }
        start += 1;
        dropped = true;
    }
}

/// The newest part of `s` whose characters cost at most `units`; at least one character.
fn tail_by_units(s: &str, units: usize) -> &str {
    let mut used = 0;
    let mut at_least_one = false;
    for (i, c) in s.char_indices().rev() {
        let cost = glyph_units(c);
        if at_least_one && used + cost > units {
            return s.get(i + c.len_utf8()..).unwrap_or_default();
        }
        used += cost;
        at_least_one = true;
    }
    s
}

/// The wrapping cost of one character in units of [`ADVANCE_RATIO`] em: an East Asian wide
/// character or an emoji occupies about one em, everything else about half an em.
fn glyph_units(c: char) -> usize {
    if is_wide(c) { 2 } else { 1 }
}

/// The wrapping cost of `s`; see [`glyph_units`].
fn units_of(s: &str) -> usize {
    s.chars().map(glyph_units).sum()
}

/// Whether `c` is drawn about two average Latin advances wide: the East Asian wide and
/// fullwidth blocks (CJK ideographs, kana, Hangul, their punctuation) and emoji.
fn is_wide(c: char) -> bool {
    matches!(
        u32::from(c),
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F900..=0x1F9FF
            | 0x20000..=0x3FFFD
    )
}

/// The longest prefix of `s` that costs at most `units`; at least one character.
fn take_units(s: &str, units: usize) -> &str {
    s.get(..units_cut(s, units)).unwrap_or_default()
}

/// The byte index after the longest prefix of `s` that costs at most `units`; at least one
/// character, so a single glyph wider than the line still advances.
fn units_cut(s: &str, units: usize) -> usize {
    let mut used = 0;
    let mut at_least_one = false;
    for (i, c) in s.char_indices() {
        let cost = glyph_units(c);
        if at_least_one && used + cost > units {
            return i;
        }
        used += cost;
        at_least_one = true;
    }
    s.len()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn screen_slide() -> (Rect, Rect) {
        (Rect::new(0.0, 0.0, 1600.0, 900.0), Rect::new(0.0, 0.0, 1600.0, 900.0))
    }

    #[test]
    fn bottom_overlay_sits_inside_the_slide_with_safe_margins() {
        let (screen, slide) = screen_slide();
        let l = layout(&LayoutInput::new(screen, slide, Position::BottomOverlay, "Hello there"));
        assert_eq!(l.lines, vec!["Hello there"]);
        assert_eq!((l.rect.x, l.rect.y, l.rect.w, l.rect.h), (80.0, 852.0, 1440.0, 30.0));
        assert!(!l.overlaps_slide);
        assert!(l.rect.contains(800.0, 860.0));
    }

    #[test]
    fn top_overlay_sits_under_the_top_edge() {
        let (screen, slide) = screen_slide();
        let l = layout(&LayoutInput::new(screen, slide, Position::TopOverlay, "Hello"));
        assert_eq!((l.rect.y, l.rect.h), (18.0, 30.0));
        assert_eq!(l.rect.x, 80.0);
    }

    #[test]
    fn the_bands_above_and_below_the_slide_hold_the_caption() {
        let screen = Rect::new(0.0, 0.0, 1600.0, 1200.0);
        let slide = Rect::new(0.0, 150.0, 1600.0, 900.0);
        let below = layout(&LayoutInput::new(screen, slide, Position::BelowSlide, "Below"));
        assert_eq!(below.rect.y, 1068.0, "slide bottom + margin");
        assert!(!below.overlaps_slide);
        let above = layout(&LayoutInput::new(screen, slide, Position::AboveSlide, "Above"));
        assert_eq!(above.rect.y, 102.0, "slide top - margin - height");
        assert!(!above.overlaps_slide);
        let overlay = layout(&LayoutInput::new(screen, slide, Position::BottomOverlay, "Over"));
        assert_eq!(overlay.rect.y, 1002.0, "inside the slide");
    }

    #[test]
    fn a_band_with_no_room_falls_back_over_the_slide() {
        let (screen, slide) = screen_slide();
        let below = layout(&LayoutInput::new(screen, slide, Position::BelowSlide, "Squeezed"));
        assert!(below.overlaps_slide);
        assert_eq!(below.rect.y, 852.0);
        assert!(below.rect.bottom() <= screen.bottom() - DEFAULT_MARGIN + 1e-9);
        let above = layout(&LayoutInput::new(screen, slide, Position::AboveSlide, "Squeezed"));
        assert!(above.overlaps_slide);
        assert_eq!(above.rect.y, 18.0);
    }

    #[test]
    fn wrapped_lines_grow_the_box_downwards() {
        let (screen, slide) = screen_slide();
        let text =
            "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty";
        let mut input = LayoutInput::new(screen, slide, Position::BottomOverlay, text);
        input.max_lines = 3;
        input.width_fraction = 0.2;
        let l = layout(&input);
        assert_eq!(l.lines.len(), 3, "{:?}", l.lines);
        assert_eq!(l.rect.h, 3.0 * 30.0);
        assert_eq!(l.rect.bottom(), 900.0 - DEFAULT_MARGIN);
        assert!(l.clipped, "the rest of the text does not fit");
    }

    #[test]
    fn wrapping_keeps_words_whole_and_splits_only_long_ones() {
        assert_eq!(wrap("one two three", 5, 2), (vec!["one".to_string(), "two t".to_string()], true));
        assert_eq!(wrap("abcdefghij", 4, 3), (vec!["abcd".to_string(), "efgh".to_string(), "ij".to_string()], false));
        assert_eq!(wrap("a b c d", 5, 2), (vec!["a b c".to_string(), "d".to_string()], false));
        assert_eq!(wrap("   ", 10, 2), (vec![], false));
        assert!(wrap(&"x".repeat(MAX_TEXT_CHARS + 10), 4, 2).1, "an overlong line is clipped");
    }

    #[test]
    fn a_wide_glyph_is_never_squeezed_into_the_last_line() {
        // `room` is one unit and the next glyph needs two: drop it rather than overflow.
        assert_eq!(wrap("ab 字字", 4, 1), (vec!["ab".to_string()], true));
        assert_eq!(wrap("ab cd", 4, 1), (vec!["ab c".to_string()], true), "a narrow glyph still fills the room");
    }

    #[test]
    fn wide_characters_take_two_wrapping_units() {
        assert_eq!(wrap("中文字", 6, 1), (vec!["中文字".to_string()], false));
        assert_eq!(wrap("中文字", 5, 2), (vec!["中文".to_string(), "字".to_string()], false));
        assert_eq!(wrap("字字字", 1, 2), (vec!["字".to_string(), "字".to_string()], true), "a glyph wider than the line still advances");
        assert_eq!(units_of("abc"), 3);
        assert_eq!(units_of("日本"), 4);
    }

    #[test]
    fn a_cjk_caption_stays_inside_its_box() {
        let (screen, slide) = screen_slide();
        let text = "字幕测试这是一段非常长的中文文本用于检查换行是否超出字幕边框".repeat(8);
        let mut input = LayoutInput::new(screen, slide, Position::BottomOverlay, &text);
        input.max_lines = 3;
        let l = layout(&input);
        assert_eq!(l.lines.len(), 3, "{:?}", l.lines);
        assert!(l.clipped, "the rest of the text does not fit");
        let advance = l.font_pt * ADVANCE_RATIO;
        for line in &l.lines {
            let estimated = units_of(line) as f64 * advance;
            assert!(estimated <= l.rect.w + 1e-9, "{line:?} estimates {estimated} wide, the box is {}", l.rect.w);
        }
    }

    #[test]
    fn a_long_caption_keeps_its_newest_words() {
        let (screen, slide) = screen_slide();
        let text =
            "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty";
        let mut input = LayoutInput::new(screen, slide, Position::BottomOverlay, text);
        input.max_lines = 3;
        input.width_fraction = 0.2;
        let l = layout(&input);
        assert_eq!(l.lines.len(), 3, "{:?}", l.lines);
        assert!(l.clipped, "the beginning scrolls off");
        assert!(l.lines.iter().any(|line| line.contains("twenty")), "the newest word is shown: {:?}", l.lines);
        assert!(!l.lines.iter().any(|line| line.split_whitespace().any(|w| w == "one")), "the oldest word is dropped: {:?}", l.lines);
    }

    #[test]
    fn wrap_tail_drops_the_beginning_not_the_end() {
        assert_eq!(wrap_tail("one two three", 5, 2), (vec!["two".to_string(), "three".to_string()], true));
        assert_eq!(wrap_tail("how are you", 20, 3), (vec!["how are you".to_string()], false));
        assert_eq!(wrap_tail("   ", 10, 2), (vec![], false));
        // A run with no word boundaries (CJK) keeps its newest glyphs.
        let (lines, clipped) = wrap_tail(&"字".repeat(30), 6, 2);
        assert!(clipped);
        assert_eq!(lines.concat().chars().count(), 6, "two lines of three wide glyphs: {lines:?}");
    }

    #[test]
    fn a_cut_at_a_word_boundary_keeps_that_word() {
        assert_eq!(tail_by_units("abcd efgh ij", 7), "efgh ij", "the cut lands right after a space");
        assert_eq!(wrap_tail("abcd efgh ij", 7, 1), (vec!["efgh ij".to_string()], true));
        assert_eq!(tail_by_units("agent cdef", 4), "cdef", "a cut inside a word starts after it");
        assert_eq!(wrap_tail("abcd cdef", 4, 2), (vec!["cdef".to_string()], true));
    }

    #[test]
    fn a_caption_never_leaves_the_screen() {
        let screen = Rect::new(0.0, 0.0, 320.0, 200.0);
        let slide = Rect::new(-40.0, -30.0, 500.0, 400.0);
        for p in Position::ALL {
            let l = layout(&LayoutInput::new(screen, slide, p, "A caption wider than the screen and taller than the band"));
            assert!(l.rect.x >= -1e-9 && l.rect.right() <= 320.0 + 1e-9, "{p:?} {:?}", l.rect);
            assert!(l.rect.y >= -1e-9 && l.rect.bottom() <= 200.0 + 1e-9, "{p:?} {:?}", l.rect);
        }
    }

    #[test]
    fn hostile_sizes_and_empty_text_draw_nothing() {
        let empty = Rect::new(f64::NAN, 0.0, 0.0, f64::INFINITY);
        let l = layout(&LayoutInput::new(empty, empty, Position::BottomOverlay, "text"));
        assert!(l.is_empty() && l.rect.w == 0.0 && l.rect.h == 0.0);
        let (screen, slide) = screen_slide();
        let mut input = LayoutInput::new(screen, slide, Position::BottomOverlay, "  ");
        input.font_pt = f64::NAN;
        input.max_lines = 0;
        input.margin = -1.0e300;
        input.width_fraction = 1.0e300;
        let l = layout(&input);
        assert!(l.is_empty());
        assert_eq!(l.font_pt, DEFAULT_FONT_PT);
        let mut input = LayoutInput::new(screen, slide, Position::BottomOverlay, "Hi");
        input.font_pt = 1.0e30;
        assert_eq!(layout(&input).font_pt, MAX_FONT_PT);
        input.font_pt = 0.1;
        assert_eq!(layout(&input).font_pt, MIN_FONT_PT);
    }

    #[test]
    fn positions_round_trip_through_their_names() {
        for p in Position::ALL {
            assert_eq!(Position::parse(p.as_str()), Some(p));
            assert!(!p.label().is_empty());
        }
        assert_eq!(Position::parse("below slide"), Some(Position::BelowSlide));
        assert_eq!(Position::parse(""), None);
        assert!(Position::BottomOverlay.on_slide() && !Position::BelowSlide.on_slide());
    }
}
