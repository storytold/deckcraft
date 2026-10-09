//! Text layout for slide text bodies.
//!
//! [`layout`] resolves every run's formatting through the placeholder/style chain and lays each
//! paragraph out in the order bidirectional text needs:
//! 1. embedding levels for the whole logical paragraph (UAX #9, base level from `a:pPr/@rtl`);
//! 2. itemization by level, script, face (per-cluster fallback through the `a:latin`/`a:ea`/`a:cs`
//!    slots and the theme's script fonts) and shaping style, each item shaped with the whole
//!    paragraph as context; paint-only attributes (colour, highlight, underline) never split a
//!    word, so Arabic joining and ligatures survive them;
//! 3. line breaking in logical order (UAX #14 opportunities at cluster boundaries, greedy fill);
//! 4. per line, rules L1/L2 reorder the characters visually (L4 mirroring is done by the shaper).
//!
//! It then places bullets and numbers from the paragraph's start edge (the right edge for RTL
//! paragraphs), applies indents, spacing and alignment (kashida for `justLow`), anchors the block
//! in the shape's text box, and shrinks text on overflow when the body asks for it. The result is
//! positioned glyph runs in shape-local coordinates plus per-line caret geometry for editing:
//! bidirectional carets with affinity, logical and visual movement, and selection as the union of
//! visual boxes.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod bidi;
pub mod datetime;

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_color::Rgba;
use deckcraft_fonts::script::{self, joins_following, joins_preceding};
use deckcraft_fonts::{FontDb, FontFace, FontSlot, ScriptTag, ShapeParams};
use deckcraft_geom::{Point, Rect};
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::{Align, Anchor, AutoFit, BodyProps, Bullet, Caps, ParaProps, RunKind, RunProps, Spacing, Strike, TextBody, TextDir};
use deckcraft_model::{Fill, Shape};
use unicode_segmentation::UnicodeSegmentation;

/// A run of glyphs in one face, size and colour.
#[derive(Clone)]
pub struct GlyphRun {
    pub face: Arc<FontFace>,
    /// Font size in points (after autofit scaling).
    pub size: f64,
    pub color: Rgba,
    /// Glyph id and pen position of its origin (baseline), in shape-local points.
    pub glyphs: Vec<(u32, f64, f64)>,
    /// Per glyph: the paragraph character it draws (index, char); empty for bullets.
    pub cells: Vec<(usize, char)>,
    /// Draw a heavier stroke because the family has no bold face.
    pub fake_bold: bool,
    /// Slant because the family has no italic face.
    pub fake_italic: bool,
    pub outline: Option<(Rgba, f64)>,
    /// Paragraph and character range this run covers (for hyperlinks, selection colours).
    pub para: usize,
    pub chars: (usize, usize),
    pub link: bool,
    /// Alpha of the fill (0–1).
    pub alpha: f64,
    pub gradient: Option<deckcraft_model::style::Gradient>,
}

impl std::fmt::Debug for GlyphRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlyphRun").field("family", &self.face.family).field("size", &self.size).field("glyphs", &self.glyphs.len()).finish()
    }
}

/// An underline, strikethrough or highlight box.
#[derive(Clone, Debug)]
pub struct Deco {
    pub rect: Rect,
    pub color: Rgba,
    /// Highlights draw behind text.
    pub behind: bool,
    /// Paragraph the decoration belongs to (by-paragraph animation).
    pub para: usize,
}

#[derive(Clone, Debug, Default)]
pub struct LineInfo {
    pub para: usize,
    /// Character range [start, end) in the paragraph (end excludes the line's break/newline).
    pub start: usize,
    pub end: usize,
    pub top: f64,
    pub baseline: f64,
    pub bottom: f64,
    /// Caret x for each character boundary start..=end (len = end - start + 1), downstream: the
    /// leading edge of the character after the boundary (its right edge if it is right-to-left);
    /// the last entry is the trailing edge of the line's last character.
    pub caret_x: Vec<f64>,
    /// Column index (multi-column bodies).
    pub column: usize,
    /// Right-to-left paragraph.
    pub rtl: bool,
    /// Bidi level of each character start..end after rule L1 (odd = right-to-left).
    pub levels: Vec<u8>,
    /// Visual box (left, right) of each character start..end. Characters inside a grapheme have
    /// empty boxes; a ligature's characters share its width (GDEF ligature carets).
    pub edges: Vec<(f64, f64)>,
    /// Whether a caret may stand before each character start..end (grapheme boundaries).
    pub boundary: Vec<bool>,
    /// Visual extent (left, right) of the line's text.
    pub extent: (f64, f64),
    /// Distance from the paragraph's start edge to the line's text (indent, margin, bullet).
    pub start_offset: f64,
}

#[derive(Clone, Debug, Default)]
pub struct TextLayout {
    pub runs: Vec<GlyphRun>,
    pub decos: Vec<Deco>,
    pub lines: Vec<LineInfo>,
    /// Inner text box (insets applied), shape-local.
    pub inner: Rect,
    /// Height of the laid-out text (before anchoring).
    pub content_height: f64,
    /// Widest line.
    pub content_width: f64,
    /// Autofit font scale used (1 = none).
    pub font_scale: f64,
    pub line_reduction: f64,
    pub overflow: bool,
    /// Text rotation about the shape centre, degrees (vertical text = 90/270).
    pub rotation: f64,
    /// Per paragraph: (first line index, line count).
    pub para_lines: Vec<(usize, usize)>,
    /// Per paragraph: whether a caret may stand at each character offset 0..=len (grapheme
    /// boundaries), for logical caret movement.
    pub boundaries: Vec<Vec<bool>>,
}

/// Values for fields (`slidenum`, `datetime…`, `footer`). `None` keeps the field's saved text.
pub trait Fields {
    fn field(&self, kind: &str) -> Option<String>;
    /// The value of field `kind` in a run of language `lang` (BCP 47, e.g. `en-US`): dates are
    /// written the language's way.
    fn field_in(&self, kind: &str, lang: Option<&str>) -> Option<String> {
        let _ = lang;
        self.field(kind)
    }
}

pub struct NoFields;
impl Fields for NoFields {
    fn field(&self, kind: &str) -> Option<String> {
        if kind == "slidenum" { Some("1".into()) } else { None }
    }
}

pub struct Opts<'a> {
    /// The text box: the preset's text rectangle in shape-local space.
    pub rect: Rect,
    pub fields: &'a dyn Fields,
    /// Draw this text instead of the body (placeholder prompts), in this colour.
    pub prompt_color: Option<Rgba>,
    /// Ignore autofit shrinking (used to measure natural height).
    pub no_shrink: bool,
}

struct Style {
    face: Arc<FontFace>,
    size: f64,
    color: Rgba,
    alpha: f64,
    fake_bold: bool,
    fake_italic: bool,
    baseline: f64,
    spacing: f64,
    caps: Caps,
    underline: Option<Rgba>,
    strike: Option<Strike>,
    highlight: Option<Rgba>,
    outline: Option<(Rgba, f64)>,
    link: bool,
    gradient: Option<deckcraft_model::style::Gradient>,
    /// "Regular", "Bold"… for the slot and fallback faces.
    style_name: &'static str,
    /// BCP 47 language (`a:rPr/@lang`), for shaping.
    lang: Option<String>,
    /// The `a:ea` and `a:cs` families (theme references resolved), if the document names one.
    ea: Option<String>,
    cs: Option<String>,
    /// Heading font (`+mj-…`): the theme's major font set applies.
    major: bool,
}

/// One character of a paragraph after shaping.
#[derive(Clone)]
struct Cell {
    ch: char,
    style: usize,
    face: usize,
    /// Resolved bidi embedding level (before the line rules).
    level: u8,
    /// Glyphs of the shaping cluster this cell heads, from the cluster's visual left edge (gid,
    /// x, y). Empty for the other cells of a cluster.
    glyphs: Vec<(u32, f64, f64)>,
    /// This character's advance: its share of the cluster (ligature carets); 0 inside a grapheme.
    adv: f64,
    /// Index of the cell heading this character's shaping cluster.
    head: usize,
    /// A grapheme starts here: carets stand and lines break only before such cells.
    boundary: bool,
}

impl Cell {
    /// A line may start before this cell: a grapheme that starts a shaping cluster.
    fn breakable(&self, i: usize) -> bool {
        self.boundary && self.head == i
    }
}

struct ParaShaped {
    cells: Vec<Cell>,
    styles: Vec<Style>,
    faces: Vec<Arc<FontFace>>,
    props: ParaProps,
    /// Size of the paragraph's first run (spacing percentages, empty lines).
    lead_size: f64,
    lead_face: Arc<FontFace>,
    bullet: Option<(String, Style)>,
    #[allow(dead_code)]
    level: u8,
    /// Right-to-left paragraph (`a:pPr/@rtl`).
    rtl: bool,
}

fn style_name(bold: bool, italic: bool) -> &'static str {
    match (bold, italic) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (false, false) => "Regular",
    }
}

