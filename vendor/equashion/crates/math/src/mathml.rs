//! Presentation MathML (MathML 3 / MathML Core) writer and reader.
//!
//! The writer produces a standalone `<math xmlns="http://www.w3.org/1998/Math/MathML">` element:
//! well-formed XML, no XML declaration, no DOCTYPE, no prefix, and no named entities (`<`, `&`
//! and `>` are written as numeric character references, everything else as raw UTF-8). That is
//! the shape Word, LibreOffice and ONLYOFFICE convert into an equation when it is pasted as text.
//!
//! | Tree | MathML |
//! |---|---|
//! | text | one `mi` per letter, `mn` per number, `mo` per operator, `mspace` per typed space |
//! | `Styled` | `mi` (`mathvariant` normal, bold, bold-italic) or `mtext` |
//! | `Frac` | `mfrac` |
//! | `Rad` | `msqrt` or `mroot` |
//! | `Script` | `msub`, `msup`, `msubsup` |
//! | `Nary` | `munderover` (`msubsup` for integrals) around the operator `mo`, then the body |
//! | `Delim` | `mrow` with `mo fence="true" stretchy="true"` at both ends and `mo separator="true"` between items (never `mfenced`) |
//! | `Matrix`, `EqArr` | `mtable` / `mtr` / `mtd` (`columnalign="left"` marks an equation array; cells split at an em space) |
//! | `Func` | `mi` name, `mo` U+2061 (function application), argument |
//! | `Accent` | `mover accent="true"` with the combining mark (U+0302 and so on), as OMML stores it |
//! | `Limit` | `munder` / `mover`, then U+2061 (see `FN_APPLY`) |
//! | `Bar`, `GroupChr` | `mover` / `munder` with a stretchy `mo` (a bar is U+00AF over or under) |
//! | `PreScript` | `mmultiscripts` with `mprescripts` |
//! | `Boxed` | `menclose notation="box"`, or `mpadded` for an invisible box |
//!
//! The reader takes the same constructs (and `mfenced`, `semantics`, `mstyle`, prefixed names
//! such as `mml:math`), and is bounded: input size, XML depth and element count are capped, and
//! hostile input never panics.

use crate::xml::{self, XEl, XNode};
use crate::{MAX_DEPTH, Math, Node, OMATH_BREAK, Seq, Style};

/// The MathML namespace.
pub const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";

/// Largest MathML text [`from_mathml`] reads, bytes.
pub const MAX_MATHML: usize = 1 << 20;

/// Why [`from_mathml`] gave no equation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathmlError {
    /// Nothing but whitespace.
    Empty,
    /// Longer than [`MAX_MATHML`] bytes.
    TooLarge,
    /// No `<math>` element in the text.
    NoMath,
}

impl std::fmt::Display for MathmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            MathmlError::Empty => "empty MathML",
            MathmlError::TooLarge => "MathML too large",
            MathmlError::NoMath => "no <math> element",
        })
    }
}

impl std::error::Error for MathmlError {}

/// Combining accent marks and the spacing characters MathML puts in the `mo` over the base.
const ACCENT_MARKS: &[(char, char)] = &[
    ('\u{0302}', '^'),
    ('\u{0303}', '~'),
    ('\u{0305}', '\u{00AF}'),
    ('\u{0307}', '\u{02D9}'),
    ('\u{0308}', '\u{00A8}'),
    ('\u{20D7}', '\u{2192}'),
    ('\u{0306}', '\u{02D8}'),
    ('\u{030C}', '\u{02C7}'),
    ('\u{0301}', '\u{00B4}'),
    ('\u{0300}', '`'),
];

/// Typed spaces and their `mspace` widths. The first entry for a width is the one read back.
const SPACES: &[(char, &str, f32)] = &[
    ('\u{2009}', "0.1667em", 0.1667),
    ('\u{2005}', "0.2222em", 0.2222),
    ('\u{2004}', "0.2778em", 0.2778),
    ('\u{2001}', "1em", 1.0),
    ('\u{2002}', "0.5em", 0.5),
    ('\u{200A}', "0.0833em", 0.0833),
    ('\u{00A0}', "0.25em", 0.25),
    ('\u{2003}', "1em", 1.0),
    ('\u{2006}', "0.1667em", 0.1667),
    ('\u{2008}', "0.2222em", 0.2222),
];

/// An invisible function application after an `munder` / `mover` that ONLYOFFICE reads as a
/// limit: its importer gives such a limit the next sibling as an argument and drops it, and this
/// is the sibling it takes (Word writes `lim` the same way, as a function name).
const FN_APPLY: &str = "<mo>&#x2061;</mo>";

/// Grouping marks that ONLYOFFICE reads as `m:groupChr` (no `FN_APPLY` needed after them).
const BRACES: &[char] = &['\u{23DE}', '\u{23DF}', '\u{23DC}', '\u{23DD}', '\u{23B4}', '\u{23B5}', '\u{00AF}'];

/// Characters drawn as stretchy grouping marks over or under a base (brace, bracket, arrows).
const GROUP_CHARS: &[char] =
    &['\u{23DE}', '\u{23DF}', '\u{23B4}', '\u{23B5}', '\u{23DC}', '\u{23DD}', '\u{2190}', '\u{2192}', '\u{2194}', '\u{21D0}', '\u{21D2}', '\u{21D4}'];

const OPEN_FENCES: &[&str] = &["(", "[", "{", "|", "\u{2016}", "\u{27E8}", "\u{2329}", "\u{230A}", "\u{2308}", "\u{27E6}"];
const CLOSE_FENCES: &[&str] = &[")", "]", "}", "|", "\u{2016}", "\u{27E9}", "\u{232A}", "\u{230B}", "\u{2309}", "\u{27E7}"];

