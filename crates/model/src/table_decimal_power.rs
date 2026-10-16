//! Certified positive-base fractional power with fixed 96-decimal interval arithmetic.
//! Every rounding is directed. A result is returned only when both enclosing
//! endpoints round to the same representable 28-digit decimal (ties even).
use super::{Big, Decimal, limit};
use crate::{ModelError, Result};
use std::cmp::Ordering;

const PRECISION: usize = 96;
const LOG_STEPS: usize = 1024;
const EXP_STEPS: u32 = 256;
const STEP_FUEL: usize = 262_144;

fn unit() -> Result<Big> {
    let mut n = Big::from(1);
    n.shift(PRECISION)?;
    Ok(n)
}
#[derive(Clone)]
struct Positive {
    lo: Big,
    hi: Big,
}
impl Positive {
    fn sqrt(&self) -> Result<Self> {
        let (lo, _) = self.lo.product(&unit()?)?.sqrt()?;
        let (mut hi, remainder) = self.hi.product(&unit()?)?.sqrt()?;
        if !remainder.is_zero() {
            hi.add_small(1)?;
        }
        Ok(Self { lo, hi })
    }
    fn exact(n: Big) -> Self {
        Self { lo: n.clone(), hi: n }
    }
    fn add(&self, other: &Self) -> Result<Self> {
        Ok(Self { lo: self.lo.add(&other.lo)?, hi: self.hi.add(&other.hi)? })
    }
    fn mul(&self, other: &Self) -> Result<Self> {
        let (lo, _) = self.lo.product(&other.lo)?.drop_digits(PRECISION)?;
        let (mut hi, remainder) = self.hi.product(&other.hi)?.drop_digits(PRECISION)?;
        if remainder {
            hi.add_small(1)?;
        }
        Ok(Self { lo, hi })
    }
    fn div_small(&self, n: u32) -> Result<Self> {
        let divisor = Big::from(u128::from(n));
        let (lo, _) = self.lo.div_rem(&divisor)?;
        let (mut hi, remainder) = self.hi.div_rem(&divisor)?;
        if !remainder.is_zero() {
            hi.add_small(1)?;
        }
        Ok(Self { lo, hi })
    }
    fn reciprocal(&self) -> Result<Self> {
        if self.lo.is_zero() {
            return Err(limit());
        }
        let numerator = unit()?.product(&unit()?)?;
        let (lo, _) = numerator.div_rem(&self.hi)?;
        let (mut hi, remainder) = numerator.div_rem(&self.lo)?;
        if !remainder.is_zero() {
            hi.add_small(1)?;
        }
        Ok(Self { lo, hi })
    }
}
#[derive(Clone)]
struct Signed {
    negative: bool,
    magnitude: Big,
}
impl Signed {
    fn new(negative: bool, magnitude: Big) -> Self {
        Self { negative: negative && !magnitude.is_zero(), magnitude }
    }
    fn neg(&self) -> Self {
        Self::new(!self.negative, self.magnitude.clone())
    }
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => self.magnitude.cmp(&other.magnitude),
            (true, true) => self.magnitude.cmp(&other.magnitude).reverse(),
        }
    }
    fn add(&self, other: &Self) -> Result<Self> {
        if self.negative == other.negative {
            return Ok(Self::new(self.negative, self.magnitude.add(&other.magnitude)?));
        }
        if self.magnitude.cmp(&other.magnitude) != Ordering::Less {
            let mut n = self.magnitude.clone();
            n.sub(&other.magnitude)?;
            Ok(Self::new(self.negative, n))
        } else {
            let mut n = other.magnitude.clone();
            n.sub(&self.magnitude)?;
            Ok(Self::new(other.negative, n))
        }
    }
}
#[derive(Clone)]
struct Interval {
    lo: Signed,
    hi: Signed,
}
impl Interval {
    fn positive(n: Positive) -> Self {
        Self { lo: Signed::new(false, n.lo), hi: Signed::new(false, n.hi) }
    }
    fn add(&self, other: &Self) -> Result<Self> {
        Ok(Self { lo: self.lo.add(&other.lo)?, hi: self.hi.add(&other.hi)? })
    }
    fn scale_integer(&self, n: i32) -> Result<Self> {
        let mut lo = self.lo.clone();
        let mut hi = self.hi.clone();
        lo.magnitude.mul_small(n.unsigned_abs())?;
        hi.magnitude.mul_small(n.unsigned_abs())?;
        if n < 0 { Ok(Self { lo: hi.neg(), hi: lo.neg() }) } else { Ok(Self { lo, hi }) }
    }
    fn multiply_decimal(&self, n: Decimal) -> Result<Self> {
        let endpoint = |value: &Signed, upper: bool| -> Result<Signed> {
            let negative = value.negative ^ (n.sign() < 0);
            let mut magnitude = value.magnitude.product(&Big::from(n.coefficient.unsigned_abs()))?;
            if n.scale <= 0 {
                magnitude.shift(n.scale.unsigned_abs() as usize)?;
            } else {
                let (mut quotient, remainder) = magnitude.drop_digits(n.scale as usize)?;
                if remainder && (upper != negative) {
                    quotient.add_small(1)?;
                }
                magnitude = quotient;
            }
            Ok(Signed::new(negative, magnitude))
        };
        if n.sign() < 0 {
            Ok(Self { lo: endpoint(&self.hi, false)?, hi: endpoint(&self.lo, true)? })
        } else {
            Ok(Self { lo: endpoint(&self.lo, false)?, hi: endpoint(&self.hi, true)? })
        }
    }
}

