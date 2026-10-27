use super::*;

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

pub(super) fn text_outline(s: &mut Session, p: &Value) -> Result<Value> {
    let c = color_param(p, "color");
    let w = f64_or(p, "width", 0.75);
    format_run(s, &move |r| r.outline = c.clone().map(|c| Line::solid(c, w)))
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
