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
    /// Opacity of the fill on top of `color`'s own alpha (0 for no fill).
    pub alpha: f64,
    pub gradient: Option<deckcraft_model::style::Gradient>,
    /// Shadow and glow behind the glyphs (`a:rPr/a:effectLst`).
    pub effects: Option<RunEffects>,
    /// East Asian vertical text (`eaVert`): each glyph stands upright, turned a quarter turn
    /// back about the centre of its em box ([`GlyphRun::upright_centre`]) inside text that is
    /// laid out turned a quarter turn.
    pub upright: bool,
}

/// Share of the size the ideographic em box's centre sits above the baseline (CJK fonts put the
/// em box from 0.88 em above to 0.12 em below it).
pub const UPRIGHT_CENTRE: f64 = 0.38;

impl GlyphRun {
    /// The point an upright glyph at pen position (`x`, `y`) turns about: the centre of its em box,
    /// `advance` wide (`None` when the run is not upright).
    pub fn upright_centre(&self, gid: u32, x: f64, y: f64) -> Option<(f64, f64)> {
        if !self.upright {
            return None;
        }
        let adv = self.face.advance(gid) * self.size / self.face.upem.max(1.0);
        Some((x + adv / 2.0, y - self.size * UPRIGHT_CENTRE))
    }
}

/// A text run's shadow and glow, colours resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunEffects {
    pub shadow: Option<TextShadow>,
    /// Glow colour (with its alpha) and radius in points.
    pub glow: Option<(Rgba, f64)>,
}

/// An outer shadow: colour (with its alpha), blur radius and offset in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextShadow {
    pub color: Rgba,
    pub blur: f64,
    pub dx: f64,
    pub dy: f64,
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
    /// How the line fills `rect`.
    pub line: DecoLine,
}

/// How an underline fills its box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DecoLine {
    /// The whole box.
    #[default]
    Solid,
    /// Dashes or dots across the box's height, from its left end.
    Dashed(Dash),
    /// A zigzag between the box's top and bottom: `stroke` wide, one wave every `period`.
    Wave { stroke: f64, period: f64 },
}

/// Dash patterns of the dotted and dashed underlines, in units of the line's thickness. Measured
/// on PowerPoint's export: dots and their gaps as long as the line is thick, dashes three
/// thicknesses long with gaps of two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dash {
    Dot,
    Dash,
    Long,
    DotDash,
    DotDotDash,
}

impl Dash {
    /// On, off, on, off… lengths in thicknesses.
    fn pattern(self) -> &'static [f64] {
        match self {
            Dash::Dot => &[1.0, 1.0],
            Dash::Dash => &[3.0, 2.0],
            Dash::Long => &[6.0, 2.0],
            Dash::DotDash => &[3.0, 2.0, 1.0, 2.0],
            Dash::DotDotDash => &[3.0, 2.0, 1.0, 2.0, 1.0, 2.0],
        }
    }
}

impl Deco {
    /// The filled boxes of a solid or dashed line (dashes start at the box's left end). A wavy
    /// line gives a thin box along its middle, for drawing that can't stroke a path.
    pub fn pieces(&self) -> Vec<Rect> {
        let r = self.rect;
        match self.line {
            DecoLine::Solid => vec![r],
            DecoLine::Wave { stroke, .. } => {
                let c = (r.y0 + r.y1) / 2.0;
                vec![Rect::new(r.x0, c - stroke / 2.0, r.x1, c + stroke / 2.0)]
            }
            DecoLine::Dashed(d) => {
                let unit = r.height().max(0.1);
                let pat = d.pattern();
                let mut out = Vec::new();
                let mut x = r.x0;
                let mut k = 0usize;
                while x < r.x1 && out.len() < 10_000 {
                    let len = pat.get(k % pat.len()).copied().unwrap_or(1.0) * unit;
                    if k.is_multiple_of(2) {
                        out.push(Rect::new(x, r.y0, (x + len).min(r.x1), r.y1));
                    }
                    x += len;
                    k += 1;
                }
                out
            }
        }
    }

    /// The zigzag of a wavy underline, left to right from the box's top, and its stroke width;
    /// None for other lines.
    pub fn wave(&self) -> Option<(Vec<Point>, f64)> {
        let DecoLine::Wave { stroke, period } = self.line else { return None };
        let r = self.rect;
        let half = (period / 2.0).max(0.1);
        let n = ((r.width().max(0.0) / half).ceil() as usize).min(20_000);
        let pts = (0..=n)
            .map(|i| {
                let x = (r.x0 + i as f64 * half).min(r.x1);
                Point::new(x, if i % 2 == 0 { r.y0 } else { r.y1 })
            })
            .collect();
        Some((pts, stroke))
    }
}

/// An underline style (`a:rPr/@u`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Underline {
    Single,
    Double,
    Heavy,
    /// Dotted or dashed, heavy or not.
    Dashed(Dash, bool),
    /// Wavy, heavy or not.
    Wavy(bool),
}

impl Underline {
    fn from_xml(u: &str) -> Option<Self> {
        Some(match u {
            "none" => return None,
            // PowerPoint underlines the spaces of a "words" underline too.
            "words" => Underline::Single,
            // At text sizes PowerPoint's double wave reads as two straight lines.
            "dbl" | "wavyDbl" => Underline::Double,
            "heavy" => Underline::Heavy,
            "dotted" => Underline::Dashed(Dash::Dot, false),
            "dottedHeavy" => Underline::Dashed(Dash::Dot, true),
            "dash" => Underline::Dashed(Dash::Dash, false),
            "dashHeavy" => Underline::Dashed(Dash::Dash, true),
            "dashLong" => Underline::Dashed(Dash::Long, false),
            "dashLongHeavy" => Underline::Dashed(Dash::Long, true),
            "dotDash" => Underline::Dashed(Dash::DotDash, false),
            "dotDashHeavy" => Underline::Dashed(Dash::DotDash, true),
            "dotDotDash" => Underline::Dashed(Dash::DotDotDash, false),
            "dotDotDashHeavy" => Underline::Dashed(Dash::DotDotDash, true),
            "wavy" => Underline::Wavy(false),
            "wavyHeavy" => Underline::Wavy(true),
            _ => Underline::Single,
        })
    }

