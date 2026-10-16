//! Independent expectations for the versioned presentation formula profile.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::{
    Table, TextBody,
    table::{CellConstant, CellFormula, CellNumberKind, CellValue, FormulaError, FormulaOutcome},
};
use std::sync::Arc;

fn number(table: &mut Table, row: usize, col: usize, value: &str) {
    let cell = table.cell_mut(row, col).unwrap();
    cell.text = TextBody::from_text(value);
    cell.data =
        Some(Arc::new(CellConstant::from_display(CellValue::Number { kind: CellNumberKind::Float, lexical: value.into() }, &cell.text).unwrap()));
}
fn outcome(table: &Table, row: usize, col: usize) -> FormulaOutcome {
    table.cell(row, col).unwrap().formula.as_ref().unwrap().outcome().clone()
}
fn evaluated(source: &str) -> FormulaOutcome {
    let mut table = Table::new(1, 1, 100., 20.);
    table.set_cell_formula_v2(0, 0, source).unwrap();
    table.validate_formulas().unwrap();
    outcome(&table, 0, 0)
}
fn n(value: &str) -> FormulaOutcome {
    FormulaOutcome::Number { lexical: value.into() }
}
fn e(code: FormulaError) -> FormulaOutcome {
    FormulaOutcome::Error { code }
}
fn s(value: &str) -> FormulaOutcome {
    FormulaOutcome::String { value: value.into() }
}
fn b(value: bool) -> FormulaOutcome {
    FormulaOutcome::Boolean { value }
}

#[test]
fn division_precision_precedence_and_exact_inputs() {
    for (source, expected) in [
        ("=1/2", "0.5"),
        ("=1/3", "0.3333333333333333333333333333"),
        ("=2/3", "0.6666666666666666666666666667"),
        ("=-2^2", "4"),
        ("=2^3^2", "64"),
        ("=-(2^2)", "-4"),
        ("=2+3*4", "14"),
        ("=50%", "0.5"),
        ("=(2+3)%", "0.05"),
        ("=2^-3", "0.125"),
        ("=0.1+0.2", "0.3"),
        ("=1e-28/1", "0.0000000000000000000000000001"),
        ("=1e28/1", "10000000000000000000000000000"),
        ("=SQRT(4)", "2"),
        ("=SQRT(2)", "1.414213562373095048801688724"),
        ("=9^0.5", "3"),
        ("=SQRT(1e-28)", "0.00000000000001"),
    ] {
        assert_eq!(evaluated(source), n(expected), "{source}");
    }
}

#[test]
fn round_modes_and_mod_do_not_inherit_division_rounding() {
    for (source, expected) in [
        ("=ROUND(2.5)", "3"),
        ("=ROUND(-2.5)", "-3"),
        ("=ROUND(9.995;2)", "10"),
        ("=ROUND(125;-1)", "130"),
        ("=ROUNDUP(-1.21;1)", "-1.3"),
        ("=ROUNDDOWN(-1.29;1)", "-1.2"),
        ("=TRUNC(-1.99)", "-1"),
        ("=INT(-1.01)", "-2"),
        ("=MOD(-5;3)", "1"),
        ("=MOD(5;-3)", "-1"),
        ("=ABS(-5)", "5"),
        ("=SIGN(-5)", "-1"),
        ("=SIGN(0)", "0"),
        ("=POWER(3;3)", "27"),
    ] {
        assert_eq!(evaluated(source), n(expected), "{source}");
    }
}

