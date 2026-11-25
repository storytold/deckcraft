//! Presentation text defaults across PPTX import, export and reopen.

use std::io::{Cursor, Read, Write};

use craft_xml::Local;
use deckcraft_model::{Presentation, defaults};

const PRESENTATION: &str = "ppt/presentation.xml";

fn imported_with_style(style_xml: &str) -> Presentation {
    let base = deckcraft_pptx::export(&defaults::new_presentation(None)).expect("author fixture");
    let mut original = zip::ZipArchive::new(Cursor::new(base)).expect("open fixture package");
    let mut fixture = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..original.len() {
        let mut part = original.by_index(i).expect("package part");
        fixture.start_file(part.name(), zip::write::SimpleFileOptions::default()).expect("write package part");
        if part.name() == PRESENTATION {
            let mut xml = String::new();
            part.read_to_string(&mut xml).expect("author fixture presentation XML");
            let start = xml.find("<p:defaultTextStyle>").expect("new decks have a populated style");
            let end_tag = "</p:defaultTextStyle>";
            let end = xml.find(end_tag).expect("end of presentation text style") + end_tag.len();
            xml.replace_range(start..end, style_xml);
            fixture.write_all(xml.as_bytes()).expect("write fixture presentation XML");
        } else {
            std::io::copy(&mut part, &mut fixture).expect("copy fixture part");
        }
    }
    let bytes = fixture.finish().expect("finish fixture package").into_inner();
    deckcraft_pptx::import(&bytes).expect("import authored fixture")
}

fn assert_empty_after_two_saves(mut presentation: Presentation) {
    assert!(presentation.default_text_style.is_empty(), "the imported style begins empty");
    for save in 1..=2 {
        let bytes = deckcraft_pptx::export(&presentation).expect("save PPTX");
        let mut package = zip::ZipArchive::new(Cursor::new(&bytes)).expect("open saved package");
        let mut xml = Vec::new();
        package.by_name(PRESENTATION).expect("presentation part").read_to_end(&mut xml).expect("read presentation XML");
        let document = craft_drawingml::xmlx::parse(&xml).expect("parse saved presentation XML");
        assert!(
            document.child(Local("defaultTextStyle")).is_none_or(|style| style.elements().next().is_none()),
            "save {save} must not add inherited text formatting to an empty presentation style"
        );
        presentation = deckcraft_pptx::import(&bytes).expect("reopen PPTX");
        assert!(presentation.default_text_style.is_empty(), "reopen {save} must retain the empty style");
    }
}

#[test]
fn an_absent_presentation_text_style_stays_empty_after_two_saves() {
    assert_empty_after_two_saves(imported_with_style(""));
}

#[test]
fn an_empty_presentation_text_style_stays_empty_after_two_saves() {
    assert_empty_after_two_saves(imported_with_style("<p:defaultTextStyle/>"));
}

#[test]
fn authored_presentation_text_styles_survive_two_saves() {
    let imported = imported_with_style(r#"<p:defaultTextStyle><a:lvl1pPr><a:defRPr kern="8800"/></a:lvl1pPr></p:defaultTextStyle>"#);
    assert_eq!(imported.default_text_style.level(0).expect("explicit level").run.kern, Some(88.0));
    let new_deck = defaults::new_presentation(None);
    assert!(!new_deck.default_text_style.is_empty());

    for mut presentation in [imported, new_deck] {
        let expected = presentation.default_text_style.clone();
        for _ in 0..2 {
            let bytes = deckcraft_pptx::export(&presentation).expect("save authored style");
            presentation = deckcraft_pptx::import(&bytes).expect("reopen authored style");
            assert_eq!(presentation.default_text_style, expected);
        }
    }
}
