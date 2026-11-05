//! Equations: insert (empty, from the gallery, linear format or LaTeX), edit in place (type,
//! structures, symbols, caret movement, delete, build-up), convert between built-up and linear
//! form, inline/display, justification and equation numbers.

use deckcraft_geom::{Affine, Point, Rect};
use deckcraft_model::ShapeKind;
use deckcraft_model::edit as ed;
use deckcraft_model::math::{Arg, MNode, MRun, Math, MathJc, has_number_mark, math_class, parse_linear, to_linear};
use deckcraft_model::math_edit::{self as me, MathPos, TypeStyle};
use deckcraft_model::math_latex::{parse_latex, to_latex};
use deckcraft_model::text::{Paragraph, Run, RunKind, TextBody};
use serde_json::{Value, json};

use super::text::{body_of, edit_text, text_frame};
use super::{CommandSpec, always, bad, bool_or, bool_param, cmd, has_doc, has_slide, str_param, usize_param};
use crate::{DocState, EngineError, MathEdit, Result, Session};

const MAX_EQUATION_TYPE_CHARS: usize = 4_096;
const MAX_EQUATION_DELETE_UNITS: usize = 4_096;

/// Returns Ok if an equation is currently being edited.
pub fn is_editing_equation(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.selection.math.is_some()) { Ok(()) } else { Err("not editing an equation".into()) }
}

/// Returns Ok if the caret is within or adjacent to an equation.
pub fn is_near_equation(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.selection.math.is_some() || equation_at_caret_in(d).is_some()) {
        Ok(())
    } else {
        Err("no equation at the caret".into())
    }
}

/// Returns command specifications for all equation editing and formatting commands.
pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "insert.equation",
            "Equation",
            ["Insert", "Symbols"],
            Some("Alt+="),
            "{id?: templateId, linear?: string, latex?: string, builtin?: pythagoras|area|quadratic|binomial|expansion|euler|derivative|taylor|gaussian|fourier|trig1|trig2|normal|bayes, display?: bool, edit?: bool}",
            has_slide,
            insert
        ),
        cmd!(
            noundo "equation.edit",
            "Edit Equation",
            ["Equation"],
            None,
            "{pos?: {path, off}}",
            is_near_equation,
            edit
        ),
        cmd!(
            noundo "equation.exit",
            "Leave Equation",
            ["Equation"],
            None,
            "{before?: bool}",
            is_editing_equation,
            exit
        ),
        cmd!("equation.type", "Type in Equation", ["Equation"], None, "{text}", is_editing_equation, type_text),
        cmd!(
            "equation.insertStructure",
            "Insert Structure",
            ["Equation", "Structures"],
            None,
            "{id?: string, linear?: string, latex?: string}",
            has_slide,
            insert_structure
        ),
        cmd!("equation.insertSymbol", "Insert Symbol", ["Equation", "Symbols"], None, "{char}", has_slide, insert_symbol),
        cmd!(
            noundo "equation.move",
            "Move in Equation",
            ["Equation"],
            None,
            "{dir?: left|right|up|down|home|end|next|prev}",
            is_editing_equation,
            move_caret
        ),
        cmd!("equation.backspace", "Delete Back in Equation", ["Equation"], None, "{}", is_editing_equation, |s, _| delete(s, true)),
        cmd!("equation.delete", "Delete in Equation", ["Equation"], None, "{}", is_editing_equation, |s, _| delete(s, false)),
        CommandSpec {
            id: "equation.deleteSurrounding",
            label: "Delete Around Caret in Equation",
            menu: &["Equation"],
            shortcut: None,
            params: "{before, after}",
            enabled: is_editing_equation,
            run: delete_surrounding,
            journal: false,
            undoable: true,
        },
        cmd!("equation.enter", "Enter in Equation", ["Equation"], None, "{}", is_editing_equation, enter),
        cmd!("equation.convert", "Convert", ["Equation", "Conversions"], None, "{to?: professional|linear, all?: bool}", has_doc, convert),
        cmd!(
            query "equation.inputFormat",
            "Input Format",
            ["Equation", "Conversions"],
            None,
            "{format?: unicode|latex}",
            always,
            |s, v| {
                if let Some(f) = str_param(v, "format") {
                    s.math_latex = f.eq_ignore_ascii_case("latex");
                }
                Ok(json!({"format": if s.math_latex { "latex" } else { "unicode" }}))
            }
        ),
        cmd!(
            query "equation.normalText",
            "Normal Text",
            ["Equation", "Tools"],
            None,
            "{value?: bool}",
            always,
            |s, v| {
                s.math_normal_text = bool_param(v, "value").unwrap_or(!s.math_normal_text);
                Ok(json!({"normalText": s.math_normal_text}))
            }
        ),
        cmd!("equation.display", "Change to Display", ["Equation"], None, "{value?: bool}", is_near_equation, set_display),
        cmd!("equation.justify", "Justification", ["Equation"], None, "{jc?: centerGroup|center|left|right}", is_near_equation, justify),
        cmd!("equation.number", "Equation Number", ["Equation"], None, "{value?: bool}", is_near_equation, number),
        cmd!(
            "equation.set",
            "Set Equation",
            ["Equation"],
            None,
            "{id?: string, linear?: string, latex?: string, builtin?: string}",
            is_near_equation,
            set
        ),
        cmd!(
            query "equation.get",
            "Equation Info",
            ["Equation"],
            None,
            "{}",
            is_near_equation,
            get
        ),
        cmd!(
            query "equation.gallery",
            "Equation Gallery",
            ["Equation", "Structures"],
            None,
            "{}",
            always,
            gallery
        ),
        cmd!(
            query "equation.structureActions",
            "Structure Actions",
            ["Equation"],
            None,
            "{}",
            is_editing_equation,
            structure_actions
        ),
        cmd!("equation.structure", "Change Structure", ["Equation"], None, "{action, level?: n}", is_editing_equation, structure),
    ]
}

