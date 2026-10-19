//! Equations laid out in two dimensions from their OMML tree: fractions, scripts, radicals, big
//! operators, delimiters, matrices and the rest, set TeX-style in the run's own font (faces that
//! lack a symbol fall back as text does). The result is a box of glyphs and rules that the
//! paragraph places like one character.

use std::sync::Arc;

use deckcraft_fonts::{FontDb, FontFace};
use deckcraft_geom::KurboShape;
use equashion_math::{Math, Node, Seq, Style};

/// A glyph at its pen position from the box's origin (the left end of its baseline, y down).
pub(crate) struct Glyph {
    pub face: Arc<FontFace>,
    pub size: f64,
    pub gid: u32,
    pub ch: char,
    pub x: f64,
    pub y: f64,
    pub fake_bold: bool,
    pub fake_italic: bool,
}

/// A laid-out equation: its width, its extent above and below the baseline, its glyphs and its
/// rules (fraction bars, overbars: `x0, y0, x1, y1`).
#[derive(Default)]
pub(crate) struct MathBox {
    pub w: f64,
    pub asc: f64,
    pub desc: f64,
    pub glyphs: Vec<Glyph>,
    pub rules: Vec<(f64, f64, f64, f64)>,
}

impl MathBox {
    /// Add `b` with its origin at (`x`, `y`) of this box, growing the extent to cover it.
    fn put(&mut self, b: MathBox, x: f64, y: f64) {
        self.w = self.w.max(x + b.w);
        self.asc = self.asc.max(b.asc - y);
        self.desc = self.desc.max(b.desc + y);
        self.glyphs.extend(b.glyphs.into_iter().map(|g| Glyph { x: g.x + x, y: g.y + y, ..g }));
        self.rules.extend(b.rules.into_iter().map(|(x0, y0, x1, y1)| (x0 + x, y0 + y, x1 + x, y1 + y)));
    }
    fn rule(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.rules.push((x0, y0, x1, y1));
        self.asc = self.asc.max(-y0);
        self.desc = self.desc.max(y1);
    }
}

/// Lay out equation `m` at `size` points in `family`; a display equation (`m:oMathPara`) with
/// bigger operators and full-size fractions.
pub(crate) fn layout(m: &Math, family: &str, bold: bool, size: f64) -> MathBox {
    let l = Lay { family, bold, base: size };
    l.seq(&m.body, St { size, display: m.para, depth: 0 })
}

struct Lay<'a> {
    family: &'a str,
    bold: bool,
    base: f64,
}

/// The style a part is set in: its size, display or text style, and nesting depth.
#[derive(Clone, Copy)]
struct St {
    size: f64,
    display: bool,
    depth: usize,
}

/// Deeper parts are not drawn (the parsers already cap nesting well below this).
const MAX_DEPTH: usize = 64;

fn is_binary(c: char) -> bool {
    matches!(c, '+' | '−' | '±' | '∓' | '×' | '÷' | '·' | '⋅' | '∗' | '∘' | '∪' | '∩' | '∧' | '∨' | '⊕' | '⊗')
}

fn is_relation(c: char) -> bool {
    matches!(
        c,
        '=' | '<'
            | '>'
            | '≤'
            | '≥'
            | '≠'
            | '≈'
            | '≡'
            | '∼'
            | '≃'
            | '≅'
            | '∝'
            | '≪'
            | '≫'
            | '→'
            | '←'
            | '↔'
            | '⇒'
            | '⇐'
            | '⇔'
            | '↦'
            | '∈'
            | '∉'
            | '∋'
            | '⊂'
            | '⊃'
            | '⊆'
            | '⊇'
    )
}

/// Letters set in math italic: Latin and lowercase Greek (TeX keeps capital Greek upright).
fn is_math_italic(c: char) -> bool {
    c.is_ascii_alphabetic() || ('α'..='ω').contains(&c) || matches!(c, 'ϑ' | 'ϕ' | 'ϖ' | 'ϵ' | 'ϱ')
}

