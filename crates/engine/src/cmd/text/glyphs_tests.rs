//! `font.used {detail: true}`, the answer of `format.font` and `text.glyphs`: which face draws a
//! deck's text. The decks are built with commands. The families are the default theme's body
//! font (Inter, bundled with every build that carries craft-fonts) and a family no machine has,
//! which is drawn in a stand-in.

use deckcraft_fonts::FontDb;
use serde_json::{Value, json};

use crate::Session;

/// A family no machine has installed.
const NOWHERE: &str = "Qqq Nonexistent Sans";

/// A new presentation with a blank slide added and selected, and that slide's index.
fn deck() -> (Session, usize) {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let slide = s.doc().unwrap().selection.slide;
    (s, slide)
}

/// Adds a text box holding `text` at `y` on the current slide and leaves it selected; its id.
fn text_box(s: &mut Session, y: f64, text: &str) -> Value {
    let id = s.execute("insert.textBox", &json!({"rect": [40.0, y, 600.0, 60.0], "text": text})).unwrap()["id"].clone();
    s.execute("text.exit", &json!({})).unwrap();
    id
}

/// The default theme's body font.
fn body_font(s: &Session) -> String {
    s.doc().unwrap().doc.masters[0].theme.fonts.minor.latin.clone()
}

/// The strings of an answer's list `key`.
fn strings(v: &Value, key: &str) -> Vec<String> {
    v[key].as_array().unwrap().iter().filter_map(Value::as_str).map(String::from).collect()
}

/// `font.used {detail: true}` answers, per family and in the order of the plain list, whether this
/// machine has it, the faces that draw it, and its runs, sizes and slides. A family no machine has
/// is drawn in a stand-in and never named as its own face.
#[test]
fn font_used_detail_tells_installed_from_missing_families() {
    let (mut s, slide) = deck();
    text_box(&mut s, 40.0, "Installed");
    s.execute("format.size", &json!({"size": 20})).unwrap();
    text_box(&mut s, 140.0, "Missing");
    s.execute("format.font", &json!({"family": NOWHERE})).unwrap();
    s.execute("format.size", &json!({"size": 30})).unwrap();
    let theme = body_font(&s);
    let mut want = vec![theme.clone(), NOWHERE.to_string()];
    want.sort_by_key(|f| f.to_lowercase());
    assert_eq!(s.execute("font.used", &json!({})).unwrap(), json!(want), "the plain answer stays the sorted names");
    let detail = s.execute("font.used", &json!({"detail": true})).unwrap();
    let entries = detail.as_array().unwrap();
    let families: Vec<&str> = entries.iter().map(|e| e["family"].as_str().unwrap()).collect();
    assert_eq!(families, want);
    let entry = |family: &str| entries.iter().find(|e| e["family"] == family).unwrap().clone();
    let db = FontDb::global();

    let missing = entry(NOWHERE);
    assert_eq!(missing["installed"], false, "{missing}");
    let stand_in = db.document_face(NOWHERE, "Regular").family.clone();
    assert_eq!(missing["drawnWith"][0], stand_in.as_str(), "{missing}");
    assert!(!strings(&missing, "drawnWith").contains(&NOWHERE.to_string()), "{missing}");
    assert_eq!(missing["runs"], 1, "{missing}");
    assert_eq!(missing["bullets"], 0, "{missing}");
    assert_eq!(missing["sizes"], json!([30.0]), "{missing}");
    assert_eq!(missing["slides"], json!([slide]), "{missing}");

    let installed = entry(theme.as_str());
    assert!(db.has_family(&theme) || option_env!("CRAFT_FONTS_REQUIRED").is_none(), "a required build bundles {theme}");
    assert_eq!(installed["installed"], db.has_family(&theme), "{installed}");
    assert_eq!(installed["drawnWith"][0], db.document_face(&theme, "Regular").family.as_str(), "{installed}");
    assert_eq!(installed["runs"], 1, "{installed}");
    assert_eq!(installed["sizes"], json!([20.0]), "{installed}");
    assert_eq!(installed["slides"], json!([slide]), "{installed}");
}

