use std::sync::Arc;

use super::*;
use deckcraft_color::Rgba;
use deckcraft_model::style::{ColorRef, Effects, Fill as ShapeFill, Gradient, GradientShape, GradientStop, PictureFill, Shadow};
use deckcraft_model::text::{Hyperlink, TextBody};
use deckcraft_model::{Shape, ShapeId, ShapeKind, Xfrm};
use std::collections::HashMap;

use deckcraft_fonts::FontDb;
use deckcraft_render::RenderOpts;
use kurbo::Rect;

fn deck(n: usize) -> Presentation {
    let mut p = deckcraft_model::defaults::new_presentation(None);
    let layout = p.slides[0].layout;
    while p.slides.len() < n {
        let s = deckcraft_model::defaults::new_slide(&mut p, layout).expect("slide");
        p.slides.push(Arc::new(s));
    }
    for (i, s) in p.slides.iter_mut().enumerate() {
        let s = Arc::make_mut(s);
        if let Some(sh) = s.shapes.first_mut() {
            sh.text = Some(TextBody::from_text(&format!("Slide title {i}")));
        }
        s.notes = TextBody::from_text("Speaker notes that wrap across the page width to test line breaking of long notes.");
    }
    p
}

fn opts(layout: PageLayout) -> PdfOptions {
    PdfOptions { layout, dpi: 48.0, ..Default::default() }
}

fn add(p: &mut Presentation, slide: usize, sh: Shape) {
    Arc::make_mut(&mut p.slides[slide]).shapes.push(sh);
}

fn rect(id: u32, x: f64, y: f64, fill: ShapeFill) -> Shape {
    Shape { id: ShapeId(id), xfrm: Some(Xfrm::new(x, y, 200.0, 100.0)), fill: Some(fill), ..Default::default() }
}

fn shadow() -> Effects {
    Effects {
        outer_shadow: Some(Shadow {
            color: ColorRef::rgb(Rgba::BLACK),
            blur: 8.0,
            dist: 6.0,
            dir: 45.0,
            inner: false,
            sx: 1.0,
            sy: 1.0,
            kx: 0.0,
            ky: 0.0,
            align: String::new(),
            rotate_with_shape: false,
        }),
        ..Default::default()
    }
}

