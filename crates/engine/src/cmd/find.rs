//! Home › Editing: Find and Replace across slide text, table cells and speaker notes.

use std::sync::Arc;

use deckcraft_model::text::RunKind;
use deckcraft_model::{Paragraph, Shape, ShapeKind, Slide, TextBody};
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, cmd, has_doc, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "text.find",
            "Find",
            ["Home", "Editing"],
            None,
            "{text, matchCase?: bool, wholeWords?: bool, includeHidden?: bool (default false: hidden slides and shapes are skipped)} → [{slide, shape (null = notes), cell?: [row, col], para, start, end}] (char offsets in the paragraph)",
            has_doc,
            find
        ),
        cmd!(
            "text.replace",
            "Replace",
            ["Home", "Editing"],
            None,
            "{text, with, matchCase?, wholeWords?, includeHidden?: bool (default false: hidden slides and shapes are skipped), at?: a text.find match (replace just that one; default: replace all)} → {replaced}",
            has_doc,
            replace
        ),
    ]
}

/// Where a text body lives on a slide.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Loc {
    shape: Option<u64>,
    cell: Option<(usize, usize)>,
}

/// Visit the visible text bodies on a slide: shapes depth-first (groups, table cells), then the
/// notes. Hidden shapes are skipped unless `include_hidden`, as `pptx` and the outline export do.
fn bodies(slide: &mut Slide, include_hidden: bool, f: &mut dyn FnMut(Loc, &mut TextBody)) {
    fn rec(shapes: &mut [Shape], depth: usize, include_hidden: bool, f: &mut dyn FnMut(Loc, &mut TextBody)) {
        if depth > 64 {
            return;
        }
        for sh in shapes {
            if sh.hidden && !include_hidden {
                continue;
            }
            let id = Some(u64::from(sh.id.0));
            if let Some(t) = &mut sh.text {
                f(Loc { shape: id, cell: None }, t);
            }
            match &mut sh.kind {
                ShapeKind::Table(t) => {
                    for (r, row) in t.rows.iter_mut().enumerate() {
                        for (c, cell) in row.cells.iter_mut().enumerate() {
                            f(Loc { shape: id, cell: Some((r, c)) }, &mut cell.text);
                        }
                    }
                }
                ShapeKind::Group { children, .. } => rec(children, depth + 1, include_hidden, f),
                _ => {}
            }
        }
    }
    rec(&mut slide.shapes, 0, include_hidden, f);
    f(Loc { shape: None, cell: None }, &mut slide.notes);
}

struct Opts {
    pat: Vec<char>,
    case: bool,
    whole: bool,
    include_hidden: bool,
}

impl Opts {
    fn from(p: &Value, cmd: &str) -> Result<Self> {
        let pat: Vec<char> = str_param(p, "text").unwrap_or("").chars().collect();
        if pat.is_empty() {
            return Err(bad(cmd, "missing `text`"));
        }
        Ok(Opts {
            pat,
            case: bool_or(p, "matchCase", false),
            whole: bool_or(p, "wholeWords", false),
            // Hidden slides and shapes are outside the show, so find and replace skip them
            // unless `includeHidden` — as the outline export does. The editor's dialog passes
            // `includeHidden: true`, where PowerPoint does search hidden slides.
            include_hidden: bool_or(p, "includeHidden", false),
        })
    }
    fn eq(&self, a: char, b: char) -> bool {
        a == b || (!self.case && a.to_lowercase().eq(b.to_lowercase()))
    }
    /// Non-overlapping matches as char ranges.
    fn matches(&self, text: &[char]) -> Vec<(usize, usize)> {
        let n = self.pat.len();
        let word = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric() || *c == '_');
        let mut out = vec![];
        let mut i = 0;
        while i + n <= text.len() {
            let hit = text[i..i + n].iter().zip(&self.pat).all(|(a, b)| self.eq(*a, *b))
                && (!self.whole || (!word(i.checked_sub(1).and_then(|j| text.get(j))) && !word(text.get(i + n))));
            if hit {
                out.push((i, i + n));
                i += n;
            } else {
                i += 1;
            }
        }
        out
    }
}

fn para_chars(p: &Paragraph) -> Vec<char> {
    p.runs.iter().flat_map(|r| r.content().chars()).collect()
}