fn logarithm_mantissa(mantissa: Big, charge: &mut dyn FnMut(usize) -> Result<()>) -> Result<Positive> {
    let one = unit()?;
    if mantissa.cmp(&one) == Ordering::Equal {
        return Ok(Positive::exact(Big::zero()));
    }
    // Four directed square roots reduce m in [1,10] to [1,10^(1/16)].
    // This cuts the atanh series from roughly 550 terms to fewer than 50.
    let mut reduced = Positive::exact(mantissa);
    for _ in 0..4 {
        charge(STEP_FUEL.checked_mul(4).ok_or_else(limit)?)?;
        reduced = reduced.sqrt()?;
    }
    let ratio = |value: &Big, up: bool| -> Result<Big> {
        let mut numerator = value.clone();
        numerator.sub(&one)?;
        numerator.shift(PRECISION)?;
        let denominator = value.add(&one)?;
        let (mut quotient, remainder) = numerator.div_rem(&denominator)?;
        if up && !remainder.is_zero() {
            quotient.add_small(1)?;
        }
        Ok(quotient)
    };
    let lo = ratio(&reduced.lo, false)?;
    let hi = ratio(&reduced.hi, true)?;
    let z = Positive { lo, hi };
    let z2 = z.mul(&z)?;
    let mut term = z.clone();
    let mut sum = z;
    for index in 1..=LOG_STEPS {
        charge(STEP_FUEL)?;
        term = term.mul(&z2)?;
        let denominator = u32::try_from(index).ok().and_then(|n| n.checked_mul(2)).and_then(|n| n.checked_add(1)).ok_or_else(limit)?;
        sum = sum.add(&term.div_small(denominator)?)?;
        if term.hi.cmp(&Big::from(4)) != Ordering::Greater {
            // z <=9/11. Remaining atanh powers have ratio <=81/121,
            // so their tail <=3*current power; ln doubles this bound.
            sum.lo.mul_small(32)?;
            sum.hi.mul_small(32)?;
            let mut tail = term.hi;
            tail.mul_small(96)?;
            sum.hi = sum.hi.add(&tail)?;
            return Ok(sum);
        }
    }
    Err(limit())
}
fn logarithm(value: Decimal, charge: &mut dyn FnMut(usize) -> Result<()>) -> Result<Interval> {
    let coefficient = Big::from(value.coefficient.unsigned_abs());
    let digits = i32::try_from(coefficient.len).map_err(|_| limit())?;
    let exponent = digits - 1 - value.scale;
    let mut mantissa = coefficient;
    mantissa.shift(PRECISION.checked_sub((digits - 1) as usize).ok_or_else(limit)?)?;
    let log_mantissa = Interval::positive(logarithm_mantissa(mantissa, charge)?);
    // floor(ln(10)*10^96), independently enclosed by the exact rational
    // 600-term 2*atanh(9/11) sum and its geometric tail. The proof is in
    // table_decimal_power's test and the B01b report, not a binary-float literal.
    let lo = Big::decimal("2302585092994045684017991454684364207601101488628772976033327900967572609677352480235997205089598")?;
    let mut hi = lo.clone();
    hi.add_small(1)?;
    log_mantissa.add(&Interval::positive(Positive { lo, hi }).scale_integer(exponent)?)
}
fn exponential_positive(value: Positive, charge: &mut dyn FnMut(usize) -> Result<()>) -> Result<Positive> {
    // Reduce to [0,320/512], evaluate exp by positive Taylor terms, then
    // square nine times. Directed intervals bound every rounding operation.
    let reduced = value.div_small(512)?;
    let one = Positive::exact(unit()?);
    let mut term = one.clone();
    let mut sum = one;
    let mut converged = false;
    for n in 1..=EXP_STEPS {
        charge(STEP_FUEL)?;
        term = term.mul(&reduced)?.div_small(n)?;
        sum = sum.add(&term)?;
        if term.hi.cmp(&Big::from(2)) != Ordering::Greater {
            // For n>=1 the remaining successive ratios are <=0.625/2;
            // twice the current term is a conservative tail enclosure.
            let mut tail = term.hi.clone();
            tail.mul_small(2)?;
            sum.hi = sum.hi.add(&tail)?;
            converged = true;
            break;
        }
    }
    if !converged {
        return Err(limit());
    }
    for _ in 0..9 {
        charge(STEP_FUEL)?;
        sum = sum.mul(&sum)?;
    }
    Ok(sum)
}
fn exp_endpoint(value: &Signed, charge: &mut dyn FnMut(usize) -> Result<()>) -> Result<Positive> {
    let positive = exponential_positive(Positive::exact(value.magnitude.clone()), charge)?;
    if value.negative { positive.reciprocal() } else { Ok(positive) }
}
fn rounded(value: &Big) -> Result<Decimal> {
    let remove = value.len.saturating_sub(28);
    let (mut coefficient, _) = value.drop_digits(remove)?;
    if remove != 0 {
        let lead = value.digits.get(remove - 1).copied().ok_or_else(limit)?;
        let further = value.digits.get(..remove - 1).ok_or_else(limit)?.iter().any(|n| *n != 0);
        let odd = coefficient.digits.first().is_some_and(|n| n % 2 != 0);
        if lead > 5 || (lead == 5 && (further || odd)) {
            coefficient.add_small(1)?;
        }
    }
    let scale = i32::try_from(PRECISION).map_err(|_| limit())?.checked_sub(i32::try_from(remove).map_err(|_| limit())?).ok_or_else(limit)?;
    Decimal::normalized_v2(coefficient.integer()?, scale)
}
fn calculate(base: Decimal, exponent: Decimal, charge: &mut dyn FnMut(usize) -> Result<()>) -> Result<Decimal> {
    if base.sign() <= 0 {
        return Err(limit());
    }
    if base == Decimal::one() {
        return Ok(Decimal::one());
    }
    let log = logarithm(base, charge)?.multiply_decimal(exponent)?;
    let mut bound = unit()?;
    bound.mul_small(320)?;
    let positive_bound = Signed::new(false, bound.clone());
    let negative_bound = Signed::new(true, bound);
    if log.lo.cmp(&negative_bound) == Ordering::Less || log.hi.cmp(&positive_bound) == Ordering::Greater {
        return Err(limit());
    }
    let lower = exp_endpoint(&log.lo, charge)?;
    let upper = exp_endpoint(&log.hi, charge)?;
    let a = rounded(&lower.lo)?;
    let b = rounded(&upper.hi)?;
    if a != b {
        return Err(limit());
    }
    Ok(a)
}
impl Decimal {
    /// Outer error = cumulative work failure; None = domain/precision/scale limit.
    /// A numeric limit can be caught by IFERROR, a work failure never can.
    pub(crate) fn pow_fractional(self, exponent: Self, charge: &mut dyn FnMut(usize) -> Result<()>) -> Result<Option<Self>> {
        // Exact powers of ten avoid interval ambiguity at very small, exactly
        // representable results. Prove the exponent product integral before
        // using this path; do not round a nearly integral product into one.
        if self.coefficient == 1 {
            let product = exponent.coefficient.checked_mul(-i128::from(self.scale));
            let integer = product.and_then(|n| {
                if exponent.scale <= 0 {
                    super::super::power10(-exponent.scale).ok().and_then(|factor| n.checked_mul(factor))
                } else {
                    super::super::power10(exponent.scale).ok().and_then(|divisor| if n % divisor == 0 { Some(n / divisor) } else { None })
                }
            });
            if let Some(integer) = integer {
                return Ok(i32::try_from(integer).ok().and_then(|n| n.checked_neg()).and_then(|scale| Self::normalized_v2(1, scale).ok()));
            }
        }
        let mut resource: Option<ModelError> = None;
        let result = calculate(self, exponent, &mut |units| match charge(units) {
            Ok(()) => Ok(()),
            Err(error) => {
                resource = Some(error);
                Err(limit())
            }
        });
        if let Some(error) = resource {
            return Err(error);
        }
        Ok(result.ok())
    }
}
