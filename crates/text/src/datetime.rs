//! Date and time fields (`a:fld type="datetime…"`): the text each field type shows for a moment,
//! and the local clock.
//!
//! The field types and their formats follow ECMA-376 Part 1, §21.1.2.2.4 (`fld`): `datetime1`
//! to `datetime13`, written as in PowerPoint's English (United States) Date and Time list.
//! `datetime` and `datetimeFigureOut` show the language's short date. English keeps its exact
//! text; `ja-JP` follows what PowerPoint 16.0 on Windows showed with `lang="ja-JP"` (see
//! [`japanese_field_text`]); every other language keeps the text the file was saved with when
//! the format needs names.

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

/// Is field `kind` a date/time field?
pub fn is_date_field(kind: &str) -> bool {
    kind.starts_with("datetime")
}

/// The date and time field types PowerPoint's Header and Footer and Insert ▸ Date & Time offer,
/// in the order the dialogs list them. `datetime` and `datetimeFigureOut` are read as `datetime1`
/// (see [`field_text`]) but are not offered as a choice.
pub const DATE_KINDS: [&str; 13] = [
    "datetime1",
    "datetime2",
    "datetime3",
    "datetime4",
    "datetime5",
    "datetime6",
    "datetime7",
    "datetime8",
    "datetime9",
    "datetime10",
    "datetime11",
    "datetime12",
    "datetime13",
];

/// The text date/time field `kind` shows at `now` in a run of language `lang` (BCP 47, e.g.
/// `en-US`). `None` when `kind` isn't a date/time type this knows, `now` isn't a real date, or
/// the format needs names in a language other than English or Japanese: the field keeps its
/// saved text.
pub fn field_text(kind: &str, lang: Option<&str>, now: DateTime) -> Option<String> {
    if !now.is_valid() {
        return None;
    }
    let lang = lang.map(str::trim).filter(|l| !l.is_empty()).unwrap_or("en-US").to_ascii_lowercase();
    if is_ja_jp(&lang) {
        return japanese_field_text(kind, now);
    }
    let english = lang == "en" || lang.starts_with("en-");
    let (h, mi, s) = (now.hour, now.minute, now.second);
    if !english {
        // A language without month, weekday and AM/PM names keeps the field's saved text for
        // every format that needs them. The numeric date follows the language's date order and
        // the 24-hour clock is language-neutral, so those are still computed.
        return match kind {
            "datetime" | "datetimeFigureOut" | "datetime1" => Some(short_date(&lang, now)),
            "datetime10" => Some(format!("{h}:{mi:02}")),
            "datetime11" => Some(format!("{h}:{mi:02}:{s:02}")),
            _ => None,
        };
    }
    let month = MONTHS.get(now.month.checked_sub(1)? as usize)?;
    let mon = month.get(..3)?;
    let day = DAYS.get(now.weekday())?;
    let (d, y) = (now.day, now.year);
    let yy = y.rem_euclid(100);
    let h12 = match h % 12 {
        0 => 12,
        h => h,
    };
    let ampm = if h < 12 { "AM" } else { "PM" };
    let short = short_date(&lang, now);
    // British-style English writes the day before the month ("9 October 2026"). The punctuation
    // follows PowerPoint on Windows per locale: Irish English has no comma after the weekday,
    // British and Australian English put one after the month in `datetime3`, and South African
    // English writes the weekday form day first too.
    let day_first = matches!(lang.as_str(), "en-gb" | "en-au" | "en-nz" | "en-ie" | "en-in");
    let named = |text: String| english.then_some(text);
    match kind {
        "datetime" | "datetimeFigureOut" | "datetime1" => Some(short),
        "datetime2" if lang == "en-ie" => named(format!("{day} {d} {month} {y}")),
        "datetime2" if day_first || lang == "en-za" => named(format!("{day}, {d} {month} {y}")),
        "datetime2" => named(format!("{day}, {month} {d}, {y}")),
        "datetime3" if matches!(lang.as_str(), "en-gb" | "en-au") => named(format!("{d} {month}, {y}")),
        "datetime3" => named(format!("{d} {month} {y}")),
        "datetime4" if day_first => named(format!("{d} {month} {y}")),
        "datetime4" => named(format!("{month} {d}, {y}")),
        "datetime5" => named(format!("{d}-{mon}-{yy:02}")),
        "datetime6" => named(format!("{month} {yy:02}")),
        "datetime7" => named(format!("{mon}-{yy:02}")),
        "datetime8" => named(format!("{short} {h12}:{mi:02} {ampm}")),
        "datetime9" => named(format!("{short} {h12}:{mi:02}:{s:02} {ampm}")),
        "datetime10" => Some(format!("{}:{mi:02}", now.hour)),
        "datetime11" => Some(format!("{}:{mi:02}:{s:02}", now.hour)),
        "datetime12" => named(format!("{h12}:{mi:02} {ampm}")),
        "datetime13" => named(format!("{h12}:{mi:02}:{s:02} {ampm}")),

        _ => None,
    }
}

