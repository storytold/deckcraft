//! Fonts a presentation uses that this machine doesn't have, so opening it can say so (its text
//! draws in a stand-in; see `FontDb::document_face`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use deckcraft_fonts::embed::{Refusal, embed_face};
use deckcraft_fonts::script::{FontSlot, ScriptTag};
use deckcraft_fonts::{FontDb, FontFace};
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::{Bullet, Caps, Paragraph, RunKind, RunProps, TextBody};
use deckcraft_model::{EmbeddedFont, Presentation, Shape, ShapeKind};

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
                body_fonts(&ctx, s, body, &mut add);
            }
            if let ShapeKind::Table(t) = &s.kind {
                for cell in t.rows.iter().flat_map(|r| &r.cells) {
                    body_fonts(&ctx, s, &cell.text, &mut add);
                }
            }
        });
    }
    out
}

fn body_fonts(ctx: &Ctx, s: &Shape, body: &TextBody, add: &mut dyn FnMut(String)) {
    for para in &body.paragraphs {
        let has_text = para.runs.iter().any(|r| !r.text.trim().is_empty());
        if !has_text {
            continue;
        }
        for run in para.runs.iter().filter(|r| !r.text.trim().is_empty()) {
            let r = resolve::run(ctx, s, para, &run.props);
            add(resolve::font_family(ctx, &r));
        }
        let pp = resolve::para(ctx, s, para);
        if matches!(pp.bullet, Some(Bullet::Char { .. }))
            && let Some(f) = pp.bullet_font.as_deref()
        {
            add(ctx.master.theme.font(f));
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

/// What saving with Embed fonts in the file did: the typefaces the saved file carries and the
/// ones it doesn't, with why.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontEmbedding {
    pub embedded: Vec<String>,
    pub skipped: Vec<(String, SkipReason)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// Not installed here (nor embedded already), so there is no file to embed.
    NotInstalled,
    /// The font's licence (`fsType`) forbids embedding.
    Restricted,
    /// The font file couldn't be read, or is larger than a font part may be.
    Unusable,
    /// The presentation already carries as many embedded typefaces as a file may.
    TooMany,
}

impl SkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            SkipReason::NotInstalled => "notInstalled",
            SkipReason::Restricted => "restricted",
            SkipReason::Unusable => "unusable",
            SkipReason::TooMany => "tooMany",
        }
    }
}

/// The most typefaces a saved file embeds (the PowerPoint reader's limit too).
const MAX_EMBEDDED: usize = 64;
/// The largest font part the PowerPoint writer stores.
const MAX_PART: usize = 32 * 1024 * 1024;
const FACE_STYLES: [&str; 4] = ["Regular", "Bold", "Italic", "Bold Italic"];

/// One typeface the deck draws text in: the faces it uses (regular, bold, italic, bold italic)
/// and the text drawn in it.
#[derive(Default)]
struct Usage {
    name: String,
    faces: [bool; 4],
    text: Vec<String>,
}

/// Every typeface the slides, layouts and masters draw text in, in first-use order: each run's
/// Latin, East Asian or complex script typeface by character (theme fonts and the theme's
/// per-script fonts resolved as the text is drawn), and bullet fonts. Placeholders on layouts and
/// masters only hold prompt text and are left out.
fn font_usage(p: &Presentation, db: &FontDb) -> Vec<Usage> {
    let mut usage: Vec<Usage> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut add = |family: &str, face: usize, text: String| {
        let family = family.trim();
        if family.is_empty() || text.is_empty() {
            return;
        }
        let i = *index.entry(family.to_lowercase()).or_insert_with(|| {
            usage.push(Usage { name: family.to_string(), ..Default::default() });
            usage.len() - 1
        });
        if let Some(u) = usage.get_mut(i) {
            if let Some(f) = u.faces.get_mut(face) {
                *f = true;
            }
            u.text.push(text);
        }
    };
    let mut visit = |ctx: &Ctx, shapes: &[Shape], placeholders: bool| {
        deckcraft_model::walk(shapes, &mut |s, _| {
            if s.ph.is_some() && !placeholders {
                return;
            }
            if let Some(body) = &s.text {
                body_usage(ctx, s, body, db, &mut add);
            }
            if let ShapeKind::Table(t) = &s.kind {
                for cell in t.rows.iter().flat_map(|r| &r.cells) {
                    body_usage(ctx, s, &cell.text, db, &mut add);
                }
            }
        });
    };
    for slide in &p.slides {
        if let Some(ctx) = Ctx::for_slide(p, slide) {
            visit(&ctx, &slide.shapes, true);
        }
    }
    for m in &p.masters {
        visit(&Ctx::for_master(p, m), &m.shapes, false);
        for l in &m.layouts {
            visit(&Ctx::for_layout(p, m, l), &l.shapes, false);
        }
    }
    usage
}

