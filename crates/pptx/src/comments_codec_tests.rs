#![allow(clippy::unwrap_used, clippy::panic)]
use super::*;
use deckcraft_model::text::{Paragraph, Run, RunKind, RunProps};
const MESSAGE: &str = "{01234567-89AB-CDEF-0123-456789ABCDEF}";
const REPLY: &str = "{01234567-89AB-CDEF-0123-000000000002}";
const AUTHOR: &str = "{01234567-89AB-CDEF-0123-000000000001}";
struct Rich;
impl BodyCodec for Rich {
    fn encode(&mut self, body: &TextBody) -> Result<EncodedBody> {
        let mut xml = Xml::new();
        xml.raw("<p188:txBody><a:bodyPr/><a:lstStyle/>")?;
        for paragraph in &body.paragraphs {
            xml.raw("<a:p>")?;
            for run in &paragraph.runs {
                if run.kind == RunKind::Break {
                    xml.raw("<a:br/>")?;
                    continue;
                }
                xml.raw("<a:r><a:rPr")?;
                if let Some(b) = run.props.bold {
                    xml.attr("b", if b { "1" } else { "0" })?;
                }
                if let Some(i) = run.props.italic {
                    xml.attr("i", if i { "1" } else { "0" })?;
                }
                xml.raw("/><a:t>")?;
                xml.escaped(&run.text, false)?;
                xml.raw("</a:t></a:r>")?;
            }
            xml.raw("</a:p>")?;
        }
        xml.raw("</p188:txBody>")?;
        Ok(EncodedBody { xml: xml.0, projected: body.clone() })
    }
    fn decode(&mut self, body: &crate::xml::El) -> Result<DecodedBody> {
        let paragraphs = body
            .children_named("p")
            .map(|p| {
                let runs = p
                    .elements()
                    .filter_map(|el| match el.local() {
                        "r" => Some(Run {
                            text: el.child("t").map(|t| t.text()).unwrap_or_default(),
                            props: RunProps {
                                bold: el.child("rPr").and_then(|p| p.bool("b")),
                                italic: el.child("rPr").and_then(|p| p.bool("i")),
                                ..Default::default()
                            },
                            kind: RunKind::Text,
                        }),
                        "br" => Some(Run { text: String::new(), props: Default::default(), kind: RunKind::Break }),
                        _ => None,
                    })
                    .collect();
                Paragraph { runs, ..Default::default() }
            })
            .collect();
        Ok(DecodedBody { body: TextBody { paragraphs, ..Default::default() }, unmapped: false })
    }
}
fn identity(id: u32, guid: &str) -> CommentAuthorIdentity {
    CommentAuthorIdentity {
        id,
        origin: AuthorOrigin::Imported,
        external_id: Some(guid.into()),
        user_id: Some("opaque-user".into()),
        provider_id: Some("opaque-provider".into()),
        source_initials: Some("PA".into()),
        source_scope: Some("original-part".into()),
        wire_guid: Some(guid.into()),
    }
}
fn comment() -> Comment {
    let mut body = TextBody::from_text("rich α");
    body.paragraphs[0].runs[0].props.bold = Some(true);
    Comment {
        author: "Pat".into(),
        initials: "PA".into(),
        text: "rich α".into(),
        date: "2026-10-16T12:00:00Z".into(),
        body: Some(body),
        comment_id: Some(10),
        author_identity: Some(identity(20, AUTHOR)),
        wire_guid: Some(MESSAGE.into()),
        ..Default::default()
    }
}
fn owners(c: Comment) -> Vec<ExtensionOwner> {
    vec![ExtensionOwner { key: "page0".into(), projection_owners: vec!["page0:modern".into()], comments: vec![c] }]
}
fn map() -> WireMap {
    WireMap { owner: "page0".into(), slide_id: 256, shapes: BTreeMap::new() }
}
fn read_written(written: &Written, table: &Authors, map: &WireMap) -> Read {
    let mut dom = dom::Budget::default();
    let author_bytes = write_authors(table, written.lane == Lane::Modern, &written.last_indices).unwrap();
    let mut next = 100;
    let mut ids = IdPool::reserve(&mut next, 100).unwrap();
    let authors = read_authors(&author_bytes, "source", &mut ids, &mut dom).unwrap();
    read_page(&written.bytes, &authors, map, &mut Rich, &mut ids, &mut dom).unwrap()
}
#[test]
fn real_modern_xml_reparses_rich_body_closed_status_and_exact_identity() {
    let mut c = comment();
    c.set_comment_status(CommentStatus::Closed);
    let owners = owners(c.clone());
    let authors = Authors::native(&owners, &BTreeMap::new()).unwrap();
    let written = write_page(&owners[0].comments, &authors, &map(), &mut Rich, false).unwrap();
    assert_eq!(written.lane, Lane::Modern);
    let read = read_written(&written, &authors, &map());
    assert_eq!(read.projection, written.projection);
    assert_eq!(read.comments[0].comment_status(), CommentStatus::Closed);
    assert_eq!(read.comments[0].body, c.body);
    assert_eq!(read.comments[0].wire_guid, c.wire_guid);
    assert_eq!(read.comments[0].author_identity.as_ref().unwrap().provider_id, Some("opaque-provider".into()));
}
#[test]
fn nested_native_replies_are_all_visible_direct_replies_with_stable_ids() {
    let mut root = comment();
    let mut reply = comment();
    reply.comment_id = Some(11);
    reply.wire_guid = Some(REPLY.into());
    let mut deep = comment();
    deep.comment_id = Some(12);
    deep.wire_guid = Some("{01234567-89AB-CDEF-0123-000000000003}".into());
    reply.replies.push(deep);
    root.replies.push(reply);
    let owners = owners(root);
    let table = Authors::native(&owners, &BTreeMap::new()).unwrap();
    let written = write_page(&owners[0].comments, &table, &map(), &mut Rich, false).unwrap();
    let read = read_written(&written, &table, &map());
    assert_eq!(read.comments[0].replies.len(), 2);
    assert!(read.comments[0].replies.iter().all(|c| c.replies.is_empty()));
    assert_eq!(read.projection, written.projection);
}
#[test]
fn missing_created_or_wire_guid_uses_real_legacy_without_fabrication() {
    for absent_date in [true, false] {
        let mut c = comment();
        if absent_date {
            c.date.clear();
        } else {
            c.wire_guid = None;
        }
        let owners = owners(c);
        let table = Authors::native(&owners, &BTreeMap::new()).unwrap();
        let written = write_page(&owners[0].comments, &table, &map(), &mut Rich, false).unwrap();
        assert_eq!(written.lane, Lane::Legacy);
        let read = read_written(&written, &table, &map());
        if absent_date {
            assert!(read.comments[0].date.is_empty());
            assert!(!String::from_utf8_lossy(&written.bytes).contains(" dt="));
        }
        assert_eq!(read.projection, written.projection);
        assert!(!read.retain_source, "ordinary legacy scalar text is a fully parsed standard projection");
    }
}
#[test]
fn actual_shape_ids_and_relative_position_are_resolved_not_changed_to_unknown() {
    let mut c = comment();
    c.x = 120.0;
    c.y = 75.0;
    c.set_comment_anchor(CommentAnchor::Shape { shape: ShapeId(30) });
    let owners = owners(c.clone());
    let table = Authors::native(&owners, &BTreeMap::new()).unwrap();
    let mut map = map();
    map.shapes.insert(ShapeId(30), WireShape { id: 900, kind: ShapeKind::Picture, origin_x: 100.0, origin_y: 50.0, creation_id: None });
    let written = write_page(&owners[0].comments, &table, &map, &mut Rich, false).unwrap();
    let xml = String::from_utf8_lossy(&written.bytes);
    assert!(xml.contains("ac:picMk id=\"900\""));
    assert!(xml.contains("x=\"254000\""));
    let read = read_written(&written, &table, &map);
    assert_eq!(read.comments[0].shape, Some(ShapeId(30)));
    assert_eq!((read.comments[0].x, read.comments[0].y), (c.x, c.y));
    assert_eq!(read.projection, written.projection);
}
#[test]
fn task_dates_assigned_identity_and_absent_fields_survive_actual_core_parse() {
    let mut c = comment();
    c.task = Some(CommentTask {
        start_date: Some("2026-10-16T12:00:00Z".into()),
        due_date: None,
        assigned_to: vec![20],
        authors: vec![],
        complete: Some("50.25%".into()),
        title: Some("".into()),
    });
    let owners = owners(c);
    let table = Authors::native(&owners, &BTreeMap::new()).unwrap();
    let written = write_page(&owners[0].comments, &table, &map(), &mut Rich, false).unwrap();
    let read = read_written(&written, &table, &map());
    let task = read.comments[0].task.as_ref().unwrap();
    assert_eq!(task.complete, Some("50.25%".into()));
    assert!(task.due_date.is_none());
    assert_eq!(task.title, Some("".into()));
    assert_eq!(task.authors.len(), 1);
    assert_eq!(read.projection, written.projection);
}
#[test]
fn same_names_with_different_author_identity_are_not_merged() {
    let mut a = comment();
    let mut b = comment();
    b.comment_id = Some(11);
    b.wire_guid = Some(REPLY.into());
    b.author_identity = Some(identity(21, "{01234567-89AB-CDEF-0123-000000000099}"));
    a.replies.push(b);
    let owners = owners(a);
    let table = Authors::native(&owners, &BTreeMap::new()).unwrap();
    assert_eq!(table.by_local.len(), 2);
    let written = write_page(&owners[0].comments, &table, &map(), &mut Rich, false).unwrap();
    let read = read_written(&written, &table, &map());
    assert_ne!(read.comments[0].author_identity.as_ref().unwrap().id, read.comments[0].replies[0].author_identity.as_ref().unwrap().id);
}
#[test]
fn arbitrary_prefixes_and_shadowed_namespaces_cannot_spoof_core_fields() {
    let owners = owners(comment());
    let table = Authors::native(&owners, &BTreeMap::new()).unwrap();
    let written = write_page(&owners[0].comments, &table, &map(), &mut Rich, false).unwrap();
    let xml = String::from_utf8(written.bytes.clone()).unwrap().replace("p188:", "q:").replace("xmlns:p188=", "xmlns:q=");
    let mut changed = Written { bytes: xml.into_bytes(), ..written };
    assert_eq!(read_written(&changed, &table, &map()).comments.len(), 1);
    changed.bytes = String::from_utf8(changed.bytes).unwrap().replace(dom::M, "urn:foreign").into_bytes();
    let mut next = 500;
    let mut ids = IdPool::reserve(&mut next, 50).unwrap();
    assert!(read_page(&changed.bytes, &table, &map(), &mut Rich, &mut ids, &mut dom::Budget::default()).is_err());
}
#[test]
fn unknown_officeart_text_range_is_retained_with_a_precise_diagnostic() {
    let owners = owners(comment());
    let table = Authors::native(&owners, &BTreeMap::new()).unwrap();
    let mut written = write_page(&owners[0].comments, &table, &map(), &mut Rich, false).unwrap();
    let xml = String::from_utf8(written.bytes).unwrap().replace(
        "<pc:sldMkLst><pc:docMk/><pc:sldMk sldId=\"256\"/></pc:sldMkLst>",
        "<ac:txMkLst><pc:docMk/><pc:sldMk sldId=\"256\"/><ac:txMk cp=\"1\" len=\"2\"/></ac:txMkLst>",
    );
    written.bytes = xml.into_bytes();
    let read = read_written(&written, &table, &map());
    assert!(read.retain_source);
    assert!(read.issues.iter().any(|issue| issue.contains("CP/context")));
    assert_eq!(read.comments[0].comment_anchor(), CommentAnchor::Unknown);
}
#[test]
fn regenerated_projection_comes_from_payload_not_saved_witness() {
    let mut original = owners(comment());
    original[0].projection_owners.push("page0:legacy".into());
    let maps = BTreeMap::from([("page0".into(), map())]);
    let first = regenerate(&original, &BTreeMap::new(), &maps, &mut Rich, true).unwrap();
    let mut changed = original;
    changed[0].comments[0].set_comment_status(CommentStatus::Closed);
    changed[0].comments[0].body.as_mut().unwrap().paragraphs[0].runs[0].props.bold = Some(false);
    let second = regenerate(&changed, &BTreeMap::new(), &maps, &mut Rich, true).unwrap();
    assert_ne!(first, second);
}
#[test]
fn more_than_a_thousand_replies_are_really_serialized_and_reparsed() {
    let mut root = comment();
    for index in 0..1001 {
        let mut reply = comment();
        reply.comment_id = Some(1000 + index);
        reply.wire_guid = Some(format!("{{01234567-89AB-CDEF-0123-{:012X}}}", 1000 + index));
        root.replies.push(reply);
    }
    let owners = owners(root);
    let table = Authors::native(&owners, &BTreeMap::new()).unwrap();
    let written = write_page(&owners[0].comments, &table, &map(), &mut Rich, false).unwrap();
    let author_bytes = write_authors(&table, true, &written.last_indices).unwrap();
    let mut next = 10000;
    let mut ids = IdPool::reserve(&mut next, 4000).unwrap();
    let mut budget = dom::Budget::default();
    let parsed_authors = read_authors(&author_bytes, "source", &mut ids, &mut budget).unwrap();
    let read = read_page(&written.bytes, &parsed_authors, &map(), &mut Rich, &mut ids, &mut budget).unwrap();
    assert_eq!(read.comments[0].replies.len(), 1001);
    assert_eq!(read.projection.messages.len(), 1002);
    assert_eq!(read.projection, written.projection);
}

