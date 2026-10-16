//! Contributor-original comment timestamps and package wiring fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{Comment, Presentation, ShapeId, defaults};
use deckcraft_pptx::{export, import};
use quick_xml::{Reader, events::Event};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    sync::Arc,
};

const GOOD: &str = "2026-10-16T12:34:56Z";
const MODERN_REL: &str = "http://schemas.microsoft.com/office/2018/10/relationships/comments";
const LEGACY_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";

fn fixture_identity(id: u32, initials: &str) -> deckcraft_model::comments::CommentAuthorIdentity {
    deckcraft_model::comments::CommentAuthorIdentity {
        id,
        origin: deckcraft_model::comments::AuthorOrigin::Imported,
        source_initials: Some(initials.into()),
        wire_guid: Some(format!("{{01234567-89AB-CDEF-0002-{id:012X}}}")),
        external_id: None,
        user_id: None,
        provider_id: None,
        source_scope: None,
    }
}
fn comment(date: &str, text: &str) -> Comment {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1_000_000);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Comment {
        comment_id: Some(id),
        author_identity: Some(fixture_identity(900_000, "OR")),
        wire_guid: Some(format!("{{01234567-89AB-CDEF-0001-{id:012X}}}")),
        author: "Original Reviewer".into(),
        initials: "OR".into(),
        date: date.into(),
        text: text.into(),
        x: 90.0,
        y: 45.0,
        ..Default::default()
    }
}
fn deck(comments: Vec<Comment>) -> Presentation {
    let mut p = defaults::new_presentation(None);
    Arc::make_mut(&mut p.slides[0]).comments = comments;
    p
}
fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut parts = BTreeMap::new();
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        parts.insert(file.name().into(), bytes);
    }
    parts
}
fn xml(parts: &BTreeMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(parts[name].clone()).unwrap()
}
/// Independent low-level decoding also tests attribute character-reference spelling.
fn attributes(xml: &str, local: &str) -> Vec<BTreeMap<String, String>> {
    let mut reader = Reader::from_str(xml);
    let mut result = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(element) | Event::Empty(element) if element.local_name().as_ref() == local.as_bytes() => {
                let mut attrs = BTreeMap::new();
                for attribute in element.attributes() {
                    let attribute = attribute.unwrap();
                    let name = String::from_utf8(attribute.key.as_ref().to_vec()).unwrap();
                    let value = attribute.decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder()).unwrap().into_owned();
                    assert!(attrs.insert(name, value).is_none());
                }
                result.push(attrs);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    result
}
fn no_fictional_time(parts: &BTreeMap<String, Vec<u8>>) {
    for (name, bytes) in parts.iter().filter(|(name, _)| name.starts_with("ppt/comments/")) {
        assert!(!String::from_utf8_lossy(bytes).contains("2024-01-01T00:00:00.000"), "invented date in {name}");
    }
}
fn legacy_only(parts: &BTreeMap<String, Vec<u8>>, slide: usize, number: usize) {
    assert!(parts.contains_key(&format!("ppt/comments/comment{number}.xml")));
    assert!(!parts.contains_key(&format!("ppt/comments/modernComment{number}.xml")));
    let rels = attributes(&xml(parts, &format!("ppt/slides/_rels/slide{slide}.xml.rels")), "Relationship");
    assert_eq!(rels.iter().filter(|rel| rel["Type"] == LEGACY_REL).count(), 1);
    assert!(!rels.iter().any(|rel| rel["Type"] == MODERN_REL));
    assert!(attributes(&xml(parts, &format!("ppt/slides/slide{slide}.xml")), "commentRel").is_empty());
}
fn atomic_refusal(p: &Presentation, expected: &str) {
    let before = serde_json::to_vec(p).unwrap();
    let error = export(p).unwrap_err().to_string();
    assert!(error.contains(expected), "unexpected refusal: {error}");
    assert_eq!(serde_json::to_vec(p).unwrap(), before, "refusal mutated model");
}

