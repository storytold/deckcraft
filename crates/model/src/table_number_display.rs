use std::cmp::Ordering;

use super::{DateFormatPart, NumberComparison, NumberLocale, NumberStyle, invalid};
use crate::{Result, table::CellValue};

/// Decimal lexical expansion without a binary floating-point conversion.
pub(super) fn canonical(source: &str) -> Result<String> {
    if source.len() > 1024 || !source.is_ascii() {
        return Err(invalid("number-format input exceeds lexical limit"));
    }
    let source = source.trim();
    let negative = source.starts_with('-');
    let unsigned = source.strip_prefix(['-', '+']).unwrap_or(source);
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse::<i32>().map_err(|_| invalid("invalid number-format exponent"))?),
        None => (unsigned, 0),
    };
    if !(-512..=512).contains(&exponent) {
        return Err(invalid("number-format exponent exceeds limit"));
    }
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty() && fraction.is_empty() || !whole.bytes().chain(fraction.bytes()).all(|b| b.is_ascii_digit()) {
        return Err(invalid("invalid number-format decimal"));
    }
    let mut digits = format!("{whole}{fraction}");
    let leading = digits.bytes().take_while(|b| *b == b'0').count();
    if leading == digits.len() {
        return Ok("0".into());
    }
    digits.drain(..leading);
    let point = i32::try_from(whole.len())
        .map_err(|_| invalid("number-format decimal exceeds limit"))?
        .checked_sub(i32::try_from(leading).map_err(|_| invalid("number-format decimal exceeds limit"))?)
        .and_then(|n| n.checked_add(exponent))
        .ok_or_else(|| invalid("number-format decimal exceeds limit"))?;
    let mut out = String::new();
    out.try_reserve_exact(1024).map_err(|_| invalid("cannot allocate number-format decimal"))?;
    if negative {
        out.push('-');
    }
    if point <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', point.unsigned_abs() as usize));
        out.push_str(&digits);
    } else {
        let point = usize::try_from(point).map_err(|_| invalid("invalid number-format decimal point"))?;
        if point >= digits.len() {
            out.push_str(&digits);
            out.extend(std::iter::repeat_n('0', point - digits.len()));
        } else {
            let (a, b) = digits.split_at(point);
            out.push_str(a);
            out.push('.');
            out.push_str(b);
        }
    }
    if out.contains('.') {
        let trim = out.trim_end_matches('0').trim_end_matches('.').len();
        out.truncate(trim);
    }
    if out.len() > 1024 {
        return Err(invalid("number-format decimal output exceeds limit"));
    }
    Ok(out)
}

pub(super) fn comparison(left: &str, right: &str, op: NumberComparison) -> Result<bool> {
    let left = canonical(left)?;
    let right = canonical(right)?;
    let a = left.trim_start_matches('-');
    let b = right.trim_start_matches('-');
    let order = if left.starts_with('-') != right.starts_with('-') {
        if left.starts_with('-') { Ordering::Less } else { Ordering::Greater }
    } else {
        let (ai, af) = a.split_once('.').unwrap_or((a, ""));
        let (bi, bf) = b.split_once('.').unwrap_or((b, ""));
        let order = ai.len().cmp(&bi.len()).then_with(|| ai.cmp(bi)).then_with(|| {
            let n = af.len().max(bf.len());
            af.bytes().chain(std::iter::repeat(b'0')).take(n).cmp(bf.bytes().chain(std::iter::repeat(b'0')).take(n))
        });
        if left.starts_with('-') { order.reverse() } else { order }
    };
    Ok(match op {
        NumberComparison::Less => order.is_lt(),
        NumberComparison::LessEqual => !order.is_gt(),
        NumberComparison::Equal => order.is_eq(),
        NumberComparison::NotEqual => !order.is_eq(),
        NumberComparison::GreaterEqual => !order.is_lt(),
        NumberComparison::Greater => order.is_gt(),
    })
}

fn increment(digits: &mut Vec<u8>) {
    for digit in digits.iter_mut().rev() {
        if *digit != b'9' {
            *digit += 1;
            return;
        }
        *digit = b'0';
    }
    digits.insert(0, b'1');
}

