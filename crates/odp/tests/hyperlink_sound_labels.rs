//! Per-use sound labels must not silently disappear through the generic ODF hyperlink path.
#![allow(clippy::unwrap_used)]
use deckcraft_model::{
    Shape, ShapeId, Xfrm, defaults,
    text::{Action, Hyperlink},
};
use std::sync::Arc;
#[test]
fn unsupported_standalone_sound_label_has_an_explicit_odp_export_error() {
    let mut p = defaults::new_presentation(None);
    Arc::make_mut(&mut p.slides[0]).shapes.push(Shape {
        id: ShapeId(9001),
        name: "Original action owner".into(),
        xfrm: Some(Xfrm::new(10.0, 10.0, 100.0, 50.0)),
        click: Some(Hyperlink {
            action: Action::Url { url: "https://example.org/own".into() },
            tooltip: String::new(),
            highlight_click: false,
            sound: None,
            stop_sound: false,
            zoom_return: None,
            sound_name: Some("Unrepresented per-use label".into()),
        }),
        ..Default::default()
    });
    let error = deckcraft_odp::export(&p).unwrap_err().to_string();
    assert!(error.contains("sound"), "{error}");
}