fn make_style(ctx: &Ctx, r: &RunProps, scale: f64, prompt: Option<Rgba>) -> Style {
    let family = resolve::font_family(ctx, r);
    let bold = r.bold.unwrap_or(false);
    let italic = r.italic.unwrap_or(false);
    let face = FontDb::global().face(&family, style_name(bold, italic));
    let fake_bold = bold && face.weight < 550.0;
    let fake_italic = italic && !face.italic;
    let size = (r.size.unwrap_or(18.0) * scale).clamp(0.5, 4000.0);
    let (mut color, alpha, gradient) = match &r.fill {
        Some(Fill::Solid { color }) => (ctx.color(color, None), color.alpha(), None),
        Some(Fill::Gradient(g)) => (g.stops.first().map(|s| ctx.color(&s.color, None)).unwrap_or(Rgba::BLACK), 1.0, Some(g.clone())),
        Some(Fill::None) => (Rgba::TRANSPARENT, 0.0, None),
        _ => (resolve::text_color(ctx, r), 1.0, None),
    };
    if let Some(p) = prompt {
        color = p;
    }
    let major = r.font.as_deref().is_some_and(|f| f.starts_with("+mj"));
    let theme = &ctx.master.theme;
    let slot_family = |explicit: &Option<String>, theme_ref: &str| {
        let name = theme.font(explicit.as_deref().unwrap_or(theme_ref));
        (!name.trim().is_empty()).then_some(name)
    };
    let ea = slot_family(&r.font_ea, if major { "+mj-ea" } else { "+mn-ea" });
    let cs = slot_family(&r.font_cs, if major { "+mj-cs" } else { "+mn-cs" });
    let underline = r.underline.as_deref().filter(|u| *u != "none").map(|_| r.underline_color.as_ref().map(|c| ctx.color(c, None)).unwrap_or(color));
    let outline = r.outline.as_ref().and_then(|l| match &l.fill {
        Some(Fill::Solid { color: c }) => Some((ctx.color(c, None), l.width.unwrap_or(0.75))),
        _ => None,
    });
    Style {
        face,
        size,
        color,
        alpha,
        fake_bold,
        fake_italic,
        baseline: r.baseline.unwrap_or(0.0),
        spacing: r.spacing.unwrap_or(0.0) * scale,
        caps: r.caps.unwrap_or(Caps::None),
        underline,
        strike: r.strike.filter(|s| *s != Strike::None),
        highlight: r.highlight.as_ref().map(|c| ctx.color(c, None)),
        outline,
        link: r.link.is_some(),
        gradient,
        style_name: style_name(bold, italic),
        lang: r.lang.clone().filter(|l| !l.is_empty()),
        ea,
        cs,
        major,
    }
}

/// The faces to try for a cluster of `slot`/`script` in style `st`, in order: the slot's
/// typeface, the theme's font for the script (`a:font[@script]`), then the run's Latin face.
fn face_chain(ctx: &Ctx, st: &Style, slot: FontSlot, script: Option<ScriptTag>) -> Vec<Arc<FontFace>> {
    let db = FontDb::global();
    let mut chain: Vec<Arc<FontFace>> = Vec::with_capacity(3);
    let mut push = |family: &str| {
        // Only families that exist: `FontDb::face` would substitute the default face.
        if db.has_family(family) {
            let f = db.face(family, st.style_name);
            if !chain.iter().any(|c| Arc::ptr_eq(c, &f)) {
                chain.push(f);
            }
        }
    };
    match slot {
        FontSlot::EastAsian => st.ea.as_deref().into_iter().for_each(&mut push),
        FontSlot::ComplexScript => st.cs.as_deref().into_iter().for_each(&mut push),
        FontSlot::Latin => {}
    }
    if let Some(sc) = script.and_then(|t| std::str::from_utf8(&t).ok().map(String::from)) {
        let set = if st.major { &ctx.master.theme.fonts.major } else { &ctx.master.theme.fonts.minor };
        if let Some(f) = set.script(&sc) {
            push(f);
        }
    }
    if !chain.iter().any(|c| Arc::ptr_eq(c, &st.face)) {
        chain.push(st.face.clone());
    }
    chain
}

fn spacing_pts(s: Option<Spacing>, size: f64, reduce: f64) -> f64 {
    match s {
        Some(Spacing::Pct(p)) => p * size * 1.2 * (1.0 - reduce),
        Some(Spacing::Pts(v)) => v * (1.0 - reduce),
        None => 0.0,
    }
}

