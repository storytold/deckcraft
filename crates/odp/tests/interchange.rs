#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_model::{defaults, style::PictureFill, text::RunKind, *};
use deckcraft_odp::{MIME, export, import, sniff};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};
use zip::{CompressionMethod, write::SimpleFileOptions};

fn deck() -> Presentation {
    let mut p = defaults::blank_presentation(Size::new(720.0, 405.0), Theme::default(), false);
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    let slide_id = SlideId(p.alloc_id().unwrap());
    let id = ShapeId(p.alloc_id().unwrap());
    let mut body = TextBody::from_text("  Hello & <world>\tZ\nSecond paragraph");
    body.paragraphs[0].runs[0].props =
        RunProps { font: Some("Liberation Sans".into()), size: Some(27.0), bold: Some(true), italic: Some(true), ..Default::default() };
    body.paragraphs[0].runs.push(Run { kind: RunKind::Break, ..Run::new("") });
    body.paragraphs[0].runs.push(Run::new("after break"));
    let mut shapes = vec![Shape {
        id,
        name: "Text & rectangle".into(),
        xfrm: Some(Xfrm::new(20.0, 30.0, 500.0, 100.0)),
        fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(12, 34, 56)))),
        line: Some(Line::solid(ColorRef::rgb(Rgba::BLACK), 2.0)),
        text: Some(body),
        ..Default::default()
    }];
    shapes.push(Shape {
        id: ShapeId(p.alloc_id().unwrap()),
        name: "Ellipse".into(),
        geom: Geom::preset("ellipse"),
        xfrm: Some(Xfrm::new(10.0, 200.0, 30.0, 40.0)),
        fill: Some(Fill::None),
        ..Default::default()
    });
    let mut line = Xfrm::new(200.0, 200.0, 40.0, 60.0);
    line.flip_h = true;
    shapes.push(Shape {
        id: ShapeId(p.alloc_id().unwrap()),
        name: "Line".into(),
        geom: Geom::preset("line"),
        xfrm: Some(line),
        fill: Some(Fill::None),
        line: Some(Line::solid(ColorRef::rgb(Rgba::BLACK), 1.0)),
        ..Default::default()
    });
    p.slides.push(Arc::new(Slide {
        id: slide_id,
        layout,
        name: "Slide & one".into(),
        shapes,
        notes: TextBody::from_text("Remember  this\nAnd this"),
        hidden: true,
        show_master_shapes: false,
        ..Default::default()
    }));
    let id = SlideId(p.alloc_id().unwrap());
    p.slides.push(Arc::new(Slide { id, layout, name: "Two".into(), ..Default::default() }));
    p
}

fn package(content: &str, styles: Option<&str>, manifest: Option<&str>) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // Deliberately put mimetype last: import must not assume local entry order.
    for (name, data) in [
        ("content.xml", Some(content)),
        ("styles.xml", styles),
        (
            "META-INF/manifest.xml",
            Some(manifest.unwrap_or("<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>")),
        ),
        ("mimetype", Some(MIME)),
    ] {
        if let Some(data) = data {
            z.start_file(name, SimpleFileOptions::default()).unwrap();
            z.write_all(data.as_bytes()).unwrap();
        }
    }
    z.finish().unwrap().into_inner()
}

fn document(shapes: &str, styles: &str) -> String {
    format!(
        "<o:document-content xmlns:o='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:t='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:v='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:f='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:x='http://www.w3.org/1999/xlink'><o:automatic-styles>{styles}</o:automatic-styles><o:body><o:presentation><d:page d:name='Independent fixture'>{shapes}</d:page></o:presentation></o:body></o:document-content>"
    )
}

#[test]
fn round_trip_editable_objects_rich_text_notes_and_slide_settings() {
    let p = deck();
    let bytes = export(&p).unwrap();
    assert!(sniff(&bytes));
    let q = import(&bytes).unwrap();
    assert_eq!(q.slide_size, p.slide_size);
    assert_eq!(q.slides.len(), 2);
    let s = &q.slides[0];
    assert!(s.hidden);
    assert_eq!(s.name, "Slide & one");
    assert_eq!(s.notes.text(), p.slides[0].notes.text());
    for (a, b) in s.shapes.iter().zip(&p.slides[0].shapes) {
        assert_eq!(a.name, b.name);
        let ax = a.xfrm.unwrap();
        let bx = b.xfrm.unwrap();
        assert!((ax.w - bx.w).abs() < 1e-6 && (ax.h - bx.h).abs() < 1e-6);
        for (a, b) in ax.affine().as_coeffs().iter().zip(bx.affine().as_coeffs()) {
            assert!((a - b).abs() < 1e-6);
        }
        assert_eq!(a.geom, b.geom);
        assert_eq!(a.text.as_ref().map(TextBody::text), b.text.as_ref().map(TextBody::text));
    }
    let run = &s.shapes[0].text.as_ref().unwrap().paragraphs[0].runs[0];
    assert_eq!(run.props.font.as_deref(), Some("Liberation Sans"));
    assert_eq!(run.props.size, Some(27.0));
    assert_eq!(run.props.bold, Some(true));
    assert_eq!(run.props.italic, Some(true));
    let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mime = z.by_index(0).unwrap();
    assert_eq!(mime.name(), "mimetype");
    assert_eq!(mime.compression(), CompressionMethod::Stored);
    assert_eq!(mime.extra_data(), Some(&[][..]));
    // Optional local standards-validation artifact, never committed.
    if let Ok(path) = std::env::var("DECKCRAFT_ODP_TEST_OUTPUT") {
        std::fs::write(path, &bytes).unwrap();
    }
}