/// `kind` in Japanese, as PowerPoint 16.0 on Windows showed it with `lang="ja-JP"` at
/// 2026-10-27 19:43:50 on an en-US machine (measured 2026-10-27):
///
/// ```text
/// datetime1  10/27/2026
/// datetime2  Tuesday, October 27, 2026
/// datetime3  令和8年10月27日
/// datetime4  2026年10月27日
/// datetime5  27-Oct-26
/// datetime6  令和8年10月
/// datetime7  令和八年十月二十七日
/// datetime8  26/10/27 19時43分
/// datetime9  26/10/27 19時43分50秒
/// datetime10 19:43
/// datetime11 19:43:50
/// datetime12 19時43分
/// datetime13 19時43分50秒
/// ```
///
/// No kind showed a Japanese weekday. `datetime1` and `datetime2` are the operating system's
/// regional short and long dates rather than Japanese patterns: on the measured en-US machine
/// they came out as the English ones, and they would change with the regional settings of the
/// machine, not with the run's language. DeckCraft has no locale-aware date formatter (`chrono`
/// carries no month names) and no view of the OS regional settings, so the two use the measured
/// en-US shapes; the rest use the measured Japanese patterns. A bare `lang="ja"` did not get
/// these formats from PowerPoint and does not get them here (see [`is_ja_jp`]).
fn japanese_field_text(kind: &str, now: DateTime) -> Option<String> {
    let (y, mo, d) = (now.year, now.month, now.day);
    let (h, mi, s) = (now.hour, now.minute, now.second);
    let month_en = MONTHS.get(mo.checked_sub(1)? as usize)?;
    let mon = month_en.get(..3)?;
    let yy = y.rem_euclid(100);
    match kind {
        // The OS regional dates: measured as the en-US short and long date.
        "datetime" | "datetimeFigureOut" | "datetime1" => Some(system_short_date(now)),
        "datetime2" => system_long_date(now),
        // The Japanese era (令和) in Arabic numerals.
        "datetime3" => {
            let era = era_year_text(now)?;
            Some(format!("令和{era}年{mo}月{d}日"))
        }
        // The Gregorian date, unchanged by the era.
        "datetime4" => Some(format!("{y}年{mo}月{d}日")),
        // PowerPoint kept the English month abbreviation for this one.
        "datetime5" => Some(format!("{d}-{mon}-{yy:02}")),
        "datetime6" => {
            let era = era_year_text(now)?;
            Some(format!("令和{era}年{mo}月"))
        }
        // The era date in kanji numerals: 令和八年十月二十七日.
        "datetime7" => {
            let era = era_year_kanji(now)?;
            Some(format!("令和{era}年{}月{}日", kanji_number(mo)?, kanji_number(d)?))
        }
        // Two-digit year, then 時/分/秒; the hour is not zero-padded, minute and second are.
        "datetime8" => Some(format!("{yy:02}/{mo:02}/{d:02} {h}時{mi:02}分")),
        "datetime9" => Some(format!("{yy:02}/{mo:02}/{d:02} {h}時{mi:02}分{s:02}秒")),
        "datetime10" => Some(format!("{h}:{mi:02}")),
        "datetime11" => Some(format!("{h}:{mi:02}:{s:02}")),
        "datetime12" => Some(format!("{h}時{mi:02}分")),
        "datetime13" => Some(format!("{h}時{mi:02}分{s:02}秒")),
        _ => None,
    }
}

