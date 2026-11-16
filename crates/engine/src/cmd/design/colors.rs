use super::{
    Arc, Result, Session, Value, bad, builtin_color_schemes, builtin_font_schemes, builtin_format_schemes, json, master_index, ok, str_param,
};

pub(super) fn colors(s: &mut Session, p: &Value) -> Result<Value> {
    let name_v = p.get("name");
    let colors_v = p.get("colors");
    let tints_v = p.get("tints");
    let extra_v = p.get("extra");
    let computed = match p.get("computedTints") {
        None => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(bad("design.colors", "`computedTints` must be true or false")),
    };
    if name_v.is_none() && colors_v.is_none() && tints_v.is_none() && extra_v.is_none() && computed.is_none() {
        let sc = current_colors(s)?;
        s.ui_requests.push(crate::UiRequest::Dialog { id: "themeColors".into(), params: scheme_json(&sc) });
        return ok();
    }
    let given = match name_v {
        None => None,
        Some(Value::String(n)) => {
            let n = n.trim();
            if n.is_empty() {
                return Err(bad("design.colors", "name is empty"));
            }
            Some(n.to_string())
        }
        Some(_) => return Err(bad("design.colors", "`name` must be a string")),
    };
    let slots = parse_color_slots(colors_v)?;
    let tints = parse_tints(tints_v)?;
    let extra = parse_extra(extra_v)?;
    let current = current_colors(s)?;
    let mut next = next_scheme(&current, given.as_deref(), slots.as_deref())?;
    if let Some(tints) = tints {
        for (slot, v) in tints {
            match v {
                Some(v) => {
                    next.tints.insert(slot, v);
                }
                None => {
                    next.tints.remove(&slot);
                }
            }
        }
    }
    if let Some(extra) = extra {
        next.extra = extra;
    }
    if let Some(c) = computed {
        next.computed_tints = c;
    }
    if next == current {
        return ok();
    }
    s.edit(|doc, sel| {
        let mi = master_index(doc, sel);
        let m = Arc::make_mut(doc.masters.get_mut(mi).ok_or_else(|| bad("design.colors", "no master"))?);
        m.theme.colors = next;
        Ok(())
    })?;
    ok()
}

pub(super) fn variant(s: &mut Session, p: &Value) -> Result<Value> {
    let index = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("design.variant", "missing `index` (0..3)"))? as usize;
    s.edit(|doc, sel| {
        let mi = master_index(doc, sel);
        let m = Arc::make_mut(doc.masters.get_mut(mi).ok_or_else(|| bad("design.variant", "no master"))?);
        let v = deckcraft_model::theme::theme_variants(&m.theme);
        m.theme.colors =
            v.get(index).cloned().ok_or_else(|| bad("design.variant", format!("no variant {index}; 0..{}", v.len().saturating_sub(1))))?;
        Ok(())
    })?;
    Ok(json!({"variant": index}))
}

pub(super) fn variants(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let mi = master_index(&st.doc, &st.selection);
    let th = st.doc.masters.get(mi).map(|m| m.theme.clone()).unwrap_or_default();
    let v = deckcraft_model::theme::theme_variants(&th);
    let current = v.iter().position(|c| c.colors == th.colors.colors);
    Ok(json!({
        "theme": th.name,
        "current": current,
        "variants": v.iter().map(|c| json!({"name": c.name, "colors": c.colors.iter().map(|x| format!("#{}", x.hex())).collect::<Vec<_>>()})).collect::<Vec<_>>(),
    }))
}

fn current_colors(s: &Session) -> Result<deckcraft_color::ColorScheme> {
    let st = s.doc()?;
    let mi = master_index(&st.doc, &st.selection);
    st.doc.masters.get(mi).map(|m| m.theme.colors.clone()).ok_or_else(|| bad("design.colors", "no master"))
}

fn hex_list(v: &[deckcraft_color::Rgba]) -> Value {
    Value::Array(v.iter().map(|c| Value::String(format!("#{}", c.hex()))).collect())
}