/// Matches that lie wholly in plain text runs (never inside a field, equation or break).
fn para_matches(p: &Paragraph, o: &Opts) -> Vec<(usize, usize)> {
    let mut spans = vec![];
    let mut off = 0;
    for r in &p.runs {
        let n = r.char_len();
        spans.push((off, off + n, matches!(r.kind, RunKind::Text)));
        off += n;
    }
    o.matches(&para_chars(p)).into_iter().filter(|&(a, b)| spans.iter().all(|&(s, e, text)| text || e <= a || s >= b || s == e)).collect()
}

/// Replace chars `a..b` of the paragraph; the replacement takes the first run's formatting.
fn splice(p: &mut Paragraph, a: usize, b: usize, with: &str) {
    let mut off = 0;
    let mut placed = false;
    for r in &mut p.runs {
        let chars: Vec<char> = r.content().chars().collect();
        let (s, e) = (off, off + chars.len());
        off = e;
        if e <= a || s >= b {
            continue;
        }
        let keep_head: String = chars[..a.saturating_sub(s)].iter().collect();
        let keep_tail: String = chars[b.saturating_sub(s).min(chars.len())..].iter().collect();
        r.text = if placed { keep_tail } else { format!("{keep_head}{with}{keep_tail}") };
        placed = true;
    }
    if p.runs.len() > 1 {
        p.runs.retain(|r| !(r.text.is_empty() && matches!(r.kind, RunKind::Text)));
    }
}

fn find(s: &mut Session, p: &Value) -> Result<Value> {
    let o = Opts::from(p, "text.find")?;
    let st = s.doc()?;
    let mut out = vec![];
    for (i, sl) in st.doc.slides.iter().enumerate() {
        // A hidden slide is outside the show: skipped unless `includeHidden`.
        if sl.hidden && !o.include_hidden {
            continue;
        }
        let mut sl = (**sl).clone();
        bodies(&mut sl, o.include_hidden, &mut |loc, t| {
            for (pi, para) in t.paragraphs.iter().enumerate() {
                for (a, b) in para_matches(para, &o) {
                    out.push(json!({"slide": i, "shape": loc.shape, "cell": loc.cell.map(|(r, c)| [r, c]), "para": pi, "start": a, "end": b}));
                }
            }
        });
    }
    Ok(Value::Array(out))
}