/// Big operators read as n-ary operators.
fn is_nary_char(c: char) -> bool {
    matches!(c, '\u{2211}' | '\u{220F}' | '\u{2210}' | '\u{222B}'..='\u{2233}' | '\u{22C0}'..='\u{22C3}' | '\u{2A00}'..='\u{2A0C}')
}

fn is_fence_char(c: char) -> bool {
    let mut b = [0u8; 4];
    let s: &str = c.encode_utf8(&mut b);
    OPEN_FENCES.contains(&s) || CLOSE_FENCES.contains(&s)
}

fn is_integral(c: char) -> bool {
    matches!(c, '\u{222B}'..='\u{2233}')
}

// ---- writer -------------------------------------------------------------------------------

/// The equation as a MathML `<math>` element; `display="block"` when it is a display equation
/// (`m.para`), `display="inline"` otherwise.
pub fn to_mathml(m: &Math) -> String {
    to_mathml_display(m, m.para)
}

/// The equation as a MathML `<math>` element with `display="block"` (`block`) or
/// `display="inline"`. The text starts exactly with `<math xmlns="http://www.w3.org/1998/Math/MathML"`.
pub fn to_mathml_display(m: &Math, block: bool) -> String {
    let mut out = String::with_capacity(256);
    out.push_str("<math xmlns=\"");
    out.push_str(MATHML_NS);
    out.push_str("\" display=\"");
    out.push_str(if block { "block" } else { "inline" });
    out.push_str("\">");
    let mut body = String::new();
    let n = seq(&mut body, &m.body, 0);
    // One top-level row, so programs that expect a single child of `math` get one.
    if n == 1 {
        out.push_str(&body);
    } else if n > 1 {
        out.push_str("<mrow>");
        out.push_str(&body);
        out.push_str("</mrow>");
    }
    out.push_str("</math>");
    out
}

