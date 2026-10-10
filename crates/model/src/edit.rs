//! Text editing on a [`TextBody`]: positions are (paragraph, character offset). Inserting text
//! with `\n` splits paragraphs; formatting a range splits runs at its ends.

use crate::text::{Paragraph, Run, RunKind, RunProps, TextBody};

/// (paragraph, char offset in the paragraph).
pub type Pos = (usize, usize);

fn clamp(body: &TextBody, p: Pos) -> Pos {
    if body.paragraphs.is_empty() {
        return (0, 0);
    }
    let pi = p.0.min(body.paragraphs.len() - 1);
    let n = body.paragraphs.get(pi).map(Paragraph::char_len).unwrap_or(0);
    (pi, p.1.min(n))
}

fn order(a: Pos, b: Pos) -> (Pos, Pos) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Split the paragraph's runs so a run boundary falls at char `at`; returns the index of the
/// first run starting at or after `at`.
fn split_at(p: &mut Paragraph, at: usize) -> usize {
    let mut pos = 0;
    let mut i = 0;
    while i < p.runs.len() {
        let len = p.runs.get(i).map(Run::char_len).unwrap_or(0);
        if pos == at {
            return i;
        }
        if at < pos + len {
            let Some(r) = p.runs.get(i) else { return i };
            if r.kind != RunKind::Text {
                return i + 1;
            }
            let k = at - pos;
            let byte = r.text.char_indices().nth(k).map(|(b, _)| b).unwrap_or(r.text.len());
            let (a, b) = r.text.split_at(byte);
            let second = Run { text: b.to_string(), props: r.props.clone(), kind: RunKind::Text };
            let first = a.to_string();
            if let Some(r) = p.runs.get_mut(i) {
                r.text = first;
            }
            p.runs.insert(i + 1, second);
            return i + 1;
        }
        pos += len;
        i += 1;
    }
    p.runs.len()
}

/// The char span `(start, end)` of the `Math` run that strictly contains offset `at` (the offset
/// falls inside the equation, not on one of its edges). An equation is an atom: edits never cut it.
fn math_span(p: &Paragraph, at: usize) -> Option<(usize, usize)> {
    let mut pos = 0;
    for r in &p.runs {
        let len = r.char_len();
        if matches!(r.kind, RunKind::Math { .. }) && pos < at && at < pos + len {
            return Some((pos, pos + len));
        }
        pos += len;
    }
    None
}

/// The span `(start, end)` of the `Math` run that ends at (`forward` false) or starts at
/// (`forward` true) offset `at`: word motion treats an equation as a word of its own.
fn math_adjacent(p: &Paragraph, at: usize, forward: bool) -> Option<(usize, usize)> {
    let mut pos = 0;
    for r in &p.runs {
        let len = r.char_len();
        if matches!(r.kind, RunKind::Math { .. }) && len > 0 && ((forward && pos == at) || (!forward && pos + len == at)) {
            return Some((pos, pos + len));
        }
        pos += len;
    }
    None
}

/// Move `at` out of an equation: to its start (`forward` false) or its end (`forward` true).
fn snap_math(body: &TextBody, p: Pos, forward: bool) -> Pos {
    match body.paragraphs.get(p.0).and_then(|para| math_span(para, p.1)) {
        Some((a, b)) => (p.0, if forward { b } else { a }),
        None => p,
    }
}

/// The explicit run properties in effect at `pos` (the run before the caret, or the end mark).
pub fn props_at(body: &TextBody, pos: Pos) -> RunProps {
    let (pi, ch) = clamp(body, pos);
    let Some(p) = body.paragraphs.get(pi) else { return RunProps::default() };
    let mut acc = 0;
    let mut last = None;
    for r in &p.runs {
        let len = r.char_len();
        if ch <= acc + len && ch > acc {
            return r.props.clone();
        }
        if acc + len >= ch && ch == 0 {
            return r.props.clone();
        }
        acc += len;
        last = Some(&r.props);
    }
    last.cloned().unwrap_or_else(|| p.end_props.clone())
}

