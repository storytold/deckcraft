//! Theme parts.

use deckcraft_color::{ColorScheme, SchemeSlot};
use deckcraft_model::style::{ColorRef, Effects, Fill};
use deckcraft_model::theme::{FontSet, Theme, default_format};

use super::{Exp, Out, dml};
use crate::opc::{EXT_DC_COLORS, NS_A, NS_DC_COLORS, NS_R};
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

/// The scheme's extra named colours, explicit tint rows and `computed_tints`, as a DeckCraft
/// `a:ext` after the twelve slots: `<dc:extraColors computed="0"?>` holding
/// `<dc:tints slot="accent1"><dc:clr val="RRGGBB"/>…</dc:tints>` per slot with a list (an empty
/// list is an empty element) and `<dc:color name="Sand" val="RRGGBB">(<dc:tints>…</dc:tints>
/// when it has a list)</dc:color>` per extra colour.
fn extra_colors(w: &mut W, c: &ColorScheme) {
    if c.extra.is_empty() && c.tints.is_empty() && c.computed_tints {
        return;
    }
    w.open0("a:extLst");
    w.open("a:ext", A::new().a("uri", EXT_DC_COLORS));
    w.open("dc:extraColors", A::new().a("xmlns:dc", NS_DC_COLORS).o("computed", (!c.computed_tints).then_some("0")));
    for (slot, tints) in &c.tints {
        rows(w, A::new().a("slot", slot.xml_name()), tints);
    }
    // What the reader takes back, no more.
    for e in c.extra.iter().take(deckcraft_color::MAX_EXTRA_COLORS) {
        w.open("dc:color", A::new().a("name", &e.name).a("val", e.color.hex()));
        if let Some(t) = &e.tints {
            rows(w, A::new(), t);
        }
        w.close("dc:color");
    }
    w.close("dc:extraColors");
    w.close("a:ext");
    w.close("a:extLst");
}

/// A tint list, cut to `MAX_TINT_ROWS` like the reader takes it (and the galleries show it).
fn rows(w: &mut W, a: A, tints: &[deckcraft_color::Rgba]) {
    w.open("dc:tints", a);
    for t in tints.iter().take(deckcraft_color::MAX_TINT_ROWS) {
        w.val("dc:clr", t.hex());
    }
    w.close("dc:tints");
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
    extra_colors(&mut w, &t.colors);
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
        } else if let Some(xml) = e.bevel.as_deref().and_then(dml::bevel_sp3d) {
            w.raw(&xml);
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
