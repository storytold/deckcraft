//! Cameo round trips: the `alf:liveFeedProps` marker and the `asvg:svgBlip` placeholder.
//!
//! The decks are built in code, never produced by PowerPoint: an SVG placeholder we draw and an
//! ordinary PNG for the checked-in-picture case.

use std::io::{Cursor, Read, Write};

use deckcraft_geom::Xfrm;
use deckcraft_model::style::PictureFill;
use deckcraft_model::{Cameo, CameraBackground, CameraBackgroundMode, Geom, Presentation, Shape, ShapeId, ShapeKind, defaults};

fn svg() -> Vec<u8> {
    br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" rx="8" fill="#33404f"/><circle cx="16" cy="16" r="7" fill="#8fd3ff"/></svg>"##.to_vec()
}

fn png() -> Vec<u8> {
    let img = image::RgbImage::from_pixel(4, 4, image::Rgb([10, 200, 30]));
    let mut out = Cursor::new(vec![]);
    image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).expect("png");
    out.into_inner()
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut f = z.by_name(name).expect("part");
    let mut s = String::new();
    f.read_to_string(&mut s).expect("read");
    s
}

fn deck(media: &str, bytes: Vec<u8>) -> Presentation {
    let mut p = defaults::new_presentation(None);
    let m = p.add_media(media, if media.ends_with(".svg") { "image/svg+xml" } else { "image/png" }, bytes).expect("media");
    let shape = Shape {
        id: ShapeId(p.alloc_id().expect("id")),
        name: "Camera 1".into(),
        xfrm: Some(Xfrm::new(700.0, 360.0, 162.0, 162.0)),
        kind: ShapeKind::Picture { fill: PictureFill { media: m, ..Default::default() } },
        geom: Geom::preset("ellipse"),
        cameo: Some(Cameo::live()),
        ..Default::default()
    };
    if let Some(s) = p.slides.first_mut() {
        std::sync::Arc::make_mut(s).shapes.push(shape);
    }
    p
}

/// `asvg:svgBlip` carries `r:embed`, which may only name a part inside the package: an SVG that
/// lives outside it must be written as an ordinary blip with `r:link`, or PowerPoint repairs the
/// file.
#[test]
fn a_linked_svg_cameo_placeholder_is_a_link_not_an_embed() {
    let mut p = deck("cameo-placeholder.svg", Vec::new());
    for m in &mut p.media {
        m.link = Some("file:///tmp/cameo-placeholder.svg".into());
    }
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    assert!(!xml.contains("asvg:svgBlip"), "an external SVG has no part to embed: {xml}");
    assert!(xml.contains("r:link="), "the blip names the external file: {xml}");
}

#[test]
fn cameo_round_trip_writes_the_live_feed_marker_and_svg_blip() {
    let p = deck("cameo-placeholder.svg", svg());
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    assert!(
        xml.contains(
            "<p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/><a:extLst><a:ext uri=\"{51228E76-BA90-4043-B771-695A4F85340A}\"><alf:liveFeedProps xmlns:alf=\"http://schemas.microsoft.com/office/drawing/2021/livefeed\"/></a:ext></a:extLst></p:cNvPicPr>"
        ),
        "{xml}"
    );
    assert!(
        xml.contains(
            "<a:blip><a:extLst><a:ext uri=\"{96DAC541-7B7A-43D3-8B79-37D633B846F1}\"><asvg:svgBlip xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\" r:embed=\"rId"
        ),
        "{xml}"
    );
    // The placeholder is its own image part, related from the slide.
    assert!(xml.contains("<a:prstGeom prst=\"ellipse\">"), "{xml}");
    assert!(part(&bytes, "ppt/slides/_rels/slide1.xml.rels").contains("image1.svg"));

    let q = deckcraft_pptx::import(&bytes).expect("import");
    let mut found = None;
    deckcraft_model::walk(&q.slides[0].shapes, &mut |sh, _| {
        if sh.name == "Camera 1" {
            found = Some(sh.clone());
        }
    });
    let sh = found.expect("cameo shape");
    assert!(sh.is_cameo());
    assert_eq!(sh.cameo.as_ref().map(|c| c.live), Some(true));
    assert_eq!(sh.geom.preset_name(), Some("ellipse"));
    let media = match &sh.kind {
        ShapeKind::Picture { fill } => fill.media,
        _ => panic!("picture"),
    };
    assert_eq!(q.media(media).map(|m| m.content_type.as_str()), Some("image/svg+xml"));
}

