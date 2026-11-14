//! Picture facts: the pixel size, resolution and aspect `slide.inspect` and `shape.inspect`
//! report for a picture as drawn, the `review.pictures` check, and `fit` on `insert.picture`.
//! The pictures are PNGs made here, and each test's comment works out the numbers it expects.

use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::style::PictureFill;
use deckcraft_model::{Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use crate::Session;
use crate::cmd::{bad, base64_encode};

/// A `w` × `h` px PNG.
fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb([200, 120, 40]));
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// Insert a `w` × `h` px PNG with the extra params `extra` (`rect`, `fit`) and return its id.
fn insert(s: &mut Session, w: u32, h: u32, extra: Value) -> u64 {
    let mut p = json!({"data": base64_encode(&png(w, h)), "name": "picture.png"});
    if let (Some(o), Some(more)) = (p.as_object_mut(), extra.as_object()) {
        o.extend(more.clone());
    }
    s.execute("insert.picture", &p).unwrap()["id"].as_u64().unwrap()
}

/// Shape `id`'s `slide.inspect` entry on the current slide, looked for inside groups too.
fn entry(s: &mut Session, id: u64) -> Value {
    fn find(list: &[Value], id: u64) -> Option<Value> {
        list.iter().find_map(|e| if e["id"] == id { Some(e.clone()) } else { e["children"].as_array().and_then(|c| find(c, id)) })
    }
    let slide = s.execute("slide.inspect", &json!({})).unwrap();
    find(slide["shapes"].as_array().unwrap(), id).unwrap_or_else(|| panic!("no entry for {id} in {slide}"))
}

/// `review.pictures` entries for shape `id`, run with `p`.
fn findings(s: &mut Session, p: Value, id: u64) -> Vec<Value> {
    let all = s.execute("review.pictures", &p).unwrap();
    all.as_array().unwrap().iter().filter(|e| e["shape"] == id).cloned().collect()
}

fn kinds(found: &[Value]) -> Vec<&str> {
    found.iter().filter_map(|e| e["kind"].as_str()).collect()
}

/// Picture `id`'s stored box and crop.
fn stored(s: &Session, id: u64) -> (Xfrm, [f64; 4]) {
    let sh = s.doc().unwrap().shape(ShapeId(u32::try_from(id).unwrap())).unwrap().clone();
    let ShapeKind::Picture { fill } = sh.kind else { panic!("shape {id} is not a picture") };
    (sh.xfrm.unwrap_or_default(), fill.crop)
}

fn assert_near(got: [f64; 4], want: [f64; 4]) {
    assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-9), "got {got:?}, expected {want:?}");
}

fn assert_box(x: &Xfrm, want: [f64; 4]) {
    assert_near([x.x, x.y, x.w, x.h], want);
}

/// Pictures with `fill`'s crop and the given ids and boxes, added to the first slide as they
/// are, with `bytes` as their image.
fn add_pictures(s: &mut Session, bytes: &[u8], content_type: &str, crop: [f64; 4], pictures: &[(u32, Xfrm)]) {
    s.edit(|doc, _| {
        let media = doc.add_media("picture", content_type, bytes.to_vec()).ok_or_else(|| bad("test", "media"))?;
        let slide = doc.slides.first_mut().ok_or_else(|| bad("test", "slide"))?;
        for &(id, x) in pictures {
            Arc::make_mut(slide).shapes.push(Shape {
                id: ShapeId(id),
                xfrm: Some(x),
                kind: ShapeKind::Picture { fill: PictureFill { media, crop, ..Default::default() } },
                ..Default::default()
            });
        }
        Ok(())
    })
    .unwrap();
}

