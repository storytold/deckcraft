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

/// A parenthesised matrix (a bare grid reads as a mistake to most people).
fn matrix(r: usize, c: usize) -> Node {
    let grid = Node::Matrix { rows: (0..r).map(|_| (0..c).map(|_| slot()).collect()).collect() };
    Node::Delim { open: "(".into(), close: ")".into(), sep: "|".into(), items: vec![vec![grid]] }
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
        mk("matrix2", "2 x 2 matrix", "Matrices", matrix(2, 2)),
        mk("matrix3", "3 x 3 matrix", "Matrices", matrix(3, 3)),
        mk("sin", "Sine", "Functions", func("sin")),
        mk("cos", "Cosine", "Functions", func("cos")),
        mk("tan", "Tangent", "Functions", func("tan")),
        mk("log", "Logarithm", "Functions", func("log")),
        mk("ln", "Natural log", "Functions", func("ln")),
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
