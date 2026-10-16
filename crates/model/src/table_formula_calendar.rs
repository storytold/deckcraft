//! Fixed Gregorian formula epoch, independent of host locale and stored date constants.
use super::{Decimal, invalid};
use crate::Result;

const EPOCH_ORDINAL: i128 = 693_594; // 1899-12-30, with 0001-01-01 = 1.
const LAST_ORDINAL: i128 = 3_652_059; // 9999-12-31.

fn before_year(year: i128) -> i128 {
    let y = year - 1;
    y * 365 + y / 4 - y / 100 + y / 400
}
fn month_days(year: i128, month: i128) -> i128 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 31,
    }
}

pub(crate) fn date_serial(year: Decimal, month: Decimal, day: Decimal) -> Result<Decimal> {
    let year = year.round(0, 2)?.integer()?;
    let month = month.round(0, 2)?.integer()?;
    let day = day.round(0, 2)?.integer()?;
    if !(1..=9999).contains(&year) || !(-120_000..=120_000).contains(&month) || !(-3_652_059..=3_652_059).contains(&day) {
        return Err(invalid("formula date exceeds Gregorian year range"));
    }
    let adjusted = year
        .checked_add((month - 1).div_euclid(12))
        .filter(|y| (1..=9999).contains(y))
        .ok_or_else(|| invalid("formula date month exceeds Gregorian year range"))?;
    let month = (month - 1).rem_euclid(12) + 1;
    let mut ordinal = before_year(adjusted) + day;
    for previous in 1..month {
        ordinal += month_days(adjusted, previous);
    }
    if !(1..=LAST_ORDINAL).contains(&ordinal) {
        return Err(invalid("formula date day exceeds Gregorian year range"));
    }
    Decimal::parse_v2(&(ordinal - EPOCH_ORDINAL).to_string())
}

pub(crate) fn date_parts(serial: Decimal) -> Result<(i128, i128, i128)> {
    let days = serial.round(0, 3)?.integer()?;
    let ordinal = days
        .checked_add(EPOCH_ORDINAL)
        .filter(|n| (1..=LAST_ORDINAL).contains(n))
        .ok_or_else(|| invalid("formula serial date exceeds Gregorian year range"))?;
    let (mut low, mut high) = (1i128, 10_000i128);
    while high - low > 1 {
        let middle = (low + high) / 2;
        if before_year(middle) < ordinal {
            low = middle;
        } else {
            high = middle;
        }
    }
    let year = low;
    let mut remaining = ordinal - before_year(year);
    let mut month = 1;
    while month < 12 && remaining > month_days(year, month) {
        remaining -= month_days(year, month);
        month += 1;
    }
    Ok((year, month, remaining))
}

