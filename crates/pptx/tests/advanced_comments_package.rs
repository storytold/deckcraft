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
    let encoded = deckcraft_pptx::export(&p).unwrap();
    emit_completion_package("advanced-comments.pptx", &encoded);
    let modern = unpack(&encoded).into_iter().find(|(name, _)| name == "ppt/comments/modernComment1.xml").unwrap().1;
    assert_eq!(modern.windows(5).filter(|bytes| *bytes == b"<?xml").count(), 1, "embedded rich bodies must be XML fragments");
    let actual = deckcraft_pptx::import(&encoded).unwrap();
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
    let twice = deckcraft_pptx::import(&deckcraft_pptx::export(&actual).unwrap()).unwrap();
    assert_eq!(twice.slides[0].comments[0].replies[0].replies[0].text, "deep reply");
}
#[test]
fn payload_claim_with_matching_saved_witness_cannot_bless_changed_rich_body() {
    let mut parts = unpack(&deckcraft_pptx::export(&fixture()).unwrap());
    let mut extension: serde_json::Value = serde_json::from_slice(parts.get(SIDECAR).unwrap()).unwrap();
    extension["owners"][0]["comments"][0]["body"]["paragraphs"][0]["runs"][0]["props"]["bold"] = serde_json::json!(false);
    parts.insert(SIDECAR.into(), serde_json::to_vec(&extension).unwrap());
    let actual = deckcraft_pptx::import(&repack(parts)).unwrap();
    assert!(actual.tags.iter().any(|(key, _)| key == WARNING));
    let c = &actual.slides[0].comments[0];
    assert!(c.body.as_ref().unwrap().paragraphs[0].runs.iter().any(|r| r.props.bold == Some(true)));
    assert!(c.attachments.is_empty());
}
#[test]
fn standard_body_edit_invalidates_extension_and_keeps_actual_visible_text() {
    let mut parts = unpack(&deckcraft_pptx::export(&fixture()).unwrap());
    let body = String::from_utf8(parts.get(CORE).unwrap().clone()).unwrap();
    assert!(body.contains("rich"));
    parts.insert(CORE.into(), body.replace("rich", "changed").into_bytes());
    let actual = deckcraft_pptx::import(&repack(parts)).unwrap();
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
    let encoded = deckcraft_pptx::export(&p).unwrap();
    let actual = deckcraft_pptx::import(&encoded).unwrap();
    let c = &actual.slides[0].comments[0];
    assert!(c.date.is_empty());
    assert_eq!(c.wire_guid, None);
    assert_eq!(c.comment_status(), CommentStatus::Closed);
    assert_eq!(c.replies[0].replies[0].text, "deep reply");
    assert!(!actual.tags.iter().any(|(key, _)| key == WARNING));
}
const SIDECAR: &str = "ppt/deckcraft/comments.json";
const CORE: &str = "ppt/comments/modernComment1.xml";
const WARNING: &str = "pptx:comment-warning";

fn emit_completion_package(name: &str, bytes: &[u8]) {
    if let Ok(directory) = std::env::var("DECKCRAFT_COMPLETION_QA_DIR") {
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join(name), bytes).unwrap();
    }
}

// Append to crates/pptx/tests/advanced_comments_package.rs. UNRUN.
fn without_comment_sidecar(mut parts: BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
    parts.remove(SIDECAR);
    let name = "ppt/_rels/presentation.xml.rels";
    let mut rels = String::from_utf8(parts.get(name).unwrap().clone()).unwrap();
    let marker = rels.find("Type=\"urn:deckcraft:relationships:comments:1\"").unwrap();
    let start = rels.get(..marker).unwrap().rfind("<Relationship ").unwrap();
    let end = marker + rels.get(marker..).unwrap().find("/>").unwrap() + 2;
    rels.replace_range(start..end, "");
    parts.insert(name.into(), rels.into_bytes());
    parts
}
#[test]
fn unsupported_known_namespace_rich_attribute_retains_actual_standard_body_and_source() {
    let parts = without_comment_sidecar(unpack(&deckcraft_pptx::export(&fixture()).unwrap()));
    let baseline = deckcraft_pptx::import(&repack(parts.clone())).unwrap();
    assert!(!baseline.tags.iter().any(|(key, _)| key == WARNING), "clean standard-only input must not manufacture a warning");
    let mut parts = parts;
    let xml = String::from_utf8(parts.get(CORE).unwrap().clone()).unwrap();
    assert!(xml.contains("<a:rPr"));
    let changed = xml.replacen("<a:rPr", "<a:rPr altLang=\"fr-FR\"", 1);
    parts.insert(CORE.into(), changed.into_bytes());
    let encoded = repack(parts);
    let actual = deckcraft_pptx::import(&encoded).unwrap();
    assert_eq!(actual.slides[0].comments[0].text, "rich comment");
    assert!(actual.slides[0].comments[0].body.as_ref().unwrap().paragraphs[0].runs.iter().any(|run| run.props.bold == Some(true)));
    assert!(actual.tags.iter().any(|(key, _)| key == WARNING));
    assert!(
        actual.media.iter().any(|item| item.content_type == "application/vnd.deckcraft.pptx-source" && item.data.as_slice() == encoded.as_slice())
    );
}