#[test]
fn errors_are_typed_and_native_results_are_not_numeric_only() {
    for (source, code) in [
        ("=1/0", FormulaError::DivisionByZero),
        ("=0^0", FormulaError::NumericLimit),
        ("=SQRT(-1)", FormulaError::NumericLimit),
        ("=MOD(1;0)", FormulaError::DivisionByZero),
        ("=NOTREAL(3)", FormulaError::UnknownName),
        ("=[.Z999]", FormulaError::InvalidReference),
        ("=[#REF!]", FormulaError::InvalidReference),
        ("=NA()", FormulaError::NotAvailable),
        ("=ROUND(1;0.5)", FormulaError::InputNotNumeric),
        ("=LEN(1/0)", FormulaError::DivisionByZero),
    ] {
        assert_eq!(evaluated(source), e(code), "{source}");
    }
    assert_eq!(evaluated("=1=1"), b(true));
    assert_eq!(evaluated("=1=TRUE()"), b(false));
    assert_eq!(evaluated("=1<>TRUE()"), b(true));
    assert_eq!(evaluated("=2>=1"), b(true));
    assert_eq!(evaluated("=2<1"), b(false));
    assert_eq!(evaluated("=\"a\"<\"b\""), b(true));
    assert_eq!(evaluated("=\"a\"&\"b\""), s("ab"));
    assert_eq!(evaluated("=\"a\"\"b\""), s("a\"b"));
    assert_eq!(evaluated("=NOT(FALSE())"), b(true));
    assert_eq!(evaluated("=TRUE(1)"), e(FormulaError::InputNotNumeric));
}

#[test]
fn lazy_branches_do_not_visit_error_or_self_reference() {
    for (source, expected) in [
        ("=IF(FALSE();1/0;7)", n("7")),
        ("=IF(TRUE();\"yes\";1/0)", s("yes")),
        ("=IF(FALSE();[.A1];7)", n("7")),
        ("=IF(TRUE();[.A1];7)", e(FormulaError::Cycle)),
        ("=IFERROR(1/0;9)", n("9")),
        ("=IFERROR(7;[.A1])", n("7")),
        ("=IFNA(NA();\"missing\")", s("missing")),
        ("=IFNA(1/0;9)", e(FormulaError::DivisionByZero)),
        ("=IF(FALSE();7)", b(false)),
    ] {
        assert_eq!(evaluated(source), expected, "{source}");
    }
}

#[test]
fn aggregate_ranges_use_authority_not_numeric_looking_display() {
    let mut table = Table::new(4, 3, 300., 20.);
    number(&mut table, 0, 0, "2");
    number(&mut table, 1, 0, "4");
    table.cell_mut(2, 0).unwrap().text = TextBody::from_text("999");
    let tests = [
        ("=SUM([.A1:.A4])", n("6")),
        ("=AVERAGE([.A1:.A4])", n("3")),
        ("=COUNT([.A1:.A4])", n("2")),
        ("=COUNTA([.A1:.A4])", n("3")),
        ("=COUNTBLANK([.A1:.A4])", n("1")),
        ("=PRODUCT([.A1:.A4])", n("8")),
        ("=MIN([.A1:.A4])", n("2")),
        ("=MAX([.A1:.A4])", n("4")),
        ("=SUM()", n("0")),
        ("=PRODUCT()", n("0")),
        ("=SUM([.$A$1:.$A$2])", n("6")),
        ("=SUM([.A2:.A1])", n("6")),
    ];
    for (source, expected) in tests {
        table.set_cell_formula_v2(0, 2, source).unwrap();
        assert_eq!(outcome(&table, 0, 2), expected, "{source}");
    }
    number(&mut table, 0, 0, "10");
    table.set_cell_formula_v2(0, 2, "=SUM([.A1:.A2])").unwrap();
    assert_eq!(outcome(&table, 0, 2), n("14"));
    table.cell_mut(0, 0).unwrap().text = TextBody::from_text("changed");
    table.recalculate().unwrap();
    assert_eq!(outcome(&table, 0, 2), e(FormulaError::InputNotNumeric));
}

