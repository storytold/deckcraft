//! Contributor-original independent regression for master/layout artwork visibility.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::*;
use std::{
    io::{Cursor, Read},
    sync::Arc,
};

fn artwork(id: u32, text: &str, hidden: bool) -> Shape {
    let mut body = TextBody::from_text(text);
    body.paragraphs[0].runs[0].props.font = Some("BIZ UDPGothic".into());
    Shape {
        id: ShapeId(id),
        name: text.into(),
        xfrm: Some(Xfrm::new(10.0, f64::from(id), 100.0, 25.0)),
        text: Some(body),
        line: Some(Line::none()),
        hidden,
        ..Default::default()
    }
}

#[test]
fn master_and_layout_artwork_follow_the_same_four_visibility_combinations_as_renderer() {
    for slide_shows_shared in [false, true] {
        for layout_shows_master in [false, true] {
            let layout = Layout {
                id: LayoutId(20),
                kind: LayoutType::Blank,
                shapes: vec![artwork(200, "LAYOUT_VISIBLE", false)],
                show_master_shapes: layout_shows_master,
                ..Default::default()
            };
            let master =
                Master { id: MasterId(10), shapes: vec![artwork(100, "MASTER_VISIBLE", false)], layouts: vec![layout], ..Default::default() };
            let slide = Slide {
                id: SlideId(30),
                layout: LayoutId(20),
                show_master_shapes: slide_shows_shared,
                shapes: vec![artwork(300, "LOCAL_VISIBLE", false), artwork(301, "LOCAL_HIDDEN", true)],
                ..Default::default()
            };
            let p = Presentation { masters: vec![Arc::new(master)], slides: vec![Arc::new(slide)], ..Default::default() };
            let bytes = deckcraft_odp::export(&p).unwrap();
            let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
            let mut content = String::new();
            archive.by_name("content.xml").unwrap().read_to_string(&mut content).unwrap();
            let mut styles = String::new();
            archive.by_name("styles.xml").unwrap().read_to_string(&mut styles).unwrap();
            assert!(!content.contains("MASTER_VISIBLE"));
            assert_eq!(styles.matches("draw:name=\"MASTER_VISIBLE\"").count(), 1);
            assert_eq!(styles.matches(">MASTER_VISIBLE<").count(), 1);
            assert_eq!(content.contains("LAYOUT_VISIBLE"), slide_shows_shared);
            let reopened = deckcraft_odp::import(&bytes).unwrap();
            let find = |label: &str| reopened.slides[0].shapes.iter().find(|shape| shape.text.as_ref().is_some_and(|body| body.text() == label));
            assert!(find("MASTER_VISIBLE").is_none());
            assert_eq!(find("LAYOUT_VISIBLE").is_some(), slide_shows_shared);
            for presentation in [&p, &reopened] {
                let placed = deckcraft_render::place_slide(presentation, 0);
                assert_eq!(placed.texts.iter().any(|text| text.body.text() == "MASTER_VISIBLE"), slide_shows_shared && layout_shows_master);
                assert_eq!(placed.texts.iter().any(|text| text.body.text() == "LAYOUT_VISIBLE"), slide_shows_shared);
                assert!(placed.texts.iter().any(|text| text.body.text() == "LOCAL_VISIBLE"));
                assert!(!placed.texts.iter().any(|text| text.body.text() == "LOCAL_HIDDEN"));
            }
            assert!(!find("LOCAL_VISIBLE").unwrap().hidden);
            assert!(find("LOCAL_HIDDEN").unwrap().hidden);
        }
    }
}
