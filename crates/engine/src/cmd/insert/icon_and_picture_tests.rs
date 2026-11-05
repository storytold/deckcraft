use deckcraft_model::ShapeKind;
use deckcraft_model::style::LineJoin;

use super::*;

/// 8×8 red PNG.
fn png() -> Vec<u8> {
    vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0, 0, 0, 75, 109, 41, 220, 0, 0, 0, 17, 73, 68,
        65, 84, 120, 156, 99, 184, 163, 161, 129, 21, 49, 12, 45, 9, 0, 123, 123, 75, 1, 251, 162, 81, 240, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
        130,
    ]
}

fn pic(name: &str) -> Value {
    json!({"name": name, "data": base64_encode(&png())})
}

fn n_slides(s: &Session) -> usize {
    s.active().map(|d| d.doc.slides.len()).unwrap_or(0)
}

fn pictures(s: &Session, index: usize) -> Vec<deckcraft_model::Shape> {
    s.active()
        .and_then(|d| d.doc.slides.get(index))
        .map(|sl| sl.shapes.iter().filter(|sh| matches!(sh.kind, ShapeKind::Picture { .. })).cloned().collect())
        .unwrap_or_default()
}

fn box_text(s: &Session, index: usize) -> Vec<String> {
    s.active()
        .and_then(|d| d.doc.slides.get(index))
        .map(|sl| sl.shapes.iter().filter(|sh| sh.text_box).filter_map(|sh| sh.text.as_ref().map(|t| t.text())).collect())
        .unwrap_or_default()
}

#[test]
fn two_pictures_one_per_slide_are_one_undo() {
    let mut s = Session::with_new();
    let before = n_slides(&s);
    let r = s.execute("insert.photoAlbum", &json!({"pictures": [pic("hill.png"), pic("lake.png")], "layout": "1"})).expect("album");
    assert_eq!(r["slides"], 2);
    assert_eq!(n_slides(&s), before + 2);
    assert_eq!(pictures(&s, 1).len(), 1);
    assert_eq!(pictures(&s, 2).len(), 1);
    assert_eq!(s.active().unwrap().selection.slide, 1);
    let undo = &s.active().unwrap().history.undo;
    assert_eq!(undo.len(), 1);
    assert_eq!(undo[0].label, "Photo Album…");
    s.execute("edit.undo", &json!({})).expect("undo");
    assert_eq!(n_slides(&s), before);
    assert!(pictures(&s, 1).is_empty());
}

#[test]
fn two_up_shares_one_slide() {
    let mut s = Session::with_new();
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png"), pic("b.png")], "layout": "2"})).expect("album");
    assert_eq!(n_slides(&s), 2);
    assert_eq!(pictures(&s, 1).len(), 2);
    assert!(box_text(&s, 1).is_empty());
}

#[test]
fn captions_and_titles_use_the_file_stem() {
    let mut s = Session::with_new();
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("harbor.png")], "layout": "1", "captions": true})).expect("captions");
    assert_eq!(box_text(&s, 1), ["harbor"]);
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("harbor.png")], "layout": "1title"})).expect("title");
    assert_eq!(box_text(&s, 1), ["harbor"]);
    assert_eq!(pictures(&s, 1).len(), 1);
    let pic_y = pictures(&s, 1)[0].xfrm.unwrap().y;
    assert!(pic_y > 40.0, "the picture sits below the title");
}

#[test]
fn frames_set_the_picture_line() {
    let mut s = Session::with_new();
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "frame": "rect"})).expect("rect");
    let line = pictures(&s, 1)[0].line.clone().expect("line");
    assert_eq!(line.width, Some(1.5));
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "frame": "rounded"})).expect("rounded");
    let line = pictures(&s, 1)[0].line.clone().expect("line");
    assert_eq!(line.width, Some(2.25));
    assert_eq!(line.join, Some(LineJoin::Round));
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "frame": "simple"})).expect("simple");
    assert_eq!(pictures(&s, 1)[0].line.as_ref().and_then(|l| l.width), Some(6.0));
}

#[test]
fn a_text_box_breaks_the_picture_row() {
    let mut s = Session::with_new();
    let r = s
        .execute("insert.photoAlbum", &json!({"pictures": [pic("a.png"), {"text": "A day on the water"}, pic("b.png")], "layout": "2"}))
        .expect("album");
    assert_eq!(r["slides"], 3);
    assert_eq!(pictures(&s, 1).len(), 1);
    assert_eq!(box_text(&s, 2), ["A day on the water"]);
    assert!(pictures(&s, 2).is_empty());
    assert_eq!(pictures(&s, 3).len(), 1);
    assert_eq!(s.active().unwrap().history.undo.len(), 1);
}