/// A combining accent as the spacing mark drawn over a base.
fn spacing_accent(c: char) -> char {
    match c {
        '\u{0300}' => 'ˋ',
        '\u{0301}' => 'ˊ',
        '\u{0302}' => 'ˆ',
        '\u{0303}' => '˜',
        '\u{0306}' => '˘',
        '\u{0307}' => '˙',
        '\u{0308}' => '¨',
        '\u{030C}' => 'ˇ',
        '\u{20D6}' => '←',
        '\u{20D7}' | '\u{20D1}' => '→',
        c => c,
    }
}

impl Lay<'_> {
    fn smaller(&self, st: St) -> St {
        St { size: (st.size * 0.7).max(self.base * 0.5), display: false, depth: st.depth + 1 }
    }
    fn inner(&self, st: St) -> St {
        St { depth: st.depth + 1, ..st }
    }
    fn axis(&self, size: f64) -> f64 {
        let f = FontDb::global().document_face(self.family, "Regular");
        f.x_height / f.upem.max(1.0) * size * 0.5
    }
    fn rule_width(&self, size: f64) -> f64 {
        (size * 0.045).max(0.4)
    }

    /// The face that draws `c` in this family and style, and whether it must be emboldened or
    /// slanted.
    fn face(&self, c: char, bold: bool, italic: bool) -> (Arc<FontFace>, bool, bool) {
        let db = FontDb::global();
        let style = crate::style_name(bold, italic);
        let face = db.document_face(self.family, style);
        let face = if face.covers(c) { face } else { db.cascade(&[c], std::slice::from_ref(&face), style) };
        let (fake_bold, fake_italic) = (bold && face.weight < 550.0, italic && !face.italic);
        (face, fake_bold, fake_italic)
    }

    /// One glyph and its ink: (box with the glyph at the origin, ink top, ink bottom, ink left,
    /// ink right), y down.
    fn glyph(&self, c: char, size: f64, bold: bool, italic: bool) -> (MathBox, f64, f64, f64, f64) {
        let (face, fake_bold, fake_italic) = self.face(c, bold, italic);
        let gid = face.glyph_for(c);
        let k = size / face.upem.max(1.0);
        let ink = FontDb::global().outline(&face, gid).bounding_box();
        let (top, bottom) = if ink.is_finite() && ink.height() > 0.0 { (ink.y0 * k, ink.y1 * k) } else { (0.0, 0.0) };
        let (left, right) = if ink.is_finite() && ink.width() > 0.0 { (ink.x0 * k, ink.x1 * k) } else { (0.0, 0.0) };
        let b = MathBox {
            w: face.advance(gid) * k,
            asc: -top,
            desc: bottom,
            glyphs: vec![Glyph { face, size, gid, ch: c, x: 0.0, y: 0.0, fake_bold, fake_italic }],
            rules: vec![],
        };
        (b, top, bottom, left, right)
    }

    /// A glyph scaled up until its ink spans `height`, centred on the math axis; and where its ink
    /// ends on the right.
    fn tall(&self, c: char, st: St, height: f64) -> (MathBox, f64) {
        let (_, top, bottom, ..) = self.glyph(c, st.size, self.bold, false);
        let size = if bottom - top > 0.0 { st.size * (height / (bottom - top)).max(1.0) } else { st.size };
        let (g, top, bottom, _, right) = self.glyph(c, size.min(st.size * 12.0), self.bold, false);
        let mut out = MathBox::default();
        out.put(g, 0.0, -self.axis(st.size) - (top + bottom) / 2.0);
        (out, right)
    }

    fn seq(&self, s: &Seq, st: St) -> MathBox {
        let mut out = MathBox::default();
        if st.depth > MAX_DEPTH {
            return out;
        }
        // Whether what came last is an operand, so a following `+` or `-` is binary.
        let mut operand = false;
        for n in s {
            let x = out.w;
            match n.bare() {
                Node::Text(t) => self.text(&mut out, t, None, st, &mut operand),
                Node::Styled { text, style } => self.text(&mut out, text, Some(*style), st, &mut operand),
                _ => {
                    out.put(self.node(n, st), x, 0.0);
                    operand = true;
                }
            }
        }
        out
    }

    fn text(&self, out: &mut MathBox, t: &str, style: Option<Style>, st: St, operand: &mut bool) {
        // Operator spacing (medium around binary operators, thick around relations), not in scripts.
        let spaced = st.size >= self.base * 0.99;
        let (medium, thick) = if spaced { (st.size * 4.0 / 18.0, st.size * 5.0 / 18.0) } else { (0.0, 0.0) };
        let bold = self.bold || matches!(style, Some(Style::Bold | Style::BoldItalic));
        for c in t.chars() {
            let c = if c == '-' { '−' } else { c };
            let italic = match style {
                None => is_math_italic(c),
                Some(s) => s == Style::BoldItalic,
            };
            let space = if is_binary(c) && *operand {
                medium
            } else if is_relation(c) {
                thick
            } else {
                0.0
            };
            let (g, ..) = self.glyph(c, st.size, bold, italic);
            let x = out.w + space;
            let w = g.w;
            out.put(g, x, 0.0);
            out.w = x + w + if c == ',' || c == ';' { st.size * 3.0 / 18.0 } else { space };
            *operand = !(is_binary(c) || is_relation(c) || matches!(c, '(' | '[' | '{' | ',' | ';'));
        }
    }

    fn node(&self, n: &Node, st: St) -> MathBox {
        let s = st.size;
        let mut out = MathBox::default();
        match n {
            Node::Text(t) => self.text(&mut out, t, None, st, &mut true),
            Node::Styled { text, style } => self.text(&mut out, text, Some(*style), st, &mut true),
            Node::Frac { num, den } => {
                let part = if st.display { St { display: false, ..self.inner(st) } } else { self.smaller(st) };
                let (n, d) = (self.seq(num, part), self.seq(den, part));
                let (a, t, gap) = (self.axis(s), self.rule_width(s), s * if st.display { 0.15 } else { 0.1 });
                let pad = s * 0.1;
                let w = n.w.max(d.w) + 2.0 * pad;
                let (nw, nd, dw, da) = (n.w, n.desc, d.w, d.asc);
                out.put(n, (w - nw) / 2.0, -(a + t / 2.0 + gap + nd));
                out.put(d, (w - dw) / 2.0, -a + t / 2.0 + gap + da);
                out.rule(pad / 2.0, -a - t / 2.0, w - pad / 2.0, -a + t / 2.0);
                out.w = w;
            }
            Node::Script { base, sub, sup } => {
                let b = self.seq(base, self.inner(st));
                self.scripts(&mut out, b, sub.as_ref(), sup.as_ref(), st, false);
            }
            Node::PreScript { sub, sup, base } => {
                let b = self.seq(base, self.inner(st));
                self.scripts(&mut out, b, Some(sub), Some(sup), st, true);
            }
            Node::Rad { deg, body } => {
                let b = self.seq(body, self.inner(st));
                let (t, gap) = (self.rule_width(s), s * if st.display { 0.18 } else { 0.12 });
                let top = -(b.asc + gap + t);
                // The sign spans the body, the gap and the bar, its top meeting the bar.
                let (sign, ink) = self.tall('√', st, b.asc + b.desc + gap + t);
                let lift = top + sign.asc;
                let (sw, sh) = (ink.max(sign.w * 0.5), sign.asc + sign.desc);
                let d = deg.as_ref().map(|d| self.seq(d, self.smaller(self.smaller(st))));
                let dx = d.as_ref().map_or(0.0, |d| (d.w - sw * 0.5).max(0.0));
                if let Some(d) = d {
                    let dd = d.desc;
                    out.put(d, 0.0, top + sign.asc + sign.desc - sh * 0.55 - dd);
                }
                out.put(sign, dx, lift);
                let x = dx + sw;
                let bw = b.w;
                out.put(b, x + s * 0.05, 0.0);
                out.rule(x - t * 0.5, top, x + bw + s * 0.12, top + t);
                out.w = x + bw + s * 0.15;
            }
            Node::Nary { op, sub, sup, body } => self.nary(&mut out, *op, sub.as_ref(), sup.as_ref(), body, st, None),
            Node::Delim { open, close, sep, items } => {
                let mut c = MathBox::default();
                let parts: Vec<MathBox> = items.iter().map(|i| self.seq(i, self.inner(st))).collect();
                let a = self.axis(s);
                let height = parts.iter().map(|p| 2.0 * (p.asc - a).max(p.desc + a)).fold(s * 0.9, f64::max);
                let fence = |c: &mut MathBox, ch: Option<char>| {
                    if let Some(ch) = ch {
                        let (g, _) = self.tall(ch, st, height);
                        let x = c.w + s * 0.04;
                        let w = g.w;
                        c.put(g, x, 0.0);
                        c.w = x + w + s * 0.04;
                    }
                };
                fence(&mut c, open.chars().next());
                for (i, p) in parts.into_iter().enumerate() {
                    if i > 0 {
                        fence(&mut c, sep.chars().next());
                    }
                    let x = c.w;
                    c.put(p, x, 0.0);
                }
                fence(&mut c, close.chars().next());
                out.put(c, 0.0, 0.0);
            }
            Node::Matrix { rows } => {
                let cells: Vec<Vec<MathBox>> =
                    rows.iter().map(|r| r.iter().map(|c| self.seq(c, St { display: false, ..self.inner(st) })).collect()).collect();
                self.grid(&mut out, cells, st, s * 0.8);
            }
            Node::EqArr { rows } => {
                let cells = rows.iter().map(|r| vec![self.seq(r, self.inner(st))]).collect();
                self.grid(&mut out, cells, st, 0.0);
            }
            Node::Func { name, body } => {
                self.text(&mut out, name, Some(Style::Plain), st, &mut false);
                let x = out.w + s / 6.0;
                out.put(self.seq(body, self.inner(st)), x, 0.0);
            }
            Node::Accent { ch, body } => {
                let b = self.seq(body, self.inner(st));
                let (bw, ba) = (b.w, b.asc);
                out.put(b, 0.0, 0.0);
                let gap = s * 0.08;
                if matches!(ch, '\u{0304}' | '\u{0305}' | '\u{00AF}') {
                    let t = self.rule_width(s);
                    out.rule(0.0, -(ba + gap + t), bw, -(ba + gap));
                } else {
                    let size = if matches!(ch, '\u{20D6}' | '\u{20D7}' | '\u{20D1}') { s * 0.7 } else { s };
                    let (g, _, bottom, left, right) = self.glyph(spacing_accent(*ch), size, self.bold, false);
                    out.put(g, (bw - (right - left)) / 2.0 - left, -(ba + gap) - bottom);
                }
            }
            Node::Limit { lower, base, lim } => {
                let b = self.seq(base, self.inner(st));
                let l = self.seq(lim, self.smaller(st));
                let w = b.w.max(l.w);
                let gap = s * 0.1;
                let (bw, ba, bd, lw, la, ld) = (b.w, b.asc, b.desc, l.w, l.asc, l.desc);
                out.put(b, (w - bw) / 2.0, 0.0);
                out.put(l, (w - lw) / 2.0, if *lower { bd + gap + la } else { -(ba + gap + ld) });
            }
            Node::Bar { top, body } => {
                let b = self.seq(body, self.inner(st));
                let (bw, ba, bd, t, gap) = (b.w, b.asc, b.desc, self.rule_width(s), s * 0.08);
                out.put(b, 0.0, 0.0);
                if *top {
                    out.rule(0.0, -(ba + gap + t), bw, -(ba + gap));
                } else {
                    out.rule(0.0, bd + gap, bw, bd + gap + t);
                }
            }
            Node::GroupChr { ch, top, body } => {
                let b = self.seq(body, self.inner(st));
                let (g, gtop, gbottom, left, right) = self.glyph(*ch, s, self.bold, false);
                let (bw, ba, bd, gap) = (b.w, b.asc, b.desc, s * 0.08);
                let w = bw.max(right - left);
                out.put(b, (w - bw) / 2.0, 0.0);
                let y = if *top { -(ba + gap) - gbottom } else { bd + gap - gtop };
                out.put(g, (w - (right - left)) / 2.0 - left, y);
            }
            Node::Boxed { border, body } => {
                let b = self.seq(body, self.inner(st));
                if *border {
                    let (t, pad) = (self.rule_width(s), s * 0.1);
                    let (w, top, bottom) = (b.w + 2.0 * pad, -(b.asc + pad), b.desc + pad);
                    out.put(b, pad, 0.0);
                    out.rule(0.0, top - t, w, top);
                    out.rule(0.0, bottom, w, bottom + t);
                    out.rule(0.0, top - t, t, bottom + t);
                    out.rule(w - t, top - t, w, bottom + t);
                } else {
                    out.put(b, 0.0, 0.0);
                }
            }
            Node::Props { extra, node } => {
                // The tree keeps a big operator's explicit limit placement among its properties.
                if let Node::Nary { op, sub, sup, body } = node.bare() {
                    let lim = extra.pr.iter().find(|(k, _)| k == "limLoc").map(|(_, v)| v.contains("undOvr"));
                    self.nary(&mut out, *op, sub.as_ref(), sup.as_ref(), body, st, lim);
                } else {
                    out = self.node(node, st);
                }
            }
            Node::Raw(_) => {}
        }
        out
    }

    /// `b` with sub- and superscripts after it (or before it, `pre`).
    fn scripts(&self, out: &mut MathBox, b: MathBox, sub: Option<&Seq>, sup: Option<&Seq>, st: St, pre: bool) {
        let s = st.size;
        let small = self.smaller(st);
        let p = sup.map(|q| self.seq(q, small));
        let q = sub.map(|q| self.seq(q, small));
        let mut up = p.as_ref().map_or(0.0, |p| (b.asc - small.size * 0.3).max(s * 0.4).max(p.desc + s * 0.1));
        let mut down = q.as_ref().map_or(0.0, |q| (b.desc + small.size * 0.1).max(s * 0.18).max(q.asc - s * 0.35));
        if let (Some(p), Some(q)) = (&p, &q) {
            let gap = (up - p.desc) - (q.asc - down);
            let min = self.rule_width(s) * 4.0;
            if gap < min {
                down += (min - gap) / 2.0;
                up += (min - gap) / 2.0;
            }
        }
        let sw = p.as_ref().map_or(0.0, |p| p.w).max(q.as_ref().map_or(0.0, |q| q.w));
        let (bx, sx) = if pre { (sw + s * 0.04, 0.0) } else { (0.0, b.w + s * 0.04) };
        let bw = b.w;
        out.put(b, bx, 0.0);
        if let Some(p) = p {
            out.put(p, sx, -up);
        }
        if let Some(q) = q {
            out.put(q, sx, down);
        }
        out.w = bw + sw + s * 0.08;
    }

    /// A big operator (sum, product, integral…) with its limits, then its body. Limits go under
    /// and over in display style (never for integrals) unless `under` says otherwise.
    #[allow(clippy::too_many_arguments)]
    fn nary(&self, out: &mut MathBox, op: char, sub: Option<&Seq>, sup: Option<&Seq>, body: &Seq, st: St, under: Option<bool>) {
        let s = st.size;
        // A missing operator character is an integral (OMML's default).
        let op = if op == '\0' { '∫' } else { op };
        let integral = ('∫'..='∳').contains(&op);
        let (g, top, bottom, ..) = self.glyph(op, s * if st.display { 1.6 } else { 1.2 }, self.bold, false);
        let mut o = MathBox::default();
        o.put(g, 0.0, -self.axis(s) - (top + bottom) / 2.0);
        if under.unwrap_or(st.display && !integral) {
            let small = self.smaller(st);
            let (p, q) = (sup.map(|x| self.seq(x, small)), sub.map(|x| self.seq(x, small)));
            let w = o.w.max(p.as_ref().map_or(0.0, |p| p.w)).max(q.as_ref().map_or(0.0, |q| q.w));
            let (ow, oa, od, gap) = (o.w, o.asc, o.desc, s * 0.1);
            out.put(o, (w - ow) / 2.0, 0.0);
            if let Some(p) = p {
                let (pw, pd) = (p.w, p.desc);
                out.put(p, (w - pw) / 2.0, -(oa + gap + pd));
            }
            if let Some(q) = q {
                let (qw, qa) = (q.w, q.asc);
                out.put(q, (w - qw) / 2.0, od + gap + qa);
            }
            out.w = w;
        } else {
            self.scripts(out, o, sub, sup, st, false);
        }
        let x = out.w + s / 6.0;
        out.put(self.seq(body, self.inner(st)), x, 0.0);
    }

    /// Rows of cells, columns `gap` apart, centred in their columns and on the math axis.
    fn grid(&self, out: &mut MathBox, rows: Vec<Vec<MathBox>>, st: St, gap: f64) {
        let s = st.size;
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        let widths: Vec<f64> = (0..cols).map(|j| rows.iter().filter_map(|r| r.get(j)).map(|c| c.w).fold(0.0, f64::max)).collect();
        let lines: Vec<(f64, f64)> =
            rows.iter().map(|r| (r.iter().map(|c| c.asc).fold(s * 0.7, f64::max), r.iter().map(|c| c.desc).fold(s * 0.2, f64::max))).collect();
        let lead = s * 0.25;
        let height: f64 = lines.iter().map(|(a, d)| a + d).sum::<f64>() + lead * lines.len().saturating_sub(1) as f64;
        let mut y = -self.axis(s) - height / 2.0;
        for (r, (a, d)) in rows.into_iter().zip(lines) {
            y += a;
            let mut x = 0.0;
            for (c, w) in r.into_iter().zip(&widths) {
                let cw = c.w;
                out.put(c, x + (w - cw) / 2.0, y);
                x += w + gap;
            }
            y += d + lead;
        }
        out.w = widths.iter().sum::<f64>() + gap * cols.saturating_sub(1) as f64;
    }
}

