//! Design tab and masters: themes, colours, fonts, backgrounds, slide size, header & footer,
//! Slide Master view.

use std::collections::BTreeSet;
use std::sync::Arc;

use deckcraft_geom::Size;
use deckcraft_model::text::{Run, RunKind, TextBody};
use deckcraft_model::theme::{builtin_color_schemes, builtin_font_schemes, builtin_format_schemes, builtin_themes};
use deckcraft_model::{Background, Fill, PhType, Presentation, Shape, ShapeId, defaults};
use serde_json::{Value, json};

use super::{
    CommandSpec, NO_ID_SPACE, always, bad, bool_or, bool_param, cmd, color_param, color_value, f64_param, has_doc, has_slide, ok, opt_usize_param,
    rect_param, str_param, usize_param, with_param,
};
use crate::{Result, Session, Target};

mod colors;
mod master_view;
mod slide_background;
mod slide_setup;

use colors::{colors, effects, fonts, variant, variants};
use master_view::{
    close_master, delete_layout, handout_placeholders, handout_view, in_handout, in_notes_master, insert_layout, insert_placeholder, master_view,
    notes_placeholders, notes_view, placeholders, preserve, rename_layout,
};
pub use master_view::{handout_placeholder, notes_placeholder};
use slide_background::{background, hide_bg_graphics};
pub use slide_setup::{civil, date_field_text, today};
use slide_setup::{header_footer, scale_shapes, slide_size};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("design.theme", "Themes", ["Design", "Themes"], None, "{name} (see design.themes)", has_doc, theme),
        cmd!(
            "design.browseTheme",
            "Browse for Themes…",
            ["Design", "Themes"],
            None,
            "{path} | {name, data: base64} — apply a .pptx or .potx's slide master and layouts (slides move to the matching layouts), or a .thmx's theme",
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
    // A presentation or template brings its slide master and layouts, as in PowerPoint; a .thmx
    // (its main part is a theme, not a presentation) brings the theme alone.
    if let Ok(src) = super::file::open_presentation("theme.pptx", &bytes)
        && src.masters.first().is_some_and(|m| !m.layouts.is_empty())
    {
        return apply_master(s, &src);
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

/// Design › Browse for Themes… with a `.pptx` / `.potx`: its first master and layouts replace the
/// master the current slide uses (a preserved master stays beside it), stretched to this deck's
/// slide size. Each slide on it moves to the new layout with the same name, else the same kind,
/// keeping its content as Layout does.
fn apply_master(s: &mut Session, src: &Presentation) -> Result<Value> {
    let from = src.masters.first().ok_or_else(|| bad("design.browseTheme", "no master"))?;
    let theme = from.theme.name.clone();
    s.edit(|doc, sel| {
        let mi = master_index(doc, sel);
        let old = doc.masters.get(mi).cloned().ok_or_else(|| bad("design.browseTheme", "no master"))?;
        let (mut media, mut layouts) = Default::default();
        let mut m = super::reuse::import_master("design.browseTheme", doc, src, from, &mut media, &mut layouts)?;
        let (sx, sy) = (doc.slide_size.width / src.slide_size.width.max(1.0), doc.slide_size.height / src.slide_size.height.max(1.0));
        if (sx - 1.0).abs() > 1e-9 || (sy - 1.0).abs() > 1e-9 {
            scale_shapes(&mut m.shapes, sx, sy, 1.0, 0.0, 0.0);
            for l in &mut m.layouts {
                scale_shapes(&mut l.shapes, sx, sy, 1.0, 0.0, 0.0);
            }
        }
        let moves: Vec<(usize, deckcraft_model::LayoutId)> =
            doc.slides.iter().enumerate().filter_map(|(i, sl)| old.layout(sl.layout).and_then(|l| matching_layout(&m, l)).map(|l| (i, l))).collect();
        let layouts = m.layouts.len();
        if old.preserve {
            doc.masters.insert(mi, Arc::new(m));
        } else if let Some(slot) = doc.masters.get_mut(mi) {
            *slot = Arc::new(m);
        }
        for (i, l) in &moves {
            super::slide::apply_layout(doc, *i, *l)?;
        }
        if let Target::Layout { master, .. } = sel.target {
            sel.target = Target::Master { master };
        }
        sel.shapes.clear();
        sel.text = None;
        Ok(json!({"theme": theme, "layouts": layouts, "slides": moves.len()}))
    })
}

/// The layout of `m` a slide on `from` moves to: same name, else same kind (not for custom
/// layouts), else Title and Content, else the first.
fn matching_layout(m: &deckcraft_model::Master, from: &deckcraft_model::Layout) -> Option<deckcraft_model::LayoutId> {
    use deckcraft_model::LayoutType;
    let ls = &m.layouts;
    ls.iter()
        .find(|l| !from.name.is_empty() && l.name.trim().eq_ignore_ascii_case(from.name.trim()))
        .or_else(|| ls.iter().find(|l| from.kind != LayoutType::Custom && l.kind == from.kind))
        .or_else(|| ls.iter().find(|l| l.kind == LayoutType::TitleAndContent))
        .or_else(|| ls.first())
        .map(|l| l.id)
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

#[cfg(test)]
mod tests;

#[cfg(test)]
mod browse_theme_tests;
