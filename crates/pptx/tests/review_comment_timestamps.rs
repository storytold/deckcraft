//! Fresh public-API adversaries for absent creation times and exact dateTime spelling.
use std::{collections::BTreeMap, io::Read, sync::Arc};

use deckcraft_model::{Comment, Presentation, ShapeId, defaults};
use quick_xml::{NsReader, Reader, events::Event, name::ResolveResult};

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
        author_identity: Some(fixture_identity(900_000, "IR")),
        wire_guid: Some(format!("{{01234567-89AB-CDEF-0001-{id:012X}}}")),
        author: "Independent reviewer".into(),
        initials: "IR".into(),
        text: text.into(),
        date: date.into(),
        x: 2.0,
        y: 4.0,
        ..Default::default()
    }
}

fn deck(comments: Vec<Comment>) -> Presentation {
    let mut p = defaults::new_presentation(None);
    Arc::make_mut(&mut p.slides[0]).comments = comments;
    p
}

fn parts(bytes: &[u8]) -> BTreeMap<String, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut result = BTreeMap::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).expect("entry");
        if entry.name().ends_with(".xml") || entry.name().ends_with(".rels") {
            let mut xml = String::new();
            entry.read_to_string(&mut xml).expect("XML UTF8");
            assert!(result.insert(entry.name().into(), xml).is_none(), "duplicate package part");
        }
    }
    result
}

/// Inspect serialized attributes directly with quick-xml, independently of the PPTX DOM/importer.
fn attrs(xml: &str, element: &str) -> Vec<BTreeMap<String, String>> {
    let mut reader = Reader::from_str(xml);
    let mut result = vec![];
    loop {
        match reader.read_event().expect("well-formed XML") {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == element.as_bytes() => {
                result.push(
                    e.attributes()
                        .map(|a| {
                            let a = a.expect("attribute");
                            (
                                String::from_utf8(a.key.as_ref().to_vec()).expect("key"),
                                a.decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder()).expect("value").into_owned(),
                            )
                        })
                        .collect(),
                );
            }
            Event::Eof => break,
            _ => {}
        }
    }
    result
}

fn namespaces_valid(xml: &str) -> bool {
    let mut reader = NsReader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                if matches!(reader.resolver().resolve_element(e.name()).0, ResolveResult::Unknown(_)) {
                    return false;
                }
                for attribute in e.attributes() {
                    let Ok(attribute) = attribute else { return false };
                    if attribute.key.as_ref() == b"xmlns" || attribute.key.as_ref().starts_with(b"xmlns:") {
                        continue;
                    }
                    if matches!(reader.resolver().resolve_attribute(attribute.key).0, ResolveResult::Unknown(_)) {
                        return false;
                    }
                }
            }
            Ok(Event::End(e)) => {
                if matches!(reader.resolver().resolve_element(e.name()).0, ResolveResult::Unknown(_)) {
                    return false;
                }
            }
            Ok(Event::Eof) => return true,
            Err(_) => return false,
            _ => {}
        }
    }
}