#[test]
fn condition_functions_predicates_and_logical_ranges() {
    let mut table = Table::new(3, 4, 400., 20.);
    for (row, value) in ["2", "4", "6"].into_iter().enumerate() {
        number(&mut table, row, 0, value);
    }
    for (row, value) in ["20", "40", "60"].into_iter().enumerate() {
        number(&mut table, row, 1, value);
    }
    for (source, expected) in [
        ("=SUMIF([.A1:.A3];\">2\";[.B1:.B3])", n("100")),
        ("=COUNTIF([.A1:.A3];\">=4\")", n("2")),
        ("=AVERAGEIF([.A1:.A3];\">2\";[.B1:.B3])", n("50")),
        ("=AVERAGEIF([.A1:.A3];\">9\")", e(FormulaError::DivisionByZero)),
        ("=ISBLANK([.C1])", b(true)),
        ("=ISBLANK(\"\")", b(false)),
        ("=ISNUMBER([.A1])", b(true)),
        ("=ISTEXT(\"3\")", b(true)),
        ("=ISLOGICAL(TRUE())", b(true)),
        ("=ISERROR(1/0)", b(true)),
        ("=ISERR(NA())", b(false)),
        ("=ISNA(NA())", b(true)),
        ("=AND(TRUE();1;[.A1:.A3])", b(true)),
        ("=OR(FALSE();0;[.A1:.A3])", b(true)),
        ("=AND(FALSE();1/0)", e(FormulaError::DivisionByZero)),
    ] {
        table.set_cell_formula_v2(0, 3, source).unwrap();
        assert_eq!(outcome(&table, 0, 3), expected, "{source}");
    }
}

#[test]
fn text_functions_keep_utf8_boundaries_and_strict_arity() {
    for (source, expected) in [
        ("=LEN(\"é🙂x\")", n("3")),
        ("=LEFT(\"é🙂x\";2)", s("é🙂")),
        ("=RIGHT(\"é🙂x\";2)", s("🙂x")),
        ("=MID(\"é🙂x\";2;1)", s("🙂")),
        ("=LEFT(\"abc\")", s("a")),
        ("=RIGHT(\"abc\";99)", s("abc")),
        ("=MID(\"abc\";99;1)", s("")),
        ("=CONCATENATE(\"x\";1;TRUE())", s("x1TRUE")),
        ("=MID(\"abc\";0;1)", e(FormulaError::InputNotNumeric)),
        ("=LEFT(\"abc\";-1)", e(FormulaError::InputNotNumeric)),
    ] {
        assert_eq!(evaluated(source), expected, "{source}");
    }
}

#[test]
fn temporal_profile_uses_explicit_gregorian_epoch_and_numeric_serials() {
    for (source, expected) in [
        ("=DATE(2024;2;29)", n("45351")),
        ("=DATE(2024;13;1)", n("45658")),
        ("=DATE(2024;3;0)", n("45351")),
        ("=YEAR(DATE(2024;2;29))", n("2024")),
        ("=MONTH(DATE(2024;2;29))", n("2")),
        ("=DAY(DATE(2024;2;29))", n("29")),
        ("=TIME(25;1;2)", n("1.042384259259259259259259259")),
        ("=HOUR(TIME(25;1;2))", n("1")),
        ("=MINUTE(TIME(25;1;2))", n("1")),
        ("=SECOND(TIME(25;1;2))", n("2")),
        ("=YEAR(\"2024-01-01junk\")", e(FormulaError::InputNotNumeric)),
    ] {
        assert_eq!(evaluated(source), expected, "{source}");
    }
}

#[test]
fn reference_rewrite_is_token_based_and_preserves_absolute_copy_semantics() {
    let mut table = Table::new(2, 3, 300., 20.);
    number(&mut table, 0, 0, "1");
    number(&mut table, 1, 0, "2");
    table.set_cell_formula_v2(0, 2, "=SUM([.A1:.A2])+[.$A$1]").unwrap();
    let formula = table.cell(0, 2).unwrap().formula.as_ref().unwrap();
    assert_eq!(formula.rewritten_axis(false, 1, 1, false).unwrap(), "=SUM([.A1:.A3])+[.$A$1]");
    assert_eq!(formula.rewritten_axis(false, 0, 1, true).unwrap(), "=SUM([.A1:.A1])+[#REF!]");
    assert_eq!(formula.relocated_v2(1, 1).unwrap(), "=SUM([.B2:.B3])+[.$A$1]");
    let f = CellFormula::from_display_v2("=\"[.A1]\"&[.A1]".into(), s("x"), &TextBody::from_text("x")).unwrap();
    assert_eq!(f.relocated_v2(0, 1).unwrap(), "=\"[.A1]\"&[.B1]");
    assert_eq!(f.relocated_v2(-1, 0).unwrap(), "=\"[.A1]\"&[#REF!]");
}

