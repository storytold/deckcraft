//! The native `.deckcraft` format: a zip holding `presentation.json` (the model, minus media
//! bytes), `media/<id>` and `chart-parts/<index>` binary entries, and `mimetype`. Readers ignore unknown
//! fields, so newer files open in older builds with what those builds understand.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod chart_parts;
mod finite;
pub mod rtf;

#[cfg(test)]
mod chart_parts_tests;

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use deckcraft_model::text::{Paragraph, TextBody};
use deckcraft_model::{LayoutType, Presentation, ShapeKind, defaults};

pub const MIMETYPE: &str = "application/vnd.storyteller.deckcraft+zip";
/// Files written before the SlideCraft → DeckCraft rename (`.slidecraft`) are still read.
pub const LEGACY_MIMETYPE: &str = concat!("application/vnd.storyteller.", "slide", "craft+zip");
pub const EXTENSION: &str = "deckcraft";
/// Format version written into the manifest.
pub const VERSION: u32 = 1;
/// Largest entry we will inflate (guards against zip bombs).
const MAX_ENTRY: u64 = 2 << 30;
/// All embedded media and chart sidecars together, inflated (#393).
const MAX_TOTAL_MEDIA: u64 = 4 << 30;
/// Media items and chart sidecar parts one file may list.
const MAX_MEDIA_ITEMS: usize = 100_000;

/// What one load may inflate.
#[derive(Clone, Copy)]
struct Budget {
    total: u64,
    items: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("not a DeckCraft file: {0}")]
    NotOurs(String),
    #[error("damaged file: {0}")]
    Damaged(String),
    #[error("write failed: {0}")]
    Write(String),
}

pub type Result<T> = std::result::Result<T, FormatError>;

#[derive(serde::Serialize, serde::Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    generator: String,
    presentation: Presentation,
}

/// Serialize a presentation.
pub fn save(p: &Presentation) -> Result<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let w = |e: zip::result::ZipError| FormatError::Write(e.to_string());
        z.start_file("mimetype", stored).map_err(w)?;
        z.write_all(MIMETYPE.as_bytes()).map_err(|e| FormatError::Write(e.to_string()))?;
        let mut presentation = p.clone();
        let parts = chart_parts::separate(&mut presentation)?;
        let m =
            Manifest { format: "deckcraft".into(), version: VERSION, generator: format!("DeckCraft {}", env!("CARGO_PKG_VERSION")), presentation };
        let json = serde_json::to_vec(&finite::Finite(&m)).map_err(|e| FormatError::Write(e.to_string()))?;
        z.start_file("presentation.json", deflate).map_err(w)?;
        z.write_all(&json).map_err(|e| FormatError::Write(e.to_string()))?;
        for item in &p.media {
            if item.link.is_some() && item.data.is_empty() {
                continue;
            }
            // Already-compressed media is stored, not deflated again.
            let opts = if is_compressed(&item.content_type) { stored } else { deflate };
            z.start_file(format!("media/{}", item.id.0), opts).map_err(w)?;
            z.write_all(&item.data).map_err(|e| FormatError::Write(e.to_string()))?;
        }
        for (name, content_type, bytes) in parts {
            let opts = if is_compressed(&content_type) { stored } else { deflate };
            z.start_file(name, opts).map_err(w)?;
            z.write_all(&bytes).map_err(|e| FormatError::Write(e.to_string()))?;
        }
        z.finish().map_err(w)?;
    }
    Ok(buf.into_inner())
}

fn is_compressed(ct: &str) -> bool {
    ct.starts_with("image/jpeg")
        || ct.starts_with("image/png")
        || ct.starts_with("video/")
        || ct.starts_with("audio/mpeg")
        || ct.starts_with("audio/mp4")
        || ct.contains("zip")
}

/// Does this look like a `.deckcraft` file?
pub fn sniff(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK") && [MIMETYPE, LEGACY_MIMETYPE].iter().any(|m| bytes.windows(m.len()).take(200).any(|w| w == m.as_bytes()))
}

fn read_entry(z: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>> {
    read_entry_max(z, name, MAX_ENTRY)
}

/// Read one member, refusing it before inflating if it declares more than `max` bytes, and
/// stopping at `max` if it inflates further than it declared.
fn read_entry_max(z: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str, max: u64) -> Result<Vec<u8>> {
    let f = z.by_name(name).map_err(|e| FormatError::Damaged(format!("{name}: {e}")))?;
    if f.size() > max {
        return Err(FormatError::Damaged(format!("{name} is too large")));
    }
    let mut out = Vec::with_capacity(f.size().min(64 << 20) as usize);
    let mut f = f.take(max.saturating_add(1));
    f.read_to_end(&mut out).map_err(|e| FormatError::Damaged(format!("{name}: {e}")))?;
    if out.len() as u64 > max {
        return Err(FormatError::Damaged(format!("{name} is too large")));
    }
    Ok(out)
}

/// Read a presentation. Missing media/chart parts are left empty; use [`load_checked`] to
/// learn which. A damaged chart member refuses the file before further chart inflation.
pub fn load(bytes: &[u8]) -> Result<Presentation> {
    load_checked(bytes).map(|(p, _)| p)
}

/// Read a presentation and list media that couldn't be read, plus missing chart members.
/// A chart member that is present but fails decoding, CRC or its entry limit refuses the file.
/// Linked media saved without its bytes isn't damage.
pub fn load_checked(bytes: &[u8]) -> Result<(Presentation, Vec<String>)> {
    load_within(bytes, Budget { total: MAX_TOTAL_MEDIA, items: MAX_MEDIA_ITEMS })
}

fn load_within(bytes: &[u8], budget: Budget) -> Result<(Presentation, Vec<String>)> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    let json = read_entry(&mut z, "presentation.json").map_err(|_| FormatError::NotOurs("no presentation.json".into()))?;
    let m = parse_manifest(&json)?;
    if m.format != "deckcraft" {
        return Err(FormatError::NotOurs(m.format));
    }
    let mut p = m.presentation;
    let mut damaged = vec![];
    let items = p.media.len();
    if items > budget.items {
        return Err(FormatError::Damaged(format!("{} media items (at most {})", p.media.len(), budget.items)));
    }
    // A repeated id names the same member: keep the first item, so the member is inflated once
    // and the deck can be saved again (a zip can't hold two `media/<id>`).
    let mut seen = std::collections::HashSet::new();
    p.media.retain(|m| seen.insert(m.id));
    let mut left = budget.total;
    for item in &mut p.media {
        let name = format!("media/{}", item.id.0);
        let linked_only = item.link.is_some() && z.index_for_name(&name).is_none();
        if z.by_name(&name).is_ok_and(|f| f.size() > left) {
            return Err(FormatError::Damaged("embedded media is larger than DeckCraft can open".into()));
        }
        match read_entry_max(&mut z, &name, left.min(MAX_ENTRY)) {
            Ok(data) => {
                left -= data.len() as u64;
                item.data = Arc::new(data);
            }
            Err(_) if linked_only => {}
            Err(e) => damaged.push(e.to_string()),
        }
    }
    chart_parts::restore(&mut p, &mut z, &mut left, items, budget.items, &mut damaged)?;
    repair(&mut p);
    Ok((p, damaged))
}

