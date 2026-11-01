//! TeX-style layout of equations (`equashioncraft_math::Math`) into positioned glyphs and rules.
//!
//! The result is a [`MathBox`]: a width, an ascent and a descent around a baseline, plus glyph
//! items and rules in box-local coordinates (x right, y down, baseline at y = 0). A host's
//! paragraph shaper can treat the box as one inline atom; [`emit`] turns it into backend-neutral
//! scene [`Prim`]itives.
//!
//! Metrics come from glyph outlines and the font's own vertical metrics, so layout works with
//! whatever face resolves (including the last-resort font when craft-fonts is absent). Radicals
//! and delimiters grow by scaling the glyph; fractions, radical bars, bars and borders are rules.
//! Recursion is bounded by `MAX_DEPTH` and the number of laid-out nodes by a budget.

use std::collections::HashMap;
use std::sync::Arc;

use equashioncraft_font::FontFace;
use equashioncraft_math::{MAX_DEPTH, Math, Node, Style as MStyle, from_omml};
use kurbo::{Affine, BezPath, Rect, Shape as _};

use crate::MathFonts;
use crate::scene::{Prim, Rgb, Scene};

/// Most nodes laid out for one equation; the rest is dropped.
const NODE_BUDGET: usize = 20_000;
/// Most layouts a [`LayoutCache`] keeps before it starts over.
const CACHE_MAX: usize = 256;
/// Largest growth factor of a stretched delimiter or radical.
const MAX_STRETCH: f64 = 8.0;
/// Thickness of fraction bars and radical bars, em.
const RULE: f64 = 0.055;

#[derive(Clone)]
pub(crate) struct GlyphItem {
    pub face: Arc<FontFace>,
    pub size: f64,
    pub glyphs: Vec<(u32, f64, f64)>,
    pub fake_bold: bool,
    pub fake_italic: bool,
}

#[derive(Clone)]
pub(crate) enum Item {
    Glyphs(GlyphItem),
    Rule(Rect),
    /// A filled vector shape (radical sign, delimiter, big operator).
    Path(BezPath),
}

/// A laid-out equation (or part of one).
#[derive(Clone, Default)]
pub struct MathBox {
    pub(crate) w: f64,
    pub(crate) asc: f64,
    pub(crate) desc: f64,
    pub(crate) items: Vec<Item>,
    /// Caret geometry of every row laid out inside this box (box-local).
    pub(crate) geo: Vec<SeqGeo>,
    /// Width of the last line when the equation was broken into several (inline equations then
    /// sit with that line on the text baseline, so following text continues after it).
    pub(crate) last_w: Option<f64>,
}

/// Where the caret positions of one row (a child sequence of the math tree) sit.
#[derive(Clone, Debug, Default)]
pub struct SeqGeo {
    /// Route to the row, as in `equashioncraft_math::Path`.
    pub path: Vec<(usize, usize)>,
    /// x of each unit boundary: `units + 1` entries.
    pub xs: Vec<f64>,
    /// Vertical extent of the caret in this row (y down, baseline 0).
    pub top: f64,
    pub bottom: f64,
    /// Horizontal extent of the row when it is wider than its caret positions say: an empty slot
    /// has one caret position but shows a box to click in.
    pub x_range: Option<(f64, f64)>,
}

impl MathBox {
    /// Width, points.
    pub fn width(&self) -> f64 {
        self.w
    }
    /// Width of the last line of a broken-up inline equation; `None` for an equation on one line.
    pub fn last_line_width(&self) -> Option<f64> {
        self.last_w
    }
    /// Height above the baseline, points.
    pub fn ascent(&self) -> f64 {
        self.asc
    }
    /// Depth below the baseline, points.
    pub fn descent(&self) -> f64 {
        self.desc
    }
    fn space(w: f64) -> MathBox {
        MathBox { w: w.max(0.0), ..Default::default() }
    }
    /// Add `o` with its origin at (dx, dy) (y down); grows the box to hold it.
    fn put(&mut self, o: MathBox, dx: f64, dy: f64) {
        self.w = self.w.max(dx + o.w);
        self.asc = self.asc.max(o.asc - dy);
        self.desc = self.desc.max(o.desc + dy);
        for mut g in o.geo {
            for x in &mut g.xs {
                *x += dx;
            }
            if let Some((a, b)) = g.x_range.as_mut() {
                *a += dx;
                *b += dx;
            }
            g.top += dy;
            g.bottom += dy;
            self.geo.push(g);
        }
        for it in o.items {
            self.items.push(match it {
                Item::Glyphs(mut g) => {
                    for p in &mut g.glyphs {
                        p.1 += dx;
                        p.2 += dy;
                    }
                    Item::Glyphs(g)
                }
                Item::Rule(r) => Item::Rule(Rect::new(r.x0 + dx, r.y0 + dy, r.x1 + dx, r.y1 + dy)),
                Item::Path(mut p) => {
                    p.apply_affine(Affine::translate((dx, dy)));
                    Item::Path(p)
                }
            });
        }
    }
    fn rule(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        if x1 > x0 && y1 > y0 {
            self.items.push(Item::Rule(Rect::new(x0, y0, x1, y1)));
            self.asc = self.asc.max(-y0);
            self.desc = self.desc.max(y1);
            self.w = self.w.max(x1);
        }
    }
    /// Add a filled shape; grows the box to hold it.
    fn path(&mut self, p: BezPath) {
        let b = p.bounding_box();
        if b.x1 > b.x0 && b.y1 > b.y0 && b.x0.is_finite() && b.y0.is_finite() && b.x1.is_finite() && b.y1.is_finite() {
            self.asc = self.asc.max(-b.y0);
            self.desc = self.desc.max(b.y1);
            self.w = self.w.max(b.x1);
            self.items.push(Item::Path(p));
        }
    }
    /// Boxes side by side on a common baseline.
    fn row(parts: Vec<MathBox>) -> MathBox {
        let mut out = MathBox::default();
        let mut x = 0.0;
        for p in parts {
            let w = p.w;
            out.put(p, x, 0.0);
            x += w;
        }
        out.w = x;
        out
    }
    /// Number of glyph items (tests).
    #[cfg(test)]
    pub(crate) fn glyph_items(&self) -> usize {
        self.items.iter().filter(|i| matches!(i, Item::Glyphs(_))).count()
    }
    /// Number of rules (tests).
    #[cfg(test)]
    pub(crate) fn rules(&self) -> usize {
        self.items.iter().filter(|i| matches!(i, Item::Rule(_))).count()
    }
    /// Number of filled shapes (tests).
    #[cfg(test)]
    pub(crate) fn paths(&self) -> usize {
        self.items.iter().filter(|i| matches!(i, Item::Path(_))).count()
    }
    /// Largest glyph size used (tests).
    #[cfg(test)]
    pub(crate) fn max_size(&self) -> f64 {
        self.items.iter().filter_map(|i| if let Item::Glyphs(g) = i { Some(g.size) } else { None }).fold(0.0, f64::max)
    }
}

/// Style of the surrounding formula: display, text, script, script-script.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum St {
    Display,
    Text,
    Script,
    ScriptScript,
}

impl St {
    fn factor(self) -> f64 {
        match self {
            St::Display | St::Text => 1.0,
            St::Script => 0.72,
            St::ScriptScript => 0.58,
        }
    }
    fn script(self) -> St {
        match self {
            St::Display | St::Text => St::Script,
            _ => St::ScriptScript,
        }
    }
    /// Style of fraction numerators and denominators.
    fn inner(self) -> St {
        match self {
            St::Display => St::Text,
            St::Text => St::Script,
            _ => St::ScriptScript,
        }
    }
    fn is_script(self) -> bool {
        self >= St::Script
    }
}

/// Caret unit mark: x lies at `f` between part boundaries `a` and `b`.
type Mark = (usize, usize, f64);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Var {
    Italic,
    Upright,
    Bold,
    BoldItalic,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    None,
    Ord,
    Op,
    Bin,
    Rel,
    Punct,
    /// A built-up fraction: it takes a thin space from its neighbours (outside scripts).
    Inner,
}

/// Where a trailing differential (`dx`, `dt`...) starts in an operator's body: the last text
/// node ends in `d` and one letter, with something before it.
fn differential(s: &[Node]) -> Option<(usize, usize)> {
    let i = s.len().checked_sub(1)?;
    let Node::Text(t) = s.get(i)?.bare() else { return None };
    let chars: Vec<char> = t.chars().collect();
    let n = chars.len();
    let d = n.checked_sub(2)?;
    let (a, b) = (*chars.get(d)?, *chars.get(n - 1)?);
    let before = (d > 0 || i > 0) && d.checked_sub(1).and_then(|k| chars.get(k)).is_none_or(|c| space_em(*c).is_none());
    (a == 'd'
        && b.is_alphabetic()
        && before
        && (d.checked_sub(1).and_then(|k| chars.get(k)).is_none_or(|c| !c.is_alphabetic())
            || d.checked_sub(2).and_then(|k| chars.get(k)).is_none_or(|c| !c.is_alphabetic())))
    .then_some((i, d))
}

/// Width in em of the explicit space characters the typed-LaTeX parser produces: thin
/// (`\,`), medium (`\:`), thick (`\;`) and one em (`\quad`).
fn space_em(c: char) -> Option<f64> {
    match c {
        '\u{2009}' => Some(3.0 / 18.0),
        '\u{2005}' => Some(4.0 / 18.0),
        '\u{2004}' => Some(5.0 / 18.0),
        '\u{2001}' => Some(1.0),
        _ => None,
    }
}

fn classify(c: char) -> Class {
    match c {
        '=' | '<' | '>' | '\u{2264}' | '\u{2265}' | '\u{2260}' | '\u{2248}' | '\u{2261}' | '\u{223C}' | '\u{2245}' | '\u{2208}' | '\u{2209}'
        | '\u{220B}' | '\u{2282}' | '\u{2283}' | '\u{2286}' | '\u{2287}' | '\u{2192}' | '\u{2190}' | '\u{2194}' | '\u{21D2}' | '\u{21D0}'
        | '\u{21D4}' | '\u{221D}' | '\u{2225}' | '\u{22A5}' | '\u{226A}' | '\u{226B}' | '\u{27F6}' | '\u{21A6}' | '\u{2223}' | '\u{2243}'
        | '\u{2250}' => Class::Rel,
        '+' | '\u{2212}' | '-' | '\u{D7}' | '\u{F7}' | '\u{B7}' | '\u{B1}' | '\u{2213}' | '\u{2217}' | '\u{2218}' | '\u{222A}' | '\u{2229}'
        | '\u{2227}' | '\u{2228}' | '\u{2295}' | '\u{2297}' | '\u{2216}' | '*' | '\u{22C5}' => Class::Bin,
        ',' | ';' => Class::Punct,
        _ => Class::Ord,
    }
}

/// Space (in em) between two adjacent atoms.
fn gap(prev: Class, next: Class, script: bool) -> f64 {
    // A little tighter than TeX's 4/18 and 5/18 (the serif face has wide side bearings), and a
    // small space kept in script size so "1+x" in a script-size denominator does not run together.
    let med = if script { 3.5 / 18.0 } else { 5.0 / 18.0 };
    let thick = if script { 0.0 } else { 6.0 / 18.0 };
    let thin = 3.0 / 18.0;
    match (prev, next) {
        (Class::None, _) => 0.0,
        (Class::Bin, _) | (_, Class::Bin) => med,
        (Class::Rel, _) | (_, Class::Rel) => thick,
        (Class::Punct, _) => {
            if script {
                0.0
            } else {
                thin
            }
        }
        (Class::Op, _) | (_, Class::Op) => thin,
        (Class::Inner, _) | (_, Class::Inner) => {
            if script {
                0.0
            } else {
                thin
            }
        }
        _ => 0.0,
    }
}

