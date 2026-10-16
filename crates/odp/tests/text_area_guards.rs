//! Contributor-original ODF text-area ownership guards (ODF 1.3 §19.220).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::ShapeKind;
use deckcraft_odp::{MIME, export, import};
use std::io::{Cursor, Write};

fn package(area: &str, bounds: &str, path: bool, equations: &str) -> Vec<u8> {
    package_kind(area, bounds, path, equations, "rectangle")
}
fn package_kind(area: &str, bounds: &str, path: bool, equations: &str, kind: &str) -> Vec<u8> {
    let path = if path { "draw:enhanced-path='M 0 0 L 100 0 100 100 0 100 Z N'" } else { "" };
    let content = format!(
        "<office:document-content xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' office:version='1.3'><office:body><office:presentation><draw:page><draw:custom-shape svg:width='200pt' svg:height='100pt'><draw:enhanced-geometry draw:type='{kind}' svg:viewBox='{bounds}' draw:text-areas='{area}' {path}>{equations}</draw:enhanced-geometry></draw:custom-shape></draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME),
        ("content.xml", content.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(data.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn full_frame_areas_keep_editable_paths_and_presets_in_both_writing_modes() {
    for path in [false, true] {
        for (bounds, area, equations) in [
            ("0 0 100 100", "0 0 100 100", ""),
            ("0 0 100 100", "0 0 100 100 0 0 100 100", ""),
            ("10 20 100 80", "10 20 110 100", ""),
            ("0 0 100 100", "0 0 ?w ?h", "<draw:equation draw:name='w' draw:formula='width'/><draw:equation draw:name='h' draw:formula='height'/>"),
        ] {
            let deck = import(&package(area, bounds, path, equations)).unwrap();
            assert!(matches!(deck.slides[0].shapes[0].kind, ShapeKind::Shape));
            assert!(!deck.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", deck.tags);
            assert!(export(&import(&export(&deck).unwrap()).unwrap()).is_ok());
        }
    }
}

#[test]
fn custom_vertical_malformed_and_unresolved_areas_cannot_silently_disappear() {
    for path in [false, true] {
        for area in [
            "20 10 80 90",
            "0 0 100 100 20 10 80 90",
            "20 10 80 90 0 0 100 100",
            "",
            "0 0 100",
            "0 0 100 100 0",
            "0 0 100 100 0 0 100 100 0",
            "0 0 NaN 100",
            "0 0 ?missing 100",
            "0 0 1e999 100",
            "100 0 0 100",
        ] {
            let source = package(area, "0 0 100 100", path, "");
            let deck = import(&source).unwrap();
            assert!(matches!(deck.slides[0].shapes[0].kind, ShapeKind::Opaque { .. }), "{area}");
            assert!(deck.tags.iter().any(|(key, _)| key == "odp:warning"), "{area}");
            assert!(export(&deck).is_err(), "{area}");
            assert_eq!(
                deck.media.iter().find(|media| media.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(),
                source.as_slice()
            );
        }
    }
}

#[test]
fn full_frame_override_cannot_disappear_into_an_inset_preset_text_box() {
    for kind in ["circle", "diamond", "round-rectangle"] {
        let source = package_kind("0 0 100 100", "0 0 100 100", false, "", kind);
        let deck = import(&source).unwrap();
        assert!(matches!(deck.slides[0].shapes[0].kind, ShapeKind::Opaque { .. }), "{kind}");
        assert!(deck.tags.iter().any(|(key, _)| key == "odp:warning"));
        assert!(export(&deck).is_err());
        assert_eq!(
            deck.media.iter().find(|media| media.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(),
            source.as_slice()
        );
        // An explicit enhanced path already has a full-frame native text box.
        let explicit = import(&package_kind("0 0 100 100", "0 0 100 100", true, "", kind)).unwrap();
        assert!(matches!(explicit.slides[0].shapes[0].kind, ShapeKind::Shape));
        assert!(export(&explicit).is_ok());
    }
}