#[test]
fn absent_creation_time_uses_a_complete_legacy_package_without_fabrication() {
    let original = Comment { x: 90.125, y: 45.25, ..comment("", "Original <comment> & second\nline") };
    let p = deck(vec![original.clone()]);
    let output = export(&p).unwrap();
    let files = parts(&output);
    legacy_only(&files, 1, 1);
    assert!(files.contains_key("ppt/commentAuthors.xml"));
    assert!(!files.contains_key("ppt/authors.xml"));
    let rows = attributes(&xml(&files, "ppt/comments/comment1.xml"), "cm");
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].contains_key("dt"));
    let types = attributes(&xml(&files, "[Content_Types].xml"), "Override");
    assert!(types.iter().any(|entry| entry["PartName"] == "/ppt/comments/comment1.xml"));
    assert!(!types.iter().any(|entry| entry["ContentType"] == "application/vnd.ms-powerpoint.comments+xml"));
    assert!(!types.iter().any(|entry| entry["ContentType"] == "application/vnd.ms-powerpoint.authors+xml"));
    let rels = attributes(&xml(&files, "ppt/_rels/presentation.xml.rels"), "Relationship");
    assert!(!rels.iter().any(|entry| entry["Type"].ends_with("2018/10/relationships/authors")));
    assert_eq!(import(&output).unwrap().slides[0].comments, [original]);
    no_fictional_time(&files);
    if let Ok(dir) = std::env::var("DECKCRAFT_PPTX_COMMENT_TIME_QA_DIR") {
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("undated-legacy.xml"), xml(&files, "ppt/comments/comment1.xml")).unwrap();
    }
}

#[test]
fn dated_and_undated_flat_siblings_keep_order_and_independent_date_presence() {
    let originals = vec![comment(GOOD, "First dated"), comment("", "Middle undated"), comment("2024-02-29T24:00:00.000-14:00", "Last dated")];
    let p = deck(originals.clone());
    let mut p: Presentation = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
    let output = export(&p).unwrap();
    let files = parts(&output);
    legacy_only(&files, 1, 1);
    let rows = attributes(&xml(&files, "ppt/comments/comment1.xml"), "cm");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].get("dt").map(String::as_str), Some(GOOD));
    assert!(!rows[1].contains_key("dt"));
    assert_eq!(rows[2].get("dt").map(String::as_str), Some("2024-02-29T24:00:00.000-14:00"));
    assert_eq!(import(&output).unwrap().slides[0].comments, originals);
    let edit = &mut Arc::make_mut(&mut p.slides[0]).comments[1];
    edit.date = "2026-10-16T13:00:00+00:00".into();
    edit.text = "Actual date and body edit".into();
    let modern = export(&p).unwrap();
    let modern_files = parts(&modern);
    assert!(modern_files.contains_key("ppt/comments/modernComment1.xml"));
    assert!(modern_files.contains_key("ppt/authors.xml"));
    assert_eq!(import(&modern).unwrap().slides[0].comments, p.slides[0].comments);
    Arc::make_mut(&mut p.slides[0]).comments[1].date.clear();
    let legacy_again = export(&p).unwrap();
    let again = parts(&legacy_again);
    legacy_only(&again, 1, 1);
    assert!(!again.contains_key("ppt/authors.xml"));
    assert_eq!(again.keys().collect::<Vec<_>>(), files.keys().collect::<Vec<_>>());
    assert_eq!(import(&legacy_again).unwrap().slides[0].comments, p.slides[0].comments);
    no_fictional_time(&again);
}

#[test]
fn mixed_slide_formats_wire_only_their_actual_comment_parts_and_anchors() {
    let mut p = deck(vec![comment("", "Legacy slide")]);
    let layout = p.slides[0].layout;
    for _ in 0..2 {
        let slide = defaults::new_slide(&mut p, layout).unwrap();
        p.slides.push(Arc::new(slide));
    }
    let mut thread = comment(GOOD, "Dated thread");
    thread.resolved = true;
    let mut reply = comment("2026-10-16T13:00:00Z", "Dated reply");
    reply.x = 0.0;
    reply.y = 0.0;
    thread.replies.push(reply);
    Arc::make_mut(&mut p.slides[2]).comments = vec![thread];
    let output = export(&p).unwrap();
    let files = parts(&output);
    legacy_only(&files, 1, 1);
    assert!(files.contains_key("ppt/comments/comment2.xml"));
    assert!(files.contains_key("ppt/comments/modernComment2.xml"));
    assert!(files.contains_key("ppt/authors.xml"));
    let slide = xml(&files, "ppt/slides/slide3.xml");
    let anchors = attributes(&slide, "commentRel");
    assert_eq!(anchors.len(), 1);
    let rid = &anchors[0]["r:id"];
    let rels = attributes(&xml(&files, "ppt/slides/_rels/slide3.xml.rels"), "Relationship");
    let relation = rels.iter().find(|rel| rel["Id"] == *rid).unwrap();
    assert_eq!(relation["Type"], MODERN_REL);
    assert_eq!(relation["Target"], "../comments/modernComment2.xml");
    let empty_rels = attributes(&xml(&files, "ppt/slides/_rels/slide2.xml.rels"), "Relationship");
    assert!(!empty_rels.iter().any(|rel| rel["Type"].ends_with("/comments")));
    let reopened = import(&output).unwrap();
    for (actual, expected) in reopened.slides.iter().zip(&p.slides) {
        assert_eq!(actual.comments, expected.comments);
    }
    no_fictional_time(&files);
}

