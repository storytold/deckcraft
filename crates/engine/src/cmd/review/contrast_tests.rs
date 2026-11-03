//! `review.contrast`: the contrast of each text run with what the slide draws behind it, on decks
//! built here with commands and pictures made from pixels.

use serde_json::{Value, json};

use crate::Session;

/// A session on a new blank slide with a plain white background.
fn white_slide() -> Session {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("design.background", &json!({"color": "#FFFFFF"})).unwrap();
    s
}

/// Adds a text box holding `text` in `color` at `size` points and leaves it selected.
fn text_box(s: &mut Session, rect: [f64; 4], text: &str, color: &str, size: f64) -> Value {
    let id = s.execute("insert.textBox", &json!({"rect": rect, "text": text})).unwrap()["id"].clone();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    s.execute("format.color", &json!({"color": color})).unwrap();
    s.execute("format.size", &json!({"size": size})).unwrap();
    id
}

/// Adds a rectangle holding `text`, filled with `fill` (`transparency` 0..1), its text in
/// `color` at `size` points, and leaves it selected.
fn filled_shape(s: &mut Session, rect: [f64; 4], text: &str, fill: &str, transparency: f64, color: &str, size: f64) -> Value {
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": rect, "text": text})).unwrap()["id"].clone();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    s.execute("shape.fill", &json!({"color": fill, "transparency": transparency})).unwrap();
    s.execute("format.color", &json!({"color": color})).unwrap();
    s.execute("format.size", &json!({"size": size})).unwrap();
    id
}

/// Adds a picture of one colour covering the whole slide.
fn dark_picture(s: &mut Session) {
    let img = image::RgbaImage::from_fn(64, 36, |_, _| image::Rgba([17, 25, 44, 255]));
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let data = crate::cmd::base64_encode(&png);
    s.execute("insert.picture", &json!({"data": data, "name": "dark.png", "rect": [0, 0, 960, 540]})).unwrap();
}

/// The entries `review.contrast` gives for shape `id`, with `more` parameters.
fn contrast_of(s: &mut Session, id: &Value, more: Value) -> Vec<Value> {
    let mut p = json!({"ids": [id]});
    if let (Some(map), Some(more)) = (p.as_object_mut(), more.as_object()) {
        map.extend(more.clone());
    }
    let v = s.execute("review.contrast", &p).unwrap();
    v.as_array().unwrap().clone()
}

/// The single entry for shape `id`.
fn only(s: &mut Session, id: &Value, more: Value) -> Value {
    let v = contrast_of(s, id, more);
    assert_eq!(v.len(), 1, "{v:?}");
    v[0].clone()
}

fn ratio(e: &Value, key: &str) -> f64 {
    e["ratio"][key].as_f64().unwrap()
}

#[test]
fn black_text_on_a_white_slide_measures_21_to_1() {
    let mut s = white_slide();
    let id = text_box(&mut s, [100.0, 100.0, 400.0, 60.0], "Plain text", "#000000", 12.0);
    let other = text_box(&mut s, [100.0, 300.0, 400.0, 60.0], "Another box", "#767676", 12.0);
    let slide = s.doc().unwrap().selection.slide as u64;
    let revision = s.doc().unwrap().revision;
    let e = only(&mut s, &id, json!({}));
    assert_eq!(e["slide"].as_u64(), Some(slide), "{e}");
    assert_eq!(e["shape"], id);
    assert_eq!((e["para"].as_u64(), e["run"].as_u64()), (Some(0), Some(0)), "{e}");
    assert_eq!(e["text"], "Plain text");
    assert_eq!(e["color"], "#000000");
    assert_eq!(e["sizePt"].as_f64(), Some(12.0), "{e}");
    assert_eq!(e["bold"], false);
    assert_eq!(e["required"].as_f64(), Some(4.5), "{e}");
    for key in ["min", "p5", "median"] {
        assert_eq!(ratio(&e, key), 21.0, "{key}: {e}");
    }
    assert_eq!(e["passShare"].as_f64(), Some(1.0), "{e}");
    assert_eq!(e["backdrop"], "solid");
    // Without `ids`, every text shape on the slide, in drawing order; `slide` picks the slide.
    let all = s.execute("review.contrast", &json!({})).unwrap();
    let shapes: Vec<&Value> = all.as_array().unwrap().iter().map(|e| &e["shape"]).collect();
    assert_eq!(shapes, [&id, &other], "{all}");
    assert_eq!(s.execute("review.contrast", &json!({"slide": slide})).unwrap(), all);
    // The first slide holds only empty placeholders.
    assert_eq!(s.execute("review.contrast", &json!({"slide": 0})).unwrap(), json!([]));
    // A query: the deck is unchanged and no undo step is added.
    assert_eq!(s.doc().unwrap().revision, revision);
}

