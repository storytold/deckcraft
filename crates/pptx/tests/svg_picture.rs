//! SVG pictures in a `.pptx`: PowerPoint keeps the vector original in `a:extLst` next to a raster
//! fallback in `a:blip/@r:embed` (Insert Icons writes SVG + PNG). Import must take the vector when
//! the part really is an SVG with bytes, and keep the fallback otherwise.

use std::io::{Cursor, Read, Write};

use deckcraft_geom::Xfrm;
use deckcraft_model::style::PictureFill;
use deckcraft_model::{LayoutType, Presentation, Shape, ShapeId, ShapeKind, defaults};

const SVG: &[u8] = b"<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24'>\
    <path d='M0 0 L24 0 L24 24 Z' fill='#ff0000'/></svg>";

/// A 1 by 1 PNG: the raster fallback PowerPoint stores next to the vector.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// What the `asvg:svgBlip` extension points at.
#[derive(Clone, Copy, PartialEq)]
enum Vector {
    /// No extension at all: the PNG fallback is the picture.
    None,
    /// An SVG part inside the package.
    Embedded,
    /// An SVG outside the package (`r:link`): no bytes to draw.
    Linked,
    /// A part declared as SVG whose bytes do not parse.
    Broken,
    /// An SVG that parses but draws through a filter we do not draw.
    Filtered,
}

/// Broken SVG bytes: an `svg` root that never closes.
const BROKEN: &[u8] = b"<svg";

/// An SVG that draws through a filter we do not draw, so it would show incomplete.
const FILTERED: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><defs><filter id='f'><feGaussianBlur stdDeviation='2'/></filter></defs><rect width='24' height='24' fill='red' filter='url(#f)'/></svg>";

fn deck() -> Presentation {
    let mut p = defaults::new_presentation(None);
    p.slides.clear();
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap_or_default();
    let mut slide = defaults::new_slide(&mut p, layout).expect("slide");
    let media = p.add_media("a.png", "image/png", PNG.to_vec()).expect("media");
    slide.shapes.push(Shape {
        id: ShapeId(p.alloc_id().expect("id")),
        xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 200.0)),
        kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
        ..Default::default()
    });
    p.slides.push(std::sync::Arc::new(slide));
    p
}

/// Rewrite the package: add the SVG part, its relationship and its content type, and the
/// `a:extLst` that names it from the picture — with the `a14:useLocalDpi` extension first, as
/// Microsoft writes it.
fn patched(vector: Vector) -> Vec<u8> {
    const IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
    let bytes = deckcraft_pptx::export(&deck()).expect("export");
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().to_string();
        let mut data = vec![];
        entry.read_to_end(&mut data).expect("read");
        match name.as_str() {
            "ppt/slides/slide1.xml" if vector != Vector::None => {
                let xml = String::from_utf8(data).expect("slide xml");
                let ext = format!(
                    "<a:extLst>\
                     <a:ext uri=\"{{28A0092B-C50C-407E-A947-70E740481C1C}}\">\
                     <a14:useLocalDpi xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\" val=\"0\"/></a:ext>\
                     <a:ext uri=\"{{96DAC541-7B7A-43D3-8B79-37D633B846F1}}\">\
                     <asvg:svgBlip xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\" {}=\"rIdSvg\"/>\
                     </a:ext></a:extLst>",
                    if vector == Vector::Linked { "r:link" } else { "r:embed" }
                );
                data = into_blip(&xml, &ext).into_bytes();
            }
            "ppt/slides/_rels/slide1.xml.rels" if vector != Vector::None => {
                let xml = String::from_utf8(data).expect("rels");
                let rel = match vector {
                    Vector::Embedded | Vector::Broken | Vector::Filtered => {
                        format!("<Relationship Id=\"rIdSvg\" Type=\"{IMAGE}\" Target=\"../media/image2.svg\"/>")
                    }
                    _ => format!("<Relationship Id=\"rIdSvg\" Type=\"{IMAGE}\" Target=\"https://example.com/i.svg\" TargetMode=\"External\"/>"),
                };
                data = xml.replacen("</Relationships>", &format!("{rel}</Relationships>"), 1).into_bytes();
            }
            "[Content_Types].xml" if matches!(vector, Vector::Embedded | Vector::Broken | Vector::Filtered) => {
                let xml = String::from_utf8(data).expect("types");
                let ct = "<Default Extension=\"svg\" ContentType=\"image/svg+xml\"/>";
                data = xml.replacen("</Types>", &format!("{ct}</Types>"), 1).into_bytes();
            }
            _ => {}
        }
        out.start_file(&name, opts).expect("start");
        out.write_all(&data).expect("write");
        if name == "ppt/slides/slide1.xml" && matches!(vector, Vector::Embedded | Vector::Broken | Vector::Filtered) {
            out.start_file("ppt/media/image2.svg", opts).expect("start svg");
            let body = match vector {
                Vector::Broken => BROKEN,
                Vector::Filtered => FILTERED.as_bytes(),
                _ => SVG,
            };
            out.write_all(body).expect("write svg");
        }
    }
    out.finish().expect("finish").into_inner()
}

