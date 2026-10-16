//! Deterministic, value-preserving presentation-table number formats.
//!
//! Format programs contain no executable expressions or host-locale lookups.
//! Their visible text is a cache; the stored scalar remains authoritative.
use serde::{Deserialize, Serialize};

use super::CellValue;
use crate::{ModelError, Result};

#[path = "table_number_display.rs"]
mod display;

pub const MAX_FORMAT_TEXT: usize = 1024;
pub const MAX_FORMAT_PARTS: usize = 64;
pub const MAX_FORMAT_SECTIONS: usize = 16;

pub(super) fn invalid(message: &'static str) -> ModelError {
    ModelError::Invalid(message.into())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NumberLocale {
    #[default]
    #[serde(rename = "en-US", alias = "en-us")]
    EnUs,
    #[serde(rename = "fr-BE", alias = "fr-be")]
    FrBe,
    #[serde(rename = "de-DE", alias = "de-de")]
    DeDe,
}
impl NumberLocale {
    pub fn separators(self) -> (char, char) {
        match self {
            Self::EnUs => ('.', ','),
            Self::FrBe => (',', '\u{202f}'),
            Self::DeDe => (',', '.'),
        }
    }
    pub fn language(self) -> &'static str {
        match self {
            Self::EnUs => "en",
            Self::FrBe => "fr",
            Self::DeDe => "de",
        }
    }
    pub fn country(self) -> &'static str {
        match self {
            Self::EnUs => "US",
            Self::FrBe => "BE",
            Self::DeDe => "DE",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NegativeDisplay {
    #[default]
    Minus,
    Parentheses,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NumberComparison {
    Less,
    LessEqual,
    Equal,
    NotEqual,
    GreaterEqual,
    Greater,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum DateFormatPart {
    Literal {
        text: String,
    },
    Year {
        long: bool,
    },
    Month {
        long: bool,
        textual: bool,
    },
    Day {
        long: bool,
    },
    DayOfWeek {
        long: bool,
    },
    Hours {
        long: bool,
        elapsed: bool,
    },
    Minutes {
        long: bool,
    },
    Seconds {
        long: bool,
        decimal_places: u8,
    },
    AmPm,
    /// Preserve the authored timezone without conversion to the host timezone.
    Zone,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum NumberStyle {
    #[default]
    General,
    Number {
        decimal_places: u8,
        min_decimal_places: u8,
        min_integer_digits: u8,
        grouping: bool,
    },
    Percentage {
        decimal_places: u8,
        min_decimal_places: u8,
        grouping: bool,
    },
    Currency {
        decimal_places: u8,
        min_decimal_places: u8,
        grouping: bool,
        code: String,
        symbol: String,
        symbol_after: bool,
        space: bool,
    },
    Scientific {
        decimal_places: u8,
        min_exponent_digits: u8,
        exponent_sign: bool,
    },
    Fraction {
        denominator_digits: u8,
        fixed_denominator: Option<u32>,
    },
    DateTime {
        parts: Vec<DateFormatPart>,
    },
    Duration {
        parts: Vec<DateFormatPart>,
    },
    Boolean {
        true_text: String,
        false_text: String,
    },
    Text,
}

impl NumberStyle {
    fn validate(&self) -> Result<()> {
        let precision = |decimal: u8, minimum: u8| {
            if decimal > 28 || minimum > decimal { Err(invalid("invalid number-format decimal precision")) } else { Ok(()) }
        };
        match self {
            Self::General | Self::Text => {}
            Self::Number { decimal_places, min_decimal_places, min_integer_digits, .. } => {
                precision(*decimal_places, *min_decimal_places)?;
                if *min_integer_digits > 64 {
                    return Err(invalid("number-format minimum integer digits exceeds limit"));
                }
            }
            Self::Percentage { decimal_places, min_decimal_places, .. } => precision(*decimal_places, *min_decimal_places)?,
            Self::Currency { decimal_places, min_decimal_places, code, symbol, .. } => {
                precision(*decimal_places, *min_decimal_places)?;
                if code.len() > 32 || symbol.len() > 64 || !xml_text(code) || !xml_text(symbol) {
                    return Err(invalid("invalid number-format currency code or symbol"));
                }
            }
            Self::Scientific { decimal_places, min_exponent_digits, .. } => {
                precision(*decimal_places, 0)?;
                if *min_exponent_digits > 8 {
                    return Err(invalid("scientific exponent width exceeds limit"));
                }
            }
            Self::Fraction { denominator_digits, fixed_denominator } => {
                if !(1..=4).contains(denominator_digits) || fixed_denominator.is_some_and(|d| d == 0 || d > 9999) {
                    return Err(invalid("fraction denominator exceeds format limits"));
                }
            }
            Self::DateTime { parts } | Self::Duration { parts } => {
                if parts.is_empty() || parts.len() > MAX_FORMAT_PARTS {
                    return Err(invalid("date/time format part count exceeds limit"));
                }
                let mut bytes = 0usize;
                for part in parts {
                    match part {
                        DateFormatPart::Literal { text } => {
                            bytes = bytes.checked_add(text.len()).ok_or_else(|| invalid("format text exceeds limit"))?;
                            if !xml_text(text) {
                                return Err(invalid("invalid date/time format literal"));
                            }
                        }
                        DateFormatPart::Seconds { decimal_places, .. } if *decimal_places > 28 => {
                            return Err(invalid("date/time fractional seconds exceeds limit"));
                        }
                        _ => {}
                    }
                }
                if bytes > MAX_FORMAT_TEXT {
                    return Err(invalid("date/time format literal bytes exceeds limit"));
                }
            }
            Self::Boolean { true_text, false_text } => {
                if true_text.len() > MAX_FORMAT_TEXT || false_text.len() > MAX_FORMAT_TEXT || !xml_text(true_text) || !xml_text(false_text) {
                    return Err(invalid("invalid Boolean format text"));
                }
            }
        }
        Ok(())
    }
}

/// One flat conditional section. No style-reference cycles or executable conditions exist.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NumberFormatSection {
    pub comparison: NumberComparison,
    pub threshold: String,
    pub style: NumberStyle,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub negative: NegativeDisplay,
    #[serde(default)]
    pub color: Option<[u8; 4]>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct NumberFormat {
    /// Source data-style identity; the same definition may be shared by several cells.
    pub name: String,
    pub locale: NumberLocale,
    pub style: NumberStyle,
    pub prefix: String,
    pub suffix: String,
    pub negative: NegativeDisplay,
    pub zero_display: Option<String>,
    pub color: Option<[u8; 4]>,
    pub sections: Vec<NumberFormatSection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormattedCellValue {
    pub text: String,
    pub color: Option<[u8; 4]>,
}

pub(super) fn xml_text(text: &str) -> bool {
    text.chars().all(|c| matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}'))
}

impl NumberFormat {
    pub fn metadata_bytes(&self) -> Result<usize> {
        self.validate()?;
        struct Counter(usize);
        impl std::io::Write for Counter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0 = self
                    .0
                    .checked_add(bytes.len())
                    .filter(|n| *n <= 256 * 1024)
                    .ok_or_else(|| std::io::Error::other("number-format metadata exceeds size limit"))?;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut counter = Counter(0);
        serde_json::to_writer(&mut counter, self).map_err(|_| invalid("invalid or oversized number-format metadata"))?;
        counter.0.checked_mul(2).and_then(|n| n.checked_add(512)).ok_or_else(|| invalid("number-format metadata exceeds size limit"))
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.len() > 256 || !xml_text(&self.name) || self.sections.len() > MAX_FORMAT_SECTIONS {
            return Err(invalid("invalid number-format name or section count"));
        }
        let mut bytes = self.name.len();
        for text in [&self.prefix, &self.suffix].into_iter().chain(self.zero_display.iter()) {
            bytes = bytes.checked_add(text.len()).ok_or_else(|| invalid("format text exceeds limit"))?;
            if text.len() > MAX_FORMAT_TEXT || !xml_text(text) {
                return Err(invalid("invalid number-format text"));
            }
        }
        self.style.validate()?;
        for section in &self.sections {
            CellValue::Number { kind: super::CellNumberKind::Float, lexical: section.threshold.clone() }.validate()?;
            display::canonical(&section.threshold)?;
            section.style.validate()?;
            for text in [&section.prefix, &section.suffix, &section.threshold] {
                bytes = bytes.checked_add(text.len()).ok_or_else(|| invalid("format text exceeds limit"))?;
                if text.len() > MAX_FORMAT_TEXT || !xml_text(text) {
                    return Err(invalid("invalid conditional number-format text"));
                }
            }
        }
        if bytes > 32 * MAX_FORMAT_TEXT {
            return Err(invalid("number-format text budget exceeded"));
        }
        Ok(())
    }

    pub fn format_number(&self, lexical: &str) -> Result<String> {
        Ok(self.format_value(&CellValue::Number { kind: super::CellNumberKind::Float, lexical: lexical.into() })?.text)
    }

    pub fn format_boolean(&self, value: bool) -> Result<String> {
        Ok(self.format_value(&CellValue::Boolean { value })?.text)
    }

    pub fn format_text(&self, value: &str) -> Result<String> {
        Ok(self.format_value(&CellValue::String { value: value.into() })?.text)
    }

    pub fn format_value(&self, value: &CellValue) -> Result<FormattedCellValue> {
        self.validate()?;
        value.validate()?;
        let number = match value {
            CellValue::Number { lexical, .. } => Some(display::canonical(lexical)?),
            _ => None,
        };
        let section = if let Some(number) = &number {
            {
                let mut matched = None;
                for section in &self.sections {
                    if display::comparison(number, &section.threshold, section.comparison)? {
                        matched = Some(section);
                        break;
                    }
                }
                matched
            }
        } else {
            None
        };
        let (style, prefix, suffix, negative, color) = match section {
            Some(s) => (&s.style, s.prefix.as_str(), s.suffix.as_str(), s.negative, s.color),
            None => (&self.style, self.prefix.as_str(), self.suffix.as_str(), self.negative, self.color),
        };
        if number.as_deref() == Some("0")
            && let Some(zero) = &self.zero_display
        {
            return Ok(FormattedCellValue { text: zero.clone(), color });
        }
        let raw = display::format(style, self.locale, value)?;
        let (sign, raw) = match raw.strip_prefix('-') {
            Some(raw) if number.is_some() || matches!(value, CellValue::Duration { .. }) => (true, raw),
            _ => (false, raw.as_str()),
        };
        let mut text = String::new();
        text.try_reserve_exact(prefix.len().saturating_add(raw.len()).saturating_add(suffix.len()).saturating_add(2))
            .map_err(|_| invalid("cannot allocate formatted table value"))?;
        if sign {
            text.push(if negative == NegativeDisplay::Parentheses { '(' } else { '-' });
        }
        text.push_str(prefix);
        text.push_str(raw);
        text.push_str(suffix);
        if sign && negative == NegativeDisplay::Parentheses {
            text.push(')');
        }
        if text.len() > 4 * MAX_FORMAT_TEXT {
            return Err(invalid("formatted table value exceeds output limit"));
        }
        Ok(FormattedCellValue { text, color })
    }
}
