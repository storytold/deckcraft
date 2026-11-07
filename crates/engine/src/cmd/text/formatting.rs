use super::layout::fit_text_box;
use super::{
    DocState, Hyperlink, Pos, Result, RunProps, SELECT_TEXT_OR_SHAPE, Session, ShapeId, ShapeKind, TextBody, TextSel, Value, bad, body_of, ed,
    edit_shapes, edit_text, json, ok, str_param,
};

/// Apply run formatting to the text selection (or, when not editing, to all text of the
/// selected shapes). A caret without a range formats the word it's in (PowerPoint behaviour),
/// or sets the formatting for what is typed next at a word boundary.
pub(crate) fn format_run(s: &mut Session, f: &(dyn Fn(&mut RunProps) + Send + Sync)) -> Result<Value> {
    format_run_levels(s, f, &|r, _| f(r))
}

/// Like [`format_run`], with a separate change for the list-style levels of a layout or master
/// placeholder: `lf` gets the level's own run props and the level's effective ones (what a run at
/// that level resolves to), so a relative change (grow/shrink) steps each level from its own value.
pub(crate) fn format_run_levels(
    s: &mut Session,
    f: &(dyn Fn(&mut RunProps) + Send + Sync),
    lf: &(dyn Fn(&mut RunProps, &RunProps) + Send + Sync),
) -> Result<Value> {
    let st = s.doc()?;
    if let Some(t) = st.selection.text.clone() {
        let (a, b) = format_range(st, &t);
        // Only the levels of the paragraphs the selected range touches.
        let levels = if a == b { Vec::new() } else { master_levels(st, &t, t.ordered()) };
        edit_text(s, |body, _| {
            if levels.is_empty() {
                ed::format(body, a, b, f);
            } else {
                format_by_level(body, a, b, lf, &levels);
            }
            for (l, eff) in &levels {
                let mut lvl = body.list_style.level(*l).cloned().unwrap_or_default();
                lf(&mut lvl.run, eff);
                body.list_style.set(*l, lvl);
            }
        })?;
        return ok();
    }
    let ids = st.selection.shapes.clone();
    if ids.is_empty() {
        return Err(bad("format", SELECT_TEXT_OR_SHAPE));
    }
    let cells = st.selection.cells;
    // Effective run props of every level of the selected placeholders, resolved before editing.
    let effective: Vec<(ShapeId, Vec<RunProps>)> = if editing_master(st) {
        let ctx = crate::cmd::text::ctx_for(&st.doc, &st.selection);
        ids.iter()
            .filter_map(|id| {
                let sh = st.shape(*id).filter(|sh| sh.ph.is_some())?;
                let ctx = ctx.as_ref()?;
                Some((*id, (0..LIST_LEVELS).map(|l| deckcraft_model::resolve::level_style(ctx, sh, l).run).collect()))
            })
            .collect()
    } else {
        Vec::new()
    };
    edit_shapes(s, &json!({"ids": ids}), "format", |sh| {
        let id = sh.id;
        match &mut sh.kind {
            ShapeKind::Table(t) => {
                // Only the selected cells, when a range of them is selected.
                let range = crate::cmd::table::selected_range(cells, id, t);
                for (r, row) in t.rows.iter_mut().enumerate() {
                    for (c, cell) in row.cells.iter_mut().enumerate() {
                        if crate::cmd::table::in_range(range, r, c) {
                            ed::format_all(&mut cell.text, f);
                        }
                    }
                }
            }
            _ => {
                let body = sh.text.get_or_insert_with(TextBody::default);
                let eff = effective.iter().find(|(i, _)| *i == id).map(|(_, e)| e);
                match eff {
                    Some(eff) => {
                        let levels: Vec<(u8, RunProps)> = (0..LIST_LEVELS).zip(eff.iter().cloned()).collect();
                        format_by_level(body, (0, 0), (usize::MAX, usize::MAX), lf, &levels);
                    }
                    None => ed::format_all(body, f),
                }
                // The whole shape is selected: every level of its list style.
                if let Some(eff) = eff {
                    for (l, eff) in (0..LIST_LEVELS).zip(eff) {
                        let mut lvl = body.list_style.level(l).cloned().unwrap_or_default();
                        lf(&mut lvl.run, eff);
                        body.list_style.set(l, lvl);
                    }
                }
            }
        }
        Ok(())
    })?;
    for id in ids {
        fit_text_box(s, id)?;
    }
    ok()
}

