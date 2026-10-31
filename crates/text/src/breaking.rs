use super::style::{Cell, ParaShaped};
use super::{FontFace, ParaProps};

pub(crate) fn measure_str(face: &FontFace, size: f64, s: &str) -> (f64, Vec<(u32, f64)>) {
    let g = deckcraft_fonts::shape(face, s, &[], |c| c);
    let k = size / face.upem.max(1.0);
    let mut x = 0.0;
    let mut out = vec![];
    for gl in g {
        out.push((gl.gid, x + gl.x_offset as f64 * k));
        x += gl.x_advance as f64 * k;
    }
    (x, out)
}

/// Single line spacing as a multiple of the font size. PowerPoint spaces single-spaced lines 1.2
/// times the size apart in every font (observed on twenty Latin, CJK, Thai and Indic fonts at 18
/// and 40 pt), whatever the font's own ascent and descent.
///
/// Caveat: this was measured on one Windows install of desktop PowerPoint. It may be a Windows or
/// machine setting; PowerPoint on macOS or the web may space lines by the font's own metrics.
/// Keep it a single tunable value, and re-measure on the other platform before changing it.
pub(crate) const LINE_PITCH: f64 = 1.2;

/// A line's height above and below the baseline at `size`: [`LINE_PITCH`] × size, split where
/// the font's Windows ascent and descent put the baseline.
pub(crate) fn line_metrics(face: &FontFace, size: f64) -> (f64, f64) {
    let h = size * LINE_PITCH;
    (h * face.line_split, h * (1.0 - face.line_split))
}

/// Hangul syllables and jamo: PowerPoint wraps Korean at word boundaries (spaces), not between
/// syllables.
fn is_hangul(c: char) -> bool {
    matches!(c, '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' | '\u{A960}'..='\u{A97F}' | '\u{AC00}'..='\u{D7FF}')
}

/// East Asian commas and full stops, the punctuation PowerPoint lets hang past the end of a line
/// (`hangingPunct`). Closing brackets don't hang: "「。」" overflowing by its "」" moves to the next
/// line whole.
fn is_hanging_punct(c: char) -> bool {
    matches!(c, '、' | '。' | '，' | '．' | '､' | '｡')
}

/// Small kana, the prolonged sound mark and iteration marks: PowerPoint keeps them off the start
/// of a line (strict Japanese line breaking), where UAX #14's default allows a break before them.
fn is_cj_nonstarter(c: char) -> bool {
    matches!(
        c,
        'ぁ' | 'ぃ'
            | 'ぅ'
            | 'ぇ'
            | 'ぉ'
            | 'っ'
            | 'ゃ'
            | 'ゅ'
            | 'ょ'
            | 'ゎ'
            | 'ゕ'
            | 'ゖ'
            | 'ァ'
            | 'ィ'
            | 'ゥ'
            | 'ェ'
            | 'ォ'
            | 'ッ'
            | 'ャ'
            | 'ュ'
            | 'ョ'
            | 'ヮ'
            | 'ヵ'
            | 'ヶ'
            | 'ー'
            | '々'
            | '〻'
            | 'ゝ'
            | 'ゞ'
            | 'ヽ'
            | 'ヾ'
            | '\u{31F0}'..='\u{31FF}'
            | 'ｧ'..='ｰ'
    )
}