/// `format.font` answers the family the runs now ask for, whether this machine has it and the
/// faces that draw the text it was set on, on every call: for a family no machine has, for the
/// theme's own font, and for a theme reference, which answers with the family it names.
#[test]
fn format_font_answers_what_the_family_is_drawn_in() {
    let (mut s, _) = deck();
    text_box(&mut s, 40.0, "Words");
    let db = FontDb::global();

    let missing = s.execute("format.font", &json!({"family": NOWHERE})).unwrap();
    assert_eq!(missing["family"], NOWHERE, "{missing}");
    assert_eq!(missing["installed"], false, "{missing}");
    assert_eq!(missing["drawnWith"], json!([db.document_face(NOWHERE, "Regular").family]), "{missing}");
    assert!(!strings(&missing, "drawnWith").contains(&NOWHERE.to_string()), "{missing}");

    let theme = body_font(&s);
    let installed = s.execute("format.font", &json!({"family": theme})).unwrap();
    assert!(db.has_family(&theme) || option_env!("CRAFT_FONTS_REQUIRED").is_none(), "a required build bundles {theme}");
    assert_eq!(installed, json!({"family": theme, "installed": db.has_family(&theme), "drawnWith": [db.document_face(&theme, "Regular").family]}));

    let by_ref = s.execute("format.font", &json!({"family": "+mn-lt"})).unwrap();
    assert_eq!(by_ref, installed);
    assert_eq!(s.execute("font.used", &json!({})).unwrap(), json!([theme]), "the run is set to the theme's font");
}

/// `format.fontDialog` answers as `format.font` does when it sets the family, with other fields
/// beside it or with a theme reference; without a family it answers nothing, as before.
#[test]
fn font_dialog_setting_a_family_answers_as_format_font_does() {
    let (mut s, _) = deck();
    text_box(&mut s, 40.0, "Words");

    let dialog = s.execute("format.fontDialog", &json!({"family": NOWHERE, "size": 24})).unwrap();
    let font = s.execute("format.font", &json!({"family": NOWHERE})).unwrap();
    assert_eq!(dialog, font);
    assert_eq!(dialog["family"], NOWHERE, "{dialog}");
    assert_eq!(dialog["installed"], false, "{dialog}");

    let by_ref = s.execute("format.fontDialog", &json!({"family": "+mn-lt", "size": 20})).unwrap();
    assert_eq!(by_ref, s.execute("format.font", &json!({"family": "+mn-lt"})).unwrap());
    assert_eq!(by_ref["family"], body_font(&s), "{by_ref}");

    assert_eq!(s.execute("format.fontDialog", &json!({"size": 28})).unwrap(), Value::Null, "no family set, no answer");
}

