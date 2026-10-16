//! Immutable bounded table constants and exact cached-display consistency.
//! Attachment/import boundaries must additionally enforce cumulative metadata budgets.

use std::{fmt, mem::size_of};

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};

use crate::{
    ModelError, Result,
    text::{RunKind, TextBody},
};

pub const MAX_CELL_STRING_BYTES: usize = 64 * 1024;
pub const MAX_CURRENCY_BYTES: usize = 64;
pub const MAX_NUMBER_BYTES: usize = 128;
pub const MAX_TEMPORAL_BYTES: usize = 256;
pub const MAX_DISPLAY_BYTES: usize = 64 * 1024;
pub const MAX_DISPLAY_PARAGRAPHS: usize = 1024;
pub const MAX_DISPLAY_RUNS: usize = 100_000;

fn invalid(message: &'static str) -> ModelError {
    ModelError::Invalid(message.into())
}

/// The authoritative scalar, independently of its rich displayed text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CellValue {
    String {
        value: String,
    },
    Boolean {
        value: bool,
    },
    Number {
        #[serde(rename = "numberKind")]
        kind: CellNumberKind,
        lexical: String,
    },
    /// Exact supported XML Schema date or dateTime lexical, independent of display.
    Date {
        lexical: String,
    },
    /// Exact XML Schema duration lexical; years/months are never converted to seconds.
    Duration {
        lexical: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CellNumberKind {
    Float,
    Percentage,
    Currency {
        #[serde(skip_serializing_if = "Option::is_none")]
        currency: Option<String>,
    },
}

fn xml_attribute_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}

impl CellNumberKind {
    pub fn validate(&self) -> Result<()> {
        if let Self::Currency { currency: Some(currency) } = self {
            if currency.len() > MAX_CURRENCY_BYTES {
                return Err(invalid("cell currency exceeds size limit"));
            }
            if !currency.chars().all(xml_attribute_char) {
                return Err(invalid("cell currency contains invalid XML characters"));
            }
        }
        Ok(())
    }
}

impl CellValue {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::String { value } => {
                if value.len() > MAX_CELL_STRING_BYTES {
                    return Err(invalid("cell string exceeds size limit"));
                }
                if !value.chars().all(xml_attribute_char) {
                    return Err(invalid("cell string contains invalid XML characters"));
                }
                Ok(())
            }
            Self::Number { kind, lexical } => {
                if !valid_number(lexical) {
                    return Err(invalid("invalid or unsupported cell numeric literal"));
                }
                kind.validate()
            }
            Self::Date { lexical } => {
                if super::temporal::valid_date(lexical) {
                    Ok(())
                } else {
                    Err(invalid("invalid or unsupported cell date literal"))
                }
            }
            Self::Duration { lexical } => {
                if super::temporal::valid_duration(lexical) {
                    Ok(())
                } else {
                    Err(invalid("invalid or unsupported cell duration literal"))
                }
            }
            Self::Boolean { .. } => Ok(()),
        }
    }

    fn allocation_bytes(&self) -> usize {
        let allocated = |s: &String| {
            if s.capacity() == 0 { 0 } else { s.capacity().saturating_add(32) }
        };
        match self {
            Self::String { value } => allocated(value),
            Self::Boolean { .. } => 0,
            Self::Date { lexical } | Self::Duration { lexical } => allocated(lexical),
            Self::Number { kind, lexical } => allocated(lexical).saturating_add(match kind {
                CellNumberKind::Currency { currency: Some(currency) } => allocated(currency),
                _ => 0,
            }),
        }
    }
}

