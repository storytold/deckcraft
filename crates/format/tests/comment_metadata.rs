//! Contributor-original annotations exercise native metadata and old positional records.
use std::sync::Arc;

use deckcraft_model::{Comment, Presentation};
use serde_json::json;

#[test]
fn native_comment_dates_preserve_independent_presence_and_text() {
    let mut presentation = Presentation::default();
    for display_date in [None, Some(String::new()), Some("Rehearsal T → Z\r\n漢".into())] {
        Arc::make_mut(&mut presentation.slides[0]).comments = vec![Comment {
            author: "DeckCraft contributor".into(),
            initials: "DC".into(),
            text: "Notes with independent dates".into(),
            date: "2026-10-16T12:00:00Z".into(),
            display_date,
            replies: vec![Comment { display_date: Some("Later".into()), ..Default::default() }],
            ..Default::default()
        }];
        let bytes = deckcraft_format::save(&presentation).unwrap();
        let reopened = deckcraft_format::load(&bytes).unwrap();
        let mut expected = presentation.clone();
        deckcraft_model::comments::normalize(&mut expected).unwrap();
        assert_eq!(reopened.slides[0].comments, expected.slides[0].comments);
        assert_eq!(deckcraft_format::load(&deckcraft_format::save(&reopened).unwrap()).unwrap().slides[0].comments, reopened.slides[0].comments);
    }
}

#[test]
fn old_named_and_positional_comments_default_to_no_display_label() {
    for value in [
        json!({"author":"Original author", "date":"2026-10-16T12:00:00Z"}),
        json!(["Original author", "OA", "Original body", "2026-10-16T12:00:00Z", 1.0, 2.0, false, [], null]),
    ] {
        let comment: Comment = serde_json::from_value(value).unwrap();
        assert_eq!(comment.display_date, None);
        assert_eq!(comment.date, "2026-10-16T12:00:00Z");
        let output = serde_json::to_value(comment).unwrap();
        assert!(output.get("displayDate").is_none());
    }
}