#[test]
fn native_cache_profile_strictness_and_staged_plan_authority() {
    let mut table = Table::new(1, 2, 200., 20.);
    table.cell_mut(0, 0).unwrap().text = TextBody::from_text("a");
    table.set_cell_formula_v2(0, 1, "=LEN([.A1])").unwrap();
    let bytes = serde_json::to_vec(&table).unwrap();
    let restored: Table = serde_json::from_slice(&bytes).unwrap();
    restored.validate_formulas().unwrap();
    let mut bad = serde_json::to_value(&table).unwrap();
    bad["rows"][0]["cells"][1]["formula"]["outcome"]["lexical"] = "9".into();
    let bad: Table = serde_json::from_value(bad).unwrap();
    assert!(bad.validate_formulas().is_err());
    table.cell_mut(0, 0).unwrap().text = TextBody::from_text("ab");
    let plan = table.plan_recalculation().unwrap();
    table.cell_mut(0, 0).unwrap().text = TextBody::from_text("abc");
    let before = table.clone();
    assert!(plan.apply(&mut table).is_err());
    assert_eq!(table, before);
    assert!(CellFormula::number("=1/2".into(), "0.5".into(), &TextBody::from_text("0.5")).is_err());
    assert!(CellFormula::from_display("=1".into(), b(true), &TextBody::from_text("TRUE")).is_err());
}

#[test]
fn resource_failure_cannot_be_caught_and_assignment_rolls_back() {
    let mut table = Table::new(100, 101, 1010., 20.);
    let before = table.clone();
    let terms = std::iter::repeat_n("SUM([.A1:.CV100])", 32).collect::<Vec<_>>().join(";");
    let source = format!("=IFERROR(SUM({terms});9)");
    assert!(table.set_cell_formula_v2(0, 100, &source).is_err());
    assert_eq!(table, before);
    assert!(table.set_cell_formula_v2(0, 100, &format!("={}1{}", "(".repeat(33), ")".repeat(33))).is_err());
    assert_eq!(table, before);
}

#[test]
fn general_fractional_power_is_interval_certified_and_decimal_only() {
    assert_eq!(evaluated("=16^0.5"), n("4"));
    for (source, expected) in [
        ("=8^(1/3)", "2"),
        ("=27^(1/3)", "3"),
        ("=2^0.1", "1.071773462536293164213006325"),
        ("=4^0.25", "1.414213562373095048801688724"),
        ("=10^-0.5", "0.3162277660168379331998893544"),
        ("=0.01^0.2", "0.3981071705534972507702523051"),
        ("=2^-1.5", "0.3535533905932737622004221811"),
        ("=1.000000000000000000000000001^1000000000000000000000000000", "2.71828182845904523536028747"),
        ("=0^0.1", "0"),
    ] {
        assert_eq!(evaluated(source), n(expected), "{source}");
    }
    assert_eq!(evaluated("=(-2)^0.1"), e(FormulaError::NumericLimit));
    assert_eq!(evaluated("=0^-0.1"), e(FormulaError::DivisionByZero));
}

#[test]
fn formatted_formula_baseline_is_independent_and_plans_bind_the_format() {
    use deckcraft_model::table::{NumberFormat, NumberStyle};
    let mut table = Table::new(1, 2, 200., 20.);
    number(&mut table, 0, 0, "1");
    table.cell_mut(0, 1).unwrap().number_format = Some(Arc::new(NumberFormat {
        style: NumberStyle::Currency {
            decimal_places: 2,
            min_decimal_places: 2,
            grouping: false,
            code: "USD".into(),
            symbol: "$".into(),
            symbol_after: false,
            space: false,
        },
        ..Default::default()
    }));
    table.set_cell_formula_v2(0, 1, "=[.A1]+11").unwrap();
    assert_eq!(outcome(&table, 0, 1), n("12"));
    assert_eq!(table.cell(0, 1).unwrap().text.text(), "$12.00");
    let restored: Table = serde_json::from_slice(&serde_json::to_vec(&table).unwrap()).unwrap();
    restored.validate_formulas().unwrap();
    number(&mut table, 0, 0, "2");
    let plan = table.plan_recalculation().unwrap();
    table.cell_mut(0, 1).unwrap().number_format = None;
    let before = table.clone();
    assert!(plan.apply(&mut table).is_err());
    assert_eq!(table, before);
}

