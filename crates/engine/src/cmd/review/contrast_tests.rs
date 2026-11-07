//! `review.contrast`: the contrast of each text run with what the slide draws behind it, on decks
//! built here with commands and pictures made from pixels.

use deckcraft_geom::Point;
use deckcraft_model::text::{Run, RunKind, RunProps};
use deckcraft_model::{Shape, ShapeId, ShapeKind, Xfrm};
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

/// Adds a rectangle with no text, filled with `fill` (`transparency` 0..1), drawn over the
/// shapes added before it.
fn band(s: &mut Session, rect: [f64; 4], fill: &str, transparency: f64) -> Value {
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": rect})).unwrap()["id"].clone();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    s.execute("shape.fill", &json!({"color": fill, "transparency": transparency})).unwrap();
    id
}

fn shape_id(id: &Value) -> ShapeId {
    ShapeId(u32::try_from(id.as_u64().unwrap()).unwrap())
}

/// The properties of the first run of shape `id` on the current slide.
fn first_run_props(s: &mut Session, id: &Value) -> RunProps {
    let st = s.doc().unwrap();
    let slide = &st.doc.slides[st.selection.slide];
    let shape = deckcraft_model::find_shape(&slide.shapes, shape_id(id)).unwrap();
    shape.text.as_ref().unwrap().paragraphs[0].runs[0].props.clone()
}

/// Puts `runs` in place of the runs of the first paragraph of shape `id` on the current slide.
fn set_runs(s: &mut Session, id: &Value, runs: Vec<Run>) {
    let st = s.doc_mut().unwrap();
    let index = st.selection.slide;
    let doc = std::sync::Arc::make_mut(&mut st.doc);
    let slide = std::sync::Arc::make_mut(&mut doc.slides[index]);
    let shape = deckcraft_model::find_shape_mut(&mut slide.shapes, shape_id(id)).unwrap();
    shape.text.as_mut().unwrap().paragraphs[0].runs = runs;
}

/// Puts shape `id`, one of the current slide's top-level shapes, in a group whose frame is `kx`
/// times as wide and `ky` times as tall as its child space, the shape's own box.
fn stretch_in_group(s: &mut Session, id: &Value, kx: f64, ky: f64) {
    let st = s.doc_mut().unwrap();
    let index = st.selection.slide;
    let doc = std::sync::Arc::make_mut(&mut st.doc);
    let slide = std::sync::Arc::make_mut(&mut doc.slides[index]);
    let at = slide.shapes.iter().position(|sh| sh.id == shape_id(id)).unwrap();
    let shape = slide.shapes.remove(at);
    let x = shape.xfrm.unwrap();
    let (child, frame) = (Xfrm::new(x.x, x.y, x.w, x.h), Xfrm::new(x.x, x.y, x.w * kx, x.h * ky));
    let group = Shape { id: ShapeId(900), xfrm: Some(frame), kind: ShapeKind::Group { children: vec![shape], child }, ..Default::default() };
    slide.shapes.push(group);
}

fn run(text: &str, props: &RunProps, kind: RunKind) -> Run {
    Run { text: text.into(), props: props.clone(), kind }
}

