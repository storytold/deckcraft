use serde_json::json;

use super::*;
use crate::Session;
use crate::cmd::{base64_decode, base64_encode};

const LEGACY: &[u8] = include_bytes!("../../../fonts/tests/fixtures/legacy-regular.ttf");

fn with_fs_type(font: &[u8], fs: u16) -> Vec<u8> {
    let n = u16::from_be_bytes([font[4], font[5]]) as usize;
    let rec = (0..n).map(|i| 12 + i * 16).find(|r| &font[*r..*r + 4] == b"OS/2").unwrap();
    let at = u32::from_be_bytes(font[rec + 8..rec + 12].try_into().unwrap()) as usize + 8;
    let mut out = font.to_vec();
    out[at..at + 2].copy_from_slice(&fs.to_be_bytes());
    out
}

fn deck(text: &str, font: &str, ea: Option<&str>) -> Presentation {
    let mut p = Presentation::default();
    let slide = Arc::make_mut(&mut p.slides[0]);
    let mut body = TextBody::from_text(text);
    body.paragraphs[0].runs[0].props.font = Some(font.into());
    body.paragraphs[0].runs[0].props.font_ea = ea.map(String::from);
    slide.shapes[0].text = Some(body);
    p
}

fn session_with(p: Presentation) -> Session {
    let mut s = Session::with_new();
    s.edit(|doc, _| {
        *doc = p;
        Ok(())
    })
    .unwrap();
    s
}

fn save_pptx(s: &mut Session) -> (Vec<u8>, serde_json::Value) {
    let r = s.execute("file.saveBytes", &json!({"format": "pptx"})).unwrap();
    (base64_decode(r["data"].as_str().unwrap()).unwrap(), r)
}

fn font_parts(p: &Presentation) -> usize {
    p.media.iter().filter(|m| m.content_type == "application/x-fontdata").count()
}

fn ubuntu() -> Vec<u8> {
    let f = FontDb::global().face("Ubuntu Light", "Regular");
    assert_eq!(f.legacy.as_ref().map(|l| l.0.as_str()), Some("Ubuntu Light"));
    f.data().to_vec()
}

#[test]
fn saving_with_all_characters_embeds_the_font_file() {
    let mut s = session_with(deck("Hello", "Ubuntu Light", None));
    let r = s.execute("file.embedFonts", &json!({"on": true, "characters": "all"})).unwrap();
    assert_eq!(r, json!({"on": true, "characters": "all"}));
    let (bytes, r) = save_pptx(&mut s);
    assert_eq!(r["fonts"]["embedded"], json!(["Ubuntu Light"]), "{r}");
    assert!(s.doc().unwrap().doc.embedded_fonts.is_empty(), "the open presentation itself is left as it was");
    let q = deckcraft_pptx::import(&bytes).unwrap();
    let [f] = q.embedded_fonts.as_slice() else { panic!("{:?}", q.embedded_fonts) };
    assert_eq!(f.typeface, "Ubuntu Light");
    assert!(f.bold.is_none() && f.italic.is_none() && f.bold_italic.is_none(), "Ubuntu Light has no other faces");
    let part = q.media(f.regular.unwrap()).unwrap();
    assert_eq!(deckcraft_fonts::embedded_sfnt(&part.data), Some(ubuntu().as_slice()));
    assert!(f.panose.as_deref().is_some_and(|p| p.len() == 20));
}

#[test]
fn saving_with_only_the_characters_used_embeds_a_subset() {
    let mut s = session_with(deck("Hello", "Ubuntu Light", None));
    s.execute("file.embedFonts", &json!({"on": true})).unwrap();
    assert_eq!(s.execute("file.embedFonts", &json!({})).unwrap(), json!({"on": true, "characters": "used"}));
    let (bytes, _) = save_pptx(&mut s);
    let q = deckcraft_pptx::import(&bytes).unwrap();
    let part = q.media(q.embedded_fonts[0].regular.unwrap()).unwrap();
    assert_eq!(u32::from_le_bytes(part.data[12..16].try_into().unwrap()), 1, "TTEMBED_SUBSET");
    let sub = deckcraft_fonts::embedded_sfnt(&part.data).unwrap();
    assert!(sub.len() * 2 < ubuntu().len());
}

#[test]
fn a_font_whose_licence_forbids_embedding_is_left_out() {
    let db = FontDb::with_font_dirs(Vec::new());
    assert!(db.add_font(with_fs_type(LEGACY, 0x0002)) > 0);
    let mut p = deck("A", "DeckCraft Legacy", None);
    let r = embed_used_fonts(&mut p, &db);
    assert_eq!(r.skipped, [("DeckCraft Legacy".to_string(), SkipReason::Restricted)]);
    assert!(r.embedded.is_empty() && p.embedded_fonts.is_empty() && font_parts(&p) == 0);
    let db = FontDb::with_font_dirs(Vec::new());
    assert!(db.add_font(with_fs_type(LEGACY, 0x0004)) > 0);
    let r = embed_used_fonts(&mut p, &db);
    assert_eq!(r.embedded, ["DeckCraft Legacy"], "preview & print fonts may be embedded");
    assert_eq!(p.embedded_fonts.len(), 1);
}

