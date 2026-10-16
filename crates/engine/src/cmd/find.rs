//! Home › Editing: Find and Replace across slide text, table cells and speaker notes.

use std::sync::Arc;

use deckcraft_model::text::RunKind;
use deckcraft_model::{Paragraph, Shape, ShapeKind, Slide, TextBody};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "text.find",
            "Find",
            ["Home", "Editing"],
            None,
            "{text, matchCase?: bool, wholeWords?: bool} → [{slide, shape (null = notes), cell?: [row, col], para, start, end}] (char offsets in the paragraph)",
            has_doc,
            find
        ),
        cmd!(
            "text.replace",
            "Replace",
            ["Home", "Editing"],
            None,
            "{text, with, matchCase?, wholeWords?, at?: a text.find match (replace just that one; default: replace all)} → {replaced}",
            has_doc,
            replace
        ),
    ]
}

/// Where a text body lives on a slide.
#[derive(Clone, PartialEq, Debug)]
struct Loc {
    shape: Option<u64>,
    cell: Option<(usize, usize)>,
    path: deckcraft_model::table::TablePath,
    block: Option<u32>,
}

/// Visit every text body on a slide: shapes depth-first (groups, table cells), then the notes.
fn bodies(slide: &mut Slide, f: &mut dyn FnMut(Loc, &mut TextBody) -> Result<bool>) -> Result<()> {
    fn rec(shapes: &mut [Shape], depth: usize, f: &mut dyn FnMut(Loc, &mut TextBody) -> Result<bool>) -> Result<()> {
        if depth > 64 {
            return Err(bad("text", "shape nesting exceeds text visitor limit"));
        }
        for sh in shapes {
            let id = Some(u64::from(sh.id.0));
            if let Some(t) = &mut sh.text {
                f(Loc { shape: id, cell: None, path: Default::default(), block: None }, t)?;
            }
            match &mut sh.kind {
                ShapeKind::Table(t) => {
                    t.validate_content()?;
                    table_bodies(t, id, &Default::default(), f)?;
                }
                ShapeKind::Group { children, .. } => rec(children, depth + 1, f)?,
                _ => {}
            }
        }
        Ok(())
    }
    rec(&mut slide.shapes, 0, f)?;
    f(Loc { shape: None, cell: None, path: Default::default(), block: None }, &mut slide.notes)?;
    Ok(())
}

fn table_bodies(
    table: &mut deckcraft_model::Table,
    shape: Option<u64>,
    path: &deckcraft_model::table::TablePath,
    f: &mut dyn FnMut(Loc, &mut TextBody) -> Result<bool>,
) -> Result<()> {
    path.validate()?;
    for (row, values) in table.rows.iter_mut().enumerate() {
        for (col, cell) in values.cells.iter_mut().enumerate() {
            if cell.is_covered() {
                continue;
            }
            if f(Loc { shape, cell: Some((row, col)), path: path.clone(), block: None }, &mut cell.text)? {
                cell.reconcile_constant()?;
            }
            for block in &mut cell.blocks {
                match block {
                    deckcraft_model::table::CellBlock::Text(block) => {
                        f(Loc { shape, cell: Some((row, col)), path: path.clone(), block: Some(block.id) }, &mut block.body)?;
                    }
                    deckcraft_model::table::CellBlock::Table(child) => {
                        let mut path = path.clone();
                        path.0.push(deckcraft_model::table::TablePathStep { row, col, child: child.id });
                        table_bodies(&mut child.table, shape, &path, f)?;
                    }
                }
            }
        }
    }
    Ok(())
}

struct Opts {
    pat: Vec<char>,
    case: bool,
    whole: bool,
}

