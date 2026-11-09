//! Cameo round trips: the `alf:liveFeedProps` marker and the `asvg:svgBlip` placeholder.
//!
//! The decks are built in code, never produced by PowerPoint: an SVG placeholder we draw and an
//! ordinary PNG for the checked-in-picture case.

use std::io::{Cursor, Read, Write};

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

/// The smallest MP4 header worth writing: the writer only stores bytes, and the reader takes the
/// type from the package, so no real H.264 payload is needed here (the media tests cover that).
fn mp4() -> Vec<u8> {
    let mut v = vec![0u8, 0, 0, 0x18];
    v.extend_from_slice(b"ftypisom");
    v.extend_from_slice(&[0, 0, 2, 0]);
    v.extend_from_slice(b"isomiso2avc1mp41");
    v
}

fn recorded_deck() -> Presentation {
    let mut p = defaults::new_presentation(None);
    let video = p.add_media("cameo.mp4", "video/mp4", mp4()).expect("video");
    let poster = p.add_media("cameo-poster.png", "image/png", png()).expect("poster");
    let shape = Shape {
        id: ShapeId(p.alloc_id().expect("id")),
        name: "Camera 1".into(),
        xfrm: Some(Xfrm::new(700.0, 360.0, 162.0, 162.0)),
        kind: ShapeKind::Picture { fill: PictureFill { media: poster, ..Default::default() } },
        geom: Geom::preset("ellipse"),
        cameo: Some(Cameo::recorded(video)),
        descr: "Recorded camera clip".into(),
        ..Default::default()
    };
    if let Some(s) = p.slides.first_mut() {
        std::sync::Arc::make_mut(s).shapes.push(shape);
    }
    p
}

#[test]
fn a_recorded_cameo_writes_video_links_and_a_media_timing_node() {
    let p = recorded_deck();
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    assert!(xml.contains("<alf:liveFeedProps"), "still a Cameo: {xml}");
    assert!(xml.contains("<a:videoFile r:link=\"rId"), "the video relationship: {xml}");
    assert!(xml.contains("<p14:media xmlns:p14="), "the embedded media part: {xml}");
    assert!(xml.contains("<p:video><p:cMediaNode vol=\"100000\" showWhenStopped=\"0\">"), "plays on entry: {xml}");
    assert!(xml.contains("<p:cmd type=\"call\" cmd=\"playFrom(0.0)\">"), "the media-call step PowerPoint uses: {xml}");
    assert!(xml.contains("<p:spTgt spid=\""), "targets the cameo picture: {xml}");
    // The poster stays the picture's image, not a second part the timing points at.
    assert!(xml.contains("<a:blip r:embed=\"rId"), "{xml}");
    let rels = part(&bytes, "ppt/slides/_rels/slide1.xml.rels");
    assert!(rels.contains("/relationships/video"), "{rels}");
    assert!(rels.contains("office/2007/relationships/media"), "{rels}");

    let q = deckcraft_pptx::import(&bytes).expect("import");
    let mut found = None;
    deckcraft_model::walk(&q.slides[0].shapes, &mut |sh, _| {
        if sh.name == "Camera 1" {
            found = Some(sh.clone());
        }
    });
    let sh = found.expect("cameo shape");
    assert!(sh.is_cameo());
    let cameo = sh.cameo.as_ref().expect("cameo");
    assert!(!cameo.live);
    assert!(cameo.autoplay, "the p:video node means it plays when the slide appears");
    let recorded = cameo.recorded.expect("recording");
    assert_eq!(q.media(recorded).map(|m| m.content_type.as_str()), Some("video/mp4"));
    let poster = match &sh.kind {
        ShapeKind::Picture { fill } => fill.media,
        _ => panic!("the recording stays a picture shape"),
    };
    assert_eq!(q.media(poster).map(|m| m.content_type.as_str()), Some("image/png"));
}

#[test]
fn a_recorded_cameo_keeps_the_camera_background_it_was_imported_with() {
    let raw = concat!(
        "<alf:liveFeedProps xmlns:alf=\"http://schemas.microsoft.com/office/drawing/2021/livefeed\">",
        "<alf:backgroundProps><alf:Blur/></alf:backgroundProps>",
        "</alf:liveFeedProps>"
    );
    let mut p = recorded_deck();
    if let Some(s) = p.slides.first_mut() {
        for sh in &mut std::sync::Arc::make_mut(s).shapes {
            if let Some(c) = &mut sh.cameo {
                c.raw = Some(raw.to_string());
            }
        }
    }
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let q = deckcraft_pptx::import(&bytes).expect("import");
    let mut found = None;
    deckcraft_model::walk(&q.slides[0].shapes, &mut |sh, _| {
        if sh.name == "Camera 1" {
            found = Some(sh.clone());
        }
    });
    let cameo = found.expect("cameo shape").cameo.expect("cameo");
    assert!(!cameo.live);
    assert!(cameo.recorded.is_some(), "the recording survives the import");
    let kept = cameo.raw.expect("the feed kept from the file");
    assert!(kept.contains("backgroundProps") && kept.contains("Blur"), "{kept}");
    // Attaching the recording must not turn the kept feed into an empty one on the next save.
    let bytes = deckcraft_pptx::export(&q).expect("export");
    assert!(part(&bytes, "ppt/slides/slide1.xml").contains("backgroundProps"));
}

