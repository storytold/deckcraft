//! Add-in tags (`p:custDataLst` → `ppt/tags/tagN.xml`) on the presentation, slides and shapes
//! survive a save and reopen.

use std::io::{Cursor, Read};
use std::sync::Arc;

use deckcraft_engine::Session;

fn t(n: &str, v: &str) -> (String, String) {
    (n.to_string(), v.to_string())
}

#[test]
fn tags_survive_round_trip() {
    let mut s = Session::new();
    deckcraft_engine::sample::open_sample(&mut s).expect("sample");
    let mut p = s.doc().expect("doc").doc.as_ref().clone();
    p.tags = vec![t("THINKCELLPRESENTATIONDONOTDELETE", "<xml & stuff>")];
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.tags = vec![t("TIMING", "5"), t("SECTION", "Intro")];
    let shape_id = slide.shapes[0].id;
    slide.shapes[0].tags = vec![t("THINKCELLSHAPEDONOTDELETE", "t!abc")];

    let bytes = deckcraft_pptx::export(&p).expect("export");
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.clone())).expect("zip");
    let mut ct = String::new();
    zip.by_name("[Content_Types].xml").expect("ct").read_to_string(&mut ct).expect("read");
    assert!(ct.contains("presentationml.tags+xml"));
    assert_eq!((0..zip.len()).filter(|i| zip.by_index(*i).is_ok_and(|f| f.name().unwrap().starts_with("ppt/tags/"))).count(), 3);

    let back = deckcraft_pptx::import(&bytes).expect("import");
    assert_eq!(back.tags, p.tags);
    assert_eq!(back.slides[0].tags, p.slides[0].tags);
    let first = &back.slides[0].shapes[0];
    assert_eq!(first.name, p.slides[0].shapes.iter().find(|s| s.id == shape_id).map(|s| s.name.clone()).unwrap_or_default());
    assert_eq!(first.tags, vec![t("THINKCELLSHAPEDONOTDELETE", "t!abc")]);
    assert!(back.slides.iter().skip(1).all(|s| s.tags.is_empty()));
}