// Proposed addition to crates/pptx/src/comments_codec_tests.rs. UNRUN.
#[test]
fn legacy_scalar_projection_does_not_call_rich_body_projection() {
    struct StrictRich;
    impl BodyCodec for StrictRich {
        fn projection(&self, body: &dom::Element) -> Result<Option<String>> {
            assert!(body.is(dom::M, "txBody"), "legacy scalar text is not a rich body");
            Ok(Some("modern rich witness".into()))
        }
        fn encode(&mut self, body: &TextBody) -> Result<EncodedBody> {
            Rich.encode(body)
        }
        fn decode(&mut self, body: &crate::xml::El) -> Result<DecodedBody> {
            Rich.decode(body)
        }
    }
    let mut c = comment();
    c.date.clear();
    let owners = owners(c);
    let table = Authors::native(&owners, &BTreeMap::new()).unwrap();
    let written = write_page(&owners[0].comments, &table, &map(), &mut StrictRich, false).unwrap();
    assert_eq!(written.lane, Lane::Legacy);
    let author_bytes = write_authors(&table, false, &written.last_indices).unwrap();
    let mut next = 100;
    let mut ids = IdPool::reserve(&mut next, 100).unwrap();
    let mut budget = dom::Budget::default();
    let authors = read_authors(&author_bytes, "source", &mut ids, &mut budget).unwrap();
    let read = read_page(&written.bytes, &authors, &map(), &mut StrictRich, &mut ids, &mut budget).unwrap();
    assert_eq!(read.projection, written.projection);
    assert!(!read.retain_source);
    assert_eq!(read.projection.messages[0].body_xml, None);
}

