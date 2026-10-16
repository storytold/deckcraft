//! Independent exact-authority, display-consistency and hostile serde regression fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::{
    table::{CellConstant, CellConstantState, CellNumberKind, CellValue},
    text::{Paragraph, Run, RunKind, RunProps},
    *,
};
use std::sync::Arc;

fn number(s: &str) -> CellValue {
    CellValue::Number { kind: CellNumberKind::Float, lexical: s.into() }
}
fn constant(value: CellValue, shown: &str) -> CellConstant {
    CellConstant::from_display(value, &TextBody::from_text(shown)).unwrap()
}

#[test]
fn numeric_authority_retains_digits_sign_exponent_and_currency_presence() {
    for lexical in ["9007199254740993", "-0.000", "+.125", "0001.2500E+02", "4.9406564584124654e-324"] {
        let original = constant(number(lexical), "rounded differently");
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: CellConstant = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.value(), &number(lexical));
        assert_eq!(decoded.display_baseline(), &["rounded differently"]);
    }
    let none = constant(CellValue::Number { kind: CellNumberKind::Currency { currency: None }, lexical: "1.25".into() }, "$1.25");
    let empty = constant(CellValue::Number { kind: CellNumberKind::Currency { currency: Some("".into()) }, lexical: "1.25".into() }, "$1.25");
    assert_ne!(none, empty);
    assert!(!serde_json::to_string(&none).unwrap().contains("\"currency\":\"\""));
    assert!(serde_json::to_string(&empty).unwrap().contains("\"currency\":\"\""));
}

#[test]
fn numerical_and_xml_scalar_rejections_never_invent_zero_or_strip_characters() {
    for lexical in ["1e-999", "-1e-999", "1e309", "NaN", "+INF", "0x1", "1_2", " 1", "1 ", "1\n", "１", ".", "+", "1E-"] {
        assert!(CellConstant::from_display(number(lexical), &TextBody::from_text("visible")).is_err());
    }
    for invalid in ["\0", "\u{b}", "\u{fffe}", "\u{ffff}"] {
        assert!(CellConstant::from_display(CellValue::String { value: invalid.into() }, &TextBody::default()).is_err());
        assert!(
            CellConstant::from_display(
                CellValue::Number { kind: CellNumberKind::Currency { currency: Some(invalid.into()) }, lexical: "0".into() },
                &TextBody::default()
            )
            .is_err()
        );
    }
    assert!(CellConstant::from_display(CellValue::String { value: "\t\r\n漢ب\u{1f680}".into() }, &TextBody::default()).is_ok());
}

#[test]
fn logical_run_splits_preserve_value_but_paragraphs_unicode_and_dynamic_runs_do_not() {
    let saved = constant(CellValue::Boolean { value: true }, "e\u{301}漢\u{b}ب\nsecond");
    let mut body = TextBody::from_text("e\u{301}漢\u{b}ب\nsecond");
    body.paragraphs[0].runs = vec![
        Run::new("e"),
        Run::with("\u{301}漢", RunProps { bold: Some(true), ..Default::default() }),
        Run { kind: RunKind::Break, ..Run::new("ignored") },
        Run::new("ب"),
    ];
    assert!(saved.matches_display(&body).unwrap());
    assert!(!saved.matches_display(&TextBody::from_text("é漢\u{b}ب\nsecond")).unwrap());
    assert!(!saved.matches_display(&TextBody::from_text("e\u{301}漢\nب\nsecond")).unwrap());
    for kind in [RunKind::Field { field: "slidenum".into() }, RunKind::Math { omml: "<math/>".into() }] {
        let mut cell = Cell::new("shown");
        cell.data = Some(Arc::new(constant(CellValue::Boolean { value: true }, "shown")));
        cell.text.paragraphs[0].runs[0].kind = kind;
        assert!(matches!(cell.constant_state().unwrap(), CellConstantState::Stale));
        assert!(cell.reconcile_constant().unwrap());
        assert!(cell.data.is_none());
    }
}