/// Make a loaded presentation safe to edit: a master exists, every slide's layout exists, tables
/// are rectangular, and duplicate ids are re-id'd — unless the deck has used every id, in which
/// case a duplicate stays (growth is refused; see the comment in the loop).
pub fn repair(p: &mut Presentation) {
    if p.masters.is_empty() {
        let fresh = defaults::blank_presentation(p.slide_size, Default::default(), false);
        p.masters = fresh.masters;
    }
    p.fix_next_id();
    let first_layout = p.masters.first().and_then(|m| m.layouts.first()).map(|l| l.id).unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    for i in 0..p.slides.len() {
        let needs_layout = p.slides.get(i).is_some_and(|s| p.layout(s.layout).is_none());
        let dup = p.slides.get(i).is_some_and(|s| !seen.insert(s.id));
        if needs_layout || dup {
            // An exhausted id space leaves a duplicate slide as it is: the engine refuses to
            // grow such a deck (edits by id stay ambiguous for the duplicate), and the duplicate
            // stays visible rather than the file failing to open (#407).
            let new_id = if dup { p.alloc_id().map(deckcraft_model::SlideId) } else { None };
            if let Some(s) = p.slides.get_mut(i) {
                let s = Arc::make_mut(s);
                if needs_layout {
                    s.layout = first_layout;
                }
                if let Some(id) = new_id {
                    s.id = id;
                }
            }
        }
    }
    reid_duplicate_shapes(p);
    p.slide_size = sane_slide_size(p.slide_size);
    // The notes page only lays out notes, but its size divides too.
    if !(p.notes_size.width.is_finite() && p.notes_size.width > 0.0 && p.notes_size.height.is_finite() && p.notes_size.height > 0.0) {
        p.notes_size = Presentation::default().notes_size;
    }
    sane_numbers(p);
}

/// Give a shape that repeats an id already used on its slide, layout or master (groups included)
/// a fresh one, so edits, selection and the Selection Pane find one shape per id. The first keeps
/// its id, and with it what animations and connectors point at. An exhausted id space leaves the
/// rest as they are (#407).
fn reid_duplicate_shapes(p: &mut Presentation) {
    fn has_duplicates(shapes: &[deckcraft_model::Shape]) -> bool {
        let mut seen = std::collections::HashSet::new();
        let mut dup = false;
        deckcraft_model::walk(shapes, &mut |sh, _| dup |= !seen.insert(sh.id));
        dup
    }
    fn reid(shapes: &mut [deckcraft_model::Shape], seen: &mut std::collections::HashSet<deckcraft_model::ShapeId>, next: &mut u32, depth: usize) {
        if depth > 64 {
            return;
        }
        for sh in shapes {
            if !seen.insert(sh.id)
                && let Some(id) = (*next).max(1).checked_add(1)
            {
                *next = id;
                sh.id = deckcraft_model::ShapeId(id);
                seen.insert(sh.id);
            }
            if let deckcraft_model::ShapeKind::Group { children, .. } = &mut sh.kind {
                reid(children, seen, next, depth + 1);
            }
        }
    }
    let mut next = p.next_id;
    let mut fix = |shapes: &mut Vec<deckcraft_model::Shape>| {
        if has_duplicates(shapes) {
            reid(shapes, &mut std::collections::HashSet::new(), &mut next, 0);
        }
    };
    for s in &mut p.slides {
        if has_duplicates(&s.shapes) {
            fix(&mut Arc::make_mut(s).shapes);
        }
    }
    for m in p.masters.iter_mut().chain(p.notes_master.iter_mut()).chain(p.handout_master.iter_mut()) {
        if has_duplicates(&m.shapes) || m.layouts.iter().any(|l| has_duplicates(&l.shapes)) {
            let m = Arc::make_mut(m);
            fix(&mut m.shapes);
            for l in &mut m.layouts {
                fix(&mut l.shapes);
            }
        }
    }
    p.next_id = next;
}

/// Smallest and largest slide side, in points: PowerPoint's 1–56 in, as Design › Slide Size allows.
pub const SLIDE_SIDE: std::ops::RangeInclusive<f64> = 72.0..=4032.0;

/// A slide size the views and renderer can always lay out: a missing, zero, negative or
/// non-finite side gives the 16:9 default, and each side is brought into [`SLIDE_SIDE`].
pub fn sane_slide_size(s: deckcraft_model::Size) -> deckcraft_model::Size {
    let ok = |v: f64| v.is_finite() && v > 0.0;
    if !(ok(s.width) && ok(s.height)) {
        return defaults::WIDE;
    }
    deckcraft_model::Size::new(s.width.clamp(*SLIDE_SIDE.start(), *SLIDE_SIDE.end()), s.height.clamp(*SLIDE_SIDE.start(), *SLIDE_SIDE.end()))
}

/// Slides read from an outline: each title with its body paragraphs (level 0–8, text).
type OutlineSlides = Vec<(String, Vec<(u8, String)>)>;

/// Append a Title and Content slide per outline entry; the number made.
fn add_outline_slides(p: &mut Presentation, slides: OutlineSlides) -> usize {
    let Some(lay) = defaults::layout_of_kind(p, LayoutType::TitleAndContent) else { return 0 };
    let mut made = 0;
    for (title, body) in slides {
        // An exhausted id space makes no new slides: what fit is kept (#407).
        let Some(mut s) = defaults::new_slide(p, lay) else { break };
        if let Some(t) = s.shapes.first_mut() {
            t.text = Some(TextBody::from_text(&title));
        }
        if let Some(b) = s.shapes.get_mut(1) {
            for (level, line) in body {
                let tb = b.text.get_or_insert_with(TextBody::default);
                let mut para = Paragraph::new(&line);
                para.level = level.min(8);
                tb.paragraphs.push(para);
            }
        }
        p.slides.push(Arc::new(s));
        made += 1;
    }
    made
}

/// Most `null`s standing in for numbers that [`parse_manifest`] patches in one file, and the
/// candidates it tries for each.
const MAX_NULL_FIXES: usize = 256;
const MAX_TRIES_PER_FIX: usize = 32;
/// Bytes [`parse_manifest`] may re-parse in all while patching, so a hostile file full of `null`s
/// can't keep it busy: past this the file is reported damaged.
const MAX_REPARSE_BYTES: usize = 256 * 1024 * 1024;