fn body_usage(ctx: &Ctx, s: &Shape, body: &TextBody, db: &FontDb, add: &mut dyn FnMut(&str, usize, String)) {
    for para in &body.paragraphs {
        let mut first_family = None;
        for run in &para.runs {
            let text = match &run.kind {
                RunKind::Break => continue,
                RunKind::Field { .. } => format!("{}0123456789", run.text),
                _ => run.text.clone(),
            };
            if text.trim().is_empty() {
                continue;
            }
            let r = resolve::run(ctx, s, para, &run.props);
            let latin = resolve::font_family(ctx, &r);
            first_family.get_or_insert_with(|| latin.clone());
            run_usage(ctx, &r, &latin, &text, db, add);
        }
        bullet_usage(ctx, s, para, first_family, add);
    }
}

fn bullet_usage(ctx: &Ctx, s: &Shape, para: &Paragraph, run_family: Option<String>, add: &mut dyn FnMut(&str, usize, String)) {
    let Some(run_family) = run_family else { return };
    let pp = resolve::para(ctx, s, para);
    if let Some(Bullet::Char { char }) = &pp.bullet {
        let family = pp.bullet_font.as_deref().map(|f| ctx.master.theme.font(f)).unwrap_or(run_family);
        add(&family, 0, char.clone());
    }
}

/// The run's text split by the typeface each character is drawn in, added to `add` with the
/// run's face (bold and italic).
fn run_usage(ctx: &Ctx, r: &RunProps, latin: &str, text: &str, db: &FontDb, add: &mut dyn FnMut(&str, usize, String)) {
    let face = usize::from(r.bold.unwrap_or(false)) | usize::from(r.italic.unwrap_or(false)) << 1;
    let text = match r.caps {
        Some(Caps::All | Caps::Small) => format!("{text}{}", text.to_uppercase()),
        _ => text.to_string(),
    };
    let theme = &ctx.master.theme;
    let major = r.font.as_deref().is_some_and(|f| f.starts_with("+mj"));
    let set = if major { &theme.fonts.major } else { &theme.fonts.minor };
    let named = |explicit: &Option<String>, theme_ref: &str| {
        let name = theme.font(explicit.as_deref().unwrap_or(theme_ref));
        (!name.trim().is_empty()).then_some(name)
    };
    let ea = named(&r.font_ea, if major { "+mj-ea" } else { "+mn-ea" });
    let cs = named(&r.font_cs, if major { "+mj-cs" } else { "+mn-cs" });
    let mut cache: HashMap<(FontSlot, Option<ScriptTag>), String> = HashMap::new();
    let mut family_for = |slot: FontSlot, script: Option<ScriptTag>| -> String {
        cache
            .entry((slot, script))
            .or_insert_with(|| {
                let slot_face = match slot {
                    FontSlot::EastAsian => ea.clone(),
                    FontSlot::ComplexScript => cs.clone(),
                    FontSlot::Latin => None,
                };
                let script_face = script.and_then(|t| std::str::from_utf8(&t).ok().and_then(|t| set.script(t)).map(String::from));
                let chain: Vec<String> = slot_face.into_iter().chain(script_face).collect();
                chain.iter().find(|f| db.has_family(f)).or(chain.first()).cloned().unwrap_or_else(|| latin.to_string())
            })
            .clone()
    };
    let chars: Vec<char> = text.chars().collect();
    let mut seg = String::new();
    let mut cur: Option<String> = None;
    for (c, (script, slot)) in chars.iter().zip(deckcraft_fonts::script::resolve(&chars)) {
        let family = if slot == FontSlot::Latin && script.is_none_or(|t| set.script(std::str::from_utf8(&t).unwrap_or_default()).is_none()) {
            latin.to_string()
        } else {
            family_for(slot, script)
        };
        if cur.as_deref() != Some(family.as_str()) {
            if let Some(f) = cur.take() {
                add(&f, face, std::mem::take(&mut seg));
            }
            cur = Some(family);
        }
        seg.push(*c);
    }
    if let Some(f) = cur {
        add(&f, face, seg);
    }
}

/// Is `face` the `slot` face (0 regular, 1 bold, 2 italic, 3 bold italic) of `family` itself,
/// rather than the closest face [`FontDb::face`] settled for? A named instance of a variable font
/// isn't: the file would draw its default instance.
fn is_slot_face(face: &FontFace, family: &str, slot: usize) -> bool {
    if face.is_variable() {
        return false;
    }
    let (bold, italic) = (slot & 1 != 0, slot & 2 != 0);
    if face.family.eq_ignore_ascii_case(family) {
        return (face.weight >= 550.0) == bold && face.italic == italic;
    }
    let norm = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    face.legacy.as_ref().is_some_and(|(lf, ls)| lf.eq_ignore_ascii_case(family) && FACE_STYLES.get(slot).is_some_and(|s| norm(ls) == norm(s)))
}

