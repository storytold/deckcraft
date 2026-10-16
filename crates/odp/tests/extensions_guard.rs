//! Independent extension coverage guards; contributor-original ODF XML only.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_odp::{MIME, export, import};
use std::io::{Cursor, Write};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0'";
fn package(declarations: &str, shapes: &str, styles: Option<&str>) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} {declarations} office:version='1.3'><office:body><office:presentation><draw:page>{shapes}</draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = styles.map(|s| format!("<office:document-styles {NS} xmlns:v='urn:review:style-extension' office:version='1.3'><office:styles>{s}</office:styles></office:document-styles>"));
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, value) in [
        ("mimetype", MIME),
        ("content.xml", &content),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ]
    .into_iter()
    .chain(styles.as_deref().map(|s| ("styles.xml", s)))
    {
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(value.as_bytes()).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn assert_guarded(bytes: &[u8]) {
    let p = import(bytes).unwrap();
    assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"), "foreign semantic content must be reported");
    assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), bytes);
    assert!(export(&p).is_err(), "source recovery must not be mistaken for edited export preservation");
}
const RECT: &str = "<draw:rect svg:width='72pt' svg:height='72pt'/>";

#[test]
fn unused_foreign_namespace_declarations_alone_do_not_warn() {
    let bytes = package("xmlns:v='urn:review:unused' xmlns:q='urn:review:qnames'", RECT, Some(""));
    let p = import(&bytes).unwrap();
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "declarations are namespace context, not unsupported attributes: {:?}", p.tags);
    assert!(export(&p).is_ok());
}
#[test]
fn foreign_attribute_on_a_supported_shape_warns_and_retains_exact_source() {
    assert_guarded(&package("xmlns:v='urn:review:shape-extension'", "<draw:rect svg:width='72pt' svg:height='72pt' v:tracking='a&amp;b'/>", None));
}
#[test]
fn foreign_attribute_in_used_external_style_warns_and_retains_exact_source() {
    assert_guarded(&package(
        "",
        "<draw:rect draw:style-name='Known' svg:width='72pt' svg:height='72pt'/>",
        Some("<style:style style:name='Known' style:family='graphic'><style:graphic-properties v:rendering='q:Mode'/></style:style>"),
    ));
}
#[test]
fn foreign_child_inside_a_supported_shape_warns_and_retains_exact_source() {
    assert_guarded(&package(
        "xmlns:v='urn:review:shape-child'",
        "<draw:rect svg:width='72pt' svg:height='72pt'><v:payload><!--unchanged-->extension</v:payload></draw:rect>",
        None,
    ));
}
#[test]
fn inline_foreign_wrapper_keeps_editable_text_but_reports_lost_wrapper_semantics() {
    let bytes = package(
        "xmlns:v='urn:review:inline'",
        "<draw:frame svg:width='144pt' svg:height='72pt'><draw:text-box><text:p>Before<v:tracked> middle </v:tracked>after</text:p></draw:text-box></draw:frame>",
        None,
    );
    assert_guarded(&bytes);
    let p = import(&bytes).unwrap();
    let body = p.slides[0].shapes[0].text.as_ref().unwrap();
    assert_eq!(body.paragraphs[0].runs.iter().map(|run| run.text.as_str()).collect::<String>(), "Before middle after");
}
#[test]
fn namespace_aliases_are_classified_by_uri_and_rebinding_remains_foreign() {
    let bytes = package("xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0'", "<d:rect svg:width='72pt' svg:height='72pt'/>", None);
    let p = import(&bytes).unwrap();
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"));
    assert_guarded(&package("", "<draw:rect svg:width='72pt' svg:height='72pt'><draw:rect xmlns:draw='urn:review:rebound'/></draw:rect>", None));
}
#[test]
fn namespace_declarations_still_count_toward_the_attribute_budget() {
    let declarations = (0..140).map(|i| format!("xmlns:v{i}='urn:review:{i}' ")).collect::<String>();
    assert!(import(&package(&declarations, RECT, None)).is_err());
}

#[test]
fn partially_recognized_loext_namespace_keeps_unimplemented_semantics_guarded() {
    const LOEXT: &str = "xmlns:q='urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0'";
    for (declarations, shape, style) in [
        (LOEXT, "<draw:rect svg:width='72pt' svg:height='72pt' q:custom='1'/>", ""),
        (LOEXT, "<draw:rect svg:width='72pt' svg:height='72pt'><q:unknown/></draw:rect>", ""),
        (
            LOEXT,
            RECT,
            "<style:style style:name='P' style:family='paragraph'><q:graphic-properties draw:fill='solid' draw:fill-color='#123456'/><q:unknown/></style:style>",
        ),
        (
            LOEXT,
            RECT,
            "<style:style style:name='P' style:family='paragraph'><q:graphic-properties draw:fill='solid' draw:fill-color='#123456' q:custom='1'/></style:style>",
        ),
    ] {
        // styles.xml declares only its own bindings: repeat the alias there explicitly.
        let style = style.replace("<style:style ", &format!("<style:style {LOEXT} "));
        assert_guarded(&package(declarations, shape, Some(&style)));
    }
}

#[test]
fn paragraph_fill_exception_requires_exact_context_values_and_children() {
    const URI: &str = "urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0";
    for (family, payload) in [
        ("graphic", "<q:graphic-properties draw:fill='none'/>"),
        ("paragraph", "<q:graphic-properties draw:fill='gradient'/>"),
        ("paragraph", "<q:graphic-properties draw:fill='solid'/>"),
        ("paragraph", "<q:graphic-properties draw:fill='solid' draw:fill-color='#xyzxyz'/>"),
        ("paragraph", "<q:graphic-properties draw:fill='none' draw:opacity='50%'/>"),
        ("paragraph", "<q:graphic-properties draw:fill='none'><text:p/></q:graphic-properties>"),
        ("paragraph", "<q:graphic-properties draw:fill='none'>payload</q:graphic-properties>"),
    ] {
        let style = format!("<style:style xmlns:q='{URI}' style:name='P' style:family='{family}'>{payload}</style:style>");
        let result = import(&package("", RECT, Some(&style)));
        if let Ok(p) = result {
            assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"), "{payload}");
            assert!(export(&p).is_err(), "{payload}");
        }
    }
    assert_guarded(&package(
        "",
        RECT,
        Some(
            "<style:style xmlns:q='urn:foreign:lookalike' style:name='P' style:family='paragraph'><q:graphic-properties draw:fill='none'/></style:style>",
        ),
    ));
}

#[test]
fn namespace_aliases_cannot_supply_duplicate_fill_attributes() {
    let styles = "<style:style xmlns:q='urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0' xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' style:name='P' style:family='paragraph'><q:graphic-properties draw:fill='none' d:fill='solid' draw:fill-color='#123456'/></style:style>";
    assert!(import(&package("", RECT, Some(styles))).unwrap_err().0.contains("duplicate expanded XML attribute"));
}
