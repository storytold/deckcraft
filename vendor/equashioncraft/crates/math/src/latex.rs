//! LaTeX output: print an equation as the LaTeX a MathLive-style editor would produce.
//!
//! [`to_linear`](crate::to_linear) prints the typed linear form (`√(x)`, `·`); this prints LaTeX
//! (`\sqrt{x}`, `\cdot`) for hosts that store and exchange it. The parser side is
//! [`from_linear`](crate::from_linear), which reads the same LaTeX subset, so
//! `from_linear(&to_latex(m))` gives back an equal tree for everything this printer emits.
//!
//! Choices follow MathLive's output where they are visible to a consumer: one-character scripts
//! are written without braces (`x^2`), delimiters are `\left..\right`, matrices use the
//! `pmatrix`-style environments, upright words are `\operatorname{..}`.

use crate::funcs::LATEX;
use crate::linear::{ACCENTS, GREEK, NARY, SYMS, raw_text};
use crate::{MAX_DEPTH, Math, Node, Seq, Style};

/// Print an equation as LaTeX.
pub fn to_latex(m: &Math) -> String {
    let mut t = Tex::default();
    t.seq(&m.body, 0);
    t.out.trim().to_string()
}

/// Output buffer that knows when a command name was just written, so a following letter gets a
/// separating space (`\pi x`, never `\pix`).
#[derive(Default)]
struct Tex {
    out: String,
    after_command: bool,
}

impl Tex {
    fn raw(&mut self, s: &str) {
        if let Some(c) = s.chars().next() {
            self.space_for(c);
        }
        self.out.push_str(s);
        self.after_command = false;
    }

    fn space_for(&mut self, c: char) {
        if self.after_command && c.is_ascii_alphabetic() {
            self.out.push(' ');
        }
    }

    /// A command such as `\pi`; the next letter will be separated from it.
    fn cmd(&mut self, name: &str) {
        self.raw("\\");
        self.out.push_str(name);
        self.after_command = name.chars().last().is_some_and(|c| c.is_ascii_alphabetic());
    }

    fn ch(&mut self, c: char) {
        self.space_for(c);
        self.out.push(c);
        self.after_command = false;
    }

    fn text(&mut self, s: &str) {
        for c in s.chars() {
            self.text_char(c);
        }
    }

    fn text_char(&mut self, c: char) {
        if let Some((name, _)) = GREEK.iter().find(|(_, g)| *g == c) {
            return self.cmd(name);
        }
        if let Some((name, _)) = SYMS.iter().find(|(_, v)| v.chars().count() == 1 && v.starts_with(c)) {
            return match *name {
                // MathLive prints these three as bare characters / scripts.
                "prime" => self.raw("'"),
                "leq" => self.cmd("le"),
                "geq" => self.cmd("ge"),
                "neq" => self.cmd("ne"),
                "dprime" => self.raw("''"),
                n => self.cmd(n),
            };
        }
        if let Some(name) = extra_command(c) {
            return self.cmd(name);
        }
        if let Some(l) = double_struck_letter(c) {
            self.cmd("mathbb");
            self.raw("{");
            self.ch(l);
            self.raw("}");
            return;
        }
        match c {
            '\u{2212}' => self.ch('-'),
            '\u{2009}' => self.raw("\\,"),
            '\u{2005}' => self.raw("\\:"),
            '\u{2004}' => self.raw("\\;"),
            '\u{2001}' => self.cmd("quad"),
            '\u{00A0}' => self.raw("~"),
            '{' | '}' | '#' | '%' | '&' | '$' | '_' => {
                self.raw("\\");
                self.out.push(c);
            }
            '\\' => self.cmd("backslash"),
            '^' => self.raw("\\^{}"),
            '~' => self.raw("\\~{}"),
            c => self.ch(c),
        }
    }

    /// `{...}` around a printed argument; an empty argument is an empty group.
    fn group(&mut self, s: &Seq, d: usize) {
        self.raw("{");
        self.seq(s, d);
        self.raw("}");
    }

    /// A script argument: bare for one ASCII letter or digit (`x^2`), braced otherwise.
    fn script(&mut self, mark: char, s: &Seq, d: usize) {
        self.raw(&mark.to_string());
        match s.as_slice() {
            [Node::Text(t)] if t.chars().count() == 1 && t.chars().all(|c| c.is_ascii_alphanumeric()) => self.text(t),
            _ => self.group(s, d),
        }
    }