/// XML text with `<`, `&` and `>` as numeric references (never named entities).
fn esc(s: &str, out: &mut String) {
    for c in s.chars() {
        if !xml::xml_ok(c) {
            continue;
        }
        match c {
            '&' => out.push_str("&#38;"),
            '<' => out.push_str("&#60;"),
            '>' => out.push_str("&#62;"),
            '"' => out.push_str("&#34;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
}

fn token(out: &mut String, tag: &str, attrs: &str, text: &str) {
    out.push('<');
    out.push_str(tag);
    out.push_str(attrs);
    out.push('>');
    esc(text, out);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

/// Write a sequence; returns how many top-level elements were written.
fn seq(out: &mut String, s: &[Node], d: usize) -> usize {
    if d > MAX_DEPTH {
        return 0;
    }
    s.iter().map(|n| node(out, n.bare(), d + 1)).sum()
}

/// One argument of a layout element (exactly one element: bare, or an `mrow`).
fn arg(out: &mut String, s: &[Node], d: usize) {
    let mut tmp = String::new();
    if seq(&mut tmp, s, d) == 1 {
        out.push_str(&tmp);
    } else {
        out.push_str("<mrow>");
        out.push_str(&tmp);
        out.push_str("</mrow>");
    }
}

fn open(out: &mut String, tag: &str, attrs: &str) {
    out.push('<');
    out.push_str(tag);
    out.push_str(attrs);
    out.push('>');
}

fn close(out: &mut String, tag: &str) {
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

/// Plain text: one token per letter, number, operator or typed space. ASCII spaces separate
/// tokens and are not written (MathML spaces by operator, not by typed blanks).
fn text(out: &mut String, t: &str) -> usize {
    let cs: Vec<char> = t.chars().filter(|&c| xml::xml_ok(c)).collect();
    let mut n = 0;
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if let Some((_, w, _)) = SPACES.iter().find(|(s, _, _)| *s == c) {
            out.push_str("<mspace width=\"");
            out.push_str(w);
            out.push_str("\"/>");
            n += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < cs.len() && (cs[i].is_ascii_digit() || (cs[i] == '.' && cs.get(i + 1).is_some_and(char::is_ascii_digit))) {
                i += 1;
            }
            let num: String = cs[start..i].iter().collect();
            token(out, "mn", "", &num);
            n += 1;
            continue;
        }
        let s = c.to_string();
        if c.is_alphabetic() || matches!(c, '\u{221E}' | '\u{2202}' | '\u{2207}' | '\u{210F}' | '\u{2113}' | '\u{2135}' | '\u{2205}') {
            token(out, "mi", "", &s);
        } else if is_nary_char(c) {
            // A big-operator character inside text is not an n-ary structure.
            token(out, "mo", " largeop=\"false\"", &s);
        } else if is_fence_char(c) {
            // Typed brackets do not grow; `\left..\right` pairs are `Delim`.
            token(out, "mo", " stretchy=\"false\"", &s);
        } else {
            token(out, "mo", "", &s);
        }
        n += 1;
        i += 1;
    }
    n
}

fn styled(out: &mut String, t: &str, style: Style) -> usize {
    if t.is_empty() {
        return 0;
    }
    match style {
        Style::Normal => {
            // Token content is trimmed by MathML renderers: keep edge spaces as no-break spaces.
            let lead = t.len() - t.trim_start_matches(' ').len();
            let core = t.trim_matches(' ');
            let trail = if core.is_empty() { 0 } else { t.len() - t.trim_end_matches(' ').len() };
            let s = format!("{}{}{}", "\u{00A0}".repeat(lead), core, "\u{00A0}".repeat(trail));
            token(out, "mtext", "", &s);
        }
        Style::Plain => {
            let word = t.chars().count() > 1 && t.chars().all(char::is_alphabetic);
            token(out, "mi", if word { "" } else { " mathvariant=\"normal\"" }, t);
        }
        Style::Bold => token(out, "mi", " mathvariant=\"bold\"", t),
        Style::BoldItalic => token(out, "mi", " mathvariant=\"bold-italic\"", t),
    }
    1
}

/// Split an equation-array row into cells at em spaces in its text.
fn cells(row: &[Node]) -> Vec<Seq> {
    let mut out = vec![Seq::new()];
    for n in row {
        match n.bare() {
            Node::Text(t) if t.contains('\u{2003}') => {
                for (i, part) in t.split('\u{2003}').enumerate() {
                    if i > 0 {
                        out.push(Seq::new());
                    }
                    if !part.is_empty()
                        && let Some(c) = out.last_mut()
                    {
                        c.push(Node::Text(part.to_string()));
                    }
                }
            }
            other => {
                if let Some(c) = out.last_mut() {
                    c.push(other.clone());
                }
            }
        }
    }
    out
}

fn node(out: &mut String, n: &Node, d: usize) -> usize {
    match n {
        Node::Props { node: inner, .. } => node(out, inner.bare(), d),
        Node::Text(t) => text(out, t),
        Node::Styled { text: t, style } => styled(out, t, *style),
        Node::Frac { num, den } => {
            open(out, "mfrac", "");
            arg(out, num, d);
            arg(out, den, d);
            close(out, "mfrac");
            1
        }
        Node::Rad { deg: None, body } => {
            open(out, "msqrt", "");
            seq(out, body, d);
            close(out, "msqrt");
            1
        }
        Node::Rad { deg: Some(deg), body } => {
            open(out, "mroot", "");
            arg(out, body, d);
            arg(out, deg, d);
            close(out, "mroot");
            1
        }
        Node::Script { base, sub, sup } => {
            let tag = match (sub, sup) {
                (None, None) => return seq(out, base, d),
                (Some(_), None) => "msub",
                (None, Some(_)) => "msup",
                (Some(_), Some(_)) => "msubsup",
            };
            open(out, tag, "");
            arg(out, base, d);
            for s in [sub, sup].into_iter().flatten() {
                arg(out, s, d);
            }
            close(out, tag);
            1
        }
        Node::Nary { op, sub, sup, body } => {
            open(out, "mrow", "");
            let mut opx = String::new();
            let known = is_nary_char(*op);
            let s = if *op == '\0' { String::new() } else { op.to_string() };
            token(&mut opx, "mo", if known { "" } else { " largeop=\"true\"" }, &s);
            let side = is_integral(*op);
            let tag = match (sub, sup) {
                (None, None) => None,
                (Some(_), None) => Some(if side { "msub" } else { "munder" }),
                (None, Some(_)) => Some(if side { "msup" } else { "mover" }),
                (Some(_), Some(_)) => Some(if side { "msubsup" } else { "munderover" }),
            };
            match tag {
                None => out.push_str(&opx),
                Some(tag) => {
                    open(out, tag, "");
                    out.push_str(&opx);
                    for s in [sub, sup].into_iter().flatten() {
                        arg(out, s, d);
                    }
                    close(out, tag);
                }
            }
            arg(out, body, d);
            close(out, "mrow");
            1
        }
        Node::Delim { open: o, close: c, sep, items } => {
            open(out, "mrow", "");
            token(out, "mo", " fence=\"true\" stretchy=\"true\"", o);
            if items.is_empty() {
                out.push_str("<mrow></mrow>");
            }
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    token(out, "mo", " separator=\"true\"", sep);
                }
                arg(out, it, d);
            }
            token(out, "mo", " fence=\"true\" stretchy=\"true\"", c);
            close(out, "mrow");
            1
        }
        Node::Matrix { rows } => {
            open(out, "mtable", "");
            for r in rows {
                open(out, "mtr", "");
                for cell in r {
                    open(out, "mtd", "");
                    seq(out, cell, d);
                    close(out, "mtd");
                }
                close(out, "mtr");
            }
            close(out, "mtable");
            1
        }
        Node::EqArr { rows } => {
            open(out, "mtable", " columnalign=\"left\"");
            for r in rows {
                open(out, "mtr", "");
                for cell in cells(r) {
                    open(out, "mtd", "");
                    seq(out, &cell, d);
                    close(out, "mtd");
                }
                close(out, "mtr");
            }
            close(out, "mtable");
            1
        }
        Node::Func { name, body } => {
            open(out, "mrow", "");
            let one = name.chars().count() == 1;
            token(out, "mi", if one { " mathvariant=\"normal\"" } else { "" }, name);
            out.push_str("<mo>&#x2061;</mo>");
            arg(out, body, d);
            close(out, "mrow");
            1
        }
        Node::Accent { ch, body } => {
            // The combining mark itself (as OMML `m:acc` stores it): ONLYOFFICE turns that into an
            // accent, while a spacing `^` becomes an upper limit there.
            open(out, "mover", " accent=\"true\"");
            arg(out, body, d);
            token(out, "mo", " stretchy=\"false\"", &ch.to_string());
            close(out, "mover");
            1
        }
        Node::Limit { lower, base, lim } => {
            let tag = if *lower { "munder" } else { "mover" };
            open(out, "mrow", "");
            open(out, tag, "");
            arg(out, base, d);
            arg(out, lim, d);
            close(out, tag);
            out.push_str(FN_APPLY);
            close(out, "mrow");
            1
        }
        Node::PreScript { sub, sup, base } => {
            open(out, "mmultiscripts", "");
            arg(out, base, d);
            out.push_str("<mprescripts/>");
            arg(out, sub, d);
            arg(out, sup, d);
            close(out, "mmultiscripts");
            1
        }
        Node::Bar { top, body } => {
            let (tag, attrs) = if *top { ("mover", " accent=\"true\"") } else { ("munder", " accentunder=\"true\"") };
            let mark = "\u{00AF}";
            open(out, tag, attrs);
            arg(out, body, d);
            token(out, "mo", " stretchy=\"true\"", mark);
            close(out, tag);
            1
        }
        Node::GroupChr { ch, top, body } => {
            let tag = if *top { "mover" } else { "munder" };
            let brace = BRACES.contains(ch);
            if !brace {
                open(out, "mrow", "");
            }
            open(out, tag, "");
            arg(out, body, d);
            token(out, "mo", " stretchy=\"true\"", &ch.to_string());
            close(out, tag);
            if !brace {
                out.push_str(FN_APPLY);
                close(out, "mrow");
            }
            1
        }
        Node::Boxed { border, body } => {
            let (tag, attrs) = if *border { ("menclose", " notation=\"box\"") } else { ("mpadded", "") };
            open(out, tag, attrs);
            seq(out, body, d);
            close(out, tag);
            1
        }
        Node::Raw(src) if src == OMATH_BREAK || src.starts_with(&OMATH_BREAK[..OMATH_BREAK.len() - 1]) => {
            out.push_str("<mspace linebreak=\"newline\"/>");
            1
        }
        Node::Raw(src) => {
            let t = crate::linear::raw_text(src);
            if t.is_empty() {
                0
            } else {
                token(out, "mtext", "", &t);
                1
            }
        }
    }
}

// ---- reader -------------------------------------------------------------------------------

/// The `<math>` element of `src` (any prefix), from its start tag to its end tag (or the end of
/// the text), so MathML pasted inside an HTML page or with an XML declaration still reads.
fn math_slice(src: &str) -> Option<&str> {
    let b = src.as_bytes();
    let name_char = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b':');
    let mut start = None;
    let mut i = 0;
    while let Some(off) = src[i..].find('<') {
        let at = i + off;
        let mut j = at + 1;
        while j < b.len() && name_char(b[j]) {
            j += 1;
        }
        let name = &src[at + 1..j];
        let local = name.rsplit(':').next().unwrap_or(name);
        if local == "math" && (j == b.len() || matches!(b[j], b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/')) {
            start = Some((at, name.to_string()));
            break;
        }
        i = at + 1;
    }
    let (at, name) = start?;
    let end_tag = format!("</{name}");
    let end = src[at..].rfind(&end_tag).map(|e| {
        let e = at + e;
        src[e..].find('>').map_or(src.len(), |g| e + g + 1)
    });
    Some(&src[at..end.unwrap_or(src.len())])
}

fn find_math(els: &[XEl], d: usize) -> Option<&XEl> {
    if d > crate::XML_DEPTH {
        return None;
    }
    for e in els {
        if e.local() == "math" {
            return Some(e);
        }
        if let Some(m) = e.children.iter().find_map(|c| match c {
            XNode::El(x) => find_math(std::slice::from_ref(x), d + 1),
            XNode::Text(_) => None,
        }) {
            return Some(m);
        }
    }
    None
}

/// Read MathML: a `<math>` element (any namespace prefix, `display="block"` gives a display
/// equation), possibly inside other markup. Unknown elements give their text. Never panics.
pub fn from_mathml(src: &str) -> Result<Math, MathmlError> {
    if src.trim().is_empty() {
        return Err(MathmlError::Empty);
    }
    if src.len() > MAX_MATHML {
        return Err(MathmlError::TooLarge);
    }
    let slice = math_slice(src).ok_or(MathmlError::NoMath)?;
    let roots = xml::parse(slice);
    let root = find_math(&roots, 0).ok_or(MathmlError::NoMath)?;
    let para = root.attr_local("display") == Some("block") || root.attr_local("mode") == Some("display");
    Ok(Math { body: read_seq(root.els().collect(), 0), para, ..Default::default() })
}

/// Append a node, merging text into the text before it.
fn push(out: &mut Seq, n: Node) {
    match (out.last_mut(), n) {
        (_, Node::Text(t)) if t.is_empty() => {}
        (Some(Node::Text(a)), Node::Text(b)) => a.push_str(&b),
        (Some(Node::Styled { text: a, style: sa }), Node::Styled { text: b, style: sb }) if *sa == sb => a.push_str(&b),
        (_, n) => out.push(n),
    }
}

fn token_text(e: &XEl) -> String {
    e.text().trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r')).to_string()
}

fn attr<'a>(e: &'a XEl, name: &str) -> Option<&'a str> {
    e.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.trim())
}

