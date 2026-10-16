//! On-slide editing reuses the structured editor; the document still holds only OMML runs.
use deckcraft_model::text::{RunKind, RunProps, TextBody};
use equashion_math::{Caret, Editor, Key};
use serde_json::{Value, json};

use super::super::text;
use crate::{DocState, EquationSel, Result, Session, TextSel};

/// Active equation, validated against the outer text caret and the current document.
pub fn active(st: &DocState) -> Option<&EquationSel> {
    let t = st.selection.text.as_ref()?;
    let e = t.equation.as_ref()?;
    let p = text::body_of(st, t)?.paragraphs.get(t.caret.0)?;
    let run = p.runs.get(e.run)?;
    let start: usize = p.runs.iter().take(e.run).map(|r| r.char_len()).sum();
    (matches!(run.kind, RunKind::Math { .. }) && !t.is_range() && t.caret.1 == start).then_some(e)
}

pub(crate) fn editor(st: &DocState) -> Option<Editor> {
    let e = active(st)?;
    let t = st.selection.text.as_ref()?;
    let r = text::body_of(st, t)?.paragraphs.get(t.caret.0)?.runs.get(e.run)?;
    let RunKind::Math { omml } = &r.kind else { return None };
    let mut ed = Editor::new(equashion_math::from_omml(omml));
    if let Some(a) = e.anchor {
        ed.set_caret(Caret { path: e.caret.path.clone(), pos: a });
        ed.drag_caret(e.caret.clone());
    } else {
        ed.set_caret(e.caret.clone());
    }
    ed.restore_input_state(&e.input);
    Some(ed)
}

fn selection(run: usize, ed: &Editor) -> EquationSel {
    let anchor = ed.selection().map(|(a, b)| if ed.caret().pos == a { b } else { a });
    EquationSel { run, caret: ed.caret().clone(), anchor, input: ed.input_state() }
}

fn store(s: &mut Session, ed: &Editor) -> Result<Value> {
    let st = s.doc()?;
    let Some(e) = active(st).cloned() else { return super::ok() };
    let old = editor(st);
    let next = selection(e.run, ed);
    if old.as_ref().is_some_and(|old| old.math() == ed.math()) {
        s.select(|_, sel| {
            if let Some(t) = sel.text.as_mut() {
                t.equation = Some(next);
            }
        })?;
    } else {
        let (omml, linear) = super::run_payload(ed.math());
        if omml.len() > super::MAX_OMML || linear.len() > super::MAX_LINEAR {
            return Err(super::bad("equation.edit", "equation is too large"));
        }
        text::edit_text(s, |body, t| {
            if let Some(run) = body.paragraphs.get_mut(t.caret.0).and_then(|p| p.runs.get_mut(e.run)) {
                run.text = linear;
                run.kind = RunKind::Math { omml };
                t.equation = Some(next);
            }
        })?;
    }
    super::ok()
}

pub(super) fn enter(s: &mut Session, p: &Value) -> Result<Value> {
    let (id, cell, pi, ri) = super::locate(s, "equation.edit", p)?;
    let st = s.doc()?;
    let body = st.shape(id).and_then(|sh| super::body_ref(sh, cell)).ok_or_else(|| super::bad("equation.edit", "no text"))?;
    let para = body.paragraphs.get(pi).ok_or_else(|| super::bad("equation.edit", "no paragraph"))?;
    let run = para.runs.get(ri).ok_or_else(|| super::bad("equation.edit", "no equation"))?;
    let RunKind::Math { omml } = &run.kind else { return Err(super::bad("equation.edit", "not an equation")) };
    let mut ed = Editor::new(equashion_math::from_omml(omml));
    if p.get("start").and_then(Value::as_bool).unwrap_or(false) {
        ed.set_caret(Caret::default());
    }
    if let Some(c) = p.get("caret") {
        let c: Caret = serde_json::from_value(c.clone()).map_err(|_| super::bad("equation.edit", "invalid math caret"))?;
        if c.path.len() > equashion_math::MAX_DEPTH {
            return Err(super::bad("equation.edit", "caret too deep"));
        }
        ed.set_caret(c);
    }
    if p.get("extend").and_then(Value::as_bool).unwrap_or(false)
        && let Some(t) = st.selection.text.as_ref()
        && t.shape == id
        && t.cell == cell
        && t.caret.0 == pi
        && active(st).is_some_and(|e| e.run == ri)
        && let Some(mut prev) = editor(st)
    {
        prev.drag_caret(ed.caret().clone());
        ed = prev;
    }
    let at = (pi, para.runs.iter().take(ri).map(|r| r.char_len()).sum());
    let equation = Some(selection(ri, &ed));
    s.select(|_, sel| {
        sel.shapes = vec![id];
        sel.text = Some(TextSel { shape: id, cell, anchor: at, caret: at, equation, ..Default::default() });
    })?;
    super::ok()
}

pub(super) fn leave(s: &mut Session, p: &Value) -> Result<Value> {
    leave_at(s, p.get("start").and_then(Value::as_bool).unwrap_or(false))
}

pub(crate) fn leave_at(s: &mut Session, start: bool) -> Result<Value> {
    let st = s.doc()?;
    let Some(e) = active(st) else { return super::ok() };
    let Some(t) = st.selection.text.as_ref() else { return super::ok() };
    let len = text::body_of(st, t).and_then(|b| b.paragraphs.get(t.caret.0)).and_then(|p| p.runs.get(e.run)).map_or(0, |r| r.char_len());
    s.select(|_, sel| {
        if let Some(t) = sel.text.as_mut() {
            if !start {
                t.caret.1 += len;
            }
            t.anchor = t.caret;
            t.equation = None;
        }
    })?;
    super::ok()
}