fn roman(mut n: u32, upper: bool) -> String {
    let table = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut s = String::new();
    if n == 0 || n > 3999 {
        return n.to_string();
    }
    for (v, r) in table {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    if upper { s.to_uppercase() } else { s }
}

fn alpha_num(n: u32, upper: bool) -> String {
    if n == 0 {
        return String::new();
    }
    let mut n = n - 1;
    let c = (b'a' + (n % 26) as u8) as char;
    let reps = n / 26 + 1;
    n = reps;
    let s: String = std::iter::repeat_n(c, n as usize).collect();
    if upper { s.to_uppercase() } else { s }
}

/// Text of an auto-number bullet: `arabicPeriod` 3 → "3.".
pub fn autonum(scheme: &str, n: u32) -> String {
    let (core, upper) = if scheme.starts_with("romanUc") {
        (roman(n, true), true)
    } else if scheme.starts_with("romanLc") {
        (roman(n, false), false)
    } else if scheme.starts_with("alphaUc") {
        (alpha_num(n, true), true)
    } else if scheme.starts_with("alphaLc") {
        (alpha_num(n, false), false)
    } else if scheme.starts_with("circleNum") {
        let c = char::from_u32(0x2460 + n.saturating_sub(1).min(19)).unwrap_or('•');
        return c.to_string();
    } else {
        (n.to_string(), false)
    };
    let _ = upper;
    if scheme.ends_with("ParenBoth") {
        format!("({core})")
    } else if scheme.ends_with("ParenR") {
        format!("{core})")
    } else if scheme.ends_with("Period") {
        format!("{core}.")
    } else if scheme.ends_with("Minus") {
        format!("- {core} -")
    } else {
        core
    }
}

fn shape_para(ctx: &Ctx, shape: &Shape, body: &TextBody, pi: usize, scale: f64, opts: &Opts, number: Option<u32>) -> ParaShaped {
    let db = FontDb::global();
    let Some(para) = body.paragraphs.get(pi) else {
        let face = db.face("Inter", "Regular");
        return ParaShaped {
            cells: vec![],
            styles: vec![],
            faces: vec![],
            props: ParaProps::default(),
            lead_size: 18.0,
            lead_face: face,
            bullet: None,
            level: 0,
            rtl: false,
        };
    };
    let props = resolve::para(ctx, shape, para);
    let rtl = props.rtl.unwrap_or(false);
    let mut styles: Vec<Style> = vec![];
    let mut faces: Vec<Arc<FontFace>> = vec![];
    // The logical paragraph: its text and each character's style.
    let mut text = String::new();
    let mut char_style: Vec<usize> = vec![];
    for run in &para.runs {
        let rp = resolve::run(ctx, shape, para, &run.props);
        let st = make_style(ctx, &rp, scale, opts.prompt_color);
        let t: String = match &run.kind {
            RunKind::Text => run.text.clone(),
            RunKind::Break => "\u{b}".into(),
            RunKind::Field { field } => opts.fields.field_in(field, rp.lang.as_deref()).unwrap_or_else(|| run.text.clone()),
            RunKind::Math { .. } => run.text.clone(),
        };
        let si = styles.len();
        styles.push(st);
        char_style.extend(std::iter::repeat_n(si, t.chars().count()));
        text.push_str(&t);
    }
    let cells = shape_cells(ctx, &text, &char_style, &styles, &mut faces, rtl);
    let lead = para.runs.first().map(|r| resolve::run(ctx, shape, para, &r.props)).unwrap_or_else(|| resolve::run(ctx, shape, para, &para.end_props));
    let lead_style = make_style(ctx, &lead, scale, opts.prompt_color);
    let lead_size = lead_style.size;
    let lead_face = lead_style.face.clone();
    // Bullet.
    let bullet = if para.is_empty() && opts.prompt_color.is_none() {
        None
    } else {
        match &props.bullet {
            Some(Bullet::Char { char }) => {
                let mut rp = lead.clone();
                if let Some(f) = &props.bullet_font {
                    rp.font = Some(f.clone());
                }
                rp.bold = Some(false);
                rp.italic = Some(false);
                if let Some(c) = &props.bullet_color {
                    rp.fill = Some(Fill::solid(c.clone()));
                }
                rp.size = Some(lead.size.unwrap_or(18.0) * props.bullet_size.unwrap_or(1.0));
                rp.underline = None;
                rp.strike = None;
                rp.highlight = None;
                let mut st = make_style(ctx, &rp, scale, opts.prompt_color);
                st.baseline = 0.0;
                // Symbol fonts (Wingdings) aren't available: map common private-use bullets.
                let ch = map_symbol_bullet(char, props.bullet_font.as_deref());
                if !st.face.covers(ch.chars().next().unwrap_or('•')) {
                    st.face = db.face("Inter", "Regular");
                }
                Some((ch, st))
            }
            Some(Bullet::AutoNum { scheme, start_at }) => {
                let mut rp = lead.clone();
                if let Some(c) = &props.bullet_color {
                    rp.fill = Some(Fill::solid(c.clone()));
                }
                rp.size = Some(lead.size.unwrap_or(18.0) * props.bullet_size.unwrap_or(1.0));
                rp.underline = None;
                let st = make_style(ctx, &rp, scale, opts.prompt_color);
                Some((autonum(scheme, start_at.saturating_sub(1).saturating_add(number.unwrap_or(1))), st))
            }
            _ => None,
        }
    };
    ParaShaped { cells, styles, faces, props, lead_size, lead_face, bullet, level: para.level, rtl }
}

/// Shape a logical paragraph into cells: bidi levels, per-grapheme faces, items of one level,
/// script, face and shaping style, each shaped with the whole paragraph as context.
fn shape_cells(ctx: &Ctx, text: &str, char_style: &[usize], styles: &[Style], faces: &mut Vec<Arc<FontFace>>, rtl: bool) -> Vec<Cell> {
    let db = FontDb::global();
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    // Byte offset of each character (and of the end).
    let mut byte_of: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
    byte_of.push(text.len());
    let char_at_byte: HashMap<usize, usize> = byte_of.iter().enumerate().map(|(i, b)| (*b, i)).collect();
    let levels = bidi::levels(text, &chars, rtl);
    let scripts = script::resolve(&chars);
    let mut boundary = vec![false; n];
    let mut graphemes: Vec<(usize, usize)> = Vec::new();
    for (b, g) in text.grapheme_indices(true) {
        let Some(&i) = char_at_byte.get(&b) else { continue };
        if let Some(x) = boundary.get_mut(i) {
            *x = true;
        }
        graphemes.push((i, i + g.chars().count()));
    }
    let face_index = |faces: &mut Vec<Arc<FontFace>>, f: &Arc<FontFace>| -> usize {
        if let Some(i) = faces.iter().position(|x| Arc::ptr_eq(x, f)) {
            return i;
        }
        faces.push(f.clone());
        faces.len() - 1
    };
    // A face per grapheme cluster (never split between faces).
    let mut char_face = vec![0usize; n];
    let mut chains: HashMap<(usize, FontSlot, Option<ScriptTag>), Vec<Arc<FontFace>>> = HashMap::new();
    let mut prev: Option<Arc<FontFace>> = None;
    for &(a, b) in &graphemes {
        let Some(cluster) = chars.get(a..b) else { continue };
        let si = char_style.get(a).copied().unwrap_or(0);
        let Some(st) = styles.get(si) else { continue };
        let (sc, slot) = scripts.get(a).copied().unwrap_or((None, FontSlot::Latin));
        // Spaces, digits and punctuation stay in the face of the text before them when it can
        // draw them, so a phrase isn't cut into items at every space.
        let neutral = cluster.iter().all(|c| script::script_of(*c).is_none());
        let face = match prev.as_ref() {
            Some(p) if neutral && deckcraft_fonts::covers_cluster(p, cluster) && char_style.get(a.wrapping_sub(1)) == Some(&si) => p.clone(),
            _ => {
                let chain = chains.entry((si, slot, sc)).or_insert_with(|| face_chain(ctx, st, slot, sc));
                db.cascade(cluster, chain, st.style_name)
            }
        };
        let fi = face_index(faces, &face);
        for f in char_face.get_mut(a..b).into_iter().flatten() {
            *f = fi;
        }
        prev = Some(face);
    }
    let mut cells: Vec<Cell> = (0..n)
        .map(|i| Cell {
            ch: chars.get(i).copied().unwrap_or(' '),
            style: char_style.get(i).copied().unwrap_or(0),
            face: char_face.get(i).copied().unwrap_or(0),
            level: levels.get(i).copied().unwrap_or(0),
            glyphs: vec![],
            adv: 0.0,
            head: i,
            boundary: boundary.get(i).copied().unwrap_or(true),
        })
        .collect();
    // Items: maximal ranges of one level, face, script and shaping style (case mapping, language).
    let upper = |i: usize| styles.get(char_style.get(i).copied().unwrap_or(0)).is_some_and(|st| st.caps != Caps::None);
    let lang = |i: usize| styles.get(char_style.get(i).copied().unwrap_or(0)).and_then(|st| st.lang.as_deref());
    let key = |i: usize| (levels.get(i).copied(), char_face.get(i).copied(), scripts.get(i).map(|s| s.0), upper(i), lang(i));
    let mut a = 0;
    while a < n {
        let mut b = a + 1;
        while b < n && key(b) == key(a) {
            b += 1;
        }
        shape_item(&mut cells, styles, faces, text, &byte_of, &char_at_byte, a..b, scripts.get(a).and_then(|s| s.0), lang(a), upper(a));
        a = b;
    }
    cells
}

/// Shape cells `range` (one item) and hang each cluster's glyphs on its first cell, sharing the
/// cluster's advance among its graphemes (by the ligature's GDEF carets when it has them).
#[allow(clippy::too_many_arguments)]
fn shape_item(
    cells: &mut [Cell],
    styles: &[Style],
    faces: &[Arc<FontFace>],
    text: &str,
    byte_of: &[usize],
    char_at_byte: &HashMap<usize, usize>,
    range: std::ops::Range<usize>,
    script: Option<ScriptTag>,
    lang: Option<&str>,
    upper: bool,
) {
    let Some(first) = cells.get(range.start) else { return };
    let (fi, level) = (first.face, first.level);
    let Some(face) = faces.get(fi) else { return };
    let rtl = level % 2 == 1;
    let (Some(&b0), Some(&b1)) = (byte_of.get(range.start), byte_of.get(range.end)) else { return };
    let params = ShapeParams { features: &[], rtl, script, language: lang };
    let glyphs = deckcraft_fonts::shape_range(face, text, b0..b1, &params, |ch| if upper { ch.to_uppercase().next().unwrap_or(ch) } else { ch });
    let upem = face.upem.max(1.0);
    let scale_of = |cell: &Cell| {
        let st = styles.get(cell.style);
        let size = st.map(|s| s.size).unwrap_or(18.0);
        let small = st.is_some_and(|s| s.caps == Caps::Small) && cell.ch.is_lowercase();
        (if small { size * 0.8 } else { size }) / upem
    };
    // Cluster heads in this item, ascending; each cluster spans to the next head.
    let mut heads: Vec<usize> = glyphs.iter().filter_map(|g| char_at_byte.get(&g.cluster).copied()).filter(|h| range.contains(h)).collect();
    heads.sort_unstable();
    heads.dedup();
    let mut width: HashMap<usize, f64> = HashMap::new();
    let mut lig: HashMap<usize, (u32, usize)> = HashMap::new();
    for g in &glyphs {
        let Some(&h) = char_at_byte.get(&g.cluster) else { continue };
        let Some(cell) = cells.get_mut(h) else { continue };
        if matches!(cell.ch, '\t' | '\u{b}' | '\n') {
            continue;
        }
        let s = scale_of(cell);
        let w = width.entry(h).or_insert(0.0);
        cell.glyphs.push((g.gid, *w + g.x_offset as f64 * s, -(g.y_offset as f64) * s));
        *w += g.x_advance as f64 * s;
        let e = lig.entry(h).or_insert((g.gid, 0));
        e.1 += 1;
    }
    for (hi, &h) in heads.iter().enumerate() {
        let end = heads.get(hi + 1).copied().unwrap_or(range.end).min(range.end);
        let w = width.get(&h).copied().unwrap_or(0.0);
        for c in cells.get_mut(h..end).into_iter().flatten() {
            c.head = h;
        }
        let stops: Vec<usize> = (h..end).filter(|&i| i == h || cells.get(i).is_some_and(|c| c.boundary)).collect();
        let m = stops.len().max(1);
        // Visual widths of the cluster's graphemes, left to right.
        let mut bounds = vec![0.0];
        if m > 1
            && let Some(&(gid, 1)) = lig.get(&h)
        {
            let s = cells.get(h).map(scale_of).unwrap_or(0.0);
            let carets = deckcraft_fonts::ligature_carets(face, gid);
            if carets.len() + 1 >= m {
                bounds.extend(carets.iter().take(m - 1).map(|c| (*c as f64 * s).clamp(0.0, w)));
            }
        }
        if bounds.len() != m {
            bounds = (0..m).map(|j| w * j as f64 / m as f64).collect();
        }
        bounds.push(w);
        for (j, &i) in stops.iter().enumerate() {
            let vj = if rtl { m - 1 - j } else { j };
            let share = bounds.get(vj + 1).copied().unwrap_or(w) - bounds.get(vj).copied().unwrap_or(0.0);
            if let Some(c) = cells.get_mut(i) {
                c.adv = share.max(0.0);
            }
        }
    }
    // Tracking (character spacing) after every drawn grapheme and space.
    for i in range {
        let Some(c) = cells.get(i) else { continue };
        let drawn = cells.get(c.head).is_some_and(|hd| !hd.glyphs.is_empty());
        if c.boundary
            && (drawn || c.ch == ' ')
            && let Some(sp) = styles.get(c.style).map(|s| s.spacing)
            && let Some(c) = cells.get_mut(i)
        {
            c.adv += sp;
        }
    }
}

fn map_symbol_bullet(c: &str, font: Option<&str>) -> String {
    let f = font.unwrap_or("").to_ascii_lowercase();
    let first = c.chars().next().unwrap_or('•');
    if f.contains("wingdings") || f.contains("symbol") || ('\u{f000}'..='\u{f0ff}').contains(&first) {
        let code = (first as u32) & 0xff;
        let m = match code {
            0xa7 => '▪',
            0xd8 => '➢',
            0xfc => '✓',
            0x76 => '❖',
            0x71 => '❑',
            0x6e => '■',
            0x6c => '●',
            0xa8 => '◆',
            0xb7 => '•',
            0x2d => '–',
            _ => '•',
        };
        return m.to_string();
    }
    c.to_string()
}

fn measure_str(face: &FontFace, size: f64, s: &str) -> (f64, Vec<(u32, f64)>) {
    let g = deckcraft_fonts::shape(face, s, &[], |c| c);
    let k = size / face.upem.max(1.0);
    let mut x = 0.0;
    let mut out = vec![];
    for gl in g {
        out.push((gl.gid, x + gl.x_offset as f64 * k));
        x += gl.x_advance as f64 * k;
    }
    (x, out)
}

fn line_metrics(face: &FontFace, size: f64) -> (f64, f64) {
    let k = size / face.upem.max(1.0);
    (face.ascent * k, face.descent * k)
}

/// Break opportunities: allowed[i] = a line may start at char i.
fn break_opportunities(text: &str) -> (Vec<bool>, Vec<bool>) {
    let n = text.chars().count();
    let mut allowed = vec![false; n + 1];
    let mut mandatory = vec![false; n + 1];
    let mut byte_to_char = std::collections::HashMap::new();
    for (ci, (b, _)) in text.char_indices().enumerate() {
        byte_to_char.insert(b, ci);
    }
    byte_to_char.insert(text.len(), n);
    for (b, op) in unicode_linebreak::linebreaks(text) {
        if let Some(&ci) = byte_to_char.get(&b) {
            if let Some(a) = allowed.get_mut(ci) {
                *a = true;
            }
            if op == unicode_linebreak::BreakOpportunity::Mandatory
                && let Some(m) = mandatory.get_mut(ci)
            {
                *m = true;
            }
        }
    }
    (allowed, mandatory)
}

struct RawLine {
    start: usize,
    end: usize,
    /// End including trailing spaces / the break char consumed.
    next: usize,
}

fn break_lines(
    p: &ParaShaped,
    first_w: f64,
    rest_w: f64,
    wrap: bool,
    tab_stops: &[f64],
    default_tab: f64,
    x0_first: f64,
    x0_rest: f64,
) -> Vec<RawLine> {
    let text: String = p.cells.iter().map(|c| c.ch).collect();
    let (allowed, _) = break_opportunities(&text);
    let n = p.cells.len();
    let mut lines = vec![];
    let mut start = 0;
    if n == 0 {
        return vec![RawLine { start: 0, end: 0, next: 0 }];
    }
    while start < n {
        let avail = if lines.is_empty() { first_w } else { rest_w };
        let x0 = if lines.is_empty() { x0_first } else { x0_rest };
        let mut x = 0.0;
        let mut last_break: Option<usize> = None;
        let mut i = start;
        let mut end = n;
        let mut next = n;
        while i < n {
            let c = p.cells.get(i).map(|c| c.ch).unwrap_or(' ');
            if c == '\u{b}' || c == '\n' {
                end = i;
                next = i + 1;
                break;
            }
            let adv = if c == '\t' { tab_advance(x0 + x, tab_stops, default_tab) } else { p.cells.get(i).map(|c| c.adv).unwrap_or(0.0) };
            let breakable = |j: usize| p.cells.get(j).is_none_or(|c| c.breakable(j));
            if i > start && allowed.get(i).copied().unwrap_or(false) && breakable(i) {
                last_break = Some(i);
            }
            if wrap && x + adv > avail + 0.01 && !c.is_whitespace() && i > start {
                // Break at the last opportunity, or force mid-word (between clusters).
                let b = last_break
                    .filter(|b| *b > start)
                    .unwrap_or_else(|| (start + 1..=i).rev().find(|&j| breakable(j)).or_else(|| (i + 1..n).find(|&j| breakable(j))).unwrap_or(n));
                end = b;
                next = b;
                // Trailing spaces belong to the line but don't count.
                while end > start && p.cells.get(end - 1).is_some_and(|c| c.ch == ' ') {
                    end -= 1;
                }
                break;
            }
            x += adv;
            i += 1;
        }
        if i >= n {
            end = n;
            next = n;
        }
        lines.push(RawLine { start, end, next });
        if next <= start {
            // No progress: force one char.
            let last = lines.len() - 1;
            let to = (start + 1..n).find(|&j| p.cells.get(j).is_none_or(|c| c.breakable(j))).unwrap_or(n);
            if let Some(l) = lines.get_mut(last) {
                l.end = to;
                l.next = to;
            }
            start = to;
        } else {
            start = next;
        }
        if next == n && p.cells.last().is_some_and(|c| c.ch == '\u{b}') && start >= n {
            // A trailing line break opens an empty last line.
            lines.push(RawLine { start: n, end: n, next: n });
        }
    }
    lines
}

fn tab_advance(x: f64, stops: &[f64], default_tab: f64) -> f64 {
    for s in stops {
        if *s > x + 0.5 {
            return s - x;
        }
    }
    let d = if default_tab > 1.0 { default_tab } else { 72.0 };
    let next = ((x / d).floor() + 1.0) * d;
    (next - x).max(1.0)
}

/// Lay out a shape's text body.
pub fn layout(ctx: &Ctx, shape: &Shape, body: &TextBody, opts: &Opts) -> TextLayout {
    let bp = resolve::body(ctx, shape);
    let autofit = bp.autofit.unwrap_or(AutoFit::None);
    let mut l = layout_scaled(ctx, shape, body, &bp, opts, 1.0, 0.0);
    if let (AutoFit::Shrink { .. }, false) = (autofit, opts.no_shrink)
        && l.overflow
    {
        // Binary search the largest scale that fits (steps like 90%, 80%… with line reduction).
        let (mut lo, mut hi) = (0.1_f64, 1.0_f64);
        let mut best = None;
        for _ in 0..12 {
            let mid = (lo + hi) / 2.0;
            let red: f64 = ((1.0 - mid) * 0.8_f64).min(0.2);
            let t = layout_scaled(ctx, shape, body, &bp, opts, mid, red);
            if t.overflow {
                hi = mid;
            } else {
                lo = mid;
                best = Some(t);
            }
        }
        l = best.unwrap_or_else(|| layout_scaled(ctx, shape, body, &bp, opts, 0.1, 0.2));
    }
    l
}

fn layout_scaled(ctx: &Ctx, shape: &Shape, body: &TextBody, bp: &BodyProps, opts: &Opts, scale: f64, reduce: f64) -> TextLayout {
    let r = opts.rect;
    let vert = bp.vert.unwrap_or(TextDir::Horizontal);
    let rotation = match vert {
        TextDir::Vertical | TextDir::EaVertical | TextDir::Stacked => 90.0,
        TextDir::Vertical270 => 270.0,
        TextDir::Horizontal => 0.0,
    } + bp.rot.unwrap_or(0.0);
    // For vertical text, lay out in a box with swapped dimensions centred on the text rect.
    let r = if (rotation - 90.0).abs() < 1e-6 || (rotation - 270.0).abs() < 1e-6 {
        let c = r.center();
        Rect::new(c.x - r.height() / 2.0, c.y - r.width() / 2.0, c.x + r.height() / 2.0, c.y + r.width() / 2.0)
    } else {
        r
    };
    let inner = Rect::new(
        r.x0 + bp.inset_l.unwrap_or(7.2),
        r.y0 + bp.inset_t.unwrap_or(3.6),
        (r.x1 - bp.inset_r.unwrap_or(7.2)).max(r.x0 + bp.inset_l.unwrap_or(7.2)),
        (r.y1 - bp.inset_b.unwrap_or(3.6)).max(r.y0 + bp.inset_t.unwrap_or(3.6)),
    );
    let wrap = bp.wrap.unwrap_or(true) && !matches!(bp.autofit, Some(AutoFit::Shape) if shape.text_box && !bp.wrap.unwrap_or(true));
    let ncols = bp.columns.unwrap_or(1).clamp(1, 16) as usize;
    let col_gap = bp.col_spacing.unwrap_or(0.0).max(0.0);
    let col_w = ((inner.width() - col_gap * (ncols as f64 - 1.0)) / ncols as f64).max(1.0);

    let mut out = TextLayout { inner, font_scale: scale, line_reduction: reduce, rotation, ..Default::default() };
    let mut y = 0.0;
    let mut numbering: Vec<u32> = vec![0; 9];
    let mut all_lines: Vec<(LineInfo, LinePlace, f64, Option<(String, f64, f64)>)> = vec![];
    let mut prev_after = 0.0;
    let mut shaped_paras: Vec<ParaShaped> = Vec::with_capacity(body.paragraphs.len());
    for (pi, para) in body.paragraphs.iter().enumerate() {
        let lvl = para.level.min(8) as usize;
        let pp = resolve::para(ctx, shape, para);
        let number = if matches!(pp.bullet, Some(Bullet::AutoNum { .. })) && !para.is_empty() {
            if let Some(n) = numbering.get_mut(lvl) {
                *n += 1;
            }
            for deeper in numbering.iter_mut().skip(lvl + 1) {
                *deeper = 0;
            }
            numbering.get(lvl).copied()
        } else {
            if !para.is_empty() {
                for n in numbering.iter_mut().skip(lvl) {
                    *n = 0;
                }
            }
            None
        };
        shaped_paras.push(shape_para(ctx, shape, body, pi, scale, opts, number));
    }
    for (pi, p) in shaped_paras.iter().enumerate() {
        let props = &p.props;
        let margin = props.margin_left.unwrap_or(0.0).max(0.0);
        let indent = props.indent.unwrap_or(0.0);
        let mr = props.margin_right.unwrap_or(0.0).max(0.0);
        let bullet_w = p.bullet.as_ref().map(|(s, st)| measure_str(&st.face, st.size, s).0).unwrap_or(0.0);
        let first_x = if p.bullet.is_some() {
            let bx = (margin + indent).max(0.0);
            if indent < 0.0 && bx + bullet_w <= margin { margin } else { bx + bullet_w + if indent >= 0.0 { 0.0 } else { 3.0 } }
        } else {
            (margin + indent).max(0.0)
        };
        let rest_x = margin;
        let tabs: Vec<f64> = props.tabs.as_ref().map(|t| t.iter().map(|s| s.pos).collect()).unwrap_or_default();
        let first_w = (col_w - first_x - mr).max(1.0);
        let rest_w = (col_w - rest_x - mr).max(1.0);
        let raw = break_lines(p, first_w, rest_w, wrap, &tabs, props.default_tab.unwrap_or(72.0), first_x, rest_x);
        let before = if pi == 0 { 0.0 } else { spacing_pts(props.space_before, p.lead_size, reduce) };
        y += before.max(0.0) + prev_after;
        let first_line_idx = all_lines.len();
        for (li, rl) in raw.iter().enumerate() {
            // Line metrics: max ascent/descent over the line's styles (or the lead style if empty).
            let (mut asc, mut desc, mut size) = (0.0f64, 0.0f64, 0.0f64);
            for c in p.cells.get(rl.start..rl.end.max(rl.start)).unwrap_or(&[]) {
                if let (Some(st), Some(face)) = (p.styles.get(c.style), p.faces.get(c.face)) {
                    let sz = st.size;
                    let (a, d) = line_metrics(face, sz);
                    asc = asc.max(a);
                    desc = desc.max(d);
                    size = size.max(sz);
                }
            }
            if size == 0.0 {
                let (a, d) = line_metrics(&p.lead_face, p.lead_size);
                asc = a;
                desc = d;
                size = p.lead_size;
            }
            let natural = (asc + desc).max(size * 1.0);
            let lh = match props.line_spacing {
                Some(Spacing::Pct(f)) => natural * f.max(0.0) * (1.0 - reduce),
                Some(Spacing::Pts(v)) => v * (1.0 - reduce),
                None => natural * (1.0 - reduce),
            };
            let ratio = if natural > 0.0 { lh / natural } else { 1.0 };
            let top = y;
            let baseline = top + asc * ratio;
            y += lh;
            let x0 = if li == 0 { first_x } else { rest_x };
            let avail = (col_w - x0 - mr).max(0.0);
            let n = rl.end.saturating_sub(rl.start);
            let line_cells = p.cells.get(rl.start..rl.end).unwrap_or(&[]);
            // Logical advances; tabs measure from the start edge.
            let mut adv = Vec::with_capacity(n);
            let mut x = 0.0;
            for c in line_cells {
                let a = if c.ch == '\t' { tab_advance(x0 + x, &tabs, props.default_tab.unwrap_or(72.0)) } else { c.adv };
                adv.push(a);
                x += a;
            }
            let width = x;
            let align = props.align.unwrap_or(Align::Left);
            let last_line = li + 1 == raw.len();
            let slack = (avail - width).max(0.0);
            // Justification: kashida (justLow, Arabic), spaces, or every character.
            let stretch = !last_line && wrap && slack > 0.0;
            let mut kashida = vec![0.0; n];
            let mut filled = false;
            if stretch && align == Align::JustLow {
                let ops = kashida_opportunities(p, rl.start, rl.end);
                if !ops.is_empty() {
                    let each = slack / ops.len() as f64;
                    for k in ops {
                        if let (Some(e), Some(a)) = (kashida.get_mut(k), adv.get_mut(k)) {
                            *e = each;
                            *a += each;
                        }
                    }
                    filled = true;
                }
            }
            if stretch && !filled && matches!(align, Align::Justify | Align::JustLow) {
                let spaces = line_cells.iter().filter(|c| c.ch == ' ').count();
                if spaces > 0 {
                    for (a, c) in adv.iter_mut().zip(line_cells) {
                        if c.ch == ' ' {
                            *a += slack / spaces as f64;
                        }
                    }
                    filled = true;
                }
            }
            if align == Align::Distributed && n > 1 {
                for a in adv.iter_mut().take(n - 1) {
                    *a += slack / (n - 1) as f64;
                }
                filled = true;
            }
            let line_w: f64 = adv.iter().sum();
            // The line box, in column coordinates: the start edge is the left one for LTR and the
            // right one (S = W - rIns) for RTL paragraphs.
            let (box_l, box_r) = if p.rtl { (mr, col_w - x0) } else { (x0, col_w - mr) };
            let left = if filled {
                box_l
            } else {
                match align {
                    Align::Left => box_l,
                    Align::Right => box_r - line_w,
                    Align::Center => box_l + (box_r - box_l - line_w) / 2.0,
                    Align::Distributed if n <= 1 => box_l + slack / 2.0,
                    // Justified lines that aren't stretched (the last one) sit at the start edge.
                    Align::Justify | Align::JustLow | Align::Distributed if p.rtl => box_r - line_w,
                    Align::Justify | Align::JustLow | Align::Distributed => box_l,
                }
            };
            // Distance from the box's start edge to the text (bullets move with centred or
            // end-aligned text).
            let content_shift = if p.rtl { box_r - (left + line_w) } else { left - box_l };
            // Rules L1 + L2: this line's visual order.
            let chars: Vec<char> = line_cells.iter().map(|c| c.ch).collect();
            let raw_levels: Vec<u8> = line_cells.iter().map(|c| c.level).collect();
            let levels = bidi::line_levels(&chars, &raw_levels, u8::from(p.rtl));
            let order = bidi::visual_order(&levels);
            let mut edges = vec![(left, left); n];
            let mut vx = left;
            for &k in &order {
                let a = adv.get(k).copied().unwrap_or(0.0);
                if let Some(e) = edges.get_mut(k) {
                    *e = (vx, vx + a);
                }
                vx += a;
            }
            let odd = |k: usize| levels.get(k).is_some_and(|l| l % 2 == 1);
            let mut caret_x: Vec<f64> = (0..n).map(|k| edges.get(k).map(|e| if odd(k) { e.1 } else { e.0 }).unwrap_or(left)).collect();
            caret_x.push(match n.checked_sub(1) {
                Some(k) => edges.get(k).map(|e| if odd(k) { e.0 } else { e.1 }).unwrap_or(left),
                None if p.rtl => left + line_w,
                None => left,
            });
            let bullet = if li == 0 {
                p.bullet.as_ref().map(|(s, _)| {
                    // Centred and end-aligned text carries its bullet along.
                    let end_aligned = if p.rtl { align == Align::Left } else { align == Align::Right };
                    let shifted = align == Align::Center || end_aligned;
                    let b = if shifted && p.cells.is_empty() {
                        content_shift + x0 - bullet_w
                    } else {
                        (margin + indent).max(0.0) + if shifted { content_shift } else { 0.0 }
                    };
                    // x_bullet = S - (marL + indent) for RTL: the bullet's start edge is its right.
                    let bx = if p.rtl { col_w - b - bullet_w } else { b };
                    (s.clone(), bx, baseline)
                })
            } else {
                None
            };
            out.content_width = out.content_width.max(width + x0 + mr);
            let boundary: Vec<bool> = line_cells.iter().map(|c| c.boundary).collect();
            all_lines.push((
                LineInfo {
                    para: pi,
                    start: rl.start,
                    end: rl.end,
                    top,
                    baseline,
                    bottom: y,
                    caret_x,
                    column: 0,
                    rtl: p.rtl,
                    levels,
                    edges,
                    boundary,
                    extent: (left, left + line_w),
                    start_offset: x0,
                },
                LinePlace { order, kashida },
                lh,
                bullet,
            ));
        }
        out.para_lines.push((first_line_idx, raw.len()));
        prev_after = spacing_pts(props.space_after, p.lead_size, reduce).max(0.0);
    }
    let total = y;
    out.content_height = total;
    // Columns: distribute lines by height.
    let col_h = if ncols > 1 { inner.height() } else { f64::INFINITY };
    let mut col = 0usize;
    let mut col_y0 = 0.0;
    for (li, _, _, _) in all_lines.iter_mut() {
        if ncols > 1 && li.bottom - col_y0 > col_h + 0.01 && col + 1 < ncols {
            col += 1;
            col_y0 = li.top;
        }
        li.column = col;
        if col > 0 {
            let dy = col_y0;
            li.top -= dy;
            li.baseline -= dy;
            li.bottom -= dy;
            li.shift_x(col as f64 * (col_w + col_gap));
        }
    }
    let used_h = if ncols > 1 { all_lines.iter().map(|(l, ..)| l.bottom).fold(0.0, f64::max) } else { total };
    out.overflow = used_h > inner.height() + 0.5 || (ncols > 1 && all_lines.last().is_some_and(|(l, ..)| l.bottom > inner.height() + 0.5));
    let anchor = bp.anchor.unwrap_or(Anchor::Top);
    let dy = match anchor {
        Anchor::Top | Anchor::Justified | Anchor::Distributed => 0.0,
        Anchor::Middle => (inner.height() - used_h) / 2.0,
        Anchor::Bottom => inner.height() - used_h,
    };
    // anchorCtr: centre the block horizontally.
    let dx = if bp.anchor_ctr.unwrap_or(false) { ((inner.width() - out.content_width) / 2.0).max(0.0) } else { 0.0 };
    let ox = inner.x0 + dx;
    let oy = inner.y0 + dy;
    // Emit runs, in visual order.
    for (line, place, _, bullet) in all_lines.iter_mut() {
        line.top += oy;
        line.baseline += oy;
        line.bottom += oy;
        line.shift_x(ox);
        let Some(p) = shaped_paras.get(line.para) else { continue };
        if let Some((s, bx, by)) = bullet.take()
            && let Some((_, st)) = p.bullet.as_ref()
        {
            let (_, g) = measure_str(&st.face, st.size, &s);
            out.runs.push(GlyphRun {
                face: st.face.clone(),
                size: st.size,
                color: st.color,
                glyphs: g
                    .into_iter()
                    .map(|(gid, x)| (gid, ox + bx + x + if line.column > 0 { line.column as f64 * (col_w + col_gap) } else { 0.0 }, by + oy))
                    .collect(),
                cells: vec![],
                fake_bold: false,
                fake_italic: false,
                outline: None,
                para: line.para,
                chars: (0, 0),
                link: false,
                alpha: st.alpha,
                gradient: None,
            });
        }
        // Group visually consecutive cells by (style, face).
        let mut cur: Option<GlyphRun> = None;
        let mut cur_key = (usize::MAX, usize::MAX);
        for &k in &place.order {
            let ci = line.start + k;
            let Some(cell) = p.cells.get(ci) else { continue };
            let Some(st) = p.styles.get(cell.style) else { continue };
            let Some(&(cx, cx1)) = line.edges.get(k) else { continue };
            let base_y = line.baseline - st.baseline * st.size;
            let size = if st.baseline != 0.0 { st.size * 0.66 } else { st.size };
            let size = if st.caps == Caps::Small && cell.ch.is_lowercase() { size * 0.8 } else { size };
            let key = (cell.style, cell.face);
            if key != cur_key {
                if let Some(r) = cur.take() {
                    out.runs.push(r);
                }
                let Some(face) = p.faces.get(cell.face) else { continue };
                cur = Some(GlyphRun {
                    face: face.clone(),
                    size,
                    color: st.color,
                    glyphs: vec![],
                    cells: vec![],
                    fake_bold: st.fake_bold,
                    fake_italic: st.fake_italic,
                    outline: st.outline,
                    para: line.para,
                    chars: (ci, ci + 1),
                    link: st.link,
                    alpha: st.alpha,
                    gradient: st.gradient.clone(),
                });
                cur_key = key;
            }
            if let Some(r) = cur.as_mut() {
                r.chars = (r.chars.0.min(ci), r.chars.1.max(ci + 1));
                if cell.head == ci && !cell.glyphs.is_empty() {
                    // The cluster's glyphs start at its leftmost cell; kashida (right-to-left
                    // only) fills the gap on the cluster's left with tatweel glyphs.
                    let cells_of =
                        (k..line.end.saturating_sub(line.start)).take_while(|&j| p.cells.get(line.start + j).is_some_and(|c| c.head == ci));
                    let (mut cl, mut kash) = (cx, 0.0);
                    for j in cells_of {
                        cl = cl.min(line.edges.get(j).map(|e| e.0).unwrap_or(cx));
                        kash += place.kashida.get(j).copied().unwrap_or(0.0);
                    }
                    if kash > 0.0 {
                        let face = p.faces.get(cell.face);
                        let tg = face.map(|f| f.glyph_for('\u{0640}')).unwrap_or(0);
                        let tw = face.map(|f| f.advance(tg) * size / f.upem.max(1.0)).unwrap_or(0.0);
                        if tg != 0 && tw > 0.0 {
                            let count = ((kash / tw).ceil() as usize).clamp(1, 256);
                            for i in 0..count {
                                r.glyphs.push((tg, cl + (i as f64 * tw).min((kash - tw).max(0.0)), base_y));
                                r.cells.push((ci, cell.ch));
                            }
                        }
                    }
                    let k66 = if st.baseline != 0.0 { 0.66 } else { 1.0 };
                    for (gid, gx, gy) in &cell.glyphs {
                        r.glyphs.push((*gid, cl + kash + gx * k66, base_y + gy));
                        r.cells.push((ci, cell.ch));
                    }
                }
            }
            let adv = cx1 - cx;
            if adv <= 0.0 {
                continue;
            }
            if let Some(hl) = st.highlight {
                out.decos.push(Deco { rect: Rect::new(cx, line.top, cx1, line.bottom), color: hl, behind: true, para: line.para });
            }
            if !cell.ch.is_whitespace() || st.underline.is_some() {
                if let Some(uc) = st.underline {
                    let t = (st.size * 0.06).max(0.5);
                    let uy = line.baseline + st.size * 0.12;
                    out.decos.push(Deco { rect: Rect::new(cx, uy, cx1, uy + t), color: uc, behind: false, para: line.para });
                }
                if let Some(sk) = st.strike {
                    let t = (st.size * 0.05).max(0.5);
                    let sy = line.baseline - st.size * 0.3;
                    out.decos.push(Deco { rect: Rect::new(cx, sy, cx1, sy + t), color: st.color, behind: false, para: line.para });
                    if sk == Strike::Double {
                        out.decos.push(Deco { rect: Rect::new(cx, sy - t * 2.0, cx1, sy - t), color: st.color, behind: false, para: line.para });
                    }
                }
            }
        }
        if let Some(r) = cur.take() {
            out.runs.push(r);
        }
    }
    out.boundaries = shaped_paras
        .iter()
        .map(|p| {
            let mut b: Vec<bool> = p.cells.iter().map(|c| c.boundary).collect();
            b.push(true);
            if let Some(f) = b.first_mut() {
                *f = true;
            }
            b
        })
        .collect();
    out.lines = all_lines.into_iter().map(|(l, ..)| l).collect();
    // Merge adjacent decorations of the same colour on the same line.
    out.decos = merge_decos(std::mem::take(&mut out.decos));
    out
}

fn merge_decos(v: Vec<Deco>) -> Vec<Deco> {
    let mut out: Vec<Deco> = Vec::with_capacity(v.len());
    for d in v {
        if let Some(last) = out.last_mut()
            && last.color == d.color
            && last.behind == d.behind
            && (last.rect.y0 - d.rect.y0).abs() < 0.01
            && (last.rect.y1 - d.rect.y1).abs() < 0.01
            && (last.rect.x1 - d.rect.x0).abs() < 0.5
        {
            last.rect.x1 = d.rect.x1;
            continue;
        }
        out.push(d);
    }
    out
}

/// How a line's cells are placed: visual order (indices from the line start) and kashida widths
/// (the boxes, with justification, are in [`LineInfo::edges`]).
struct LinePlace {
    order: Vec<usize>,
    kashida: Vec<f64>,
}

/// Kashida opportunities on a line (indices from the line start), at most one per word (its last
/// join, where elongation reads most naturally): the last cell of a right-to-left Arabic grapheme
/// that joins the next letter, in a face with a tatweel glyph, when the two aren't one ligature.
fn kashida_opportunities(p: &ParaShaped, start: usize, end: usize) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    let mut word_has = false;
    for i in start..end {
        let Some(c) = p.cells.get(i) else { continue };
        if c.ch.is_whitespace() {
            word_has = false;
            continue;
        }
        if !c.boundary || c.level % 2 == 0 || !joins_following(c.ch) {
            continue;
        }
        // The next grapheme on the line.
        let Some(nx) = (i + 1..end).find(|&j| p.cells.get(j).is_some_and(|d| d.boundary)) else { continue };
        let Some(d) = p.cells.get(nx) else { continue };
        let same_cluster = d.head == c.head || d.head != nx;
        let tatweel = p.faces.get(c.face).is_some_and(|f| f.glyph_for('\u{0640}') != 0);
        if joins_preceding(d.ch) && d.face == c.face && !same_cluster && tatweel {
            // A later join in the same word replaces the earlier one.
            if word_has {
                out.pop();
            }
            out.push(nx - 1 - start);
            word_has = true;
        }
    }
    out
}

