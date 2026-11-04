//! Camera Format ▸ Accessibility: a Cameo's Alt Text round trips.
//!
//! The description lives in `p:cNvPr@descr` like any other object's, beside the `alf:liveFeedProps`
//! extension that makes PowerPoint read the picture as a Cameo; "Mark as decorative" goes to
//! `adec:decorative`.

use std::io::Read;

use deckcraft_engine::Session;
use serde_json::json;

fn deck() -> (Session, u64) {
    let mut s = Session::with_new();
    s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("insert.cameo", &json!({})).unwrap()["id"].as_u64().unwrap();
    (s, id)
}

fn slide_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut x = String::new();
    zip.by_name("ppt/slides/slide1.xml").unwrap().read_to_string(&mut x).unwrap();
    x
}

#[test]
fn a_cameos_alt_text_is_written_to_cnvpr_descr_and_read_back() {
    let (mut s, id) = deck();
    s.execute("shape.altText", &json!({"text": "A webcam view of the speaker", "ids": [id]})).unwrap();
    let bytes = deckcraft_pptx::export(&s.active().unwrap().doc).unwrap();
    let xml = slide_xml(&bytes);
    assert!(xml.contains(r#"descr="A webcam view of the speaker""#), "the description is the picture's descr:\n{xml}");
    assert!(xml.contains("liveFeedProps"), "the picture keeps its Cameo extension:\n{xml}");

    let back = deckcraft_pptx::import(&bytes).unwrap();
    let sh = back.slides[0].shapes.iter().find(|x| x.id.0 as u64 == id).unwrap();
    assert!(sh.is_cameo(), "still a Cameo after a save and reopen");
    assert_eq!(sh.descr, "A webcam view of the speaker");
    assert!(!sh.decorative);
}

#[test]
fn decorative_round_trips_alongside_the_extension() {
    let (mut s, id) = deck();
    s.execute("shape.altText", &json!({"text": "", "decorative": true, "ids": [id]})).unwrap();
    let bytes = deckcraft_pptx::export(&s.active().unwrap().doc).unwrap();
    let xml = slide_xml(&bytes);
    assert!(xml.contains("decorative"), "the decorative flag is written:\n{xml}");
    let back = deckcraft_pptx::import(&bytes).unwrap();
    let sh = back.slides[0].shapes.iter().find(|x| x.id.0 as u64 == id).unwrap();
    assert!(sh.is_cameo());
    assert!(sh.decorative);
    assert!(sh.descr.is_empty());
}
