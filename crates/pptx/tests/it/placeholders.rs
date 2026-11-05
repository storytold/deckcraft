//! Placeholder inheritance in decks written by PowerPoint.

use std::io::{Cursor, Read, Write};

use deckcraft_geom::Xfrm;
use deckcraft_model::style::PictureFill;
use deckcraft_model::{Geom, PhType, Placeholder, Shape, ShapeId, ShapeKind, defaults};

/// `bytes` with `edit` applied to every slide part.
fn edit_slides(bytes: &[u8], edit: impl Fn(String) -> String) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let mut d = vec![];
        f.read_to_end(&mut d).expect("read");
        let name = f.name().to_string();
        if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
            d = edit(String::from_utf8(d).expect("utf8")).into_bytes();
        }
        out.start_file(name, o).expect("start");
        out.write_all(&d).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

#[test]
fn a_placeholder_without_geometry_takes_its_layouts() {
    // PowerPoint leaves a slide placeholder's spPr empty and draws it with the layout
    // placeholder's geometry (a picture placeholder cut into a shape keeps the shape).
    let mut p = defaults::new_presentation(None);
    let layout = p.slides[0].layout;
    let m = std::sync::Arc::make_mut(&mut p.masters[0]);
    let l = m.layouts.iter_mut().find(|l| l.id == layout).expect("layout");
    for s in &mut l.shapes {
        if s.ph.is_some() {
            s.geom = Geom::Preset { name: "ellipse".into(), adj: vec![] };
        }
    }
    let bytes = deckcraft_pptx::export(&p).expect("export");
    // Strip the slide's own geometry, as PowerPoint writes inheriting placeholders.
    let bare = edit_slides(&bytes, |s| {
        let mut s = s;
        while let Some(i) = s.find("<a:prstGeom") {
            let Some(j) = s[i..].find("</a:prstGeom>").map(|j| j + i + "</a:prstGeom>".len()) else { break };
            s.replace_range(i..j, "");
        }
        s
    });
    let q = deckcraft_pptx::import(&bare).expect("import");
    let phs: Vec<_> = q.slides[0].shapes.iter().filter(|s| s.ph.is_some()).collect();
    assert!(!phs.is_empty());
    for s in phs {
        assert!(matches!(&s.geom, Geom::Preset { name, .. } if name == "ellipse"), "{:?}", s.geom);
    }
    // A placeholder with its own geometry keeps it.
    let first = p.slides[0].id;
    for s in &mut p.slide_mut(first).expect("slide").shapes {
        if s.ph.is_some() {
            s.geom = Geom::Preset { name: "triangle".into(), adj: vec![] };
        }
    }
    let q = deckcraft_pptx::import(&deckcraft_pptx::export(&p).expect("export")).expect("import");
    assert!(q.slides[0].shapes.iter().filter(|s| s.ph.is_some()).all(|s| matches!(&s.geom, Geom::Preset { name, .. } if name == "triangle")));
}

#[test]
fn a_picture_placeholder_without_geometry_takes_its_layouts() {
    // The slide's picture sits in a layout's picture placeholder drawn as an ellipse; PowerPoint
    // writes its spPr with a frame only and shows the picture cut into the circle.
    let mut p = defaults::new_presentation(None);
    let img = {
        let mut out = Cursor::new(vec![]);
        let px = image::RgbImage::from_pixel(4, 4, image::Rgb([10, 200, 30]));
        image::DynamicImage::ImageRgb8(px).write_to(&mut out, image::ImageFormat::Png).expect("png");
        p.add_media("image1.png", "image/png", out.into_inner()).expect("media")
    };
    let ph = Placeholder { kind: PhType::Picture, idx: 15, ..Default::default() };
    let layout = p.slides[0].layout;
    let slot = Shape {
        id: ShapeId(p.alloc_id().expect("ids")),
        name: "Picture Placeholder".into(),
        xfrm: Some(Xfrm::new(100.0, 100.0, 200.0, 200.0)),
        geom: Geom::Preset { name: "ellipse".into(), adj: vec![] },
        ph: Some(ph.clone()),
        ..Default::default()
    };
    std::sync::Arc::make_mut(&mut p.masters[0]).layouts.iter_mut().find(|l| l.id == layout).expect("layout").shapes.push(slot);
    let pic = Shape {
        id: ShapeId(p.alloc_id().expect("ids")),
        name: "Photo".into(),
        xfrm: Some(Xfrm::new(100.0, 100.0, 200.0, 200.0)),
        kind: ShapeKind::Picture { fill: PictureFill { media: img, ..Default::default() } },
        ph: Some(ph),
        ..Default::default()
    };
    std::sync::Arc::make_mut(&mut p.slides[0]).shapes = vec![pic];
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let bare = edit_slides(&bytes, |mut s| {
        while let Some(i) = s.find("<a:prstGeom") {
            let Some(j) = s[i..].find("</a:prstGeom>").map(|j| j + i + "</a:prstGeom>".len()) else { break };
            s.replace_range(i..j, "");
        }
        s
    });
    let q = deckcraft_pptx::import(&bare).expect("import");
    let photo = q.slides[0].shapes.iter().find(|s| matches!(s.kind, ShapeKind::Picture { .. })).expect("picture");
    assert!(matches!(&photo.geom, Geom::Preset { name, .. } if name == "ellipse"), "{:?}", photo.geom);
}

