use super::{
    Line, Result, Session, Value, bad, bool_or, color_param, current_run, ed, edit_shapes, format_body, format_run, json, ok, on_param, str_param,
};
use deckcraft_model::{ColorRef, style::Fill};

fn update_outline(
    run: &mut deckcraft_model::text::RunProps,
    inherited: Option<&deckcraft_model::Line>,
    c: Option<&ColorRef>,
    width: Option<f64>,
    dash: Option<&Option<deckcraft_model::style::Dash>>,
    remove: bool,
) {
    if remove {
        run.outline = inherited.is_some().then(deckcraft_model::Line::none);
        return;
    }
    let mut line = run
        .outline
        .take()
        .or_else(|| inherited.cloned())
        .unwrap_or_else(|| deckcraft_model::Line::solid(ColorRef::rgb(deckcraft_color::Rgba::BLACK), 0.75));
    line.width.get_or_insert(0.75);
    if let Some(color) = c {
        line.fill = Some(Fill::solid(color.clone()));
    } else if matches!(line.fill, None | Some(Fill::None)) {
        line.fill = Some(Fill::solid(ColorRef::rgb(deckcraft_color::Rgba::BLACK)));
    }
    if let Some(width) = width {
        line.width = Some(width);
    }
    if let Some(dash) = dash {
        line.dash.clone_from(dash);
    }
    run.outline = Some(line);
}

type OutlineTarget = (deckcraft_model::ShapeId, Option<(usize, usize)>, usize, Option<usize>);

struct OutlineEdit<'a> {
    color: Option<&'a ColorRef>,
    width: Option<f64>,
    dash: Option<&'a Option<deckcraft_model::style::Dash>>,
    remove: bool,
}

fn remember_body_outlines(
    out: &mut std::collections::HashMap<OutlineTarget, Option<Line>>,
    ctx: Option<&deckcraft_model::resolve::Ctx<'_>>,
    shape: &deckcraft_model::Shape,
    cell: Option<(usize, usize)>,
    body: &deckcraft_model::text::TextBody,
) {
    for (pi, para) in body.paragraphs.iter().enumerate() {
        for (ri, run) in para.runs.iter().enumerate() {
            let props = ctx.map_or(run.props.clone(), |ctx| deckcraft_model::resolve::run(ctx, shape, para, &run.props));
            out.insert((shape.id, cell, pi, Some(ri)), props.outline);
        }
        let props = ctx.map_or(para.end_props.clone(), |ctx| deckcraft_model::resolve::run(ctx, shape, para, &para.end_props));
        out.insert((shape.id, cell, pi, None), props.outline);
    }
}

fn apply_body_outline(
    body: &mut deckcraft_model::text::TextBody,
    out: &std::collections::HashMap<OutlineTarget, Option<Line>>,
    shape: deckcraft_model::ShapeId,
    cell: Option<(usize, usize)>,
    edit: &OutlineEdit<'_>,
) {
    for (pi, para) in body.paragraphs.iter_mut().enumerate() {
        for (ri, run) in para.runs.iter_mut().enumerate() {
            let inherited = out.get(&(shape, cell, pi, Some(ri))).and_then(Option::as_ref);
            update_outline(&mut run.props, inherited, edit.color, edit.width, edit.dash, edit.remove);
        }
        let inherited = out.get(&(shape, cell, pi, None)).and_then(Option::as_ref);
        update_outline(&mut para.end_props, inherited, edit.color, edit.width, edit.dash, edit.remove);
    }
}

