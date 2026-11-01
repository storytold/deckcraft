//! The template palette: structures with empty slots (`Text("")`) the dialog fills in.

use crate::{Math, Node, Seq, Style};

/// One palette entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub math: Math,
}

fn slot() -> Seq {
    vec![Node::Text(String::new())]
}

fn delim(open: &str, close: &str) -> Node {
    Node::Delim { open: open.into(), close: close.into(), sep: "|".into(), items: vec![slot()] }
}

fn nary(op: char, limits: bool) -> Node {
    Node::Nary { op, sub: limits.then(slot), sup: limits.then(slot), body: slot() }
}

/// A bracketed matrix (a bare grid reads as a mistake to most people).
fn matrix(r: usize, c: usize, open: &str, close: &str) -> Node {
    let grid = Node::Matrix { rows: (0..r).map(|_| (0..c).map(|_| slot()).collect()).collect() };
    Node::Delim { open: open.into(), close: close.into(), sep: "|".into(), items: vec![vec![grid]] }
}

/// An empty `rows` x `cols` matrix in the given brackets, as a one-node equation.
pub fn matrix_template(rows: usize, cols: usize, open: &str, close: &str) -> Math {
    Math::new(vec![matrix(rows.max(1), cols.max(1), open, close)])
}

/// `(n over k)`: a two-row, one-column matrix in parentheses, as `\binom` reads.
fn binom() -> Node {
    let grid = Node::Matrix { rows: vec![vec![slot()], vec![slot()]] };
    Node::Delim { open: "(".into(), close: ")".into(), sep: "|".into(), items: vec![vec![grid]] }
}

/// `name(□, □)`: a function applied to two places, as `nCr(n, r)` is written.
fn call2(id: &'static str, label: &'static str, name: &str) -> Template {
    let args = Node::Delim { open: "(".into(), close: ")".into(), sep: ",".into(), items: vec![slot(), slot()] };
    Template { id, label, group: "Functions", math: Math::new(vec![Node::Func { name: name.into(), body: vec![args] }]) }
}

/// `ₙC_r`: the letter with a place before it (n) and one after it (r), as in the textbook
/// notation for combinations and permutations.
fn choose(id: &'static str, label: &'static str, letter: &str) -> Template {
    let n = Node::Script { base: vec![], sub: Some(slot()), sup: None };
    let c = Node::Script { base: vec![Node::Text(letter.into())], sub: Some(slot()), sup: None };
    Template { id, label, group: "Functions", math: Math::new(vec![n, c]) }
}

fn mk(id: &'static str, label: &'static str, group: &'static str, n: Node) -> Template {
    Template { id, label, group, math: Math::new(vec![n]) }
}

/// All templates, in palette order.
pub fn templates() -> Vec<Template> {
    let script = |sub: bool, sup: bool| Node::Script { base: slot(), sub: sub.then(slot), sup: sup.then(slot) };
    let func = |n: &str| Node::Func { name: n.into(), body: slot() };
    vec![
        mk("frac", "Fraction", "Fractions", Node::Frac { num: slot(), den: slot() }),
        mk("sup", "Superscript", "Scripts", script(false, true)),
        mk("sub", "Subscript", "Scripts", script(true, false)),
        mk("subsup", "Sub-superscript", "Scripts", script(true, true)),
        mk("sqrt", "Square root", "Radicals", Node::Rad { deg: None, body: slot() }),
        mk("root", "Nth root", "Radicals", Node::Rad { deg: Some(slot()), body: slot() }),
        mk("sum", "Sum", "Large operators", nary('∑', true)),
        mk("prod", "Product", "Large operators", nary('∏', true)),
        mk("int", "Integral", "Large operators", nary('∫', false)),
        mk("intlim", "Definite integral", "Large operators", nary('∫', true)),
        mk("paren", "Parentheses", "Brackets", delim("(", ")")),
        mk("bracket", "Square brackets", "Brackets", delim("[", "]")),
        mk("brace", "Braces", "Brackets", delim("{", "}")),
        mk("abs", "Absolute value", "Brackets", delim("|", "|")),
        mk("matrix2", "2 x 2 matrix", "Matrices", matrix(2, 2, "(", ")")),
        mk("matrix3", "3 x 3 matrix", "Matrices", matrix(3, 3, "(", ")")),
        mk("bmatrix2", "2 x 2 matrix, square brackets", "Matrices", matrix(2, 2, "[", "]")),
        mk("bmatrix3", "3 x 3 matrix, square brackets", "Matrices", matrix(3, 3, "[", "]")),
        mk("binom", "Binomial coefficient", "Fractions", binom()),
        mk("sin", "Sine", "Functions", func("sin")),
        mk("cos", "Cosine", "Functions", func("cos")),
        mk("tan", "Tangent", "Functions", func("tan")),
        mk("log", "Logarithm", "Functions", func("log")),
        mk("ln", "Natural log", "Functions", func("ln")),
        mk("sinh", "Hyperbolic sine", "Functions", func("sinh")),
        mk("cosh", "Hyperbolic cosine", "Functions", func("cosh")),
        mk("tanh", "Hyperbolic tangent", "Functions", func("tanh")),
        mk("csch", "Hyperbolic cosecant", "Functions", func("csch")),
        mk("sech", "Hyperbolic secant", "Functions", func("sech")),
        mk("coth", "Hyperbolic cotangent", "Functions", func("coth")),
        mk("round", "Round", "Functions", func("round")),
        mk("sign", "Sign", "Functions", func("sign")),
        choose("nCr", "Combinations", "C"),
        choose("nPr", "Permutations", "P"),
        call2("lcm", "Least common multiple", "lcm"),
        mk("mean", "Mean", "Functions", func("mean")),
        mk("median", "Median", "Functions", func("median")),
        mk("stdev", "Standard deviation", "Functions", func("stdev")),
        mk("var", "Variance", "Functions", func("var")),
        Template {
            id: "lim",
            label: "Limit",
            group: "Functions",
            math: Math::new(vec![
                Node::Limit { lower: true, base: vec![Node::Styled { text: "lim".into(), style: Style::Plain }], lim: slot() },
                Node::Text(String::new()),
            ]),
        },
        mk(
            "cases",
            "Cases",
            "Brackets",
            Node::Delim { open: "{".into(), close: String::new(), sep: "|".into(), items: vec![vec![Node::EqArr { rows: vec![slot(), slot()] }]] },
        ),
        mk("overline", "Overline", "Accents", Node::Bar { top: true, body: slot() }),
        mk("underline", "Underline", "Accents", Node::Bar { top: false, body: slot() }),
        mk("overbrace", "Overbrace", "Accents", Node::GroupChr { ch: '\u{23DE}', top: true, body: slot() }),
        mk("hat", "Hat", "Accents", Node::Accent { ch: '\u{0302}', body: slot() }),
        mk("bar", "Bar", "Accents", Node::Accent { ch: '\u{0305}', body: slot() }),
        mk("vec", "Vector arrow", "Accents", Node::Accent { ch: '\u{20D7}', body: slot() }),
    ]
}

/// Look a template up by id.
pub fn template(id: &str) -> Option<Template> {
    templates().into_iter().find(|t| t.id == id)
}
