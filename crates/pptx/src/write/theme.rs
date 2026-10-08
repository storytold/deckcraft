//! Theme parts.

use deckcraft_color::SchemeSlot;
use deckcraft_model::style::{ColorRef, Effects, Fill};
use deckcraft_model::theme::{FontSet, Theme, default_format};

use super::{Exp, Out, dml};
use crate::opc::{NS_A, NS_R};
use crate::xml::{A, W};

fn font_set(w: &mut W, tag: &str, f: &FontSet) {
    w.open0(tag);
    w.empty("a:latin", A::new().a("typeface", if f.latin.is_empty() { "Arial" } else { &f.latin }));
    w.empty("a:ea", A::new().a("typeface", &f.ea));
    w.empty("a:cs", A::new().a("typeface", &f.cs));
    for sf in &f.scripts {
        w.empty("a:font", A::new().a("script", &sf.script).a("typeface", &sf.typeface));
    }
    w.close(tag);
}

fn style_fill(f: &Fill) -> Fill {
    match f {
        Fill::Background | Fill::Group => Fill::solid(ColorRef::scheme(SchemeSlot::PhClr)),
        f => f.clone(),
    }
}

pub fn theme_xml(x: &mut Exp, o: &mut Out, t: &Theme) -> Vec<u8> {
    let mut w = W::new();
    w.open("a:theme", A::new().a("xmlns:a", NS_A).a("xmlns:r", NS_R).a("name", if t.name.is_empty() { "Theme" } else { &t.name }));
    w.open0("a:themeElements");
    w.open("a:clrScheme", A::new().a("name", if t.colors.name.is_empty() { &t.name } else { &t.colors.name }));
    for slot in SchemeSlot::THEME {
        let tag = format!("a:{}", slot.xml_name());
        w.open0(&tag);
        w.val("a:srgbClr", t.colors.get(slot).hex());
        w.close(&tag);
    }
    w.close("a:clrScheme");
    w.open("a:fontScheme", A::new().a("name", if t.fonts.name.is_empty() { &t.name } else { &t.fonts.name }));
    font_set(&mut w, "a:majorFont", &t.fonts.major);
    font_set(&mut w, "a:minorFont", &t.fonts.minor);
    w.close("a:fontScheme");
    let d = default_format();
    let f = &t.format;
    w.open("a:fmtScheme", A::new().a("name", if f.name.is_empty() { &t.name } else { &f.name }));
    let three = |v: &[Fill], d: &[Fill]| -> Vec<Fill> {
        let mut out: Vec<Fill> = v.iter().map(style_fill).collect();
        while out.len() < 3 {
            out.push(d.get(out.len()).cloned().unwrap_or(Fill::solid(ColorRef::scheme(SchemeSlot::PhClr))));
        }
        out
    };
    w.open0("a:fillStyleLst");
    for fl in three(&f.fills, &d.fills) {
        dml::fill(&mut w, x, o, &fl);
    }
    w.close("a:fillStyleLst");
    w.open0("a:lnStyleLst");
    let mut lines = f.lines.clone();
    while lines.len() < 3 {
        lines.push(d.lines.get(lines.len()).cloned().unwrap_or_default());
    }
    for l in &lines {
        let mut l = l.clone();
        if l.fill.is_none() {
            l.fill = Some(Fill::solid(ColorRef::scheme(SchemeSlot::PhClr)));
        }
        if let Some(fl) = &l.fill {
            l.fill = Some(style_fill(fl));
        }
        dml::line(&mut w, x, o, "a:ln", &l);
    }
    w.close("a:lnStyleLst");
    w.open0("a:effectStyleLst");
    let mut fx = f.effects.clone();
    while fx.len() < 3 {
        fx.push(Effects::default());
    }
    for e in &fx {
        w.open0("a:effectStyle");
        dml::effects(&mut w, e);
        if let Some(raw) = &e.raw3d {
            w.raw(raw);
        }
        w.close("a:effectStyle");
    }
    w.close("a:effectStyleLst");
    w.open0("a:bgFillStyleLst");
    for fl in three(&f.bg_fills, &d.bg_fills) {
        dml::fill(&mut w, x, o, &fl);
    }
    w.close("a:bgFillStyleLst");
    w.close("a:fmtScheme");
    w.close("a:themeElements");
    match &t.raw_extra {
        Some(raw) if raw.contains("objectDefaults") => w.raw(raw),
        Some(raw) => {
            w.empty0("a:objectDefaults");
            w.raw(raw);
        }
        None => {
            w.empty0("a:objectDefaults");
            w.empty0("a:extraClrSchemeLst");
        }
    }
    w.close("a:theme");
    w.finish()
}
