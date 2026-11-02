//! Outline / RTF: PowerPoint's "Outline/RTF" save type, and reading such a file back as slides.
//!
//! The export writes one paragraph per slide title (outline level 0, bold, larger) and one per
//! body paragraph (outline level = indent + 1, indented), keeping bold, italic and underline.
//! The import reads paragraphs and their `\outlinelevel` (or, failing that, `\li` indent) into
//! the tab-indented outline [`crate::outline_to_slides`] understands.

use deckcraft_model::Presentation;
use deckcraft_model::text::{Paragraph, RunKind};

/// RTF-escape `s` (non-ASCII as `\uN?`, UTF-16, signed as RTF wants).
fn escape(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '\\' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '\t' => out.push_str("\\tab "),
            '\u{b}' | '\n' => out.push_str("\\line "),
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            c if c.is_ascii() => {}
            c => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{}?", *u as i16));
                }
            }
        }
    }
}

fn runs(out: &mut String, p: &Paragraph) {
    for r in &p.runs {
        if matches!(r.kind, RunKind::Break) {
            out.push_str("\\line ");
            continue;
        }
        let (b, i, u) = (r.props.bold == Some(true), r.props.italic == Some(true), r.props.underline.as_deref().is_some_and(|u| u != "none"));
        if !(b || i || u) {
            escape(out, &r.text);
            continue;
        }
        out.push('{');
        out.push_str(if b { "\\b" } else { "" });
        out.push_str(if i { "\\i" } else { "" });
        out.push_str(if u { "\\ul" } else { "" });
        out.push(' ');
        escape(out, &r.text);
        out.push('}');
    }
}

/// The presentation's outline (slide titles and body text) as RTF.
pub fn slides_to_rtf(p: &Presentation) -> String {
    let mut out = String::from("{\\rtf1\\ansi\\ansicpg1252\\deff0\\uc1{\\fonttbl{\\f0\\fswiss Liberation Sans;}}\n");
    for s in &p.slides {
        let title = s.shapes.iter().find(|sh| sh.ph_type().is_some_and(|k| k.is_title())).and_then(|sh| sh.text.as_ref());
        out.push_str("\\pard\\outlinelevel0\\sb240\\b\\fs32 ");
        // The title stays one outline paragraph (a `\par` would start another slide on re-import),
        // so its paragraphs are joined with line breaks.
        let mut tps = title.into_iter().flat_map(|t| &t.paragraphs).filter(|p| !p.is_empty()).peekable();
        if tps.peek().is_none() {
            escape(&mut out, &s.title());
        }
        for (k, tp) in tps.enumerate() {
            if k > 0 {
                out.push_str("\\line ");
            }
            runs(&mut out, tp);
        }
        out.push_str("\\b0\\par\n");
        for sh in &s.shapes {
            if sh.ph_type().is_some_and(|k| k.is_title()) {
                continue;
            }
            let Some(t) = &sh.text else { continue };
            for para in t.paragraphs.iter().filter(|p| !p.is_empty()) {
                let lvl = para.level.min(8) as u32;
                out.push_str(&format!("\\pard\\outlinelevel{}\\li{}\\fs24 ", lvl + 1, 360 * (lvl + 1)));
                runs(&mut out, para);
                out.push_str("\\par\n");
            }
        }
    }
    out.push_str("}\n");
    out
}

/// Whether `bytes` look like RTF.
pub fn sniff_rtf(bytes: &[u8]) -> bool {
    bytes.trim_ascii_start().starts_with(b"{\\rtf")
}

/// Destinations whose text is not document text.
const SKIP: &[&str] = &[
    "fonttbl",
    "colortbl",
    "stylesheet",
    "info",
    "pict",
    "header",
    "footer",
    "footnote",
    "listtable",
    "listoverridetable",
    "generator",
    "xmlnstbl",
    "themedata",
    "datastore",
    "latentstyles",
    "rsidtbl",
    "object",
    "fldinst",
    // A list item's rendered bullet or number, written before its text (Word: listtext; RTF 1.5 and
    // older writers: pntext, pntxta, pntxtb). Its `\pard` must not reset the paragraph's indent.
    "listtext",
    "pntext",
    "pntxta",
    "pntxtb",
];

/// `i + n`, or one character on when that would land inside a multi-byte character (a fallback
/// run after `\uN` or `\'hh` may be followed by non-ASCII text).
fn advance(rtf: &str, i: usize, n: usize) -> usize {
    if rtf.is_char_boundary(i + n) { i + n } else { i + rtf.get(i..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8) }
}