    /// The boxes of this underline below the baseline (top, bottom, fill), from the font's
    /// underline position `p` and thickness `t` at the run's size. Measured on PowerPoint's export
    /// (Arial 96 pt: p = 10.2, t = 7.0): single at p..p+t; heavy from p-t to p+0.55t; double as two
    /// lines 0.55t thick spanning p-t..p+t; wavy a thin zigzag from p-0.6t to p+1.1t, a wave every
    /// 3.9t.
    fn boxes(self, p: f64, t: f64) -> Vec<(f64, f64, DecoLine)> {
        let heavy = (p - t, p + 0.55 * t);
        match self {
            Underline::Single => vec![(p, p + t, DecoLine::Solid)],
            Underline::Heavy => vec![(heavy.0, heavy.1, DecoLine::Solid)],
            Underline::Double => vec![(p - t, p - 0.45 * t, DecoLine::Solid), (p + 0.45 * t, p + t, DecoLine::Solid)],
            Underline::Dashed(d, false) => vec![(p, p + t, DecoLine::Dashed(d))],
            Underline::Dashed(d, true) => vec![(heavy.0, heavy.1, DecoLine::Dashed(d))],
            Underline::Wavy(h) => {
                let stroke = (t * 0.15).max(0.4) * if h { 2.0 } else { 1.0 };
                vec![(p - 0.6 * t, p + 1.1 * t, DecoLine::Wave { stroke, period: t * 3.9 })]
            }
        }
    }
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
    /// Empty list markers for the editor only; ordinary rendering and export omit them.
    /// Their space is still reserved so the caret starts where the first character will.
    pub empty_bullets: Vec<GlyphRun>,
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
    /// Per paragraph: its characters as laid out (fields filled in), indexed like
    /// [`GlyphRun::cells`] and [`GlyphRun::chars`] (text extraction of exports with real text).
    pub texts: Vec<String>,
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
    underline: Option<(Rgba, Underline)>,
    strike: Option<Strike>,
    highlight: Option<Rgba>,
    outline: Option<(Rgba, f64)>,
    link: bool,
    gradient: Option<deckcraft_model::style::Gradient>,
    effects: Option<RunEffects>,
    /// "Regular", "Bold"… for the slot and fallback faces.
    style_name: &'static str,
    /// BCP 47 language (`a:rPr/@lang`), for shaping.
    lang: Option<String>,
    /// The `a:ea` and `a:cs` families (theme references resolved), if the document names one.
    ea: Option<String>,
    cs: Option<String>,
    /// The `a:sym` family: private-use characters (U+F000–U+F0FF, as PowerPoint stores symbol-font
    /// characters) are drawn in it.
    sym: Option<String>,
    /// Heading font (`+mj-…`): the theme's major font set applies.
    major: bool,
    /// Pair kerning on: `a:rPr/@kern` is set and the size reaches it.
    kern: bool,
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
    /// Drawn upright in East Asian vertical text.
    upright: bool,
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
    let face = FontDb::global().document_face(&family, style_name(bold, italic));
    let fake_bold = bold && face.weight < 550.0;
    let fake_italic = italic && !face.italic;
    let size = (r.size.unwrap_or(18.0) * scale).clamp(0.5, 4000.0);
    let (mut color, alpha, gradient) = match &r.fill {
        // The resolved colour carries the fill's transparency (`a:alpha`): it is applied once.
        Some(Fill::Solid { color }) => (ctx.color(color, None), 1.0, None),
        Some(Fill::Gradient(g)) => {
            // Colours resolved here (theme, colour map), so the renderer needs no colour scheme.
            let mut g = g.clone();
            for s in &mut g.stops {
                s.color = deckcraft_model::ColorRef::rgb(ctx.color(&s.color, None));
            }
            (g.stops.first().map(|s| ctx.color(&s.color, None)).unwrap_or(Rgba::BLACK), 1.0, Some(g))
        }
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
    let underline =
        r.underline.as_deref().and_then(Underline::from_xml).map(|u| (r.underline_color.as_ref().map(|c| ctx.color(c, None)).unwrap_or(color), u));
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
        effects: run_effects(ctx, r, scale),
        style_name: style_name(bold, italic),
        lang: r.lang.clone().filter(|l| !l.is_empty()),
        ea,
        cs,
        sym: r.font_sym.as_deref().map(|f| ctx.master.theme.font(f)).filter(|f| !f.trim().is_empty()),
        major,
        kern: r.kern.is_some_and(|k| k > 0.0 && size + 1e-6 >= k * scale),
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

/// Space before or after a paragraph, points. Shrink-on-overflow's line spacing reduction
/// doesn't apply to it (PowerPoint keeps it).
fn spacing_pts(s: Option<Spacing>, size: f64) -> f64 {
    match s {
        Some(Spacing::Pct(p)) => p * size * 1.2,
        Some(Spacing::Pts(v)) => v,
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

/// Letters of a numbering alphabet: 1 → the first, past the end the letter doubles ("aa"), as
/// the Latin alphabetic schemes do.
fn letters(list: &[char], n: u32) -> String {
    let len = list.len().max(1) as u32;
    let Some(i) = n.checked_sub(1) else { return String::new() };
    let c = list.get((i % len) as usize).copied().unwrap_or('?');
    std::iter::repeat_n(c, (i / len + 1).min(50) as usize).collect()
}

/// `n` written with the digits `zero..zero+9` (Thai, Devanagari, full-width digits).
fn digits_in(n: u32, zero: char) -> String {
    n.to_string().chars().map(|d| d.to_digit(10).and_then(|v| char::from_u32(zero as u32 + v)).unwrap_or(d)).collect()
}

const CJK_DIGITS: [char; 10] = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];

/// Chinese numerals: 10 → 十, 11 → 十一, 25 → 二十五, 105 → 一百零五.
fn chinese_num(n: u32) -> String {
    if n == 0 || n > 99_999 {
        return n.to_string();
    }
    let units = ['\0', '十', '百', '千', '万'];
    let ds: Vec<u32> = n.to_string().chars().filter_map(|c| c.to_digit(10)).collect();
    let mut out = String::new();
    let mut zero = false;
    for (i, d) in ds.iter().enumerate() {
        let place = ds.len() - 1 - i;
        if *d == 0 {
            zero = true;
            continue;
        }
        if zero {
            out.push('零');
            zero = false;
        }
        // "十" alone for 10–19, not "一十".
        if !(*d == 1 && place == 1 && i == 0) {
            out.push(CJK_DIGITS.get(*d as usize).copied().unwrap_or('?'));
        }
        if let Some(u) = units.get(place).filter(|u| **u != '\0') {
            out.push(*u);
        }
    }
    out
}

/// Text of an auto-number bullet: `arabicPeriod` 3 → "3.". The East Asian, Thai, Hindi, Arabic
/// and Hebrew schemes are written as PowerPoint draws them: `ea1ChsPeriod` 10 → "十．",
/// `ea1JpnKorPeriod` 10 → "一〇．" (digit by digit), `arabicDbPeriod` 13 → "１３．",
/// `thaiAlphaParenR` 4 → "ง)", `hindiAlpha1Period` 1 → "क.", `arabic2Minus` 10 → "ي-"
/// (abjad order), `circleNumWdWhitePlain` 13 → "③" (those circles go to ten).
pub fn autonum(scheme: &str, n: u32) -> String {
    const THAI: [char; 41] = [
        'ก', 'ข', 'ค', 'ง', 'จ', 'ฉ', 'ช', 'ซ', 'ฌ', 'ญ', 'ฎ', 'ฏ', 'ฐ', 'ฑ', 'ฒ', 'ณ', 'ด', 'ต', 'ถ', 'ท', 'ธ', 'น', 'บ', 'ป', 'ผ', 'ฝ', 'พ', 'ฟ',
        'ภ', 'ม', 'ย', 'ร', 'ล', 'ว', 'ศ', 'ษ', 'ส', 'ห', 'ฬ', 'อ', 'ฮ',
    ];
    const HINDI_VOWELS: [char; 11] = ['अ', 'आ', 'इ', 'ई', 'उ', 'ऊ', 'ऋ', 'ए', 'ऐ', 'ओ', 'औ'];
    const HINDI_CONSONANTS: [char; 33] = [
        'क', 'ख', 'ग', 'घ', 'ङ', 'च', 'छ', 'ज', 'झ', 'ञ', 'ट', 'ठ', 'ड', 'ढ', 'ण', 'त', 'थ', 'द', 'ध', 'न', 'प', 'फ', 'ब', 'भ', 'म', 'य', 'र', 'ल',
        'व', 'श', 'ष', 'स', 'ह',
    ];
    const ARABIC_ALPHABET: [char; 28] =
        ['ا', 'ب', 'ت', 'ث', 'ج', 'ح', 'خ', 'د', 'ذ', 'ر', 'ز', 'س', 'ش', 'ص', 'ض', 'ط', 'ظ', 'ع', 'غ', 'ف', 'ق', 'ك', 'ل', 'م', 'ن', 'ه', 'و', 'ي'];
    const ARABIC_ABJAD: [char; 28] =
        ['ا', 'ب', 'ج', 'د', 'ه', 'و', 'ز', 'ح', 'ط', 'ي', 'ك', 'ل', 'م', 'ن', 'س', 'ع', 'ف', 'ص', 'ق', 'ر', 'ش', 'ت', 'ث', 'خ', 'ذ', 'ض', 'ظ', 'غ'];
    const HEBREW: [char; 22] = ['א', 'ב', 'ג', 'ד', 'ה', 'ו', 'ז', 'ח', 'ט', 'י', 'כ', 'ל', 'מ', 'נ', 'ס', 'ע', 'פ', 'צ', 'ק', 'ר', 'ש', 'ת'];
    let circled = |first: u32, count: u32| char::from_u32(first + n.saturating_sub(1) % count).map(String::from).unwrap_or_else(|| n.to_string());
    let core = if scheme.starts_with("romanUc") {
        roman(n, true)
    } else if scheme.starts_with("romanLc") {
        roman(n, false)
    } else if scheme.starts_with("alphaUc") {
        alpha_num(n, true)
    } else if scheme.starts_with("alphaLc") {
        alpha_num(n, false)
    } else if scheme.starts_with("circleNumWdWhite") {
        return circled(0x2460, 10);
    } else if scheme.starts_with("circleNumWdBlack") {
        return circled(0x2776, 10);
    } else if scheme.starts_with("circleNum") {
        return char::from_u32(0x2460 + n.saturating_sub(1).min(19)).map(String::from).unwrap_or_else(|| n.to_string());
    } else if scheme.starts_with("ea1Chs") || scheme.starts_with("ea1Cht") {
        chinese_num(n)
    } else if scheme.starts_with("ea1Jpn") {
        n.to_string().chars().map(|d| d.to_digit(10).and_then(|v| CJK_DIGITS.get(v as usize).copied()).unwrap_or(d)).collect()
    } else if scheme.starts_with("arabicDb") {
        digits_in(n, '０')
    } else if scheme.starts_with("thaiAlpha") {
        letters(&THAI, n)
    } else if scheme.starts_with("thaiNum") {
        digits_in(n, '๐')
    } else if scheme.starts_with("hindiAlpha1") {
        letters(&HINDI_CONSONANTS, n)
    } else if scheme.starts_with("hindiAlpha") {
        letters(&HINDI_VOWELS, n)
    } else if scheme.starts_with("hindiNum") {
        digits_in(n, '०')
    } else if scheme.starts_with("arabic1") {
        letters(&ARABIC_ALPHABET, n)
    } else if scheme.starts_with("arabic2") {
        letters(&ARABIC_ABJAD, n)
    } else if scheme.starts_with("hebrew2") {
        letters(&HEBREW, n)
    } else {
        n.to_string()
    };
    // East Asian schemes end with a full-width full stop.
    let wide = scheme.starts_with("ea1") || scheme.starts_with("arabicDb");
    if scheme.ends_with("ParenBoth") {
        format!("({core})")
    } else if scheme.ends_with("ParenR") {
        format!("{core})")
    } else if scheme.ends_with("Period") {
        format!("{core}{}", if wide { '．' } else { '.' })
    } else if scheme.ends_with("Minus") {
        format!("{core}-")
    } else {
        core
    }
}

#[allow(clippy::too_many_arguments)]
fn shape_para(ctx: &Ctx, shape: &Shape, body: &TextBody, pi: usize, scale: f64, opts: &Opts, number: Option<u32>, vertical: Upright) -> ParaShaped {
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
    let cells = shape_cells(ctx, &text, &char_style, &styles, &mut faces, rtl, vertical);
    let lead = para.runs.first().map(|r| resolve::run(ctx, shape, para, &r.props)).unwrap_or_else(|| resolve::run(ctx, shape, para, &para.end_props));
    let lead_style = make_style(ctx, &lead, scale, opts.prompt_color);
    let lead_size = lead_style.size;
    let lead_face = lead_style.face.clone();
    // Bullet.
    let bullet = match &props.bullet {
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
            bullet_face(db, &mut st, &ch);

            // A symbol font (Wingdings, Symbol…) that is installed draws its own glyph, found
            // at U+F000 + the character code as such fonts map them; otherwise common symbol
            // bullets are mapped to Unicode look-alikes.
            let ch = match symbol_font_char(&st.face, char, props.bullet_font.as_deref()) {
                Some(c) => c.to_string(),
                None => map_symbol_bullet(char, props.bullet_font.as_deref()),
            };
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
            let mut st = make_style(ctx, &rp, scale, opts.prompt_color);
            let label = autonum(scheme, start_at.saturating_sub(1).saturating_add(number.unwrap_or(1)));
            bullet_face(db, &mut st, &label);
            Some((label, st))
        }
        _ => None,
    };
    ParaShaped { cells, styles, faces, props, lead_size, lead_face, bullet, level: para.level, rtl }
}

/// A face that draws the bullet `label` when the bullet's own can't (circled numbers ①, arrows
/// and other symbols missing from the text font), by the usual fallback.
fn bullet_face(db: &FontDb, st: &mut Style, label: &str) {
    let chars: Vec<char> = label.chars().collect();
    if !chars.is_empty() && !deckcraft_fonts::covers_cluster(&st.face, &chars) {
        st.face = db.cascade(&chars, std::slice::from_ref(&st.face), st.style_name);
    }
}

/// Shape a logical paragraph into cells: bidi levels, per-grapheme faces, items of one level,
/// script, face and shaping style, each shaped with the whole paragraph as context.
/// A run's outer shadow and glow (inner shadows aren't drawn on text), at the layout's `scale`.
fn run_effects(ctx: &Ctx, r: &RunProps, scale: f64) -> Option<RunEffects> {
    let shadow = r.shadow.as_ref().filter(|s| !s.inner).map(|s| {
        let a = s.dir.to_radians();
        TextShadow { color: ctx.color(&s.color, None), blur: s.blur.max(0.0) * scale, dx: a.cos() * s.dist * scale, dy: a.sin() * s.dist * scale }
    });
    let glow = r.glow.as_ref().filter(|g| g.radius > 0.0).map(|g| (ctx.color(&g.color, None), g.radius * scale));
    (shadow.is_some() || glow.is_some()).then_some(RunEffects { shadow, glow })
}

#[allow(clippy::too_many_arguments)]
fn shape_cells(
    ctx: &Ctx,
    text: &str,
    char_style: &[usize],
    styles: &[Style],
    faces: &mut Vec<Arc<FontFace>>,
    rtl: bool,
    vertical: Upright,
) -> Vec<Cell> {
    let db = FontDb::global();
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    // Byte offset of each character (and of the end).
    let mut byte_of: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
    byte_of.push(text.len());
    let char_at_byte: HashMap<usize, usize> = byte_of.iter().enumerate().map(|(i, b)| (*b, i)).collect();
    let style_rtl: Vec<bool> = styles.iter().map(|st| st.lang.as_deref().is_some_and(bidi::is_rtl_lang)).collect();
    let rtl_lang: Vec<bool> = char_style.iter().map(|&si| style_rtl.get(si).copied().unwrap_or(false)).collect();
    let levels = bidi::levels_lang(text, &chars, rtl, &rtl_lang);
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
        // Emoji, and pictographs asked to look like text (U+FE0E), pick their own face.
        let neutral =
            cluster.iter().all(|c| script::script_of(*c).is_none()) && !deckcraft_fonts::is_emoji_cluster(cluster) && !cluster.contains(&'\u{FE0E}');
        // A symbol-font character (U+F0xx) with an `a:sym` font draws in that font when it's there.
        let sym_face = st
            .sym
            .as_deref()
            .filter(|_| cluster.first().is_some_and(|c| ('\u{F000}'..='\u{F0FF}').contains(c)))
            .filter(|f| db.has_family(f))
            .map(|f| db.face(f, st.style_name))
            .filter(|f| deckcraft_fonts::covers_cluster(f, cluster));
        let face = match (sym_face, prev.as_ref()) {
            (Some(f), _) => f,
            (None, Some(p))
                if neutral && !p.is_color() && deckcraft_fonts::covers_cluster(p, cluster) && char_style.get(a.wrapping_sub(1)) == Some(&si) =>
            {
                p.clone()
            }
            (None, _) => {
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
            upright: false,
        })
        .collect();
    // In East Asian vertical text, ideographs, kana, Hangul, full-width forms and the like stand
    // upright (a whole grapheme at a time); other text lies on its side.
    if vertical != Upright::None {
        for &(a, b) in &graphemes {
            let up = chars.get(a).is_some_and(|c| if vertical == Upright::All { !c.is_whitespace() } else { is_upright(*c) });
            for c in cells.get_mut(a..b).into_iter().flatten() {
                c.upright = up;
            }
        }
    }
    // Items: maximal ranges of one level, face, script and shaping style (case mapping, language).
    let upper = |i: usize| styles.get(char_style.get(i).copied().unwrap_or(0)).is_some_and(|st| st.caps != Caps::None);
    let lang = |i: usize| styles.get(char_style.get(i).copied().unwrap_or(0)).and_then(|st| st.lang.as_deref());
    let kern = |i: usize| styles.get(char_style.get(i).copied().unwrap_or(0)).is_some_and(|st| st.kern);
    let up = |i: usize| cells.get(i).is_some_and(|c| c.upright);
    let key = |i: usize| (levels.get(i).copied(), char_face.get(i).copied(), scripts.get(i).map(|s| s.0), upper(i), lang(i), kern(i), up(i));
    let mut items: Vec<(usize, usize)> = Vec::new();
    let mut a = 0;
    while a < n {
        let mut b = a + 1;
        while b < n && key(b) == key(a) {
            b += 1;
        }
        items.push((a, b));
        a = b;
    }
    // Upright glyphs keep their horizontal forms: PowerPoint turns "、", "。" and "「" as they are
    // (no `vert` alternates).
    for (a, b) in items {
        shape_item(&mut cells, styles, faces, text, &byte_of, &char_at_byte, a..b, scripts.get(a).and_then(|s| s.0), lang(a), upper(a));
    }
    // Stacked text: each character takes one em down the line, centred across it (PowerPoint
    // stacks 18 pt Latin letters about 18 pt apart, a space only its own width).
    if vertical == Upright::All {
        for i in 0..n {
            let Some(c) = cells.get(i) else { continue };
            if !c.upright || c.head != i || c.glyphs.is_empty() {
                continue;
            }
            let size = styles.get(c.style).map(|st| st.size).unwrap_or(18.0);
            let width: f64 = (i..n).take_while(|&j| cells.get(j).is_some_and(|d| d.head == i)).filter_map(|j| cells.get(j)).map(|d| d.adv).sum();
            let pad = (size - width) / 2.0;
            if let Some(c) = cells.get_mut(i) {
                for g in &mut c.glyphs {
                    g.1 += pad;
                }
            }
            for j in i..n {
                match cells.get_mut(j) {
                    Some(d) if d.head == i => d.adv = if j == i { size } else { 0.0 },
                    _ => break,
                }
            }
        }
    }
    cells
}

/// A character in capitals (`cap="all"`/`"small"`): its uppercase when that is one character.
/// PowerPoint leaves characters whose capital is a sequence as they are: "ß" stays "ß" (not "SS"),
/// "ﬁ" and "ŉ" stay.
fn caps_char(ch: char) -> char {
    let mut up = ch.to_uppercase();
    match (up.next(), up.next()) {
        (Some(u), None) => u,
        _ => ch,
    }
}

/// Does `c` stand upright in East Asian vertical text? Ideographs, kana, Hangul, Bopomofo, CJK
/// symbols and punctuation, full-width forms, enclosed and compatibility CJK, circled numbers
/// and emoji (Unicode's Vertical_Orientation U and T, roughly); Latin, digits and the rest of the
/// half-width text lie on their side as in `vert`.
fn is_upright(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF
            | 0x2460..=0x24FF
            | 0x2E80..=0x2FFF
            | 0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x3100..=0x312F
            | 0x3130..=0x318F
            | 0x3190..=0x31FF
            | 0x3200..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7FF
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE1F
            | 0xFE30..=0xFE6F
            | 0xFF01..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F000..=0x1FAFF
            | 0x20000..=0x3FFFF
    )
}

/// Which characters stand upright in vertical text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Upright {
    /// None (horizontal text, vert, vert270, mongolianVert).
    None,
    /// Ideographs, kana and the like (eaVert).
    Cjk,
    /// Every character, one under the other (wordArtVert).
    All,
}

