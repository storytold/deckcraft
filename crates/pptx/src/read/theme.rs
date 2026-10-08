//! Theme parts (`a:theme`).

use deckcraft_color::{ColorScheme, Rgba, SchemeSlot};
use deckcraft_model::style::{ColorBase, Effects};
use deckcraft_model::theme::{FontScheme, FontSet, FormatScheme, ScriptFont, Theme, default_format};

use super::{Imp, Part, dml};
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
        let mut scheme = ColorScheme { name: cs.attr("name").unwrap_or("").to_string(), colors: t.colors.colors };
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

fn pad<T: Clone>(v: &mut Vec<T>, d: &[T]) {
    while v.len() < 3 {
        match d.get(v.len()) {
            Some(x) => v.push(x.clone()),
            None => break,
        }
    }
}