fn text_outline_shapes(
    s: &mut Session,
    c: Option<ColorRef>,
    width: Option<f64>,
    dash: Option<Option<deckcraft_model::style::Dash>>,
    remove: bool,
) -> Result<Value> {
    use deckcraft_model::{ShapeKind, TextBody};
    use std::collections::HashMap;

    let st = s.doc()?;
    let ids = st.selection.shapes.clone();
    let cells = st.selection.cells;
    let ctx = crate::cmd::text::ctx_for(&st.doc, &st.selection);
    let mut inherited = HashMap::new();
    for sh in st.selected_shapes() {
        match &sh.kind {
            ShapeKind::Table(table) => {
                for (ri, row) in table.rows.iter().enumerate() {
                    for (ci, cell) in row.cells.iter().enumerate() {
                        remember_body_outlines(&mut inherited, ctx.as_ref(), sh, Some((ri, ci)), &cell.text);
                    }
                }
            }
            _ => {
                if let Some(body) = &sh.text {
                    remember_body_outlines(&mut inherited, ctx.as_ref(), sh, None, body);
                }
            }
        }
    }
    let edit = OutlineEdit { color: c.as_ref(), width, dash: dash.as_ref(), remove };
    edit_shapes(s, &json!({"ids": ids.clone()}), "format.textOutline", |sh| {
        match &mut sh.kind {
            ShapeKind::Table(table) => {
                let range = crate::cmd::table::selected_range(cells, sh.id, table);
                for (row_index, row) in table.rows.iter_mut().enumerate() {
                    for (column_index, cell) in row.cells.iter_mut().enumerate() {
                        if crate::cmd::table::in_range(range, row_index, column_index) {
                            apply_body_outline(&mut cell.text, &inherited, sh.id, Some((row_index, column_index)), &edit);
                        }
                    }
                }
            }
            _ => apply_body_outline(sh.text.get_or_insert_with(TextBody::default), &inherited, sh.id, None, &edit),
        }
        Ok(())
    })?;
    for id in ids {
        crate::cmd::text::fit_text_box(s, id)?;
    }
    ok()
}

pub(super) fn text_shadow(s: &mut Session, p: &Value) -> Result<Value> {
    let v = on_param(p, Some(current_run(s).shadow.is_some()));
    let sh = deckcraft_model::style::Shadow {
        color: deckcraft_model::ColorRef::rgb(deckcraft_color::Rgba::BLACK).with(deckcraft_color::ColorTransform::Alpha(43000)),
        blur: 3.0,
        dist: 2.0,
        dir: 45.0,
        inner: false,
        sx: 1.0,
        sy: 1.0,
        kx: 0.0,
        ky: 0.0,
        align: String::new(),
        rotate_with_shape: false,
    };
    format_run(s, &move |r| r.shadow = if v { Some(sh.clone()) } else { None })
}

/// Text Outline. With `none`, or with neither `color`, `width` nor `dash`, the outline goes; otherwise
/// the given parts replace those of the outline the run shows (a new one is the text colour's black, 0.75 pt, solid).
pub(super) fn text_outline(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "format.textOutline";
    let c = color_param(p, "color");
    let w = match p.get("width") {
        None => None,
        Some(v) => Some(
            v.as_f64()
                .filter(|w| w.is_finite() && (0.0..=1584.0).contains(w))
                .ok_or_else(|| bad(CMD, "`width` is a number of points from 0 to 1584"))?,
        ),
    };
    // A part that is given but not understood is an error, not a request to remove the outline.
    if p.get("color").is_some_and(|v| !v.is_null()) && c.is_none() {
        return Err(bad(CMD, "`color` is not a colour"));
    }
    if p.get("dash").is_some_and(|v| !v.is_null() && !v.is_string()) {
        return Err(bad(CMD, "`dash` is a dash name"));
    }
    let dash = match str_param(p, "dash") {
        None => None,
        Some("solid") => Some(None),
        Some(d) if deckcraft_model::style::Dash::PRESETS.contains(&d) => Some(Some(deckcraft_model::style::Dash::Preset { name: d.to_string() })),
        Some(d) => return Err(bad(CMD, format!("unknown dash `{d}`"))),
    };
    if bool_or(p, "none", false) || (c.is_none() && w.is_none() && dash.is_none()) {
        // An outline the text only inherits is switched off with an explicit no-line of its own.
        if s.doc()?.selection.text.is_none() {
            return text_outline_shapes(s, c, w, dash, true);
        }
        let inherited = current_run(s).outline.is_some();
        return format_run(s, &move |r| r.outline = inherited.then(Line::none));
    }
    if s.doc()?.selection.text.is_none() {
        return text_outline_shapes(s, c, w, dash, false);
    }
    let inherited = current_run(s).outline;
    format_run(s, &move |r| {
        update_outline(r, inherited.as_ref(), c.as_ref(), w, dash.as_ref(), false);
    })
}

