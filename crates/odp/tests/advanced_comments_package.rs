//! Contributor-original actual package tests for advanced comment semantics and stale metadata.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    comments::{self, AuthorOrigin, CommentAnchor, CommentAttachment, CommentAuthorIdentity, CommentStatus},
    text::{Action, Hyperlink, RunProps, TextBody},
    *,
};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};
use zip::write::SimpleFileOptions;
fn fixture() -> Presentation {
    let mut p = defaults::new_presentation(None);
    let shape = ShapeId(p.alloc_id().unwrap());
    let author = p.alloc_id().unwrap();
    let root = p.alloc_id().unwrap();
    let reply = p.alloc_id().unwrap();
    let deep = p.alloc_id().unwrap();
    let file = MediaId(p.alloc_id().unwrap());
    let attachment = p.alloc_id().unwrap();
    let identity = CommentAuthorIdentity {
        id: author,
        origin: AuthorOrigin::Imported,
        external_id: Some("opaque-original-author".into()),
        user_id: Some("opaque-user".into()),
        provider_id: Some("opaque-provider".into()),
        source_initials: Some("AB".into()),
        source_scope: Some("original-source".into()),
        wire_guid: Some("{01234567-89AB-CDEF-0123-000000000001}".into()),
    };
    let mut body = TextBody::from_text("rich comment");
    body.paragraphs[0].runs[0].props = RunProps {
        bold: Some(true),
        link: Some(Hyperlink {
            action: Action::Url { url: "https://example.org/contributor-original".into() },
            tooltip: "own link".into(),
            highlight_click: false,
            sound: None,
            stop_sound: false,
            zoom_return: None,
            sound_name: None,
        }),
        ..Default::default()
    };
    let create = |id, guid: &str, text: &str| Comment {
        comment_id: Some(id),
        author: "A. Author".into(),
        initials: "AB".into(),
        author_identity: Some(identity.clone()),
        text: text.into(),
        date: "2026-03-04T05:06:07.123+03:00".into(),
        wire_guid: Some(guid.into()),
        ..Default::default()
    };
    let mut c = create(root, "{01234567-89AB-CDEF-0123-000000000002}", "rich comment");
    c.body = Some(body);
    c.display_date = Some("Original authored label".into());
    c.set_comment_status(CommentStatus::Closed);
    c.set_comment_anchor(CommentAnchor::TextRange { shape, start: 1, end: 4 });
    c.attachments.push(CommentAttachment { id: attachment, media: file, name: "original.bin".into() });
    let mut r = create(reply, "{01234567-89AB-CDEF-0123-000000000003}", "reply");
    r.replies.push(create(deep, "{01234567-89AB-CDEF-0123-000000000004}", "deep reply"));
    c.replies.push(r);
    p.media.push(MediaItem {
        id: file,
        name: "original.bin".into(),
        content_type: "application/octet-stream".into(),
        data: Arc::new(vec![1, 3, 3, 7, 0, 42]),
        link: None,
    });
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.shapes.push(Shape {
        id: shape,
        name: "Own comment text owner".into(),
        xfrm: Some(Xfrm::new(30.0, 40.0, 250.0, 80.0)),
        text: Some(TextBody::from_text("a😀 bc")),
        ..Default::default()
    });
    slide.comments.push(c);
    p
}
fn unpack(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = BTreeMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        out.insert(f.name().into(), data);
    }
    out
}
fn repack(parts: BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in parts {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
#[test]
fn actual_package_restores_full_thread_rich_body_files_exact_dates_identity_and_range() {
    let p = fixture();
    let encoded = deckcraft_odp::export(&p).unwrap();
    emit_completion_package("advanced-comments.odp", &encoded);
    let content = unpack(&encoded).remove("content.xml").unwrap();
    let content = String::from_utf8(content).unwrap();
    let start = content.find("<office:annotation ").unwrap();
    let annotation = &content[start..];
    let positions = ["<dc:creator>", "<dc:date>", "<meta:date-string>", "<meta:creator-initials>"].map(|tag| annotation.find(tag).unwrap());
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "annotation metadata must follow the ODF sequence");

    let actual = deckcraft_odp::import(&encoded).unwrap();
    let c = &actual.slides[0].comments[0];
    assert_eq!(c.comment_id, p.slides[0].comments[0].comment_id);
    assert_eq!(c.comment_status(), CommentStatus::Closed);
    assert_eq!(c.replies[0].replies[0].text, "deep reply");
    assert_eq!(c.date, "2026-03-04T05:06:07.123+03:00");
    assert_eq!(c.display_date, Some("Original authored label".into()));
    assert_eq!(c.author_identity.as_ref().unwrap().provider_id, Some("opaque-provider".into()));
    assert_eq!(c.body, p.slides[0].comments[0].body);
    let shape = actual.slides[0].shapes.iter().find(|s| s.name == "Own comment text owner").unwrap();
    assert_eq!(c.comment_anchor(), CommentAnchor::TextRange { shape: shape.id, start: 1, end: 4 });
    assert_eq!(actual.media(c.attachments[0].media).unwrap().data.as_slice(), [1, 3, 3, 7, 0, 42]);
    comments::validate(&actual, true).unwrap();
    assert!(!actual.tags.iter().any(|(k, _)| k == WARNING));
    let twice = deckcraft_odp::import(&deckcraft_odp::export(&actual).unwrap()).unwrap();
    assert_eq!(twice.slides[0].comments[0].replies[0].replies[0].text, "deep reply");
}
#[test]
fn payload_claim_with_matching_saved_witness_cannot_bless_changed_rich_body() {
    let mut parts = unpack(&deckcraft_odp::export(&fixture()).unwrap());
    let mut extension: serde_json::Value = serde_json::from_slice(parts.get(SIDECAR).unwrap()).unwrap();
    extension["owners"][0]["comments"][0]["body"]["paragraphs"][0]["runs"][0]["props"]["bold"] = serde_json::json!(false);
    parts.insert(SIDECAR.into(), serde_json::to_vec(&extension).unwrap());
    let actual = deckcraft_odp::import(&repack(parts)).unwrap();
    assert!(actual.tags.iter().any(|(key, _)| key == WARNING));
    let c = &actual.slides[0].comments[0];
    assert!(c.body.as_ref().unwrap().paragraphs[0].runs.iter().any(|r| r.props.bold == Some(true)));
    assert!(c.attachments.is_empty());
}
#[test]
fn standard_body_edit_invalidates_extension_and_keeps_actual_visible_text() {
    let mut parts = unpack(&deckcraft_odp::export(&fixture()).unwrap());
    let body = String::from_utf8(parts.get(CORE).unwrap().clone()).unwrap();
    assert!(body.contains("rich"));
    parts.insert(CORE.into(), body.replace("rich", "changed").into_bytes());
    let actual = deckcraft_odp::import(&repack(parts)).unwrap();
    assert!(actual.slides[0].comments[0].text.contains("changed"));
    assert!(actual.slides[0].comments[0].attachments.is_empty());
    assert!(actual.tags.iter().any(|(key, _)| key == WARNING));
}
#[test]
fn undated_missing_wire_identity_keeps_absence_and_full_native_thread() {
    let mut p = fixture();
    let c = &mut Arc::make_mut(&mut p.slides[0]).comments[0];
    c.date.clear();
    c.wire_guid = None;
    let encoded = deckcraft_odp::export(&p).unwrap();
    let actual = deckcraft_odp::import(&encoded).unwrap();
    let c = &actual.slides[0].comments[0];
    assert!(c.date.is_empty());
    assert_eq!(c.wire_guid, None);
    assert_eq!(c.comment_status(), CommentStatus::Closed);
    assert_eq!(c.replies[0].replies[0].text, "deep reply");
    assert!(!actual.tags.iter().any(|(key, _)| key == WARNING));
}
const SIDECAR: &str = "DeckCraft/comments.json";
const CORE: &str = "content.xml";
const WARNING: &str = "odp:warning";

fn emit_completion_package(name: &str, bytes: &[u8]) {
    if let Ok(directory) = std::env::var("DECKCRAFT_COMPLETION_QA_DIR") {
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join(name), bytes).unwrap();
    }
}
