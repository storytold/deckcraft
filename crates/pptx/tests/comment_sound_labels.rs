//! Contributor-original standard and bound-native package tests for per-link audio labels.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    Comment, MediaId, Presentation,
    comments::{self, AuthorOrigin, CommentAttachment, CommentAuthorIdentity, CommentStatus},
    defaults,
    text::{Action, Hyperlink, Run, TextBody},
};
use quick_xml::{Reader, events::Event};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Write},
    sync::Arc,
};
use zip::write::SimpleFileOptions;
const CORE: &str = "ppt/comments/modernComment1.xml";
const SIDECAR: &str = "ppt/deckcraft/comments.json";
const WARNING: &str = "pptx:comment-warning";
fn fixture(labels: &[Option<&str>]) -> Presentation {
    let mut p = defaults::new_presentation(None);
    // Contributor-original mono PCM silence, not an external asset.
    let mut wav = b"RIFF\x26\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x80\x3e\0\0\x02\0\x10\0data\x02\0\0\0\0\0".to_vec();
    wav.shrink_to_fit();
    let media = p.add_media("resource-basename.wav", "audio/wav", wav).unwrap();
    let author = p.alloc_id().unwrap();
    let id = p.alloc_id().unwrap();
    let attachment = p.alloc_id().unwrap();
    let mut body = TextBody::from_text("");
    body.paragraphs[0].runs = labels
        .iter()
        .enumerate()
        .map(|(i, label)| Run {
            text: format!("original{i}|"),
            props: deckcraft_model::text::RunProps {
                bold: Some(true),
                link: Some(Hyperlink {
                    action: Action::Url { url: "https://example.org/own-audio".into() },
                    tooltip: String::new(),
                    highlight_click: false,
                    sound: Some(media),
                    stop_sound: false,
                    zoom_return: None,
                    sound_name: label.map(str::to_string),
                }),
                ..Default::default()
            },
            kind: deckcraft_model::text::RunKind::Text,
        })
        .collect();
    let mut c = Comment {
        comment_id: Some(id),
        author: "Original Author".into(),
        initials: "OA".into(),
        date: "2026-10-16T12:34:56.789+03:00".into(),
        wire_guid: Some("{01234567-89AB-4CDE-8123-000000000002}".into()),
        text: body.text(),
        body: Some(body),
        author_identity: Some(CommentAuthorIdentity {
            id: author,
            origin: AuthorOrigin::Imported,
            wire_guid: Some("{01234567-89AB-4CDE-8123-000000000001}".into()),
            source_initials: Some("OA".into()),
            external_id: None,
            user_id: None,
            provider_id: None,
            source_scope: None,
        }),
        attachments: vec![CommentAttachment { id: attachment, media, name: "original-download.wav".into() }],
        ..Default::default()
    };
    c.set_comment_status(CommentStatus::Closed);
    Arc::make_mut(&mut p.slides[0]).comments.push(c);
    comments::validate(&p, true).unwrap();
    p
}
fn unpack(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = BTreeMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes).unwrap();
        out.insert(f.name().into(), bytes);
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
fn remove_empty_element(xml: &mut String, tag: &str, marker: &str) {
    let at = xml.find(marker).unwrap();
    let start = xml[..at].rfind(&format!("<{tag} ")).unwrap();
    let end = at + xml[at..].find("/>").unwrap() + 2;
    xml.replace_range(start..end, "");
}
fn standard_only(mut parts: BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
    parts.remove(SIDECAR);
    let name = "ppt/_rels/presentation.xml.rels";
    let mut xml = String::from_utf8(parts[name].clone()).unwrap();
    remove_empty_element(&mut xml, "Relationship", "Type=\"urn:deckcraft:relationships:comments:1\"");
    parts.insert(name.into(), xml.into_bytes());
    let name = "[Content_Types].xml";
    let mut xml = String::from_utf8(parts[name].clone()).unwrap();
    remove_empty_element(&mut xml, "Override", "PartName=\"/ppt/deckcraft/comments.json\"");
    parts.insert(name.into(), xml.into_bytes());
    parts
}
fn labels(p: &Presentation) -> Vec<Option<String>> {
    p.slides[0].comments[0].body.as_ref().unwrap().paragraphs[0]
        .runs
        .iter()
        .filter_map(|r| r.props.link.as_ref().map(|h| h.sound_name.clone()))
        .collect()
}
fn wire_labels(xml: &[u8]) -> Vec<Option<String>> {
    let mut reader = Reader::from_reader(xml);
    let mut out = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"snd" => {
                let name = e
                    .attributes()
                    .map(|a| a.unwrap())
                    .find(|a| a.key.as_ref() == b"name")
                    .map(|a| a.decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder()).unwrap().into_owned());
                out.push(name);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}
