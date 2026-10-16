//! Fixed-capacity base-ten arithmetic. No float conversion or input-sized allocation.
use super::{Decimal, limit};
use crate::Result;
use std::cmp::Ordering;

const DIGITS: usize = 384;
#[path = "table_decimal_power.rs"]
mod power;
#[derive(Clone)]
struct Big {
    digits: [u8; DIGITS],
    len: usize,
}
impl Big {
    fn decimal(text: &str) -> Result<Self> {
        if text.is_empty() || text.len() > DIGITS || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(limit());
        }
        let mut out = Self::zero();
        out.len = text.len();
        for (at, byte) in text.bytes().rev().enumerate() {
            *out.digits.get_mut(at).ok_or_else(limit)? = byte - b'0';
        }
        out.trim();
        Ok(out)
    }
    fn add(&self, other: &Self) -> Result<Self> {
        let mut out = Self::zero();
        out.len = self.len.max(other.len);
        let mut carry = 0u16;
        for at in 0..out.len {
            let a = if at < self.len { self.digits.get(at).copied().ok_or_else(limit)? } else { 0 };
            let b = if at < other.len { other.digits.get(at).copied().ok_or_else(limit)? } else { 0 };
            let sum = u16::from(a) + u16::from(b) + carry;
            *out.digits.get_mut(at).ok_or_else(limit)? = (sum % 10) as u8;
            carry = sum / 10;
        }
        if carry != 0 {
            *out.digits.get_mut(out.len).ok_or_else(limit)? = carry as u8;
            out.len += 1;
        }
        Ok(out)
    }
    fn product(&self, other: &Self) -> Result<Self> {
        if self.is_zero() || other.is_zero() {
            return Ok(Self::zero());
        }
        let size = self.len.checked_add(other.len).filter(|n| *n <= DIGITS).ok_or_else(limit)?;
        let mut out = Self::zero();
        out.len = size;
        for a in 0..self.len {
            let left = u16::from(self.digits.get(a).copied().ok_or_else(limit)?);
            let mut carry = 0u16;
            for b in 0..other.len {
                let right = u16::from(other.digits.get(b).copied().ok_or_else(limit)?);
                let slot = out.digits.get_mut(a + b).ok_or_else(limit)?;
                let n = u16::from(*slot) + left * right + carry;
                *slot = (n % 10) as u8;
                carry = n / 10;
            }
            let mut at = a + other.len;
            while carry != 0 {
                let slot = out.digits.get_mut(at).ok_or_else(limit)?;
                let n = u16::from(*slot) + carry;
                *slot = (n % 10) as u8;
                carry = n / 10;
                at += 1;
            }
        }
        out.trim();
        Ok(out)
    }
    /// Divide by a power of ten without general long division.
    fn drop_digits(&self, count: usize) -> Result<(Self, bool)> {
        let remainder = self.digits.get(..count.min(self.len)).ok_or_else(limit)?.iter().any(|d| *d != 0);
        if count >= self.len {
            return Ok((Self::zero(), remainder));
        }
        let mut out = Self::zero();
        out.len = self.len - count;
        for at in 0..out.len {
            *out.digits.get_mut(at).ok_or_else(limit)? = self.digits.get(at + count).copied().ok_or_else(limit)?;
        }
        out.trim();
        Ok((out, remainder))
    }
    fn zero() -> Self {
        Self { digits: [0; DIGITS], len: 1 }
    }
    fn from(mut n: u128) -> Self {
        let mut out = Self::zero();
        let mut at = 0;
        while n != 0 && at < DIGITS {
            if let Some(slot) = out.digits.get_mut(at) {
                *slot = (n % 10) as u8;
            }
            n /= 10;
            at += 1;
        }
        out.len = at.max(1);
        out
    }
    fn trim(&mut self) {
        while self.len > 1 && self.digits.get(self.len - 1) == Some(&0) {
            self.len -= 1;
        }
    }
    fn is_zero(&self) -> bool {
        self.len == 1 && self.digits.first() == Some(&0)
    }
    fn cmp(&self, other: &Self) -> Ordering {
        self.len.cmp(&other.len).then_with(|| {
            for i in (0..self.len).rev() {
                let c = self.digits.get(i).cmp(&other.digits.get(i));
                if c != Ordering::Equal {
                    return c;
                }
            }
            Ordering::Equal
        })
    }
    fn shift(&mut self, count: usize) -> Result<()> {
        if self.is_zero() {
            return Ok(());
        }
        let end = self.len.checked_add(count).filter(|n| *n <= DIGITS).ok_or_else(limit)?;
        for at in (0..self.len).rev() {
            let digit = self.digits.get(at).copied().ok_or_else(limit)?;
            *self.digits.get_mut(at + count).ok_or_else(limit)? = digit;
        }
        self.digits.get_mut(..count).ok_or_else(limit)?.fill(0);
        self.len = end;
        Ok(())
    }
    fn add_small(&mut self, mut n: u32) -> Result<()> {
        let mut at = 0;
        while n != 0 {
            let slot = self.digits.get_mut(at).ok_or_else(limit)?;
            n += u32::from(*slot);
            *slot = (n % 10) as u8;
            n /= 10;
            at += 1;
            self.len = self.len.max(at);
        }
        Ok(())
    }
    fn mul_small(&mut self, n: u32) -> Result<()> {
        let mut carry = 0;
        for at in 0..self.len {
            let slot = self.digits.get_mut(at).ok_or_else(limit)?;
            let product = u32::from(*slot).checked_mul(n).and_then(|p| p.checked_add(carry)).ok_or_else(limit)?;
            *slot = (product % 10) as u8;
            carry = product / 10;
        }
        while carry != 0 {
            *self.digits.get_mut(self.len).ok_or_else(limit)? = (carry % 10) as u8;
            carry /= 10;
            self.len += 1;
        }
        self.trim();
        Ok(())
    }
    fn sub(&mut self, other: &Self) -> Result<()> {
        if self.cmp(other) == Ordering::Less {
            return Err(limit());
        }
        let mut borrow = 0i16;
        for at in 0..self.len {
            let right = if at < other.len { other.digits.get(at).copied().ok_or_else(limit)? } else { 0 };
            let slot = self.digits.get_mut(at).ok_or_else(limit)?;
            let n = i16::from(*slot) - i16::from(right) - borrow;
            *slot = if n < 0 { (n + 10) as u8 } else { n as u8 };
            borrow = i16::from(n < 0);
        }
        self.trim();
        Ok(())
    }
    fn div_rem(&self, divisor: &Self) -> Result<(Self, Self)> {
        if divisor.is_zero() {
            return Err(limit());
        }
        let mut quotient = Self::zero();
        quotient.len = self.len;
        let mut remainder = Self::zero();
        for at in (0..self.len).rev() {
            remainder.shift(1)?;
            remainder.add_small(u32::from(self.digits.get(at).copied().ok_or_else(limit)?))?;
            let mut q = 0;
            while remainder.cmp(divisor) != Ordering::Less {
                remainder.sub(divisor)?;
                q += 1;
                if q > 9 {
                    return Err(limit());
                }
            }
            *quotient.digits.get_mut(at).ok_or_else(limit)? = q;
        }
        quotient.trim();
        Ok((quotient, remainder))
    }
    fn integer(&self) -> Result<i128> {
        let mut n = 0i128;
        for at in (0..self.len).rev() {
            n = n.checked_mul(10).and_then(|n| n.checked_add(i128::from(self.digits.get(at).copied()?))).ok_or_else(limit)?;
        }
        Ok(n)
    }
    fn sqrt(&self) -> Result<(Self, Self)> {
        let mut root = Self::zero();
        let mut remainder = Self::zero();
        let pairs = self.len.div_ceil(2);
        for pair in (0..pairs).rev() {
            remainder.shift(2)?;
            let lo = self.digits.get(pair * 2).copied().ok_or_else(limit)?;
            let hi = if pair * 2 + 1 < self.len { self.digits.get(pair * 2 + 1).copied().ok_or_else(limit)? } else { 0 };
            remainder.add_small(u32::from(hi) * 10 + u32::from(lo))?;
            let mut candidate = 0u32;
            for digit in 1..=9 {
                let mut trial = root.clone();
                trial.mul_small(20)?;
                trial.add_small(digit)?;
                trial.mul_small(digit)?;
                if trial.cmp(&remainder) == Ordering::Greater {
                    break;
                }
                candidate = digit;
            }
            let mut deduction = root.clone();
            deduction.mul_small(20)?;
            deduction.add_small(candidate)?;
            deduction.mul_small(candidate)?;
            remainder.sub(&deduction)?;
            root.shift(1)?;
            root.add_small(candidate)?;
        }
        Ok((root, remainder))
    }
}

