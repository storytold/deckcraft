//! Number formats: Excel format codes (`#,##0`, `0.0%`, `_-* # ##0 "₽"_-;…`, `General`) as chart
//! axes and data labels show them, in the chart's language (digit grouping and decimal mark).

/// Digit grouping and decimal mark for a language tag (`ru-RU`, `de-DE`, `en-US`).
fn marks(lang: Option<&str>) -> (char, char) {
    let l = lang.unwrap_or("en").split(['-', '_']).next().unwrap_or("en").to_ascii_lowercase();
    match l.as_str() {
        // Space-grouped, comma decimal.
        "ru" | "uk" | "be" | "kk" | "fr" | "pl" | "cs" | "sk" | "sv" | "fi" | "nb" | "nn" | "no" | "bg" | "lt" | "lv" | "et" | "hu" | "uz" | "hy"
        | "ka" | "az" => ('\u{a0}', ','),
        // Dot-grouped, comma decimal.
        "de" | "es" | "it" | "pt" | "nl" | "da" | "tr" | "el" | "ro" | "id" | "sr" | "hr" | "sl" | "vi" => ('.', ','),
        _ => (',', '.'),
    }
}

#[derive(Debug, PartialEq)]
enum Tok {
    /// Text shown as is.
    Lit(String),
    /// A digit placeholder: `0`, `#` or `?`.
    Digit(char),
    Dot,
    Comma,
    Percent,
}

/// A section's tokens (`[$₽-419]` gives its symbol; colours, conditions and locales drop out).
fn tokens(section: &str) -> Vec<Tok> {
    let mut out = vec![];
    let mut it = section.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                let mut s = String::new();
                for d in it.by_ref() {
                    if d == '"' {
                        break;
                    }
                    s.push(d);
                }
                out.push(Tok::Lit(s));
            }
            '\\' => {
                if let Some(d) = it.next() {
                    out.push(Tok::Lit(d.to_string()));
                }
            }
            // Padding the width of the next character: a space.
            '_' => {
                it.next();
                out.push(Tok::Lit(" ".into()));
            }
            // Repeat-to-fill: nothing on an axis.
            '*' => {
                it.next();
            }
            '[' => {
                let mut s = String::new();
                for d in it.by_ref() {
                    if d == ']' {
                        break;
                    }
                    s.push(d);
                }
                if let Some(sym) = s.strip_prefix('$') {
                    let sym = sym.split('-').next().unwrap_or("");
                    if !sym.is_empty() {
                        out.push(Tok::Lit(sym.to_string()));
                    }
                }
            }
            '0' | '#' | '?' => out.push(Tok::Digit(c)),
            '.' => out.push(Tok::Dot),
            ',' => out.push(Tok::Comma),
            '%' => out.push(Tok::Percent),
            '@' => {}
            _ => out.push(Tok::Lit(c.to_string())),
        }
    }
    out
}