fn is_true(e: &XEl, name: &str) -> bool {
    attr(e, name) == Some("true")
}

/// A grouping element whose children belong to the surrounding row.
fn is_group(e: &XEl) -> bool {
    match e.local() {
        "mrow" => !is_delim_row(e),
        "mstyle" | "maction" | "merror" => true,
        "mpadded" => e.attrs.iter().any(|(k, _)| k != "xmlns" && !k.starts_with("xmlns:")),
        _ => false,
    }
}

/// Children with `semantics`, `maction` and annotations looked through. Grouping rows are
/// read by `read_el`, so an n-ary operator only takes its body from its own row.
fn expand<'a>(els: Vec<&'a XEl>, d: usize, out: &mut Vec<&'a XEl>) {
    if d > crate::XML_DEPTH {
        return;
    }
    for e in els {
        match e.local() {
            "semantics" => expand(e.els().take(1).collect(), d + 1, out),
            "annotation" | "annotation-xml" | "mphantom" | "none" | "mprescripts" => {}
            "maction" => expand(e.els().take(1).collect(), d + 1, out),
            _ => out.push(e),
        }
    }
}

fn single_char(s: &str) -> Option<char> {
    let mut it = s.chars();
    let c = it.next()?;
    it.next().is_none().then_some(c)
}