/// Strict decimal/exponent grammar. The token, not its f64 approximation, remains authoritative.
fn valid_number(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_NUMBER_BYTES || !s.is_ascii() {
        return false;
    }
    let mut i = 0usize;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let mut digits = 0usize;
    let mut nonzero = false;
    while let Some(digit) = bytes.get(i).copied().filter(u8::is_ascii_digit) {
        digits += 1;
        nonzero |= digit != b'0';
        i += 1;
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        while let Some(digit) = bytes.get(i).copied().filter(u8::is_ascii_digit) {
            digits += 1;
            nonzero |= digit != b'0';
            i += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    if i != bytes.len() {
        return false;
    }
    s.parse::<f64>().is_ok_and(|value| value.is_finite() && !(value == 0.0 && nonzero))
}

/// Private immutable baseline. No public setter can rebind a value after editing its display.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CellConstant {
    value: CellValue,
    display: Vec<String>,
}

impl CellConstant {
    /// Validate the whole display before allocating the baseline.
    pub fn from_display(value: CellValue, body: &TextBody) -> Result<Self> {
        Self::metadata_bytes_for_display(&value, body)?;
        let mut display = Vec::new();
        display.try_reserve_exact(body.paragraphs.len().max(1)).map_err(|_| invalid("cannot allocate cell constant display"))?;
        for paragraph in &body.paragraphs {
            // supported_display has already bounded all lengths and checked every kind.
            let bytes = paragraph.runs.iter().fold(0usize, |sum, run| sum.saturating_add(run.content().len()));
            let mut text = String::new();
            text.try_reserve_exact(bytes).map_err(|_| invalid("cannot allocate cell constant display"))?;
            for run in &paragraph.runs {
                text.push_str(run.content());
            }
            display.push(text);
        }
        if display.is_empty() {
            display.push(String::new());
        }
        Ok(Self { value, display })
    }

    /// Conservative charge before capturing a display baseline. Repeated imports must
    /// multiply this by their attachment count before constructing or cloning records.
    pub fn metadata_bytes_for_display(value: &CellValue, body: &TextBody) -> Result<usize> {
        value.validate()?;
        if !supported_display(body) {
            return Err(invalid("cell constant display is unsupported or exceeds size limits"));
        }
        let mut bytes = size_of::<Self>()
            .saturating_add(64)
            .saturating_add(body.paragraphs.len().max(1).saturating_mul(size_of::<String>()))
            .saturating_add(32)
            .saturating_add(value.allocation_bytes());
        for paragraph in &body.paragraphs {
            let length = paragraph.runs.iter().fold(0usize, |sum, run| sum.saturating_add(run.content().len()));
            if length > 0 {
                bytes = bytes.saturating_add(length).saturating_add(32);
            }
        }
        Ok(bytes)
    }

    pub fn value(&self) -> &CellValue {
        &self.value
    }
    pub fn display_baseline(&self) -> &[String] {
        &self.display
    }

    fn validate_metadata(&self) -> Result<()> {
        self.value.validate()?;
        if self.display.is_empty() || self.display.len() > MAX_DISPLAY_PARAGRAPHS {
            return Err(invalid("invalid cell constant display paragraph count"));
        }
        let mut bytes = 0usize;
        for text in &self.display {
            bytes = bytes.saturating_add(text.len());
            if bytes > MAX_DISPLAY_BYTES {
                return Err(invalid("cell constant display exceeds size limit"));
            }
        }
        Ok(())
    }

    /// Conservative owned allocation charge, including Arc/allocator/vector overhead.
    /// Callers must apply cumulative budgets before repeated cell cloning or attachment.
    pub fn metadata_bytes(&self) -> usize {
        let strings = self
            .display
            .iter()
            .fold(0usize, |sum, text| sum.saturating_add(if text.capacity() == 0 { 0 } else { text.capacity().saturating_add(32) }));
        size_of::<Self>()
            .saturating_add(64)
            .saturating_add(self.display.capacity().saturating_mul(size_of::<String>()))
            .saturating_add(32)
            .saturating_add(strings)
            .saturating_add(self.value.allocation_bytes())
    }

    /// Exact comparison without creating another display snapshot.
    /// Unsupported/newly oversized current text is stale; malformed metadata is an error.
    pub fn matches_display(&self, body: &TextBody) -> Result<bool> {
        self.validate_metadata()?;
        if !supported_display(body) {
            return Ok(false);
        }
        if self.display.len() != body.paragraphs.len().max(1) {
            return Ok(false);
        }
        if body.paragraphs.is_empty() {
            return Ok(self.display.first().is_some_and(String::is_empty));
        }
        for (index, paragraph) in body.paragraphs.iter().enumerate() {
            let Some(expected) = self.display.get(index) else {
                return Ok(false);
            };
            let mut offset = 0usize;
            for run in &paragraph.runs {
                let content = run.content();
                let Some(end) = offset.checked_add(content.len()) else {
                    return Ok(false);
                };
                if expected.as_bytes().get(offset..end) != Some(content.as_bytes()) {
                    return Ok(false);
                }
                offset = end;
            }
            if offset != expected.len() {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Reject oversized paragraph/run collections before walking their contents.
fn supported_display(body: &TextBody) -> bool {
    if body.paragraphs.len() > MAX_DISPLAY_PARAGRAPHS {
        return false;
    }
    let mut runs = 0usize;
    let mut bytes = 0usize;
    for paragraph in &body.paragraphs {
        runs = runs.saturating_add(paragraph.runs.len());
        if runs > MAX_DISPLAY_RUNS {
            return false;
        }
        for run in &paragraph.runs {
            if !matches!(run.kind, RunKind::Text | RunKind::Break) {
                return false;
            }
            bytes = bytes.saturating_add(run.content().len());
            if bytes > MAX_DISPLAY_BYTES {
                return false;
            }
        }
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellConstantState<'a> {
    Absent,
    Current(&'a CellConstant),
    Stale,
}

impl super::Cell {
    pub fn constant_state(&self) -> Result<CellConstantState<'_>> {
        if self.data.is_some() && self.formula.is_some() {
            return Err(invalid("cell cannot contain both constant data and formula"));
        }
        match self.data.as_deref() {
            None => Ok(CellConstantState::Absent),
            Some(data) if data.matches_display(&self.text)? => Ok(CellConstantState::Current(data)),
            Some(_) => Ok(CellConstantState::Stale),
        }
    }

    pub fn reconcile_constant(&mut self) -> Result<bool> {
        let formula_changed = self.reconcile_formula()?;
        if matches!(self.constant_state()?, CellConstantState::Stale) {
            self.data = None;
            return Ok(true);
        }
        Ok(formula_changed)
    }

    /// The caller owns rollback. If T is itself a Result, handle both result layers.
    pub fn edit_display<T>(&mut self, edit: impl FnOnce(&mut TextBody) -> T) -> Result<T> {
        let result = edit(&mut self.text);
        self.reconcile_constant()?;
        Ok(result)
    }
}

// Custom visitors intentionally avoid serde's internally-tagged buffering and derived Vec.
// Unknown fields/duplicates are rejected before decoding their values.
#[derive(Clone, Copy)]
enum Field {
    Kind,
    Value,
    NumberKind,
    Lexical,
    Currency,
    Display,
}
impl<'de> Deserialize<'de> for Field {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct KeyVisitor;
        impl Visitor<'_> for KeyVisitor {
            type Value = Field;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a cell constant field")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Field, E> {
                match value {
                    "kind" => Ok(Field::Kind),
                    "value" => Ok(Field::Value),
                    "numberKind" => Ok(Field::NumberKind),
                    "lexical" => Ok(Field::Lexical),
                    "currency" => Ok(Field::Currency),
                    "display" => Ok(Field::Display),
                    _ => Err(E::custom("unknown cell constant field")),
                }
            }
        }
        deserializer.deserialize_identifier(KeyVisitor)
    }
}

#[derive(Clone, Copy)]
pub(super) struct BoundedStringSeed {
    pub(super) max: usize,
}
impl<'de> DeserializeSeed<'de> for BoundedStringSeed {
    type Value = String;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> std::result::Result<String, D::Error> {
        struct StringVisitor {
            max: usize,
        }
        impl Visitor<'_> for StringVisitor {
            type Value = String;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded string")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<String, E> {
                if value.len() > self.max {
                    return Err(E::custom("cell constant string exceeds size limit"));
                }
                let mut owned = String::new();
                owned.try_reserve_exact(value.len()).map_err(|_| E::custom("cannot allocate cell constant string"))?;
                owned.push_str(value);
                Ok(owned)
            }
            fn visit_borrowed_str<E: de::Error>(self, value: &str) -> std::result::Result<String, E> {
                self.visit_str(value)
            }
            fn visit_string<E: de::Error>(self, value: String) -> std::result::Result<String, E> {
                if value.len() > self.max { Err(E::custom("cell constant string exceeds size limit")) } else { Ok(value) }
            }
        }
        deserializer.deserialize_str(StringVisitor { max: self.max })
    }
}

struct OptionalCurrencySeed;
impl<'de> DeserializeSeed<'de> for OptionalCurrencySeed {
    type Value = Option<String>;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error> {
        struct OptionalVisitor;
        impl<'de> Visitor<'de> for OptionalVisitor {
            type Value = Option<String>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an optional bounded currency string")
            }
            fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(None)
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(None)
            }
            fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error> {
                BoundedStringSeed { max: MAX_CURRENCY_BYTES }.deserialize(deserializer).map(Some)
            }
        }
        deserializer.deserialize_option(OptionalVisitor)
    }
}

