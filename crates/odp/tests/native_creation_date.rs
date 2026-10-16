//! Contributor-original native creation metadata; no external document assets.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::{Comment, Presentation, datetime::MAX_DATE_TIME_BYTES};
use deckcraft_odp::{export, import};
use std::{
    io::{Cursor, Read},
    sync::Arc,
};

fn part(bytes: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut value = String::new();
    archive.by_name(name).unwrap().read_to_string(&mut value).unwrap();
    value
}

fn roundtrip(created: &str) -> (Presentation, String) {
    let mut presentation = Presentation::default();
    presentation.props.created = created.into();
    let original = presentation.clone();
    let bytes = export(&presentation).unwrap();
    assert_eq!(presentation, original, "export changed its source");
    let meta = part(&bytes, "meta.xml");
    assert_eq!(meta.matches("<meta:creation-date>").count(), 1);
    assert!(!meta.contains("meta:name=\"Created\""), "creation must have one canonical metadata owner");
    assert!(!meta.contains("<dc:date>"), "creation must not invent document modification time");
    let reopened = import(&bytes).unwrap();
    assert_eq!(reopened.props.created, created, "creation lexical scalar changed");
    assert!(reopened.props.modified.is_empty());
    let second = import(&export(&reopened).unwrap()).unwrap();
    assert_eq!(second.props.created, created, "second interchange changed the creation scalar");
    assert!(second.props.modified.is_empty());
    (reopened, meta)
}

fn refused(created: String) -> String {
    let mut presentation = Presentation::default();
    presentation.props.created = created;
    let original = presentation.clone();
    let error = match export(&presentation) {
        Err(error) => error,
        Ok(_) => panic!("unsupported native creation timestamp was accepted"),
    };
    assert_eq!(presentation, original, "refusal changed its source");
    error.to_string()
}

#[test]
fn supported_creation_lexical_dates_survive_two_interchanges_exactly() {
    for created in [
        "2032-02-29T09:08:07Z",
        "0001-01-01T00:00:00",
        "2000-02-29T24:00:00.000+14:00",
        "2032-02-29T09:08:07.123456789-13:59",
        "\t2032-02-29T09:08:07Z\r\n",
        "\r2032-02-29T09:08:07Z\r",
    ] {
        let (_, meta) = roundtrip(created);
        assert!(!meta.contains('\r'), "creation CR must be a character reference");
        if created.contains('\r') {
            assert!(meta.contains("&#13;"));
        }
    }
}

#[test]
fn creation_scalar_accepts_exact_byte_cap_including_outer_whitespace() {
    let exact = format!("2032-02-29T09:08:07.{}Z", "0".repeat(235));
    assert_eq!(exact.len(), MAX_DATE_TIME_BYTES);
    roundtrip(&exact);
    let padded = format!("\t2032-02-29T09:08:07.{}Z\r\n", "0".repeat(232));
    assert_eq!(padded.len(), MAX_DATE_TIME_BYTES);
    roundtrip(&padded);
}

#[test]
fn invalid_creation_profile_and_xml_characters_refuse_without_mutation() {
    for created in [
        " ",
        "2031-02-29T09:08:07Z",
        "1900-02-29T09:08:07Z",
        "0000-01-01T00:00:00Z",
        "2032-13-01T00:00:00Z",
        "2032-01-00T00:00:00Z",
        "2032-02-29",
        "2032-02-29t09:08:07Z",
        "2032-02-29T09:08:07z",
        "2032-02-29T09:08:60Z",
        "2032-02-29T24:00:00.001Z",
        "2032-02-29T24:01:00Z",
        "2032-02-29T09:08:07+14:01",
        "2032-02-29T09:08:07+15:00",
        "2032-02-29T09:08:07.Z",
        "2032-02-29T09:\t08:07Z",
        "2032-02-29T09:08:07Z\u{a0}",
        "2032-02-29T09:08:07Z\0",
        "2032-02-29T09:08:07Z\u{b}",
        "2032-02-29T09:08:07Z\u{fffe}",
        "2032-02-29T09:08:07Z</meta:creation-date><dc:date>2032-02-29T09:08:07Z",
    ] {
        let error = refused(created.into());
        assert!(error.contains("document creation-date outside the supported XML Schema dateTime profile"), "{error}");
    }
}

#[test]
fn oversized_creation_refuses_before_metadata_serialization() {
    for created in [
        format!("2032-02-29T09:08:07.{}Z", "0".repeat(236)),
        format!("2032-02-29T09:08:07.{}Z", "0".repeat(1024 * 1024)),
        "λ".repeat(MAX_DATE_TIME_BYTES / 2 + 1),
    ] {
        let error = refused(created);
        assert!(error.contains("document creation-date scalar exceeds size limit"), "{error}");
    }
}

#[test]
fn dated_annotation_does_not_invent_document_creation_or_modification_time() {
    let mut presentation = Presentation::default();
    Arc::make_mut(&mut presentation.slides[0]).comments.push(Comment {
        text: "Independent annotation".into(),
        date: "2024-02-29T24:00:00Z".into(),
        ..Default::default()
    });
    let original = presentation.clone();
    let bytes = export(&presentation).unwrap();
    assert_eq!(presentation, original);
    let meta = part(&bytes, "meta.xml");
    assert!(!meta.contains("meta:creation-date"));
    assert!(!meta.contains("<dc:date>"));
    let reopened = import(&bytes).unwrap();
    assert!(reopened.props.created.is_empty());
    assert!(reopened.props.modified.is_empty());
    assert_eq!(reopened.slides[0].comments[0].date, "2024-02-29T24:00:00Z");
}

#[test]
fn document_creation_annotation_date_and_opaque_modified_stay_independent() {
    let mut presentation = Presentation::default();
    presentation.props.created = "\t2032-02-29T09:08:07Z\r\n".into();
    presentation.props.modified = "Opaque modified label".into();
    Arc::make_mut(&mut presentation.slides[0]).comments.push(Comment {
        text: "Separate creation times".into(),
        date: "2024-02-29T24:00:00Z".into(),
        display_date: Some("Rehearsal".into()),
        ..Default::default()
    });
    let original = presentation.clone();
    let bytes = export(&presentation).unwrap();
    assert_eq!(presentation, original);
    let meta = part(&bytes, "meta.xml");
    assert!(meta.contains("<meta:creation-date>\t2032-02-29T09:08:07Z&#13;\n</meta:creation-date>"));
    assert!(meta.contains("<meta:user-defined meta:name=\"Modified\" meta:value-type=\"string\">Opaque modified label</meta:user-defined>"));
    assert!(!meta.contains("<dc:date>"), "opaque modified label must not become a modification-date claim");
    let reopened = import(&bytes).unwrap();
    assert_eq!(reopened.props.created, presentation.props.created);
    assert!(reopened.props.modified.is_empty(), "existing opaque-modified import policy remains unchanged");
    assert_eq!(reopened.slides[0].comments[0].date, "2024-02-29T24:00:00Z");
    assert_eq!(reopened.slides[0].comments[0].display_date.as_deref(), Some("Rehearsal"));
    if let Ok(directory) = std::env::var("DECKCRAFT_COMPLETION_QA_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(std::path::Path::new(&directory).join("native-creation-date.odp"), bytes).unwrap();
    }
}
