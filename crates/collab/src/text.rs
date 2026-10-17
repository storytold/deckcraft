//! Text bodies as Yjs text (schema 1, "Text"): each paragraph's characters, then a `\n` that
//! ends it. Formatting travels as Yjs formatting attributes, one per property, so two people
//! formatting different properties of the same text both keep their change, and two people typing
//! in the same paragraph both keep their text.
//!
//! - A character's attributes are its run's properties (the model's `RunProps`, camelCase, only
//!   those set): `bold: true`, `size: 24`, `font: "Arial"`, `fill: {…}`. A field run has `field`
//!   (`slidenum`, `datetime1`…) with its last value as its text; an equation has `math` (its OMML)
//!   with its linear form as its text. A line break is the character U+000B.
//! - The `\n` ending a paragraph carries the paragraph: `level` (when not 0), its properties as
//!   `p:<name>` (`p:align: "ctr"`), its default run properties as `defRPr` (a map), and the
//!   paragraph mark's own run properties as plain run attributes.
//!
//! Offsets are UTF-16 code units, as in Yjs.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use deckcraft_model::text::{Paragraph, Run, RunKind, RunProps};
use serde_json::{Map as JsonMap, Value as Json};
use yrs::types::text::YChange;
use yrs::{Any, Out, ReadTxn, Text, TextRef, TransactionMut};

use crate::json::{self, canon, lenient, set_entries, to_any, to_json};

/// Attributes of one character, in their canonical form.
pub type Attrs = BTreeMap<String, Any>;

/// One character of a text and its attributes.
#[derive(Clone, Debug, PartialEq)]
pub struct Atom {
    pub ch: char,
    pub attrs: Rc<Attrs>,
}

/// The paragraph mark.
const MARK: char = '\n';
/// A line break inside a paragraph.
const BREAK: char = '\u{b}';
const LEVEL: &str = "level";
const DEF_RPR: &str = "defRPr";
const PARA_PREFIX: &str = "p:";
const FIELD: &str = "field";
const MATH: &str = "math";

/// Most characters read from one text (a hostile peer's text is cut there).
const MAX_ATOMS: usize = 4_000_000;

fn attrs_of_json(v: &Json, prefix: &str) -> Vec<(String, Any)> {
    set_entries(v).into_iter().map(|(k, x)| (format!("{prefix}{k}"), to_any(&x))).collect()
}

/// A run's attributes.
pub fn run_attrs(r: &Run) -> Attrs {
    let mut a: Attrs = attrs_of_json(&serde_json::to_value(&r.props).unwrap_or(Json::Null), "").into_iter().collect();
    match &r.kind {
        RunKind::Field { field } => {
            a.insert(FIELD.into(), Any::String(Arc::from(field.as_str())));
        }
        RunKind::Math { omml } => {
            a.insert(MATH.into(), Any::String(Arc::from(omml.as_str())));
        }
        RunKind::Text | RunKind::Break => {}
    }
    a
}

/// A paragraph mark's attributes.
pub fn mark_attrs(p: &Paragraph) -> Attrs {
    let mut a: Attrs = attrs_of_json(&serde_json::to_value(&p.end_props).unwrap_or(Json::Null), "").into_iter().collect();
    if p.level != 0 {
        a.insert(LEVEL.into(), to_any(&Json::from(p.level)));
    }
    a.extend(attrs_of_json(&serde_json::to_value(&p.props).unwrap_or(Json::Null), PARA_PREFIX));
    let def = serde_json::to_value(&p.def_props).unwrap_or(Json::Null);
    let def: JsonMap<String, Json> = set_entries(&def).into_iter().collect();
    if !def.is_empty() {
        a.insert(DEF_RPR.into(), to_any(&Json::Object(def)));
    }
    a
}

/// The characters of paragraphs, with their attributes.
pub fn atoms_of(paragraphs: &[Paragraph]) -> Vec<Atom> {
    let mut out = Vec::new();
    for p in paragraphs {
        for r in &p.runs {
            let attrs = Rc::new(run_attrs(r));
            for ch in r.content().chars() {
                out.push(Atom { ch, attrs: attrs.clone() });
            }
        }
        out.push(Atom { ch: MARK, attrs: Rc::new(mark_attrs(p)) });
    }
    out
}