/// Where a parse failed, comparable: later is progress.
fn failure(src: &[u8]) -> std::result::Result<Manifest, (String, (usize, usize))> {
    serde_json::from_slice::<Manifest>(src).map_err(|e| (e.to_string(), (e.line(), e.column())))
}

/// Parse `presentation.json`. serde_json writes a NaN or infinite number as `null`, which then
/// fails to read back as a number ("invalid type: null, expected f64"), so DeckCraft could save
/// a file it couldn't open. Each such `null` is read as 0 instead ([`repair`] then brings the
/// value into range); anything else that doesn't parse is damage.
///
/// serde reports the failing `null` exactly, except inside tagged enums (buffered), where it
/// reports the end of the enclosing object: there the `null`s before that point are tried
/// nearest first, and a patch is kept only if the parse then gets further. (Files saved since
/// [`finite::Finite`] never contain such `null`s; this reads older ones.)
fn parse_manifest(json: &[u8]) -> Result<Manifest> {
    let mut src = json.to_vec();
    let mut last = match failure(&src) {
        Ok(m) => return Ok(m),
        Err(f) => f,
    };
    let mut budget = MAX_REPARSE_BYTES;
    'fixes: for _ in 0..MAX_NULL_FIXES {
        let (msg, pos) = &last;
        let number = msg.starts_with("invalid type: null, expected f") || msg.starts_with("invalid type: null, expected a number");
        if !number {
            break;
        }
        let end = offset(&src, pos.0, pos.1).unwrap_or(src.len());
        let mut progressed = false;
        let candidates: Vec<usize> = value_nulls_before(&src, end + 4).take(MAX_TRIES_PER_FIX).collect();
        for at in candidates {
            let (Some(head), Some(tail)) = (src.get(..at), src.get(at.saturating_add(4)..)) else { continue };
            let Some(left) = budget.checked_sub(src.len()) else { break 'fixes };
            budget = left;
            let mut next = Vec::with_capacity(src.len());
            next.extend_from_slice(head);
            next.push(b'0');
            next.extend_from_slice(tail);
            match failure(&next) {
                Ok(m) => return Ok(m),
                // The patch shortened the text by 3 bytes before the error: compare positions
                // allowing for that.
                Err(f) if f.1 > (pos.0, pos.1.saturating_sub(3)) => {
                    src = next;
                    last = f;
                    progressed = true;
                    break;
                }
                Err(_) => {}
            }
        }
        if !progressed {
            break;
        }
    }
    Err(FormatError::Damaged(last.0))
}

/// Byte offset of a 1-based `line`:`column`.
fn offset(src: &[u8], line: usize, column: usize) -> Option<usize> {
    let mut start = 0;
    for _ in 1..line {
        start += src.get(start..)?.iter().position(|b| *b == b'\n')? + 1;
    }
    Some((start + column).min(src.len()))
}

/// Starts of `null` tokens in value position (after `:`, `[` or `,`) before `end`, nearest first.
fn value_nulls_before(src: &[u8], end: usize) -> impl Iterator<Item = usize> + '_ {
    let end = end.min(src.len());
    (0..end.saturating_sub(3)).rev().filter(move |&i| {
        src.get(i..i + 4) == Some(b"null")
            && src[..i].iter().rev().find(|b| !b.is_ascii_whitespace()).is_some_and(|b| matches!(b, b':' | b'[' | b','))
            && src.get(i + 4).is_none_or(|b| !b.is_ascii_alphanumeric())
    })
}

/// Largest coordinate or size kept, in points (about 350 m; PowerPoint's own limit is far smaller).
const MAX_COORD: f64 = 1.0e6;

/// Numbers no view can draw or no file should keep: non-finite positions, sizes and angles become
/// 0, huge ones are clamped, angles are brought into 0–360, crops into a sane range, font sizes
/// into 1–4000 pt and line widths into 0–1584 pt (PowerPoint's limits).
fn sane_xfrm(x: &mut deckcraft_model::Xfrm) {
    let fin = |v: f64| if v.is_finite() { v } else { 0.0 };
    x.x = fin(x.x).clamp(-MAX_COORD, MAX_COORD);
    x.y = fin(x.y).clamp(-MAX_COORD, MAX_COORD);
    x.w = fin(x.w).clamp(0.0, MAX_COORD);
    x.h = fin(x.h).clamp(0.0, MAX_COORD);
    x.rot = fin(x.rot).rem_euclid(360.0);
    if x.rot >= 360.0 {
        x.rot = 0.0;
    }
}

fn sane_run(p: &mut deckcraft_model::text::RunProps) {
    p.size = p.size.filter(|v| v.is_finite() && *v > 0.0).map(|v| v.clamp(1.0, 4000.0));
    for s in p.shadow.iter_mut().chain(p.inner_shadow.iter_mut()) {
        sane_shadow(s);
    }
    if let Some(g) = p.glow.as_mut() {
        g.radius = effect_size(g.radius, MAX_GLOW);
    }
    if let Some(r) = p.reflection.as_mut() {
        sane_reflection(r);
    }
    p.soft_edge = p.soft_edge.map(|v| effect_size(v, MAX_SOFT_EDGE));
}

/// The largest effect sizes, in points: what Format Shape and `shape.effects` allow. A file can
/// say anything, and the renderer's memory grows with these (a glow of 10^18 pt asked for 34 GB).
const MAX_GLOW: f64 = 150.0;
const MAX_SOFT_EDGE: f64 = 100.0;
const MAX_SHADOW: f64 = 200.0;
const MAX_REFLECTION: f64 = 100.0;

/// `v` within 0..=`max`; not a number is 0.
fn effect_size(v: f64, max: f64) -> f64 {
    if v.is_finite() { v.clamp(0.0, max) } else { 0.0 }
}

fn sane_shadow(s: &mut deckcraft_model::style::Shadow) {
    s.blur = effect_size(s.blur, MAX_SHADOW);
    s.dist = effect_size(s.dist, MAX_SHADOW);
    s.dir = if s.dir.is_finite() { s.dir.rem_euclid(360.0) } else { 0.0 };
    // Perspective scale (1 = 100%) and skew (degrees).
    for v in [&mut s.sx, &mut s.sy] {
        *v = if v.is_finite() { v.clamp(-10.0, 10.0) } else { 1.0 };
    }
    for v in [&mut s.kx, &mut s.ky] {
        *v = if v.is_finite() { v.clamp(-89.0, 89.0) } else { 0.0 };
    }
}

