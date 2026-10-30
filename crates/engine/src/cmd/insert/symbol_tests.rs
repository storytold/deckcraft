use deckcraft_model::{Presentation, ShapeId, text::TextBody};
use serde_json::json;

use crate::{Session, cmd::file};

fn editing() -> (Session, ShapeId) {
    let mut session = Session::with_new();
    let id = session.execute("insert.textBox", &json!({"text": "ab"})).unwrap()["id"].as_u64().unwrap();
    session.execute("format.font", &json!({"family": "Inter"})).unwrap();
    session.execute("text.edit", &json!({"id": id, "at": [0, 1]})).unwrap();
    (session, ShapeId(id as u32))
}

fn body(doc: &Presentation, id: ShapeId) -> TextBody {
    doc.slides[0].shape(id).unwrap().text.clone().unwrap()
}

#[test]
fn symbol_font_is_local_and_survives_native_and_pptx_saves() {
    let (mut s, id) = editing();
    s.execute("insert.symbol", &json!({"text": "α", "font": "Liberation Serif"})).unwrap();
    assert_eq!(s.execute("format.state", &json!({})).unwrap()["font"], "Inter");
    s.execute("text.insert", &json!({"text": "Z"})).unwrap();
    for format in ["deckcraft", "pptx"] {
        let bytes = file::save_bytes(&s.doc().unwrap().doc, format).unwrap();
        let doc = file::open_presentation(&format!("symbols.{format}"), &bytes).unwrap();
        let text = body(&doc, id);
        assert_eq!(text.text(), "aαZb");
        let props = |index| deckcraft_model::edit::props_at(&text, (0, index));
        assert_eq!(props(2).font.as_deref(), Some("Liberation Serif"), "{format}");
        assert_eq!(props(2).font_cs.as_deref(), Some("Liberation Serif"), "{format}");
        assert_eq!(props(3).font.as_deref(), Some("Inter"), "{format}");
        assert_eq!(props(4).font.as_deref(), Some("Inter"), "{format}");
    }
}

#[test]
fn consecutive_symbols_resume_the_original_font_and_undo_one_at_a_time() {
    let (mut s, id) = editing();
    let before = s.doc().unwrap().history.undo.len();
    for c in ["©", "™"] {
        s.execute("insert.symbol", &json!({"text": c, "font": "Liberation Serif"})).unwrap();
    }
    assert_eq!(s.doc().unwrap().history.undo.len(), before + 2);
    assert_eq!(body(&s.doc().unwrap().doc, id).text(), "a©™b");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(body(&s.doc().unwrap().doc, id).text(), "a©b");
    s.execute("edit.redo", &json!({})).unwrap();
    s.execute("text.insert", &json!({"text": "Z"})).unwrap();
    let text = body(&s.doc().unwrap().doc, id);
    assert_eq!(text.text(), "a©™Zb");
    assert_eq!(deckcraft_model::edit::props_at(&text, (0, 4)).font.as_deref(), Some("Inter"));
}

#[test]
fn symbols_are_literal_and_replace_a_selection() {
    let (mut s, id) = editing();
    s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, 2]})).unwrap();
    s.execute("insert.symbol", &json!({"text": "\""})).unwrap();
    assert_eq!(body(&s.doc().unwrap().doc, id).text(), "\"");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(body(&s.doc().unwrap().doc, id).text(), "ab");
}

#[test]
fn bad_font_arguments_do_not_edit_the_document() {
    let (mut s, id) = editing();
    let undo = s.doc().unwrap().history.undo.len();
    for font in [json!(null), json!(true), json!(" "), json!("a".repeat(257))] {
        assert!(s.execute("insert.symbol", &json!({"text": "©", "font": font})).is_err());
    }
    assert_eq!(body(&s.doc().unwrap().doc, id).text(), "ab");
    assert_eq!(s.doc().unwrap().history.undo.len(), undo);
}

#[test]
fn explicit_formatting_after_a_symbol_applies_to_the_next_text() {
    let (mut s, id) = editing();
    s.execute("insert.symbol", &json!({"text": "©", "font": "Liberation Serif"})).unwrap();
    s.execute("format.font", &json!({"family": "Liberation Mono"})).unwrap();
    s.execute("text.insert", &json!({"text": "Z"})).unwrap();
    let text = body(&s.doc().unwrap().doc, id);
    assert_eq!(deckcraft_model::edit::props_at(&text, (0, 2)).font.as_deref(), Some("Liberation Serif"));
    assert_eq!(deckcraft_model::edit::props_at(&text, (0, 3)).font.as_deref(), Some("Liberation Mono"));
}