/// Display rounding is decimal, ties away from zero; it never changes the stored value.
fn rounded(source: &str, places: u8, minimum: u8, integer_digits: u8, grouping: bool, locale: NumberLocale) -> Result<String> {
    let source = canonical(source)?;
    let negative = source.starts_with('-');
    let unsigned = source.trim_start_matches('-');
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let places = usize::from(places);
    let mut digits = Vec::new();
    digits.try_reserve_exact(whole.len().saturating_add(places).saturating_add(1)).map_err(|_| invalid("cannot allocate rounded number"))?;
    digits.extend_from_slice(whole.as_bytes());
    digits.extend(fraction.bytes().chain(std::iter::repeat(b'0')).take(places));
    if fraction.as_bytes().get(places).is_some_and(|byte| *byte >= b'5') {
        increment(&mut digits);
    }
    let integer_len = digits.len().saturating_sub(places);
    let (integer, fraction) = digits.split_at(integer_len);
    let integer = std::str::from_utf8(integer).map_err(|_| invalid("invalid rounded decimal"))?;
    let fraction = std::str::from_utf8(fraction).map_err(|_| invalid("invalid rounded decimal"))?;
    let visible = fraction.trim_end_matches('0').len().max(usize::from(minimum));
    let mut integer = format!("{:0>width$}", integer, width = usize::from(integer_digits).max(1));
    let (decimal, separator) = locale.separators();
    if grouping {
        let mut grouped = String::new();
        grouped.try_reserve_exact(integer.len().saturating_mul(2)).map_err(|_| invalid("cannot allocate grouped number"))?;
        for (i, digit) in integer.chars().enumerate() {
            if i > 0 && (integer.len() - i).is_multiple_of(3) {
                grouped.push(separator);
            }
            grouped.push(digit);
        }
        integer = grouped;
    }
    let is_zero = digits.iter().all(|digit| *digit == b'0');
    let mut text = String::new();
    if negative && !is_zero {
        text.push('-');
    }
    text.push_str(&integer);
    if visible > 0 {
        text.push(decimal);
        text.push_str(fraction.get(..visible).ok_or_else(|| invalid("rounded fractional width exceeds limit"))?);
    }
    Ok(text)
}

fn shifted(source: &str, count: i32) -> Result<String> {
    let source = canonical(source)?;
    canonical(&format!("{source}e{count}"))
}

fn scientific(source: &str, places: u8, exponent_digits: u8, exponent_sign: bool, locale: NumberLocale) -> Result<String> {
    let number = canonical(source)?;
    let sign = if number.starts_with('-') { "-" } else { "" };
    let unsigned = number.trim_start_matches('-');
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let all = format!("{integer}{fraction}");
    let leading = all.bytes().take_while(|c| *c == b'0').count();
    let digits = all.get(leading..).ok_or_else(|| invalid("invalid scientific number"))?;
    let mut exponent = if digits.is_empty() { 0 } else { integer.len() as i32 - leading as i32 - 1 };
    let mantissa = if digits.is_empty() {
        "0".into()
    } else {
        format!("{}.{}", digits.get(..1).ok_or_else(|| invalid("invalid scientific number"))?, digits.get(1..).unwrap_or(""))
    };
    let mut out = rounded(&mantissa, places, places, 1, false, locale)?;
    if out.starts_with("10") {
        exponent += 1;
        out = rounded("1", places, places, 1, false, locale)?;
    }
    let exponent_sign = if exponent < 0 {
        "-"
    } else if exponent_sign {
        "+"
    } else {
        ""
    };
    Ok(format!("{sign}{out}E{exponent_sign}{:0>width$}", exponent.unsigned_abs(), width = usize::from(exponent_digits)))
}

fn fraction(source: &str, digits: u8, fixed: Option<u32>, locale: NumberLocale) -> Result<String> {
    let canonical = canonical(source)?;
    let sign = if canonical.starts_with('-') { "-" } else { "" };
    let unsigned = canonical.trim_start_matches('-');
    let (whole, fractional) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let (mut n, d) = if let Some(d) = fixed {
        let scaled = multiply_digits(fractional, d)?;
        let split = scaled.len().saturating_sub(fractional.len());
        let (integer, rest) = scaled.split_at(split);
        let n = if integer.is_empty() { 0 } else { numeric(integer)? };
        // Left-zero padding can be implicit only when scaled has fewer digits
        // than the original fractional width; in that case its first digit is 0.
        let up = scaled.len() >= fractional.len() && rest.as_bytes().first().is_some_and(|b| *b >= b'5');
        (n + u32::from(up), d)
    } else {
        closest_fraction_lexical(fractional, 10u32.pow(u32::from(digits)) - 1)?
    };
    let mut whole = whole.to_string();
    if n >= d {
        let mut bytes = whole.into_bytes();
        increment(&mut bytes);
        whole = String::from_utf8(bytes).map_err(|_| invalid("invalid fraction whole number"))?;
        n -= d;
    }
    let whole = if n == 0 || whole != "0" { whole } else { String::new() };
    let gap = if !whole.is_empty() && n > 0 { " " } else { "" };
    let frac = if n == 0 { String::new() } else { format!("{n}/{d}") };
    let _ = locale;
    let sign = if whole == "0" && n == 0 { "" } else { sign };
    Ok(format!("{sign}{whole}{gap}{frac}"))
}