pub(super) fn text_warp(s: &mut Session, p: &Value) -> Result<Value> {
    let preset = str_param(p, "preset").ok_or_else(|| bad("format.textWarp", "missing `preset`"))?;
    let clear = preset == "none" || preset == "textNoShape";
    if !clear && !deckcraft_geom::warp::is_supported(preset) {
        return Err(bad("format.textWarp", format!("unknown text transform `{preset}`")));
    }
    let adj = match p.get("adj") {
        None => Vec::new(),
        Some(Value::Array(arr)) => {
            if arr.len() > 8 {
                return Err(bad("format.textWarp", "at most 8 adjust values"));
            }
            let mut v = Vec::with_capacity(arr.len());
            for a in arr {
                let Some(n) = a.as_f64().filter(|n| n.is_finite()) else {
                    return Err(bad("format.textWarp", "adjust values must be finite numbers"));
                };
                v.push(n.clamp(-2.0e9, 2.0e9));
            }
            v
        }
        Some(_) => return Err(bad("format.textWarp", "`adj` must be an array of numbers")),
    };
    let stored = if clear { None } else { Some(preset.to_string()) };
    format_body(s, &|b| {
        b.warp.clone_from(&stored);
        b.warp_adj.clone_from(&adj);
    })
}

pub(super) fn case(s: &mut Session, p: &Value) -> Result<Value> {
    let mode = str_param(p, "mode").unwrap_or("cycle").to_string();
    let st = s.doc()?;
    if let Some(t) = st.selection.text.clone() {
        let (mut a, mut b) = t.ordered();
        let body = crate::cmd::text::body_of(st, &t).cloned().unwrap_or_default();
        let collapsed = a == b;
        if collapsed {
            // Shift+F3 with a bare caret changes the word it is in and puts the caret at its start.
            let (wa, wb) = ed::word_at(&body, a);
            let is_word = ed::text_range(&body, wa, wb).chars().any(char::is_alphanumeric);
            if !(is_word && wa.1 <= a.1 && a.1 < wb.1) {
                return ok();
            }
            (a, b) = (wa, wb);
        }
        let m = if mode == "cycle" { ed::next_case(&ed::text_range(&body, a, b)).to_string() } else { mode.clone() };
        crate::cmd::text::edit_text(s, move |body, t| {
            let last = body.paragraphs.len().saturating_sub(1);
            for pi in a.0..=b.0.min(last) {
                let Some(para) = body.paragraphs.get_mut(pi) else { continue };
                let n = para.char_len();
                let from = if pi == a.0 { a.1 } else { 0 };
                let to = if pi == b.0 { b.1 } else { n };
                let mut pos = 0;
                let mut pieces = Vec::new();
                for (ri, r) in para.runs.iter().enumerate() {
                    // Breaks and fields are read for context (a break ends a word) but not rewritten.
                    let len = r.char_len();
                    let rs = from.max(pos).saturating_sub(pos);
                    let re = to.min(pos + len).saturating_sub(pos);
                    if re > rs {
                        pieces.push((ri, rs, re));
                    }
                    pos += len;
                }
                let mids: Vec<String> = pieces
                    .iter()
                    .map(|&(ri, rs, re)| para.runs.get(ri).map(|r| r.content().chars().skip(rs).take(re - rs).collect()).unwrap_or_default())
                    .collect();
                let changed = ed::change_case_parts(&mids.iter().map(String::as_str).collect::<Vec<_>>(), &m);
                for (&(ri, rs, re), new) in pieces.iter().zip(changed) {
                    let Some(r) = para.runs.get_mut(ri).filter(|r| r.kind == deckcraft_model::text::RunKind::Text) else { continue };
                    let pre: String = r.text.chars().take(rs).collect();
                    let post: String = r.text.chars().skip(re).collect();
                    r.text = format!("{pre}{new}{post}");
                }
            }
            if collapsed {
                t.anchor = a;
                t.caret = a;
            }
        })?;
        return ok();
    }
    let mode = if mode == "cycle" {
        let all: String =
            st.selection.shapes.iter().filter_map(|id| st.shape(*id)).filter_map(|sh| sh.text.as_ref()).map(|t| t.text()).collect::<Vec<_>>().join(
                "
",
            );
        ed::next_case(&all).to_string()
    } else {
        mode
    };
    edit_shapes(s, &json!({}), "format.case", |sh| {
        if let Some(t) = sh.text.as_mut() {
            for para in &mut t.paragraphs {
                // Breaks and fields are read for context (a break ends a word) but not rewritten.
                let texts: Vec<&str> = para.runs.iter().map(|r| r.content()).collect();
                let changed = ed::change_case_parts(&texts, &mode);
                for (r, new) in para.runs.iter_mut().zip(changed) {
                    if r.kind == deckcraft_model::text::RunKind::Text {
                        r.text = new;
                    }
                }
            }
        }
        Ok(())
    })
}