    /// The base of a script: bare when it is one character, a delimiter pair, a word or a
    /// function name, grouped otherwise so the script attaches to all of it.
    fn script_base(&mut self, base: &Seq, d: usize) {
        let bare = match base.as_slice() {
            [] => false,
            [n] => match n.bare() {
                Node::Text(t) => t.chars().count() == 1,
                Node::Delim { .. } | Node::Styled { .. } => true,
                Node::Func { body, .. } => body.is_empty(),
                _ => false,
            },
            _ => false,
        };
        if bare {
            self.seq(base, d);
        } else {
            self.group(base, d);
        }
    }

    fn delimiter(&mut self, s: &str, left: bool) {
        let name = match s {
            "" => ".",
            "(" | ")" | "[" | "]" | "/" => s,
            "{" => "\\{",
            "}" => "\\}",
            "|" => "|",
            "\u{2016}" => "\\Vert",
            "\u{27E8}" => "\\langle",
            "\u{27E9}" => "\\rangle",
            "\u{2329}" => "\\langle",
            "\u{232A}" => "\\rangle",
            "\u{230A}" => "\\lfloor",
            "\u{230B}" => "\\rfloor",
            "\u{2308}" => "\\lceil",
            "\u{2309}" => "\\rceil",
            other => other,
        };
        self.cmd(if left { "left" } else { "right" });
        self.after_command = false;
        self.out.push_str(name);
        self.after_command = name.starts_with('\\') && name.chars().last().is_some_and(|c| c.is_ascii_alphabetic());
    }

    fn seq(&mut self, s: &Seq, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let d = depth + 1;
        for n in s {
            self.node(n.bare(), d);
        }
    }

    fn node(&mut self, n: &Node, d: usize) {
        match n {
            Node::Props { .. } => {}
            Node::Text(t) => self.text(t),
            Node::Styled { text, style } => self.styled(text, *style),
            Node::Frac { num, den } => {
                self.cmd("frac");
                self.group(num, d);
                self.group(den, d);
            }
            Node::Rad { deg, body } => {
                self.cmd("sqrt");
                if let Some(dg) = deg {
                    self.raw("[");
                    self.seq(dg, d);
                    self.raw("]");
                }
                self.group(body, d);
            }
            Node::Script { base, sub, sup } => {
                self.script_base(base, d);
                if let Some(s) = sub {
                    self.script('_', s, d);
                }
                if let Some(s) = sup {
                    self.script('^', s, d);
                }
            }
            Node::Nary { op, sub, sup, body } => {
                if let Some((name, _)) = NARY.iter().find(|(_, c)| c == op) {
                    self.cmd(name);
                } else if *op != '\0' {
                    self.ch(*op);
                }
                if let Some(s) = sub {
                    self.script('_', s, d);
                }
                if let Some(s) = sup {
                    self.script('^', s, d);
                }
                // One node (a letter, `i^2`, a parenthesised sum) follows bare; a longer run is
                // braced so the operator still covers all of it when the LaTeX is read back.
                let one = match body.as_slice() {
                    [] => true,
                    [Node::Text(t)] => t.chars().count() == 1,
                    [_] => true,
                    _ => false,
                };
                if one {
                    self.seq(body, d);
                } else {
                    self.group(body, d);
                }
            }
            Node::Delim { open, close, sep, items } => self.delim(open, close, sep, items, d),
            Node::Matrix { rows } => self.matrix("matrix", rows, d),
            Node::EqArr { rows } => {
                self.raw("\\begin{aligned}");
                self.rows(rows, d);
                self.raw("\\end{aligned}");
            }
            Node::Func { name, body } => {
                self.function(name);
                if let [only @ Node::Delim { open, close, .. }] = body.as_slice()
                    && open == "("
                    && close == ")"
                {
                    // The argument list already has its own parentheses.
                    self.node(only, d);
                } else if !body.is_empty() {
                    self.delimiter("(", true);
                    self.seq(body, d);
                    self.delimiter(")", false);
                }
            }
            Node::Accent { ch, body } => match ACCENTS.iter().find(|(_, c)| c == ch) {
                Some((name, _)) => {
                    self.cmd(name);
                    self.group(body, d);
                }
                None => {
                    self.seq(body, d);
                    self.ch(*ch);
                }
            },
            Node::Bar { top, body } => {
                self.cmd(if *top { "overline" } else { "underline" });
                self.group(body, d);
            }
            Node::GroupChr { ch, top, body } => {
                let side = if *top { "over" } else { "under" };
                let name = match *ch {
                    '\u{2192}' => Some(format!("{side}rightarrow")),
                    '\u{2190}' => Some(format!("{side}leftarrow")),
                    '\u{2194}' => Some(format!("{side}leftrightarrow")),
                    '\u{23DE}' if *top => Some("overbrace".to_string()),
                    '\u{23DF}' if !*top => Some("underbrace".to_string()),
                    _ => None,
                };
                match name {
                    Some(n) => {
                        self.cmd(&n);
                        self.group(body, d);
                    }
                    None => self.seq(body, d),
                }
            }
            Node::Boxed { border, body } => {
                if *border {
                    self.cmd("boxed");
                    self.group(body, d);
                } else {
                    self.seq(body, d);
                }
            }
            Node::Limit { lower, base, lim } => {
                let word = match base.as_slice() {
                    [Node::Styled { text, .. }] if *lower && matches!(text.as_str(), "lim" | "max" | "min") => Some(text.as_str()),
                    _ => None,
                };
                match word {
                    Some(w) => {
                        self.cmd(w);
                        self.script('_', lim, d);
                    }
                    None => {
                        self.cmd(if *lower { "underset" } else { "overset" });
                        self.group(lim, d);
                        self.group(base, d);
                    }
                }
            }
            Node::PreScript { sub, sup, base } => {
                self.raw("{}");
                self.script('_', sub, d);
                self.script('^', sup, d);
                self.seq(base, d);
            }
            Node::Raw(src) if src == crate::OMATH_BREAK => self.raw(" "),
            Node::Raw(src) if src.starts_with("<m:argPr") => {}
            Node::Raw(src) => {
                let t = raw_text(src);
                if t.is_empty() {
                    self.raw("{}");
                } else {
                    self.text(&t);
                }
            }
        }
    }

