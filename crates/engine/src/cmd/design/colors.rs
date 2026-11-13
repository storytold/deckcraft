use super::{
    Arc, Result, Session, Value, bad, builtin_color_schemes, builtin_font_schemes, builtin_format_schemes, json, master_index, ok, str_param,
};

pub(super) fn colors(s: &mut Session, p: &Value) -> Result<Value> {
    let name_v = p.get("name");
    let colors_v = p.get("colors");
    if name_v.is_none() && colors_v.is_none() {
        let sc = current_colors(s)?;
        s.ui_requests.push(crate::UiRequest::Dialog { id: "themeColors".into(), params: scheme_dialog_params(&sc) });
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
    let current = current_colors(s)?;
    let next = next_scheme(&current, given.as_deref(), slots.as_deref())?;
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

fn scheme_dialog_params(sc: &deckcraft_color::ColorScheme) -> Value {
    let mut colors = serde_json::Map::new();
    for slot in deckcraft_color::SchemeSlot::THEME {
        colors.insert(slot.xml_name().to_string(), Value::String(format!("#{}", sc.get(slot).hex())));
    }
    json!({"name": sc.name, "colors": colors})
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
