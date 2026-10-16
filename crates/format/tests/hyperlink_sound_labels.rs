//! Contributor-original native archives exercise pre-soundName object and positional records.
#![allow(clippy::unwrap_used)]
use deckcraft_model::{Shape, ShapeId, defaults};
use serde_json::json;
use std::{
    io::{Cursor, Write},
    sync::Arc,
};
#[test]
fn native_old_hyperlink_objects_and_six_field_sequences_remain_readable() {
    for link in [
        json!({"action":{"kind":"url","url":"https://example.org/own"},"tooltip":"original","highlightClick":false,"sound":null,"stopSound":false,"zoomReturn":null}),
        json!([{"kind":"url","url":"https://example.org/own"},"original",false,null,false,null]),
    ] {
        let mut p = defaults::new_presentation(None);
        Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape { id: ShapeId(9001), name: "Original hyperlink owner".into(), ..Default::default() }];
        let mut value = serde_json::to_value(&p).unwrap();
        value["slides"][0]["shapes"][0]["click"] = link;
        let manifest = json!({"format":"deckcraft","version":1,"generator":"contributor-original compatibility test","presentation":value});
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("presentation.json", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&serde_json::to_vec(&manifest).unwrap()).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let actual = deckcraft_format::load(&bytes).unwrap();
        let click = actual.slides[0].shapes[0].click.as_ref().unwrap();
        assert_eq!(click.sound_name, None);
        assert_eq!(click.tooltip, "original");
        let reopened = deckcraft_format::load(&deckcraft_format::save(&actual).unwrap()).unwrap();
        assert_eq!(reopened.slides[0].shapes[0].click, actual.slides[0].shapes[0].click);
    }
}
