//! Cameo round trips: the `alf:liveFeedProps` marker and the `asvg:svgBlip` placeholder.
//!
//! The decks are built in code, never produced by PowerPoint: an SVG placeholder we draw and an
//! ordinary PNG for the checked-in-picture case.

use std::io::{Cursor, Read};

use deckcraft_geom::Xfrm;
use deckcraft_model::style::PictureFill;
use deckcraft_model::{Cameo, Geom, Presentation, Shape, ShapeId, ShapeKind, defaults};

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
    assert_eq!(sh.cameo.as_ref().and_then(|c| c.device.as_deref()), None);
    assert_eq!(sh.cameo.as_ref().and_then(|c| c.recorded), None);
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
fn an_svg_cameo_keeps_its_blip_adjustments() {
    // A grayscale SVG Cameo must write `a:grayscl` inside the blip (before its `a:extLst`) and
    // read it back, like the raster form does.
    let mut p = deck("cameo-placeholder.svg", svg());
    if let Some(s) = p.slides.first_mut() {
        for sh in &mut std::sync::Arc::make_mut(s).shapes {
            if let ShapeKind::Picture { fill } = &mut sh.kind {
                fill.adjust.grayscale = true;
            }
        }
    }
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    let blip = xml.find("<a:blip>").expect("an open blip in {xml}");
    let after = &xml[blip..];
    let ext = after.find("<a:extLst>").expect("an extLst in {xml}");
    let gray = after.find("<a:grayscl/>").expect("grayscale in {xml}");
    assert!(gray < ext, "adjustments ride in the blip before its extLst: {xml}");
    let q = deckcraft_pptx::import(&bytes).expect("import");
    let mut found = None;
    deckcraft_model::walk(&q.slides[0].shapes, &mut |sh, _| {
        if sh.name == "Camera 1" {
            found = Some(sh.clone());
        }
    });
    let grayscale = match &found.expect("cameo").kind {
        ShapeKind::Picture { fill } => fill.adjust.grayscale,
        _ => panic!("picture"),
    };
    assert!(grayscale, "grayscale survives the round trip");
}

#[test]
fn a_linked_svg_cameo_writes_r_link_not_r_embed() {
    // Linked media has no package part: `r:embed` on `asvg:svgBlip` would name a relationship
    // that is not inside the package, so the linked form delegates to the raster blip's `r:link`.
    let mut p = deck("cameo-placeholder.svg", svg());
    let media = p.media.iter_mut().find(|m| m.name == "cameo-placeholder.svg").expect("placeholder");
    media.data = std::sync::Arc::new(vec![]);
    media.link = Some("https://example.org/cameo-placeholder.svg".into());
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    assert!(!xml.contains("<asvg:svgBlip"), "no SVG blip for linked media: {xml}");
    assert!(xml.contains("<a:blip r:link=\"rId"), "linked media rides r:link: {xml}");
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