/// A colour scheme as `design.colors` takes it: `{name, colors: {slot: #RRGGBB}, computedTints,
/// tints: {slot: [#RRGGBB…]}, extra: [{name, color, tints?}]}` (the Create New Theme Colors
/// dialog's parameters, and `design.themes`' `current`).
pub(super) fn scheme_json(sc: &deckcraft_color::ColorScheme) -> Value {
    let mut colors = serde_json::Map::new();
    for slot in deckcraft_color::SchemeSlot::THEME {
        colors.insert(slot.xml_name().to_string(), Value::String(format!("#{}", sc.get(slot).hex())));
    }
    let mut tints = serde_json::Map::new();
    for (slot, v) in &sc.tints {
        tints.insert(slot.xml_name().to_string(), hex_list(v));
    }
    let extra: Vec<Value> = sc
        .extra
        .iter()
        .map(|e| {
            let mut o = json!({"name": e.name, "color": format!("#{}", e.color.hex())});
            if let (Some(t), Some(o)) = (&e.tints, o.as_object_mut()) {
                o.insert("tints".into(), hex_list(t));
            }
            o
        })
        .collect();
    json!({"name": sc.name, "colors": colors, "computedTints": sc.computed_tints, "tints": tints, "extra": extra})
}

/// `[#RRGGBB…]` (0 to MAX_TINT_ROWS colours) for `what`; `None` for `null` (the list is removed).
fn parse_tint_rows(what: &str, v: &Value) -> Result<Option<Vec<deckcraft_color::Rgba>>> {
    if v.is_null() {
        return Ok(None);
    }
    let Some(arr) = v.as_array() else {
        return Err(bad("design.colors", format!("`{what}` tints must be a list of colours like #RRGGBB")));
    };
    if arr.len() > deckcraft_color::MAX_TINT_ROWS {
        return Err(bad("design.colors", format!("`{what}` tints hold at most {} colours, got {}", deckcraft_color::MAX_TINT_ROWS, arr.len())));
    }
    arr.iter()
        .map(|c| {
            c.as_str()
                .and_then(deckcraft_color::Rgba::from_hex)
                .ok_or_else(|| bad("design.colors", format!("`{what}` tints need colours like #RRGGBB")))
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

/// `None` when the key is absent. `{slot: [#RRGGBB…] | null}`: a list (even empty) is exactly
/// the gallery rows under that theme colour, `null` removes it (computed rows, or none when
/// `computedTints` is false).
#[allow(clippy::type_complexity)]
fn parse_tints(v: Option<&Value>) -> Result<Option<Vec<(deckcraft_color::SchemeSlot, Option<Vec<deckcraft_color::Rgba>>)>>> {
    let Some(v) = v else { return Ok(None) };
    let Some(map) = v.as_object() else {
        return Err(bad("design.colors", "`tints` must be an object {slot: [#RRGGBB…]}"));
    };
    let mut out = Vec::with_capacity(map.len());
    for (k, v) in map {
        let Some(slot) = deckcraft_color::SchemeSlot::from_xml(k) else {
            return Err(bad("design.colors", format!("unknown colour `{k}` in `tints`")));
        };
        if !deckcraft_color::SchemeSlot::THEME.contains(&slot) {
            return Err(bad("design.colors", format!("`{k}` is not a theme colour")));
        }
        out.push((slot, parse_tint_rows(k, v)?));
    }
    Ok(Some(out))
}

/// `None` when the key is absent. `[{name, color: #RRGGBB, tints?: [#RRGGBB…]}]` replaces the
/// scheme's extra colours (`[]` removes them). Names are unique (case-insensitive) and must not be
/// a slot name, so that each is reachable by name.
fn parse_extra(v: Option<&Value>) -> Result<Option<Vec<deckcraft_color::ExtraColor>>> {
    let Some(v) = v else { return Ok(None) };
    let Some(arr) = v.as_array() else {
        return Err(bad("design.colors", "`extra` must be a list of {name, color: #RRGGBB, tints?}"));
    };
    if arr.len() > deckcraft_color::MAX_EXTRA_COLORS {
        return Err(bad("design.colors", format!("`extra` holds at most {} colours", deckcraft_color::MAX_EXTRA_COLORS)));
    }
    let mut out: Vec<deckcraft_color::ExtraColor> = Vec::with_capacity(arr.len());
    for e in arr {
        let Some(o) = e.as_object() else {
            return Err(bad("design.colors", "each `extra` entry is {name, color: #RRGGBB, tints?}"));
        };
        let name = o.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("");
        if name.is_empty() {
            return Err(bad("design.colors", "an `extra` colour needs a `name`"));
        }
        if deckcraft_color::SchemeSlot::from_xml(name).is_some() || deckcraft_color::Rgba::from_hex(name).is_some() {
            return Err(bad("design.colors", format!("`{name}` cannot name an extra colour: it is a theme colour name or a hex colour")));
        }
        if out.iter().any(|x| x.name.eq_ignore_ascii_case(name)) {
            return Err(bad("design.colors", format!("extra colour `{name}` is listed twice")));
        }
        let Some(color) = o.get("color").and_then(Value::as_str).and_then(deckcraft_color::Rgba::from_hex) else {
            return Err(bad("design.colors", format!("extra colour `{name}` needs `color` like #RRGGBB")));
        };
        let tints = match o.get("tints") {
            None => None,
            Some(t) => parse_tint_rows(name, t)?,
        };
        out.push(deckcraft_color::ExtraColor { name: name.to_string(), color, tints });
    }
    Ok(Some(out))
}

/// `None` when the key is absent. Present values must be the twelve stored slots; aliases such
/// as `bg1` are rejected because `ColorScheme::set` ignores them.
fn parse_color_slots(colors_v: Option<&Value>) -> Result<Option<Vec<(deckcraft_color::SchemeSlot, deckcraft_color::Rgba)>>> {
    let Some(v) = colors_v else { return Ok(None) };
    let Some(map) = v.as_object() else {
        return Err(bad("design.colors", "`colors` must be an object"));
    };
    if map.is_empty() {
        return Err(bad("design.colors", "`colors` is empty"));
    }
    let mut slots = Vec::with_capacity(map.len());
    for (k, v) in map {
        let Some(slot) = deckcraft_color::SchemeSlot::from_xml(k) else {
            return Err(bad("design.colors", format!("unknown colour `{k}`")));
        };
        if !deckcraft_color::SchemeSlot::THEME.contains(&slot) {
            return Err(bad("design.colors", format!("`{k}` is not a theme colour")));
        }
        let Some(rgb) = v.as_str().and_then(deckcraft_color::Rgba::from_hex) else {
            return Err(bad("design.colors", format!("`{k}` needs a colour like #RRGGBB")));
        };
        slots.push((slot, rgb));
    }
    Ok(Some(slots))
}

/// A builtin `name` is applied first, then `colors` overlays those slots. The stored scheme name
/// is the given name, or `Custom` when only colours are sent. An unknown name without colours is an error.
fn next_scheme(
    current: &deckcraft_color::ColorScheme,
    given: Option<&str>,
    slots: Option<&[(deckcraft_color::SchemeSlot, deckcraft_color::Rgba)]>,
) -> Result<deckcraft_color::ColorScheme> {
    let mut next = current.clone();
    if let Some(name) = given {
        if let Some(builtin) = builtin_color_schemes().into_iter().find(|c| c.name.eq_ignore_ascii_case(name)) {
            next = builtin;
        } else if slots.is_none() {
            return Err(bad("design.colors", format!("no colour scheme `{name}`")));
        }
        next.name = name.to_string();
    }
    if let Some(changes) = slots {
        for (slot, rgb) in changes {
            next.set(*slot, *rgb);
        }
        if given.is_none() {
            next.name = "Custom".into();
        }
    }
    Ok(next)
}

pub(super) fn fonts(s: &mut Session, p: &Value) -> Result<Value> {
    let name_v = p.get("name");
    let major_v = p.get("major");
    let minor_v = p.get("minor");
    if name_v.is_none() && major_v.is_none() && minor_v.is_none() {
        let scheme = current_fonts(s)?;
        s.ui_requests.push(crate::UiRequest::Dialog { id: "themeFonts".into(), params: font_dialog_params(&scheme) });
        return ok();
    }
    let given = match name_v {
        None => None,
        Some(Value::String(n)) => {
            let n = n.trim();
            if n.is_empty() {
                return Err(bad("design.fonts", "name is empty"));
            }
            Some(n.to_string())
        }
        Some(_) => return Err(bad("design.fonts", "`name` must be a string")),
    };
    let major = face_param("major", major_v)?;
    let minor = face_param("minor", minor_v)?;
    let current = current_fonts(s)?;
    let next = next_font_scheme(&current, given.as_deref(), major.as_deref(), minor.as_deref())?;
    if next == current {
        return ok();
    }
    s.edit(|doc, sel| {
        let mi = master_index(doc, sel);
        Arc::make_mut(doc.masters.get_mut(mi).ok_or_else(|| bad("design.fonts", "no master"))?).theme.fonts = next;
        Ok(())
    })?;
    ok()
}

fn current_fonts(s: &Session) -> Result<deckcraft_model::theme::FontScheme> {
    let st = s.doc()?;
    let mi = master_index(&st.doc, &st.selection);
    st.doc.masters.get(mi).map(|m| m.theme.fonts.clone()).ok_or_else(|| bad("design.fonts", "no master"))
}

fn font_dialog_params(scheme: &deckcraft_model::theme::FontScheme) -> Value {
    json!({"name": scheme.name, "major": scheme.major.latin, "minor": scheme.minor.latin})
}

/// `None` when the key is absent. A present face must be a non-blank string.
fn face_param(key: &str, v: Option<&Value>) -> Result<Option<String>> {
    match v {
        None => Ok(None),
        Some(Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() { Err(bad("design.fonts", format!("`{key}` is empty"))) } else { Ok(Some(s.to_string())) }
        }
        Some(_) => Err(bad("design.fonts", format!("`{key}` must be a string"))),
    }
}

/// A builtin `name` is applied first, then `major` / `minor` replace only the latin faces (East
/// Asian, complex-script and per-script slots stay). The stored name is the given string when
/// faces are overlaid or the name is not a builtin; a name-only builtin keeps its own name.
/// Faces without a name are a custom scheme: an omitted `minor` copies `major`.
fn next_font_scheme(
    current: &deckcraft_model::theme::FontScheme,
    given: Option<&str>,
    major: Option<&str>,
    minor: Option<&str>,
) -> Result<deckcraft_model::theme::FontScheme> {
    let mut next = current.clone();
    if let Some(name) = given {
        let replaced = if let Some(builtin) = builtin_font_schemes().into_iter().find(|f| f.name.eq_ignore_ascii_case(name)) {
            next = builtin;
            true
        } else {
            false
        };
        if !replaced && major.is_none() && minor.is_none() {
            return Err(bad("design.fonts", format!("no font scheme `{name}`")));
        }
        if major.is_some() || minor.is_some() || !replaced {
            next.name = name.to_string();
        }
    }
    if given.is_none() {
        if let Some(face) = major {
            next.major.latin = face.to_string();
            if minor.is_none() {
                next.minor.latin = face.to_string();
            }
        }
        if let Some(face) = minor {
            next.minor.latin = face.to_string();
        }
        next.name = "Custom".into();
    } else {
        if let Some(face) = major {
            next.major.latin = face.to_string();
        }
        if let Some(face) = minor {
            next.minor.latin = face.to_string();
        }
    }
    Ok(next)
}

pub(super) fn effects(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("design.effects", "missing `name`"))?;
    let scheme = builtin_format_schemes()
        .into_iter()
        .find(|f| f.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| bad("design.effects", format!("no effect scheme `{name}`")))?;
    let stored = scheme.name.clone();
    let same = {
        let st = s.doc()?;
        let mi = master_index(&st.doc, &st.selection);
        st.doc.masters.get(mi).is_some_and(|m| m.theme.format == scheme)
    };
    if same {
        return Ok(json!({"name": stored}));
    }
    s.edit(|doc, sel| {
        let mi = master_index(doc, sel);
        Arc::make_mut(doc.masters.get_mut(mi).ok_or_else(|| bad("design.effects", "no master"))?).theme.format = scheme;
        Ok(())
    })?;
    Ok(json!({"name": stored}))
}
