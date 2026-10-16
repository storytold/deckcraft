//! Bounded XML Schema dateTime profile for editable document creation timestamps.

pub const MAX_DATE_TIME_BYTES: usize = 256;

/// Validate without normalizing lexical data. Extended and negative years remain
/// outside this profile; accepted years are 0001..9999, with Gregorian leap days.
pub fn valid_date_time(value: &str) -> bool {
    fn digits(value: &str, count: usize) -> Option<u32> {
        if value.len() != count || !value.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        value.parse().ok()
    }
    fn parse(value: &str) -> Option<()> {
        let value = value.trim_matches([' ', '\t', '\r', '\n']);
        let (date, time) = value.split_once('T')?;
        let mut date = date.split('-');
        let year = digits(date.next()?, 4)?;
        let month = digits(date.next()?, 2)?;
        let day = digits(date.next()?, 2)?;
        if year == 0 || date.next().is_some() {
            return None;
        }
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
        let time = if let Some(time) = time.strip_suffix('Z') {
            time
        } else if let Some((offset, _)) = time.char_indices().find(|(_, c)| matches!(c, '+' | '-')) {
            let (time, zone) = time.split_at_checked(offset)?;
            let zone = zone.get(1..)?;
            let (hours, minutes) = zone.split_once(':')?;
            let hours = digits(hours, 2)?;
            let minutes = digits(minutes, 2)?;
            if hours > 14 || minutes > 59 || (hours == 14 && minutes != 0) {
                return None;
            }
            time
        } else {
            time
        };
        let mut clock = time.split(':');
        let hour = digits(clock.next()?, 2)?;
        let minute = digits(clock.next()?, 2)?;
        let seconds = clock.next()?;
        let (seconds, fraction) = seconds.split_once('.').map_or((seconds, None), |(s, f)| (s, Some(f)));
        let second = digits(seconds, 2)?;
        if clock.next().is_some() || hour > 24 || minute > 59 || second > 59 {
            return None;
        }
        if fraction.is_some_and(|f| f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit())) {
            return None;
        }
        if hour == 24 && (minute != 0 || second != 0 || fraction.is_some_and(|f| f.bytes().any(|b| b != b'0'))) {
            return None;
        }
        Some(())
    }
    value.len() <= MAX_DATE_TIME_BYTES && parse(value).is_some()
}
