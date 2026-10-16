//! Bounded canonical presentation formula grammar, separate from the frozen v1 parser.
use super::{Decimal, FormulaError, MAX_FORMULA_DEPTH, MAX_FORMULA_NODES, MAX_FORMULA_SOURCE_BYTES, invalid};
use crate::Result;
use serde::{
    Deserialize, Deserializer,
    de::{self, Visitor},
};
use std::fmt;

pub(super) const MAX_STRING: usize = 128;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OutcomeValue {
    Boolean(bool),
    String(String),
}
impl<'de> Deserialize<'de> for OutcomeValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Value;
        impl Visitor<'_> for Value {
            type Value = OutcomeValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a Boolean or bounded formula string")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> std::result::Result<Self::Value, E> {
                Ok(OutcomeValue::Boolean(value))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                if value.len() > MAX_STRING || value.chars().any(char::is_control) {
                    return Err(E::custom("formula string exceeds display profile"));
                }
                Ok(OutcomeValue::String(value.into()))
            }
        }
        deserializer.deserialize_any(Value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Address {
    pub row: usize,
    pub col: usize,
    pub row_absolute: bool,
    pub col_absolute: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Area {
    pub first: Address,
    pub last: Address,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Reference {
    pub area: Area,
    pub start: usize,
    pub end: usize,
    pub range: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Binary {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Concat,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    Number(Decimal),
    Text(String),
    Error(FormulaError),
    Reference(Area),
    Positive(usize),
    Negative(usize),
    Percent(usize),
    Binary(Binary, usize, usize),
    Call(String, Vec<usize>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Parsed {
    pub nodes: Vec<Node>,
    pub references: Vec<Reference>,
    pub root: usize,
}
impl Parsed {
    pub fn metadata_bytes(&self) -> usize {
        self.nodes
            .capacity()
            .saturating_mul(std::mem::size_of::<Node>())
            .saturating_add(self.references.capacity().saturating_mul(std::mem::size_of::<Reference>()))
            .saturating_add(
                self.nodes
                    .iter()
                    .map(|n| match n {
                        Node::Text(s) => s.capacity(),
                        Node::Call(s, a) => s.capacity().saturating_add(a.capacity().saturating_mul(std::mem::size_of::<usize>())),
                        _ => 0,
                    })
                    .sum::<usize>(),
            )
    }
    pub fn parse(source: &str) -> Result<Self> {
        if source.len() > MAX_FORMULA_SOURCE_BYTES || source.as_bytes().first() != Some(&b'=') {
            return Err(invalid("unsupported or oversized v2 formula source"));
        }
        let mut parser = Parser { source, at: 1, nodes: Vec::new(), references: Vec::new(), depths: Vec::new() };
        parser.nodes.try_reserve_exact(MAX_FORMULA_NODES).map_err(|_| invalid("cannot allocate v2 formula AST"))?;
        parser.depths.try_reserve_exact(MAX_FORMULA_NODES).map_err(|_| invalid("cannot allocate v2 formula depth scratch"))?;
        parser.references.try_reserve_exact(MAX_FORMULA_NODES).map_err(|_| invalid("cannot allocate v2 formula references"))?;
        let root = parser.expression(0, 0)?;
        parser.space();
        if parser.at != source.len() {
            return Err(invalid("unsupported v2 formula token"));
        }
        Ok(Self { nodes: parser.nodes, references: parser.references, root })
    }
}
struct Parser<'a> {
    source: &'a str,
    at: usize,
    nodes: Vec<Node>,
    references: Vec<Reference>,
    depths: Vec<usize>,
}
impl Parser<'_> {
    fn byte(&self) -> Option<u8> {
        self.source.as_bytes().get(self.at).copied()
    }
    fn space(&mut self) {
        while matches!(self.byte(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }
    fn push(&mut self, node: Node) -> Result<usize> {
        let depth = |i: usize| self.depths.get(i).copied().ok_or_else(|| invalid("invalid v2 formula AST child"));
        let child_depth = match &node {
            Node::Positive(i) | Node::Negative(i) | Node::Percent(i) => depth(*i)?,
            Node::Binary(_, a, b) => depth(*a)?.max(depth(*b)?),
            Node::Call(_, args) => args.iter().try_fold(0usize, |d, i| Ok::<_, crate::ModelError>(d.max(depth(*i)?)))?,
            _ => 0,
        };
        if child_depth >= MAX_FORMULA_DEPTH || self.nodes.len() >= MAX_FORMULA_NODES {
            return Err(invalid("v2 formula AST exceeds node or depth limit"));
        }
        let index = self.nodes.len();
        self.nodes.push(node);
        self.depths.push(child_depth + 1);
        Ok(index)
    }
    fn expression(&mut self, minimum: u8, depth: usize) -> Result<usize> {
        if depth >= MAX_FORMULA_DEPTH {
            return Err(invalid("v2 formula source nesting exceeds limit"));
        }
        self.space();
        let mut left = self.atom(depth + 1)?;
        loop {
            self.space();
            if self.byte() == Some(b'%') && minimum <= 60 {
                self.at += 1;
                left = self.push(Node::Percent(left))?;
                continue;
            }
            let tail = self.source.get(self.at..).ok_or_else(|| invalid("invalid formula byte boundary"))?;
            let op = [
                ("<=", 10, Binary::LessEqual),
                (">=", 10, Binary::GreaterEqual),
                ("<>", 10, Binary::NotEqual),
                ("=", 10, Binary::Equal),
                ("<", 10, Binary::Less),
                (">", 10, Binary::Greater),
                ("&", 20, Binary::Concat),
                ("+", 30, Binary::Add),
                ("-", 30, Binary::Sub),
                ("*", 40, Binary::Mul),
                ("/", 40, Binary::Div),
                ("^", 50, Binary::Pow),
            ]
            .into_iter()
            .find(|(token, _, _)| tail.starts_with(token));
            let Some((token, precedence, operator)) = op else {
                break;
            };
            if precedence < minimum {
                break;
            }
            self.at += token.len();
            let right = self.expression(precedence + 1, depth + 1)?;
            left = self.push(Node::Binary(operator, left, right))?;
        }
        Ok(left)
    }
    fn atom(&mut self, depth: usize) -> Result<usize> {
        self.space();
        match self.byte() {
            Some(b'+') => {
                self.at += 1;
                let i = self.expression(70, depth)?;
                self.push(Node::Positive(i))
            }
            Some(b'-') => {
                self.at += 1;
                let i = self.expression(70, depth)?;
                self.push(Node::Negative(i))
            }
            Some(b'(') => {
                self.at += 1;
                let i = self.expression(0, depth)?;
                self.space();
                if self.byte() != Some(b')') {
                    return Err(invalid("unclosed v2 formula parenthesis"));
                }
                self.at += 1;
                Ok(i)
            }
            Some(b'"') => self.text(),
            Some(b'[') => self.reference(),
            Some(b'#') => self.error(),
            Some(b'0'..=b'9' | b'.') => self.number(),
            Some(b'A'..=b'Z' | b'a'..=b'z' | b'_') => self.call(depth),
            _ => Err(invalid("unsupported v2 formula operand")),
        }
    }
    fn text(&mut self) -> Result<usize> {
        self.at += 1;
        let mut text = String::new();
        text.try_reserve_exact(MAX_STRING).map_err(|_| invalid("cannot allocate formula string"))?;
        loop {
            let tail = self.source.get(self.at..).ok_or_else(|| invalid("invalid formula string boundary"))?;
            let c = tail.chars().next().ok_or_else(|| invalid("unterminated formula string"))?;
            self.at += c.len_utf8();
            if c == '"' {
                if self.byte() == Some(b'"') {
                    self.at += 1;
                } else {
                    break;
                }
            }
            if c.is_control() || text.len().checked_add(c.len_utf8()).is_none_or(|n| n > MAX_STRING) {
                return Err(invalid("formula string exceeds display profile"));
            }
            text.push(c);
        }
        self.push(Node::Text(text))
    }
    fn number(&mut self) -> Result<usize> {
        let start = self.at;
        while self.byte().is_some_and(|b| b.is_ascii_digit()) {
            self.at += 1;
        }
        if self.byte() == Some(b'.') {
            self.at += 1;
            let fractional = self.at;
            while self.byte().is_some_and(|b| b.is_ascii_digit()) {
                self.at += 1;
            }
            if fractional == self.at {
                return Err(invalid("decimal requires fractional digits"));
            }
        }
        if matches!(self.byte(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.byte(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            let exponent = self.at;
            while self.byte().is_some_and(|b| b.is_ascii_digit()) {
                self.at += 1;
            }
            if exponent == self.at {
                return Err(invalid("exponent requires digits"));
            }
        }
        let value = self.source.get(start..self.at).ok_or_else(|| invalid("invalid numeric span"))?;
        self.push(Node::Number(Decimal::parse_v2(value)?))
    }
    fn call(&mut self, depth: usize) -> Result<usize> {
        let start = self.at;
        while self.byte().is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.')) {
            self.at += 1;
        }
        let name = self
            .source
            .get(start..self.at)
            .filter(|s| s.len() <= 64)
            .ok_or_else(|| invalid("formula function name exceeds limit"))?
            .to_ascii_uppercase();
        self.space();
        if self.byte() != Some(b'(') {
            return Err(invalid("formula function requires parentheses"));
        }
        self.at += 1;
        self.space();
        let mut args = Vec::new();
        args.try_reserve_exact(32).map_err(|_| invalid("cannot allocate formula arguments"))?;
        if self.byte() != Some(b')') {
            loop {
                if args.len() >= 32 {
                    return Err(invalid("formula argument count exceeds limit"));
                }
                args.push(self.expression(0, depth)?);
                self.space();
                if self.byte() != Some(b';') {
                    break;
                }
                self.at += 1;
            }
        }
        if self.byte() != Some(b')') {
            return Err(invalid("unclosed function arguments"));
        }
        self.at += 1;
        self.push(Node::Call(name, args))
    }
    fn address(&mut self) -> Result<Address> {
        if self.byte() != Some(b'.') {
            return Err(invalid("v2 formula requires same-table reference"));
        }
        self.at += 1;
        let col_absolute = self.byte() == Some(b'$');
        if col_absolute {
            self.at += 1;
        }
        let start = self.at;
        let mut col = 0usize;
        while let Some(b @ b'A'..=b'Z') = self.byte() {
            col = col.checked_mul(26).and_then(|n| n.checked_add(usize::from(b - b'A') + 1)).ok_or_else(|| invalid("formula column overflow"))?;
            self.at += 1;
        }
        if start == self.at {
            return Err(invalid("formula column missing"));
        }
        let row_absolute = self.byte() == Some(b'$');
        if row_absolute {
            self.at += 1;
        }
        if !matches!(self.byte(), Some(b'1'..=b'9')) {
            return Err(invalid("formula row missing"));
        }
        let mut row = 0usize;
        while let Some(b) = self.byte().filter(u8::is_ascii_digit) {
            row = row.checked_mul(10).and_then(|n| n.checked_add(usize::from(b - b'0'))).ok_or_else(|| invalid("formula row overflow"))?;
            self.at += 1;
        }
        Ok(Address {
            row: row.checked_sub(1).ok_or_else(|| invalid("invalid formula row"))?,
            col: col.checked_sub(1).ok_or_else(|| invalid("invalid formula column"))?,
            row_absolute,
            col_absolute,
        })
    }
    fn reference(&mut self) -> Result<usize> {
        let start = self.at;
        self.at += 1;
        if self.source.get(self.at..).is_some_and(|s| s.starts_with("#REF!]")) {
            self.at += 6;
            return self.push(Node::Error(FormulaError::InvalidReference));
        }
        let first = self.address()?;
        let range = self.byte() == Some(b':');
        let last = if range {
            self.at += 1;
            self.address()?
        } else {
            first
        };
        if self.byte() != Some(b']') {
            return Err(invalid("formula reference terminator missing"));
        }
        self.at += 1;
        let area = Area { first, last };
        self.references.push(Reference { area, start, end: self.at, range });
        self.push(Node::Reference(area))
    }
    fn error(&mut self) -> Result<usize> {
        let tail = self.source.get(self.at..).ok_or_else(|| invalid("invalid error span"))?;
        for (token, error) in [
            ("#DIV/0!", FormulaError::DivisionByZero),
            ("#VALUE!", FormulaError::InputNotNumeric),
            ("#NUM!", FormulaError::NumericLimit),
            ("#REF!", FormulaError::InvalidReference),
            ("#NAME?", FormulaError::UnknownName),
            ("#N/A", FormulaError::NotAvailable),
            ("#CYCLE!", FormulaError::Cycle),
            ("#DEPEND!", FormulaError::DependencyError),
        ] {
            if tail.starts_with(token) {
                self.at += token.len();
                return self.push(Node::Error(error));
            }
        }
        Err(invalid("unknown formula error constant"))
    }
}

fn address_text(address: Address) -> Result<String> {
    let mut column = address.col.checked_add(1).ok_or_else(|| invalid("reference column overflow"))?;
    let mut letters = Vec::new();
    letters.try_reserve_exact(32).map_err(|_| invalid("cannot allocate reference column"))?;
    while column != 0 {
        letters.push(b'A' + ((column - 1) % 26) as u8);
        column = (column - 1) / 26;
    }
    let mut out = String::from(".");
    if address.col_absolute {
        out.push('$');
    }
    for byte in letters.into_iter().rev() {
        out.push(char::from(byte));
    }
    if address.row_absolute {
        out.push('$');
    }
    out.push_str(&address.row.checked_add(1).ok_or_else(|| invalid("reference row overflow"))?.to_string());
    Ok(out)
}
impl super::CellFormula {
    /// Pure source rewrite for a subsequent atomic topology transaction. Absolute markers
    /// follow structural edits; source spelling outside reference tokens stays unchanged.
    pub fn rewritten_axis(&self, columns: bool, at: usize, count: usize, delete: bool) -> Result<String> {
        if count == 0 {
            return Ok(self.expression.clone());
        }
        let end = at.checked_add(count).ok_or_else(|| invalid("reference edit range overflow"))?;
        self.rewrite(|reference| {
            let mut area = reference.area;
            let (a, b) = if columns { (&mut area.first.col, &mut area.last.col) } else { (&mut area.first.row, &mut area.last.row) };
            if !delete {
                if *a >= at {
                    *a = a.checked_add(count).ok_or_else(|| invalid("rewritten reference overflow"))?;
                }
                if *b >= at {
                    *b = b.checked_add(count).ok_or_else(|| invalid("rewritten reference overflow"))?;
                }
            } else if !reference.range {
                if *a >= at && *a < end {
                    return Ok(None);
                }
                if *a >= end {
                    *a -= count;
                }
                *b = *a;
            } else {
                let reverse = *a > *b;
                let low = (*a).min(*b);
                let high = (*a).max(*b);
                let first = if low >= at && low < end { end } else { low };
                let last = if high >= at && high < end { at.saturating_sub(1) } else { high };
                if first > last || (low >= at && high < end) {
                    return Ok(None);
                }
                let first = if first >= end { first - count } else { first };
                let last = if last >= end { last - count } else { last };
                if reverse {
                    *a = last;
                    *b = first;
                } else {
                    *a = first;
                    *b = last;
                }
            }
            Ok(Some(area))
        })
    }
    pub fn relocated_v2(&self, row_delta: i64, column_delta: i64) -> Result<String> {
        self.rewrite(|reference| {
            let mut area = reference.area;
            for address in [&mut area.first, &mut area.last] {
                let shifted = |value: usize, delta: i64| -> Option<usize> {
                    if delta >= 0 {
                        value.checked_add(usize::try_from(delta).ok()?)
                    } else {
                        value.checked_sub(usize::try_from(delta.unsigned_abs()).ok()?)
                    }
                };
                if !address.row_absolute {
                    let Some(row) = shifted(address.row, row_delta) else {
                        return Ok(None);
                    };
                    address.row = row;
                }
                if !address.col_absolute {
                    let Some(col) = shifted(address.col, column_delta) else {
                        return Ok(None);
                    };
                    address.col = col;
                }
            }
            Ok(Some(area))
        })
    }
    fn rewrite(&self, mut edit: impl FnMut(&Reference) -> Result<Option<Area>>) -> Result<String> {
        let legacy_parsed;
        let parsed = match self.parsed_v2.as_ref() {
            Some(parsed) => parsed,
            None => {
                legacy_parsed = Parsed::parse(&self.expression)?;
                &legacy_parsed
            }
        };
        let mut output = String::new();
        output.try_reserve_exact(MAX_FORMULA_SOURCE_BYTES).map_err(|_| invalid("cannot allocate rewritten formula"))?;
        let mut copied = 0;
        for reference in &parsed.references {
            let prefix = self.expression.get(copied..reference.start).ok_or_else(|| invalid("invalid reference source span"))?;
            output.push_str(prefix);
            match edit(reference)? {
                None => output.push_str("[#REF!]"),
                Some(area) => {
                    output.push('[');
                    output.push_str(&address_text(area.first)?);
                    if reference.range {
                        output.push(':');
                        output.push_str(&address_text(area.last)?);
                    }
                    output.push(']');
                }
            }
            if output.len() > MAX_FORMULA_SOURCE_BYTES {
                return Err(invalid("rewritten formula exceeds source limit"));
            }
            copied = reference.end;
        }
        output.push_str(self.expression.get(copied..).ok_or_else(|| invalid("invalid rewritten formula suffix"))?);
        Parsed::parse(&output)?;
        Ok(output)
    }
}
