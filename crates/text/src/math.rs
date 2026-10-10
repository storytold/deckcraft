//! TeX-style layout of equations (`deckcraft_math::Math`) into positioned glyphs and rules.
//!
//! The result is a [`MathBox`]: a width, an ascent and a descent around a baseline, plus glyph
//! items and rules in box-local coordinates (x right, y down, baseline at y = 0). The paragraph
//! shaper treats the box as one inline atom; [`emit`] turns it into `GlyphRun`s and `Deco`s.
//!
//! Metrics come from glyph outlines and the font's own vertical metrics, so layout works with
//! whatever face resolves (including the last-resort font when craft-fonts is absent). Radicals
//! and delimiters grow by scaling the glyph; fractions, radical bars, bars and borders are rules.
//! Recursion is bounded by `MAX_DEPTH` and the number of laid-out nodes by a budget.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use deckcraft_color::Rgba;
use deckcraft_fonts::{FontDb, FontFace};
use deckcraft_geom::Rect;
use deckcraft_math::{MAX_DEPTH, Math, Node, Style as MStyle, from_omml};
use kurbo::{Affine, BezPath, Shape as _};

use crate::{Deco, GlyphRun};

/// Most nodes laid out for one equation; the rest is dropped.
const NODE_BUDGET: usize = 20_000;
/// Cached equation layouts.
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
pub(crate) struct MathBox {
    pub w: f64,
    pub asc: f64,
    pub desc: f64,
    pub items: Vec<Item>,
    /// Caret geometry of every row laid out inside this box (box-local).
    pub geo: Vec<SeqGeo>,
}

/// Where the caret positions of one row (a child sequence of the math tree) sit.
#[derive(Clone, Debug, Default)]
pub struct SeqGeo {
    /// Route to the row, as in `deckcraft_math::Path`.
    pub path: Vec<(usize, usize)>,
    /// x of each unit boundary: `units + 1` entries.
    pub xs: Vec<f64>,
    /// Vertical extent of the caret in this row (y down, baseline 0).
    pub top: f64,
    pub bottom: f64,
}