fn digit_order(left: &str, right: &str) -> Ordering {
    let left = left.trim_start_matches('0');
    let right = right.trim_start_matches('0');
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}
fn digit_difference(left: &str, right: &str) -> Result<String> {
    let (greater, lesser) = if digit_order(left, right).is_lt() { (right, left) } else { (left, right) };
    let mut out = Vec::new();
    out.try_reserve_exact(greater.len()).map_err(|_| invalid("cannot allocate fraction difference"))?;
    let mut less = lesser.bytes().rev();
    let mut borrow = 0i16;
    for a in greater.bytes().rev() {
        let b = less.next().unwrap_or(b'0');
        let value = i16::from(a) - i16::from(b) - borrow;
        borrow = i16::from(value < 0);
        out.push(b'0' + if value < 0 { (value + 10) as u8 } else { value as u8 });
    }
    out.reverse();
    String::from_utf8(out).map_err(|_| invalid("invalid fraction difference"))
}
/// Find adjacent Stern-Brocot bounds with denominators <= cap. Bisection
/// skips an entire run of equal turns. Decimal comparisons retain every lexical
/// digit; no precision-sized coefficient or rounded proxy is involved.
fn closest_fraction_lexical(fractional: &str, cap: u32) -> Result<(u32, u32)> {
    if fractional.bytes().all(|b| b == b'0') {
        return Ok((0, 1));
    }
    let cross =
        |p: u32, q: u32| -> Result<(String, String)> { Ok((multiply_digits(fractional, q)?, format!("{p}{}", "0".repeat(fractional.len())))) };
    let compare = |p, q| -> Result<Ordering> {
        let (a, b) = cross(p, q)?;
        Ok(digit_order(&a, &b))
    };
    let (mut lp, mut lq, mut rp, mut rq) = (0u32, 1u32, 1u32, 1u32);
    // With cap <=9999 each accelerated turn replaces a denominator by at
    // least the sum of the old pair. 128 is a conservative fixed limit.
    for _ in 0..128 {
        if lq + rq > cap {
            let (la, lb) = cross(lp, lq)?;
            let (ra, rb) = cross(rp, rq)?;
            let left = multiply_digits(&digit_difference(&la, &lb)?, rq)?;
            let right = multiply_digits(&digit_difference(&ra, &rb)?, lq)?;
            return Ok(match digit_order(&left, &right) {
                Ordering::Less => (lp, lq),
                Ordering::Greater => (rp, rq),
                Ordering::Equal if lq <= rq => (lp, lq),
                Ordering::Equal => (rp, rq),
            });
        }
        let order = compare(lp + rp, lq + rq)?;
        if order.is_eq() {
            return Ok((lp + rp, lq + rq));
        }
        let low_side = order.is_gt();
        let (base_p, base_q, step_p, step_q) = if low_side { (lp, lq, rp, rq) } else { (rp, rq, lp, lq) };
        let (mut low, mut high) = (1u32, (cap - base_q) / step_q);
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            let order = compare(base_p + middle * step_p, base_q + middle * step_q)?;
            if order.is_eq() {
                return Ok((base_p + middle * step_p, base_q + middle * step_q));
            }
            if order.is_gt() == low_side {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        if low_side {
            lp += low * rp;
            lq += low * rq;
        } else {
            rp += low * lp;
            rq += low * lq;
        }
    }
    Err(invalid("fraction approximation exceeds bounded work"))
}

pub(super) fn format(style: &NumberStyle, locale: NumberLocale, value: &CellValue) -> Result<String> {
    match style {
        NumberStyle::General | NumberStyle::Text => match value {
            CellValue::String { value } => Ok(value.clone()),
            CellValue::Boolean { value } => Ok(if *value { "TRUE" } else { "FALSE" }.into()),
            CellValue::Number { lexical, .. } => {
                let number = canonical(lexical)?;
                Ok(number.replace('.', &locale.separators().0.to_string()))
            }
            CellValue::Date { lexical } | CellValue::Duration { lexical } => Ok(lexical.clone()),
        },
        NumberStyle::Boolean { true_text, false_text } => match value {
            CellValue::Boolean { value } => Ok(if *value { true_text } else { false_text }.clone()),
            _ => Err(invalid("Boolean format requires a Boolean value")),
        },
        NumberStyle::DateTime { parts } => match value {
            CellValue::Date { lexical } => date_time(lexical, parts, locale),
            CellValue::Number { lexical, .. } => date_time(&serial_datetime_lexical(lexical)?, parts, locale),
            _ => Err(invalid("date/time format requires a date or dateTime value")),
        },
        NumberStyle::Duration { parts } => match value {
            CellValue::Duration { lexical } => duration(lexical, parts, locale),
            _ => Err(invalid("duration format requires a duration value")),
        },
        _ => {
            let CellValue::Number { lexical, .. } = value else {
                return Err(invalid("numeric format requires a stored or computed number"));
            };
            match style {
                NumberStyle::Number { decimal_places, min_decimal_places, min_integer_digits, grouping } => {
                    rounded(lexical, *decimal_places, *min_decimal_places, *min_integer_digits, *grouping, locale)
                }
                NumberStyle::Percentage { decimal_places, min_decimal_places, grouping } => {
                    Ok(format!("{}%", rounded(&shifted(lexical, 2)?, *decimal_places, *min_decimal_places, 1, *grouping, locale)?))
                }
                NumberStyle::Currency { decimal_places, min_decimal_places, grouping, code, symbol, symbol_after, space } => {
                    let number = rounded(lexical, *decimal_places, *min_decimal_places, 1, *grouping, locale)?;
                    let (sign, magnitude) = number.strip_prefix('-').map_or(("", number.as_str()), |m| ("-", m));
                    let label = if symbol.is_empty() { code } else { symbol };
                    let gap = if *space { " " } else { "" };
                    Ok(if *symbol_after { format!("{sign}{magnitude}{gap}{label}") } else { format!("{sign}{label}{gap}{magnitude}") })
                }
                NumberStyle::Scientific { decimal_places, min_exponent_digits, exponent_sign } => {
                    scientific(lexical, *decimal_places, *min_exponent_digits, *exponent_sign, locale)
                }
                NumberStyle::Fraction { denominator_digits, fixed_denominator } => fraction(lexical, *denominator_digits, *fixed_denominator, locale),
                _ => Err(invalid("incompatible number format")),
            }
        }
    }
}

fn numeric(text: &str) -> Result<u32> {
    text.parse::<u32>().map_err(|_| invalid("invalid date/time format input"))
}

// Formatting retains every original scalar digit, independently of formula precision.
fn serial_datetime_lexical(source: &str) -> Result<String> {
    let value = canonical(source)?;
    let negative = value.starts_with('-');
    let (whole, fraction) = value.trim_start_matches('-').split_once('.').unwrap_or((value.trim_start_matches('-'), ""));
    let mut day = whole.parse::<i32>().map_err(|_| invalid("numeric date exceeds Gregorian range"))?;
    let nonzero = fraction.bytes().any(|b| b != b'0');
    let fraction = if negative && nonzero {
        digit_difference(&format!("1{}", "0".repeat(fraction.len())), fraction)?
            .get(1..)
            .ok_or_else(|| invalid("invalid serial complement"))?
            .to_string()
    } else {
        fraction.into()
    };
    if negative {
        day = day.checked_neg().and_then(|day| day.checked_sub(i32::from(nonzero))).ok_or_else(|| invalid("numeric date exceeds Gregorian range"))?;
    }
    let date = crate::table::formula_date_from_serial(&day.to_string())?;
    let scaled = multiply_digits(&fraction, 86400)?;
    let split = scaled.len().saturating_sub(fraction.len());
    let seconds = if split == 0 { 0 } else { numeric(scaled.get(..split).ok_or_else(|| invalid("invalid serial clock"))?)? };
    let rest = scaled.get(split..).ok_or_else(|| invalid("invalid serial fraction"))?;
    let rest = format!("{}{}", "0".repeat(fraction.len().saturating_sub(rest.len())), rest);
    let rest = rest.trim_end_matches('0');
    Ok(format!(
        "{date}T{:02}:{:02}:{:02}{}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        if rest.is_empty() { String::new() } else { format!(".{rest}") }
    ))
}
fn clock_precision(parts: &[DateFormatPart]) -> Option<u8> {
    parts
        .iter()
        .filter_map(|part| match part {
            DateFormatPart::Seconds { decimal_places, .. } => Some(*decimal_places),
            _ => None,
        })
        .min()
}
fn next_civil_day(year: &mut u32, month: &mut u32, day: &mut u32) -> Result<()> {
    let leap = (*year).is_multiple_of(4) && (!(*year).is_multiple_of(100) || (*year).is_multiple_of(400));
    let days = match *month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    *day += 1;
    if *day > days {
        *day = 1;
        *month += 1;
    }
    if *month > 12 {
        *month = 1;
        *year = year.checked_add(1).filter(|year| *year <= 9999).ok_or_else(|| invalid("rounded date exceeds Gregorian range"))?;
    }
    Ok(())
}

fn date_time(lexical: &str, parts: &[DateFormatPart], locale: NumberLocale) -> Result<String> {
    let lexical = lexical.trim();
    let mut year = numeric(lexical.get(..4).ok_or_else(|| invalid("invalid date format year"))?)?;
    let mut month = numeric(lexical.get(5..7).ok_or_else(|| invalid("invalid date format month"))?)?;
    let mut day = numeric(lexical.get(8..10).ok_or_else(|| invalid("invalid date format day"))?)?;
    let tail = lexical.get(10..).unwrap_or("");
    let (mut hour, mut minute, second, zone) = if let Some(time) = tail.strip_prefix('T') {
        let zone_start = time.char_indices().skip(8).find(|(_, c)| matches!(c, 'Z' | '+' | '-')).map(|x| x.0).unwrap_or(time.len());
        let time_only = time.get(..zone_start).ok_or_else(|| invalid("invalid dateTime zone"))?;
        (
            numeric(time_only.get(..2).ok_or_else(|| invalid("invalid dateTime hour"))?)?,
            numeric(time_only.get(3..5).ok_or_else(|| invalid("invalid dateTime minute"))?)?,
            time_only.get(6..).ok_or_else(|| invalid("invalid dateTime seconds"))?,
            time.get(zone_start..).unwrap_or(""),
        )
    } else {
        (0, 0, "0", tail)
    };
    let mut second = second.to_string();
    if let Some(precision) = clock_precision(parts) {
        second = rounded(&second, precision, precision, 1, false, NumberLocale::EnUs)?;
        if second.split('.').next() == Some("60") {
            second = rounded("0", precision, precision, 1, false, NumberLocale::EnUs)?;
            minute += 1;
        }
        if minute >= 60 {
            minute -= 60;
            hour += 1;
        }
        if hour >= 24 {
            hour -= 24;
            next_civil_day(&mut year, &mut month, &mut day)?;
        }
    }
    let weekday = weekday(year, month, day)?;
    let am_pm = parts.iter().any(|p| matches!(p, DateFormatPart::AmPm));
    let mut out = String::new();
    for part in parts {
        let text = match part {
            DateFormatPart::Literal { text } => text.clone(),
            DateFormatPart::Year { long } => {
                if *long {
                    format!("{year:04}")
                } else {
                    format!("{:02}", year % 100)
                }
            }
            DateFormatPart::Month { long, textual: true } => month_name(locale, month, *long)?.into(),
            DateFormatPart::Month { long, textual: false } => padded(month, *long),
            DateFormatPart::Day { long } => padded(day, *long),
            DateFormatPart::DayOfWeek { long } => day_name(locale, weekday, *long)?.into(),
            DateFormatPart::Hours { long, elapsed: false } => padded(if am_pm { if hour % 12 == 0 { 12 } else { hour % 12 } } else { hour }, *long),
            DateFormatPart::Hours { elapsed: true, .. } => {
                return Err(invalid("elapsed hours require a duration value"));
            }
            DateFormatPart::Minutes { long } => padded(minute, *long),
            DateFormatPart::Seconds { long, decimal_places } => {
                rounded(&second, *decimal_places, *decimal_places, if *long { 2 } else { 1 }, false, locale)?
            }
            DateFormatPart::AmPm => {
                if hour < 12 {
                    "AM".into()
                } else {
                    "PM".into()
                }
            }
            DateFormatPart::Zone => zone.into(),
        };
        append(&mut out, &text)?;
    }
    Ok(out)
}

fn append(out: &mut String, text: &str) -> Result<()> {
    if out.len().checked_add(text.len()).is_none_or(|n| n > 4096) {
        return Err(invalid("formatted date/time exceeds output limit"));
    }
    out.try_reserve(text.len()).map_err(|_| invalid("cannot allocate formatted date/time"))?;
    out.push_str(text);
    Ok(())
}

fn padded(number: u32, long: bool) -> String {
    if long { format!("{number:02}") } else { number.to_string() }
}

fn weekday(year: u32, month: u32, day: u32) -> Result<usize> {
    let offsets = [0u32, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let year = if month < 3 { year.checked_sub(1).ok_or_else(|| invalid("invalid Gregorian year"))? } else { year };
    let offset = offsets.get(month.saturating_sub(1) as usize).ok_or_else(|| invalid("invalid Gregorian month"))?;
    Ok(((year + year / 4 - year / 100 + year / 400 + offset + day) % 7) as usize)
}

fn month_name(locale: NumberLocale, month: u32, long: bool) -> Result<&'static str> {
    let names = match (locale, long) {
        (NumberLocale::EnUs, true) => {
            ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"]
        }
        (NumberLocale::EnUs, false) => ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
        (NumberLocale::FrBe, true) => {
            ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"]
        }
        (NumberLocale::FrBe, false) => ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."],
        (NumberLocale::DeDe, true) => {
            ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"]
        }
        (NumberLocale::DeDe, false) => ["Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.", "Dez."],
    };
    names.get(month.saturating_sub(1) as usize).copied().ok_or_else(|| invalid("invalid date format month"))
}

fn day_name(locale: NumberLocale, day: usize, long: bool) -> Result<&'static str> {
    let names = match (locale, long) {
        (NumberLocale::EnUs, true) => ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"],
        (NumberLocale::EnUs, false) => ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
        (NumberLocale::FrBe, true) => ["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"],
        (NumberLocale::FrBe, false) => ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."],
        (NumberLocale::DeDe, true) => ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"],
        (NumberLocale::DeDe, false) => ["So.", "Mo.", "Di.", "Mi.", "Do.", "Fr.", "Sa."],
    };
    names.get(day).copied().ok_or_else(|| invalid("invalid weekday"))
}