/// The characters of a Yjs text, with their attributes (embeds are skipped).
pub fn read_atoms<T: ReadTxn>(text: &TextRef, txn: &T) -> Vec<Atom> {
    let mut out = Vec::new();
    for d in text.diff(txn, YChange::identity) {
        let Out::Any(Any::String(s)) = &d.insert else { continue };
        let attrs: Attrs = d.attributes.as_ref().map(|a| a.iter().map(|(k, v)| (k.to_string(), canon(v))).collect()).unwrap_or_default();
        let attrs = Rc::new(attrs);
        for ch in s.chars() {
            if out.len() >= MAX_ATOMS {
                return out;
            }
            out.push(Atom { ch, attrs: attrs.clone() });
        }
    }
    out
}

fn props_from(attrs: &Attrs) -> RunProps {
    let obj: JsonMap<String, Json> = attrs
        .iter()
        .filter(|(k, _)| !matches!(k.as_str(), LEVEL | DEF_RPR | FIELD | MATH) && !k.starts_with(PARA_PREFIX))
        .map(|(k, v)| (k.clone(), to_json(v)))
        .collect();
    if obj.is_empty() { RunProps::default() } else { lenient(obj) }
}

fn finish_paragraph(runs: Vec<Run>, mark: Option<&Attrs>) -> Paragraph {
    let mut p = Paragraph { runs, ..Default::default() };
    if let Some(a) = mark {
        p.end_props = props_from(a);
        p.level = a.get(LEVEL).map(to_json).and_then(|v| v.as_u64()).map(|l| l.min(8) as u8).unwrap_or(0);
        let para: JsonMap<String, Json> = a.iter().filter_map(|(k, v)| k.strip_prefix(PARA_PREFIX).map(|k| (k.to_string(), to_json(v)))).collect();
        if !para.is_empty() {
            p.props = lenient(para);
        }
        if let Some(Json::Object(def)) = a.get(DEF_RPR).map(to_json)
            && !def.is_empty()
        {
            p.def_props = lenient(def);
        }
    }
    p
}

/// Paragraphs from characters (the inverse of [`atoms_of`]). Characters after the last `\n`
/// make a last paragraph with no properties.
pub fn paragraphs_of(atoms: &[Atom]) -> Vec<Paragraph> {
    let mut out = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    let mut cur: Option<(Rc<Attrs>, String)> = None;
    let flush = |runs: &mut Vec<Run>, cur: &mut Option<(Rc<Attrs>, String)>| {
        if let Some((a, text)) = cur.take() {
            let kind = if let Some(Any::String(f)) = a.get(FIELD) {
                RunKind::Field { field: f.to_string() }
            } else if let Some(Any::String(m)) = a.get(MATH) {
                RunKind::Math { omml: m.to_string() }
            } else {
                RunKind::Text
            };
            runs.push(Run { text, props: props_from(&a), kind });
        }
    };
    for at in atoms {
        match at.ch {
            MARK => {
                flush(&mut runs, &mut cur);
                out.push(finish_paragraph(std::mem::take(&mut runs), Some(&at.attrs)));
            }
            BREAK => {
                flush(&mut runs, &mut cur);
                runs.push(Run { text: String::new(), props: props_from(&at.attrs), kind: RunKind::Break });
            }
            ch => match &mut cur {
                Some((a, text)) if *a == at.attrs || **a == *at.attrs => text.push(ch),
                _ => {
                    flush(&mut runs, &mut cur);
                    cur = Some((at.attrs.clone(), ch.to_string()));
                }
            },
        }
    }
    flush(&mut runs, &mut cur);
    if !runs.is_empty() {
        out.push(finish_paragraph(runs, None));
    }
    out
}

fn utf16(atoms: &[Atom]) -> u32 {
    atoms.iter().map(|a| a.ch.len_utf16() as u32).sum()
}

fn yattrs(a: &Attrs) -> yrs::types::Attrs {
    a.iter().map(|(k, v)| (Arc::from(k.as_str()), v.clone())).collect()
}

/// Inserts `atoms` at UTF-16 offset `at`, a chunk per run of equal attributes.
fn insert_atoms(text: &TextRef, txn: &mut TransactionMut, mut at: u32, atoms: &[Atom]) {
    let mut i = 0;
    while i < atoms.len() {
        let Some(first) = atoms.get(i) else { break };
        let mut j = i;
        let mut chunk = String::new();
        while let Some(a) = atoms.get(j) {
            if !(Rc::ptr_eq(&a.attrs, &first.attrs) || a.attrs == first.attrs) {
                break;
            }
            chunk.push(a.ch);
            j += 1;
        }
        // Never past the end (yrs panics there); the text is ours, so this holds.
        let len = text.len(txn);
        let n = chunk.encode_utf16().count() as u32;
        text.insert_with_attributes(txn, at.min(len), &chunk, yattrs(&first.attrs));
        at = at.min(len) + n;
        i = j;
    }
}