/// A 400 × 300 px picture in a 420 × 150 pt box. The image is 4:3 (1.333) and the box shows it
/// at 2.8, so it is drawn 2.1 times as wide, relative to its height, as it is (distortion
/// 2.8 / 1.333 - 1 = 1.1). Across, 400 px over 420 pt (5.83 in) is 68.57 ppi, under 96; down,
/// 300 px over 150 pt is 144. The box that fits it is 4:3 at the full 150 pt height, 200 pt wide
/// and centred: 40 + (420 - 200) / 2 = 150.
#[test]
fn a_picture_stretched_into_a_wide_box_is_reported_with_the_box_that_fits_it() {
    let mut s = Session::with_new();
    let id = insert(&mut s, 400, 300, json!({"rect": [40, 130, 420, 150]}));
    let e = entry(&mut s, id);
    assert_eq!(e["pixels"], json!([400, 300]), "{e}");
    assert_eq!(e["ppi"], json!([68.57, 144.0]), "{e}");
    assert_eq!(e["aspect"], json!({"source": 1.333, "shown": 2.8}), "{e}");
    let inspected = s.execute("shape.inspect", &json!({"id": id})).unwrap();
    assert_eq!(inspected["picture"], json!({"pixels": [400, 300], "ppi": [68.57, 144.0], "aspect": {"source": 1.333, "shown": 2.8}}), "{inspected}");
    assert_eq!(inspected["shape"]["id"], id, "{inspected}");

    let found = findings(&mut s, json!({}), id);
    assert_eq!(kinds(&found), ["stretched", "lowResolution"], "{found:?}");
    for f in &found {
        assert_eq!(f["slide"], 0, "{f}");
        assert_eq!(f["name"], e["name"], "{f}");
        assert_eq!(f["ppi"], json!([68.57, 144.0]), "{f}");
        assert_eq!(f["distortion"], json!(1.1), "{f}");
        assert_eq!(f["fitRect"], json!([150.0, 130.0, 200.0, 150.0]), "{f}");
    }
    // In that box the picture is undistorted and 144 ppi both ways, so nothing is left to report.
    s.execute("shape.setBounds", &json!({"ids": [id], "x": 150, "y": 130, "w": 200, "h": 150})).unwrap();
    let e = entry(&mut s, id);
    assert_eq!(e["ppi"], json!([144.0, 144.0]), "{e}");
    assert_eq!(e["aspect"], json!({"source": 1.333, "shown": 1.333}), "{e}");
    assert!(findings(&mut s, json!({}), id).is_empty());
}

/// A 48 × 48 px picture in a 380 pt square is 48 / (380 / 72) = 9.09 ppi, a low-resolution
/// picture that is not stretched. `minPpi` moves the bar, and 0 turns it off.
#[test]
fn a_small_picture_in_a_big_box_is_low_resolution() {
    let mut s = Session::with_new();
    let id = insert(&mut s, 48, 48, json!({"rect": [520, 120, 380, 380]}));
    let e = entry(&mut s, id);
    assert_eq!(e["pixels"], json!([48, 48]), "{e}");
    assert_eq!(e["ppi"], json!([9.09, 9.09]), "{e}");
    assert_eq!(e["aspect"], json!({"source": 1.0, "shown": 1.0}), "{e}");
    let found = findings(&mut s, json!({}), id);
    assert_eq!(kinds(&found), ["lowResolution"], "{found:?}");
    assert_eq!(found[0]["ppi"], json!([9.09, 9.09]));
    assert_eq!(found[0]["distortion"], json!(0.0));
    assert_eq!(found[0]["fitRect"], json!([520.0, 120.0, 380.0, 380.0]));
    assert!(findings(&mut s, json!({"minPpi": 9}), id).is_empty());
    assert_eq!(kinds(&findings(&mut s, json!({"minPpi": 10}), id)), ["lowResolution"]);
    assert!(findings(&mut s, json!({"minPpi": 0}), id).is_empty());
}