/// `format.font` answers the faces the slide draws the text in, as `text.glyphs` reports them:
/// kana, which the stand-in has no glyph for, is drawn in another face; with a range selected only
/// the runs in it count; a shape without text answers the face the family is drawn in.
#[test]
fn format_font_answers_the_faces_drawing_the_text() {
    let (mut s, slide) = deck();
    let db = FontDb::global();
    let stand_in = db.document_face(NOWHERE, "Regular");
    let kana_face = db.fallback_for('\u{3042}', u32::MAX).map(|f| f.family.clone());
    assert!(kana_face.is_some() || option_env!("CRAFT_FONTS_REQUIRED").is_none(), "a required build bundles a kana face");
    // The face kana is drawn in, when the stand-in has no glyph for it.
    let kana = kana_face.filter(|f| !stand_in.covers('\u{3042}') && !f.eq_ignore_ascii_case(&stand_in.family));
    let drawn = |s: &mut Session, id: &Value, run: u64| {
        let out = s.execute("text.glyphs", &json!({"slide": slide, "ids": [id]})).unwrap();
        out.as_array().unwrap().iter().find(|r| r["run"] == run).unwrap()["drawnWith"].clone()
    };

    // The whole shape: four Latin characters, drawn in the stand-in, and one kana.
    let id = text_box(&mut s, 40.0, "Kana \u{3042}");
    let whole = s.execute("format.font", &json!({"family": NOWHERE})).unwrap();
    assert_eq!(whole["drawnWith"], drawn(&mut s, &id, 0), "{whole}");
    assert_eq!(strings(&whole, "drawnWith").first(), Some(&stand_in.family), "{whole}");
    if let Some(k) = &kana {
        assert_eq!(strings(&whole, "drawnWith"), [stand_in.family.clone(), k.clone()], "{whole}");
    }

    // A range: only the selected kana is set in the family, so only its faces count.
    let part = text_box(&mut s, 140.0, "Latin \u{3042}\u{3044}");
    s.execute("text.edit", &json!({"id": part})).unwrap();
    s.execute("text.select", &json!({"anchor": [0, 6], "caret": [0, 8]})).unwrap();
    let ranged = s.execute("format.font", &json!({"family": NOWHERE})).unwrap();
    s.execute("text.exit", &json!({})).unwrap();
    assert_eq!(ranged["drawnWith"], drawn(&mut s, &part, 1), "{ranged}");
    if let Some(k) = &kana {
        assert_eq!(strings(&ranged, "drawnWith"), std::slice::from_ref(k), "{ranged}");
    }

    // No text drawn: the face a run in the family is drawn in.
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 240.0, 100.0, 60.0]})).unwrap();
    let empty = s.execute("format.font", &json!({"family": NOWHERE})).unwrap();
    assert_eq!(empty["drawnWith"], json!([stand_in.family]), "{empty}");

    // A caret between words sets no character, so the same holds there, though the run around the
    // caret is already in the family and draws its kana in another face.
    let theme = body_font(&s);
    let around = text_box(&mut s, 340.0, "Kana \u{3042}");
    s.execute("text.edit", &json!({"id": around})).unwrap();
    s.execute("text.select", &json!({"anchor": [0, 4], "caret": [0, 4]})).unwrap();
    let caret = s.execute("format.font", &json!({"family": theme})).unwrap();
    s.execute("text.exit", &json!({})).unwrap();
    assert_eq!(caret["drawnWith"], json!([db.document_face(&theme, "Regular").family]), "{caret}");
}

/// `text.glyphs` lists each run with the families it asks for and the faces drawing it, the
/// characters drawn in a fallback face (kana, which the body font lacks) and the characters no
/// face has (U+10FFFD, a private-use code point, drawn as an empty box). Spaces are left out.
#[test]
fn text_glyphs_reports_fallback_and_missing_characters() {
    let (mut s, slide) = deck();
    let text = "Kana \u{3042} and \u{10FFFD}";
    let id = text_box(&mut s, 40.0, text);
    let other = text_box(&mut s, 140.0, "Elsewhere");
    let theme = body_font(&s);
    let out = s.execute("text.glyphs", &json!({"slide": slide, "ids": [id]})).unwrap();
    let runs = out.as_array().unwrap();
    assert_eq!(runs.len(), 1, "{out}");
    let r = &runs[0];
    assert_eq!(r["slide"], json!(slide), "{r}");
    assert_eq!(r["shape"], id, "{r}");
    assert_eq!((r["para"].as_u64(), r["run"].as_u64()), (Some(0), Some(0)), "{r}");
    assert_eq!(r["text"], text, "{r}");
    assert_eq!(r["requested"], json!([theme]), "{r}");

    let db = FontDb::global();
    let body = db.document_face(&theme, "Regular").family.clone();
    let drawn = strings(r, "drawnWith");
    assert_eq!(drawn.first(), Some(&body), "the body font draws most of the run: {r}");
    match db.fallback_for('\u{3042}', u32::MAX) {
        Some(f) if !f.family.eq_ignore_ascii_case(&body) => {
            assert_eq!(strings(r, "fallbackChars"), ["\u{3042}"], "{r}");
            assert!(drawn.contains(&f.family), "{r}");
        }
        _ => assert!(option_env!("CRAFT_FONTS_REQUIRED").is_none(), "a required build bundles a kana face"),
    }
    if db.fallback_for('\u{10FFFD}', u32::MAX).is_none() {
        // A build without a kana face (allowed while CRAFT_FONTS_REQUIRED is unset) draws the kana
        // as an empty box too.
        let want: &[&str] = if db.fallback_for('\u{3042}', u32::MAX).is_none() { &["\u{3042}", "\u{10FFFD}"] } else { &["\u{10FFFD}"] };
        assert_eq!(strings(r, "missingChars"), want, "{r}");
    }

    let all = s.execute("text.glyphs", &json!({"slide": slide})).unwrap();
    let shapes: Vec<Value> = all.as_array().unwrap().iter().map(|e| e["shape"].clone()).collect();
    assert_eq!(shapes, [id, other], "every run on the slide, in drawing order");
}

