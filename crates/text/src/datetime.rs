//! Date and time fields (`a:fld type="datetime…"`): the text each field type shows for a moment,
//! and the local clock.
//!
//! The field types and their formats follow ECMA-376 Part 1, §21.1.2.2.4 (`fld`): `datetime1`
//! to `datetime13`, written as in PowerPoint's English (United States) Date and Time list.
//! `datetime` and `datetimeFigureOut` show the language's short date. Numeric dates follow the
//! run's language; formats with month or day names, or AM/PM, are computed for English and
//! Japanese, so other languages keep the text the file was saved with.

/// A local date and time: what the clock read, or a fixed moment (tests, deterministic output).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateTime {
    pub year: i32,
    /// 1–12.
    pub month: u32,
    /// 1–31.
    pub day: u32,
    /// 0–23.
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl DateTime {
    pub fn new(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> Self {
        DateTime { year, month, day, hour, minute, second }
    }

    /// The local date and time now (the user's time zone); `None` where there is no clock (wasm).
    pub fn now_local() -> Option<DateTime> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use chrono::{Datelike, Timelike};
            let n = chrono::Local::now();
            Some(DateTime::new(n.year(), n.month(), n.day(), n.hour(), n.minute(), n.second()))
        }
        #[cfg(target_arch = "wasm32")]
        {
            None
        }
    }

    /// A real calendar date and time of day.
    fn is_valid(&self) -> bool {
        let date_ok = (1..=9999).contains(&self.year) && chrono::NaiveDate::from_ymd_opt(self.year, self.month, self.day).is_some();
        date_ok && self.hour < 24 && self.minute < 60 && self.second < 61
    }

    /// Day of the week, 0 = Sunday.
    fn weekday(&self) -> usize {
        match chrono::NaiveDate::from_ymd_opt(self.year, self.month, self.day) {
            // `chrono::Weekday::num_days_from_sunday` is 0 = Sunday.
            Some(d) => chrono::Datelike::weekday(&d).num_days_from_sunday() as usize,
            None => 0,
        }
    }
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
/// Japanese months are a numeral and 月; there is no abbreviated form.
const MONTHS_JA: [&str; 12] = ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"];
const DAYS_JA: [&str; 7] = ["日曜日", "月曜日", "火曜日", "水曜日", "木曜日", "金曜日", "土曜日"];

/// Is field `kind` a date/time field?
pub fn is_date_field(kind: &str) -> bool {
    kind.starts_with("datetime")
}