/// Add a closed polygon to `p`, wound the same way whatever the order of `pts` (so overlapping
/// pieces of one shape never cancel under the non-zero rule).
fn add_poly(p: &mut BezPath, pts: &[(f64, f64)]) {
    let n = pts.len();
    if n < 3 {
        return;
    }
    let mut area = 0.0;
    for i in 0..n {
        let (a, b) = (pts.get(i).copied().unwrap_or_default(), pts.get((i + 1) % n).copied().unwrap_or_default());
        area += a.0 * b.1 - b.0 * a.1;
    }
    let ordered: Vec<(f64, f64)> = if area >= 0.0 { pts.to_vec() } else { pts.iter().rev().copied().collect() };
    for (i, pt) in ordered.iter().enumerate() {
        if i == 0 {
            p.move_to(*pt);
        } else {
            p.line_to(*pt);
        }
    }
    p.close_path();
}

/// A straight stroke of perpendicular thickness `w` from `a` to `b`; `ext` lengthens it by half a
/// width at both ends so consecutive strokes overlap at their joints.
fn add_segment(p: &mut BezPath, a: (f64, f64), b: (f64, f64), w: f64, ext: bool) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    if !(len.is_finite() && len > 1e-9) {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let e = if ext { w / 2.0 } else { 0.0 };
    let (a, b) = ((a.0 - ux * e, a.1 - uy * e), (b.0 + ux * e, b.1 + uy * e));
    let (nx, ny) = (-uy * w / 2.0, ux * w / 2.0);
    add_poly(p, &[(a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny), (b.0 - nx, b.1 - ny), (a.0 - nx, a.1 - ny)]);
}

/// Narrow a box made of one glyph run to its ink's right edge (plus a little), so scripts hang
/// by the glyph rather than by its side bearing.
fn trim_right(fonts: &dyn MathFonts, mut b: MathBox, sz: f64) -> MathBox {
    let mut right: f64 = 0.0;
    for it in &b.items {
        if let Item::Glyphs(g) = it {
            let k = g.size / g.face.upem.max(1.0);
            for (gid, x, _) in &g.glyphs {
                if let Some((_, _, x1, _)) = ink(fonts, &g.face, *gid) {
                    right = right.max(x + x1 * k);
                }
            }
        }
    }
    if right > 0.0 && right + 0.03 * sz < b.w {
        b.w = right + 0.03 * sz;
    }
    b
}

/// A parenthesis from `top` to `bot` (y down): a crescent that is thickest in the middle and
/// never thinner than a hairline at the tips. `x` is the tip column; the belly bulges left for an
/// opening parenthesis and right for a closing one.
fn paren_path(open: bool, x: f64, top: f64, bot: f64, bulge: f64, thick: f64, tip: f64) -> BezPath {
    let dir = if open { -1.0 } else { 1.0 };
    let h = bot - top;
    let k = 4.0 / 3.0; // cubic control offset for a bulge of `b` at the middle
    let curve = |p: &mut BezPath, x0: f64, b: f64, from_top: bool| {
        let y1 = if from_top { bot } else { top };
        let cx = x0 + dir * b * k;
        let (c1, c2) = if from_top { (top + h * 0.12, bot - h * 0.12) } else { (bot - h * 0.12, top + h * 0.12) };
        p.curve_to((cx, c1), (cx, c2), (x0, y1));
    };
    let mut p = BezPath::new();
    p.move_to((x, top));
    curve(&mut p, x, bulge, true);
    let inner_x = x - dir * tip;
    p.line_to((inner_x, bot));
    let inner_bulge = (bulge - thick + tip).max(0.0);
    curve(&mut p, inner_x, inner_bulge, false);
    p.close_path();
    p
}

/// The four faces of the equation font family.
struct Faces {
    regular: Arc<FontFace>,
    italic: Arc<FontFace>,
    bold: Arc<FontFace>,
    bold_italic: Arc<FontFace>,
}

impl Faces {
    fn new(db: &dyn MathFonts, family: &str) -> Faces {
        Faces {
            regular: db.face(family, "Regular"),
            italic: db.face(family, "Italic"),
            bold: db.face(family, "Bold"),
            bold_italic: db.face(family, "Bold Italic"),
        }
    }
    fn get(&self, v: Var) -> &Arc<FontFace> {
        match v {
            Var::Upright => &self.regular,
            Var::Italic => &self.italic,
            Var::Bold => &self.bold,
            Var::BoldItalic => &self.bold_italic,
        }
    }
}

/// The layout engine for one equation: fonts, size and the state of the row being laid out.
struct Lay<'f> {
    fonts: &'f dyn MathFonts,
    faces: Faces,
    /// Font size of the text style, points.
    size: f64,
    bold: bool,
    budget: usize,
    /// Route to the row being laid out (see `SeqGeo::path`).
    path: Vec<(usize, usize)>,
    /// Index, in its row, of the node being laid out.
    cur: usize,
    /// Widest the top-level row may be before it breaks into several lines, points.
    wrap: f64,
    /// Parts of the top-level row after which it may break (after a binary operator or relation).
    brks: Vec<usize>,
    /// The next row laid out is the body of a big operator (so a trailing `dx` takes a thin space).
    nary_body: bool,
    /// (node, character) in the row being laid out where a differential `dx` starts.
    thin_at: Option<(usize, usize)>,
    /// The next row laid out is the empty base of a script that has something after it (the `n`
    /// of `ₙC_r`): it shows no box, only the scripts are places.
    bare_base: bool,
}

/// Ink box of a glyph in font units, y down: (x0, y0, x1, y1).
fn ink(fonts: &dyn MathFonts, face: &FontFace, gid: u32) -> Option<(f64, f64, f64, f64)> {
    let p = fonts.outline(face, gid);
    if p.elements().is_empty() {
        return None;
    }
    let b = p.bounding_box();
    if b.x0.is_finite() && b.y0.is_finite() && b.x1.is_finite() && b.y1.is_finite() && b.y1 > b.y0 { Some((b.x0, b.y0, b.x1, b.y1)) } else { None }
}

fn is_upper_greek(c: char) -> bool {
    ('\u{391}'..='\u{3A9}').contains(&c)
}