#[cfg(test)]
mod tests {
    use equashion_math::from_linear;

    use super::*;

    fn lay(linear: &str, display: bool) -> MathBox {
        layout(&Math { para: display, ..from_linear(linear) }, "Liberation Serif", false, 20.0)
    }

    #[test]
    fn plain_text_is_one_line_of_glyphs() {
        let b = lay("x+y", false);
        assert_eq!(b.glyphs.len(), 3);
        assert!(b.w > 0.0 && b.asc > 0.0 && b.rules.is_empty());
        assert!(b.glyphs.iter().all(|g| g.y == 0.0));
        // `+` between operands is spaced; a leading minus is not.
        assert!(lay("x+y", false).w > lay("xy", false).w + lay("+", false).w + 4.0);
        assert!(lay("-x", false).w < lay("x-x", false).w);
    }

    #[test]
    fn a_fraction_stacks_over_a_bar() {
        let f = lay("a/b", true);
        assert_eq!(f.rules.len(), 1, "the fraction bar");
        let (a, b) = (&f.glyphs[0], &f.glyphs[1]);
        assert!(a.y < 0.0 && b.y > 0.0, "numerator above the baseline, denominator below");
        assert!(f.asc > lay("a", true).asc * 1.5);
        // Inline, the parts are smaller.
        assert!(lay("a/b", false).glyphs[0].size < a.size);
    }