/// Was `face` loaded from `entry`'s own parts (the machine has the typeface only because the
/// file brought it)?
fn from_entry(p: &Presentation, entry: &EmbeddedFont, face: &FontFace) -> bool {
    entry.faces().into_iter().flatten().filter_map(|id| p.media(id)).any(|m| deckcraft_fonts::embedded_sfnt(&m.data) == Some(face.data()))
}

/// Embed the typefaces `p` draws text in, as File ▸ Options ▸ Save ▸ Embed fonts in the file
/// does on save: each installed face the deck uses (every regular, bold, italic and bold italic
/// face of the family with [`Presentation::embed_all_characters`]), cut down to the characters
/// used unless that option asks for all of them. A typeface already embedded is embedded afresh
/// from the installed font, keeping faces this machine lacks; one this machine has only from the
/// file's own copy keeps that copy. Fonts whose licence forbids embedding, and fonts that aren't
/// installed, are reported and left out. Typefaces embedded before and no longer used stay.
pub fn embed_used_fonts(p: &mut Presentation, db: &FontDb) -> FontEmbedding {
    let mut report = FontEmbedding::default();
    let all = p.embed_all_characters;
    for u in font_usage(p, db) {
        if u.name.starts_with('.') || u.name.starts_with('+') {
            continue;
        }
        let existing = p.embedded_fonts.iter().position(|e| e.typeface.trim().eq_ignore_ascii_case(&u.name));
        let mut faces: [Option<Arc<FontFace>>; 4] = Default::default();
        if db.has_family(&u.name) {
            for (slot, style) in FACE_STYLES.iter().enumerate() {
                if !all && !u.faces.get(slot).copied().unwrap_or(false) {
                    continue;
                }
                let f = db.face(&u.name, style);
                if is_slot_face(&f, &u.name, slot) && !faces.iter().flatten().any(|g| g.id() == f.id()) {
                    faces[slot] = Some(f);
                }
            }
        }
        let old = existing.and_then(|i| p.embedded_fonts.get(i)).cloned();
        if faces.iter().all(Option::is_none) {
            if old.is_some() {
                report.embedded.push(u.name);
            } else {
                report.skipped.push((u.name, SkipReason::NotInstalled));
            }
            continue;
        }
        if let Some(old) = &old
            && faces.iter().flatten().any(|f| from_entry(p, old, f))
        {
            report.embedded.push(u.name);
            continue;
        }
        if old.is_none() && p.embedded_fonts.len() >= MAX_EMBEDDED {
            report.skipped.push((u.name, SkipReason::TooMany));
            continue;
        }
        let text = (!all).then_some(u.text.as_slice());
        let mut entry = EmbeddedFont { typeface: u.name.chars().take(255).collect(), ..Default::default() };
        let mut why = SkipReason::Unusable;
        for (slot, face) in faces.iter().enumerate() {
            let Some(face) = face else { continue };
            let part = match embed_face(face, text) {
                Ok(part) if part.data.len() <= MAX_PART => part,
                Ok(_) | Err(Refusal::Unreadable) => continue,
                Err(Refusal::NotPermitted(_)) => {
                    why = SkipReason::Restricted;
                    continue;
                }
            };
            if entry.panose.is_none() {
                entry.panose = part.panose.map(|b| b.iter().map(|x| format!("{x:02X}")).collect());
            }
            let id = p.add_media("font.fntdata", "application/x-fontdata", part.data);
            match slot {
                0 => entry.regular = id,
                1 => entry.bold = id,
                2 => entry.italic = id,
                _ => entry.bold_italic = id,
            }
        }
        if let Some(old) = &old {
            entry.regular = entry.regular.or(old.regular);
            entry.bold = entry.bold.or(old.bold);
            entry.italic = entry.italic.or(old.italic);
            entry.bold_italic = entry.bold_italic.or(old.bold_italic);
            entry.panose = entry.panose.or_else(|| old.panose.clone());
            entry.pitch_family = old.pitch_family;
            entry.charset = old.charset;
        }
        if entry.faces().iter().all(Option::is_none) {
            report.skipped.push((u.name, why));
            continue;
        }
        match existing.and_then(|i| p.embedded_fonts.get_mut(i)) {
            Some(slot) => *slot = entry,
            None => p.embedded_fonts.push(entry),
        }
        report.embedded.push(u.name);
    }
    report
}

#[cfg(test)]
mod embed_tests;
