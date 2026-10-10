//! The "linear" format: equations as typed text, e.g. `x^2 + sqrt(a)/b`, `pi`, `a<=b`.
//!
//! [`from_linear`] is a port of (and superset of) the old `pretty_equation` dialog helper: `^`,
//! `*`, `sqrt`, `<=`, `>=`, `!=`, `pi`, `+-` all keep working, and `a/b`, `x_1` and Greek names
//! now build real structure. [`to_linear`] prints a tree back in the same syntax (Unicode symbols
//! are written as themselves), so `from_linear(to_linear(m))` reproduces canonical trees.
//!
//! The parser is forgiving about ordinary typing: spaces around `/` and before the body of
//! `sqrt`, `sum`, `int` and a bare function (`sin x`); `lim_(x->0)`; `[a,b]`, `|x|`; `matrix(..)`
//! (parenthesised), `■(..)` (bare grid), `cases(..)`; and a LaTeX subset (`\frac{a}{b}`, `\leq`,
//! `\sqrt[3]{x}`, `\sum_{i=1}^{n}`, `\{ .. \}`, `\text{..}`, braces as grouping).

use crate::xml::{self, XEl};
use crate::{MAX_DEPTH, Math, Node, Seq, Style};

const GREEK: &[(&str, char)] = &[
    ("alpha", 'α'),
    ("beta", 'β'),
    ("gamma", 'γ'),
    ("delta", 'δ'),
    ("epsilon", 'ε'),
    ("varepsilon", 'ϵ'),
    ("zeta", 'ζ'),
    ("eta", 'η'),
    ("theta", 'θ'),
    ("vartheta", 'ϑ'),
    ("iota", 'ι'),
    ("kappa", 'κ'),
    ("lambda", 'λ'),
    ("mu", 'μ'),
    ("nu", 'ν'),
    ("xi", 'ξ'),
    ("omicron", 'ο'),
    ("pi", 'π'),
    ("varpi", 'ϖ'),
    ("rho", 'ρ'),
    ("sigma", 'σ'),
    ("varsigma", 'ς'),
    ("tau", 'τ'),
    ("upsilon", 'υ'),
    ("phi", 'φ'),
    ("varphi", 'ϕ'),
    ("chi", 'χ'),
    ("psi", 'ψ'),
    ("omega", 'ω'),
    ("Gamma", 'Γ'),
    ("Delta", 'Δ'),
    ("Theta", 'Θ'),
    ("Lambda", 'Λ'),
    ("Xi", 'Ξ'),
    ("Pi", 'Π'),
    ("Sigma", 'Σ'),
    ("Phi", 'Φ'),
    ("Psi", 'Ψ'),
    ("Omega", 'Ω'),
];

const FUNCS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "log", "ln", "exp", "lim", "max", "min", "det",
    "gcd",
];

