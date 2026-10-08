//! Text layout for slide text bodies.
//!
//! [`layout`] resolves every run's formatting through the placeholder/style chain, shapes it with
//! font fallback, breaks lines (UAX #14 opportunities, greedy fill), places bullets and numbers,
//! applies indents, spacing and alignment, anchors the block in the shape's text box, and shrinks
//! text on overflow when the body asks for it. The result is positioned glyph runs in shape-local
//! coordinates plus per-line caret positions for editing.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::sync::Arc;

use deckcraft_color::Rgba;
use deckcraft_fonts::{FontDb, FontFace};
use deckcraft_geom::{Point, Rect};
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::{Align, Anchor, AutoFit, BodyProps, Bullet, Caps, ParaProps, RunKind, RunProps, Spacing, Strike, TextBody, TextDir};
use deckcraft_model::{Fill, Shape};

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
    /// Caret x for each character boundary start..=end (len = end - start + 1).
    pub caret_x: Vec<f64>,
    /// Column index (multi-column bodies).
    pub column: usize,
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
}

/// Values for fields (`slidenum`, `datetime…`, `footer`).
pub trait Fields {
    fn field(&self, kind: &str) -> Option<String>;
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
}

/// One shaped character cell of a paragraph.
#[derive(Clone)]
struct Cell {
    ch: char,
    style: usize,
    /// Glyphs anchored at this char (gid, x offset from char start, y offset).
    glyphs: Vec<(u32, f64, f64)>,
    adv: f64,
    face: usize,
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
    }
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
        };
    };
    let props = resolve::para(ctx, shape, para);
    let mut styles: Vec<Style> = vec![];
    let mut faces: Vec<Arc<FontFace>> = vec![];
    let mut cells: Vec<Cell> = vec![];
    let face_index = |faces: &mut Vec<Arc<FontFace>>, f: &Arc<FontFace>| -> usize {
        if let Some(i) = faces.iter().position(|x| Arc::ptr_eq(x, f)) {
            return i;
        }
        faces.push(f.clone());
        faces.len() - 1
    };
    for run in &para.runs {
        let rp = resolve::run(ctx, shape, para, &run.props);
        let st = make_style(ctx, &rp, scale, opts.prompt_color);
        let text: String = match &run.kind {
            RunKind::Text => run.text.clone(),
            RunKind::Break => "\u{b}".into(),
            RunKind::Field { field } => opts.fields.field(field).unwrap_or_else(|| run.text.clone()),
            RunKind::Math { .. } => run.text.clone(),
        };
        let si = styles.len();
        // Caps mapping and small-caps sizing.
        let small = st.caps == Caps::Small;
        let upper = st.caps != Caps::None;
        styles.push(st);
        let Some(stl) = styles.get(si) else { continue };
        // Split into face segments by coverage.
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars.get(i).copied().unwrap_or(' ');
            let base = &stl.face;
            let face = if c.is_control() || c.is_whitespace() || base.covers(c) {
                base.clone()
            } else {
                db.fallback_for(c, base.id()).unwrap_or_else(|| base.clone())
            };
            let mut j = i + 1;
            while j < chars.len() {
                let d = chars.get(j).copied().unwrap_or(' ');
                let same = if d.is_whitespace() || d.is_control() {
                    true
                } else if Arc::ptr_eq(&face, base) {
                    base.covers(d)
                } else {
                    !base.covers(d) && face.covers(d)
                };
                if !same {
                    break;
                }
                j += 1;
            }
            let seg: String = chars.get(i..j).map(|s| s.iter().collect()).unwrap_or_default();
            let fi = face_index(&mut faces, &face);
            let upem = face.upem.max(1.0);
            let sz = |ch: char| if small && ch.is_lowercase() { stl.size * 0.8 } else { stl.size };
            let shaped = deckcraft_fonts::shape(&face, &seg, &[], |ch| if upper { ch.to_uppercase().next().unwrap_or(ch) } else { ch });
            // byte offset → char index within seg
            let mut byte_to_char = vec![0usize; seg.len() + 1];
            for (ci, (b, _)) in seg.char_indices().enumerate() {
                if let Some(x) = byte_to_char.get_mut(b) {
                    *x = ci;
                }
            }
            let start_cell = cells.len();
            for ch in seg.chars() {
                cells.push(Cell { ch, style: si, glyphs: vec![], adv: 0.0, face: fi });
            }
            for g in shaped {
                let ci = byte_to_char.get(g.cluster).copied().unwrap_or(0);
                if let Some(cell) = cells.get_mut(start_cell + ci) {
                    let s = sz(cell.ch) / upem;
                    let ch = cell.ch;
                    if ch == '\t' || ch == '\u{b}' || ch == '\n' {
                        continue;
                    }
                    cell.glyphs.push((g.gid, cell.adv + g.x_offset as f64 * s, -(g.y_offset as f64) * s));
                    cell.adv += g.x_advance as f64 * s;
                }
            }
            for cell in cells.iter_mut().skip(start_cell) {
                if !cell.glyphs.is_empty() || cell.ch == ' ' {
                    cell.adv += stl.spacing;
                }
            }
            i = j;
        }
    }
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
    ParaShaped { cells, styles, faces, props, lead_size, lead_face, bullet, level: para.level }
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
            if i > start && allowed.get(i).copied().unwrap_or(false) {
                last_break = Some(i);
            }
            if wrap && x + adv > avail + 0.01 && !c.is_whitespace() && i > start {
                // Break at the last opportunity, or force mid-word.
                let b = last_break.filter(|b| *b > start).unwrap_or(i);
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
            if let Some(l) = lines.get_mut(last) {
                l.end = (start + 1).min(n);
                l.next = l.end;
            }
            start += 1;
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
    let mut all_lines: Vec<(LineInfo, Vec<(usize, f64, f64)>, f64, Option<(String, f64, f64)>)> = vec![];
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
            let avail = col_w - x0 - mr;
            // Positions per char.
            let mut xs = vec![];
            let mut x = 0.0;
            for i in rl.start..rl.end {
                xs.push(x);
                let c = p.cells.get(i);
                let adv = match c.map(|c| c.ch) {
                    Some('\t') => tab_advance(x0 + x, &tabs, props.default_tab.unwrap_or(72.0)),
                    _ => c.map(|c| c.adv).unwrap_or(0.0),
                };
                x += adv;
            }
            xs.push(x);
            let width = x;
            let align = props.align.unwrap_or(Align::Left);
            let last_line = li + 1 == raw.len();
            let slack = (avail - width).max(0.0);
            let (shift, extra_per_space, extra_per_char) = match align {
                Align::Left => (0.0, 0.0, 0.0),
                Align::Center => (slack / 2.0, 0.0, 0.0),
                Align::Right => (slack, 0.0, 0.0),
                Align::Justify => {
                    let spaces = (rl.start..rl.end).filter(|i| p.cells.get(*i).is_some_and(|c| c.ch == ' ')).count();
                    if last_line || spaces == 0 || !wrap { (0.0, 0.0, 0.0) } else { (0.0, slack / spaces as f64, 0.0) }
                }
                Align::Distributed => {
                    let n = rl.end.saturating_sub(rl.start);
                    if n > 1 { (0.0, 0.0, slack / (n - 1) as f64) } else { (slack / 2.0, 0.0, 0.0) }
                }
            };
            let mut adj = vec![];
            let mut extra = 0.0;
            for (k, i) in (rl.start..=rl.end).enumerate() {
                adj.push(xs.get(k).copied().unwrap_or(width) + extra + shift + x0);
                if i < rl.end {
                    if p.cells.get(i).is_some_and(|c| c.ch == ' ') {
                        extra += extra_per_space;
                    }
                    extra += extra_per_char;
                }
            }
            let mut glyph_cells = vec![];
            for (k, i) in (rl.start..rl.end).enumerate() {
                glyph_cells.push((i, adj.get(k).copied().unwrap_or(0.0), 0.0));
            }
            let bullet = if li == 0 {
                p.bullet.as_ref().map(|(s, _)| {
                    let bx = if matches!(align, Align::Center | Align::Right) && p.cells.is_empty() {
                        shift + x0 - bullet_w
                    } else {
                        (margin + indent).max(0.0) + if align == Align::Center || align == Align::Right { shift } else { 0.0 }
                    };
                    (s.clone(), bx, baseline)
                })
            } else {
                None
            };
            out.content_width = out.content_width.max(width + x0 + mr);
            all_lines.push((
                LineInfo { para: pi, start: rl.start, end: rl.end, top, baseline, bottom: y, caret_x: adj, column: 0 },
                glyph_cells,
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
            let dx = col as f64 * (col_w + col_gap);
            for x in &mut li.caret_x {
                *x += dx;
            }
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
    // Emit runs.
    for (line, cells, _, bullet) in all_lines.iter_mut() {
        line.top += oy;
        line.baseline += oy;
        line.bottom += oy;
        for x in &mut line.caret_x {
            *x += ox;
        }
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
        // Group consecutive cells by (style, face).
        let mut cur: Option<GlyphRun> = None;
        let mut cur_key = (usize::MAX, usize::MAX);
        for (ci, x, _) in cells.iter() {
            let Some(cell) = p.cells.get(*ci) else { continue };
            let Some(st) = p.styles.get(cell.style) else { continue };
            let _ = x;
            let x = line.caret_x.get(ci.saturating_sub(line.start)).copied().unwrap_or(0.0);
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
                    chars: (*ci, *ci),
                    link: st.link,
                    alpha: st.alpha,
                    gradient: st.gradient.clone(),
                });
                cur_key = key;
            }
            if let Some(r) = cur.as_mut() {
                r.chars.1 = ci + 1;
                for (gid, gx, gy) in &cell.glyphs {
                    let k = if st.baseline != 0.0 { 0.66 } else { 1.0 };
                    r.glyphs.push((*gid, x + gx * k, base_y + gy));
                    r.cells.push((*ci, cell.ch));
                }
            }
            let adv = line.caret_x.get(ci - line.start + 1).copied().unwrap_or(x) - line.caret_x.get(ci - line.start).copied().unwrap_or(x);
            let cx = line.caret_x.get(ci - line.start).copied().unwrap_or(x);
            if let Some(hl) = st.highlight {
                out.decos.push(Deco { rect: Rect::new(cx, line.top, cx + adv, line.bottom), color: hl, behind: true, para: line.para });
            }
            if !cell.ch.is_whitespace() || st.underline.is_some() {
                if let Some(uc) = st.underline {
                    let t = (st.size * 0.06).max(0.5);
                    let uy = line.baseline + st.size * 0.12;
                    out.decos.push(Deco { rect: Rect::new(cx, uy, cx + adv, uy + t), color: uc, behind: false, para: line.para });
                }
                if let Some(sk) = st.strike {
                    let t = (st.size * 0.05).max(0.5);
                    let sy = line.baseline - st.size * 0.3;
                    out.decos.push(Deco { rect: Rect::new(cx, sy, cx + adv, sy + t), color: st.color, behind: false, para: line.para });
                    if sk == Strike::Double {
                        out.decos.push(Deco { rect: Rect::new(cx, sy - t * 2.0, cx + adv, sy - t), color: st.color, behind: false, para: line.para });
                    }
                }
            }
        }
        if let Some(r) = cur.take() {
            out.runs.push(r);
        }
    }
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