#[test]
fn valid_calendar_fraction_timezone_and_whitespace_lexicals_are_exact_in_both_formats() {
    let maximum = format!("2026-10-16T12:34:56.{}Z", "0".repeat(235));
    assert_eq!(maximum.len(), 256);
    for (index, date) in [
        "0001-01-01T00:00:00",
        "2000-02-29T23:59:59Z",
        "2024-02-29T24:00:00.000-14:00",
        "2026-10-16T12:34:56.123456+14:00",
        "2026-10-16T12:34:56-00:00",
        "9999-12-31T24:00:00",
        " \t2026-10-16T12:34:56Z\r\n",
        &maximum,
    ]
    .into_iter()
    .enumerate()
    {
        let mut top = comment(date, "Dated original");
        top.replies.push(comment(date, "Reply"));
        let p = deck(vec![top]);
        let output = export(&p).unwrap();
        let files = parts(&output);
        let legacy = xml(&files, "ppt/comments/comment1.xml");
        let modern = xml(&files, "ppt/comments/modernComment1.xml");
        let rows = attributes(&legacy, "cm");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row["dt"] == date));
        assert_eq!(attributes(&modern, "cm")[0]["created"], date);
        assert_eq!(attributes(&modern, "reply")[0]["created"], date);
        if date.contains('\t') {
            for xml in [&legacy, &modern] {
                assert!(xml.contains("&#9;") && xml.contains("&#10;") && xml.contains("&#13;"));
            }
        }
        assert_eq!(import(&output).unwrap().slides[0].comments, p.slides[0].comments);
        no_fictional_time(&files);
        if let Ok(dir) = std::env::var("DECKCRAFT_PPTX_COMMENT_TIME_QA_DIR") {
            let dir = std::path::Path::new(&dir);
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(format!("valid-{index}-legacy.xml")), legacy).unwrap();
            std::fs::write(dir.join(format!("valid-{index}-modern.xml")), modern).unwrap();
        }
    }
    atomic_refusal(&deck(vec![comment(&format!("{maximum} "), "Over cap")]), "scalar/body exceeds size limit");
}

#[test]
fn invalid_nonempty_timestamps_refuse_top_replies_and_unemitted_nested_replies() {
    let oversized = format!("2026-10-16T00:00:00.{}Z", "0".repeat(256));
    for date in [
        "2026-10-16",
        "2026-02-29T12:00:00Z",
        "1900-02-29T12:00:00Z",
        "2026-04-31T12:00:00Z",
        "0000-01-01T12:00:00Z",
        "2026-01-00T12:00:00Z",
        "2026-00-01T12:00:00Z",
        "2026-10-16T24:00:00.001Z",
        "2026-10-16T24:01:00Z",
        "2026-10-16T25:00:00Z",
        "2026-10-16T12:60:00Z",
        "2026-10-16T12:00:60Z",
        "2026-10-16T12:00:00.Z",
        "2026-10-16T12:00:00+14:01",
        "2026-10-16T12:00:00-15:00",
        "2026-10-16T12:00:00Zjunk",
        "-0001-01-01T12:00:00Z",
        "10000-01-01T12:00:00Z",
        "２０２６-01-01T12:00:00Z",
        "2026-10-16T12:00:00Z\0",
        " \t\r\n",
        &oversized,
    ] {
        let expected = if date.len() > 256 { "scalar/body exceeds size limit" } else { "creation timestamp" };
        atomic_refusal(&deck(vec![comment(date, "Top")]), expected);
        let mut thread = comment(GOOD, "Top");
        thread.replies.push(comment(date, "Reply"));
        atomic_refusal(&deck(vec![thread]), expected);
        let mut thread = comment(GOOD, "Top");
        let mut reply = comment(GOOD, "Reply");
        reply.replies.push(comment(date, "Nested reply"));
        thread.replies.push(reply);
        atomic_refusal(&deck(vec![thread]), expected);
    }
}

