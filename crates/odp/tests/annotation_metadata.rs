//! Contributor-original annotation fixtures, based on OASIS ODF 1.2/1.3.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::*;
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'";

fn parts(version: &str, annotation: &str) -> Vec<(String, String)> {
    vec![
        ("mimetype".into(), MIME.into()),
        (
            "content.xml".into(),
            format!(
                "<office:document-content {NS} office:version='{version}'><office:body><office:presentation><draw:page draw:name='Page' draw:master-page-name='M'><draw:frame svg:x='0pt' svg:y='0pt' svg:width='1pt' svg:height='1pt'><draw:text-box><text:p>{annotation}</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
            ),
        ),
        (
            "styles.xml".into(),
            format!(
                "<office:document-styles {NS} office:version='{version}'><office:styles/><office:automatic-styles><style:page-layout style:name='PM'><style:page-layout-properties fo:page-width='720pt' fo:page-height='405pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='M' style:page-layout-name='PM'/></office:master-styles></office:document-styles>"
            ),
        ),
        (
            "META-INF/manifest.xml".into(),
            format!(
                "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='{version}'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}'/><manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/><manifest:file-entry manifest:full-path='styles.xml' manifest:media-type='text/xml'/></manifest:manifest>"
            ),
        ),
    ]
}
fn package(parts: &[(String, String)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, value) in parts {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(value.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn annotation(metadata: &str) -> String {
    format!("<office:annotation svg:x='12pt' svg:y='34pt'>{metadata}<text:p>Original body</text:p></office:annotation>")
}
fn source(metadata: &str) -> Vec<u8> {
    package(&parts("1.3", &annotation(metadata)))
}
fn comment(p: &Presentation) -> &Comment {
    assert_eq!(p.slides.len(), 1);
    assert_eq!(p.slides[0].comments.len(), 1);
    &p.slides[0].comments[0]
}
fn native(c: Comment) -> Presentation {
    let mut p = defaults::blank_presentation(Size::new(720.0, 405.0), Theme::default(), false);
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    p.slides.push(Arc::new(Slide { id: SlideId(100), layout, comments: vec![c], show_master_shapes: false, ..Default::default() }));
    p
}
fn normalized_native(c: Comment) -> Presentation {
    let mut p = native(c);
    comments::normalize(&mut p).unwrap();
    p
}
fn no_warning(p: &Presentation) {
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", p.tags);
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut value = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut value).unwrap();
    value
}

#[test]
fn typed_and_display_dates_initials_author_survive_native_and_actual_edit() {
    let original = Comment {
        author: "Original & <author>\n\r".into(),
        initials: "OA & Ω\n".into(),
        date: "2024-02-29T23:59:59.123456+14:00".into(),
        display_date: Some("As entered & <yesterday>\n\r".into()),
        text: "Original body\nsecond line\ttab".into(),
        x: 12.0,
        y: 34.0,
        ..Default::default()
    };
    let source = native(original.clone());
    let source_before = source.clone();
    let bytes = export(&source).unwrap();
    assert_eq!(source, source_before, "export must normalize its own copy, not the caller's document");
    let expected = normalized_native(original.clone());
    let original = comment(&expected).clone();
    let mut p = import(&bytes).unwrap();
    no_warning(&p);
    assert_eq!(comment(&p), &original);
    p = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
    assert_eq!(comment(&p), &original);
    let edited = &mut Arc::make_mut(&mut p.slides[0]).comments[0];
    edited.author = "Edited author".into();
    edited.initials = "EA".into();
    edited.date = "2026-10-16T00:00:00-00:00".into();
    edited.display_date = Some("Independent edited display".into());
    edited.text = "Actual edit".into();
    let expected = edited.clone();
    let reopened = import(&export(&p).unwrap()).unwrap();
    no_warning(&reopened);
    assert_eq!(comment(&reopened), &expected);
}

#[test]
fn independent_dates_both_equal_different_empty_or_absent_have_no_precedence() {
    for (date, display_date) in [
        ("", None),
        ("", Some("")),
        ("", Some("2026-10-16T12:00:00Z")),
        ("2026-10-16T12:00:00Z", None),
        ("2026-10-16T12:00:00Z", Some("")),
        ("2026-10-16T12:00:00Z", Some("2026-10-16T12:00:00Z")),
        ("2026-10-16T12:00:00Z", Some("Yesterday, unparsed")),
    ] {
        let original = Comment { date: date.into(), display_date: display_date.map(str::to_owned), ..Default::default() };
        let expected = normalized_native(original);
        let original = comment(&expected);
        let bytes = export(&expected).unwrap();
        let xml = part(&bytes, "content.xml");
        assert_eq!(xml.contains("<dc:date>"), !date.is_empty());
        assert_eq!(xml.contains("<meta:date-string>"), display_date.is_some());
        let p = import(&bytes).unwrap();
        no_warning(&p);
        assert_eq!(comment(&p), original);
    }
}

#[test]
fn source_metadata_aliases_default_namespaces_entities_and_trailing_newlines() {
    let input = source(
        "<d:creator xmlns:d='http://purl.org/dc/elements/1.1/'>A &amp; &lt;B&gt;&#10;</d:creator><date xmlns='http://purl.org/dc/elements/1.1/'>2026-10-16T12:34:56Z</date><m:date-string xmlns:m='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'>As &quot;entered&quot;&#13;&#10;</m:date-string><creator-initials xmlns='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'>A&amp;B&#10;</creator-initials>",
    );
    let p = import(&input).unwrap();
    no_warning(&p);
    let c = comment(&p);
    assert_eq!(c.author, "A & <B>\n");
    assert_eq!(c.date, "2026-10-16T12:34:56Z");
    assert_eq!(c.display_date.as_deref(), Some("As \"entered\"\r\n"));
    assert_eq!(c.initials, "A&B\n");
    assert_eq!(comment(&import(&export(&p).unwrap()).unwrap()), c);
}

#[test]
fn source_empty_and_missing_string_metadata_stay_distinct_where_representable() {
    for (metadata, display) in [("", None), ("<dc:creator/><meta:date-string/><meta:creator-initials/>", Some(""))] {
        let p = import(&source(metadata)).unwrap();
        no_warning(&p);
        assert!(comment(&p).author.is_empty());
        assert!(comment(&p).initials.is_empty());
        assert!(comment(&p).date.is_empty());
        assert_eq!(comment(&p).display_date.as_deref(), display);
        assert_eq!(comment(&import(&export(&p).unwrap()).unwrap()), comment(&p));
    }
}

#[test]
fn valid_calendar_clock_and_timezone_forms_are_preserved_lexically() {
    for date in [
        "0001-01-01T00:00:00",
        "2000-02-29T23:59:59Z",
        "2024-02-29T24:00:00.000-14:00",
        "2026-10-16T12:34:56.000001+00:00",
        "2026-10-16T12:34:56-00:00",
        " \t2026-10-16T12:34:56Z\r\n",
        "9999-12-31T24:00:00",
    ] {
        let c = Comment { date: date.into(), ..Default::default() };
        let bytes = export(&native(c.clone())).unwrap();
        let p = import(&bytes).unwrap();
        no_warning(&p);
        assert_eq!(comment(&p).date, date);
    }
}

#[test]
fn invalid_or_out_of_profile_typed_dates_refuse_without_inventing_time() {
    for date in [
        "",
        "2026-10-16",
        "0000-01-01T00:00:00",
        "1900-02-29T00:00:00Z",
        "2026-02-29T00:00:00Z",
        "2026-04-31T00:00:00Z",
        "2026-00-01T00:00:00Z",
        "2026-01-00T00:00:00Z",
        "2026-01-01T25:00:00Z",
        "2026-01-01T24:00:00.001Z",
        "2026-01-01T24:00:01Z",
        "2026-01-01T24:01:00Z",
        "2026-01-01T12:60:00Z",
        "2026-01-01T12:00:60Z",
        "2026-01-01T12:00:00.Z",
        "2026-01-01T12:00:00+14:01",
        "2026-01-01T12:00:00-15:00",
        "2026-01-01T12:00:00+01:60",
        "2026-01-01T12:00:00z",
        "2026-01-01T12:00:00Ztrailing",
        "２０２６-01-01T12:00:00Z",
        "-0001-01-01T12:00:00Z",
        "10000-01-01T12:00:00Z",
        "2026-01-01T12:00:00+0:00",
        "2026-01-01T12:00:00+01:00Z",
    ] {
        let source = source(&format!("<dc:date>{date}</dc:date>"));
        assert!(import(&source).is_err(), "accepted source {date:?}");
        if !date.is_empty() {
            assert!(export(&native(Comment { date: date.into(), ..Default::default() })).is_err(), "accepted native {date:?}");
        }
    }
}

#[test]
fn duplicate_or_structured_scalar_metadata_refuses() {
    for metadata in [
        "<dc:creator>A</dc:creator><dc:creator>B</dc:creator>",
        "<dc:date>2026-01-01T00:00:00Z</dc:date><dc:date>2026-01-01T00:00:00Z</dc:date>",
        "<meta:date-string>A</meta:date-string><meta:date-string/>",
        "<meta:creator-initials>A</meta:creator-initials><meta:creator-initials>B</meta:creator-initials>",
        "<dc:creator><text:span>Nested</text:span></dc:creator>",
        "<meta:date-string xml:lang='en'>Yesterday</meta:date-string>",
        "<meta:creator-initials><text:p>AB</text:p></meta:creator-initials>",
    ] {
        assert!(import(&source(metadata)).is_err(), "accepted {metadata}");
    }
}

#[test]
fn foreign_metadata_and_unrepresented_annotation_structure_remain_guarded() {
    for (case, annotation) in [
        ("foreign metadata namespace", annotation("<meta:date-string xmlns:meta='urn:foreign'>Foreign date</meta:date-string>")),
        ("unmapped display behavior", "<office:annotation office:display='true'><text:p>Body</text:p></office:annotation>".into()),
        (
            "undefined rich style reference",
            "<office:annotation><text:p><text:span text:style-name='MissingRich'>Body</text:span></text:p></office:annotation>".into(),
        ),
        (
            "foreign rich inline namespace",
            "<office:annotation><text:p><other:span xmlns:other='urn:foreign'>Body</other:span></text:p></office:annotation>".into(),
        ),
        (
            "nested annotations",
            "<office:annotation><office:annotation><text:p>Reply</text:p></office:annotation><text:p>Body</text:p></office:annotation>".into(),
        ),
        ("unpaired annotation end", "<office:annotation-end office:name='range'/>".into()),
    ] {
        match import(&package(&parts("1.3", &annotation))) {
            Err(_) => {}
            Ok(p) => {
                assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"), "{case} lost its source-recovery warning: {:?}", p.tags);
                assert!(export(&p).is_err(), "{case} must not silently flatten on export");
                if case == "foreign metadata namespace" {
                    assert!(comment(&p).display_date.is_none(), "foreign date-string must not impersonate metadata");
                }
            }
        }
    }
}

#[test]
fn named_and_defined_rich_annotations_preserve_source_name_and_formatting() {
    let annotation = "<office:annotation office:name='original-annotation'><text:p><text:span text:style-name='Rich'>Body</text:span></text:p></office:annotation>";
    let mut files = parts("1.3", annotation);
    for (name, value) in &mut files {
        if name == "styles.xml" {
            *value = value.replace("<office:styles/>", "<office:styles><style:style style:name='Rich' style:family='text'><style:text-properties fo:font-weight='bold'/></style:style></office:styles>");
        }
    }
    let p = import(&package(&files)).unwrap();
    no_warning(&p);
    let c = comment(&p);
    assert_eq!(c.text, "Body");
    assert!(matches!(&c.source_id, Some(comments::CommentSourceId::Odf { name }) if name == "original-annotation"));
    assert!(c.body.as_ref().unwrap().paragraphs[0].runs.iter().all(|run| run.props.bold == Some(true)));
    assert!(c.date.is_empty(), "import must not invent a creation timestamp");
    let reopened = import(&export(&p).unwrap()).unwrap();
    no_warning(&reopened);
    assert_eq!(comment(&reopened), c);
}

#[test]
fn unsupported_rich_annotation_style_semantics_keep_source_guard() {
    let annotation = "<office:annotation><text:p><text:span text:style-name='UnmappedScale'>Body</text:span></text:p></office:annotation>";
    let mut files = parts("1.3", annotation);
    for (name, value) in &mut files {
        if name == "styles.xml" {
            *value = value.replace("<office:styles/>", "<office:styles><style:style style:name='UnmappedScale' style:family='text'><style:text-properties style:text-scale='80%'/></style:style></office:styles>");
        }
    }
    let p = import(&package(&files)).unwrap();
    assert_eq!(comment(&p).text, "Body");
    assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"), "unmapped character scaling lost its recovery warning: {:?}", p.tags);
    assert!(export(&p).is_err(), "unmapped character scaling must not silently flatten");
}

#[test]
fn inherited_annotation_formatting_is_mapped_and_preserved() {
    let mut files = parts("1.3", &annotation(""));
    for (name, value) in &mut files {
        if name == "styles.xml" {
            *value = value.replace("<office:styles/>", "<office:styles><style:default-style style:family='paragraph'><style:text-properties fo:font-weight='bold'/></style:default-style></office:styles>");
        }
    }
    let p = import(&package(&files)).unwrap();
    no_warning(&p);
    let original = comment(&p);
    assert_eq!(original.text, "Original body");
    let body = original.body.as_ref().unwrap();
    assert_eq!(body.paragraphs[0].def_props.bold, Some(true));
    assert!(!body.paragraphs[0].runs.is_empty());
    assert!(body.paragraphs[0].runs.iter().all(|run| run.props.bold == Some(true)));
    let reopened = import(&export(&p).unwrap()).unwrap();
    no_warning(&reopened);
    assert_eq!(comment(&reopened), original);
}

#[test]
fn scalar_and_datetime_caps_and_invalid_xml_text_refuse() {
    for name in ["dc:creator", "meta:date-string", "meta:creator-initials"] {
        let value = "a".repeat(comments::MAX_SCALAR + 1);
        assert!(import(&source(&format!("<{name}>{value}</{name}>"))).is_err());
    }
    for c in [
        Comment { author: "a".repeat(comments::MAX_SCALAR + 1), ..Default::default() },
        Comment { initials: "a".repeat(comments::MAX_SCALAR + 1), ..Default::default() },
        Comment { display_date: Some("a".repeat(comments::MAX_SCALAR + 1)), ..Default::default() },
        Comment { date: format!("2026-01-01T00:00:00.{}Z", "0".repeat(256)), ..Default::default() },
        Comment { display_date: Some("bad\0XML".into()), ..Default::default() },
    ] {
        assert!(export(&native(c)).is_err());
    }
    let value = "a".repeat(comments::MAX_SCALAR);
    let c = Comment { author: value.clone(), initials: value.clone(), display_date: Some(value), ..Default::default() };
    let expected = normalized_native(c);
    assert_eq!(comment(&import(&export(&expected).unwrap()).unwrap()), comment(&expected));
    let date = format!("2026-01-01T00:00:00.{}Z", "0".repeat(235));
    assert_eq!(date.len(), 256);
    let p = import(&source(&format!("<dc:date>{date}</dc:date>"))).unwrap();
    assert_eq!(comment(&p).date, date);
}

#[test]
fn native_thread_resolution_and_verified_shape_anchor_roundtrip_without_flattening() {
    for (case, c) in [
        (
            "thread",
            Comment { replies: vec![Comment { author: "Reply author".into(), text: "Reply".into(), ..Default::default() }], ..Default::default() },
        ),
        ("resolved", Comment { resolved: true, ..Default::default() }),
        ("shape anchor", Comment { shape: Some(ShapeId(5)), ..Default::default() }),
    ] {
        let mut p = native(c);
        if case == "shape anchor" {
            let slide = Arc::make_mut(&mut p.slides[0]);
            slide.shapes.push(Shape {
                id: ShapeId(5),
                name: "Original annotation owner".into(),
                xfrm: Some(Xfrm::new(30.0, 40.0, 200.0, 80.0)),
                text: Some(TextBody::from_text("Owning annotation text")),
                ..Default::default()
            });
            slide.comments[0].set_comment_anchor(comments::CommentAnchor::Shape { shape: ShapeId(5) });
        }
        comments::normalize(&mut p).unwrap();
        let before = p.clone();
        let bytes = export(&p).unwrap();
        assert_eq!(p, before, "{case}: export changed the native source");
        let reopened = import(&bytes).unwrap();
        no_warning(&reopened);
        let mut expected = comment(&p).clone();
        if case == "shape anchor" {
            let actual_owner = reopened.slides[0].shapes.iter().find(|shape| shape.name == "Original annotation owner").unwrap();
            assert_eq!(actual_owner.text.as_ref().unwrap().text(), "Owning annotation text");
            expected.set_comment_anchor(comments::CommentAnchor::Shape { shape: actual_owner.id });
        }
        assert_eq!(comment(&reopened), &expected, "{case}: native metadata changed");
        assert!(comment(&reopened).date.is_empty(), "{case}: a creation date was invented");
        assert!(comment(&reopened).wire_guid.is_none(), "{case}: an imported wire GUID was invented");
        if case == "thread" {
            assert!(comment(&reopened).replies[0].date.is_empty());
            assert_eq!(part(&bytes, "content.xml").matches("<office:annotation ").count(), 2, "every reply needs a visible standard fallback");
        }
        if case == "resolved" {
            assert_eq!(comment(&reopened).comment_status(), comments::CommentStatus::Resolved);
        }
    }
}

#[test]
fn versioned_source_and_output_schema_artifacts() {
    for version in ["1.2", "1.3"] {
        let initials = if version == "1.3" { "<meta:creator-initials>OA</meta:creator-initials>" } else { "" };
        let metadata = format!(
            "<dc:creator>Original author</dc:creator><dc:date>2024-02-29T24:00:00.000Z</dc:date><meta:date-string>Entered display date</meta:date-string>{initials}"
        );
        let source = parts(version, &annotation(&metadata));
        let p = import(&package(&source)).unwrap();
        no_warning(&p);
        assert_eq!(comment(&p).date, "2024-02-29T24:00:00.000Z");
        assert_eq!(comment(&p).display_date.as_deref(), Some("Entered display date"));
        let output = export(&p).unwrap();
        let reopened = import(&output).unwrap();
        no_warning(&reopened);
        assert_eq!(comment(&reopened), comment(&p));
        if let Ok(dir) = std::env::var("DECKCRAFT_ODP_ANNOTATION_QA_DIR") {
            let dir = std::path::Path::new(&dir);
            std::fs::create_dir_all(dir).unwrap();
            for (name, value) in source.into_iter().filter(|(name, _)| name.ends_with(".xml")) {
                std::fs::write(dir.join(format!("{version}-source-{}", name.replace('/', "-"))), value).unwrap();
            }
            for name in ["content.xml", "styles.xml", "meta.xml", "META-INF/manifest.xml"] {
                std::fs::write(dir.join(format!("{version}-output-{}", name.replace('/', "-"))), part(&output, name)).unwrap();
            }
        }
    }
}
