//! Fonts a presentation uses that this machine doesn't have, so opening it can say so (its text
//! draws in a stand-in; see `FontDb::document_face`).

use deckcraft_fonts::FontDb;
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::{Bullet, TextBody};
use deckcraft_model::{Presentation, Shape, ShapeKind};

/// Install supported embedded faces under private document-local families. Original names and
/// unsupported MicroType Express subsets remain unchanged for editing and round trip.
pub fn install_embedded_fonts(p: &mut Presentation) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_SCOPE: AtomicU64 = AtomicU64::new(1);
    let scope = NEXT_SCOPE.fetch_add(1, Ordering::Relaxed);
    let db = FontDb::global();
    let mut aliases: Vec<(String, String)> = Vec::new();
    let mut faces = Vec::new();
    for font in &p.embedded_fonts {
        let family = font.typeface.trim();
        if family.is_empty() {
            continue;
        }
        let alias = if let Some((_, alias)) = aliases.iter().find(|(source, _)| source.eq_ignore_ascii_case(family)) {
            alias.clone()
        } else {
            let alias = format!("deckcraft-document-{scope}-font-{}", aliases.len());
            aliases.push((family.to_owned(), alias.clone()));
            alias
        };
        for id in font.faces().into_iter().flatten() {
            let Some(item) = p.media(id) else { continue };
            let Some(sfnt) = deckcraft_fonts::embedded_sfnt(&item.data) else { continue };
            faces.push((sfnt.to_vec(), family.to_owned(), alias.clone()));
        }
    }
    db.add_document_fonts(faces);
    aliases.retain(|(_, alias)| db.has_family(alias));
    p.font_aliases = std::sync::Arc::new(aliases);
}

/// The font families the slides' text is drawn in (theme fonts resolved, bullet fonts included)
/// that aren't installed or bundled, sorted, without duplicates (case-insensitive).
pub fn missing_fonts(p: &Presentation) -> Vec<String> {
    let db = FontDb::global();
    let mut out: Vec<String> = Vec::new();
    for family in used_fonts(p) {
        if !db.has_family(p.render_font_family(&family)) && !out.iter().any(|f| f.eq_ignore_ascii_case(&family)) {
            out.push(family);
        }
    }
    out.sort_by_key(|f| f.to_lowercase());
    out
}

/// The families the slides' text runs and bullets are drawn in.
pub fn used_fonts(p: &Presentation) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |f: &str| {
        let f = f.trim();
        if !f.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(f)) {
            out.push(f.to_string());
        }
    };
    for slide in &p.slides {
        let Some(ctx) = Ctx::for_slide(p, slide) else { continue };
        deckcraft_model::walk(&slide.shapes, &mut |s, _| {
            if let Some(body) = &s.text {
                body_fonts(&ctx, s, body, false, None, &mut add);
            }
            if let ShapeKind::Table(t) = &s.kind {
                for cell in t.rows.iter().flat_map(|r| &r.cells) {
                    body_fonts(&ctx, s, &cell.text, false, None, &mut add);
                }
            }
        });
    }
    out
}

pub(crate) fn body_fonts(
    ctx: &Ctx,
    s: &Shape,
    body: &TextBody,
    include_slots: bool,
    range: Option<(deckcraft_model::edit::Pos, deckcraft_model::edit::Pos)>,
    add: &mut dyn FnMut(&str),
) {
    for (pi, para) in body.paragraphs.iter().enumerate() {
        if range.is_some_and(|(a, b)| pi < a.0 || pi > b.0) {
            continue;
        }
        let mut offset = 0usize;
        let mut has_text = false;
        for run in &para.runs {
            let (from, count) = if let Some((a, b)) = range {
                let end = offset.saturating_add(run.char_len());
                let from = if pi == a.0 { a.1.saturating_sub(offset) } else { 0 };
                let to = if pi == b.0 { b.1.min(end).saturating_sub(offset) } else { end - offset };
                offset = end;
                if from >= to {
                    continue;
                }
                if matches!(run.kind, deckcraft_model::text::RunKind::Text) { (from, to - from) } else { (0, usize::MAX) }
            } else {
                (0, usize::MAX)
            };
            let chars = run.text.chars().skip(from).take(count);
            if !chars.clone().any(|c| !c.is_whitespace()) {
                continue;
            }
            has_text = true;
            let r = resolve::run(ctx, s, para, &run.props);
            add(&resolve::font_family(ctx, &r));
            if include_slots {
                let major = r.font.as_deref().is_some_and(|f| f.starts_with("+mj"));
                for (explicit, fallback) in
                    [(&r.font_ea, if major { "+mj-ea" } else { "+mn-ea" }), (&r.font_cs, if major { "+mj-cs" } else { "+mn-cs" }), (&r.font_sym, "")]
                {
                    add(&ctx.master.theme.font(explicit.as_deref().unwrap_or(fallback)));
                }
                let set = if major { &ctx.master.theme.fonts.major } else { &ctx.master.theme.fonts.minor };
                let mut previous = None;
                for script in chars.filter_map(deckcraft_fonts::script::script_of) {
                    if previous == Some(script) {
                        continue;
                    }
                    previous = Some(script);
                    if let Ok(tag) = std::str::from_utf8(&script)
                        && let Some(family) = set.script(tag)
                    {
                        add(family);
                    }
                }
            }
        }
        let pp = resolve::para(ctx, s, para);
        if has_text
            && matches!(pp.bullet, Some(Bullet::Char { .. }))
            && let Some(f) = pp.bullet_font.as_deref()
        {
            add(&ctx.master.theme.font(f));
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