fn duration(lexical: &str, parts: &[DateFormatPart], locale: NumberLocale) -> Result<String> {
    let negative = lexical.trim().starts_with('-');
    let token = lexical.trim().trim_start_matches('-').strip_prefix('P').ok_or_else(|| invalid("invalid duration format input"))?;
    let mut components = ["0"; 6];
    let mut time = false;
    let mut at = 0usize;
    while at < token.len() {
        if token.as_bytes().get(at) == Some(&b'T') {
            time = true;
            at += 1;
        }
        let start = at;
        while token.as_bytes().get(at).is_some_and(|b| b.is_ascii_digit() || *b == b'.') {
            at += 1;
        }
        let magnitude = token.get(start..at).ok_or_else(|| invalid("invalid duration magnitude"))?;
        let index = match (time, token.as_bytes().get(at)) {
            (false, Some(b'Y')) => 0,
            (false, Some(b'M')) => 1,
            (false, Some(b'D')) => 2,
            (true, Some(b'H')) => 3,
            (true, Some(b'M')) => 4,
            (true, Some(b'S')) => 5,
            _ => return Err(invalid("invalid duration component")),
        };
        let slot = components.get_mut(index).ok_or_else(|| invalid("invalid duration component"))?;
        *slot = magnitude;
        at += 1;
    }
    let get = |index: usize| components.get(index).copied().ok_or_else(|| invalid("invalid duration component"));
    // Arbitrary calendar years/months cannot be converted into elapsed hours without a date.
    if parts.iter().any(|p| matches!(p, DateFormatPart::Hours { .. } | DateFormatPart::Minutes { .. } | DateFormatPart::Seconds { .. }))
        && [get(0)?, get(1)?].iter().any(|v| !v.trim_start_matches('0').is_empty())
    {
        return Err(invalid("calendar duration requires an anchor date before clock formatting"));
    }
    let seconds = if let Some(precision) = clock_precision(parts) {
        rounded(get(5)?, precision, precision, 1, false, NumberLocale::EnUs)?
    } else {
        get(5)?.to_string()
    };
    let (seconds_whole, seconds_fraction) = seconds.split_once('.').unwrap_or((seconds.as_str(), ""));
    let mut total = multiply_digits(get(2)?, 86400)?;
    total = add_digits(&total, &multiply_digits(get(3)?, 3600)?)?;
    total = add_digits(&total, &multiply_digits(get(4)?, 60)?)?;
    total = add_digits(&total, seconds_whole)?;
    let (minutes, seconds) = divide_digits(&total, 60)?;
    let (hours, minutes) = divide_digits(&minutes, 60)?;
    let (_, clock_hours) = divide_digits(&hours, 24)?;
    let mut out = if negative { "-".into() } else { String::new() };
    for part in parts {
        let text = match part {
            DateFormatPart::Literal { text } => text.clone(),
            DateFormatPart::Year { long } => padded(numeric(get(0)?)?, *long),
            DateFormatPart::Month { long, textual: false } => padded(numeric(get(1)?)?, *long),
            DateFormatPart::Day { long } => format!("{:0>width$}", get(2)?.trim_start_matches('0'), width = if *long { 2 } else { 1 }),
            DateFormatPart::Hours { long, elapsed: true } => format!("{:0>width$}", hours, width = if *long { 2 } else { 1 }),
            DateFormatPart::Hours { long, elapsed: false } => padded(clock_hours, *long),
            DateFormatPart::Minutes { long } => padded(minutes, *long),
            DateFormatPart::Seconds { long, decimal_places } => {
                let lexical = if seconds_fraction.is_empty() { seconds.to_string() } else { format!("{seconds}.{seconds_fraction}") };
                rounded(&lexical, *decimal_places, *decimal_places, if *long { 2 } else { 1 }, false, locale)?
            }
            _ => return Err(invalid("incompatible calendar part in duration format")),
        };
        append(&mut out, &text)?;
    }
    Ok(out)
}