#[test]
fn mixed_version_tables_keep_old_numeric_semantics_and_typed_native_results() {
    let mut table = Table::new(1, 3, 300., 20.);
    number(&mut table, 0, 0, "3");
    table.set_cell_formula(0, 1, "=[.A1]*2").unwrap();
    table.set_cell_formula_v2(0, 2, "=[.B1]/4").unwrap();
    assert_eq!(outcome(&table, 0, 1), n("6"));
    assert_eq!(outcome(&table, 0, 2), n("1.5"));
    number(&mut table, 0, 0, "4");
    table.recalculate().unwrap();
    assert_eq!(outcome(&table, 0, 1), n("8"));
    assert_eq!(outcome(&table, 0, 2), n("2"));
    assert_eq!(table.cell(0, 1).unwrap().formula.as_ref().unwrap().profile(), "exact-decimal-v1");
    for source in ["=TRUE()", "=\"text\"", "=1/0"] {
        table.set_cell_formula_v2(0, 2, source).unwrap();
        let restored: Table = serde_json::from_slice(&serde_json::to_vec(&table).unwrap()).unwrap();
        restored.validate_formulas().unwrap();
        assert_eq!(outcome(&restored, 0, 2), outcome(&table, 0, 2));
    }
}

#[test]
fn syntax_and_duplicate_native_keys_do_not_become_catchable_value_errors() {
    for source in ["=1/", "=SUM(1;)", "=\"unterminated", "=[.A0]", "=[.a1]", "=1 2"] {
        let mut table = Table::new(1, 1, 100., 20.);
        let before = table.clone();
        assert!(table.set_cell_formula_v2(0, 0, source).is_err(), "{source}");
        assert_eq!(table, before);
    }
    let duplicate =
        r#"{"profile":"presentation-decimal-v2","expression":"=TRUE()","outcome":{"kind":"boolean","value":true,"value":false},"display":"TRUE"}"#;
    assert!(serde_json::from_str::<CellFormula>(duplicate).is_err());
}

#[test]
fn epoch_negative_dates_leap_rollover_and_numeric_time_round_trip() {
    use deckcraft_model::table::{formula_date_from_serial, formula_datetime_from_serial};
    for (source, expected) in [
        ("=DATE(1899;12;30)", "0"),
        ("=DATE(1900;1;1)", "2"),
        ("=DATE(1900;2;28)", "60"),
        ("=DATE(1900;2;29)", "61"),
        ("=DATE(1900;3;1)", "61"),
        ("=DATE(2000;2;29)-DATE(2000;2;28)", "1"),
        ("=DATE(2024.9;2.9;29.9)", "45351"),
        ("=DATE(2024;0;1)", "45261"),
        ("=DATE(1;1;1)", "-693593"),
        ("=DATE(9999;12;31)", "2958465"),
        ("=YEAR(-1)", "1899"),
        ("=MONTH(-1)", "12"),
        ("=DAY(-1)", "29"),
        ("=TIME(0;0;1)", "0.00001157407407407407407407407407"),
        ("=TIME(0;0;-1)", "-0.00001157407407407407407407407407"),
        ("=TIME(0;0;0.5)", "0.000005787037037037037037037037037"),
        ("=TIME(24;0;0)", "1"),
        ("=TIME(0;60;0)", "0.04166666666666666666666666667"),
        ("=MINUTE(TIME(0;1;0))", "1"),
        ("=HOUR(TIME(1;0;0))", "1"),
        ("=SECOND(TIME(0;0;1))", "1"),
        ("=SECOND(TIME(0;0;0.5))", "1"),
        ("=HOUR(TIME(0;0;-1))", "23"),
        ("=MINUTE(TIME(0;0;-1))", "59"),
        ("=SECOND(TIME(0;0;-1))", "59"),
        ("=HOUR(TIME(24;0;0))", "0"),
        ("=SECOND(TIME(23;59;59.9))", "60"),
        ("=ISNUMBER(DATE(2024;1;1))", "1"),
    ] {
        let actual = evaluated(source);
        if source.starts_with("=ISNUMBER") {
            assert_eq!(actual, b(true));
        } else {
            assert_eq!(actual, n(expected), "{source}");
        }
    }
    assert_eq!(formula_date_from_serial("0").unwrap(), "1899-12-30");
    assert_eq!(formula_date_from_serial("-0.25").unwrap(), "1899-12-29");
    assert_eq!(formula_datetime_from_serial("-0.25").unwrap(), "1899-12-29T18:00:00");
    assert_eq!(formula_datetime_from_serial("45351.5").unwrap(), "2024-02-29T12:00:00");
    assert_eq!(formula_datetime_from_serial("1e-96").unwrap().len(), 20 + 94);
    assert!(formula_date_from_serial("2958466").is_err());
    let mut table = Table::new(1, 1, 100., 20.);
    table.set_cell_formula_v2(0, 0, "=TIME(0;0;1)").unwrap();
    let restored: Table = serde_json::from_slice(&serde_json::to_vec(&table).unwrap()).unwrap();
    restored.validate_formulas().unwrap();
    assert_eq!(outcome(&restored, 0, 0), outcome(&table, 0, 0));
    assert!(CellFormula::number("=1e-32".into(), "1e-32".into(), &TextBody::from_text("0.00000000000000000000000000000001")).is_err());
}

