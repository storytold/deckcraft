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
}

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
                    Vector::Embedded => {
                        format!("<Relationship Id=\"rIdSvg\" Type=\"{IMAGE}\" Target=\"../media/image2.svg\"/>")
                    }
                    _ => format!("<Relationship Id=\"rIdSvg\" Type=\"{IMAGE}\" Target=\"https://example.com/i.svg\" TargetMode=\"External\"/>"),
                };
                data = xml.replacen("</Relationships>", &format!("{rel}</Relationships>"), 1).into_bytes();
            }
            "[Content_Types].xml" if vector == Vector::Embedded => {
                let xml = String::from_utf8(data).expect("types");
                let ct = "<Default Extension=\"svg\" ContentType=\"image/svg+xml\"/>";
                data = xml.replacen("</Types>", &format!("{ct}</Types>"), 1).into_bytes();
            }
            _ => {}
        }
        out.start_file(&name, opts).expect("start");
        out.write_all(&data).expect("write");
        if name == "ppt/slides/slide1.xml" && vector == Vector::Embedded {
            out.start_file("ppt/media/image2.svg", opts).expect("start svg");
            out.write_all(SVG).expect("write svg");
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
fn a_linked_svg_leaves_the_raster_fallback_in_place() {
    let p = deckcraft_pptx::import(&patched(Vector::Linked)).expect("import");
    let (ct, data) = picture_media(&p);
    assert_eq!(ct, "image/png");
    assert_eq!(data, PNG);
}

#[test]
fn without_the_extension_the_raster_fallback_stays() {
    let p = deckcraft_pptx::import(&patched(Vector::None)).expect("import");
    let (ct, data) = picture_media(&p);
    assert_eq!(ct, "image/png");
    assert_eq!(data, PNG);
}