enum Tag {
    String,
    Boolean,
    Number,
    Date,
    Duration,
    Float,
    Percentage,
    Currency,
}
impl<'de> Deserialize<'de> for Tag {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct TagVisitor;
        impl Visitor<'_> for TagVisitor {
            type Value = Tag;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a cell value kind")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Tag, E> {
                match value {
                    "string" => Ok(Tag::String),
                    "boolean" => Ok(Tag::Boolean),
                    "number" => Ok(Tag::Number),
                    "date" => Ok(Tag::Date),
                    "duration" => Ok(Tag::Duration),
                    "float" => Ok(Tag::Float),
                    "percentage" => Ok(Tag::Percentage),
                    "currency" => Ok(Tag::Currency),
                    _ => Err(E::custom("unknown cell value kind")),
                }
            }
        }
        deserializer.deserialize_str(TagVisitor)
    }
}

impl<'de> Deserialize<'de> for CellNumberKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct KindVisitor;
        impl<'de> Visitor<'de> for KindVisitor {
            type Value = CellNumberKind;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a cell number kind object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Self::Value, A::Error> {
                let mut kind = None;
                let mut currency = None;
                while let Some(field) = map.next_key::<Field>()? {
                    match field {
                        Field::Kind if kind.is_none() => kind = Some(map.next_value::<Tag>()?),
                        Field::Currency if currency.is_none() => currency = Some(map.next_value_seed(OptionalCurrencySeed)?),
                        Field::Kind | Field::Currency => {
                            return Err(de::Error::custom("duplicate cell number kind field"));
                        }
                        _ => return Err(de::Error::custom("unexpected cell number kind field")),
                    }
                }
                let out = match (kind, currency) {
                    (Some(Tag::Float), None) => CellNumberKind::Float,
                    (Some(Tag::Percentage), None) => CellNumberKind::Percentage,
                    (Some(Tag::Currency), currency) => CellNumberKind::Currency { currency: currency.flatten() },
                    _ => return Err(de::Error::custom("invalid cell number kind fields")),
                };
                out.validate().map_err(<A::Error as de::Error>::custom)?;
                Ok(out)
            }
        }
        deserializer.deserialize_map(KindVisitor)
    }
}

