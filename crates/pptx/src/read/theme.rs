//! Theme parts (`a:theme`).

use deckcraft_color::{ColorScheme, ExtraColor, MAX_TINT_ROWS, Rgba, SchemeSlot};
use deckcraft_model::style::{ColorBase, Effects};
use deckcraft_model::theme::{FontScheme, FontSet, FormatScheme, ScriptFont, Theme, default_format};

use super::{Imp, Part, dml};
use crate::opc::EXT_DC_COLORS;
use crate::xml::El;

fn font_set(e: Option<&El>) -> FontSet {
    let tf = |n: &str| e.and_then(|e| e.child(n)).and_then(|c| c.attr("typeface")).unwrap_or("").to_string();
    let scripts = e
        .map(|e| {
            e.children_named("font")
                .filter_map(|f| Some(ScriptFont { script: f.attr("script")?.to_string(), typeface: f.attr("typeface").unwrap_or("").to_string() }))
                .take(256)
                .collect()
        })
        .unwrap_or_default();
    FontSet { latin: tf("latin"), ea: tf("ea"), cs: tf("cs"), scripts }
}

pub fn read_theme(imp: &mut Imp, part: &Part, root: &El) -> Theme {
    let mut t = Theme { name: root.attr("name").unwrap_or("Theme").to_string(), ..Default::default() };
    let Some(te) = root.child("themeElements") else { return t };
    if let Some(cs) = te.child("clrScheme") {
        let mut scheme = ColorScheme::new(cs.attr("name").unwrap_or(""), t.colors.colors);
        for slot in SchemeSlot::THEME {
            if let Some(c) = cs.child(slot.xml_name()).and_then(dml::color) {
                let rgb = match c.base {
                    ColorBase::Rgb { rgb } => rgb,
                    ColorBase::System { last, .. } => last,
                    ColorBase::Preset { name } => deckcraft_color::preset(&name).unwrap_or(Rgba::BLACK),
                    ColorBase::Scheme { .. } => continue,
                };
                scheme.set(slot, rgb);
            }
        }
        extra_colors(cs, &mut scheme);
        t.colors = scheme;
    }
    if let Some(fs) = te.child("fontScheme") {
        t.fonts = FontScheme {
            name: fs.attr("name").unwrap_or("").to_string(),
            major: font_set(fs.child("majorFont")),
            minor: font_set(fs.child("minorFont")),
        };
    }
    if let Some(fm) = te.child("fmtScheme") {
        let dflt = default_format();
        let mut f = FormatScheme { name: fm.attr("name").unwrap_or("").to_string(), ..Default::default() };
        if let Some(l) = fm.child("fillStyleLst") {
            f.fills = l.elements().take(16).filter_map(|e| dml::fill_el(imp, part, e)).collect();
        }
        if let Some(l) = fm.child("lnStyleLst") {
            f.lines = l.children_named("ln").take(16).map(|e| dml::line(imp, part, e)).collect();
        }
        if let Some(l) = fm.child("effectStyleLst") {
            f.effects = l
                .children_named("effectStyle")
                .take(16)
                .map(|e| {
                    let mut fx = e.child("effectLst").map(dml::effects).unwrap_or_default();
                    if let Some(b) = e.path(&["sp3d", "bevelT"]) {
                        fx.bevel = Some(b.attr("prst").unwrap_or("circle").to_string());
                    }
                    let raw: String = e.elements().filter(|c| c.is("scene3d") || c.is("sp3d")).map(|c| part.keep(c)).collect();
                    if !raw.is_empty() {
                        fx.raw3d = Some(raw);
                    }
                    fx
                })
                .collect();
        }
        if let Some(l) = fm.child("bgFillStyleLst") {
            f.bg_fills = l.elements().take(16).filter_map(|e| dml::fill_el(imp, part, e)).collect();
        }
        // The style matrix needs three entries of each kind.
        pad(&mut f.fills, &dflt.fills);
        pad(&mut f.lines, &dflt.lines);
        pad(&mut f.bg_fills, &dflt.bg_fills);
        while f.effects.len() < 3 {
            f.effects.push(Effects::default());
        }
        t.format = f;
    }
    let extra: String = root
        .elements()
        .filter(|e| e.is("objectDefaults") || e.is("extraClrSchemeLst"))
        .filter(|e| !super::has_rel_refs(e))
        .map(|e| part.keep(e))
        .collect();
    if !extra.is_empty() {
        t.raw_extra = Some(extra);
    }
    t
}

/// DeckCraft's extra named colours and tint rows (see `write::theme::extra_colors`); a scheme
/// saved by another application has none.
fn extra_colors(cs: &El, scheme: &mut ColorScheme) {
    let Some(x) =
        cs.child("extLst").and_then(|l| l.children_named("ext").find(|e| e.attr("uri") == Some(EXT_DC_COLORS))).and_then(|e| e.child("extraColors"))
    else {
        return;
    };
    let clrs = |e: &El| -> Vec<Rgba> { e.children_named("clr").filter_map(|c| c.attr("val").and_then(Rgba::from_hex)).take(MAX_TINT_ROWS).collect() };
    scheme.computed_tints = x.attr("computed") != Some("0");
    for t in x.children_named("tints").take(SchemeSlot::THEME.len()) {
        if let Some(slot) = t.attr("slot").and_then(SchemeSlot::from_xml)
            && SchemeSlot::THEME.contains(&slot)
        {
            scheme.tints.insert(slot, clrs(t));
        }
    }
    for c in x.children_named("color").take(deckcraft_color::MAX_EXTRA_COLORS) {
        let (Some(name), Some(color)) = (c.attr("name"), c.attr("val").and_then(Rgba::from_hex)) else { continue };
        if name.trim().is_empty() {
            continue;
        }
        scheme.extra.push(ExtraColor { name: name.to_string(), color, tints: c.child("tints").map(clrs) });
    }
}

fn pad<T: Clone>(v: &mut Vec<T>, d: &[T]) {
    while v.len() < 3 {
        match d.get(v.len()) {
            Some(x) => v.push(x.clone()),
            None => break,
        }
    }
}