#[test]
fn cameo_with_a_raster_placeholder_keeps_the_marker() {
    let p = deck("still.png", png());
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    assert!(xml.contains("<alf:liveFeedProps"), "{xml}");
    assert!(xml.contains("<a:blip r:embed=\"rId"), "{xml}");
    let q = deckcraft_pptx::import(&bytes).expect("import");
    let mut found = None;
    deckcraft_model::walk(&q.slides[0].shapes, &mut |sh, _| {
        if sh.name == "Camera 1" {
            found = Some(sh.clone());
        }
    });
    assert!(found.expect("cameo").is_cameo());
}

#[test]
fn a_camera_background_survives_a_round_trip() {
    let raw = concat!(
        "<alf:liveFeedProps xmlns:alf=\"http://schemas.microsoft.com/office/drawing/2021/livefeed\">",
        "<alf:backgroundProps><alf:Blur/></alf:backgroundProps>",
        "</alf:liveFeedProps>"
    );
    let mut p = deck("cameo-placeholder.svg", svg());
    if let Some(s) = p.slides.first_mut() {
        for sh in &mut std::sync::Arc::make_mut(s).shapes {
            if let Some(c) = &mut sh.cameo {
                c.raw = Some(raw.to_string());
            }
        }
    }
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    assert!(xml.contains("<alf:backgroundProps><alf:Blur/></alf:backgroundProps>"), "{xml}");

    let q = deckcraft_pptx::import(&bytes).expect("import");
    let mut found = None;
    deckcraft_model::walk(&q.slides[0].shapes, &mut |sh, _| {
        if sh.name == "Camera 1" {
            found = Some(sh.clone());
        }
    });
    let kept = found.expect("cameo").cameo.and_then(|c| c.raw).expect("kept feed");
    assert!(kept.contains("backgroundProps") && kept.contains("Blur"), "{kept}");

    // A second save writes the same settings back.
    let bytes = deckcraft_pptx::export(&q).expect("export");
    assert!(part(&bytes, "ppt/slides/slide1.xml").contains("backgroundProps"));
}

#[test]
fn an_ordinary_svg_picture_is_not_a_cameo() {
    let mut p = deck("plain.svg", svg());
    if let Some(s) = p.slides.first_mut() {
        let slide = std::sync::Arc::make_mut(s);
        for sh in &mut slide.shapes {
            sh.cameo = None;
        }
    }
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    assert!(!xml.contains("liveFeedProps"), "{xml}");
    assert!(xml.contains("<a:blip r:embed=\"rId"), "{xml}");
}

/// Some generators declare the feed's `alf` prefix on the enclosing `a:ext` instead of the feed
/// element. The kept feed must carry that declaration, or the exported slide holds an unbound
/// prefix and PowerPoint offers to repair it.
#[test]
fn a_feed_whose_prefix_is_bound_on_the_wrapper_exports_bound() {
    let bytes = deckcraft_pptx::export(&deck("cameo-placeholder.svg", svg())).expect("export");
    let mut src = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..src.len() {
        let mut f = src.by_index(i).expect("entry");
        let name = f.name().to_string();
        let mut data = Vec::new();
        f.read_to_end(&mut data).expect("read");
        if name == "ppt/slides/slide1.xml" {
            let xml = String::from_utf8(data).expect("utf8");
            let moved = xml.replacen(
                "<a:ext uri=\"{51228E76-BA90-4043-B771-695A4F85340A}\"><alf:liveFeedProps xmlns:alf=\"http://schemas.microsoft.com/office/drawing/2021/livefeed\"/>",
                "<a:ext uri=\"{51228E76-BA90-4043-B771-695A4F85340A}\" xmlns:alf=\"http://schemas.microsoft.com/office/drawing/2021/livefeed\"><alf:liveFeedProps/>",
                1,
            );
            assert_ne!(moved, xml, "the exported feed is rewritten onto the wrapper");
            data = moved.into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).expect("start");
        out.write_all(&data).expect("write");
    }
    let bytes = out.finish().expect("finish").into_inner();

    let q = deckcraft_pptx::import(&bytes).expect("import");
    let mut found = None;
    deckcraft_model::walk(&q.slides[0].shapes, &mut |sh, _| {
        if sh.name == "Camera 1" {
            found = Some(sh.clone());
        }
    });
    let kept = found.expect("cameo").cameo.and_then(|c| c.raw).expect("kept feed");
    assert!(kept.contains("xmlns:alf=\"http://schemas.microsoft.com/office/drawing/2021/livefeed\""), "{kept}");

    let bytes = deckcraft_pptx::export(&q).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    assert!(
        xml.contains(
            "<a:ext uri=\"{51228E76-BA90-4043-B771-695A4F85340A}\"><alf:liveFeedProps xmlns:alf=\"http://schemas.microsoft.com/office/drawing/2021/livefeed\"/></a:ext>"
        ),
        "{xml}"
    );
    assert!(!xml.contains("<alf:liveFeedProps/>"), "no unbound prefix: {xml}");
}