    /// A function name: `\sin` for the standard commands, `\operatorname{..}` otherwise.
    fn function(&mut self, name: &str) {
        if LATEX.contains(&name) {
            self.cmd(name);
        } else {
            self.cmd("operatorname");
            self.raw("{");
            self.text(name);
            self.raw("}");
        }
    }

    fn styled(&mut self, text: &str, style: Style) {
        match style {
            Style::Normal => {
                self.cmd("text");
                self.raw("{");
                self.text(text);
                self.raw("}");
            }
            Style::Bold => {
                self.cmd("mathbf");
                self.raw("{");
                self.text(text);
                self.raw("}");
            }
            Style::BoldItalic => {
                self.cmd("boldsymbol");
                self.raw("{");
                self.text(text);
                self.raw("}");
            }
            Style::Plain => {
                let word = text.chars().count() > 1 && text.chars().all(|c| c.is_ascii_alphabetic());
                if word {
                    self.function(text);
                } else {
                    self.cmd("mathrm");
                    self.raw("{");
                    self.text(text);
                    self.raw("}");
                }
            }
        }
    }

    fn rows(&mut self, rows: &[Seq], d: usize) {
        for (i, r) in rows.iter().enumerate() {
            if i > 0 {
                self.raw(" \\\\ ");
            }
            self.seq(r, d);
        }
    }