/// Insert `text` at `pos` (with `props`, else the props at `pos`). `\n` starts a new paragraph
/// (same level and paragraph props); `\u{b}` inserts a line break. Returns the caret after it.
pub fn insert(body: &mut TextBody, pos: Pos, text: &str, props: Option<RunProps>) -> Pos {
    if body.paragraphs.is_empty() {
        body.paragraphs.push(Paragraph::default());
    }
    let (mut pi, mut ch) = snap_math(body, clamp(body, pos), true);
    let props = props.unwrap_or_else(|| props_at(body, (pi, ch)));
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut first = true;
    for part in text.split('\n') {
        if !first {
            // Split paragraph pi at ch.
            let Some(p) = body.paragraphs.get_mut(pi) else { break };
            let idx = split_at(p, ch);
            let tail: Vec<Run> = p.runs.drain(idx..).collect();
            let newp = Paragraph { level: p.level, props: p.props.clone(), runs: tail, end_props: p.end_props.clone() };
            p.end_props = props.clone();
            body.paragraphs.insert(pi + 1, newp);
            pi += 1;
            ch = 0;
        }
        first = false;
        for (k, seg) in part.split('\u{b}').enumerate() {
            let Some(p) = body.paragraphs.get_mut(pi) else { break };
            if k > 0 {
                let idx = split_at(p, ch);
                p.runs.insert(idx, Run { text: String::new(), props: props.clone(), kind: RunKind::Break });
                ch += 1;
            }
            if seg.is_empty() {
                continue;
            }
            let idx = split_at(p, ch);
            p.runs.insert(idx, Run::with(seg, props.clone()));
            ch += seg.chars().count();
        }
        if let Some(p) = body.paragraphs.get_mut(pi) {
            p.normalize();
            if p.runs.is_empty() {
                p.end_props = props.clone();
            }
        }
    }
    (pi, ch)
}

/// Delete the range between `a` and `b`; returns the caret at the start.
pub fn delete(body: &mut TextBody, a: Pos, b: Pos) -> Pos {
    if body.paragraphs.is_empty() {
        return (0, 0);
    }
    let (s, e) = order(clamp(body, a), clamp(body, b));
    if s == e {
        return s;
    }
    // A range that touches an equation takes the whole equation with it.
    let (s, e) = (snap_math(body, s, false), snap_math(body, e, true));
    if s.0 == e.0 {
        if let Some(p) = body.paragraphs.get_mut(s.0) {
            let end_props = props_of_range(p, s.1);
            let i0 = split_at(p, s.1);
            let i1 = split_at(p, e.1);
            if i1 > i0 {
                p.runs.drain(i0..i1);
            }
            p.normalize();
            if p.runs.is_empty() {
                p.end_props = end_props;
            }
        }
        return s;
    }
    // Keep the head of s.0 and the tail of e.0, joined; drop paragraphs between.
    let tail: Vec<Run> = match body.paragraphs.get_mut(e.0) {
        Some(p) => {
            let i = split_at(p, e.1);
            p.runs.drain(i..).collect()
        }
        None => vec![],
    };
    if let Some(p) = body.paragraphs.get_mut(s.0) {
        let i = split_at(p, s.1);
        p.runs.truncate(i);
        p.runs.extend(tail);
        p.normalize();
    }
    if e.0 > s.0 && e.0 < body.paragraphs.len() {
        body.paragraphs.drain(s.0 + 1..=e.0);
    }
    s
}

fn props_of_range(p: &Paragraph, at: usize) -> RunProps {
    let mut acc = 0;
    for r in &p.runs {
        let len = r.char_len();
        if at < acc + len || (at == acc + len && at > 0) {
            return r.props.clone();
        }
        acc += len;
    }
    p.runs.last().map(|r| r.props.clone()).unwrap_or_else(|| p.end_props.clone())
}

