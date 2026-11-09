//! Original vector/preview probes. All image bytes are generated here, never vendor assets.
use deckcraft_engine::render::{RenderOpts, render_slide};
use deckcraft_model::style::PictureFill;
use deckcraft_model::{LayoutType, Presentation, Shape, ShapeId, ShapeKind, Xfrm, defaults};
use std::sync::Arc;

fn probe() -> Presentation {
    let mut p = Presentation::default();
    p.slides.clear();
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    let mut slide = defaults::new_slide(&mut p, layout).unwrap();
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="600" height="300" viewBox="0 0 600 300"><defs><linearGradient id="g"><stop stop-color="#cc2200"/><stop offset="1" stop-color="#0033dd"/></linearGradient><clipPath id="c"><circle cx="150" cy="150" r="120"/></clipPath></defs><rect width="300" height="300" clip-path="url(#c)" fill="url(#g)"/><g transform="translate(320 30) rotate(12 100 100)"><path d="M0 0H220V220H0Z" fill="#00aa66"/><text x="20" y="120" font-size="48" font-family="Inter" fill="white">Vector</text></g></svg>"##;
    let vector = p.add_media("original-vector.svg", "image/svg+xml", svg.to_vec()).unwrap();
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(60, 30, image::Rgba([255, 0, 255, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let raster = p.add_media("original-preview.png", "image/png", png.into_inner()).unwrap();
    slide.shapes.push(Shape {
        id: ShapeId(100),
        xfrm: Some(Xfrm::new(80.0, 80.0, 800.0, 400.0)),
        kind: ShapeKind::Picture { fill: PictureFill { media: raster, svg: Some(vector), ..Default::default() } },
        ..Default::default()
    });
    p.slides.push(Arc::new(slide));
    p
}

#[test]
fn embedded_svg_precedes_preview_and_keeps_both_relationships() {
    let p = probe();
    let expected = render_slide(&p, 0, &RenderOpts::default());
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    let ShapeKind::Picture { fill } = &back.slides[0].shapes[0].kind else { panic!("picture") };
    assert_eq!(back.media(fill.svg.unwrap()).unwrap().content_type, "image/svg+xml");
    assert_eq!(back.media(fill.media).unwrap().content_type, "image/png");
    let actual = render_slide(&back, 0, &RenderOpts::default());
    assert_eq!(expected.pixels, actual.pixels);
    assert!(actual.pixels.chunks_exact(4).filter(|p| p[0] < 20 && p[1] > 120 && p[2] > 60 && p[2] < 130).count() > 20000);
    assert_eq!(
        actual.pixels.chunks_exact(4).filter(|p| p[0] == 255 && p[1] == 0 && p[2] == 255).count(),
        0,
        "SVG should replace the low-resolution preview"
    );
    let mut broken = back.clone();
    let vector = fill.svg.unwrap();
    let media = broken.media.iter_mut().find(|m| m.id == vector).unwrap();
    media.data = Arc::new(b"not an image".to_vec());
    let fallback = render_slide(&broken, 0, &RenderOpts::default());
    assert_eq!(fallback.pixel(200, 200), [255, 0, 255, 255], "broken SVG must use retained PNG");
    let reopened = deckcraft_pptx::import(&deckcraft_pptx::export(&back).unwrap()).unwrap();
    assert_eq!(render_slide(&reopened, 0, &RenderOpts::default()).pixels, actual.pixels);
    if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::PathBuf::from(dir).join("svg-picture.pptx"), bytes).unwrap();
    }
}

#[test]
fn extension_only_svg_imports_without_a_raster_reference() {
    use std::io::{Read, Write};
    let bytes = deckcraft_pptx::export(&probe()).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let name = file.name().to_string();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        if name == "ppt/slides/slide1.xml" {
            let mut xml = String::from_utf8(data).unwrap();
            let start = xml.find("<a:blip r:embed=\"").unwrap() + "<a:blip".len();
            let end = xml[start..].find('>').unwrap() + start;
            xml.replace_range(start..end, "");
            data = xml.into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(&data).unwrap();
    }
    let p = deckcraft_pptx::import(&out.finish().unwrap().into_inner()).unwrap();
    let ShapeKind::Picture { fill } = &p.slides[0].shapes[0].kind else { panic!("picture") };
    assert_eq!(fill.svg, Some(fill.media));
    assert_eq!(p.media(fill.media).unwrap().content_type, "image/svg+xml");
    assert_eq!(render_slide(&p, 0, &RenderOpts::default()).pixels, render_slide(&probe(), 0, &RenderOpts::default()).pixels);
}

#[test]
fn svg_text_uses_own_embedded_font_and_cache_does_not_leak_across_decks() {
    let fonts = deckcraft_fonts::CRAFT_FONTS;
    let Some(sans) = fonts.iter().find(|f| f.family == "Inter" && f.style == "Regular") else { return };
    let Some(serif) = fonts.iter().find(|f| f.family == "Liberation Serif" && f.style == "Regular") else { return };
    let original = probe();
    let with_font = |bytes: &[u8]| {
        let mut p = original.clone();
        let id = p.add_media("own-font.fntdata", "application/x-fontdata", bytes.to_vec()).unwrap();
        p.embedded_fonts.push(deckcraft_model::EmbeddedFont { typeface: "Inter".into(), regular: Some(id), ..Default::default() });
        p
    };
    let a = with_font(sans.bytes);
    let b = with_font(serif.bytes);
    let first = render_slide(&a, 0, &RenderOpts::default());
    let second = render_slide(&b, 0, &RenderOpts::default());
    assert_ne!(first.pixels, second.pixels, "same SVG bytes must use each document's own embedded face");
    assert_eq!(render_slide(&a, 0, &RenderOpts::default()).pixels, first.pixels, "font-aware cache remains isolated");
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&b).unwrap()).unwrap();
    assert_eq!(render_slide(&back, 0, &RenderOpts::default()).pixels, second.pixels);
}