/// Find the `RunKind::Math` run starting at `pos = (para, ch_start)` in `body`.
/// Returns `(run_index, char_start, char_len, &Run)`.
pub fn find_math_run_at(body: &TextBody, pos: (usize, usize)) -> Option<(usize, usize, usize, &Run)> {
    let para = body.paragraphs.get(pos.0)?;
    let mut ch = 0usize;
    for (ri, r) in para.runs.iter().enumerate() {
        let len = r.char_len();
        if ch == pos.1 && matches!(r.kind, RunKind::Math { .. }) {
            return Some((ri, ch, len, r));
        }
        ch += len;
    }
    None
}

/// Finds mutable `RunKind::Math` run starting at `pos = (para, ch_start)` in `body`.
fn find_math_run_mut(body: &mut TextBody, pos: (usize, usize)) -> Option<(usize, usize, &mut Run)> {
    let para = body.paragraphs.get_mut(pos.0)?;
    let mut ch = 0usize;
    for r in &mut para.runs {
        let len = r.char_len();
        if ch == pos.1 && matches!(r.kind, RunKind::Math { .. }) {
            return Some((ch, len, r));
        }
        ch += len;
    }
    None
}

/// Find the `(para, ch_start)` of the equation at or adjacent to the text caret in `st`.
pub fn equation_at_caret_in(st: &DocState) -> Option<(usize, usize)> {
    if let Some(m) = &st.selection.math {
        return Some(m.pos);
    }
    let t = st.selection.text.as_ref()?;
    let body = body_of(st, t)?;
    let (pi, ci) = t.caret;
    let para = body.paragraphs.get(pi)?;
    let mut ch = 0usize;
    let mut just_before = None;
    for r in &para.runs {
        let len = r.char_len();
        if matches!(r.kind, RunKind::Math { .. }) {
            if ci >= ch && ci < ch + len {
                return Some((pi, ch));
            }
            if ci == ch + len {
                just_before = Some((pi, ch));
            }
        }
        ch += len;
    }
    if just_before.is_some() {
        return just_before;
    }
    let (a, b) = t.ordered();
    if a.0 == b.0 && find_math_run_at(body, a).is_some_and(|(_, s, l, _)| s + l == b.1) {
        return Some(a);
    }
    None
}

/// Flattens a slice of math nodes into unformatted plain text.
fn plain(nodes: &[MNode]) -> String {
    let mut s = String::new();
    for n in nodes {
        if let MNode::Run(r) = n {
            s.push_str(&r.text);
        }
    }
    s
}

/// Applies a mutating closure to the equation at position `at`.
fn with_eq<R>(s: &mut Session, at: (usize, usize), f: impl FnOnce(&mut Math, &mut bool) -> R) -> Result<R> {
    edit_text(s, |body, t| {
        let Some((ch_start, _, r)) = find_math_run_mut(body, at) else {
            return Err(EngineError::Other("no equation there".into()));
        };
        let RunKind::Math { omml, math, display } = &mut r.kind else {
            return Err(EngineError::Other("no equation there".into()));
        };
        if math.nodes.is_empty() && !r.text.is_empty() && !math.linear {
            math.nodes = parse_linear(&r.text);
        }
        let res = f(math, display);
        r.text = if math.linear { plain(&math.nodes) } else { to_linear(&math.nodes) };
        omml.clear();
        t.anchor = (at.0, ch_start);
        t.caret = (at.0, ch_start);
        Ok(res)
    })?
}

/// Returns the nodes of the equation at `at`, parsing if not yet loaded.
fn nodes_of(st: &DocState, at: (usize, usize)) -> Option<Arg> {
    let t = st.selection.text.as_ref()?;
    let body = body_of(st, t)?;
    let (_, _, _, r) = find_math_run_at(body, at)?;
    let RunKind::Math { math, .. } = &r.kind else { return None };
    Some(if math.nodes.is_empty() && !r.text.is_empty() && !math.linear { parse_linear(&r.text) } else { math.nodes.clone() })
}

/// Builds the JSON response describing current equation state.
fn result(s: &Session) -> Result<Value> {
    let st = s.doc()?;
    let Some(m) = &st.selection.math else {
        let caret = st.selection.text.as_ref().map(|t| [t.caret.0, t.caret.1]);
        return Ok(json!({"editing": false, "caret": caret}));
    };
    let info =
        st.selection.text.as_ref().and_then(|t| body_of(st, t)).and_then(|b| find_math_run_at(b, m.pos)).and_then(|(_, _, _, r)| match &r.kind {
            RunKind::Math { math, display, .. } => {
                let lin = if !r.text.is_empty() { r.text.clone() } else { to_linear(&math.nodes) };
                Some((lin, *display))
            }
            _ => None,
        });
    let (linear, display) = info.unwrap_or_default();
    Ok(json!({
        "editing": true,
        "linear": linear,
        "display": display,
        "pos": serde_json::to_value(&m.caret).unwrap_or(Value::Null),
        "at": [m.pos.0, m.pos.1]
    }))
}

fn nodes_from(v: &Value) -> Option<Arg> {
    if let Some(id) = str_param(v, "id") {
        return crate::math_gallery::template(id).map(|t| t.nodes());
    }
    if let Some(b) = str_param(v, "builtin") {
        return crate::math_gallery::BUILT_INS.iter().find(|(id, _, _)| *id == b).map(|(_, _, lin)| parse_linear(lin));
    }
    if let Some(l) = str_param(v, "latex") {
        return Some(parse_latex(l));
    }
    str_param(v, "linear").map(parse_linear)
}

fn ensure_text_editing(s: &mut Session) -> Result<()> {
    if s.doc()?.selection.text.is_some() {
        return Ok(());
    }
    let can_edit_selected = {
        let st = s.doc()?;
        if st.selection.shapes.len() == 1 {
            let id = st.selection.shapes[0];
            st.shape(id).is_some_and(|sh| matches!(sh.kind, ShapeKind::Shape | ShapeKind::Connector { .. }))
        } else {
            false
        }
    };
    if can_edit_selected {
        s.execute("text.edit", &json!({}))?;
        return Ok(());
    }
    let size = s.doc()?.doc.slide_size;
    s.execute("insert.textBox", &json!({"rect": [size.width / 2.0 - 150.0, size.height / 2.0 - 25.0, 300.0, 50.0], "text": ""}))?;
    Ok(())
}