/// The crop decides which part of the image the box shows. Cutting a quarter off the left and
/// right of a 400 × 300 px image leaves 200 × 300 px (2:3, 0.667), which a 200 × 150 pt box draws
/// at 4:3, twice as wide as it is (distortion 1), and 200 px across 200 pt is 72 ppi. The box
/// that fits it is 2:3 at the full 150 pt height: 100 pt wide, 50 pt in from each side. A
/// negative crop pads: a quarter of padding each side makes the image 600 px wide (2:1), which
/// a 300 × 150 pt box draws undistorted at 144 ppi.
#[test]
fn a_crop_changes_the_aspect_and_resolution_the_box_shows() {
    let mut s = Session::with_new();
    let image = png(400, 300);
    add_pictures(&mut s, &image, "image/png", [0.25, 0.0, 0.25, 0.0], &[(80_201, Xfrm::new(100.0, 100.0, 200.0, 150.0))]);
    add_pictures(&mut s, &image, "image/png", [-0.25, 0.0, -0.25, 0.0], &[(80_202, Xfrm::new(400.0, 100.0, 300.0, 150.0))]);
    let cut = entry(&mut s, 80_201);
    assert_eq!(cut["pixels"], json!([400, 300]), "{cut}");
    assert_eq!(cut["ppi"], json!([72.0, 144.0]), "{cut}");
    assert_eq!(cut["aspect"], json!({"source": 0.667, "shown": 1.333}), "{cut}");
    let found = findings(&mut s, json!({}), 80_201);
    assert_eq!(kinds(&found), ["stretched", "lowResolution"], "{found:?}");
    assert_eq!(found[0]["distortion"], json!(1.0));
    assert_eq!(found[0]["fitRect"], json!([150.0, 100.0, 100.0, 150.0]));
    let padded = entry(&mut s, 80_202);
    assert_eq!(padded["ppi"], json!([144.0, 144.0]), "{padded}");
    assert_eq!(padded["aspect"], json!({"source": 2.0, "shown": 2.0}), "{padded}");
    assert!(findings(&mut s, json!({}), 80_202).is_empty());
}

/// `fit: "cover"` keeps the box and crops the picture to its aspect: a 4:3 image in a 2.8:1 box
/// keeps the middle 1.333 / 2.8 = 0.476 of its height, so (1 - 0.476) / 2 = 0.262 comes off the
/// top and the bottom. Nothing is stretched. 400 px across 420 pt is still 68.57 ppi, and the
/// 142.9 px left of the height over 150 pt gives the same.
#[test]
fn fit_cover_crops_the_picture_to_the_box() {
    let mut s = Session::with_new();
    let id = insert(&mut s, 400, 300, json!({"rect": [40, 130, 420, 150], "fit": "cover"}));
    let (x, crop) = stored(&s, id);
    assert_box(&x, [40.0, 130.0, 420.0, 150.0]);
    let c = (1.0 - (4.0 / 3.0) / 2.8) / 2.0;
    assert_near(crop, [0.0, c, 0.0, c]);
    assert_eq!(entry(&mut s, id)["aspect"], json!({"source": 2.8, "shown": 2.8}));
    let found = findings(&mut s, json!({}), id);
    assert_eq!(kinds(&found), ["lowResolution"], "{found:?}");
    assert_eq!(found[0]["ppi"], json!([68.57, 68.57]));
    assert_eq!(found[0]["distortion"], json!(0.0));
}

/// `fit: "contain"` shrinks the box to the picture's aspect, centred in the box asked for: 4:3 at
/// 150 pt high is 200 pt wide, 110 pt in from each side of 420, and 4:3 at 300 pt wide is 225 pt
/// high, 37.5 pt down from the top of a 300 pt square.
#[test]
fn fit_contain_shrinks_the_box_to_the_picture() {
    let mut s = Session::with_new();
    let id = insert(&mut s, 400, 300, json!({"rect": [40, 130, 420, 150], "fit": "contain"}));
    let (x, crop) = stored(&s, id);
    assert_box(&x, [150.0, 130.0, 200.0, 150.0]);
    assert_near(crop, [0.0; 4]);
    assert_eq!(entry(&mut s, id)["ppi"], json!([144.0, 144.0]));
    assert!(findings(&mut s, json!({}), id).is_empty());
    let tall = insert(&mut s, 400, 300, json!({"rect": [600, 100, 300, 300], "fit": "contain"}));
    assert_box(&stored(&s, tall).0, [600.0, 137.5, 300.0, 225.0]);
}