fn multiply_digits(source: &str, factor: u32) -> Result<String> {
    let mut out = Vec::new();
    let mut carry = 0u32;
    for digit in source.bytes().rev() {
        if !digit.is_ascii_digit() {
            return Err(invalid("invalid duration magnitude"));
        }
        let next = u32::from(digit - b'0') * factor + carry;
        out.push(b'0' + (next % 10) as u8);
        carry = next / 10;
    }
    while carry > 0 {
        out.push(b'0' + (carry % 10) as u8);
        carry /= 10;
    }
    out.reverse();
    String::from_utf8(out).map_err(|_| invalid("invalid duration magnitude"))
}

fn add_digits(left: &str, right: &str) -> Result<String> {
    let mut left = left.bytes().rev();
    let mut right = right.bytes().rev();
    let mut carry = 0u8;
    let mut out = Vec::new();
    loop {
        let a = left.next();
        let b = right.next();
        if a.is_none() && b.is_none() && carry == 0 {
            break;
        }
        let digit = |d: Option<u8>| -> Result<u8> {
            match d {
                None => Ok(0),
                Some(b'0'..=b'9') => Ok(d.unwrap_or(b'0') - b'0'),
                _ => Err(invalid("invalid duration magnitude")),
            }
        };
        let next = digit(a)? + digit(b)? + carry;
        out.push(b'0' + next % 10);
        carry = next / 10;
    }
    out.reverse();
    String::from_utf8(out).map_err(|_| invalid("invalid duration magnitude"))
}

fn divide_digits(source: &str, divisor: u32) -> Result<(String, u32)> {
    if divisor == 0 {
        return Err(invalid("invalid duration scale"));
    }
    let mut out = String::new();
    let mut remainder = 0u32;
    for digit in source.bytes() {
        if !digit.is_ascii_digit() {
            return Err(invalid("invalid duration magnitude"));
        }
        let value = remainder * 10 + u32::from(digit - b'0');
        out.push(char::from(b'0' + (value / divisor) as u8));
        remainder = value % divisor;
    }
    let out = out.trim_start_matches('0');
    Ok((if out.is_empty() { "0".into() } else { out.into() }, remainder))
}