enum Scalar {
    String(String),
    Boolean(bool),
}
impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct ScalarVisitor;
        impl Visitor<'_> for ScalarVisitor {
            type Value = Scalar;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded string or boolean scalar")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> std::result::Result<Scalar, E> {
                Ok(Scalar::Boolean(value))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Scalar, E> {
                if value.len() > MAX_CELL_STRING_BYTES {
                    return Err(E::custom("cell string exceeds size limit"));
                }
                let mut owned = String::new();
                owned.try_reserve_exact(value.len()).map_err(|_| E::custom("cannot allocate cell string"))?;
                owned.push_str(value);
                Ok(Scalar::String(owned))
            }
            fn visit_borrowed_str<E: de::Error>(self, value: &str) -> std::result::Result<Scalar, E> {
                self.visit_str(value)
            }
            fn visit_string<E: de::Error>(self, value: String) -> std::result::Result<Scalar, E> {
                if value.len() > MAX_CELL_STRING_BYTES { Err(E::custom("cell string exceeds size limit")) } else { Ok(Scalar::String(value)) }
            }
        }
        deserializer.deserialize_any(ScalarVisitor)
    }
}

impl<'de> Deserialize<'de> for CellValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct ValueVisitor;
        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = CellValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a typed cell value object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Self::Value, A::Error> {
                let (mut kind, mut value, mut number_kind, mut lexical) = (None, None, None, None);
                while let Some(field) = map.next_key::<Field>()? {
                    match field {
                        Field::Kind if kind.is_none() => kind = Some(map.next_value::<Tag>()?),
                        Field::Value if value.is_none() => value = Some(map.next_value::<Scalar>()?),
                        Field::NumberKind if number_kind.is_none() => number_kind = Some(map.next_value::<CellNumberKind>()?),
                        Field::Lexical if lexical.is_none() => lexical = Some(map.next_value_seed(BoundedStringSeed { max: MAX_TEMPORAL_BYTES })?),
                        Field::Kind | Field::Value | Field::NumberKind | Field::Lexical => {
                            return Err(de::Error::custom("duplicate cell value field"));
                        }
                        _ => return Err(de::Error::custom("unexpected cell value field")),
                    }
                }
                let out = match (kind, value, number_kind, lexical) {
                    (Some(Tag::String), Some(Scalar::String(value)), None, None) => CellValue::String { value },
                    (Some(Tag::Boolean), Some(Scalar::Boolean(value)), None, None) => CellValue::Boolean { value },
                    (Some(Tag::Number), None, Some(kind), Some(lexical)) => CellValue::Number { kind, lexical },
                    (Some(Tag::Date), None, None, Some(lexical)) => CellValue::Date { lexical },
                    (Some(Tag::Duration), None, None, Some(lexical)) => CellValue::Duration { lexical },
                    _ => return Err(de::Error::custom("invalid cell value fields")),
                };
                out.validate().map_err(<A::Error as de::Error>::custom)?;
                Ok(out)
            }
        }
        deserializer.deserialize_map(ValueVisitor)
    }
}