/// Without `fit`, insert.picture does what it did: an explicit box is filled as given, and a
/// selected empty placeholder filled without a box crops the picture to the placeholder's
/// aspect. The Picture with Caption layout's picture placeholder is narrower than 4:3, so equal
/// parts come off the left and right. `fit: "cover"` asks for the same; `fit: "stretch"` fills
/// the placeholder uncropped.
#[test]
fn without_fit_boxes_stretch_and_placeholders_crop_as_before() {
    let mut s = Session::with_new();
    let id = insert(&mut s, 400, 300, json!({"rect": [40, 130, 420, 150]}));
    let (x, crop) = stored(&s, id);
    assert_box(&x, [40.0, 130.0, 420.0, 150.0]);
    assert_near(crop, [0.0; 4]);
    assert_eq!(entry(&mut s, id)["aspect"]["shown"], json!(2.8));
    for fit in [None, Some("cover"), Some("stretch")] {
        s.execute("slide.new", &json!({"layout": "pictureWithCaption"})).unwrap();
        let slide = s.execute("slide.inspect", &json!({})).unwrap();
        let ph = slide["shapes"].as_array().unwrap().iter().find(|e| e["placeholder"] == "pic").cloned().unwrap_or_else(|| panic!("{slide}"));
        let ph_id = ph["id"].as_u64().unwrap();
        s.execute("edit.select", &json!({"ids": [ph_id]})).unwrap();
        let id = insert(&mut s, 400, 300, fit.map_or(json!({}), |f| json!({"fit": f})));
        assert_eq!(id, ph_id, "{fit:?}: the picture fills the placeholder");
        let e = entry(&mut s, id);
        assert_eq!(e["box"], ph["box"], "{fit:?}: {e}");
        let (x, crop) = stored(&s, id);
        let shown = x.w / x.h;
        assert_eq!(e["aspect"]["shown"].as_f64(), Some((shown * 1000.0).round() / 1000.0), "{fit:?}: {e}");
        if fit == Some("stretch") {
            assert_near(crop, [0.0; 4]);
            assert_eq!(e["aspect"]["source"], json!(1.333), "{e}");
        } else {
            let c = (1.0 - shown / (4.0 / 3.0)) / 2.0;
            assert_near(crop, [c, 0.0, c, 0.0]);
            assert_eq!(e["aspect"]["source"], e["aspect"]["shown"], "{fit:?}: {e}");
        }
    }
}