fn sane_reflection(r: &mut deckcraft_model::style::Reflection) {
    r.blur = effect_size(r.blur, MAX_REFLECTION);
    r.dist = effect_size(r.dist, MAX_REFLECTION);
}

/// Run properties a list style hands down to the text that inherits it.
fn sane_list_style(ls: &mut deckcraft_model::text::ListStyle) {
    for level in ls.levels.iter_mut().flatten() {
        sane_run(&mut level.run);
    }
}

fn sane_effects(e: &mut deckcraft_model::Effects) {
    for s in e.outer_shadow.iter_mut().chain(e.inner_shadow.iter_mut()) {
        sane_shadow(s);
    }
    if let Some(g) = e.glow.as_mut() {
        g.radius = effect_size(g.radius, MAX_GLOW);
    }
    e.soft_edge = e.soft_edge.map(|v| effect_size(v, MAX_SOFT_EDGE));
    if let Some(r) = e.reflection.as_mut() {
        sane_reflection(r);
    }
}

fn sane_shapes(shapes: &mut [deckcraft_model::Shape], depth: usize) {
    if depth > 64 {
        return;
    }
    for sh in shapes {
        if let Some(x) = sh.xfrm.as_mut() {
            sane_xfrm(x);
        }
        if let Some(l) = sh.line.as_mut() {
            l.width = l.width.filter(|v| v.is_finite() && *v >= 0.0).map(|v| v.min(1584.0));
        }
        if let Some(e) = sh.effects.as_mut() {
            sane_effects(e);
        }
        if let Some(t) = sh.text.as_mut() {
            sane_list_style(&mut t.list_style);
            for para in &mut t.paragraphs {
                sane_run(&mut para.end_props);
                for r in &mut para.runs {
                    sane_run(&mut r.props);
                }
            }
        }
        match &mut sh.kind {
            deckcraft_model::ShapeKind::Picture { fill: deckcraft_model::style::PictureFill { crop, .. } }
            | deckcraft_model::ShapeKind::Media(deckcraft_model::MediaClip { crop, .. }) => {
                for c in crop {
                    *c = if c.is_finite() { c.clamp(-10.0, 1.0) } else { 0.0 };
                }
            }
            deckcraft_model::ShapeKind::Group { children, child } => {
                sane_xfrm(child);
                sane_shapes(children, depth + 1);
            }
            deckcraft_model::ShapeKind::Table(t) => {
                for cell in t.rows.iter_mut().flat_map(|r| r.cells.iter_mut()) {
                    sane_list_style(&mut cell.text.list_style);
                }
                for para in t.rows.iter_mut().flat_map(|r| r.cells.iter_mut()).flat_map(|c| c.text.paragraphs.iter_mut()) {
                    sane_run(&mut para.end_props);
                    for r in &mut para.runs {
                        sane_run(&mut r.props);
                    }
                }
            }
            _ => {}
        }
    }
}

/// [`sane_shapes`] over every slide, layout and master; slides are only copied when they change.
pub fn sane_numbers(p: &mut Presentation) {
    for slide in &mut p.slides {
        let mut fixed = (**slide).clone();
        sane_shapes(&mut fixed.shapes, 0);
        if fixed != **slide {
            *slide = Arc::new(fixed);
        }
    }
    // What text and shapes inherit: the deck's default text style, each master's title, body and
    // other styles, and the theme's effect styles that shape styles point at.
    sane_list_style(&mut p.default_text_style);
    for m in p.masters.iter_mut().chain(p.notes_master.iter_mut()).chain(p.handout_master.iter_mut()) {
        let mut fixed = (**m).clone();
        sane_shapes(&mut fixed.shapes, 0);
        for l in &mut fixed.layouts {
            sane_shapes(&mut l.shapes, 0);
        }
        for ls in [&mut fixed.title_style, &mut fixed.body_style, &mut fixed.other_style] {
            sane_list_style(ls);
        }
        for e in &mut fixed.theme.format.effects {
            sane_effects(e);
        }
        if fixed != **m {
            *m = Arc::new(fixed);
        }
    }
}

/// Outline text: one slide per unindented line; tab-indented lines become body bullets at their
/// level (Insert ▸ Slides from Outline, View ▸ Outline copy).
pub fn outline_to_slides(p: &mut Presentation, text: &str) -> usize {
    let mut slides: OutlineSlides = Vec::new();
    for raw in text.lines() {
        if raw.trim().is_empty() {
            continue;
        }
        let tabs = raw.chars().take_while(|c| *c == '\t').count();
        let spaces = raw.chars().take_while(|c| *c == ' ').count() / 2;
        let level = tabs.max(spaces);
        let t = raw.trim();
        let line = match t.strip_prefix(['-', '*', '•']) {
            Some(rest) if rest.starts_with([' ', '\t']) => rest.trim(),
            _ => t,
        }
        .to_string();
        if level == 0 {
            slides.push((line, Vec::new()));
        } else if let Some((_, body)) = slides.last_mut() {
            body.push(((level - 1).min(8) as u8, line));
        }
    }
    add_outline_slides(p, slides)
}

/// An ATX heading's text (CommonMark: up to three spaces, one to six `#`, then a space or the end;
/// a closing run of `#` is dropped).
fn md_heading(line: &str) -> Option<&str> {
    let t = line.strip_prefix("   ").or_else(|| line.strip_prefix("  ")).or_else(|| line.strip_prefix(' ')).unwrap_or(line);
    let hashes = t.chars().take_while(|c| *c == '#').count();
    let rest = t.get(hashes..)?;
    if !(1..=6).contains(&hashes) || !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    let rest = rest.trim();
    let closed = rest.trim_end_matches('#');
    Some(if closed.is_empty() || closed.ends_with([' ', '\t']) { closed.trim_end() } else { rest })
}

/// A list item's nesting level (two spaces or a tab each) and text: `-`, `*` or `+`, or digits
/// then `.` or `)`, followed by a space.
fn md_list_item(line: &str) -> Option<(u8, &str)> {
    let indent: usize = line.chars().take_while(|c| matches!(c, ' ' | '\t')).map(|c| if c == '\t' { 2 } else { 1 }).sum();
    let t = line.trim_start();
    let rest = match t.strip_prefix(['-', '*', '+']) {
        Some(r) => r,
        None => {
            let digits = t.chars().take_while(char::is_ascii_digit).count();
            if digits == 0 || digits > 9 {
                return None;
            }
            t.get(digits..)?.strip_prefix(['.', ')'])?
        }
    };
    rest.starts_with([' ', '\t']).then(|| ((indent / 2).min(8) as u8, rest.trim()))
}