struct DisplaySeed;
impl<'de> DeserializeSeed<'de> for DisplaySeed {
    type Value = Vec<String>;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error> {
        struct DisplayVisitor;
        struct RejectExtra;
        impl<'de> DeserializeSeed<'de> for RejectExtra {
            type Value = ();
            fn deserialize<D: Deserializer<'de>>(self, _deserializer: D) -> std::result::Result<(), D::Error> {
                Err(de::Error::custom("cell display has too many paragraphs"))
            }
        }
        impl<'de> Visitor<'de> for DisplayVisitor {
            type Value = Vec<String>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded cell display paragraph sequence")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
                if seq.size_hint().is_some_and(|n| n > MAX_DISPLAY_PARAGRAPHS) {
                    return Err(de::Error::custom("cell display has too many paragraphs"));
                }
                let mut out = Vec::new();
                let mut bytes = 0usize;
                loop {
                    if out.len() == MAX_DISPLAY_PARAGRAPHS {
                        // Ask only whether another item exists. The rejection seed never
                        // decodes or allocates that item, even if it is a giant string/map.
                        if seq.next_element_seed(RejectExtra)?.is_some() {
                            return Err(de::Error::custom("cell display has too many paragraphs"));
                        }
                        break;
                    }
                    let Some(text) = seq.next_element_seed(BoundedStringSeed { max: MAX_DISPLAY_BYTES.saturating_sub(bytes) })? else {
                        break;
                    };
                    bytes = bytes.saturating_add(text.len());
                    out.try_reserve(1).map_err(|_| <A::Error as de::Error>::custom("cannot allocate cell display paragraphs"))?;
                    out.push(text);
                }
                if out.is_empty() {
                    out.try_reserve(1).map_err(|_| <A::Error as de::Error>::custom("cannot allocate cell display paragraphs"))?;
                    out.push(String::new());
                }
                Ok(out)
            }
        }
        deserializer.deserialize_seq(DisplayVisitor)
    }
}

