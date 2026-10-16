//! Contributor-original compatibility coverage for optional per-use audio labels.
#![allow(clippy::unwrap_used)]
use deckcraft_model::text::{Action, Hyperlink};
use serde_json::json;
#[test]
fn old_object_and_six_field_positional_hyperlinks_keep_absent_sound_labels() {
    let old = json!({"action":{"kind":"url","url":"https://example.org/original"},"tooltip":"tip","highlightClick":true,"sound":17,"stopSound":false,"zoomReturn":19});
    let object: Hyperlink = serde_json::from_value(old).unwrap();
    let positional: Hyperlink = serde_json::from_value(json!([{"kind":"url","url":"https://example.org/original"},"tip",true,17,false,19])).unwrap();
    assert_eq!(object, positional);
    assert_eq!(object.sound_name, None);
    assert!(!serde_json::to_value(object).unwrap().as_object().unwrap().contains_key("soundName"));
}
#[test]
fn explicit_empty_and_unicode_per_use_labels_are_exact_and_not_resource_names() {
    for label in ["", "Écho partagé · 音🦀\tline\nnext\rend"] {
        let value = json!({"action":{"kind":"nextSlide"},"sound":17,"soundName":label});
        let link: Hyperlink = serde_json::from_value(value).unwrap();
        assert_eq!(link.action, Action::NextSlide);
        assert_eq!(link.sound_name.as_deref(), Some(label));
        assert_eq!(serde_json::from_slice::<Hyperlink>(&serde_json::to_vec(&link).unwrap()).unwrap(), link);
        assert_eq!(serde_json::to_value(link).unwrap()["soundName"], label);
    }
}
#[test]
fn seventh_positional_field_is_the_optional_sound_label() {
    let link: Hyperlink = serde_json::from_value(json!([{"kind":"nextSlide"},"",false,17,false,null,"label"])).unwrap();
    assert_eq!(link.sound.unwrap().0, 17);
    assert_eq!(link.zoom_return, None);
    assert_eq!(link.sound_name.as_deref(), Some("label"));
}