// Proposed addition to crates/pptx/src/comments_codec_tests.rs. UNRUN.
#[test]
fn comment_dom_refuses_nested_repeated_and_trailing_xml_declarations() {
    let decl = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>";
    for source in [format!("{decl}<x>{decl}<a/></x>"), format!("{decl}{decl}<x/>"), format!("<x/>{decl}"), format!(" {decl}<x/>")] {
        assert!(dom::parse(source.as_bytes(), &mut dom::Budget::default()).is_err(), "accepted invalid XML: {source}");
    }
    assert!(dom::parse(format!("{decl}<x/>").as_bytes(), &mut dom::Budget::default()).is_ok());
    assert!(dom::parse(b"<x/>", &mut dom::Budget::default()).is_ok());
}

#[test]
fn comment_attribute_limits_measure_decoded_strings_and_bound_encoded_entities() {
    let valid = format!("<x value=\"{}\"/>", "&quot;".repeat(8192));
    let root = dom::parse(valid.as_bytes(), &mut dom::Budget::default()).unwrap();
    assert_eq!(root.attr("value").unwrap(), "\"".repeat(8192));
    for invalid in [format!("<x value=\"{}\"/>", "a".repeat(8193)), format!("<x value=\"{}\"/>", "&quot;".repeat(8193))] {
        assert!(dom::parse(invalid.as_bytes(), &mut dom::Budget::default()).is_err());
    }
}