/// Writes `atoms` into an empty text.
pub fn write(text: &TextRef, txn: &mut TransactionMut, atoms: &[Atom]) {
    insert_atoms(text, txn, 0, atoms);
}

/// The attribute changes from `old` to `new` (removed ones as `null`).
fn changes(old: &Attrs, new: &Attrs) -> Attrs {
    let mut c: Attrs = new.iter().filter(|(k, v)| old.get(*k) != Some(*v)).map(|(k, v)| (k.clone(), v.clone())).collect();
    for k in old.keys() {
        if !new.contains_key(k) {
            c.insert(k.clone(), Any::Null);
        }
    }
    c
}

/// Formats aligned characters whose attributes differ, `old` and `new` starting at UTF-16
/// offset `at`.
fn reformat(text: &TextRef, txn: &mut TransactionMut, at: u32, old: &[Atom], new: &[Atom]) {
    let mut pos = at;
    let mut run: Option<(u32, u32, Attrs)> = None;
    let flush = |text: &TextRef, txn: &mut TransactionMut, run: &mut Option<(u32, u32, Attrs)>| {
        if let Some((start, len, c)) = run.take() {
            let total = text.len(txn);
            if start.saturating_add(len) <= total && !c.is_empty() {
                text.format(txn, start, len, yattrs(&c));
            }
        }
    };
    for (o, n) in old.iter().zip(new) {
        let w = o.ch.len_utf16() as u32;
        let same = Rc::ptr_eq(&o.attrs, &n.attrs) || o.attrs == n.attrs;
        if same {
            flush(text, txn, &mut run);
        } else {
            let c = changes(&o.attrs, &n.attrs);
            match &mut run {
                Some((start, len, rc)) if *rc == c && *start + *len == pos => *len += w,
                _ => {
                    flush(text, txn, &mut run);
                    run = Some((pos, w, c));
                }
            }
        }
        pos += w;
    }
    flush(text, txn, &mut run);
}

/// Turns the text holding `old` into `new` with the fewest edits that keep concurrent typing:
/// the common start and end stay (formatted where their attributes changed), the middle is
/// replaced. Typing one character inserts one character.
pub fn apply_diff(text: &TextRef, txn: &mut TransactionMut, old: &[Atom], new: &[Atom]) {
    let min = old.len().min(new.len());
    let mut p = 0;
    while p < min && old.get(p).map(|a| a.ch) == new.get(p).map(|a| a.ch) {
        p += 1;
    }
    let mut s = 0;
    while s < min - p && old.get(old.len() - 1 - s).map(|a| a.ch) == new.get(new.len() - 1 - s).map(|a| a.ch) {
        s += 1;
    }
    let (old_head, old_mid, old_tail) = (&old[..p], &old[p..old.len() - s], &old[old.len() - s..]);
    let (new_head, new_mid, new_tail) = (&new[..p], &new[p..new.len() - s], &new[new.len() - s..]);
    let head_len = utf16(old_head);
    // Formatting first, in the old offsets (it doesn't change lengths).
    reformat(text, txn, 0, old_head, new_head);
    reformat(text, txn, head_len + utf16(old_mid), old_tail, new_tail);
    let gone = utf16(old_mid);
    if gone > 0 {
        let total = text.len(txn);
        if head_len.saturating_add(gone) <= total {
            text.remove_range(txn, head_len, gone);
        }
    }
    insert_atoms(text, txn, head_len, new_mid);
}

/// The UTF-16 offset of `(paragraph, char)` in text holding `paragraphs`.
pub fn offset_of(paragraphs: &[Paragraph], pos: (usize, usize)) -> u32 {
    let mut at = 0u32;
    for (i, p) in paragraphs.iter().enumerate() {
        if i == pos.0 {
            let mut n = 0;
            for r in &p.runs {
                for ch in r.content().chars() {
                    if n >= pos.1 {
                        return at;
                    }
                    at += ch.len_utf16() as u32;
                    n += 1;
                }
            }
            return at;
        }
        at += p.runs.iter().map(|r| r.content().encode_utf16().count() as u32).sum::<u32>() + 1;
    }
    at
}

