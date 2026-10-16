//! Immutable formula source, closed cache and display baseline. The AST is rebuilt
//! from bounded source; native input never supplies an executable graph.
use std::fmt;

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, Visitor},
};

use super::{data::BoundedStringSeed, decimal::Decimal};
use crate::{ModelError, Result, TextBody, text::RunKind};

pub const MAX_FORMULA_SOURCE_BYTES: usize = 4096;
pub const MAX_FORMULA_NODES: usize = 256;
pub const MAX_FORMULA_DEPTH: usize = 32;
pub const FORMULA_PROFILE: &str = "exact-decimal-v1";
pub const FORMULA_PROFILE_V2: &str = "presentation-decimal-v2";
pub const FORMULA_PROFILE_V1_FORMATTED: &str = "exact-decimal-v1-formatted";
#[path = "table_formula_calendar.rs"]
pub(super) mod calendar;
#[path = "table_formula_v2.rs"]
pub(super) mod v2;
pub use calendar::{formula_date_from_serial, formula_datetime_from_serial};

pub(crate) fn invalid(message: &'static str) -> ModelError {
    ModelError::Invalid(message.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormulaError {
    InputNotNumeric,
    NumericLimit,
    DependencyError,
    DivisionByZero,
    InvalidReference,
    UnknownName,
    NotAvailable,
    Cycle,
}
impl FormulaError {
    pub fn display(self) -> &'static str {
        match self {
            Self::InputNotNumeric => "#VALUE!",
            Self::NumericLimit => "#NUM!",
            Self::DependencyError => "#DEPEND!",
            Self::DivisionByZero => "#DIV/0!",
            Self::InvalidReference => "#REF!",
            Self::UnknownName => "#NAME?",
            Self::NotAvailable => "#N/A",
            Self::Cycle => "#CYCLE!",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FormulaOutcome {
    Number { lexical: String },
    Error { code: FormulaError },
    Boolean { value: bool },
    String { value: String },
}
impl FormulaOutcome {
    pub fn numeric_lexical(&self) -> Option<&str> {
        match self {
            Self::Number { lexical } => Some(lexical),
            Self::Error { .. } | Self::Boolean { .. } | Self::String { .. } => None,
        }
    }
    pub(crate) fn value(&self) -> Result<std::result::Result<Decimal, FormulaError>> {
        match self {
            Self::Number { lexical } => Ok(Ok(Decimal::parse(lexical)?)),
            Self::Error { code } => Ok(Err(*code)),
            Self::Boolean { .. } | Self::String { .. } => Err(invalid("v1 formula requires numeric or error outcome")),
        }
    }
    pub(crate) fn equivalent(&self, other: &Self) -> Result<bool> {
        match (self, other) {
            (Self::Number { lexical: a }, Self::Number { lexical: b }) => Ok(Decimal::parse_v2(a)? == Decimal::parse_v2(b)?),
            _ => Ok(self == other),
        }
    }
    pub(crate) fn display_text(&self) -> Result<String> {
        self.plain_display_text()
    }
    pub fn plain_display_text(&self) -> Result<String> {
        match self {
            Self::Number { lexical } => Ok(Decimal::parse_v2(lexical)?.canonical()),
            Self::Error { code } => Ok(code.display().into()),
            Self::Boolean { value } => Ok(if *value { "TRUE" } else { "FALSE" }.into()),
            Self::String { value } => {
                if value.len() > 128 || value.chars().any(char::is_control) {
                    return Err(invalid("formula string exceeds display profile"));
                }
                Ok(value.clone())
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CellFormula {
    profile: String,
    expression: String,
    outcome: FormulaOutcome,
    display: String,
    #[serde(skip)]
    pub(crate) parsed: Parsed,
    #[serde(skip)]
    pub(crate) parsed_v2: Option<v2::Parsed>,
}

impl CellFormula {
    pub fn from_display(expression: String, outcome: FormulaOutcome, body: &TextBody) -> Result<Self> {
        let display = Self::display_text(body)?;
        Self::from_parts(expression, outcome, display)
    }
    pub fn number(expression: String, lexical: String, body: &TextBody) -> Result<Self> {
        Self::from_display(expression, FormulaOutcome::Number { lexical }, body)
    }
    fn from_parts(expression: String, outcome: FormulaOutcome, display: String) -> Result<Self> {
        let parsed = Parsed::parse(&expression)?;
        let legacy_broken_reference = parsed.nodes.iter().any(|node| matches!(node, Node::BrokenReference));
        if matches!(
            outcome,
            FormulaOutcome::Error {
                code: FormulaError::DivisionByZero | FormulaError::UnknownName | FormulaError::NotAvailable | FormulaError::Cycle
            }
        ) || (matches!(outcome, FormulaOutcome::Error { code: FormulaError::InvalidReference }) && !legacy_broken_reference)
        {
            return Err(invalid("v1 formula error code requires v2 profile or generated broken reference"));
        }
        match outcome.value()? {
            Ok(value) if Decimal::parse(&display)? != value => {
                return Err(invalid("formula numeric display differs from cached outcome"));
            }
            Err(code) if display != code.display() => {
                return Err(invalid("formula error display differs from cached outcome"));
            }
            _ => {}
        }
        Ok(Self { profile: FORMULA_PROFILE.into(), expression, outcome, display, parsed, parsed_v2: None })
    }
    pub fn profile(&self) -> &str {
        &self.profile
    }
    pub fn is_v2(&self) -> bool {
        self.parsed_v2.is_some()
    }
    pub fn uses_formatted_display(&self) -> bool {
        self.is_v2() || self.profile == FORMULA_PROFILE_V1_FORMATTED
    }
    pub fn with_number_format_display(&self, body: &TextBody) -> Result<Self> {
        let profile = if self.is_v2() { FORMULA_PROFILE_V2 } else { FORMULA_PROFILE_V1_FORMATTED };
        Self::from_profile_display(profile, self.expression.clone(), self.outcome.clone(), body)
    }
    pub fn from_profile_display(profile: &str, expression: String, outcome: FormulaOutcome, body: &TextBody) -> Result<Self> {
        let display = Self::display_text(body)?;
        match profile {
            FORMULA_PROFILE => Self::from_parts(expression, outcome, display),
            FORMULA_PROFILE_V1_FORMATTED => Self::from_legacy_formatted_parts(expression, outcome, display),
            FORMULA_PROFILE_V2 => Self::from_baseline_v2(expression, outcome, display),
            _ => Err(invalid("unknown formula profile")),
        }
    }
    fn from_legacy_formatted_parts(expression: String, outcome: FormulaOutcome, display: String) -> Result<Self> {
        let canonical = outcome.display_text()?;
        let mut formula = Self::from_parts(expression, outcome, canonical)?;
        formula.profile = FORMULA_PROFILE_V1_FORMATTED.into();
        formula.display = display;
        Ok(formula)
    }
    pub fn from_display_v2(expression: String, outcome: FormulaOutcome, body: &TextBody) -> Result<Self> {
        let display = Self::display_text(body)?;
        Self::from_parts_v2(expression, outcome, display)
    }
    fn from_parts_v2(expression: String, outcome: FormulaOutcome, display: String) -> Result<Self> {
        if outcome.display_text()? != display {
            return Err(invalid("v2 formula display differs from cached outcome"));
        }
        Self::from_baseline_v2(expression, outcome, display)
    }
    fn from_baseline_v2(expression: String, outcome: FormulaOutcome, display: String) -> Result<Self> {
        let parsed_v2 = v2::Parsed::parse(&expression)?;
        outcome.plain_display_text()?;
        if let FormulaOutcome::Number { lexical } = &outcome {
            Decimal::parse_v2(lexical)?;
        }
        Ok(Self {
            profile: FORMULA_PROFILE_V2.into(),
            expression,
            outcome,
            display,
            parsed: Parsed { nodes: Vec::new(), references: Vec::new() },
            parsed_v2: Some(parsed_v2),
        })
    }
    pub(crate) fn with_outcome(&self, outcome: FormulaOutcome, body: &TextBody) -> Result<Self> {
        Self::from_profile_display(&self.profile, self.expression.clone(), outcome, body)
    }
    pub(crate) fn node_count(&self) -> usize {
        self.parsed_v2.as_ref().map_or(self.parsed.nodes.len(), |p| p.nodes.len())
    }
    pub(crate) fn reference_count(&self) -> usize {
        self.parsed_v2.as_ref().map_or(self.parsed.references.len(), |p| p.references.len())
    }
    pub fn expression(&self) -> &str {
        &self.expression
    }
    pub fn outcome(&self) -> &FormulaOutcome {
        &self.outcome
    }
    pub fn display_baseline(&self) -> &str {
        &self.display
    }
    pub fn metadata_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(128)
            .saturating_add(self.profile.capacity())
            .saturating_add(self.expression.capacity())
            .saturating_add(self.display.capacity())
            .saturating_add(self.parsed.nodes.capacity().saturating_mul(std::mem::size_of::<Node>()))
            .saturating_add(self.parsed.references.capacity().saturating_mul(std::mem::size_of::<(usize, usize)>()))
            .saturating_add(self.parsed_v2.as_ref().map_or(0, v2::Parsed::metadata_bytes))
            .saturating_add(match &self.outcome {
                FormulaOutcome::Number { lexical } => lexical.capacity(),
                FormulaOutcome::String { value } => value.capacity(),
                FormulaOutcome::Error { .. } | FormulaOutcome::Boolean { .. } => 0,
            })
    }
    /// One paragraph with bounded ordinary runs; segmentation and styling are not authority.
    pub fn display_text(body: &TextBody) -> Result<String> {
        if !ordinary_display(body) {
            return Err(invalid("formula display requires one paragraph of bounded ordinary text runs"));
        }
        let paragraph = body.paragraphs.first().ok_or_else(|| invalid("formula paragraph missing"))?;
        let bytes = paragraph.runs.iter().map(|run| run.text.len()).sum();
        let mut text = String::new();
        text.try_reserve_exact(bytes).map_err(|_| invalid("cannot allocate formula display"))?;
        for run in &paragraph.runs {
            text.push_str(&run.text);
        }
        Ok(text)
    }
    pub fn matches_display(&self, body: &TextBody) -> Result<bool> {
        if !ordinary_display(body) {
            return Ok(false);
        }
        let mut offset = 0usize;
        for run in body.paragraphs.iter().flat_map(|paragraph| &paragraph.runs) {
            let end = offset.checked_add(run.text.len()).ok_or_else(|| invalid("formula display exceeds limit"))?;
            if self.display.as_bytes().get(offset..end) != Some(run.text.as_bytes()) {
                return Ok(false);
            }
            offset = end;
        }
        Ok(offset == self.display.len())
    }
}

pub(crate) fn ordinary_display(body: &TextBody) -> bool {
    if body.paragraphs.len() != 1 {
        return false;
    }
    let Some(paragraph) = body.paragraphs.first() else {
        return false;
    };
    if paragraph.runs.is_empty() || paragraph.runs.len() > 1024 {
        return false;
    }
    let mut bytes = 0usize;
    for run in &paragraph.runs {
        bytes = bytes.saturating_add(run.text.len());
        if run.kind != RunKind::Text || run.props.link.is_some() || run.props.hover.is_some() || bytes > 128 {
            return false;
        }
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellFormulaState<'a> {
    Absent,
    Current(&'a CellFormula),
    Stale,
}
impl super::Cell {
    pub fn formula_state(&self) -> Result<CellFormulaState<'_>> {
        if self.data.is_some() && self.formula.is_some() {
            return Err(invalid("cell cannot contain both constant data and formula"));
        }
        match self.formula.as_deref() {
            None => Ok(CellFormulaState::Absent),
            Some(formula) if formula.matches_display(&self.text)? => Ok(CellFormulaState::Current(formula)),
            Some(_) => Ok(CellFormulaState::Stale),
        }
    }
    pub fn reconcile_formula(&mut self) -> Result<bool> {
        if matches!(self.formula_state()?, CellFormulaState::Stale) {
            self.formula = None;
            return Ok(true);
        }
        Ok(false)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    Number(Decimal),
    Reference(usize, usize),
    BrokenReference,
    Positive(usize),
    Negative(usize),
    Add(usize, usize),
    Subtract(usize, usize),
    Multiply(usize, usize),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Parsed {
    pub nodes: Vec<Node>,
    pub references: Vec<(usize, usize)>,
}
impl Parsed {
    pub(crate) fn parse(source: &str) -> Result<Self> {
        if source.len() > MAX_FORMULA_SOURCE_BYTES || source.as_bytes().first() != Some(&b'=') || !source.is_ascii() {
            return Err(invalid("unsupported or oversized formula source"));
        }
        let mut parser = Parser { bytes: source.as_bytes(), at: 1, nodes: Vec::new(), depths: Vec::new(), references: Vec::new() };
        parser.nodes.try_reserve_exact(MAX_FORMULA_NODES).map_err(|_| invalid("cannot allocate formula AST"))?;
        parser.depths.try_reserve_exact(MAX_FORMULA_NODES).map_err(|_| invalid("cannot allocate formula depth scratch"))?;
        parser.references.try_reserve_exact(MAX_FORMULA_NODES).map_err(|_| invalid("cannot allocate formula references"))?;
        parser.expression(0)?;
        parser.space();
        if parser.at != parser.bytes.len() {
            return Err(invalid("unsupported formula token"));
        }
        parser.references.sort_unstable();
        parser.references.dedup();
        Ok(Self { nodes: parser.nodes, references: parser.references })
    }
}
struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
    nodes: Vec<Node>,
    depths: Vec<usize>,
    references: Vec<(usize, usize)>,
}
impl Parser<'_> {
    fn space(&mut self) {
        while self.bytes.get(self.at).is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n')) {
            self.at += 1;
        }
    }
    fn push(&mut self, node: Node) -> Result<usize> {
        let child = |index: usize| self.depths.get(index).copied().ok_or_else(|| invalid("invalid formula node"));
        let depth = match node {
            Node::Number(_) | Node::Reference(..) | Node::BrokenReference => 1,
            Node::Positive(a) | Node::Negative(a) => child(a)? + 1,
            Node::Add(a, b) | Node::Subtract(a, b) | Node::Multiply(a, b) => child(a)?.max(child(b)?) + 1,
        };
        if depth > MAX_FORMULA_DEPTH || self.nodes.len() >= MAX_FORMULA_NODES {
            return Err(invalid("formula AST exceeds node or depth limit"));
        }
        let index = self.nodes.len();
        self.nodes.push(node);
        self.depths.push(depth);
        Ok(index)
    }
    fn expression(&mut self, depth: usize) -> Result<usize> {
        let mut left = self.product(depth)?;
        loop {
            self.space();
            let operator = self.bytes.get(self.at).copied();
            if !matches!(operator, Some(b'+' | b'-')) {
                break;
            }
            self.at += 1;
            let right = self.product(depth)?;
            left = self.push(if operator == Some(b'+') { Node::Add(left, right) } else { Node::Subtract(left, right) })?;
        }
        Ok(left)
    }
    fn product(&mut self, depth: usize) -> Result<usize> {
        let mut left = self.unary(depth)?;
        loop {
            self.space();
            if self.bytes.get(self.at) != Some(&b'*') {
                break;
            }
            self.at += 1;
            let right = self.unary(depth)?;
            left = self.push(Node::Multiply(left, right))?;
        }
        Ok(left)
    }
    fn unary(&mut self, depth: usize) -> Result<usize> {
        if depth >= MAX_FORMULA_DEPTH {
            return Err(invalid("formula source nesting exceeds limit"));
        }
        self.space();
        match self.bytes.get(self.at).copied() {
            Some(b'+') => {
                self.at += 1;
                let node = self.unary(depth + 1)?;
                self.push(Node::Positive(node))
            }
            Some(b'-') => {
                self.at += 1;
                let node = self.unary(depth + 1)?;
                self.push(Node::Negative(node))
            }
            Some(b'(') => {
                self.at += 1;
                let node = self.expression(depth + 1)?;
                self.space();
                if self.bytes.get(self.at) != Some(&b')') {
                    return Err(invalid("formula has an unclosed parenthesis"));
                }
                self.at += 1;
                Ok(node)
            }
            Some(b'[') => self.reference(),
            Some(b'0'..=b'9' | b'.') => self.number(),
            _ => Err(invalid("unsupported formula operand")),
        }
    }
    fn reference(&mut self) -> Result<usize> {
        self.at += 1;
        if self.bytes.get(self.at..).is_some_and(|tail| tail.starts_with(b"#REF!]")) {
            self.at += 6;
            return self.push(Node::BrokenReference);
        }
        if self.bytes.get(self.at) != Some(&b'.') {
            return Err(invalid("formula requires same-table reference"));
        }
        self.at += 1;
        let start = self.at;
        let mut column = 0usize;
        while let Some(byte @ b'A'..=b'Z') = self.bytes.get(self.at).copied() {
            column = column
                .checked_mul(26)
                .and_then(|value| value.checked_add(usize::from(byte - b'A') + 1))
                .ok_or_else(|| invalid("formula column exceeds limit"))?;
            self.at += 1;
        }
        if self.at == start {
            return Err(invalid("invalid formula column"));
        }
        if !self.bytes.get(self.at).is_some_and(|byte| matches!(byte, b'1'..=b'9')) {
            return Err(invalid("invalid formula row"));
        }
        let mut row = 0usize;
        while let Some(byte) = self.bytes.get(self.at).copied().filter(u8::is_ascii_digit) {
            row = row
                .checked_mul(10)
                .and_then(|value| value.checked_add(usize::from(byte - b'0')))
                .ok_or_else(|| invalid("formula row exceeds limit"))?;
            self.at += 1;
        }
        if self.bytes.get(self.at) != Some(&b']') {
            return Err(invalid("invalid formula reference terminator"));
        }
        self.at += 1;
        let coordinate = (
            row.checked_sub(1).ok_or_else(|| invalid("invalid formula row"))?,
            column.checked_sub(1).ok_or_else(|| invalid("invalid formula column"))?,
        );
        if self.references.len() >= MAX_FORMULA_NODES {
            return Err(invalid("formula reference count exceeds limit"));
        }
        self.references.push(coordinate);
        self.push(Node::Reference(coordinate.0, coordinate.1))
    }
    fn number(&mut self) -> Result<usize> {
        let start = self.at;
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
        }
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            let fractional = self.at;
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
            }
            if fractional == self.at {
                return Err(invalid("formula decimal requires fractional digits"));
            }
        }
        if matches!(self.bytes.get(self.at), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            let exponent = self.at;
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
            }
            if exponent == self.at {
                return Err(invalid("formula exponent requires digits"));
            }
        }
        let token = self
            .bytes
            .get(start..self.at)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .ok_or_else(|| invalid("invalid formula numeric token"))?;
        self.push(Node::Number(Decimal::parse(token)?))
    }
}

// All maps reject unknown/duplicate keys before their values are read.
#[derive(Clone, Copy)]
enum Field {
    Profile,
    Expression,
    Outcome,
    Display,
    Kind,
    Lexical,
    Code,
    Value,
}
impl<'de> Deserialize<'de> for Field {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Key;
        impl Visitor<'_> for Key {
            type Value = Field;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a formula field")
            }
            fn visit_str<E: de::Error>(self, key: &str) -> std::result::Result<Field, E> {
                match key {
                    "profile" => Ok(Field::Profile),
                    "expression" => Ok(Field::Expression),
                    "outcome" => Ok(Field::Outcome),
                    "display" => Ok(Field::Display),
                    "kind" => Ok(Field::Kind),
                    "lexical" => Ok(Field::Lexical),
                    "code" => Ok(Field::Code),
                    "value" => Ok(Field::Value),
                    _ => Err(E::custom("unknown formula field")),
                }
            }
        }
        deserializer.deserialize_identifier(Key)
    }
}
impl<'de> Deserialize<'de> for FormulaOutcome {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Outcome;
        impl<'de> Visitor<'de> for Outcome {
            type Value = FormulaOutcome;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a strict formula outcome")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> std::result::Result<Self::Value, M::Error> {
                let (mut kind, mut lexical, mut code, mut value) = (None, None, None, None);
                while let Some(key) = map.next_key::<Field>()? {
                    match key {
                        Field::Kind if kind.is_none() => kind = Some(map.next_value_seed(BoundedStringSeed { max: 16 })?),
                        Field::Lexical if lexical.is_none() => lexical = Some(map.next_value_seed(BoundedStringSeed { max: 128 })?),
                        Field::Code if code.is_none() => code = Some(map.next_value::<FormulaError>()?),
                        Field::Value if value.is_none() => value = Some(map.next_value::<v2::OutcomeValue>()?),
                        _ => {
                            return Err(de::Error::custom("unknown or duplicate formula outcome field"));
                        }
                    }
                }
                match (kind.as_deref(), lexical, code, value) {
                    (Some("number"), Some(lexical), None, None) => {
                        Decimal::parse_v2(&lexical).map_err(de::Error::custom)?;
                        Ok(FormulaOutcome::Number { lexical })
                    }
                    (Some("error"), None, Some(code), None) => Ok(FormulaOutcome::Error { code }),
                    (Some("boolean"), None, None, Some(v2::OutcomeValue::Boolean(value))) => Ok(FormulaOutcome::Boolean { value }),
                    (Some("string"), None, None, Some(v2::OutcomeValue::String(value))) => Ok(FormulaOutcome::String { value }),
                    _ => Err(de::Error::custom("invalid formula outcome")),
                }
            }
        }
        deserializer.deserialize_map(Outcome)
    }
}
impl<'de> Deserialize<'de> for CellFormula {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Formula;
        impl<'de> Visitor<'de> for Formula {
            type Value = CellFormula;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a strict exact-decimal-v1 formula")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> std::result::Result<Self::Value, M::Error> {
                let (mut profile, mut expression, mut outcome, mut display) = (None, None, None, None);
                while let Some(key) = map.next_key::<Field>()? {
                    match key {
                        Field::Profile if profile.is_none() => profile = Some(map.next_value_seed(BoundedStringSeed { max: 32 })?),
                        Field::Expression if expression.is_none() => {
                            expression = Some(map.next_value_seed(BoundedStringSeed { max: MAX_FORMULA_SOURCE_BYTES })?)
                        }
                        Field::Outcome if outcome.is_none() => outcome = Some(map.next_value::<FormulaOutcome>()?),
                        Field::Display if display.is_none() => display = Some(map.next_value_seed(BoundedStringSeed { max: 128 })?),
                        _ => return Err(de::Error::custom("unknown or duplicate formula field")),
                    }
                }
                if !matches!(profile.as_deref(), Some(FORMULA_PROFILE | FORMULA_PROFILE_V2 | FORMULA_PROFILE_V1_FORMATTED)) {
                    return Err(de::Error::custom("unknown or missing formula profile"));
                }
                let expression = expression.ok_or_else(|| de::Error::missing_field("expression"))?;
                let outcome = outcome.ok_or_else(|| de::Error::missing_field("outcome"))?;
                let display = display.ok_or_else(|| de::Error::missing_field("display"))?;
                if profile.as_deref() == Some(FORMULA_PROFILE_V2) {
                    CellFormula::from_baseline_v2(expression, outcome, display)
                } else if profile.as_deref() == Some(FORMULA_PROFILE_V1_FORMATTED) {
                    CellFormula::from_legacy_formatted_parts(expression, outcome, display)
                } else {
                    CellFormula::from_parts(expression, outcome, display)
                }
                .map_err(de::Error::custom)
            }
        }
        deserializer.deserialize_map(Formula)
    }
}
