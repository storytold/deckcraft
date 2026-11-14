//! Pictures that carry an SVG through Office's SVG extension (`asvg:svgBlip`).

use std::io::{Cursor, Read, Write};

use deckcraft_geom::Xfrm;
use deckcraft_model::style::PictureFill;
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind, defaults};

const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#c2185b"/></svg>"##;

fn png() -> Vec<u8> {
    let mut out = Cursor::new(vec![]);
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 2, image::Rgb([194, 24, 91])))
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("png");
    out.into_inner()
}

/// A deck with one picture showing an SVG with a PNG fallback, exported.
fn deck() -> Vec<u8> {
    let mut p = defaults::new_presentation(None);
    let media = p.add_media("step.png", "image/png", png()).expect("png");
    let svg = p.add_media("step.svg", "image/svg+xml", SVG.to_vec()).expect("svg");
    let id = ShapeId(p.alloc_id().expect("ids"));
    let first = p.slides[0].id;
    p.slide_mut(first).expect("slide").shapes.push(Shape {
        id,
        name: "Step".into(),
        xfrm: Some(Xfrm::new(10.0, 10.0, 200.0, 100.0)),
        kind: ShapeKind::Picture { fill: PictureFill { media, svg: Some(svg), ..Default::default() } },
        ..Default::default()
    });
    deckcraft_pptx::export(&p).expect("export")
}

/// `bytes` with `edit` applied to the first slide.
fn edit_slide(bytes: &[u8], edit: impl Fn(String) -> String) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let mut d = vec![];
        f.read_to_end(&mut d).expect("read");
        let name = f.name().to_string();
        if name == "ppt/slides/slide1.xml" {
            d = edit(String::from_utf8(d).expect("utf8")).into_bytes();
        }
        out.start_file(name, o).expect("start");
        out.write_all(&d).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

fn slide_xml(bytes: &[u8]) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    z.by_name("ppt/slides/slide1.xml").expect("slide").read_to_string(&mut out).expect("utf8");
    out
}

/// The picture named Step and the bytes of its image and SVG.
fn step(p: &Presentation) -> (Vec<u8>, Option<Vec<u8>>) {
    let s = p.slides[0].shapes.iter().find(|s| s.name == "Step").expect("picture");
    let ShapeKind::Picture { fill } = &s.kind else { panic!("not a picture: {:?}", s.kind) };
    let data = |m| p.media(m).map(|m| m.data.as_ref().clone()).expect("media");
    (data(fill.media), fill.svg.map(data))
}

#[test]
fn an_svg_with_a_png_fallback_round_trips() {
    let bytes = deck();
    let xml = slide_xml(&bytes);
    assert!(xml.contains("<asvg:svgBlip"), "{xml}");
    assert!(xml.contains("<a:blip r:embed="), "{xml}");
    let (image, svg) = step(&deckcraft_pptx::import(&bytes).expect("import"));
    assert_eq!((image, svg), (png(), Some(SVG.to_vec())));
}

#[test]
fn an_svg_without_a_fallback_stays_a_picture() {
    // Decks can name the SVG only, with no raster image in the blip itself.
    let bytes = edit_slide(&deck(), |x| {
        let start = x.find("<a:blip r:embed=\"").expect("blip");
        let end = start + x[start..].find('>').expect("end");
        format!("{}<a:blip>{}", &x[..start], &x[end + 1..])
    });
    let q = deckcraft_pptx::import(&bytes).expect("import");
    assert_eq!(step(&q), (SVG.to_vec(), Some(SVG.to_vec())));
    let again = deckcraft_pptx::export(&q).expect("export");
    let xml = slide_xml(&again);
    assert!(xml.contains("<p:pic>") && xml.contains("<a:blip><a:extLst>") && xml.contains("<asvg:svgBlip"), "{xml}");
    assert_eq!(step(&deckcraft_pptx::import(&again).expect("import")), (SVG.to_vec(), Some(SVG.to_vec())));
}