#[test]
fn each_paragraph_gets_its_own_entry() {
    let mut s = white_slide();
    let id = text_box(&mut s, [100.0, 100.0, 500.0, 120.0], "First line\nSecond line", "#000000", 12.0);
    let v = contrast_of(&mut s, &id, json!({}));
    let found: Vec<(Option<u64>, Option<u64>, Option<&str>)> =
        v.iter().map(|e| (e["para"].as_u64(), e["run"].as_u64(), e["text"].as_str())).collect();
    assert_eq!(found, [(Some(0), Some(0), Some("First line")), (Some(1), Some(0), Some("Second line"))], "{v:?}");
}

#[test]
fn white_text_on_a_spring_green_card_measures_the_card() {
    let mut s = white_slide();
    let id = filled_shape(&mut s, [290.0, 185.0, 155.0, 190.0], "Card", "#00FF7F", 0.0, "#FFFFFF", 24.0);
    let e = only(&mut s, &id, json!({}));
    for key in ["min", "p5", "median"] {
        assert!((ratio(&e, key) - 1.35).abs() < 0.015, "{key}: {e}");
    }
    assert_eq!(e["color"], "#FFFFFF");
    assert_eq!(e["required"].as_f64(), Some(3.0), "{e}");
    assert_eq!(e["passShare"].as_f64(), Some(0.0), "{e}");
    assert_eq!(e["backdrop"], "shapeFill");
}

#[test]
fn black_text_on_a_dark_picture_is_measured_against_the_picture() {
    let mut s = white_slide();
    dark_picture(&mut s);
    let id = text_box(&mut s, [60.0, 40.0, 840.0, 80.0], "Quarterly results", "#000000", 40.0);
    let e = only(&mut s, &id, json!({}));
    assert!(ratio(&e, "median") < 1.5, "{e}");
    assert_eq!(e["required"].as_f64(), Some(3.0), "{e}");
    assert_eq!(e["passShare"].as_f64(), Some(0.0), "{e}");
    assert_eq!(e["backdrop"], "picture");
    // White text on the same picture passes.
    let light = text_box(&mut s, [60.0, 300.0, 840.0, 80.0], "Quarterly results", "#FFFFFF", 40.0);
    let e = only(&mut s, &light, json!({}));
    assert!(ratio(&e, "min") > 15.0, "{e}");
    assert_eq!(e["passShare"].as_f64(), Some(1.0), "{e}");
}

#[test]
fn white_text_on_a_see_through_band_over_a_picture_is_mixed() {
    let mut s = white_slide();
    dark_picture(&mut s);
    let id = filled_shape(&mut s, [0.0, 200.0, 960.0, 140.0], "Band title", "#FFFFFF", 0.25, "#FFFFFF", 40.0);
    let e = only(&mut s, &id, json!({}));
    assert!(ratio(&e, "median") < 3.0 && ratio(&e, "median") > 1.0, "{e}");
    assert_eq!(e["passShare"].as_f64(), Some(0.0), "{e}");
    assert_eq!(e["backdrop"], "mixed");
}

#[test]
fn white_text_on_a_gradient_background_spreads_across_it() {
    let mut s = white_slide();
    s.execute("design.background", &json!({"gradient": {"stops": [[0, "#1E1B4B"], [1, "#FFFFFF"]], "angle": 0}})).unwrap();
    let id = text_box(&mut s, [60.0, 200.0, 840.0, 100.0], "Gradient title across the slide", "#FFFFFF", 44.0);
    let e = only(&mut s, &id, json!({}));
    assert_eq!(e["backdrop"], "gradient");
    let (min, p5, median) = (ratio(&e, "min"), ratio(&e, "p5"), ratio(&e, "median"));
    assert!(min <= p5 && p5 <= median && min + 1.0 < median, "{e}");
    let pass = e["passShare"].as_f64().unwrap();
    assert!(pass > 0.0 && pass < 1.0, "{e}");
}

#[test]
fn required_contrast_follows_the_drawn_size_and_weight() {
    let mut s = white_slide();
    // #767676 on white is 4.54:1: enough for AA at any size and for AAA large text only.
    let cases = [
        (40.0, 12.0, false, 4.5, 7.0),
        (130.0, 18.0, false, 3.0, 4.5),
        (220.0, 14.0, true, 3.0, 4.5),
        (310.0, 14.0, false, 4.5, 7.0),
        (400.0, 17.5, false, 4.5, 7.0),
    ];
    for (y, size, bold, aa, aaa) in cases {
        let id = text_box(&mut s, [60.0, y, 600.0, 60.0], "Sample", "#767676", size);
        if bold {
            s.execute("format.bold", &json!({"on": true})).unwrap();
        }
        let e = only(&mut s, &id, json!({}));
        assert_eq!((e["sizePt"].as_f64(), e["bold"].as_bool()), (Some(size), Some(bold)), "{e}");
        assert_eq!(e["required"].as_f64(), Some(aa), "AA, {size} pt, bold {bold}: {e}");
        assert!((ratio(&e, "median") - 4.54).abs() < 0.011, "{e}");
        assert_eq!(e["passShare"].as_f64(), Some(1.0), "{e}");
        let e = only(&mut s, &id, json!({"level": "AAA"}));
        assert_eq!(e["required"].as_f64(), Some(aaa), "AAA, {size} pt, bold {bold}: {e}");
        assert_eq!(e["passShare"].as_f64(), Some(if aaa < 5.0 { 1.0 } else { 0.0 }), "{e}");
    }
}