#[test]
fn standard_only_shared_audio_preserves_distinct_unicode_absent_and_empty_labels() {
    let requested = [Some("First playback"), Some("Écho partagé · 音🦀\tline\nnext\rend"), None, Some("")];
    let p = fixture(&requested);
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let parts = unpack(&bytes);
    let expected = requested.iter().map(|s| s.map(str::to_string)).collect::<Vec<_>>();
    assert_eq!(wire_labels(&parts[CORE]), expected);
    let actual = deckcraft_pptx::import(&repack(standard_only(parts))).unwrap();
    assert_eq!(labels(&actual), expected);
    assert!(!actual.tags.iter().any(|(k, _)| k == WARNING));
    let sounds = actual.slides[0].comments[0].body.as_ref().unwrap().paragraphs[0]
        .runs
        .iter()
        .map(|r| r.props.link.as_ref().unwrap().sound.unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(sounds.len(), 1);
    let again = deckcraft_pptx::import(&repack(standard_only(unpack(&deckcraft_pptx::export(&actual).unwrap())))).unwrap();
    assert_eq!(labels(&again), expected);
    assert!(!again.tags.iter().any(|(k, _)| k == WARNING));
}
#[test]
fn typed_native_sound_names_restore_without_filename_alias_guesses() {
    let p = fixture(&[Some("Different from resource filename"), Some("")]);
    let actual = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    assert_eq!(actual.slides[0].comments[0].comment_id, p.slides[0].comments[0].comment_id);
    assert_eq!(actual.slides[0].comments[0].comment_status(), CommentStatus::Closed);
    assert_eq!(labels(&actual), labels(&p));
    assert_eq!(actual.slides[0].comments[0].attachments[0].name, "original-download.wav");
    assert!(!actual.tags.iter().any(|(k, _)| k == WARNING));
}
#[test]
fn changing_actual_sound_label_invalidates_native_metadata_and_keeps_standard_label() {
    let p = fixture(&[Some("First playback")]);
    let mut parts = unpack(&deckcraft_pptx::export(&p).unwrap());
    let core = String::from_utf8(parts[CORE].clone()).unwrap();
    assert!(core.contains("First playback"));
    parts.insert(CORE.into(), core.replace("First playback", "Foreign edited label").into_bytes());
    let actual = deckcraft_pptx::import(&repack(parts)).unwrap();
    assert_eq!(labels(&actual), [Some("Foreign edited label".into())]);
    assert!(actual.tags.iter().any(|(k, _)| k == WARNING));
    assert!(actual.slides[0].comments[0].attachments.is_empty());
    assert_ne!(actual.slides[0].comments[0].comment_id, p.slides[0].comments[0].comment_id);
}
#[test]
fn native_label_claim_cannot_reuse_unchanged_actual_projection_witness() {
    let p = fixture(&[Some("First playback")]);
    let mut parts = unpack(&deckcraft_pptx::export(&p).unwrap());
    let mut extension: serde_json::Value = serde_json::from_slice(&parts[SIDECAR]).unwrap();
    extension["owners"][0]["comments"][0]["body"]["paragraphs"][0]["runs"][0]["props"]["link"]["soundName"] =
        serde_json::json!("Contradictory native label");
    parts.insert(SIDECAR.into(), serde_json::to_vec(&extension).unwrap());
    let actual = deckcraft_pptx::import(&repack(parts)).unwrap();
    assert_eq!(labels(&actual), [Some("First playback".into())]);
    assert!(actual.tags.iter().any(|(k, _)| k == WARNING));
    assert!(actual.slides[0].comments[0].attachments.is_empty());
}
#[test]
fn decoded_4096_byte_label_boundary_including_xml_escaping_roundtrips() {
    for label in ["L".repeat(4096), "<&\"'".repeat(1024)] {
        let p = fixture(&[Some(&label)]);
        let bytes = deckcraft_pptx::export(&p).unwrap();
        let actual = deckcraft_pptx::import(&bytes).unwrap();
        assert_eq!(labels(&actual), [Some(label)]);
        assert_eq!(actual.slides[0].comments[0].comment_id, p.slides[0].comments[0].comment_id);
        assert!(!actual.tags.iter().any(|(k, _)| k == WARNING));
    }
}
#[test]
fn unsupported_label_size_controls_or_missing_sound_fail_explicitly() {
    for label in ["x".repeat(4097), "invalid\0label".into(), "invalid\u{b}label".into()] {
        let mut p = fixture(&[Some("valid")]);
        Arc::make_mut(&mut p.slides[0]).comments[0].body.as_mut().unwrap().paragraphs[0].runs[0].props.link.as_mut().unwrap().sound_name =
            Some(label);
        assert!(deckcraft_pptx::export(&p).is_err());
    }
    for missing in [None, Some(MediaId(4_000_000))] {
        let mut p = fixture(&[Some("label")]);
        Arc::make_mut(&mut p.slides[0]).comments[0].body.as_mut().unwrap().paragraphs[0].runs[0].props.link.as_mut().unwrap().sound = missing;
        assert!(deckcraft_pptx::export(&p).is_err());
    }
}

#[test]
fn generic_shape_action_with_missing_sound_resource_cannot_silently_export() {
    for sound_name in [None, Some("Explicit label".into())] {
        let mut p = defaults::new_presentation(None);
        let id = deckcraft_model::ShapeId(p.alloc_id().unwrap());
        Arc::make_mut(&mut p.slides[0]).shapes.push(deckcraft_model::Shape {
            id,
            name: "Original generic sound action".into(),
            xfrm: Some(deckcraft_model::Xfrm::new(10.0, 10.0, 100.0, 50.0)),
            click: Some(Hyperlink {
                action: Action::NextSlide,
                tooltip: String::new(),
                highlight_click: false,
                sound: Some(MediaId(4_000_000)),
                stop_sound: false,
                zoom_return: None,
                sound_name,
            }),
            ..Default::default()
        });
        assert!(deckcraft_pptx::export(&p).is_err());
    }
}
