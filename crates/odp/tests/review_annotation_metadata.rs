//! Independent contributor-original annotation boundary and XML semantics checks.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    io::{Cursor, Write},
    sync::Arc,
};

use deckcraft_model::{Comment, Presentation, comments};
use deckcraft_odp::{MIME, export, import};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'";

fn source(metadata: &str, body: &str, before: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:presentation><draw:page>{before}<draw:frame svg:width='120pt' svg:height='60pt'><draw:text-box><text:p><office:annotation>{metadata}{body}</office:annotation></text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut package = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, value) in [
        ("mimetype", MIME),
        ("content.xml", content.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        package.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        package.write_all(value.as_bytes()).unwrap();
    }
    package.finish().unwrap().into_inner()
}

fn native(comment: Comment) -> Presentation {
    let mut presentation = Presentation::default();
    Arc::make_mut(&mut presentation.slides[0]).comments = vec![comment];
    presentation
}

fn only_comment(presentation: &Presentation) -> &Comment {
    assert_eq!(presentation.slides.len(), 1);
    assert_eq!(presentation.slides[0].comments.len(), 1);
    &presentation.slides[0].comments[0]
}

fn roundtrip(comment: Comment) {
    let mut presentation = native(comment);
    comments::normalize(&mut presentation).unwrap();
    let original = presentation.clone();
    let reopened = import(&export(&presentation).unwrap()).unwrap();
    assert_eq!(presentation, original, "export mutated its input");
    assert!(
        only_comment(&reopened) == only_comment(&original),
        "exact comment metadata/body must survive after explicit local identity normalization"
    );
    assert!(!reopened.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", reopened.tags);
}

#[test]
fn annotation_body_retains_line_feeds_tabs_and_large_plain_text() {
    roundtrip(Comment {
        text: format!("A\nB\n\n\tC{}\n", "λ".repeat(5000)),
        author: "Independent reviewer".into(),
        display_date: Some("Rehearsal\r\nlabel".into()),
        ..Default::default()
    });
}

#[test]
fn native_body_carriage_returns_survive_standard_projection_and_verified_metadata() {
    for text in ["\rLeading", "A\rB", "A\r\nB", "Trailing\r"] {
        roundtrip(Comment { text: text.into(), ..Default::default() });
    }
}

#[test]
fn invalid_xml_body_refuses_without_mutating_native_document() {
    for invalid in ['\0', '\u{1}', '\u{1f}', '\u{fffe}', '\u{ffff}'] {
        for rich in [false, true] {
            let text = format!("Before{invalid}after");
            let body = rich.then(|| deckcraft_model::TextBody::from_text(&text));
            let presentation = native(Comment { text, body, ..Default::default() });
            let original = presentation.clone();
            assert!(export(&presentation).is_err(), "accepted XML-invalid body character {invalid:?}, rich {rich}");
            assert_eq!(presentation, original);
        }
    }
}

#[test]
fn logical_soft_breaks_preserve_plain_and_modeled_rich_run_ownership() {
    roundtrip(Comment { text: "Before\u{b}after".into(), ..Default::default() });
    let mut body = deckcraft_model::TextBody::from_text("Before");
    body.paragraphs[0].runs.push(deckcraft_model::Run {
        text: String::new(),
        props: Default::default(),
        kind: deckcraft_model::text::RunKind::Break,
    });
    body.paragraphs[0].runs.push(deckcraft_model::Run::new("after"));
    roundtrip(Comment { text: body.text(), body: Some(body), ..Default::default() });
}

#[test]
fn cdata_literal_newlines_normalize_while_character_reference_cr_remains_literal() {
    let bytes = source(
        "<dc:creator><![CDATA[A\rB\r\nC]]></dc:creator><meta:date-string><![CDATA[D\rE\r\nF]]>&#13;G</meta:date-string><meta:creator-initials><![CDATA[H\rI]]></meta:creator-initials>",
        "<text:p><![CDATA[J\rK\r\nL]]>&#13;M</text:p>",
        "",
    );
    let presentation = import(&bytes).unwrap();
    let comment = only_comment(&presentation);
    assert_eq!(comment.author, "A\nB\nC");
    assert_eq!(comment.initials, "H\nI");
    assert_eq!(comment.display_date.as_deref(), Some("D\nE\nF\rG"));
    assert_eq!(comment.text, "J K L M");
    let original = presentation.clone();
    let reopened = import(&export(&presentation).unwrap()).unwrap();
    let actual = only_comment(&reopened);
    let source_owner = presentation.slides[0].shape(comment.shape.unwrap()).unwrap();
    let actual_owner = reopened.slides[0].shape(actual.shape.unwrap()).unwrap();
    assert_eq!(actual_owner.xfrm, source_owner.xfrm);
    assert_eq!(actual_owner.text.as_ref().unwrap().text(), source_owner.text.as_ref().unwrap().text());
    let mut expected = comment.clone();
    expected.set_comment_anchor(comments::CommentAnchor::Shape { shape: actual_owner.id });
    assert!(actual == &expected, "decoded scalar, rich body and identities must survive owner ID remapping");
    assert_eq!(presentation, original);
}

#[test]
fn annotation_scalar_caps_count_decoded_utf8_bytes_across_xml_events() {
    for (name, field) in [("dc:creator", 0), ("meta:date-string", 1), ("meta:creator-initials", 2)] {
        let exact = format!("<{name}>{}</{name}>", "&#x3bb;".repeat(comments::MAX_SCALAR / "λ".len()));
        let presentation = import(&source(&exact, "<text:p/>", "")).unwrap();
        let comment = only_comment(&presentation);
        let value = match field {
            0 => comment.author.as_str(),
            1 => comment.display_date.as_deref().unwrap(),
            _ => comment.initials.as_str(),
        };
        assert_eq!(value.len(), comments::MAX_SCALAR);
        let overflow = format!("<{name}>{}<![CDATA[x]]></{name}>", "&#x3bb;".repeat(comments::MAX_SCALAR / "λ".len()));
        assert!(import(&source(&overflow, "<text:p/>", "")).is_err(), "accepted split-event overflow for {name}");
    }
}

#[test]
fn creation_timestamp_raw_byte_limit_includes_fraction_and_datatype_whitespace() {
    let exact = format!("2000-02-29T00:00:00.{}Z", "0".repeat(235));
    assert_eq!(exact.len(), 256);
    roundtrip(Comment { date: exact.clone(), display_date: Some(String::new()), ..Default::default() });
    for value in [format!(" {exact}"), format!("2000-02-29T00:00:00.{}Z", "0".repeat(236))] {
        assert!(import(&source(&format!("<dc:date>{value}</dc:date>"), "<text:p/>", "")).is_err());
        assert!(export(&native(Comment { date: value, ..Default::default() })).is_err());
    }
}

#[test]
fn namespace_alias_duplicates_refuse_and_foreign_collisions_cannot_override_metadata() {
    for name in ["creator", "date"] {
        let value = if name == "date" { "2026-10-16T01:02:03Z" } else { "Original reviewer" };
        let metadata = format!("<dc:{name}>{value}</dc:{name}><alias:{name} xmlns:alias='http://purl.org/dc/elements/1.1/'>{value}</alias:{name}>");
        assert!(import(&source(&metadata, "<text:p/>", "")).is_err());
    }
    let metadata = "<dc:creator>Allowed</dc:creator><meta:date-string>As entered</meta:date-string><meta:date-string xmlns:meta='urn:review:foreign'>Do not substitute</meta:date-string>";
    let presentation = import(&source(metadata, "<text:p/>", "")).unwrap();
    assert_eq!(only_comment(&presentation).display_date.as_deref(), Some("As entered"));
    assert!(presentation.tags.iter().any(|(key, _)| key == "odp:warning"));
    assert!(export(&presentation).is_err());
}

#[test]
fn rich_lists_and_links_preserve_bodies_while_nested_range_markers_refuse() {
    let invalid = source("", "<text:p><office:annotation-end office:name='range'/></text:p>", "");
    let error = match import(&invalid) {
        Err(error) => error,
        Ok(_) => panic!("range end inside an annotation must reject"),
    };
    assert!(error.to_string().contains("annotation-end inside annotation body"));
    for (body, expected_text, link) in [
        ("<text:list><text:list-item><text:p>List body</text:p></text:list-item></text:list>", "List body", None),
        (
            "<text:p><text:a xmlns:xlink='http://www.w3.org/1999/xlink' xlink:href='https://example.org/'>Linked</text:a></text:p>",
            "Linked",
            Some("https://example.org/"),
        ),
    ] {
        let presentation = import(&source("<dc:date>2024-02-29T24:00:00.000Z</dc:date><meta:date-string/>", body, "")).unwrap();
        let comment = only_comment(&presentation);
        assert_eq!(comment.display_date.as_deref(), Some(""));
        assert_eq!(comment.date, "2024-02-29T24:00:00.000Z");
        assert_eq!(comment.text, expected_text);
        let rich = comment.body.as_ref().expect("actual rich body");
        if let Some(url) = link {
            assert!(rich.paragraphs.iter().flat_map(|paragraph| &paragraph.runs).any(|run| {
                run.props.link.as_ref().is_some_and(|link| matches!(&link.action,deckcraft_model::text::Action::Url {url:actual} if actual==url))
            }));
        } else {
            assert!(rich.paragraphs.iter().any(|paragraph| paragraph.props.bullet.is_some()), "the list must remain editable list content");
        }
        assert!(
            !presentation.tags.iter().any(|(key, value)| key == "odp:warning" && value.starts_with("ODF annotation")),
            "supported rich annotation must not become source-only"
        );
        let original = presentation.clone();
        let reopened = import(&export(&presentation).unwrap()).unwrap();
        let actual = only_comment(&reopened);
        let mut expected = comment.clone();
        expected.set_comment_anchor(comments::CommentAnchor::Shape { shape: actual.shape.unwrap() });
        assert!(actual == &expected, "rich body/metadata/link/list must survive a verified owner mapping");
        assert_eq!(presentation, original);
    }
}

#[test]
fn annotation_loss_warning_fails_closed_when_prior_unique_warnings_fill_tag_cap() {
    let earlier = (0..1000)
        .map(|index| format!("<draw:custom-shape svg:width='1pt' svg:height='1pt'><draw:enhanced-geometry draw:type='review-unrepresented-{index}'/></draw:custom-shape>"))
        .collect::<String>();
    let body = "<text:p>Visible body</text:p><foreign:opaque xmlns:foreign='urn:review:unsupported'>Source-only annotation child</foreign:opaque>";
    let error = match import(&source("", body, &earlier)) {
        Err(error) => error,
        Ok(_) => panic!("a source-only annotation diagnostic must not disappear at the tag cap"),
    };
    assert!(error.to_string().contains("comment diagnostic could not be retained"), "{error}");
}
