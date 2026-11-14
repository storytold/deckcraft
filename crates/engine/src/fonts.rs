//! Fonts a presentation uses that this machine doesn't have, so opening it can say so (its text
//! draws in a stand-in; see `FontDb::document_face`).

use std::collections::HashSet;

use deckcraft_fonts::FontDb;
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::{Bullet, RunProps, TextBody};
use deckcraft_model::{Presentation, Shape, ShapeKind};

/// Load each embedded face into the font database, so text in that family draws with the file's
/// font on a machine that doesn't have it. A face already loaded (bundled or installed) stays;
/// the part bytes are kept either way. MicroType Express subsets have no TrueType or OpenType
/// font to load and are left for the round trip.
pub fn install_embedded_fonts(p: &Presentation) {
    let db = FontDb::global();
    // Faces can name one media item many times; each is copied and enumerated once.
    let mut seen = HashSet::new();
    for font in &p.embedded_fonts {
        for id in font.faces().into_iter().flatten() {
            if !seen.insert(id) {
                continue;
            }
            let Some(item) = p.media(id) else { continue };
            let Some(sfnt) = deckcraft_fonts::embedded_sfnt(&item.data) else { continue };
            db.add_font(sfnt.to_vec());
        }
    }
}

/// The font families the slides' text is drawn in (theme fonts resolved, bullet fonts included)
/// that aren't installed or bundled, sorted, without duplicates (case-insensitive). Names that
/// start with a dot are macOS hidden system fonts (`.SF NS`); they never appear in a font list
/// and can't be installed, so they aren't reported.
pub fn missing_fonts(p: &Presentation) -> Vec<String> {
    let db = FontDb::global();
    // used_fonts is already free of case-insensitive duplicates.
    let mut out: Vec<String> = used_fonts(p).into_iter().filter(|f| !f.starts_with('.') && !db.has_family(f)).collect();
    out.sort_by_key(|f| f.to_lowercase());
    out
}

/// The families the slides' text runs and bullets are drawn in.
pub fn used_fonts(p: &Presentation) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut add = |f: String| {
        let f = f.trim().to_string();
        if !f.is_empty() && seen.insert(f.to_ascii_lowercase()) {
            out.push(f);
        }
    };
    for slide in &p.slides {
        let Some(ctx) = Ctx::for_slide(p, slide) else { continue };
        deckcraft_model::walk(&slide.shapes, &mut |s, _| {
            if let Some(body) = &s.text {
                body_font_uses(&ctx, s, body, &mut |u| add(u.family));
            }
            if let ShapeKind::Table(t) = &s.kind {
                for cell in t.rows.iter().flat_map(|r| &r.cells) {
                    body_font_uses(&ctx, s, &cell.text, &mut |u| add(u.family));
                }
            }
        });
    }
    out
}

/// One use of a font family in a text body, as `font.used` counts them.
pub(crate) struct FontUse<'a> {
    /// The paragraph's index in the body.
    pub(crate) para: usize,
    /// The run's index in the paragraph and its properties, resolved through the style chain;
    /// `None` for the paragraph's character bullet.
    pub(crate) run: Option<(usize, &'a RunProps)>,
    /// The family it is set in, theme references resolved (untrimmed, possibly empty).
    pub(crate) family: String,
}

