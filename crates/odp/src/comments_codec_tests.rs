#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use deckcraft_model::comments::ExtensionOwner;
use deckcraft_model::text::{Paragraph, Run, RunKind};
struct Rich;
impl BodyCodec for Rich {
    fn encode(&mut self, body: &TextBody) -> Result<EncodedBody> {
        let mut xml = String::new();
        for para in &body.paragraphs {
            xml.push_str("<text:p>");
            for run in &para.runs {
                if run.props.bold == Some(true) {
                    xml.push_str("<text:span text:style-name=\"TestBold\">");
                }
                if matches!(run.kind, RunKind::Break) {
                    xml.push_str("<text:line-break/>");
                } else {
                    Markers::default().text(&run.text, &mut xml)?;
                }
                if run.props.bold == Some(true) {
                    xml.push_str("</text:span>");
                }
            }
            xml.push_str("</text:p>");
        }
        Ok(EncodedBody { xml, projected: body.clone() })
    }
    fn decode(&mut self, annotation: &El) -> Result<DecodedBody> {
        fn inline(el: &El, para: &mut Paragraph, bold: bool) -> Result<()> {
            for node in &el.children {
                match node {
                    Node::Text(text) => {
                        let mut run = Run::new(text);
                        run.props.bold = if bold { Some(true) } else { None };
                        para.runs.push(run);
                    }
                    Node::El(el) => match el.name.as_str() {
                        "text:span" => inline(el, para, bold || el.attr("text:style-name") == Some("TestBold"))?,
                        "text:s" => para.runs.push(Run::new(" ")),
                        "text:tab" => para.runs.push(Run::new("\t")),
                        "text:line-break" => para.runs.push(Run { kind: RunKind::Break, ..Run::new("") }),
                        _ => inline(el, para, bold)?,
                    },
                }
            }
            Ok(())
        }
        let mut paragraphs = Vec::new();
        for el in annotation.elements().filter(|e| e.name == "text:p") {
            let mut para = Paragraph::default();
            inline(el, &mut para, false)?;
            paragraphs.push(para);
        }
        Ok(DecodedBody { body: TextBody { paragraphs, ..Default::default() }, unmapped: false })
    }
}
fn wrap(inner: &str) -> El {
    let mut xml = String::from("<draw:page");
    for (prefix, uri) in crate::xml::NS {
        xml.push_str(&format!(" xmlns:{prefix}=\"{uri}\""));
    }
    xml.push('>');
    xml.push_str(inner);
    xml.push_str("</draw:page>");
    crate::xml::parse(xml.as_bytes()).unwrap()
}
fn comment(id: u32, text: &str) -> Comment {
    Comment { comment_id: Some(id), author: "Author".into(), initials: "A".into(), text: text.into(), ..Default::default() }
}
fn map() -> WireMap {
    WireMap { owner: "slide:1".into(), texts: BTreeMap::new(), holder_style: "CommentHolder".into() }
}
fn pool() -> IdPool {
    IdPool::reserve(&mut 500, 10_000).unwrap()
}
fn floating(page: &El) -> BTreeSet<usize> {
    page.elements().filter_map(|el| holder_annotation(el, "CommentHolder")).map(El::source_start).collect()
}
#[test]
fn all_nested_messages_are_standard_annotations_without_invented_dates() {
    let mut c = comment(1, "root");
    let mut reply = comment(2, "reply");
    reply.replies.push(comment(3, "deep"));
    c.replies.push(reply);
    c.set_comment_status(comments::CommentStatus::Closed);
    let written = write_page(&[c], &map(), &mut Rich).unwrap();
    assert_eq!(written.floating.len(), 3);
    let page = wrap(&written.floating.concat());
    let actual = read_page(&page, "slide:1", &[], &floating(&page), &mut Rich, &mut pool(), &mut Budget::default()).unwrap();
    assert_eq!(actual.comments.len(), 3);
    assert!(actual.comments.iter().all(|c| c.date.is_empty()));
    assert_eq!(actual.projection, written.projection);
    assert!(!actual.retain_source);
}
#[test]
fn rich_comment_body_soft_break_and_exact_optional_metadata() {
    let mut c = comment(1, "bold\u{b}text");
    let mut body = TextBody::from_text("boldtext");
    body.paragraphs[0].runs = vec![
        Run { props: deckcraft_model::text::RunProps { bold: Some(true), ..Default::default() }, ..Run::new("bold") },
        Run { kind: RunKind::Break, ..Run::new("") },
        Run::new("text"),
    ];
    c.body = Some(body);
    c.date = "2025-03-01T12:00:00+02:00".into();
    c.display_date = Some(String::new());
    let written = write_page(&[c], &map(), &mut Rich).unwrap();
    let page = wrap(&written.floating.concat());
    let actual = read_page(&page, "slide:1", &[], &floating(&page), &mut Rich, &mut pool(), &mut Budget::default()).unwrap();
    assert_eq!(actual.projection, written.projection);
    assert_eq!(actual.comments[0].body.as_ref().unwrap().paragraphs[0].runs[0].props.bold, Some(true));
    assert_eq!(actual.comments[0].display_date, Some(String::new()));
}
#[test]
fn unicode_scalar_range_is_emitted_and_independently_verified() {
    let shape = ShapeId(42);
    let owner = TextBody::from_text("a😀 bc");
    let mut m = map();
    m.texts.insert(shape, WireText { key: "shape42".into(), body: owner.clone() });
    let mut c = comment(1, "range");
    c.set_comment_anchor(CommentAnchor::TextRange { shape, start: 1, end: 4 });
    let mut written = write_page(&[c], &m, &mut Rich).unwrap();
    let mut markers = written.markers.remove(&shape).unwrap();
    let mut text = String::from("<draw:text-box><text:p>");
    markers.text("a😀 bc", &mut text).unwrap();
    markers.finish(&mut text).unwrap();
    text.push_str("</text:p></draw:text-box>");
    let page = wrap(&text);
    let actual = read_page(
        &page,
        "slide:1",
        &[ReadText { element: page.elements().next().unwrap(), shape, wire: "shape42", body: &owner }],
        &BTreeSet::new(),
        &mut Rich,
        &mut pool(),
        &mut Budget::default(),
    )
    .unwrap();
    assert_eq!(actual.comments[0].comment_anchor(), CommentAnchor::TextRange { shape, start: 1, end: 4 });
    assert_eq!(actual.projection, written.projection);
}
#[test]
fn zero_length_range_keeps_start_and_end_at_one_position() {
    let shape = ShapeId(2);
    let owner = TextBody::from_text("ab");
    let mut m = map();
    m.texts.insert(shape, WireText { key: "s2".into(), body: owner.clone() });
    let mut c = comment(1, "point");
    c.set_comment_anchor(CommentAnchor::TextRange { shape, start: 1, end: 1 });
    let mut written = write_page(&[c], &m, &mut Rich).unwrap();
    let mut markers = written.markers.remove(&shape).unwrap();
    let mut xml = String::from("<draw:text-box><text:p>");
    markers.text("ab", &mut xml).unwrap();
    markers.finish(&mut xml).unwrap();
    xml.push_str("</text:p></draw:text-box>");
    let page = wrap(&xml);
    let actual = read_page(
        &page,
        "slide:1",
        &[ReadText { element: page.elements().next().unwrap(), shape, wire: "s2", body: &owner }],
        &BTreeSet::new(),
        &mut Rich,
        &mut pool(),
        &mut Budget::default(),
    )
    .unwrap();
    assert_eq!(actual.comments[0].comment_anchor(), CommentAnchor::TextRange { shape, start: 1, end: 1 });
    assert_eq!(actual.projection, written.projection);
}
#[test]
fn paragraph_and_soft_break_offsets_are_native_scalars() {
    let shape = ShapeId(2);
    let mut owner = TextBody::from_text("ab\ncd");
    owner.paragraphs[0].runs.push(Run { kind: RunKind::Break, ..Run::new("") });
    let xml = "<draw:text-box><text:p>ab<text:line-break/><office:annotation office:name=\"x\"><text:p>note</text:p></office:annotation></text:p><text:p>c<office:annotation-end office:name=\"x\"/>d</text:p></draw:text-box>";
    let page = wrap(xml);
    let scanned =
        scan_placements(&[ReadText { element: page.elements().next().unwrap(), shape, wire: "s2", body: &owner }], &mut Budget::default()).unwrap();
    let p = scanned.placements.values().next().unwrap();
    assert!(p.proven);
    assert_eq!((p.start, p.end), (3, Some(5)));
}
#[test]
fn ambiguous_whitespace_or_mismatched_body_never_invents_range() {
    let page = wrap(
        "<draw:text-box><text:p>a <office:annotation office:name=\"x\"><text:p>note</text:p></office:annotation>b<office:annotation-end office:name=\"x\"/></text:p></draw:text-box>",
    );
    let body = TextBody::from_text("a b");
    let actual = read_page(
        &page,
        "slide:1",
        &[ReadText { element: page.elements().next().unwrap(), shape: ShapeId(2), wire: "s2", body: &body }],
        &BTreeSet::new(),
        &mut Rich,
        &mut pool(),
        &mut Budget::default(),
    )
    .unwrap();
    assert_eq!(actual.comments[0].comment_anchor(), CommentAnchor::Unknown);
    assert!(actual.retain_source);
}
#[test]
fn namespace_spoof_does_not_become_annotation() {
    let page = wrap("<evil:annotation xmlns:evil=\"urn:evil\"><text:p>not a comment</text:p></evil:annotation>");
    let read = read_page(&page, "slide:1", &[], &BTreeSet::new(), &mut Rich, &mut pool(), &mut Budget::default()).unwrap();
    assert!(read.comments.is_empty());
}
#[test]
fn foreign_inline_is_retained_even_when_plaintext_happens_to_match() {
    let page = wrap(
        "<draw:text-box><text:p><evil:span xmlns:evil=\"urn:evil\">a</evil:span><office:annotation office:name=\"x\"><text:p>note</text:p></office:annotation></text:p></draw:text-box>",
    );
    let body = TextBody::from_text("a");
    let read = read_page(
        &page,
        "slide:1",
        &[ReadText { element: page.elements().next().unwrap(), shape: ShapeId(2), wire: "s2", body: &body }],
        &BTreeSet::new(),
        &mut Rich,
        &mut pool(),
        &mut Budget::default(),
    )
    .unwrap();
    assert!(read.retain_source);
    assert_eq!(read.comments[0].comment_anchor(), CommentAnchor::Unknown);
}
#[test]
fn duplicate_annotation_name_or_dangling_end_is_an_error() {
    let page = wrap(
        "<draw:text-box><text:p><office:annotation office:name=\"x\"><text:p>one</text:p></office:annotation><office:annotation office:name=\"x\"><text:p>two</text:p></office:annotation></text:p></draw:text-box>",
    );
    let body = TextBody::from_text("");
    assert!(
        scan_placements(&[ReadText { element: page.elements().next().unwrap(), shape: ShapeId(2), wire: "s2", body: &body }], &mut Budget::default())
            .is_err()
    );
    let page = wrap("<draw:text-box><text:p><office:annotation-end office:name=\"missing\"/></text:p></draw:text-box>");
    assert!(
        scan_placements(&[ReadText { element: page.elements().next().unwrap(), shape: ShapeId(2), wire: "s2", body: &body }], &mut Budget::default())
            .is_err()
    );
}
#[test]
fn native_regeneration_changes_when_core_representable_payload_changes() {
    let mut owner = ExtensionOwner { key: "slide:1".into(), projection_owners: vec!["slide:1".into()], comments: vec![comment(1, "original")] };
    let maps = BTreeMap::from([("slide:1".into(), map())]);
    let first = regenerate(&[owner.clone()], &maps, &mut Rich).unwrap();
    owner.comments[0].text = "changed".into();
    let changed = regenerate(&[owner], &maps, &mut Rich).unwrap();
    assert_ne!(first, changed);
}
#[test]
fn stable_source_name_survives_and_same_names_do_not_merge_authors() {
    let mut a = comment(1, "one");
    a.source_id = Some(CommentSourceId::Odf { name: "foreign-name".into() });
    let b = comment(2, "two");
    let written = write_page(&[a, b], &map(), &mut Rich).unwrap();
    assert_eq!(written.projection.authors.len(), 2);
    let page = wrap(&written.floating.concat());
    let read = read_page(&page, "slide:1", &[], &floating(&page), &mut Rich, &mut pool(), &mut Budget::default()).unwrap();
    assert_eq!(read.comments[0].source_id, Some(CommentSourceId::Odf { name: "foreign-name".into() }));
    assert_ne!(read.comments[0].author_identity.as_ref().unwrap().id, read.comments[1].author_identity.as_ref().unwrap().id);
}
#[test]
fn source_with_1001_messages_is_complete_and_invalid_xml_is_error() {
    let comments = (1..=1001).map(|id| comment(id, "note")).collect::<Vec<_>>();
    let written = write_page(&comments, &map(), &mut Rich).unwrap();
    let page = wrap(&written.floating.concat());
    let read = read_page(&page, "slide:1", &[], &floating(&page), &mut Rich, &mut pool(), &mut Budget::default()).unwrap();
    assert_eq!(read.comments.len(), 1001);
    let c = comment(1002, "bad\u{1}");
    assert!(write_page(&[c], &map(), &mut Rich).is_err());
}