/// The same package with every part whose name matches `drop` removed.
fn without_parts(bytes: &[u8], drop: &dyn Fn(&str) -> bool) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("zip");
    let mut w = zip::ZipWriter::new(Cursor::new(vec![]));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let name = f.name().to_string();
        let mut buf = vec![];
        f.read_to_end(&mut buf).expect("read");
        if drop(&name) {
            continue;
        }
        w.start_file(name, opts).expect("start");
        w.write_all(&buf).expect("write");
    }
    w.finish().expect("finish").into_inner()
}

#[test]
fn a_recorded_cameo_without_a_readable_poster_stays_a_picture() {
    let p = recorded_deck();
    let bytes = deckcraft_pptx::export(&p).expect("export");
    // The poster image is gone, so the picture's fill cannot be read; the recording remains.
    let bytes = without_parts(&bytes, &|name| name.starts_with("ppt/media/") && name.ends_with(".png"));
    let q = deckcraft_pptx::import(&bytes).expect("import");
    let mut found = None;
    deckcraft_model::walk(&q.slides[0].shapes, &mut |sh, _| {
        if sh.name == "Camera 1" {
            found = Some(sh.clone());
        }
    });
    let sh = found.expect("cameo shape");
    assert!(matches!(sh.kind, ShapeKind::Picture { .. }), "the cameo keeps its picture shape");
    assert!(sh.is_cameo());
    let cameo = sh.cameo.as_ref().expect("cameo");
    assert!(cameo.recorded.is_some(), "the recording survives a missing poster");
    // The fallback poster is visible art, not the transparent placeholder.
    let poster = match &sh.kind {
        ShapeKind::Picture { fill } => fill.media,
        _ => panic!("picture"),
    };
    let m = q.media(poster).expect("fallback poster");
    assert_eq!(m.content_type, "image/png", "the renderer decodes PNG, not SVG");
    assert!(!m.data.is_empty());
    // And it is written back as a picture.
    let bytes = deckcraft_pptx::export(&q).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    assert!(xml.contains("<a:blip r:embed=\"rId"), "the cameo keeps a picture: {xml}");
    assert!(xml.contains("<p:video>"), "the recording still plays: {xml}");
}

#[test]
fn a_live_cameo_without_a_readable_poster_stays_a_picture() {
    let p = deck("still.png", png());
    let bytes = deckcraft_pptx::export(&p).expect("export");
    // The picture's image is gone, so its fill cannot be read; the live feed remains.
    let bytes = without_parts(&bytes, &|name| name.starts_with("ppt/media/") && name.ends_with(".png"));
    let q = deckcraft_pptx::import(&bytes).expect("import");
    let mut found = None;
    deckcraft_model::walk(&q.slides[0].shapes, &mut |sh, _| {
        if sh.name == "Camera 1" {
            found = Some(sh.clone());
        }
    });
    let sh = found.expect("cameo shape");
    assert!(sh.is_cameo(), "a live Cameo without a readable poster stays a picture");
    assert_eq!(sh.cameo.as_ref().map(|c| c.live), Some(true));
    let poster = match &sh.kind {
        ShapeKind::Picture { fill } => fill.media,
        _ => panic!("picture"),
    };
    let m = q.media(poster).expect("fallback poster");
    assert_eq!(m.content_type, "image/png", "the renderer decodes PNG, not SVG");
    // Re-export keeps the marker, so the feed settings are not lost with the poster.
    let again = deckcraft_pptx::export(&q).expect("export");
    assert!(part(&again, "ppt/slides/slide1.xml").contains("<alf:liveFeedProps"), "the live feed survives");
}