impl MathBox {
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
    fn new(family: &str) -> Faces {
        let db = FontDb::global();
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

struct Lay {
    faces: Faces,
    /// Font size of the text style, points.
    size: f64,
    bold: bool,
    budget: usize,
    /// Route to the row being laid out (see `SeqGeo::path`).
    path: Vec<(usize, usize)>,
    /// Index, in its row, of the node being laid out.
    cur: usize,
}

/// Ink box of a glyph in font units, y down: (x0, y0, x1, y1).
fn ink(face: &FontFace, gid: u32) -> Option<(f64, f64, f64, f64)> {
    let p = FontDb::global().outline(face, gid);
    if p.elements().is_empty() {
        return None;
    }
    let b = p.bounding_box();
    if b.x0.is_finite() && b.y0.is_finite() && b.x1.is_finite() && b.y1.is_finite() && b.y1 > b.y0 { Some((b.x0, b.y0, b.x1, b.y1)) } else { None }
}

fn is_upper_greek(c: char) -> bool {
    ('\u{391}'..='\u{3A9}').contains(&c)
}

impl Lay {
    fn pick(&self, v: Var, c: char) -> Arc<FontFace> {
        let base = self.faces.get(v);
        if c.is_whitespace() || c.is_control() || base.covers(c) {
            return base.clone();
        }
        FontDb::global().fallback_for(c, base.id()).unwrap_or_else(|| base.clone())
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
            let cv = self.var_for(c, v);
            let face = self.pick(cv, c);
            let mut j = i + 1;
            while j < chars.len() {
                let d = chars.get(j).copied().unwrap_or(' ');
                let dv = self.var_for(d, v);
                if dv != cv || !Arc::ptr_eq(&self.pick(dv, d), &face) {
                    break;
                }
                j += 1;
            }
            let seg: String = chars.get(i..j).map(|s| s.iter().collect()).unwrap_or_default();
            let k = sz / face.upem.max(1.0);
            let shaped = deckcraft_fonts::shape(&face, &seg, &[], |c| c);
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
                if let Some((_, y0, _, y1)) = ink(&face, g.gid) {
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
        let (x0, y0, x1, y1) = (gid != 0).then(|| ink(face, gid)).flatten()?;
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
        let Some((_, y0, _, y1)) = (gid != 0).then(|| ink(&face, gid)).flatten() else {
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
        for (i, n) in s.iter().enumerate() {
            if self.budget == 0 {
                break;
            }
            self.cur = i;
            self.node(n, st, depth, &mut prev, &mut parts, &mut marks);
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
        let mut row = MathBox::row(parts);
        let sz = self.size * st.factor();
        row.geo.push(SeqGeo { path, xs, top: (-row.asc).min(-0.72 * sz), bottom: row.desc.max(0.22 * sz) });
        row
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
                for c in t.chars() {
                    let before = parts.len();
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
                    parts.push(self.text(&c.to_string(), Var::Italic, sz));
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
                let k = if matches!(other, Node::Nary { .. }) { Class::Op } else { Class::Ord };
                let before = parts.len();
                self.push_gap(parts, *prev, k, st);
                marks.push((before, parts.len(), 0.5));
                let b = self.structure(other, st, depth + 1, me);
                if matches!(other, Node::Delim { .. }) {
                    // Breathing room so `n(` and `)(` do not crowd.
                    parts.push(MathBox::space(0.04 * sz));
                    parts.push(b);
                    parts.push(MathBox::space(0.04 * sz));
                } else {
                    parts.push(b);
                }
                *prev = Class::Ord;
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
                        1.6
                    } else {
                        1.75
                    };
                let ob = self.big_op(*op, display, sz).unwrap_or_else(|| self.stretch(*op, opsz * 0.85, opsz));
                let sb = sub.as_ref().map(|s| self.child(me, 0, s, st.script(), depth));
                let sp = sup.as_ref().map(|s| self.child(me, usize::from(sub.is_some()), s, st.script(), depth));
                let body_box = self.child(me, usize::from(sub.is_some()) + usize::from(sup.is_some()), body, st, depth);
                let head = if integral || !display { self.scripts(ob, sb, sp, false, st) } else { self.limits(ob, sb, sp, sz) };
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
                let t = (0.05 * sz).max(0.4);
                let (w, asc, desc) = (b.w, b.asc, b.desc);
                let mut out = MathBox::default();
                out.put(b, 0.0, 0.0);
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
        let gap_min = if display { (3.5 * t).max(0.2 * sz) } else { (4.0 * t).max(0.24 * sz) };
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
            out.put(d, kern_in * 0.5, -(total * 0.55));
        }
        out.put(body, end_body, 0.0);
        out.w = end;
        out
    }

    fn scripts(&self, base: MathBox, sub: Option<MathBox>, sup: Option<MathBox>, is_char: bool, st: St) -> MathBox {
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
        let mut u = if is_char { 0.0 } else { basc - 0.386 * ssz };
        let mut v = if is_char { 0.0 } else { bdesc + 0.05 * ssz };
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
        out.w = width + 0.04 * sz;
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

    /// Summation and product signs drawn as shapes, heavy and square like a typeset display
    /// operator, centred on the math axis. `None` for other operators.
    fn big_op(&self, c: char, display: bool, sz: f64) -> Option<MathBox> {
        let h = if display { 1.4 * sz } else { 0.95 * sz };
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
        let mut parts = vec![MathBox::space(0.04 * sz)];
        if let Some(o) = make(open) {
            parts.push(o);
            parts.push(MathBox::space(0.05 * sz));
        }
        let n = items.len();
        for (i, b) in items.into_iter().enumerate() {
            parts.push(b);
            if i + 1 < n {
                parts.push(MathBox::space(0.1 * sz));
                if let Some(s) = make(sep) {
                    parts.push(s);
                }
                parts.push(MathBox::space(0.1 * sz));
            }
        }
        if let Some(c) = make(close) {
            parts.push(MathBox::space(0.05 * sz));
            parts.push(c);
        }
        parts.push(MathBox::space(0.03 * sz));
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
        let col_gap = 0.7 * sz;
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
                let bottom = ink(&face, gid).map(|(_, _, _, y1)| y1 * k).unwrap_or(-0.5 * sz);
                let gw = g.w;
                out.put(g, (w - gw) / 2.0, -top - bottom);
            }
        }
        out.desc = out.desc.max(desc);
        out.w = out.w.max(w);
        out
    }
}

/// Lay out an equation at `size` points in the family of `base` (bold when `bold`).
pub(crate) fn layout_math(m: &Math, base: &FontFace, size: f64, bold: bool, display: bool) -> MathBox {
    let mut lay = Lay { faces: Faces::new(&base.family), size: size.clamp(0.5, 4000.0), bold, budget: NODE_BUDGET, path: vec![], cur: 0 };
    let st = if display { St::Display } else { St::Text };
    lay.seq(&m.body, st, 0)
}

/// An equation laid out on its own (the equation editor's canvas), with the geometry that
/// carets, selections and mouse clicks need. Coordinates are points from the top-left corner.
pub struct EqLayout {
    /// Glyphs and rules, ready for `deckcraft_render::render_eq`.
    pub layout: crate::TextLayout,
    pub width: f64,
    pub height: f64,
    geo: Vec<SeqGeo>,
}

impl EqLayout {
    fn row(&self, path: &[(usize, usize)]) -> Option<&SeqGeo> {
        self.geo.iter().find(|g| g.path == path)
    }

    /// The thin rectangle of the caret at `c`; `None` when the row was not laid out (too deep).
    pub fn caret_rect(&self, c: &deckcraft_math::Caret) -> Option<Rect> {
        let g = self.row(&c.path)?;
        let x = g.xs.get(c.pos).or(g.xs.last()).copied()?;
        Some(Rect::new(x, g.top, x, g.bottom))
    }

    /// The box of the whole row at `path` (the active slot's highlight); `None` when the row was
    /// not laid out.
    pub fn row_rect(&self, path: &[(usize, usize)]) -> Option<Rect> {
        let g = self.row(path)?;
        let (x0, x1) = (g.xs.first().copied()?, g.xs.last().copied()?);
        Some(Rect::new(x0.min(x1), g.top, x0.max(x1), g.bottom))
    }

    /// The rectangle covering units `a..b` of the row at `path`.
    pub fn span_rect(&self, path: &[(usize, usize)], a: usize, b: usize) -> Option<Rect> {
        let g = self.row(path)?;
        let x0 = g.xs.get(a).or(g.xs.last()).copied()?;
        let x1 = g.xs.get(b).or(g.xs.last()).copied()?;
        Some(Rect::new(x0.min(x1), g.top, x0.max(x1), g.bottom))
    }

    /// Number of laid-out rows (tests).
    pub fn rows(&self) -> usize {
        self.geo.len()
    }

    /// The caret position nearest the point (x, y).
    pub fn hit(&self, x: f64, y: f64) -> deckcraft_math::Caret {
        const NEAR: f64 = 1.5;
        let dist = |g: &SeqGeo| {
            let (x0, x1) = (g.xs.first().copied().unwrap_or(0.0), g.xs.last().copied().unwrap_or(0.0));
            let dx = (x0 - x).max(x - x1).max(0.0);
            let dy = (g.top - y).max(y - g.bottom).max(0.0);
            dx.hypot(dy)
        };
        let mut best: Option<(&SeqGeo, f64)> = None;
        for g in &self.geo {
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
        let Some((g, _)) = best else { return deckcraft_math::Caret::default() };
        let pos = g.xs.iter().enumerate().min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).map_or(0, |(i, _)| i);
        deckcraft_math::Caret { path: g.path.clone(), pos }
    }
}

/// Lay an equation out at `size` points in `family` with `pad` points around it, tall enough for
/// a caret even when it is empty.
pub fn layout_equation(m: &Math, family: &str, size: f64, display: bool, color: Rgba, pad: f64) -> EqLayout {
    let base = FontDb::global().face(family, "Regular");
    let b = layout_math(m, &base, size, false, display);
    let sz = size.clamp(0.5, 4000.0);
    let (asc, desc) = (b.asc.max(0.78 * sz), b.desc.max(0.25 * sz));
    let (ox, oy) = (pad, pad + asc);
    let mut layout = crate::TextLayout { inner: Rect::new(0.0, 0.0, b.w + 2.0 * pad, asc + desc + 2.0 * pad), ..Default::default() };
    emit(&b, ox, oy, color, 1.0, 0, (0, 0), &mut layout.runs, &mut layout.decos);
    let geo = b
        .geo
        .iter()
        .map(|g| SeqGeo { path: g.path.clone(), xs: g.xs.iter().map(|x| x + ox).collect(), top: g.top + oy, bottom: g.bottom + oy })
        .collect();
    EqLayout { width: b.w + 2.0 * pad, height: asc + desc + 2.0 * pad, layout, geo }
}

type Key = (String, u32, u64, bool, bool);
static CACHE: Mutex<Option<HashMap<Key, Arc<MathBox>>>> = Mutex::new(None);

/// The laid-out box for an OMML equation, cached by (source, face, size, weight).
pub(crate) fn layout_math_cached(omml: &str, base: &Arc<FontFace>, size: f64, bold: bool, solo: bool) -> Arc<MathBox> {
    let key: Key = (omml.to_string(), base.id(), size.to_bits(), bold, solo);
    if let Ok(g) = CACHE.lock()
        && let Some(hit) = g.as_ref().and_then(|c| c.get(&key))
    {
        return hit.clone();
    }
    let m = from_omml(omml);
    let b = Arc::new(layout_math(&m, base, size, bold, m.para || solo));
    if let Ok(mut g) = CACHE.lock() {
        let c = g.get_or_insert_with(HashMap::new);
        if c.len() >= CACHE_MAX {
            c.clear();
        }
        c.insert(key, b.clone());
    }
    b
}

/// Number of cached equation layouts (tests).
#[cfg(test)]
pub(crate) fn cache_len() -> usize {
    CACHE.lock().ok().and_then(|g| g.as_ref().map(|c| c.len())).unwrap_or(0)
}

/// Emit a box with its origin at (x, baseline) as glyph runs and rule decorations.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit(
    b: &MathBox,
    x: f64,
    baseline: f64,
    color: Rgba,
    alpha: f64,
    para: usize,
    chars: (usize, usize),
    runs: &mut Vec<GlyphRun>,
    decos: &mut Vec<Deco>,
) {
    for it in &b.items {
        match it {
            Item::Glyphs(g) => runs.push(GlyphRun {
                face: g.face.clone(),
                size: g.size,
                color,
                glyphs: g.glyphs.iter().map(|(id, gx, gy)| (*id, x + gx, baseline + gy)).collect(),
                // One character to the morph transition: every glyph is the equation's first.
                cells: vec![(chars.0, '\u{FFFC}'); g.glyphs.len()],
                fake_bold: g.fake_bold,
                fake_italic: g.fake_italic,
                outline: None,
                para,
                chars,
                link: false,
                alpha,
                gradient: None,
            }),
            Item::Rule(r) => {
                decos.push(Deco { rect: Rect::new(x + r.x0, baseline + r.y0, x + r.x1, baseline + r.y1), color, behind: false, para, path: None })
            }
            Item::Path(p) => {
                let mut q = p.clone();
                q.apply_affine(Affine::translate((x, baseline)));
                let bb = q.bounding_box();
                decos.push(Deco { rect: Rect::new(bb.x0, bb.y0, bb.x1, bb.y1), color, behind: false, para, path: Some(Arc::new(q)) });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_math::from_linear;

    fn face() -> Arc<FontFace> {
        FontDb::global().face("Inter", "Regular")
    }

    fn lay(lin: &str, display: bool) -> MathBox {
        let m = from_linear(lin);
        layout_math(&m, &face(), 20.0, false, display)
    }

    #[test]
    fn plain_text_has_width_and_height() {
        let b = lay("xy", false);
        assert!(b.w > 0.0 && b.asc > 0.0);
        assert!(b.glyph_items() >= 1);
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
        let disp = layout_math(&m, &face(), 20.0, false, true);
        let text = layout_math(&m, &face(), 20.0, false, false);
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
        let brace = layout_math(&from_omml(o), &face(), 20.0, false, false);
        assert!(brace.paths() >= 2);
        // All shapes stay inside finite extents.
        for b in [&paren, &sum, &brace] {
            assert!(b.w.is_finite() && b.asc.is_finite() && b.desc.is_finite());
        }
    }

    #[test]
    fn parenthesis_matches_the_text_it_encloses() {
        let m = layout_math(&from_linear("(12)"), &face(), 24.0, false, false);
        let digits = layout_math(&from_linear("12"), &face(), 24.0, false, false);
        assert!(m.asc + m.desc < (digits.asc + digits.desc) * 1.8, "no taller than needed");
        assert!(m.asc + m.desc > digits.asc + digits.desc, "still encloses the digits");
    }

    #[test]
    fn script_size_operators_keep_a_space() {
        let m = from_linear("a/(1+b)");
        let b = layout_math(&m, &face(), 20.0, false, true);
        let flat = layout_math(&from_linear("1b"), &face(), 20.0, false, true);
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
        for t in deckcraft_math::templates() {
            let b = layout_math(&t.math, &face(), 18.0, false, true);
            assert!(b.w.is_finite() && b.asc.is_finite() && b.desc.is_finite(), "{}", t.id);
            assert!(b.w >= 0.0 && b.asc >= 0.0 && b.desc >= 0.0, "{}", t.id);
        }
    }

    #[test]
    fn layout_cache_returns_the_same_box() {
        let o = deckcraft_math::to_omml(&from_linear("a^2+b^2"));
        let f = face();
        let a = layout_math_cached(&o, &f, 24.0, false, false);
        let b = layout_math_cached(&o, &f, 24.0, false, false);
        assert!(Arc::ptr_eq(&a, &b));
        assert!(cache_len() >= 1);
    }

    #[test]
    fn emit_places_glyphs_relative_to_origin() {
        let b = lay("a/b", false);
        let (mut runs, mut decos) = (vec![], vec![]);
        emit(&b, 100.0, 50.0, Rgba::BLACK, 1.0, 0, (0, 3), &mut runs, &mut decos);
        assert!(runs.iter().all(|r| r.glyphs.iter().all(|g| g.1 >= 100.0 - 1e-9)));
        assert!(!decos.is_empty());
        assert!(decos.iter().all(|d| d.rect.x0 >= 100.0 - 1e-9));
    }

    /// Every row path of a tree (the root included).
    fn all_rows(seq: &[Node], path: &mut Vec<(usize, usize)>, out: &mut Vec<Vec<(usize, usize)>>) {
        out.push(path.clone());
        for (i, n) in seq.iter().enumerate() {
            for (c, k) in deckcraft_math::children(n).into_iter().enumerate() {
                path.push((i, c));
                all_rows(k, path, out);
                path.pop();
            }
        }
    }

    fn eq(m: &Math) -> EqLayout {
        layout_equation(m, "Inter", 28.0, false, Rgba::rgb(0, 0, 0), 6.0)
    }

    #[test]
    fn every_caret_position_of_every_template_has_a_rectangle() {
        for t in deckcraft_math::templates() {
            let mut ed = deckcraft_math::Editor::new(Math::default());
            ed.type_str("x+");
            ed.insert_template(&t.math);
            ed.type_str("y");
            let shown = ed.display_math();
            let l = eq(&shown);
            let mut rows = vec![];
            all_rows(&shown.body, &mut vec![], &mut rows);
            for path in rows {
                let Some(seq) = deckcraft_math::seq_at(&shown.body, &path) else { continue };
                let mut last_x = f64::MIN;
                for pos in 0..=deckcraft_math::units(seq) {
                    let c = deckcraft_math::Caret { path: path.clone(), pos };
                    let r = l.caret_rect(&c).unwrap_or_else(|| panic!("{}: no rect for {c:?}", t.id));
                    assert!(r.x0.is_finite() && r.y1 > r.y0, "{}: {r:?}", t.id);
                    assert!(
                        r.x0 >= -0.5 && r.x0 <= l.width + 0.5 && r.y0 >= -0.5 && r.y1 <= l.height + 0.5,
                        "{}: {c:?} {r:?} in {}x{}",
                        t.id,
                        l.width,
                        l.height
                    );
                    assert!(r.x0 >= last_x - 1e-6, "{}: carets run left to right in {path:?}", t.id);
                    last_x = r.x0;
                }
            }
        }
    }

    #[test]
    fn script_carets_sit_higher_and_fraction_rows_stack() {
        let mut ed = deckcraft_math::Editor::new(Math::default());
        ed.type_str("x^2");
        let l = eq(&ed.display_math());
        let sup = l.caret_rect(ed.caret()).unwrap_or_default();
        let base = l.caret_rect(&deckcraft_math::Caret { path: vec![], pos: 0 }).unwrap_or_default();
        assert!(sup.y0 < base.y0 && sup.y1 - sup.y0 < base.y1 - base.y0, "{sup:?} {base:?}");
        let mut ed = deckcraft_math::Editor::new(Math::default());
        ed.type_str("a/b");
        let l = eq(&ed.display_math());
        let den = l.caret_rect(ed.caret()).unwrap_or_default();
        let num = l.caret_rect(&deckcraft_math::Caret { path: vec![(0, 0)], pos: 0 }).unwrap_or_default();
        assert!(num.y1 <= den.y0 + 1.0, "numerator above denominator: {num:?} {den:?}");
    }

    #[test]
    fn clicking_a_caret_rectangle_returns_that_caret() {
        let mut ed = deckcraft_math::Editor::new(Math::default());
        ed.type_str("1+x^2/sqrt");
        ed.type_str("y");
        let shown = ed.display_math();
        let l = eq(&shown);
        let mut rows = vec![];
        all_rows(&shown.body, &mut vec![], &mut rows);
        let mut checked = 0;
        for path in rows {
            let Some(seq) = deckcraft_math::seq_at(&shown.body, &path) else { continue };
            // Not the boundary shared by two neighbours; the middle of a row is unambiguous.
            let n = deckcraft_math::units(seq);
            for pos in 0..=n {
                let c = deckcraft_math::Caret { path: path.clone(), pos };
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
        assert_eq!(l.hit(-500.0, 0.0), deckcraft_math::Caret { path: vec![], pos: 0 });
        assert_eq!(l.hit(5000.0, 0.0).path, Vec::<(usize, usize)>::new());
    }

    #[test]
    fn an_empty_equation_still_has_a_caret_row() {
        let l = eq(&Math::default());
        assert_eq!(l.rows(), 1);
        assert!(l.caret_rect(&deckcraft_math::Caret::default()).is_some());
        assert!(l.height > 10.0);
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
}