/// Size of superscript and subscript text relative to the run's size.
const SUB_SUPER: f64 = 0.66;

/// The OpenType features to change from the shaper's defaults for an item, as PowerPoint draws
/// it: kerning only where the run asks for it (`kern`), and no standard or contextual ligatures in
/// Latin, Greek and Cyrillic ("fi", "ffi" and Calibri's "ti" stay apart). Other scripts need
/// their ligatures (Arabic lam-alef), and emoji sequences are joined by them.
fn shaping_features(cells: &[Cell], styles: &[Style], script: Option<ScriptTag>) -> Vec<deckcraft_fonts::Feature> {
    let mut f = vec![];
    if !cells.first().and_then(|c| styles.get(c.style)).is_some_and(|st| st.kern) {
        f.extend(deckcraft_fonts::feature("-kern"));
    }
    let alphabetic = matches!(script.as_ref(), Some(b"Latn" | b"Grek" | b"Cyrl"));
    let emoji = cells.iter().any(|c| matches!(c.ch as u32, 0x200D | 0x20E3 | 0xFE0F | 0x2600..=0x27BF | 0x1F000..=0x1FAFF | 0xE0020..=0xE007F));
    if alphabetic && !emoji {
        f.extend(deckcraft_fonts::feature("-liga"));
        f.extend(deckcraft_fonts::feature("-clig"));
    }
    f
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
    let features = shaping_features(cells.get(range.clone()).unwrap_or(&[]), styles, script);
    let params = ShapeParams { features: &features, rtl, script, language: lang };
    let glyphs = deckcraft_fonts::shape_range(face, text, b0..b1, &params, |ch| if upper { caps_char(ch) } else { ch });
    let upem = face.upem.max(1.0);
    let scale_of = |cell: &Cell| {
        let st = styles.get(cell.style);
        let size = st.map(|s| s.size).unwrap_or(18.0);
        let small = st.is_some_and(|s| s.caps == Caps::Small) && cell.ch.is_lowercase();
        let size = if small { size * 0.8 } else { size };
        // Superscript and subscript glyphs are drawn at two thirds of the size, so they advance by
        // two thirds too.
        let size = if st.is_some_and(|s| s.baseline != 0.0) { size * SUB_SUPER } else { size };
        size / upem
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

/// Is `family` a symbol font (characters at U+F000 + code)?
fn is_symbol_font(family: &str) -> bool {
    let f = family.to_ascii_lowercase();
    f.starts_with("wingdings") || f == "symbol" || f.starts_with("webdings") || f.starts_with("marlett")
}

/// The character to draw bullet `c` with the installed symbol font `face`, when `font` names a
/// symbol font that `face` is (not a substitute) and it has the glyph.
fn symbol_font_char(face: &FontFace, c: &str, font: Option<&str>) -> Option<char> {
    let font = font?;
    if !is_symbol_font(font) || !face.family.eq_ignore_ascii_case(font) {
        return None;
    }
    let first = c.chars().next()?;
    let pua = char::from_u32(0xF000 | ((first as u32) & 0xFF))?;
    if face.covers(pua) {
        Some(pua)
    } else if face.covers(first) {
        Some(first)
    } else {
        None
    }
}

/// The character that draws `c`, a character of text set in the symbol font `font` (Wingdings,
/// Symbol, Webdings…: its code, or U+F000 + its code), with `face`: the installed symbol font's own
/// glyph when `face` is that font, else a Unicode look-alike as bullets use. None when `font` is not
/// a symbol font or `c` is not a symbol code. Metafile text takes this path.
pub fn symbol_char(face: &FontFace, font: &str, c: char) -> Option<char> {
    let code = c as u32;
    if !is_symbol_font(font) || !(code < 0x100 || (0xF000..=0xF0FF).contains(&code)) {
        return None;
    }
    let plain = char::from_u32(code & 0xFF)?;
    let s = plain.to_string();
    Some(symbol_font_char(face, &s, Some(font)).unwrap_or_else(|| map_symbol_bullet(&s, Some(font)).chars().next().unwrap_or('•')))
}

/// The Unicode bullet for a symbol-font bullet character (Wingdings, Symbol), or the character.
pub fn map_symbol_bullet(c: &str, font: Option<&str>) -> String {
    let f = font.unwrap_or("").to_ascii_lowercase();
    let first = c.chars().next().unwrap_or('•');
    if f.starts_with("webdings") {
        let m = match (first as u32) & 0xff {
            0x6e => '●',
            0x67 => '■',
            0x61 => '✓',
            0x72 => '✕',
            0x33 => '◀',
            0x34 => '▶',
            0x35 => '▲',
            0x36 => '▼',
            _ => '•',
        };
        return m.to_string();
    }
    if f.contains("wingdings") || f.contains("symbol") || ('\u{f000}'..='\u{f0ff}').contains(&first) {
        let code = (first as u32) & 0xff;
        // Wingdings 2 and 3 put different shapes at the same codes.
        if f.starts_with("wingdings 2") {
            let m = match code {
                0x97 | 0x98 => '●',
                0x9e..=0xa1 => '■',
                0xa2..=0xa5 => '◆',
                0xea..=0xf3 => '★',
                0xb7..=0xbf => '○',
                0x50 => '✓',
                _ => '•',
            };
            return m.to_string();
        }
        if f.starts_with("wingdings 3") {
            let m = match code {
                0x7d | 0x75 => '►',
                0x7c | 0x74 => '◄',
                0x70 => '▲',
                0x71 => '▼',
                0xc6 | 0xc7 => '➔',
                _ => '▶',
            };
            return m.to_string();
        }
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
    // PowerPoint draws only the first character of a longer `buChar` ("••" draws one dot).
    c.graphemes(true).next().unwrap_or("•").to_string()
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

/// Single line spacing as a multiple of the font size. PowerPoint spaces single-spaced lines 1.2
/// times the size apart in every font (observed on twenty Latin, CJK, Thai and Indic fonts at 18
/// and 40 pt), whatever the font's own ascent and descent.
///
/// Caveat: this was measured on one Windows install of desktop PowerPoint. It may be a Windows or
/// machine setting; PowerPoint on macOS or the web may space lines by the font's own metrics.
/// Keep it a single tunable value, and re-measure on the other platform before changing it.
const LINE_PITCH: f64 = 1.2;

/// A line's height above and below the baseline at `size`: [`LINE_PITCH`] × size, split where
/// the font's Windows ascent and descent put the baseline.
fn line_metrics(face: &FontFace, size: f64) -> (f64, f64) {
    let h = size * LINE_PITCH;
    (h * face.line_split, h * (1.0 - face.line_split))
}

/// Hangul syllables and jamo: PowerPoint wraps Korean at word boundaries (spaces), not between
/// syllables.
fn is_hangul(c: char) -> bool {
    matches!(c, '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' | '\u{A960}'..='\u{A97F}' | '\u{AC00}'..='\u{D7FF}')
}

/// East Asian commas and full stops, the punctuation PowerPoint lets hang past the end of a line
/// (`hangingPunct`). Closing brackets don't hang: "「。」" overflowing by its "」" moves to the next
/// line whole.
fn is_hanging_punct(c: char) -> bool {
    matches!(c, '、' | '。' | '，' | '．' | '､' | '｡')
}

/// Small kana, the prolonged sound mark and iteration marks: PowerPoint keeps them off the start
/// of a line (strict Japanese line breaking), where UAX #14's default allows a break before them.
fn is_cj_nonstarter(c: char) -> bool {
    matches!(
        c,
        'ぁ' | 'ぃ'
            | 'ぅ'
            | 'ぇ'
            | 'ぉ'
            | 'っ'
            | 'ゃ'
            | 'ゅ'
            | 'ょ'
            | 'ゎ'
            | 'ゕ'
            | 'ゖ'
            | 'ァ'
            | 'ィ'
            | 'ゥ'
            | 'ェ'
            | 'ォ'
            | 'ッ'
            | 'ャ'
            | 'ュ'
            | 'ョ'
            | 'ヮ'
            | 'ヵ'
            | 'ヶ'
            | 'ー'
            | '々'
            | '〻'
            | 'ゝ'
            | 'ゞ'
            | 'ヽ'
            | 'ヾ'
            | '\u{31F0}'..='\u{31FF}'
            | 'ｧ'..='ｰ'
    )
}

/// Break opportunities: allowed[i] = a line may start at char i. Unicode line breaking, then
/// PowerPoint's differences: without `latinLnBrk` a word doesn't break after `/` or `\`, and
/// Korean breaks only between words.
fn break_opportunities(text: &str, latin_break: bool) -> (Vec<bool>, Vec<bool>) {
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
    let chars: Vec<char> = text.chars().collect();
    for (i, w) in chars.windows(2).enumerate() {
        let (prev, next) = (w[0], w[1]);
        // PowerPoint never breaks after a zero-width space, nor before an em dash (only after it).
        let keep = if (is_hangul(prev) && is_hangul(next)) || prev == '\u{200B}' || next == '\u{2014}' || is_cj_nonstarter(next) {
            false
        } else {
            latin_break || !matches!(prev, '/' | '\\') || next.is_whitespace()
        };
        if !keep
            && !mandatory.get(i + 1).copied().unwrap_or(false)
            && let Some(a) = allowed.get_mut(i + 1)
        {
            *a = false;
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
    tab_stops: &[Tab],
    default_tab: f64,
    x0_first: f64,
    x0_rest: f64,
) -> Vec<RawLine> {
    let text: String = p.cells.iter().map(|c| c.ch).collect();
    let (allowed, _) = break_opportunities(&text, p.props.latin_line_break.unwrap_or(false));
    let hang = p.props.hanging_punct.unwrap_or(false);
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
            let adv = if c == '\t' {
                tab_advance(x0 + x, tab_stops, default_tab, tab_segment(&p.cells, i))
            } else {
                p.cells.get(i).map(|c| c.adv).unwrap_or(0.0)
            };
            let breakable = |j: usize| p.cells.get(j).is_none_or(|c| c.breakable(j));
            if i > start && allowed.get(i).copied().unwrap_or(false) && breakable(i) {
                last_break = Some(i);
            }
            // Closing East Asian punctuation hangs past the end instead of wrapping
            // (`hangingPunct`); the line breaks after it.
            if wrap && hang && x + adv > avail + 0.01 && x <= avail + 0.01 && is_hanging_punct(c) && i > start {
                end = i + 1;
                next = i + 1;
                break;
            }
            // A tab that would run past the end moves to the next line (and to its first stop there).
            if wrap && c == '\t' && x + adv > avail + 0.01 && i > start {
                end = i;
                next = i;
                while end > start && p.cells.get(end - 1).is_some_and(|c| c.ch == ' ') {
                    end -= 1;
                }
                break;
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

/// How a tab stop aligns the text after it (`a:tab/@algn`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum TabAlign {
    Left,
    Center,
    Right,
    /// The first "." of the text sits at the stop (text without one ends there).
    Decimal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Tab {
    pos: f64,
    align: TabAlign,
}

fn tabs_of(props: &ParaProps) -> Vec<Tab> {
    props
        .tabs
        .as_ref()
        .map(|t| {
            t.iter()
                .map(|s| Tab {
                    pos: s.pos,
                    align: match s.align.as_str() {
                        "ctr" => TabAlign::Center,
                        "r" => TabAlign::Right,
                        "dec" => TabAlign::Decimal,
                        _ => TabAlign::Left,
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Width of the text after the tab at cell `i` up to the next tab or line break, and the width of
/// that text before its first ".".
fn tab_segment(cells: &[Cell], i: usize) -> (f64, f64) {
    let mut w = 0.0;
    let mut dec = None;
    for c in cells.iter().skip(i + 1) {
        if matches!(c.ch, '\t' | '\u{b}' | '\n') {
            break;
        }
        if c.ch == '.' && dec.is_none() {
            dec = Some(w);
        }
        w += c.adv;
    }
    (w, dec.unwrap_or(w))
}

/// Advance of a tab at `x` (from the start edge) to the next stop, placing the text after it
/// (`seg`: its width, and its width before the decimal point) as the stop aligns it but never
/// moving back, as PowerPoint does; past the last stop, to the next default stop.
fn tab_advance(x: f64, stops: &[Tab], default_tab: f64, seg: (f64, f64)) -> f64 {
    for s in stops {
        if s.pos > x + 0.5 {
            let to = s.pos - x;
            return match s.align {
                TabAlign::Left => to,
                TabAlign::Right => (to - seg.0).max(0.0),
                TabAlign::Center => (to - seg.0 / 2.0).max(0.0),
                TabAlign::Decimal => (to - seg.1).max(0.0),
            };
        }
    }
    let d = if default_tab > 1.0 { default_tab } else { 72.0 };
    let next = ((x / d).floor() + 1.0) * d;
    (next - x).max(1.0)
}

#[cfg(test)]
thread_local! {
    /// How many times `layout_scaled` ran on this thread (tests count autofit's passes).
    pub(crate) static LAYOUT_PASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Lay out a shape's text body.
pub fn layout(ctx: &Ctx, shape: &Shape, body: &TextBody, opts: &Opts) -> TextLayout {
    let bp = resolve::body(ctx, shape);
    let autofit = bp.autofit.unwrap_or(AutoFit::None);
    // Shrink on overflow starts from the scale the file was saved with (`fontScale`,
    // `lnSpcReduction`), which is what PowerPoint draws; editing the text resets it.
    let (scale0, reduce0, no_saved_scale) = match (autofit, opts.no_shrink) {
        (AutoFit::Shrink { font_scale, line_reduction }, false) => (
            font_scale.filter(|s| s.is_finite()).map(|s| s.clamp(0.1, 1.0)).unwrap_or(1.0),
            if line_reduction.is_finite() { line_reduction.clamp(0.0, 0.2) } else { 0.0 },
            font_scale.is_none(),
        ),
        _ => (1.0, 0.0, false),
    };
    let mut l = layout_scaled(ctx, shape, body, &bp, opts, scale0, reduce0);
    // A scale PowerPoint saved is drawn as it is: PowerPoint fits the text again only when it
    // changes (and editing resets the saved scale), not when a file opens, even where our
    // metrics would call it overflowing (trailing empty paragraphs, substituted fonts).
    // A body whose file has no `fontScale` at all (`<a:normAutofit/>`) is the schema default
    // 100 % and is drawn as it is too, not refitted on open (#724).
    let saved = no_saved_scale || scale0 < 1.0 || reduce0 > 0.0;
    if let (AutoFit::Shrink { .. }, false) = (autofit, opts.no_shrink)
        && l.overflow
        && !saved
    {
        // Text that overflows even at the smallest scale gets that scale: every larger one
        // overflows too, so searching would only lay it out a dozen more times (seconds for a
        // body of tens of thousands of paragraphs, on every redraw of the slide).
        let smallest = layout_scaled(ctx, shape, body, &bp, opts, 0.1, 0.2);
        if smallest.overflow {
            return smallest;
        }
        // Binary search the largest scale that fits (steps like 90%, 80%… with line reduction).
        let (mut lo, mut hi) = (0.1_f64, scale0);
        let mut best = Some(smallest);
        for _ in 0..12 {
            let mid = (lo + hi) / 2.0;
            let red: f64 = ((1.0 - mid) * 0.8_f64).min(0.2).max(reduce0);
            let t = layout_scaled(ctx, shape, body, &bp, opts, mid, red);
            if t.overflow {
                hi = mid;
            } else {
                lo = mid;
                best = Some(t);
            }
        }
        if let Some(b) = best {
            l = b;
        }
    }
    l
}

#[allow(clippy::too_many_arguments)]
fn layout_scaled(ctx: &Ctx, shape: &Shape, body: &TextBody, bp: &BodyProps, opts: &Opts, scale: f64, reduce: f64) -> TextLayout {
    #[cfg(test)]
    LAYOUT_PASSES.with(|n| n.set(n.get() + 1));
    let r = opts.rect;
    let vert = bp.vert.unwrap_or(TextDir::Horizontal);
    // East Asian vertical text: CJK stands upright in text turned a quarter turn; stacked text
    // stands every character upright, one under the other.
    let upright_text = match vert {
        TextDir::EaVertical => Upright::Cjk,
        TextDir::Stacked | TextDir::StackedRtl => Upright::All,
        _ => Upright::None,
    };
    // Mongolian vertical and stacked text: turned like vert, their lines run from left to right
    // (as PowerPoint draws both).
    let lines_ltr = matches!(vert, TextDir::MongolianVertical | TextDir::Stacked);
    let rotation = match vert {
        TextDir::Vertical | TextDir::EaVertical | TextDir::Stacked | TextDir::StackedRtl | TextDir::MongolianVertical => 90.0,
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
    // Text that follows a WordArt arch or circle is set on one line per paragraph.
    let follow = bp.warp.as_deref().is_some_and(deckcraft_geom::warp::is_follow);
    let wrap = bp.wrap.unwrap_or(true) && !follow && !matches!(bp.autofit, Some(AutoFit::Shape) if shape.text_box && !bp.wrap.unwrap_or(true));
    let ncols = bp.columns.unwrap_or(1).clamp(1, 16) as usize;
    let col_gap = bp.col_spacing.unwrap_or(0.0).max(0.0);
    let col_w = ((inner.width() - col_gap * (ncols as f64 - 1.0)) / ncols as f64).max(1.0);

    let mut out = TextLayout { inner, font_scale: scale, line_reduction: reduce, rotation, ..Default::default() };
    let mut y = 0.0;
    let mut numbering: Vec<u32> = vec![0; 9];
    let mut all_lines: Vec<(LineInfo, LinePlace, f64, Option<(String, f64, f64)>)> = vec![];
    let mut prev_after = 0.0;
    // How far the last line's full pitch reaches past the text (exact spacing; see below).
    let mut tail = 0.0;
    let first_last = bp.space_first_last.unwrap_or(false);
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
        } else if matches!(pp.bullet, Some(Bullet::AutoNum { .. })) {
            // Preview the next number without consuming it: empty paragraphs do not advance
            // numbering in the saved document or affect later populated paragraphs.
            numbering.get(lvl).map(|n| n.saturating_add(1))
        } else {
            if !para.is_empty() {
                for n in numbering.iter_mut().skip(lvl) {
                    *n = 0;
                }
            }
            None
        };
        shaped_paras.push(shape_para(ctx, shape, body, pi, scale, opts, number, upright_text));
    }
    for (pi, p) in shaped_paras.iter().enumerate() {
        let props = &p.props;
        let margin = props.margin_left.unwrap_or(0.0).max(0.0);
        let indent = props.indent.unwrap_or(0.0);
        let mr = props.margin_right.unwrap_or(0.0).max(0.0);
        let bullet_w = p.bullet.as_ref().map(|(s, st)| measure_str(&st.face, st.size, s).0).unwrap_or(0.0);
        let first_x = if p.bullet.is_some() {
            let bx = (margin + indent).max(0.0);
            // A hanging bullet's text starts at the margin. When the hang reaches left of the box
            // (marL + indent < 0), PowerPoint pins the bullet to the box's edge and keeps the
            // hang: the first line's text starts at -indent (later lines stay at the margin).
            let stop = margin.max(-indent);
            if indent < 0.0 && bx + bullet_w <= stop { stop } else { bx + bullet_w + if indent >= 0.0 { 0.0 } else { 3.0 } }
        } else {
            (margin + indent).max(0.0)
        };
        let rest_x = margin;
        let tabs = tabs_of(props);
        let first_w = (col_w - first_x - mr).max(1.0);
        let rest_w = (col_w - rest_x - mr).max(1.0);
        let raw = break_lines(p, first_w, rest_w, wrap, &tabs, props.default_tab.unwrap_or(72.0), first_x, rest_x);
        // The first paragraph's space before counts only when the body says so.
        let before = if pi == 0 && !first_last { 0.0 } else { spacing_pts(props.space_before, p.lead_size) };
        y += before.max(0.0) + prev_after;
        let first_line_idx = all_lines.len();
        for (li, rl) in raw.iter().enumerate() {
            // Line metrics: max ascent/descent over the line's styles (or the lead style if empty).
            let (mut asc, mut desc, mut size) = (0.0f64, 0.0f64, 0.0f64);
            for c in p.cells.get(rl.start..rl.end.max(rl.start)).unwrap_or(&[]) {
                if let (Some(st), Some(face)) = (p.styles.get(c.style), p.faces.get(c.face)) {
                    let sz = st.size;
                    // The run's own fonts set the line; a fallback face (emoji, a symbol or a
                    // script the run's fonts lack) does not move the baseline in PowerPoint.
                    let own = Arc::ptr_eq(face, &st.face)
                        || [st.ea.as_deref(), st.cs.as_deref(), st.sym.as_deref()].into_iter().flatten().any(|f| face.family.eq_ignore_ascii_case(f));
                    let (a, d) = line_metrics(if own { face } else { &st.face }, sz);
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
                // The reduction comes off the percentage (90 % less 20 % is 70 %), as PowerPoint
                // applies `lnSpcReduction`.
                Some(Spacing::Pct(f)) => natural * (f - reduce).max(f * 0.1).max(0.0),
                Some(Spacing::Pts(v)) => v * (1.0 - reduce),
                None => natural * (1.0 - reduce),
            };
            let ratio = if natural > 0.0 { lh / natural } else { 1.0 };
            let top = y;
            // Exact spacing (`spcPts`), as PowerPoint sets it: the baseline sits at 80 % of the
            // line pitch, and the text ends at the last line's descent rather than a full pitch.
            let exact = matches!(props.line_spacing, Some(Spacing::Pts(_)));
            let baseline = if exact { top + lh * 0.8 } else { top + asc * ratio };
            tail = if exact { (lh * 0.2 - desc).max(-lh) } else { 0.0 };
            y += lh;
            let x0 = if li == 0 { first_x } else { rest_x };
            let avail = (col_w - x0 - mr).max(0.0);
            let n = rl.end.saturating_sub(rl.start);
            let line_cells = p.cells.get(rl.start..rl.end).unwrap_or(&[]);
            // Logical advances; tabs measure from the start edge.
            let mut adv = Vec::with_capacity(n);
            let mut x = 0.0;
            // A line that breaks at a soft hyphen ends with a visible hyphen.
            let shy_end = li + 1 < raw.len() && line_cells.last().is_some_and(|c| c.ch == '\u{AD}');
            for (k, c) in line_cells.iter().enumerate() {
                let a = if shy_end && k + 1 == n {
                    p.faces.get(c.face).zip(p.styles.get(c.style)).map(|(f, st)| measure_str(f, st.size, "-").0).unwrap_or(0.0)
                } else if c.ch == '\t' {
                    tab_advance(x0 + x, &tabs, props.default_tab.unwrap_or(72.0), tab_segment(&p.cells, rl.start + k))
                } else {
                    c.adv
                };
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
        prev_after = spacing_pts(props.space_after, p.lead_size).max(0.0);
    }
    // The text ends at the last line's descent under exact spacing; the last paragraph's space
    // after counts only when the body says so.
    let total = (y - tail).max(0.0) + if first_last { prev_after } else { 0.0 };
    out.content_height = total;
    // Where column `c` starts: from the left, or from the right with `rtlCol` (PowerPoint puts the
    // first column on the right).
    let rtl_cols = bp.rtl_col.unwrap_or(false) && ncols > 1;
    let col_x = |c: usize| (if rtl_cols { ncols.saturating_sub(1 + c) } else { c }) as f64 * (col_w + col_gap);
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
        }
        let cx = col_x(col);
        if cx != 0.0 {
            li.shift_x(cx);
        }
    }
    let used_h = if ncols > 1 { all_lines.iter().map(|(l, ..)| l.bottom).fold(0.0, f64::max) } else { total };
    out.overflow = used_h > inner.height() + 0.5 || (ncols > 1 && all_lines.last().is_some_and(|(l, ..)| l.bottom > inner.height() + 0.5));
    let anchor = bp.anchor.unwrap_or(Anchor::Top);
    let dy = match anchor {
        Anchor::Top => 0.0,
        Anchor::Middle => (inner.height() - used_h) / 2.0,
        // PowerPoint draws justified and distributed anchoring at the bottom.
        Anchor::Bottom | Anchor::Justified | Anchor::Distributed => inner.height() - used_h,
    };
    // anchorCtr: centre the drawn text block on the box. Lines sit where their alignment put
    // them, so the origin is set so the block's centre lands on the box's centre — a left-edge
    // offset alone pushed right-aligned lines out of the box (#736). Empty lines carry no ink
    // and stay out of the block. Overflowing content centres symmetrically past both edges.
    // Not applied to vertical text (there the offset would run along the wrong axis) or
    // multi-column bodies (centring unimplemented for them).
    let centre_block = bp.anchor_ctr.unwrap_or(false) && ncols == 1 && matches!(vert, TextDir::Horizontal);
    let ox = if centre_block && !all_lines.is_empty() {
        let ink = all_lines.iter().filter(|(li, ..)| li.end > li.start);
        let (bmin, bmax) = ink.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (li, ..)| (a.min(li.extent.0), b.max(li.extent.1)));
        if bmax >= bmin { inner.x0 + inner.width() / 2.0 - (bmin + bmax) / 2.0 } else { inner.x0 }
    } else {
        inner.x0
    };
    let oy = inner.y0 + dy;
    // Emit runs, in visual order.
    for (line, place, _, bullet) in all_lines.iter_mut() {
        line.top += oy;
        line.baseline += oy;
        line.bottom += oy;
        line.shift_x(ox);
        if lines_ltr {
            // The line's place mirrored across the box: the first line ends up on the left.
            let d = inner.y0 + inner.y1 - line.top - line.bottom;
            line.top += d;
            line.baseline += d;
            line.bottom += d;
        }
        let Some(p) = shaped_paras.get(line.para) else { continue };
        if let Some((s, bx, _)) = bullet.take()
            && let Some((_, st)) = p.bullet.as_ref()
        {
            let (_, g) = measure_str(&st.face, st.size, &s);
            let run = GlyphRun {
                face: st.face.clone(),
                size: st.size,
                color: st.color,
                glyphs: g.into_iter().map(|(gid, x)| (gid, ox + bx + x + col_x(line.column), line.baseline)).collect(),
                cells: vec![],
                fake_bold: false,
                fake_italic: false,
                outline: None,
                para: line.para,
                chars: (0, 0),
                link: false,
                alpha: st.alpha,
                gradient: None,
                effects: None,
                upright: false,
            };
            if body.paragraphs.get(line.para).is_some_and(|p| p.is_empty()) && opts.prompt_color.is_none() {
                out.empty_bullets.push(run);
            } else {
                out.runs.push(run);
            }
        }
        // Group visually consecutive cells by (style, face).
        let mut cur: Option<GlyphRun> = None;
        let mut cur_key = (usize::MAX, usize::MAX, false, false);
        for &k in &place.order {
            let ci = line.start + k;
            let Some(cell) = p.cells.get(ci) else { continue };
            let Some(st) = p.styles.get(cell.style) else { continue };
            let Some(&(cx, cx1)) = line.edges.get(k) else { continue };
            let base_y = line.baseline - st.baseline * st.size;
            let size = if st.baseline != 0.0 { st.size * SUB_SUPER } else { st.size };
            let size = if st.caps == Caps::Small && cell.ch.is_lowercase() { size * 0.8 } else { size };
            // Small capitals are drawn smaller than the capitals beside them: a run of their own.
            let key = (cell.style, cell.face, st.caps == Caps::Small && cell.ch.is_lowercase(), cell.upright);
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
                    effects: st.effects,
                    upright: cell.upright,
                });
                cur_key = key;
            }
            if let Some(r) = cur.as_mut() {
                r.chars = (r.chars.0.min(ci), r.chars.1.max(ci + 1));
                if cell.ch == '\u{AD}' && cx1 - cx > 0.01 {
                    // The soft hyphen a line breaks at is drawn as a hyphen.
                    if let Some(f) = p.faces.get(cell.face) {
                        r.glyphs.push((f.glyph_for('-'), cx, base_y));
                        r.cells.push((ci, cell.ch));
                    }
                } else if cell.head == ci && !cell.glyphs.is_empty() {
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
                    for (gid, gx, gy) in &cell.glyphs {
                        r.glyphs.push((*gid, cl + kash + gx, base_y + gy));
                        r.cells.push((ci, cell.ch));
                    }
                }
            }
            let adv = cx1 - cx;
            if adv <= 0.0 {
                continue;
            }
            if let Some(hl) = st.highlight {
                out.decos.push(Deco {
                    rect: Rect::new(cx, line.top, cx1, line.bottom),
                    color: hl,
                    behind: true,
                    para: line.para,
                    line: DecoLine::Solid,
                });
            }
            if !cell.ch.is_whitespace() || st.underline.is_some() {
                if let Some((uc, u)) = st.underline {
                    // At the underline position and thickness of the run's own font.
                    let k = st.size / st.face.upem.max(1.0);
                    let (p, t) = (st.face.underline.0 * k, (st.face.underline.1 * k).max(0.5));
                    for (y0, y1, kind) in u.boxes(p, t) {
                        out.decos.push(Deco {
                            rect: Rect::new(cx, line.baseline + y0, cx1, line.baseline + y1),
                            color: uc,
                            behind: false,
                            para: line.para,
                            line: kind,
                        });
                    }
                }
                if let Some(sk) = st.strike {
                    // At the strikeout position and thickness of the run's own font.
                    let k = st.size / st.face.upem.max(1.0);
                    let t = (st.face.strikeout.1 * k).max(0.5);
                    let sy = line.baseline - st.face.strikeout.0 * k;
                    out.decos.push(Deco {
                        rect: Rect::new(cx, sy, cx1, sy + t),
                        color: st.color,
                        behind: false,
                        para: line.para,
                        line: DecoLine::Solid,
                    });
                    if sk == Strike::Double {
                        out.decos.push(Deco {
                            rect: Rect::new(cx, sy - t * 2.0, cx1, sy - t),
                            color: st.color,
                            behind: false,
                            para: line.para,
                            line: DecoLine::Solid,
                        });
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
    out.texts = shaped_paras.iter().map(|p| p.cells.iter().map(|c| c.ch).collect()).collect();
    out.lines = all_lines.into_iter().map(|(l, ..)| l).collect();
    // Merge adjacent decorations of the same colour on the same line.
    out.decos = merge_decos(std::mem::take(&mut out.decos));
    out
}

fn merge_decos(v: Vec<Deco>) -> Vec<Deco> {
    let mut out: Vec<Deco> = Vec::with_capacity(v.len());
    for d in v {
        // Lines drawn together (a double underline, a strikethrough) interleave: look a few back.
        if let Some(last) = out.iter_mut().rev().take(4).find(|last| {
            last.color == d.color
                && last.behind == d.behind
                && last.line == d.line
                && (last.rect.y0 - d.rect.y0).abs() < 0.01
                && (last.rect.y1 - d.rect.y1).abs() < 0.01
                && (last.rect.x1 - d.rect.x0).abs() < 0.5
        }) {
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
    /// Underlines for text an input method is composing, from `a` to `b` (#101): `(x0, x1, y)` per
    /// visual span, a little below each line's baseline (PowerPoint underlines the composition
    /// in place). A composition that wraps gets one underline per line.
    pub fn composition_underlines(&self, a: Pos, b: Pos) -> Vec<(f64, f64, f64)> {
        let (s, e) = if a <= b { (a, b) } else { (b, a) };
        let mut out = vec![];
        if s == e {
            return out;
        }
        for l in &self.lines {
            let lp = Pos { para: l.para, ch: l.start };
            let le = Pos { para: l.para, ch: l.end };
            if le <= s || lp >= e {
                continue;
            }
            let from = if s > lp { s.ch } else { l.start };
            let to = if e < le { e.ch } else { l.end };
            // Under the baseline, in the descent, never below the line.
            let y = (l.baseline + (l.bottom - l.baseline) * 0.35).min(l.bottom);
            for (x0, x1) in l.spans(from, to) {
                if x1 > x0 {
                    out.push((x0, x1, y));
                }
            }
        }
        out
    }
    /// Vertical caret movement: the position on the line above/below closest to `x`.
    pub fn vertical(&self, pos: Pos, down: bool, x: f64) -> Pos {
        self.vertical_affine(pos, Affinity::Downstream, down, x).0
    }
    /// The line a caret is on. An upstream caret at the start of a soft-wrapped line (where End
    /// leaves it, after the previous line's trailing spaces) belongs to the line before.
    pub fn line_index(&self, pos: Pos, affinity: Affinity) -> Option<usize> {
        if affinity == Affinity::Upstream && pos.ch > 0 {
            for (i, w) in self.lines.windows(2).enumerate() {
                if let [a, b] = w
                    && b.para == pos.para
                    && a.para == pos.para
                    && b.start == pos.ch
                {
                    return Some(i);
                }
            }
        }
        self.line_of_affine(pos, affinity)
    }
    /// Up/Down from a caret with its affinity: the position on the line above/below nearest `x`.
    pub fn vertical_affine(&self, pos: Pos, affinity: Affinity, down: bool, x: f64) -> (Pos, Affinity) {
        let Some(li) = self.line_index(pos, affinity) else { return (pos, affinity) };
        let target = if down { li.checked_add(1) } else { li.checked_sub(1) };
        let Some(l) = target.and_then(|t| self.lines.get(t)) else { return (pos, affinity) };
        let p = Point::new(x, (l.top + l.bottom) / 2.0);
        let mut h = self.hit(p);
        if h.para != l.para {
            h.para = l.para;
        }
        h.ch = h.ch.clamp(l.start, l.end);
        (h, Affinity::Downstream)
    }
    /// Start/end of the visual line containing `pos` (logical: Home/End).
    pub fn line_bounds(&self, pos: Pos) -> (Pos, Pos) {
        self.line_bounds_affine(pos, Affinity::Downstream)
    }
    /// Start/end of the visual line a caret with this affinity is on.
    pub fn line_bounds_affine(&self, pos: Pos, affinity: Affinity) -> (Pos, Pos) {
        match self.line_index(pos, affinity).and_then(|i| self.lines.get(i)) {
            Some(l) => (Pos { para: l.para, ch: l.start }, Pos { para: l.para, ch: l.end }),
            None => (pos, pos),
        }
    }
    /// Where End puts the caret: after a soft wrap, past the line's trailing spaces at the start
    /// of the next line with upstream affinity (so it is still drawn on this line), else the end
    /// of the paragraph.
    pub fn line_end_caret(&self, pos: Pos, affinity: Affinity) -> (Pos, Affinity) {
        let Some(li) = self.line_index(pos, affinity) else { return (pos, Affinity::Downstream) };
        let Some(l) = self.lines.get(li) else { return (pos, Affinity::Downstream) };
        match self.lines.get(li + 1) {
            Some(n) if n.para == l.para && n.start > l.start => (Pos { para: l.para, ch: n.start }, Affinity::Upstream),
            _ => (Pos { para: l.para, ch: l.end }, Affinity::Downstream),
        }
    }
    /// The caret to draw (x, top, bottom): an upstream caret after a soft wrap is drawn at the end
    /// of the line it belongs to.
    pub fn caret_drawn(&self, pos: Pos, affinity: Affinity) -> Option<(f64, f64, f64)> {
        let li = self.line_index(pos, affinity)?;
        let l = self.lines.get(li)?;
        if pos.ch > l.end {
            let x = l.caret_x.last().copied()?;
            return Some((x, l.top, l.bottom));
        }
        self.caret_at(pos, affinity)
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