impl Decimal {
    pub(crate) fn parse_v2(source: &str) -> Result<Self> {
        Self::parse_with_scale(source, 96)
    }
    fn normalized_v2(coefficient: i128, scale: i32) -> Result<Self> {
        Self::normalized_with_scale(coefficient, scale, 96)
    }
    pub(crate) fn neg_v2(self) -> Result<Self> {
        Self::normalized_v2(self.coefficient.checked_neg().ok_or_else(limit)?, self.scale)
    }
    fn rounded_big(magnitude: Big, negative: bool, scale: i32) -> Result<Self> {
        let remove = magnitude.len.saturating_sub(28);
        let (mut coefficient, _) = magnitude.drop_digits(remove)?;
        if remove != 0 {
            let lead = magnitude.digits.get(remove - 1).copied().ok_or_else(limit)?;
            let further = magnitude.digits.get(..remove - 1).ok_or_else(limit)?.iter().any(|d| *d != 0);
            let odd = coefficient.digits.first().is_some_and(|n| n % 2 != 0);
            if lead > 5 || (lead == 5 && (further || odd)) {
                coefficient.add_small(1)?;
            }
        }
        let mut n = coefficient.integer()?;
        if negative {
            n = n.checked_neg().ok_or_else(limit)?;
        }
        Self::normalized_v2(n, scale.checked_sub(i32::try_from(remove).map_err(|_| limit())?).ok_or_else(limit)?)
    }
    pub(crate) fn add_v2(self, other: Self) -> Result<Self> {
        let scale = self.scale.max(other.scale);
        let mut left = Big::from(self.coefficient.unsigned_abs());
        left.shift((scale - self.scale) as usize)?;
        let mut right = Big::from(other.coefficient.unsigned_abs());
        right.shift((scale - other.scale) as usize)?;
        let (magnitude, negative) = if self.sign() == other.sign() {
            (left.add(&right)?, self.sign() < 0)
        } else if left.cmp(&right) != Ordering::Less {
            left.sub(&right)?;
            (left, self.sign() < 0)
        } else {
            right.sub(&left)?;
            (right, other.sign() < 0)
        };
        Self::rounded_big(magnitude, negative, scale)
    }
    pub(crate) fn sub_v2(self, other: Self) -> Result<Self> {
        self.add_v2(other.neg_v2()?)
    }
    pub(crate) fn mul_v2(self, other: Self) -> Result<Self> {
        Self::rounded_big(
            Big::from(self.coefficient.unsigned_abs()).product(&Big::from(other.coefficient.unsigned_abs()))?,
            self.sign() != other.sign(),
            self.scale.checked_add(other.scale).ok_or_else(limit)?,
        )
    }
    /// Exact clock decomposition, with no host timezone conversion.
    pub(crate) fn day_parts(self) -> Result<(u32, u32, u32, String)> {
        self.day_parts_inner(false)
    }
    pub(crate) fn clock_parts(self) -> Result<(u32, u32, u32, String)> {
        self.day_parts_inner(true)
    }
    fn day_parts_inner(self, compensate: bool) -> Result<(u32, u32, u32, String)> {
        if self.scale <= 0 {
            return Ok((0, 0, 0, String::new()));
        }
        let mut denominator = Big::from(1);
        denominator.shift(self.scale as usize)?;
        let (_, mut fraction) = Big::from(self.coefficient.unsigned_abs()).div_rem(&denominator)?;
        if self.sign() < 0 && !fraction.is_zero() {
            let old = fraction;
            fraction = denominator.clone();
            fraction.sub(&old)?;
        }
        fraction.mul_small(86_400)?;
        let (whole, mut remainder) = fraction.div_rem(&denominator)?;
        let mut whole = whole.integer()?;
        // Numeric serials are rounded to 28 significant decimal digits. Correct
        // only a sub-half-ULP deficit at an integer-second boundary; this makes
        // MINUTE(TIME(0;1;0)) stable without broad millisecond tolerances.
        if compensate && self.coefficient.unsigned_abs().to_string().len() == 28 && !remainder.is_zero() {
            let mut gap = denominator.clone();
            gap.sub(&remainder)?;
            if gap.cmp(&Big::from(43_200)) != Ordering::Greater {
                whole = (whole + 1) % 86_400;
                remainder = Big::zero();
            } else {
                let (half, _) = denominator.div_rem(&Big::from(2))?;
                if remainder.cmp(&half) == Ordering::Less {
                    let mut gap = half.clone();
                    gap.sub(&remainder)?;
                    if gap.cmp(&Big::from(43_200)) != Ordering::Greater {
                        remainder = half;
                    }
                }
            }
        }
        let mut text = String::new();
        text.try_reserve_exact(self.scale as usize).map_err(|_| limit())?;
        if !remainder.is_zero() {
            for at in (0..self.scale as usize).rev() {
                let digit = if at < remainder.len { remainder.digits.get(at).copied().ok_or_else(limit)? } else { 0 };
                text.push(char::from(b'0' + digit));
            }
            while text.ends_with('0') {
                text.pop();
            }
        }
        Ok((
            u32::try_from(whole / 3600).map_err(|_| limit())?,
            u32::try_from((whole / 60) % 60).map_err(|_| limit())?,
            u32::try_from(whole % 60).map_err(|_| limit())?,
            text,
        ))
    }
    pub(crate) fn zero() -> Self {
        Self { coefficient: 0, scale: 0 }
    }
    pub(crate) fn one() -> Self {
        Self { coefficient: 1, scale: 0 }
    }
    pub(crate) fn is_zero(self) -> bool {
        self.coefficient == 0
    }
    pub(crate) fn sign(self) -> i8 {
        if self.coefficient < 0 {
            -1
        } else if self.coefficient > 0 {
            1
        } else {
            0
        }
    }
    pub(crate) fn abs(self) -> Result<Self> {
        Self::normalized_v2(self.coefficient.checked_abs().ok_or_else(limit)?, self.scale)
    }
    pub(crate) fn compare(self, other: Self) -> Ordering {
        if self.sign() != other.sign() {
            return self.sign().cmp(&other.sign());
        }
        if self.is_zero() {
            return Ordering::Equal;
        }
        let left = self.coefficient.unsigned_abs().to_string();
        let right = other.coefficient.unsigned_abs().to_string();
        // Coefficients are normalized and bounded to 28 digits. Compare magnitude
        // exponents, then virtual right-zero-padded digits; no risky rescaling.
        let left_exponent = i64::try_from(left.len()).unwrap_or(i64::MAX) - i64::from(self.scale);
        let right_exponent = i64::try_from(right.len()).unwrap_or(i64::MAX) - i64::from(other.scale);
        let order = left_exponent.cmp(&right_exponent).then_with(|| {
            for at in 0..left.len().max(right.len()) {
                let a = left.as_bytes().get(at).copied().unwrap_or(b'0');
                let b = right.as_bytes().get(at).copied().unwrap_or(b'0');
                if a != b {
                    return a.cmp(&b);
                }
            }
            Ordering::Equal
        });
        if self.sign() < 0 { order.reverse() } else { order }
    }
    pub(crate) fn integer(self) -> Result<i128> {
        if self.scale > 0 {
            return Err(limit());
        }
        self.coefficient.checked_mul(super::power10(-self.scale)?).ok_or_else(limit)
    }
    /// Correctly rounded 28-significant-digit quotient, ties even.
    pub(crate) fn div(self, other: Self) -> Result<Self> {
        if other.is_zero() {
            return Err(limit());
        }
        if self.is_zero() {
            return Ok(Self::zero());
        }
        let a = Big::from(self.coefficient.unsigned_abs());
        let b = Big::from(other.coefficient.unsigned_abs());
        let mut exponent = i32::try_from(a.len).map_err(|_| limit())? - i32::try_from(b.len).map_err(|_| limit())?;
        let mut left = a.clone();
        let mut right = b.clone();
        if exponent >= 0 {
            right.shift(exponent as usize)?;
        } else {
            left.shift((-exponent) as usize)?;
        }
        if left.cmp(&right) == Ordering::Less {
            exponent -= 1;
        }
        exponent = exponent.checked_add(other.scale - self.scale).ok_or_else(limit)?;
        let scale = 27i32.checked_sub(exponent).ok_or_else(limit)?;
        let shift = other.scale.checked_sub(self.scale).and_then(|s| s.checked_add(scale)).ok_or_else(limit)?;
        let mut numerator = a;
        let mut denominator = b;
        if shift >= 0 {
            numerator.shift(shift as usize)?;
        } else {
            denominator.shift((-shift) as usize)?;
        }
        let (quotient, mut remainder) = numerator.div_rem(&denominator)?;
        let mut n = quotient.integer()?;
        remainder.mul_small(2)?;
        let comparison = remainder.cmp(&denominator);
        if comparison == Ordering::Greater || (comparison == Ordering::Equal && n % 2 != 0) {
            n = n.checked_add(1).ok_or_else(limit)?;
        }
        if self.sign() != other.sign() {
            n = n.checked_neg().ok_or_else(limit)?;
        }
        Self::normalized_v2(n, scale)
    }
    /// mode: 0 ties away, 1 away, 2 toward zero, 3 floor.
    pub(crate) fn round(self, digits: i32, mode: u8) -> Result<Self> {
        if !(-124..=124).contains(&digits) {
            return Err(limit());
        }
        if self.scale <= digits {
            return Ok(self);
        }
        let shift = self.scale.checked_sub(digits).ok_or_else(limit)?;
        let denominator = super::power10(shift);
        let (mut quotient, remainder, divisor) = match denominator {
            Ok(divisor) => (self.coefficient / divisor, self.coefficient % divisor, Some(divisor)),
            Err(_) => (0, self.coefficient, None),
        };
        let increment = if remainder == 0 {
            false
        } else {
            match mode {
                0 => divisor.is_some_and(|d| remainder.unsigned_abs() >= (d as u128).div_ceil(2)),
                1 => true,
                2 => false,
                3 => self.sign() < 0,
                _ => return Err(limit()),
            }
        };
        if increment {
            quotient = quotient.checked_add(i128::from(self.sign())).ok_or_else(limit)?;
        }
        Self::normalized_v2(quotient, digits)
    }
    pub(crate) fn modulo(self, divisor: Self) -> Result<Self> {
        if divisor.is_zero() {
            return Err(limit());
        }
        let scale = self.scale.max(divisor.scale);
        let mut a = Big::from(self.coefficient.unsigned_abs());
        a.shift((scale - self.scale) as usize)?;
        let mut b = Big::from(divisor.coefficient.unsigned_abs());
        b.shift((scale - divisor.scale) as usize)?;
        let (_, mut remainder) = a.div_rem(&b)?;
        if !remainder.is_zero() && self.sign() != divisor.sign() {
            let old = remainder;
            remainder = b;
            remainder.sub(&old)?;
        }
        let mut n = remainder.integer()?;
        if divisor.sign() < 0 {
            n = n.checked_neg().ok_or_else(limit)?;
        }
        Self::normalized_v2(n, scale)
    }
    pub(crate) fn pow_integer(self, exponent: i128) -> Result<Self> {
        if !(-4096..=4096).contains(&exponent) || (self.is_zero() && exponent <= 0) {
            return Err(limit());
        }
        let mut power = exponent.unsigned_abs();
        let mut base = self;
        let mut result = Self::one();
        while power != 0 {
            if !power.is_multiple_of(2) {
                result = result.mul_v2(base)?;
            }
            power /= 2;
            if power != 0 {
                base = base.mul_v2(base)?;
            }
        }
        if exponent < 0 { Self::one().div(result) } else { Ok(result) }
    }
    /// Correctly rounded square root via bounded integer decimal digit extraction.
    pub(crate) fn sqrt(self) -> Result<Self> {
        if self.sign() < 0 {
            return Err(limit());
        }
        if self.is_zero() {
            return Ok(self);
        }
        let mut n = Big::from(self.coefficient.unsigned_abs());
        let exponent = i32::try_from(n.len).map_err(|_| limit())? - 1 - self.scale;
        let scale = 27 - exponent.div_euclid(2);
        let shift = scale.checked_mul(2).and_then(|s| s.checked_sub(self.scale)).ok_or_else(limit)?;
        if shift < 0 {
            return Err(limit());
        }
        n.shift(shift as usize)?;
        let (root, remainder) = n.sqrt()?;
        let mut q = root.integer()?;
        // N-q² = r; sqrt(N) > q+1/2 iff 4r > 4q+1. An integer N cannot tie.
        let mut left = remainder;
        left.mul_small(4)?;
        let mut right = root;
        right.mul_small(4)?;
        right.add_small(1)?;
        if left.cmp(&right) == Ordering::Greater {
            q = q.checked_add(1).ok_or_else(limit)?;
        }
        Self::normalized_v2(q, scale)
    }
}
