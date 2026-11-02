//! Font diagnostics use the same paragraph itemization and shaping as rendering. No text or
//! local font paths are included: callers get character ranges and the faces actually selected.

use super::shape::{Upright, shape_para};
use super::style::requested_families;
use super::{Arc, Ctx, FontDb, FontFace, Opts, Rect, Shape, TextBody, resolve, script};

/// Primary requested families for characters actually present, without shaping.
pub fn requested_fonts(ctx: &Ctx, shape: &Shape, body: &TextBody) -> Vec<String> {
    let mut out = Vec::new();
    for para in &body.paragraphs {
        for run in &para.runs {
            if run.text.trim().is_empty() {
                continue;
            }
            let props = resolve::run(ctx, shape, para, &run.props);
            let style = super::style::make_style(ctx, &props, 1.0, None);
            let chars: Vec<_> = run.text.chars().collect();
            let scripts = script::resolve(&chars);
            let mut seen = Vec::new();
            for (ch, (sc, slot)) in chars.iter().zip(scripts) {
                let symbol = ('\u{F000}'..='\u{F0FF}').contains(ch) && style.sym.is_some();
                if seen.contains(&(sc, slot, symbol)) {
                    continue;
                }
                seen.push((sc, slot, symbol));
                let family = if symbol { style.sym.clone() } else { requested_families(ctx, &style, slot, sc).into_iter().next() };
                if let Some(family) = family
                    && !out.iter().any(|f: &String| f.eq_ignore_ascii_case(&family))
                {
                    out.push(family);
                }
            }
        }
    }
    out
}

pub struct FontUse {
    pub paragraph: usize,
    pub run: usize,
    pub start: usize,
    pub end: usize,
    pub requested: Vec<String>,
    pub actual: Arc<FontFace>,
    pub reason: &'static str,
    pub missing_glyph: bool,
    pub synthetic_bold: bool,
    pub synthetic_italic: bool,
}

/// Inspect the saved text (fields retain their saved value), with offsets in Unicode characters.
/// Each segment has one requested cascade, source run and actual face. It includes explicit
/// East Asian/complex-script slots and theme script fonts, even when those fonts are absent.
pub fn font_usage(ctx: &Ctx, shape: &Shape, body: &TextBody) -> Vec<FontUse> {
    struct SavedFields;
    impl super::Fields for SavedFields {
        fn field(&self, _: &str) -> Option<String> {
            None
        }
    }
    let opts = Opts { rect: Rect::new(0.0, 0.0, 1000.0, 1000.0), fields: &SavedFields, prompt_color: None, no_shrink: true };
    let mut out: Vec<FontUse> = Vec::new();
    for paragraph in 0..body.paragraphs.len() {
        let p = shape_para(ctx, shape, body, paragraph, 1.0, &opts, None, Upright::None);
        let chars: Vec<_> = p.cells.iter().map(|c| c.ch).collect();
        let scripts = script::resolve(&chars);
        for (i, cell) in p.cells.iter().enumerate() {
            if cell.ch.is_control() {
                continue;
            }
            let (Some(st), Some(face)) = (p.styles.get(cell.style), p.faces.get(cell.face)) else { continue };
            let (sc, slot) = scripts.get(i).copied().unwrap_or((None, super::FontSlot::Latin));
            let mut requested = requested_families(ctx, st, slot, sc);
            if ('\u{F000}'..='\u{F0FF}').contains(&cell.ch)
                && let Some(sym) = &st.sym
            {
                requested.insert(0, sym.clone());
            }
            let missing_glyph = p.cells.get(cell.head).is_some_and(|c| c.glyphs.iter().any(|g| g.0 == 0));
            let matches = requested
                .first()
                .is_some_and(|r| face.family.eq_ignore_ascii_case(r) || face.legacy.as_ref().is_some_and(|(name, _)| name.eq_ignore_ascii_case(r)));
            let reason = if missing_glyph {
                "missing_glyph"
            } else if face.embedded {
                "embedded"
            } else if matches {
                "requested"
            } else if deckcraft_fonts::is_emoji_cluster(&[cell.ch]) && face.is_color() {
                "emoji"
            } else if requested.first().is_some_and(|r| !FontDb::global().has_family(r)) {
                "family_unavailable"
            } else {
                "glyph_fallback"
            };
            if let Some(last) = out.last_mut()
                && last.paragraph == paragraph
                && last.run == cell.style
                && last.end == i
                && Arc::ptr_eq(&last.actual, face)
                && last.requested == requested
                && last.reason == reason
                && last.missing_glyph == missing_glyph
            {
                last.end = i + 1;
                continue;
            }
            out.push(FontUse {
                paragraph,
                run: cell.style,
                start: i,
                end: i + 1,
                requested,
                actual: face.clone(),
                reason,
                missing_glyph,
                synthetic_bold: st.style_name.contains("Bold") && face.weight < 550.0,
                synthetic_italic: st.style_name.contains("Italic") && !face.italic,
            });
        }
    }
    out
}
