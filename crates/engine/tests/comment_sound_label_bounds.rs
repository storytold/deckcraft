//! Contributor-original command admission and atomicity tests for typed audio labels.
#![allow(clippy::unwrap_used)]
use deckcraft_engine::Session;
use deckcraft_model::{
    MediaId,
    text::{Action, Hyperlink, TextBody},
};
use serde_json::json;
#[test]
fn invalid_sound_label_edits_do_not_publish_document_or_history_changes() {
    let mut s = Session::with_new();
    let id = s.execute("comment.add", &json!({"text":"original"})).unwrap()["commentId"].clone();
    s.execute("comment.attachment.import", &json!({"commentId":id,"name":"own.bin","data":"AQMDBwAq"})).unwrap();
    let media = s.execute("comment.list", &json!({"commentId":id})).unwrap()["attachments"][0]["media"].as_u64().unwrap();
    let before = s.doc().unwrap().doc.clone();
    for (name, sound) in [
        ("invalid\0label".to_string(), Some(MediaId(media as u32))),
        ("invalid\u{b}label".into(), Some(MediaId(media as u32))),
        ("x".repeat(4097), Some(MediaId(media as u32))),
        ("orphan label".into(), None),
    ] {
        let mut body = TextBody::from_text("replacement");
        body.paragraphs[0].runs[0].props.link = Some(Hyperlink {
            action: Action::Url { url: "https://example.org/own".into() },
            tooltip: String::new(),
            highlight_click: false,
            sound,
            stop_sound: false,
            zoom_return: None,
            sound_name: Some(name),
        });
        assert!(s.execute("comment.edit", &json!({"commentId":id,"body":body})).is_err());
        assert_eq!(s.doc().unwrap().doc, before);
    }
    // Failed attempts must not add undo entries: one undo removes the preceding successful import.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.execute("comment.list", &json!({"commentId":id})).unwrap().get("attachments").is_none());
    assert_eq!(s.execute("comment.list", &json!({"commentId":id})).unwrap()["text"], "original");
}
#[test]
fn valid_unicode_empty_and_boundary_labels_are_editable_and_undoable() {
    let mut s = Session::with_new();
    let id = s.execute("comment.add", &json!({"text":"original"})).unwrap()["commentId"].clone();
    s.execute("comment.attachment.import", &json!({"commentId":id,"name":"own.bin","data":"AQMDBwAq"})).unwrap();
    let media = s.execute("comment.list", &json!({"commentId":id})).unwrap()["attachments"][0]["media"].as_u64().unwrap();
    for name in ["".to_string(), "Écho · 音🦀\tline\nnext\rend".into(), "<&\"'".repeat(1024)] {
        let before = s.doc().unwrap().doc.clone();
        let mut body = TextBody::from_text("replacement");
        body.paragraphs[0].runs[0].props.link = Some(Hyperlink {
            action: Action::Url { url: "https://example.org/own".into() },
            tooltip: String::new(),
            highlight_click: false,
            sound: Some(MediaId(media as u32)),
            stop_sound: false,
            zoom_return: None,
            sound_name: Some(name.clone()),
        });
        s.execute("comment.edit", &json!({"commentId":id,"body":body})).unwrap();
        assert_eq!(
            s.execute("comment.list", &json!({"commentId":id})).unwrap()["body"]["paragraphs"][0]["runs"][0]["props"]["link"]["soundName"],
            name
        );
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
    }
}