/// A 12 pt text box on a white slide whose first paragraph holds `runs`, built from the
/// properties of black text and of #767676 grey text.
fn runs_box(runs: impl Fn(&RunProps, &RunProps) -> Vec<Run>) -> (Session, Value) {
    let mut s = white_slide();
    let id = text_box(&mut s, [100.0, 100.0, 700.0, 60.0], "Grey", "#767676", 12.0);
    let grey = first_run_props(&mut s, &id);
    s.execute("format.color", &json!({"color": "#000000"})).unwrap();
    let black = first_run_props(&mut s, &id);
    set_runs(&mut s, &id, runs(&black, &grey));
    (s, id)
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
fn text_turned_in_a_stretched_group_is_judged_at_its_font_size() {
    // A shape turned 45° inside a group twice as wide as its child space is drawn slanted, and
    // its 20 pt text is still 20 pt along the slant: large text, 3:1 at AA. #767676 on white is
    // 4.54:1.
    let mut s = white_slide();
    let id = text_box(&mut s, [100.0, 150.0, 300.0, 80.0], "Sample", "#767676", 20.0);
    s.execute("shape.rotate", &json!({"ids": [id], "deg": 45.0})).unwrap();
    stretch_in_group(&mut s, &id, 2.0, 1.0);
    let e = only(&mut s, &id, json!({}));
    assert_eq!((e["sizePt"].as_f64(), e["required"].as_f64()), (Some(20.0), Some(3.0)), "{e}");
    assert!((ratio(&e, "median") - 4.54).abs() < 0.011, "{e}");
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

#[test]
fn black_text_under_a_half_clear_white_band_is_measured_as_seen() {
    let mut s = white_slide();
    let id = text_box(&mut s, [100.0, 100.0, 400.0, 60.0], "Covered text", "#000000", 12.0);
    band(&mut s, [80.0, 80.0, 440.0, 100.0], "#FFFFFF", 0.5);
    let e = only(&mut s, &id, json!({}));
    // The band turns the black text mid grey and leaves the white slide white: about 3.95:1,
    // where the text without the band gives 21:1.
    for key in ["min", "p5", "median"] {
        let r = ratio(&e, key);
        assert!(r > 3.85 && r < 4.1, "{key}: {e}");
    }
    assert_eq!(e["color"], "#000000");
    assert_eq!(e["passShare"].as_f64(), Some(0.0), "{e}");
}

#[test]
fn text_under_an_opaque_shape_measures_1_to_1() {
    let mut s = white_slide();
    let id = text_box(&mut s, [100.0, 100.0, 400.0, 60.0], "Hidden text", "#000000", 12.0);
    band(&mut s, [80.0, 80.0, 440.0, 100.0], "#3366CC", 0.0);
    let e = only(&mut s, &id, json!({}));
    for key in ["min", "p5", "median"] {
        assert_eq!(ratio(&e, key), 1.0, "{key}: {e}");
    }
    assert_eq!(e["passShare"].as_f64(), Some(0.0), "{e}");
}

#[test]
fn a_shape_drawn_over_another_part_of_the_slide_changes_nothing() {
    let mut s = white_slide();
    let id = text_box(&mut s, [100.0, 100.0, 400.0, 60.0], "Plain text", "#000000", 12.0);
    band(&mut s, [80.0, 300.0, 440.0, 100.0], "#3366CC", 0.0);
    let e = only(&mut s, &id, json!({}));
    for key in ["min", "p5", "median"] {
        assert_eq!(ratio(&e, key), 21.0, "{key}: {e}");
    }
    assert_eq!(e["passShare"].as_f64(), Some(1.0), "{e}");
}

#[test]
fn a_later_member_of_the_texts_group_counts_as_drawn_over_it() {
    let mut s = white_slide();
    let id = text_box(&mut s, [100.0, 100.0, 400.0, 60.0], "Grouped text", "#000000", 12.0);
    let cover = band(&mut s, [80.0, 80.0, 440.0, 100.0], "#FFFFFF", 0.5);
    s.execute("arrange.group", &json!({"ids": [id, cover]})).unwrap();
    let e = only(&mut s, &id, json!({}));
    let r = ratio(&e, "median");
    assert!(r > 3.85 && r < 4.1, "{e}");
}

#[test]
fn text_half_under_a_band_is_read_on_both_sides() {
    let mut s = white_slide();
    let id = text_box(&mut s, [100.0, 100.0, 600.0, 60.0], "Half of this line sits under the band", "#000000", 12.0);
    band(&mut s, [180.0, 80.0, 500.0, 100.0], "#FFFFFF", 0.5);
    let e = only(&mut s, &id, json!({}));
    assert!(ratio(&e, "min") < 4.1, "{e}");
    let pass = e["passShare"].as_f64().unwrap();
    assert!(pass > 0.1 && pass < 0.9, "{e}");
}

#[test]
fn a_slide_number_field_reports_the_number_it_draws() {
    let (mut s, id) = runs_box(|black, grey| {
        vec![
            run("Page ", black, RunKind::Text),
            run("‹#›", black, RunKind::Field { field: "slidenum".into() }),
            run(" of the deck", grey, RunKind::Text),
        ]
    });
    let v = contrast_of(&mut s, &id, json!({}));
    // The field draws the slide's number, 2, in place of its three saved characters, and the run
    // after it is measured with its own glyphs.
    let found: Vec<(Option<u64>, Option<&str>, Option<&str>)> =
        v.iter().map(|e| (e["run"].as_u64(), e["text"].as_str(), e["color"].as_str())).collect();
    assert_eq!(
        found,
        [(Some(0), Some("Page "), Some("#000000")), (Some(1), Some("2"), Some("#000000")), (Some(2), Some(" of the deck"), Some("#767676"))],
        "{v:?}"
    );
    assert_eq!(v[2]["ratio"]["min"].as_f64(), Some(4.54), "{v:?}");
}

#[test]
fn runs_after_two_fields_are_measured_with_their_own_glyphs() {
    let (mut s, id) = runs_box(|black, grey| {
        vec![
            run("Slide ", black, RunKind::Text),
            run("‹#›", black, RunKind::Field { field: "slidenum".into() }),
            run(" printed ", black, RunKind::Text),
            run("date", black, RunKind::Field { field: "datetime1".into() }),
            run(" in grey", grey, RunKind::Text),
        ]
    });
    let v = contrast_of(&mut s, &id, json!({}));
    let runs: Vec<Option<u64>> = v.iter().map(|e| e["run"].as_u64()).collect();
    assert_eq!(runs, [Some(0), Some(1), Some(2), Some(3), Some(4)], "{v:?}");
    assert_eq!(v[1]["text"], "2", "{v:?}");
    // datetime1 draws the date as month/day/year, longer than its four saved characters.
    let date = v[3]["text"].as_str().unwrap();
    assert!(date.chars().count() >= 8 && date.contains('/'), "{v:?}");
    for e in &v[..4] {
        assert_eq!(e["color"], "#000000", "{e}");
    }
    assert_eq!((v[4]["text"].as_str(), v[4]["color"].as_str()), (Some(" in grey"), Some("#767676")), "{v:?}");
    assert_eq!(v[4]["ratio"]["min"].as_f64(), Some(4.54), "{v:?}");
}

#[test]
fn a_conjunct_is_measured_and_the_space_after_it_is_not() {
    // Shaping draws the Devanagari conjunct क्ष (three characters) as one glyph, so "क्ष क" is
    // three glyphs for five characters. Each glyph is paired with the character it draws, the
    // conjunct with its first, so the space is not measured: a black band behind the middle of the
    // space leaves black text at 21:1.
    let fonts = deckcraft_fonts::FontDb::global();
    let deva = fonts.document_face("Noto Sans Devanagari", "Regular");
    // Required native builds embed the pinned Noto Sans Devanagari (craft-fonts'
    // fonts/manifest.txt); an optional local build without it skips this test.
    let pinned = deva.family == "Noto Sans Devanagari";
    assert!(pinned || option_env!("CRAFT_FONTS_REQUIRED").is_none(), "required native build draws {}", deva.family);
    if !pinned {
        return;
    }
    let build = |behind: Option<[f64; 4]>| {
        let mut s = white_slide();
        if let Some(r) = behind {
            band(&mut s, r, "#000000", 0.0);
        }
        let id = text_box(&mut s, [100.0, 100.0, 300.0, 80.0], "क्ष क", "#000000", 40.0);
        (s, id)
    };
    let (s, id) = build(None);
    let st = s.doc().unwrap();
    let placed = deckcraft_render::place_slide(&st.doc, st.selection.slide);
    let text = placed.texts.iter().find(|t| t.shape == shape_id(&id)).unwrap();
    let chars: Vec<char> = text.layout.runs.iter().flat_map(|g| g.cells.iter().map(|&(_, ch)| ch)).collect();
    assert_eq!(chars, ['क', ' ', 'क'], "one glyph for the conjunct, paired with its first character");
    let (g, i) = text.layout.runs.iter().find_map(|g| Some((g, g.cells.iter().position(|&(_, ch)| ch == ' ')?))).unwrap();
    let (gid, x, _) = g.glyphs[i];
    let w = g.face.advance(gid) * g.size / g.face.upem.max(1.0);
    let mid = text.transform * Point::new(x + w / 2.0, 0.0);
    let (mut s, id) = build(Some([mid.x - w / 5.0, 100.0, w * 0.4, 80.0]));
    let e = only(&mut s, &id, json!({}));
    assert!(ratio(&e, "min") > 20.0, "the space over the band is not measured: {e}");
}

#[test]
fn a_call_past_the_pixel_budget_is_refused_before_drawing() {
    let mut s = white_slide();
    // At scale 4 the 960 by 540 point slide is drawn at 3840 by 2160 pixels. Thirteen text boxes,
    // each with a shape drawn after it, take 26 drawings: 216 million pixels.
    let ids: Vec<Value> = (0..13).map(|k| text_box(&mut s, [20.0 + 70.0 * f64::from(k), 100.0, 60.0, 40.0], "Text", "#000000", 12.0)).collect();
    band(&mut s, [0.0, 400.0, 960.0, 100.0], "#3366CC", 0.0);
    let e = s.execute("review.contrast", &json!({"scale": 4})).unwrap_err().to_string();
    assert!(e.contains("26 drawings of 3840×2160 pixels"), "{e}");
    assert!(e.contains("200 million") && e.contains("`ids`") && e.contains("`scale`"), "{e}");
    // One of them on its own takes two drawings and is measured.
    let entry = only(&mut s, &ids[0], json!({"scale": 4}));
    assert_eq!(entry["text"], "Text");
    assert_eq!(ratio(&entry, "median"), 21.0);
}
