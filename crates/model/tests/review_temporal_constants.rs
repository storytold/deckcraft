//! Independent contributor-original temporal grammar and accounting witnesses.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::{
    Presentation, Shape, ShapeId, ShapeKind, Table, TextBody,
    table::{CellConstant, CellValue, validate_constants},
};
use std::sync::Arc;

fn value(kind: &str, lexical: &str) -> CellValue {
    match kind {
        "date" => CellValue::Date { lexical: lexical.into() },
        "duration" => CellValue::Duration { lexical: lexical.into() },
        _ => panic!("test kind"),
    }
}

#[test]
fn independent_calendar_matrix_checks_both_date_alternatives() {
    // Expected leap status is specified independently, including both century rules.
    for (year, february) in [(1, 28), (4, 29), (1600, 29), (1700, 28), (1800, 28), (1900, 28), (2000, 29), (2100, 28), (2400, 29), (9999, 28)] {
        for (month, last) in [(1, 31), (2, february), (3, 31), (4, 30), (5, 31), (6, 30), (7, 31), (8, 31), (9, 30), (10, 31), (11, 30), (12, 31)] {
            for day in 0..=32 {
                for suffix in ["", "Z", "+13:59", "-14:00", "T24:00:00.000+00:00", "T23:59:59.000001-00:00"] {
                    let lexical = format!("{year:04}-{month:02}-{day:02}{suffix}");
                    assert_eq!(value("date", &lexical).validate().is_ok(), (1..=last).contains(&day), "{lexical}");
                }
            }
        }
    }
    for lexical in [
        "2004-13-01",
        "2004-01-01T00:00:00Z+00:00",
        "2004-01-01T00:00:00+00:00Z",
        "2004-01-01T00:00:00.0.0",
        "2004-01-01T24:01:00",
        "2004-01-01T24:00:00.00001",
        "2004-01-01+13:60",
        "2004-01-01-14:01",
    ] {
        assert!(value("date", lexical).validate().is_err(), "{lexical}");
    }
}

#[test]
fn every_duration_component_subset_preserves_lexical_identity() {
    // Six independent presence bits cover every omission of Y/M/D/H/M/S.
    let date = ["0007Y", "0013M", "0041D"];
    let time = ["0073H", "0089M", "0097.0001200S"];
    for mask in 1u8..64 {
        let mut token = String::from("P");
        for (index, component) in date.iter().enumerate() {
            if mask & (1 << index) != 0 {
                token.push_str(component);
            }
        }
        if mask & 56 != 0 {
            token.push('T');
            for (index, component) in time.iter().enumerate() {
                if mask & (1 << (index + 3)) != 0 {
                    token.push_str(component);
                }
            }
        }
        for lexical in [token.clone(), format!("-{token}"), format!(" \r\n\t{token}\t ")] {
            let expected = value("duration", &lexical);
            expected.validate().unwrap();
            let roundtrip: CellValue = serde_json::from_str(&serde_json::to_string(&expected).unwrap()).unwrap();
            assert_eq!(roundtrip, expected);
        }
    }
    for lexical in ["P1Y2M3DT4H5M6S7S", "P1Y2MT3M4H", "P1Y2M3DT0.0H", "P1Y2M3D4S", "PT1M2Y", "P1D\tT2H", "P1DT2H\u{85}", "\u{2003}PT2H"] {
        assert!(value("duration", lexical).validate().is_err(), "{lexical:?}");
    }
}

#[test]
fn escaped_wire_tokens_obey_decoded_raw_bounds_and_early_field_rejection() {
    for kind in ["date", "duration"] {
        let core = if kind == "date" { "2004-02-29" } else { "PT007.000S" };
        let exact = format!("{}{core}", " ".repeat(256 - core.len()));
        let escaped = serde_json::to_string(&exact).unwrap().replace(' ', "\\u0020");
        let json = format!(r#"{{"lexical":{escaped},"kind":"{kind}"}}"#);
        assert_eq!(serde_json::from_str::<CellValue>(&json).unwrap(), value(kind, &exact));
        let too_long = escaped.replacen('"', "\"\\u0009", 1);
        assert!(serde_json::from_str::<CellValue>(&format!(r#"{{"lexical":{too_long},"kind":"{kind}"}}"#)).is_err());
        for tail in [r#","display":{"bad":["#, r#","currency":{"bad":["#, r#","lexical":{"bad":["#, r#","kind":{"bad":["#] {
            let error = serde_json::from_str::<CellValue>(&format!(r#"{{"kind":"{kind}","lexical":"{core}"{tail}"#)).unwrap_err().to_string();
            assert!(error.contains("field"), "{error}");
            assert!(!error.contains("EOF"), "unknown/duplicate payload was decoded: {error}");
        }
    }
}

#[test]
fn each_temporal_string_capacity_is_charged_per_shared_attachment() {
    for kind in ["date", "duration"] {
        let mut lexical = String::with_capacity(1024 * 1024);
        lexical.push_str(if kind == "date" { "2004-02-29" } else { "P7M" });
        let scalar = if kind == "date" { CellValue::Date { lexical } } else { CellValue::Duration { lexical } };
        let body = TextBody::from_text("independent cache");
        let precharge = CellConstant::metadata_bytes_for_display(&scalar, &body).unwrap();
        let constant = Arc::new(CellConstant::from_display(scalar, &body).unwrap());
        assert!(precharge >= constant.metadata_bytes());
        assert!(constant.metadata_bytes() >= 1024 * 1024);
        let mut table = Table::new(1, 1, 100.0, 30.0);
        table.rows[0].cells[0].text = body;
        table.rows[0].cells[0].data = Some(constant.clone());
        let shape = Shape { id: ShapeId(721), kind: ShapeKind::Table(table), ..Default::default() };
        let mut p = Presentation::default();
        Arc::make_mut(&mut p.slides[0]).shapes = vec![shape.clone(); 2];
        assert!(validate_constants(&p).unwrap() >= 2 * constant.metadata_bytes());
        Arc::make_mut(&mut p.slides[0]).shapes = vec![shape; 33];
        assert!(validate_constants(&p).unwrap_err().to_string().contains("metadata"));
    }
}