/// Break opportunities: allowed[i] = a line may start at char i. Unicode line breaking, then
/// PowerPoint's differences: without `latinLnBrk` a word doesn't break after `/` or `\`, Korean
/// breaks only between words, and without `eaLnBrk` (East Asian line breaking, the default) the
/// strict Japanese rule that keeps a small kana or the prolonged sound mark off the start of a
/// line is relaxed.
pub(crate) fn break_opportunities(text: &str, latin_break: bool, east_asian_break: bool) -> (Vec<bool>, Vec<bool>) {
    let n = text.chars().count();
    let mut allowed = vec![false; n + 1];
    let mut mandatory = vec![false; n + 1];
    let mut byte_to_char = std::collections::HashMap::new();
    for (ci, (b, _)) in text.char_indices().enumerate() {
        byte_to_char.insert(b, ci);
    }
    byte_to_char.insert(text.len(), n);
    for (b, op) in unicode_linebreak::linebreaks(text) {
        if let Some(&ci) = byte_to_char.get(&b) {
            if let Some(a) = allowed.get_mut(ci) {
                *a = true;
            }
            if op == unicode_linebreak::BreakOpportunity::Mandatory
                && let Some(m) = mandatory.get_mut(ci)
            {
                *m = true;
            }
        }
    }
    let chars: Vec<char> = text.chars().collect();
    // Relaxing 禁則 (`eaLnBrk="0"`) lifts only the Unicode rule that keeps a non-starter off the
    // start of a line (LB21, and LB16 before a non-starter), never a rule that binds the position
    // for another reason: an opening bracket still ends no line (LB14), and U+2060 or a no-break
    // space still holds (LB11, LB12). Rerun the algorithm on a copy where each non-starter is an
    // ordinary kana — one char for one char, so char indices map back — and accept a break before
    // a non-starter only where that relaxed run allows it.
    let mut relaxed_allowed = vec![false; n + 1];
    if !east_asian_break && chars.iter().any(|c| is_cj_nonstarter(*c)) {
        let relaxed: String = chars.iter().map(|c| if is_cj_nonstarter(*c) { 'あ' } else { *c }).collect();
        let mut char_of_byte = std::collections::HashMap::new();
        for (ci, (b, _)) in relaxed.char_indices().enumerate() {
            char_of_byte.insert(b, ci);
        }
        char_of_byte.insert(relaxed.len(), n);
        for (b, _) in unicode_linebreak::linebreaks(&relaxed) {
            if let Some(&ci) = char_of_byte.get(&b)
                && let Some(a) = relaxed_allowed.get_mut(ci)
            {
                *a = true;
            }
        }
    }
    for (i, w) in chars.windows(2).enumerate() {
        let (prev, next) = (w[0], w[1]);
        // PowerPoint never breaks after a zero-width space, nor before an em dash (only after it).
        let other_prohibition = (is_hangul(prev) && is_hangul(next)) || prev == '\u{200B}' || next == '\u{2014}';
        let latin_allowed = latin_break || !matches!(prev, '/' | '\\') || next.is_whitespace();
        let keep = if other_prohibition || (east_asian_break && is_cj_nonstarter(next)) { false } else { latin_allowed };
        let mandatory_here = mandatory.get(i + 1).copied().unwrap_or(false);
        if !keep
            && !mandatory_here
            && let Some(a) = allowed.get_mut(i + 1)
        {
            *a = false;
        }
        // With East Asian line breaking off (`eaLnBrk="0"`, 禁則処理なし) a line may start with a
        // small kana or the prolonged sound mark. Unicode offers no break before them at all, so
        // relaxing 禁則 means adding the opportunity, not keeping one — but only where no *other*
        // rule forbids the break (after a zero-width space, before an em dash, or inside a Latin
        // word without `latinLnBrk`) and where the relaxed run above still allows it (an opening
        // bracket, U+2060 or a no-break space keeps its Unicode prohibition).
        if !east_asian_break
            && is_cj_nonstarter(next)
            && !other_prohibition
            && latin_allowed
            && !mandatory_here
            && relaxed_allowed.get(i + 1).copied().unwrap_or(false)
            && let Some(a) = allowed.get_mut(i + 1)
        {
            *a = true;
        }
    }
    (allowed, mandatory)
}

pub(crate) struct RawLine {
    pub(crate) start: usize,
    pub(crate) end: usize,
    /// End including trailing spaces / the break char consumed.
    next: usize,
}

/// Wrapping geometry of one paragraph: line widths, start offsets, wrap flag and tab settings.
pub(crate) struct BreakParams<'a> {
    pub(crate) first_w: f64,
    pub(crate) rest_w: f64,
    pub(crate) wrap: bool,
    pub(crate) tab_stops: &'a [Tab],
    pub(crate) default_tab: f64,
    pub(crate) x0_first: f64,
    pub(crate) x0_rest: f64,
}