/// Gregorian date at floor(serial), using the explicit presentation-v2 epoch
/// 1899-12-30 and no fictitious 1900 leap day. Supports years 0001..9999.
pub fn formula_date_from_serial(lexical: &str) -> Result<String> {
    let (year, month, day) = date_parts(Decimal::parse_v2(lexical)?)?;
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

/// Date and exact lexical fractional clock seconds from a numeric day serial.
/// This is a formatting helper; it never changes the authoritative numeric value.
pub fn formula_datetime_from_serial(lexical: &str) -> Result<String> {
    let serial = Decimal::parse_v2(lexical)?;
    let date = formula_date_from_serial(lexical)?;
    let (hour, minute, second, fraction) = serial.day_parts()?;
    let suffix = if fraction.is_empty() { String::new() } else { format!(".{fraction}") };
    Ok(format!("{date}T{hour:02}:{minute:02}:{second:02}{suffix}"))
}

pub(crate) fn serial_from_iso(source: &str) -> Result<Decimal> {
    let text = source.trim_matches([' ', '\t', '\n', '\r']);
    if text.len() > 256 {
        return Err(invalid("formula ISO temporal string exceeds limit"));
    }
    let (date, time) = if text.get(4..5) == Some("-") {
        (crate::table::CellValue::Date { lexical: text.into() }).validate()?;
        let parse = |start, end| text.get(start..end).ok_or_else(|| invalid("missing ISO date component")).and_then(Decimal::parse_v2);
        let date = date_serial(parse(0, 4)?, parse(5, 7)?, parse(8, 10)?)?;
        (date, text.split_once('T').map(|(_, time)| time))
    } else {
        (Decimal::zero(), Some(text))
    };
    let Some(time) = time else { return Ok(date) };
    // An authored timezone is retained as wall-clock fields, never converted by
    // the host. Source dateTime validation above authenticates its zone spelling.
    let time = time.strip_suffix('Z').unwrap_or(time);
    let end = time.char_indices().find(|(at, c)| *at >= 8 && matches!(c, '+' | '-')).map_or(time.len(), |(at, _)| at);
    let clock = time.get(..end).ok_or_else(|| invalid("invalid ISO clock boundary"))?;
    if end != time.len() {
        let zone = time.get(end..).ok_or_else(|| invalid("invalid ISO zone boundary"))?;
        let digits = |part: Option<&str>| part.filter(|s| s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit())).and_then(|s| s.parse::<u32>().ok());
        let (Some(hour), Some(minute)) = (digits(zone.get(1..3)), digits(zone.get(4..6))) else {
            return Err(invalid("invalid ISO clock zone"));
        };
        if zone.len() != 6 || zone.get(3..4) != Some(":") || hour > 14 || minute > 59 || (hour == 14 && minute != 0) {
            return Err(invalid("invalid ISO clock zone"));
        }
    }
    let fields: Vec<&str> = clock.split(':').collect();
    if fields.len() != 3
        || !fields.first().is_some_and(|s| s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit()))
        || !fields.get(1).is_some_and(|s| s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit()))
        || !fields.get(2).is_some_and(|s| {
            let mut parts = s.split('.');
            let integer = parts.next().unwrap_or("");
            let fraction = parts.next();
            integer.len() == 2
                && integer.bytes().all(|b| b.is_ascii_digit())
                && fraction.is_none_or(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                && parts.next().is_none()
        })
    {
        return Err(invalid("invalid ISO clock lexical"));
    }

    let mut parts = clock.split(':');
    let hour = Decimal::parse_v2(parts.next().ok_or_else(|| invalid("missing ISO hour"))?)?;
    let minute = Decimal::parse_v2(parts.next().ok_or_else(|| invalid("missing ISO minute"))?)?;
    let second = Decimal::parse_v2(parts.next().ok_or_else(|| invalid("missing ISO second"))?)?;
    if parts.next().is_some()
        || hour.sign() < 0
        || minute.sign() < 0
        || second.sign() < 0
        || hour.compare(Decimal::parse_v2("24")?) == std::cmp::Ordering::Greater
        || minute.compare(Decimal::parse_v2("60")?) != std::cmp::Ordering::Less
        || second.compare(Decimal::parse_v2("60")?) != std::cmp::Ordering::Less
        || hour.integer().is_err()
        || minute.integer().is_err()
    {
        return Err(invalid("invalid ISO clock component"));
    }
    if hour == Decimal::parse_v2("24")? && (!minute.is_zero() || !second.is_zero()) {
        return Err(invalid("ISO end-of-day clock must be midnight"));
    }
    let seconds = hour.mul_v2(Decimal::parse_v2("3600")?)?.add_v2(minute.mul_v2(Decimal::parse_v2("60")?)?)?.add_v2(second)?;
    // Divide only after adding the integer civil date in seconds. This avoids
    // adding a rounded 28-digit day fraction to a multi-digit date serial.
    date.mul_v2(Decimal::parse_v2("86400")?)?.add_v2(seconds)?.div(Decimal::parse_v2("86400")?)
}