#[test]
fn no_pictures_opens_the_dialog_without_editing() {
    let mut s = Session::with_new();
    let before = n_slides(&s);
    let r = s.execute("insert.photoAlbum", &json!({})).expect("open");
    assert_eq!(r["dialog"], "photoAlbum");
    assert_eq!(n_slides(&s), before);
    assert!(s.active().unwrap().history.undo.is_empty());
    assert_eq!(s.ui_requests.len(), 1);
    match &s.ui_requests[0] {
        crate::UiRequest::Dialog { id, .. } => assert_eq!(id, "photoAlbum"),
        other => panic!("dialog request, got {other:?}"),
    }
}

#[test]
fn bad_input_adds_no_slides() {
    let mut s = Session::with_new();
    let before = n_slides(&s);
    assert!(s.execute("insert.photoAlbum", &json!({"pictures": [], "layout": "1"})).is_err());
    assert!(s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "layout": "grid"})).is_err());
    assert!(s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "frame": "glow"})).is_err());
    assert!(s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "captions": "yes"})).is_err());
    assert!(s.execute("insert.photoAlbum", &json!({"pictures": [{"name": "notes.txt", "data": base64_encode(b"hello")}]} )).is_err());
    assert!(s.execute("insert.photoAlbum", &json!({"pictures": [{"text": "  "}]})).is_err());
    let mut many = Vec::new();
    for i in 0..49 {
        many.push(json!({"text": format!("box {i}")}));
    }
    assert!(s.execute("insert.photoAlbum", &json!({"pictures": many})).is_err());
    assert_eq!(n_slides(&s), before);
    assert!(s.active().unwrap().history.undo.is_empty());
}

#[test]
fn fit_is_larger_than_one_picture_and_four_up_is_one_slide() {
    let mut s = Session::with_new();
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "layout": "1"})).unwrap();
    let one = pictures(&s, 1)[0].xfrm.unwrap().h;
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "layout": "fit"})).unwrap();
    let fitted = pictures(&s, 1)[0].xfrm.unwrap().h;
    assert!(fitted > one, "fit {fitted} should be taller than one-up {one}");
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png"), pic("b.png"), pic("c.png"), pic("d.png")], "layout": "4"})).unwrap();
    assert_eq!(n_slides(&s), 2);
    assert_eq!(pictures(&s, 1).len(), 4);
}

fn adjust(s: &Session, index: usize) -> deckcraft_model::style::PictureAdjust {
    match &pictures(s, index)[0].kind {
        ShapeKind::Picture { fill } => fill.adjust.clone(),
        _ => panic!("picture"),
    }
}

fn theme_name(s: &Session) -> String {
    s.active().and_then(|d| d.doc.masters.first()).map(|m| m.theme.name.clone()).unwrap_or_default()
}

#[test]
fn edit_without_pictures_reopens_the_dialog() {
    let mut s = Session::with_new();
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("hill.png"), pic("lake.png")], "layout": "1"})).unwrap();
    s.ui_requests.clear();
    let before = n_slides(&s);
    let undos = s.active().unwrap().history.undo.len();
    let r = s.execute("insert.photoAlbumEdit", &json!({})).expect("reopen");
    assert_eq!(r["dialog"], "photoAlbum");
    assert_eq!(n_slides(&s), before);
    assert_eq!(s.active().unwrap().history.undo.len(), undos);
    let params = match &s.ui_requests[0] {
        crate::UiRequest::Dialog { id, params } => {
            assert_eq!(id, "photoAlbum");
            params
        }
        other => panic!("dialog request, got {other:?}"),
    };
    assert_eq!(params["items"][0]["name"], "hill.png");
    assert_eq!(params["items"][1]["name"], "lake.png");
    assert_eq!(params["layout"], "1");
    assert!(params["album"].as_u64().unwrap_or(0) > 0);
}

#[test]
fn edit_reflows_the_album_in_one_undo() {
    let mut s = Session::with_new();
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("hill.png"), pic("lake.png")], "layout": "1"})).unwrap();
    assert_eq!(n_slides(&s), 3);
    let r = s.execute("insert.photoAlbumEdit", &json!({"pictures": [pic("hill.png"), pic("lake.png")], "layout": "2"})).expect("reflow");
    assert_eq!(r["slides"], 1);
    assert_eq!(n_slides(&s), 2);
    assert_eq!(pictures(&s, 1).len(), 2);
    let undo = &s.active().unwrap().history.undo;
    assert_eq!(undo.last().map(|u| u.label.as_str()), Some("Edit Photo Album…"));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(n_slides(&s), 3);
    assert_eq!(pictures(&s, 1).len(), 1);
    assert_eq!(pictures(&s, 2).len(), 1);
}