impl Lay<'_> {
    fn pick(&self, v: Var, c: char) -> Arc<FontFace> {
        let base = self.faces.get(v);
        if c.is_whitespace() || c.is_control() || base.covers(c) {
            return base.clone();
        }
        self.fonts.fallback_for(c, base.id()).unwrap_or_else(|| base.clone())
    }

    /// Ink box of a glyph in font units, y down (see [`ink`]).
    fn ink(&self, face: &FontFace, gid: u32) -> Option<(f64, f64, f64, f64)> {
        ink(self.fonts, face, gid)
    }

    fn var_for(&self, c: char, v: Var) -> Var {
        // Math italic applies to letters only; digits and symbols stay upright.
        let italic_letter = !is_upper_greek(c) && (c.is_ascii_alphabetic() || ('\u{3B1}'..='\u{3C9}').contains(&c) || c == '\u{3D1}');
        match (v, self.bold) {
            (Var::Italic, b) => match (italic_letter, b) {
                (true, false) => Var::Italic,
                (true, true) => Var::BoldItalic,
                (false, false) => Var::Upright,
                (false, true) => Var::Bold,
            },
            (Var::Upright, true) => Var::Bold,
            (other, _) => other,
        }
    }

    /// A string at `sz` points in variant `v` (per-character italic choice for `Var::Italic`).
    fn text(&self, s: &str, v: Var, sz: f64) -> MathBox {
        let mut out = MathBox::default();
        let mut x = 0.0;
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars.get(i).copied().unwrap_or(' ');
            // Wide spaces are fixed gaps, not glyphs (the math fonts may lack them): column breaks.
            let gap = match c {
                '\u{2003}' => Some(1.0),
                '\u{2002}' => Some(0.5),
                c => space_em(c),
            };
            if let Some(g) = gap {
                x += g * sz;
                i += 1;
                continue;
            }
            let cv = self.var_for(c, v);
            let face = self.pick(cv, c);
            let mut j = i + 1;
            while j < chars.len() {
                let d = chars.get(j).copied().unwrap_or(' ');
                let dv = self.var_for(d, v);
                if matches!(d, '\u{2003}' | '\u{2002}') || space_em(d).is_some() || dv != cv || !Arc::ptr_eq(&self.pick(dv, d), &face) {
                    break;
                }
                j += 1;
            }
            let seg: String = chars.get(i..j).map(|s| s.iter().collect()).unwrap_or_default();
            let k = sz / face.upem.max(1.0);
            let shaped = equashioncraft_font::shape(&face, &seg, &[], |c| c);
            let mut item = GlyphItem {
                face: face.clone(),
                size: sz,
                glyphs: vec![],
                fake_bold: matches!(cv, Var::Bold | Var::BoldItalic) && face.weight < 550.0,
                fake_italic: matches!(cv, Var::Italic | Var::BoldItalic) && !face.italic,
            };
            for g in shaped {
                let gx = x + g.x_offset as f64 * k;
                let gy = -(g.y_offset as f64) * k;
                if let Some((_, y0, _, y1)) = self.ink(&face, g.gid) {
                    out.asc = out.asc.max(-y0 * k - gy);
                    out.desc = out.desc.max(y1 * k + gy);
                }
                item.glyphs.push((g.gid, gx, gy));
                x += g.x_advance as f64 * k;
            }
            if !item.glyphs.is_empty() {
                out.items.push(Item::Glyphs(item));
            }
            i = j;
        }
        out.w = x;
        out.asc = out.asc.max(0.0);
        out.desc = out.desc.max(0.0);
        out
    }

    fn x_height(&self, sz: f64) -> f64 {
        let f = self.faces.get(Var::Upright);
        let xh = f.x_height / f.upem.max(1.0) * sz;
        if xh.is_finite() && xh > 0.0 { xh } else { 0.45 * sz }
    }

    /// Ink extent (width, height) of a character in the upright face, in em; `None` if the face
    /// has no outline for it (no fonts installed).
    fn ink_em(&self, c: char) -> Option<(f64, f64)> {
        let face = self.faces.get(Var::Upright);
        let gid = face.glyph_for(c);
        let (x0, y0, x1, y1) = (gid != 0).then(|| self.ink(face, gid)).flatten()?;
        let u = face.upem.max(1.0);
        let (w, h) = ((x1 - x0) / u, (y1 - y0) / u);
        (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some((w, h))
    }

    /// Weight of horizontal rules (fraction bars, radical strokes): the thickness of the font's
    /// own minus sign, so rules match the text.
    fn hbar(&self, sz: f64) -> f64 {
        let em = self.ink_em('\u{2212}').map(|(_, h)| h).unwrap_or(RULE);
        em.clamp(0.048, 0.075) * sz
    }

    /// Weight of vertical strokes (brackets, bars): the stem of the font's own `|`.
    fn stem(&self, sz: f64) -> f64 {
        let em = self.ink_em('|').map(|(w, _)| w).unwrap_or(RULE);
        em.clamp(0.03, 0.08) * sz
    }

    fn axis(&self, sz: f64) -> f64 {
        0.25 * sz
    }

    /// One character grown so its ink is `target` tall (never smaller than `sz`), its ink centre
    /// on the math axis. Falls back to the plain glyph when the font has no outline for it.
    fn stretch(&self, c: char, target: f64, sz: f64) -> MathBox {
        let face = self.pick(Var::Upright, c);
        let gid = face.glyph_for(c);
        let Some((_, y0, _, y1)) = (gid != 0).then(|| self.ink(&face, gid)).flatten() else {
            return self.text(&c.to_string(), Var::Upright, sz);
        };
        let upem = face.upem.max(1.0);
        let natural = (y1 - y0) / upem; // ink height per point of size
        let want = if target.is_finite() { target } else { 0.0 };
        let s = (want / natural).clamp(sz, sz * MAX_STRETCH);
        let b = self.text(&c.to_string(), Var::Upright, s);
        let centre = (y0 + y1) / 2.0 * s / upem;
        let dy = -self.axis(sz) - centre;
        let w = b.w;
        let mut shifted = MathBox::default();
        shifted.put(b, 0.0, dy);
        shifted.w = w;
        shifted
    }

    fn seq(&mut self, s: &[Node], st: St, depth: usize) -> MathBox {
        let mut parts: Vec<MathBox> = vec![];
        let mut marks: Vec<Mark> = vec![];
        let mut prev = Class::None;
        let path = self.path.clone();
        let body_of_nary = std::mem::take(&mut self.nary_body);
        let bare = std::mem::take(&mut self.bare_base);
        let thin = if body_of_nary { differential(s) } else { None };
        for (i, n) in s.iter().enumerate() {
            if self.budget == 0 {
                break;
            }
            self.cur = i;
            self.thin_at = thin;
            // A script with no base and something after it (the `n` of `ₙC_r`): its base shows no box.
            self.bare_base = i + 1 < s.len() && matches!(n, Node::Script { base, sub, sup } if base.is_empty() && (sub.is_some() || sup.is_some()));
            self.node(n, st, depth, &mut prev, &mut parts, &mut marks);
            self.bare_base = false;
        }
        // An empty argument of a structure (under a radical, in a fraction, in a script) shows a
        // dotted box, so it reads as a place to type. The top-level row stays blank.
        let slot = s.is_empty() && !path.is_empty() && !bare && self.budget > 0;
        if slot {
            parts.push(self.slot_box(self.size * st.factor()));
        }
        // x of every part boundary, then of every caret unit.
        let mut pre = vec![0.0];
        let mut x = 0.0;
        for p in &parts {
            x += p.w;
            pre.push(x);
        }
        let at = |k: usize| pre.get(k).copied().unwrap_or(x);
        let mut xs: Vec<f64> = marks.iter().map(|m| at(m.0) + m.2 * (at(m.1) - at(m.0))).collect();
        xs.push(x);
        let sz = self.size * st.factor();
        // The caret of an empty slot sits just inside the box's left edge.
        let x_range = if slot {
            xs = vec![0.1 * sz];
            Some((0.0, x))
        } else {
            None
        };
        if self.path.is_empty() && depth == 0 && x > self.wrap {
            let brks = std::mem::take(&mut self.brks);
            return self.wrapped(parts, &brks, sz, st != St::Display);
        }
        let mut row = MathBox::row(parts);
        row.geo.push(SeqGeo { path, xs, top: (-row.asc).min(-0.72 * sz), bottom: row.desc.max(0.22 * sz), x_range });
        row
    }

    /// The dotted box an empty slot shows: about a letter wide and as tall as a capital.
    fn slot_box(&self, sz: f64) -> MathBox {
        let (w, top, bottom) = (0.5 * sz, -0.7 * sz, 0.02 * sz);
        let t = (0.04 * sz).max(0.5);
        let d = (0.1 * sz).max(1.2);
        let mut b = MathBox::default();
        let mut x = 0.0;
        while x < w {
            let x1 = (x + d).min(w);
            b.rule(x, top, x1, top + t);
            b.rule(x, bottom - t, x1, bottom);
            x += 2.0 * d;
        }
        let mut y = top;
        while y < bottom {
            let y1 = (y + d).min(bottom);
            b.rule(0.0, y, t, y1);
            b.rule(w - t, y, w, y1);
            y += 2.0 * d;
        }
        // The box's own width and height, not its dashes' extent.
        b.w = w;
        b
    }

    /// The top-level `parts` broken into lines of at most `self.wrap` points, stacked left-aligned.
    fn wrapped(&self, parts: Vec<MathBox>, brks: &[usize], sz: f64, last_on_baseline: bool) -> MathBox {
        let max = self.wrap.max(4.0 * sz);
        // Choose where each line starts, by widths alone.
        let mut starts = vec![0usize];
        let (mut x, mut start, mut last): (f64, usize, Option<usize>) = (0.0, 0, None);
        for (i, p) in parts.iter().enumerate() {
            if brks.binary_search(&i).is_ok() && i > start {
                last = Some(i);
            }
            if x + p.w > max && i > start {
                let at = last.filter(|b| *b > start && *b <= i).unwrap_or(i);
                starts.push(at);
                x = parts.get(at..i).map_or(0.0, |s| s.iter().map(|q| q.w).sum());
                start = at;
                last = None;
            }
            x += p.w;
        }
        let mut rows: Vec<MathBox> = Vec::new();
        let mut it = parts.into_iter().enumerate().peekable();
        for r in 0..starts.len() {
            let end = starts.get(r + 1).copied().unwrap_or(usize::MAX);
            let mut line = Vec::new();
            while let Some((i, _)) = it.peek() {
                if *i >= end {
                    break;
                }
                if let Some((_, p)) = it.next() {
                    // A line does not start with the space that followed the operator.
                    if r > 0 && line.is_empty() && p.items.is_empty() && p.geo.is_empty() {
                        continue;
                    }
                    line.push(p);
                }
            }
            rows.push(MathBox::row(line));
        }
        // Offset of each line's baseline from the first line's.
        let mut offs = Vec::with_capacity(rows.len());
        let (mut dy, mut prev_desc) = (0.0, 0.0);
        for (r, row) in rows.iter().enumerate() {
            if r > 0 {
                dy += prev_desc + row.asc + 0.25 * sz;
            }
            prev_desc = row.desc;
            offs.push(dy);
        }
        // An inline equation puts its last line on the text baseline.
        let lift = if last_on_baseline { dy } else { 0.0 };
        let last_w = (last_on_baseline && rows.len() > 1).then(|| rows.last().map_or(0.0, |r| r.w));
        let mut out = MathBox::default();
        for (row, off) in rows.into_iter().zip(offs) {
            out.put(row, 0.0, off - lift);
        }
        out.last_w = last_w;
        out
    }

    fn push_gap(&self, parts: &mut Vec<MathBox>, prev: Class, next: Class, st: St) {
        let g = gap(prev, next, st.is_script()) * self.size * st.factor();
        if g > 0.0 {
            parts.push(MathBox::space(g));
        }
    }

    fn node(&mut self, n: &Node, st: St, depth: usize, prev: &mut Class, parts: &mut Vec<MathBox>, marks: &mut Vec<Mark>) {
        self.budget = self.budget.saturating_sub(1);
        let me = self.cur;
        let sz = self.size * st.factor();
        match n.bare() {
            Node::Text(t) => {
                for (ci, c) in t.chars().enumerate() {
                    let before = parts.len();
                    if self.thin_at == Some((me, ci)) {
                        // The `d` of a trailing differential: a thin space after the integrand.
                        parts.push(MathBox::space(3.0 / 18.0 * sz));
                        *prev = Class::None;
                    }
                    if c == '\u{2003}' || c == '\u{2002}' {
                        // A column break (cases): a fixed wide gap, and what follows starts afresh.
                        marks.push((before, before, 0.0));
                        parts.push(MathBox::space(if c == '\u{2003}' { sz * 1.3 } else { sz * 0.5 }));
                        *prev = Class::None;
                        continue;
                    }
                    if let Some(em) = space_em(c) {
                        // `\,` `\:` `\;` `\quad`: a fixed space; it separates, so what follows
                        // starts afresh.
                        marks.push((before, before, 0.0));
                        parts.push(MathBox::space(em * sz));
                        *prev = Class::None;
                        continue;
                    }
                    if c.is_whitespace() {
                        marks.push((before, before, 0.0));
                        continue;
                    }
                    let mut k = classify(c);
                    if k == Class::Bin && matches!(*prev, Class::None | Class::Bin | Class::Rel | Class::Punct | Class::Op) {
                        k = Class::Ord;
                    }
                    self.push_gap(parts, *prev, k, st);
                    marks.push((before, parts.len(), 0.5));
                    match self.arrow(c, sz) {
                        Some(a) => parts.push(a),
                        None => parts.push(self.text(&c.to_string(), Var::Italic, sz)),
                    }
                    if matches!(k, Class::Bin | Class::Rel) && self.path.is_empty() {
                        self.brks.push(parts.len());
                    }
                    *prev = k;
                }
            }
            Node::Styled { text, style } => {
                let (v, keep_space) = match style {
                    MStyle::Plain => (Var::Upright, false),
                    MStyle::Bold => (Var::Bold, false),
                    MStyle::BoldItalic => (Var::BoldItalic, false),
                    MStyle::Normal => (Var::Upright, true),
                };
                let s: String = if keep_space { text.clone() } else { text.chars().filter(|c| !c.is_whitespace()).collect() };
                self.push_gap(parts, *prev, Class::Ord, st);
                // The characters share one box: spread their caret positions across it.
                let n = text.chars().count().max(1);
                for k in 0..n {
                    marks.push((parts.len(), parts.len() + 1, k as f64 / n as f64));
                }
                parts.push(self.text(&s, v, sz));
                *prev = Class::Ord;
            }
            Node::Func { name, body } => {
                if depth >= MAX_DEPTH {
                    return;
                }
                self.push_gap(parts, *prev, Class::Op, st);
                marks.push((parts.len(), parts.len(), 0.0));
                let mut b = self.text(name, Var::Upright, sz);
                b.w += 3.0 / 18.0 * sz;
                parts.push(b);
                *prev = Class::Op;
                let inner = self.child(me, 0, body, st, depth + 1);
                if inner.w > 0.0 {
                    parts.push(inner);
                    *prev = Class::Ord;
                }
            }
            Node::Raw(_) => marks.push((parts.len(), parts.len(), 0.0)),
            other => {
                if depth >= MAX_DEPTH {
                    marks.push((parts.len(), parts.len(), 0.0));
                    return;
                }
                let k = match other {
                    Node::Nary { .. } | Node::Limit { .. } => Class::Op,
                    Node::Frac { .. } => Class::Inner,
                    _ => Class::Ord,
                };
                let before = parts.len();
                self.push_gap(parts, *prev, k, st);
                marks.push((before, parts.len(), 0.5));
                let b = self.structure(other, st, depth + 1, me);
                parts.push(b);
                // `lim`, like `sin`, takes a thin space before what follows.
                *prev = match other {
                    Node::Limit { .. } => Class::Op,
                    Node::Frac { .. } => Class::Inner,
                    _ => Class::Ord,
                };
            }
        }
    }

    /// `seq` for a child at nesting `depth` (stops at the depth limit).
    fn seq_in(&mut self, s: &[Node], st: St, depth: usize) -> MathBox {
        if depth >= MAX_DEPTH {
            return MathBox::default();
        }
        self.seq(s, st, depth)
    }

    /// Lay out child row `c` of the node at index `me` of the current row.
    fn child(&mut self, me: usize, c: usize, s: &[Node], st: St, depth: usize) -> MathBox {
        self.path.push((me, c));
        let b = self.seq_in(s, st, depth);
        self.path.pop();
        b
    }

    fn structure(&mut self, n: &Node, st: St, depth: usize, me: usize) -> MathBox {
        let sz = self.size * st.factor();
        match n {
            Node::Frac { num, den } => {
                let ch = st.inner();
                let nb = self.child(me, 0, num, ch, depth);
                let db = self.child(me, 1, den, ch, depth);
                self.frac(nb, db, st, sz)
            }
            Node::Rad { deg, body } => {
                let b = self.child(me, usize::from(deg.is_some()), body, st, depth);
                let d = deg.as_ref().map(|d| self.child(me, 0, d, St::ScriptScript, depth));
                self.radical(b, d, sz)
            }
            Node::Script { base, sub, sup } => {
                let is_char = base.iter().all(|n| matches!(n.bare(), Node::Text(_) | Node::Styled { .. }));
                let b = self.child(me, 0, base, st, depth);
                let sb = sub.as_ref().map(|s| self.child(me, 1, s, st.script(), depth));
                let sp = sup.as_ref().map(|s| self.child(me, 1 + usize::from(sub.is_some()), s, st.script(), depth));
                self.scripts(b, sb, sp, is_char, st)
            }
            Node::PreScript { sub, sup, base } => {
                let b = self.child(me, 2, base, st, depth);
                let sb = self.child(me, 0, sub, st.script(), depth);
                let sp = self.child(me, 1, sup, st.script(), depth);
                let pre = self.scripts(MathBox::default(), Some(sb), Some(sp), true, st);
                MathBox::row(vec![pre, b])
            }
            Node::Nary { op, sub, sup, body } => {
                let display = st == St::Display;
                let integral = matches!(*op, '\u{222B}'..='\u{2233}');
                let opsz = sz
                    * if !display {
                        1.0
                    } else if integral {
                        2.5
                    } else {
                        1.75
                    };
                let ob = self.big_op(*op, display, sz).unwrap_or_else(|| self.stretch(*op, opsz * 0.85, opsz));
                let ob = if integral { trim_right(self.fonts, ob, sz) } else { ob };
                let sb = sub.as_ref().map(|s| self.child(me, 0, s, st.script(), depth));
                let sp = sup.as_ref().map(|s| self.child(me, usize::from(sub.is_some()), s, st.script(), depth));
                self.nary_body = true;
                let body_box = self.child(me, usize::from(sub.is_some()) + usize::from(sup.is_some()), body, st, depth);
                self.nary_body = false;
                let head = if integral || !display {
                    self.scripts_tucked(ob, sb, sp, false, st, if integral && display { 0.14 } else { 0.0 })
                } else {
                    self.limits(ob, sb, sp, sz)
                };
                let mut parts = vec![head];
                if body_box.w > 0.0 {
                    parts.push(MathBox::space(3.0 / 18.0 * sz));
                    parts.push(body_box);
                }
                MathBox::row(parts)
            }
            Node::Delim { open, close, sep, items } => {
                let boxes: Vec<MathBox> = items.iter().enumerate().map(|(k, i)| self.child(me, k, i, st, depth)).collect();
                self.delimiters(open, close, sep, boxes, sz)
            }
            Node::Matrix { rows } => {
                let cs = if st == St::Display { St::Text } else { st };
                let mut k = 0usize;
                let mut cells: Vec<Vec<MathBox>> = vec![];
                for r in rows {
                    let mut row = vec![];
                    for c in r {
                        row.push(self.child(me, k, c, cs, depth));
                        k += 1;
                    }
                    cells.push(row);
                }
                self.matrix(cells, sz)
            }
            Node::EqArr { rows } => {
                let cells: Vec<Vec<MathBox>> = rows.iter().enumerate().map(|(k, r)| vec![self.child(me, k, r, st, depth)]).collect();
                self.matrix(cells, sz)
            }
            Node::Accent { ch, body } => {
                let b = self.child(me, 0, body, st, depth);
                self.accent(*ch, b, sz)
            }
            Node::Limit { lower, base, lim } => {
                let b = self.child(me, 0, base, st, depth);
                let l = self.child(me, 1, lim, st.script(), depth);
                if *lower { self.limits(b, Some(l), None, sz) } else { self.limits(b, None, Some(l), sz) }
            }
            Node::Bar { top, body } => {
                let b = self.child(me, 0, body, st, depth);
                let t = (0.05 * sz).max(0.4);
                let (w, asc, desc) = (b.w, b.asc, b.desc);
                let mut out = MathBox::default();
                out.put(b, 0.0, 0.0);
                if *top {
                    let y = -(asc + 0.12 * sz);
                    out.rule(0.0, y - t, w, y);
                } else {
                    let y = desc + 0.12 * sz;
                    out.rule(0.0, y, w, y + t);
                }
                out.w = w;
                out
            }
            Node::GroupChr { ch, top, body } => {
                let b = self.child(me, 0, body, st, depth);
                let (w, asc, desc) = (b.w, b.asc, b.desc);
                let gap = 0.1 * sz;
                let mut out = MathBox::default();
                out.put(b, 0.0, 0.0);
                if let Some(g) = self.group_shape(*ch, w, sz, *top) {
                    // The shape has its near edge at its origin and grows away from the text.
                    if *top {
                        out.put(g, 0.0, -(asc + gap));
                    } else {
                        out.put(g, 0.0, desc + gap);
                    }
                    out.w = out.w.max(w);
                    return out;
                }
                let t = (0.05 * sz).max(0.4);
                let g = self.text(&ch.to_string(), Var::Upright, sz * 0.8);
                let gw = g.w;
                if *top {
                    let y = -(asc + 0.12 * sz);
                    out.rule(0.0, y - t, w, y);
                    out.put(g, ((w - gw) / 2.0).max(0.0), y - t - 0.1 * sz);
                } else {
                    let y = desc + 0.12 * sz;
                    out.rule(0.0, y, w, y + t);
                    let ga = g.asc;
                    out.put(g, ((w - gw) / 2.0).max(0.0), y + t + 0.1 * sz + ga);
                }
                out.w = out.w.max(w);
                out
            }
            Node::Boxed { border, body } => {
                let b = self.child(me, 0, body, st, depth);
                if !border {
                    return b;
                }
                let pad = 0.12 * sz;
                let t = (0.05 * sz).max(0.4);
                let (w, asc, desc) = (b.w, b.asc, b.desc);
                let mut out = MathBox::default();
                out.put(b, pad + t, 0.0);
                let (x1, y0, y1) = (w + 2.0 * (pad + t), -(asc + pad + t), desc + pad + t);
                out.rule(0.0, y0, x1, y0 + t);
                out.rule(0.0, y1 - t, x1, y1);
                out.rule(0.0, y0, t, y1);
                out.rule(x1 - t, y0, x1, y1);
                out.w = x1;
                out
            }
            // Text-like nodes are handled by `node`.
            _ => MathBox::default(),
        }
    }

    fn frac(&self, num: MathBox, den: MathBox, st: St, sz: f64) -> MathBox {
        let t = self.hbar(sz);
        let axis = self.axis(sz);
        let display = st == St::Display;
        // TeX keeps 3 rule widths between the bar and the nearest ink in every style, so a radical
        // or a nested fraction in a numerator / denominator never touches the bar.
        let gap_min = if display { (3.0 * t).max(0.16 * sz) } else { (3.5 * t).max(0.2 * sz) };
        let num_shift = if display { 0.677 } else { 0.394 } * sz;
        let den_shift = if display { 0.686 } else { 0.345 } * sz;
        let up = num_shift.max(axis + t / 2.0 + gap_min + num.desc);
        let down = den_shift.max(den.asc + gap_min + t / 2.0 - axis);
        let pad = 0.06 * sz;
        let inner = num.w.max(den.w);
        let (nw, dw) = (num.w, den.w);
        let mut out = MathBox::default();
        out.put(num, pad + (inner - nw) / 2.0, -up);
        out.put(den, pad + (inner - dw) / 2.0, down);
        out.rule(pad, -axis - t / 2.0, pad + inner, -axis + t / 2.0);
        out.w = inner + 2.0 * pad;
        out
    }

    fn radical(&self, body: MathBox, deg: Option<MathBox>, sz: f64) -> MathBox {
        let t = self.hbar(sz);
        // TeX clearance under the bar, so nested bars stay visibly apart.
        let bar_top = body.asc + 0.32 * sz + t;
        let bottom = body.desc + 0.05 * sz;
        let total = bar_top + bottom;
        let kern_in = 0.1 * sz;
        let pre = deg.as_ref().map(|d| (d.w - 0.45 * sz).max(0.0) + kern_in).unwrap_or(0.0);
        // The sign and its bar are one continuous stroke of the bar's own weight: a short tick,
        // the down stroke, the up stroke and the vinculum, joined without seams.
        let top = -bar_top;
        let left = 0.04 * sz + t;
        // The check mark grows with the root: its down stroke is about half the total height.
        let tall = (0.6 * total).max(0.6 * sz).min(total * 0.8);
        let hook_y = bottom - tall;
        let c0 = (pre + left, hook_y + 0.05 * sz);
        let c1 = (c0.0 + 0.09 * sz, hook_y);
        let c2 = (c1.0 + 0.1 * sz + 0.16 * tall, bottom - t / 2.0);
        let dxu = 0.3 * sz + 0.14 * (total - tall).clamp(0.0, 3.0 * sz);
        let c3 = (c2.0 + dxu, top + t / 2.0);
        let end_body = c3.0 + 0.1 * sz;
        let bw = body.w;
        let end = end_body + bw + 0.03 * sz;
        // Thin tick, heavy down stroke, thin up stroke and bar: the contrast of a typeset radical.
        let heavy = t * 1.7;
        let c2 = (c2.0, c2.1 - heavy * 0.4);
        let mut sign = BezPath::new();
        add_segment(&mut sign, c0, c1, t * 0.8, true);
        add_segment(&mut sign, c1, c2, heavy, false);
        add_segment(&mut sign, c2, c3, t, true);
        add_segment(&mut sign, (c3.0 - t * 0.3, c3.1), (end, c3.1), t, false);
        let mut out = MathBox::default();
        out.path(sign);
        if let Some(d) = deg {
            // The degree sits in the notch above the hook: its baseline just clear of the tick's
            // top, its right edge a little past the tick's end.
            let dx = (c1.0 - d.w + 0.05 * sz).max(0.0);
            let dy = hook_y - 0.12 * sz - d.desc;
            out.put(d, dx, dy);
        }
        out.put(body, end_body, 0.0);
        out.w = end;
        out
    }

    fn scripts(&self, base: MathBox, sub: Option<MathBox>, sup: Option<MathBox>, is_char: bool, st: St) -> MathBox {
        self.scripts_tucked(base, sub, sup, is_char, st, 0.0)
    }

    /// [`scripts`](Self::scripts) with the scripts pulled `tuck` (a fraction of the base's height)
    /// towards the base's middle: a tall slanted integral sign has its ink at the centre.
    fn scripts_tucked(&self, base: MathBox, sub: Option<MathBox>, sup: Option<MathBox>, is_char: bool, st: St, tuck: f64) -> MathBox {
        let sz = self.size * st.factor();
        let ssz = sz * 0.7;
        let xh = self.x_height(sz);
        let t = 0.06 * sz;
        let bw = base.w;
        let (basc, bdesc) = (base.asc, base.desc);
        let mut out = MathBox::default();
        out.put(base, 0.0, 0.0);
        out.w = bw;
        let sup_min = if st == St::Display { 0.413 } else { 0.363 } * sz;
        let mut u = if is_char { 0.0 } else { basc - 0.386 * ssz - tuck * basc };
        let mut v = if is_char { 0.0 } else { bdesc + 0.05 * ssz - tuck * bdesc };
        let x = bw + 0.02 * sz;
        let mut width = bw;
        match (sub, sup) {
            (None, None) => {}
            (None, Some(sp)) => {
                u = u.max(sup_min).max(sp.desc + 0.25 * xh);
                width = x + sp.w;
                out.put(sp, x, -u);
            }
            (Some(sb), None) => {
                v = v.max(0.15 * sz).max(sb.asc - 0.8 * xh);
                width = x + sb.w;
                out.put(sb, x, v);
            }
            (Some(sb), Some(sp)) => {
                u = u.max(sup_min).max(sp.desc + 0.25 * xh);
                v = v.max(0.247 * sz).max(sb.asc - 0.8 * xh);
                let clear = (u - sp.desc) - (sb.asc - v);
                if clear < 4.0 * t {
                    v += 4.0 * t - clear;
                    let psi = 0.8 * xh - (u - sp.desc);
                    if psi > 0.0 {
                        u += psi;
                        v -= psi;
                    }
                }
                width = x + sp.w.max(sb.w);
                out.put(sp, x, -u);
                out.put(sb, x, v);
            }
        }
        out.w = width + 0.02 * sz;
        out
    }

    /// Limits centred over and under a base (display-style big operators, `limLow`/`limUpp`).
    fn limits(&self, base: MathBox, below: Option<MathBox>, above: Option<MathBox>, sz: f64) -> MathBox {
        // Big-operator spacing: the limits sit close to the operator's ink.
        let gap = 0.12 * sz;
        let w = base.w.max(below.as_ref().map(|b| b.w).unwrap_or(0.0)).max(above.as_ref().map(|b| b.w).unwrap_or(0.0));
        let (bw, basc, bdesc) = (base.w, base.asc, base.desc);
        let mut out = MathBox::default();
        out.put(base, (w - bw) / 2.0, 0.0);
        if let Some(a) = above {
            let aw = a.w;
            let dy = -(basc + gap + a.desc);
            out.put(a, (w - aw) / 2.0, dy);
        }
        if let Some(b) = below {
            let bwid = b.w;
            let dy = bdesc + gap + b.asc;
            out.put(b, (w - bwid) / 2.0, dy);
        }
        out.w = w + 0.1 * sz;
        out
    }

    /// A stretchy grouping mark drawn across `w`: a brace or an arrow. Its origin is the edge
    /// nearest the text; it grows downward for an under-mark and upward for an over-mark.
    fn group_shape(&self, ch: char, w: f64, sz: f64, top: bool) -> Option<MathBox> {
        let t = (self.hbar(sz) * 1.1).max(0.045 * sz);
        let w = w.max(0.5 * sz);
        let mut b = MathBox::default();
        match ch {
            '\u{23DE}' | '\u{23DF}' | '\u{23B4}' | '\u{23B5}' => {
                let h = 0.4 * sz;
                let r = (h / 2.0).min(w / 4.0 - t).max(0.02 * sz);
                let (y0, ym) = (t / 2.0, t / 2.0 + r);
                let hw = w / 2.0;
                let mut cl = BezPath::new();
                let k = 0.5523 * r;
                cl.move_to((t / 2.0, y0));
                cl.curve_to((t / 2.0, y0 + k), (t / 2.0 + r - k, ym), (t / 2.0 + r, ym));
                cl.line_to((hw - r, ym));
                cl.curve_to((hw - r + k, ym), (hw, ym + r - k), (hw, ym + r));
                cl.curve_to((hw, ym + r - k), (hw + r - k, ym), (hw + r, ym));
                cl.line_to((w - t / 2.0 - r, ym));
                cl.curve_to((w - t / 2.0 - r + k, ym), (w - t / 2.0, y0 + k), (w - t / 2.0, y0));
                let stroke = kurbo::Stroke::new(t).with_caps(kurbo::Cap::Butt).with_join(kurbo::Join::Round);
                b.path(kurbo::stroke(cl.elements().iter().copied(), &stroke, &kurbo::StrokeOpts::default(), 0.05));
                // Keep the full height even if the stroke bounds are a hair smaller.
                b.asc = 0.0;
                b.desc = 2.0 * r + t;
                b.w = w;
            }
            '\u{2192}' | '\u{2190}' | '\u{2194}' => {
                let head = 0.26 * sz;
                let y = 0.17 * sz + t;
                let (x0, x1) = (0.02 * sz, w - 0.02 * sz);
                let mut p = BezPath::new();
                add_segment(&mut p, (x0, y), (x1, y), t, false);
                if ch != '\u{2190}' {
                    add_segment(&mut p, (x1, y), (x1 - head, y - 0.17 * sz), t, true);
                    add_segment(&mut p, (x1, y), (x1 - head, y + 0.17 * sz), t, true);
                }
                if ch != '\u{2192}' {
                    add_segment(&mut p, (x0, y), (x0 + head, y - 0.17 * sz), t, true);
                    add_segment(&mut p, (x0, y), (x0 + head, y + 0.17 * sz), t, true);
                }
                b.path(p);
                b.asc = 0.0;
                b.desc = 2.0 * y;
                b.w = w;
            }
            _ => return None,
        }
        if top {
            // Mirror about the near edge so the ends point down at the text.
            for it in &mut b.items {
                if let Item::Path(p) = it {
                    p.apply_affine(Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, 0.0]));
                }
            }
            std::mem::swap(&mut b.asc, &mut b.desc);
        }
        Some(b)
    }

    /// Summation and product signs drawn as shapes, heavy and square like a typeset display
    /// operator, centred on the math axis. `None` for other operators.
    /// A right or left arrow drawn with a full-length shaft and a clear head (the text fonts'
    /// own arrows are short and faint at script size).
    fn arrow(&self, c: char, sz: f64) -> Option<MathBox> {
        let dir = match c {
            '\u{2192}' => 1.0,
            '\u{2190}' => -1.0,
            _ => return None,
        };
        let t = (self.hbar(sz) * 0.9).max(0.035 * sz);
        let w = 0.95 * sz;
        let y = -self.axis(sz);
        let (tail, tip) = if dir > 0.0 { (0.04 * sz, w - 0.02 * sz) } else { (w - 0.04 * sz, 0.02 * sz) };
        let head = 0.26 * sz;
        let mut p = BezPath::new();
        add_segment(&mut p, (tail, y), (tip - dir * t, y), t, false);
        add_segment(&mut p, (tip, y), (tip - dir * head, y - 0.17 * sz), t, true);
        add_segment(&mut p, (tip, y), (tip - dir * head, y + 0.17 * sz), t, true);
        let mut b = MathBox::default();
        b.path(p);
        b.w = w;
        Some(b)
    }

    fn big_op(&self, c: char, display: bool, sz: f64) -> Option<MathBox> {
        let h = if display { 1.7 * sz } else { 0.95 * sz };
        let top = -self.axis(sz) - h / 2.0;
        let mut p = BezPath::new();
        let pad = 0.05 * sz;
        let w;
        match c {
            '\u{2211}' => {
                // Thin bars with small serifs, a thin upper diagonal and a heavy lower one.
                w = 0.86 * h;
                let tb = (self.hbar(sz) * 1.2).min(0.055 * h);
                let at = |x: f64, y: f64| (pad + x, top + y);
                let (sw, sh) = (0.045 * h, 0.11 * h);
                add_poly(&mut p, &[at(0.0, 0.0), at(w, 0.0), at(w, sh), at(w - sw, sh), at(w - sw, tb), at(0.0, tb)]);
                add_poly(&mut p, &[at(0.0, h), at(w, h), at(w, h - sh), at(w - sw, h - sh), at(w - sw, h - tb), at(0.0, h - tb)]);
                let m = (0.56 * w, h / 2.0);
                add_segment(&mut p, at(0.05 * w, tb * 0.5), at(m.0, m.1), 0.05 * h, true);
                add_segment(&mut p, at(m.0, m.1), at(0.1 * w, h - tb * 0.5), 0.07 * h, true);
            }
            '\u{220F}' => {
                w = 0.9 * h;
                let (tb, lg) = (0.075 * h, 0.1 * h);
                let at = |x: f64, y: f64| (pad + x, top + y);
                add_poly(&mut p, &[at(0.0, 0.0), at(w, 0.0), at(w, tb), at(0.0, tb)]);
                for x0 in [0.1 * w, 0.9 * w - lg] {
                    add_poly(&mut p, &[at(x0, 0.0), at(x0 + lg, 0.0), at(x0 + lg, h), at(x0, h)]);
                }
                add_poly(
                    &mut p,
                    &[at(0.04 * w, h - 0.07 * h), at(0.1 * w + lg + 0.05 * w, h - 0.07 * h), at(0.1 * w + lg + 0.05 * w, h), at(0.04 * w, h)],
                );
                add_poly(
                    &mut p,
                    &[at(0.9 * w - lg - 0.05 * w, h - 0.07 * h), at(0.96 * w, h - 0.07 * h), at(0.96 * w, h), at(0.9 * w - lg - 0.05 * w, h)],
                );
            }
            _ => return None,
        }
        let mut b = MathBox::default();
        b.path(p);
        b.w = w + 2.0 * pad;
        Some(b)
    }

    /// Parentheses, braces and angle brackets drawn as shapes of the requested height: matched
    /// strokes at every size instead of a scaled text glyph.
    fn path_delim(&self, c: char, target: f64, sz: f64) -> Option<MathBox> {
        let (top, bot) = (-self.axis(sz) - target / 2.0, -self.axis(sz) + target / 2.0);
        let pad = 0.06 * sz;
        let mut b = MathBox::default();
        match c {
            '(' | ')' => {
                let open = c == '(';
                // Proportions follow the font's own parenthesis: its width sets the belly, the
                // stem of `|` the weight; taller shapes only deepen the belly slowly.
                let (gw, gh) = self.ink_em('(').unwrap_or((0.2, 0.9));
                let grow = (target / (gh * sz).max(1e-6) - 1.0).clamp(0.0, 4.0);
                let thick = self.stem(sz) * 2.1;
                let tip = (thick * 0.25).max(0.4);
                let bulge = ((gw * sz - tip).max(0.09 * sz) * (1.25 + 0.15 * grow)).min(0.34 * sz);
                let wid = bulge + tip;
                let x = if open { pad + wid } else { pad };
                b.path(paren_path(open, x, top, bot, bulge, thick, tip));
                b.w = 2.0 * pad + wid;
            }
            '{' | '}' => {
                let open = c == '{';
                let sw = self.stem(sz) * 1.3;
                let wid = 0.3 * sz;
                let (mid, q) = ((top + bot) / 2.0, (target * 0.12).min(0.3 * sz).max(0.12 * sz));
                // Centre line in a box of width `wid`, drawn for `{` and mirrored for `}`.
                let fx = |x: f64| if open { pad + sw / 2.0 + x } else { pad + wid - sw / 2.0 - x };
                let xs = 0.5 * wid; // stem column
                let xt = wid - sw; // tip column (outer end of the arms)
                let (t0, b0) = (top + sw / 2.0, bot - sw / 2.0);
                let mut cl = BezPath::new();
                cl.move_to((fx(xt), t0));
                cl.curve_to((fx(xs), t0), (fx(xs), t0 + q * 0.2), (fx(xs), t0 + q));
                cl.line_to((fx(xs), mid - q));
                cl.curve_to((fx(xs), mid - q * 0.2), (fx(0.0), mid), (fx(0.0), mid));
                cl.curve_to((fx(xs), mid), (fx(xs), mid + q * 0.2), (fx(xs), mid + q));
                cl.line_to((fx(xs), b0 - q));
                cl.curve_to((fx(xs), b0 - q * 0.2), (fx(xs), b0), (fx(xt), b0));
                let st = kurbo::Stroke::new(sw).with_caps(kurbo::Cap::Butt).with_join(kurbo::Join::Round);
                b.path(kurbo::stroke(cl.elements().iter().copied(), &st, &kurbo::StrokeOpts::default(), 0.05));
                b.w = 2.0 * pad + wid;
            }
            '\u{27E8}' | '\u{27E9}' | '\u{2329}' | '\u{232A}' => {
                let open = matches!(c, '\u{27E8}' | '\u{2329}');
                let sw = self.stem(sz).max(0.4) * 1.4;
                let wid = (0.12 * sz + 0.1 * target).min(0.4 * sz);
                let (a, z) = if open { (pad + wid, pad) } else { (pad, pad + wid) };
                let mid = (top + bot) / 2.0;
                let mut p = BezPath::new();
                add_segment(&mut p, (a, top), (z, mid), sw, true);
                add_segment(&mut p, (z, mid), (a, bot), sw, true);
                b.path(p);
                b.w = 2.0 * pad + wid;
            }
            _ => return None,
        }
        Some(b)
    }

    fn delimiters(&self, open: &str, close: &str, sep: &str, items: Vec<MathBox>, sz: f64) -> MathBox {
        let axis = self.axis(sz);
        let (mut asc, mut desc) = (0.0f64, 0.0f64);
        for b in &items {
            asc = asc.max(b.asc);
            desc = desc.max(b.desc);
        }
        let half = (asc - axis).max(desc + axis).max(0.0);
        // Brackets enclose their content: as tall as it, plus a hair of overshoot.
        let target = (2.0 * half + 0.05 * sz).max(sz);
        // Drawn (stretched) delimiters overshoot the content a little, like extensible ones.
        let big = target + 0.16 * sz;
        let make = |s: &str| -> Option<MathBox> {
            let c = s.chars().find(|c| !c.is_whitespace())?;
            // Around ordinary text a bracket is the font's own glyph, so it matches the text.
            if matches!(c, '(' | ')' | '{' | '}')
                && let Some((_, gh)) = self.ink_em(c)
                && target <= gh * sz * 1.3
            {
                return Some(self.stretch(c, target, sz));
            }
            Some(self.rule_delim(c, big, sz).or_else(|| self.path_delim(c, big, sz)).unwrap_or_else(|| self.stretch(c, big, sz)))
        };
        let mut parts = vec![];
        if let Some(o) = make(open) {
            // A brace with nothing closing it (cases) keeps the rows a clear distance away.
            let gap = if open.trim() == "{" && close.trim().is_empty() { 0.3 } else { 0.02 };
            parts.push(o);
            parts.push(MathBox::space(gap * sz));
        }
        let n = items.len();
        for (i, b) in items.into_iter().enumerate() {
            parts.push(b);
            if i + 1 < n {
                parts.push(MathBox::space(0.1 * sz));
                // A bar separator stretches with the brackets; a comma or semicolon is plain text.
                if sep.chars().all(|c| c.is_whitespace() || matches!(c, ',' | ';' | '\u{FF0C}')) {
                    parts.push(self.text(sep.trim(), Var::Upright, sz));
                } else if let Some(s) = make(sep) {
                    parts.push(s);
                }
                parts.push(MathBox::space(0.1 * sz));
            }
        }
        if let Some(c) = make(close) {
            parts.push(MathBox::space(0.02 * sz));
            parts.push(c);
        }
        MathBox::row(parts)
    }

    /// Delimiters drawn as rules (brackets, floors, ceilings, bars): crisp and as tall as asked.
    fn rule_delim(&self, c: char, target: f64, sz: f64) -> Option<MathBox> {
        let t = self.stem(sz) * 1.35;
        let arm = 0.28 * sz;
        let pad = 0.06 * sz;
        let (top, bot) = (-self.axis(sz) - target / 2.0, -self.axis(sz) + target / 2.0);
        let mut b = MathBox::default();
        match c {
            '[' | '\u{230A}' | '\u{2308}' => {
                b.rule(pad, top, pad + t, bot);
                if c != '\u{230A}' {
                    b.rule(pad, top, pad + arm, top + t);
                }
                if c != '\u{2308}' {
                    b.rule(pad, bot - t, pad + arm, bot);
                }
                b.w = pad + arm + pad;
            }
            ']' | '\u{230B}' | '\u{2309}' => {
                b.rule(pad + arm - t, top, pad + arm, bot);
                if c != '\u{230B}' {
                    b.rule(pad, top, pad + arm, top + t);
                }
                if c != '\u{2309}' {
                    b.rule(pad, bot - t, pad + arm, bot);
                }
                b.w = pad + arm + pad;
            }
            '|' | '\u{2223}' => {
                b.rule(pad, top, pad + t, bot);
                b.w = 2.0 * pad + t;
            }
            '\u{2016}' | '\u{2225}' => {
                b.rule(pad, top, pad + t, bot);
                b.rule(pad + 2.0 * t, top, pad + 3.0 * t, bot);
                b.w = 2.0 * pad + 3.0 * t;
            }
            _ => return None,
        }
        Some(b)
    }

    fn matrix(&self, cells: Vec<Vec<MathBox>>, sz: f64) -> MathBox {
        let axis = self.axis(sz);
        let ncols = cells.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut col_w = vec![0.0f64; ncols];
        let mut row_h: Vec<(f64, f64)> = vec![];
        for r in &cells {
            let (mut a, mut d) = (0.7 * sz, 0.3 * sz);
            for (j, c) in r.iter().enumerate() {
                if let Some(w) = col_w.get_mut(j) {
                    *w = w.max(c.w);
                }
                a = a.max(c.asc);
                d = d.max(c.desc);
            }
            row_h.push((a, d));
        }
        let col_gap = 1.0 * sz;
        let row_gap = 0.2 * sz;
        let total: f64 = row_h.iter().map(|(a, d)| a + d).sum::<f64>() + row_gap * row_h.len().saturating_sub(1) as f64;
        let mut out = MathBox::default();
        let mut y_top = -(total / 2.0 + axis);
        for (r, (a, d)) in cells.into_iter().zip(row_h) {
            let base = y_top + a;
            let mut x = 0.0;
            for (j, c) in r.into_iter().enumerate() {
                let cw = col_w.get(j).copied().unwrap_or(0.0);
                let w = c.w;
                out.put(c, x + (cw - w) / 2.0, base);
                x += cw + col_gap;
            }
            y_top += a + d + row_gap;
        }
        out.asc = out.asc.max(total / 2.0 + axis);
        out.desc = out.desc.max(total / 2.0 - axis);
        out.w = col_w.iter().sum::<f64>() + col_gap * ncols.saturating_sub(1) as f64;
        out
    }

    fn accent(&self, ch: char, body: MathBox, sz: f64) -> MathBox {
        let (w, asc, desc) = (body.w, body.asc, body.desc);
        let t = (0.05 * sz).max(0.4);
        let mut out = MathBox::default();
        let top = asc.max(self.x_height(sz)) + 0.05 * sz;
        out.put(body, 0.0, 0.0);
        let spacing = match ch {
            '\u{302}' => Some('\u{2C6}'),
            '\u{303}' => Some('\u{2DC}'),
            '\u{307}' => Some('\u{2D9}'),
            '\u{308}' => Some('\u{A8}'),
            '\u{30C}' => Some('\u{2C7}'),
            '\u{301}' => Some('\u{B4}'),
            '\u{300}' => Some('`'),
            '\u{306}' => Some('\u{2D8}'),
            '\u{20D7}' => Some('\u{2192}'),
            '\u{304}' | '\u{305}' => None,
            c => Some(c),
        };
        match spacing {
            None => out.rule(0.0, -(top + t), w, -top),
            Some(c) => {
                let msz = sz * if c == '\u{2192}' { 0.7 } else { 1.0 };
                let g = self.text(&c.to_string(), Var::Upright, msz);
                let face = self.pick(Var::Upright, c);
                let gid = face.glyph_for(c);
                let k = msz / face.upem.max(1.0);
                // Ink bottom just above the body; the mark centred over it.
                let bottom = self.ink(&face, gid).map(|(_, _, _, y1)| y1 * k).unwrap_or(-0.5 * sz);
                let gw = g.w;
                out.put(g, (w - gw) / 2.0, -top - bottom);
            }
        }
        out.desc = out.desc.max(desc);
        out.w = out.w.max(w);
        out
    }
}

