//! Bounded lexical constants, with no calendar arithmetic or display formatting.

use super::MAX_TEMPORAL_BYTES;

fn token(value: &str) -> Option<&str> {
    (value.len() <= MAX_TEMPORAL_BYTES).then(|| value.trim_matches([' ', '\t', '\r', '\n']))
}

fn digits(value: &str, count: usize) -> Option<u32> {
    if value.len() != count || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// XML Schema 1.0 date/dateTime subset: positive four-digit Gregorian years.
pub(super) fn valid_date(value: &str) -> bool {
    fn date(value: &str) -> Option<()> {
        let year = digits(value.get(..4)?, 4)?;
        if year == 0 || value.get(4..5)? != "-" || value.get(7..8)? != "-" {
            return None;
        }
        let month = digits(value.get(5..7)?, 2)?;
        let day = digits(value.get(8..10)?, 2)?;
        let leap = year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100));
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if leap {
                    29
                } else {
                    28
                }
            }
            _ => return None,
        };
        if day == 0 || day > days {
            return None;
        }
        let zone = value.get(10..)?;
        if zone.is_empty() || zone == "Z" {
            return Some(());
        }
        if !matches!(zone.as_bytes().first(), Some(b'+' | b'-')) || zone.get(3..4)? != ":" {
            return None;
        }
        let hour = digits(zone.get(1..3)?, 2)?;
        let minute = digits(zone.get(4..)?, 2)?;
        (hour <= 14 && minute <= 59 && (hour != 14 || minute == 0)).then_some(())
    }
    let Some(token) = token(value) else { return false };
    if token.contains('T') { crate::datetime::valid_date_time(value) } else { date(token).is_some() }
}

/// XML Schema 1.0 duration. Magnitudes remain digits, never integer/float casts.
pub(super) fn valid_duration(value: &str) -> bool {
    let Some(value) = token(value) else { return false };
    let value = value.strip_prefix('-').unwrap_or(value);
    let Some(value) = value.strip_prefix('P') else { return false };
    let bytes = value.as_bytes();
    let (mut position, mut rank, mut components, mut time_components) = (0usize, 0u8, 0usize, 0usize);
    let mut time = false;
    while position < bytes.len() {
        if bytes.get(position) == Some(&b'T') {
            if time {
                return false;
            }
            time = true;
            rank = 0;
            position += 1;
        }
        let start = position;
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
        if position == start {
            return false;
        }
        let fractional = bytes.get(position) == Some(&b'.');
        if fractional {
            position += 1;
            let start = position;
            while bytes.get(position).is_some_and(u8::is_ascii_digit) {
                position += 1;
            }
            if position == start {
                return false;
            }
        }
        let next_rank = match (time, bytes.get(position)) {
            (false, Some(b'Y')) if !fractional => 1,
            (false, Some(b'M')) if !fractional => 2,
            (false, Some(b'D')) if !fractional => 3,
            (true, Some(b'H')) if !fractional => 1,
            (true, Some(b'M')) if !fractional => 2,
            (true, Some(b'S')) => 3,
            _ => return false,
        };
        if next_rank <= rank {
            return false;
        }
        rank = next_rank;
        components += 1;
        time_components += usize::from(time);
        position += 1;
    }
    components > 0 && (!time || time_components > 0)
}