#[test]
fn unrepresented_hyperlink_action_preserves_visible_comment_and_original_source() {
    let mut parts = without_comment_sidecar(unpack(&deckcraft_pptx::export(&fixture()).unwrap()));
    let xml = String::from_utf8(parts.get(CORE).unwrap().clone()).unwrap();
    assert!(xml.contains("<a:hlinkClick"));
    parts.insert(CORE.into(), xml.replacen("<a:hlinkClick", "<a:hlinkClick action=\"ppaction://unmapped?opaque=1\"", 1).into_bytes());
    let encoded = repack(parts);
    let actual = deckcraft_pptx::import(&encoded).unwrap();
    assert_eq!(actual.slides[0].comments[0].text, "rich comment");
    assert!(actual.tags.iter().any(|(key, _)| key == WARNING));
    assert!(
        actual.media.iter().any(|item| item.content_type == "application/vnd.deckcraft.pptx-source" && item.data.as_slice() == encoded.as_slice())
    );
}

#[test]
fn native_picture_artistic_reflection_sound_and_field_profile_restores_without_warning() {
    let mut p = fixture();
    let file = p.media[0].id;
    let c = &mut Arc::make_mut(&mut p.slides[0]).comments[0];
    let body = c.body.as_mut().unwrap();
    let props = &mut body.paragraphs[0].runs[0].props;
    props.fill = Some(deckcraft_model::Fill::Picture(deckcraft_model::style::PictureFill {
        media: file,
        artistic: Some(deckcraft_model::style::Artistic { effect: "artisticBlur".into(), original: file }),
        alpha: Some(0.75),
        ..Default::default()
    }));
    props.reflection = Some(deckcraft_model::style::Reflection { blur: 2.0, start_alpha: 0.8, end_alpha: 0.1, end_pos: 0.9, dist: 3.0 });
    props.link.as_mut().unwrap().sound = Some(file);
    body.paragraphs[0].runs.push(deckcraft_model::text::Run {
        text: "3".into(),
        kind: deckcraft_model::text::RunKind::Field { field: "slidenum".into() },
        props: Default::default(),
    });
    c.text = body.text();
    for label in [None, Some("Écho partagé · Αρχείο"), Some("")] {
        let mut p = p.clone();
        Arc::make_mut(&mut p.slides[0]).comments[0].body.as_mut().unwrap().paragraphs[0].runs[0].props.link.as_mut().unwrap().sound_name =
            label.map(str::to_string);
        let encoded = deckcraft_pptx::export(&p).unwrap();
        let actual = deckcraft_pptx::import(&encoded).unwrap();
        assert_eq!(actual.slides[0].comments[0].comment_id, p.slides[0].comments[0].comment_id);
        assert_eq!(actual.slides[0].comments[0].comment_status(), CommentStatus::Closed);
        assert_eq!(actual.slides[0].comments[0].text, "rich comment3");
        assert_eq!(
            actual.slides[0].comments[0].body.as_ref().unwrap().paragraphs[0].runs[0].props.link.as_ref().unwrap().sound_name.as_deref(),
            label
        );
        assert!(!actual.tags.iter().any(|(key, _)| key == WARNING));
    }
}

#[test]
fn native_raw_omml_comment_preserves_math_attributes_without_warning() {
    let mut p = fixture();
    let c = &mut Arc::make_mut(&mut p.slides[0]).comments[0];
    let body = c.body.as_mut().unwrap();
    body.paragraphs[0].runs.push(deckcraft_model::text::Run {
        text: "x".into(),
        kind: deckcraft_model::text::RunKind::Math {
            omml: r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:rPr><m:sty m:val="i"/></m:rPr><m:t>x</m:t></m:r></m:oMath>"#.into(),
        },
        props: Default::default(),
    });
    c.text = body.text();
    let actual = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    assert_eq!(actual.slides[0].comments[0].comment_id, p.slides[0].comments[0].comment_id);
    assert_eq!(actual.slides[0].comments[0].body, p.slides[0].comments[0].body);
    assert!(!actual.tags.iter().any(|(key, _)| key == WARNING));
}