fn selected_text_of(st: &DocState) -> String {
    let Some(t) = &st.selection.text else { return String::new() };
    if !t.is_range() {
        return String::new();
    }
    let Some(body) = body_of(st, t) else { return String::new() };
    let (a, b) = t.ordered();
    ed::text_range(body, a, b)
}

fn insert(s: &mut Session, v: &Value) -> Result<Value> {
    ensure_text_editing(s)?;
    let selected = selected_text_of(s.doc()?);
    let nodes = match nodes_from(v) {
        Some(n) => n,
        None if str_param(v, "id").is_some() || str_param(v, "builtin").is_some() => {
            return Err(bad("insert.equation", "unknown equation"));
        }
        None => parse_linear(selected.trim()),
    };
    insert_nodes_as_equation(s, nodes, v)
}

fn insert_nodes_as_equation(s: &mut Session, nodes: Arg, v: &Value) -> Result<Value> {
    ensure_text_editing(s)?;
    let display_opt = bool_param(v, "display");
    let edit_after = bool_or(v, "edit", true);
    let linear = to_linear(&nodes);
    let caret = match me::next_placeholder(&nodes, &MathPos::default(), false) {
        Some(p) if !nodes.is_empty() => p,
        _ => MathPos::new(Vec::new(), me::units(&nodes)),
    };
    let (eq_pos, end_pos) = edit_text(s, |body, t| {
        let (a, b) = t.ordered();
        let at = ed::delete(body, a, b);
        let props = ed::props_at(body, at);
        let empty_line = body.paragraphs.get(at.0).is_none_or(|p| p.text().trim().is_empty());
        let display = display_opt.unwrap_or(empty_line);
        let src = TextBody {
            paragraphs: vec![Paragraph {
                runs: vec![Run {
                    text: linear.clone(),
                    props,
                    kind: RunKind::Math { omml: String::new(), math: Math { nodes: nodes.clone(), ..Default::default() }, display },
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let end = ed::insert_body(body, at, &src);
        if edit_after {
            t.anchor = at;
            t.caret = at;
        } else {
            t.anchor = end;
            t.caret = end;
        }
        (at, end)
    })?;
    let _ = end_pos;
    if let Some(st) = s.active_mut() {
        st.selection.math = if edit_after { Some(MathEdit { pos: eq_pos, caret }) } else { None };
    }
    result(s)
}

fn edit(s: &mut Session, v: &Value) -> Result<Value> {
    let at = {
        let st = s.doc()?;
        equation_at_caret_in(st).ok_or_else(|| EngineError::Disabled("equation.edit".into(), "no equation at the caret".into()))?
    };
    let nodes = nodes_of(s.doc()?, at).ok_or_else(|| EngineError::Other("no equation there".into()))?;
    let pos =
        v.get("pos").and_then(|p| serde_json::from_value::<MathPos>(p.clone()).ok()).unwrap_or_else(|| MathPos::new(Vec::new(), me::units(&nodes)));
    let caret = me::clamp(&nodes, &pos);
    s.select(|_, sel| {
        if let Some(t) = sel.text.as_mut() {
            t.anchor = at;
            t.caret = at;
        }
        sel.math = Some(MathEdit { pos: at, caret });
    })?;
    result(s)
}

fn exit(s: &mut Session, v: &Value) -> Result<Value> {
    let before = bool_or(v, "before", false);
    s.select(|doc, sel| {
        if let Some(m) = sel.math.take()
            && let Some((shape, cell, notes)) = sel.text.as_ref().map(|t| (t.shape, t.cell, t.notes))
        {
            let len = if notes {
                doc.slides.get(sel.slide).and_then(|sl| find_math_run_at(&sl.notes, m.pos)).map_or(1, |(_, _, l, _)| l)
            } else {
                super::current_shapes(doc, sel)
                    .and_then(|list| deckcraft_model::find_shape(list, shape))
                    .and_then(|sh| match (cell, &sh.kind) {
                        (Some((r, c)), ShapeKind::Table(tb)) => tb.cell(r, c).map(|x| &x.text),
                        _ => sh.text.as_ref(),
                    })
                    .and_then(|b| find_math_run_at(b, m.pos))
                    .map_or(1, |(_, _, l, _)| l)
            };
            let ch = if before { m.pos.1 } else { m.pos.1 + len };
            if let Some(t) = sel.text.as_mut() {
                t.anchor = (m.pos.0, ch);
                t.caret = (m.pos.0, ch);
            }
        }
    })?;
    result(s)
}

fn current(s: &Session) -> Result<MathEdit> {
    s.doc()?.selection.math.clone().ok_or_else(|| EngineError::Disabled("equation".into(), "not editing an equation".into()))
}

fn set_literal(nodes: &mut Arg) {
    for n in nodes.iter_mut() {
        if let MNode::Run(r) = n {
            r.lit = true;
        }
    }
}

/// Types text into the active equation, triggering autocorrect or build-up on delimiters.
fn type_text(s: &mut Session, v: &Value) -> Result<Value> {
    let text = str_param(v, "text").ok_or_else(|| bad("equation.type", "missing `text`"))?.to_string();
    if text.chars().count() > MAX_EQUATION_TYPE_CHARS {
        return Err(bad("equation.type", format!("text exceeds maximum of {MAX_EQUATION_TYPE_CHARS} characters")));
    }
    let m = current(s)?;
    if text.is_empty() {
        return result(s);
    }
    let latex = s.math_latex;
    let style = TypeStyle { nor: s.math_normal_text };
    let mut pos = m.caret.clone();
    with_eq(s, m.pos, |math, _| {
        if math.linear {
            pos = me::insert_text(&mut math.nodes, &pos, &text, TypeStyle::default());
            set_literal(&mut math.nodes);
            return;
        }
        for c in text.chars() {
            let rel = matches!(math_class(c), deckcraft_model::math::MClass::Rel);
            if !style.nor && (c == ' ' || c == '\t' || rel) {
                let mut changed = false;
                if let Some(p) = me::autocorrect(&mut math.nodes, &pos) {
                    pos = p;
                    changed = true;
                }
                if let Some(p) = me::build_up(&mut math.nodes, &pos, latex) {
                    pos = p;
                    changed = true;
                }
                if c == ' ' && changed {
                    continue;
                }
            }
            pos = me::insert_text(&mut math.nodes, &pos, &c.to_string(), style);
        }
    })?;
    if let Some(st) = s.active_mut()
        && let Some(me_st) = st.selection.math.as_mut()
    {
        me_st.caret = pos;
    }
    result(s)
}

/// Inserts math nodes into the currently active equation selection, or creates a new equation if none is active.
pub(crate) fn insert_nodes_into_active(s: &mut Session, nodes: Vec<MNode>) -> Result<Value> {
    let Some(m) = s.doc()?.selection.math.clone() else {
        return insert_nodes_as_equation(s, nodes, &json!({}));
    };
    let mut pos = m.caret.clone();
    with_eq(s, m.pos, |math, _| {
        if math.linear {
            pos = me::insert_text(&mut math.nodes, &pos, &to_linear(&nodes), TypeStyle::default());
            set_literal(&mut math.nodes);
        } else {
            pos = me::insert_nodes(&mut math.nodes, &pos, nodes);
        }
    })?;
    if let Some(st) = s.active_mut()
        && let Some(me_st) = st.selection.math.as_mut()
    {
        me_st.caret = pos;
    }
    result(s)
}

/// Inserts a template or structure into the active equation from template id, linear string, or LaTeX.
fn insert_structure(s: &mut Session, v: &Value) -> Result<Value> {
    let nodes = nodes_from(v).ok_or_else(|| bad("equation.insertStructure", "give an id, linear or latex"))?;
    insert_nodes_into_active(s, nodes)
}

fn insert_symbol(s: &mut Session, v: &Value) -> Result<Value> {
    let c = str_param(v, "char").ok_or_else(|| bad("equation.insertSymbol", "missing `char`"))?.to_string();
    if c.is_empty() {
        return Err(bad("equation.insertSymbol", "empty char"));
    }
    let Some(m) = s.doc()?.selection.math.clone() else {
        return insert_nodes_as_equation(s, parse_linear(&c), &json!({}));
    };
    let mut pos = m.caret.clone();
    let style = TypeStyle { nor: s.math_normal_text };
    with_eq(s, m.pos, |math, _| {
        pos = me::insert_text(&mut math.nodes, &pos, &c, style);
        if math.linear {
            set_literal(&mut math.nodes);
        }
    })?;
    if let Some(st) = s.active_mut()
        && let Some(me_st) = st.selection.math.as_mut()
    {
        me_st.caret = pos;
    }
    result(s)
}

fn move_caret(s: &mut Session, v: &Value) -> Result<Value> {
    let m = current(s)?;
    let dir = str_param(v, "dir").unwrap_or("right");
    if !matches!(dir, "left" | "right" | "up" | "down" | "home" | "end" | "next" | "prev") {
        return Err(bad("equation.move", format!("unknown direction {dir}")));
    }
    let st = s.doc()?;
    let shown_linear = st
        .selection
        .text
        .as_ref()
        .and_then(|t| body_of(st, t))
        .and_then(|b| find_math_run_at(b, m.pos))
        .is_some_and(|(_, _, _, r)| matches!(&r.kind, RunKind::Math { math, .. } if math.linear));
    let mut nodes = nodes_of(st, m.pos).ok_or_else(|| EngineError::Other("no equation there".into()))?;
    let mut pos = m.caret.clone();
    if !s.math_normal_text && !shown_linear {
        let latex = s.math_latex;
        let before = nodes.clone();
        let mut candidate = nodes.clone();
        let mut candidate_pos = pos.clone();
        let mut changed = false;
        if let Some(p) = me::autocorrect(&mut candidate, &candidate_pos) {
            candidate_pos = p;
            changed = true;
        }
        if let Some(p) = me::build_up(&mut candidate, &candidate_pos, latex) {
            candidate_pos = p;
            changed = true;
        }
        pos = candidate_pos;
        if changed && candidate != before {
            let old_doc = s.doc()?.doc.clone();
            let old_sel = s.doc()?.selection.clone();
            let vk = s.tool.vertex_kind.clone();
            with_eq(s, m.pos, |math, _| math.nodes = candidate.clone())?;
            if let Some(st) = s.active_mut() {
                crate::push_undo(st, crate::HistoryEntry { label: "Build Up".into(), doc: old_doc, selection: old_sel, vertex_kind: vk });
            }
            nodes = candidate;
        }
    }
    let next = match dir {
        "left" => me::move_left(&nodes, &pos),
        "right" => me::move_right(&nodes, &pos),
        "up" => Some(me::move_vertical(&nodes, &pos, true).unwrap_or(pos.clone())),
        "down" => Some(me::move_vertical(&nodes, &pos, false).unwrap_or(pos.clone())),
        "home" => Some(me::home_end(&nodes, &pos, false)),
        "end" => Some(me::home_end(&nodes, &pos, true)),
        "next" | "prev" => Some(me::next_placeholder(&nodes, &pos, dir == "prev").unwrap_or(pos.clone())),
        other => return Err(bad("equation.move", format!("unknown direction {other}"))),
    };
    match next {
        Some(p) => {
            s.select(|_, sel| {
                if let Some(me_st) = sel.math.as_mut() {
                    me_st.caret = p;
                }
            })?;
            result(s)
        }
        None => exit(s, &json!({"before": dir == "left"})),
    }
}

fn delete(s: &mut Session, back: bool) -> Result<Value> {
    let m = current(s)?;
    let nodes = nodes_of(s.doc()?, m.pos).ok_or_else(|| EngineError::Other("no equation there".into()))?;
    if nodes.is_empty() {
        edit_text(s, |body, t| {
            if let Some((_, ch_start, len, _)) = find_math_run_at(body, m.pos) {
                let at = ed::delete(body, (m.pos.0, ch_start), (m.pos.0, ch_start + len));
                t.anchor = at;
                t.caret = at;
            }
        })?;
        if let Some(st) = s.active_mut() {
            st.selection.math = None;
        }
        return result(s);
    }
    let mut pos = m.caret.clone();
    with_eq(s, m.pos, |math, _| {
        pos = if back { me::delete_back(&mut math.nodes, &pos) } else { me::delete_forward(&mut math.nodes, &pos) };
    })?;
    if let Some(st) = s.active_mut()
        && let Some(me_st) = st.selection.math.as_mut()
    {
        me_st.caret = pos;
    }
    result(s)
}

fn delete_surrounding(s: &mut Session, v: &Value) -> Result<Value> {
    let m = current(s)?;
    let before = usize_param(v, "before").unwrap_or(usize::MAX);
    let after = usize_param(v, "after").unwrap_or(usize::MAX);
    if before.saturating_add(after) == 0 {
        return Err(bad("equation.deleteSurrounding", "nothing to delete"));
    }
    if before > MAX_EQUATION_DELETE_UNITS || after > MAX_EQUATION_DELETE_UNITS {
        return Err(bad("equation.deleteSurrounding", format!("at most {MAX_EQUATION_DELETE_UNITS} units on each side")));
    }
    let mut nodes = nodes_of(s.doc()?, m.pos).ok_or_else(|| EngineError::Other("no equation there".into()))?;
    let mut pos = m.caret.clone();
    for _ in 0..before {
        let prev_nodes = nodes.clone();
        let prev_pos = pos.clone();
        pos = me::delete_back(&mut nodes, &pos);
        if nodes == prev_nodes {
            return Err(EngineError::Other(if pos == prev_pos {
                "the equation cannot delete that many units".into()
            } else {
                "the equation delete made no progress".into()
            }));
        }
    }
    for _ in 0..after {
        let prev_nodes = nodes.clone();
        let prev_pos = pos.clone();
        pos = me::delete_forward(&mut nodes, &pos);
        if nodes == prev_nodes {
            return Err(EngineError::Other(if pos == prev_pos {
                "the equation cannot delete that many units".into()
            } else {
                "the equation delete made no progress".into()
            }));
        }
    }
    with_eq(s, m.pos, |math, _| math.nodes = nodes)?;
    if let Some(st) = s.active_mut()
        && let Some(me_st) = st.selection.math.as_mut()
    {
        me_st.caret = pos;
    }
    Ok(json!({"before": before, "after": after, "changed": true}))
}

fn enter(s: &mut Session, _: &Value) -> Result<Value> {
    let m = current(s)?;
    let latex = s.math_latex;
    let mut pos = m.caret.clone();
    let mut changed = false;
    with_eq(s, m.pos, |math, _| {
        if math.linear {
            return;
        }
        if let Some(p) = me::autocorrect(&mut math.nodes, &pos) {
            pos = p;
            changed = true;
        }
        if let Some(p) = me::build_up(&mut math.nodes, &pos, latex) {
            pos = p;
            changed = true;
        }
        if me::number_equation(&mut math.nodes) {
            pos = MathPos::new(Vec::new(), me::units(&math.nodes));
            changed = true;
        }
    })?;
    if changed {
        if let Some(st) = s.active_mut()
            && let Some(me_st) = st.selection.math.as_mut()
        {
            me_st.caret = pos;
        }
        return result(s);
    }
    exit(s, &json!({"before": false}))?;
    s.execute("text.insert", &json!({"text": "\n"}))
}

fn convert_math(math: &mut Math, professional: bool, latex: bool) {
    if professional {
        if math.linear {
            let text = plain(&math.nodes);
            math.nodes = if latex { parse_latex(&text) } else { parse_linear(&text) };
            math.linear = false;
        }
    } else if !math.linear {
        let text = if latex { to_latex(&math.nodes) } else { to_linear(&math.nodes) };
        math.nodes = vec![MNode::Run(MRun { text, lit: true, ..Default::default() })];
        math.linear = true;
    }
}

fn convert(s: &mut Session, v: &Value) -> Result<Value> {
    let professional = str_param(v, "to").map(|t| !t.eq_ignore_ascii_case("linear")).unwrap_or(true);
    let latex = s.math_latex;
    if bool_or(v, "all", false) {
        let n = edit_text(s, |body, _| {
            let mut count = 0usize;
            for p in &mut body.paragraphs {
                for r in &mut p.runs {
                    if let RunKind::Math { omml, math, .. } = &mut r.kind {
                        if math.nodes.is_empty() && !r.text.is_empty() && !math.linear {
                            math.nodes = parse_linear(&r.text);
                        }
                        convert_math(math, professional, latex);
                        r.text = if math.linear { plain(&math.nodes) } else { to_linear(&math.nodes) };
                        omml.clear();
                        count += 1;
                    }
                }
            }
            count
        })?;
        if let Some(st) = s.active_mut()
            && let Some(m) = st.selection.math.as_mut()
        {
            m.caret = MathPos::default();
        }
        return Ok(json!({"converted": n}));
    }
    let at = {
        let st = s.doc()?;
        equation_at_caret_in(st).ok_or_else(|| EngineError::Disabled("equation.convert".into(), "no equation at the caret".into()))?
    };
    let mut end = 0;
    with_eq(s, at, |math, _| {
        convert_math(math, professional, latex);
        end = me::units(&math.nodes);
    })?;
    if let Some(st) = s.active_mut()
        && let Some(m) = st.selection.math.as_mut()
    {
        m.caret = MathPos::new(Vec::new(), end);
    }
    result(s)
}

fn set_display(s: &mut Session, v: &Value) -> Result<Value> {
    let at = {
        let st = s.doc()?;
        equation_at_caret_in(st).ok_or_else(|| EngineError::Disabled("equation.display".into(), "no equation at the caret".into()))?
    };
    with_eq(s, at, |_, display| *display = bool_param(v, "value").unwrap_or(!*display))?;
    result(s)
}

fn justify(s: &mut Session, v: &Value) -> Result<Value> {
    let at = {
        let st = s.doc()?;
        equation_at_caret_in(st).ok_or_else(|| EngineError::Disabled("equation.justify".into(), "no equation at the caret".into()))?
    };
    let jc = match str_param(v, "jc").unwrap_or("centerGroup") {
        "left" => MathJc::Left,
        "right" => MathJc::Right,
        "center" => MathJc::Center,
        _ => MathJc::CenterGroup,
    };
    with_eq(s, at, |math, display| {
        math.jc = jc;
        *display = true;
    })?;
    result(s)
}

fn without_number(row: Arg) -> Arg {
    let mut out = Vec::new();
    for n in row {
        match n {
            MNode::Run(mut r) if !r.lit && r.text.contains('#') => {
                if let Some(k) = r.text.find('#') {
                    r.text.truncate(k);
                }
                if !r.text.is_empty() {
                    out.push(MNode::Run(r));
                }
                break;
            }
            n => out.push(n),
        }
    }
    out
}

fn number(s: &mut Session, v: &Value) -> Result<Value> {
    let at = {
        let st = s.doc()?;
        equation_at_caret_in(st).ok_or_else(|| EngineError::Disabled("equation.number".into(), "no equation at the caret".into()))?
    };
    let mut pos_reset = false;
    with_eq(s, at, |math, display| {
        let numbered = matches!(math.nodes.as_slice(), [MNode::EqArr { rows }] if rows.iter().any(|r| has_number_mark(r)));
        let want = bool_param(v, "value").unwrap_or(!numbered);
        if want && !numbered {
            let mut row = std::mem::take(&mut math.nodes);
            row.push(MNode::Run(MRun::new("#")));
            math.nodes = vec![MNode::EqArr { rows: vec![row] }];
            *display = true;
            pos_reset = true;
        } else if !want && numbered {
            if let [MNode::EqArr { rows }] = math.nodes.as_mut_slice() {
                let mut rows: Vec<Arg> = std::mem::take(rows).into_iter().map(without_number).collect();
                math.nodes = if rows.len() == 1 { rows.pop().unwrap_or_default() } else { vec![MNode::EqArr { rows }] };
            }
            pos_reset = true;
        }
    })?;
    if pos_reset
        && let Some(st) = s.active_mut()
        && let Some(m) = st.selection.math.as_mut()
    {
        m.caret = MathPos::default();
    }
    result(s)
}

fn set(s: &mut Session, v: &Value) -> Result<Value> {
    let at = {
        let st = s.doc()?;
        equation_at_caret_in(st).ok_or_else(|| EngineError::Disabled("equation.set".into(), "no equation at the caret".into()))?
    };
    let nodes = nodes_from(v).ok_or_else(|| bad("equation.set", "give linear or latex"))?;
    let end = me::units(&nodes);
    with_eq(s, at, |math, _| {
        math.nodes = nodes;
        math.linear = false;
    })?;
    if let Some(st) = s.active_mut()
        && let Some(m) = st.selection.math.as_mut()
    {
        m.caret = MathPos::new(Vec::new(), end);
    }
    result(s)
}

fn get(s: &mut Session, _: &Value) -> Result<Value> {
    let st = s.doc()?;
    let at = equation_at_caret_in(st).ok_or_else(|| EngineError::Disabled("equation.get".into(), "no equation at the caret".into()))?;
    let t = st.selection.text.as_ref().ok_or_else(|| EngineError::Disabled("equation.get".into(), "no equation at the caret".into()))?;
    let body = body_of(st, t).ok_or_else(|| EngineError::Other("no equation there".into()))?;
    let (_, _, _, r) = find_math_run_at(body, at).ok_or_else(|| EngineError::Other("no equation there".into()))?;
    let RunKind::Math { math, display, .. } = &r.kind else {
        return Err(EngineError::Other("no equation there".into()));
    };
    let nodes = if math.nodes.is_empty() && !r.text.is_empty() && !math.linear { parse_linear(&r.text) } else { math.nodes.clone() };
    let linear = if !r.text.is_empty() { r.text.clone() } else { to_linear(&nodes) };
    Ok(json!({
        "linear": linear,
        "latex": to_latex(&nodes),
        "display": *display,
        "jc": math.jc,
        "linearForm": math.linear,
        "numbered": matches!(math.nodes.as_slice(), [MNode::EqArr { rows }] if rows.iter().any(|r| has_number_mark(r))),
        "editing": st.selection.math.is_some(),
        "pos": st.selection.math.as_ref().map(|m| serde_json::to_value(&m.caret).unwrap_or(Value::Null)),
        "at": [at.0, at.1],
    }))
}

fn structure_actions(s: &mut Session, _: &Value) -> Result<Value> {
    let m = current(s)?;
    let nodes = nodes_of(s.doc()?, m.pos).ok_or_else(|| EngineError::Other("no equation there".into()))?;
    let acts: Vec<Value> =
        me::structure_actions(&nodes, &m.caret).into_iter().map(|(l, a, label)| json!({"level": l, "action": a, "label": label})).collect();
    Ok(json!({"actions": acts}))
}

fn structure(s: &mut Session, v: &Value) -> Result<Value> {
    let m = current(s)?;
    let action = str_param(v, "action").ok_or_else(|| bad("equation.structure", "missing `action`"))?.to_string();
    let nodes = nodes_of(s.doc()?, m.pos).ok_or_else(|| EngineError::Other("no equation there".into()))?;
    let level = match usize_param(v, "level") {
        Some(l) => l,
        None => me::structure_actions(&nodes, &m.caret)
            .into_iter()
            .find(|(_, a, _)| *a == action)
            .map(|(l, _, _)| l)
            .ok_or_else(|| bad("equation.structure", format!("{action} doesn't apply here")))?,
    };
    let mut pos = None;
    with_eq(s, m.pos, |math, _| pos = me::apply_structure(&mut math.nodes, &m.caret, level, &action))?;
    let pos = pos.ok_or_else(|| bad("equation.structure", format!("{action} doesn't apply here")))?;
    if let Some(st) = s.active_mut()
        && let Some(me_st) = st.selection.math.as_mut()
    {
        me_st.caret = pos;
    }
    result(s)
}

fn gallery(_: &mut Session, _: &Value) -> Result<Value> {
    let structures: Vec<Value> = crate::math_gallery::STRUCTURES
        .iter()
        .map(|g| {
            json!({
                "id": g.id,
                "label": g.label.replace('\n', " "),
                "sections": g.sections.iter().map(|(name, ts)| json!({
                    "name": name,
                    "templates": ts.iter().map(|t| json!({"id": t.id, "label": t.label, "linear": t.linear})).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let symbols: Vec<Value> = crate::math_gallery::SYMBOL_SETS.iter().map(|(n, chars)| json!({"name": n, "chars": chars})).collect();
    let builtins: Vec<Value> = crate::math_gallery::BUILT_INS.iter().map(|(id, l, lin)| json!({"id": id, "label": l, "linear": lin})).collect();
    Ok(json!({"structures": structures, "symbols": symbols, "builtins": builtins}))
}

// ---------- Canvas overlay helpers ----------

/// The `PlacedEquation` and its layout-to-slide transform for the equation currently being edited
/// (or at the caret).
pub fn placed_equation(st: &DocState) -> Option<(deckcraft_text::PlacedEquation, Affine)> {
    let t = st.selection.text.as_ref()?;
    let at = equation_at_caret_in(st)?;
    let (layout, to_slide) = text_frame(st, t)?;
    let eq = layout.equations.into_iter().find(|e| (e.para, e.ch) == at)?;
    Some((eq, to_slide))
}

/// The caret line `(top, bottom)` in slide coordinates for the active equation.
pub fn math_caret_line(st: &DocState) -> Option<(Point, Point)> {
    let m = st.selection.math.as_ref()?;
    let (eq, to_slide) = placed_equation(st)?;
    let s = eq.math.slot(&m.caret.path, m.caret.off)?;
    let x = eq.x + f64::from(s.x);
    let y0 = eq.base - f64::from(s.y + s.a);
    let y1 = eq.base - f64::from(s.y - s.d);
    Some((to_slide * Point::new(x, y0), to_slide * Point::new(x, y1)))
}

/// The active argument box `(rect_in_layout, to_slide)` when the caret is inside a structure slot.
pub fn active_arg_rect(st: &DocState) -> Option<(Rect, Affine)> {
    let m = st.selection.math.as_ref()?;
    if m.caret.path.is_empty() {
        return None;
    }
    let (eq, to_slide) = placed_equation(st)?;
    let (x0, x1, up, down) = eq.math.arg_extent(&m.caret.path)?;
    let rect = Rect::new(eq.x + f64::from(x0) - 1.5, eq.base - f64::from(up) - 1.0, eq.x + f64::from(x1) + 1.5, eq.base + f64::from(down) + 1.0);
    Some((rect, to_slide))
}

/// Delete the entire active equation run from the active text body.
pub fn delete_entire_equation(s: &mut Session) -> Result<()> {
    let m = current(s)?;
    edit_text(s, |body, t| {
        if let Some((_ri, ch_start, len, _r)) = find_math_run_at(body, m.pos) {
            let at = ed::delete(body, (m.pos.0, ch_start), (m.pos.0, ch_start + len));
            t.caret = at;
            t.anchor = at;
        }
        t.caret
    })?;
    s.select(|_, sel| {
        sel.math = None;
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_equation_copy_cut_paste() {
        let mut s = Session::with_new();
        let id = s.execute("insert.textBox", &json!({"rect": [100.0, 100.0, 300.0, 50.0], "text": ""})).unwrap()["id"].as_u64().unwrap() as u32;
        s.execute("insert.equation", &json!({"linear": "∫_0^1 f(x)dx", "edit": true})).unwrap();
        assert!(s.active().unwrap().selection.math.is_some());

        // Copy while editing equation
        let r = s.execute("edit.copy", &json!({})).unwrap();
        assert!(!r["text"].as_str().unwrap().is_empty());
        assert!(s.clipboard.plain.contains("∫_0^1"));
        assert!(s.clipboard.plain.contains("f(x)dx"));
        assert!(s.clipboard.text.is_some());

        // Cut while editing equation
        let r_cut = s.execute("edit.cut", &json!({})).unwrap();
        assert!(r_cut["text"].as_str().unwrap().contains("∫_0^1"));
        assert!(s.active().unwrap().selection.math.is_none());

        // Paste back
        s.execute("edit.paste", &json!({})).unwrap();
        let body = s.doc().unwrap().shape(deckcraft_model::ShapeId(id)).unwrap().text.as_ref().unwrap();
        assert!(body.paragraphs[0].runs.iter().any(|r| matches!(&r.kind, RunKind::Math { .. })));
    }

    #[test]
    fn test_typing_integral_scripts() {
        let mut s = Session::with_new();
        let id = s.execute("insert.textBox", &json!({"rect": [100.0, 100.0, 300.0, 50.0], "text": ""})).unwrap()["id"].as_u64().unwrap() as u32;
        s.execute("insert.equation", &json!({"edit": true})).unwrap();
        // Type \int then space
        s.execute("equation.type", &json!({"text": "\\int "})).unwrap();
        // Type _0^1 with space
        s.execute("equation.type", &json!({"text": "_0^1 "})).unwrap();
        // Type f(x)dx
        s.execute("equation.type", &json!({"text": "f(x)dx"})).unwrap();

        let st = s.active().unwrap();
        let sh = st.shape(deckcraft_model::ShapeId(id)).unwrap();
        let math = &sh.text.as_ref().unwrap().paragraphs[0].runs[0];
        if let RunKind::Math { math, .. } = &math.kind {
            assert!(matches!(math.nodes.first(), Some(MNode::Nary { chr: '∫', sub_hide: false, sup_hide: false, .. })));
        } else {
            panic!("expected Math run");
        }
    }

    #[test]
    fn test_equation_commands() {
        let mut s = Session::with_new();
        let _id = s.execute("insert.textBox", &json!({"rect": [100.0, 100.0, 300.0, 50.0], "text": ""})).unwrap()["id"].as_u64().unwrap() as u32;
        s.execute("insert.equation", &json!({"linear": "(x+1)", "edit": true})).unwrap();

        // equation.display
        s.execute("equation.display", &json!({"value": false})).unwrap();
        let val = s.execute("equation.get", &json!({})).unwrap();
        assert_eq!(val["display"], false);
        s.execute("equation.display", &json!({"value": true})).unwrap();
        let val = s.execute("equation.get", &json!({})).unwrap();
        assert_eq!(val["display"], true);

        // equation.justify
        s.execute("equation.justify", &json!({"jc": "left"})).unwrap();
        let val = s.execute("equation.get", &json!({})).unwrap();
        assert_eq!(val["jc"], "left");
        s.execute("equation.justify", &json!({"jc": "right"})).unwrap();
        let val = s.execute("equation.get", &json!({})).unwrap();
        assert_eq!(val["jc"], "right");
        s.execute("equation.justify", &json!({"jc": "center"})).unwrap();

        // equation.number toggling on and off
        s.execute("equation.number", &json!({})).unwrap();
        let val = s.execute("equation.get", &json!({})).unwrap();
        assert!(val["numbered"].as_bool().unwrap_or(false));
        s.execute("equation.number", &json!({})).unwrap();
        let val = s.execute("equation.get", &json!({})).unwrap();
        assert!(!val["numbered"].as_bool().unwrap_or(true));

        // equation.convert: linear & professional, scope current & all
        s.execute("equation.convert", &json!({"to": "linear"})).unwrap();
        let val = s.execute("equation.get", &json!({})).unwrap();
        assert_eq!(val["linearForm"], true);
        s.execute("equation.convert", &json!({"to": "professional"})).unwrap();
        let val = s.execute("equation.get", &json!({})).unwrap();
        assert_eq!(val["linearForm"], false);
        s.execute("equation.convert", &json!({"to": "linear", "scope": "all"})).unwrap();
        let val = s.execute("equation.get", &json!({})).unwrap();
        assert_eq!(val["linearForm"], true);
        s.execute("equation.convert", &json!({"to": "professional", "scope": "all"})).unwrap();

        // equation.structureActions and equation.structure
        let acts = s.execute("equation.structureActions", &json!({})).unwrap();
        assert!(acts["actions"].is_array());

        // equation.deleteSurrounding error when not applicable
        assert!(s.execute("equation.deleteSurrounding", &json!({})).is_err());

        // equation.exit with before: false
        s.execute("equation.exit", &json!({"before": false})).unwrap();
        assert!(s.active().unwrap().selection.math.is_none());
        assert!(s.active().unwrap().selection.text.is_some());

        // Insert new equation and exit with before: true
        s.execute("insert.equation", &json!({"edit": true})).unwrap();
        assert!(s.active().unwrap().selection.math.is_some());
        s.execute("equation.exit", &json!({"before": true})).unwrap();
        assert!(s.active().unwrap().selection.math.is_none());
    }

    #[test]
    fn test_equation_move_build_up() {
        let mut s = Session::with_new();
        s.execute("insert.textBox", &json!({"rect": [100.0, 100.0, 300.0, 50.0], "text": ""})).unwrap();
        s.execute("insert.equation", &json!({"edit": true})).unwrap();
        // Insert text that can build up via autocorrect (\alpha) without space yet
        s.execute("equation.type", &json!({"text": "\\alpha"})).unwrap();
        let undo_count_before = s.active().unwrap().history.undo.len();
        // Move caret triggers autocorrect/build_up and records "Build Up" undo step
        s.execute("equation.move", &json!({"dir": "right"})).unwrap();
        let undo_count_after = s.active().unwrap().history.undo.len();
        assert!(undo_count_after > undo_count_before);
        assert_eq!(s.active().unwrap().history.undo.last().unwrap().label, "Build Up");
    }

    #[test]
    fn test_equation_paste_internal_vs_external() {
        let mut s = Session::with_new();
        s.execute("insert.textBox", &json!({"rect": [100.0, 100.0, 300.0, 50.0], "text": ""})).unwrap();
        s.execute("insert.equation", &json!({"linear": "1/2", "edit": true})).unwrap();

        // Copy internal equation (stored in s.clipboard as structured TextBody with RunKind::Math)
        let copied = s.execute("edit.copy", &json!({})).unwrap();
        assert_eq!(copied["text"], "1/2");

        // Create a new equation and paste internal clipboard
        s.execute("equation.exit", &json!({"before": false})).unwrap();
        s.execute("insert.equation", &json!({"edit": true})).unwrap();
        s.execute("edit.paste", &json!({})).unwrap();

        let val = s.execute("equation.get", &json!({})).unwrap();
        assert_eq!(val["linear"], "1/2");

        // Paste external text (differs from clipboard plain)
        s.execute("edit.paste", &json!({"text": "+y"})).unwrap();
        let val2 = s.execute("equation.get", &json!({})).unwrap();
        assert!(val2["linear"].as_str().unwrap().contains("+y"));
    }

    #[test]
    fn test_phasor_insert_structure_and_latex() {
        let mut s = Session::with_new();
        s.execute("insert.textBox", &json!({"rect": [100.0, 100.0, 300.0, 50.0], "text": ""})).unwrap();

        // 1. Insert via gallery template id
        s.execute("insert.equation", &json!({"edit": true})).unwrap();
        s.execute("equation.insertStructure", &json!({"id": "acc.phasor"})).unwrap();
        let val = s.execute("equation.get", &json!({})).unwrap();
        assert!(val["linear"].as_str().unwrap().starts_with("∠"));

        // 2. Insert via LaTeX \phase{30}
        s.execute("equation.exit", &json!({"before": false})).unwrap();
        s.execute("insert.equation", &json!({"edit": true})).unwrap();
        s.execute("equation.insertStructure", &json!({"latex": r"\phase{30}"})).unwrap();
        let val2 = s.execute("equation.get", &json!({})).unwrap();
        assert_eq!(val2["linear"], "∠(30)");
    }

    #[test]
    fn test_equation_type_length_limit() {
        let mut s = Session::with_new();
        s.execute("insert.textBox", &json!({"rect": [100.0, 100.0, 300.0, 50.0], "text": ""})).unwrap();
        s.execute("insert.equation", &json!({"edit": true})).unwrap();

        let ok_str = "x".repeat(MAX_EQUATION_TYPE_CHARS);
        s.execute("equation.type", &json!({"text": ok_str})).unwrap();

        let too_long = "x".repeat(MAX_EQUATION_TYPE_CHARS + 1);
        let err = s.execute("equation.type", &json!({"text": too_long})).unwrap_err();
        assert!(err.to_string().contains("text exceeds maximum"));
    }
}