/// Lay out an equation at `size` points in `family` (bold when `bold`).
pub fn layout_math(fonts: &dyn MathFonts, m: &Math, family: &str, size: f64, bold: bool, display: bool) -> MathBox {
    layout_math_wrapped(fonts, m, family, size, bold, display, f64::INFINITY)
}

/// [`layout_math`], breaking the top-level row into lines no wider than `max_w` where it can
/// (after binary operators and relations first, between any two items when there is none).
pub fn layout_math_wrapped(fonts: &dyn MathFonts, m: &Math, family: &str, size: f64, bold: bool, display: bool, max_w: f64) -> MathBox {
    let mut cur = size.clamp(0.5, 4000.0);
    let floor = cur * 0.3;
    let mut out = lay_once(fonts, m, family, cur, bold, display, max_w);
    // Something unbreakable (a wide matrix) that still overflows the line: shrink it to fit,
    // rather than letting the page clip it.
    for _ in 0..3 {
        if !(max_w.is_finite() && max_w > 0.0 && out.w > max_w * 1.001 && cur > floor) {
            break;
        }
        cur = (cur * max_w / out.w * 0.98).max(floor);
        out = lay_once(fonts, m, family, cur, bold, display, max_w);
    }
    out
}

fn lay_once(fonts: &dyn MathFonts, m: &Math, family: &str, size: f64, bold: bool, display: bool, max_w: f64) -> MathBox {
    let mut lay = Lay {
        fonts,
        faces: Faces::new(fonts, family),
        size,
        bold,
        budget: NODE_BUDGET,
        path: vec![],
        cur: 0,
        wrap: max_w,
        brks: vec![],
        nary_body: false,
        thin_at: None,
        bare_base: false,
    };
    let st = if display { St::Display } else { St::Text };
    lay.seq(&m.body, st, 0)
}