#[test]
fn bold_after_a_symbol_without_a_font_formats_the_current_word() {
    let (mut s, id) = editing();
    s.execute("insert.symbol", &json!({"text": "x"})).unwrap();
    s.execute("format.bold", &json!({})).unwrap();
    let text = body(&s.doc().unwrap().doc, id);
    assert_eq!(text.text(), "axb");
    for index in 1..=3 {
        assert_eq!(deckcraft_model::edit::props_at(&text, (0, index)).bold, Some(true));
    }
    s.execute("edit.undo", &json!({})).unwrap();
    let text = body(&s.doc().unwrap().doc, id);
    assert_eq!(text.text(), "axb");
    for index in 1..=3 {
        assert_ne!(deckcraft_model::edit::props_at(&text, (0, index)).bold, Some(true));
    }
}

#[test]
fn input_method_text_after_a_symbol_resumes_the_surrounding_font() {
    let (mut s, id) = editing();
    s.execute("insert.symbol", &json!({"text": "©", "font": "Liberation Serif"})).unwrap();
    s.execute("text.compose", &json!({"text": "ka"})).unwrap();
    s.execute("text.commit", &json!({"text": "か"})).unwrap();
    let text = body(&s.doc().unwrap().doc, id);
    assert_eq!(text.text(), "a©かb");
    assert_eq!(deckcraft_model::edit::props_at(&text, (0, 3)).font.as_deref(), Some("Inter"));
}

#[test]
fn symbol_replacement_resumes_the_remaining_texts_formatting() {
    for font in [None, Some("Liberation Mono")] {
        let (mut s, id) = editing();
        s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, 1]})).unwrap();
        s.execute("format.font", &json!({"family": "Liberation Serif"})).unwrap();
        let mut params = json!({"text": "\u{00a9}"});
        if let Some(font) = font {
            params["font"] = json!(font);
        }
        s.execute("insert.symbol", &params).unwrap();
        assert_eq!(s.execute("format.state", &json!({})).unwrap()["font"], "Inter");
        s.execute("text.insert", &json!({"text": "Z"})).unwrap();
        let text = body(&s.doc().unwrap().doc, id);
        assert_eq!(text.text(), "\u{00a9}Zb");
        assert_eq!(deckcraft_model::edit::props_at(&text, (0, 1)).font.as_deref(), Some(font.unwrap_or("Inter")));
        assert_eq!(deckcraft_model::edit::props_at(&text, (0, 2)).font.as_deref(), Some("Inter"));
    }
}

#[test]
fn symbol_at_a_hyperlink_end_does_not_extend_the_link() {
    for at in [1, 2] {
        let (mut s, id) = editing();
        s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, at]})).unwrap();
        s.execute("insert.hyperlink", &json!({"url": "https://example.com"})).unwrap();
        s.execute("text.edit", &json!({"id": id.0, "at": [0, at]})).unwrap();
        s.execute("insert.symbol", &json!({"text": "\u{00a9}", "font": "Liberation Serif"})).unwrap();
        s.execute("text.insert", &json!({"text": "Z"})).unwrap();
        let text = body(&s.doc().unwrap().doc, id);
        assert!(deckcraft_model::edit::props_at(&text, (0, at)).link.is_some());
        assert!(deckcraft_model::edit::props_at(&text, (0, at + 1)).link.is_none());
        assert!(deckcraft_model::edit::props_at(&text, (0, at + 2)).link.is_none());
    }
}

#[test]
fn symbol_inside_a_hyperlink_preserves_the_link() {
    let (mut s, id) = editing();
    s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, 2]})).unwrap();
    s.execute("insert.hyperlink", &json!({"url": "https://example.com"})).unwrap();
    s.execute("text.edit", &json!({"id": id.0, "at": [0, 1]})).unwrap();
    s.execute("insert.symbol", &json!({"text": "\u{00a9}", "font": "Liberation Serif"})).unwrap();
    let text = body(&s.doc().unwrap().doc, id);
    assert!(deckcraft_model::edit::props_at(&text, (0, 2)).link.is_some());
}

#[test]
fn first_strong_rtl_symbol_promotes_the_paragraph() {
    for symbol in ["\u{05d0}", "\u{0627}"] {
        let (mut s, id) = editing();
        s.execute("text.selectAll", &json!({})).unwrap();
        s.execute("insert.symbol", &json!({"text": symbol})).unwrap();
        let text = body(&s.doc().unwrap().doc, id);
        assert_eq!(text.paragraphs[0].props.rtl, Some(true));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(body(&s.doc().unwrap().doc, id).text(), "ab");
    }
}
