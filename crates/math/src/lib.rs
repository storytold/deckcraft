//! DeckCraft equations: the math tree, OMML (Office Math Markup) import and export, a typed
//! "linear" text format, and the template palette.
//!
//! Layer 1, no dependency on the model or fonts: layout lives in `deckcraft-text`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod linear;
mod omml;
mod templates;
mod xml;
pub(crate) use xml::XML_DEPTH;

pub use linear::{from_linear, to_linear};
pub use omml::{OMATH_BREAK, OMML_NS, from_omml, to_omml};
pub use templates::{Template, template, templates};

use serde::{Deserialize, Serialize};

/// Maximum nesting depth honoured anywhere in this crate (parsing, printing, layout).
pub const MAX_DEPTH: usize = 48;

/// Maximum number of XML elements read from one OMML document; the rest is dropped.
pub const MAX_NODES: usize = 250_000;

/// A sequence of math nodes.
pub type Seq = Vec<Node>;

/// One equation (an `m:oMath`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Math {
    pub body: Seq,
    /// True when the source was an `m:oMathPara` (display equation); written back the same way.
    pub para: bool,
    /// Paragraph justification (`m:oMathParaPr/m:jc`), only meaningful with `para`.
    pub jc: Option<String>,
    /// Attributes of the (first) `m:oMath` element, as ` name="value"` text ready to write.
    #[serde(default)]
    pub attrs: String,
}

impl Math {
    pub fn new(body: Seq) -> Self {
        Math { body, para: false, jc: None, attrs: String::new() }
    }
    pub fn is_empty(&self) -> bool {
        self.body.iter().all(|n| matches!(n, Node::Text(t) if t.is_empty()))
    }
}

/// Run style of a [`Node::Styled`] text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Style {
    /// `m:sty p`: upright.
    Plain,
    Bold,
    /// `m:sty bi`.
    BoldItalic,
    /// `m:nor`: normal text, not math.
    Normal,
}

/// A node of the math tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Node {
    /// Plain run of characters (letters, digits, operators).
    Text(String),
    /// Fraction `num / den`.
    Frac { num: Seq, den: Seq },
    /// Radical; `deg` is the optional root degree.
    Rad { deg: Option<Seq>, body: Seq },
    /// Sub/superscript on `base`.
    Script { base: Seq, sub: Option<Seq>, sup: Option<Seq> },
    /// N-ary operator (sum, product, integral...) with optional limits.
    Nary { op: char, sub: Option<Seq>, sup: Option<Seq>, body: Seq },
    /// Delimiters around one or more items separated by `sep`.
    Delim { open: String, close: String, sep: String, items: Vec<Seq> },
    /// Matrix: rows of cells.
    Matrix { rows: Vec<Vec<Seq>> },
    /// Named function (sin, log...) applied to `body`.
    Func { name: String, body: Seq },
    /// Accent mark `ch` (a combining character) over `body`.
    Accent { ch: char, body: Seq },
    /// Text with an explicit run style (`m:sty` / `m:nor`).
    Styled { text: String, style: Style },
    /// `m:limLow` (`lower`) or `m:limUpp`: a limit expression under or over `base`.
    Limit { lower: bool, base: Seq, lim: Seq },
    /// `m:eqArr`: a vertical array of equation lines.
    EqArr { rows: Vec<Seq> },
    /// `m:sPre`: scripts placed before the base.
    PreScript { sub: Seq, sup: Seq, base: Seq },
    /// `m:bar`: a line over (`top`) or under the body.
    Bar { top: bool, body: Seq },
    /// `m:groupChr`: a stretchy grouping character (brace, arrow) over or under the body.
    GroupChr { ch: char, top: bool, body: Seq },
    /// `m:box` (`border` false) or `m:borderBox`.
    Boxed { border: bool, body: Seq },
    /// Unknown OMML element, kept verbatim so it survives a round trip.
    Raw(String),
    /// A node together with the OMML properties the tree does not model (`fPr/type`, `m:scr`,
    /// `m:lit`, `m:ctrlPr`, `w:rPr` fonts, `limLoc`, `grow`, `shp`, ...), re-emitted on write so a
    /// first edit does not rewrite the equation into a lossy form. Layout and linear text see
    /// straight through it.
    Props { extra: Extra, node: Box<Node> },
}

/// Unmodelled OMML properties of one element (see [`Node::Props`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extra {
    /// Children of the element's property container (`m:fPr`, `m:rPr`, ...): (name, verbatim XML).
    pub pr: Vec<(String, String)>,
    /// Run only: sibling elements other than `m:rPr` and `m:t` (for example `w:rPr`), verbatim.
    pub side: Vec<String>,
    /// Attributes of the element itself, as ` name="value"` text ready to write.
    #[serde(default)]
    pub attrs: String,
    /// Children of a structural element that the tree does not model, verbatim.
    #[serde(default)]
    pub tail: Vec<String>,
}

impl Extra {
    pub fn is_empty(&self) -> bool {
        self.pr.is_empty() && self.side.is_empty() && self.attrs.is_empty() && self.tail.is_empty()
    }
}

impl Node {
    /// The node with any [`Node::Props`] wrappers removed.
    pub fn bare(&self) -> &Node {
        let mut n = self;
        while let Node::Props { node, .. } = n {
            n = node;
        }
        n
    }
}

#[cfg(test)]
mod tests;