/// Group state: whether the text is being skipped, and the `\uc` fallback count.
#[derive(Clone, Copy)]
struct St {
    skip: bool,
    uc: usize,
}

/// Parser state for [`rtf_to_outline`]: the cursor, the group stack and the outline so far.
struct Outline<'a> {
    rtf: &'a str,
    b: &'a [u8],
    i: usize,
    stack: Vec<St>,
    st: St,
    level: Option<u32>,
    indent: i64,
    pending_skip: usize,
    line: String,
    out: String,
}

/// Read an RTF outline into tab-indented outline text: a paragraph's outline level (or its left
/// indent in 0.25" steps) becomes its number of tabs.
pub fn rtf_to_outline(rtf: &str) -> String {
    let mut p = Outline {
        rtf,
        b: rtf.as_bytes(),
        i: 0,
        stack: vec![],
        st: St { skip: false, uc: 1 },
        level: None,
        indent: 0,
        pending_skip: 0,
        line: String::new(),
        out: String::new(),
    };
    p.run();
    p.flush();
    finish_surrogates(p.out)
}

impl Outline<'_> {
    fn run(&mut self) {
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'{' => self.open_group(),
                b'}' => self.close_group(),
                b'\\' => {
                    self.i += 1;
                    if !self.escape() {
                        break;
                    }
                }
                b'\r' | b'\n' => self.i += 1,
                _ => self.text(),
            }
        }
    }

    /// `{`: push the group; `{\*\dest …}` is an ignorable destination.
    fn open_group(&mut self) {
        self.stack.push(self.st);
        self.i += 1;
        if self.b[self.i..].starts_with(b"\\*") {
            self.st.skip = true;
        }
    }

    fn close_group(&mut self) {
        self.st = self.stack.pop().unwrap_or(St { skip: false, uc: 1 });
        self.i += 1;
    }

    /// The character after a backslash: a control word, a hex escape or a symbol. False at the
    /// end of the input.
    fn escape(&mut self) -> bool {
        let Some(&n) = self.b.get(self.i) else { return false };
        if n.is_ascii_alphabetic() {
            self.control_word();
        } else if n == b'\'' {
            self.hex_escape();
        } else {
            self.symbol(n);
        }
        true
    }

    fn control_word(&mut self) {
        let rtf = self.rtf;
        let s = self.i;
        while self.i < self.b.len() && self.b[self.i].is_ascii_alphabetic() {
            self.i += 1;
        }
        let word = &rtf[s..self.i];
        let ns = self.i;
        if self.i < self.b.len() && (self.b[self.i] == b'-' || self.b[self.i].is_ascii_digit()) {
            self.i += 1;
            while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
                self.i += 1;
            }
        }
        let arg: Option<i64> = rtf[ns..self.i].parse().ok();
        if self.i < self.b.len() && self.b[self.i] == b' ' {
            self.i += 1;
        }
        if self.pending_skip > 0 {
            self.pending_skip -= 1;
            return;
        }
        if SKIP.contains(&word) {
            self.st.skip = true;
        }
        if !self.st.skip {
            self.apply(word, arg);
        }
    }

    fn apply(&mut self, word: &str, arg: Option<i64>) {
        match word {
            "par" => self.flush(),
            "pard" => {
                self.level = None;
                self.indent = 0;
            }
            "outlinelevel" => self.level = arg.map(|a| a.clamp(0, 9) as u32),
            "li" => self.indent = arg.unwrap_or(0),
            "tab" | "line" => self.line.push(' '),
            "uc" => self.st.uc = arg.unwrap_or(1).clamp(0, 8) as usize,
            "u" => self.unicode(arg.unwrap_or(0)),
            "emdash" => self.line.push('—'),
            "endash" => self.line.push('–'),
            "bullet" => self.line.push('•'),
            "lquote" => self.line.push('‘'),
            "rquote" => self.line.push('’'),
            "ldblquote" => self.line.push('“'),
            "rdblquote" => self.line.push('”'),
            _ => {}
        }
    }

    /// `\uN`: one UTF-16 unit, then the `\uc` fallback characters, which are skipped.
    fn unicode(&mut self, v: i64) {
        // RTF writes UTF-16 units as signed 16-bit values.
        push_utf16(&mut self.line, (v & 0xffff) as u32);
        self.pending_skip = 0;
        let mut k = self.st.uc;
        while k > 0 && self.i < self.b.len() {
            if self.b[self.i] == b'\\' && self.b.get(self.i + 1) == Some(&b'\'') {
                self.i = advance(self.rtf, self.i, 4);
            } else if matches!(self.b[self.i], b'{' | b'}' | b'\\') {
                break;
            } else {
                self.i = advance(self.rtf, self.i, 1);
            }
            k -= 1;
        }
    }

    /// `\'hh`: one cp1252 byte.
    fn hex_escape(&mut self) {
        let hex =
            self.rtf.get(self.i + 1..self.i + 3).filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit())).and_then(|h| u8::from_str_radix(h, 16).ok());
        self.i = advance(self.rtf, self.i, 3);
        if !self.st.skip
            && let Some(h) = hex
            && h != 0
        {
            self.line.push(cp1252(h));
        }
    }

    /// A one-character control symbol (`\\`, `\{`, `\~`, ...).
    fn symbol(&mut self, n: u8) {
        self.i += 1;
        if self.st.skip {
            return;
        }
        match n {
            b'\\' | b'{' | b'}' => self.line.push(n as char),
            b'~' => self.line.push('\u{a0}'),
            b'_' => self.line.push('\u{2011}'),
            b'\n' | b'\r' => self.flush(),
            _ => {}
        }
    }

    /// Plain text up to the next special character.
    fn text(&mut self) {
        let s = self.i;
        while self.i < self.b.len() && !matches!(self.b[self.i], b'{' | b'}' | b'\\' | b'\r' | b'\n') {
            self.i += 1;
        }
        if !self.st.skip
            && let Some(t) = self.rtf.get(s..self.i)
        {
            self.line.push_str(&t.replace('\u{0}', ""));
        }
    }

    /// Ends the current line: a non-empty line becomes one outline entry, indented by its level.
    fn flush(&mut self) {
        let text = self.line.trim();
        if !text.is_empty() {
            let n = self.level.map(|l| l as i64).unwrap_or(self.indent.saturating_add(180) / 360).clamp(0, 9) as usize;
            self.out.push_str(&"\t".repeat(n));
            self.out.push_str(text);
            self.out.push('\n');
        }
        self.line.clear();
    }
}