fn cameo_with(background: Option<CameraBackground>, raw: Option<&str>) -> Presentation {
    let mut p = deck("cameo-placeholder.svg", svg());
    if let Some(s) = p.slides.first_mut() {
        for sh in &mut std::sync::Arc::make_mut(s).shapes {
            if let Some(c) = &mut sh.cameo {
                c.background = background.clone();
                c.raw = raw.map(str::to_string);
            }
        }
    }
    p
}

fn exported_shape(bytes: &[u8]) -> shape_import::Found {
    shape_import::find(bytes)
}

mod shape_import {
    //! The imported cameo, found by name, plus the slide XML that produced it.
    use deckcraft_model::{Presentation, Shape};

    pub struct Found {
        pub presentation: Presentation,
        pub shape: Shape,
        pub slide_xml: String,
    }

    pub fn find(bytes: &[u8]) -> Found {
        let presentation = deckcraft_pptx::import(bytes).expect("import");
        let mut shape = None;
        deckcraft_model::walk(&presentation.slides[0].shapes, &mut |sh, _| {
            if sh.name == "Camera 1" {
                shape = Some(sh.clone());
            }
        });
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("zip");
        let mut f = z.by_name("ppt/slides/slide1.xml").expect("slide");
        use std::io::Read;
        let mut slide_xml = String::new();
        f.read_to_string(&mut slide_xml).expect("read");
        Found { presentation, shape: shape.expect("cameo shape"), slide_xml }
    }
}

#[test]
fn every_mode_round_trips_through_the_structured_field() {
    for mode in [CameraBackgroundMode::Normal, CameraBackgroundMode::Removed, CameraBackgroundMode::Blur, CameraBackgroundMode::Custom] {
        let p = cameo_with(Some(CameraBackground::new(mode)), None);
        let bytes = deckcraft_pptx::export(&p).expect("export");
        let found = exported_shape(&bytes);
        let expected = format!("<alf:backgroundProps><alf:{}/></alf:backgroundProps>", mode.xml_name());
        assert!(found.slide_xml.contains(&expected), "{mode:?}: {}", found.slide_xml);
        let bg = found.shape.cameo.and_then(|c| c.background).expect("structured background");
        assert_eq!(bg.mode, mode);
        // A second save keeps it.
        let bytes = deckcraft_pptx::export(&found.presentation).expect("export again");
        assert!(exported_shape(&bytes).slide_xml.contains(&expected));
    }
}