/// Which side of a boundary a caret belongs to: `Downstream` sticks to the character after it
/// (the default), `Upstream` to the character before it, which differs at direction changes and
/// at the end of a wrapped line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Affinity {
    Upstream,
    #[default]
    Downstream,
}

/// Caret movement: through the text order, or across the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MoveMode {
    /// Next/previous grapheme in logical order (`forward` = towards the end of the text).
    #[default]
    Logical,
    /// Next grapheme boundary to the right/left on screen (`forward` = right).
    Visual,
}

impl LineInfo {
    fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }
    fn odd(&self, k: usize) -> bool {
        self.levels.get(k).is_some_and(|l| l % 2 == 1)
    }
    /// Leading edge (where its direction begins) of character `k` of the line.
    fn leading(&self, k: usize) -> Option<f64> {
        self.edges.get(k).map(|e| if self.odd(k) { e.1 } else { e.0 })
    }
    /// Trailing edge (where its direction ends) of character `k` of the line.
    fn trailing(&self, k: usize) -> Option<f64> {
        self.edges.get(k).map(|e| if self.odd(k) { e.0 } else { e.1 })
    }
    fn shift_x(&mut self, dx: f64) {
        for x in &mut self.caret_x {
            *x += dx;
        }
        for e in &mut self.edges {
            e.0 += dx;
            e.1 += dx;
        }
        self.extent.0 += dx;
        self.extent.1 += dx;
    }
    /// Graphemes on the line as (first, end) character indices from the line start.
    fn graphemes(&self) -> Vec<(usize, usize)> {
        let n = self.len();
        let mut out = Vec::new();
        let mut k = 0;
        while k < n {
            let mut e = k + 1;
            while e < n && !self.boundary.get(e).copied().unwrap_or(true) {
                e += 1;
            }
            out.push((k, e));
            k = e;
        }
        out
    }
    /// Visual extent of characters `from..to` (k from the line start).
    fn grapheme_box(&self, from: usize, to: usize) -> Option<(f64, f64)> {
        let es = self.edges.get(from..to)?;
        let l = es.iter().map(|e| e.0).fold(f64::INFINITY, f64::min);
        let r = es.iter().map(|e| e.1).fold(f64::NEG_INFINITY, f64::max);
        (l <= r).then_some((l, r))
    }
    /// Visual x intervals covering paragraph characters `from..to` on this line, left to right
    /// and merged where they touch. Bidi text can make a logical range several boxes.
    pub fn spans(&self, from: usize, to: usize) -> Vec<(f64, f64)> {
        let a = from.max(self.start) - self.start;
        let b = to.min(self.end).saturating_sub(self.start);
        merge_spans(self.edges.get(a..b.max(a)).unwrap_or(&[]).to_vec())
    }
    /// Horizontal extent of paragraph characters `from..to` on this line.
    pub fn span(&self, from: usize, to: usize) -> Option<(f64, f64)> {
        let s = self.spans(from, to);
        Some((s.first()?.0, s.last()?.1))
    }
    /// Caret stops left to right: (x, character offset in the paragraph, affinity).
    fn visual_stops(&self) -> Vec<(f64, usize, Affinity)> {
        let mut gs: Vec<(f64, f64, usize, usize, bool)> =
            self.graphemes().into_iter().filter_map(|(a, b)| self.grapheme_box(a, b).map(|(l, r)| (l, r, a, b, self.odd(a)))).collect();
        gs.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut out: Vec<(f64, usize, Affinity)> = Vec::with_capacity(gs.len() * 2 + 1);
        let mut push = |s: (f64, usize, Affinity)| {
            if !out.last().is_some_and(|l: &(f64, usize, Affinity)| (l.0 - s.0).abs() < 0.01 && l.1 == s.1) {
                out.push(s);
            }
        };
        for (l, r, a, b, rtl) in gs {
            let (a, b) = (self.start + a, self.start + b);
            if rtl {
                push((l, b, Affinity::Upstream));
                push((r, a, Affinity::Downstream));
            } else {
                push((l, a, Affinity::Downstream));
                push((r, b, Affinity::Upstream));
            }
        }
        if out.is_empty() {
            out.push((self.caret_x.first().copied().unwrap_or(0.0), self.start, Affinity::Downstream));
        }
        out
    }
}