/// Is `lang` (lower case) the Japanese tag PowerPoint expands date/time fields for? Only
/// `ja-JP`: a bare `lang="ja"` was not treated like `lang="ja-JP"` in the measurement, so it
/// falls through to the generic rules (a numeric date, otherwise the saved text) instead of
/// taking the Japanese patterns. Case and `_`/`-` are normalised because OOXML writers differ.
fn is_ja_jp(lang: &str) -> bool {
    let mut parts = lang.split(['-', '_']);
    parts.next() == Some("ja") && parts.next() == Some("jp")
}

/// The short (numeric) date Windows' en-US regional settings give (`M/d/yyyy`, no leading
/// zeros): what `datetime1` showed in the ja-JP measurement, and this crate's default for a
/// language whose date order it doesn't know.
fn system_short_date(t: DateTime) -> String {
    format!("{}/{}/{}", t.month, t.day, t.year)
}

/// The long date Windows' en-US regional settings give (`dddd, MMMM d, yyyy`): the English
/// `datetime2`, and what `datetime2` showed in the ja-JP measurement.
fn system_long_date(t: DateTime) -> Option<String> {
    let month = MONTHS.get(t.month.checked_sub(1)? as usize)?;
    let day = DAYS.get(t.weekday())?;
    Some(format!("{day}, {month} {}, {}", t.day, t.year))
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
        _ => system_short_date(t),
    }
}

/// The Reiwa year of `t`: 1 on 2019-05-01. `None` before the era began, where PowerPoint writes
/// 平成 or an older era: the measurement covers 令和 only, so those fields keep their saved text
/// rather than show a wrong era.
fn reiwa_year(t: DateTime) -> Option<i32> {
    if (t.year, t.month, t.day) < (2019, 5, 1) {
        return None;
    }
    Some(t.year - 2018)
}

/// The era year as `datetime3`/`datetime6` write it: 元 in the first year, the number after (`8`).
fn era_year_text(t: DateTime) -> Option<String> {
    Some(match reiwa_year(t)? {
        1 => "元".to_string(),
        year => year.to_string(),
    })
}

/// The era year in kanji numerals (元, 八, 十八 …) for `datetime7`.
fn era_year_kanji(t: DateTime) -> Option<String> {
    Some(match reiwa_year(t)? {
        1 => "元".to_string(),
        year => kanji_number(u32::try_from(year).ok()?)?,
    })
}