#[test]
fn generated_modern_comment_namespaces_resolve_for_every_element_and_attribute() {
    let mut root = comment("2000-01-01T00:00:00Z", "Top & text\nsecond line");
    root.replies.push(comment("2000-01-01T00:00:01Z", "Reply <text>"));
    let package = parts(&deckcraft_pptx::export(&deck(vec![root])).expect("export"));
    let modern: Vec<_> = package.iter().filter(|(name, _)| name.starts_with("ppt/comments/modernComment")).collect();
    assert!(!modern.is_empty());
    for (name, xml) in modern {
        assert!(namespaces_valid(xml), "unresolved namespace in {name}: {xml}");
        let without_drawingml = xml.replace(" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"", "");
        assert_ne!(without_drawingml, *xml, "DrawingML namespace declaration");
        assert!(!namespaces_valid(&without_drawingml), "namespace inspector must reject an undeclared element prefix");
    }
    assert!(!namespaces_valid("<root missing:attribute=\"value\"/>"), "namespace inspector must reject undeclared attribute prefixes");
}

fn refuse_unchanged(p: &Presentation, expected: &str) {
    let before = serde_json::to_value(p).expect("snapshot");
    let error = deckcraft_pptx::export(p).expect_err("must refuse");
    assert!(matches!(error, deckcraft_pptx::PptxError::Unsupported(_)), "{error}");
    assert!(error.to_string().contains(expected), "{error}");
    assert_eq!(serde_json::to_value(p).expect("after"), before, "failed export must leave its input untouched");
}

#[test]
fn absent_and_dated_flat_siblings_keep_order_and_independent_attribute_presence() {
    let original = vec![comment("", "first absent"), comment("2400-02-29T00:00:00-00:00", "dated middle"), comment("", "last absent")];
    let p = deck(original.clone());
    let bytes = deckcraft_pptx::export(&p).expect("legacy export");
    let package = parts(&bytes);
    let records = attrs(&package["ppt/comments/comment1.xml"], "p:cm");
    assert_eq!(records.len(), 3);
    assert!(!records[0].contains_key("dt"));
    assert_eq!(records[1]["dt"], original[1].date);
    assert!(!records[2].contains_key("dt"));
    assert!(!package.keys().any(|name| name.contains("modernComment") || name == "ppt/authors.xml"));
    assert!(package.contains_key("ppt/commentAuthors.xml"));
    assert!(!package["ppt/slides/slide1.xml"].contains("commentRel"));
    assert!(!package["[Content_Types].xml"].contains("vnd.ms-powerpoint.comments"));
    assert!(!package["[Content_Types].xml"].contains("vnd.ms-powerpoint.authors"));
    assert!(!package["ppt/_rels/presentation.xml.rels"].contains("2018/10/relationships/authors"));
    assert_eq!(deckcraft_pptx::import(&bytes).expect("import").slides[0].comments, original);
    assert_eq!(p.slides[0].comments, original);
}

#[test]
fn mixed_slide_numbering_anchor_targets_and_global_author_closure() {
    let mut p = deck(vec![comment("", "legacy first")]);
    let layout = p.slides[0].layout;
    for _ in 0..3 {
        let slide = defaults::new_slide(&mut p, layout).expect("slide");
        p.slides.push(Arc::new(slide));
    }
    let mut modern = comment("2032-02-29T24:00:00.000+14:00", "thread third");
    modern.resolved = true;
    modern.replies.push(Comment {
        author: "Reply only author".into(),
        initials: "RA".into(),
        author_identity: Some(fixture_identity(900_001, "RA")),
        x: 0.0,
        y: 0.0,
        ..comment("2032-03-01T01:02:03.400Z", "reply")
    });
    Arc::make_mut(&mut p.slides[2]).comments = vec![modern];
    Arc::make_mut(&mut p.slides[3]).comments = vec![comment("", "legacy fourth")];
    let bytes = deckcraft_pptx::export(&p).expect("mixed export");
    let package = parts(&bytes);
    for (slide, comment_part, modern) in [(1, Some(1), false), (2, None, false), (3, Some(2), true), (4, Some(3), false)] {
        let xml = &package[&format!("ppt/slides/slide{slide}.xml")];
        let relationships = attrs(&package[&format!("ppt/slides/_rels/slide{slide}.xml.rels")], "Relationship");
        let comment_rels: Vec<_> = relationships.iter().filter(|r| r["Type"].ends_with("/comments")).collect();
        assert_eq!(comment_rels.len(), usize::from(comment_part.is_some()) + usize::from(modern));
        let anchors = attrs(xml, "p188:commentRel");
        assert_eq!(anchors.len(), usize::from(modern), "slide {slide}");
        if let Some(number) = comment_part {
            assert!(comment_rels.iter().any(|r| r["Target"] == format!("../comments/comment{number}.xml")));
            assert_eq!(package.contains_key(&format!("ppt/comments/modernComment{number}.xml")), modern);
            if modern {
                let relationship = relationships.iter().find(|r| r["Id"] == anchors[0]["r:id"]).expect("anchor relationship");
                assert_eq!(relationship["Target"], format!("../comments/modernComment{number}.xml"));
                assert!(relationship["Type"].contains("2018/10/relationships/comments"));
            }
        }
    }
    let modern_authors = attrs(&package["ppt/authors.xml"], "p188:author");
    let legacy_authors = attrs(&package["ppt/commentAuthors.xml"], "p:cmAuthor");
    assert!(modern_authors.iter().any(|a| a["name"] == "Reply only author"));
    for (name, xml) in &package {
        if name.starts_with("ppt/comments/") {
            for record in attrs(xml, "p:cm") {
                assert!(legacy_authors.iter().any(|a| a["id"] == record["authorId"]));
            }
            for record in attrs(xml, "p188:cm").into_iter().chain(attrs(xml, "p188:reply")) {
                assert!(modern_authors.iter().any(|a| a["id"] == record["authorId"]));
            }
        }
    }
    let overrides = attrs(&package["[Content_Types].xml"], "Override");
    assert_eq!(overrides.iter().filter(|a| a["ContentType"] == "application/vnd.ms-powerpoint.comments+xml").count(), 1);
    assert_eq!(overrides.iter().filter(|a| a["ContentType"] == "application/vnd.ms-powerpoint.authors+xml").count(), 1);
    let q = deckcraft_pptx::import(&bytes).expect("mixed import");
    for (actual, expected) in q.slides.iter().zip(&p.slides) {
        assert_eq!(actual.comments, expected.comments);
    }
}

#[test]
fn modern_to_absent_edit_drops_generated_anchors_and_modern_global_parts() {
    let dated = deck(vec![comment("2001-01-01T01:01:01Z", "original")]);
    let mut p = deckcraft_pptx::import(&deckcraft_pptx::export(&dated).expect("export dated")).expect("reopen");
    let c = &mut Arc::make_mut(&mut p.slides[0]).comments[0];
    c.date.clear();
    c.text = "edited absent".into();
    let bytes = deckcraft_pptx::export(&p).expect("export edit");
    let package = parts(&bytes);
    assert!(
        !package.values().any(|xml| xml.contains("commentRel") || xml.contains("modernComment") || xml.contains("2018/10/relationships/authors"))
    );
    assert!(!package.contains_key("ppt/authors.xml"));
    assert_eq!(deckcraft_pptx::import(&bytes).expect("reimport").slides[0].comments, p.slides[0].comments);
}

#[test]
fn exact_valid_lexical_dates_survive_both_parts_and_direct_reply_attributes() {
    let boundary = format!("2000-02-29T00:00:00.{}Z", "7".repeat(235));
    assert_eq!(boundary.len(), 256);
    for date in [
        "0001-01-01T00:00:00",
        "9999-12-31T23:59:59-14:00",
        "1900-02-28T24:00:00.000000000+00:00",
        "2000-02-29T12:34:56.000000123456789-13:59",
        " \t\r\n2004-02-29T24:00:00Z\n\r\t ",
        boundary.as_str(),
    ] {
        let mut root = comment(date, "top");
        root.replies.push(Comment { x: 0.0, y: 0.0, ..comment(date, "direct reply") });
        let p = deck(vec![root]);
        let bytes = deckcraft_pptx::export(&p).expect(date);
        let package = parts(&bytes);
        let legacy = &package["ppt/comments/comment1.xml"];
        let modern = &package["ppt/comments/modernComment1.xml"];
        for record in attrs(legacy, "p:cm") {
            assert_eq!(record["dt"], date);
        }
        for record in attrs(modern, "p188:cm").into_iter().chain(attrs(modern, "p188:reply")) {
            assert_eq!(record["created"], date);
        }
        if date.contains('\t') {
            for xml in [legacy, modern] {
                assert!(xml.contains("&#9;") && xml.contains("&#10;") && xml.contains("&#13;"));
                assert!(!xml.contains('\t') && !xml.contains('\r'));
            }
        }
        let q = deckcraft_pptx::import(&bytes).expect("import date");
        assert_eq!(q.slides[0].comments, p.slides[0].comments);
    }
}

#[test]
fn invalid_nonempty_dates_refuse_even_beside_absent_sibling_or_beyond_direct_replies() {
    let over_cap = format!("2000-02-29T00:00:00.{}Z", "7".repeat(236));
    for date in [
        " ",
        "\t\r\n",
        "2024-02-29",
        "1900-02-29T00:00:00Z",
        "2100-02-29T00:00:00Z",
        "2004-04-31T00:00:00Z",
        "2024-00-01T00:00:00Z",
        "2024-01-00T00:00:00Z",
        "2024-01-01T24:00:00.01Z",
        "2024-01-01T23:59:60Z",
        "2024-01-01T12:60:00Z",
        "2024-01-01T12:00:00+14:01",
        "2024-01-01T12:00:00-15:00",
        "2024-01-01T12:00:00.Z",
        "2024-01-01T12:00:00ZZ",
        "2024-01-01T12:00:00Zjunk",
        "2024-01-01t12:00:00z",
        "2024-01-01T12:00:00+1:00",
        "0000-01-01T00:00:00Z",
        "-0001-01-01T00:00:00Z",
        "10000-01-01T00:00:00Z",
        "２０２４-01-01T00:00:00Z",
        "2024-01-01T00:00:00Z\u{a0}",
        "2024-01-01T00:00:00Z\0",
        over_cap.as_str(),
    ] {
        let expected = if date.len() > 256 { "scalar/body exceeds size limit" } else { "creation timestamp" };
        refuse_unchanged(&deck(vec![comment("", "absent"), comment(date, "invalid")]), expected);
        let mut root = comment("2000-01-01T00:00:00Z", "valid root");
        let mut reply = comment("2000-01-01T00:00:01Z", "valid direct reply");
        reply.replies.push(comment(date, "invalid grandchild"));
        root.replies.push(reply);
        refuse_unchanged(&deck(vec![root]), expected);
    }
}

#[test]
fn absent_creation_times_keep_native_thread_status_and_verified_shape_owner() {
    for missing_root in [false, true] {
        for semantic in 0..3 {
            let mut c = comment("2000-01-01T00:00:00Z", "semantics");
            match semantic {
                0 => c.resolved = true,
                1 => c.shape = Some(ShapeId(123)),
                _ => c.replies.push(comment("2000-01-01T00:00:01Z", "reply")),
            }
            if missing_root {
                c.date.clear();
            }
            let mut p = deck(vec![c, comment("", "absent sibling")]);
            Arc::make_mut(&mut p.slides[0]).shapes.push(deckcraft_model::Shape {
                id: ShapeId(123),
                name: "Independent anchor owner".into(),
                xfrm: Some(deckcraft_model::Xfrm::new(20., 30., 100., 50.)),
                ..Default::default()
            });
            let before = serde_json::to_value(&p).expect("snapshot");
            let bytes = deckcraft_pptx::export(&p).expect("preserved missing time");
            let package = parts(&bytes);
            assert!(!package.contains_key("ppt/comments/modernComment1.xml"));
            let q = deckcraft_pptx::import(&bytes).expect("reimport");
            let mut expected = p.slides[0].comments.clone();
            if semantic == 1 {
                let shape = q.slides[0].shapes.iter().find(|shape| shape.name == "Independent anchor owner").expect("owner").id;
                expected[0].set_comment_anchor(deckcraft_model::comments::CommentAnchor::Shape { shape });
            }
            assert_eq!(q.slides[0].comments, expected);
            assert!(q.slides[0].comments[1].date.is_empty());
            assert_eq!(q.slides[0].comments[0].date.is_empty(), missing_root);
            assert_eq!(serde_json::to_value(&p).expect("source after"), before);
            let records = attrs(&package["ppt/comments/comment1.xml"], "p:cm");
            assert!(!records.last().expect("absent sibling").contains_key("dt"));
        }
    }
    let mut root = comment("2000-01-01T00:00:00Z", "dated root");
    root.replies.push(comment("", "absent reply"));
    let p = deck(vec![root]);
    let original = p.clone();
    let bytes = deckcraft_pptx::export(&p).expect("undated reply");
    let q = deckcraft_pptx::import(&bytes).expect("reimport");
    assert_eq!(q.slides[0].comments, p.slides[0].comments);
    assert_eq!(p, original);
    assert!(!attrs(&parts(&bytes)["ppt/comments/comment1.xml"], "p:cm")[1].contains_key("dt"));
}

#[test]
fn independent_authored_labels_preserve_empty_presence_creation_time_and_nested_ownership() {
    for label in ["", "Friday", "2000-01-01T00:00:00Z"] {
        for date in ["", "2000-01-01T00:00:00Z"] {
            let mut annotated = comment(date, "labelled");
            annotated.display_date = Some(label.into());
            let mut root = comment("2000-01-01T00:00:00Z", "root");
            let mut reply = comment("2000-01-01T00:00:00Z", "reply");
            reply.replies.push(annotated.clone());
            root.replies.push(reply);
            for p in [deck(vec![annotated]), deck(vec![root])] {
                let original = p.clone();
                let bytes = deckcraft_pptx::export(&p).expect("preserved authored label");
                let q = deckcraft_pptx::import(&bytes).expect("reimport");
                assert_eq!(q.slides[0].comments, p.slides[0].comments, "exact labels, creation spelling, identities and depth");
                assert_eq!(p, original);
                let mut labelled = &q.slides[0].comments[0];
                while let Some(reply) = labelled.replies.first() {
                    labelled = reply;
                }
                assert_eq!(labelled.display_date.as_deref(), Some(label));
                assert_eq!(labelled.date, date);
                let package = parts(&bytes);
                let legacy = attrs(&package["ppt/comments/comment1.xml"], "p:cm");
                let actual = legacy.last().expect("labelled leaf");
                assert_eq!(actual.get("dt").map(String::as_str), (!date.is_empty()).then_some(date));
            }
        }
    }
}

#[test]
fn native_comment_position_metadata_preserves_fractional_coordinates_and_profile_boundaries() {
    for coordinate in [0.0, 0.1, 0.125, 1_000_000.0] {
        let mut c = comment("", "coordinate");
        c.x = coordinate;
        c.y = coordinate;
        let p = deck(vec![c]);
        let original = p.clone();
        let q = deckcraft_pptx::import(&deckcraft_pptx::export(&p).expect("retained coordinate")).expect("import");
        assert_eq!(q.slides[0].comments, p.slides[0].comments);
        assert_eq!(p, original);
    }
    for coordinate in [1_000_000.125, f64::MAX, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        for axis in 0..2 {
            let mut c = comment("", "invalid coordinate");
            if axis == 0 {
                c.x = coordinate;
            } else {
                c.y = coordinate;
            }
            let p = deck(vec![c]);
            refuse_unchanged(&p, "invalid comment position");
            let retained = if axis == 0 { p.slides[0].comments[0].x } else { p.slides[0].comments[0].y };
            assert_eq!(retained.to_bits(), coordinate.to_bits());
        }
    }
}

#[test]
fn legacy_only_count_guard_refuses_instead_of_truncating() {
    let p = deck((0..10_001).map(|_| comment("", "one past serialization cap")).collect());
    refuse_unchanged(&p, "root count");
}

#[test]
fn document_properties_and_date_looking_body_never_supply_an_absent_creation_time() {
    let mut p = deck(vec![comment("", "2000-01-01T00:00:00Z\n2024-01-01T00:00:00.000")]);
    p.props.created = "2030-12-31T23:59:59Z".into();
    p.props.modified = "2031-01-01T00:00:01Z".into();
    p.kept_parts.push(deckcraft_model::KeptPart {
        name: "customXml/item1.xml".into(),
        content_type: "application/xml".into(),
        xml: "<review xmlns=\"urn:deckcraft:independent-review\" created=\"2099-01-01T00:00:00Z\"><unknown>2034-01-01T00:00:00Z</unknown></review>"
            .into(),
        rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml".into(),
        ..Default::default()
    });
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let package = parts(&bytes);
    let records = attrs(&package["ppt/comments/comment1.xml"], "p:cm");
    assert_eq!(records.len(), 1);
    assert!(!records[0].contains_key("dt"));
    assert!(!package.contains_key("ppt/comments/modernComment1.xml"));
    assert_eq!(package["customXml/item1.xml"], p.kept_parts[0].xml);
    let q = deckcraft_pptx::import(&bytes).expect("reimport");
    assert_eq!(q.slides[0].comments, p.slides[0].comments);
}