/// Functions that take a bare (unparenthesised) argument: `sin x`.
const BARE_FUNCS: &[&str] = &["sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "log", "ln", "exp"];

/// LaTeX-style symbol names.
const SYMS: &[(&str, &str)] = &[
    ("leq", "≤"),
    ("le", "≤"),
    ("geq", "≥"),
    ("ge", "≥"),
    ("neq", "≠"),
    ("ne", "≠"),
    ("pm", "±"),
    ("mp", "∓"),
    ("cdot", "·"),
    ("div", "÷"),
    ("to", "→"),
    ("rightarrow", "→"),
    ("leftarrow", "←"),
    ("Rightarrow", "⇒"),
    ("Leftrightarrow", "⇔"),
    ("approx", "≈"),
    ("equiv", "≡"),
    ("sim", "∼"),
    ("ldots", "…"),
    ("cdots", "⋯"),
    ("dots", "…"),
    ("in", "∈"),
    ("notin", "∉"),
    ("subset", "⊂"),
    ("subseteq", "⊆"),
    ("cup", "∪"),
    ("cap", "∩"),
    ("forall", "∀"),
    ("exists", "∃"),
    ("partial", "∂"),
    ("nabla", "∇"),
    ("emptyset", "∅"),
    ("degree", "°"),
    ("angle", "∠"),
    ("perp", "⊥"),
    ("propto", "∝"),
    ("ll", "≪"),
    ("gg", "≫"),
];

const ACCENTS: &[(&str, char)] = &[
    ("hat", '\u{0302}'),
    ("tilde", '\u{0303}'),
    ("bar", '\u{0305}'),
    ("dot", '\u{0307}'),
    ("ddot", '\u{0308}'),
    ("vec", '\u{20D7}'),
    ("breve", '\u{0306}'),
    ("check", '\u{030C}'),
    ("acute", '\u{0301}'),
    ("grave", '\u{0300}'),
];

/// Operator names that LaTeX spells as commands and that print as plain words.
const PASS: &[&str] = &["mod", "bmod", "pmod", "arg", "dim", "ker", "deg", "hom", "Pr", "lg", "sgn", "limsup", "liminf", "sup"];

/// Double-struck capital for the common number sets; everything else is left alone.
fn double_struck(c: char) -> char {
    match c {
        'C' => 'ℂ',
        'H' => 'ℍ',
        'N' => 'ℕ',
        'P' => 'ℙ',
        'Q' => 'ℚ',
        'R' => 'ℝ',
        'Z' => 'ℤ',
        c => c,
    }
}

const NARY: &[(&str, char)] =
    &[("sum", '∑'), ("prod", '∏'), ("int", '∫'), ("iint", '∬'), ("iiint", '∭'), ("oint", '∮'), ("bigcup", '⋃'), ("bigcap", '⋂')];

const NARY_CHARS: &[char] = &['∑', '∏', '∫', '∬', '∭', '∮', '⋃', '⋂'];

// ---- parsing ------------------------------------------------------------------------------

struct Parser {
    c: Vec<char>,
    pos: usize,
    depth: usize,
    /// Lazily built bracket-match tables for `(`, `[` and `{` (one linear pass each).
    tables: [Option<Vec<Option<usize>>>; 3],
}

impl Parser {
    fn new(c: Vec<char>, depth: usize) -> Parser {
        Parser { c, pos: 0, depth, tables: [None, None, None] }
    }
}

const WS: fn(char) -> bool = |c| matches!(c, ' ' | '\t' | '\n' | '\r');

/// Typographic minus for an ASCII hyphen.
fn minus(c: char) -> char {
    if c == '-' { '\u{2212}' } else { c }
}

/// Nesting depth of a node, counting at most `cap` levels (the walk itself is bounded by `cap`).
fn node_depth(n: &Node, cap: usize) -> usize {
    if cap == 0 {
        return 0;
    }
    let none: &[Node] = &[];
    let mut kids: Vec<&[Node]> = Vec::new();
    match n.bare() {
        Node::Props { .. } => {}
        Node::Text(_) | Node::Styled { .. } | Node::Raw(_) => {}
        Node::Frac { num, den } => kids.extend([num.as_slice(), den.as_slice()]),
        Node::Rad { deg, body } => kids.extend([deg.as_deref().unwrap_or(none), body.as_slice()]),
        Node::Script { base, sub, sup } => kids.extend([base.as_slice(), sub.as_deref().unwrap_or(none), sup.as_deref().unwrap_or(none)]),
        Node::Nary { sub, sup, body, .. } => kids.extend([sub.as_deref().unwrap_or(none), sup.as_deref().unwrap_or(none), body.as_slice()]),
        Node::Delim { items, .. } => kids.extend(items.iter().map(|i| i.as_slice())),
        Node::Matrix { rows } => kids.extend(rows.iter().flatten().map(|c| c.as_slice())),
        Node::EqArr { rows } => kids.extend(rows.iter().map(|r| r.as_slice())),
        Node::Func { body, .. } | Node::Accent { body, .. } | Node::Bar { body, .. } | Node::GroupChr { body, .. } | Node::Boxed { body, .. } => {
            kids.push(body.as_slice())
        }
        Node::Limit { base, lim, .. } => kids.extend([base.as_slice(), lim.as_slice()]),
        Node::PreScript { sub, sup, base } => kids.extend([sub.as_slice(), sup.as_slice(), base.as_slice()]),
    }
    1 + kids.iter().map(|k| seq_depth(k, cap - 1)).max().unwrap_or(0)
}

fn seq_depth(s: &[Node], cap: usize) -> usize {
    s.iter().map(|n| node_depth(n, cap)).max().unwrap_or(0)
}

/// Trim blanks off both ends of a cell.
fn trim_seq(mut s: Seq) -> Seq {
    if let Some(Node::Text(t)) = s.first_mut() {
        *t = t.trim_start_matches(WS).to_string();
    }
    if let Some(Node::Text(t)) = s.last_mut() {
        *t = t.trim_end_matches(WS).to_string();
    }
    s.retain(|n| !matches!(n, Node::Text(t) if t.is_empty()));
    s
}

fn push_text(out: &mut Seq, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(Node::Text(t)) = out.last_mut() {
        t.push_str(s);
    } else {
        out.push(Node::Text(s.to_string()));
    }
}

fn push_node(out: &mut Seq, n: Node) {
    match n {
        Node::Text(t) => push_text(out, &t),
        n => out.push(n),
    }
}

/// Parse typed text into an equation. Never fails; bad input stays literal text.
pub fn from_linear(s: &str) -> Math {
    let mut p = Parser::new(s.chars().collect(), 0);
    Math::new(p.seq())
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.c.get(self.pos).copied()
    }
    fn at(&self, off: usize) -> Option<char> {
        self.c.get(self.pos + off).copied()
    }
    fn skip_ws(&mut self) {
        while self.peek().is_some_and(|c| c == ' ' || c == '\t') {
            self.pos += 1;
        }
    }

    /// Index of the `cl` matching the `o` at `open`, if any.
    fn matching(&mut self, open: usize, o: char, cl: char) -> Option<usize> {
        let ti = match (o, cl) {
            ('(', ')') => 0,
            ('[', ']') => 1,
            ('{', '}') => 2,
            _ => return self.matching_scan(open, o, cl),
        };
        if self.tables.get(ti).is_some_and(|t| t.is_none()) {
            let mut t: Vec<Option<usize>> = vec![None; self.c.len()];
            let mut stack: Vec<usize> = Vec::new();
            for (i, &ch) in self.c.iter().enumerate() {
                if ch == o {
                    stack.push(i);
                } else if ch == cl
                    && let Some(j) = stack.pop()
                    && let Some(slot) = t.get_mut(j)
                {
                    *slot = Some(i);
                }
            }
            if let Some(slot) = self.tables.get_mut(ti) {
                *slot = Some(t);
            }
        }
        self.tables.get(ti)?.as_ref()?.get(open).copied().flatten()
    }

    fn matching_scan(&self, open: usize, o: char, cl: char) -> Option<usize> {
        let mut d = 0usize;
        for i in open..self.c.len() {
            let ch = *self.c.get(i)?;
            if ch == o {
                d += 1;
            } else if ch == cl {
                d = d.saturating_sub(1);
                if d == 0 {
                    return Some(i);
                }
            }
        }
        None
    }

    fn sub(&self, from: usize, to: usize) -> Parser {
        Parser::new(self.c.get(from..to).map(|s| s.to_vec()).unwrap_or_default(), self.depth + 1)
    }

    /// Parse a balanced `o ... cl` group at the cursor; returns the inner sequence.
    fn group_with(&mut self, o: char, cl: char) -> Option<Seq> {
        if self.depth >= MAX_DEPTH || self.peek() != Some(o) {
            return None;
        }
        let m = self.matching(self.pos, o, cl)?;
        let inner = self.sub(self.pos + 1, m).seq();
        self.pos = m + 1;
        Some(inner)
    }

    /// `[a, b; c, d]`: a bracket group with a top-level `;` is a matrix.
    fn bracket_grid(&mut self) -> bool {
        if self.depth >= MAX_DEPTH || self.peek() != Some('[') {
            return false;
        }
        match self.matching(self.pos, '[', ']') {
            Some(m) => self.has_top_level(self.pos + 1, m, &[';']),
            None => false,
        }
    }

    fn group(&mut self) -> Option<Seq> {
        self.group_with('(', ')')
    }

    fn seq(&mut self) -> Seq {
        let mut out = Seq::new();
        while let Some(ch) = self.peek() {
            match ch {
                '(' => {
                    let start = self.pos;
                    match self.group() {
                        Some(inner) => out.push(Node::Delim { open: "(".into(), close: ")".into(), sep: "|".into(), items: vec![inner] }),
                        None => {
                            self.pos = start + 1;
                            push_text(&mut out, "(");
                        }
                    }
                }
                '[' if self.bracket_grid() => {
                    let m = self.matching(self.pos, '[', ']').unwrap_or(self.pos);
                    let rows = self.grid(self.pos + 1, m, ',', ';');
                    self.pos = m + 1;
                    out.push(Node::Delim { open: "[".into(), close: "]".into(), sep: "|".into(), items: vec![vec![Node::Matrix { rows }]] });
                }
                '[' => match self.group_with('[', ']') {
                    Some(inner) => out.push(Node::Delim { open: "[".into(), close: "]".into(), sep: "|".into(), items: vec![inner] }),
                    None => {
                        self.pos += 1;
                        push_text(&mut out, "[");
                    }
                },
                '{' => match self.group_with('{', '}') {
                    // Braces only group; their content is spliced in.
                    Some(inner) => {
                        for n in inner {
                            push_node(&mut out, n);
                        }
                    }
                    None => {
                        self.pos += 1;
                        push_text(&mut out, "{");
                    }
                },
                '|' => self.bar_group(&mut out),
                '\\' => self.command(&mut out),
                '^' | '_' => {
                    self.pos += 1;
                    let operand = self.atom(false);
                    attach_script(&mut out, ch == '^', operand);
                }
                '/' => {
                    self.pos += 1;
                    if out.last().is_some_and(|n| node_depth(n, MAX_DEPTH) + 1 >= MAX_DEPTH) {
                        // Too deeply nested to wrap again: keep the slash literal.
                        push_text(&mut out, "/");
                        continue;
                    }
                    let num = take_numerator(&mut out);
                    let den = self.denominator();
                    out.push(Node::Frac { num, den });
                }
                '*' => {
                    self.pos += 1;
                    push_text(&mut out, "·");
                }
                '<' if self.at(1) == Some('=') => {
                    self.pos += 2;
                    push_text(&mut out, "≤");
                }
                '>' if self.at(1) == Some('=') => {
                    self.pos += 2;
                    push_text(&mut out, "≥");
                }
                '!' if self.at(1) == Some('=') => {
                    self.pos += 2;
                    push_text(&mut out, "≠");
                }
                '+' if self.at(1) == Some('-') => {
                    self.pos += 2;
                    push_text(&mut out, "±");
                }
                '-' if self.at(1) == Some('>') => {
                    self.pos += 2;
                    push_text(&mut out, "→");
                }
                '√' => {
                    self.pos += 1;
                    let n = self.radical();
                    out.push(n);
                }
                '■' if self.at(1) == Some('(') => {
                    self.pos += 1;
                    match self.matrix() {
                        Some(n) => out.push(n),
                        None => push_text(&mut out, "■"),
                    }
                }
                '█' if self.at(1) == Some('(') => {
                    self.pos += 1;
                    match self.eq_array() {
                        Some(n) => out.push(n),
                        None => push_text(&mut out, "█"),
                    }
                }
                c if NARY_CHARS.contains(&c) => {
                    self.pos += 1;
                    let n = self.nary(c);
                    out.push(n);
                }
                c if c.is_ascii_alphabetic() => {
                    let w = self.word();
                    match self.special(&w) {
                        Some(n) => push_node(&mut out, n),
                        None => push_text(&mut out, &w),
                    }
                }
                c => {
                    self.pos += 1;
                    push_text(&mut out, &minus(c).to_string());
                }
            }
        }
        out
    }

    /// `|x|` absolute value when the first bar opens (not directly after a letter or `)`).
    fn bar_group(&mut self, out: &mut Seq) {
        let opens = match out.last() {
            None => true,
            Some(Node::Text(t)) => t.chars().last().is_some_and(|c| !c.is_alphanumeric() && c != ')' && c != '|'),
            Some(_) => false,
        };
        let close = if opens && self.depth < MAX_DEPTH { (self.pos + 1..self.c.len()).find(|&i| self.c.get(i) == Some(&'|')) } else { None };
        match close {
            Some(j) if j > self.pos + 1 => {
                let inner = self.sub(self.pos + 1, j).seq();
                self.pos = j + 1;
                out.push(Node::Delim { open: "|".into(), close: "|".into(), sep: "|".into(), items: vec![inner] });
            }
            _ => {
                self.pos += 1;
                push_text(out, "|");
            }
        }
    }

    /// A LaTeX-style command or escape at the cursor (`\frac`, `\leq`, `\{`).
    fn command(&mut self, out: &mut Seq) {
        self.pos += 1;
        let Some(c) = self.peek() else { return push_text(out, "\\") };
        if !c.is_ascii_alphabetic() {
            self.pos += 1;
            match c {
                '{' => self.escaped_brace(out),
                ',' | ';' | ':' | ' ' => push_text(out, " "),
                '!' => {}
                c => push_text(out, &c.to_string()),
            }
            return;
        }
        let w = self.word();
        match w.as_str() {
            "frac" | "dfrac" | "tfrac" => {
                let num = self.brace_arg();
                let den = self.brace_arg();
                out.push(Node::Frac { num, den });
            }
            "begin" => self.environment(out),
            "binom" | "dbinom" | "tbinom" => {
                let n = self.brace_arg();
                let k = self.brace_arg();
                let m = Node::Matrix { rows: vec![vec![n], vec![k]] };
                out.push(Node::Delim { open: "(".into(), close: ")".into(), sep: "|".into(), items: vec![vec![m]] });
            }
            "mathbb" => {
                let arg = self.brace_arg();
                for n in arg {
                    match n {
                        Node::Text(t) => push_text(out, &t.chars().map(double_struck).collect::<String>()),
                        n => push_node(out, n),
                    }
                }
            }
            "mathcal" | "mathscr" | "mathfrak" | "mathit" | "mathsf" | "mathtt" | "boldsymbol" | "bm" | "textit" | "textsf" | "emph" | "mbox"
            | "hbox" | "mathnormal" => {
                for n in self.brace_arg() {
                    push_node(out, n);
                }
            }
            "widehat" | "widetilde" | "overrightarrow" | "vec" | "hat" | "tilde" | "bar" | "dot" | "ddot" | "breve" | "check" | "acute" | "grave"
                if {
                    self.skip_ws();
                    self.peek().is_some()
                } =>
            {
                let name = match w.as_str() {
                    "widehat" => "hat",
                    "widetilde" => "tilde",
                    "overrightarrow" => "vec",
                    o => o,
                };
                let body = self.brace_arg();
                if let Some((_, ch)) = ACCENTS.iter().find(|(n, _)| *n == name) {
                    out.push(Node::Accent { ch: *ch, body });
                }
            }
            "overline" | "underline" | "overbrace" | "underbrace" | "boxed" | "box" | "fbox"
                if {
                    self.skip_ws();
                    self.peek() == Some('{')
                } =>
            {
                let body = self.brace_arg();
                out.push(match w.as_str() {
                    "overline" => Node::Bar { top: true, body },
                    "underline" => Node::Bar { top: false, body },
                    "overbrace" => Node::GroupChr { ch: '\u{23DE}', top: true, body },
                    "underbrace" => Node::GroupChr { ch: '\u{23DF}', top: false, body },
                    "boxed" | "fbox" => Node::Boxed { border: true, body },
                    _ => Node::Boxed { border: false, body },
                });
            }
            "left" | "right" | "big" | "Big" | "bigl" | "bigr" | "Bigl" | "Bigr" | "displaystyle" | "quad" | "qquad" => {}
            "text" | "textrm" | "mathrm" | "operatorname" | "mathbf" | "textbf" => {
                let style = match w.as_str() {
                    "text" | "textrm" => Style::Normal,
                    "mathbf" | "textbf" => Style::Bold,
                    _ => Style::Plain,
                };
                self.skip_ws();
                if self.peek() == Some('{')
                    && let Some(m) = self.matching(self.pos, '{', '}')
                {
                    let text: String = self.c.get(self.pos + 1..m).map(|s| s.iter().collect()).unwrap_or_default();
                    self.pos = m + 1;
                    if !text.is_empty() {
                        out.push(Node::Styled { text, style });
                    }
                }
            }
            _ => {
                if let Some(n) = self.special(&w) {
                    push_node(out, n);
                } else if let Some((_, sym)) = SYMS.iter().find(|(n, _)| *n == w) {
                    push_text(out, sym);
                } else if FUNCS.contains(&w.as_str()) || PASS.contains(&w.as_str()) {
                    push_text(out, &w);
                } else {
                    // Unknown command: keep it visible, backslash included.
                    push_text(out, &format!("\\{w}"));
                }
            }
        }
    }

    /// After `\{`: a delimiter pair up to the matching `\}`, else a literal brace.
    fn escaped_brace(&mut self, out: &mut Seq) {
        let mut d = 1usize;
        let mut i = self.pos;
        let mut close = None;
        while i + 1 < self.c.len() && self.depth < MAX_DEPTH {
            match (self.c.get(i), self.c.get(i + 1)) {
                (Some('\\'), Some('{')) => {
                    d += 1;
                    i += 2;
                }
                (Some('\\'), Some('}')) => {
                    d -= 1;
                    if d == 0 {
                        close = Some(i);
                        break;
                    }
                    i += 2;
                }
                _ => i += 1,
            }
        }
        match close {
            Some(j) => {
                let inner = self.sub(self.pos, j).seq();
                self.pos = j + 2;
                out.push(Node::Delim { open: "{".into(), close: "}".into(), sep: "|".into(), items: vec![inner] });
            }
            None => push_text(out, "{"),
        }
    }

    fn at_str(&self, i: usize, s: &str) -> bool {
        s.chars().enumerate().all(|(k, ch)| self.c.get(i + k) == Some(&ch))
    }

    /// Index of the `\end{name}` closing an already-consumed `\begin{name}`.
    fn find_end(&self, from: usize, name: &str) -> Option<usize> {
        let begin = format!("\\begin{{{name}}}");
        let end = format!("\\end{{{name}}}");
        let (bl, el) = (begin.chars().count(), end.chars().count());
        let (mut d, mut i) = (1usize, from);
        while i < self.c.len() {
            if self.at_str(i, &end) {
                d = d.saturating_sub(1);
                if d == 0 {
                    return Some(i);
                }
                i += el;
            } else if self.at_str(i, &begin) {
                d += 1;
                i += bl;
            } else {
                i += 1;
            }
        }
        None
    }

    fn cell(&mut self, from: usize, to: usize) -> Seq {
        trim_seq(self.sub(from, to).seq())
    }

    /// Rows and cells of a LaTeX environment body (`&` between cells, `\\` between rows).
    fn env_cells(&mut self, from: usize, to: usize) -> Vec<Vec<Seq>> {
        let mut rows: Vec<Vec<Seq>> = Vec::new();
        let mut row: Vec<Seq> = Vec::new();
        let (mut start, mut i, mut envd, mut brace) = (from, from, 0usize, 0usize);
        while i < to {
            let ch = self.c.get(i).copied().unwrap_or(' ');
            if ch == '\\' {
                match self.c.get(i + 1).copied() {
                    Some('\\') => {
                        if envd == 0 && brace == 0 {
                            row.push(self.cell(start, i));
                            rows.push(std::mem::take(&mut row));
                            start = i + 2;
                        }
                        i += 2;
                    }
                    Some(c) if c.is_ascii_alphabetic() => {
                        if self.at_str(i, "\\begin") {
                            envd += 1;
                            i += 6;
                        } else if self.at_str(i, "\\end") {
                            envd = envd.saturating_sub(1);
                            i += 4;
                        } else {
                            i += 1;
                        }
                    }
                    Some(_) => i += 2,
                    None => i += 1,
                }
                continue;
            }
            match ch {
                '{' => brace += 1,
                '}' => brace = brace.saturating_sub(1),
                '&' if envd == 0 && brace == 0 => {
                    row.push(self.cell(start, i));
                    start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        let last = self.cell(start, to);
        if !row.is_empty() || !last.is_empty() {
            row.push(last);
            rows.push(row);
        }
        rows
    }

    /// After `\begin`: a matrix, cases or aligned environment up to its `\end`.
    fn environment(&mut self, out: &mut Seq) {
        self.skip_ws();
        let Some(name) = self.raw_braces() else { return push_text(out, "\\begin") };
        if name == "array" {
            self.skip_ws();
            let _ = self.raw_braces();
        }
        let start = self.pos;
        let end = if self.depth < MAX_DEPTH { self.find_end(start, &name) } else { None };
        let Some(end) = end else {
            // Unterminated or too deep: keep the opener visible and carry on.
            push_text(out, &format!("\\begin{{{name}}}"));
            return;
        };
        let rows = self.env_cells(start, end);
        self.pos = end + format!("\\end{{{name}}}").chars().count();
        let base = name.trim_end_matches('*');
        let flat = |rows: Vec<Vec<Seq>>, sep: &str| -> Vec<Seq> {
            rows.into_iter()
                .map(|r| {
                    let mut o = Seq::new();
                    for (k, c) in r.into_iter().enumerate() {
                        if k > 0 && !sep.is_empty() {
                            push_text(&mut o, sep);
                        }
                        for n in c {
                            push_node(&mut o, n);
                        }
                    }
                    o
                })
                .collect()
        };
        let delim =
            |open: &str, close: &str, inner: Node| Node::Delim { open: open.into(), close: close.into(), sep: "|".into(), items: vec![vec![inner]] };
        let node = match base {
            "pmatrix" => delim("(", ")", Node::Matrix { rows }),
            "bmatrix" => delim("[", "]", Node::Matrix { rows }),
            "Bmatrix" => delim("{", "}", Node::Matrix { rows }),
            "vmatrix" => delim("|", "|", Node::Matrix { rows }),
            "Vmatrix" => delim("\u{2016}", "\u{2016}", Node::Matrix { rows }),
            "matrix" | "smallmatrix" | "array" => Node::Matrix { rows },
            "cases" => delim("{", "", Node::EqArr { rows: flat(rows, "\u{2003}") }),
            _ => Node::EqArr { rows: flat(rows, "") },
        };
        out.push(node);
    }

    /// Raw text of a `{...}` group at the cursor (no parsing).
    fn raw_braces(&mut self) -> Option<String> {
        if self.peek() != Some('{') {
            return None;
        }
        let m = self.matching(self.pos, '{', '}')?;
        let t: String = self.c.get(self.pos + 1..m)?.iter().collect();
        self.pos = m + 1;
        Some(t)
    }

    fn brace_arg(&mut self) -> Seq {
        if self.depth >= MAX_DEPTH {
            return vec![];
        }
        self.skip_ws();
        self.depth += 1;
        let r = self.atom(false);
        self.depth -= 1;
        r
    }

    fn word(&mut self) -> String {
        let mut w = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() {
                w.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        w
    }

    /// A word with meaning (Greek letter, `sqrt`, function, accent, n-ary). Consumes input only
    /// when it returns `Some`.
    fn special(&mut self, w: &str) -> Option<Node> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let r = self.special_inner(w);
        self.depth -= 1;
        r
    }

    fn special_inner(&mut self, w: &str) -> Option<Node> {
        if let Some((_, g)) = GREEK.iter().find(|(n, _)| *n == w) {
            return Some(Node::Text(g.to_string()));
        }
        match w {
            "sqrt" => return Some(self.radical()),
            "inf" | "infty" => return Some(Node::Text("∞".into())),
            "times" => return Some(Node::Text("×".into())),
            _ => {}
        }
        if let Some((_, op)) = NARY.iter().find(|(n, _)| *n == w) {
            return Some(self.nary(*op));
        }
        if matches!(w, "lim" | "max" | "min") && self.peek() == Some('_') && self.depth < MAX_DEPTH {
            self.pos += 1;
            let lim = self.atom(false);
            return Some(Node::Limit { lower: true, base: vec![Node::Styled { text: w.to_string(), style: Style::Plain }], lim });
        }
        if self.peek() == Some('(') && self.depth < MAX_DEPTH {
            if FUNCS.contains(&w) {
                let body = self.group()?;
                return Some(Node::Func { name: w.to_string(), body });
            }
            if w == "root" {
                let deg = self.group()?;
                self.skip_ws();
                let body = self.operand(true);
                return Some(Node::Rad { deg: Some(deg), body });
            }
            if let Some((_, ch)) = ACCENTS.iter().find(|(n, _)| *n == w) {
                let body = self.group()?;
                return Some(Node::Accent { ch: *ch, body });
            }
            let delims = match w {
                "matrix" | "pmatrix" => Some(("(", ")")),
                "bmatrix" => Some(("[", "]")),
                "vmatrix" => Some(("|", "|")),
                "Bmatrix" => Some(("{", "}")),
                _ => None,
            };
            if let Some((open, close)) = delims {
                let m = self.matrix()?;
                return Some(Node::Delim { open: open.into(), close: close.into(), sep: "|".into(), items: vec![vec![m]] });
            }
            match w {
                "eqarr" => return self.eq_array(),
                "cases" => {
                    let a = self.eq_array()?;
                    return Some(Node::Delim { open: "{".into(), close: String::new(), sep: "|".into(), items: vec![vec![a]] });
                }
                "overline" | "underline" => {
                    let body = self.group()?;
                    return Some(Node::Bar { top: w == "overline", body });
                }
                "overbrace" | "underbrace" => {
                    let body = self.group()?;
                    let top = w == "overbrace";
                    return Some(Node::GroupChr { ch: if top { '\u{23DE}' } else { '\u{23DF}' }, top, body });
                }
                "box" | "boxed" => {
                    let body = self.group()?;
                    return Some(Node::Boxed { border: w == "boxed", body });
                }
                _ => {}
            }
        }
        if FUNCS.contains(&w) && matches!(self.peek(), Some('^' | '_')) {
            // `sin^2 x`, `log_2 x`: keep the name whole so the script attaches to it.
            return Some(Node::Styled { text: w.to_string(), style: Style::Plain });
        }
        if BARE_FUNCS.contains(&w) && self.depth < MAX_DEPTH {
            // `sin x`, `ln 2`, `cos\theta`: an unparenthesised argument.
            let save = self.pos;
            self.skip_ws();
            if self.peek().is_some_and(|c| c.is_alphanumeric() || c == '\\' || c == '√') {
                let body = self.operand(true);
                return Some(Node::Func { name: w.to_string(), body });
            }
            self.pos = save;
        }
        None
    }

    fn radical(&mut self) -> Node {
        self.skip_ws();
        let mut deg = None;
        if self.peek() == Some('[')
            && let Some(m) = self.matching(self.pos, '[', ']')
            && self.depth < MAX_DEPTH
        {
            deg = Some(self.sub(self.pos + 1, m).seq());
            self.pos = m + 1;
        }
        self.skip_ws();
        let body = self.operand(true);
        Node::Rad { deg, body }
    }

    fn nary(&mut self, op: char) -> Node {
        let (mut sub, mut sup) = (None, None);
        for _ in 0..2 {
            match self.peek() {
                Some('_') if sub.is_none() => {
                    self.pos += 1;
                    sub = Some(self.atom(false));
                }
                Some('^') if sup.is_none() => {
                    self.pos += 1;
                    sup = Some(self.atom(false));
                }
                _ => break,
            }
        }
        self.skip_ws();
        let mut body = self.operand(true);
        loop {
            // `sum_(i=1)^n 1/i`: a fraction in the body belongs to the sum.
            let save = self.pos;
            self.skip_ws();
            if self.peek() == Some('/') && seq_depth(&body, MAX_DEPTH) + 1 < MAX_DEPTH {
                self.pos += 1;
                let den = self.denominator();
                body = vec![Node::Frac { num: body, den }];
            } else {
                self.pos = save;
                break;
            }
        }
        if matches!(op, '∫' | '∬' | '∭' | '∮') {
            // `int_0^1 x^2 dx`: the differential belongs to the integrand.
            let save = self.pos;
            self.skip_ws();
            if self.peek() == Some('d') && self.at(1).is_some_and(|c| c.is_ascii_alphabetic()) && !self.at(2).is_some_and(|c| c.is_ascii_alphabetic())
            {
                let dx: String = [self.at(0), self.at(1)].into_iter().flatten().collect();
                self.pos += 2;
                push_node(&mut body, Node::Text(format!(" {dx}")));
            } else {
                self.pos = save;
            }
        }
        Node::Nary { op, sub, sup, body }
    }

    fn matrix(&mut self) -> Option<Node> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        let m = self.matching(self.pos, '(', ')')?;
        let (from, to) = (self.pos + 1, m);
        // Typed form `matrix(a,b;c,d)`: when no `&`/`@` appears at top level, `,` splits
        // cells and `;` splits rows.
        let formal = self.has_top_level(from, to, &['&', '@']);
        let (cs, rs) = if formal { ('&', '@') } else { (',', ';') };
        let rows = self.grid(from, to, cs, rs);
        self.pos = m + 1;
        Some(Node::Matrix { rows })
    }

    /// True when one of `chars` occurs in `from..to` outside any bracket nesting.
    fn has_top_level(&self, from: usize, to: usize, chars: &[char]) -> bool {
        let mut depth = 0usize;
        for i in from..to {
            match self.c.get(i).copied() {
                Some('(' | '[' | '{') => depth += 1,
                Some(')' | ']' | '}') => depth = depth.saturating_sub(1),
                Some(c) if depth == 0 && chars.contains(&c) => return true,
                _ => {}
            }
        }
        false
    }

    /// Split `from..to` into rows (`rs`) of cells (`cs`) at bracket depth zero.
    fn grid(&mut self, from: usize, to: usize, cs: char, rs: char) -> Vec<Vec<Seq>> {
        let mut rows: Vec<Vec<Seq>> = vec![];
        let mut row: Vec<Seq> = vec![];
        let (mut depth, mut start) = (0usize, from);
        for i in from..=to {
            let ch = if i == to { '\u{0}' } else { self.c.get(i).copied().unwrap_or('\u{0}') };
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth = depth.saturating_sub(1),
                c if depth == 0 && (c == cs || c == rs || i == to) => {
                    row.push(trim_seq(self.sub(start, i).seq()));
                    start = i + 1;
                    if c != cs {
                        rows.push(std::mem::take(&mut row));
                    }
                }
                _ => {}
            }
        }
        rows
    }

    /// `█(a@b)`: lines separated by `@` (cells within a line are concatenated).
    fn eq_array(&mut self) -> Option<Node> {
        match self.matrix()? {
            Node::Matrix { rows } => Some(Node::EqArr { rows: rows.into_iter().map(|r| r.into_iter().flatten().collect()).collect() }),
            _ => None,
        }
    }

    /// An atom followed by any scripts, used for radicands and n-ary bodies.
    fn operand(&mut self, full_word: bool) -> Seq {
        self.operand_ex(full_word, false)
    }

    /// Like [`operand`](Self::operand); `coef` lets a number absorb letters stuck to it (`2a`).
    fn operand_ex(&mut self, full_word: bool, coef: bool) -> Seq {
        if self.depth >= MAX_DEPTH {
            return vec![];
        }
        self.depth += 1;
        let numeric = self.peek().is_some_and(|c| c.is_ascii_digit());
        let mut out = self.atom(full_word);
        if coef
            && numeric
            && matches!(out.as_slice(), [Node::Text(t)] if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || c == '.'))
            && self.peek().is_some_and(|c| c.is_ascii_alphabetic())
        {
            let w = self.word();
            match self.special(&w) {
                Some(n) => push_node(&mut out, n),
                None => push_text(&mut out, &w),
            }
        }
        while let Some(ch @ ('^' | '_')) = self.peek() {
            self.pos += 1;
            let operand = self.atom(false);
            attach_script(&mut out, ch == '^', operand);
        }
        self.depth -= 1;
        out
    }

    /// The right-hand side of `/`: leading spaces are skipped, `2a` is one term.
    fn denominator(&mut self) -> Seq {
        self.skip_ws();
        let mut d = self.operand_ex(true, true);
        if self.peek() == Some('!') && self.at(1) != Some('=') {
            self.pos += 1;
            push_text(&mut d, "!");
        }
        d
    }

    /// One atom: a group (delimiters stripped), a number, a word, a command or a single symbol.
    fn atom(&mut self, full_word: bool) -> Seq {
        let Some(ch) = self.peek() else { return vec![] };
        if ch == '(' || ch == '{' {
            let start = self.pos;
            let closer = if ch == '(' { ')' } else { '}' };
            return match self.group_with(ch, closer) {
                Some(inner) => inner,
                None => {
                    self.pos = start + 1;
                    vec![Node::Text(ch.to_string())]
                }
            };
        }
        if ch == '\\' {
            let mut tmp = Seq::new();
            self.command(&mut tmp);
            return tmp;
        }
        if ch == '√' {
            self.pos += 1;
            return vec![self.radical()];
        }
        let signed = (ch == '+' || ch == '-') && self.at(1).is_some_and(|c| c.is_ascii_digit());
        if ch.is_ascii_digit() || signed {
            let mut s = String::new();
            if signed {
                s.push(minus(ch));
                self.pos += 1;
            }
            while let Some(d) = self.peek() {
                let dot = d == '.' && self.at(1).is_some_and(|c| c.is_ascii_digit());
                if d.is_ascii_digit() || dot {
                    s.push(d);
                    self.pos += 1;
                } else {
                    break;
                }
            }
            return vec![Node::Text(s)];
        }
        if (ch == '+' || ch == '-') && self.at(1).is_some_and(|c| c.is_alphabetic()) {
            self.pos += 1;
            let mut rest = self.atom(false);
            let mut out = vec![Node::Text(minus(ch).to_string())];
            out.append(&mut rest);
            let mut merged = Seq::new();
            for n in out {
                push_node(&mut merged, n);
            }
            return merged;
        }
        if ch.is_ascii_alphabetic() {
            let start = self.pos;
            let w = self.word();
            if let Some(n) = self.special(&w) {
                return vec![n];
            }
            if full_word {
                return vec![Node::Text(w)];
            }
            self.pos = start + 1;
            return vec![Node::Text(ch.to_string())];
        }
        self.pos += 1;
        vec![Node::Text(minus(ch).to_string())]
    }
}