#[test]
fn a_semibold_face_at_14_pt_needs_the_normal_text_ratio() {
    // A face counts as bold from weight 700, CSS's `bold`: 14 pt text drawn in a SemiBold face
    // (600) is normal text, and 14 pt text in a Bold face (700) is large text.
    let fonts = deckcraft_fonts::FontDb::global();
    let (semibold_face, bold_face) = (fonts.document_face("Inter SemiBold", "Regular"), fonts.document_face("Inter", "Bold"));
    let drawn = (semibold_face.family.as_str(), semibold_face.weight, bold_face.family.as_str(), bold_face.weight);
    // Required native builds embed the pinned Inter SemiBold and Bold faces (craft-fonts'
    // fonts/latin-manifest.txt); an optional local build without them skips this test.
    let pinned = drawn == ("Inter", 600.0, "Inter", 700.0);
    assert!(pinned || option_env!("CRAFT_FONTS_REQUIRED").is_none(), "required native build draws {drawn:?}");
    if !pinned {
        return;
    }
    let mut s = white_slide();
    // #888888 on white is 3.54:1: enough for large text at AA, not for normal text.
    let cases = [(40.0, "Inter SemiBold", false, 4.5, 7.0, 0.0), (130.0, "Inter", true, 3.0, 4.5, 1.0)];
    for (y, family, bold, aa, aaa, pass) in cases {
        let id = text_box(&mut s, [60.0, y, 600.0, 60.0], "Sample", "#888888", 14.0);
        s.execute("format.font", &json!({"family": family})).unwrap();
        if bold {
            s.execute("format.bold", &json!({"on": true})).unwrap();
        }
        let e = only(&mut s, &id, json!({}));
        assert_eq!(e["required"].as_f64(), Some(aa), "AA, 14 pt {family}, bold {bold}: {e}");
        assert_eq!((e["sizePt"].as_f64(), e["bold"].as_bool()), (Some(14.0), Some(bold)), "{family}: {e}");
        assert!((ratio(&e, "median") - 3.54).abs() < 0.011, "{e}");
        assert_eq!(e["passShare"].as_f64(), Some(pass), "{family}: {e}");
        let e = only(&mut s, &id, json!({"level": "AAA"}));
        assert_eq!(e["required"].as_f64(), Some(aaa), "AAA, 14 pt {family}, bold {bold}: {e}");
    }
}

#[test]
fn a_group_id_measures_its_members() {
    let mut s = white_slide();
    let a = text_box(&mut s, [100.0, 100.0, 300.0, 60.0], "First", "#000000", 12.0);
    let b = text_box(&mut s, [100.0, 300.0, 300.0, 60.0], "Second", "#767676", 12.0);
    let g = s.execute("arrange.group", &json!({"ids": [a, b]})).unwrap()["id"].clone();
    let v = s.execute("review.contrast", &json!({"ids": [g]})).unwrap();
    let shapes: Vec<&Value> = v.as_array().unwrap().iter().map(|e| &e["shape"]).collect();
    assert_eq!(shapes, [&a, &b], "{v}");
    assert_eq!(v[1]["ratio"]["min"].as_f64(), Some(4.54), "{v}");
}

#[test]
fn bad_parameters_are_refused_with_the_reason() {
    let mut s = white_slide();
    let id = text_box(&mut s, [100.0, 100.0, 400.0, 60.0], "Plain text", "#000000", 12.0);
    let refused = |s: &mut Session, p: &Value| s.execute("review.contrast", p).unwrap_err().to_string();
    for p in [json!({"level": "AA+"}), json!({"level": 3}), json!({"level": ["AA"]})] {
        let e = refused(&mut s, &p);
        assert!(e.contains("`level`"), "{p}: {e}");
    }
    let e = refused(&mut s, &json!({"slide": 99}));
    assert!(e.contains("no slide 99"), "{e}");
    for p in [json!({"slide": -1}), json!({"slide": 1.5}), json!({"slide": "two"})] {
        let e = refused(&mut s, &p);
        assert!(e.contains("`slide`"), "{p}: {e}");
    }
    for p in [
        json!({"ids": "x"}),
        json!({"ids": [-1]}),
        json!({"ids": [1.5]}),
        json!({"ids": ["a"]}),
        json!({"ids": {"id": 1}}),
        json!({"ids": [u64::MAX]}),
    ] {
        let e = refused(&mut s, &p);
        assert!(e.contains("`ids`"), "{p}: {e}");
    }
    let e = refused(&mut s, &json!({"ids": [999_999]}));
    assert!(e.contains("no shape 999999"), "{e}");
    // A scale out of range is clamped and the level's case is ignored.
    for p in [json!({"scale": 1e308}), json!({"scale": -5}), json!({"scale": "big"}), json!({"level": "aaa"})] {
        let v = contrast_of(&mut s, &id, p.clone());
        assert_eq!(v.len(), 1, "{p}: {v:?}");
    }
}
