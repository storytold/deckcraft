//! Original calendar/duration and hostile wire-format fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_model::{
    TextBody,
    table::{Cell, CellConstant, CellValue, MAX_TEMPORAL_BYTES},
};
use std::sync::Arc;

fn date(lexical: &str) -> CellValue {
    CellValue::Date { lexical: lexical.into() }
}
fn duration(lexical: &str) -> CellValue {
    CellValue::Duration { lexical: lexical.into() }
}

#[test]
fn exact_calendar_lexicals_preserve_timezones_fraction_and_edge_whitespace() {
    for lexical in [
        "0001-01-01",
        "9999-12-31Z",
        "2000-02-29",
        "2024-02-29+14:00",
        "2024-02-29-14:00",
        "2024-02-29+00:00",
        "2024-02-29-00:00",
        "2024-02-29T24:00:00.000Z",
        "2024-02-29T01:02:03.1200",
        " \t2024-02-29T01:02:03.000-05:30\r\n",
    ] {
        let value = date(lexical);
        value.validate().unwrap();
        let data = CellConstant::from_display(value.clone(), &TextBody::from_text("calendar label")).unwrap();
        let reopened: CellConstant = serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();
        assert_eq!(reopened.value(), &value);
        assert_eq!(reopened.display_baseline(), &["calendar label"]);
    }
    for lexical in [
        "",
        "1900-02-29",
        "0000-01-01",
        "-0001-01-01",
        "10000-01-01",
        "+2024-01-01",
        "2024-2-29",
        "2024-04-31",
        "2024-00-01",
        "2024-01-00",
        "2024-01-01+14:01",
        "2024-01-01-15:00",
        "2024-01-01z",
        "2024-01-01+00:0",
        "2024-01-01+00:000",
        "2024-01-01Zx",
        "2024-01-01T24:00:00.001",
        "2024-01-01T23:59:60",
        "2024-01-01T00:00:00.",
        "2024-01-01 T00:00:00",
        "\u{a0}2024-01-01",
        "２０２４-01-01",
        "2024-01-01\0",
    ] {
        assert!(date(lexical).validate().is_err(), "{lexical:?}");
    }
}

#[test]
fn duration_components_are_exact_unbounded_magnitudes_with_a_bounded_token() {
    for lexical in
        ["P0D", "PT0S", "-PT0.000S", "P1Y2M", "P1347M", "P1Y2M3DT4H5M6.700S", "PT60S", "PT999H", "P0001DT002H", "P1YT2S", "\t-P01DT02H03M04.500S\r\n"]
    {
        let value = duration(lexical);
        value.validate().unwrap();
        let reopened: CellValue = serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
        assert_eq!(reopened, value);
    }
    let maximal = format!("P{}D", "9".repeat(MAX_TEMPORAL_BYTES - 2));
    assert_eq!(maximal.len(), 256);
    duration(&maximal).validate().unwrap();
    assert!(duration(&(maximal + " ")).validate().is_err());
    for lexical in [
        "",
        "P",
        "PT",
        "P1DT",
        "P1T",
        "P1DTT1S",
        "P1M1Y",
        "P1D1D",
        "PT1S1M",
        "PT1H1H",
        "P1H",
        "PT1D",
        "P1W",
        "P1.5Y",
        "PT1.5M",
        "PT.5S",
        "PT1.S",
        "+P1D",
        "P-1D",
        "--P1D",
        "PT1e2S",
        "PT1S+00:00",
        "P 1D",
        "p1d",
        "P１D",
        "\u{a0}P1D",
        "P1D\0",
    ] {
        assert!(duration(lexical).validate().is_err(), "{lexical:?}");
    }
}

#[test]
fn native_visitor_preserves_field_order_and_rejects_correlated_or_duplicate_fields() {
    for (kind, lexical) in [("date", "2024-02-29".to_owned()), ("duration", format!("P{}D", "0".repeat(254)))] {
        let escaped = serde_json::to_string(&lexical).unwrap();
        for json in [format!(r#"{{"kind":"{kind}","lexical":{escaped}}}"#), format!(r#"{{"lexical":{escaped},"kind":"{kind}"}}"#)] {
            let value: CellValue = serde_json::from_str(&json).unwrap();
            assert_eq!(value, if kind == "date" { date(&lexical) } else { duration(&lexical) });
        }
        for json in [
            format!(r#"{{"kind":"{kind}","lexical":{escaped},"value":true}}"#),
            format!(r#"{{"kind":"{kind}","lexical":{escaped},"numberKind":{{"kind":"float"}}}}"#),
            format!(r#"{{"kind":"{kind}","lexical":{escaped},"currency":"USD"}}"#),
            format!(r#"{{"kind":"{kind}","lexical":{escaped},"lexical":{escaped}}}"#),
            format!(r#"{{"kind":"{kind}","kind":"{kind}","lexical":{escaped}}}"#),
            format!(r#"{{"kind":"{kind}"}}"#),
            format!(r#"{{"kind":"{kind}","lexical":null}}"#),
        ] {
            assert!(serde_json::from_str::<CellValue>(&json).is_err(), "{json}");
        }
    }
    let too_long = format!("P{}D", "0".repeat(255));
    for json in [serde_json::json!({"kind":"duration","lexical":too_long}).to_string(), format!(r#"{{"lexical":"{too_long}","kind":"duration"}}"#)] {
        assert!(serde_json::from_str::<CellValue>(&json).is_err());
    }
    // Widening the temporary lexical visitor must not relax the old numeric limit.
    let number = format!("1.{}", "0".repeat(127));
    let json = serde_json::json!({"kind":"number","numberKind":{"kind":"float"},"lexical":number});
    assert!(serde_json::from_value::<CellValue>(json).is_err());
    for json in [r#"{"kind":"date","unknown":{"unterminated": "#, r#"{"kind":"duration","lexical":"P1D","lexical":{"unterminated": "#] {
        let error = serde_json::from_str::<CellValue>(json).unwrap_err().to_string();
        assert!(error.contains("field"));
        assert!(!error.contains("unterminated"));
    }
}

#[test]
fn cached_labels_and_reserved_capacity_keep_existing_edit_and_allocation_contracts() {
    for value in [date("2024-02-29"), duration("P1Y2M")] {
        let mut cell = Cell::new("unrelated label");
        let saved = Arc::new(CellConstant::from_display(value.clone(), &cell.text).unwrap());
        cell.data = Some(saved.clone());
        cell.edit_display(|body| body.paragraphs[0].runs[0].props.bold = Some(true)).unwrap();
        assert!(Arc::ptr_eq(cell.data.as_ref().unwrap(), &saved));
        cell.edit_display(|body| body.paragraphs[0].runs[0].text.push('!')).unwrap();
        assert!(cell.data.is_none());
        assert_eq!(saved.value(), &value);
    }
    let body = TextBody::from_text("");
    let mut large = String::with_capacity(1_000_000);
    large.push_str("P1D");
    let normal = CellConstant::metadata_bytes_for_display(&duration("P1D"), &body).unwrap();
    let reserved = CellConstant::metadata_bytes_for_display(&CellValue::Duration { lexical: large }, &body).unwrap();
    assert!(reserved > normal + 900_000);
}