/// Remove and return the base a script attaches to.
fn pop_base(out: &mut Seq) -> Seq {
    match out.pop() {
        None => vec![],
        Some(Node::Text(t)) => {
            let chars: Vec<char> = t.chars().collect();
            let mut i = chars.len();
            if chars.last().is_some_and(|c| c.is_ascii_digit()) {
                while i > 0 && chars.get(i - 1).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                    i -= 1;
                }
            } else {
                i = i.saturating_sub(1);
            }
            let head: String = chars.iter().take(i).collect();
            let tail: String = chars.iter().skip(i).collect();
            if !head.is_empty() {
                out.push(Node::Text(head));
            }
            vec![Node::Text(tail)]
        }
        Some(n) => vec![n],
    }
}

fn attach_script(out: &mut Seq, sup: bool, operand: Seq) {
    if out.last().is_some_and(|n| node_depth(n, MAX_DEPTH) + 1 >= MAX_DEPTH) {
        // Scripts on scripts on scripts...: past the depth cap the mark stays literal.
        push_text(out, if sup { "^" } else { "_" });
        for n in operand {
            push_node(out, n);
        }
        return;
    }
    if let Some(Node::Script { sub: s, sup: p, .. }) = out.last_mut() {
        let slot = if sup { p } else { s };
        if slot.is_none() {
            *slot = Some(operand);
            return;
        }
    }
    let base = pop_base(out);
    let (sub, sup_v) = if sup { (None, Some(operand)) } else { (Some(operand), None) };
    out.push(Node::Script { base, sub, sup: sup_v });
}