fn replace(s: &mut Session, p: &Value) -> Result<Value> {
    let o = Opts::from(p, "text.replace")?;
    let with = str_param(p, "with").unwrap_or("").to_string();
    let at = p.get("at").filter(|v| v.is_object()).map(|v| -> Result<(usize, Loc, usize, usize)> {
        let n = |k: &str| v.get(k).and_then(Value::as_u64).map(|x| x as usize).ok_or_else(|| bad("text.replace", format!("`at` needs `{k}`")));
        let cell = v.get("cell").and_then(Value::as_array).and_then(|c| Some((c.first()?.as_u64()? as usize, c.get(1)?.as_u64()? as usize)));
        Ok((n("slide")?, Loc { shape: v.get("shape").and_then(Value::as_u64), cell }, n("para")?, n("start")?))
    });
    let at = at.transpose()?;
    let count = s.edit(|doc, _| {
        let mut count = 0;
        for (i, slp) in doc.slides.iter_mut().enumerate() {
            if at.is_some_and(|a| a.0 != i) {
                continue;
            }
            // A hidden slide is outside the show: skipped unless `includeHidden`.
            if slp.hidden && !o.include_hidden {
                continue;
            }
            let mut sl = (**slp).clone();
            let mut changed = 0;
            bodies(&mut sl, o.include_hidden, &mut |loc, t| {
                if at.is_some_and(|a| a.1 != loc) {
                    return;
                }
                for (pi, para) in t.paragraphs.iter_mut().enumerate() {
                    if at.is_some_and(|a| a.2 != pi) {
                        continue;
                    }
                    for (a, b) in para_matches(para, &o).into_iter().rev() {
                        if at.is_some_and(|x| x.3 != a) {
                            continue;
                        }
                        splice(para, a, b, &with);
                        changed += 1;
                    }
                }
            });
            if changed > 0 {
                *slp = Arc::new(sl);
                count += changed;
            }
        }
        if count == 0 {
            return Err(bad("text.replace", "no matches"));
        }
        Ok(count)
    })?;
    Ok(json!({"replaced": count}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::{Run, RunProps};

    fn opts(t: &str, case: bool, whole: bool) -> Opts {
        Opts { pat: t.chars().collect(), case, whole, include_hidden: false }
    }

    #[test]
    fn matching_honours_case_and_whole_words() {
        let t: Vec<char> = "Cat cat category".chars().collect();
        assert_eq!(opts("cat", false, false).matches(&t).len(), 3);
        assert_eq!(opts("cat", true, false).matches(&t).len(), 2);
        assert_eq!(opts("cat", false, true).matches(&t), vec![(0, 3), (4, 7)]);
    }

    fn text_run(t: &str) -> Run {
        Run { text: t.into(), props: RunProps::default(), kind: RunKind::Text }
    }

    fn br() -> Run {
        Run { text: String::new(), props: RunProps::default(), kind: RunKind::Break }
    }

    #[test]
    fn a_match_never_spans_a_line_break() {
        let p = Paragraph { runs: vec![text_run("a"), br(), text_run("b")], ..Default::default() };
        assert!(para_matches(&p, &opts("ab", true, false)).is_empty());
    }

    #[test]
    fn offsets_count_a_line_break_as_one_char() {
        let p = Paragraph { runs: vec![text_run("ab"), br(), text_run("cd")], ..Default::default() };
        assert_eq!(para_matches(&p, &opts("cd", true, false)), vec![(3, 5)]);
    }

    #[test]
    fn splice_after_a_line_break_keeps_the_break() {
        let mut p = Paragraph { runs: vec![text_run("ab"), br(), text_run("cd")], ..Default::default() };
        splice(&mut p, 3, 5, "X");
        assert_eq!(p.text(), "ab\u{b}X");
        assert_eq!(p.runs[1].kind, RunKind::Break);
    }

    #[test]
    fn splice_across_runs_keeps_the_first_runs_format() {
        let bold = RunProps { bold: Some(true), ..Default::default() };
        let mut p = Paragraph {
            runs: vec![
                Run { text: "Hel".into(), props: bold.clone(), kind: RunKind::Text },
                Run { text: "lo world".into(), props: RunProps::default(), kind: RunKind::Text },
            ],
            ..Default::default()
        };
        splice(&mut p, 1, 5, "ola");
        assert_eq!(para_chars(&p).iter().collect::<String>(), "Hola world");
        assert_eq!(p.runs[0].text, "Hola");
        assert_eq!(p.runs[0].props, bold);
    }

    /// Hidden slides and shapes are outside the show: find and replace skip them unless
    /// `includeHidden` — on the engine command and the editor dialog alike.
    #[test]
    fn find_and_replace_skip_hidden_content_unless_included() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"title": "Shown"})).unwrap();
        s.execute("insert.textBox", &json!({"rect": [10, 10, 100, 40], "text": "Secret"})).unwrap();
        let shape_id = s.doc().unwrap().selection.shapes[0].0;
        s.execute("slide.new", &json!({"title": "Hidden deck"})).unwrap();
        let last = s.doc().unwrap().doc.slides.len().saturating_sub(1);
        s.execute("slide.hide", &json!({"index": last})).unwrap();
        // Hide the text box through the model (the engine hides shapes through the selection).
        let st = s.doc_mut().unwrap();
        let doc = Arc::make_mut(&mut st.doc);
        for sl in doc.slides.iter_mut() {
            if let Some(sh) = Arc::make_mut(sl).shapes.iter_mut().find(|sh| sh.id.0 == shape_id) {
                sh.hidden = true;
            }
        }

        // Default scope: hidden slides and shapes are outside the find.
        let hits = s.execute("text.find", &json!({"text": "ecret"})).unwrap();
        assert!(hits.as_array().is_some_and(Vec::is_empty), "{hits}");
        let hits = s.execute("text.find", &json!({"text": "idden deck"})).unwrap();
        assert!(hits.as_array().is_some_and(Vec::is_empty), "{hits}");
        // `includeHidden: true` finds the hidden shape text and the hidden slide title.
        let hits = s.execute("text.find", &json!({"text": "ecret", "includeHidden": true})).unwrap();
        assert_eq!(hits.as_array().map(Vec::len), Some(1), "{hits}");
        let hits = s.execute("text.find", &json!({"text": "idden deck", "includeHidden": true})).unwrap();
        assert_eq!(hits.as_array().map(Vec::len), Some(1), "{hits}");

        // Replace follows the same scope: nothing replaced by default, replaced with the flag.
        assert!(s.execute("text.replace", &json!({"text": "ecret", "with": "X"})).is_err());
        let out = s.execute("text.replace", &json!({"text": "ecret", "with": "X", "includeHidden": true})).unwrap();
        assert_eq!(out.get("replaced").and_then(Value::as_u64), Some(1), "{out}");
        let hits = s.execute("text.find", &json!({"text": "X", "includeHidden": true})).unwrap();
        assert_eq!(hits.as_array().map(Vec::len), Some(1), "{hits}");
    }
}