    fn matrix(&mut self, env: &str, rows: &[Vec<Seq>], d: usize) {
        self.raw(&format!("\\begin{{{env}}}"));
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                self.raw(" \\\\ ");
            }
            for (k, cell) in row.iter().enumerate() {
                if k > 0 {
                    self.raw(" & ");
                }
                self.seq(cell, d);
            }
        }
        self.raw(&format!("\\end{{{env}}}"));
    }

    fn delim(&mut self, open: &str, close: &str, sep: &str, items: &[Seq], d: usize) {
        // A lone matrix or `cases` array is an environment, not a delimited group.
        if let [only] = items {
            match only.as_slice() {
                [Node::Matrix { rows }] => {
                    if open == "(" && close == ")" && rows.len() == 2 && rows.iter().all(|r| r.len() == 1) {
                        self.cmd("binom");
                        for r in rows {
                            if let Some(c) = r.first() {
                                self.group(c, d);
                            }
                        }
                        return;
                    }
                    let env = match (open, close) {
                        ("(", ")") => Some("pmatrix"),
                        ("[", "]") => Some("bmatrix"),
                        ("{", "}") => Some("Bmatrix"),
                        ("|", "|") => Some("vmatrix"),
                        ("\u{2016}", "\u{2016}") => Some("Vmatrix"),
                        _ => None,
                    };
                    if let Some(env) = env {
                        return self.matrix(env, rows, d);
                    }
                }
                [Node::EqArr { rows }] if open == "{" && close.is_empty() => {
                    self.raw("\\begin{cases}");
                    for (i, r) in rows.iter().enumerate() {
                        if i > 0 {
                            self.raw(" \\\\ ");
                        }
                        let mut sub = Tex::default();
                        sub.seq(r, d);
                        self.raw(&sub.out.replace('\u{2003}', " & "));
                    }
                    self.raw("\\end{cases}");
                    return;
                }
                _ => {}
            }
        }
        self.delimiter(open, true);
        for (i, it) in items.iter().enumerate() {
            if i > 0 {
                // `|` stretches with the brackets; any other separator (a comma) is a plain character.
                if sep == "|" || sep.is_empty() {
                    self.cmd("middle");
                    self.raw("|");
                } else {
                    self.raw(sep);
                }
            }
            self.seq(it, d);
        }
        self.delimiter(close, false);
    }
}

/// Symbols that have a LaTeX command but are not in the typed-command table.
fn extra_command(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{00D7}' => "times",
        '\u{221E}' => "infty",
        '\u{2218}' => "circ",
        '\u{2216}' => "setminus",
        '\u{00AC}' => "neg",
        '\u{2227}' => "wedge",
        '\u{2228}' => "vee",
        '\u{2295}' => "oplus",
        '\u{2297}' => "otimes",
        '\u{21A6}' => "mapsto",
        '\u{2194}' => "leftrightarrow",
        '\u{21D0}' => "Leftarrow",
        '\u{2234}' => "therefore",
        '\u{2235}' => "because",
        '\u{210F}' => "hbar",
        '\u{2113}' => "ell",
        '\u{2135}' => "aleph",
        '\u{221A}' => "surd",
        '\u{2260}' => "neq",
        '\u{22C5}' => "cdot",
        '\u{2217}' => "ast",
        '\u{2286}' => "subseteq",
        '\u{2287}' => "supseteq",
        '\u{2283}' => "supset",
        '\u{2192}' => "to",
        _ => return None,
    })
}