/// An equation laid out on its own (the equation editor's canvas), with the geometry that
/// carets, selections and mouse clicks need. Coordinates are points from the top-left corner.
pub struct EqLayout {
    /// Glyphs, rules and shapes, ready for the renderers (its size is the canvas size).
    pub scene: Scene,
    geo: Vec<SeqGeo>,
    baseline: f64,
}

impl EqLayout {
    /// Canvas width, points (padding included).
    pub fn width(&self) -> f64 {
        self.scene.width
    }

    /// Canvas height, points (padding included).
    pub fn height(&self) -> f64 {
        self.scene.height
    }

    /// The equation's baseline, points from the top of the canvas.
    pub fn baseline(&self) -> f64 {
        self.baseline
    }

    fn row(&self, path: &[(usize, usize)]) -> Option<&SeqGeo> {
        self.geo.iter().find(|g| g.path == path)
    }

    /// The thin rectangle of the caret at `c`; `None` when the row was not laid out (too deep).
    pub fn caret_rect(&self, c: &equashioncraft_math::Caret) -> Option<Rect> {
        let g = self.row(&c.path)?;
        let x = g.xs.get(c.pos).or(g.xs.last()).copied()?;
        Some(Rect::new(x, g.top, x, g.bottom))
    }

    /// The box of the whole row at `path` (the active slot's highlight); `None` when the row was
    /// not laid out.
    pub fn row_rect(&self, path: &[(usize, usize)]) -> Option<Rect> {
        let g = self.row(path)?;
        let (x0, x1) = match g.x_range {
            Some(r) => r,
            None => (g.xs.first().copied()?, g.xs.last().copied()?),
        };
        Some(Rect::new(x0.min(x1), g.top, x0.max(x1), g.bottom))
    }