/// The text date/time field `kind` shows at `now` in a run of language `lang` (BCP 47, e.g.
/// `en-US`). `None` when `kind` isn't a date/time type this knows, `now` isn't a real date, or
/// the format needs names in a language other than English or Japanese: the field keeps its
/// saved text.
pub fn field_text(kind: &str, lang: Option<&str>, now: DateTime) -> Option<String> {
    if !now.is_valid() {
        return None;
    }
    let lang = lang.map(str::trim).filter(|l| !l.is_empty()).unwrap_or("en-US").to_ascii_lowercase();
    let english = lang == "en" || lang.starts_with("en-");
    // The primary subtag, so `ja`, `ja-JP`, `ja_JP` and `JA-jp` all count, but `jam` doesn't.
    let japanese = lang.split(['-', '_']).next() == Some("ja");
    let m = now.month.checked_sub(1)? as usize;
    let month = MONTHS.get(m)?;
    let mon = month.get(..3)?;
    let month_ja = MONTHS_JA.get(m)?;
    let day = DAYS.get(now.weekday())?;
    let day_ja = DAYS_JA.get(now.weekday())?;
    let (d, y) = (now.day, now.year);
    let yy = y.rem_euclid(100);
    let h = now.hour;
    let h12 = match h % 12 {
        0 => 12,
        h => h,
    };
    let ampm = if h < 12 { "AM" } else { "PM" };
    let (mi, s) = (now.minute, now.second);
    let short = short_date(&lang, now);
    // English keeps its exact text; Japanese follows PowerPoint's Japanese Date and Time list,
    // which has no AM/PM form and no abbreviated month or day names:
    //
    //   datetime2  Friday, October 9, 2026  →  2026年10月9日金曜日
    //   datetime3  9 October 2026           →  2026年10月9日
    //   datetime4  October 9, 2026          →  2026年10月9日
    //   datetime5  9-Oct-26                 →  2026年10月9日
    //   datetime6  October 26               →  2026年10月
    //   datetime7  Oct-26                   →  2026年10月
    //   datetime8  10/9/2026 8:05 PM        →  2026/10/9 20:05
    //   datetime9  10/9/2026 8:05:07 PM     →  2026/10/9 20:05:07
    //   datetime12 8:05 PM                  →  20:05
    //   datetime13 8:05:07 PM               →  20:05:07
    //
    // `datetime1`, `datetime`, `datetimeFigureOut`, `datetime10` and `datetime11` are numeric
    // and already language-neutral. Japanese months are numeric, so the English kinds'
    // abbreviations fold onto their full forms (`Oct-26` and `October 26` both `2026年10月`)
    // and the four-digit year is kept throughout: a two-digit Gregorian year reads as an era
    // year (26年 = 平成26年) in Japanese. `datetime2` uses the full weekday name, as the English
    // kind does; the parenthesised short style (`(金)`) belongs to none of the thirteen kinds.
    // `None` keeps the text the file was saved with in every other language.
    let named = |en: String, ja: String| {
        if english { Some(en) } else { japanese.then_some(ja) }
    };
    match kind {
        "datetime" | "datetimeFigureOut" | "datetime1" => Some(short),
        "datetime2" => named(format!("{day}, {month} {d}, {y}"), format!("{y}年{month_ja}{d}日{day_ja}")),
        "datetime3" => named(format!("{d} {month} {y}"), format!("{y}年{month_ja}{d}日")),
        "datetime4" => named(format!("{month} {d}, {y}"), format!("{y}年{month_ja}{d}日")),
        "datetime5" => named(format!("{d}-{mon}-{yy:02}"), format!("{y}年{month_ja}{d}日")),
        "datetime6" => named(format!("{month} {yy:02}"), format!("{y}年{month_ja}")),
        "datetime7" => named(format!("{mon}-{yy:02}"), format!("{y}年{month_ja}")),
        "datetime8" => named(format!("{short} {h12}:{mi:02} {ampm}"), format!("{short} {h}:{mi:02}")),
        "datetime9" => named(format!("{short} {h12}:{mi:02}:{s:02} {ampm}"), format!("{short} {h}:{mi:02}:{s:02}")),
        "datetime10" => Some(format!("{h}:{mi:02}")),
        "datetime11" => Some(format!("{h}:{mi:02}:{s:02}")),
        "datetime12" => named(format!("{h12}:{mi:02} {ampm}"), format!("{h}:{mi:02}")),
        "datetime13" => named(format!("{h12}:{mi:02}:{s:02} {ampm}"), format!("{h}:{mi:02}:{s:02}")),
        _ => None,
    }
}