/// Format the runs of [a, b) paragraph by paragraph, each with its own level's effective props, so
/// a relative change (grow/shrink) steps every paragraph from its own size instead of one shared
/// value. `a`/`b` are clamped to the body.
fn format_by_level(body: &mut TextBody, a: Pos, b: Pos, lf: &(dyn Fn(&mut RunProps, &RunProps) + Send + Sync), levels: &[(u8, RunProps)]) {
    let Some(last) = body.paragraphs.len().checked_sub(1) else { return };
    let (first, end) = (a.0.min(last), b.0.min(last));
    for pi in first..=end {
        let p = &body.paragraphs[pi];
        let from = if pi == first { a.1.min(p.char_len()) } else { 0 };
        let to = if pi == end { b.1.min(p.char_len()) } else { p.char_len() };
        if from == to && p.char_len() > 0 {
            continue;
        }
        let lvl = p.level.min(LIST_LEVELS - 1);
        let Some((_, eff)) = levels.iter().find(|(l, _)| *l == lvl) else { continue };
        ed::format(body, (pi, from), (pi, to), &|r| lf(r, eff));
    }
}

/// List-style levels (`lvl1pPr`–`lvl9pPr`) a placeholder passes on to the slides that use it.
const LIST_LEVELS: u8 = 9;

/// Formatting a placeholder on a layout or master (Slide Master view) also sets its list style,
/// which is what the placeholders on slides inherit; its own runs may be few or none.
fn editing_master(st: &DocState) -> bool {
    matches!(st.selection.target, crate::Target::Layout { .. } | crate::Target::Master { .. })
}

/// The distinct list-style levels of the paragraphs `range` touches in a layout or master
/// placeholder, with each level's effective run props. Empty anywhere else.
fn master_levels(st: &DocState, t: &TextSel, range: (Pos, Pos)) -> Vec<(u8, RunProps)> {
    if !editing_master(st) || t.notes || t.cell.is_some() {
        return Vec::new();
    }
    let (Some(sh), Some(body), Some(ctx)) =
        (st.shape(t.shape).filter(|sh| sh.ph.is_some()), body_of(st, t), crate::cmd::text::ctx_for(&st.doc, &st.selection))
    else {
        return Vec::new();
    };
    let mut levels: Vec<u8> =
        body.paragraphs.iter().enumerate().filter(|(i, _)| (range.0.0..=range.1.0).contains(i)).map(|(_, p)| p.level.min(LIST_LEVELS - 1)).collect();
    if body.paragraphs.is_empty() {
        levels.push(0);
    }
    levels.sort_unstable();
    levels.dedup();
    levels.into_iter().map(|l| (l, deckcraft_model::resolve::level_style(&ctx, sh, l).run)).collect()
}

/// What `format_run` formats while editing: the selection, or for a bare caret the word it's in
/// (without its trailing space). Empty when the caret is at a word boundary. Insert Hyperlink
/// edits this same span, so the dialog reads it too.
pub fn format_range(st: &DocState, t: &TextSel) -> (Pos, Pos) {
    let body = body_of(st, t).cloned().unwrap_or_default();
    let (mut a, mut b) = t.ordered();
    if a == b {
        let (wa, wb) = ed::word_at(&body, a);
        let inside = wa.1 < a.1 && a.1 < wb.1;
        if inside {
            a = wa;
            b = wb;
            // Don't include the trailing space.
            let line: Vec<char> = body.paragraphs.get(b.0).map(|p| p.text().chars().collect()).unwrap_or_default();
            while b.1 > a.1 && line.get(b.1 - 1) == Some(&' ') {
                b.1 -= 1;
            }
        }
    }
    (a, b)
}