/// A PDF with readable (uncompressed) page contents and text maps.
fn plain(p: &Presentation, o: &PdfOptions) -> Vec<u8> {
    export_with(p, o, false).expect("export")
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

fn pages(pdf: &[u8]) -> usize {
    count(pdf, b"/Type/Page") - count(pdf, b"/Type/Pages")
}

/// (width, height) of every image XObject, soft masks included.
fn images(pdf: &[u8]) -> Vec<(u32, u32)> {
    let num = |d: &[u8], key: &[u8]| -> u32 {
        let Some(at) = d.windows(key.len()).position(|w| w == key) else { return 0 };
        d[at + key.len()..]
            .iter()
            .skip_while(|c| c.is_ascii_whitespace())
            .take_while(|c| c.is_ascii_digit())
            .fold(0, |n, c| n * 10 + (c - b'0') as u32)
    };
    let key = b"/Subtype/Image";
    let mut out = vec![];
    for (at, _) in pdf.windows(key.len()).enumerate().filter(|(_, w)| *w == key) {
        let start = pdf[..at].windows(3).rposition(|w| w == b"obj").unwrap_or(0);
        let end = at + pdf[at..].windows(6).position(|w| w == b"stream").unwrap_or(0);
        let d = &pdf[start..end];
        out.push((num(d, b"/Width"), num(d, b"/Height")));
    }
    out
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn exports_slides_notes_and_handouts() {
    let p = deck(5);
    for vector in [true, false] {
        for layout in [PageLayout::Slides, PageLayout::Notes, PageLayout::Handouts { per_page: 3 }, PageLayout::Handouts { per_page: 6 }] {
            let bytes = export(&p, &PdfOptions { vector, ..opts(layout) }).expect("export");
            assert!(bytes.starts_with(b"%PDF-"), "{layout:?}");
            assert!(bytes.len() > 1000);
            let pdf = hayro_syntax::Pdf::new(bytes.clone()).unwrap();
            let (pages, size) = match layout {
                PageLayout::Slides => (5, (960.0, 540.0)),
                PageLayout::Notes => (5, (612.0, 792.0)),
                PageLayout::Handouts { per_page: 3 } => (2, (612.0, 792.0)),
                _ => (1, (612.0, 792.0)),
            };
            assert_eq!(pdf.pages().len(), pages);
            assert!(pdf.pages().iter().all(|page| page.render_dimensions() == size));
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn empty_range_is_an_error() {
    let p = deck(1);
    let o = PdfOptions { slides: Some(vec![7]), ..opts(PageLayout::Slides) };
    assert!(matches!(export(&p, &o), Err(PdfError::NoPages)));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn hidden_slides_skipped_unless_asked() {
    let mut p = deck(2);
    Arc::make_mut(&mut p.slides[1]).hidden = true;
    for vector in [true, false] {
        let a = plain(&p, &PdfOptions { vector, ..opts(PageLayout::Slides) });
        let b = plain(&p, &PdfOptions { vector, include_hidden: true, ..opts(PageLayout::Slides) });
        assert_eq!(pages(&a), 1, "vector {vector}");
        assert_eq!(pages(&b), 2, "vector {vector}");
    }
    let ranged = export(&p, &PdfOptions { slides: Some(vec![1, 0, 9]), ..opts(PageLayout::Slides) }).unwrap();
    assert_eq!(hayro_syntax::Pdf::new(ranged).unwrap().pages().len(), 2);
}

#[test]
fn vector_slides_are_paths_and_text_without_a_page_image() {
    let mut p = deck(1);
    add(&mut p, 0, rect(900, 40.0, 300.0, ShapeFill::solid(ColorRef::rgb(Rgba::rgb(0x20, 0x60, 0xC0)))));
    let grad = Gradient {
        stops: vec![
            GradientStop { pos: 0.0, color: ColorRef::rgb(Rgba::rgb(255, 0, 0)) },
            GradientStop { pos: 1.0, color: ColorRef::rgb(Rgba::rgba(0, 0, 255, 128)) },
        ],
        shape: GradientShape::Linear { angle: 30.0, scaled: false },
        rotate_with_shape: true,
        tile: [0.0; 4],
    };
    add(&mut p, 0, rect(901, 300.0, 300.0, ShapeFill::Gradient(grad)));
    let pdf = plain(&p, &opts(PageLayout::Slides));
    assert!(images(&pdf).is_empty(), "{:?}", images(&pdf));
    // Paths (filled), real text and a shading for the gradient.
    assert!(count(&pdf, b"\nf\n") + count(&pdf, b" f\n") > 0);
    assert!(count(&pdf, b"BT") > 0 && count(&pdf, b" Tf") > 0);
    assert!(count(&pdf, b"/ShadingType 2") > 0);
    // The raster mode draws the slide as one image the size of the page.
    let raster = plain(&p, &PdfOptions { vector: false, ..opts(PageLayout::Slides) });
    assert!(images(&raster).iter().any(|(w, _)| *w >= 600), "{:?}", images(&raster));
}

#[test]
fn vector_text_is_extractable() {
    let p = deck(1);
    let pdf = plain(&p, &opts(PageLayout::Slides));
    // The title's characters are mapped back to Unicode (UTF-16 hex in the ToUnicode map).
    assert!(count(&pdf, b"begincmap") > 0);
    for c in "Slide title".chars().filter(|c| !c.is_whitespace()) {
        let hex = format!("<{:04X}>", c as u32);
        assert!(count(&pdf, hex.as_bytes()) > 0, "{c}");
    }
    // No invisible layer: the text is drawn once.
    assert_eq!(count(&pdf, b"3 Tr"), 0);
}

#[test]
fn vector_pdf_is_much_smaller_than_raster() {
    let mut p = deck(5);
    add(&mut p, 1, rect(900, 40.0, 300.0, ShapeFill::solid(ColorRef::rgb(Rgba::rgb(0x20, 0x60, 0xC0)))));
    let o = PdfOptions { dpi: 200.0, ..Default::default() };
    let vector = export(&p, &o).expect("vector");
    let raster = export(&p, &PdfOptions { vector: false, ..o }).expect("raster");
    assert!(vector.len() * 5 < raster.len(), "vector {} raster {}", vector.len(), raster.len());
}

#[test]
fn a_shadow_rasterizes_that_shape_only() {
    let mut p = deck(1);
    let mut sh = rect(900, 100.0, 300.0, ShapeFill::solid(ColorRef::rgb(Rgba::rgb(0xF0, 0x80, 0x20))));
    sh.effects = Some(shadow());
    sh.text = Some(TextBody::from_text("Shadowed"));
    add(&mut p, 0, sh);
    let pdf = plain(&p, &PdfOptions { dpi: 72.0, ..opts(PageLayout::Slides) });
    let imgs = images(&pdf);
    // One local raster (and its soft mask), far smaller than the 960 × 540 page.
    assert!(!imgs.is_empty() && imgs.len() <= 2, "{imgs:?}");
    assert!(imgs.iter().all(|(w, h)| *w > 200 && *w < 400 && *h < 250), "{imgs:?}");
    // The shape's text stays text.
    assert!(count(&pdf, format!("<{:04X}>", 'S' as u32).as_bytes()) > 0);
    assert!(count(&pdf, format!("<{:04X}>", 'w' as u32).as_bytes()) > 0);
}

#[test]
fn pattern_fills_rasterize_their_area_only() {
    let mut p = deck(1);
    let pat = deckcraft_model::style::PatternFill {
        preset: "dkHorz".into(),
        fg: ColorRef::rgb(Rgba::rgb(0x20, 0x20, 0x20)),
        bg: ColorRef::rgb(Rgba::rgb(0xF0, 0xF0, 0xF0)),
    };
    add(&mut p, 0, rect(900, 100.0, 300.0, ShapeFill::Pattern(pat)));
    let pdf = plain(&p, &PdfOptions { dpi: 72.0, ..opts(PageLayout::Slides) });
    let imgs = images(&pdf);
    assert!(!imgs.is_empty() && imgs.iter().all(|(w, h)| *w <= 201 && *h <= 101), "{imgs:?}");
}

#[test]
fn jpeg_pictures_are_embedded_as_they_are() {
    let mut p = deck(1);
    let px: Vec<u8> = (0..64 * 48).flat_map(|i| [(i % 64 * 4) as u8, (i / 64 * 5) as u8, 128, 255]).collect();
    let jpeg = deckcraft_render::Image { width: 64, height: 48, pixels: px }.to_jpeg(90);
    assert!(jpeg.starts_with(&[0xFF, 0xD8]));
    let media = p.add_media("photo.jpg", "image/jpeg", jpeg.clone()).expect("media");
    let pic = Shape {
        id: ShapeId(900),
        xfrm: Some(Xfrm::new(100.0, 100.0, 320.0, 240.0)),
        kind: ShapeKind::Picture { fill: PictureFill { media, crop: [0.1, 0.0, 0.0, 0.1], ..Default::default() } },
        ..Default::default()
    };
    add(&mut p, 0, pic);
    let pdf = plain(&p, &opts(PageLayout::Slides));
    assert_eq!(count(&pdf, b"/DCTDecode"), 1);
    assert_eq!(images(&pdf), vec![(64, 48)]);
    // The file itself, byte for byte.
    assert_eq!(count(&pdf, &jpeg[..64]), 1);
    // Cropped: drawn under a clip.
    assert!(count(&pdf, b"W n") + count(&pdf, b"W\nn") > 0);
}

/// A PNG whose header reads but whose data doesn't shows the missing-picture placeholder, not a hole.
#[test]
fn a_picture_that_cannot_be_decoded_draws_the_placeholder() {
    let mut p = deck(1);
    let empty = plain(&p, &opts(PageLayout::Slides));
    let png = deckcraft_render::Image { width: 8, height: 8, pixels: vec![255; 8 * 8 * 4] }.to_png();
    let media = p.add_media("broken.png", "image/png", png[..33].to_vec()).expect("media");
    let pic = Shape {
        id: ShapeId(900),
        xfrm: Some(Xfrm::new(100.0, 100.0, 320.0, 240.0)),
        kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
        ..Default::default()
    };
    add(&mut p, 0, pic);
    let pdf = plain(&p, &opts(PageLayout::Slides));
    assert!(images(&pdf).is_empty());
    assert!(pdf.len() > empty.len(), "the placeholder is drawn");
}

#[test]
fn links_and_bookmarks_survive_vector_export() {
    let mut p = deck(3);
    let mut sh = rect(900, 40.0, 300.0, ShapeFill::solid(ColorRef::rgb(Rgba::rgb(0x20, 0x60, 0xC0))));
    sh.click = Some(Hyperlink {
        action: deckcraft_model::text::Action::Url { url: "https://example.org/deck".into() },
        tooltip: String::new(),
        highlight_click: false,
        sound: None,
        stop_sound: false,
        zoom_return: None,
    });
    add(&mut p, 0, sh);
    let mut next = rect(901, 300.0, 300.0, ShapeFill::solid(ColorRef::rgb(Rgba::rgb(0x20, 0xA0, 0x60))));
    next.click = Some(Hyperlink {
        action: deckcraft_model::text::Action::LastSlide,
        tooltip: String::new(),
        highlight_click: false,
        sound: None,
        stop_sound: false,
        zoom_return: None,
    });
    add(&mut p, 0, next);
    for layout in [PageLayout::Slides, PageLayout::Handouts { per_page: 6 }] {
        let pdf = plain(&p, &opts(layout));
        assert_eq!(count(&pdf, b"https://example.org/deck"), 1, "{layout:?}");
        assert!(count(&pdf, b"/Subtype/Link") >= 2, "{layout:?}");
        assert!(count(&pdf, b"/Outlines") > 0, "{layout:?}");
        assert!(count(&pdf, b"Slide title 2") > 0, "{layout:?}");
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn handout_boxes_fit_the_paper() {
    for n in [1, 2, 3, 4, 6, 9] {
        let v = handout_boxes(n, (612.0, 792.0), 16.0 / 9.0);
        assert_eq!(v.len(), n as usize);
        for r in &v {
            assert!(r.x0 >= 0.0 && r.y0 >= 0.0 && r.x1 <= 612.0 && r.y1 <= 792.0, "{n}: {r:?}");
            assert!((r.width() / r.height() - 16.0 / 9.0).abs() < 1e-6);
        }
        for (i, a) in v.iter().enumerate() {
            for b in &v[i + 1..] {
                assert!(a.intersect(*b).area() <= 0.0, "{n}: overlap");
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn metadata_retains_values_and_omits_blank_fields() {
    let mut p = deck(1);
    p.props.title = "  Deck title  ".into();
    p.props.author = "  Author  ".into();
    let bytes = export(&p, &opts(PageLayout::Slides)).unwrap();
    let pdf = hayro_syntax::Pdf::new(bytes.clone()).unwrap();
    assert_eq!(pdf.metadata().title.as_deref(), Some(b"  Deck title  ".as_slice()));
    assert_eq!(pdf.metadata().author.as_deref(), Some(b"  Author  ".as_slice()));
    assert_eq!(pdf.metadata().creator.as_deref(), Some(b"DeckCraft".as_slice()));
    assert_eq!(pdf.metadata().producer.as_deref(), Some(b"DeckCraft".as_slice()));
    assert!(pdf.metadata().creation_date.is_none());
    p.props.author = " ".into();
    let bytes = export(&p, &PdfOptions { title: Some(" ".into()), ..opts(PageLayout::Slides) }).unwrap();
    let pdf = hayro_syntax::Pdf::new(bytes.clone()).unwrap();
    assert!(pdf.metadata().title.is_none());
    assert!(pdf.metadata().author.is_none());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn text_layer_links_and_bookmarks_survive_serialization() {
    use deckcraft_model::text::{Action, Hyperlink};
    use hayro_syntax::object::{Array, Dict};
    let mut p = deck(2);
    let last = p.slides[1].id;
    let slide = Arc::make_mut(&mut p.slides[0]);
    for (shape, action) in slide.shapes.iter_mut().zip([Action::Url { url: "https://example.com/deck".into() }, Action::Slide { slide: last }]) {
        shape.click = Some(Hyperlink { action, tooltip: "Link".into(), highlight_click: false, sound: None, stop_sound: false, zoom_return: None });
    }
    // Vector slides always carry real text; raster slides only with the text layer.
    for (vector, text_layer) in [(false, true), (false, false), (true, false)] {
        let bytes = export(&p, &PdfOptions { vector, text_layer, ..opts(PageLayout::Slides) }).unwrap();
        let pdf = hayro_syntax::Pdf::new(bytes.clone()).unwrap();
        let page = &pdf.pages()[0];
        let annotations = page.raw().get::<Array<'_>>(b"Annots").unwrap();
        assert_eq!(annotations.iter::<Dict<'_>>().count(), 2);
        let resources = page.raw().get::<Dict<'_>>(b"Resources").unwrap();
        let fonts = resources.get::<Dict<'_>>(b"Font");
        assert_eq!(fonts.is_some_and(|fonts| !fonts.is_empty()), vector || text_layer);
        let catalog = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id()).unwrap();
        let outlines = catalog.get::<Dict<'_>>(b"Outlines").unwrap();
        let first = outlines.get::<Dict<'_>>(b"First").unwrap();
        assert!(first.get::<Dict<'_>>(b"Next").is_some());
    }
}

/// Links from an untrusted deck: only web and mail links become PDF links. Scripts, `data:`,
/// local and relative files, OS protocol handlers and network shares (a UNC path leaks the user's
/// NTLM credentials on Windows) do not.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn unsafe_url_links_are_not_exported() {
    use deckcraft_model::text::{Action, Hyperlink};
    use deckcraft_model::{Shape, ShapeId, Xfrm};
    use hayro_syntax::object::{Array, Dict, String as PdfString};
    let safe = ["https://example.com/deck", "mailto:someone@example.com"];
    let refused = [
        "file:///C:/talks/notes.pdf",
        "notes.pdf",
        "ms-msdt:/id PCWDiagnostic /skip force",
        "ms-word:ofe|u|\\\\attacker.example\\share\\x.docx",
        "ldap://attacker.example/x",
        "file:///%5C%5Cattacker.example/share/x",
        "javascript:alert(1)",
        "\t JavaScript:alert(1)",
        "java\nscript:alert(1)",
        "vbscript:MsgBox(1)",
        "data:text/html,<script>alert(1)</script>",
        "\\\\attacker.example\\share\\x",
        "//attacker.example/share/x",
        "file://attacker.example/share/x",
        "file:////attacker.example/share/x",
        "smb://attacker.example/share/x",
        "search-ms:query=x&crumb=location:\\\\attacker.example\\share",
        "search:query=x&crumb=location:\\\\attacker.example\\share",
    ];
    let mut p = deck(1);
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.shapes = (0..)
        .zip(safe.iter().chain(&refused))
        .map(|(i, url)| Shape {
            id: ShapeId(9000 + i),
            xfrm: Some(Xfrm::new(20.0, 20.0 + 32.0 * f64::from(i), 400.0, 26.0)),
            click: Some(Hyperlink {
                action: Action::Url { url: url.to_string() },
                tooltip: String::new(),
                highlight_click: false,
                sound: None,
                stop_sound: false,
                zoom_return: None,
            }),
            ..Default::default()
        })
        .collect();
    let bytes = export(&p, &opts(PageLayout::Slides)).unwrap();
    let pdf = hayro_syntax::Pdf::new(bytes.clone()).unwrap();
    let annotations = pdf.pages()[0].raw().get::<Array<'_>>(b"Annots").unwrap();
    let uris: Vec<String> = annotations
        .iter::<Dict<'_>>()
        .filter_map(|a| a.get::<Dict<'_>>(b"A")?.get::<PdfString<'_>>(b"URI"))
        .map(|s| String::from_utf8_lossy(s.as_bytes()).into_owned())
        .collect();
    assert_eq!(uris, safe);
}

/// Build a small damaged derivative of our openly licensed last-resort font entirely in
/// memory. Its naming/shaping tables still parse, but its missing head table prevents PDF
/// embedding. No font bytes or vendor assets are stored as test fixtures.
fn font_without_head() -> Vec<u8> {
    let mut bytes = FontDb::global().face("Ubuntu", "Regular").data().to_vec();
    for (old, new) in [(b"Ubuntu".as_slice(), b"BadPDF".as_slice()), (b"\0U\0b\0u\0n\0t\0u".as_slice(), b"\0B\0a\0d\0P\0D\0F".as_slice())] {
        let mut offset = 0;
        while offset + old.len() <= bytes.len() {
            if bytes[offset..offset + old.len()] == *old {
                bytes[offset..offset + old.len()].copy_from_slice(new);
                offset += old.len();
            } else {
                offset += 1;
            }
        }
    }
    let tables = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    for i in 0..tables {
        let offset = 12 + i * 16;
        if &bytes[offset..offset + 4] == b"head" {
            bytes[offset..offset + 4].copy_from_slice(b"zzzz");
        }
    }
    bytes
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn unembeddable_font_returns_error_instead_of_a_pdf_missing_text() {
    use deckcraft_model::{Shape, ShapeId, Xfrm};
    let db = FontDb::global();
    assert_eq!(db.add_font(font_without_head()), 1);
    assert_eq!(db.face("BadPDF", "Regular").family, "BadPDF");
    let mut p = deck(1);
    let slide = Arc::make_mut(&mut p.slides[0]);
    let mut body = TextBody::from_text("Text that must not disappear silently");
    for paragraph in &mut body.paragraphs {
        for run in &mut paragraph.runs {
            run.props.font = Some("BadPDF".into());
            run.props.size = Some(12.0);
        }
    }
    slide.shapes = vec![Shape { id: ShapeId(9000), xfrm: Some(Xfrm::new(20.0, 20.0, 500.0, 200.0)), text: Some(body), ..Default::default() }];
    // Vector slides and the raster mode's text layer both report it.
    for vector in [true, false] {
        let error = export(&p, &PdfOptions { vector, ..opts(PageLayout::Slides) }).unwrap_err();
        assert!(matches!(error, PdfError::Write(_)));
        assert!(error.to_string().contains("font could not be read for embedding"), "vector {vector}: {error}");
    }
    // Master text remains visible even when the optional slide text layer is off.
    // It must propagate the same embedding error through notes and handout pages.
    let mut header = ph(deckcraft_model::PhType::Header, 0.0, 0.0, 500.0, 40.0, "");
    header.text = p.slides[0].shapes[0].text.clone();
    for layout in [PageLayout::Notes, PageLayout::Handouts { per_page: 3 }] {
        let mut p = deck(1);
        let master = Some(Arc::new(deckcraft_model::Master { shapes: vec![header.clone()], ..Default::default() }));
        if layout == PageLayout::Notes {
            p.notes_master = master;
        } else {
            p.handout_master = master;
        }
        let error = export(&p, &PdfOptions { text_layer: false, ..opts(layout) }).unwrap_err();
        assert!(matches!(error, PdfError::Write(_)));
        assert!(error.to_string().contains("font could not be read for embedding"), "{layout:?}: {error}");
    }
    // The failed export leaves no poisoned writer state behind.
    assert!(export(&deck(1), &opts(PageLayout::Slides)).is_ok());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn supported_unicode_is_shaped_and_recovered_from_the_page_text_layer() {
    use deckcraft_model::{Shape, ShapeId, Xfrm};
    use hayro_syntax::content::ops::TypedInstruction;
    use hayro_syntax::object::{Dict, Object, Stream};

    let text = "Café Ελληνικά Привет";
    let face = FontDb::global().face("Ubuntu", "Regular");
    assert_eq!(face.family, "Ubuntu");
    assert!(text.chars().all(|c| face.glyph_for(c) != 0), "missing: {:?}", text.chars().filter(|c| face.glyph_for(*c) == 0).collect::<Vec<_>>());
    let mut body = TextBody::from_text(text);
    for paragraph in &mut body.paragraphs {
        for run in &mut paragraph.runs {
            run.props.font = Some("Ubuntu".into());
            run.props.size = Some(20.0);
        }
    }
    let mut p = deck(1);
    Arc::make_mut(&mut p.slides[0]).shapes =
        vec![Shape { id: ShapeId(9001), xfrm: Some(Xfrm::new(20.0, 20.0, 920.0, 100.0)), text: Some(body), ..Default::default() }];
    let placed = deckcraft_render::place_slide(&p, 0);
    let runs: Vec<_> = placed.texts.iter().flat_map(|placed| &placed.layout.runs).collect();
    assert!(!runs.is_empty());
    assert!(runs.iter().all(|run| run.face.family == "Ubuntu" && !run.glyphs.is_empty() && run.glyphs.iter().all(|glyph| glyph.0 != 0)));

    let bytes = export(&p, &opts(PageLayout::Slides)).unwrap();
    let pdf = hayro_syntax::Pdf::new(bytes.clone()).unwrap();
    let page = &pdf.pages()[0];
    let resources = page.raw().get::<Dict<'_>>(b"Resources").unwrap();
    let fonts = resources.get::<Dict<'_>>(b"Font").unwrap();
    assert_eq!(fonts.len(), 1);
    let font = fonts.get::<Dict<'_>>(fonts.keys().next().unwrap()).unwrap();
    let cmap = font.get::<Stream<'_>>(b"ToUnicode").unwrap().decoded().unwrap().into_owned();
    let cmap = std::str::from_utf8(&cmap).unwrap();
    // This fixture uses the writer's two-byte CIDs and bfchar mappings. Decode the
    // content's text-showing operators through that map, not document metadata.
    let mut mapping = HashMap::<u16, String>::new();
    let mut in_char_map = false;
    for line in cmap.lines() {
        if line.ends_with("beginbfchar") {
            in_char_map = true;
        } else if line == "endbfchar" {
            in_char_map = false;
        } else if in_char_map {
            let parts: Vec<_> = line.split_whitespace().collect();
            let cid = u16::from_str_radix(parts[0].trim_matches(['<', '>']), 16).unwrap();
            let value = parts[1].trim_matches(['<', '>']);
            let units: Vec<_> = value
                .as_bytes()
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| u16::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16).unwrap())
                .collect();
            mapping.insert(cid, String::from_utf16(&units).unwrap());
        }
    }
    let mut extracted = String::new();
    let mut append = |bytes: &[u8]| {
        assert!(bytes.len().is_multiple_of(2));
        for pair in bytes.as_chunks::<2>().0 {
            extracted.push_str(mapping.get(&u16::from_be_bytes([pair[0], pair[1]])).unwrap());
        }
    };
    let mut operations = page.typed_operations();
    while let Some(operation) = operations.next() {
        match operation {
            TypedInstruction::ShowText(value) => append(value.0.as_bytes()),
            TypedInstruction::ShowTexts(values) => {
                for value in values.0.iter::<Object<'_>>() {
                    if let Object::String(value) = value {
                        append(value.as_bytes());
                    }
                }
            }
            _ => {}
        }
    }
    assert_eq!(extracted, text);
}

/// Notes pages and handouts print on the chosen paper, landscape included, with every slide box
/// on the page and room left for the notes.
#[test]
fn notes_and_handouts_fit_any_paper_and_orientation() {
    let p = deck(3);
    for (id, _, _) in PAPERS {
        for landscape in [false, true] {
            let paper = paper_size(id, landscape).expect("known paper");
            assert_eq!(paper.0 > paper.1, landscape, "{id}");
            let b = notes_slide_box(paper, 16.0 / 9.0);
            assert!(b.x0 >= 0.0 && b.x1 <= paper.0 && b.y1 <= paper.1 * 0.5, "{id} {landscape}: {b:?}");
            assert!((b.width() / b.height() - 16.0 / 9.0).abs() < 1e-6);
            for n in [1, 3, 6, 9] {
                for r in handout_boxes(n, paper, 16.0 / 9.0) {
                    assert!(r.x0 >= 0.0 && r.y0 >= 0.0 && r.x1 <= paper.0 && r.y1 <= paper.1, "{id} {landscape} {n}: {r:?}");
                }
            }
            for layout in [PageLayout::Notes, PageLayout::Handouts { per_page: 6 }] {
                let bytes = export(&p, &PdfOptions { paper, ..opts(layout) }).expect("export");
                let text = String::from_utf8_lossy(&bytes);
                let media = format!("/MediaBox[0 0 {} {}]", fmt(paper.0), fmt(paper.1));
                assert!(text.contains(&media), "{id} {landscape} {layout:?}: no {media}");
            }
        }
    }
    assert_eq!(paper_size("bogus", false), None);
}

fn fmt(v: f64) -> String {
    let s = format!("{:.2}", v as f32);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn ph(kind: deckcraft_model::PhType, x: f64, y: f64, w: f64, h: f64, text: &str) -> deckcraft_model::Shape {
    deckcraft_model::Shape {
        xfrm: Some(deckcraft_model::Xfrm::new(x, y, w, h)),
        ph: Some(deckcraft_model::Placeholder { kind, ..Default::default() }),
        text: Some(TextBody::from_text(text)),
        ..Default::default()
    }
}

#[test]
fn notes_pages_and_handouts_follow_their_masters() {
    use deckcraft_model::PhType;
    let mut p = deck(2);
    let paper = (612.0, 792.0);
    // No masters: the default layout (slide at the top, one-inch side margins).
    let (slide, notes) = notes_frames(&p, paper);
    assert_eq!((slide.x0, slide.y0), (72.0, 54.0));
    assert!(notes.y0 > slide.y1);
    // A notes master moves the slide and the notes, scaled from the notes size to the paper.
    p.notes_size = deckcraft_model::Size::new(612.0, 792.0);
    p.notes_master = Some(Arc::new(deckcraft_model::Master {
        shapes: vec![
            ph(PhType::SlideImage, 100.0, 400.0, 412.0, 300.0, ""),
            ph(PhType::Body, 50.0, 50.0, 500.0, 300.0, ""),
            ph(PhType::Header, 0.0, 0.0, 300.0, 30.0, "Q3 review"),
            ph(PhType::Date, 312.0, 0.0, 300.0, 30.0, ""),
            ph(PhType::SlideNum, 312.0, 760.0, 300.0, 30.0, "<#>"),
        ],
        ..Default::default()
    }));
    let (slide, notes) = notes_frames(&p, paper);
    assert!(slide.y0 >= 400.0 && slide.y1 <= 700.0 + 1e-6 && slide.x0 >= 100.0 - 1e-6, "{slide:?}");
    assert!(((slide.width() / slide.height()) - p.slide_size.width / p.slide_size.height).abs() < 1e-6);
    assert_eq!(notes, Rect::new(50.0, 50.0, 550.0, 350.0));
    let texts = master_texts(&p, p.notes_master.as_deref().unwrap(), paper, 7);
    let got: Vec<(&str, bool)> = texts.iter().map(|(_, t, r, _)| (t.as_str(), *r)).collect();
    assert_eq!(got, [("Q3 review", false), ("7", true)], "empty date left out, number is the page");
    // Notes pages and handouts export with the masters.
    for layout in [PageLayout::Notes, PageLayout::Handouts { per_page: 3 }] {
        if layout != PageLayout::Notes {
            p.handout_master = Some(Arc::new(deckcraft_model::Master {
                shapes: vec![ph(PhType::Footer, 0.0, 760.0, 300.0, 30.0, "Confidential")],
                ..Default::default()
            }));
        }
        let bytes = export(&p, &PdfOptions { layout, ..Default::default() }).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }
}

#[test]
fn master_text_keeps_its_own_formatting() {
    use deckcraft_model::text::RunProps;
    use deckcraft_model::{ColorRef, Fill, PhType, Rgba};
    let mut p = deck(1);
    p.notes_size = deckcraft_model::Size::new(612.0, 792.0);
    let mut header = ph(PhType::Header, 0.0, 0.0, 300.0, 30.0, "Board pack");
    if let Some(run) = header.text.as_mut().and_then(|t| t.paragraphs.first_mut()).and_then(|p| p.runs.first_mut()) {
        run.props = RunProps {
            size: Some(20.0),
            bold: Some(true),
            fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(200, 0, 0)))),
            font: Some("+mj-lt".into()),
            ..Default::default()
        };
    }
    let m = deckcraft_model::Master { shapes: vec![header, ph(PhType::Footer, 0.0, 760.0, 300.0, 30.0, "plain")], ..Default::default() };
    let texts = master_texts(&p, &m, (612.0, 792.0), 1);
    let st = &texts[0].3;
    assert_eq!((st.size, st.bold, st.italic, st.color), (20.0, true, false, Rgba::rgb(200, 0, 0)));
    assert_eq!(st.family, m.theme.font("+mj-lt"), "theme font resolved");
    // Unstyled text falls back through the master's styles; sizes scale with the paper.
    let half = master_texts(&p, &m, (306.0, 396.0), 1);
    assert!((half[0].3.size - 10.0).abs() < 1e-9);
    assert!(texts[1].3.size > 0.0 && !texts[1].3.bold);
    p.notes_master = Some(Arc::new(m));
    assert!(export(&p, &PdfOptions { layout: PageLayout::Notes, ..Default::default() }).unwrap().starts_with(b"%PDF"));
}

#[test]
fn master_text_wraps_and_keeps_mixed_formatting() {
    use deckcraft_model::text::{Run, RunProps};
    use deckcraft_model::{PhType, Rgba};
    let mut p = deck(1);
    p.notes_size = deckcraft_model::Size::new(612.0, 792.0);
    let mut header = ph(PhType::Header, 0.0, 0.0, 200.0, 80.0, "");
    if let Some(para) = header.text.as_mut().and_then(|t| t.paragraphs.first_mut()) {
        para.runs = vec![
            Run::with("Quarterly board pack ", RunProps { size: Some(18.0), bold: Some(true), ..Default::default() }),
            Run::with(
                "for internal circulation only, please do not forward",
                RunProps {
                    size: Some(9.0),
                    fill: Some(deckcraft_model::Fill::solid(deckcraft_model::ColorRef::rgb(Rgba::rgb(200, 0, 0)))),
                    ..Default::default()
                },
            ),
        ];
    }
    let m = deckcraft_model::Master { shapes: vec![header, ph(PhType::SlideNum, 312.0, 760.0, 300.0, 30.0, "")], ..Default::default() };
    let placed = deckcraft_render::place_master_texts(&p, &m, 4);
    assert_eq!(placed.len(), 2, "header and page number");
    let h = &placed[0].layout;
    assert!(h.lines.len() >= 2, "wraps inside its 200 pt placeholder: {} lines", h.lines.len());
    let sizes: Vec<f64> = h.runs.iter().filter(|r| !r.glyphs.is_empty()).map(|r| r.size).collect();
    assert!(sizes.iter().any(|s| (*s - 18.0).abs() < 0.5) && sizes.iter().any(|s| (*s - 9.0).abs() < 0.5), "{sizes:?}");
    assert!(h.runs.iter().any(|r| r.color == Rgba::rgb(200, 0, 0)));
    let num: String = placed[1].body.text();
    assert_eq!(num, "4");
    // The same layout as pieces for the Notes Page view: on more than one line, scaled to paper.
    let pieces = master_text_pieces(&p, &m, (306.0, 396.0), 4);
    let mut ys: Vec<i64> = pieces.iter().map(|x| x.at.1.round() as i64).collect();
    ys.dedup();
    assert!(ys.len() >= 3, "two header lines and the number: {pieces:?}");
    assert!(pieces.iter().any(|x| x.bold && (x.size - 9.0).abs() < 0.5), "18 pt bold at half size");
    assert!(pieces.iter().any(|x| x.color == Rgba::rgb(200, 0, 0) && !x.bold));
    assert!(pieces.iter().any(|x| x.text == "4"));
    p.notes_master = Some(Arc::new(m));
    assert!(export(&p, &PdfOptions { layout: PageLayout::Notes, ..Default::default() }).unwrap().starts_with(b"%PDF"));
}

/// Slides that share their layout store their backdrop raster once in the exported PDF, over which
/// each slide's own shapes draw as their own raster (#845).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn slides_share_one_backdrop_raster_and_keep_their_pixels() {
    use deckcraft_model::{Shape, ShapeId, Xfrm};
    use hayro_syntax::object::{Dict, Number, Stream};

    /// The image XObjects the export stores, one entry per distinct object: the pages that draw
    /// it, its filters, color channel, alpha channel (the SMask) and pixel size.
    fn images(p: &Presentation) -> Vec<(Vec<usize>, Vec<hayro_syntax::Filter>, Vec<u8>, Option<Vec<u8>>, (u32, u32))> {
        let bytes = export(p, &PdfOptions { vector: false, ..opts(PageLayout::Slides) }).expect("export");
        let pdf = hayro_syntax::Pdf::new(bytes).expect("pdf");
        let mut out: Vec<(String, Vec<usize>, Vec<hayro_syntax::Filter>, Vec<u8>, Option<Vec<u8>>, (u32, u32))> = Vec::new();
        for (n, page) in pdf.pages().iter().enumerate() {
            let resources = page.raw().get::<Dict>(b"Resources").expect("resources");
            let xobjects = resources.get::<Dict>(b"XObject").expect("xobjects");
            for key in xobjects.keys() {
                let stream = xobjects.get::<Stream>(key).expect("stream");
                let id = format!("{:?}", stream.obj_id());
                match out.iter_mut().find(|(seen, ..)| *seen == id) {
                    Some(entry) => entry.1.push(n),
                    None => {
                        let dict = stream.dict();
                        let alpha = dict.get::<Stream>(b"SMask").map(|mask| mask.decoded().expect("decoded").into_owned());
                        let size = (
                            dict.get::<Number>(b"Width").expect("width").as_f64() as u32,
                            dict.get::<Number>(b"Height").expect("height").as_f64() as u32,
                        );
                        out.push((id, vec![n], stream.filters().into_vec(), stream.decoded().expect("decoded").into_owned(), alpha, size));
                    }
                }
            }
        }
        out.into_iter().map(|(_, pages, filters, rgb, alpha, size)| (pages, filters, rgb, alpha, size)).collect()
    }

    // The raster scale the exporter uses for one slide per page.
    let scale = |p: &Presentation| (p.slide_size.width / 72.0 * opts(PageLayout::Slides).dpi) / p.slide_size.width;

    // Three slides without shapes: every page is pure backdrop and shares one raster, which is
    // exactly what the renderer draws.
    let mut p = deck(3);
    for slide in &mut p.slides {
        Arc::make_mut(slide).shapes.clear();
    }
    let stored = images(&p);
    assert_eq!(stored.len(), 1, "all three pages must share one backdrop raster");
    let (pages, filters, rgb, alpha, size) = stored.into_iter().next().expect("one image");
    assert_eq!(pages, [0, 1, 2]);
    assert_eq!(filters, [hayro_syntax::Filter::FlateDecode], "backdrop artwork stays lossless");
    assert!(alpha.is_none(), "the backdrop is opaque and needs no alpha mask");
    let expect = deckcraft_render::render_slide(&p, 0, &RenderOpts { scale: scale(&p), threads: 4, ..Default::default() });
    assert_eq!(size, (expect.width, expect.height));
    let want: Vec<u8> = expect.pixels.as_chunks::<4>().0.iter().flat_map(|px| px[..3].iter().copied()).collect();
    assert_eq!(rgb, want, "the shared raster is exactly what the renderer draws");

    // Slides with different text still share the backdrop: one raster on every page plus one per
    // slide's own shapes.
    let stored = images(&deck(3));
    assert_eq!(stored.len(), 4, "one shared backdrop plus three content rasters");
    assert_eq!(stored.iter().filter(|(pages, ..)| pages.len() == 3).count(), 1, "the backdrop is the only raster every page draws");

    // A slide's own shapes draw over the shared backdrop as a second raster, stored as straight
    // RGBA (color channel plus alpha mask), never the renderer's premultiplied pixels.
    let mut p = deck(1);
    Arc::make_mut(&mut p.slides[0]).shapes.push(Shape {
        id: ShapeId(9002),
        xfrm: Some(Xfrm::new(20.0, 20.0, 400.0, 300.0)),
        fill: Some(deckcraft_model::Fill::solid(deckcraft_model::ColorRef::rgb(deckcraft_model::Rgba::rgb(200, 30, 30)))),
        ..Default::default()
    });
    let stored = images(&p);
    assert_eq!(stored.len(), 2, "shared backdrop plus the slide's content");
    let (_, filters, rgb, alpha, _) = stored.into_iter().find(|(_, _, _, alpha, _)| alpha.is_some()).expect("content raster");
    assert_eq!(filters, [hayro_syntax::Filter::FlateDecode]);
    let expect = deckcraft_render::render_slide_part(
        &p,
        0,
        deckcraft_render::SlidePart::Content,
        &RenderOpts { scale: scale(&p), threads: 4, ..Default::default() },
    );
    let straight = expect.to_straight();
    let want_rgb: Vec<u8> = straight.as_chunks::<4>().0.iter().flat_map(|px| px[..3].iter().copied()).collect();
    let want_alpha: Vec<u8> = straight.as_chunks::<4>().0.iter().map(|px| px[3]).collect();
    assert_eq!(rgb, want_rgb, "content colors are straight, not premultiplied");
    assert_eq!(alpha, Some(want_alpha), "content alpha keeps the shape's coverage");

    // A backdrop that comes back after another one is still stored once, and every page draws its
    // own slide's backdrop.
    let mut p = deck(4);
    for (i, slide) in p.slides.iter_mut().enumerate() {
        let s = Arc::make_mut(slide);
        s.shapes.clear();
        let c = if i % 2 == 0 { deckcraft_model::Rgba::rgb(20, 90, 160) } else { deckcraft_model::Rgba::rgb(230, 230, 210) };
        s.background = Some(deckcraft_model::Background::Fill { fill: deckcraft_model::Fill::solid(deckcraft_model::ColorRef::rgb(c)) });
    }
    let stored = images(&p);
    assert_eq!(stored.iter().map(|(pages, ..)| pages.clone()).collect::<Vec<_>>(), [vec![0, 2], vec![1, 3]]);
    for (pages, _, rgb, alpha, _) in &stored {
        let expect = deckcraft_render::render_slide(&p, pages[0], &RenderOpts { scale: scale(&p), threads: 4, ..Default::default() });
        let want: Vec<u8> = expect.pixels.as_chunks::<4>().0.iter().flat_map(|px| px[..3].iter().copied()).collect();
        assert_eq!(rgb, &want, "pages {pages:?} draw their own backdrop");
        assert!(alpha.is_none());
    }
}

/// The block-skipping pixel conversions give exactly the per-pixel formulas, in blocks that skip
/// and blocks that don't, and in a tail shorter than a block.
#[test]
fn backdrop_and_content_pixels_convert_per_pixel() {
    let mut px: Vec<u8> = Vec::new();
    for a in 0..=255u8 {
        for c in 0..=255u8 {
            px.extend([c, c / 2, 255 - c, a]);
        }
    }
    px.extend([0, 0, 0, 0].repeat(32));
    px.extend([9, 8, 7, 255].repeat(32));
    px.extend([1, 2, 3, 128, 4, 5, 6, 0]);
    let (mut o, mut s) = (px.clone(), px.clone());
    opaque(&mut o);
    straight(&mut s);
    for ((p, o), s) in px.chunks(4).zip(o.chunks(4)).zip(s.chunks(4)) {
        let a = p[3] as u32;
        for k in 0..3 {
            assert_eq!(o[k] as u32, (p[k] as u32 + 255 - a).min(255), "{p:?}");
            let want = if a == 0 || a == 255 { p[k] as u32 } else { ((p[k] as u32 * 255 + a / 2) / a).min(255) };
            assert_eq!(s[k] as u32, want, "{p:?}");
        }
        assert_eq!((o[3], s[3]), (255, p[3]));
    }
}

#[test]
fn glyph_ranges_follow_shaping_clusters() {
    // "fl" ligature: its glyph maps to both characters, the rest stay one to one (#164).
    assert_eq!(glyph_text_ranges("flash", (0, 5), &[(0, 'f'), (2, 'a'), (3, 's'), (4, 'h')], 4), vec![0..2, 2..3, 3..4, 4..5]);
    // Two glyphs in one cluster (base and mark) share its range.
    assert_eq!(glyph_text_ranges("ab", (0, 2), &[(0, 'a'), (0, 'a'), (1, 'b')], 3), vec![0..1, 0..1, 1..2]);
    // A run starting mid-paragraph, multi-byte characters in visual (right-to-left) order.
    assert_eq!(glyph_text_ranges("αβγ", (3, 6), &[(5, 'γ'), (4, 'β'), (3, 'α')], 3), vec![4..6, 2..4, 0..2]);
}

/// A placeable WMF, 2 x 1 inch: a red rectangle over the left half.
fn half_red_wmf() -> Vec<u8> {
    fn rec(func: u16, params: &[i16]) -> Vec<u8> {
        let mut v = ((3 + params.len()) as u32).to_le_bytes().to_vec();
        v.extend(func.to_le_bytes());
        for p in params {
            v.extend(p.to_le_bytes());
        }
        v
    }
    let body = [rec(0x02FC, &[0, 0xFF, 0, 0]), rec(0x012D, &[0]), rec(0x041B, &[1440, 1440, 0, 0]), rec(0, &[])].concat();
    let mut f = 0x9AC6_CDD7u32.to_le_bytes().to_vec();
    f.extend(0u16.to_le_bytes());
    for v in [0i16, 0, 2880, 1440] {
        f.extend(v.to_le_bytes());
    }
    f.extend(1440u16.to_le_bytes());
    f.extend(0u32.to_le_bytes());
    f.extend(0u16.to_le_bytes());
    let total = (f.len() + 18 + body.len()) / 2;
    for v in [1u16, 9, 0x300] {
        f.extend(v.to_le_bytes());
    }
    f.extend((total as u32).to_le_bytes());
    f.extend(16u16.to_le_bytes());
    f.extend(0u32.to_le_bytes());
    f.extend(0u16.to_le_bytes());
    f.extend(body);
    f
}

/// What the renderer draws only as pixels (pattern fills, rectangle path gradients, text shadows,
/// WMF pictures, colour emoji) exports as local images in a vector PDF: nothing panics, nothing is
/// dropped, and the text around stays text.
#[test]
fn pixel_only_drawing_exports_as_local_images() {
    use deckcraft_model::style::{PatternFill, Shadow};
    let mut p = deck(1);
    let pat = PatternFill { preset: "dkVert".into(), fg: ColorRef::rgb(Rgba::rgb(0x20, 0x20, 0x20)), bg: ColorRef::rgb(Rgba::rgb(0xF0, 0xF0, 0xF0)) };
    add(&mut p, 0, rect(900, 40.0, 300.0, ShapeFill::Pattern(pat)));
    let stops = vec![
        GradientStop { pos: 0.0, color: ColorRef::rgb(Rgba::rgb(255, 0, 0)) },
        GradientStop { pos: 1.0, color: ColorRef::rgb(Rgba::rgb(0, 0, 255)) },
    ];
    let path =
        Gradient { stops, shape: GradientShape::Path { path: "rect".into(), focus: [0.5, 0.5, 0.5, 0.5] }, rotate_with_shape: true, tile: [0.0; 4] };
    add(&mut p, 0, rect(901, 260.0, 300.0, ShapeFill::Gradient(path)));
    let mut text = Shape { id: ShapeId(902), xfrm: Some(Xfrm::new(480.0, 280.0, 440.0, 120.0)), text_box: true, ..Default::default() };
    let mut body = TextBody::from_text("Shade \u{1F600} me");
    body.paragraphs[0].runs[0].props.size = Some(40.0);
    body.paragraphs[0].runs[0].props.shadow =
        Some(Shadow { color: ColorRef::rgb(Rgba { r: 0, g: 0, b: 0, a: 160 }), blur: 4.0, dist: 4.0, ..shadow_of() });
    text.text = Some(body);
    add(&mut p, 0, text);
    let media = p.add_media("image1.wmf", "image/x-wmf", half_red_wmf()).expect("media");
    let pic = Shape {
        id: ShapeId(903),
        xfrm: Some(Xfrm::new(480.0, 420.0, 200.0, 100.0)),
        kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
        ..Default::default()
    };
    add(&mut p, 0, pic);
    let pdf = plain(&p, &PdfOptions { dpi: 72.0, ..opts(PageLayout::Slides) });
    let imgs = images(&pdf);
    // Pattern, path gradient, text shadow, metafile (emoji when a colour font is installed),
    // each a local image well under the page size, soft masks included.
    assert!(imgs.len() >= 4, "{imgs:?}");
    assert!(imgs.iter().all(|(w, h)| *w < 600 && *h < 300), "{imgs:?}");
    for c in "Shademe".chars() {
        assert!(count(&pdf, format!("<{:04X}>", c as u32).as_bytes()) > 0, "{c} is text");
    }
}

fn shadow_of() -> Shadow {
    shadow().outer_shadow.expect("shadow")
}

/// Upright glyphs in East Asian vertical text (`eaVert`) each turn about their own em box, which a
/// PDF text run can't express: they are drawn as outlines over the same text, invisible, so the
/// characters still search, select and copy.
#[test]
fn upright_vertical_text_is_outlines_over_invisible_text() {
    let mut p = deck(1);
    let mut sh = Shape { id: ShapeId(904), xfrm: Some(Xfrm::new(100.0, 40.0, 120.0, 400.0)), text_box: true, ..Default::default() };
    let mut body = TextBody::from_text("一二三");
    body.body.vert = Some(deckcraft_model::text::TextDir::EaVertical);
    body.paragraphs[0].runs[0].props.size = Some(40.0);
    sh.text = Some(body);
    add(&mut p, 0, sh);
    let pdf = plain(&p, &opts(PageLayout::Slides));
    // The run's text, drawn fully transparent (the outlines carry the ink).
    assert!(count(&pdf, b"/Type/ExtGState/ca 0>>") > 0, "an invisible text layer for the upright run");
    for c in "一二三".chars() {
        assert!(count(&pdf, format!("<{:04X}>", c as u32).as_bytes()) > 0, "{c} is text");
    }
    let mut q = deck(1);
    let mut sh = Shape { id: ShapeId(905), xfrm: Some(Xfrm::new(100.0, 40.0, 400.0, 100.0)), text_box: true, ..Default::default() };
    sh.text = Some(TextBody::from_text("一二三"));
    add(&mut q, 0, sh);
    assert_eq!(count(&plain(&q, &opts(PageLayout::Slides)), b"/Type/ExtGState/ca 0>>"), 0, "horizontal text is drawn as text");
}

#[test]
fn raster_scale_caps_total_pixels_not_each_side() {
    // A huge square slide at 600 dpi would be 16,000 x 16,000 px (~1 GB RGBA) without the cap.
    let s = raster_scale(20_000.0, (20_000.0, 20_000.0), 600.0);
    let px = 20_000.0 * s * 20_000.0 * s;
    assert!(px <= MAX_RASTER_PIXELS * 1.0001, "{px}");
    // A normal slide keeps the requested dpi.
    let s = raster_scale(720.0, (960.0, 540.0), 200.0);
    assert!((s - 720.0 / 72.0 * 200.0 / 960.0).abs() < 1e-12);
}
