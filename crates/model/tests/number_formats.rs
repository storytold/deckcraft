use deckcraft_model::table::{
    CellNumberKind, CellValue, DateFormatPart, NegativeDisplay, NumberComparison, NumberFormat, NumberFormatSection, NumberLocale, NumberStyle,
};

fn number(style: NumberStyle) -> NumberFormat {
    NumberFormat { style, ..Default::default() }
}

fn fixed(places: u8, grouping: bool) -> NumberStyle {
    NumberStyle::Number { decimal_places: places, min_decimal_places: places, min_integer_digits: 1, grouping }
}

#[test]
fn decimal_display_rounds_without_binary_float_or_changing_value_authority() {
    let format = number(fixed(2, false));
    for (source, expected) in [("1.005", "1.01"), ("2.675", "2.68"), ("-1.005", "-1.01"), ("9.999", "10.00"), ("-0.0001", "0.00"), ("1e-2", "0.01")] {
        let value = CellValue::Number { kind: CellNumberKind::Float, lexical: source.into() };
        let before = value.clone();
        assert_eq!(format.format_value(&value).unwrap().text, expected);
        assert_eq!(value, before);
    }
    let large = number(fixed(0, false)).format_number("1e100").unwrap();
    assert_eq!(large, format!("1{}", "0".repeat(100)));
}

#[test]
fn grouping_locales_minimum_digits_optional_decimals_and_negatives_are_explicit() {
    let mut format = number(fixed(2, true));
    assert_eq!(format.format_number("12345.5").unwrap(), "12,345.50");
    format.locale = NumberLocale::FrBe;
    assert_eq!(format.format_number("12345.5").unwrap(), "12\u{202f}345,50");
    format.locale = NumberLocale::DeDe;
    assert_eq!(format.format_number("12345.5").unwrap(), "12.345,50");
    format.negative = NegativeDisplay::Parentheses;
    assert_eq!(format.format_number("-12345.5").unwrap(), "(12.345,50)");
    format.zero_display = Some("—".into());
    assert_eq!(format.format_number("0").unwrap(), "—");
    format.style = NumberStyle::Number { decimal_places: 4, min_decimal_places: 1, min_integer_digits: 5, grouping: false };
    assert_eq!(format.format_number("12.3").unwrap(), "00012,3");
}

#[test]
fn percentage_and_currency_do_not_reinterpret_or_convert_stored_units() {
    let percent = number(NumberStyle::Percentage { decimal_places: 1, min_decimal_places: 1, grouping: false });
    assert_eq!(percent.format_number("0.125").unwrap(), "12.5%");
    let mut currency = number(NumberStyle::Currency {
        decimal_places: 2,
        min_decimal_places: 2,
        grouping: true,
        code: "EUR".into(),
        symbol: "€".into(),
        symbol_after: true,
        space: true,
    });
    currency.locale = NumberLocale::FrBe;
    assert_eq!(currency.format_number("1234.5").unwrap(), "1\u{202f}234,50 €");
    assert_eq!(currency.format_number("-1.005").unwrap(), "-1,01 €");
    currency.negative = NegativeDisplay::Parentheses;
    assert_eq!(currency.format_number("-1.005").unwrap(), "(1,01 €)");
}

#[test]
fn scientific_and_fraction_outputs_have_independent_mathematical_expectations() {
    let science = number(NumberStyle::Scientific { decimal_places: 3, min_exponent_digits: 2, exponent_sign: true });
    for (source, expected) in [("1234.5", "1.235E+03"), ("0.00099995", "1.000E-03"), ("-0.01", "-1.000E-02"), ("0", "0.000E+00")] {
        assert_eq!(science.format_number(source).unwrap(), expected);
    }
    let fraction = number(NumberStyle::Fraction { denominator_digits: 2, fixed_denominator: None });
    assert_eq!(fraction.format_number("0.3333333333333333333333333333").unwrap(), "1/3");
    assert_eq!(fraction.format_number("-2.75").unwrap(), "-2 3/4");
    assert_eq!(fraction.format_number("-0.0000001").unwrap(), "0");
    let quarters = number(NumberStyle::Fraction { denominator_digits: 1, fixed_denominator: Some(4) });
    assert_eq!(quarters.format_number("2.375").unwrap(), "2 2/4");
}

#[test]
fn conditional_sections_and_color_use_authoritative_values_not_display_text() {
    let mut format = number(fixed(1, false));
    format.sections.push(NumberFormatSection {
        comparison: NumberComparison::Less,
        threshold: "0".into(),
        style: fixed(2, false),
        prefix: "loss ".into(),
        suffix: "".into(),
        negative: NegativeDisplay::Parentheses,
        color: Some([190, 20, 20, 255]),
    });
    let negative = format.format_value(&CellValue::Number { kind: CellNumberKind::Float, lexical: "-2.5".into() }).unwrap();
    assert_eq!(negative.text, "(loss 2.50)");
    assert_eq!(negative.color, Some([190, 20, 20, 255]));
    assert_eq!(format.format_number("2.5").unwrap(), "2.5");
    assert!(format.format_text("-2.5").is_err());
}