    /// The rectangle covering units `a..b` of the row at `path`.
    pub fn span_rect(&self, path: &[(usize, usize)], a: usize, b: usize) -> Option<Rect> {
        let g = self.row(path)?;
        let x0 = g.xs.get(a).or(g.xs.last()).copied()?;
        let x1 = g.xs.get(b).or(g.xs.last()).copied()?;
        Some(Rect::new(x0.min(x1), g.top, x0.max(x1), g.bottom))
    }

    /// Number of laid-out rows.
    pub fn rows(&self) -> usize {
        self.geo.len()
    }

    /// The caret position nearest the point (x, y).
    pub fn hit(&self, x: f64, y: f64) -> equashioncraft_math::Caret {
        hit_geo(&self.geo, x, y)
    }
}

/// The caret position nearest (x, y) among laid-out rows.
pub fn hit_geo(geo: &[SeqGeo], x: f64, y: f64) -> equashioncraft_math::Caret {
    const NEAR: f64 = 1.5;
    let dist = |g: &SeqGeo| {
        let (x0, x1) = g.x_range.unwrap_or((g.xs.first().copied().unwrap_or(0.0), g.xs.last().copied().unwrap_or(0.0)));
        let dx = (x0 - x).max(x - x1).max(0.0);
        let dy = (g.top - y).max(y - g.bottom).max(0.0);
        dx.hypot(dy)
    };
    let mut best: Option<(&SeqGeo, f64)> = None;
    for g in geo {
        let d = dist(g);
        let better = match best {
            None => true,
            Some((b, bd)) => {
                let (near, bnear) = (d <= NEAR, bd <= NEAR);
                if near && bnear {
                    g.path.len() > b.path.len()
                } else if near != bnear {
                    near
                } else {
                    d < bd
                }
            }
        };
        if better {
            best = Some((g, d));
        }
    }
    let Some((g, _)) = best else { return equashioncraft_math::Caret::default() };
    let pos = g.xs.iter().enumerate().min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).map_or(0, |(i, _)| i);
    equashioncraft_math::Caret { path: g.path.clone(), pos }
}

/// Lay an equation out at `size` points in `family` with `pad` points around it, tall enough for
/// a caret even when it is empty.
pub fn layout_equation(fonts: &dyn MathFonts, m: &Math, family: &str, size: f64, display: bool, color: Rgb, pad: f64) -> EqLayout {
    layout_equation_wrapped(fonts, m, family, size, display, color, pad, f64::INFINITY)
}