#[test]
fn an_imported_background_props_becomes_structured_and_survives_editing_another_mode() {
    let raw = concat!(
        "<alf:liveFeedProps xmlns:alf=\"http://schemas.microsoft.com/office/drawing/2021/livefeed\">",
        "<alf:backgroundProps><alf:Blur/></alf:backgroundProps>",
        "</alf:liveFeedProps>"
    );
    let p = cameo_with(None, Some(raw));
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let found = exported_shape(&bytes);
    assert_eq!(found.shape.cameo.and_then(|c| c.background).map(|b| b.mode), Some(CameraBackgroundMode::Blur), "{}", found.slide_xml);

    // Choosing Removed overrides the imported element rather than sitting beside it.
    let mut p = found.presentation;
    for sh in &mut std::sync::Arc::make_mut(&mut p.slides[0]).shapes {
        if let Some(c) = &mut sh.cameo {
            c.background = Some(CameraBackground::new(CameraBackgroundMode::Removed));
        }
    }
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = exported_shape(&bytes).slide_xml;
    assert!(xml.contains("<alf:backgroundProps><alf:Removed/></alf:backgroundProps>"), "{xml}");
    assert!(!xml.contains("<alf:Blur/>"), "{xml}");
}

#[test]
fn unknown_feed_children_survive_a_background_change() {
    let raw = concat!(
        "<alf:liveFeedProps xmlns:alf=\"http://schemas.microsoft.com/office/drawing/2021/livefeed\" ",
        "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">",
        "<alf:extLst><a:ext uri=\"urn:example:kept\"/></alf:extLst>",
        "</alf:liveFeedProps>"
    );
    let p = cameo_with(Some(CameraBackground::new(CameraBackgroundMode::Blur)), Some(raw));
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = exported_shape(&bytes).slide_xml;
    assert!(xml.contains("<alf:backgroundProps><alf:Blur/></alf:backgroundProps>"), "{xml}");
    assert!(xml.contains("<a:ext uri=\"urn:example:kept\"/>"), "{xml}");
}

#[test]
fn unknown_children_of_background_props_survive() {
    let raw = concat!(
        "<alf:liveFeedProps xmlns:alf=\"http://schemas.microsoft.com/office/drawing/2021/livefeed\" ",
        "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">",
        "<alf:backgroundProps><alf:Blur/><a:extLst><a:ext uri=\"urn:example:bg\"/></a:extLst></alf:backgroundProps>",
        "</alf:liveFeedProps>"
    );
    let p = cameo_with(Some(CameraBackground::new(CameraBackgroundMode::Custom)), Some(raw));
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = exported_shape(&bytes).slide_xml;
    assert!(xml.contains("<alf:Custom/>"), "{xml}");
    assert!(!xml.contains("<alf:Blur/>"), "{xml}");
    assert!(xml.contains("<a:ext uri=\"urn:example:bg\"/>"), "{xml}");
}

/// A feed that names a package relationship (`r:embed`, `r:link`, `r:id`) cannot be written back:
/// the export creates no such relationship, so PowerPoint would repair a dangling reference. The
/// feed is replaced by one carrying only the mode, and the exported slide holds no such id.
#[test]
fn a_feed_that_names_a_relationship_exports_only_the_mode() {
    for attr in ["r:embed", "r:link", "r:id"] {
        for background in [None, Some(CameraBackground::new(CameraBackgroundMode::Blur))] {
            let raw = format!(
                concat!(
                    "<alf:liveFeedProps xmlns:alf=\"http://schemas.microsoft.com/office/drawing/2021/livefeed\" ",
                    "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">",
                    "<alf:backgroundProps><alf:Custom {}=\"rId9\"/></alf:backgroundProps>",
                    "</alf:liveFeedProps>"
                ),
                attr
            );
            let p = cameo_with(background.clone(), Some(&raw));
            let bytes = deckcraft_pptx::export(&p).expect("export");
            let found = exported_shape(&bytes);
            assert!(!found.slide_xml.contains("rId9"), "{attr}/{background:?}: {}", found.slide_xml);
            let kept = found.shape.cameo.and_then(|c| c.raw).unwrap_or_default();
            assert!(!kept.contains("rId9"), "{attr}/{background:?}: {kept}");
            match &background {
                Some(bg) => {
                    let expected = format!("<alf:backgroundProps><alf:{}/></alf:backgroundProps>", bg.mode.xml_name());
                    assert!(found.slide_xml.contains(&expected), "{attr}: {}", found.slide_xml);
                }
                None => assert!(!kept.contains("<alf:Custom"), "{attr}: {kept}"),
            }
        }
    }
}