/// `(paragraph, char)` at UTF-16 offset `at` in text holding `paragraphs`.
pub fn pos_of(paragraphs: &[Paragraph], at: u32) -> (usize, usize) {
    let mut left = at;
    for (i, p) in paragraphs.iter().enumerate() {
        let mut n = 0;
        for r in &p.runs {
            for ch in r.content().chars() {
                let w = ch.len_utf16() as u32;
                if left < w {
                    return (i, n);
                }
                left -= w;
                n += 1;
            }
        }
        if left == 0 || i + 1 == paragraphs.len() {
            return (i, n);
        }
        left -= 1;
    }
    (0, 0)
}

/// Text attributes as JSON (tests, diagnostics).
pub fn attrs_json(a: &Attrs) -> Json {
    Json::Object(a.iter().map(|(k, v)| (k.clone(), json::to_json(v))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::text::{Align, ParaProps, TextBody};
    use yrs::{Doc, GetString, OffsetKind, Options, Transact};

    fn doc() -> Doc {
        let mut o = Options::with_client_id(yrs::block::ClientID::new(1));
        o.offset_kind = OffsetKind::Utf16;
        Doc::with_options(o)
    }

    fn body() -> TextBody {
        let mut b = TextBody::from_text("Hello world\nSecond é𝄞 line");
        if let Some(p) = b.paragraphs.get_mut(0) {
            p.runs = vec![Run::new("Hello "), Run::with("world", RunProps { bold: Some(true), size: Some(24.0), ..Default::default() })];
            p.props = ParaProps { align: Some(Align::Center), ..Default::default() };
            p.level = 2;
        }
        if let Some(p) = b.paragraphs.get_mut(1) {
            p.runs.push(Run { text: String::new(), props: RunProps::default(), kind: RunKind::Break });
            p.runs.push(Run { text: "7".into(), props: RunProps::default(), kind: RunKind::Field { field: "slidenum".into() } });
        }
        b
    }

    #[test]
    fn round_trips_paragraphs_runs_and_fields() {
        let d = doc();
        let t = d.get_or_insert_text("t");
        let b = body();
        write(&t, &mut d.transact_mut(), &atoms_of(&b.paragraphs));
        let txn = d.transact();
        assert_eq!(t.get_string(&txn), "Hello world\nSecond é𝄞 line\u{b}7\n");
        let back = paragraphs_of(&read_atoms(&t, &txn));
        assert_eq!(back, b.paragraphs);
    }

    #[test]
    fn diffs_are_small_and_concurrent_typing_merges() {
        let a = doc();
        let t = a.get_or_insert_text("t");
        let b = body();
        write(&t, &mut a.transact_mut(), &atoms_of(&b.paragraphs));
        let mut typed = b.clone();
        deckcraft_model::edit::insert(&mut typed, (0, 5), ",", None);
        let up = {
            let mut txn = a.transact_mut();
            let old = read_atoms(&t, &txn);
            apply_diff(&t, &mut txn, &old, &atoms_of(&typed.paragraphs));
            txn.encode_update_v1()
        };
        assert!(up.len() < 40, "one character, {} bytes", up.len());
        assert_eq!(paragraphs_of(&read_atoms(&t, &a.transact())), typed.paragraphs);
        // Formatting a range.
        let mut bold = typed.clone();
        deckcraft_model::edit::format(&mut bold, (1, 0), (1, 6), &|p| p.italic = Some(true));
        {
            let mut txn = a.transact_mut();
            let old = read_atoms(&t, &txn);
            apply_diff(&t, &mut txn, &old, &atoms_of(&bold.paragraphs));
        }
        let mut got = paragraphs_of(&read_atoms(&t, &a.transact()));
        for p in got.iter_mut() {
            p.normalize();
        }
        let mut want = bold.paragraphs.clone();
        for p in want.iter_mut() {
            p.normalize();
        }
        assert_eq!(got, want);
    }

    #[test]
    fn offsets() {
        let b = body();
        assert_eq!(offset_of(&b.paragraphs, (0, 0)), 0);
        assert_eq!(offset_of(&b.paragraphs, (1, 0)), 12);
        assert_eq!(offset_of(&b.paragraphs, (1, 8)), 20);
        assert_eq!(offset_of(&b.paragraphs, (1, 9)), 22, "after the surrogate pair");
        for pos in [(0, 0), (0, 11), (1, 0), (1, 9), (1, 10), (1, 16)] {
            assert_eq!(pos_of(&b.paragraphs, offset_of(&b.paragraphs, pos)), pos);
        }
    }
}
