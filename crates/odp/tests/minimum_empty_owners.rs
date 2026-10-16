//! Original empty text owners retain editable line policy and Latin typing defaults.
use deckcraft_model::{Placeholder, Presentation, Shape, ShapeId, TextBody, Xfrm, text::LineHeight};
use std::sync::Arc;

#[test]
fn empty_placeholder_and_plain_notes_keep_their_line_policy_after_odp_rewrite() {
    for policy in [LineHeight::Normal, LineHeight::Minimum { points: 36.0 }] {
        let mut presentation = Presentation::default();
        let mut body = TextBody::from_text("");
        body.paragraphs[0].props.line_height = Some(policy);
        body.paragraphs[0].def_props.font = Some("BIZ UDPGothic".into());
        body.paragraphs[0].def_props.size = Some(22.0);
        let slide = Arc::make_mut(&mut presentation.slides[0]);
        slide.shapes = vec![Shape {
            id: ShapeId(900),
            ph: Some(Placeholder::default()),
            text: Some(body.clone()),
            xfrm: Some(Xfrm::new(20.0, 20.0, 200.0, 100.0)),
            ..Default::default()
        }];
        slide.notes = body;
        for _ in 0..2 {
            let output = deckcraft_odp::export(&presentation).unwrap();
            presentation = deckcraft_odp::import(&output).unwrap();
            assert!(!presentation.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", presentation.tags);
            assert_eq!(presentation.slides[0].shapes.len(), 1);
            for text in [presentation.slides[0].shapes[0].text.as_ref().unwrap(), &presentation.slides[0].notes] {
                assert!(text.is_empty());
                assert_eq!(text.paragraphs.len(), 1);
                assert_eq!(text.paragraphs[0].props.line_height, Some(policy));
                assert_eq!(text.paragraphs[0].def_props.font.as_deref(), Some("BIZ UDPGothic"));
                assert_eq!(text.paragraphs[0].def_props.size, Some(22.0));
            }
        }
    }
}
