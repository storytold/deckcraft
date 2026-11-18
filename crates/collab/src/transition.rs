//! The wire form of a slide transition: `deckcraft_model::anim::Transition` ↔ `TransitionSpec`.
//!
//! `collab-proto` carries no model types, so the whole transition travels as its own struct. The
//! two types have the same fields, so the mapping goes through JSON: one definition of the
//! field names (serde's `camelCase`) and one place to look when the model grows a field.

use deckcraft_collab_proto::TransitionSpec;
use deckcraft_model::anim::Transition;

/// The wire value of a model transition.
pub fn spec(value: &Transition) -> Option<TransitionSpec> {
    serde_json::from_value(serde_json::to_value(value).ok()?).ok()
}

/// The model value of a wire transition.
pub fn value(spec: &TransitionSpec) -> Option<Transition> {
    serde_json::from_value(serde_json::to_value(spec).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transition_survives_the_round_trip_through_the_wire_type() {
        let at = Transition {
            kind: "wipe".into(),
            option: "ru".into(),
            duration_ms: 1250,
            advance_on_click: false,
            advance_after_ms: Some(10_000),
            sound: Some(deckcraft_model::MediaId(4)),
            sound_loop: true,
            stop_sound: true,
            raw: Some("<p14:kept/>".into()),
        };
        let wire = spec(&at).expect("wire value");
        assert_eq!(value(&wire), Some(at), "the exact model value survives");
    }
}