/// Non-empty x intervals sorted left to right, merged where they touch.
fn merge_spans(mut v: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    v.retain(|e| e.1 > e.0);
    v.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(v.len());
    for e in v {
        match out.last_mut() {
            Some(last) if e.0 <= last.1 + 0.01 => last.1 = last.1.max(e.1),
            _ => out.push(e),
        }
    }
    out
}

/// A caret position: paragraph and character offset within it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub para: usize,
    pub ch: usize,
}

impl TextLayout {
    /// The line containing a caret position (the later line at a soft wrap boundary).
    pub fn line_of(&self, pos: Pos) -> Option<usize> {
        self.line_of_affine(pos, Affinity::Downstream)
    }
    /// The line containing a caret position: at a soft wrap boundary, the earlier line for an
    /// upstream caret and the later one for a downstream caret.
    pub fn line_of_affine(&self, pos: Pos, affinity: Affinity) -> Option<usize> {
        let mut found = None;
        for (i, l) in self.lines.iter().enumerate() {
            if l.para == pos.para && pos.ch >= l.start && pos.ch <= l.end {
                if affinity == Affinity::Upstream && pos.ch == l.end && pos.ch > l.start {
                    return Some(i);
                }
                found = Some(i);
                if pos.ch < l.end {
                    break;
                }
            }
        }
        found
            .or_else(|| self.lines.iter().rposition(|l| l.para == pos.para && l.start <= pos.ch))
            .or_else(|| self.lines.iter().rposition(|l| l.para == pos.para))
    }
    /// Caret line segment (x, top, bottom) in shape-local, unrotated coordinates (downstream).
    pub fn caret(&self, pos: Pos) -> Option<(f64, f64, f64)> {
        self.caret_at(pos, Affinity::Downstream)
    }
    /// Caret line segment (x, top, bottom) for a position and affinity: downstream at the leading
    /// edge of the character after the boundary, upstream at the trailing edge of the one before.
    pub fn caret_at(&self, pos: Pos, affinity: Affinity) -> Option<(f64, f64, f64)> {
        let li = self.line_of_affine(pos, affinity)?;
        let l = self.lines.get(li)?;
        let k = pos.ch.clamp(l.start, l.end) - l.start;
        let x = match affinity {
            Affinity::Downstream if k < l.len() => l.leading(k),
            Affinity::Upstream if k > 0 => l.trailing(k - 1),
            _ => None,
        };
        let x = x.or_else(|| l.caret_x.get(k).copied()).or(l.caret_x.last().copied())?;
        Some((x, l.top, l.bottom))
    }
    /// Nearest caret position for a point.
    pub fn hit(&self, p: Point) -> Pos {
        self.hit_affinity(p).0
    }
    /// Nearest caret position for a point, with the affinity that keeps the caret on the side of
    /// the character that was hit (bidi text has two carets at a direction change).
    pub fn hit_affinity(&self, p: Point) -> (Pos, Affinity) {
        let mut best: Option<&LineInfo> = None;
        for l in &self.lines {
            if p.y >= l.top && p.y < l.bottom {
                // In multi-column bodies pick the line whose column contains x.
                if best.is_none() || p.x >= l.extent.0.min(l.caret_x.first().copied().unwrap_or(l.extent.0)) - 4.0 {
                    best = Some(l);
                }
            }
        }
        let l = match best {
            Some(l) => l,
            None => {
                let first = self.lines.first();
                let last = self.lines.last();
                match (first, last) {
                    (Some(f), _) if p.y < f.top => f,
                    (_, Some(l)) => l,
                    _ => return (Pos::default(), Affinity::Downstream),
                }
            }
        };
        let mut gs: Vec<(f64, f64, usize, usize, bool)> =
            l.graphemes().into_iter().filter_map(|(a, b)| l.grapheme_box(a, b).map(|(x0, x1)| (x0, x1, a, b, l.odd(a)))).collect();
        if gs.is_empty() {
            return (Pos { para: l.para, ch: l.start }, Affinity::Downstream);
        }
        gs.sort_by(|x, y| x.0.total_cmp(&y.0));
        let hit = gs.iter().find(|g| p.x >= g.0 && p.x <= g.1 && g.1 > g.0).copied();
        let (g, left_half) = match hit {
            Some(g) => (g, p.x < (g.0 + g.1) / 2.0),
            None => match (gs.first().copied(), gs.last().copied()) {
                (Some(f), _) if p.x < f.0 => (f, true),
                (_, Some(z)) if p.x > z.1 => (z, false),
                // Between zero-width graphemes: the nearest one.
                _ => match gs.iter().min_by(|a, b| ((a.0 + a.1) / 2.0 - p.x).abs().total_cmp(&((b.0 + b.1) / 2.0 - p.x).abs())).copied() {
                    Some(g) => (g, p.x < (g.0 + g.1) / 2.0),
                    None => return (Pos { para: l.para, ch: l.start }, Affinity::Downstream),
                },
            },
        };
        let (_, _, a, b, rtl) = g;
        let (before, after) = (Pos { para: l.para, ch: l.start + a }, Pos { para: l.para, ch: l.start + b });
        // The leading half of a character puts the caret before it, the trailing half after it.
        if left_half != rtl { (before, Affinity::Downstream) } else { (after, Affinity::Upstream) }
    }
    /// Move a caret one grapheme: logically (`forward` = towards the end of the text, crossing
    /// paragraphs) or visually (`forward` = to the right on screen, continuing on the next or
    /// previous line in the paragraph's direction at the line's edge).
    pub fn move_caret(&self, pos: Pos, affinity: Affinity, forward: bool, mode: MoveMode) -> (Pos, Affinity) {
        if mode == MoveMode::Visual
            && let Some(li) = self.line_of_affine(pos, affinity)
            && let Some(l) = self.lines.get(li)
        {
            let stops = l.visual_stops();
            let cur_x = self.caret_at(pos, affinity).map(|c| c.0).unwrap_or(0.0);
            let idx = stops
                .iter()
                .position(|s| s.1 == pos.ch && s.2 == affinity)
                .or_else(|| stops.iter().position(|s| s.1 == pos.ch))
                .or_else(|| stops.iter().enumerate().min_by(|a, b| (a.1.0 - cur_x).abs().total_cmp(&(b.1.0 - cur_x).abs())).map(|(i, _)| i));
            if let Some(idx) = idx {
                let mut j = idx;
                loop {
                    let next = if forward { j.checked_add(1) } else { j.checked_sub(1) };
                    let Some(nj) = next else { break };
                    let Some(s) = stops.get(nj) else { break };
                    // Skip stops that wouldn't move the caret on screen or in the text.
                    if s.1 != pos.ch && (s.0 - cur_x).abs() > 0.01 {
                        return (Pos { para: l.para, ch: s.1 }, s.2);
                    }
                    j = nj;
                }
            }
            // Off the line's edge: on to the neighbouring line in reading order.
            let logical_forward = forward != l.rtl;
            let target = if logical_forward { li.checked_add(1) } else { li.checked_sub(1) };
            return match target.and_then(|t| self.lines.get(t)) {
                Some(t) if logical_forward => (Pos { para: t.para, ch: t.start }, Affinity::Downstream),
                Some(t) => (Pos { para: t.para, ch: t.end }, if t.para == l.para { Affinity::Upstream } else { Affinity::Downstream }),
                None => (pos, affinity),
            };
        }
        let Some(b) = self.boundaries.get(pos.para) else { return (pos, affinity) };
        let len = b.len().saturating_sub(1);
        let ch = pos.ch.min(len);
        let next = if forward {
            (ch + 1..=len).find(|&k| b.get(k).copied().unwrap_or(true))
        } else {
            (0..ch).rev().find(|&k| b.get(k).copied().unwrap_or(true))
        };
        match next {
            Some(k) => (Pos { para: pos.para, ch: k }, Affinity::Downstream),
            None if forward && pos.para + 1 < self.boundaries.len() => (Pos { para: pos.para + 1, ch: 0 }, Affinity::Downstream),
            None if !forward && pos.para > 0 => {
                let prev = pos.para - 1;
                (Pos { para: prev, ch: self.boundaries.get(prev).map(|b| b.len().saturating_sub(1)).unwrap_or(0) }, Affinity::Downstream)
            }
            None => (Pos { para: pos.para, ch }, Affinity::Downstream),
        }
    }
    /// Selection highlight rectangles between two positions: per line, the union of the visual
    /// boxes of the selected characters (one logical range can be several boxes in bidi text).
    pub fn selection_rects(&self, a: Pos, b: Pos) -> Vec<Rect> {
        let (s, e) = if a <= b { (a, b) } else { (b, a) };
        let mut out = vec![];
        for l in &self.lines {
            let lp = Pos { para: l.para, ch: l.start };
            let le = Pos { para: l.para, ch: l.end };
            if le < s || lp > e {
                continue;
            }
            let from = if s > lp { s.ch } else { l.start };
            let to = if e < le { e.ch } else { l.end };
            let mut spans = l.spans(from, to);
            // Selecting across a paragraph end shows a small extra box for the newline, at the
            // paragraph's end side.
            if e.para > l.para && to == l.end {
                let end_x = l.caret_x.last().copied().unwrap_or(l.extent.1);
                spans.push(if l.rtl { (end_x - 6.0, end_x) } else { (end_x, end_x + 6.0) });
                spans = merge_spans(spans);
            }
            for (x0, x1) in spans {
                if x1 > x0 {
                    out.push(Rect::new(x0, l.top, x1, l.bottom));
                }
            }
        }
        out
    }
    /// Vertical caret movement: the position on the line above/below closest to `x`.
    pub fn vertical(&self, pos: Pos, down: bool, x: f64) -> Pos {
        let Some(li) = self.line_of(pos) else { return pos };
        let target = if down { li + 1 } else { li.wrapping_sub(1) };
        let Some(l) = self.lines.get(target) else { return pos };
        let p = Point::new(x, (l.top + l.bottom) / 2.0);
        let mut h = self.hit(p);
        if h.para != l.para {
            h.para = l.para;
            h.ch = h.ch.clamp(l.start, l.end);
        }
        h
    }
    /// Start/end of the visual line containing `pos` (logical: Home/End).
    pub fn line_bounds(&self, pos: Pos) -> (Pos, Pos) {
        match self.line_of(pos).and_then(|i| self.lines.get(i)) {
            Some(l) => (Pos { para: l.para, ch: l.start }, Pos { para: l.para, ch: l.end }),
            None => (pos, pos),
        }
    }
    /// Bounding box of all glyphs/lines (shape-local).
    pub fn bounds(&self) -> Rect {
        let mut r: Option<Rect> = None;
        for l in &self.lines {
            let lr = Rect::new(l.extent.0, l.top, l.extent.1.max(l.extent.0), l.bottom);
            r = Some(r.map(|a| a.union(lr)).unwrap_or(lr));
        }
        r.unwrap_or(self.inner)
    }
}