/// Put `ext` (the `a:extLst`) inside the picture's `a:blip`, which the writer leaves self-closing.
fn into_blip(xml: &str, ext: &str) -> String {
    let Some(start) = xml.find("<a:blip") else { return xml.to_string() };
    let Some(end) = xml.get(start..).and_then(|r| r.find("/>")).map(|i| start + i) else { return xml.to_string() };
    format!("{}>{ext}</a:blip>{}", xml.get(..end).unwrap_or_default(), xml.get(end + 2..).unwrap_or_default())
}

fn picture_media(p: &Presentation) -> (String, Vec<u8>) {
    let slide = p.slides.first().expect("slide");
    let fill = slide
        .shapes
        .iter()
        .find_map(|s| match &s.kind {
            ShapeKind::Picture { fill } => Some(fill),
            _ => None,
        })
        .expect("picture");
    let m = p.media.iter().find(|m| m.id == fill.media).expect("media");
    (m.content_type.clone(), m.data.as_ref().clone())
}

#[test]
fn the_vector_original_wins_over_the_raster_fallback() {
    let p = deckcraft_pptx::import(&patched(Vector::Embedded)).expect("import");
    let (ct, data) = picture_media(&p);
    assert_eq!(ct, "image/svg+xml");
    assert_eq!(data, SVG);
}

#[test]
fn a_picture_that_uses_features_we_skip_keeps_its_raster_fallback() {
    let p = deckcraft_pptx::import(&patched(Vector::Filtered)).expect("import");
    let (ct, _) = picture_media(&p);
    assert_eq!(ct, "image/png", "the raster copy stays visible");
}

#[test]
fn unreadable_svg_bytes_leave_the_raster_fallback_in_place() {
    let p = deckcraft_pptx::import(&patched(Vector::Broken)).expect("import");
    let (ct, data) = picture_media(&p);
    assert_eq!(ct, "image/png");
    assert_eq!(data, PNG);
}

#[test]
fn a_linked_svg_leaves_the_raster_fallback_in_place() {
    let p = deckcraft_pptx::import(&patched(Vector::Linked)).expect("import");
    let (ct, data) = picture_media(&p);
    assert_eq!(ct, "image/png");
    assert_eq!(data, PNG);
}

/// A deck whose picture is the SVG itself, as DeckCraft's insert path stores it.
fn svg_deck() -> Presentation {
    let mut p = deck();
    let media = p.add_media("a.svg", "image/svg+xml", SVG.to_vec()).expect("media");
    let slide = std::sync::Arc::make_mut(&mut p.slides[0]);
    for s in &mut slide.shapes {
        if let ShapeKind::Picture { fill } = &mut s.kind {
            fill.media = media;
        }
    }
    p
}

/// The parts of a package, by name.
fn parts(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("zip");
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).expect("entry");
        let name = e.name().to_string();
        let mut data = vec![];
        e.read_to_end(&mut data).expect("read");
        out.push((name, data));
    }
    out
}