pub(crate) fn type_text(s: &mut Session, input: &str) -> Result<Option<Value>> {
    let Some(mut ed) = editor(s.doc()?) else { return Ok(None) };
    if input.len() > super::MAX_LINEAR {
        return Err(super::bad("text.insert", "equation input is too large"));
    }
    ed.type_str(input);
    store(s, &ed).map(Some)
}

/// Navigation in the math tree; Right at the root end returns to surrounding prose.
pub(crate) fn key(s: &mut Session, key: Key, shift: bool) -> Result<Option<Value>> {
    let Some(mut ed) = editor(s.doc()?) else { return Ok(None) };
    if !shift && ed.selection().is_none() && ed.caret().path.is_empty() {
        if key == Key::Right && ed.caret().pos == equashion_math::units(&ed.math().body) {
            return leave_at(s, false).map(Some);
        }
        if key == Key::Left && ed.caret().pos == 0 {
            return leave_at(s, true).map(Some);
        }
    }
    ed.key(key, shift);
    store(s, &ed).map(Some)
}

pub(super) fn key_command(s: &mut Session, p: &Value) -> Result<Value> {
    let k = p.get("key").and_then(Value::as_str).and_then(Key::parse).ok_or_else(|| super::bad("equation.key", "unknown key"))?;
    // Document undo/redo owns both OMML and caret snapshots.
    if matches!(k, Key::Undo | Key::Redo) {
        return s.execute(if k == Key::Undo { "edit.undo" } else { "edit.redo" }, &json!({}));
    }
    key(s, k, p.get("extend").and_then(Value::as_bool).unwrap_or(false))?.ok_or_else(|| super::bad("equation.key", "not editing an equation"))
}

pub(crate) fn select_all(s: &mut Session) -> Result<Option<Value>> {
    let Some(mut ed) = editor(s.doc()?) else { return Ok(None) };
    ed.select_all();
    store(s, &ed).map(Some)
}

/// Arrow movement from prose enters a neighbouring equation without skipping over it.
pub(crate) fn enter_adjacent(s: &mut Session, right: bool) -> Result<bool> {
    let st = s.doc()?;
    let Some(t) = st.selection.text.as_ref().filter(|t| !t.is_range() && !t.notes) else { return Ok(false) };
    let Some(p) = text::body_of(st, t).and_then(|b| b.paragraphs.get(t.caret.0)) else { return Ok(false) };
    let mut at = 0;
    for (ri, run) in p.runs.iter().enumerate() {
        let end = at + run.char_len();
        if matches!(run.kind, RunKind::Math { .. }) && t.caret.1 == if right { at } else { end } {
            let mut params = json!({"id": t.shape, "paragraph": t.caret.0, "run": ri, "start": right});
            if let Some((r, c)) = t.cell {
                params["cell"] = json!([r, c]);
            }
            enter(s, &params)?;
            return Ok(true);
        }
        at = end;
    }
    Ok(false)
}

/// At a math/prose boundary, inherit prose formatting, never an equation's italic style.
pub(crate) fn prose_props(body: &TextBody, at: (usize, usize)) -> Option<RunProps> {
    let p = body.paragraphs.get(at.0)?;
    let mut ch = 0;
    for (i, run) in p.runs.iter().enumerate() {
        let end = ch + run.char_len();
        if matches!(run.kind, RunKind::Math { .. }) && (at.1 == ch || at.1 == end) {
            let right = p.runs.iter().skip(i + 1).find(|r| matches!(r.kind, RunKind::Text));
            let left = p.runs.iter().take(i).rev().find(|r| matches!(r.kind, RunKind::Text));
            return Some(right.or(left).map(|r| r.props.clone()).unwrap_or_else(|| p.end_props.clone()));
        }
        ch = end;
    }
    None
}

/// Recognised clipboard math joins the current tree instead of being typed as XML markup.
pub(crate) fn paste(s: &mut Session, input: &str) -> Result<Value> {
    let Some(mut ed) = editor(s.doc()?) else {
        return Err(super::bad("edit.paste", "not editing an equation"));
    };
    if let Some((_, Ok(math))) = crate::equashion_bridge::clipboard_equation(input) {
        ed.insert_template(&math);
        store(s, &ed)
    } else {
        type_text(s, input)?.ok_or_else(|| super::bad("edit.paste", "not editing an equation"))
    }
}

/// Gallery choices use the same structured editor, selection and undo as typing.
pub(super) fn template(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("blank").and_then(Value::as_bool) == Some(true) {
        return super::insert_equation(s, &json!({}));
    }
    let g = equashion_math::galleries();
    let (math, replace) = if let Some(id) = p.get("template").and_then(Value::as_str) {
        (g.templates().find(|t| t.id == id).ok_or_else(|| super::bad("equation.template", "unknown template"))?.math.clone(), false)
    } else if let Some(id) = p.get("builtin").and_then(Value::as_str) {
        (g.builtins.iter().find(|t| t.id == id).ok_or_else(|| super::bad("equation.template", "unknown equation"))?.math.clone(), true)
    } else if let Some(symbol) = p.get("symbol").and_then(Value::as_str).filter(|s| s.chars().count() == 1) {
        (equashion_math::Math::new(vec![equashion_math::Node::Text(symbol.into())]), false)
    } else {
        return Err(super::bad("equation.template", "choose a template, built-in equation, or symbol"));
    };
    if active(s.doc()?).is_none() {
        super::insert_equation(s, &json!({}))?;
    }
    let Some(mut ed) = editor(s.doc()?) else {
        return Err(super::bad("equation.template", "no equation"));
    };
    if replace {
        ed = Editor::new(math);
    } else {
        ed.insert_template(&math);
    }
    store(s, &ed)
}