#[test]
fn picture_options_and_theme_are_one_step() {
    let mut s = Session::with_new();
    s.execute(
        "insert.photoAlbum",
        &json!({"pictures": [{"name": "hill.png", "data": base64_encode(&png()), "rot": 90, "brightness": 0.2, "contrast": -0.1}], "grayscale": true, "theme": "Harbor"}),
    )
    .unwrap();
    let adj = adjust(&s, 1);
    assert!(adj.grayscale);
    assert!((adj.brightness - 0.2).abs() < 1e-6);
    assert!((adj.contrast + 0.1).abs() < 1e-6);
    assert!((pictures(&s, 1)[0].xfrm.unwrap().rot - 90.0).abs() < 0.01);
    assert_eq!(theme_name(&s), "Harbor");
    let mark = s.active().unwrap().doc.slides[1].raw_ext.clone().unwrap_or_default();
    assert!(mark.contains("gray=1"), "{mark}");
    assert!(mark.contains("theme=Harbor"), "{mark}");
    assert_eq!(s.active().unwrap().doc.slides[1].name, "Photo Album");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(theme_name(&s), "DeckCraft");
    assert_eq!(n_slides(&s), 1);
}

#[test]
fn unknown_theme_and_a_plain_slide_change_nothing() {
    let mut s = Session::with_new();
    let before = n_slides(&s);
    let err = s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "theme": "NotATheme"})).unwrap_err();
    assert!(err.to_string().contains("unknown theme"), "{err}");
    assert!(s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png"), {"brightness": "high"}]})).is_err());
    assert!(s.execute("insert.photoAlbum", &json!({"pictures": [{"name": "a.png", "data": base64_encode(&png()), "brightness": 2}]})).is_err());
    let err = s.execute("insert.photoAlbumEdit", &json!({})).unwrap_err();
    assert!(err.to_string().contains("not part of a photo album"), "{err}");
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")]})).unwrap();
    s.execute("slide.go", &json!({"index": 0})).unwrap();
    assert!(s.execute("insert.photoAlbumEdit", &json!({"pictures": [pic("b.png")], "layout": "2"})).is_err());
    assert_eq!(n_slides(&s), before + 1);
    assert_eq!(pictures(&s, 1).len(), 1);
    assert_eq!(theme_name(&s), "DeckCraft");
}

#[test]
fn two_albums_stay_independent() {
    let mut s = Session::with_new();
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png"), pic("b.png")], "layout": "1"})).unwrap();
    let last = n_slides(&s) - 1;
    s.execute("slide.go", &json!({"index": last})).unwrap();
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("c.png"), pic("d.png")], "layout": "1"})).unwrap();
    assert_eq!(n_slides(&s), 5);
    s.execute("insert.photoAlbumEdit", &json!({"pictures": [pic("c.png"), pic("d.png")], "layout": "2"})).unwrap();
    assert_eq!(n_slides(&s), 4);
    assert_eq!(pictures(&s, 1).len(), 1);
    assert_eq!(pictures(&s, 2).len(), 1);
    assert_eq!(pictures(&s, 3).len(), 2);
    let first = s.active().unwrap().doc.slides[1].raw_ext.clone().unwrap_or_default();
    let second = s.active().unwrap().doc.slides[3].raw_ext.clone().unwrap_or_default();
    assert!(first.starts_with("deckcraft-album:"));
    assert!(second.starts_with("deckcraft-album:"));
    assert_ne!(first.split(';').next(), second.split(';').next());
}

#[test]
fn a_named_run_without_a_marker_still_reflows() {
    let mut s = Session::with_new();
    s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png"), pic("b.png")], "layout": "1"})).unwrap();
    {
        let doc = std::sync::Arc::make_mut(&mut s.active_mut().unwrap().doc);
        for sl in doc.slides.iter_mut().skip(1) {
            std::sync::Arc::make_mut(sl).raw_ext = None;
        }
    }
    s.execute("insert.photoAlbumEdit", &json!({"pictures": [pic("a.png"), pic("b.png")], "layout": "2"})).unwrap();
    assert_eq!(n_slides(&s), 2);
    assert_eq!(pictures(&s, 1).len(), 2);
    let mark = s.active().unwrap().doc.slides[1].raw_ext.clone().unwrap_or_default();
    assert!(mark.starts_with("deckcraft-album:"), "{mark}");
}