/// Drop trailing blanks from the last text node (`a / b`: the space before `/` is not part of
/// the numerator and is not kept).
fn trim_trailing_ws(out: &mut Seq) {
    if let Some(Node::Text(t)) = out.last_mut() {
        let trimmed = t.trim_end().len();
        t.truncate(trimmed);
        if t.is_empty() {
            out.pop();
        }
    }
}

fn take_numerator(out: &mut Seq) -> Seq {
    trim_trailing_ws(out);
    match out.pop() {
        None => vec![],
        Some(Node::Text(t)) => {
            let chars: Vec<char> = t.chars().collect();
            let mut i = chars.len();
            if chars.last() == Some(&'!') {
                i -= 1;
            }
            while i > 0 && chars.get(i - 1).is_some_and(|c| c.is_alphanumeric() || *c == '.') {
                i -= 1;
            }
            let head: String = chars.iter().take(i).collect();
            let tail: String = chars.iter().skip(i).collect();
            if !head.is_empty() {
                out.push(Node::Text(head));
            }
            if tail.is_empty() { vec![] } else { vec![Node::Text(tail)] }
        }
        Some(Node::Delim { open, close, items, .. }) if open == "(" && close == ")" && items.len() == 1 => {
            items.into_iter().next().unwrap_or_default()
        }
        Some(n) => vec![n],
    }
}

