//! Native line-height policies retain exact values and reject invalid records before repair.
use deckcraft_model::{
    Presentation, Shape, ShapeId, TextBody, Xfrm,
    text::{LineHeight, Spacing},
};
use std::sync::Arc;

#[test]
fn native_line_height_roundtrip_and_invalid_save_are_atomic() {
    for policy in [LineHeight::Normal, LineHeight::Minimum { points: 0.0 }, LineHeight::Minimum { points: 26.123456789 }] {
        let mut presentation = Presentation::default();
        let mut body = TextBody::from_text("Original minimum");
        body.paragraphs[0].props.line_height = Some(policy);
        let id = ShapeId(presentation.alloc_id().unwrap());
        Arc::make_mut(&mut presentation.slides[0]).shapes =
            vec![Shape { id, xfrm: Some(Xfrm::new(0.0, 0.0, 300.0, 100.0)), text: Some(body), ..Default::default() }];
        let output = deckcraft_format::save(&presentation).unwrap();
        let back = deckcraft_format::load(&output).unwrap();
        assert_eq!(back.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0].props.line_height, Some(policy));
        let props = &mut Arc::make_mut(&mut presentation.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].props;
        props.line_spacing = Some(Spacing::Pts(12.0));
        let before = presentation.clone();
        assert!(deckcraft_format::save(&presentation).unwrap_err().to_string().contains("conflicting"));
        assert_eq!(presentation, before);
    }
    for points in [-1.0, 1_000_000.1, f64::NAN, f64::INFINITY] {
        let mut presentation = Presentation::default();
        let notes = &mut Arc::make_mut(&mut presentation.slides[0]).notes;
        *notes = TextBody::from_text("Invalid policy");
        notes.paragraphs[0].props.line_height = Some(LineHeight::Minimum { points });
        assert!(deckcraft_format::save(&presentation).is_err(), "invalid policy must not be repaired into zero");
    }
}
