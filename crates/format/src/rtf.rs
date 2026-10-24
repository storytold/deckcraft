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
        match title.and_then(|t| t.paragraphs.iter().find(|p| !p.is_empty())) {
            Some(tp) => runs(&mut out, tp),
            None => escape(&mut out, &s.title()),
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
];

/// `i + n`, or one character on when that would land inside a multi-byte character (a fallback
/// run after `\uN` or `\'hh` may be followed by non-ASCII text).
fn advance(rtf: &str, i: usize, n: usize) -> usize {
    if rtf.is_char_boundary(i + n) { i + n } else { i + rtf.get(i..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8) }
}

/// Read an RTF outline into tab-indented outline text: a paragraph's outline level (or its left
/// indent in 0.25" steps) becomes its number of tabs.
pub fn rtf_to_outline(rtf: &str) -> String {
    #[derive(Clone, Copy)]
    struct St {
        skip: bool,
        uc: usize,
    }
    let b = rtf.as_bytes();
    let mut stack: Vec<St> = vec![];
    let mut st = St { skip: false, uc: 1 };
    let (mut level, mut indent): (Option<u32>, i64) = (None, 0);
    let mut line = String::new();
    let mut out = String::new();
    let mut pending_skip = 0usize;
    let mut i = 0;
    let flush = |line: &mut String, level: Option<u32>, indent: i64, out: &mut String| {
        let text = line.trim();
        if !text.is_empty() {
            let n = level.map(|l| l as i64).unwrap_or(indent.saturating_add(180) / 360).clamp(0, 9) as usize;
            out.push_str(&"\t".repeat(n));
            out.push_str(text);
            out.push('\n');
        }
        line.clear();
    };
    while i < b.len() {
        let c = b[i];
        match c {
            b'{' => {
                stack.push(st);
                i += 1;
                // `{\*\dest …}`: an ignorable destination.
                if b[i..].starts_with(b"\\*") {
                    st.skip = true;
                }
            }
            b'}' => {
                st = stack.pop().unwrap_or(St { skip: false, uc: 1 });
                i += 1;
            }
            b'\\' => {
                i += 1;
                let Some(&n) = b.get(i) else { break };
                if n.is_ascii_alphabetic() {
                    let s = i;
                    while i < b.len() && b[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let word = &rtf[s..i];
                    let ns = i;
                    if i < b.len() && (b[i] == b'-' || b[i].is_ascii_digit()) {
                        i += 1;
                        while i < b.len() && b[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    let arg: Option<i64> = rtf[ns..i].parse().ok();
                    if i < b.len() && b[i] == b' ' {
                        i += 1;
                    }
                    if pending_skip > 0 {
                        pending_skip -= 1;
                        continue;
                    }
                    if SKIP.contains(&word) {
                        st.skip = true;
                    }
                    if st.skip {
                        continue;
                    }
                    match word {
                        "par" => {
                            flush(&mut line, level, indent, &mut out);
                        }
                        "pard" => {
                            level = None;
                            indent = 0;
                        }
                        "outlinelevel" => level = arg.map(|a| a.clamp(0, 9) as u32),
                        "li" => indent = arg.unwrap_or(0),
                        "tab" | "line" => line.push(' '),
                        "uc" => st.uc = arg.unwrap_or(1).clamp(0, 8) as usize,
                        "u" => {
                            let v = arg.unwrap_or(0);
                            // RTF writes UTF-16 units as signed 16-bit values.
                            push_utf16(&mut line, (v & 0xffff) as u32);
                            pending_skip = 0;
                            // Skip the fallback characters.
                            let mut k = st.uc;
                            while k > 0 && i < b.len() {
                                if b[i] == b'\\' && b.get(i + 1) == Some(&b'\'') {
                                    i = advance(rtf, i, 4);
                                } else if b[i] == b'{' || b[i] == b'}' || b[i] == b'\\' {
                                    break;
                                } else {
                                    i = advance(rtf, i, 1);
                                }
                                k -= 1;
                            }
                        }
                        "emdash" => line.push('—'),
                        "endash" => line.push('–'),
                        "bullet" => line.push('•'),
                        "lquote" => line.push('‘'),
                        "rquote" => line.push('’'),
                        "ldblquote" => line.push('“'),
                        "rdblquote" => line.push('”'),
                        _ => {}
                    }
                } else if n == b'\'' {
                    let hex = rtf.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok());
                    i = advance(rtf, i, 3);
                    if !st.skip
                        && let Some(h) = hex
                        && h != 0
                    {
                        line.push(cp1252(h));
                    }
                } else {
                    i += 1;
                    if st.skip {
                        continue;
                    }
                    match n {
                        b'\\' | b'{' | b'}' => line.push(n as char),
                        b'~' => line.push('\u{a0}'),
                        b'\n' | b'\r' => flush(&mut line, level, indent, &mut out),
                        _ => {}
                    }
                }
            }
            b'\r' | b'\n' => i += 1,
            _ => {
                let s = i;
                while i < b.len() && !matches!(b[i], b'{' | b'}' | b'\\' | b'\r' | b'\n') {
                    i += 1;
                }
                if !st.skip
                    && let Some(t) = rtf.get(s..i)
                {
                    line.push_str(&t.replace('\u{0}', ""));
                }
            }
        }
    }
    flush(&mut line, level, indent, &mut out);
    finish_surrogates(out)
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

    #[test]
    fn reads_word_style_rtf_by_indent_and_hex_escapes() {
        let rtf = r"{\rtf1\ansi{\fonttbl{\f0 Arial;}}{\*\generator X;}\pard\b Title one\b0\par\pard\li720 Point \'93a\'94\par\pard\li1440 Sub\par}";
        assert_eq!(rtf_to_outline(rtf), "Title one\n\t\tPoint “a”\n\t\t\t\tSub\n");
    }
}