/// A caret position: paragraph and character offset within it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub para: usize,
    pub ch: usize,
}

impl TextLayout {
    /// The line containing a caret position (the later line at a soft wrap boundary).
    pub fn line_of(&self, pos: Pos) -> Option<usize> {
        let mut found = None;
        for (i, l) in self.lines.iter().enumerate() {
            if l.para == pos.para && pos.ch >= l.start && pos.ch <= l.end {
                found = Some(i);
                if pos.ch < l.end {
                    break;
                }
            }
        }
        found.or_else(|| self.lines.iter().rposition(|l| l.para == pos.para))
    }
    /// Caret line segment (x, top, bottom) in shape-local, unrotated coordinates.
    pub fn caret(&self, pos: Pos) -> Option<(f64, f64, f64)> {
        let li = self.line_of(pos)?;
        let l = self.lines.get(li)?;
        let k = pos.ch.clamp(l.start, l.end) - l.start;
        let x = l.caret_x.get(k).copied().or(l.caret_x.last().copied())?;
        Some((x, l.top, l.bottom))
    }
    /// Nearest caret position for a point.
    pub fn hit(&self, p: Point) -> Pos {
        let mut best: Option<&LineInfo> = None;
        for l in &self.lines {
            if p.y >= l.top && p.y < l.bottom {
                // In multi-column bodies pick the line whose column contains x.
                if best.is_none() || l.caret_x.first().is_some_and(|x0| p.x >= *x0 - 4.0) {
                    best = Some(l);
                }
            }
        }
        let l = match best {
            Some(l) => l,
            None => {
                if self.lines.is_empty() {
                    return Pos::default();
                }
                let first = self.lines.first();
                let last = self.lines.last();
                match (first, last) {
                    (Some(f), _) if p.y < f.top => f,
                    (_, Some(l)) => l,
                    _ => return Pos::default(),
                }
            }
        };
        let mut idx = l.end;
        for k in 0..l.caret_x.len().saturating_sub(1) {
            let (a, b) = (l.caret_x.get(k).copied().unwrap_or(0.0), l.caret_x.get(k + 1).copied().unwrap_or(0.0));
            if p.x < (a + b) / 2.0 {
                idx = l.start + k;
                break;
            }
        }
        Pos { para: l.para, ch: idx }
    }
    /// Selection highlight rectangles between two positions.
    pub fn selection_rects(&self, a: Pos, b: Pos) -> Vec<Rect> {
        let (s, e) = if a <= b { (a, b) } else { (b, a) };
        let mut out = vec![];
        for l in &self.lines {
            let lp = Pos { para: l.para, ch: l.start };
            let le = Pos { para: l.para, ch: l.end };
            if le < s || lp > e {
                continue;
            }
            let from = if s > lp { s.ch - l.start } else { 0 };
            let to = if e < le { e.ch.saturating_sub(l.start) } else { l.end - l.start };
            let x0 = l.caret_x.get(from).copied().unwrap_or(0.0);
            let mut x1 = l.caret_x.get(to).copied().unwrap_or(x0);
            // Selecting across a paragraph end shows a small extra box for the newline.
            if e.para > l.para && l.end == to + l.start {
                x1 += 6.0;
            }
            if x1 > x0 {
                out.push(Rect::new(x0, l.top, x1, l.bottom));
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
    /// Start/end of the visual line containing `pos`.
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
            let x0 = l.caret_x.first().copied().unwrap_or(0.0);
            let x1 = l.caret_x.last().copied().unwrap_or(x0);
            let lr = Rect::new(x0, l.top, x1.max(x0), l.bottom);
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
    let w = l.lines.iter().map(|li| li.caret_x.last().copied().unwrap_or(0.0) - wide.x0 - bp.inset_l.unwrap_or(7.2)).fold(0.0, f64::max);
    w + bp.inset_l.unwrap_or(7.2) + bp.inset_r.unwrap_or(7.2) + 1.0
}

#[cfg(test)]
mod tests;