#[test]
fn local_reconciliation_keeps_arc_for_noop_and_clears_only_changed_display() {
    let mut cell = Cell::new("shown");
    let data = Arc::new(constant(CellValue::String { value: "authoritative".into() }, "shown"));
    cell.data = Some(data.clone());
    cell.edit_display(|body| body.paragraphs[0].runs[0].props.italic = Some(true)).unwrap();
    assert!(Arc::ptr_eq(cell.data.as_ref().unwrap(), &data));
    cell.edit_display(|body| body.paragraphs[0].runs = vec![Run::new("sh"), Run::new("own")]).unwrap();
    assert!(Arc::ptr_eq(cell.data.as_ref().unwrap(), &data));
    cell.edit_display(|body| body.paragraphs[0].runs[0].text.push('!')).unwrap();
    assert!(cell.data.is_none());
    assert_eq!(data.value(), &CellValue::String { value: "authoritative".into() });
}

#[test]
fn metadata_field_order_is_free_but_missing_duplicate_unknown_and_positional_records_refuse() {
    let good = r#"{"display":["shown"],"value":{"lexical":"9007199254740993","numberKind":{"kind":"float"},"kind":"number"}}"#;
    let data: CellConstant = serde_json::from_str(good).unwrap();
    assert_eq!(data.value(), &number("9007199254740993"));
    for bad in [
        r#"{"value":{"kind":"boolean","value":true}}"#,
        r#"{"display":[],"display":[],"value":{"kind":"boolean","value":true}}"#,
        r#"{"display":[],"value":{"kind":"boolean","kind":"boolean","value":true}}"#,
        r#"{"display":[],"value":{"kind":"string","value":"s","lexical":"1"}}"#,
        r#"{"display":[],"value":{"kind":"number","lexical":"1","numberKind":{"kind":"float","currency":"USD"}}}"#,
        r#"[{"kind":"boolean","value":true},["shown"]]"#,
        r#"{"display":null,"value":{"kind":"boolean","value":true}}"#,
        r#"{"display":[],"value":{"kind":"boolean","value":true},"unknown":0}"#,
    ] {
        assert!(serde_json::from_str::<CellConstant>(bad).is_err());
    }
    for bad in [r#"["float"]"#, r#"{"kind":"currency","currency":null,"currency":"USD"}"#, r#"{"kind":"currency","unexpected":0}"#] {
        assert!(serde_json::from_str::<CellNumberKind>(bad).is_err());
    }
}

#[test]
fn unknown_and_duplicate_fields_fail_before_decoding_their_hostile_values() {
    for (bad, expected) in [
        (r#"{"unknown":{"unterminated": "#, "unknown cell constant field"),
        (r#"{"display":[],"display":{"unterminated": "#, "duplicate cell constant field"),
        (r#"{"value":{"kind":"boolean","value":true,"kind":{"unterminated": "#, "duplicate cell value field"),
    ] {
        let error = serde_json::from_str::<CellConstant>(bad).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("unterminated"));
    }
}

#[test]
fn exact_caps_empty_canonicalization_and_paragraph_vector_cost_remain_distinct() {
    let empty: CellConstant = serde_json::from_str(r#"{"value":{"kind":"boolean","value":true},"display":[]}"#).unwrap();
    assert_eq!(empty.display_baseline(), &[""]);
    assert!(empty.matches_display(&TextBody::default()).unwrap());
    assert!(!empty.matches_display(&TextBody::from_text("\n")).unwrap());
    let maximal = "漢".repeat(21_845) + "x";
    assert_eq!(maximal.len(), 65_536);
    assert!(CellConstant::from_display(CellValue::String { value: maximal.clone() }, &TextBody::from_text(&maximal)).is_ok());
    assert!(CellConstant::from_display(CellValue::String { value: maximal.clone() + "x" }, &TextBody::default()).is_err());
    assert!(CellConstant::from_display(CellValue::Boolean { value: true }, &TextBody::from_text(&(maximal + "x"))).is_err());
    let body = TextBody { paragraphs: vec![Paragraph::default(); 1024], ..Default::default() };
    let many = CellConstant::from_display(CellValue::Boolean { value: true }, &body).unwrap();
    assert!(many.metadata_bytes() >= 1024 * std::mem::size_of::<String>());
    let mut too_many = body;
    too_many.paragraphs.push(Paragraph::default());
    assert!(CellConstant::from_display(CellValue::Boolean { value: true }, &too_many).is_err());
    assert!(!many.matches_display(&too_many).unwrap());
    let too_many_json = format!(r#"{{"value":{{"kind":"boolean","value":true}},"display":[{},{{"malformed":"#, vec![r#""""#; 1024].join(","));
    let error = serde_json::from_str::<CellConstant>(&too_many_json).unwrap_err().to_string();
    assert!(error.contains("too many paragraphs"), "the 1025th item must refuse before decoding: {error}");
}