/// The short (numeric) date of language `lang` (lower case), US order unless the language is
/// known to write its dates another way.
fn short_date(lang: &str, t: DateTime) -> String {
    let (y, m, d) = (t.year, t.month, t.day);
    let primary = lang.split(['-', '_']).next().unwrap_or("");
    match lang {
        "en-gb" | "en-au" | "en-nz" | "en-ie" | "en-in" => return format!("{d:02}/{m:02}/{y}"),
        "en-ca" | "fr-ca" | "en-za" => return format!("{y}-{m:02}-{d:02}"),
        _ => {}
    }
    match primary {
        "fr" | "es" | "it" | "pt" | "el" | "ca" | "vi" => format!("{d:02}/{m:02}/{y}"),
        "de" | "ru" | "pl" | "cs" | "sk" | "tr" | "fi" | "nb" | "nn" | "no" | "da" | "uk" | "ro" => format!("{d:02}.{m:02}.{y}"),
        "nl" => format!("{d}-{m}-{y}"),
        "ja" | "zh" => format!("{y}/{m}/{d}"),
        "ko" | "sv" | "lt" => format!("{y}-{m:02}-{d:02}"),
        _ => format!("{m}/{d}/{y}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRI: DateTime = DateTime { year: 2026, month: 10, day: 9, hour: 20, minute: 5, second: 7 };
    /// A Sunday (weekday 0).
    const SUN: DateTime = DateTime { year: 2026, month: 2, day: 1, hour: 9, minute: 5, second: 7 };
    /// A leap day, a Thursday, at midnight.
    const LEAP: DateTime = DateTime { year: 2024, month: 2, day: 29, hour: 0, minute: 0, second: 0 };

    #[test]
    fn english_formats_of_every_type() {
        let f = |k: &str| field_text(k, Some("en-US"), FRI);
        assert_eq!(f("datetime1").as_deref(), Some("10/9/2026"));
        assert_eq!(f("datetime2").as_deref(), Some("Friday, October 9, 2026"));
        assert_eq!(f("datetime3").as_deref(), Some("9 October 2026"));
        assert_eq!(f("datetime4").as_deref(), Some("October 9, 2026"));
        assert_eq!(f("datetime5").as_deref(), Some("9-Oct-26"));
        assert_eq!(f("datetime6").as_deref(), Some("October 26"));
        assert_eq!(f("datetime7").as_deref(), Some("Oct-26"));
        assert_eq!(f("datetime8").as_deref(), Some("10/9/2026 8:05 PM"));
        assert_eq!(f("datetime9").as_deref(), Some("10/9/2026 8:05:07 PM"));
        assert_eq!(f("datetime10").as_deref(), Some("20:05"));
        assert_eq!(f("datetime11").as_deref(), Some("20:05:07"));
        assert_eq!(f("datetime12").as_deref(), Some("8:05 PM"));
        assert_eq!(f("datetime13").as_deref(), Some("8:05:07 PM"));
        assert_eq!(f("datetimeFigureOut").as_deref(), Some("10/9/2026"));
        assert_eq!(f("datetime").as_deref(), Some("10/9/2026"));
        // No language: US English. Midnight and noon are 12 on the clock.
        assert_eq!(field_text("datetime12", None, DateTime::new(2024, 2, 29, 0, 0, 0)).as_deref(), Some("12:00 AM"));
        assert_eq!(field_text("datetime12", None, DateTime::new(2024, 2, 29, 12, 30, 0)).as_deref(), Some("12:30 PM"));
        assert_eq!(field_text("datetime2", None, DateTime::new(2000, 1, 1, 0, 0, 0)).as_deref(), Some("Saturday, January 1, 2000"));
    }

    #[test]
    fn japanese_formats_of_every_type() {
        let f = |k: &str| field_text(k, Some("ja-JP"), FRI);
        assert_eq!(f("datetime1").as_deref(), Some("2026/10/9"));
        assert_eq!(f("datetime2").as_deref(), Some("2026年10月9日金曜日"));
        assert_eq!(f("datetime3").as_deref(), Some("2026年10月9日"));
        assert_eq!(f("datetime4").as_deref(), Some("2026年10月9日"));
        assert_eq!(f("datetime5").as_deref(), Some("2026年10月9日"));
        assert_eq!(f("datetime6").as_deref(), Some("2026年10月"));
        assert_eq!(f("datetime7").as_deref(), Some("2026年10月"));
        assert_eq!(f("datetime8").as_deref(), Some("2026/10/9 20:05"));
        assert_eq!(f("datetime9").as_deref(), Some("2026/10/9 20:05:07"));
        assert_eq!(f("datetime10").as_deref(), Some("20:05"));
        assert_eq!(f("datetime11").as_deref(), Some("20:05:07"));
        assert_eq!(f("datetime12").as_deref(), Some("20:05"));
        assert_eq!(f("datetime13").as_deref(), Some("20:05:07"));
        assert_eq!(f("datetimeFigureOut").as_deref(), Some("2026/10/9"));
        assert_eq!(f("datetime").as_deref(), Some("2026/10/9"));
    }

    #[test]
    fn japanese_tag_forms_all_reach_the_japanese_names() {
        for tag in ["ja", "ja-JP", "ja_JP", "JA-jp", " ja-JP "] {
            assert_eq!(field_text("datetime2", Some(tag), FRI).as_deref(), Some("2026年10月9日金曜日"), "{tag}");
            assert_eq!(field_text("datetime8", Some(tag), FRI).as_deref(), Some("2026/10/9 20:05"), "{tag}");
        }
        // The primary subtag decides: a tag that merely starts with `ja` keeps its saved text.
        assert_eq!(field_text("datetime2", Some("jam"), FRI), None);
    }

    #[test]
    fn japanese_weekday_zero_and_a_leap_day() {
        assert_eq!(field_text("datetime2", Some("ja-JP"), SUN).as_deref(), Some("2026年2月1日日曜日"));
        assert_eq!(field_text("datetime3", Some("ja-JP"), SUN).as_deref(), Some("2026年2月1日"));
        assert_eq!(field_text("datetime2", Some("ja-JP"), LEAP).as_deref(), Some("2024年2月29日木曜日"));
        // Japanese keeps the 24-hour clock through midnight, where English uses 12 AM.
        assert_eq!(field_text("datetime8", Some("ja-JP"), LEAP).as_deref(), Some("2024/2/29 0:00"));
        assert_eq!(field_text("datetime12", Some("ja-JP"), LEAP).as_deref(), Some("0:00"));
        assert_eq!(field_text("datetime2", Some("ja-JP"), DateTime::new(2026, 2, 30, 0, 0, 0)), None);
    }

    #[test]
    fn languages_without_names_still_keep_their_saved_text() {
        // Japanese is a second language with names; English keeps every string it had.
        assert_eq!(field_text("datetime2", Some("en-US"), FRI).as_deref(), Some("Friday, October 9, 2026"));
        assert_eq!(field_text("datetime5", Some("en-GB"), FRI).as_deref(), Some("9-Oct-26"));
        assert_eq!(field_text("datetime7", Some("en-US"), FRI).as_deref(), Some("Oct-26"));
        assert_eq!(field_text("datetime13", Some("en-US"), FRI).as_deref(), Some("8:05:07 PM"));
        assert_eq!(field_text("datetime2", Some("de-DE"), FRI), None);
        assert_eq!(field_text("datetime7", Some("ko-KR"), FRI), None);
        assert_eq!(field_text("datetime12", Some("fr-FR"), FRI), None);
        // The numeric kinds need no names and are unchanged.
        assert_eq!(field_text("datetime10", Some("de-DE"), FRI).as_deref(), Some("20:05"));
        assert_eq!(field_text("datetime1", Some("ja-JP"), FRI).as_deref(), Some("2026/10/9"));
    }

    #[test]
    fn language_orders_numeric_dates_and_keeps_names_english_only() {
        assert_eq!(field_text("datetime1", Some("en-GB"), FRI).as_deref(), Some("09/10/2026"));
        assert_eq!(field_text("datetimeFigureOut", Some("de-DE"), FRI).as_deref(), Some("09.10.2026"));
        assert_eq!(field_text("datetime1", Some("ja-JP"), FRI).as_deref(), Some("2026/10/9"));
        assert_eq!(field_text("datetime1", Some("sv-SE"), FRI).as_deref(), Some("2026-10-09"));
        assert_eq!(field_text("datetime10", Some("fr-FR"), FRI).as_deref(), Some("20:05"));
        // Month names in another language: not computed (the field keeps its saved text).
        assert_eq!(field_text("datetime2", Some("de-DE"), FRI), None);
        assert_eq!(field_text("datetime12", Some("fr-FR"), FRI), None);
        assert_eq!(field_text("datetime4", Some("en-GB"), FRI).as_deref(), Some("October 9, 2026"));
    }

    #[test]
    fn unknown_types_and_impossible_dates_give_nothing() {
        assert_eq!(field_text("slidenum", None, FRI), None);
        assert_eq!(field_text("datetime14", None, FRI), None);
        for bad in [
            DateTime::new(2026, 0, 9, 0, 0, 0),
            DateTime::new(2026, 13, 9, 0, 0, 0),
            DateTime::new(2026, 2, 29, 0, 0, 0),
            DateTime::new(2026, 4, 31, 0, 0, 0),
            DateTime::new(2026, 1, 1, 24, 0, 0),
            DateTime::new(i32::MIN, 1, 1, 0, 0, 0),
            DateTime::new(i32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        ] {
            assert_eq!(field_text("datetime2", None, bad), None, "{bad:?}");
        }
        assert!(is_date_field("datetimeFigureOut") && !is_date_field("slidenum"));
    }

    #[test]
    fn the_clock_reads_a_real_date() {
        if let Some(now) = DateTime::now_local() {
            assert!(field_text("datetime1", None, now).is_some(), "{now:?}");
        }
    }
}