/// A picture in a group is measured as drawn. The group draws its 72 × 72 child space into a
/// 720 × 360 pt box, so the member's 72 × 72 box is 720 pt wide and 360 pt high on the slide: a
/// 400 × 300 px image there is 40 ppi across and 60 down, shown at 2:1 against 4:3 (distortion
/// 2 / 1.333 - 1 = 0.5). fitRect is in the member's own coordinates, which shape.setBounds
/// takes: the stored aspect that shows 4:3 is 1.333 / 2 = 0.667, 48 wide in the 72 square, 12
/// in from each side.
#[test]
fn a_picture_in_a_scaled_group_is_measured_as_drawn() {
    let mut s = Session::with_new();
    let image = png(400, 300);
    s.edit(|doc, _| {
        let media = doc.add_media("grouped.png", "image/png", image.clone()).ok_or_else(|| bad("test", "media"))?;
        let picture = Shape {
            id: ShapeId(80_301),
            xfrm: Some(Xfrm::new(0.0, 0.0, 72.0, 72.0)),
            kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
            ..Default::default()
        };
        let group = Shape {
            id: ShapeId(80_302),
            xfrm: Some(Xfrm::new(100.0, 50.0, 720.0, 360.0)),
            kind: ShapeKind::Group { children: vec![picture], child: Xfrm::new(0.0, 0.0, 72.0, 72.0) },
            ..Default::default()
        };
        let slide = doc.slides.first_mut().ok_or_else(|| bad("test", "slide"))?;
        Arc::make_mut(slide).shapes.push(group);
        Ok(())
    })
    .unwrap();
    let facts = json!({"pixels": [400, 300], "ppi": [40.0, 60.0], "aspect": {"source": 1.333, "shown": 2.0}});
    let e = entry(&mut s, 80_301);
    for key in ["pixels", "ppi", "aspect"] {
        assert_eq!(e[key], facts[key], "{key}: {e}");
    }
    assert_eq!(s.execute("shape.inspect", &json!({"id": 80_301})).unwrap()["picture"], facts);
    let found = findings(&mut s, json!({}), 80_301);
    assert_eq!(kinds(&found), ["stretched", "lowResolution"], "{found:?}");
    assert_eq!(found[0]["distortion"], json!(0.5));
    assert_eq!(found[0]["fitRect"], json!([12.0, 0.0, 48.0, 72.0]));
    // Applied, the group keeps its scale: 48 × 72 child units are 480 × 360 pt, 60 ppi both ways.
    s.execute("shape.setBounds", &json!({"ids": [80_301], "x": 12, "y": 0, "w": 48, "h": 72})).unwrap();
    let e = entry(&mut s, 80_301);
    assert_eq!(e["ppi"], json!([60.0, 60.0]), "{e}");
    assert_eq!(e["aspect"]["shown"], json!(1.333), "{e}");
    assert_eq!(kinds(&findings(&mut s, json!({}), 80_301)), ["lowResolution"]);
}

/// Extreme and missing sizes give finite answers. A 1 × 60000 px PNG in a 100 pt square is 0.72
/// ppi across and 43200 down, drawn 60000 times as wide as it is (distortion 59999), and the box
/// that fits it is 0.0017 pt wide (0 at two decimals). A PNG whose header states a width of 0,
/// and SVG markup, give no pixel size: pixels, ppi and the source aspect are null, and
/// review.pictures leaves them out.
#[test]
fn extreme_and_missing_pixel_sizes_give_finite_facts() {
    let mut s = Session::with_new();
    let id = insert(&mut s, 1, 60_000, json!({"rect": [100, 100, 100, 100]}));
    let e = entry(&mut s, id);
    assert_eq!(e["pixels"], json!([1, 60_000]), "{e}");
    assert_eq!(e["ppi"], json!([0.72, 43_200.0]), "{e}");
    assert_eq!(e["aspect"], json!({"source": 0.0, "shown": 1.0}), "{e}");
    let found = findings(&mut s, json!({}), id);
    assert_eq!(kinds(&found), ["stretched", "lowResolution"], "{found:?}");
    assert_eq!(found[0]["distortion"], json!(59_999.0));
    assert_eq!(found[0]["fitRect"], json!([150.0, 100.0, 0.0, 100.0]));

    // The IHDR chunk's width is bytes 16 to 19 of a PNG.
    let mut zero_width = png(4, 4);
    zero_width[16..20].copy_from_slice(&0u32.to_be_bytes());
    add_pictures(&mut s, &zero_width, "image/png", [0.0; 4], &[(80_401, Xfrm::new(100.0, 250.0, 200.0, 100.0))]);
    let svg = b"<svg xmlns='http://www.w3.org/2000/svg' width='40' height='30'/>";
    add_pictures(&mut s, svg, "image/svg+xml", [0.0; 4], &[(80_402, Xfrm::new(400.0, 250.0, 200.0, 100.0))]);
    for id in [80_401, 80_402] {
        let e = entry(&mut s, id);
        assert_eq!(e["pixels"], Value::Null, "{e}");
        assert_eq!(e["ppi"], Value::Null, "{e}");
        assert_eq!(e["aspect"], json!({"source": null, "shown": 2.0}), "{e}");
        let inspected = s.execute("shape.inspect", &json!({"id": id})).unwrap();
        assert_eq!(inspected["picture"], json!({"pixels": null, "ppi": null, "aspect": {"source": null, "shown": 2.0}}), "{inspected}");
        assert!(findings(&mut s, json!({"minPpi": 1200, "maxDistortion": 0}), id).is_empty());
    }
}