/// 1…999 in kanji numerals, as `datetime7` writes the era year, month and day (`八`, `十`,
/// `二十七`). Zero and four-digit numbers have no form here.
fn kanji_number(n: u32) -> Option<String> {
    const DIGITS: [&str; 10] = ["", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    if n == 0 || n > 999 {
        return None;
    }
    let hundreds = usize::try_from(n / 100).ok()?;
    let tens = usize::try_from((n / 10) % 10).ok()?;
    let ones = usize::try_from(n % 10).ok()?;
    let mut out = String::new();
    match hundreds {
        0 => {}
        1 => out.push('百'),
        _ => {
            out.push_str(DIGITS.get(hundreds)?);
            out.push('百');
        }
    }
    match tens {
        0 => {}
        1 => out.push('十'),
        _ => {
            out.push_str(DIGITS.get(tens)?);
            out.push('十');
        }
    }
    out.push_str(DIGITS.get(ones)?);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRI: DateTime = DateTime { year: 2026, month: 10, day: 9, hour: 20, minute: 5, second: 7 };
    /// The moment the Japanese formats were measured at: PowerPoint 16.0, 2026-10-27 19:43:50,
    /// `lang="ja-JP"`, en-US Windows.
    const MEASURED: DateTime = DateTime { year: 2026, month: 10, day: 27, hour: 19, minute: 43, second: 50 };
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
    fn japanese_formats_of_every_type_as_measured() {
        let f = |k: &str| field_text(k, Some("ja-JP"), MEASURED);
        assert_eq!(f("datetime1").as_deref(), Some("10/27/2026"));
        assert_eq!(f("datetime2").as_deref(), Some("Tuesday, October 27, 2026"));
        assert_eq!(f("datetime3").as_deref(), Some("令和8年10月27日"));
        assert_eq!(f("datetime4").as_deref(), Some("2026年10月27日"));
        assert_eq!(f("datetime5").as_deref(), Some("27-Oct-26"));
        assert_eq!(f("datetime6").as_deref(), Some("令和8年10月"));
        assert_eq!(f("datetime7").as_deref(), Some("令和八年十月二十七日"));
        assert_eq!(f("datetime8").as_deref(), Some("26/10/27 19時43分"));
        assert_eq!(f("datetime9").as_deref(), Some("26/10/27 19時43分50秒"));
        assert_eq!(f("datetime10").as_deref(), Some("19:43"));
        assert_eq!(f("datetime11").as_deref(), Some("19:43:50"));
        assert_eq!(f("datetime12").as_deref(), Some("19時43分"));
        assert_eq!(f("datetime13").as_deref(), Some("19時43分50秒"));
        assert_eq!(f("datetimeFigureOut").as_deref(), Some("10/27/2026"));
        assert_eq!(f("datetime").as_deref(), Some("10/27/2026"));
        // No kind showed a Japanese weekday.
        assert!(!f("datetime2").unwrap().contains('曜'));
    }

    #[test]
    fn japanese_era_years_and_kanji_numerals() {
        // 令和 began on 2019-05-01; the first year is written 元年.
        assert_eq!(field_text("datetime3", Some("ja-JP"), DateTime::new(2019, 5, 1, 0, 0, 0)).as_deref(), Some("令和元年5月1日"));
        assert_eq!(field_text("datetime7", Some("ja-JP"), DateTime::new(2019, 12, 31, 0, 0, 0)).as_deref(), Some("令和元年十二月三十一日"));
        assert_eq!(field_text("datetime7", Some("ja-JP"), LEAP).as_deref(), Some("令和六年二月二十九日"));
        assert_eq!(field_text("datetime6", Some("ja-JP"), DateTime::new(2026, 1, 5, 0, 0, 0)).as_deref(), Some("令和8年1月"));
        assert_eq!(field_text("datetime7", Some("ja-JP"), DateTime::new(2026, 1, 5, 0, 0, 0)).as_deref(), Some("令和八年一月五日"));
        // Before 令和 the era is not modelled, so the era fields keep their saved text, while
        // the Gregorian `datetime4` still computes.
        assert_eq!(field_text("datetime3", Some("ja-JP"), DateTime::new(2019, 4, 30, 0, 0, 0)), None);
        assert_eq!(field_text("datetime6", Some("ja-JP"), DateTime::new(2019, 4, 30, 0, 0, 0)), None);
        assert_eq!(field_text("datetime7", Some("ja-JP"), DateTime::new(2019, 4, 30, 0, 0, 0)), None);
        assert_eq!(field_text("datetime4", Some("ja-JP"), DateTime::new(2019, 4, 30, 0, 0, 0)).as_deref(), Some("2019年4月30日"));
    }

    #[test]
    fn japanese_times_use_measured_padding() {
        let early = DateTime::new(2026, 1, 5, 9, 7, 5);
        assert_eq!(field_text("datetime8", Some("ja-JP"), early).as_deref(), Some("26/01/05 9時07分"));
        assert_eq!(field_text("datetime9", Some("ja-JP"), early).as_deref(), Some("26/01/05 9時07分05秒"));
        assert_eq!(field_text("datetime12", Some("ja-JP"), early).as_deref(), Some("9時07分"));
        assert_eq!(field_text("datetime13", Some("ja-JP"), early).as_deref(), Some("9時07分05秒"));
        // The 24-hour clock forms keep colons and no leading zero on the hour.
        assert_eq!(field_text("datetime10", Some("ja-JP"), early).as_deref(), Some("9:07"));
        assert_eq!(field_text("datetime11", Some("ja-JP"), early).as_deref(), Some("9:07:05"));
    }

    #[test]
    fn only_ja_jp_reaches_the_japanese_formats() {
        for tag in ["ja-JP", "ja_JP", "JA-jp", " ja-JP "] {
            assert_eq!(field_text("datetime3", Some(tag), MEASURED).as_deref(), Some("令和8年10月27日"), "{tag}");
            assert_eq!(field_text("datetime8", Some(tag), MEASURED).as_deref(), Some("26/10/27 19時43分"), "{tag}");
        }
        // A bare `lang="ja"` is not expanded by PowerPoint, so the named and Japanese-time
        // formats keep the field's saved text here too.
        for kind in ["datetime2", "datetime3", "datetime6", "datetime7", "datetime8", "datetime13"] {
            assert_eq!(field_text(kind, Some("ja"), MEASURED), None, "{kind}");
        }
        // The numeric date and the 24-hour clock need no names and still compute.
        assert_eq!(field_text("datetime1", Some("ja"), FRI).as_deref(), Some("2026/10/9"));
        assert_eq!(field_text("datetime10", Some("ja"), FRI).as_deref(), Some("20:05"));
        // A tag that merely starts with `ja` is not Japanese either.
        assert_eq!(field_text("datetime2", Some("jam"), FRI), None);
    }

    #[test]
    fn japanese_weekday_zero_and_a_leap_day() {
        assert_eq!(field_text("datetime2", Some("ja-JP"), SUN).as_deref(), Some("Sunday, February 1, 2026"));
        assert_eq!(field_text("datetime3", Some("ja-JP"), SUN).as_deref(), Some("令和8年2月1日"));
        assert_eq!(field_text("datetime2", Some("ja-JP"), LEAP).as_deref(), Some("Thursday, February 29, 2024"));
        // Japanese keeps the 24-hour clock through midnight, where English uses 12 AM.
        assert_eq!(field_text("datetime8", Some("ja-JP"), LEAP).as_deref(), Some("24/02/29 0時00分"));
        assert_eq!(field_text("datetime12", Some("ja-JP"), LEAP).as_deref(), Some("0時00分"));
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
        assert_eq!(field_text("datetime1", Some("ja-JP"), FRI).as_deref(), Some("10/9/2026"));
    }

    #[test]
    fn language_orders_numeric_dates_and_keeps_names_english_only() {
        assert_eq!(field_text("datetime1", Some("en-GB"), FRI).as_deref(), Some("09/10/2026"));
        assert_eq!(field_text("datetimeFigureOut", Some("de-DE"), FRI).as_deref(), Some("09.10.2026"));
        assert_eq!(field_text("datetime1", Some("ja-JP"), FRI).as_deref(), Some("10/9/2026"));
        assert_eq!(field_text("datetime1", Some("sv-SE"), FRI).as_deref(), Some("2026-10-09"));
        assert_eq!(field_text("datetime10", Some("fr-FR"), FRI).as_deref(), Some("20:05"));
        // Month names in another language: not computed (the field keeps its saved text).
        assert_eq!(field_text("datetime2", Some("de-DE"), FRI), None);
        assert_eq!(field_text("datetime12", Some("fr-FR"), FRI), None);
        assert_eq!(field_text("datetime4", Some("en-GB"), FRI).as_deref(), Some("9 October 2026"));
        assert_eq!(field_text("datetime2", Some("en-GB"), FRI).as_deref(), Some("Friday, 9 October 2026"));
        assert_eq!(field_text("datetime4", Some("en-AU"), FRI).as_deref(), Some("9 October 2026"));
        assert_eq!(field_text("datetime4", Some("en-US"), FRI).as_deref(), Some("October 9, 2026"));
    }

    /// The long English dates as PowerPoint on Windows shows them on Tuesday 27 October 2026, read
    /// per run language (datetime2, datetime3, datetime4).
    #[test]
    fn long_english_dates_follow_each_locale() {
        let tue = DateTime::new(2026, 10, 27, 9, 0, 0);
        let rows = [
            ("en-US", "Tuesday, October 27, 2026", "27 October 2026", "October 27, 2026"),
            ("en-GB", "Tuesday, 27 October 2026", "27 October, 2026", "27 October 2026"),
            ("en-AU", "Tuesday, 27 October 2026", "27 October, 2026", "27 October 2026"),
            ("en-NZ", "Tuesday, 27 October 2026", "27 October 2026", "27 October 2026"),
            ("en-IE", "Tuesday 27 October 2026", "27 October 2026", "27 October 2026"),
            ("en-IN", "Tuesday, 27 October 2026", "27 October 2026", "27 October 2026"),
            ("en-CA", "Tuesday, October 27, 2026", "27 October 2026", "October 27, 2026"),
        ];
        for (lang, d2, d3, d4) in rows {
            assert_eq!(field_text("datetime2", Some(lang), tue).as_deref(), Some(d2), "{lang}");
            assert_eq!(field_text("datetime3", Some(lang), tue).as_deref(), Some(d3), "{lang}");
            assert_eq!(field_text("datetime4", Some(lang), tue).as_deref(), Some(d4), "{lang}");
        }
        assert_eq!(field_text("datetime2", Some("en-ZA"), tue).as_deref(), Some("Tuesday, 27 October 2026"));
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