#[test]
fn a_font_that_is_not_installed_is_reported() {
    let db = FontDb::with_font_dirs(Vec::new());
    let mut p = deck("A", "No Such Family Anywhere", None);
    let r = embed_used_fonts(&mut p, &db);
    assert!(r.skipped.contains(&("No Such Family Anywhere".to_string(), SkipReason::NotInstalled)), "{r:?}");
    assert!(p.embedded_fonts.iter().all(|e| e.typeface != "No Such Family Anywhere"));
}

#[test]
fn saving_again_does_not_duplicate_embedded_fonts() {
    let mut s = session_with(deck("Hello", "Ubuntu Light", None));
    s.execute("file.embedFonts", &json!({"on": true})).unwrap();
    let (first, _) = save_pptx(&mut s);
    let mut s = Session::with_new();
    s.execute("file.openBytes", &json!({"name": "a.pptx", "data": base64_encode(&first)})).unwrap();
    s.execute("file.embedFonts", &json!({"on": true})).unwrap();
    assert_eq!(s.doc().unwrap().doc.embedded_fonts.len(), 1);
    let (second, _) = save_pptx(&mut s);
    let q = deckcraft_pptx::import(&second).unwrap();
    assert_eq!(q.embedded_fonts.len(), 1);
    assert_eq!(font_parts(&q), 1);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(second)).unwrap();
    assert_eq!(zip.file_names().filter(|n| n.starts_with("ppt/fonts/")).count(), 1);
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("ppt/presentation.xml").unwrap(), &mut xml).unwrap();
    assert_eq!(xml.matches("<p:embeddedFont>").count(), 1);
}

#[test]
fn a_typeface_known_only_from_the_file_keeps_its_embedded_copy() {
    let db = FontDb::with_font_dirs(Vec::new());
    let mut p = deck("A", "DeckCraft Legacy", None);
    let original = deckcraft_fonts::embed::eot(LEGACY, false).unwrap();
    let id = p.add_media("font1.fntdata", "application/x-fontdata", original.clone()).unwrap();
    p.embedded_fonts.push(EmbeddedFont { typeface: "DeckCraft Legacy".into(), regular: Some(id), ..Default::default() });
    db.add_font(deckcraft_fonts::embedded_sfnt(&original).unwrap().to_vec());
    let r = embed_used_fonts(&mut p, &db);
    assert_eq!(r.embedded, ["DeckCraft Legacy"]);
    assert_eq!(p.embedded_fonts.len(), 1);
    assert_eq!(p.embedded_fonts[0].regular, Some(id));
    assert_eq!(font_parts(&p), 1);
}

#[test]
fn turning_embedding_off_removes_the_fonts_and_undoes() {
    let mut s = session_with(deck("Hello", "Ubuntu Light", None));
    s.execute("file.embedFonts", &json!({"on": true})).unwrap();
    let (bytes, _) = save_pptx(&mut s);
    let mut s = Session::with_new();
    s.execute("file.openBytes", &json!({"name": "a.pptx", "data": base64_encode(&bytes)})).unwrap();
    assert_eq!(s.execute("file.embedFonts", &json!({"on": false})).unwrap()["on"], json!(false));
    assert!(s.doc().unwrap().doc.embedded_fonts.is_empty());
    let (bytes, r) = save_pptx(&mut s);
    assert!(r.get("fonts").is_none());
    let q = deckcraft_pptx::import(&bytes).unwrap();
    assert!(q.embedded_fonts.is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.embedded_fonts.len(), 1);
    assert!(s.execute("file.embedFonts", &json!({"characters": "some"})).is_err());
    assert!(s.execute("file.embedFonts", &json!({"on": "yes"})).is_err());
}

#[test]
fn east_asian_text_counts_toward_the_east_asian_typeface() {
    let db = FontDb::with_font_dirs(Vec::new());
    let p = deck("Hi 안녕", "No Such Latin Face", Some("Ubuntu Light"));
    let u = font_usage(&p, &db);
    let ea = u.iter().find(|u| u.name == "Ubuntu Light").expect("east Asian typeface");
    assert_eq!(ea.text.concat(), "안녕");
    let latin = u.iter().find(|u| u.name == "No Such Latin Face").expect("latin typeface");
    assert_eq!(latin.text.concat(), "Hi ");
}

#[test]
fn saving_without_the_option_embeds_nothing() {
    let mut s = session_with(deck("Hello", "Ubuntu Light", None));
    let (bytes, r) = save_pptx(&mut s);
    assert!(r.get("fonts").is_none());
    assert!(deckcraft_pptx::import(&bytes).unwrap().embedded_fonts.is_empty());
}