/// Calls `f` with each font use in `body`, text of `s`, in order. A paragraph counts when one of
/// its runs holds non-space text: each such run is a use of its Latin typeface, and a character
/// bullet that names a font is a use of that font. `font.used` and `font.used {detail: true}`
/// both count through this.
pub(crate) fn body_font_uses(ctx: &Ctx, s: &Shape, body: &TextBody, f: &mut dyn FnMut(FontUse<'_>)) {
    for (pi, para) in body.paragraphs.iter().enumerate() {
        if !para.runs.iter().any(|r| !r.text.trim().is_empty()) {
            continue;
        }
        for (ri, run) in para.runs.iter().enumerate() {
            if run.text.trim().is_empty() {
                continue;
            }
            let props = resolve::run(ctx, s, para, &run.props);
            let family = resolve::font_family(ctx, &props);
            f(FontUse { para: pi, run: Some((ri, &props)), family });
        }
        let pp = resolve::para(ctx, s, para);
        if matches!(pp.bullet, Some(Bullet::Char { .. }))
            && let Some(font) = pp.bullet_font.as_deref()
        {
            f(FontUse { para: pi, run: None, family: ctx.master.theme.font(font) });
        }
    }
}

/// Replace the font family `from` with `to` everywhere it is named explicitly: text runs (and
/// their east Asian, complex script and symbol faces), bullet fonts, paragraph end marks, list
/// styles of shapes, layouts and masters, the master text styles, the deck default style, speaker
/// notes, tables and the theme fonts. Names compare case-insensitively; theme references
/// (`+mn-lt`) are left alone. Returns how many names changed.
pub fn replace_font(p: &mut Presentation, from: &str, to: &str) -> usize {
    use std::sync::Arc;
    let from = from.trim();
    let to = to.trim();
    if from.is_empty() || to.is_empty() || from.starts_with('+') || from == to {
        return 0;
    }
    let mut n = 0usize;
    let mut fix = |f: &mut Option<String>| {
        if f.as_deref().is_some_and(|x| x.trim().eq_ignore_ascii_case(from)) {
            *f = Some(to.to_string());
            n = n.saturating_add(1);
        }
    };
    replace_list(&mut p.default_text_style, &mut fix);
    for slide in &mut p.slides {
        let sl = Arc::make_mut(slide);
        replace_shapes(&mut sl.shapes, &mut fix);
        replace_body(&mut sl.notes, &mut fix);
    }
    let mut theme_hits = 0usize;
    let masters = p.masters.iter_mut().chain(p.notes_master.iter_mut()).chain(p.handout_master.iter_mut());
    for m in masters {
        let m = Arc::make_mut(m);
        replace_shapes(&mut m.shapes, &mut fix);
        for l in [&mut m.title_style, &mut m.body_style, &mut m.other_style] {
            replace_list(l, &mut fix);
        }
        for lay in &mut m.layouts {
            replace_shapes(&mut lay.shapes, &mut fix);
        }
        for set in [&mut m.theme.fonts.major, &mut m.theme.fonts.minor] {
            for face in [&mut set.latin, &mut set.ea, &mut set.cs].into_iter().chain(set.scripts.iter_mut().map(|s| &mut s.typeface)) {
                if face.trim().eq_ignore_ascii_case(from) {
                    *face = to.to_string();
                    theme_hits = theme_hits.saturating_add(1);
                }
            }
        }
    }
    n.saturating_add(theme_hits)
}

fn replace_shapes(shapes: &mut [Shape], fix: &mut dyn FnMut(&mut Option<String>)) {
    for s in shapes {
        if let Some(body) = &mut s.text {
            replace_body(body, fix);
        }
        if let ShapeKind::Table(t) = &mut s.kind {
            for cell in t.rows.iter_mut().flat_map(|r| &mut r.cells) {
                replace_body(&mut cell.text, fix);
            }
        }
        if let Some(children) = s.children_mut() {
            replace_shapes(children, fix);
        }
    }
}

fn replace_body(body: &mut TextBody, fix: &mut dyn FnMut(&mut Option<String>)) {
    replace_list(&mut body.list_style, fix);
    for para in &mut body.paragraphs {
        fix(&mut para.props.bullet_font);
        replace_run(&mut para.end_props, fix);
        for run in &mut para.runs {
            replace_run(&mut run.props, fix);
        }
    }
}

fn replace_list(l: &mut deckcraft_model::text::ListStyle, fix: &mut dyn FnMut(&mut Option<String>)) {
    for lvl in l.levels.iter_mut().flatten() {
        fix(&mut lvl.para.bullet_font);
        replace_run(&mut lvl.run, fix);
    }
}

fn replace_run(r: &mut deckcraft_model::text::RunProps, fix: &mut dyn FnMut(&mut Option<String>)) {
    for f in [&mut r.font, &mut r.font_ea, &mut r.font_cs, &mut r.font_sym] {
        fix(f);
    }
}