/// Push a UTF-16 unit; surrogate halves are parked as private markers (NUL plus four hex digits)
/// and joined afterwards. NUL itself is the marker, so a NUL unit is dropped.
fn push_utf16(line: &mut String, v: u32) {
    if v == 0 {
        return;
    }
    match char::from_u32(v) {
        Some(c) => line.push(c),
        None => line.push_str(&format!("\u{0}{v:04x}")),
    }
}

fn finish_surrogates(s: String) -> String {
    if !s.contains('\u{0}') {
        return s;
    }
    let mut out = String::new();
    let mut rest = s.as_str();
    while let Some(p) = rest.find('\u{0}') {
        out.push_str(&rest[..p]);
        let hi = rest.get(p + 1..p + 5).and_then(|h| u32::from_str_radix(h, 16).ok());
        let lo = rest.get(p + 6..p + 10).filter(|_| rest.get(p + 5..p + 6) == Some("\u{0}")).and_then(|h| u32::from_str_radix(h, 16).ok());
        match (hi, lo) {
            (Some(h), Some(l)) if (0xD800..0xDC00).contains(&h) && (0xDC00..0xE000).contains(&l) => {
                out.extend(char::from_u32(0x10000 + ((h - 0xD800) << 10) + (l - 0xDC00)));
                rest = &rest[p + 10..];
            }
            _ => {
                out.push('\u{fffd}');
                // Skip the NUL and, when four hex digits follow, the digits; otherwise
                // resume right after the NUL so no following character is lost.
                let skip = if hi.is_some() { p + 5 } else { p + 1 };
                rest = rest.get(skip..).unwrap_or("");
            }
        }
    }
    out.push_str(rest);
    out
}

fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—',
        '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9f => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// A sign in a `\'hh` escape is not a hex digit and must be ignored.
    #[test]
    fn hex_escape_rejects_sign() {
        assert_eq!(rtf_to_outline(r"{\rtf1 a\'+5b\par}"), "ab\n");
    }

    /// Non-ASCII text after a `\\uN` or `\\'hh` escape used to split a character and panic.
    #[test]
    fn non_ascii_text_after_escapes_does_not_panic() {
        let out = rtf_to_outline(r"{\rtf1\pard \u233é x \'€y\par}");
        assert!(out.contains('x'), "{out:?}");
        for rtf in [r"{\rtf1 \u233é€ z\par}", r"{\rtf1 \'é\par}", r"{\rtf1 \'4", r"{\rtf1 \u233"] {
            let _ = rtf_to_outline(rtf);
        }
    }

    /// NUL is the surrogate marker, so NULs from `\u0`, `\'00` or raw bytes must not reach `finish_surrogates`.
    #[test]
    fn nul_escapes_do_not_drop_text_or_add_a_slide() {
        assert_eq!(rtf_to_outline(r"{\rtf1 \u0?abcé\par}"), "abcé\n");
        assert_eq!(rtf_to_outline(r"{\rtf1 a\'00bé\par}"), "abé\n");
        assert_eq!(rtf_to_outline("{\\rtf1 a\0b\\par}\0\0"), "ab\n");
        assert_eq!(rtf_to_outline(r"{\rtf1 x\u-70000?y\par}"), "x\u{ee90}y\n");
    }

    /// `\\_` is RTF's non-breaking hyphen: a visible character, not markup.
    #[test]
    fn non_breaking_hyphen_is_kept() {
        assert_eq!(rtf_to_outline(r"{\rtf1\pard Year\_end review\par}"), "Year\u{2011}end review\n");
    }

    /// A hostile `\li` near `i64::MAX` overflowed the rounding addend in debug builds.
    #[test]
    fn huge_left_indent_gives_the_deepest_level() {
        let out = rtf_to_outline(r"{\rtf1\pard\li9223372036854775807 deep\par\pard\li-9223372036854775808 flat\par}");
        assert_eq!(out, format!("{}deep\nflat\n", "\t".repeat(9)));
    }

    #[test]
    fn export_reads_back_as_the_same_outline() {
        let mut p = deckcraft_model::defaults::blank_presentation(deckcraft_model::defaults::WIDE, Default::default(), false);
        let outline = "Plan {Q3}\n\tGrow — fast 😀\n\t\tEurope\\Asia\nCafé résumé\n\tDone\n";
        crate::outline_to_slides(&mut p, outline);
        let rtf = slides_to_rtf(&p);
        assert!(sniff_rtf(rtf.as_bytes()));
        assert!(rtf.contains("\\outlinelevel2") && rtf.contains("Plan \\{Q3\\}"), "{rtf}");
        assert_eq!(rtf_to_outline(&rtf), crate::slides_to_outline(&p));
    }

    /// A title with two paragraphs: the plain outline keeps both, the RTF export kept only the first.
    #[test]
    fn every_title_paragraph_is_exported() {
        let mut p = deckcraft_model::defaults::blank_presentation(deckcraft_model::defaults::WIDE, Default::default(), false);
        crate::outline_to_slides(&mut p, "First line\n\tBody\n");
        let slide = std::sync::Arc::make_mut(&mut p.slides[0]);
        let title = slide.shapes.iter_mut().find(|sh| sh.ph_type().is_some_and(|k| k.is_title())).and_then(|sh| sh.text.as_mut()).unwrap();
        title.paragraphs.push(Paragraph::new("Second line"));
        let rtf = slides_to_rtf(&p);
        assert!(rtf.contains("First line\\line Second line\\b0\\par"), "{rtf}");
        assert_eq!(rtf_to_outline(&rtf), crate::slides_to_outline(&p));
    }

    #[test]
    fn reads_word_style_rtf_by_indent_and_hex_escapes() {
        let rtf = r"{\rtf1\ansi{\fonttbl{\f0 Arial;}}{\*\generator X;}\pard\b Title one\b0\par\pard\li720 Point \'93a\'94\par\pard\li1440 Sub\par}";
        assert_eq!(rtf_to_outline(rtf), "Title one\n\t\tPoint “a”\n\t\t\t\tSub\n");
    }

    /// Word writes a list item's bullet as `{\\listtext\\pard\\plain …\\'b7\\tab}` (older writers:
    /// `{\\pntext …}`) after the paragraph's own `\\li`. That group is the rendered bullet, not text.
    #[test]
    fn word_list_bullets_are_not_text_and_keep_the_indent() {
        let rtf = r"{\rtf1\ansi\pard Title\par\pard\fi-360\li720{\listtext\pard\plain\f1\'b7\tab}Point\par\pard\li720{\pntext\f1\'b7\tab}Old\par}";
        assert_eq!(rtf_to_outline(rtf), "Title\n\t\tPoint\n\t\tOld\n");
    }
}
