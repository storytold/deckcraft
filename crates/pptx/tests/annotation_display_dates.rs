//! Contributor-original comment fixtures; a display label is independent of creation time.
use std::{
    io::{Cursor, Read},
    sync::Arc,
};

use deckcraft_model::{Comment, Presentation, comments};
use quick_xml::{Reader, events::Event};

fn legacy_creation_attributes(bytes: &[u8]) -> Vec<Option<String>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert!(zip.by_name("ppt/deckcraft/comments.json").is_ok(), "typed independent labels require their verified extension");
    let mut xml = String::new();
    zip.by_name("ppt/comments/comment1.xml").unwrap().read_to_string(&mut xml).unwrap();
    let mut reader = Reader::from_str(&xml);
    let mut dates = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(element) | Event::Empty(element) if element.local_name().as_ref() == b"cm" => {
                dates.push(element.attributes().map(|attribute| attribute.unwrap()).find(|attribute| attribute.key.as_ref() == b"dt").map(
                    |attribute| attribute.decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder()).unwrap().into_owned(),
                ));
            }
            Event::Eof => return dates,
            _ => {}
        }
    }
}

#[test]
fn independent_comment_date_labels_and_creation_presence_survive_every_package_kind() {
    for label in ["", "Morning rehearsal", "2026-10-16T12:00:00Z"] {
        for reply in [false, true] {
            let mut presentation = Presentation::default();
            let comment = Comment {
                author: "Original contributor".into(),
                initials: "OC".into(),
                text: "Labelled comment".into(),
                date: "2026-10-16T12:00:00Z".into(),
                display_date: Some(label.into()),
                ..Default::default()
            };
            Arc::make_mut(&mut presentation.slides[0]).comments = if reply {
                vec![Comment {
                    author: "Root contributor".into(),
                    initials: "RC".into(),
                    text: "Undated root".into(),
                    replies: vec![comment],
                    ..Default::default()
                }]
            } else {
                vec![comment]
            };
            // Prepare explicit local identities before taking the immutable source witness.
            comments::normalize(&mut presentation).unwrap();
            let original = presentation.clone();
            for extension in ["pptx", "potx", "ppsx"] {
                let bytes = deckcraft_pptx::export_as(&presentation, extension).unwrap();
                let back = deckcraft_pptx::import(&bytes).unwrap();
                assert_eq!(back.slides[0].comments, original.slides[0].comments, "{extension}: date, label, identity and reply tree");
                let labelled = if reply { &back.slides[0].comments[0].replies[0] } else { &back.slides[0].comments[0] };
                assert_eq!(labelled.date, "2026-10-16T12:00:00Z");
                assert_eq!(labelled.display_date.as_deref(), Some(label));
                let dates = legacy_creation_attributes(&bytes);
                assert_eq!(dates, if reply { vec![None, Some("2026-10-16T12:00:00Z".into())] } else { vec![Some("2026-10-16T12:00:00Z".into())] });
                if reply {
                    assert!(back.slides[0].comments[0].date.is_empty(), "a reply/display label cannot supply its parent's creation time");
                }
                let twice = deckcraft_pptx::import(&deckcraft_pptx::export_as(&back, extension).unwrap()).unwrap();
                assert_eq!(twice.slides[0].comments, back.slides[0].comments);
                assert_eq!(presentation, original, "export mutated source");
            }
        }
    }
}

#[test]
fn creation_only_comments_remain_exportable_without_inventing_a_label() {
    let mut presentation = Presentation::default();
    Arc::make_mut(&mut presentation.slides[0]).comments.push(Comment {
        author: "DeckCraft contributor".into(),
        initials: "DC".into(),
        text: "Keep this creation timestamp".into(),
        date: "2026-10-16T12:00:00Z".into(),
        ..Default::default()
    });
    let original = presentation.clone();
    let bytes = deckcraft_pptx::export(&presentation).unwrap();
    let reopened = deckcraft_pptx::import(&bytes).unwrap();
    assert_eq!(reopened.slides[0].comments[0].date, "2026-10-16T12:00:00Z");
    assert_eq!(reopened.slides[0].comments[0].display_date, None);
    assert_eq!(presentation, original);
}