#[test]
fn wildcard_criteria_are_whole_cell_case_sensitive_unicode_and_escaped() {
    let mut table = Table::new(9, 3, 300., 20.);
    for (row, text) in ["alpha", "Alpha", "a*", "a?", "a~", "🙂x", "", "beta"].into_iter().enumerate() {
        let cell = table.cell_mut(row, 0).unwrap();
        cell.text = TextBody::from_text(text);
        cell.data = Some(Arc::new(CellConstant::from_display(CellValue::String { value: text.into() }, &cell.text).unwrap()));
        number(&mut table, row, 1, &(row + 1).to_string());
    }
    for (criterion, expected) in [
        ("a*", "4"),
        ("A*", "1"),
        ("a~*", "1"),
        ("a~?", "1"),
        ("a~~", "1"),
        ("🙂?", "1"),
        ("*", "8"),
        ("", "2"),
        ("=", "2"),
        ("<>", "7"),
        ("<>a*", "5"),
        ("a~", "1"),
    ] {
        table.set_cell_formula_v2(0, 2, &format!("=COUNTIF([.A1:.A9];\"{criterion}\")")).unwrap();
        assert_eq!(outcome(&table, 0, 2), n(expected), "{criterion}");
    }
    table.set_cell_formula_v2(0, 2, "=SUMIF([.A1:.A9];\"a~?\";[.B1:.B9])").unwrap();
    assert_eq!(outcome(&table, 0, 2), n("4"));
    table.set_cell_formula_v2(0, 2, "=AVERAGEIF([.A1:.A9];\"a*\";[.B1:.B9])").unwrap();
    assert_eq!(outcome(&table, 0, 2), n("3.25"));
}

#[test]
fn unrelated_v2_does_not_canonicalize_legacy_rich_numeric_baseline_or_allow_cycles() {
    let mut table = Table::new(1, 2, 200., 20.);
    let mut body = TextBody::from_text("7.00");
    body.paragraphs[0].runs = vec![deckcraft_model::Run::new("7."), deckcraft_model::Run::new("00")];
    let formula = Arc::new(CellFormula::number("=7".into(), "7.00".into(), &body).unwrap());
    table.cell_mut(0, 0).unwrap().text = body.clone();
    table.cell_mut(0, 0).unwrap().formula = Some(formula.clone());
    table.set_cell_formula_v2(0, 1, "=1").unwrap();
    assert!(Arc::ptr_eq(table.cell(0, 0).unwrap().formula.as_ref().unwrap(), &formula));
    assert_eq!(table.cell(0, 0).unwrap().text, body);
    table.validate_formulas().unwrap();
    assert!(!table.recalculate().unwrap());
    let cell = table.cell_mut(0, 0).unwrap();
    cell.text = TextBody::from_text("#DEPEND!");
    cell.formula = Some(Arc::new(CellFormula::from_display("=[.A1]".into(), e(FormulaError::DependencyError), &cell.text).unwrap()));
    assert!(table.validate_formulas().unwrap_err().to_string().contains("cycle"));
}