// ---- printing -----------------------------------------------------------------------------

/// Print an equation in the linear format.
pub fn to_linear(m: &Math) -> String {
    let mut out = String::new();
    seq(&mut out, &m.body, 0);
    out
}

fn lin(s: &Seq, depth: usize) -> String {
    let mut o = String::new();
    seq(&mut o, s, depth);
    o
}

fn run_atomic(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '.')
}

fn number(s: &str) -> bool {
    let t = s.strip_prefix(['+', '-', '\u{2212}']).unwrap_or(s);
    !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || c == '.') && t.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn script_atomic(s: &str) -> bool {
    number(s) || (s.chars().count() == 1 && s.chars().all(|c| c.is_alphabetic()))
}

fn base_atomic(s: &str) -> bool {
    (s.chars().count() == 1 && s.chars().all(|c| c.is_alphabetic())) || (!s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

fn paren(s: String, atomic: bool) -> String {
    if atomic { s } else { format!("({s})") }
}

/// Escape characters the parser treats as syntax in literal text.
fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '{' | '}') {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

/// Would the node, printed right after an unparenthesised denominator, fuse with it?
fn starts_alnum(n: Option<&Node>, depth: usize) -> bool {
    match n.map(Node::bare) {
        Some(Node::Text(t)) | Some(Node::Styled { text: t, .. }) => t.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '.'),
        Some(Node::Func { .. }) | Some(Node::Limit { .. }) => true,
        Some(Node::Script { base, .. }) if depth < MAX_DEPTH => starts_alnum(base.first(), depth + 1),
        _ => false,
    }
}

/// Visible text inside an unknown element, so a `Raw` node still reads as something.
fn raw_text(src: &str) -> String {
    fn walk(e: &XEl, out: &mut String, d: usize) {
        if d > MAX_DEPTH + 16 {
            return;
        }
        if e.local() == "t" {
            out.push_str(&e.text());
        } else {
            for c in e.els() {
                walk(c, out, d + 1);
            }
        }
    }
    let mut s = String::new();
    for r in &xml::parse(src) {
        walk(r, &mut s, 0);
    }
    s
}

fn limits(out: &mut String, sub: &Option<Seq>, sup: &Option<Seq>, d: usize) {
    for (mark, part) in [('_', sub), ('^', sup)] {
        if let Some(s) = part {
            let t = lin(s, d);
            let a = script_atomic(&t);
            out.push(mark);
            out.push_str(&paren(t, a));
        }
    }
}

fn seq(out: &mut String, s: &Seq, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    let d = depth + 1;
    for (i, n) in s.iter().enumerate() {
        match n.bare() {
            Node::Props { .. } => {}
            Node::Text(t) => out.push_str(&esc(t)),
            Node::Styled { text, .. } => out.push_str(&esc(text)),
            Node::Frac { num, den } => {
                let (a, b) = (lin(num, d), lin(den, d));
                let glued = out.chars().last().is_some_and(|c| c.is_alphanumeric() || c == '.');
                let na = run_atomic(&a) && !glued;
                out.push_str(&paren(a, na));
                out.push('/');
                let da = run_atomic(&b) && !starts_alnum(s.get(i + 1), 0);
                out.push_str(&paren(b, da));
            }
            Node::Rad { deg, body } => {
                out.push('√');
                if let Some(dg) = deg {
                    out.push('[');
                    out.push_str(&lin(dg, d));
                    out.push(']');
                }
                out.push_str(&format!("({})", lin(body, d)));
            }
            Node::Script { base, sub, sup } => {
                let b = lin(base, d);
                let ba = base_atomic(&b)
                    || matches!(base.as_slice(), [Node::Delim { .. }])
                    || matches!(base.as_slice(), [Node::Styled { text, .. }] if FUNCS.contains(&text.as_str()));
                out.push_str(&paren(b, ba));
                limits(out, sub, sup, d);
            }
            Node::Nary { op, sub, sup, body } => {
                if *op != '\0' {
                    out.push(*op);
                }
                limits(out, sub, sup, d);
                out.push_str(&format!("({})", lin(body, d)));
            }
            Node::Delim { open, close, items, .. }
                if open == "{" && close.is_empty() && matches!(items.as_slice(), [it] if matches!(it.as_slice(), [Node::EqArr { .. }])) =>
            {
                let rows = match items.first().and_then(|it| it.first()) {
                    Some(Node::EqArr { rows }) => rows.iter().map(|r| lin(r, d)).collect::<Vec<_>>(),
                    _ => vec![],
                };
                out.push_str(&format!("cases({})", rows.join("@")));
            }
            Node::Delim { open, close, sep, items } => {
                out.push_str(&esc(open));
                let parts: Vec<String> = items.iter().map(|i| lin(i, d)).collect();
                out.push_str(&parts.join(&esc(sep)));
                out.push_str(&esc(close));
            }
            Node::Matrix { rows } => {
                let rs: Vec<String> = rows.iter().map(|r| r.iter().map(|c| lin(c, d)).collect::<Vec<_>>().join("&")).collect();
                out.push_str(&format!("■({})", rs.join("@")));
            }
            Node::EqArr { rows } => {
                let rs: Vec<String> = rows.iter().map(|r| lin(r, d)).collect();
                out.push_str(&format!("█({})", rs.join("@")));
            }
            Node::Func { name, body } => out.push_str(&format!("{name}({})", lin(body, d))),
            Node::Limit { lower, base, lim } => {
                let b = lin(base, d);
                let t = lin(lim, d);
                let a = script_atomic(&t);
                if *lower && matches!(b.as_str(), "lim" | "max" | "min") {
                    out.push_str(&b);
                    out.push('_');
                } else {
                    // No linear spelling for other bases: print it as a script.
                    out.push_str(&paren(b.clone(), base_atomic(&b)));
                    out.push(if *lower { '_' } else { '^' });
                }
                out.push_str(&paren(t, a));
            }
            Node::PreScript { sub, sup, base } => {
                // No linear spelling: the scripts are written first, in script syntax.
                out.push('_');
                out.push_str(&format!("({})", lin(sub, d)));
                out.push('^');
                out.push_str(&format!("({})", lin(sup, d)));
                out.push_str(&lin(base, d));
            }
            Node::Bar { top, body } => out.push_str(&format!("{}({})", if *top { "overline" } else { "underline" }, lin(body, d))),
            Node::GroupChr { ch, top, body } => match (*ch, *top) {
                ('\u{23DE}', true) => out.push_str(&format!("overbrace({})", lin(body, d))),
                ('\u{23DF}', false) => out.push_str(&format!("underbrace({})", lin(body, d))),
                _ => out.push_str(&lin(body, d)),
            },
            Node::Boxed { border, body } => out.push_str(&format!("{}({})", if *border { "boxed" } else { "box" }, lin(body, d))),
            Node::Accent { ch, body } => match ACCENTS.iter().find(|(_, c)| c == ch) {
                Some((name, _)) => out.push_str(&format!("{name}({})", lin(body, d))),
                None => {
                    out.push_str(&lin(body, d));
                    out.push(*ch);
                }
            },
            Node::Raw(src) if src == crate::OMATH_BREAK => out.push(' '),
            // Argument properties (`m:argPr`) carry no visible content.
            Node::Raw(src) if src.starts_with("<m:argPr") => {}
            Node::Raw(src) => {
                let t = raw_text(src);
                if t.is_empty() {
                    out.push('□');
                } else {
                    out.push_str(&esc(&t));
                }
            }
        }
    }
}

/// Tree depth for tests (bounded walk).
#[cfg(test)]
pub(crate) fn seq_depth_for_tests(s: &[Node]) -> usize {
    seq_depth(s, 100_000)
}
