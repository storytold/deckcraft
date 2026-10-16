//! Checked exact decimal arithmetic for the versioned table formula profile.
//! Values are coefficient × 10^(-scale), with 28 normalized significant digits
//! and a normalized scale in -28..=28. Intermediate i128 overflow is an error.
use crate::{ModelError, Result};

#[path = "table_decimal_v2.rs"]
mod v2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Decimal {
    coefficient: i128,
    scale: i32,
}

fn limit() -> ModelError {
    ModelError::Invalid("formula decimal exceeds exact-decimal-v1 numeric limits".into())
}

impl Decimal {
    fn normalized(coefficient: i128, scale: i32) -> Result<Self> {
        Self::normalized_with_scale(coefficient, scale, 28)
    }
    fn normalized_with_scale(mut coefficient: i128, mut scale: i32, maximum_scale: i32) -> Result<Self> {
        if coefficient == 0 {
            return Ok(Self { coefficient: 0, scale: 0 });
        }
        while coefficient % 10 == 0 {
            coefficient /= 10;
            scale = scale.checked_sub(1).ok_or_else(limit)?;
        }
        let mut magnitude = coefficient.checked_abs().ok_or_else(limit)?;
        let mut digits = 0;
        while magnitude != 0 {
            magnitude /= 10;
            digits += 1;
        }
        if digits > 28 || !(-maximum_scale..=maximum_scale).contains(&scale) {
            return Err(limit());
        }
        Ok(Self { coefficient, scale })
    }

    /// Parse the lexical decimal directly. No binary floating-point conversion occurs.
    pub(crate) fn parse(source: &str) -> Result<Self> {
        Self::parse_with_scale(source, 28)
    }
    fn parse_with_scale(source: &str, maximum_scale: i32) -> Result<Self> {
        let bytes = source.as_bytes();
        if bytes.is_empty() || bytes.len() > 128 || !source.is_ascii() {
            return Err(limit());
        }
        let mut at = 0usize;
        let negative = bytes.first() == Some(&b'-');
        if matches!(bytes.first(), Some(b'+' | b'-')) {
            at += 1;
        }
        let mut coefficient = 0i128;
        let mut digits = 0usize;
        let mut fractional = 0i32;
        let mut after_dot = false;
        let mut significant_started = false;
        let mut trailing_zeroes = 0i32;
        while let Some(byte) = bytes.get(at).copied() {
            if byte == b'.' && !after_dot {
                after_dot = true;
                at += 1;
                if !bytes.get(at).is_some_and(u8::is_ascii_digit) {
                    return Err(limit());
                }
                continue;
            }
            if !byte.is_ascii_digit() {
                break;
            }
            digits += 1;
            if after_dot {
                fractional = fractional.checked_add(1).ok_or_else(limit)?;
            }
            // Delay trailing zeroes so an admissible normalized token need not fit
            // its unnormalized coefficient into i128 (for example 1e0 with 90 zeroes).
            if byte == b'0' {
                if significant_started {
                    trailing_zeroes = trailing_zeroes.checked_add(1).ok_or_else(limit)?;
                }
            } else {
                if significant_started {
                    coefficient = coefficient.checked_mul(power10(trailing_zeroes)?).ok_or_else(limit)?;
                }
                coefficient = coefficient.checked_mul(10).and_then(|value| value.checked_add(i128::from(byte - b'0'))).ok_or_else(limit)?;
                significant_started = true;
                trailing_zeroes = 0;
            }
            at += 1;
        }
        if digits == 0 {
            return Err(limit());
        }
        let mut exponent = 0i32;
        if matches!(bytes.get(at), Some(b'e' | b'E')) {
            at += 1;
            let minus = bytes.get(at) == Some(&b'-');
            if matches!(bytes.get(at), Some(b'+' | b'-')) {
                at += 1;
            }
            let start = at;
            while let Some(byte) = bytes.get(at).copied().filter(u8::is_ascii_digit) {
                exponent = exponent.checked_mul(10).and_then(|value| value.checked_add(i32::from(byte - b'0'))).ok_or_else(limit)?;
                at += 1;
            }
            if at == start {
                return Err(limit());
            }
            if minus {
                exponent = exponent.checked_neg().ok_or_else(limit)?;
            }
        }
        if at != bytes.len() {
            return Err(limit());
        }
        if negative {
            coefficient = coefficient.checked_neg().ok_or_else(limit)?;
        }
        let scale = fractional.checked_sub(exponent).and_then(|value| value.checked_sub(trailing_zeroes)).ok_or_else(limit)?;
        Self::normalized_with_scale(coefficient, scale, maximum_scale)
    }

    pub(crate) fn neg(self) -> Result<Self> {
        Self::normalized(self.coefficient.checked_neg().ok_or_else(limit)?, self.scale)
    }

    pub(crate) fn add(self, other: Self) -> Result<Self> {
        let scale = self.scale.max(other.scale);
        let left = self.coefficient.checked_mul(power10(scale - self.scale)?).ok_or_else(limit)?;
        let right = other.coefficient.checked_mul(power10(scale - other.scale)?).ok_or_else(limit)?;
        Self::normalized(left.checked_add(right).ok_or_else(limit)?, scale)
    }

    pub(crate) fn sub(self, other: Self) -> Result<Self> {
        self.add(other.neg()?)
    }

    pub(crate) fn mul(self, other: Self) -> Result<Self> {
        Self::normalized(self.coefficient.checked_mul(other.coefficient).ok_or_else(limit)?, self.scale.checked_add(other.scale).ok_or_else(limit)?)
    }

    pub(crate) fn canonical(self) -> String {
        if self.coefficient == 0 {
            return "0".into();
        }
        // At most 125 bytes are emitted under the widest (v2) normalized profile bounds.
        let negative = self.coefficient < 0;
        let digits = self.coefficient.unsigned_abs().to_string();
        let mut result = String::with_capacity(if (-28..=28).contains(&self.scale) { 60 } else { 128 });
        if negative {
            result.push('-');
        }
        if self.scale <= 0 {
            result.push_str(&digits);
            for _ in 0..self.scale.unsigned_abs() {
                result.push('0');
            }
        } else if self.scale as usize >= digits.len() {
            result.push_str("0.");
            for _ in 0..(self.scale as usize - digits.len()) {
                result.push('0');
            }
            result.push_str(&digits);
        } else {
            let point = digits.len() - self.scale as usize;
            if let (Some(left), Some(right)) = (digits.get(..point), digits.get(point..)) {
                result.push_str(left);
                result.push('.');
                result.push_str(right);
            }
        }
        result
    }
}

fn power10(exponent: i32) -> Result<i128> {
    let exponent = u32::try_from(exponent).map_err(|_| limit())?;
    10i128.checked_pow(exponent).ok_or_else(limit)
}