/// Thresholds out of range or of the wrong type, and fits the command doesn't know, are refused
/// with the key named, and a refused insert adds nothing. Null means the default. contain and
/// cover need a pixel size, which SVG doesn't state; stretch and null take SVG as before.
#[test]
fn review_and_fit_refuse_bad_values_with_the_key_named() {
    let mut s = Session::with_new();
    let id = insert(&mut s, 48, 48, json!({"rect": [520, 120, 380, 380]}));
    for (p, message) in [
        (json!({"minPpi": -1}), "`minPpi` must be a number from 0 to 1200, not -1"),
        (json!({"minPpi": 1e300}), "`minPpi` must be a number from 0 to 1200, not 1e+300"),
        (json!({"minPpi": "96"}), "`minPpi` must be a number from 0 to 1200, not \"96\""),
        (json!({"maxDistortion": -0.5}), "`maxDistortion` must be a number from 0 to 100, not -0.5"),
        (json!({"maxDistortion": [0.1]}), "`maxDistortion` must be a number from 0 to 100, not [0.1]"),
        (json!({"maxDistortion": 101}), "`maxDistortion` must be a number from 0 to 100, not 101"),
    ] {
        let err = s.execute("review.pictures", &p).unwrap_err().to_string();
        assert!(err.contains(message), "{p}: {err}");
    }
    assert_eq!(kinds(&findings(&mut s, json!({"minPpi": null, "maxDistortion": null}), id)), ["lowResolution"]);

    let count = |s: &Session| s.doc().unwrap().doc.slides[0].shapes.len();
    let before = count(&s);
    let data = base64_encode(&png(40, 30));
    for (fit, message) in [
        (json!("fill"), "`fit` must be stretch, contain or cover, not \"fill\""),
        (json!("Cover"), "`fit` must be stretch, contain or cover, not \"Cover\""),
        (json!(1), "`fit` must be stretch, contain or cover, not 1"),
        (json!(["cover"]), "`fit` must be stretch, contain or cover, not [\"cover\"]"),
    ] {
        let p = json!({"data": data, "name": "p.png", "rect": [0, 0, 100, 100], "fit": fit});
        let err = s.execute("insert.picture", &p).unwrap_err().to_string();
        assert!(err.contains(message), "{fit}: {err}");
    }
    let svg = base64_encode(b"<svg xmlns='http://www.w3.org/2000/svg' width='40' height='30'/>");
    for fit in ["contain", "cover"] {
        let p = json!({"data": svg, "name": "p.svg", "rect": [0, 0, 100, 100], "fit": fit});
        let err = s.execute("insert.picture", &p).unwrap_err().to_string();
        assert!(err.contains("`fit` contain and cover keep the picture's aspect"), "{fit}: {err}");
    }
    assert_eq!(count(&s), before, "a refused insert adds nothing");
    for fit in [json!("stretch"), Value::Null] {
        s.execute("insert.picture", &json!({"data": svg, "name": "p.svg", "rect": [0, 0, 100, 100], "fit": fit})).unwrap();
    }
    s.execute("insert.picture", &json!({"data": data, "name": "p.png", "rect": [0, 0, 100, 100], "fit": null})).unwrap();
    assert_eq!(count(&s), before + 3);
}