#[test]
fn structural_legacy_rewrites_preserve_profile_and_broken_references_are_explicit() {
    let formula = CellFormula::number("=[.A1]+1".into(), "3.00".into(), &TextBody::from_text("3.00")).unwrap();
    assert_eq!(formula.rewritten_axis(false, 0, 1, false).unwrap(), "=[.A2]+1");
    assert_eq!(formula.relocated_v2(1, 1).unwrap(), "=[.B2]+1");
    let mut table = Table::new(2, 2, 200., 20.);
    number(&mut table, 1, 0, "2");
    let cell = table.cell_mut(0, 1).unwrap();
    cell.text = TextBody::from_text("3.00");
    cell.formula = Some(Arc::new(
        CellFormula::from_profile_display("exact-decimal-v1", formula.rewritten_axis(false, 0, 1, false).unwrap(), n("3.00"), &cell.text).unwrap(),
    ));
    table.validate_formulas().unwrap();
    assert!(!table.recalculate().unwrap());
    assert_eq!(table.cell(0, 1).unwrap().text.text(), "3.00");
    let source = formula.rewritten_axis(false, 0, 1, true).unwrap();
    assert_eq!(source, "=[#REF!]+1");
    let cell = table.cell_mut(0, 1).unwrap();
    cell.formula = Some(Arc::new(CellFormula::from_profile_display("exact-decimal-v1", source, n("3.00"), &cell.text).unwrap()));
    table.recalculate().unwrap();
    assert_eq!(outcome(&table, 0, 1), e(FormulaError::InvalidReference));
    assert_eq!(table.cell(0, 1).unwrap().formula.as_ref().unwrap().profile(), "exact-decimal-v1");
    table.validate_formulas().unwrap();
    assert!(CellFormula::from_display("=1".into(), e(FormulaError::InvalidReference), &TextBody::from_text("#REF!")).is_err());
}

#[test]
fn formatted_legacy_profile_keeps_legacy_value_coercion_and_native_authority() {
    use deckcraft_model::table::{NumberFormat, NumberStyle};
    let mut table = Table::new(1, 2, 200., 20.);
    number(&mut table, 0, 0, "7");
    table.set_cell_formula(0, 1, "=[.A1]").unwrap();
    let cell = table.cell_mut(0, 1).unwrap();
    cell.number_format = Some(Arc::new(NumberFormat {
        style: NumberStyle::Currency {
            decimal_places: 2,
            min_decimal_places: 2,
            grouping: false,
            code: "USD".into(),
            symbol: "$".into(),
            symbol_after: false,
            space: false,
        },
        ..Default::default()
    }));
    let body = TextBody::from_text("$7.00");
    cell.formula = Some(Arc::new(cell.formula.as_ref().unwrap().with_number_format_display(&body).unwrap()));
    cell.text = body;
    assert!(!cell.formula.as_ref().unwrap().is_v2());
    assert!(cell.formula.as_ref().unwrap().uses_formatted_display());
    table.validate_formulas().unwrap();
    let restored: Table = serde_json::from_slice(&serde_json::to_vec(&table).unwrap()).unwrap();
    restored.validate_formulas().unwrap();
    let cell = table.cell_mut(0, 0).unwrap();
    cell.data =
        Some(Arc::new(CellConstant::from_display(CellValue::Number { kind: CellNumberKind::Percentage, lexical: "7".into() }, &cell.text).unwrap()));
    table.recalculate().unwrap();
    assert_eq!(outcome(&table, 0, 1), e(FormulaError::InputNotNumeric));
    assert_eq!(table.cell(0, 1).unwrap().text.text(), "#VALUE!");
}

#[test]
fn repeated_fractional_power_exhaustion_remains_uncatchable_and_atomic() {
    let mut table = Table::new(1, 1, 100., 20.);
    let before = table.clone();
    let terms = std::iter::repeat_n("2^0.1", 32).collect::<Vec<_>>().join(";");
    assert!(table.set_cell_formula_v2(0, 0, &format!("=IFERROR(SUM({terms});9)")).is_err());
    assert_eq!(table, before);
}