/// [`layout_equation`], with the top-level row broken into lines no wider than `max_w` points
/// (and an unbreakable equation shrunk to fit), as [`layout_math_wrapped`] does.
pub fn layout_equation_wrapped(
    fonts: &dyn MathFonts,
    m: &Math,
    family: &str,
    size: f64,
    display: bool,
    color: Rgb,
    pad: f64,
    max_w: f64,
) -> EqLayout {
    let b = layout_math_wrapped(fonts, m, family, size, false, display, max_w);
    let sz = size.clamp(0.5, 4000.0);
    // The canvas holds every caret too: a row's caret reaches a minimum depth below its baseline
    // even when the row has no ink there (the `x` of a denominator), which WordCraft's canvas
    // cut off by up to a point or two.
    let geo_top = b.geo.iter().map(|g| -g.top).filter(|v| v.is_finite()).fold(0.0, f64::max);
    let geo_bottom = b.geo.iter().map(|g| g.bottom).filter(|v| v.is_finite()).fold(0.0, f64::max);
    let (asc, desc) = (b.asc.max(0.78 * sz).max(geo_top), b.desc.max(0.25 * sz).max(geo_bottom));
    let (ox, oy) = (pad, pad + asc);
    let mut items = Vec::new();
    emit(&b, ox, oy, color, &mut items);
    let geo = b
        .geo
        .iter()
        .map(|g| SeqGeo {
            path: g.path.clone(),
            xs: g.xs.iter().map(|x| x + ox).collect(),
            top: g.top + oy,
            bottom: g.bottom + oy,
            x_range: g.x_range.map(|(a, b)| (a + ox, b + ox)),
        })
        .collect();
    EqLayout { scene: Scene { items, width: b.w + 2.0 * pad, height: asc + desc + 2.0 * pad }, geo, baseline: oy }
}

type Key = (String, String, u64, bool, bool, u64);
/// A cached layout: the box, whether the equation is display math, and the justification of
/// display math (`left`, `right`, `center`, ...; from an `m:oMathPara` source).
pub type Cached = (Arc<MathBox>, bool, Option<String>);