#[test]
fn a_recorded_cameo_that_does_not_autoplay_waits_for_a_click() {
    let mut p = recorded_deck();
    if let Some(s) = p.slides.first_mut() {
        for sh in &mut std::sync::Arc::make_mut(s).shapes {
            if let Some(c) = &mut sh.cameo {
                c.autoplay = false;
            }
        }
    }
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    assert!(xml.contains("<p:video>"), "the clip plays when the show reaches it: {xml}");
    assert!(xml.contains("<p:cond delay=\"indefinite\"/>"), "it waits for a click, not slide entry: {xml}");
    assert!(xml.contains("<p:cMediaNode vol=\"100000\" showWhenStopped=\"1\">"), "the poster stays visible until it starts: {xml}");
    assert!(xml.contains("ppaction://media"), "a click on the cameo starts it: {xml}");
    // The recording is still part of the picture, so the round trip keeps it.
    assert!(xml.contains("<a:videoFile r:link=\"rId"), "{xml}");

    let q = deckcraft_pptx::import(&bytes).expect("import");
    let mut found = None;
    deckcraft_model::walk(&q.slides[0].shapes, &mut |sh, _| {
        if sh.name == "Camera 1" {
            found = Some(sh.clone());
        }
    });
    let cameo = found.expect("cameo shape").cameo.expect("cameo");
    assert!(cameo.recorded.is_some());
    assert!(!cameo.autoplay, "an imported recording does not start by itself");
}

#[test]
fn a_tagged_recorded_cameo_writes_the_video_before_cust_data() {
    let mut p = recorded_deck();
    if let Some(s) = p.slides.first_mut() {
        for sh in &mut std::sync::Arc::make_mut(s).shapes {
            if sh.cameo.is_some() {
                sh.tags = vec![("tag1".into(), "value".into())];
            }
        }
    }
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    let video = xml.find("<a:videoFile").expect("video element");
    let cust = xml.find("<p:custDataLst").expect("tag list");
    let ext = xml.find("{DAA4B4D4-6D71-4841-9C94-3DE7FCFB9230}").expect("media extension");
    assert!(video < cust, "p:nvPr puts the media element before p:custDataLst: {xml}");
    assert!(cust < ext, "p:extLst comes last: {xml}");
}

#[test]
fn a_cameo_below_the_writer_depth_is_not_timed() {
    let mut p = defaults::new_presentation(None);
    let video = p.add_media("cameo.mp4", "video/mp4", mp4()).expect("video");
    let poster = p.add_media("cameo-poster.png", "image/png", png()).expect("poster");
    let mut shape = Shape {
        id: ShapeId(p.alloc_id().expect("id")),
        name: "Camera 1".into(),
        xfrm: Some(Xfrm::new(10.0, 10.0, 162.0, 162.0)),
        kind: ShapeKind::Picture { fill: PictureFill { media: poster, ..Default::default() } },
        geom: Geom::preset("ellipse"),
        cameo: Some(Cameo::recorded(video)),
        ..Default::default()
    };
    // The writer stops at depth 32; a Cameo past it must not get a `p:video` target either.
    for i in 0..34 {
        shape = Shape {
            id: ShapeId(p.alloc_id().expect("id")),
            name: format!("Group {i}"),
            xfrm: Some(Xfrm::new(0.0, 0.0, 400.0, 300.0)),
            kind: ShapeKind::Group { children: vec![shape], child: Xfrm::new(0.0, 0.0, 400.0, 300.0) },
            ..Default::default()
        };
    }
    if let Some(s) = p.slides.first_mut() {
        std::sync::Arc::make_mut(s).shapes.push(shape);
    }
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    assert!(!xml.contains("<p:video>"), "no timing target without the picture: {xml}");
    assert!(!xml.contains("Camera 1"), "the Cameo past the depth limit is not written: {xml}");
}

#[test]
fn a_linked_cameo_svg_is_written_as_a_link_not_an_embed() {
    let mut p = defaults::new_presentation(None);
    let media = deckcraft_model::MediaId(p.alloc_id().expect("id"));
    p.media.push(deckcraft_model::MediaItem {
        id: media,
        name: "cameo-placeholder.svg".into(),
        content_type: "image/svg+xml".into(),
        data: std::sync::Arc::default(),
        link: Some("file:///Users/me/cameo-placeholder.svg".into()),
    });
    let shape = Shape {
        id: ShapeId(p.alloc_id().expect("id")),
        name: "Camera 1".into(),
        xfrm: Some(Xfrm::new(700.0, 360.0, 162.0, 162.0)),
        kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
        geom: Geom::preset("ellipse"),
        cameo: Some(Cameo::live()),
        ..Default::default()
    };
    if let Some(s) = p.slides.first_mut() {
        std::sync::Arc::make_mut(s).shapes.push(shape);
    }
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    assert!(xml.contains("<a:blip r:link=\"rId"), "a linked picture: {xml}");
    assert!(!xml.contains("svgBlip"), "no embed of an external part: {xml}");
    let rels = part(&bytes, "ppt/slides/_rels/slide1.xml.rels");
    assert!(rels.contains("TargetMode=\"External\""), "{rels}");
}

#[test]
fn a_live_cameo_gets_no_video_part_or_timing() {
    let p = deck("cameo-placeholder.svg", svg());
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    assert!(!xml.contains("a:videoFile"), "{xml}");
    assert!(!xml.contains("<p:video>"), "{xml}");
    assert!(!xml.contains("<p:timing"), "{xml}");
}
