//! Design tab and masters: themes, colours, fonts, backgrounds, slide size, header & footer,
//! Slide Master view.

use std::sync::Arc;

use deckcraft_geom::Size;
use deckcraft_model::text::{Run, RunKind, TextBody};
use deckcraft_model::theme::{builtin_color_schemes, builtin_font_schemes, builtin_format_schemes, builtin_themes};
use deckcraft_model::{Background, Fill, PhType, Presentation, Shape, ShapeId, defaults};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session, Target};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("design.theme", "Themes", ["Design", "Themes"], None, "{name} (see design.themes)", has_doc, theme),
        cmd!(
            "design.browseTheme",
            "Browse for Themes…",
            ["Design", "Themes"],
            None,
            "{path} | {name, data: base64} — apply the theme of a .thmx, .pptx or .potx file",
            has_doc,
            browse_theme
        ),
        cmd!(query "design.themes", "Theme Gallery", [], None, "{} → themes, colour schemes, font schemes and effect schemes", always, themes),
        cmd!(
            "design.colors",
            "Colors",
            ["Design", "Variants"],
            None,
            "{} opens Create New Theme Colors | {name} | {colors: {dk1|lt1|dk2|lt2|accent1..6|hlink|folHlink: #RRGGBB}}",
            has_doc,
            colors
        ),
        cmd!("design.variant", "Variants", ["Design", "Variants"], None, "{index: 0..3} (see design.variants)", has_doc, variant),
        cmd!(query "design.variants", "Variant Gallery", [], None, "{} → the current theme's four colour variants and which is applied", has_doc, variants),
        cmd!("design.fonts", "Fonts", ["Design", "Variants"], None, "{} opens Create New Theme Fonts | {name} | {major, minor}", has_doc, fonts),
        cmd!("design.effects", "Effects", ["Design", "Variants"], None, "{name} (see design.themes)", has_doc, effects),
        cmd!(
            "design.background",
            "Format Background",
            ["Design", "Customize"],
            None,
            "{color? | gradient? | picture?: base64 | pattern? | style?: 1..12 | reset?, all?: bool, index?, transparency?: 0..1, pictureProps?: {tile?, offsetX?, offsetY?: pt, scaleX?, scaleY?: 0.01..10, align?, mirror?, stretch?: [l,t,r,b], rotateWithShape?}, copy?: bool}",
            has_slide,
            background
        ),
        cmd!("design.hideBackgroundGraphics", "Hide Background Graphics", ["Format Background"], None, "{hide?: bool}", has_slide, hide_bg_graphics),
        cmd!(
            "design.slideSize",
            "Slide Size",
            ["Design", "Customize"],
            None,
            "{preset: widescreen|standard|…, orientation?: portrait|landscape, scale?: maximize|ensureFit|none} | {w: pt, h: pt, orientation?: portrait|landscape, scale?: maximize|ensureFit|none}",
            has_doc,
            slide_size
        ),
        cmd!(
            "design.headerFooter",
            "Header and Footer…",
            ["Insert", "Text"],
            None,
            "{date?: bool, dateText?: fixed text, slideNumber?: bool, footer?: bool, footerText?, hideOnTitle?: bool, all?: bool (default true)}",
            has_doc,
            header_footer
        ),
        cmd!(noundo "view.slideMaster", "Slide Master", ["View", "Master Views"], None, "{master?: index, layout?: index}", has_doc, master_view),
        cmd!(
            "view.handoutMaster",
            "Handout Master",
            ["View", "Master Views"],
            None,
            "{} (creates the handout master with header, date, footer and page number if the deck has none)",
            has_doc,
            handout_view
        ),
        cmd!(
            "handout.placeholders",
            "Handout Placeholders",
            ["Handout Master", "Placeholders"],
            None,
            "{header?: bool, date?: bool, footer?: bool, pageNumber?: bool} (show or hide each)",
            in_handout,
            handout_placeholders
        ),
        cmd!(
            "view.notesMaster",
            "Notes Master",
            ["View", "Master Views"],
            None,
            "{} (creates the notes master with header, date, slide image, notes body, footer and page number if the deck has none)",
            has_doc,
            notes_view
        ),
        cmd!(
            "notes.placeholders",
            "Notes Placeholders",
            ["Notes Master", "Placeholders"],
            None,
            "{header?, date?, slideImage?, body?, footer?, pageNumber?: bool} (show or hide each)",
            in_notes_master,
            notes_placeholders
        ),
        cmd!(noundo "view.closeMaster", "Close Master View", ["Slide Master", "Close"], None, "{}", has_doc, close_master),
        cmd!("master.insertLayout", "Insert Layout", ["Slide Master", "Edit Master"], None, "{name?}", has_doc, insert_layout),
        cmd!("master.renameLayout", "Rename Layout", ["Slide Master", "Edit Master"], None, "{name, master?, layout?}", has_doc, rename_layout),
        cmd!("master.deleteLayout", "Delete Layout", ["Slide Master", "Edit Master"], None, "{master?, layout?}", has_doc, delete_layout),
        cmd!(
            "master.insertPlaceholder",
            "Insert Placeholder",
            ["Slide Master", "Master Layout"],
            None,
            "{kind: content|text|picture|chart|table|media, rect?: [x,y,w,h]}",
            has_doc,
            insert_placeholder
        ),
        cmd!("master.preserve", "Preserve", ["Slide Master", "Edit Master"], None, "{preserve?: bool}", has_doc, preserve),
        cmd!(
            "master.placeholders",
            "Layout Placeholders",
            ["Slide Master", "Edit Master"],
            None,
            "{title?: bool, footers?: bool}",
            has_doc,
            placeholders
        ),
    ]
}

fn theme(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("design.theme", "missing `name`"))?;
    let t =
        builtin_themes().into_iter().find(|t| t.name.eq_ignore_ascii_case(name)).ok_or_else(|| bad("design.theme", format!("no theme `{name}`")))?;
    s.edit(|doc, sel| {
        let mi = master_index(doc, sel);
        let m = doc.masters.get_mut(mi).ok_or_else(|| bad("design.theme", "no master"))?;
        let m = Arc::make_mut(m);
        m.theme = t.clone();
        m.name = t.name.clone();
        m.color_map = vec![];
        Ok(())
    })?;
    Ok(json!({"theme": name}))
}

fn browse_theme(s: &mut Session, p: &Value) -> Result<Value> {
    let bytes = if let Some(data) = str_param(p, "data") {
        super::base64_decode(data).ok_or_else(|| bad("design.browseTheme", "invalid base64 `data`"))?
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = str_param(p, "path").ok_or_else(|| bad("design.browseTheme", "give `path` or `data`"))?;
            crate::io::read_file(path).map_err(crate::EngineError::Other)?
        }
        #[cfg(target_arch = "wasm32")]
        return Err(bad("design.browseTheme", "give `data`"));
    };
    if !deckcraft_pptx::sniff(&bytes) {
        return Err(bad("design.browseTheme", "not a theme or PowerPoint file"));
    }
    let (theme, media) = deckcraft_pptx::import_theme(&bytes).map_err(|e| bad("design.browseTheme", e.to_string()))?;
    let name = theme.name.clone();
    s.edit(|doc, sel| {
        // The theme's pictures (format-scheme blip fills) get ids of this deck.
        let mut map = std::collections::HashMap::new();
        for m in &media {
            let id = doc.add_media(&m.name, &m.content_type, m.data.as_ref().clone()).ok_or_else(|| bad("design.browseTheme", NO_ID_SPACE))?;
            map.insert(m.id.0, id.0);
        }
        let theme = if map.is_empty() { theme.clone() } else { remap_media_ids(&theme, &map)? };
        let mi = master_index(doc, sel);
        let m = Arc::make_mut(doc.masters.get_mut(mi).ok_or_else(|| bad("design.browseTheme", "no master"))?);
        m.theme = theme;
        m.color_map = vec![];
        Ok(())
    })?;
    Ok(json!({"theme": name}))
}

