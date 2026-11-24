//! Regression tests for the maths edition's fixes to upstream behaviour (fonts with an installed
//! substitute, notes pages with an empty body placeholder). See `docs/fork-changes.md`.

use super::*;

#[test]
fn a_font_with_an_installed_substitute_is_not_listed_as_missing() {
    // "Cambria Math" is not installed on CI, but STIX Two Math (its substitute) is on macOS.
    // On a machine where neither is installed the family shows as missing; here we only check that
    // a family whose first substitute IS installed is silently covered.
    let db = deckcraft_fonts::FontDb::global();
    let subs = deckcraft_fonts::substitutes("Cambria Math");
    // Find a substitute that is actually installed so the test is meaningful on this machine.
    let Some(installed_sub) = subs.iter().find(|&&s| db.has_family(s)) else {
        // No substitute is installed → nothing to assert; skip.
        return;
    };
    // A deck that uses Cambria Math when an installed substitute exists should not toast.
    let data = deck_in_font("Cambria Math");
    let mut s = Session::with_new();
    let r = s.execute("file.openBytes", &json!({"name": "math.pptx", "data": data})).unwrap();
    assert!(
        !r["missingFonts"].as_array().is_some_and(|v| v.iter().any(|f| f == "Cambria Math")),
        "Cambria Math listed as missing even though {installed_sub} is installed: {:?}",
        r["missingFonts"]
    );
}

#[test]
fn opening_and_saving_notes_skips_an_empty_body_placeholder() {
    use std::io::{Cursor, Read, Write};

    const NOTES_TEXT: &str = "Speaker notes on slide one";
    const NOTES_PART: &str = "ppt/notesSlides/notesSlide1.xml";

    let mut p = deckcraft_model::defaults::new_presentation(None);
    std::sync::Arc::make_mut(&mut p.slides[0]).notes = deckcraft_model::text::TextBody::from_text(NOTES_TEXT);
    let bytes = deckcraft_pptx::export(&p).unwrap();

    // Some notes slides contain an empty body placeholder before the one with the notes.
    // It must not mask the later, populated placeholder.
    let mut src = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut found_notes = false;
    for i in 0..src.len() {
        let mut file = src.by_index(i).unwrap();
        let name = file.name().to_string();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        if name == NOTES_PART {
            let xml = String::from_utf8(data).unwrap();
            let body = r#"<p:sp><p:nvSpPr><p:cNvPr id="3""#;
            assert!(xml.contains(body), "expected populated notes placeholder in {NOTES_PART}");
            let empty_body = r#"<p:sp><p:nvSpPr><p:cNvPr id="4" name="Empty Notes Placeholder"/><p:cNvSpPr/><p:nvPr><p:ph type="body" idx="2"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p/></p:txBody></p:sp>"#;
            data = xml.replacen(body, &format!("{empty_body}{body}"), 1).into_bytes();
            found_notes = true;
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(&data).unwrap();
    }
    assert!(found_notes, "exported package has no notes part");
    let bytes = out.finish().unwrap().into_inner();

    let mut s = session();
    s.execute("file.openBytes", &json!({"name": "notes.pptx", "data": crate::cmd::base64_encode(&bytes)})).unwrap();
    let saved = s.execute("file.saveBytes", &json!({"format": "pptx"})).unwrap()["data"].as_str().unwrap().to_string();
    let saved = crate::cmd::base64_decode(&saved).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(saved)).unwrap();
    let mut xml = String::new();
    zip.by_name(NOTES_PART).unwrap().read_to_string(&mut xml).unwrap();
    assert!(xml.contains(NOTES_TEXT), "speaker notes are present in saved XML: {xml}");
}

#[test]
fn empty_notes_page_survives_open_and_save() {
    use std::io::{Cursor, Read, Write};

    const NOTES_TEXT: &str = "This text will be removed from the source notes page";
    const NOTES_PART: &str = "ppt/notesSlides/notesSlide1.xml";

    let mut p = deckcraft_model::defaults::new_presentation(None);
    std::sync::Arc::make_mut(&mut p.slides[0]).notes = deckcraft_model::text::TextBody::from_text(NOTES_TEXT);
    let bytes = deckcraft_pptx::export(&p).unwrap();

    // Keep the notes page relationship but remove its speaker text, as on a slide-number-only
    // notes page. The notes master still supplies the slide-number placeholder.
    let mut src = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut found_notes = false;
    for i in 0..src.len() {
        let mut file = src.by_index(i).unwrap();
        let name = file.name().to_string();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        if name == NOTES_PART {
            let xml = String::from_utf8(data).unwrap();
            let text = format!("<a:t>{NOTES_TEXT}</a:t>");
            assert!(xml.contains(&text), "expected speaker text in fixture notes page");
            data = xml.replace(&text, "<a:t></a:t>").into_bytes();
            found_notes = true;
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(&data).unwrap();
    }
    assert!(found_notes, "exported package has no notes part");
    let bytes = out.finish().unwrap().into_inner();

    let mut s = session();
    s.execute("file.openBytes", &json!({"name": "empty-notes.pptx", "data": crate::cmd::base64_encode(&bytes)})).unwrap();
    let opened = s.doc().unwrap().doc.clone();
    assert!(opened.slides[0].has_notes_slide);
    assert!(opened.slides[0].notes.is_empty());

    let saved = s.execute("file.saveBytes", &json!({"format": "pptx"})).unwrap()["data"].as_str().unwrap().to_string();
    let saved = crate::cmd::base64_decode(&saved).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(saved.clone())).unwrap();
    let mut xml = String::new();
    zip.by_name(NOTES_PART).unwrap().read_to_string(&mut xml).unwrap();
    assert!(!xml.contains(NOTES_TEXT));
    let mut notes_master = String::new();
    zip.by_name("ppt/notesMasters/notesMaster1.xml").unwrap().read_to_string(&mut notes_master).unwrap();
    assert!(notes_master.contains(r#"type="sldNum""#), "the notes master retains its slide-number placeholder");
    let reopened = deckcraft_pptx::import(&saved).unwrap();
    assert!(reopened.slides[0].has_notes_slide);
    assert!(reopened.slides[0].notes.is_empty());

    let native = s.execute("file.saveBytes", &json!({"format": "deckcraft"})).unwrap()["data"].as_str().unwrap().to_string();
    let mut native_session = session();
    native_session.execute("file.openBytes", &json!({"name": "empty-notes.deckcraft", "data": native})).unwrap();
    assert!(native_session.doc().unwrap().doc.slides[0].has_notes_slide);
}