/// The format's sections split at `;` (outside quotes, brackets and escapes).
fn sections(code: &str) -> Vec<&str> {
    let mut out = vec![];
    let (mut start, mut quoted, mut bracket, mut escaped) = (0, false, false, false);
    for (i, c) in code.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if !quoted => escaped = true,
            '"' => quoted = !quoted,
            '[' if !quoted => bracket = true,
            ']' if !quoted => bracket = false,
            ';' if !quoted && !bracket => {
                out.push(code.get(start..i).unwrap_or(""));
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(code.get(start..).unwrap_or(""));
    out
}

/// `v` in the General format: up to 10 significant digits, no grouping.
fn general(v: f64, dec: char) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let a = v.abs();
    let s = if !(1e-9..1e11).contains(&a) {
        let s = format!("{v:.5E}");
        match s.split_once('E') {
            Some((m, e)) => {
                let m = if m.contains('.') { m.trim_end_matches('0').trim_end_matches('.') } else { m };
                let e: i32 = e.parse().unwrap_or(0);
                format!("{m}E{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
            }
            None => s,
        }
    } else {
        let digits = (10 - (a.log10().floor() as i32 + 1)).clamp(0, 10) as usize;
        let s = format!("{v:.digits$}");
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
    };
    s.replace('.', &dec.to_string())
}

/// Group an integer's digits in threes with `sep`.
fn group(digits: &str, sep: char) -> String {
    let n = digits.chars().count();
    let mut out = String::new();
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (n - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(d);
    }
    out
}

/// `v` formatted with the Excel number format `code` in language `lang` (axis labels: the text
/// section of a format doesn't apply).
pub fn format(code: &str, v: f64, lang: Option<&str>) -> String {
    let (sep, dec) = marks(lang);
    if !v.is_finite() {
        return String::new();
    }
    let secs = sections(code);
    // The section for the value's sign; a negative or zero section shows the value unsigned.
    let (sec, v, minus) = match (secs.as_slice(), v) {
        ([_, neg, ..], v) if v < 0.0 => (*neg, -v, false),
        ([_, _, zero, ..], v) if v == 0.0 => (*zero, v, false),
        ([pos, ..], v) => (*pos, v.abs(), v < 0.0),
        ([], v) => ("General", v.abs(), v < 0.0),
    };
    if sec.trim().eq_ignore_ascii_case("general") || (secs.len() == 1 && sec.trim().is_empty()) {
        let s = general(v, dec);
        return if minus { format!("-{s}") } else { s };
    }
    let toks = tokens(sec);
    let first = toks.iter().position(|t| matches!(t, Tok::Digit(_)));
    let last = toks.iter().rposition(|t| matches!(t, Tok::Digit(_)));
    let (Some(first), Some(last)) = (first, last) else {
        // No digits: just the section's text ("-" for zero, a label).
        let s: String = toks.iter().filter_map(|t| if let Tok::Lit(s) = t { Some(s.as_str()) } else { None }).collect();
        return s.trim().to_string();
    };
    let out = sci(&toks, v, sep, dec).unwrap_or_else(|| fixed(&toks, first, last, v, sep, dec));
    let out = out.trim();
    if minus && !out.is_empty() { format!("-{out}") } else { out.to_string() }
}

/// The text of the literal and percent tokens in `ts`.
fn lits(ts: &[Tok]) -> String {
    ts.iter()
        .filter_map(|t| match t {
            Tok::Lit(s) => Some(s.clone()),
            Tok::Percent => Some("%".into()),
            _ => None,
        })
        .collect()
}

/// Scientific notation (`0.00E+00`, `##0.0E-0`): the mantissa takes the placeholders before the `E`,
/// the exponent's sign and minimum width come from what follows it; with several integer
/// placeholders the exponent is a multiple of their count (engineering notation). `None` if the
/// section has no exponent part.
fn sci(toks: &[Tok], v: f64, sep: char, dec: char) -> Option<String> {
    let e = toks.iter().position(|t| matches!(t, Tok::Lit(s) if s.as_str() == "E" || s.as_str() == "e"))?;
    let e_char = if let Some(Tok::Lit(s)) = toks.get(e) { s.as_str() } else { "E" };
    let plus = match toks.get(e + 1)? {
        Tok::Lit(s) if s.as_str() == "+" => true,
        Tok::Lit(s) if s.as_str() == "-" => false,
        _ => return None,
    };
    let mant = toks.get(..e)?;
    let mfirst = mant.iter().position(|t| matches!(t, Tok::Digit(_)))?;
    let mlast = mant.iter().rposition(|t| matches!(t, Tok::Digit(_)))?;
    let exp_toks = toks.get(e + 2..)?;
    let exp_len = exp_toks.iter().take_while(|t| matches!(t, Tok::Digit(_))).count();
    if exp_len == 0 {
        return None;
    }
    let width = exp_toks.iter().take(exp_len).filter(|t| **t == Tok::Digit('0')).count();
    let rest = exp_toks.get(exp_len..).unwrap_or(&[]);
    let mant_frac = mant.iter().skip_while(|t| **t != Tok::Dot).filter(|t| matches!(t, Tok::Digit(_))).count();
    let (mut m, mut exp) = (0.0, 0i32);
    if v != 0.0 {
        exp = v.log10().floor() as i32;
        m = if exp < -300 {
            // 10^exp underflows for subnormals; scale in two steps.
            v * 1e300 / 10f64.powi(exp + 300)
        } else {
            v / 10f64.powi(exp)
        };
        if !m.is_finite() {
            m = 0.0;
            exp = 0;
        }
        if m >= 10.0 {
            m /= 10.0;
            exp += 1;
        } else if m < 1.0 {
            m *= 10.0;
            exp -= 1;
        }
        // Rounding the mantissa to its digits can reach 10 (9.999 as `0.00`).
        if format!("{m:.mant_frac$}").starts_with("10") {
            m /= 10.0;
            exp += 1;
        }
    }
    let step = mant.iter().take_while(|t| **t != Tok::Dot).filter(|t| matches!(t, Tok::Digit(_))).count().max(1) as i32;
    let shift = exp.rem_euclid(step);
    exp -= shift;
    m *= 10f64.powi(shift);
    let mantissa = fixed(mant, mfirst, mlast, m, sep, dec);
    let sign = if exp < 0 {
        "-"
    } else if plus {
        "+"
    } else {
        ""
    };
    let digits = exp.unsigned_abs().to_string();
    let pad = "0".repeat(width.saturating_sub(digits.len()));
    Some(format!("{mantissa}{e_char}{sign}{pad}{digits}{}", lits(rest)))
}

/// `v` (not negative) in the fixed-point format whose tokens are `toks`, with its digits from
/// `first` to `last`.
fn fixed(toks: &[Tok], first: usize, last: usize, v: f64, sep: char, dec: char) -> String {
    let percent = toks.contains(&Tok::Percent);
    let number = toks.get(first..=last).unwrap_or(&[]);
    let dot = number.iter().position(|t| *t == Tok::Dot);
    let (int_part, frac_part) = match dot {
        Some(d) => (number.get(..d).unwrap_or(&[]), number.get(d + 1..).unwrap_or(&[])),
        None => (number, &[][..]),
    };
    // Grouping: a comma (or the locale's space, written literally) between integer digits.
    let grouping = int_part.iter().any(|t| *t == Tok::Comma || matches!(t, Tok::Lit(s) if s.trim().is_empty()));
    // Commas right after the last digit scale by a thousand each (`#,##0,` = thousands).
    let after = toks.get(last + 1..).unwrap_or(&[]);
    let scale_commas = after.iter().take_while(|t| **t == Tok::Comma).count();
    let min_int = int_part.iter().filter(|t| **t == Tok::Digit('0')).count();
    let frac_digits: Vec<char> = frac_part.iter().filter_map(|t| if let Tok::Digit(c) = t { Some(*c) } else { None }).collect();
    let max_frac = frac_digits.len();
    let min_frac = frac_digits.iter().filter(|c| **c == '0').count();
    let mut x = v * if percent { 100.0 } else { 1.0 } / 1000f64.powi(scale_commas as i32);
    if !x.is_finite() {
        x = 0.0;
    }
    let s = format!("{x:.max_frac$}");
    let (ip, fp) = s.split_once('.').unwrap_or((s.as_str(), ""));
    let mut fp = fp.to_string();
    while fp.len() > min_frac && fp.ends_with('0') {
        fp.pop();
    }
    let ip = ip.trim_start_matches('0');
    let ip = if ip.len() < min_int { format!("{}{ip}", "0".repeat(min_int - ip.len())) } else { ip.to_string() };
    let mut num = if grouping { group(&ip, sep) } else { ip };
    if !fp.is_empty() {
        num.push(dec);
        num.push_str(&fp);
    }
    let before = lits(toks.get(..first).unwrap_or(&[]));
    let after = lits(toks.get(last + 1..).unwrap_or(&[]));
    format!("{before}{num}{after}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUB: &str = r#"_-* #\ ##0\ "₽"_-;\-* #\ ##0\ "₽"_-;_-* "-"??\ "₽"_-;_-@_-"#;

    #[test]
    fn scientific_subnormal() {
        assert_eq!(format("0.00E+00", 5e-324, None), "4.94E-324");
        assert_eq!(format("0.00E+00", f64::MIN_POSITIVE, None), "2.23E-308");
    }

    #[test]
    fn russian_accounting_roubles() {
        assert_eq!(format(RUB, 800_000.0, Some("ru-RU")), "800\u{a0}000 ₽");
        assert_eq!(format(RUB, 1_234_567.0, Some("ru-RU")), "1\u{a0}234\u{a0}567 ₽");
        assert_eq!(format(RUB, -5_000.0, Some("ru-RU")), "-5\u{a0}000 ₽");
        assert_eq!(format(RUB, 0.0, Some("ru-RU")), "- ₽");
    }

    #[test]
    fn common_codes() {
        assert_eq!(format("#,##0", 7000.0, Some("ru-RU")), "7\u{a0}000");
        assert_eq!(format("#,##0", 7000.0, Some("en-US")), "7,000");
        assert_eq!(format("#,##0", 7000.0, Some("de-DE")), "7.000");
        assert_eq!(format("0.0%", 0.125, Some("ru-RU")), "12,5%");
        assert_eq!(format("0.0%", 0.125, None), "12.5%");
        assert_eq!(format("0%", 0.0, None), "0%");
        assert_eq!(format("General", 2.5, Some("ru-RU")), "2,5");
        assert_eq!(format("General", 400_000.0, Some("ru-RU")), "400000");
        assert_eq!(format("General", 1.5e12, None), "1.5E+12");
        assert_eq!(format("General", 1e-10, None), "1E-10");
        assert_eq!(format("General", -2.5e-12, None), "-2.5E-12");
        assert_eq!(format("General", 1e100, None), "1E+100");
        assert_eq!(format("0.00", 1.23456, None), "1.23");
        assert_eq!(format("#,##0.0#", 1234.5, None), "1,234.5");
        assert_eq!(format("#,##0,", 12_345_678.0, None), "12,346");
        assert_eq!(format("[$₽-419]#,##0", 1500.0, Some("ru-RU")), "₽1\u{a0}500");
        assert_eq!(format("0;[Red]-0", -3.0, None), "-3");
        assert_eq!(format("0;0", -3.0, None), "3");
        assert_eq!(format("0", -3.0, None), "-3");
        assert_eq!(format("\"x\"0", 5.0, None), "x5");
    }

    #[test]
    fn trailing_commas_with_decimals() {
        assert_eq!(format("0.0,,", 15_000_000.0, Some("en-US")), "15.0");
        assert_eq!(format("0.00,\"K\"", 15_000.0, Some("en-US")), "15.00K");
    }

    #[test]
    fn explicit_empty_sections_hide_their_values() {
        for v in [3.0, -3.0, 0.0, -0.0] {
            assert_eq!(format(";;;", v, None), "", "all sections empty: {v}");
        }
        for (code, positive, negative, zero) in [
            (";0;0", "", "3", "0"),
            ("0;;0", "3", "", "0"),
            ("0;-0;", "3", "-3", ""),
            (";0", "", "3", ""),
            (r#"";"0;;"#, ";3", "", ""),
            (r"\;0;;", ";3", "", ""),
        ] {
            assert_eq!(format(code, 3.0, None), positive, "positive: {code}");
            assert_eq!(format(code, -3.0, None), negative, "negative: {code}");
            assert_eq!(format(code, 0.0, None), zero, "zero: {code}");
        }
        assert_eq!(format("0;-0;", -0.0, None), "");
    }

    #[test]
    fn blank_whole_codes_and_general_keep_their_fallback() {
        for code in ["", " \t", "General", " general "] {
            assert_eq!(format(code, 2.5, None), "2.5", "{code:?}");
            assert_eq!(format(code, -2.5, None), "-2.5", "{code:?}");
            assert_eq!(format(code, 0.0, None), "0", "{code:?}");
            assert_eq!(format(code, 2.5, Some("de-DE")), "2,5", "{code:?}");
        }
    }

    #[test]
    fn scientific_formats() {
        assert_eq!(format("0.00E+00", 1234.0, None), "1.23E+03");
        assert_eq!(format("0.00E+00", 1234.0, Some("ru-RU")), "1,23E+03");
        assert_eq!(format("0.00E+00", 0.00012, None), "1.20E-04");
        assert_eq!(format("0.00E+00", -1234.0, None), "-1.23E+03");
        assert_eq!(format("0.00E+00", 0.0, None), "0.00E+00");
        assert_eq!(format("0.0E+000", 5e10, None), "5.0E+010");
        assert_eq!(format("0.0E-0", 1234.0, None), "1.2E3");
        assert_eq!(format("0.0E-0", 0.0012, None), "1.2E-3");
        assert_eq!(format("0E+00", 9999.0, None), "1E+04");
        assert_eq!(format("0.00E+00", 9.999, None), "1.00E+01");
        assert_eq!(format("0.00e+00", 1234.0, None), "1.23e+03");
        assert_eq!(format("##0.0E+0", 12_345.0, None), "12.3E+3");
        assert_eq!(format("##0.0E+0", 0.012, None), "12.0E-3");
        assert_eq!(format("##0.0E+0", 999.96, None), "1.0E+3");
    }

    #[test]
    fn never_panics() {
        for code in ["", ";", ";;;", "\"", "[", "\\", "0.", ".", "#,", "%", "_", "*", "@", "0E+00", "[$", "\"unterminated"] {
            for v in [0.0, -1.5, 1e300, f64::NAN, f64::INFINITY, 1e-300] {
                let _ = format(code, v, Some("ru"));
            }
        }
    }
}
