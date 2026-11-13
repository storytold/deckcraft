use super::paragraph::{clamp_pt, finite_key, tab_align, tabs_params};
use super::{Result, Session, TabStop, TextDir, Value, bad, bool_or, current_para, format_para, json, ok, resolve};

/// What the horizontal ruler shows while editing text: the paragraph's indents and tab stops,
/// and where its text area sits on the slide.
pub(super) fn ruler(s: &mut Session, _p: &Value) -> Result<Value> {
    let props = current_para(s);
    let st = s.doc()?;
    let t = st.selection.text.clone().ok_or_else(|| bad("format.ruler", "not editing text"))?;
    let (x, y, w, rotated) = match (t.notes, st.shape(t.shape)) {
        (false, Some(sh)) => {
            let ctx = crate::cmd::text::ctx_for(&st.doc, &st.selection);
            let x = match &ctx {
                Some(c) => resolve::xfrm(c, sh),
                None => sh.xfrm.unwrap_or_default(),
            };
            let body = match &ctx {
                Some(c) => resolve::body(c, sh),
                None => sh.text.as_ref().map(|b| b.body.clone()).unwrap_or_default(),
            };
            let (l, r, tp) = (body.inset_l.unwrap_or(7.2), body.inset_r.unwrap_or(7.2), body.inset_t.unwrap_or(3.6));
            (x.x + l, x.y + tp, (x.w - l - r).max(0.0), x.rot != 0.0 || body.vert.is_some_and(|v| v != TextDir::Horizontal))
        }
        _ => (0.0, 0.0, 0.0, true),
    };
    let tabs: Vec<Value> = props.tabs.clone().unwrap_or_default().iter().map(|t| json!({"pos": t.pos, "align": t.align})).collect();
    Ok(json!({
        "x": x, "y": y, "width": w, "rotated": rotated,
        "marginLeft": props.margin_left.unwrap_or(0.0),
        "indent": props.indent.unwrap_or(0.0),
        "tabs": tabs,
        "defaultTab": props.default_tab.unwrap_or(72.0),
    }))
}

const MAX_TABS: usize = 32;

pub(super) fn tabs(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "format.tabs";
    let fin = |v: Option<f64>| v.filter(|x| x.is_finite()).map(|x| x.clamp(0.0, 4000.0));
    let align_of = |v: &Value| match v.get("align").and_then(Value::as_str).unwrap_or("l") {
        "ctr" | "center" => "ctr",
        "r" | "right" => "r",
        "dec" | "decimal" => "dec",
        _ => "l",
    };
    let add = p.get("add").and_then(|a| fin(a.get("pos").and_then(Value::as_f64)).map(|pos| (pos, align_of(a).to_string())));
    let mv = p.get("move").and_then(|m| Some((fin(m.get("from").and_then(Value::as_f64))?, fin(m.get("to").and_then(Value::as_f64))?)));
    let remove = fin(p.get("remove").and_then(Value::as_f64));
    // `set` replaces every stop: entries without a usable position are dropped, an unknown
    // alignment or a non-object entry is an error; stops within 0.05 pt collapse (the last wins).
    let set = match p.get("set") {
        None => None,
        Some(Value::Array(items)) => {
            if items.len() > 64 {
                return Err(bad(CMD, "`set` has more than 64 tab stops"));
            }
            let mut out: Vec<TabStop> = Vec::new();
            for (i, item) in items.iter().enumerate() {
                let Some(obj) = item.as_object() else {
                    return Err(bad(CMD, format!("tab stop {i} must be an object")));
                };
                let Some(pos) = obj.get("pos").and_then(Value::as_f64).filter(|n| n.is_finite()).map(clamp_pt) else {
                    continue;
                };
                let align = match obj.get("align").and_then(Value::as_str) {
                    None => "l",
                    Some(a) => tab_align(a).ok_or_else(|| bad(CMD, format!("tab stop {i} align must be l, ctr, r or dec")))?,
                };
                if let Some(prev) = out.iter_mut().find(|t| (t.pos - pos).abs() < 0.05) {
                    *prev = TabStop { pos, align: align.to_string() };
                } else {
                    out.push(TabStop { pos, align: align.to_string() });
                }
            }
            Some(out)
        }
        Some(_) => return Err(bad(CMD, "`set` must be an array")),
    };
    let clear = bool_or(p, "clear", false);
    let default_tab = finite_key(p, CMD, "defaultTab")?.map(|v| v.clamp(1.0, 1584.0));
    let edits_stops = set.is_some() || add.is_some() || mv.is_some() || remove.is_some() || clear;
    if !edits_stops && default_tab.is_none() {
        // No fields: open the Tabs dialog on the caret's paragraph.
        s.ui_requests.push(crate::UiRequest::Dialog { id: "tabs".into(), params: tabs_params(s) });
        return ok();
    }
    // The stops as the caret's paragraph shows them (inherited ones included), so editing a
    // placeholder's tab that came from its layout works like editing one set on the paragraph.
    let base = current_para(s).tabs.unwrap_or_default();
    let near = |a: f64, b: f64| (a - b).abs() < 0.5;
    let mut stops = if clear { vec![] } else { set.unwrap_or(base) };
    if let Some(at) = remove {
        stops.retain(|t| !near(t.pos, at));
    }
    if let Some((from, to)) = mv
        && let Some(t) = stops.iter_mut().find(|t| near(t.pos, from))
    {
        t.pos = to;
    }
    if let Some((pos, align)) = add {
        stops.retain(|t| !near(t.pos, pos));
        stops.push(TabStop { pos, align });
    }
    stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    stops.dedup_by(|a, b| near(a.pos, b.pos));
    stops.truncate(MAX_TABS);
    format_para(s, &move |para| {
        if edits_stops {
            para.props.tabs = Some(stops.clone());
        }
        if let Some(d) = default_tab {
            para.props.default_tab = Some(d);
        }
    })
}
