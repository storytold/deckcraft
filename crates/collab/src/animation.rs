//! The wire form of one shape's animation steps: `deckcraft_model::anim::Animation` ↔ JSON.
//!
//! `collab-proto` carries no model types, and the model animation has six enums, a colour and two
//! ids, so the wire value is the model's own JSON (the `ShapeRestoreItem` pattern) and the shape
//! id comes from `Target.shape`, not from the value. This module is the one place that converts:
//! the normaliser turns the built animations into payload values, the adapter turns the payload
//! values back with the shape the keys resolved.

use deckcraft_model::ShapeId;
use deckcraft_model::anim::Animation;

/// The wire value of one model animation.
pub fn spec(value: &Animation) -> Option<serde_json::Value> {
    serde_json::to_value(value).ok()
}

/// The model animation of one wire value, addressed to `shape`. The addressed shape wins over the
/// id inside the value: a payload may not animate a shape it does not name.
pub fn value(spec: &serde_json::Value, shape: ShapeId) -> Option<Animation> {
    let mut animation: Animation = serde_json::from_value(spec.clone()).ok()?;
    animation.shape = shape;
    Some(animation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::anim::{AnimClass, AnimStart, TextBuild};

    #[test]
    fn an_animation_survives_the_round_trip_through_the_wire_value() {
        let at = Animation {
            shape: ShapeId(3),
            class: AnimClass::Emphasis,
            effect: "pulse".into(),
            option: String::new(),
            start: AnimStart::AfterPrevious,
            duration_ms: 750,
            delay_ms: 250,
            repeat: 3,
            text_build: TextBuild::ByParagraph,
            path: Some("M 0 0 L 0.25 0 E".into()),
            preset_id: Some(26),
            smooth_start: 0.5,
            ..Animation::default()
        };
        let wire = spec(&at).expect("wire value");
        assert_eq!(value(&wire, ShapeId(3)), Some(at.clone()), "the exact model value survives");
        assert_eq!(value(&wire, ShapeId(9)).map(|a| a.shape), Some(ShapeId(9)), "the addressed shape wins");
    }
}