/// The operator of an n-ary structure: an `mo` holding a big operator (or marked `largeop`).
fn nary_op(e: &XEl) -> Option<char> {
    if e.local() != "mo" || attr(e, "largeop") == Some("false") {
        return None;
    }
    let t = token_text(e);
    if is_true(e, "largeop") {
        return match t.chars().count() {
            0 => Some('\0'),
            1 => single_char(&t),
            _ => None,
        };
    }
    single_char(&t).filter(|&c| is_nary_char(c))
}

/// An n-ary operator with optional limits: (op, sub, sup).
type NaryHead = (char, Option<Seq>, Option<Seq>);

fn nary_head(e: &XEl, d: usize) -> Option<NaryHead> {
    if let Some(op) = nary_op(e) {
        return Some((op, None, None));
    }
    let kids: Vec<&XEl> = e.els().collect();
    let op = nary_op(kids.first()?)?;
    let a = |i: usize| kids.get(i).map(|k| read_arg(k, d)).unwrap_or_default();
    Some(match e.local() {
        "msub" | "munder" => (op, Some(a(1)), None),
        "msup" | "mover" => (op, None, Some(a(1))),
        "msubsup" | "munderover" => (op, Some(a(1)), Some(a(2))),
        _ => return None,
    })
}

fn read_arg(e: &XEl, d: usize) -> Seq {
    read_seq(vec![e], d + 1)
}

fn read_seq(els: Vec<&XEl>, d: usize) -> Seq {
    let mut out = Seq::new();
    if d > 2 * MAX_DEPTH + 16 {
        return out;
    }
    let mut v = Vec::new();
    expand(els, d, &mut v);
    let mut i = 0;
    while i < v.len() {
        let e = v[i];
        // Function application: name, U+2061, argument.
        if e.local() == "mi" && v.get(i + 1).is_some_and(|n| n.local() == "mo" && token_text(n) == "\u{2061}") {
            let body = v.get(i + 2).map(|b| read_arg(b, d)).unwrap_or_default();
            out.push(Node::Func { name: token_text(e), body });
            i += 3;
            continue;
        }
        if let Some((op, sub, sup)) = nary_head(e, d) {
            let body = v.get(i + 1).map(|b| read_arg(b, d)).unwrap_or_default();
            out.push(Node::Nary { op, sub, sup, body });
            i += 2;
            continue;
        }
        read_el(e, d, &mut out);
        i += 1;
    }
    out
}

fn variant_style(e: &XEl) -> Option<Option<Style>> {
    match attr(e, "mathvariant") {
        Some("normal") => Some(Some(Style::Plain)),
        Some("bold") => Some(Some(Style::Bold)),
        Some("bold-italic") => Some(Some(Style::BoldItalic)),
        Some(_) => Some(None),
        None => None,
    }
}

fn double_struck(c: char) -> char {
    match c {
        'C' => '\u{2102}',
        'H' => '\u{210D}',
        'N' => '\u{2115}',
        'P' => '\u{2119}',
        'Q' => '\u{211A}',
        'R' => '\u{211D}',
        'Z' => '\u{2124}',
        c => c,
    }
}

fn space_char(e: &XEl) -> Option<char> {
    let w = attr(e, "width")?;
    let em = match w {
        "veryverythinmathspace" => 1.0 / 18.0,
        "verythinmathspace" => 2.0 / 18.0,
        "thinmathspace" => 3.0 / 18.0,
        "mediummathspace" => 4.0 / 18.0,
        "thickmathspace" => 5.0 / 18.0,
        "verythickmathspace" => 6.0 / 18.0,
        "veryverythickmathspace" => 7.0 / 18.0,
        _ => {
            let (num, unit) = w.split_at(w.find(|c: char| c.is_ascii_alphabetic() || c == '%').unwrap_or(w.len()));
            let v: f32 = num.trim().parse().ok()?;
            match unit.trim() {
                "em" => v,
                "ex" => v * 0.5,
                "mu" => v / 18.0,
                "px" => v / 16.0,
                "pt" => v / 12.0,
                _ => return None,
            }
        }
    };
    if !(em.is_finite() && em > 0.02) {
        return None;
    }
    let mut best: Option<(char, f32)> = None;
    for (c, _, w) in SPACES {
        let diff = (w - em).abs();
        if best.is_none_or(|(_, b)| diff < b - 1e-4) {
            best = Some((*c, diff));
        }
    }
    best.map(|(c, _)| c)
}

fn is_delim_row(e: &XEl) -> bool {
    if e.local() != "mrow" {
        return false;
    }
    let kids: Vec<&XEl> = e.els().collect();
    let (Some(first), Some(last)) = (kids.first(), kids.last()) else { return false };
    if kids.len() < 2 || first.local() != "mo" || last.local() != "mo" {
        return false;
    }
    let fence = |m: &XEl, set: &[&str]| is_true(m, "fence") || (attr(m, "stretchy") != Some("false") && set.contains(&token_text(m).as_str()));
    fence(first, OPEN_FENCES) && fence(last, CLOSE_FENCES)
}