/// Apply `f` to the run properties of every run in [a, b). An empty range changes the empty
/// paragraph's end mark (so typing there picks the formatting up).
pub fn format(body: &mut TextBody, a: Pos, b: Pos, f: &dyn Fn(&mut RunProps)) {
    if body.paragraphs.is_empty() {
        return;
    }
    let (s, e) = order(clamp(body, a), clamp(body, b));
    let (s, e) = if s == e { (s, e) } else { (snap_math(body, s, false), snap_math(body, e, true)) };
    for pi in s.0..=e.0 {
        let Some(p) = body.paragraphs.get_mut(pi) else { continue };
        let from = if pi == s.0 { s.1 } else { 0 };
        let to = if pi == e.0 { e.1 } else { p.char_len() };
        if p.runs.is_empty() || (from == to && s == e) {
            f(&mut p.end_props);
            continue;
        }
        let i0 = split_at(p, from);
        let i1 = split_at(p, to);
        for r in p.runs.iter_mut().take(i1).skip(i0) {
            f(&mut r.props);
        }
        if to == p.char_len() {
            f(&mut p.end_props);
        }
        p.normalize();
    }
}

/// Apply `f` to every run in the body (and end marks).
pub fn format_all(body: &mut TextBody, f: &dyn Fn(&mut RunProps)) {
    for p in &mut body.paragraphs {
        for r in &mut p.runs {
            f(&mut r.props);
        }
        f(&mut p.end_props);
        p.normalize();
    }
}

/// The text between two positions (`\n` between paragraphs).
pub fn text_range(body: &TextBody, a: Pos, b: Pos) -> String {
    let (s, e) = order(clamp(body, a), clamp(body, b));
    // Like delete: a range touching an equation takes the whole equation.
    let (s, e) = if s == e { (s, e) } else { (snap_math(body, s, false), snap_math(body, e, true)) };
    let mut out = String::new();
    for pi in s.0..=e.0 {
        let Some(p) = body.paragraphs.get(pi) else { continue };
        let t: Vec<char> = p.text().chars().collect();
        let from = if pi == s.0 { s.1 } else { 0 };
        let to = if pi == e.0 { e.1 } else { t.len() };
        out.extend(t.get(from.min(t.len())..to.min(t.len())).unwrap_or(&[]));
        if pi != e.0 {
            out.push('\n');
        }
    }
    out
}

/// A formatted copy of the range.
pub fn slice(body: &TextBody, a: Pos, b: Pos) -> TextBody {
    let (s, e) = order(clamp(body, a), clamp(body, b));
    let (s, e) = if s == e { (s, e) } else { (snap_math(body, s, false), snap_math(body, e, true)) };
    let mut out = TextBody { body: body.body.clone(), list_style: body.list_style.clone(), paragraphs: vec![] };
    for pi in s.0..=e.0 {
        let Some(p) = body.paragraphs.get(pi) else { continue };
        let mut p = p.clone();
        let to = if pi == e.0 { e.1 } else { p.char_len() };
        let i1 = split_at(&mut p, to);
        p.runs.truncate(i1);
        let from = if pi == s.0 { s.1 } else { 0 };
        let i0 = split_at(&mut p, from);
        p.runs.drain(..i0);
        out.paragraphs.push(p);
    }
    out
}