/// Link the text being edited: the selection or the word at the caret, as `format_run` would.
/// With nothing to link (caret at a word boundary, empty paragraph) `display` is inserted at the
/// caret as linked text in the surrounding formatting, as PowerPoint's Insert Hyperlink does.
pub(crate) fn link_text(s: &mut Session, link: Hyperlink, display: &str) -> Result<Value> {
    let st = s.doc()?;
    let Some(t) = st.selection.text.clone() else { return Err(bad("insert.hyperlink", "not editing text")) };
    let (a, b) = format_range(st, &t);
    if a != b {
        return format_run(s, &move |rp| rp.link = Some(link.clone()));
    }
    if display.is_empty() {
        return Err(bad("insert.hyperlink", "nothing to link: select text or give `text`"));
    }
    let pos = edit_text(s, |body, t| {
        let mut props = ed::props_at(body, a);
        props.link = Some(link);
        let c = ed::insert(body, a, display, Some(props));
        t.anchor = c;
        t.caret = c;
        c
    })?;
    Ok(json!({"caret": [pos.0, pos.1]}))
}

/// Apply paragraph changes to the paragraphs touched by the selection (or all paragraphs of the
/// selected shapes).
pub(crate) fn format_para(s: &mut Session, f: &(dyn Fn(&mut deckcraft_model::text::Paragraph) + Send + Sync)) -> Result<Value> {
    let st = s.doc()?;
    if let Some(t) = st.selection.text.clone() {
        let (a, b) = t.ordered();
        // Only the list-style levels of the paragraphs touched, on a layout or master placeholder.
        let levels: Vec<u8> = master_levels(st, &t, (a, b)).into_iter().map(|(l, _)| l).collect();
        edit_text(s, |body, _| {
            if body.paragraphs.is_empty() {
                body.paragraphs.push(Default::default());
            }
            let last = body.paragraphs.len().saturating_sub(1);
            for pi in a.0..=b.0.min(last) {
                if let Some(p) = body.paragraphs.get_mut(pi) {
                    f(p);
                }
            }
            for l in levels {
                format_list_level(body, l, f);
            }
        })?;
        return ok();
    }
    let ids = st.selection.shapes.clone();
    if ids.is_empty() {
        return Err(bad("format", SELECT_TEXT_OR_SHAPE));
    }
    let cells = st.selection.cells;
    let on_master = editing_master(st);
    edit_shapes(s, &json!({"ids": ids}), "format", |sh| {
        let id = sh.id;
        let inherited = on_master && sh.ph.is_some();
        match &mut sh.kind {
            ShapeKind::Table(t) => {
                let range = crate::cmd::table::selected_range(cells, id, t);
                for (r, row) in t.rows.iter_mut().enumerate() {
                    for (c, cell) in row.cells.iter_mut().enumerate() {
                        if crate::cmd::table::in_range(range, r, c) {
                            cell.text.paragraphs.iter_mut().for_each(f);
                        }
                    }
                }
            }
            _ => {
                let body = sh.text.get_or_insert_with(TextBody::default);
                if body.paragraphs.is_empty() {
                    body.paragraphs.push(Default::default());
                }
                body.paragraphs.iter_mut().for_each(f);
                // The whole shape is selected: every level of its list style.
                if inherited {
                    for l in 0..LIST_LEVELS {
                        format_list_level(body, l, f);
                    }
                }
            }
        }
        Ok(())
    })?;
    for id in ids {
        fit_text_box(s, id)?;
    }
    ok()
}

/// Apply a paragraph change to one level of a body's list style.
fn format_list_level(body: &mut TextBody, l: u8, f: &(dyn Fn(&mut deckcraft_model::text::Paragraph) + Send + Sync)) {
    let mut lvl = body.list_style.level(l).cloned().unwrap_or_default();
    let mut p = deckcraft_model::text::Paragraph { level: l, props: lvl.para, ..Default::default() };
    f(&mut p);
    // A level change (indent/outdent) means nothing for a list-style level.
    if p.level == l {
        lvl.para = p.props;
        body.list_style.set(l, lvl);
    }
}