/// Height the shape needs so its text fits (Resize shape to fit text): text height + insets.
pub fn fit_height(ctx: &Ctx, shape: &Shape, body: &TextBody, rect: Rect, fields: &dyn Fields) -> f64 {
    let bp = resolve::body(ctx, shape);
    let l = layout(ctx, shape, body, &Opts { rect, fields, prompt_color: None, no_shrink: true });
    l.content_height + bp.inset_t.unwrap_or(3.6) + bp.inset_b.unwrap_or(3.6)
}

/// Width a non-wrapping text box needs.
pub fn fit_width(ctx: &Ctx, shape: &Shape, body: &TextBody, rect: Rect, fields: &dyn Fields) -> f64 {
    let bp = resolve::body(ctx, shape);
    let wide = Rect::new(rect.x0, rect.y0, rect.x0 + 100_000.0, rect.y1);
    let mut s2 = shape.clone();
    if let Some(t) = s2.text.as_mut() {
        t.body.wrap = Some(false);
    }
    let l = layout(ctx, &s2, body, &Opts { rect: wide, fields, prompt_color: None, no_shrink: true });
    let w = l.lines.iter().map(|li| li.start_offset + (li.extent.1 - li.extent.0)).fold(0.0, f64::max);
    w + bp.inset_l.unwrap_or(7.2) + bp.inset_r.unwrap_or(7.2) + 1.0
}

#[cfg(test)]
mod tests;