/// `(open, close, sep, items)` from the children between the fences.
fn delim(open: String, close: String, inner: &[&XEl], seps: Option<&str>, d: usize) -> Node {
    let mut items: Vec<Seq> = Vec::new();
    let mut cur: Vec<&XEl> = Vec::new();
    let mut sep: Option<String> = None;
    for k in inner {
        let is_sep =
            k.local() == "mo" && (is_true(k, "separator") || seps.is_some_and(|s| s.contains(token_text(k).as_str()) && !token_text(k).is_empty()));
        if is_sep {
            sep.get_or_insert_with(|| token_text(k));
            items.push(read_seq(std::mem::take(&mut cur), d + 1));
        } else {
            cur.push(k);
        }
    }
    items.push(read_seq(cur, d + 1));
    Node::Delim { open, close, sep: sep.unwrap_or_else(|| "|".into()), items }
}

fn read_el(e: &XEl, d: usize, out: &mut Seq) {
    let kids: Vec<&XEl> = e.els().collect();
    let a = |i: usize| kids.get(i).map(|k| read_arg(k, d)).unwrap_or_default();
    // A single `mo` script mark: (char, stretchy attribute).
    let mark = |i: usize| -> Option<(char, Option<&str>)> {
        let k = kids.get(i)?;
        if k.local() != "mo" {
            return None;
        }
        single_char(&token_text(k)).map(|c| (c, attr(k, "stretchy")))
    };
    match e.local() {
        "mi" | "mn" | "mo" => {
            let t = token_text(e);
            if t.is_empty() || (e.local() == "mo" && matches!(t.as_str(), "\u{2061}" | "\u{2062}" | "\u{2063}" | "\u{2064}")) {
                return;
            }
            let style = match variant_style(e) {
                Some(s) => s,
                None if e.local() == "mi" && t.chars().count() > 1 => Some(Style::Plain),
                None => None,
            };
            if attr(e, "mathvariant") == Some("double-struck") {
                return push(out, Node::Text(t.chars().map(double_struck).collect()));
            }
            match style {
                Some(style) => push(out, Node::Styled { text: t, style }),
                None => push(out, Node::Text(t)),
            }
        }
        "mtext" | "ms" => {
            let raw = e.text();
            let t = raw.trim_matches(|c: char| matches!(c, '\t' | '\n' | '\r'));
            let lead = t.chars().take_while(|&c| c == '\u{00A0}').count();
            let trail = if t.chars().all(|c| c == '\u{00A0}') { 0 } else { t.chars().rev().take_while(|&c| c == '\u{00A0}').count() };
            let core: String = t.chars().skip(lead).take(t.chars().count().saturating_sub(lead + trail)).collect();
            let s = format!("{}{}{}", " ".repeat(lead), core, " ".repeat(trail));
            if !s.is_empty() {
                push(out, Node::Styled { text: s, style: Style::Normal });
            }
        }
        "mspace" => {
            if matches!(attr(e, "linebreak"), Some("newline" | "indentingnewline")) {
                out.push(Node::Raw(OMATH_BREAK.to_string()));
            } else if let Some(c) = space_char(e) {
                push(out, Node::Text(c.to_string()));
            }
        }
        "mrow" if is_delim_row(e) => {
            let inner = &kids[1..kids.len().saturating_sub(1).max(1)];
            let (Some(f), Some(l)) = (kids.first(), kids.last()) else { return };
            out.push(delim(token_text(f), token_text(l), inner, None, d));
        }
        "mfenced" => {
            let open = attr(e, "open").unwrap_or("(").to_string();
            let close = attr(e, "close").unwrap_or(")").to_string();
            let seps: String = attr(e, "separators").unwrap_or(",").chars().filter(|c| !c.is_whitespace()).collect();
            let mut items: Vec<Seq> = kids.iter().map(|k| read_arg(k, d)).collect();
            if items.is_empty() {
                items.push(Seq::new());
            }
            let sep = seps.chars().next().map_or_else(|| "|".to_string(), |c| c.to_string());
            out.push(Node::Delim { open, close, sep, items });
        }
        "mfrac" => {
            let lt = attr(e, "linethickness").unwrap_or("");
            if matches!(lt, "0" | "0px" | "0em" | "0pt" | "0.0") {
                out.push(Node::Matrix { rows: vec![vec![a(0)], vec![a(1)]] });
            } else {
                out.push(Node::Frac { num: a(0), den: a(1) });
            }
        }
        "msqrt" => out.push(Node::Rad { deg: None, body: read_seq(kids, d + 1) }),
        "mroot" => out.push(Node::Rad { deg: Some(a(1)), body: a(0) }),
        "msub" => out.push(Node::Script { base: a(0), sub: Some(a(1)), sup: None }),
        "msup" => out.push(Node::Script { base: a(0), sub: None, sup: Some(a(1)) }),
        "msubsup" => out.push(Node::Script { base: a(0), sub: Some(a(1)), sup: Some(a(2)) }),
        "mover" => {
            let base = a(0);
            let n = match mark(1) {
                Some(('\u{00AF}' | '\u{203E}' | '\u{0305}', Some("false"))) => Node::Accent { ch: '\u{0305}', body: base },
                Some(('\u{00AF}' | '\u{203E}' | '\u{0305}', _)) => Node::Bar { top: true, body: base },
                Some((c, Some("true"))) => Node::GroupChr { ch: c, top: true, body: base },
                Some((c, _)) if is_true(e, "accent") || ACCENT_MARKS.iter().any(|(m, s)| *m == c || *s == c) => {
                    let ch = ACCENT_MARKS.iter().find(|(_, s)| *s == c).map_or(c, |(m, _)| *m);
                    Node::Accent { ch, body: base }
                }
                Some((c, _)) if GROUP_CHARS.contains(&c) => Node::GroupChr { ch: c, top: true, body: base },
                _ => Node::Limit { lower: false, base, lim: a(1) },
            };
            out.push(n);
        }
        "munder" => {
            let base = a(0);
            let n = match mark(1) {
                Some(('_' | '\u{0332}' | '\u{00AF}' | '\u{203E}', _)) => Node::Bar { top: false, body: base },
                Some((c, Some("true"))) => Node::GroupChr { ch: c, top: false, body: base },
                Some((c, _)) if GROUP_CHARS.contains(&c) => Node::GroupChr { ch: c, top: false, body: base },
                _ => Node::Limit { lower: true, base, lim: a(1) },
            };
            out.push(n);
        }
        "munderover" => {
            let inner = Node::Limit { lower: true, base: a(0), lim: a(1) };
            out.push(Node::Limit { lower: false, base: vec![inner], lim: a(2) });
        }
        "mmultiscripts" => {
            let base = a(0);
            let mut post: Vec<(Seq, Seq)> = Vec::new();
            let mut pre: Vec<(Seq, Seq)> = Vec::new();
            let mut in_pre = false;
            let mut pending: Option<Seq> = None;
            for k in kids.iter().skip(1) {
                if k.local() == "mprescripts" {
                    in_pre = true;
                    pending = None;
                    continue;
                }
                let s = if k.local() == "none" { Seq::new() } else { read_arg(k, d) };
                match pending.take() {
                    None => pending = Some(s),
                    Some(sub) => {
                        if in_pre {
                            pre.push((sub, s));
                        } else {
                            post.push((sub, s));
                        }
                    }
                }
            }
            let mut n = match pre.into_iter().next() {
                Some((sub, sup)) => vec![Node::PreScript { sub, sup, base }],
                None => base,
            };
            if let Some((sub, sup)) = post.into_iter().next() {
                let sub = (!sub.is_empty()).then_some(sub);
                let sup = (!sup.is_empty()).then_some(sup);
                if sub.is_some() || sup.is_some() {
                    n = vec![Node::Script { base: n, sub, sup }];
                }
            }
            for x in n {
                push(out, x);
            }
        }
        "mtable" => {
            let eqarr = attr(e, "columnalign").is_some_and(|c| c.starts_with("left"));
            let rows: Vec<Vec<Seq>> = kids
                .iter()
                .filter(|r| matches!(r.local(), "mtr" | "mlabeledtr"))
                .map(|r| {
                    let skip = usize::from(r.local() == "mlabeledtr");
                    r.els().filter(|c| c.local() == "mtd").skip(skip).map(|c| read_seq(c.els().collect(), d + 1)).collect()
                })
                .collect();
            if eqarr {
                let rows = rows
                    .into_iter()
                    .map(|cells| {
                        let mut row = Seq::new();
                        for (i, c) in cells.into_iter().enumerate() {
                            if i > 0 {
                                push(&mut row, Node::Text("\u{2003}".into()));
                            }
                            for n in c {
                                push(&mut row, n);
                            }
                        }
                        row
                    })
                    .collect();
                out.push(Node::EqArr { rows });
            } else {
                out.push(Node::Matrix { rows });
            }
        }
        "menclose" => {
            let notation = attr(e, "notation").unwrap_or("longdiv");
            let body = read_seq(kids, d + 1);
            let n = if notation.split_whitespace().any(|w| matches!(w, "box" | "roundedbox" | "circle")) {
                Node::Boxed { border: true, body }
            } else if notation.trim() == "top" {
                Node::Bar { top: true, body }
            } else if notation.trim() == "bottom" {
                Node::Bar { top: false, body }
            } else {
                for n in body {
                    push(out, n);
                }
                return;
            };
            out.push(n);
        }
        _ if is_group(e) => {
            for n in read_seq(kids, d + 1) {
                push(out, n);
            }
        }
        "mpadded" => out.push(Node::Boxed { border: false, body: read_seq(kids, d + 1) }),
        _ => {
            if kids.is_empty() {
                let t = token_text(e);
                if !t.is_empty() {
                    push(out, Node::Text(t));
                }
            } else {
                for n in read_seq(kids, d + 1) {
                    push(out, n);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::from_linear;

    fn ml(latex: &str) -> String {
        to_mathml(&from_linear(latex))
    }

    #[test]
    fn root_and_header() {
        let mut m = from_linear(r"\frac{a}{b}");
        m.para = true;
        let x = to_mathml(&m);
        assert!(x.starts_with("<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"block\"><mfrac>"), "{x}");
        assert!(to_mathml_display(&m, false).starts_with("<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"inline\">"));
        assert_eq!(x, "<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"block\"><mfrac><mi>a</mi><mi>b</mi></mfrac></math>");
    }

    #[test]
    fn tokens_are_classified() {
        let x = ml(r"x^2+3.14\alpha-\sin x");
        assert!(x.contains("<msup><mi>x</mi><mn>2</mn></msup><mo>+</mo><mn>3.14</mn><mi>α</mi><mo>−</mo>"), "{x}");
        assert!(x.contains("<mi>sin</mi><mo>&#x2061;</mo><mi>x</mi>"), "{x}");
        assert!(ml(r"\text{if } x").contains("<mtext>if\u{a0}</mtext>"));
        assert!(ml(r"a\,b").contains("<mspace width=\"0.1667em\"/>"));
        assert!(ml("a<b&c>d").contains("<mo>&#60;</mo>"));
    }

    #[test]
    fn structures_use_the_expected_elements() {
        for (src, want) in [
            (r"\sqrt{x}", "<msqrt><mi>x</mi></msqrt>"),
            (r"\sqrt[3]{x}", "<mroot><mi>x</mi><mn>3</mn></mroot>"),
            (r"x_i^2", "<msubsup><mi>x</mi><mi>i</mi><mn>2</mn></msubsup>"),
            (r"\sum_{i=1}^{n}i", "<munderover><mo>∑</mo><mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow><mi>n</mi></munderover><mi>i</mi>"),
            (r"\int_0^1 x", "<msubsup><mo>∫</mo><mn>0</mn><mn>1</mn></msubsup><mi>x</mi>"),
            (r"\lim_{x\to0}x", "<mrow><munder><mi>lim</mi><mrow><mi>x</mi><mo>→</mo><mn>0</mn></mrow></munder><mo>&#x2061;</mo></mrow><mi>x</mi>"),
            (r"\hat{x}", "<mover accent=\"true\"><mi>x</mi><mo stretchy=\"false\">\u{302}</mo></mover>"),
            (r"\overline{x}", "<mover accent=\"true\"><mi>x</mi><mo stretchy=\"true\">¯</mo></mover>"),
            (r"\overrightarrow{x}", "<mrow><mover><mi>x</mi><mo stretchy=\"true\">→</mo></mover><mo>&#x2061;</mo></mrow>"),
            (r"\overbrace{a}^{3}", "<mrow><mover><mover><mi>a</mi><mo stretchy=\"true\">⏞</mo></mover><mn>3</mn></mover><mo>&#x2061;</mo></mrow>"),
            (r"\left(x\right)", "<mrow><mo fence=\"true\" stretchy=\"true\">(</mo><mi>x</mi><mo fence=\"true\" stretchy=\"true\">)</mo></mrow>"),
            (r"\begin{pmatrix}a&b\end{pmatrix}", "<mtable><mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr></mtable>"),
            (
                r"\begin{cases}1&x>0\end{cases}",
                "<mtable columnalign=\"left\"><mtr><mtd><mn>1</mn></mtd><mtd><mi>x</mi><mo>&#62;</mo><mn>0</mn></mtd></mtr></mtable>",
            ),
            (r"\boxed{x}", "<menclose notation=\"box\"><mi>x</mi></menclose>"),
            (r"\mathbf{v}", "<mi mathvariant=\"bold\">v</mi>"),
            (r"\mathrm{e}", "<mi mathvariant=\"normal\">e</mi>"),
        ] {
            let x = ml(src);
            assert!(x.contains(want), "{src}: {x}");
            assert!(!x.contains("mfenced"), "{src}: {x}");
        }
    }

    #[test]
    fn reads_foreign_mathml() {
        let r = |s: &str| from_mathml(s).map(|m| crate::to_latex(&m)).unwrap_or_else(|e| format!("ERR {e}"));
        assert_eq!(r(r#"<math><mfrac><mn>1</mn><mn>2</mn></mfrac></math>"#), r"\frac{1}{2}");
        assert_eq!(
            r(
                r#"<?xml version="1.0"?><mml:math xmlns:mml="http://www.w3.org/1998/Math/MathML"><mml:msup><mml:mi>x</mml:mi><mml:mn>2</mml:mn></mml:msup></mml:math>"#
            ),
            "x^2"
        );
        assert_eq!(r(r#"<math><mfenced><mi>a</mi><mi>b</mi></mfenced></math>"#), r"\left(a,b\right)");
        assert_eq!(r(r#"<math><mrow><mo>(</mo><mi>x</mi><mo>)</mo></mrow></math>"#), r"\left(x\right)");
        assert_eq!(r(r#"<math><semantics><mrow><mi>x</mi></mrow><annotation encoding="TeX">x</annotation></semantics></math>"#), "x");
        assert_eq!(r(r#"<html><body><!--StartFragment--><math display="block"><mi>y</mi></math><!--EndFragment--></body></html>"#), "y");
        assert_eq!(r(r#"<math><mrow><mo>(</mo><mfrac linethickness="0"><mi>n</mi><mi>k</mi></mfrac><mo>)</mo></mrow></math>"#), r"\binom{n}{k}");
        assert_eq!(from_mathml("  "), Err(MathmlError::Empty));
        assert_eq!(from_mathml("<p>no</p>"), Err(MathmlError::NoMath));
        assert!(from_mathml("<math display=\"block\"><mi>x</mi></math>").is_ok_and(|m| m.para));
    }

    #[test]
    fn hostile_input_is_bounded() {
        let h = std::thread::Builder::new().stack_size(1 << 20).spawn(|| {
            let cases = vec![
                format!("<math>{}", "<mfrac><mrow>".repeat(20_000)),
                format!("<math>{}", "<mrow>".repeat(50_000)),
                format!("<math>{}</math>", "<mi>x</mi>".repeat(100_000)),
                "<math><mi>&bogus;&#xFFFFFFFF;&#0;</mi></math>".to_string(),
                "<!DOCTYPE math [<!ENTITY a \"aaaaaaaaaa\"><!ENTITY b \"&a;&a;&a;&a;\">]><math><mi>&b;</mi></math>".to_string(),
                "<math><mmultiscripts><mprescripts/><mprescripts/></mmultiscripts><mtable><mtr></mtr><mlabeledtr/></mtable><mfenced separators=''/></math>".to_string(),
                "<math><mspace width=\"NaNem\"/><mspace width=\"-1em\"/><mspace width=\"1e40em\"/><mroot/><msubsup/><munderover/></math>".to_string(),
                "<math".to_string(),
                "<math></math>".to_string(),
                "<<<math>>>&&&</math".to_string(),
                "x".repeat(MAX_MATHML + 1),
            ];
            for c in &cases {
                if let Ok(m) = from_mathml(c) {
                    let _ = to_mathml(&m);
                }
            }
            assert_eq!(from_mathml(&"x".repeat(MAX_MATHML + 1)), Err(MathmlError::TooLarge));
        });
        assert!(h.is_ok_and(|h| h.join().is_ok()), "overflowed or panicked on a 1 MB stack");
    }
}