impl Opts {
    fn from(p: &Value, cmd: &str) -> Result<Self> {
        let pat: Vec<char> = str_param(p, "text").unwrap_or("").chars().collect();
        if pat.is_empty() {
            return Err(bad(cmd, "missing `text`"));
        }
        Ok(Opts { pat, case: bool_or(p, "matchCase", false), whole: bool_or(p, "wholeWords", false) })
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
    p.runs.iter().flat_map(|r| r.text.chars()).collect()
}

/// Matches that lie wholly in plain text runs (never inside a field, equation or break).
fn para_matches(p: &Paragraph, o: &Opts) -> Vec<(usize, usize)> {
    let mut spans = vec![];
    let mut off = 0;
    for r in &p.runs {
        let n = r.text.chars().count();
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
        let chars: Vec<char> = r.text.chars().collect();
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
        let mut sl = (**sl).clone();
        bodies(&mut sl, &mut |loc, t| {
            for (pi, para) in t.paragraphs.iter().enumerate() {
                for (a, b) in para_matches(para, &o) {
                    let mut hit = json!({"slide": i, "shape": loc.shape, "cell": loc.cell.map(|(r, c)| [r, c]), "para": pi, "start": a, "end": b});
                    if !loc.path.is_empty() {
                        hit["path"] = serde_json::to_value(&loc.path).map_err(|_| bad("text.find", "invalid table path"))?;
                    }
                    if let Some(block) = loc.block {
                        hit["block"] = Value::from(block);
                    }
                    out.push(hit);
                }
            }
            Ok(false)
        })?;
    }
    Ok(Value::Array(out))
}

fn replace(s: &mut Session, p: &Value) -> Result<Value> {
    let o = Opts::from(p, "text.replace")?;
    let with = str_param(p, "with").unwrap_or("").to_string();
    let at = p.get("at").filter(|v| v.is_object()).map(|v| -> Result<(usize, Loc, usize, usize)> {
        let n = |k: &str| v.get(k).and_then(Value::as_u64).map(|x| x as usize).ok_or_else(|| bad("text.replace", format!("`at` needs `{k}`")));
        let cell = v.get("cell").and_then(Value::as_array).and_then(|c| Some((c.first()?.as_u64()? as usize, c.get(1)?.as_u64()? as usize)));
        let path = if let Some(path) = v.get("path") {
            let values = path.as_array().ok_or_else(|| bad("text.replace", "invalid table path"))?;
            if values.len() > deckcraft_model::table::MAX_NESTED_TABLE_DEPTH {
                return Err(bad("text.replace", "table path exceeds limit"));
            }
            serde_json::from_value(path.clone()).map_err(|_| bad("text.replace", "invalid table path"))?
        } else {
            Default::default()
        };
        let block = v
            .get("block")
            .map(|value| value.as_u64().and_then(|value| u32::try_from(value).ok()).ok_or_else(|| bad("text.replace", "invalid text block")))
            .transpose()?;
        Ok((n("slide")?, Loc { shape: v.get("shape").and_then(Value::as_u64), cell, path, block }, n("para")?, n("start")?))
    });
    let at = at.transpose()?;
    let count = s.edit(|doc, _| {
        let mut count = 0;
        for (i, slp) in doc.slides.iter_mut().enumerate() {
            if at.as_ref().is_some_and(|a| a.0 != i) {
                continue;
            }
            let mut sl = (**slp).clone();
            let mut changed = 0;
            bodies(&mut sl, &mut |loc, t| {
                if at.as_ref().is_some_and(|a| a.1 != loc) {
                    return Ok(false);
                }
                let before = changed;
                for (pi, para) in t.paragraphs.iter_mut().enumerate() {
                    if at.as_ref().is_some_and(|a| a.2 != pi) {
                        continue;
                    }
                    for (a, b) in para_matches(para, &o).into_iter().rev() {
                        if at.as_ref().is_some_and(|x| x.3 != a) {
                            continue;
                        }
                        splice(para, a, b, &with);
                        changed += 1;
                    }
                }
                Ok(changed != before)
            })?;
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
        Opts { pat: t.chars().collect(), case, whole }
    }

    #[test]
    fn matching_honours_case_and_whole_words() {
        let t: Vec<char> = "Cat cat category".chars().collect();
        assert_eq!(opts("cat", false, false).matches(&t).len(), 3);
        assert_eq!(opts("cat", true, false).matches(&t).len(), 2);
        assert_eq!(opts("cat", false, true).matches(&t), vec![(0, 3), (4, 7)]);
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
}