/// The target a relationship id names in `rels`, and whether it is external.
fn target(rels: &str, rid: &str) -> Option<String> {
    let start = rels.find(&format!("Id=\"{rid}\""))?;
    let rest = rels.get(start..)?;
    let t = rest.find("Target=\"")? + "Target=\"".len();
    let rest = rest.get(t..)?;
    let end = rest.find('"')?;
    rest.get(..end).map(str::to_string)
}

#[test]
fn an_svg_picture_is_saved_with_a_png_fallback_beside_it() {
    let bytes = deckcraft_pptx::export_with(&svg_deck(), &|_m| Some(PNG.to_vec())).expect("export");
    let parts = parts(&bytes);
    let slide = parts.iter().find(|(n, _)| n == "ppt/slides/slide1.xml").map(|(_, d)| String::from_utf8_lossy(d).into_owned()).expect("slide");
    let rels =
        parts.iter().find(|(n, _)| n == "ppt/slides/_rels/slide1.xml.rels").map(|(_, d)| String::from_utf8_lossy(d).into_owned()).expect("rels");
    let types = parts.iter().find(|(n, _)| n == "[Content_Types].xml").map(|(_, d)| String::from_utf8_lossy(d).into_owned()).expect("types");
    let has = |name: &str| parts.iter().any(|(n, _)| n == name);

    // `r:embed` is the raster, the vector rides in the extension, and both parts are in the package.
    let embed = slide.find("<a:blip").and_then(|start| {
        let rest = slide.get(start..)?;
        let i = rest.find("r:embed=\"")? + "r:embed=\"".len();
        let rest = rest.get(i..)?;
        let end = rest.find('"')?;
        rest.get(..end)
    });
    let embed = embed.expect("r:embed");
    let raster = target(&rels, embed).expect("raster target");
    assert!(raster.ends_with(".png"), "{raster}");
    let raster_part = format!("ppt/{}", raster.trim_start_matches("../"));
    assert!(has(&raster_part), "{raster_part} is in the package");
    let svg_blip = slide.find("asvg:svgBlip").expect("the vector extension");
    let rest = &slide[svg_blip..];
    let i = rest.find("r:embed=\"").expect("svg r:embed") + "r:embed=\"".len();
    let rest = &rest[i..];
    let svg_rid = &rest[..rest.find('"').expect("end")];
    let vector = target(&rels, svg_rid).expect("vector target");
    assert!(vector.ends_with(".svg"), "{vector}");
    let vector_part = format!("ppt/{}", vector.trim_start_matches("../"));
    assert!(has(&vector_part), "{vector_part} is in the package");
    assert!(types.contains("image/png"), "the PNG content type is declared");
    assert!(types.contains("image/svg+xml"), "the SVG content type is declared");

    // And the package reopens with the vector, not the fallback.
    let back = deckcraft_pptx::import(&bytes).expect("import");
    let (ct, data) = picture_media(&back);
    assert_eq!(ct, "image/svg+xml");
    assert_eq!(data, SVG);
}

#[test]
fn without_a_fallback_an_svg_picture_stays_in_the_blip() {
    let bytes = deckcraft_pptx::export(&svg_deck()).expect("export");
    let slide =
        parts(&bytes).into_iter().find(|(n, _)| n == "ppt/slides/slide1.xml").map(|(_, d)| String::from_utf8_lossy(&d).into_owned()).expect("slide");
    assert!(!slide.contains("asvg:svgBlip"), "no extension without a raster");
    let back = deckcraft_pptx::import(&bytes).expect("import");
    assert_eq!(picture_media(&back).0, "image/svg+xml");
}

#[test]
fn without_the_extension_the_raster_fallback_stays() {
    let p = deckcraft_pptx::import(&patched(Vector::None)).expect("import");
    let (ct, data) = picture_media(&p);
    assert_eq!(ct, "image/png");
    assert_eq!(data, PNG);
}