#[test]
fn civil_date_labels_and_authored_timezones_are_not_host_timezone_converted() {
    let format = number(NumberStyle::DateTime {
        parts: vec![
            DateFormatPart::DayOfWeek { long: true },
            DateFormatPart::Literal { text: ", ".into() },
            DateFormatPart::Day { long: false },
            DateFormatPart::Literal { text: " ".into() },
            DateFormatPart::Month { long: true, textual: true },
            DateFormatPart::Literal { text: " ".into() },
            DateFormatPart::Year { long: true },
            DateFormatPart::Literal { text: " ".into() },
            DateFormatPart::Hours { long: true, elapsed: false },
            DateFormatPart::Literal { text: ":".into() },
            DateFormatPart::Minutes { long: true },
            DateFormatPart::Literal { text: " ".into() },
            DateFormatPart::Zone,
        ],
    });
    let value = CellValue::Date { lexical: "2000-02-29T13:05:06+02:00".into() };
    assert_eq!(format.format_value(&value).unwrap().text, "Tuesday, 29 February 2000 13:05 +02:00");
    let mut french = format.clone();
    french.locale = NumberLocale::FrBe;
    assert_eq!(french.format_value(&value).unwrap().text, "mardi, 29 février 2000 13:05 +02:00");
}

#[test]
fn duration_elapsed_hours_are_not_wrapped_and_arbitrary_day_magnitudes_stay_exact() {
    let format = number(NumberStyle::Duration {
        parts: vec![
            DateFormatPart::Hours { long: true, elapsed: true },
            DateFormatPart::Literal { text: ":".into() },
            DateFormatPart::Minutes { long: true },
            DateFormatPart::Literal { text: ":".into() },
            DateFormatPart::Seconds { long: true, decimal_places: 0 },
        ],
    });
    assert_eq!(format.format_value(&CellValue::Duration { lexical: "P1DT12H3M4S".into() }).unwrap().text, "36:03:04");
    assert_eq!(format.format_value(&CellValue::Duration { lexical: "-PT100H".into() }).unwrap().text, "-100:00:00");
    assert_eq!(
        format.format_value(&CellValue::Duration { lexical: "P1000000000000000000000000000000D".into() }).unwrap().text,
        "24000000000000000000000000000000:00:00"
    );
    assert!(format.format_value(&CellValue::Duration { lexical: "P1M".into() }).is_err());
}

#[test]
fn format_profiles_validate_types_limits_xml_and_unknown_serde_fields() {
    let boolean = number(NumberStyle::Boolean { true_text: "Yes".into(), false_text: "No".into() });
    assert_eq!(boolean.format_boolean(true).unwrap(), "Yes");
    assert!(boolean.format_number("1").is_err());
    assert!(number(fixed(29, false)).validate().is_err());
    let mut oversized = number(fixed(2, false));
    oversized.prefix = "x".repeat(1025);
    assert!(oversized.validate().is_err());
    oversized.prefix = "\0".into();
    assert!(oversized.validate().is_err());
    assert!(serde_json::from_str::<NumberFormat>(r#"{"unexpected":1}"#).is_err());
    let json = serde_json::to_string(&boolean).unwrap();
    assert_eq!(serde_json::from_str::<NumberFormat>(&json).unwrap(), boolean);
}

#[test]
fn numeric_date_formats_retain_original_scalar_digits_and_clock_rounding_carries() {
    let style = NumberStyle::DateTime {
        parts: vec![
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
            DateFormatPart::Seconds { long: true, decimal_places: 2 },
        ],
    };
    let format = number(style);
    assert_eq!(format.format_number("0.50000000000000000000000000000000000000001").unwrap(), "1899-12-30 12:00:00.00");
    assert_eq!(format.format_number("-0.25").unwrap(), "1899-12-29 18:00:00.00");
    assert_eq!(format.format_value(&CellValue::Date { lexical: "2024-02-28T23:59:59.999+02:00".into() }).unwrap().text, "2024-02-29 00:00:00.00");
    let duration = number(NumberStyle::Duration {
        parts: vec![
            DateFormatPart::Hours { long: true, elapsed: true },
            DateFormatPart::Literal { text: ":".into() },
            DateFormatPart::Minutes { long: true },
            DateFormatPart::Literal { text: ":".into() },
            DateFormatPart::Seconds { long: true, decimal_places: 2 },
        ],
    });
    assert_eq!(duration.format_value(&CellValue::Duration { lexical: "PT23H59M59.999S".into() }).unwrap().text, "24:00:00.00");
}