/// Insert a formatted body at `pos` (paste): first paragraph merges into the current one.
pub fn insert_body(body: &mut TextBody, pos: Pos, src: &TextBody) -> Pos {
    let mut caret = clamp(body, pos);
    for (k, sp) in src.paragraphs.iter().enumerate() {
        if k > 0 {
            caret = insert(body, caret, "\n", None);
            if let Some(p) = body.paragraphs.get_mut(caret.0) {
                p.level = sp.level;
                p.props = sp.props.clone();
            }
        }
        for r in &sp.runs {
            match r.kind {
                RunKind::Text => caret = insert(body, caret, &r.text, Some(r.props.clone())),
                RunKind::Break => caret = insert(body, caret, "\u{b}", Some(r.props.clone())),
                _ => {
                    if let Some(p) = body.paragraphs.get_mut(caret.0) {
                        let idx = split_at(p, caret.1);
                        p.runs.insert(idx, r.clone());
                        caret.1 += r.char_len();
                    }
                }
            }
        }
    }
    caret
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

/// Word boundaries around `pos` (double-click selection).
pub fn word_at(body: &TextBody, pos: Pos) -> (Pos, Pos) {
    let (pi, ch) = clamp(body, pos);
    let t: Vec<char> = body.paragraphs.get(pi).map(|p| p.text().chars().collect()).unwrap_or_default();
    if t.is_empty() {
        return ((pi, 0), (pi, 0));
    }
    let at = ch.min(t.len().saturating_sub(1));
    let w = t.get(at).copied().is_some_and(is_word);
    let mut s = at;
    while s > 0 && t.get(s - 1).copied().is_some_and(|c| is_word(c) == w && !c.is_whitespace() || (w && is_word(c))) {
        s -= 1;
    }
    let mut e = at;
    while e < t.len() && t.get(e).copied().is_some_and(|c| is_word(c) == w && (w || !c.is_whitespace())) {
        e += 1;
    }
    // Include trailing spaces after a word.
    while w && e < t.len() && t.get(e) == Some(&' ') {
        e += 1;
    }
    ((pi, s), (pi, e.max(s)))
}

/// Next/previous word start (Option/Ctrl + arrows).
pub fn word_move(body: &TextBody, pos: Pos, forward: bool) -> Pos {
    let (pi, ch) = clamp(body, pos);
    let t: Vec<char> = body.paragraphs.get(pi).map(|p| p.text().chars().collect()).unwrap_or_default();
    // Word motion starting next to an equation steps over exactly that equation.
    if let Some(span) = body.paragraphs.get(pi).and_then(|p| math_adjacent(p, ch, forward)) {
        return (pi, if forward { span.1 } else { span.0 });
    }
    if forward {
        if ch >= t.len() {
            return if pi + 1 < body.paragraphs.len() { (pi + 1, 0) } else { (pi, ch) };
        }
        let mut i = ch;
        while i < t.len() && t.get(i).copied().is_some_and(is_word) {
            i += 1;
        }
        while i < t.len() && !t.get(i).copied().is_some_and(is_word) {
            i += 1;
        }
        (pi, i)
    } else {
        if ch == 0 {
            return if pi > 0 { (pi - 1, body.paragraphs.get(pi - 1).map(Paragraph::char_len).unwrap_or(0)) } else { (0, 0) };
        }
        let mut i = ch;
        while i > 0 && !t.get(i - 1).copied().is_some_and(is_word) {
            i -= 1;
        }
        while i > 0 && t.get(i - 1).copied().is_some_and(is_word) {
            i -= 1;
        }
        (pi, i)
    }
}

/// One character left/right across paragraph boundaries.
pub fn char_move(body: &TextBody, pos: Pos, forward: bool) -> Pos {
    // An equation is one step: from inside it the caret goes to the edge in the direction of
    // travel; onto it, the caret jumps across the whole equation.
    let from = clamp(body, pos);
    let snapped = snap_math(body, from, forward);
    if snapped != from {
        return snapped;
    }
    snap_math(body, char_move_raw(body, from, forward), forward)
}

fn char_move_raw(body: &TextBody, pos: Pos, forward: bool) -> Pos {
    let (pi, ch) = clamp(body, pos);
    let n = body.paragraphs.get(pi).map(Paragraph::char_len).unwrap_or(0);
    if forward {
        if ch < n {
            (pi, ch + 1)
        } else if pi + 1 < body.paragraphs.len() {
            (pi + 1, 0)
        } else {
            (pi, ch)
        }
    } else if ch > 0 {
        (pi, ch - 1)
    } else if pi > 0 {
        (pi - 1, body.paragraphs.get(pi - 1).map(Paragraph::char_len).unwrap_or(0))
    } else {
        (0, 0)
    }
}

/// End of the body.
pub fn end(body: &TextBody) -> Pos {
    let pi = body.paragraphs.len().saturating_sub(1);
    (pi, body.paragraphs.get(pi).map(Paragraph::char_len).unwrap_or(0))
}

/// Change case: `upper`, `lower`, `sentence`, `title`, `toggle`.
pub fn change_case(s: &str, mode: &str) -> String {
    match mode {
        "upper" => s.to_uppercase(),
        "lower" => s.to_lowercase(),
        "toggle" => s.chars().map(|c| if c.is_uppercase() { c.to_lowercase().collect::<String>() } else { c.to_uppercase().collect() }).collect(),
        "title" => {
            let mut out = String::new();
            let mut start = true;
            for c in s.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                    start = false;
                } else {
                    out.extend(c.to_lowercase());
                }
                if c.is_whitespace() {
                    start = true;
                }
            }
            out
        }
        _ => {
            // sentence
            let mut out = String::new();
            let mut start = true;
            for c in s.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                    start = false;
                } else {
                    out.extend(c.to_lowercase());
                }
                if matches!(c, '.' | '!' | '?') {
                    start = true;
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(t: &str) -> TextBody {
        TextBody::from_text(t)
    }

    #[test]
    fn insert_and_split() {
        let mut b = body("Hello");
        let c = insert(&mut b, (0, 5), " world\nNext", None);
        assert_eq!(b.text(), "Hello world\nNext");
        assert_eq!(c, (1, 4));
        let c = insert(&mut b, (0, 0), ">", None);
        assert_eq!(c, (0, 1));
        assert_eq!(b.paragraphs[0].runs.len(), 1);
    }

    #[test]
    fn delete_within_and_across() {
        let mut b = body("abc\ndef\nghi");
        assert_eq!(delete(&mut b, (0, 1), (2, 1)), (0, 1));
        assert_eq!(b.text(), "ahi");
        let mut b = body("abcdef");
        delete(&mut b, (0, 4), (0, 2));
        assert_eq!(b.text(), "abef");
        // hostile positions
        delete(&mut b, (99, 99), (0, 0));
        assert_eq!(b.text(), "");
    }

    #[test]
    fn format_splits_runs() {
        let mut b = body("abcdef");
        format(&mut b, (0, 2), (0, 4), &|p| p.bold = Some(true));
        let p = &b.paragraphs[0];
        assert_eq!(p.runs.len(), 3);
        assert_eq!(p.runs[1].text, "cd");
        assert_eq!(p.runs[1].props.bold, Some(true));
        assert_eq!(props_at(&b, (0, 3)).bold, Some(true));
        assert_eq!(text_range(&b, (0, 1), (0, 5)), "bcde");
        let s = slice(&b, (0, 1), (0, 5));
        assert_eq!(s.text(), "bcde");
        let mut c = body("XY");
        let caret = insert_body(&mut c, (0, 1), &s);
        assert_eq!(c.text(), "XbcdeY");
        assert_eq!(caret, (0, 5));
    }

    #[test]
    fn words_and_moves() {
        let b = body("Hello brave world");
        assert_eq!(word_at(&b, (0, 7)), ((0, 6), (0, 12)));
        assert_eq!(word_move(&b, (0, 0), true), (0, 6));
        assert_eq!(word_move(&b, (0, 8), false), (0, 6));
        assert_eq!(char_move(&b, (0, 17), true), (0, 17));
        assert_eq!(change_case("hello world. again", "sentence"), "Hello world. Again");
        assert_eq!(change_case("hello world", "title"), "Hello World");
    }

    #[test]
    fn line_break_insert() {
        let mut b = body("ab");
        let c = insert(&mut b, (0, 1), "\u{b}", None);
        assert_eq!(c, (0, 2));
        assert_eq!(b.paragraphs[0].char_len(), 3);
        assert_eq!(b.paragraphs.len(), 1);
    }

    /// "ab" + equation "x=1/2" (5 chars) + "cd".
    fn with_math() -> TextBody {
        let mut b = body("ab");
        if let Some(p) = b.paragraphs.get_mut(0) {
            p.runs.push(Run { text: "x=1/2".into(), props: RunProps::default(), kind: RunKind::Math { omml: "<m:oMath/>".into() } });
            p.runs.push(Run::new("cd"));
        }
        b
    }

    #[test]
    fn math_run_is_deleted_whole() {
        // Range starting inside, ending inside, and fully inside all take the whole equation.
        for (a, e) in [(4, 9), (1, 4), (3, 5), (4, 4 + 1)] {
            let mut b = with_math();
            let c = delete(&mut b, (0, a), (0, e));
            assert!(b.paragraphs[0].runs.iter().all(|r| r.kind == RunKind::Text), "{a}..{e}");
            assert_eq!(c.0, 0);
            assert!(c.1 <= 2, "{a}..{e} caret {c:?}");
        }
        // Backspace at the end of the equation (offset 7) removes it all.
        let mut b = with_math();
        let to = char_move(&b, (0, 7), false);
        assert_eq!(to, (0, 2));
        delete(&mut b, (0, 7), to);
        assert_eq!(b.text(), "abcd");
        // Forward delete at the start of the equation too.
        let mut b = with_math();
        let to = char_move(&b, (0, 2), true);
        assert_eq!(to, (0, 7));
        delete(&mut b, (0, 2), to);
        assert_eq!(b.text(), "abcd");
    }

    #[test]
    fn math_run_is_never_split_by_typing_or_formatting() {
        let mut b = with_math();
        let c = insert(&mut b, (0, 4), "Z", None);
        assert_eq!(c, (0, 8));
        assert_eq!(b.text(), "abx=1/2Zcd");
        assert!(b.paragraphs[0].runs.iter().any(|r| matches!(&r.kind, RunKind::Math { .. }) && r.text == "x=1/2"));
        // Paragraph split inside the equation lands after it.
        let mut b = with_math();
        insert(&mut b, (0, 3), "\n", None);
        assert_eq!(b.paragraphs[0].text(), "abx=1/2");
        assert_eq!(b.paragraphs[1].text(), "cd");
        // Caret moves across the equation in one step from either side.
        let b = with_math();
        assert_eq!(char_move(&b, (0, 4), true), (0, 7));
        assert_eq!(char_move(&b, (0, 4), false), (0, 2));
    }

    #[test]
    fn math_run_is_atomic_for_copy_cut_and_format() {
        // Partial selection inside the equation (offsets 4..5 of "ab|x=1/2|cd").
        let b = with_math();
        assert_eq!(text_range(&b, (0, 4), (0, 5)), "x=1/2");
        let s = slice(&b, (0, 4), (0, 5));
        assert_eq!(s.text(), "x=1/2");
        assert!(matches!(s.paragraphs[0].runs[0].kind, RunKind::Math { .. }));
        // Cut = copy + delete: the clipboard keeps the equation.
        let mut c = with_math();
        delete(&mut c, (0, 4), (0, 5));
        assert_eq!(c.text(), "abcd");
        // Paste puts it back.
        insert_body(&mut c, (0, 2), &s);
        assert_eq!(c.text(), "abx=1/2cd");
        assert!(c.paragraphs[0].runs.iter().any(|r| matches!(r.kind, RunKind::Math { .. })));
        // Formatting a partial selection formats the whole equation, never splits it.
        let mut f = with_math();
        format(&mut f, (0, 4), (0, 5), &|p| p.bold = Some(true));
        let m = f.paragraphs[0].runs.iter().find(|r| matches!(r.kind, RunKind::Math { .. }));
        assert!(m.is_some_and(|r| r.text == "x=1/2" && r.props.bold == Some(true)));
        assert_eq!(f.text(), "abx=1/2cd");
    }

    #[test]
    fn empty_math_run_can_be_deleted() {
        let mut b = body("ab");
        if let Some(p) = b.paragraphs.get_mut(0) {
            p.runs.push(Run { text: String::new(), props: RunProps::default(), kind: RunKind::Math { omml: "<m:oMath/>".into() } });
        }
        assert_eq!(b.paragraphs[0].char_len(), 3);
        delete(&mut b, (0, 0), (0, 3));
        assert!(b.paragraphs[0].runs.is_empty());
    }

    #[test]
    fn word_move_steps_over_adjacent_equation() {
        let b = with_math();
        assert_eq!(word_move(&b, (0, 7), false), (0, 2));
        assert_eq!(word_move(&b, (0, 2), true), (0, 7));
    }
}
