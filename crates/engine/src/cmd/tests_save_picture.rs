//! `shape.saveAsPicture`: the right-click menu's Save as Picture…, the selected shapes as one
//! picture file (#1073).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{Value, json};

use crate::*;

/// A red rectangle and a blue one on a blank slide, both selected; returns their ids.
fn deck() -> (Session, u64, u64) {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let red = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap()["id"].as_u64().unwrap();
    s.execute("shape.fill", &json!({"color": "#ff0000"})).unwrap();
    s.execute("shape.line", &json!({"none": true})).unwrap();
    let blue = s.execute("shape.insert", &json!({"preset": "rect", "rect": [400, 300, 100, 50]})).unwrap()["id"].as_u64().unwrap();
    s.execute("shape.fill", &json!({"color": "#0000ff"})).unwrap();
    s.execute("shape.line", &json!({"none": true})).unwrap();
    s.execute("edit.select", &json!({"ids": [red, blue]})).unwrap();
    (s, red, blue)
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("deckcraft-save-picture-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save(s: &mut Session, path: &std::path::Path, extra: Value) -> Value {
    let mut p = json!({"path": path.to_str().unwrap()});
    p.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
    s.execute("shape.saveAsPicture", &p).unwrap()
}

#[test]
fn the_selection_is_one_png_cut_to_its_shapes_with_a_transparent_background() {
    let (mut s, _, _) = deck();
    let dir = scratch("png");
    let path = dir.join("Picture.png");
    let revision = s.doc().unwrap().revision;
    let v = save(&mut s, &path, json!({"scale": 1}));
    assert_eq!(s.doc().unwrap().revision, revision, "saving a picture does not change the presentation");
    let img = deckcraft_render::decode_image(&std::fs::read(&path).unwrap()).unwrap();
    // The union of the two shapes (100,100)-(500,350) and the 8 pt margin render_shapes keeps
    // for outlines and shadows: not the slide.
    assert_eq!((img.width(), img.height()), (416, 266));
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(416), Some(266)));
    let px = |x: u32, y: u32| img.get_pixel(x, y).0;
    assert_eq!(px(1, 1)[3], 0, "outside the shapes the picture is see-through");
    assert_eq!(px(300, 30)[3], 0, "the slide between the shapes is left out too");
    let red = px(8 + 100, 8 + 50);
    assert!(red[0] > 200 && red[2] < 40 && red[3] == 255, "{red:?}");
    let blue = px(8 + 350, 8 + 225);
    assert!(blue[2] > 200 && blue[0] < 40 && blue[3] == 255, "{blue:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn one_shape_alone_and_named_shapes_without_a_selection() {
    let (mut s, red, blue) = deck();
    let dir = scratch("ids");
    let path = dir.join("one.png");
    let v = save(&mut s, &path, json!({"ids": [blue], "scale": 1}));
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(116), Some(66)));
    s.execute("edit.deselect", &json!({})).unwrap();
    assert!(s.execute("shape.saveAsPicture", &json!({"path": dir.join("none.png").to_str().unwrap()})).is_err(), "nothing selected");
    assert!(!dir.join("none.png").exists());
    let v = save(&mut s, &dir.join("red.png"), json!({"ids": [red], "scale": 1}));
    assert_eq!(v["width"].as_u64(), Some(216));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_format_comes_from_the_name_and_opaque_formats_have_a_white_background() {
    let (mut s, _, _) = deck();
    let dir = scratch("formats");
    for (name, magic) in [
        ("p.jpg", &b"\xFF\xD8\xFF"[..]),
        ("p.jpeg", b"\xFF\xD8\xFF"),
        ("p.gif", b"GIF8"),
        ("p.tif", b"II*\0"),
        ("p.tiff", b"II*\0"),
        ("p.bmp", b"BM"),
        ("p.PNG", b"\x89PNG"),
    ] {
        let path = dir.join(name);
        save(&mut s, &path, json!({"scale": 1}));
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(magic), "{name}: {:?}", &bytes[..8.min(bytes.len())]);
    }
    for name in ["p.jpg", "p.bmp", "p.gif"] {
        let img = deckcraft_render::decode_image(&std::fs::read(dir.join(name)).unwrap()).unwrap();
        let corner = img.get_pixel(1, 1).0;
        assert!(corner[0] > 245 && corner[1] > 245 && corner[2] > 245, "{name}: white around the shapes, not black: {corner:?}");
        let red = img.get_pixel(8 + 100, 8 + 50).0;
        assert!(red[0] > 200 && red[1] < 60, "{name}: {red:?}");
    }
    // `format` wins over the name.
    let path = dir.join("named.png");
    save(&mut s, &path, json!({"format": "jpeg", "scale": 1}));
    assert!(std::fs::read(&path).unwrap().starts_with(b"\xFF\xD8\xFF"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_type_it_cannot_write_is_an_error_and_writes_nothing() {
    let (mut s, _, _) = deck();
    let dir = scratch("unknown");
    for name in ["p.svg", "p.emf", "p.wmf", "p"] {
        let path = dir.join(name);
        let e = s.execute("shape.saveAsPicture", &json!({"path": path.to_str().unwrap()})).unwrap_err();
        assert!(e.to_string().contains("PNG"), "{name}: {e}");
        assert!(!path.exists(), "{name}");
    }
    let e = s.execute("shape.saveAsPicture", &json!({"path": dir.join("p.png").to_str().unwrap(), "format": "svg"})).unwrap_err();
    assert!(e.to_string().contains("svg"), "{e}");
    assert!(s.execute("shape.saveAsPicture", &json!({})).is_err(), "a path is required");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_default_resolution_is_the_picture_export_s() {
    let (mut s, _, _) = deck();
    let dir = scratch("scale");
    let path = dir.join("p.png");
    let v = save(&mut s, &path, json!({}));
    // 2 px per point, as File › Export's pictures, and the PNG says 144 dpi so it opens at the
    // shapes' real size.
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(832), Some(532)));
    assert_eq!(deckcraft_render::image_dpi(&std::fs::read(&path).unwrap()), Some((144.0, 144.0)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_picture_capped_at_2048_px_says_the_resolution_it_was_drawn_at() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [0, 100, 1200, 100]})).unwrap();
    let dir = scratch("cap");
    let path = dir.join("wide.png");
    let v = save(&mut s, &path, json!({}));
    // 1200 pt plus the margins is 1216 pt: at 2 px per pt that is over render_shapes' 2048 px.
    assert_eq!(v["width"].as_u64(), Some(2048));
    let (dpi, _) = deckcraft_render::image_dpi(&std::fs::read(&path).unwrap()).unwrap();
    let drawn = 2048.0 / 1216.0 * 72.0;
    assert!((dpi - drawn).abs() < 1.0, "the PNG says {dpi} dpi, it was drawn at {drawn}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_shape_inside_a_resized_and_turned_group_is_saved_as_the_slide_shows_it() {
    let (mut s, red, blue) = deck();
    let g = s.execute("arrange.group", &json!({"ids": [red, blue]})).unwrap()["id"].as_u64().unwrap();
    // The group (400 × 250 pt) doubled: the red rectangle shows at 400 × 200 pt.
    s.execute("shape.setBounds", &json!({"ids": [g], "w": 800, "h": 500})).unwrap();
    let dir = scratch("group");
    let v = save(&mut s, &dir.join("child.png"), json!({"ids": [red], "scale": 1}));
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(416), Some(216)));
    let img = deckcraft_render::decode_image(&std::fs::read(dir.join("child.png")).unwrap()).unwrap();
    let mid = img.get_pixel(208, 108).0;
    assert!(mid[0] > 200 && mid[2] < 40 && mid[3] == 255, "{mid:?}");
    // Turned a quarter, it stands upright.
    s.execute("shape.rotate", &json!({"ids": [g], "deg": 90})).unwrap();
    let v = save(&mut s, &dir.join("turned.png"), json!({"ids": [red], "scale": 1}));
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(216), Some(416)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_id_not_on_the_page_is_an_error_and_writes_nothing() {
    let (mut s, red, _) = deck();
    let dir = scratch("unknown-id");
    let path = dir.join("p.png");
    let e = s.execute("shape.saveAsPicture", &json!({"path": path.to_str().unwrap(), "ids": [red, 9999]})).unwrap_err();
    assert!(e.to_string().contains("unknown shape id 9999"), "{e}");
    assert!(!path.exists());
    // The red rectangle is on the slide before this one.
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let e = s.execute("shape.saveAsPicture", &json!({"path": path.to_str().unwrap(), "ids": [red]})).unwrap_err();
    assert!(e.to_string().contains(&format!("unknown shape id {red}")), "{e}");
    assert!(!path.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn shapes_keep_their_stacking_order_whatever_order_they_were_picked_in() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let back = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap()["id"].as_u64().unwrap();
    s.execute("shape.fill", &json!({"color": "#ff0000"})).unwrap();
    let front = s.execute("shape.insert", &json!({"preset": "rect", "rect": [150, 120, 200, 100]})).unwrap()["id"].as_u64().unwrap();
    s.execute("shape.fill", &json!({"color": "#0000ff"})).unwrap();
    let dir = scratch("order");
    let path = dir.join("p.png");
    save(&mut s, &path, json!({"ids": [front, back], "scale": 1}));
    let img = deckcraft_render::decode_image(&std::fs::read(&path).unwrap()).unwrap();
    // Slide point (200, 150), inside both: the picture starts at (100 - 8, 100 - 8).
    let px = img.get_pixel(108, 58).0;
    assert!(px[2] > 200 && px[0] < 40, "the front shape stays in front: {px:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_group_and_a_shape_inside_it_draw_that_shape_once() {
    let (mut s, red, blue) = deck();
    s.execute("shape.fill", &json!({"ids": [red], "transparency": 0.5})).unwrap();
    let g = s.execute("arrange.group", &json!({"ids": [red, blue]})).unwrap()["id"].as_u64().unwrap();
    let dir = scratch("group-and-child");
    let pixel = |s: &mut Session, ids: Value, name: &str| {
        let path = dir.join(name);
        save(s, &path, json!({"ids": ids, "scale": 1}));
        deckcraft_render::decode_image(&std::fs::read(&path).unwrap()).unwrap().get_pixel(108, 58).0
    };
    let once = pixel(&mut s, json!([g]), "group.png");
    assert!(once[3] < 200, "the half see-through red: {once:?}");
    assert_eq!(pixel(&mut s, json!([g, red]), "both.png"), once);
    assert_eq!(pixel(&mut s, json!([red, g]), "both2.png"), once);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_wide_outline_is_saved_whole() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap();
    s.execute("shape.fill", &json!({"color": "#ff0000"})).unwrap();
    // 24 pt wide: it reaches 12 pt outside the box, past an 8 pt margin.
    s.execute("shape.line", &json!({"color": "#0000ff", "width": 24})).unwrap();
    let dir = scratch("outline");
    let path = dir.join("p.png");
    save(&mut s, &path, json!({"scale": 1}));
    let img = deckcraft_render::decode_image(&std::fs::read(&path).unwrap()).unwrap();
    let mid = img.height() / 2;
    assert_eq!(img.get_pixel(0, mid).0[3], 0, "nothing is cut at the picture's edge");
    let row: Vec<[u8; 4]> = (0..img.width()).map(|x| img.get_pixel(x, mid).0).collect();
    let first = row.iter().position(|p| p[3] > 200).unwrap();
    let last = row.iter().rposition(|p| p[3] > 200).unwrap();
    assert!((last - first + 1).abs_diff(224) <= 2, "the outline's outer edges are 224 pt apart: {first}..{last}");
    assert!(row[first + 2][2] > 200 && row[first + 2][0] < 40, "the outer edge is the blue outline: {:?}", row[first + 2]);
    let _ = std::fs::remove_dir_all(&dir);
}