#[test]
fn an_orphan_placeholder_takes_nothing_from_a_layout_picture_placeholder() {
    // A layout whose only placeholder is an ellipse picture slot, and a slide placeholder with the
    // orphan idx 4294967295 (no counterpart in any layout). PowerPoint draws the latter without the
    // ellipse, with the master's geometry; one with the picture slot's idx 15 still takes the ellipse.
    let mut p = defaults::new_presentation(None);
    let layout = p.slides[0].layout;
    let slot = Shape {
        id: ShapeId(p.alloc_id().expect("ids")),
        name: "Picture Placeholder".into(),
        xfrm: Some(Xfrm::new(100.0, 100.0, 200.0, 200.0)),
        geom: Geom::Preset { name: "ellipse".into(), adj: vec![] },
        ph: Some(Placeholder { kind: PhType::Picture, idx: 15, ..Default::default() }),
        ..Default::default()
    };
    let m = std::sync::Arc::make_mut(&mut p.masters[0]);
    let l = m.layouts.iter_mut().find(|l| l.id == layout).expect("layout");
    l.shapes = vec![slot];
    let mut content = |name: &str, idx: u32| Shape {
        id: ShapeId(p.alloc_id().expect("ids")),
        name: name.into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 300.0, 100.0)),
        ph: Some(Placeholder { idx, ..Default::default() }),
        ..Default::default()
    };
    let shapes = vec![content("Orphan", u32::MAX), content("Matching", 15), content("Unpaired", 7), content("Bare", 0)];
    std::sync::Arc::make_mut(&mut p.slides[0]).shapes = shapes;
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let bare = edit_slides(&bytes, |mut s| {
        while let Some(i) = s.find("<a:prstGeom") {
            let Some(j) = s[i..].find("</a:prstGeom>").map(|j| j + i + "</a:prstGeom>".len()) else { break };
            s.replace_range(i..j, "");
        }
        s
    });
    let q = deckcraft_pptx::import(&bare).expect("import");
    let geom_of = |name: &str| &q.slides[0].shapes.iter().find(|s| s.name == name).expect("shape").geom;
    assert!(!matches!(geom_of("Orphan"), Geom::Preset { name, .. } if name == "ellipse"), "{:?}", geom_of("Orphan"));
    assert!(matches!(geom_of("Matching"), Geom::Preset { name, .. } if name == "ellipse"), "{:?}", geom_of("Matching"));
    // An idx no layout placeholder has takes no geometry from anywhere (a red fill is not drawn on
    // it in PowerPoint); it keeps the empty geometry rather than becoming a rectangle.
    assert!(matches!(geom_of("Unpaired"), Geom::Custom { paths } if paths.is_empty()), "{:?}", geom_of("Unpaired"));
    // So does idx 0 in a layout with no placeholder of idx 0: nothing pairs by type.
    assert!(matches!(geom_of("Bare"), Geom::Custom { paths } if paths.is_empty()), "{:?}", geom_of("Bare"));
}

#[test]
fn an_unpaired_placeholder_with_a_fill_keeps_its_geometry_unset_through_a_save() {
    // A placeholder no layout placeholder pairs with has no geometry, so a fill of its own is not
    // drawn; saving must not give it a rectangle that draws it.
    let mut p = defaults::new_presentation(None);
    let shape = Shape {
        id: ShapeId(p.alloc_id().expect("ids")),
        name: "Unpaired".into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 300.0, 100.0)),
        geom: Geom::Custom { paths: vec![] },
        fill: Some(deckcraft_model::Fill::solid(deckcraft_model::ColorRef::rgb(deckcraft_color::Rgba::rgb(255, 0, 0)))),
        ph: Some(Placeholder { kind: PhType::Obj, idx: 7, ..Default::default() }),
        ..Default::default()
    };
    std::sync::Arc::make_mut(&mut p.slides[0]).shapes = vec![shape];
    let q = deckcraft_pptx::import(&deckcraft_pptx::export(&p).expect("export")).expect("import");
    let back = q.slides[0].shapes.iter().find(|s| s.name == "Unpaired").expect("shape");
    assert!(matches!(&back.geom, Geom::Custom { paths } if paths.is_empty()), "{:?}", back.geom);
    assert!(back.fill.is_some(), "the fill itself is kept");
}