fn double_struck_letter(c: char) -> Option<char> {
    Some(match c {
        '\u{2102}' => 'C',
        '\u{210D}' => 'H',
        '\u{2115}' => 'N',
        '\u{2119}' => 'P',
        '\u{211A}' => 'Q',
        '\u{211D}' => 'R',
        '\u{2124}' => 'Z',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::from_linear;

    fn t(s: &str) -> String {
        to_latex(&from_linear(s))
    }

    #[test]
    fn plain_text_and_scripts() {
        assert_eq!(t("x^2+3x-2"), "x^2+3x-2");
        assert_eq!(t("x^{2}"), "x^2");
        assert_eq!(t("x^{n+1}"), "x^{n+1}");
        assert_eq!(t("x_{12}"), "x_{12}");
        assert_eq!(t("e^{-x^2}"), "e^{-x^2}");
    }

    #[test]
    fn fractions_and_roots() {
        assert_eq!(t(r"\frac{1}{x}"), r"\frac{1}{x}");
        assert_eq!(t(r"\frac{\frac{1}{2}}{3}"), r"\frac{\frac{1}{2}}{3}");
        assert_eq!(t(r"\sqrt{x+1}"), r"\sqrt{x+1}");
        assert_eq!(t(r"\sqrt[3]{x}"), r"\sqrt[3]{x}");
    }

    #[test]
    fn functions_keep_their_parentheses() {
        assert_eq!(t(r"\sin\left(2x\right)"), r"\sin\left(2x\right)");
        assert_eq!(t(r"\sin(x)"), r"\sin\left(x\right)");
        assert_eq!(t(r"\sin x"), r"\sin\left(x\right)");
        assert_eq!(t(r"\ln(x)+\cos(x)"), r"\ln\left(x\right)+\cos\left(x\right)");
    }

    #[test]
    fn desmos_function_names_print_as_operator_names() {
        for name in crate::funcs::OPNAMES {
            let want = format!("\\operatorname{{{name}}}\\left(x\\right)");
            assert_eq!(t(&format!("{name}(x)")), want, "{name}");
        }
        assert_eq!(t("sech x"), r"\operatorname{sech}\left(x\right)");
        assert_eq!(t("nCr(6,2)"), r"\operatorname{nCr}\left(6,2\right)");
        assert_eq!(t("csch^2 x"), r"\operatorname{csch}^2 x");
        // The LaTeX commands are unchanged.
        assert_eq!(t("sinh(x)"), r"\sinh\left(x\right)");
    }

    #[test]
    fn desmos_spellings() {
        assert_eq!(t("infinity"), r"\infty");
        assert_eq!(t("u cdot v"), r"u \cdot v");
        assert_eq!(t("u cross v"), r"u \times v");
    }

    #[test]
    fn commands_are_separated_from_following_letters() {
        assert_eq!(t(r"2\pi x"), r"2\pi x");
        assert_eq!(t(r"\theta+\alpha"), r"\theta+\alpha");
        assert_eq!(t(r"a\cdot b"), r"a\cdot b");
        assert_eq!(t(r"a\leq b\geq c\neq d"), r"a\le b\ge c\ne d");
    }

    #[test]
    fn delimiters_and_absolute_value() {
        assert_eq!(t(r"\left(x+1\right)^{2}"), r"\left(x+1\right)^2");
        assert_eq!(t("|x-1|"), r"\left|x-1\right|");
        assert_eq!(t(r"\left|x\right|"), r"\left|x\right|");
        assert_eq!(t(r"\{x\}"), r"\left\{x\right\}");
        assert_eq!(t(r"\abs{x}"), r"\left|x\right|");
        assert_eq!(t(r"\sin{x}"), r"\sin\left(x\right)");
    }

    #[test]
    fn upright_constants_and_words() {
        assert_eq!(t(r"\mathrm{e}^{x}"), r"\mathrm{e}^x");
        assert_eq!(t(r"\exponentialE^{x}"), r"\mathrm{e}^x");
        assert_eq!(t(r"\differentialD x"), "d x");
        assert_eq!(t(r"\operatorname{abs}(x)"), r"\operatorname{abs}\left(x\right)");
        assert_eq!(t(r"\text{if } x"), r"\text{if } x");
    }

    #[test]
    fn big_operators_and_environments() {
        assert_eq!(t(r"\sum_{i=1}^{n}i"), r"\sum_{i=1}^ni");
        assert_eq!(t(r"\sum_{i=1}^{n}(i+1)"), r"\sum_{i=1}^n\left(i+1\right)");
        assert_eq!(t(r"\int_0^1 x\,dx"), r"\int_0^1{x\,dx}");
        assert_eq!(t(r"\begin{pmatrix}a&b\\c&d\end{pmatrix}"), r"\begin{pmatrix}a & b \\ c & d\end{pmatrix}");
        assert_eq!(t(r"\binom{n}{k}"), r"\binom{n}{k}");
    }

    #[test]
    fn printed_latex_parses_back_to_the_same_tree() {
        for src in [
            r"\frac{a+b}{c}",
            r"x^{2}+\sqrt[3]{y}",
            r"\sin\left(2x\right)+\cos\left(x\right)",
            r"\left|x-1\right|",
            r"\sum_{i=1}^{n}i^2",
            r"\begin{bmatrix}1&2\\3&4\end{bmatrix}",
            r"a\le b\ne c",
            r"\overline{x}+\hat{y}",
            r"\mathbb{R}\to\infty",
        ] {
            let m = from_linear(src);
            let l = to_latex(&m);
            assert_eq!(from_linear(&l).body, m.body, "{src} -> {l}");
        }
    }

    #[test]
    fn printing_is_stable() {
        for src in [r"\frac{x^2}{\sqrt{y}}", r"\sin\left(\pi x\right)", r"2\left(x+1\right)", r"\begin{cases}1&x>0\\0&x<0\end{cases}"] {
            let once = t(src);
            assert_eq!(t(&once), once, "{src}");
        }
    }
}