/// Apply body (text box) changes to the edited or selected shapes.
pub(crate) fn format_body(s: &mut Session, f: &(dyn Fn(&mut deckcraft_model::text::BodyProps) + Send + Sync)) -> Result<Value> {
    let st = s.doc()?;
    let ids: Vec<ShapeId> = match &st.selection.text {
        Some(t) if !t.notes => vec![t.shape],
        _ => st.selection.shapes.clone(),
    };
    if ids.is_empty() {
        return Err(bad("format", "select a shape"));
    }
    edit_shapes(s, &json!({"ids": ids}), "format", |sh| {
        f(&mut sh.text.get_or_insert_with(TextBody::default).body);
        Ok(())
    })?;
    for id in ids {
        fit_text_box(s, id)?;
    }
    ok()
}

pub(super) fn fonts_used(s: &mut Session, _: &Value) -> Result<Value> {
    let mut v = crate::fonts::used_fonts(&s.doc()?.doc);
    v.sort_by_key(|f| f.to_lowercase());
    Ok(json!(v))
}

pub(super) fn replace_font(s: &mut Session, p: &Value) -> Result<Value> {
    let from = str_param(p, "from").map(str::trim).filter(|x| !x.is_empty()).ok_or_else(|| bad("font.replace", "missing `from`"))?.to_string();
    let to = str_param(p, "to").map(str::trim).filter(|x| !x.is_empty()).ok_or_else(|| bad("font.replace", "missing `to`"))?.to_string();
    if to.len() > 256 || to.chars().any(char::is_control) {
        return Err(bad("font.replace", "`to` is not a font name"));
    }
    // Count on a copy first so a replace that finds nothing leaves no undo step.
    let mut probe = (*s.doc()?.doc).clone();
    if crate::fonts::replace_font(&mut probe, &from, &to) == 0 {
        return Ok(json!({"replaced": 0}));
    }
    let n = s.edit(|doc, _| Ok(crate::fonts::replace_font(doc, &from, &to)))?;
    let st = s.doc_mut()?;
    st.missing_fonts = crate::fonts::missing_fonts(&st.doc);
    Ok(json!({"replaced": n}))
}

#[cfg(test)]
mod replace_font_tests {
    use serde_json::json;

    use crate::Session;

    fn fonts(s: &mut Session) -> Vec<String> {
        serde_json::from_value(s.execute("font.used", &json!({})).unwrap()).unwrap()
    }

    #[test]
    fn replaces_runs_tables_notes_and_theme_and_undoes() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "T", "body": "B"})).unwrap();
        let theme = s.doc().unwrap().doc.masters[0].theme.fonts.minor.latin.clone();
        s.execute("insert.textBox", &json!({"rect": [10, 10, 200, 50], "text": "Hello"})).unwrap();
        s.execute("text.exit", &json!({})).unwrap();
        s.execute("format.font", &json!({"family": "Comic Neue"})).unwrap();
        assert!(fonts(&mut s).iter().any(|f| f == "Comic Neue"), "{:?}", fonts(&mut s));
        let r = s.execute("font.replace", &json!({"from": "comic neue", "to": "Lato"})).unwrap();
        assert!(r["replaced"].as_u64().unwrap() >= 1, "{r}");
        let used = fonts(&mut s);
        assert!(!used.iter().any(|f| f == "Comic Neue") && used.iter().any(|f| f == "Lato"), "{used:?}");
        // The theme's body font is replaced too, so placeholders follow.
        s.execute("font.replace", &json!({"from": theme, "to": "Lato"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.masters[0].theme.fonts.minor.latin, "Lato");
        assert!(s.doc().unwrap().doc.validate().is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(fonts(&mut s).iter().any(|f| f == "Comic Neue"));
    }

    #[test]
    fn nothing_to_replace_is_no_undo_step_and_bad_names_fail() {
        let mut s = Session::with_new();
        let undo = s.doc().unwrap().history.undo.len();
        assert_eq!(s.execute("font.replace", &json!({"from": "Nope Sans", "to": "Inter"})).unwrap()["replaced"], 0);
        assert_eq!(s.execute("font.replace", &json!({"from": "+mn-lt", "to": "Inter"})).unwrap()["replaced"], 0);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
        assert!(s.execute("font.replace", &json!({"from": "", "to": "Inter"})).is_err());
        assert!(s.execute("font.replace", &json!({"from": "Inter", "to": "a\nb"})).is_err());
        assert!(s.execute("font.replace", &json!({"from": "Inter"})).is_err());
    }
}