#[test]
fn missing_dates_preserve_thread_status_and_shape_semantics_without_fabrication() {
    let mut missing_top = comment("", "Undated thread");
    missing_top.replies.push(comment(GOOD, "Dated reply"));
    let mut missing_reply = comment(GOOD, "Dated thread");
    missing_reply.replies.push(comment("", "Undated reply"));
    for comments in [
        vec![missing_top],
        vec![missing_reply],
        vec![Comment { resolved: true, ..comment("", "Undated resolved") }],
        vec![Comment { shape: Some(ShapeId(123)), ..comment("", "Undated anchored") }],
        vec![comment("", "Undated flat sibling"), Comment { resolved: true, ..comment(GOOD, "Dated resolved sibling") }],
        vec![comment("", "Undated flat sibling"), Comment { shape: Some(ShapeId(123)), ..comment(GOOD, "Dated anchored sibling") }],
        vec![comment("", "Undated flat sibling"), Comment { replies: vec![comment(GOOD, "Reply")], ..comment(GOOD, "Dated thread sibling") }],
    ] {
        let mut p = deck(comments);
        Arc::make_mut(&mut p.slides[0]).shapes.push(deckcraft_model::Shape {
            id: ShapeId(123),
            name: "Original anchor owner".into(),
            xfrm: Some(deckcraft_model::Xfrm::new(20., 30., 100., 50.)),
            ..Default::default()
        });
        let original = p.clone();
        let bytes = export(&p).unwrap();
        let files = parts(&bytes);
        legacy_only(&files, 1, 1);
        no_fictional_time(&files);
        let back = import(&bytes).unwrap();
        let shape = back.slides[0].shapes.iter().find(|shape| shape.name == "Original anchor owner").unwrap().id;
        let mut expected = p.slides[0].comments.clone();
        for comment in &mut expected {
            if comment.shape == Some(ShapeId(123)) {
                comment.set_comment_anchor(deckcraft_model::comments::CommentAnchor::Shape { shape });
            }
        }
        assert_eq!(back.slides[0].comments, expected, "typed metadata restores the actual thread, status and owner");
        fn dates(items: &[Comment], out: &mut Vec<Option<String>>) {
            for comment in items {
                out.push((!comment.date.is_empty()).then(|| comment.date.clone()));
                dates(&comment.replies, out);
            }
        }
        let mut expected_dates = Vec::new();
        dates(&expected, &mut expected_dates);
        let rows = attributes(&xml(&files, "ppt/comments/comment1.xml"), "cm");
        assert_eq!(rows.iter().map(|row| row.get("dt").cloned()).collect::<Vec<_>>(), expected_dates);
        assert_eq!(p, original, "export mutated source");
    }
}

#[test]
fn comment_positions_preserve_sub_eighth_values_and_refuse_invalid_native_coordinates() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1_000_000.125, f64::MAX] {
        for axis in [true, false] {
            let mut c = comment("", "Undated");
            if axis {
                c.x = value;
            } else {
                c.y = value;
            }
            atomic_refusal(&deck(vec![c]), "invalid comment position");
        }
    }
    for value in [0.0, 0.01, 0.125, 1_000_000.0] {
        let c = Comment { x: value, y: value, ..comment("", "Exact retained native position") };
        let p = deck(vec![c.clone()]);
        let original = p.clone();
        assert_eq!(import(&export(&p).unwrap()).unwrap().slides[0].comments, [c]);
        assert_eq!(p, original);
    }
}

#[test]
fn new_legacy_lane_checks_top_level_cardinality_instead_of_truncating() {
    let p = deck((0..10_000).map(|_| comment("", "Original bounded comment")).collect());
    let output = export(&p).unwrap();
    let files = parts(&output);
    assert_eq!(attributes(&xml(&files, "ppt/comments/comment1.xml"), "cm").len(), 10_000);
    assert_eq!(import(&output).unwrap().slides[0].comments.len(), 10_000);
    atomic_refusal(&deck((0..10_001).map(|_| comment("", "Original bounded comment")).collect()), "root count");
}

#[test]
fn actual_legacy_comment_edit_changes_its_owned_core_and_verified_metadata_parts() {
    let mut p = deck(vec![comment("", "Before body")]);
    let before = parts(&export(&p).unwrap());
    Arc::make_mut(&mut p.slides[0]).comments[0].text = "After body".into();
    let output = export(&p).unwrap();
    let after = parts(&output);
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
    let changed: Vec<_> = before.iter().filter(|(name, bytes)| after.get(*name) != Some(*bytes)).map(|(name, _)| name.as_str()).collect();
    assert_eq!(changed, ["ppt/comments/comment1.xml", "ppt/deckcraft/comments.json"]);
    assert_eq!(import(&output).unwrap().slides[0].comments[0].text, "After body");
    assert!(import(&output).unwrap().slides[0].comments[0].date.is_empty());
    no_fictional_time(&after);
}