/// Markdown outline text (`.md`): each heading starts a slide with its text as the title; list
/// items under it become bullets nested by their indentation, other lines body paragraphs. Text
/// before the first heading is skipped, as indented lines before the first title are in the
/// plain format. A file without headings is read as a plain outline ([`outline_to_slides`]).
pub fn markdown_to_slides(p: &mut Presentation, text: &str) -> usize {
    if !text.lines().any(|l| md_heading(l).is_some()) {
        return outline_to_slides(p, text);
    }
    let mut slides: OutlineSlides = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(title) = md_heading(line) {
            slides.push((title.to_string(), Vec::new()));
        } else if let Some((_, body)) = slides.last_mut() {
            let (level, text) = md_list_item(line).unwrap_or((0, line.trim()));
            body.push((level, text.to_string()));
        }
    }
    add_outline_slides(p, slides)
}

/// The deck as outline text (titles unindented, body paragraphs tab-indented by level).
pub fn slides_to_outline(p: &Presentation) -> String {
    let mut out = String::new();
    for s in &p.slides {
        // One line per title: a newline would start another slide on re-import.
        out.push_str(&s.title().replace('\n', " "));
        out.push('\n');
        for sh in &s.shapes {
            if sh.ph_type().is_some_and(|k| k.is_title()) {
                continue;
            }
            if let Some(t) = &sh.text {
                for para in &t.paragraphs {
                    if para.is_empty() {
                        continue;
                    }
                    for _ in 0..=para.level {
                        out.push('\t');
                    }
                    out.push_str(&para.text().replace('\u{b}', " "));
                    out.push('\n');
                }
            }
            if let ShapeKind::Table(t) = &sh.kind {
                for row in &t.rows {
                    let cells: Vec<String> = row.cells.iter().map(|c| c.text.text().replace(['\u{b}', '\n'], " ")).collect();
                    out.push('\t');
                    out.push_str(&cells.join("\t"));
                    out.push('\n');
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_sidecars_are_optional_for_older_native_files() {
        use deckcraft_model::chart::{Chart, ChartType};
        let chart = Chart::sample(ChartType::Pie);
        let mut json = serde_json::to_value(&chart).expect("chart JSON");
        assert!(json.get("kept").is_none(), "empty sidecars should not enlarge ordinary files");
        assert_eq!(serde_json::from_value::<Chart>(json.clone()).expect("old chart without kept"), chart);
        json["kept"] = serde_json::json!({});
        assert_eq!(serde_json::from_value::<Chart>(json).expect("empty sidecar"), chart);
    }

    #[test]
    fn native_save_keeps_chart_relationships_and_payloads() {
        use deckcraft_model::chart::{Chart, ChartKept, ChartPart, ChartRel, ChartType};
        use deckcraft_model::{Shape, ShapeId, ShapeKind};
        let kept = ChartKept {
            rels: vec![ChartRel {
                id: "rWorkbook".into(),
                rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/package".into(),
                target: "../embeddings/data.xlsx".into(),
                part: "ppt/embeddings/data.xlsx".into(),
                external: false,
            }],
            parts: vec![ChartPart {
                name: "ppt/embeddings/data.xlsx".into(),
                content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
                bytes: vec![0, 255, 128, 42],
                native_entry: None,
            }],
        };
        let mut p = defaults::new_presentation(None);
        let chart = Chart { kept: kept.clone(), ..Chart::sample(ChartType::Pie) };
        let s = Shape { id: ShapeId(p.alloc_id().expect("id")), kind: ShapeKind::Chart(Box::new(chart)), ..Default::default() };
        // Serialization must reach charts inside groups as well as top-level charts.
        Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape {
            id: ShapeId(p.alloc_id().expect("group id")),
            kind: ShapeKind::Group { children: vec![s], child: Default::default() },
            ..Default::default()
        }];
        let mut q = load(&save(&p).expect("save")).expect("load");
        for _ in 0..2 {
            let ShapeKind::Group { children, .. } = &q.slides[0].shapes[0].kind else { panic!("group") };
            let ShapeKind::Chart(chart) = &children[0].kind else { panic!("chart") };
            assert_eq!(chart.kept, kept);
            q = load(&save(&q).expect("save again")).expect("load again");
        }
    }

    /// The same package without the entries `drop` names.
    fn without(bytes: &[u8], drop: &str) -> Vec<u8> {
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..z.len() {
            let f = z.by_index(i).unwrap();
            if f.name() != drop {
                out.raw_copy_file(f).unwrap();
            }
        }
        out.finish().unwrap().into_inner()
    }

    #[test]
    fn missing_or_corrupt_media_is_reported() {
        let mut p = Presentation::default();
        let id = p.add_media("a.png", "image/png", b"PICTURE-BYTES-0123456789".to_vec()).expect("media");
        let good = save(&p).unwrap();
        assert!(load_checked(&good).unwrap().1.is_empty());
        // Missing member.
        let (q, damaged) = load_checked(&without(&good, &format!("media/{}", id.0))).unwrap();
        assert_eq!(damaged.len(), 1, "{damaged:?}");
        assert!(q.media(id).is_some_and(|m| m.data.is_empty()));
        // Bad CRC: PNGs are stored, so their bytes sit in the file as they are.
        let mut bad = good.clone();
        let at = bad.windows(13).position(|w| w == b"PICTURE-BYTES").unwrap();
        bad[at] ^= 0xff;
        let (_, damaged) = load_checked(&bad).unwrap();
        assert_eq!(damaged.len(), 1, "{damaged:?}");
        // Linked media saved without its bytes isn't damage.
        let mut l = Presentation::default();
        let lid = l.add_media("v.mp4", "video/mp4", vec![]).expect("media");
        if let Some(m) = l.media.iter_mut().find(|m| m.id == lid) {
            m.link = Some("file:///movies/v.mp4".into());
        }
        assert!(load_checked(&save(&l).unwrap()).unwrap().1.is_empty());
    }

    #[test]
    fn media_budget_is_cumulative() {
        let mut p = Presentation::default();
        p.add_media("a.bin", "application/octet-stream", vec![1; 600]);
        p.add_media("b.bin", "application/octet-stream", vec![2; 600]);
        let bytes = save(&p).unwrap();
        // Exactly at the limit opens; one byte under is refused, though each item fits.
        assert!(load_within(&bytes, Budget { total: 1200, items: 2 }).is_ok());
        assert!(load_within(&bytes, Budget { total: 1199, items: 2 }).is_err());
        assert!(load_within(&bytes, Budget { total: 1200, items: 1 }).is_err());
    }

    #[test]
    fn repeated_media_ids_are_read_once() {
        use std::io::Write;
        let mut p = Presentation::default();
        p.add_media("a.bin", "application/octet-stream", vec![7; 1000]);
        let good = save(&p).unwrap();
        // The same file with its media item listed 51 times.
        let mut z = zip::ZipArchive::new(Cursor::new(good.as_slice())).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..z.len() {
            let mut f = z.by_index(i).unwrap();
            if f.name() == "presentation.json" {
                let mut json = vec![];
                f.read_to_end(&mut json).unwrap();
                let mut v: serde_json::Value = serde_json::from_slice(&json).unwrap();
                let media = &mut v["presentation"]["media"];
                let one = media[0].clone();
                *media = serde_json::Value::Array(vec![one; 51]);
                out.start_file("presentation.json", zip::write::SimpleFileOptions::default()).unwrap();
                out.write_all(&serde_json::to_vec(&v).unwrap()).unwrap();
            } else {
                out.raw_copy_file(f).unwrap();
            }
        }
        let bytes = out.finish().unwrap().into_inner();
        // 51 references to one 1000-byte member fit a 1000-byte budget: it's inflated once.
        let q = load_within(&bytes, Budget { total: 1000, items: 51 }).unwrap().0;
        assert_eq!(q.media.len(), 1);
        assert_eq!(q.media[0].data.len(), 1000);
        // And the deck saves again.
        assert!(save(&q).is_ok());
    }

    #[test]
    fn roundtrip_with_media() {
        let mut p = Presentation::default();
        let id = p.add_media("a.png", "image/png", vec![1, 2, 3, 4]).expect("media");
        let bytes = save(&p).unwrap();
        assert!(sniff(&bytes));
        let q = load(&bytes).unwrap();
        assert_eq!(q.slides.len(), p.slides.len());
        assert_eq!(q.media(id).map(|m| m.data.as_slice()), Some(&[1u8, 2, 3, 4][..]));
        assert_eq!(serde_json::to_value(&q.slides).unwrap(), serde_json::to_value(&p.slides).unwrap());
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(load(b"").is_err());
        assert!(load(b"PK\x03\x04garbage").is_err());
        assert!(load(&[0u8; 1000]).is_err());
        let mut b = save(&Presentation::default()).unwrap();
        let n = b.len();
        b.truncate(n / 2);
        assert!(load(&b).is_err());
    }

    /// A video's crop is brought into the same range as a picture's (#972).
    #[test]
    fn a_video_crop_is_repaired_like_a_picture_crop() {
        use deckcraft_model::{MediaClip, Shape};
        let mut p = Presentation::default();
        let clip = MediaClip { video: true, crop: [f64::NAN, 0.25, -1e9, 0.0], ..Default::default() };
        Arc::make_mut(&mut p.slides[0]).shapes.push(Shape { kind: ShapeKind::Media(clip), ..Default::default() });
        let back = load(&save(&p).unwrap()).unwrap_or_else(|e| panic!("{e}"));
        let ShapeKind::Media(m) = &back.slides[0].shapes.last().unwrap().kind else { panic!("media") };
        assert_eq!(m.crop, [0.0, 0.25, -10.0, 0.0]);
    }

    /// Non-finite numbers are written as `null` by serde_json; the file still opens, with each
    /// such number repaired (it failed as "invalid type: null, expected f64").
    #[test]
    fn a_file_saved_with_non_finite_numbers_reopens() {
        use deckcraft_model::{Shape, ShapeKind, Xfrm};
        let mut p = Presentation::default();
        let nan = f64::NAN;
        let inf = f64::INFINITY;
        let mut pic = Shape { xfrm: Some(Xfrm { x: nan, y: -inf, w: 1e300, h: -4.0, rot: 725.0, ..Default::default() }), ..Default::default() };
        pic.kind = ShapeKind::Picture { fill: deckcraft_model::style::PictureFill { crop: [nan, inf, 0.25, -1e9], ..Default::default() } };
        pic.line = Some(deckcraft_model::Line { width: Some(1e9), ..Default::default() });
        let mut text = Shape { xfrm: Some(Xfrm { rot: inf, w: 100.0, h: 50.0, ..Default::default() }), ..Default::default() };
        let mut body = deckcraft_model::text::TextBody::default();
        let mut para = deckcraft_model::text::Paragraph::default();
        para.runs.push(deckcraft_model::text::Run::with("x", deckcraft_model::text::RunProps { size: Some(-3.0), ..Default::default() }));
        para.end_props.size = Some(1e9);
        body.paragraphs.push(para);
        text.text = Some(body);
        let group = Shape {
            xfrm: Some(Xfrm { w: 10.0, h: 10.0, ..Default::default() }),
            kind: ShapeKind::Group { children: vec![pic, text], child: Xfrm { x: nan, w: inf, h: 10.0, ..Default::default() } },
            ..Default::default()
        };
        Arc::make_mut(&mut p.slides[0]).shapes.push(group);
        let bytes = save(&p).unwrap();
        let back = load(&bytes).unwrap_or_else(|e| panic!("{e}"));
        let g = back.slides[0].shapes.last().unwrap();
        let ShapeKind::Group { children, child } = &g.kind else { panic!("group") };
        assert_eq!((child.x, child.w), (0.0, 0.0));
        let x = children[0].xfrm.unwrap();
        assert_eq!((x.x, x.y, x.w, x.h, x.rot), (0.0, 0.0, MAX_COORD, 0.0, 5.0));
        let ShapeKind::Picture { fill } = &children[0].kind else { panic!("picture") };
        assert_eq!(fill.crop, [0.0, 0.0, 0.25, -10.0]);
        assert_eq!(children[0].line.as_ref().unwrap().width, Some(1584.0));
        assert_eq!(children[1].xfrm.unwrap().rot, 0.0);
        let para = &children[1].text.as_ref().unwrap().paragraphs[0];
        assert_eq!(para.runs[0].props.size, None);
        assert_eq!(para.end_props.size, Some(4000.0));
        // Saved again, it is clean.
        assert_eq!(load(&save(&back).unwrap()).unwrap(), back);
    }

    /// Files written before [`finite::Finite`] (plain serde_json: NaN as `null`) open too,
    /// including a `null` inside a tagged enum, where serde reports only the enclosing object.
    #[test]
    fn older_files_with_null_numbers_open() {
        use deckcraft_model::{Shape, ShapeKind, Xfrm};
        let nan = f64::NAN;
        for case in 0..3 {
            let mut p = Presentation::default();
            let sh = match case {
                0 => Shape { xfrm: Some(Xfrm { x: nan, rot: nan, w: 5.0, h: 5.0, ..Default::default() }), ..Default::default() },
                1 => Shape {
                    kind: ShapeKind::Group { children: vec![], child: Xfrm { x: nan, w: 1.0, h: 1.0, ..Default::default() } },
                    ..Default::default()
                },
                _ => Shape {
                    kind: ShapeKind::Picture { fill: deckcraft_model::style::PictureFill { crop: [0.1, nan, 0.0, 0.0], ..Default::default() } },
                    ..Default::default()
                },
            };
            Arc::make_mut(&mut p.slides[0]).shapes.push(sh);
            let m = Manifest { format: "deckcraft".into(), version: VERSION, generator: "old".into(), presentation: p };
            let json = serde_json::to_vec(&m).unwrap();
            assert!(json.windows(4).any(|w| w == b"null"));
            let back = parse_manifest(&json).unwrap_or_else(|e| panic!("case {case}: {e}"));
            assert_eq!(back.presentation.slides[0].shapes.len(), m.presentation.slides[0].shapes.len(), "case {case}");
        }
    }

    /// A body whose file had no `fontScale` (`<a:normAutofit/>`) keeps that state through the
    /// native format: it is drawn at 100 % and only fitted when its text changes (#724).
    #[test]
    fn a_bare_normautofit_body_round_trips() {
        use deckcraft_model::text::{AutoFit, TextBody};
        let mut p = Presentation::default();
        let mut body = TextBody::from_text("Overflowing");
        body.body.autofit = Some(AutoFit::Shrink { font_scale: None, line_reduction: 0.0 });
        let sh = deckcraft_model::Shape { text: Some(body), ..Default::default() };
        Arc::make_mut(&mut p.slides[0]).shapes.push(sh);
        let back = load(&save(&p).unwrap()).unwrap_or_else(|e| panic!("{e}"));
        let body = back.slides[0].shapes.last().unwrap().text.clone().unwrap();
        assert_eq!(body.body.autofit, Some(AutoFit::Shrink { font_scale: None, line_reduction: 0.0 }));
        // A scale the file saved round-trips as itself.
        let mut body = TextBody::from_text("Overflowing");
        body.body.autofit = Some(AutoFit::Shrink { font_scale: Some(0.62), line_reduction: 0.2 });
        let sh = deckcraft_model::Shape { text: Some(body), ..Default::default() };
        Arc::make_mut(&mut p.slides[0]).shapes.push(sh);
        let back = load(&save(&p).unwrap()).unwrap_or_else(|e| panic!("{e}"));
        let body = back.slides[0].shapes.last().unwrap().text.clone().unwrap();
        assert_eq!(body.body.autofit, Some(AutoFit::Shrink { font_scale: Some(0.62), line_reduction: 0.2 }));
    }

    /// Only `null`s where numbers belong are patched: other damage still fails.
    #[test]
    fn other_damage_is_still_reported() {
        let ok = serde_json::to_vec(&serde_json::json!({"format": "deckcraft", "version": 1, "generator": "t", "presentation": {}})).unwrap();
        assert!(parse_manifest(&ok).is_ok());
        let bad = br#"{"format": null, "version": 1, "generator": "t", "presentation": {}}"#;
        assert!(parse_manifest(bad).is_err());
        let truncated = &ok[..ok.len() - 3];
        assert!(parse_manifest(truncated).is_err());
        // A null number on a later line (pretty-printed by another tool) is found too.
        let pretty = b"{\n  \"format\": \"deckcraft\",\n  \"version\": 1,\n  \"generator\": \"t\",\n  \"presentation\": {\"slides\": [{\"shapes\": [{\"xfrm\": {\"x\": null, \"y\": 1, \"w\": 2, \"h\": 3}}]}]}\n}";
        let m = parse_manifest(pretty).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.presentation.slides[0].shapes[0].xfrm.unwrap().x, 0.0);
    }

    #[test]
    fn degenerate_slide_sizes_are_repaired() {
        use deckcraft_model::Size;
        let cases = [
            ((0.0, 0.0), (960.0, 540.0)),
            ((-5.0, 540.0), (960.0, 540.0)),
            ((f64::NAN, 540.0), (960.0, 540.0)),
            ((f64::INFINITY, 1.0), (960.0, 540.0)),
            ((1.0, 1.0), (72.0, 72.0)),
            ((0.5, 4000.0), (72.0, 4000.0)),
            ((1e9, 20.0), (4032.0, 72.0)),
            ((720.0, 540.0), (720.0, 540.0)),
        ];
        for ((w, h), want) in cases {
            let mut p = Presentation { slide_size: Size::new(w, h), notes_size: Size::new(0.0, f64::NAN), ..Default::default() };
            repair(&mut p);
            assert_eq!((p.slide_size.width, p.slide_size.height), want, "{w}x{h}");
            assert_eq!(p.notes_size, Presentation::default().notes_size);
        }
    }

    /// A file whose slide size is null, a string or missing a side (written by hand or by
    /// another tool; NaN is also saved as null) opens with the default size rather than failing
    /// as damaged.
    #[test]
    fn files_with_unreadable_slide_sizes_still_open() {
        let saved = save(&Presentation::default()).unwrap();
        let mut z = zip::ZipArchive::new(Cursor::new(&saved[..])).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&read_entry(&mut z, "presentation.json").unwrap()).unwrap();
        for size in [
            serde_json::json!({"width": null, "height": 540}),
            serde_json::json!({"width": "big", "height": 540}),
            serde_json::json!({"height": 0}),
            serde_json::json!(null),
            serde_json::json!(7),
        ] {
            let mut m = json.clone();
            m["presentation"]["slideSize"] = size.clone();
            let mut buf = Cursor::new(Vec::new());
            {
                let mut w = zip::ZipWriter::new(&mut buf);
                let o = zip::write::SimpleFileOptions::default();
                w.start_file("mimetype", o).unwrap();
                w.write_all(MIMETYPE.as_bytes()).unwrap();
                w.start_file("presentation.json", o).unwrap();
                w.write_all(&serde_json::to_vec(&m).unwrap()).unwrap();
                w.finish().unwrap();
            }
            let p = load(buf.get_ref()).unwrap_or_else(|e| panic!("{size}: {e}"));
            assert_eq!((p.slide_size.width, p.slide_size.height), (960.0, 540.0), "{size}");
        }
    }

    #[test]
    fn repair_fixes_missing_layout_and_size() {
        let mut p = Presentation::default();
        Arc::make_mut(&mut p.slides[0]).layout = deckcraft_model::LayoutId(99999);
        p.slide_size.width = f64::NAN;
        repair(&mut p);
        assert!(p.validate().is_empty());
        assert_eq!(p.slide_size.width, 960.0);
    }

    /// A duplicate slide in a deck whose ids are used up cannot be re-id'd: it stays as it is
    /// (the engine refuses edits to such a deck) instead of a duplicated fresh id (#407).
    #[test]
    fn repair_keeps_a_duplicate_it_cannot_reid() {
        let mut p = Presentation { next_id: u32::MAX, ..Default::default() };
        p.slides.push(p.slides[0].clone());
        repair(&mut p);
        assert_eq!(p.slides.len(), 2, "the duplicate stays visible");
        assert_eq!(p.slides[0].id, p.slides[1].id, "no fresh id was invented");
    }

    #[test]
    fn outline_roundtrip() {
        let mut p = defaults::blank_presentation(defaults::WIDE, Default::default(), false);
        let n = outline_to_slides(&mut p, "Intro\n\tPoint one\n\t\tSub point\nSecond\n\tMore");
        assert_eq!(n, 2);
        let o = slides_to_outline(&p);
        assert!(o.contains("Intro\n\tPoint one\n\t\tSub point\nSecond"), "{o}");
    }

    #[test]
    fn outline_keeps_leading_dash_text() {
        let mut p = defaults::blank_presentation(defaults::WIDE, Default::default(), false);
        let n = outline_to_slides(&mut p, "-5 °C tonight\n\t--verbose flag\n\t*args and **kwargs\n- Bullets\n\t- Point\n\t• Other");
        assert_eq!(n, 2);
        let o = slides_to_outline(&p);
        assert!(o.contains("-5 °C tonight\n\t--verbose flag\n\t*args and **kwargs\nBullets\n\tPoint\n\tOther"), "{o}");
    }

    #[test]
    fn outline_export_keeps_multi_paragraph_title_on_one_line() {
        let mut p = defaults::blank_presentation(defaults::WIDE, Default::default(), false);
        assert_eq!(outline_to_slides(&mut p, "Intro\n\tPoint one\nSecond\n\tMore"), 2);
        let title = Arc::make_mut(&mut p.slides[0]).shapes.iter_mut().find(|s| s.ph_type().is_some_and(|k| k.is_title())).expect("title placeholder");
        title.text = Some(TextBody::from_text("Line one\nLine two"));
        let o = slides_to_outline(&p);
        assert_eq!(o, "Line one Line two\n\tPoint one\nSecond\n\tMore\n");
        let mut q = defaults::blank_presentation(defaults::WIDE, Default::default(), false);
        assert_eq!(outline_to_slides(&mut q, &o), 2);
    }

    #[test]
    fn markdown_headings_and_list_items() {
        assert_eq!(md_heading("# Intro"), Some("Intro"));
        assert_eq!(md_heading("   ### Deep ###  "), Some("Deep"));
        assert_eq!(md_heading("## C# and F#"), Some("C# and F#"));
        assert_eq!(md_heading("#"), Some(""));
        assert_eq!(md_heading("## Ünïcödé"), Some("Ünïcödé"));
        for not in ["#5 priority", "####### seven", "    # code", "text", "", "é#"] {
            assert_eq!(md_heading(not), None, "{not:?}");
        }
        assert_eq!(md_list_item("- a"), Some((0, "a")));
        assert_eq!(md_list_item("    * b"), Some((2, "b")));
        assert_eq!(md_list_item("\t+ c"), Some((1, "c")));
        assert_eq!(md_list_item("12) d"), Some((0, "d")));
        for not in ["-x", "1.x", "1", "1234567890. long", "• bullet", "é. x", ""] {
            assert_eq!(md_list_item(not), None, "{not:?}");
        }
    }

    /// #814: slides_to_outline includes table cell text, which it used to skip.
    #[test]
    fn outline_includes_table_text() {
        use deckcraft_model::table::{Cell, Row, Table};
        use deckcraft_model::{Shape, ShapeKind, Xfrm};
        let mut p = defaults::new_presentation(None);
        let layout = defaults::layout_of_kind(&p, LayoutType::TitleAndContent).unwrap();
        let mut s = defaults::new_slide(&mut p, layout).expect("slide");
        s.shapes.retain(|sh| !sh.ph_type().is_some_and(|k| k.is_title()));
        let mut body = Shape { xfrm: Some(Xfrm::new(0.0, 0.0, 600.0, 100.0)), ..Default::default() };
        body.text = Some(TextBody::from_text("Text box body"));
        s.shapes.push(body);

        let mut table = Table { cols: vec![200.0, 200.0], ..Default::default() };
        for texts in [["Cell A1", "Cell B1"], ["Cell A2", "Cell B2"]] {
            let cells: Vec<Cell> = texts.iter().map(|t| Cell { text: TextBody::from_text(t), ..Default::default() }).collect();
            table.rows.push(Row { height: 30.0, cells });
        }
        let mut tbl = Shape { xfrm: Some(Xfrm::new(0.0, 100.0, 400.0, 60.0)), ..Default::default() };
        tbl.kind = ShapeKind::Table(table);
        s.shapes.push(tbl);
        p.slides = vec![std::sync::Arc::new(s)];

        let out = slides_to_outline(&p);
        assert!(out.contains("Text box body"), "{out}");
        assert!(out.contains("Cell A1\tCell B1"), "{out}");
        assert!(out.contains("Cell A2\tCell B2"), "{out}");
    }
}

#[test]
fn outline_table_rows_are_indented_and_do_not_become_slides() {
    use deckcraft_model::table::{Cell, Row, Table};
    use deckcraft_model::{Shape, ShapeKind, Xfrm};
    let mut p = defaults::new_presentation(None);
    let layout = defaults::layout_of_kind(&p, LayoutType::TitleAndContent).unwrap();
    let mut s = defaults::new_slide(&mut p, layout).expect("slide");
    for sh in s.shapes.iter_mut().filter(|sh| sh.ph_type().is_some_and(|k| k.is_title())) {
        sh.text = Some(TextBody::from_text("Slide title"));
    }
    let mut table = Table { cols: vec![200.0, 200.0], ..Default::default() };
    for texts in [["A1", "B1"], ["A2", "B2"]] {
        let cells: Vec<Cell> = texts.iter().map(|t| Cell { text: TextBody::from_text(t), ..Default::default() }).collect();
        table.rows.push(Row { height: 30.0, cells });
    }
    let mut tbl = Shape { xfrm: Some(Xfrm::new(0.0, 100.0, 400.0, 60.0)), ..Default::default() };
    tbl.kind = ShapeKind::Table(table);
    s.shapes.push(tbl);
    p.slides = vec![std::sync::Arc::new(s)];

    let out = slides_to_outline(&p);
    let mut q = defaults::new_presentation(None);
    q.slides.clear();
    assert_eq!(outline_to_slides(&mut q, &out), 1, "table rows became slides: {out:?}");
    assert_eq!(q.slides.len(), 1, "{out:?}");
    let body = q.slides[0].shapes.iter().filter_map(|sh| sh.text.as_ref()).map(|t| t.text()).collect::<Vec<_>>().join("\n");
    assert!(body.contains("A1\tB1"), "{body:?}");
    assert!(body.contains("A2\tB2"), "{body:?}");
}