/// Table text is listed per cell, with the cell's row and column, and `font.used {detail: true}`
/// counts the cells' runs.
#[test]
fn text_glyphs_lists_table_cells() {
    let (mut s, slide) = deck();
    let table = json!({"rows": 1, "cols": 2, "rect": [40.0, 40.0, 400.0, 40.0], "data": [["Left", "Right"]]});
    let id = s.execute("insert.table", &table).unwrap()["id"].clone();
    let out = s.execute("text.glyphs", &json!({"slide": slide, "ids": [id]})).unwrap();
    let cells: Vec<(Value, Value)> = out.as_array().unwrap().iter().map(|e| (e["cell"].clone(), e["text"].clone())).collect();
    assert_eq!(cells, [(json!([0, 0]), json!("Left")), (json!([0, 1]), json!("Right"))], "{out}");
    let detail = s.execute("font.used", &json!({"detail": true})).unwrap();
    let runs: u64 = detail.as_array().unwrap().iter().filter_map(|e| e["runs"].as_u64()).sum();
    assert_eq!(runs, 2, "{detail}");
}

/// Bad parameters are refused with an error that names the parameter; null means absent.
#[test]
fn font_and_glyph_queries_refuse_bad_params() {
    let (mut s, slide) = deck();
    text_box(&mut s, 40.0, "Text");
    let err = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_err().to_string();
    assert_eq!(err(&mut s, "font.used", json!({"detail": "yes"})), "invalid parameters for `font.used`: `detail` is not true or false: \"yes\"");
    let glyphs = "invalid parameters for `text.glyphs`";
    assert_eq!(err(&mut s, "text.glyphs", json!({"slide": -1})), format!("{glyphs}: `slide` is not a valid index: -1"));
    assert_eq!(
        err(&mut s, "text.glyphs", json!({"slide": 99})),
        format!("{glyphs}: `slide` is past the last slide ({} in the presentation): 99", slide + 1)
    );
    assert_eq!(err(&mut s, "text.glyphs", json!({"ids": "x"})), format!("{glyphs}: `ids` is not a list of shape ids: \"x\""));
    assert_eq!(err(&mut s, "text.glyphs", json!({"ids": [-3]})), format!("{glyphs}: `ids` has an invalid shape id: -3"));
    assert_eq!(
        err(&mut s, "text.glyphs", json!({"slide": slide, "ids": [999999]})),
        format!("{glyphs}: `ids` names a shape that is not on slide {slide}: 999999")
    );
    assert_eq!(err(&mut s, "text.glyphs", json!({"ids": [999999]})), format!("{glyphs}: `ids` names a shape that is on no slide: 999999"));
    assert_eq!(s.execute("text.glyphs", &json!({"slide": null, "ids": null})).unwrap().as_array().map(Vec::len), Some(1));
    assert_eq!(s.execute("font.used", &json!({"detail": false})).unwrap(), s.execute("font.used", &json!({})).unwrap());
    assert_eq!(s.execute("font.used", &json!({"detail": null})).unwrap(), s.execute("font.used", &json!({})).unwrap());
}
