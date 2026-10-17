//! The document chat's part (`/craft/chat.json`, craft-collab `docs/chat.md`): written with its
//! content type and root relationship, read back as the model's chat, nothing written for an
//! empty chat, and a malformed part ignored (the file still opens).

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use deckcraft_model::Presentation;
use deckcraft_model::chat::{Chat, ChatAuthor, ChatMessage};
use deckcraft_pptx::PptxKind;
use zip::write::SimpleFileOptions;

fn chat() -> Chat {
    let ann = ChatAuthor { id: "u-ann".into(), name: "Ann".into(), color: "#3B7DD8".into() };
    let bo = ChatAuthor { id: "u-bo".into(), name: "Bo".into(), color: "#D8553B".into() };
    Chat {
        cleared_at: Some(1_700_000_000_000),
        messages: vec![
            ChatMessage {
                id: "a1b2c3d4".into(),
                author: ann.clone(),
                at: 1_700_000_100_000,
                text: "Slide 3 needs a chart\nand a title".into(),
                ..Default::default()
            },
            ChatMessage {
                id: "e5f6g7h8".into(),
                author: bo,
                at: 1_700_000_200_000,
                text: "On it, @Ann 😀".into(),
                reply_to: Some("a1b2c3d4".into()),
                edited: Some(1_700_000_300_000),
                ..Default::default()
            },
            ChatMessage { id: "i9j0k1l2".into(), author: ann, at: 1_700_000_400_000, deleted: true, ..Default::default() },
        ],
    }
}

fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).unwrap();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            (f.name().to_string(), data)
        })
        .collect()
}

fn text_of(bytes: &[u8], name: &str) -> String {
    entries(bytes).into_iter().find(|(n, _)| n == name).map(|(_, d)| String::from_utf8(d).unwrap()).unwrap_or_default()
}

/// `bytes` with `name`'s content replaced.
fn replace(bytes: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut out);
        for (n, d) in entries(bytes) {
            w.start_file(n.as_str(), SimpleFileOptions::default()).unwrap();
            w.write_all(if n == name { data } else { &d }).unwrap();
        }
        w.finish().unwrap();
    }
    out.into_inner()
}

#[test]
fn the_chat_part_round_trips() {
    let p = Presentation { chat: Arc::new(chat()), ..Default::default() };
    for kind in [PptxKind::Presentation, PptxKind::MacroPresentation] {
        let bytes = deckcraft_pptx::export_as(&p, kind).unwrap();
        let types = text_of(&bytes, "[Content_Types].xml");
        assert!(types.contains(r#"PartName="/craft/chat.json" ContentType="application/vnd.craft.chat+json""#), "{types}");
        let rels = text_of(&bytes, "_rels/.rels");
        assert!(rels.contains(r#"Type="urn:craft:relationships:chat" Target="craft/chat.json""#), "{rels}");
        let part: serde_json::Value = serde_json::from_str(&text_of(&bytes, "craft/chat.json")).unwrap();
        assert_eq!(part["version"], 1);
        assert_eq!(part["clearedAt"], 1_700_000_000_000i64);
        assert_eq!(part["messages"][1]["author"]["name"], "Bo");
        assert!(part["messages"][2].get("text").is_none());
        let back = deckcraft_pptx::import(&bytes).unwrap();
        assert_eq!(*back.chat, chat());
        // Modelled, not kept as an opaque part: saved again, it's there once.
        assert!(back.kept_parts.iter().all(|k| !k.name.contains("chat")));
        let again = deckcraft_pptx::export_as(&back, kind).unwrap();
        assert_eq!(text_of(&again, "_rels/.rels").matches("urn:craft:relationships:chat").count(), 1);
        assert_eq!(*deckcraft_pptx::import(&again).unwrap().chat, chat());
    }
}

#[test]
fn an_empty_chat_writes_no_part() {
    let bytes = deckcraft_pptx::export(&Presentation::default()).unwrap();
    assert!(entries(&bytes).iter().all(|(n, _)| n != "craft/chat.json"));
    assert!(!text_of(&bytes, "_rels/.rels").contains("chat"));
    assert!(deckcraft_pptx::import(&bytes).unwrap().chat.is_empty());
}

#[test]
fn a_malformed_chat_part_is_ignored() {
    let p = Presentation { chat: Arc::new(chat()), ..Default::default() };
    let bytes = deckcraft_pptx::export(&p).unwrap();
    for junk in [&b"not json at all"[..], br#"{"version": 2, "messages": []}"#, br#"[1, 2, 3]"#, b"\xff\xfe\x00"] {
        let bad = replace(&bytes, "craft/chat.json", junk);
        let back = deckcraft_pptx::import(&bad).expect("the file still opens");
        assert!(back.chat.is_empty());
        assert_eq!(back.slides.len(), p.slides.len());
    }
    // Some entries malformed: those are skipped, the rest read.
    let mixed = br##"{"version": 1, "messages": [
        {"id": "aaaaaaaa", "author": {"id": "u", "name": "Ann", "color": "#000000"}, "at": 5, "text": "kept"},
        {"id": "bbbbbbbb", "author": 7, "at": 5, "text": "bad author"},
        {"id": "cccccccc", "author": {"id": "u", "name": "Ann"}, "at": -1, "text": "bad at"},
        {"id": "dddddddd", "author": {"id": "u", "name": "Ann"}, "at": 5, "text": ""},
        {"id": "aaaaaaaa", "author": {"id": "u", "name": "Ann"}, "at": 6, "text": "duplicate"}
    ]}"##;
    let back = deckcraft_pptx::import(&replace(&bytes, "craft/chat.json", mixed)).unwrap();
    assert_eq!(back.chat.messages.len(), 1);
    assert_eq!(back.chat.messages[0].text, "kept");
}
