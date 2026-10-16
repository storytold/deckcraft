//! Independent expected-value witnesses; no production helpers compute the oracles.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::{
    Table, TextBody,
    table::{
        CellConstant, CellFormula, CellNumberKind, CellValue, DateFormatPart, FormulaBudget, FormulaError, FormulaOutcome, NegativeDisplay,
        NumberComparison, NumberFormat, NumberFormatSection, NumberLocale, NumberStyle,
    },
};
use std::sync::Arc;
fn num(v: &str) -> FormulaOutcome {
    FormulaOutcome::Number { lexical: v.into() }
}
fn err(code: FormulaError) -> FormulaOutcome {
    FormulaOutcome::Error { code }
}
fn eval(source: &str) -> FormulaOutcome {
    let mut t = Table::new(1, 1, 100., 20.);
    t.set_cell_formula_v2(0, 0, source).unwrap();
    t.validate_formulas().unwrap();
    t.cell(0, 0).unwrap().formula.as_ref().unwrap().outcome().clone()
}
fn format(style: NumberStyle) -> NumberFormat {
    NumberFormat { style, ..Default::default() }
}
fn number_style(places: u8) -> NumberStyle {
    NumberStyle::Number { decimal_places: places, min_decimal_places: places, min_integer_digits: 1, grouping: false }
}
fn set_number(t: &mut Table, r: usize, c: usize, v: &str) {
    let cell = t.cell_mut(r, c).unwrap();
    cell.text = TextBody::from_text(v);
    cell.data = Some(Arc::new(CellConstant::from_display(CellValue::Number { kind: CellNumberKind::Float, lexical: v.into() }, &cell.text).unwrap()));
}
#[test]
fn independent_full_precision_arithmetic_and_ties_even() {
    for (source, want) in [
        ("=1234567890123456789012345678+0.5", "1234567890123456789012345678"),
        ("=1234567890123456789012345679+0.5", "1234567890123456789012345680"),
        ("=-1234567890123456789012345678-0.5", "-1234567890123456789012345678"),
        ("=9999999999999999999999999999+0.5", "10000000000000000000000000000"),
        ("=1e96+1e-96", "1e96"),
        ("=1e-96/1e-96", "1"),
        ("=ROUND(-0.0005;3)", "-0.001"),
        ("=ROUNDDOWN(-0.0005;3)", "0"),
        ("=ROUNDUP(-1e-96;0)", "-1"),
        ("=INT(-1e-96)", "-1"),
    ] {
        let want = if want == "1e96" { format!("1{}", "0".repeat(96)) } else { want.into() };
        assert_eq!(eval(source), num(&want), "{source}");
    }
}
#[test]
fn independent_power_integer_threshold_and_certified_values() {
    for (source, want) in [
        ("=1.000000000000000000000000001^1000000000000000000000000000", "2.71828182845904523536028747"),
        ("=1^9999999999999999999999999999", "1"),
        ("=0^9999999999999999999999999999", "0"),
        ("=POWER(1.0001;5000)", "1.648680055931175769628200045"),
        ("=POWER(1.0001;-5000)", "0.6065458221578347578405131291"),
        ("=5^0.3", "1.62065659669276243515045413"),
        ("=0.25^0.25", "0.7071067811865475244008443621"),
    ] {
        assert_eq!(eval(source), num(want), "{source}");
    }
    assert_eq!(eval("=0^-5000"), err(FormulaError::DivisionByZero));
    assert_eq!(eval("=(-2)^0.3"), err(FormulaError::NumericLimit));
}
#[test]
fn independent_legacy_reference_rechecks_v1_bounds() {
    let mut t = Table::new(1, 2, 200., 20.);
    set_number(&mut t, 0, 0, "1");
    t.set_cell_formula(0, 1, "=[.A1]").unwrap();
    t.set_cell_formula_v2(0, 0, "=1e-32").unwrap();
    assert_eq!(t.cell(0, 1).unwrap().formula.as_ref().unwrap().outcome(), &err(FormulaError::NumericLimit));
    assert_eq!(t.cell(0, 1).unwrap().formula.as_ref().unwrap().profile(), "exact-decimal-v1");
    t.validate_formulas().unwrap();
}
#[test]
fn independent_scalar_formats_preserve_all_authored_digits() {
    let lexical = "1234567890123456789012345678901234567890.12345678901234567890123456789";
    let style = format(number_style(28));
    assert_eq!(style.format_number(lexical).unwrap(), "1234567890123456789012345678901234567890.1234567890123456789012345679");
    let general = NumberFormat::default();
    assert_eq!(general.format_number(lexical).unwrap(), lexical);
    assert_eq!(general.format_number("1e-300").unwrap(), format!("0.{}1", "0".repeat(299)));
    assert_eq!(general.format_number("-0e200").unwrap(), "0");
}
#[test]
fn independent_fraction_near_repeating_rational_midpoint() {
    let fixed = format(NumberStyle::Fraction { denominator_digits: 1, fixed_denominator: Some(3) });
    assert_eq!(fixed.format_number("0.1666666666666666666666666666667").unwrap(), "1/3");
    assert_eq!(fixed.format_number("0.1666666666666666666666666666666").unwrap(), "0");
    let variable = format(NumberStyle::Fraction { denominator_digits: 1, fixed_denominator: None });
    assert_eq!(variable.format_number("0.0555555555555555555555555555556").unwrap(), "1/9");
    assert_eq!(variable.format_number("0.0555555555555555555555555555555").unwrap(), "0");
    assert_eq!(fixed.format_number("-1.5").unwrap(), "-1 2/3");
}
#[test]
fn independent_currency_percentage_scientific_rounding_and_locales() {
    let mut money = format(NumberStyle::Currency {
        decimal_places: 2,
        min_decimal_places: 2,
        grouping: true,
        code: "EUR".into(),
        symbol: "€".into(),
        symbol_after: true,
        space: true,
    });
    money.locale = NumberLocale::FrBe;
    money.negative = NegativeDisplay::Parentheses;
    assert_eq!(
        money.format_number("-123456789012345678901234567890.125").unwrap(),
        "(123\u{202f}456\u{202f}789\u{202f}012\u{202f}345\u{202f}678\u{202f}901\u{202f}234\u{202f}567\u{202f}890,13 €)"
    );
    let mut percent = format(NumberStyle::Percentage { decimal_places: 2, min_decimal_places: 2, grouping: false });
    percent.locale = NumberLocale::DeDe;
    assert_eq!(percent.format_number("0.12345").unwrap(), "12,35%");
    assert_eq!(percent.format_number("-0.00001").unwrap(), "0,00%");
    let scientific = format(NumberStyle::Scientific { decimal_places: 3, min_exponent_digits: 3, exponent_sign: true });
    for (input, want) in [("9.9995e123", "1.000E+124"), ("-0.000099995", "-1.000E-004"), ("0", "0.000E+000")] {
        assert_eq!(scientific.format_number(input).unwrap(), want);
    }
}
#[test]
fn independent_conditional_format_exact_threshold_and_first_match() {
    let threshold = "123456789012345678901234567890.1234567890123456789";
    let f = NumberFormat {
        style: number_style(0),
        color: Some([1, 2, 3, 255]),
        sections: vec![
            NumberFormatSection {
                comparison: NumberComparison::Greater,
                threshold: threshold.into(),
                style: number_style(2),
                prefix: "above:".into(),
                suffix: "!".into(),
                negative: NegativeDisplay::Minus,
                color: Some([9, 8, 7, 255]),
            },
            NumberFormatSection {
                comparison: NumberComparison::GreaterEqual,
                threshold: threshold.into(),
                style: number_style(1),
                prefix: "equal:".into(),
                suffix: String::new(),
                negative: NegativeDisplay::Minus,
                color: Some([4, 5, 6, 255]),
            },
        ],
        ..Default::default()
    };
    let value = CellValue::Number { kind: CellNumberKind::Float, lexical: "123456789012345678901234567890.1234567890123456790".into() };
    let out = f.format_value(&value).unwrap();
    assert_eq!(out.text, "above:123456789012345678901234567890.12!");
    assert_eq!(out.color, Some([9, 8, 7, 255]));
    let equal = f.format_value(&CellValue::Number { kind: CellNumberKind::Float, lexical: threshold.into() }).unwrap();
    assert_eq!(equal.text, "equal:123456789012345678901234567890.1");
    assert_eq!(equal.color, Some([4, 5, 6, 255]));
}
#[test]
fn independent_calendar_boundaries_and_clock_wall_fields() {
    for (source, want) in [
        ("=DATE(2000;3;0)", "36585"),
        ("=DATE(1900;3;0)", "60"),
        ("=YEAR(-693593)", "1"),
        ("=DAY(-0.1)", "29"),
        ("=HOUR(-0.1)", "21"),
        ("=MINUTE(-0.1)", "36"),
        ("=YEAR(\"2024-12-31T23:30:00-14:00\")", "2024"),
        ("=HOUR(\"2024-12-31T23:30:00-14:00\")", "23"),
    ] {
        assert_eq!(eval(source), num(want), "{source}");
    }
    assert_eq!(eval("=DATE(9999;13;1)"), err(FormulaError::NumericLimit));
}
#[test]
fn independent_date_duration_lexical_formatting() {
    let parts = vec![
        DateFormatPart::Year { long: true },
        DateFormatPart::Literal { text: "-".into() },
        DateFormatPart::Month { long: true, textual: false },
        DateFormatPart::Literal { text: "-".into() },
        DateFormatPart::Day { long: true },
        DateFormatPart::Literal { text: " ".into() },
        DateFormatPart::Hours { long: true, elapsed: false },
        DateFormatPart::Literal { text: ":".into() },
        DateFormatPart::Minutes { long: true },
        DateFormatPart::Literal { text: ":".into() },
        DateFormatPart::Seconds { long: true, decimal_places: 3 },
        DateFormatPart::Zone,
    ];
    let date = format(NumberStyle::DateTime { parts });
    assert_eq!(
        date.format_value(&CellValue::Date { lexical: "2024-02-29T23:58:59.1255+02:30".into() }).unwrap().text,
        "2024-02-29 23:58:59.126+02:30"
    );
    let elapsed = format(NumberStyle::Duration {
        parts: vec![
            DateFormatPart::Hours { long: false, elapsed: true },
            DateFormatPart::Literal { text: ":".into() },
            DateFormatPart::Minutes { long: true },
            DateFormatPart::Literal { text: ":".into() },
            DateFormatPart::Seconds { long: true, decimal_places: 2 },
        ],
    });
    assert_eq!(elapsed.format_value(&CellValue::Duration { lexical: "-P2DT3H4M5.125S".into() }).unwrap().text, "-51:04:05.13");
    assert!(elapsed.format_value(&CellValue::Duration { lexical: "P1MT1H".into() }).is_err());
}
#[test]
fn independent_numeric_fuel_survives_pass_reset_and_rolls_back() {
    let mut t = Table::new(1, 1, 100., 20.);
    let mut budget = FormulaBudget::default();
    t.set_cell_formula_v2_with_budget(0, 0, "=2^0.1", &mut budget).unwrap();
    let before = t.clone();
    let mut exhausted = false;
    for _ in 0..32 {
        budget.next_pass();
        if t.validate_formulas_with_budget(&mut budget).is_err() {
            exhausted = true;
            break;
        }
    }
    assert!(exhausted);
    assert!(budget.check_limits().is_err());
    assert_eq!(t, before);
    assert!(t.set_cell_formula_v2_with_budget(0, 0, "=IFERROR(1/0;9)", &mut budget).is_err());
    assert_eq!(t, before);
}
#[test]
fn independent_hostile_format_and_range_rejection() {
    let mut f = format(number_style(2));
    f.prefix = "x".repeat(1025);
    assert!(f.format_number("1").is_err());
    let mut f = format(NumberStyle::Fraction { denominator_digits: 4, fixed_denominator: Some(0) });
    assert!(f.format_number("1").is_err());
    f.style = NumberStyle::Scientific { decimal_places: 29, min_exponent_digits: 2, exponent_sign: true };
    assert!(f.validate().is_err());
    let mut t = Table::new(1, 1, 100., 20.);
    let before = t.clone();
    assert!(t.set_cell_formula_v2(0, 0, "=IFERROR(SUM([.A1:.ZZZZZZ999999999999999999999999999]);9)").is_err());
    assert_eq!(t, before);
    assert!(t.set_cell_formula_v2(0, 0, &format!("=IFERROR(1;\"{}\")", "x".repeat(4096))).is_err());
    assert_eq!(t, before);
}
#[test]
fn independent_formatted_v1_does_not_accept_v2_arithmetic() {
    assert!(CellFormula::from_profile_display("exact-decimal-v1-formatted", "=1/2".into(), num("0.5"), &TextBody::from_text("$0.50")).is_err());
    assert!(CellFormula::from_profile_display("exact-decimal-v1-formatted", "=1e-32".into(), num("1e-32"), &TextBody::from_text("tiny")).is_err());
}