/// What to lay out: stored OMML, or the typed linear form.
#[derive(Clone, Copy, Debug)]
pub enum Src<'a> {
    Omml(&'a str),
    Linear(&'a str),
}

/// Laid-out equations by (source, family, size, weight, display, wrap width). WordCraft kept
/// this as a process-wide static; here the host owns one (or none) and passes it in.
#[derive(Default)]
pub struct LayoutCache {
    map: HashMap<Key, Cached>,
}

impl LayoutCache {
    pub fn new() -> LayoutCache {
        LayoutCache::default()
    }

    /// Number of cached layouts.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Forget every cached layout (after the fonts change, say).
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// The laid-out box for an equation, cached by (source, family, size, weight). Returns the
    /// box, whether the equation is display math (`solo` or an `m:oMathPara` source) and the
    /// paragraph justification of display math (`left`, `right`, `center`, ...).
    pub fn layout(&mut self, fonts: &dyn MathFonts, src: Src, family: &str, size: f64, bold: bool, solo: bool, max_w: f64) -> Cached {
        let text = match src {
            Src::Omml(x) => format!("O{x}"),
            Src::Linear(x) => format!("L{x}"),
        };
        let key: Key = (text, family.to_string(), size.to_bits(), bold, solo, max_w.to_bits());
        if let Some(hit) = self.map.get(&key) {
            return hit.clone();
        }
        let val = layout_src(fonts, src, family, size, bold, solo, max_w);
        if self.map.len() >= CACHE_MAX {
            self.map.clear();
        }
        self.map.insert(key, val.clone());
        val
    }
}

/// [`LayoutCache::layout`] without a cache.
pub fn layout_src(fonts: &dyn MathFonts, src: Src, family: &str, size: f64, bold: bool, solo: bool, max_w: f64) -> Cached {
    let m = match src {
        Src::Omml(x) => from_omml(x),
        Src::Linear(x) => equashioncraft_math::from_linear(x),
    };
    let display = m.para || solo;
    (Arc::new(layout_math_wrapped(fonts, &m, family, size, bold, display, max_w)), display, m.jc.clone())
}

/// Emit a box with its origin at (x, baseline) as scene primitives.
pub fn emit(b: &MathBox, x: f64, baseline: f64, color: Rgb, out: &mut Vec<Prim>) {
    for it in &b.items {
        match it {
            Item::Glyphs(g) => out.push(Prim::Glyphs {
                font: g.face.face_id(),
                size: g.size as f32,
                glyphs: g.glyphs.iter().map(|(id, gx, gy)| (*id, (x + gx) as f32, (baseline + gy) as f32)).collect(),
                color,
                synth_bold: g.fake_bold,
                synth_italic: g.fake_italic,
            }),
            Item::Rule(r) => out.push(Prim::Rule(Rect::new(x + r.x0, baseline + r.y0, x + r.x1, baseline + r.y1), color)),
            Item::Path(p) => {
                let mut q = p.clone();
                q.apply_affine(Affine::translate((x, baseline)));
                out.push(Prim::Path(q, color));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use equashioncraft_font::FontDb;
    use equashioncraft_math::from_linear;

    /// The bundled fonts only: no system fonts, so results do not depend on the machine.
    fn fonts() -> &'static FontDb {
        static DB: std::sync::OnceLock<FontDb> = std::sync::OnceLock::new();
        DB.get_or_init(FontDb::bundled_only)
    }

    /// WordCraft's layout tests use Inter; so do these (Inter has only Regular bundled, so its
    /// italic and bold are synthesised).
    const FAMILY: &str = "Inter";

    fn layout_math(m: &Math, _face: &str, size: f64, bold: bool, display: bool) -> MathBox {
        super::layout_math(fonts(), m, FAMILY, size, bold, display)
    }

    fn layout_math_wrapped(m: &Math, _face: &str, size: f64, bold: bool, display: bool, max_w: f64) -> MathBox {
        super::layout_math_wrapped(fonts(), m, FAMILY, size, bold, display, max_w)
    }

    fn face() -> &'static str {
        FAMILY
    }

    fn lay(lin: &str, display: bool) -> MathBox {
        let m = from_linear(lin);
        layout_math(&m, face(), 20.0, false, display)
    }

    #[test]
    fn plain_text_has_width_and_height() {
        let b = lay("xy", false);
        assert!(b.w > 0.0 && b.asc > 0.0);
        assert!(b.glyph_items() >= 1);
    }

    #[test]
    fn a_trailing_differential_takes_a_thin_space() {
        let with = lay("\\int_0^1 xdx", true);
        let without = lay("\\int_0^1 xex", true);
        assert!(with.w > without.w + 0.1 * 20.0, "dx is set off from the integrand");
        // Not an operator body: no space is added.
        assert!((lay("xdx", false).w - lay("xex", false).w).abs() < 1.0);
    }

    #[test]
    fn cfrac_is_a_fraction_and_cases_keep_a_gap_after_the_brace() {
        let f = lay("1+\\cfrac{1}{x}", false);
        assert!(f.rules() >= 1, "cfrac draws a bar");
        let plain = lay("(x^2 & y)", false);
        let cases = lay("\\begin{cases} x^2 & y \\\\ z & w \\end{cases}", false);
        assert!(cases.w > plain.w + 0.5 * 20.0, "the brace and the columns are spaced");
    }

    #[test]
    fn display_operators_are_large_and_the_root_degree_stays_by_the_hook() {
        let d = lay("\\sum_{i=1}^n i", true);
        let t = lay("\\sum_{i=1}^n i", false);
        assert!(d.asc + d.desc > 1.5 * 20.0 * 1.6 / 1.4 * 0.9, "display sum is tall");
        assert!(d.asc + d.desc > t.asc + t.desc);
        let r = lay("\\sqrt[3]{x}", false);
        let r0 = lay("\\sqrt{x}", false);
        assert!(r.asc < r0.asc + 0.9 * 20.0, "the degree does not float far above the bar");
        assert!(r.w > r0.w);
    }

    #[test]
    fn fraction_stacks_and_has_a_rule() {
        let f = lay("a/b", false);
        let flat = lay("ab", false);
        assert!(f.rules() >= 1, "fraction bar");
        assert!(f.asc + f.desc > flat.asc + flat.desc);
        assert!(f.w > 0.0);
    }

    #[test]
    fn scripts_raise_and_lower() {
        let base = lay("x", false);
        let sup = lay("x^2", false);
        let sub = lay("x_2", false);
        assert!(sup.w > base.w && sup.asc > base.asc);
        assert!(sub.w > base.w && sub.desc > base.desc);
        assert!(sup.max_size() >= 20.0 - 1e-9);
    }

    #[test]
    fn root_draws_a_bar_and_grows_with_content() {
        let r = lay("sqrt(x)", false);
        let tall = lay("sqrt(a/b)", false);
        assert!(r.paths() >= 1, "radical sign and bar");
        assert!(tall.asc + tall.desc > r.asc + r.desc);
    }

    #[test]
    fn display_sum_puts_limits_above_and_below() {
        let m = from_linear("sum_(i=1)^n i");
        let disp = layout_math(&m, face(), 20.0, false, true);
        let text = layout_math(&m, face(), 20.0, false, false);
        assert!(disp.asc + disp.desc > text.asc + text.desc, "display limits stack");
        assert!(disp.w > 0.0 && text.w > 0.0);
    }

    #[test]
    fn delimiters_grow_around_tall_content() {
        let small = lay("(x)", false);
        let big = lay("(a/b)", false);
        assert_eq!(big.paths(), 2, "tall content gets drawn parentheses");
        assert!(small.paths() == 2 || small.paths() == 0, "text-height parentheses are glyphs or shapes");
        assert!(big.asc + big.desc > small.asc + small.desc);
    }

    #[test]
    fn matrix_is_taller_than_one_row() {
        let m = lay("[a,b;c,d]", false);
        let one = lay("[a,b]", false);
        assert!(m.asc + m.desc > one.asc + one.desc);
        assert!(m.w > 0.0);
    }

    #[test]
    fn tall_brackets_are_rules_that_enclose_the_content() {
        let small = lay("[x]", false);
        let big = lay("[a/b]", false);
        assert!(small.rules() >= 6, "stem and two arms per bracket");
        assert!(big.rules() > small.rules(), "plus the fraction bar");
        let inner = lay("a/b", false);
        assert!(big.asc + big.desc >= inner.asc + inner.desc);
    }

    #[test]
    fn delimiters_radicals_and_sums_are_drawn_shapes() {
        let paren = lay("(x)", false);
        assert_eq!(lay("(a/b)", false).paths(), 2, "one crescent per parenthesis");
        assert!(paren.paths() == 2 || paren.paths() == 0);
        assert!(lay("sqrt(x)", false).paths() >= 1);
        let sum = lay("sum_(i=1)^n i", true);
        assert!(sum.paths() >= 1);
        let o = "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><m:d><m:dPr><m:begChr m:val=\"{\"/><m:endChr m:val=\"}\"/></m:dPr><m:e><m:f><m:num><m:r><m:t>x</m:t></m:r></m:num><m:den><m:r><m:t>y</m:t></m:r></m:den></m:f></m:e></m:d></m:oMath>";
        let brace = layout_math(&from_omml(o), face(), 20.0, false, false);
        assert!(brace.paths() >= 2);
        // All shapes stay inside finite extents.
        for b in [&paren, &sum, &brace] {
            assert!(b.w.is_finite() && b.asc.is_finite() && b.desc.is_finite());
        }
    }

    #[test]
    fn parenthesis_matches_the_text_it_encloses() {
        let m = layout_math(&from_linear("(12)"), face(), 24.0, false, false);
        let digits = layout_math(&from_linear("12"), face(), 24.0, false, false);
        assert!(m.asc + m.desc < (digits.asc + digits.desc) * 1.8, "no taller than needed");
        assert!(m.asc + m.desc > digits.asc + digits.desc, "still encloses the digits");
    }

    #[test]
    fn script_size_operators_keep_a_space() {
        let m = from_linear("a/(1+b)");
        let b = layout_math(&m, face(), 20.0, false, true);
        let flat = layout_math(&from_linear("1b"), face(), 20.0, false, true);
        assert!(b.w > flat.w);
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let s = format!("{}x{}", "(".repeat(5000), ")".repeat(5000));
        let b = lay(&s, false);
        assert!(b.w.is_finite() && b.asc.is_finite());
        let s = "x^".repeat(3000);
        let b = lay(&s, false);
        assert!(b.w.is_finite());
    }

    #[test]
    fn all_extents_are_finite_for_every_template() {
        for t in equashioncraft_math::templates() {
            let b = layout_math(&t.math, face(), 18.0, false, true);
            assert!(b.w.is_finite() && b.asc.is_finite() && b.desc.is_finite(), "{}", t.id);
            assert!(b.w >= 0.0 && b.asc >= 0.0 && b.desc >= 0.0, "{}", t.id);
        }
    }

    #[test]
    fn layout_cache_returns_the_same_box() {
        let o = equashioncraft_math::to_omml(&from_linear("a^2+b^2"));
        let mut cache = LayoutCache::new();
        let a = cache.layout(fonts(), Src::Omml(&o), FAMILY, 24.0, false, false, f64::INFINITY).0;
        let b = cache.layout(fonts(), Src::Omml(&o), FAMILY, 24.0, false, false, f64::INFINITY).0;
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(cache.len(), 1);
        let c = cache.layout(fonts(), Src::Linear("a^2+b^2"), FAMILY, 24.0, false, false, f64::INFINITY).0;
        assert_eq!(cache.len(), 2, "linear and OMML sources are cached apart");
        assert!((c.width() - a.width()).abs() < 1e-9, "the same equation either way");
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn emit_places_glyphs_relative_to_origin() {
        let b = lay("a/b", false);
        let mut out = vec![];
        emit(&b, 100.0, 50.0, Rgb(0, 0, 0), &mut out);
        assert!(out.iter().any(|d| matches!(d, Prim::Rule(..))));
        for d in &out {
            match d {
                Prim::Glyphs { glyphs, .. } => assert!(glyphs.iter().all(|g| g.1 >= 100.0 - 1e-3)),
                Prim::Rule(rect, _) => assert!(rect.x0 >= 100.0 - 1e-3),
                Prim::Path(..) => {}
            }
        }
    }

    /// Every row path of a tree (the root included).
    fn all_rows(seq: &[Node], path: &mut Vec<(usize, usize)>, out: &mut Vec<Vec<(usize, usize)>>) {
        out.push(path.clone());
        for (i, n) in seq.iter().enumerate() {
            for (c, k) in equashioncraft_math::children(n).into_iter().enumerate() {
                path.push((i, c));
                all_rows(k, path, out);
                path.pop();
            }
        }
    }

    fn eq(m: &Math) -> EqLayout {
        layout_equation(fonts(), m, FAMILY, 28.0, false, Rgb(0, 0, 0), 6.0)
    }

    #[test]
    fn every_caret_position_of_every_template_has_a_rectangle() {
        for t in equashioncraft_math::templates() {
            let mut ed = equashioncraft_math::Editor::new(Math::default());
            ed.type_str("x+");
            ed.insert_template(&t.math);
            ed.type_str("y");
            let shown = ed.display_math();
            let l = eq(&shown);
            let mut rows = vec![];
            all_rows(&shown.body, &mut vec![], &mut rows);
            for path in rows {
                let Some(seq) = equashioncraft_math::seq_at(&shown.body, &path) else { continue };
                let mut last_x = f64::MIN;
                for pos in 0..=equashioncraft_math::units(seq) {
                    let c = equashioncraft_math::Caret { path: path.clone(), pos };
                    let r = l.caret_rect(&c).unwrap_or_else(|| panic!("{}: no rect for {c:?}", t.id));
                    assert!(r.x0.is_finite() && r.y1 > r.y0, "{}: {r:?}", t.id);
                    assert!(
                        r.x0 >= -0.5 && r.x0 <= l.width() + 0.5 && r.y0 >= -0.5 && r.y1 <= l.height() + 0.5,
                        "{}: {c:?} {r:?} in {}x{}",
                        t.id,
                        l.width(),
                        l.height()
                    );
                    assert!(r.x0 >= last_x - 1e-6, "{}: carets run left to right in {path:?}", t.id);
                    last_x = r.x0;
                }
            }
        }
    }

    #[test]
    fn script_carets_sit_higher_and_fraction_rows_stack() {
        let mut ed = equashioncraft_math::Editor::new(Math::default());
        ed.type_str("x^2");
        let l = eq(&ed.display_math());
        let sup = l.caret_rect(ed.caret()).unwrap_or_default();
        let base = l.caret_rect(&equashioncraft_math::Caret { path: vec![], pos: 0 }).unwrap_or_default();
        assert!(sup.y0 <= base.y0 + 1e-6 && sup.y1 < base.y1 && sup.y1 - sup.y0 < base.y1 - base.y0, "{sup:?} {base:?}");
        let mut ed = equashioncraft_math::Editor::new(Math::default());
        ed.type_str("a/b");
        let l = eq(&ed.display_math());
        let den = l.caret_rect(ed.caret()).unwrap_or_default();
        let num = l.caret_rect(&equashioncraft_math::Caret { path: vec![(0, 0)], pos: 0 }).unwrap_or_default();
        assert!(num.y1 <= den.y0 + 1.0, "numerator above denominator: {num:?} {den:?}");
    }

    #[test]
    fn clicking_a_caret_rectangle_returns_that_caret() {
        let mut ed = equashioncraft_math::Editor::new(Math::default());
        ed.type_str("1+x^2/sqrt");
        ed.type_str("y");
        let shown = ed.display_math();
        let l = eq(&shown);
        let mut rows = vec![];
        all_rows(&shown.body, &mut vec![], &mut rows);
        let mut checked = 0;
        for path in rows {
            let Some(seq) = equashioncraft_math::seq_at(&shown.body, &path) else { continue };
            // Not the boundary shared by two neighbours; the middle of a row is unambiguous.
            let n = equashioncraft_math::units(seq);
            for pos in 0..=n {
                let c = equashioncraft_math::Caret { path: path.clone(), pos };
                let Some(r) = l.caret_rect(&c) else { continue };
                let (cx, cy) = (r.x0, (r.y0 + r.y1) / 2.0);
                let got = l.hit(cx, cy);
                // The same place can be reached from two rows (end of one, start of the next);
                // it must at least be at the same x.
                let gr = l.caret_rect(&got).unwrap_or_default();
                assert!((gr.x0 - cx).abs() < 2.0, "{c:?} -> {got:?}");
                checked += 1;
            }
        }
        assert!(checked > 8);
        // Far outside lands on the nearest end of the top row.
        assert_eq!(l.hit(-500.0, 0.0), equashioncraft_math::Caret { path: vec![], pos: 0 });
        assert_eq!(l.hit(5000.0, 0.0).path, Vec::<(usize, usize)>::new());
    }

    #[test]
    fn an_empty_slot_shows_a_box_you_can_click_in() {
        // A square root with nothing under it: the radicand is a dotted box, as wide as a letter,
        // the bar runs over it, and a click in the box lands in the radicand, not beside it.
        let empty = lay("sqrt()", false);
        let filled = lay("sqrt(x)", false);
        assert!(empty.w > 0.4 * 20.0 && empty.w > filled.w * 0.6, "{} vs {}", empty.w, filled.w);
        assert!(empty.rules() >= 8, "the box is dashes: {} rules", empty.rules());
        let l = eq(&from_linear("sqrt()"));
        let row = l.row_rect(&[(0, 0)]).expect("the radicand row was laid out");
        assert!(row.x1 - row.x0 > 5.0, "the slot has a box to click in: {row:?}");
        let hit = l.hit((row.x0 + row.x1) / 2.0, (row.y0 + row.y1) / 2.0);
        assert_eq!(hit.path, vec![(0, 0)]);
        assert_eq!(hit.pos, 0);
        // The top-level row of an empty equation stays blank.
        assert_eq!(lay("", false).rules(), 0);
    }

    #[test]
    fn an_empty_equation_still_has_a_caret_row() {
        let l = eq(&Math::default());
        assert_eq!(l.rows(), 1);
        assert!(l.caret_rect(&equashioncraft_math::Caret::default()).is_some());
        assert!(l.height() > 10.0);
    }

    #[test]
    fn layout_geometry_survives_deep_trees() {
        let mut node = Node::Text("x".into());
        for _ in 0..(MAX_DEPTH * 3) {
            node = Node::Bar { top: true, body: vec![node] };
        }
        let l = eq(&Math::new(vec![node]));
        assert!(l.rows() <= MAX_DEPTH + 2);
        let _ = l.hit(10.0, 10.0);
    }

    #[test]
    fn explicit_spaces_have_their_widths() {
        let plain = lay("ab", false).w;
        let thin = lay(r"a\,b", false).w;
        let quad = lay(r"a\quad b", false).w;
        assert!(thin > plain + 1.0, "thin {thin} vs {plain}");
        assert!(quad > thin + 8.0, "quad {quad} vs {thin}");
    }

    #[test]
    fn a_fraction_takes_a_thin_space_from_what_follows() {
        let f = lay("a/b", false).w;
        let x = lay("x", false).w;
        let both = lay("a/b x", false).w;
        assert!(both > f + x + 1.0, "{both} vs {f}+{x}");
    }

    #[test]
    fn a_differential_after_a_thin_space_is_not_spaced_twice() {
        let one = lay(r"\int f\,dx", false).w;
        let none = lay(r"\int f dx", false).w;
        assert!((one - none).abs() < 1.0, "{one} vs {none}");
    }

    #[test]
    fn arrows_are_drawn_full_length() {
        let a = lay("x->0", false);
        assert!(a.paths() >= 1, "arrow is a shape");
        assert!(a.w > lay("x=0", false).w * 0.8);
    }

    #[test]
    fn a_broken_inline_equation_reports_its_last_line() {
        let m = from_linear(&"a+b+c+d+e+f+".repeat(8));
        let b = layout_math_wrapped(&m, face(), 20.0, false, false, 120.0);
        let lw = b.last_line_width().expect("wrapped");
        assert!(lw > 0.0 && lw <= b.w + 1e-9);
        // The last line sits on the baseline: the lines above raise the box.
        assert!(b.asc > 30.0);
        let one = layout_math_wrapped(&m, face(), 20.0, false, false, f64::INFINITY);
        assert!(one.last_line_width().is_none());
    }

    #[test]
    fn braces_span_the_operand_and_the_label_is_centred() {
        let body = lay("a+b+c+d", false);
        let b = lay(r"\underbrace{a+b+c+d}_{n}", false);
        assert_eq!(b.paths(), 1, "one drawn brace, no separate rule");
        assert_eq!(b.rules(), 0);
        let brace = b.items.iter().find_map(|i| if let Item::Path(p) = i { Some(p.bounding_box()) } else { None }).expect("brace");
        assert!((brace.width() - body.w).abs() < 0.1 * body.w, "brace {} vs operand {}", brace.width(), body.w);
        // The label sits below the brace, centred.
        let label = b.items.iter().rev().find_map(|i| if let Item::Glyphs(g) = i { g.glyphs.first().copied() } else { None }).expect("label");
        assert!(label.2 > brace.y1 - 0.5, "label below the brace");
        assert!((label.1 - body.w / 2.0).abs() < 0.15 * body.w);
        let o = lay(r"\overbrace{a+b}^{m}", false);
        assert!(o.asc > lay("a+b", false).asc + 5.0, "label above the brace");
    }

    #[test]
    fn arrows_over_a_base_span_the_whole_base() {
        let b = lay(r"\overrightarrow{AB}", false);
        let base = lay("AB", false);
        let p = b.items.iter().find_map(|i| if let Item::Path(p) = i { Some(p.bounding_box()) } else { None }).expect("arrow");
        assert!(p.width() > 0.9 * base.w, "arrow {} vs base {}", p.width(), base.w);
    }

    #[test]
    fn a_wide_unbreakable_matrix_is_shrunk_to_the_line() {
        let row = (0..30).map(|j| format!("a_{j}")).collect::<Vec<_>>().join("&");
        let m = from_linear(&format!("matrix({row})"));
        let free = layout_math_wrapped(&m, face(), 20.0, false, true, f64::INFINITY);
        assert!(free.w > 468.0);
        let fit = layout_math_wrapped(&m, face(), 20.0, false, true, 468.0);
        assert!(fit.w <= 468.0 * 1.01, "{}", fit.w);
    }

    #[test]
    fn a_display_integral_is_about_two_em_and_hugs_its_limit() {
        let b = lay(r"\oint_C f", true);
        assert!(b.asc + b.desc < 2.9 * 20.0, "{}", b.asc + b.desc);
    }
}