    #[test]
    fn scripts_are_smaller_and_shifted() {
        let b = lay("x^2_i", false);
        let (base, two, i) = (&b.glyphs[0], &b.glyphs[1], &b.glyphs[2]);
        assert!(two.size < base.size && i.size < base.size);
        assert!(two.y < 0.0, "superscript raised");
        assert!(i.y > 0.0, "subscript lowered");
    }

    #[test]
    fn radicals_operators_and_fences_grow_with_their_content() {
        let r = lay("sqrt(a/b)", true);
        assert_eq!(r.rules.len(), 2, "fraction bar and the radical's bar");
        assert!(r.glyphs.iter().any(|g| g.ch == '√' && g.size > 20.0));
        let s = lay("\\sum_(i=1)^n i", true);
        let sum = s.glyphs.iter().find(|g| g.ch == '∑').map(|g| g.size).unwrap_or(0.0);
        assert!(sum > 20.0);
        let paren = |l: &str| lay(l, true).glyphs.iter().find(|g| g.ch == '(').map(|g| g.size).unwrap_or(0.0);
        assert!(paren("(a/b)") > paren("(a)"));
    }

    #[test]
    fn matrices_accents_and_unknown_parts_lay_out_without_panicking() {
        for l in ["matrix(a&b@c&d)", "vec(v)", "hat(x)", "bar(x)", "lim_(x->0) f(x)", "\\nosuch", "", "cases(1&x>0@0&x<0)", "■(a@b)"] {
            let b = lay(l, true);
            assert!(b.w.is_finite() && b.asc.is_finite() && b.desc.is_finite(), "{l}");
        }
        let deep = "(".repeat(200) + &")".repeat(200);
        assert!(lay(&deep, false).w.is_finite());
    }
}