impl<'de> Deserialize<'de> for CellConstant {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct ConstantVisitor;
        impl<'de> Visitor<'de> for ConstantVisitor {
            type Value = CellConstant;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a cell constant with value and display baseline")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Self::Value, A::Error> {
                let (mut value, mut display) = (None, None);
                while let Some(field) = map.next_key::<Field>()? {
                    match field {
                        Field::Value if value.is_none() => value = Some(map.next_value::<CellValue>()?),
                        Field::Display if display.is_none() => display = Some(map.next_value_seed(DisplaySeed)?),
                        Field::Value | Field::Display => {
                            return Err(de::Error::custom("duplicate cell constant field"));
                        }
                        _ => return Err(de::Error::custom("unexpected cell constant field")),
                    }
                }
                let out = CellConstant {
                    value: value.ok_or_else(|| <A::Error as de::Error>::missing_field("value"))?,
                    display: display.ok_or_else(|| <A::Error as de::Error>::missing_field("display"))?,
                };
                out.validate_metadata().map_err(<A::Error as de::Error>::custom)?;
                Ok(out)
            }
        }
        deserializer.deserialize_map(ConstantVisitor)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::text::{Paragraph, Run, RunProps};
    use std::sync::Arc;

    fn number(lexical: &str) -> CellValue {
        CellValue::Number { kind: CellNumberKind::Float, lexical: lexical.into() }
    }
    fn constant(text: &str) -> CellConstant {
        CellConstant::from_display(number("0.5"), &TextBody::from_text(text)).unwrap()
    }
    fn stored_json(value: serde_json::Value, display: serde_json::Value) -> String {
        serde_json::json!({"value":value,"display":display}).to_string()
    }

    #[test]
    fn import_precharge_bounds_actual_baseline_allocations_and_rejects_unsupported_display() {
        let mut reserved = String::with_capacity(4096);
        reserved.push_str("stored");
        for body in [TextBody::default(), TextBody::from_text(""), TextBody::from_text("a\u{b}漢\n\nend")] {
            let value = CellValue::String { value: reserved.clone() };
            let charge = CellConstant::metadata_bytes_for_display(&value, &body).unwrap();
            let constant = CellConstant::from_display(value, &body).unwrap();
            assert!(charge >= constant.metadata_bytes());
            assert!(constant.matches_display(&body).unwrap());
        }
        let mut unsupported = TextBody::from_text("shown");
        unsupported.paragraphs[0].runs[0].kind = RunKind::Field { field: "slidenum".into() };
        assert!(CellConstant::metadata_bytes_for_display(&number("1"), &unsupported).is_err());
        assert!(CellConstant::metadata_bytes_for_display(&number("1"), &TextBody::from_text(&"x".repeat(MAX_DISPLAY_BYTES + 1))).is_err());
    }

    #[test]
    fn numeric_grammar_keeps_lexical_precision_and_rejects_nonfinite_underflow_and_bad_forms() {
        for literal in ["0", "-0.00", "+.5", "5.", "1E+3", "9007199254740993", "4.9406564584124654e-324"] {
            let data = CellConstant::from_display(number(literal), &TextBody::from_text("shown")).unwrap();
            let encoded = serde_json::to_string(&data).unwrap();
            let back: CellConstant = serde_json::from_str(&encoded).unwrap();
            assert_eq!(back.value(), &number(literal));
        }
        for literal in ["", " ", " 1", "1 ", "+", ".", "1e", "1e+", "NaN", "INF", "-inf", "1e309", "1e-999", "0x10", "1_000", "1,5", "１２", "1.2.3"]
        {
            assert!(CellConstant::from_display(number(literal), &TextBody::default()).is_err(), "{literal}");
        }
    }

    #[test]
    fn value_and_display_are_independent_unicode_and_currency_empty_is_distinct_from_absent() {
        for value in [
            CellValue::String { value: "stored\t漢\nب".into() },
            CellValue::Boolean { value: false },
            CellValue::Number { kind: CellNumberKind::Percentage, lexical: "0.5".into() },
            CellValue::Number { kind: CellNumberKind::Currency { currency: None }, lexical: "01.25".into() },
            CellValue::Number { kind: CellNumberKind::Currency { currency: Some(String::new()) }, lexical: "01.25".into() },
        ] {
            let data = CellConstant::from_display(value.clone(), &TextBody::from_text("shown differently")).unwrap();
            let back: CellConstant = serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();
            assert_eq!(back.value(), &value);
            assert!(back.matches_display(&TextBody::from_text("shown differently")).unwrap());
            assert!(!back.matches_display(&TextBody::from_text("stored\t漢\nب")).unwrap());
        }
    }

    #[test]
    fn formatting_and_run_splits_keep_baseline_but_paragraph_and_soft_break_boundaries_matter() {
        let data = constant("A漢\u{b}ب\nSecond");
        let mut body = TextBody::from_text("A漢\u{b}ب\nSecond");
        body.paragraphs[0].runs = vec![
            Run::new("A"),
            Run::with("漢", RunProps { bold: Some(true), ..Default::default() }),
            Run { kind: RunKind::Break, ..Run::new("ignored break payload") },
            Run::new("ب"),
        ];
        assert!(data.matches_display(&body).unwrap());
        assert!(!data.matches_display(&TextBody::from_text("A漢\nب\nSecond")).unwrap());
        assert!(!data.matches_display(&TextBody::from_text("A漢\u{b}بSecond")).unwrap());
        let empty = constant("");
        assert!(empty.matches_display(&TextBody { paragraphs: vec![], ..Default::default() }).unwrap());
        assert!(!empty.matches_display(&TextBody::from_text("\n")).unwrap());
        assert!(!constant("e\u{301}").matches_display(&TextBody::from_text("é")).unwrap(), "comparison does not normalize Unicode");
    }

    #[test]
    fn dynamic_and_oversized_new_displays_are_stale_and_can_be_reconciled() {
        for kind in [RunKind::Field { field: "slidenum".into() }, RunKind::Math { omml: "<m/>".into() }] {
            let mut body = TextBody::from_text("shown");
            body.paragraphs[0].runs[0].kind = kind;
            assert!(CellConstant::from_display(number("1"), &body).is_err());
            assert!(!constant("shown").matches_display(&body).unwrap());
        }
        let mut cell = super::super::Cell::new("shown");
        cell.data = Some(Arc::new(constant("shown")));
        cell.edit_display(|body| {
            *body = TextBody::from_text(&"x".repeat(MAX_DISPLAY_BYTES + 1));
        })
        .unwrap();
        assert!(cell.data.is_none());
        let oversized_runs =
            TextBody { paragraphs: vec![Paragraph { runs: vec![Run::new(""); MAX_DISPLAY_RUNS + 1], ..Default::default() }], ..Default::default() };
        assert!(!constant("").matches_display(&oversized_runs).unwrap());
        assert!(CellConstant::from_display(number("1"), &oversized_runs).is_err());
    }

    #[test]
    fn cell_state_and_local_edits_do_not_rebind_stale_records() {
        let mut cell = super::super::Cell::new("shown");
        assert_eq!(cell.constant_state().unwrap(), CellConstantState::Absent);
        let data = Arc::new(constant("shown"));
        cell.data = Some(data.clone());
        cell.edit_display(|body| body.paragraphs[0].runs[0].props.bold = Some(true)).unwrap();
        assert!(matches!(cell.constant_state().unwrap(), CellConstantState::Current(_)));
        assert!(Arc::ptr_eq(cell.data.as_ref().unwrap(), &data));
        cell.text = TextBody::from_text("direct external edit");
        assert_eq!(cell.constant_state().unwrap(), CellConstantState::Stale);
        assert!(cell.reconcile_constant().unwrap());
        assert!(!cell.reconcile_constant().unwrap());
        cell.text = TextBody::from_text("shown");
        assert_eq!(cell.constant_state().unwrap(), CellConstantState::Absent);
    }

    #[test]
    fn custom_serde_rejects_unknown_duplicate_missing_wrong_variant_and_extra_fields() {
        for json in [
            r#"{"value":{"kind":"boolean","value":true},"display":["x"],"extra":0}"#,
            r#"{"value":{"kind":"boolean","value":true},"display":["x"],"display":[]}"#,
            r#"{"value":{"kind":"boolean","kind":"boolean","value":true},"display":[]}"#,
            r#"{"value":{"kind":"boolean","value":true,"lexical":"1"},"display":[]}"#,
            r#"{"value":{"kind":"string","value":false},"display":[]}"#,
            r#"{"value":{"kind":"number","numberKind":{"kind":"float","currency":null},"lexical":"1"},"display":[]}"#,
            r#"{"value":{"kind":"number","numberKind":{"kind":"currency","currency":null,"currency":"USD"},"lexical":"1"},"display":[]}"#,
            r#"{"value":{"kind":"boolean","value":true}}"#,
            r#"{"display":[]}"#,
            r#"{"value":{"kind":"date","value":"2026-10-15"},"display":[]}"#,
            r#"{"value":{"kind":"boolean","value":true},"display":[false]}"#,
        ] {
            assert!(serde_json::from_str::<CellConstant>(json).is_err(), "{json}");
        }
        let reverse = r#"{"display":["x"],"value":{"lexical":"0.5","numberKind":{"kind":"percentage"},"kind":"number"}}"#;
        assert!(serde_json::from_str::<CellConstant>(reverse).unwrap().matches_display(&TextBody::from_text("x")).unwrap());
        let early = serde_json::from_str::<CellConstant>(r#"{"unknown":["#).unwrap_err().to_string();
        assert!(early.contains("unknown cell constant field"), "unknown payload must not be decoded: {early}");
    }

    #[test]
    fn string_numeric_currency_paragraph_and_aggregate_display_caps_are_enforced() {
        for value in [
            serde_json::json!({"kind":"string","value":"漢".repeat(MAX_CELL_STRING_BYTES/3+1)}),
            serde_json::json!({"kind":"number","numberKind":{"kind":"currency","currency":"x".repeat(MAX_CURRENCY_BYTES+1)},"lexical":"1"}),
            serde_json::json!({"kind":"number","numberKind":{"kind":"float"},"lexical":"0".repeat(MAX_NUMBER_BYTES+1)}),
        ] {
            assert!(serde_json::from_str::<CellConstant>(&stored_json(value, serde_json::json!([""]))).is_err());
        }
        for display in [
            serde_json::json!(["x".repeat(MAX_DISPLAY_BYTES + 1)]),
            serde_json::json!(["x".repeat(MAX_DISPLAY_BYTES / 2), "x".repeat(MAX_DISPLAY_BYTES / 2 + 1)]),
            serde_json::json!(vec![""; MAX_DISPLAY_PARAGRAPHS + 1]),
        ] {
            assert!(serde_json::from_str::<CellConstant>(&stored_json(serde_json::json!({"kind":"boolean","value":false}), display)).is_err());
        }
        let json = stored_json(
            serde_json::json!({"kind":"string","value":"x".repeat(MAX_CELL_STRING_BYTES)}),
            serde_json::json!(vec![""; MAX_DISPLAY_PARAGRAPHS]),
        );
        let data: CellConstant = serde_json::from_str(&json).unwrap();
        assert!(data.metadata_bytes() >= MAX_CELL_STRING_BYTES + MAX_DISPLAY_PARAGRAPHS * size_of::<String>());
        let empty: CellConstant =
            serde_json::from_str(&stored_json(serde_json::json!({"kind":"boolean","value":false}), serde_json::json!([]))).unwrap();
        assert_eq!(empty.display_baseline(), &[String::new()]);
    }

    #[test]
    fn malformed_internal_metadata_errors_before_current_display_checks() {
        let malformed = CellConstant { value: number("NaN"), display: vec![String::new()] };
        let body = TextBody { paragraphs: vec![Paragraph::default(); MAX_DISPLAY_PARAGRAPHS + 1], ..Default::default() };
        assert!(malformed.matches_display(&body).is_err());
    }

    #[test]
    fn stored_attribute_characters_follow_xml10_but_display_soft_breaks_remain_valid() {
        for text in ["a\0b", "\u{b}", "\u{1f}", "\u{fffe}", "\u{ffff}"] {
            for value in [
                CellValue::String { value: text.into() },
                CellValue::Number { kind: CellNumberKind::Currency { currency: Some(text.into()) }, lexical: "1".into() },
            ] {
                assert!(CellConstant::from_display(value.clone(), &TextBody::from_text("shown")).is_err());
                assert!(
                    serde_json::from_str::<CellConstant>(&stored_json(serde_json::to_value(value).unwrap(), serde_json::json!(["shown"]))).is_err()
                );
            }
        }
        let data = CellConstant::from_display(CellValue::String { value: "\t\n\r漢😀".into() }, &TextBody::from_text("A\u{b}B")).unwrap();
        let back: CellConstant = serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();
        assert!(back.matches_display(&TextBody::from_text("A\u{b}B")).unwrap());
        assert!(CellNumberKind::Currency { currency: Some("\tUSD".into()) }.validate().is_ok());
        assert!(serde_json::from_str::<CellNumberKind>(r#"{"kind":"currency","currency":"\u0000"}"#).is_err());
    }
}
