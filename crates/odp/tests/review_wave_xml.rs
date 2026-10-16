//! Independent source-preservation security regressions; contributor-original XML.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::ShapeKind;
use deckcraft_odp::{MIME, import};
use std::io::{Cursor, Write};

fn package(declarations: &str, shapes: &str) -> Vec<u8> {
    let xml = format!(
        "<office:document-content xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' {declarations}><office:body><office:presentation><draw:page>{shapes}</draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, value) in [
        ("mimetype", MIME),
        ("content.xml", &xml),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(value.as_bytes()).unwrap();
    }
    z.finish().unwrap().into_inner()
}

#[test]
fn opaque_fragment_keeps_unknown_qname_bindings_and_exact_lexical_content() {
    let bytes = package(
        "xmlns:v='urn:review:object' xmlns:q='urn:review:qname&amp;value'",
        "<draw:plugin q:type='q:Kind'><!--private lexical boundary--><v:child xmlns:q='urn:review:rebound' q:type='q:Other'/><![CDATA[<not-an-element/>]]><?retained instruction?></draw:plugin>",
    );
    let p = import(&bytes).unwrap();
    let ShapeKind::Opaque { xml, .. } = &p.slides[0].shapes[0].kind else { panic!("unknown object must stay opaque") };
    assert!(xml.contains("xmlns:v=\"urn:review:object\""));
    assert!(xml.contains("xmlns:q=\"urn:review:qname&amp;value\""));
    assert!(xml.contains("q:type='q:Kind'"));
    assert!(xml.contains(
        "<!--private lexical boundary--><v:child xmlns:q='urn:review:rebound' q:type='q:Other'/><![CDATA[<not-an-element/>]]><?retained instruction?>"
    ));
}

#[test]
fn inherited_namespaces_are_charged_to_cumulative_opaque_storage_budget() {
    // The ~1MiB source expands beyond 128MiB if every opaque fragment independently
    // receives all inherited bindings. The budget must include those inserted bytes.
    let uri = format!("urn:review:{}", "x".repeat(8170));
    let declarations = (0..120).map(|i| format!("xmlns:v{i}='{uri}' ")).collect::<String>();
    let bytes = package(&declarations, &"<draw:plugin/>".repeat(150));
    assert!(import(&bytes).is_err(), "expanded standalone namespace contexts must count toward MAX_TOTAL before unbounded allocation");
}

#[test]
fn foreign_page_content_is_visible_as_a_preservation_constraint() {
    let bytes = package("xmlns:v='urn:review:foreign-shape'", "<v:diagram><v:label>Retained extension data</v:label></v:diagram>");
    let p = import(&bytes).unwrap();
    assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"), "unknown extension content must not silently disappear on export");
    assert_eq!(p.media.iter().find(|media| media.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), bytes);
}