pub(crate) fn break_lines(p: &ParaShaped, params: &BreakParams) -> Vec<RawLine> {
    let BreakParams { first_w, rest_w, wrap, tab_stops, default_tab, x0_first, x0_rest } = *params;
    let text: String = p.cells.iter().map(|c| c.ch).collect();
    let (allowed, _) = break_opportunities(
        &text,
        p.props.latin_line_break.unwrap_or(false),
        // `eaLnBrk` absent means on (PowerPoint's default): only an explicit off relaxes 禁則.
        p.props.east_asian_line_break.unwrap_or(true),
    );
    let hang = p.props.hanging_punct.unwrap_or(false);
    let n = p.cells.len();
    let mut lines = vec![];
    let mut start = 0;
    if n == 0 {
        return vec![RawLine { start: 0, end: 0, next: 0 }];
    }
    while start < n {
        let avail = if lines.is_empty() { first_w } else { rest_w };
        let x0 = if lines.is_empty() { x0_first } else { x0_rest };
        let mut x = 0.0;
        let mut last_break: Option<usize> = None;
        let mut i = start;
        let mut end = n;
        let mut next = n;
        while i < n {
            let c = p.cells.get(i).map(|c| c.ch).unwrap_or(' ');
            if c == '\u{b}' || c == '\n' {
                end = i;
                next = i + 1;
                break;
            }
            let adv = if c == '\t' {
                tab_advance(x0 + x, tab_stops, default_tab, tab_segment(&p.cells, i))
            } else {
                p.cells.get(i).map(|c| c.adv).unwrap_or(0.0)
            };
            let breakable = |j: usize| p.cells.get(j).is_none_or(|c| c.breakable(j));
            if i > start && allowed.get(i).copied().unwrap_or(false) && breakable(i) {
                last_break = Some(i);
            }
            // Closing East Asian punctuation hangs past the end instead of wrapping
            // (`hangingPunct`); the line breaks after it.
            if wrap && hang && x + adv > avail + 0.01 && x <= avail + 0.01 && is_hanging_punct(c) && i > start {
                end = i + 1;
                next = i + 1;
                break;
            }
            // A tab that would run past the end moves to the next line (and to its first stop there).
            if wrap && c == '\t' && x + adv > avail + 0.01 && i > start {
                end = i;
                next = i;
                while end > start && p.cells.get(end - 1).is_some_and(|c| c.ch == ' ') {
                    end -= 1;
                }
                break;
            }
            if wrap && x + adv > avail + 0.01 && !c.is_whitespace() && i > start {
                // Break at the last opportunity, or force mid-word (between clusters).
                let b = last_break
                    .filter(|b| *b > start)
                    .unwrap_or_else(|| (start + 1..=i).rev().find(|&j| breakable(j)).or_else(|| (i + 1..n).find(|&j| breakable(j))).unwrap_or(n));
                end = b;
                next = b;
                // Trailing spaces belong to the line but don't count.
                while end > start && p.cells.get(end - 1).is_some_and(|c| c.ch == ' ') {
                    end -= 1;
                }
                break;
            }
            x += adv;
            i += 1;
        }
        if i >= n {
            end = n;
            next = n;
        }
        lines.push(RawLine { start, end, next });
        if next <= start {
            // No progress: force one char.
            let last = lines.len() - 1;
            let to = (start + 1..n).find(|&j| p.cells.get(j).is_none_or(|c| c.breakable(j))).unwrap_or(n);
            if let Some(l) = lines.get_mut(last) {
                l.end = to;
                l.next = to;
            }
            start = to;
        } else {
            start = next;
        }
        if next == n && p.cells.last().is_some_and(|c| c.ch == '\u{b}') && start >= n {
            // A trailing line break opens an empty last line.
            lines.push(RawLine { start: n, end: n, next: n });
        }
    }
    lines
}

/// How a tab stop aligns the text after it (`a:tab/@algn`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum TabAlign {
    Left,
    Center,
    Right,
    /// The first "." of the text sits at the stop (text without one ends there).
    Decimal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tab {
    pos: f64,
    align: TabAlign,
}

pub(crate) fn tabs_of(props: &ParaProps) -> Vec<Tab> {
    props
        .tabs
        .as_ref()
        .map(|t| {
            t.iter()
                .map(|s| Tab {
                    pos: s.pos,
                    align: match s.align.as_str() {
                        "ctr" => TabAlign::Center,
                        "r" => TabAlign::Right,
                        "dec" => TabAlign::Decimal,
                        _ => TabAlign::Left,
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Width of the text after the tab at cell `i` up to the next tab or line break, and the width of
/// that text before its first ".".
pub(crate) fn tab_segment(cells: &[Cell], i: usize) -> (f64, f64) {
    let mut w = 0.0;
    let mut dec = None;
    for c in cells.iter().skip(i + 1) {
        if matches!(c.ch, '\t'..='\u{b}') {
            break;
        }
        if c.ch == '.' && dec.is_none() {
            dec = Some(w);
        }
        w += c.adv;
    }
    (w, dec.unwrap_or(w))
}

/// Advance of a tab at `x` (from the start edge) to the next stop, placing the text after it
/// (`seg`: its width, and its width before the decimal point) as the stop aligns it but never
/// moving back, as PowerPoint does; past the last stop, to the next default stop.
pub(crate) fn tab_advance(x: f64, stops: &[Tab], default_tab: f64, seg: (f64, f64)) -> f64 {
    for s in stops {
        if s.pos > x + 0.5 {
            let to = s.pos - x;
            return match s.align {
                TabAlign::Left => to,
                TabAlign::Right => (to - seg.0).max(0.0),
                TabAlign::Center => (to - seg.0 / 2.0).max(0.0),
                TabAlign::Decimal => (to - seg.1).max(0.0),
            };
        }
    }
    let d = if default_tab > 1.0 { default_tab } else { 72.0 };
    let next = ((x / d).floor() + 1.0) * d;
    (next - x).max(1.0)
}