#[test]
fn imports_independently_authored_xml_with_arbitrary_prefixes_and_inherited_styles() {
    let content = document(
        "<d:frame d:name='Box' d:style-name='Graphic' v:x='1in' v:y='2.54cm' v:width='100mm' v:height='6pc'><d:text-box><t:p t:style-name='Child'>A<t:s t:c='3'/>B &amp; C<t:tab/><t:span t:style-name='Em'>bold</t:span><t:line-break/>tail</t:p><t:p/></d:text-box></d:frame>",
        "<s:style s:name='Base' s:family='paragraph'><s:text-properties f:font-size='24pt' f:font-family='Liberation Sans'/></s:style><s:style s:name='Child' s:family='paragraph' s:parent-style-name='Base'><s:paragraph-properties f:text-align='center'/></s:style><s:style s:name='Em' s:family='text'><s:text-properties f:font-weight='bold'/></s:style><s:style s:name='Graphic' s:family='graphic'><s:graphic-properties d:fill='none' d:stroke='none'/></s:style>",
    );
    let bytes = package(&content, None, None);
    assert!(sniff(&bytes));
    let p = import(&bytes).unwrap();
    let s = &p.slides[0].shapes[0];
    assert!(s.text_box);
    let x = s.xfrm.unwrap();
    assert!((x.x - 72.0).abs() < 1e-8 && (x.y - 72.0).abs() < 1e-8);
    let t = s.text.as_ref().unwrap();
    assert_eq!(t.text(), "A   B & C\tbold\u{b}tail\n");
    assert_eq!(t.paragraphs[0].props.align, Some(text::Align::Center));
    assert!(t.paragraphs[0].runs.iter().any(|r| r.text == "bold" && r.props.bold == Some(true) && r.props.size == Some(24.0)));
}

#[test]
fn embedded_images_are_packaged_and_restored_once() {
    let mut p = deck();
    let media = MediaId(p.alloc_id().unwrap());
    p.media.push(MediaItem {
        id: media,
        name: "original.png".into(),
        content_type: "image/png".into(),
        data: Arc::new(b"contributor-original synthetic image bytes".to_vec()),
        ..Default::default()
    });
    for _ in 0..2 {
        let id = ShapeId(p.alloc_id().unwrap());
        Arc::make_mut(&mut p.slides[0]).shapes.push(Shape {
            id,
            xfrm: Some(Xfrm::new(5.0, 5.0, 20.0, 20.0)),
            kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
            ..Default::default()
        });
    }
    let bytes = export(&p).unwrap();
    let q = import(&bytes).unwrap();
    let images: Vec<_> = q.media.iter().filter(|m| m.content_type.starts_with("image/")).collect();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].data, p.media[0].data);
    let source = q.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap();
    assert_eq!(source.data.as_slice(), bytes.as_slice());
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut manifest = String::new();
    z.by_name("META-INF/manifest.xml").unwrap().read_to_string(&mut manifest).unwrap();
    assert!(manifest.contains("image/png"));
}

#[test]
fn rejects_encrypted_packages_and_wrong_mime() {
    let encrypted =
        "<m:manifest xmlns:m='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'><m:file-entry><m:encryption-data/></m:file-entry></m:manifest>";
    assert!(import(&package(&document("", ""), None, Some(encrypted))).unwrap_err().to_string().contains("password-protected"));
    assert!(!sniff(b"not a zip"));
    assert!(import(b"not a zip").is_err());
}

#[test]
fn refuses_external_images_and_recovers_unsupported_geometry() {
    for shape in [
        "<d:frame><d:image x:href='https://example.invalid/image.png'/></d:frame>",
        "<d:frame><d:image x:href='../image.png'/></d:frame>",
        "<d:g d:transform='skewX(0.4)'><d:rect/></d:g>",
    ] {
        assert!(import(&package(&document(shape, ""), None, None)).is_err(), "{shape}");
    }
    let bytes = package(&document("<d:custom-shape/>", ""), None, None);
    let recovered = import(&bytes).unwrap();
    assert!(matches!(recovered.slides[0].shapes[0].kind, ShapeKind::Opaque { .. }));
    assert!(recovered.tags.iter().any(|(key, _)| key == "odp:warning"));
    assert!(recovered.media.iter().any(|m| m.content_type == "application/vnd.deckcraft.odp-source" && m.data.as_slice() == bytes));
    assert!(export(&recovered).is_err());
}

#[test]
fn rejects_xml_bombs_truncation_and_style_cycles() {
    for content in [
        document("<d:frame><d:text-box><t:p><t:s t:c='999999999'/></t:p></d:text-box></d:frame>", ""),
        document(
            "<d:frame d:style-name='A'/>",
            "<s:style s:name='A' s:family='graphic' s:parent-style-name='B'/><s:style s:name='B' s:family='graphic' s:parent-style-name='A'/>",
        ),
        document(&format!("{}{}", "<d:g>".repeat(70), "</d:g>".repeat(70)), ""),
        "<root>".into(),
        "<!DOCTYPE root [<!ENTITY x 'bad'>]><root>&x;</root>".into(),
    ] {
        assert!(import(&package(&content, None, None)).is_err());
    }
}
