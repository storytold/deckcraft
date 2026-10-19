//! Equations typed in place: Insert ▸ Equation, and the equation the insertion point is in.
//!
//! An equation is typed as its linear text (`a/b`, `x^2`, `\alpha`): it shows that way while the
//! caret is in it and is laid out in two dimensions otherwise. The caret belongs to the equation
//! on its left, as typing does, so before an equation it is outside; Right Arrow at an equation's
//! end leaves it, so what is typed next is text after it, and Left Arrow there goes back in.

use deckcraft_geom::Xfrm;
use deckcraft_model::edit::{self as ed, Pos};
use deckcraft_model::text::{Align, ParaProps, Paragraph, Run, RunKind, RunProps, TextBody};
use serde_json::{Value, json};

use super::*;
use crate::{DocState, Result, Session, TextSel};

/// What a new equation holds, selected, until something is typed in it.
pub const PROMPT: &str = "Type equation here";

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "insert.equation",
        "Equation",
        ["Insert", "Symbols"],
        None,
        "{} — an equation to type in place: at the insertion point (the selected text becomes it), else in a new text box",
        has_slide,
        insert
    )]
}

fn insert(s: &mut Session, _: &Value) -> Result<Value> {
    if s.doc()?.selection.text.as_ref().is_none_or(|t| t.notes) {
        // No insertion point on the slide: a new text box in the middle of it.
        let size = s.doc()?.doc.slide_size;
        let mut shape = super::insert::new_text_box(Xfrm::new((size.width - 320.0) / 2.0, (size.height - 60.0) / 2.0, 320.0, 60.0), "");
        shape.name = "Equation".into();
        // It grows with the equation rather than wrapping it, centred as PowerPoint sets a
        // display equation.
        if let Some(t) = shape.text.as_mut() {
            t.body.wrap = Some(false);
            t.paragraphs = vec![Paragraph { props: ParaProps { align: Some(Align::Center), ..Default::default() }, ..Default::default() }];
        }
        let id = super::insert::add_shape(s, "insert.equation", shape, true)?;
        s.select(|_, sel| sel.text = Some(TextSel { shape: id, ..Default::default() }))?;
    }
    super::text::edit_text(s, |body, t| {
        let (a, b) = t.ordered();
        // In an equation, the new one goes after it; selected text becomes the equation.
        let (at, text) = match at(body, t) {
            Some((_, _, e)) => ((a.0, e), String::new()),
            None => {
                let text = ed::text_range(body, a, b).replace('\n', " ").trim().to_string();
                (ed::delete(body, a, b), text)
            }
        };
        if body.paragraphs.is_empty() {
            body.paragraphs.push(Paragraph::default());
        }
        let prompt = text.is_empty();
        let text = if prompt { PROMPT.to_string() } else { text };
        let run = Run { text, props: ed::props_at(body, at), kind: RunKind::Math { omml: String::new() } };
        t.caret =
            ed::insert_body(body, at, &TextBody { paragraphs: vec![Paragraph { runs: vec![run], ..Default::default() }], ..Default::default() });
        t.anchor = if prompt { at } else { t.caret };
    })?;
    let t = s.doc()?.selection.text.clone().unwrap_or_default();
    Ok(json!({"id": t.shape, "anchor": [t.anchor.0, t.anchor.1], "caret": [t.caret.0, t.caret.1]}))
}

/// The equation selection `t` is in: (run index, first character, end character) in its
/// paragraph. A selection must lie within the equation it starts in.
pub(crate) fn at(body: &TextBody, t: &TextSel) -> Option<(usize, usize, usize)> {
    let (a, b) = t.ordered();
    if a.0 != b.0 || (!t.is_range() && t.left_math == Some(t.caret)) {
        return None;
    }
    ed::math_at(body, a, t.is_range()).filter(|m| b.1 <= m.2)
}

/// The equation shown as its linear text while text `t` (a shape's, a cell's) is laid out: the one
/// the selection is in when it edits that text. (Paragraph, first character.)
pub fn shown(st: &DocState, t: &TextSel) -> Option<(usize, usize)> {
    let live = st.selection.text.as_ref().filter(|l| !l.notes && (l.shape, l.cell) == (t.shape, t.cell))?;
    at(super::text::body_of(st, live)?, live).map(|m| (live.caret.0, m.1))
}

/// The equation being typed, for drawing it as text: its table cell, paragraph and first character.
pub fn active(st: &DocState) -> Option<(Option<(usize, usize)>, usize, usize)> {
    let t = st.selection.text.as_ref()?;
    shown(st, t).map(|(p, c)| (t.cell, p, c))
}

/// Type `text` into equation `m` (see [`at`]) in place of the selection. A new equation's prompt
/// goes as a whole; `\name` followed by a space becomes the name's symbol. Returns the caret.
pub(crate) fn type_in(body: &mut TextBody, t: &TextSel, (ri, s, e): (usize, usize, usize), text: &str) -> Pos {
    let (a, b) = t.ordered();
    let run = body.paragraphs.get(a.0).and_then(|p| p.runs.get(ri)).map(|r| r.text.clone()).unwrap_or_default();
    if run == PROMPT {
        return ed::math_replace(body, a.0, ri, s, e, text);
    }
    if text == " " && a == b {
        let before: Vec<char> = run.chars().take(a.1.saturating_sub(s)).collect();
        let name = before.iter().rev().take_while(|c| c.is_ascii_alphabetic()).count();
        if let Some(slash) = before.len().checked_sub(name + 1)
            && name > 0
            && before.get(slash) == Some(&'\\')
        {
            let command: String = before.get(slash..).unwrap_or(&[]).iter().collect();
            if let Some(sym) = ed::math_symbol(&command) {
                return ed::math_replace(body, a.0, ri, s + slash, a.1, &sym);
            }
        }
    }
    ed::math_replace(body, a.0, ri, a.1, b.1, text)
}

/// The formatting of text typed at `pos` just after an equation (having left it): the text's
/// around it rather than the equation's.
pub(crate) fn text_props(body: &TextBody, pos: Pos) -> RunProps {
    let Some(p) = body.paragraphs.get(pos.0) else { return RunProps::default() };
    let mut start = 0;
    let (mut before, mut after) = (None, None);
    for r in &p.runs {
        let end = start + r.char_len();
        if r.kind == RunKind::Text {
            if end <= pos.1 {
                before = Some(&r.props);
            } else if after.is_none() && start >= pos.1 {
                after = Some(&r.props);
            }
        }
        start = end;
    }
    before.or(after).cloned().unwrap_or_else(|| ed::props_at(body, pos))
}

#[cfg(test)]
#[path = "equation_tests.rs"]
mod tests;