/// `theme` with every `media` id field rewritten through `map` (format-scheme picture fills).
fn remap_media_ids(theme: &deckcraft_model::theme::Theme, map: &std::collections::HashMap<u32, u32>) -> Result<deckcraft_model::theme::Theme> {
    fn rec(v: &mut Value, map: &std::collections::HashMap<u32, u32>, depth: usize) {
        if depth > 64 {
            return;
        }
        match v {
            Value::Object(o) => {
                for (k, x) in o.iter_mut() {
                    if k == "media"
                        && let Some(n) = x.as_u64().and_then(|n| u32::try_from(n).ok())
                        && let Some(new) = map.get(&n)
                    {
                        *x = json!(new);
                    } else {
                        rec(x, map, depth + 1);
                    }
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| rec(x, map, depth + 1)),
            _ => {}
        }
    }
    let mut v = serde_json::to_value(theme).map_err(|e| bad("design.browseTheme", e.to_string()))?;
    rec(&mut v, map, 0);
    serde_json::from_value(v).map_err(|e| bad("design.browseTheme", e.to_string()))
}

fn master_index(doc: &Presentation, sel: &crate::Selection) -> usize {
    match sel.target {
        Target::Master { master } | Target::Layout { master, .. } => master,
        Target::Slides | Target::Handout | Target::NotesMaster => {
            doc.slides.get(sel.slide).and_then(|s| doc.masters.iter().position(|m| m.layout(s.layout).is_some())).unwrap_or(0)
        }
    }
}

fn themes(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({
        "themes": builtin_themes().iter().map(|t| json!({"name": t.name, "major": t.fonts.major.latin, "minor": t.fonts.minor.latin, "colors": t.colors.colors.iter().map(|c| format!("#{}", c.hex())).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "colorSchemes": builtin_color_schemes().iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
        "fontSchemes": builtin_font_schemes().iter().map(|f| json!({"name": f.name, "major": f.major.latin, "minor": f.minor.latin})).collect::<Vec<_>>(),
        "effectSchemes": builtin_format_schemes().iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
    }))
}

fn colors(s: &mut Session, p: &Value) -> Result<Value> {
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

fn variant(s: &mut Session, p: &Value) -> Result<Value> {
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

fn variants(s: &mut Session, _p: &Value) -> Result<Value> {
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

fn fonts(s: &mut Session, p: &Value) -> Result<Value> {
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

fn effects(s: &mut Session, p: &Value) -> Result<Value> {
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

fn background(s: &mut Session, p: &Value) -> Result<Value> {
    if bool_or(p, "copy", false) {
        return copy_slide_background(s);
    }
    let props = p.get("pictureProps").filter(|v| v.is_object()).cloned();
    let transparency = f64_param(p, "transparency");
    let reset = bool_or(p, "reset", false);
    let color = color_param(p, "color");
    let style = usize_param(p, "style");
    let op = if reset {
        BgOp::Set(None)
    } else if let Some(mut c) = color {
        if let Some(t) = transparency {
            set_solid_transparency(&mut c, t);
        }
        BgOp::Set(Some(Background::Fill { fill: Fill::solid(c) }))
    } else if let Some(i) = style {
        // Background styles: 3 theme bg fills × 4 colours (bg1, bg2, tx1, tx2).
        let slots =
            [deckcraft_color::SchemeSlot::Bg1, deckcraft_color::SchemeSlot::Bg2, deckcraft_color::SchemeSlot::Tx2, deckcraft_color::SchemeSlot::Tx1];
        let i = i.clamp(1, 12) - 1;
        BgOp::Set(Some(Background::Ref {
            idx: 1001 + (i / 4) as u32,
            color: deckcraft_model::ColorRef::scheme(slots.get(i % 4).copied().unwrap_or(deckcraft_color::SchemeSlot::Bg1)),
        }))
    } else if let Some(g) = p.get("gradient") {
        let mut g = super::shape::gradient_param(g).ok_or_else(|| bad("design.background", "gradient needs two or more `stops`"))?;
        g.rotate_with_shape = false;
        BgOp::Set(Some(Background::Fill { fill: Fill::Gradient(g) }))
    } else if let Some(pt) = p.get("pattern") {
        BgOp::Set(Some(Background::Fill {
            fill: Fill::Pattern(deckcraft_model::style::PatternFill {
                preset: pt.get("preset").and_then(Value::as_str).unwrap_or("pct50").to_string(),
                fg: pt.get("fg").and_then(color_value).unwrap_or(deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Accent1)),
                bg: pt.get("bg").and_then(color_value).unwrap_or(deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Bg1)),
            }),
        }))
    } else if p.get("picture").is_some() {
        let (name, bytes) = super::insert::media_bytes(&with_param(p, "data", p.get("picture").cloned().unwrap_or_default()), "design.background")?;
        let ct = super::insert::content_type(&name, &bytes);
        let media = s.edit(|doc, _| doc.add_media(&name, ct, bytes).ok_or_else(|| bad("design.background", NO_ID_SPACE)))?;
        let mut pf = deckcraft_model::style::PictureFill { media, ..Default::default() };
        if let Some(pp) = &props {
            apply_picture_props(&mut pf, pp);
        }
        if let Some(t) = transparency {
            set_picture_transparency(&mut pf, t);
        }
        BgOp::Set(Some(Background::Fill { fill: Fill::Picture(pf) }))
    } else if props.is_some() || transparency.is_some() {
        BgOp::Adjust
    } else {
        return Err(bad(
            "design.background",
            "give `color`, `gradient`, `pattern`, `picture`, `style`, `reset`, `pictureProps`, `transparency` or `copy`",
        ));
    };
    let all = bool_or(p, "all", false);
    let index = usize_param(p, "index");
    if matches!(op, BgOp::Adjust) {
        let next = {
            let st = s.doc()?;
            let current = background_of(&st.doc, &st.selection, index);
            let picture = matches!(current, Some(Background::Fill { fill: Fill::Picture(_) }));
            let solid = matches!(current, Some(Background::Fill { fill: Fill::Solid { .. } }));
            if props.is_some() && !picture {
                return Err(bad("design.background", "picture options need a picture background"));
            }
            if transparency.is_some() && !picture && !solid {
                return Err(bad("design.background", "transparency needs a solid or picture background"));
            }
            let mut next = current.clone();
            if let Some(bg) = next.as_mut() {
                adjust_background(bg, props.as_ref(), transparency);
            }
            if next == current { None } else { Some(next) }
        };
        let Some(next) = next else { return ok() };
        s.edit(|doc, sel| {
            write_background(doc, sel, index, false, next);
            Ok(())
        })?;
        return ok();
    }
    let BgOp::Set(bg) = op else { return ok() };
    let unchanged = {
        let st = s.doc()?;
        backgrounds_already(&st.doc, &st.selection, index, all, &bg)
    };
    if unchanged {
        return ok();
    }
    s.edit(|doc, sel| {
        write_background(doc, sel, index, all, bg);
        Ok(())
    })?;
    ok()
}

enum BgOp {
    Set(Option<Background>),
    Adjust,
}

fn background_of(doc: &Presentation, sel: &crate::Selection, index: Option<usize>) -> Option<Background> {
    match sel.target {
        Target::Master { master } => doc.masters.get(master).and_then(|m| m.background.clone()),
        Target::Layout { master, layout } => doc.masters.get(master).and_then(|m| m.layouts.get(layout)).and_then(|l| l.background.clone()),
        t @ (Target::Handout | Target::NotesMaster) => crate::page_master(doc, t).and_then(|m| m.background.clone()),
        Target::Slides => doc.slides.get(index.unwrap_or(sel.slide)).and_then(|sl| sl.background.clone()),
    }
}

fn backgrounds_already(doc: &Presentation, sel: &crate::Selection, index: Option<usize>, all: bool, bg: &Option<Background>) -> bool {
    match sel.target {
        Target::Master { master } => doc.masters.get(master).is_some_and(|m| m.background == *bg),
        Target::Layout { master, layout } => doc.masters.get(master).and_then(|m| m.layouts.get(layout)).is_some_and(|l| l.background == *bg),
        t @ (Target::Handout | Target::NotesMaster) => crate::page_master(doc, t).is_some_and(|m| m.background == *bg),
        Target::Slides => {
            let list: Vec<usize> = if all { (0..doc.slides.len()).collect() } else { vec![index.unwrap_or(sel.slide)] };
            !list.is_empty() && list.iter().all(|i| doc.slides.get(*i).is_some_and(|sl| sl.background == *bg))
        }
    }
}

fn write_background(doc: &mut Presentation, sel: &crate::Selection, index: Option<usize>, all: bool, bg: Option<Background>) {
    match sel.target {
        Target::Master { master } => {
            if let Some(m) = doc.masters.get_mut(master) {
                Arc::make_mut(m).background = bg;
            }
        }
        Target::Layout { master, layout } => {
            if let Some(l) = doc.masters.get_mut(master).and_then(|m| Arc::make_mut(m).layouts.get_mut(layout)) {
                l.background = bg;
            }
        }
        t @ (Target::Handout | Target::NotesMaster) => {
            if let Some(m) = crate::page_master_mut(doc, t) {
                Arc::make_mut(m).background = bg;
            }
        }
        Target::Slides => {
            let list: Vec<usize> = if all { (0..doc.slides.len()).collect() } else { vec![index.unwrap_or(sel.slide)] };
            for i in list {
                if let Some(sl) = doc.slides.get_mut(i) {
                    Arc::make_mut(sl).background = bg.clone();
                }
            }
        }
    }
}

fn copy_slide_background(s: &mut Session) -> Result<Value> {
    let bg = {
        let st = s.doc()?;
        if !matches!(st.selection.target, Target::Slides) {
            return Err(bad("design.background", "copy applies to slides"));
        }
        let bg = st.doc.slides.get(st.selection.slide).and_then(|sl| sl.background.clone());
        if st.doc.slides.iter().all(|sl| sl.background == bg) {
            return ok();
        }
        bg
    };
    s.edit(|doc, sel| {
        write_background(doc, sel, None, true, bg);
        Ok(())
    })?;
    ok()
}

fn adjust_background(bg: &mut Background, props: Option<&Value>, transparency: Option<f64>) {
    match bg {
        Background::Fill { fill: Fill::Picture(pf) } => {
            if let Some(pp) = props {
                apply_picture_props(pf, pp);
            }
            if let Some(t) = transparency {
                set_picture_transparency(pf, t);
            }
        }
        Background::Fill { fill: Fill::Solid { color } } => {
            if let Some(t) = transparency {
                set_solid_transparency(color, t);
            }
        }
        _ => {}
    }
}

fn set_picture_transparency(pf: &mut deckcraft_model::style::PictureFill, t: f64) {
    let a = 1.0 - t.clamp(0.0, 1.0);
    pf.alpha = (a < 1.0).then_some(a);
}

fn set_solid_transparency(color: &mut deckcraft_model::ColorRef, t: f64) {
    color.mods.retain(|m| !matches!(m, deckcraft_color::ColorTransform::Alpha(_)));
    let t = t.clamp(0.0, 1.0);
    if t > 0.0 {
        color.mods.push(deckcraft_color::ColorTransform::Alpha(((1.0 - t) * 100_000.0).round() as i32));
    }
}

/// Picture-fill options for Format Background (tile, offsets, scale, alignment, mirror, stretch).
fn apply_picture_props(pf: &mut deckcraft_model::style::PictureFill, pp: &Value) {
    use deckcraft_model::style::PictureMode;
    let f = |k: &str| pp.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
    if let Some(up) = pp.get("rotateWithShape").and_then(Value::as_bool) {
        pf.upright = !up;
    }
    match pp.get("tile").and_then(Value::as_bool) {
        Some(true) if !matches!(pf.mode, PictureMode::Tile { .. }) => {
            pf.mode = PictureMode::Tile { tx: 0.0, ty: 0.0, sx: 1.0, sy: 1.0, flip: "none".into(), align: "tl".into() };
        }
        Some(false) if !matches!(pf.mode, PictureMode::Stretch { .. }) => pf.mode = PictureMode::default(),
        _ => {}
    }
    match &mut pf.mode {
        PictureMode::Tile { tx, ty, sx, sy, flip, align } => {
            if let Some(v) = f("offsetX") {
                *tx = v.clamp(-4000.0, 4000.0);
            }
            if let Some(v) = f("offsetY") {
                *ty = v.clamp(-4000.0, 4000.0);
            }
            if let Some(v) = f("scaleX") {
                *sx = v.clamp(0.01, 10.0);
            }
            if let Some(v) = f("scaleY") {
                *sy = v.clamp(0.01, 10.0);
            }
            if let Some(m) = pp.get("mirror").and_then(Value::as_str).filter(|m| matches!(*m, "none" | "x" | "y" | "xy")) {
                *flip = m.to_string();
            }
            if let Some(a) =
                pp.get("align").and_then(Value::as_str).filter(|a| matches!(*a, "tl" | "t" | "tr" | "l" | "ctr" | "r" | "bl" | "b" | "br"))
            {
                *align = a.to_string();
            }
        }
        PictureMode::Stretch { fill_rect } => {
            if let Some(arr) = pp.get("stretch").and_then(Value::as_array) {
                for (slot, v) in fill_rect.iter_mut().zip(arr) {
                    if let Some(v) = v.as_f64().filter(|v| v.is_finite()) {
                        *slot = v.clamp(-10.0, 0.99);
                    }
                }
            }
        }
    }
}

fn hide_bg_graphics(s: &mut Session, p: &Value) -> Result<Value> {
    s.edit(|doc, sel| {
        if let Some(sl) = doc.slides.get_mut(sel.slide) {
            let sl = Arc::make_mut(sl);
            sl.show_master_shapes = !bool_param(p, "hide").unwrap_or(sl.show_master_shapes);
        }
        Ok(())
    })?;
    ok()
}

fn scale_shapes(shapes: &mut [Shape], sx: f64, sy: f64, k: f64, ox: f64, oy: f64) {
    for sh in shapes {
        if let Some(x) = sh.xfrm.as_mut() {
            x.x = x.x * sx + ox;
            x.y = x.y * sy + oy;
            x.w *= sx;
            x.h *= sy;
        }
        if let Some(t) = sh.text.as_mut() {
            deckcraft_model::edit::format_all(t, &|r| {
                if let Some(sz) = r.size.as_mut() {
                    *sz = (*sz * k).max(1.0);
                }
            });
        }
    }
}

fn slide_size(s: &mut Session, p: &Value) -> Result<Value> {
    let size = if let Some(pre) = str_param(p, "preset") {
        let k = pre.to_ascii_lowercase();
        let found = defaults::SLIDE_SIZES.iter().find(|(l, _, _)| l.to_ascii_lowercase().starts_with(&k) || l.to_ascii_lowercase().contains(&k));
        let (_, w, h) = found.ok_or_else(|| bad("design.slideSize", format!("unknown preset `{pre}`")))?;
        Size::new(*w, *h)
    } else {
        Size::new(
            f64_param(p, "w").ok_or_else(|| bad("design.slideSize", "missing `w`"))?.clamp(72.0, 4032.0),
            f64_param(p, "h").ok_or_else(|| bad("design.slideSize", "missing `h`"))?.clamp(72.0, 4032.0),
        )
    };
    let size = match str_param(p, "orientation") {
        Some("portrait") if size.width > size.height => Size::new(size.height, size.width),
        Some("landscape") if size.height > size.width => Size::new(size.height, size.width),
        Some("portrait" | "landscape") | None => size,
        Some(o) => return Err(bad("design.slideSize", format!("unknown orientation `{o}`"))),
    };
    let mode = str_param(p, "scale").unwrap_or("ensureFit").to_string();
    s.edit(|doc, _| {
        let old = doc.slide_size;
        let (sx, sy) = (size.width / old.width.max(1.0), size.height / old.height.max(1.0));
        let (sx, sy, k, ox, oy) = match mode.as_str() {
            "none" => (1.0, 1.0, 1.0, 0.0, 0.0),
            "maximize" => {
                let k = sx.max(sy);
                (k, k, k, (size.width - old.width * k) / 2.0, (size.height - old.height * k) / 2.0)
            }
            _ => {
                let k = sx.min(sy);
                (k, k, k, (size.width - old.width * k) / 2.0, (size.height - old.height * k) / 2.0)
            }
        };
        // Masters and layouts stretch to the new size; slides scale by mode.
        let (msx, msy) = (size.width / old.width.max(1.0), size.height / old.height.max(1.0));
        for m in &mut doc.masters {
            let m = Arc::make_mut(m);
            scale_shapes(&mut m.shapes, msx, msy, 1.0, 0.0, 0.0);
            for l in &mut m.layouts {
                scale_shapes(&mut l.shapes, msx, msy, 1.0, 0.0, 0.0);
            }
        }
        for sl in &mut doc.slides {
            scale_shapes(&mut Arc::make_mut(sl).shapes, sx, sy, k, ox, oy);
        }
        doc.slide_size = size;
        Ok(())
    })?;
    Ok(json!({"w": size.width, "h": size.height}))
}

fn header_footer(s: &mut Session, p: &Value) -> Result<Value> {
    s.edit(|doc, sel| {
        let hf = &mut doc.header_footer;
        if let Some(v) = bool_param(p, "date") {
            hf.date = v;
        }
        if let Some(v) = str_param(p, "dateText") {
            hf.date_text = v.into();
        }
        if let Some(v) = bool_param(p, "slideNumber") {
            hf.slide_number = v;
        }
        if let Some(v) = bool_param(p, "footer") {
            hf.footer = v;
        }
        if let Some(v) = str_param(p, "footerText") {
            hf.footer_text = v.into();
            hf.footer = hf.footer || !v.is_empty();
        }
        if let Some(v) = bool_param(p, "hideOnTitle") {
            hf.hide_on_title = v;
        }
        let hf = hf.clone();
        let date_given = str_param(p, "dateText").is_some();
        let all = bool_or(p, "all", true);
        let list: Vec<usize> = if all { (0..doc.slides.len()).collect() } else { vec![sel.slide] };
        for i in list {
            let Some(layout) = doc.slides.get(i).map(|x| x.layout) else { continue };
            let lay = doc.layout(layout).map(|(_, l)| l.clone());
            let is_title = lay.as_ref().is_some_and(|l| l.kind == deckcraft_model::LayoutType::Title);
            let want = |k: PhType| {
                !(hf.hide_on_title && is_title)
                    && match k {
                        PhType::Date => hf.date,
                        PhType::SlideNum => hf.slide_number,
                        PhType::Footer => hf.footer,
                        _ => false,
                    }
            };
            let mut new_ids = vec![];
            for _ in 0..3 {
                new_ids.push(ShapeId(doc.alloc_id().ok_or_else(|| bad("design.headerFooter", NO_ID_SPACE))?));
            }
            let Some(sl) = doc.slides.get_mut(i) else { continue };
            let sl = Arc::make_mut(sl);
            for (n, k) in [PhType::Date, PhType::Footer, PhType::SlideNum].into_iter().enumerate() {
                let has = sl.shapes.iter().position(|x| x.ph_type() == Some(k));
                match (has, want(k)) {
                    (Some(pos), false) => {
                        sl.shapes.remove(pos);
                    }
                    (None, true) => {
                        let lp = lay.as_ref().and_then(|l| l.shapes.iter().find(|x| x.ph_type() == Some(k)).cloned());
                        let Some(lp) = lp else { continue };
                        let body = match k {
                            PhType::SlideNum => TextBody {
                                paragraphs: vec![deckcraft_model::Paragraph {
                                    runs: vec![Run {
                                        text: "‹#›".into(),
                                        props: Default::default(),
                                        kind: RunKind::Field { field: "slidenum".into() },
                                    }],
                                    ..Default::default()
                                }],
                                ..Default::default()
                            },
                            PhType::Date if hf.date_text.is_empty() => TextBody {
                                paragraphs: vec![deckcraft_model::Paragraph {
                                    runs: vec![Run { text: today(), props: Default::default(), kind: RunKind::Field { field: "datetime1".into() } }],
                                    ..Default::default()
                                }],
                                ..Default::default()
                            },
                            PhType::Date => TextBody::from_text(&hf.date_text),
                            _ => TextBody::from_text(&hf.footer_text),
                        };
                        sl.shapes.push(Shape {
                            id: new_ids.get(n).copied().unwrap_or_default(),
                            name: lp.name.clone(),
                            ph: lp.ph.clone(),
                            text: Some(body),
                            ..Default::default()
                        });
                    }
                    (Some(pos), true) if k == PhType::Footer => {
                        if let Some(sh) = sl.shapes.get_mut(pos) {
                            sh.text = Some(TextBody::from_text(&hf.footer_text));
                        }
                    }
                    (Some(pos), true) if k == PhType::Date && date_given => {
                        if let Some(sh) = sl.shapes.get_mut(pos) {
                            set_date(sh, &hf.date_text);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })?;
    ok()
}

/// Point a slide's date placeholder at a fixed date (its own plain text) or, for an empty
/// `fixed`, at an automatic date field (one it already has keeps its format).
fn set_date(sh: &mut Shape, fixed: &str) {
    let runs = || sh.text.iter().flat_map(|t| &t.paragraphs).flat_map(|p| &p.runs);
    let has_field = runs().any(|r| matches!(&r.kind, RunKind::Field { field } if deckcraft_text::datetime::is_date_field(field)));
    let props = runs().next().map(|r| r.props.clone()).unwrap_or_default();
    let (text, kind) = match fixed {
        "" if has_field => return,
        "" => (today(), RunKind::Field { field: "datetime1".into() }),
        _ if !has_field && sh.text.as_ref().is_some_and(|t| t.text() == fixed) => return,
        _ => (fixed.to_string(), RunKind::Text),
    };
    let para = deckcraft_model::Paragraph { runs: vec![Run { text, props, kind }], ..Default::default() };
    sh.text = Some(TextBody { paragraphs: vec![para], ..sh.text.clone().unwrap_or_default() });
}

/// Today's local date as a `datetime1` field shows it (M/D/YYYY): the saved text of a new date
/// field. Empty where there is no clock (wasm).
pub fn today() -> String {
    date_field_text("datetime1")
}

/// What date field `kind` shows now (empty if unknown or there is no clock).
pub fn date_field_text(kind: &str) -> String {
    use deckcraft_text::datetime::{DateTime, field_text};
    DateTime::now_local().and_then(|now| field_text(kind, None, now)).unwrap_or_default()
}

/// Days since 1970-01-01 → (year, month, day) (Howard Hinnant's algorithm).
pub fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn master_view(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let mi = usize_param(p, "master").unwrap_or_else(|| master_index(&st.doc, &st.selection));
    // A master thumbnail sends `{master}` and means the master itself. `{master, layout}` selects
    // that layout. With neither key, follow the current slide's layout.
    let layout = if p.get("master").is_some() && p.get("layout").is_none() {
        None
    } else {
        usize_param(p, "layout")
            .or_else(|| st.current_slide().and_then(|sl| st.doc.masters.get(mi).and_then(|m| m.layouts.iter().position(|l| l.id == sl.layout))))
    };
    let target = match layout {
        Some(l) => Target::Layout { master: mi, layout: l },
        None => Target::Master { master: mi },
    };
    s.select(|_, sel| {
        sel.target = target;
        sel.shapes.clear();
        sel.text = None;
    })?;
    Ok(serde_json::to_value(target).unwrap_or_default())
}

fn in_handout(s: &Session) -> std::result::Result<(), String> {
    if s.active().is_some_and(|d| d.selection.target == Target::Handout) { Ok(()) } else { Err("open View ▸ Handout Master first".into()) }
}

/// The four handout-master placeholders PowerPoint offers, in a page of `size`.
pub fn handout_placeholder(kind: PhType, size: Size, id: ShapeId) -> Shape {
    let (w, h) = (size.width, size.height);
    let (name, idx, x, y) = match kind {
        PhType::Header => ("Header Placeholder", 0, 0.0, 0.0),
        PhType::Date => ("Date Placeholder", 1, w * 0.57, 0.0),
        PhType::Footer => ("Footer Placeholder", 2, 0.0, h * 0.95),
        _ => ("Slide Number Placeholder", 3, w * 0.57, h * 0.95),
    };
    let mut text = TextBody::default();
    if let Some(p) = text.paragraphs.first_mut()
        && matches!(kind, PhType::Date | PhType::SlideNum)
    {
        p.props.align = Some(deckcraft_model::text::Align::Right);
    }
    Shape {
        id,
        name: format!("{name} {}", idx + 1),
        xfrm: Some(deckcraft_geom::Xfrm::new(x, y, w * 0.43, h * 0.05)),
        ph: Some(deckcraft_model::Placeholder { kind, idx, size: Some("quarter".into()), ..Default::default() }),
        text: Some(text),
        ..Default::default()
    }
}

const HANDOUT_KINDS: [(&str, PhType); 4] =
    [("header", PhType::Header), ("date", PhType::Date), ("footer", PhType::Footer), ("pageNumber", PhType::SlideNum)];

fn handout_view(s: &mut Session, _p: &Value) -> Result<Value> {
    let created = s.doc()?.doc.handout_master.is_none();
    if created {
        s.edit(|doc, _| {
            let mut m = deckcraft_model::Master {
                name: "Handout Master".into(),
                theme: doc.masters.first().map(|m| m.theme.clone()).unwrap_or_default(),
                color_map: doc.masters.first().map(|m| m.color_map.clone()).unwrap_or_default(),
                ..Default::default()
            };
            let size = doc.notes_size;
            for (_, k) in HANDOUT_KINDS {
                let id = ShapeId(doc.alloc_id().ok_or_else(|| bad("view.handoutMaster", NO_ID_SPACE))?);
                m.shapes.push(handout_placeholder(k, size, id));
            }
            doc.handout_master = Some(Arc::new(m));
            Ok(())
        })?;
    }
    s.select(|_, sel| {
        sel.target = Target::Handout;
        sel.shapes.clear();
        sel.text = None;
    })?;
    Ok(json!({"created": created}))
}

fn handout_placeholders(s: &mut Session, p: &Value) -> Result<Value> {
    s.edit(|doc, _| {
        let size = doc.notes_size;
        let mut ids = vec![];
        for (key, _) in HANDOUT_KINDS {
            let id = match p.get(key).and_then(Value::as_bool) {
                Some(_) => Some(ShapeId(doc.alloc_id().ok_or_else(|| bad("handout.placeholders", NO_ID_SPACE))?)),
                None => None,
            };
            ids.push(id);
        }
        let m = Arc::make_mut(doc.handout_master.as_mut().ok_or_else(|| bad("handout.placeholders", "no handout master"))?);
        for ((key, kind), id) in HANDOUT_KINDS.into_iter().zip(ids) {
            let (Some(on), Some(id)) = (p.get(key).and_then(Value::as_bool), id) else { continue };
            let has = m.shapes.iter().any(|x| x.ph_type() == Some(kind));
            if on && !has {
                m.shapes.push(handout_placeholder(kind, size, id));
            } else if !on {
                m.shapes.retain(|x| x.ph_type() != Some(kind));
            }
        }
        Ok(())
    })?;
    let m = s.doc()?.doc.handout_master.clone();
    let shown = |k: PhType| m.as_ref().is_some_and(|m| m.shapes.iter().any(|x| x.ph_type() == Some(k)));
    Ok(json!({"header": shown(PhType::Header), "date": shown(PhType::Date), "footer": shown(PhType::Footer), "pageNumber": shown(PhType::SlideNum)}))
}

fn in_notes_master(s: &Session) -> std::result::Result<(), String> {
    if s.active().is_some_and(|d| d.selection.target == Target::NotesMaster) { Ok(()) } else { Err("open View ▸ Notes Master first".into()) }
}

const NOTES_KINDS: [(&str, PhType); 6] = [
    ("header", PhType::Header),
    ("date", PhType::Date),
    ("slideImage", PhType::SlideImage),
    ("body", PhType::Body),
    ("footer", PhType::Footer),
    ("pageNumber", PhType::SlideNum),
];

/// A notes-master placeholder in a page of `size`, laid out as PowerPoint's default notes master.
pub fn notes_placeholder(kind: PhType, size: Size, id: ShapeId) -> Shape {
    let (w, h) = (size.width, size.height);
    match kind {
        PhType::SlideImage => Shape {
            id,
            name: "Slide Image Placeholder 3".into(),
            xfrm: Some(deckcraft_geom::Xfrm::new(w * 0.125, h * 0.125, w * 0.75, h * 0.375)),
            ph: Some(deckcraft_model::Placeholder { kind, idx: 2, ..Default::default() }),
            fill: Some(Fill::None),
            line: Some(deckcraft_model::Line::solid(deckcraft_model::ColorRef::rgb(deckcraft_color::Rgba::BLACK), 1.0)),
            ..Default::default()
        },
        PhType::Body => {
            let mut text = TextBody::default();
            text.paragraphs = vec![deckcraft_model::text::Paragraph::new("Click to edit Master text styles")];
            Shape {
                id,
                name: "Notes Placeholder 4".into(),
                xfrm: Some(deckcraft_geom::Xfrm::new(w * 0.1, h * 0.53, w * 0.8, h * 0.36)),
                ph: Some(deckcraft_model::Placeholder { kind, idx: 3, size: Some("quarter".into()), ..Default::default() }),
                text: Some(text),
                ..Default::default()
            }
        }
        _ => {
            let mut s = handout_placeholder(kind, size, id);
            if let Some(ph) = s.ph.as_mut() {
                ph.idx = match kind {
                    PhType::Header => 0,
                    PhType::Date => 1,
                    PhType::Footer => 4,
                    _ => 5,
                };
            }
            s
        }
    }
}

fn notes_view(s: &mut Session, _p: &Value) -> Result<Value> {
    let created = s.doc()?.doc.notes_master.as_ref().is_none_or(|m| m.shapes.is_empty());
    if created {
        s.edit(|doc, _| {
            let mut m = doc.notes_master.as_deref().cloned().unwrap_or_else(|| deckcraft_model::Master {
                name: "Notes Master".into(),
                theme: doc.masters.first().map(|m| m.theme.clone()).unwrap_or_default(),
                color_map: doc.masters.first().map(|m| m.color_map.clone()).unwrap_or_default(),
                ..Default::default()
            });
            let size = doc.notes_size;
            for (_, k) in NOTES_KINDS {
                let id = ShapeId(doc.alloc_id().ok_or_else(|| bad("view.notesMaster", NO_ID_SPACE))?);
                m.shapes.push(notes_placeholder(k, size, id));
            }
            doc.notes_master = Some(Arc::new(m));
            Ok(())
        })?;
    }
    s.select(|_, sel| {
        sel.target = Target::NotesMaster;
        sel.shapes.clear();
        sel.text = None;
    })?;
    Ok(json!({"created": created}))
}

fn notes_placeholders(s: &mut Session, p: &Value) -> Result<Value> {
    s.edit(|doc, _| {
        let size = doc.notes_size;
        let ids: Vec<ShapeId> =
            NOTES_KINDS.iter().map(|_| doc.alloc_id().map(ShapeId).ok_or_else(|| bad("notes.placeholders", NO_ID_SPACE))).collect::<Result<_>>()?;
        let m = Arc::make_mut(doc.notes_master.as_mut().ok_or_else(|| bad("notes.placeholders", "no notes master"))?);
        for ((key, kind), id) in NOTES_KINDS.into_iter().zip(ids) {
            let Some(on) = p.get(key).and_then(Value::as_bool) else { continue };
            let has = m.shapes.iter().any(|x| x.ph_type() == Some(kind));
            if on && !has {
                m.shapes.push(notes_placeholder(kind, size, id));
            } else if !on {
                m.shapes.retain(|x| x.ph_type() != Some(kind));
            }
        }
        Ok(())
    })?;
    let m = s.doc()?.doc.notes_master.clone();
    let mut out = serde_json::Map::new();
    for (key, kind) in NOTES_KINDS {
        out.insert(key.into(), json!(m.as_ref().is_some_and(|m| m.shapes.iter().any(|x| x.ph_type() == Some(kind)))));
    }
    Ok(Value::Object(out))
}

fn close_master(s: &mut Session, _p: &Value) -> Result<Value> {
    s.select(|_, sel| {
        sel.target = Target::Slides;
        sel.shapes.clear();
        sel.text = None;
    })?;
    ok()
}

fn insert_layout(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").unwrap_or("Custom Layout").to_string();
    s.edit(|doc, sel| {
        let mi = master_index(doc, sel);
        let id = deckcraft_model::LayoutId(doc.alloc_id().ok_or_else(|| bad("master.insertLayout", NO_ID_SPACE))?);
        let tid = ShapeId(doc.alloc_id().ok_or_else(|| bad("master.insertLayout", NO_ID_SPACE))?);
        let size = doc.slide_size;
        let m = Arc::make_mut(doc.masters.get_mut(mi).ok_or_else(|| bad("master.insertLayout", "no master"))?);
        let title = m.shapes.iter().find(|x| x.ph_type() == Some(PhType::Title)).cloned().map(|mut t| {
            t.id = tid;
            t.text = Some(TextBody::default());
            t
        });
        let _ = size;
        let at = match sel.target {
            Target::Layout { layout, .. } => layout + 1,
            _ => m.layouts.len(),
        };
        m.layouts.insert(
            at.min(m.layouts.len()),
            deckcraft_model::Layout {
                id,
                name,
                kind: deckcraft_model::LayoutType::Custom,
                shapes: title.into_iter().collect(),
                background: None,
                show_master_shapes: true,
                preserve: true,
                color_map: None,
                raw_ext: None,
            },
        );
        sel.target = Target::Layout { master: mi, layout: at.min(m.layouts.len().saturating_sub(1)) };
        Ok(json!({"id": id}))
    })
}

fn layout_target(s: &Session, p: &Value) -> Result<(usize, usize)> {
    let st = s.doc()?;
    let (m, l) = match st.selection.target {
        Target::Layout { master, layout } => (master, layout),
        _ => (master_index(&st.doc, &st.selection), 0),
    };
    Ok((usize_param(p, "master").unwrap_or(m), usize_param(p, "layout").unwrap_or(l)))
}

fn rename_layout(s: &mut Session, p: &Value) -> Result<Value> {
    let (mi, li) = layout_target(s, p)?;
    let name = str_param(p, "name").ok_or_else(|| bad("master.renameLayout", "missing `name`"))?.to_string();
    s.edit(|doc, _| {
        let l =
            doc.masters.get_mut(mi).and_then(|m| Arc::make_mut(m).layouts.get_mut(li)).ok_or_else(|| bad("master.renameLayout", "no such layout"))?;
        l.name = name;
        Ok(())
    })?;
    ok()
}

fn delete_layout(s: &mut Session, p: &Value) -> Result<Value> {
    let (mi, li) = layout_target(s, p)?;
    s.edit(|doc, sel| {
        let id = doc.masters.get(mi).and_then(|m| m.layouts.get(li)).map(|l| l.id).ok_or_else(|| bad("master.deleteLayout", "no such layout"))?;
        if doc.slides.iter().any(|x| x.layout == id) {
            return Err(bad("master.deleteLayout", "slides use this layout"));
        }
        let m = Arc::make_mut(doc.masters.get_mut(mi).ok_or_else(|| bad("master.deleteLayout", "no master"))?);
        if m.layouts.len() <= 1 {
            return Err(bad("master.deleteLayout", "a master needs at least one layout"));
        }
        m.layouts.remove(li);
        sel.target = Target::Layout { master: mi, layout: li.min(m.layouts.len().saturating_sub(1)) };
        Ok(())
    })?;
    ok()
}

fn insert_placeholder(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = match str_param(p, "kind").unwrap_or("content") {
        "text" => PhType::Body,
        "picture" => PhType::Picture,
        "chart" => PhType::Chart,
        "table" => PhType::Table,
        "media" => PhType::Media,
        _ => PhType::Obj,
    };
    if !matches!(s.doc()?.selection.target, Target::Layout { .. }) {
        return Err(bad("master.insertPlaceholder", "select a layout in Slide Master view"));
    }
    let size = s.doc()?.doc.slide_size;
    let rect = rect_param(p, "rect").unwrap_or(deckcraft_geom::Xfrm::new(size.width * 0.25, size.height * 0.3, size.width * 0.5, size.height * 0.4));
    let idx = s.doc()?.shapes().iter().filter_map(|x| x.ph.as_ref().map(|p| p.idx)).max().unwrap_or(0) + 1;
    let shape = Shape {
        xfrm: Some(rect),
        ph: Some(deckcraft_model::Placeholder { kind, idx, ..Default::default() }),
        text: Some(TextBody::default()),
        ..Default::default()
    };
    let id = super::insert::add_shape(s, "master.insertPlaceholder", shape, true)?;
    Ok(json!({"id": id}))
}

fn preserve(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let (on_master, mi, li, current) = match st.selection.target {
        Target::Master { master } => {
            let on = st.doc.masters.get(master).map(|m| m.preserve).ok_or_else(|| bad("master.preserve", "no such master"))?;
            (true, master, 0, on)
        }
        Target::Layout { master, layout } => {
            let on = st
                .doc
                .masters
                .get(master)
                .and_then(|m| m.layouts.get(layout))
                .map(|l| l.preserve)
                .ok_or_else(|| bad("master.preserve", "no such layout"))?;
            (false, master, layout, on)
        }
        Target::Slides | Target::Handout | Target::NotesMaster => return Err(bad("master.preserve", "open Slide Master view")),
    };
    let next = bool_param(p, "preserve").unwrap_or(!current);
    if next == current {
        return Ok(json!({"preserve": next}));
    }
    s.edit(|doc, _| {
        if on_master {
            let m = Arc::make_mut(doc.masters.get_mut(mi).ok_or_else(|| bad("master.preserve", "no such master"))?);
            m.preserve = next;
        } else {
            let l =
                doc.masters.get_mut(mi).and_then(|m| Arc::make_mut(m).layouts.get_mut(li)).ok_or_else(|| bad("master.preserve", "no such layout"))?;
            l.preserve = next;
        }
        Ok(())
    })?;
    Ok(json!({"preserve": next}))
}

fn placeholders(s: &mut Session, p: &Value) -> Result<Value> {
    let title = bool_param(p, "title");
    let footers = bool_param(p, "footers");
    if title.is_none() && footers.is_none() {
        return Err(bad("master.placeholders", "give title or footers"));
    }
    let st = s.doc()?;
    if matches!(st.selection.target, Target::Slides) {
        return Err(bad("master.placeholders", "open Slide Master view"));
    }
    let has_title = st.shapes().iter().any(|sh| sh.ph_type().is_some_and(PhType::is_title));
    let has_footers = st.shapes().iter().any(|sh| sh.ph_type().is_some_and(PhType::is_footer_kind));
    let title_changes = title.is_some_and(|want| want != has_title);
    let footer_changes = footers.is_some_and(|want| want != has_footers);
    if !title_changes && !footer_changes {
        return Ok(json!({"title": has_title, "footers": has_footers}));
    }
    let mut add = Vec::new();
    if title == Some(true) && !has_title {
        add.push(PhType::Title);
    }
    if footers == Some(true) && !has_footers {
        add.extend([PhType::Date, PhType::Footer, PhType::SlideNum]);
    }
    let size = st.doc.slide_size;
    s.edit(|doc, sel| {
        let mut ids = Vec::with_capacity(add.len());
        for _ in 0..add.len() {
            ids.push(ShapeId(doc.alloc_id().ok_or_else(|| bad("master.placeholders", NO_ID_SPACE))?));
        }
        let prototypes = match sel.target {
            Target::Master { master } | Target::Layout { master, .. } => doc.masters.get(master).map(|m| m.shapes.clone()).unwrap_or_default(),
            Target::Slides => Vec::new(),
            Target::Handout | Target::NotesMaster => return Err(bad("master.placeholders", "open Slide Master view")),
        };
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad("master.placeholders", "open Slide Master view"))?;
        if title == Some(false) {
            shapes.retain(|sh| !sh.ph_type().is_some_and(PhType::is_title));
        }
        if footers == Some(false) {
            shapes.retain(|sh| !sh.ph_type().is_some_and(PhType::is_footer_kind));
        }
        for (kind, id) in add.into_iter().zip(ids) {
            let idx = free_ph_idx(shapes, preferred_ph_idx(kind));
            let proto = prototypes.iter().find(|sh| match kind {
                PhType::Title => sh.ph_type().is_some_and(PhType::is_title),
                other => sh.ph_type() == Some(other),
            });
            let shape = match proto {
                Some(src) => retarget_placeholder(src.clone(), id, idx),
                None => synthesized_placeholder(kind, id, idx, size),
            };
            shapes.push(shape);
        }
        Ok(())
    })?;
    let st = s.doc()?;
    let title = st.shapes().iter().any(|sh| sh.ph_type().is_some_and(PhType::is_title));
    let footers = st.shapes().iter().any(|sh| sh.ph_type().is_some_and(PhType::is_footer_kind));
    Ok(json!({"title": title, "footers": footers}))
}

fn preferred_ph_idx(kind: PhType) -> u32 {
    match kind {
        PhType::Title | PhType::CtrTitle => 0,
        PhType::Date => 10,
        PhType::Footer => 11,
        PhType::SlideNum => 12,
        _ => 1,
    }
}

fn free_ph_idx(shapes: &[Shape], preferred: u32) -> u32 {
    let taken = |idx: u32| shapes.iter().any(|sh| sh.ph.as_ref().is_some_and(|p| p.idx == idx));
    if !taken(preferred) { preferred } else { shapes.iter().filter_map(|sh| sh.ph.as_ref().map(|p| p.idx)).max().unwrap_or(0).saturating_add(1) }
}

/// A copy of a master placeholder, with a new id and index. Prompt text is cleared; a slide
/// number keeps its `‹#›` field.
fn retarget_placeholder(mut sh: Shape, id: ShapeId, idx: u32) -> Shape {
    sh.id = id;
    if let Some(ph) = sh.ph.as_mut() {
        ph.idx = idx;
    }
    let slide_num = sh.ph_type() == Some(PhType::SlideNum);
    if slide_num {
        let paras = vec![slide_number_paragraph()];
        if let Some(text) = sh.text.as_mut() {
            text.paragraphs = paras;
        } else {
            sh.text = Some(TextBody { paragraphs: paras, ..Default::default() });
        }
    } else if let Some(text) = sh.text.as_mut() {
        text.paragraphs.clear();
    }
    sh
}

fn synthesized_placeholder(kind: PhType, id: ShapeId, idx: u32, size: Size) -> Shape {
    let (name, text) = match kind {
        PhType::Date => ("Date Placeholder", TextBody::default()),
        PhType::Footer => ("Footer Placeholder", TextBody::default()),
        PhType::SlideNum => ("Slide Number Placeholder", TextBody { paragraphs: vec![slide_number_paragraph()], ..Default::default() }),
        _ => ("Title", TextBody::default()),
    };
    Shape {
        id,
        name: name.into(),
        xfrm: Some(placeholder_rect(kind, size)),
        ph: Some(deckcraft_model::Placeholder { kind, idx, ..Default::default() }),
        text: Some(text),
        ..Default::default()
    }
}

/// Standard placeholder boxes at 960×540, scaled to the slide size.
fn placeholder_rect(kind: PhType, size: Size) -> deckcraft_geom::Xfrm {
    let sx = size.width / 960.0;
    let sy = size.height / 540.0;
    let (x, y, w, h) = match kind {
        PhType::Date => (60.0, 500.0, 216.0, 28.0),
        PhType::Footer => (318.0, 500.0, 324.0, 28.0),
        PhType::SlideNum => (684.0, 500.0, 216.0, 28.0),
        _ => (60.0, 29.0, 840.0, 104.0),
    };
    deckcraft_geom::Xfrm::new(x * sx, y * sy, w * sx, h * sy)
}

fn slide_number_paragraph() -> deckcraft_model::Paragraph {
    deckcraft_model::Paragraph {
        runs: vec![Run { text: "‹#›".into(), props: Default::default(), kind: RunKind::Field { field: "slidenum".into() } }],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use deckcraft_color::SchemeSlot;
    use deckcraft_model::style::PictureMode;
    use deckcraft_model::theme::{FontScheme, ScriptFont, builtin_color_schemes, builtin_font_schemes};
    use deckcraft_model::{Background, Fill};
    use serde_json::{Value, json};

    use crate::{Session, UiRequest};

    fn scheme(s: &Session) -> deckcraft_color::ColorScheme {
        s.doc().unwrap().doc.masters[0].theme.colors.clone()
    }

    fn font_scheme(s: &Session) -> FontScheme {
        s.doc().unwrap().doc.masters[0].theme.fonts.clone()
    }

    fn undo_len(s: &Session) -> usize {
        s.doc().unwrap().history.undo.len()
    }

    fn png() -> String {
        let mut img = image::RgbaImage::new(2, 2);
        img.put_pixel(0, 0, image::Rgba([10, 20, 30, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
        crate::cmd::base64_encode(&bytes)
    }

    fn picture(s: &Session) -> deckcraft_model::style::PictureFill {
        match &s.doc().unwrap().doc.slides[0].background {
            Some(Background::Fill { fill: Fill::Picture(pf) }) => pf.clone(),
            other => panic!("expected a picture background, got {other:?}"),
        }
    }

    #[test]
    fn civil_dates() {
        assert_eq!(super::civil(0), (1970, 1, 1));
        assert_eq!(super::civil(20_734), (2026, 10, 8));
    }

    #[test]
    fn colors_dialog_apply_and_reject() {
        let mut s = Session::with_new();
        let original = scheme(&s);
        let n = undo_len(&s);
        s.execute("design.colors", &json!({})).unwrap();
        s.execute("design.colors", &Value::Null).unwrap();
        assert_eq!(scheme(&s), original);
        assert_eq!(undo_len(&s), n, "opening the dialog does not edit");
        assert_eq!(s.ui_requests.len(), 2);
        match &s.ui_requests[0] {
            UiRequest::Dialog { id, params } => {
                assert_eq!(id, "themeColors");
                assert_eq!(params["name"], "DeckCraft");
                let colors = params["colors"].as_object().unwrap();
                assert_eq!(colors.len(), 12);
                assert_eq!(colors["accent1"].as_str().unwrap(), format!("#{}", original.get(SchemeSlot::Accent1).hex()));
                assert!(colors.contains_key("folHlink"));
            }
            other => panic!("expected a dialog, got {other:?}"),
        }

        s.execute("design.colors", &json!({"name": "Harbor"})).unwrap();
        let harbor = builtin_color_schemes().into_iter().find(|c| c.name == "Harbor").unwrap();
        assert_eq!(scheme(&s).name, "Harbor");
        assert_eq!(scheme(&s).get(SchemeSlot::Accent1), harbor.get(SchemeSlot::Accent1));
        assert_eq!(undo_len(&s), n + 1);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Colors");
        s.execute("design.colors", &json!({"name": "Harbor"})).unwrap();
        assert_eq!(undo_len(&s), n + 1, "the same scheme is not an undo step");

        let err = s.execute("design.colors", &json!({"name": "Nope"})).unwrap_err().to_string();
        assert!(err.contains("no colour scheme `Nope`"), "{err}");
        assert!(s.execute("design.colors", &json!({"name": "  "})).is_err());
        assert!(s.execute("design.colors", &json!({"name": 1})).is_err());
        assert!(s.execute("design.colors", &json!({"colors": {}})).is_err());
        assert!(s.execute("design.colors", &json!({"colors": "red"})).is_err());
        assert!(s.execute("design.colors", &json!({"colors": {"bg1": "#112233"}})).is_err());
        assert!(s.execute("design.colors", &json!({"colors": {"tx1": "#112233"}})).is_err());
        assert!(s.execute("design.colors", &json!({"colors": {"phClr": "#112233"}})).is_err());
        assert!(s.execute("design.colors", &json!({"colors": {"nope": "#112233"}})).is_err());
        assert!(s.execute("design.colors", &json!({"colors": {"accent1": "zz"}})).is_err());
        assert!(s.execute("design.colors", &json!({"name": "Harbor", "colors": {}})).is_err());
        assert_eq!(scheme(&s).name, "Harbor");
        assert_eq!(scheme(&s).get(SchemeSlot::Accent1), harbor.get(SchemeSlot::Accent1));
        assert_eq!(undo_len(&s), n + 1);

        s.execute("design.colors", &json!({"colors": {"accent1": "112233"}})).unwrap();
        assert_eq!(scheme(&s).name, "Custom");
        assert_eq!(scheme(&s).get(SchemeSlot::Accent1).hex(), "112233");
        assert_eq!(scheme(&s).get(SchemeSlot::Accent2), harbor.get(SchemeSlot::Accent2));
        assert_eq!(undo_len(&s), n + 2);

        let meadow = builtin_color_schemes().into_iter().find(|c| c.name == "Meadow").unwrap();
        s.execute("design.colors", &json!({"name": "Meadow", "colors": {"lt1": "#010203"}})).unwrap();
        assert_eq!(scheme(&s).name, "Meadow");
        assert_eq!(scheme(&s).get(SchemeSlot::Lt1).hex(), "010203");
        assert_eq!(scheme(&s).get(SchemeSlot::Accent1), meadow.get(SchemeSlot::Accent1));
        assert_eq!(undo_len(&s), n + 3);

        s.execute("design.colors", &json!({"name": "Talk", "colors": {"dk1": "#ABCDEF"}})).unwrap();
        assert_eq!(scheme(&s).name, "Talk");
        assert_eq!(scheme(&s).get(SchemeSlot::Dk1).hex(), "ABCDEF");
        assert_eq!(scheme(&s).get(SchemeSlot::Lt1).hex(), "010203");
        assert_eq!(scheme(&s).get(SchemeSlot::Accent1), meadow.get(SchemeSlot::Accent1));

        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(scheme(&s).name, "Meadow");
        assert_eq!(scheme(&s).get(SchemeSlot::Dk1), meadow.get(SchemeSlot::Dk1));
    }

    #[test]
    fn fonts_dialog_apply_and_reject() {
        let mut s = Session::with_new();
        let original = font_scheme(&s);
        let n = undo_len(&s);
        s.execute("design.fonts", &json!({})).unwrap();
        s.execute("design.fonts", &Value::Null).unwrap();
        s.execute("design.fonts", &json!([1, 2])).unwrap();
        assert_eq!(font_scheme(&s), original);
        assert_eq!(undo_len(&s), n, "opening the dialog does not edit");
        assert_eq!(s.ui_requests.len(), 3);
        match &s.ui_requests[0] {
            UiRequest::Dialog { id, params } => {
                assert_eq!(id, "themeFonts");
                assert_eq!(params["name"], original.name);
                assert_eq!(params["major"], original.major.latin);
                assert_eq!(params["minor"], original.minor.latin);
            }
            other => panic!("expected a dialog, got {other:?}"),
        }

        s.execute("design.fonts", &json!({"name": "classic"})).unwrap();
        let classic = builtin_font_schemes().into_iter().find(|f| f.name == "Classic").unwrap();
        assert_eq!(font_scheme(&s), classic, "a builtin name keeps that scheme, including its own name");
        assert_eq!(undo_len(&s), n + 1);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Fonts");
        s.execute("design.fonts", &json!({"name": "Classic"})).unwrap();
        assert_eq!(undo_len(&s), n + 1, "the same scheme is not an undo step");

        let err = s.execute("design.fonts", &json!({"name": "Nope"})).unwrap_err().to_string();
        assert!(err.contains("no font scheme `Nope`"), "{err}");
        assert!(s.execute("design.fonts", &json!({"name": "  "})).is_err());
        assert!(s.execute("design.fonts", &json!({"name": 1})).is_err());
        assert!(s.execute("design.fonts", &json!({"major": ""})).is_err());
        assert!(s.execute("design.fonts", &json!({"major": "  "})).is_err());
        assert!(s.execute("design.fonts", &json!({"minor": 1})).is_err());
        assert!(s.execute("design.fonts", &json!({"name": "Classic", "major": ""})).is_err());
        assert_eq!(font_scheme(&s), classic);
        assert_eq!(undo_len(&s), n + 1);

        s.edit(|doc, _| {
            let fonts = &mut std::sync::Arc::make_mut(&mut doc.masters[0]).theme.fonts;
            fonts.major.ea = "EA Major".into();
            fonts.minor.cs = "CS Minor".into();
            fonts.major.scripts.push(ScriptFont { script: "Arab".into(), typeface: "Noto Naskh Arabic".into() });
            Ok(())
        })
        .unwrap();
        let marked = font_scheme(&s);
        let n = undo_len(&s);

        s.execute("design.fonts", &json!({"major": " Lato "})).unwrap();
        let custom = font_scheme(&s);
        assert_eq!(custom.name, "Custom");
        assert_eq!(custom.major.latin, "Lato");
        assert_eq!(custom.minor.latin, "Lato", "an omitted minor copies major");
        assert_eq!(custom.major.ea, "EA Major");
        assert_eq!(custom.minor.cs, "CS Minor");
        assert_eq!(custom.major.scripts, marked.major.scripts);
        assert_eq!(undo_len(&s), n + 1);

        s.execute("design.fonts", &json!({"minor": "Roboto"})).unwrap();
        assert_eq!(font_scheme(&s).name, "Custom");
        assert_eq!(font_scheme(&s).major.latin, "Lato");
        assert_eq!(font_scheme(&s).minor.latin, "Roboto");
        assert_eq!(font_scheme(&s).major.ea, "EA Major");
        assert_eq!(undo_len(&s), n + 2);

        s.execute("design.fonts", &json!({"name": "Classic", "major": "Inter"})).unwrap();
        assert_eq!(font_scheme(&s).name, "Classic");
        assert_eq!(font_scheme(&s).major.latin, "Inter");
        assert_eq!(font_scheme(&s).minor.latin, "Open Sans");
        assert_eq!(font_scheme(&s).major.ea, "");
        assert!(font_scheme(&s).major.scripts.is_empty(), "a builtin is applied before the latin overlay");
        assert_eq!(undo_len(&s), n + 3);

        s.edit(|doc, _| {
            std::sync::Arc::make_mut(&mut doc.masters[0]).theme.fonts.major.ea = "Kept".into();
            Ok(())
        })
        .unwrap();
        s.execute("design.fonts", &json!({"name": "Talk", "minor": "Nunito Sans"})).unwrap();
        assert_eq!(font_scheme(&s).name, "Talk");
        assert_eq!(font_scheme(&s).major.latin, "Inter");
        assert_eq!(font_scheme(&s).minor.latin, "Nunito Sans");
        assert_eq!(font_scheme(&s).major.ea, "Kept");
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Fonts");

        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(font_scheme(&s).name, "Classic");
        assert_eq!(font_scheme(&s).major.ea, "Kept");
        assert_eq!(font_scheme(&s).minor.latin, "Open Sans");
    }

    #[test]
    fn background_picture_options_tile_transparency_and_copy() {
        let mut s = Session::with_new();
        let n = undo_len(&s);
        let err = s.execute("design.background", &json!({"pictureProps": {"tile": true}})).unwrap_err().to_string();
        assert!(err.contains("picture background"), "{err}");
        assert!(s.execute("design.background", &json!({"transparency": 0.2})).is_err());
        assert_eq!(undo_len(&s), n);

        s.execute("design.background", &json!({"color": "accent1", "transparency": 0.25})).unwrap();
        match &s.doc().unwrap().doc.slides[0].background {
            Some(Background::Fill { fill: Fill::Solid { color } }) => assert!((color.alpha() - 0.75).abs() < 1e-4, "{}", color.alpha()),
            other => panic!("expected a solid background, got {other:?}"),
        }
        let n = undo_len(&s);
        s.execute("design.background", &json!({"transparency": 0.25})).unwrap();
        assert_eq!(undo_len(&s), n, "the same transparency is not an undo step");

        s.execute("design.background", &json!({"picture": png(), "name": "dot.png", "transparency": 0.4, "pictureProps": {"tile": true}})).unwrap();
        let pf = picture(&s);
        assert!((pf.alpha.unwrap_or(1.0) - 0.6).abs() < 1e-6);
        assert!(matches!(pf.mode, PictureMode::Tile { .. }));
        let n = undo_len(&s);

        s.execute(
            "design.background",
            &json!({"pictureProps": {"offsetX": 12.0, "offsetY": -3.0, "scaleX": 0.5, "scaleY": 50.0, "align": "ctr", "mirror": "xy", "rotateWithShape": false}}),
        )
        .unwrap();
        match picture(&s).mode {
            PictureMode::Tile { tx, ty, sx, sy, flip, align } => {
                assert_eq!((tx, ty, sx, sy), (12.0, -3.0, 0.5, 10.0));
                assert_eq!(flip, "xy");
                assert_eq!(align, "ctr");
            }
            other => panic!("expected a tile, got {other:?}"),
        }
        assert!(picture(&s).upright);
        s.execute("design.background", &json!({"pictureProps": {"align": "bogus", "offsetX": "nope"}})).unwrap();
        match picture(&s).mode {
            PictureMode::Tile { tx, align, .. } => {
                assert_eq!(tx, 12.0);
                assert_eq!(align, "ctr");
            }
            other => panic!("expected the tile to stay, got {other:?}"),
        }
        assert_eq!(undo_len(&s), n + 1, "ignored options are not an undo step");

        s.execute("design.background", &json!({"pictureProps": {"tile": false, "stretch": [0.1, 0.2, 0.0, 0.0]}})).unwrap();
        match picture(&s).mode {
            PictureMode::Stretch { fill_rect } => assert_eq!(fill_rect, [0.1, 0.2, 0.0, 0.0]),
            other => panic!("expected a stretch, got {other:?}"),
        }

        s.execute("slide.new", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.slides[1].background.is_none());
        s.execute("slide.go", &json!({"index": 0})).unwrap();
        let n = undo_len(&s);
        s.execute("design.background", &json!({"copy": true})).unwrap();
        assert_eq!(s.doc().unwrap().doc.slides[1].background, s.doc().unwrap().doc.slides[0].background);
        assert_eq!(undo_len(&s), n + 1);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Format Background");
        s.execute("design.background", &json!({"copy": true})).unwrap();
        assert_eq!(undo_len(&s), n + 1, "copying an already shared background is not an undo step");
    }
}

#[cfg(test)]
mod browse_theme_tests {
    use deckcraft_model::PhType;
    use deckcraft_model::text::RunKind;
    use serde_json::json;

    use crate::{Session, Target};

    #[test]
    fn applies_the_theme_of_another_deck_and_undoes() {
        let mut other = Session::with_new();
        other.execute("design.fonts", &json!({"major": "Lato", "minor": "Source Sans 3"})).unwrap();
        other.execute("design.colors", &json!({"colors": {"accent1": "#123456"}})).unwrap();
        let data = other.execute("file.saveBytes", &json!({"format": "pptx"})).unwrap()["data"].as_str().unwrap().to_string();
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "T", "body": "B"})).unwrap();
        let before = s.doc().unwrap().doc.masters[0].theme.clone();
        s.execute("design.browseTheme", &json!({"name": "Brand.pptx", "data": data})).unwrap();
        let t = &s.doc().unwrap().doc.masters[0].theme;
        assert_eq!(t.fonts.major.latin, "Lato");
        assert_eq!(t.fonts.minor.latin, "Source Sans 3");
        assert_eq!(t.colors.get(deckcraft_color::SchemeSlot::Accent1).hex().to_lowercase(), "123456");
        assert!(s.doc().unwrap().doc.validate().is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.masters[0].theme, before);
        assert!(s.execute("design.browseTheme", &json!({"data": "AAAA"})).is_err());
        assert!(s.execute("design.browseTheme", &json!({})).is_err());
    }

    fn undo_len(s: &crate::Session) -> usize {
        s.doc().unwrap().history.undo.len()
    }

    #[test]
    fn effect_scheme_replaces_format_and_keeps_colours_and_fonts() {
        let mut s = crate::Session::with_new();
        let listed = s.execute("design.themes", &json!({})).unwrap();
        let names: Vec<String> = listed["effectSchemes"].as_array().unwrap().iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
        assert_eq!(names, ["DeckCraft", "Soft", "Crisp", "Lift", "Glow", "Plain"]);

        let n = undo_len(&s);
        s.execute("design.effects", &json!({"name": "DeckCraft"})).unwrap();
        assert_eq!(undo_len(&s), n, "the scheme already on the theme is not an undo step");
        let err = s.execute("design.effects", &json!({"name": "Office"})).unwrap_err().to_string();
        assert!(err.contains("no effect scheme"), "{err}");
        assert!(s.execute("design.effects", &json!({})).is_err());
        assert_eq!(undo_len(&s), n);

        s.execute("design.theme", &json!({"name": "Harbor"})).unwrap();
        let before = s.doc().unwrap().doc.masters[0].theme.clone();
        s.execute("design.effects", &json!({"name": "soft"})).unwrap();
        let theme = s.doc().unwrap().doc.masters[0].theme.clone();
        assert_eq!(theme.name, "Harbor");
        assert_eq!(theme.colors, before.colors);
        assert_eq!(theme.fonts, before.fonts);
        assert_eq!(theme.format.name, "Soft");
        assert!(theme.format.effects[0].outer_shadow.is_some());
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Effects");

        let n = undo_len(&s);
        s.execute("design.effects", &json!({"name": "Soft"})).unwrap();
        assert_eq!(undo_len(&s), n, "the same scheme is not an undo step");

        s.execute("design.effects", &json!({"name": "Glow"})).unwrap();
        let glow = s.doc().unwrap().doc.masters[0].theme.format.effects[2].glow.clone();
        assert!(glow.is_some_and(|g| (g.radius - 12.0).abs() < 1e-6));
        s.execute("design.effects", &json!({"name": "Plain"})).unwrap();
        let plain = s.doc().unwrap().doc.masters[0].theme.format.clone();
        assert!(plain.effects.iter().all(|e| e.outer_shadow.is_none() && e.glow.is_none()));
        assert_eq!(plain.lines[2].width, Some(0.75));
    }

    fn shapes(s: &Session) -> Vec<deckcraft_model::Shape> {
        s.doc().unwrap().shapes().to_vec()
    }

    fn has_title(s: &Session) -> bool {
        shapes(s).iter().any(|sh| sh.ph_type().is_some_and(PhType::is_title))
    }

    fn has_footers(s: &Session) -> bool {
        shapes(s).iter().any(|sh| sh.ph_type().is_some_and(PhType::is_footer_kind))
    }

    #[test]
    fn preserve_and_placeholders_edit_the_open_master_or_layout() {
        for id in ["master.preserve", "master.placeholders"] {
            assert!(crate::command_specs().iter().any(|c| c.id == id), "{id}");
        }

        let mut s = Session::with_new();
        let blank = s.doc().unwrap().doc.masters[0].layouts.iter().position(|l| l.name == "Blank").unwrap();

        s.execute("view.slideMaster", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().selection.target, Target::Layout { master: 0, layout: 0 });
        s.execute("view.slideMaster", &json!({"master": 0})).unwrap();
        assert_eq!(s.doc().unwrap().selection.target, Target::Master { master: 0 });
        assert!(!s.doc().unwrap().doc.masters[0].preserve);

        let n = s.doc().unwrap().history.undo.len();
        s.execute("master.preserve", &json!({"preserve": false})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), n, "the same value is not an undo step");
        s.execute("master.preserve", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.masters[0].preserve);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Preserve");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(!s.doc().unwrap().doc.masters[0].preserve);

        s.execute("master.placeholders", &json!({"footers": false})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Layout Placeholders");
        assert!(has_title(&s));
        assert!(!has_footers(&s));
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(has_footers(&s));

        s.execute("view.closeMaster", &json!({})).unwrap();
        let n = s.doc().unwrap().history.undo.len();
        let err = s.execute("master.preserve", &json!({})).unwrap_err();
        assert!(err.to_string().contains("open Slide Master view"), "{err}");
        let err = s.execute("master.placeholders", &json!({"title": true})).unwrap_err();
        assert!(err.to_string().contains("open Slide Master view"), "{err}");
        let err = s.execute("master.placeholders", &json!({})).unwrap_err();
        assert!(err.to_string().contains("give title or footers"), "{err}");
        assert_eq!(s.doc().unwrap().history.undo.len(), n);

        s.execute("view.slideMaster", &json!({"master": 0, "layout": blank})).unwrap();
        assert_eq!(s.doc().unwrap().selection.target, Target::Layout { master: 0, layout: blank });
        assert!(s.doc().unwrap().doc.masters[0].layouts[blank].preserve);
        assert!(!has_title(&s));
        assert!(has_footers(&s));

        let n = s.doc().unwrap().history.undo.len();
        s.execute("master.placeholders", &json!({"title": false, "footers": true})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), n, "already in that state");

        s.execute("master.placeholders", &json!({"title": true, "footers": false})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), n + 1, "both keys are one undo step");
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Layout Placeholders");
        assert!(has_title(&s));
        assert!(!has_footers(&s));
        let title = shapes(&s).into_iter().find(|sh| sh.ph_type().is_some_and(PhType::is_title)).unwrap();
        assert_eq!(title.name, "Title 1");
        assert_eq!(title.ph.unwrap().idx, 0);
        assert!(title.text.unwrap().paragraphs.iter().all(|para| para.runs.is_empty()));
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(!has_title(&s));
        assert!(has_footers(&s));

        s.execute("master.placeholders", &json!({"footers": false})).unwrap();
        s.execute("master.placeholders", &json!({"footers": true})).unwrap();
        let restored = shapes(&s);
        assert!(restored.iter().any(|sh| sh.ph_type() == Some(PhType::Date)));
        assert!(restored.iter().any(|sh| sh.ph_type() == Some(PhType::Footer)));
        let num = restored.iter().find(|sh| sh.ph_type() == Some(PhType::SlideNum)).unwrap();
        let run = &num.text.as_ref().unwrap().paragraphs[0].runs[0];
        assert_eq!(run.text, "‹#›");
        assert!(matches!(&run.kind, RunKind::Field { field } if field == "slidenum"));

        assert!(s.doc().unwrap().doc.masters[0].layouts[blank].preserve);
        s.execute("master.preserve", &json!({"preserve": false})).unwrap();
        assert!(!s.doc().unwrap().doc.masters[0].layouts[blank].preserve);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Preserve");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.masters[0].layouts[blank].preserve);

        s.edit(|doc, _| {
            let m = std::sync::Arc::make_mut(doc.masters.get_mut(0).unwrap());
            m.shapes.retain(|sh| !sh.ph_type().is_some_and(PhType::is_title));
            Ok(())
        })
        .unwrap();
        s.execute("master.placeholders", &json!({"title": true})).unwrap();
        let title = shapes(&s).into_iter().find(|sh| sh.ph_type() == Some(PhType::Title)).unwrap();
        assert_eq!(title.name, "Title");
        let box_ = title.xfrm.unwrap();
        assert!((box_.x - 60.0).abs() < 0.01 && (box_.y - 29.0).abs() < 0.01);
        assert!((box_.w - 840.0).abs() < 0.01 && (box_.h - 104.0).abs() < 0.01);
    }
}
