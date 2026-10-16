#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Synthetic contributor-original semantic export fixtures; no vendor files or artwork.
use deckcraft_model::{
    chart::{Chart, ChartType},
    style::{Gradient, GradientShape, GradientStop, PictureFill},
    *,
};
use deckcraft_odp::export;
use std::{
    io::{Cursor, Read},
    sync::Arc,
};
fn presentation(shapes: Vec<Shape>) -> Presentation {
    let mut p = defaults::blank_presentation(Size::new(720.0, 405.0), Theme::default(), false);
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    p.slides.push(Arc::new(Slide { id: SlideId(100), layout, shapes, show_master_shapes: false, ..Default::default() }));
    p
}
fn part(bytes: &[u8], path: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = String::new();
    zip.by_name(path).unwrap().read_to_string(&mut out).unwrap();
    out
}
fn shape() -> Shape {
    Shape { id: ShapeId(200), xfrm: Some(Xfrm::new(20.0, 30.0, 240.0, 120.0)), line: Some(Line::none()), ..Default::default() }
}
#[test]
fn gradients_charts_notes_comments_metadata_are_native_and_schema_valid() {
    let mut rectangle = shape();
    rectangle.fill = Some(Fill::Gradient(Gradient {
        stops: vec![
            GradientStop { pos: 0.0, color: ColorRef::rgb(Rgba::rgb(20, 40, 60)) },
            GradientStop { pos: 0.4, color: ColorRef::rgb(Rgba::rgb(60, 80, 100)) },
            GradientStop { pos: 1.0, color: ColorRef::rgb(Rgba::rgb(100, 120, 140)) },
        ],
        shape: GradientShape::Linear { angle: 35.0, scaled: true },
        rotate_with_shape: true,
        tile: [0.0; 4],
    }));
    rectangle.hidden = true;
    let mut chart = shape();
    chart.id = ShapeId(201);
    chart.kind = ShapeKind::Chart(Box::new(Chart::sample(ChartType::Column)));
    let mut p = presentation(vec![rectangle, chart]);
    p.props.title = "Original synthetic fixture".into();
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.notes = TextBody::from_text("Speaker note");
    slide.comments.push(Comment {
        author: "Original Author".into(),
        text: "Review note".into(),
        date: "2026-10-15T12:00:00Z".into(),
        ..Default::default()
    });
    let bytes = export(&p).unwrap();
    let content = part(&bytes, "content.xml");
    let styles = part(&bytes, "styles.xml");
    let chart = part(&bytes, "Object1/content.xml");
    let meta = part(&bytes, "meta.xml");
    assert!(styles.contains("svg:linearGradient"));
    assert_eq!(styles.matches("<svg:stop ").count(), 3);
    assert!(styles.contains("svg:offset=\"0.4\""));
    assert!(content.contains("draw:layer=\"Hidden\""));
    assert!(content.contains("<presentation:notes>"));
    assert!(content.contains("Review<text:s/>note"));
    assert!(meta.contains("Original synthetic fixture"));
    assert!(chart.contains("chart:class=\"chart:bar\""));
    assert!(chart.contains("office:value=\"4.2\""));
    assert!(chart.contains("Series<text:s/>1"));
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_EXPORT_QA_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        for (path, data) in [("content.xml", content), ("styles.xml", styles), ("chart.xml", chart), ("meta.xml", meta)] {
            std::fs::write(std::path::Path::new(&dir).join(path), data).unwrap();
        }
        std::fs::write(std::path::Path::new(&dir).join("native.odp"), bytes).unwrap();
    }
}
#[test]
fn cropped_adjusted_picture_retains_original_bytes_and_editable_properties() {
    // Header-only data exercises dimension lookup without decoding or copying any artwork.
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend_from_slice(&400u32.to_be_bytes());
    png.extend_from_slice(&200u32.to_be_bytes());
    let mut s = shape();
    let mut fill = PictureFill { media: MediaId(9), crop: [0.1, 0.2, 0.1, 0.0], alpha: Some(0.6), ..Default::default() };
    fill.adjust.brightness = 0.2;
    s.kind = ShapeKind::Picture { fill };
    let mut p = presentation(vec![s]);
    p.media.push(MediaItem { id: MediaId(9), content_type: "image/png".into(), data: Arc::new(png.clone()), ..Default::default() });
    let bytes = export(&p).unwrap();
    let content = part(&bytes, "content.xml");
    assert!(content.contains("fo:clip=\"rect(30pt, 30pt, 0pt, 30pt)\""));
    assert!(content.contains("draw:luminance=\"20%\""));
    assert!(content.contains("draw:image-opacity=\"60%\""));
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut data = Vec::new();
    z.by_name("Pictures/media9.png").unwrap().read_to_end(&mut data).unwrap();
    assert_eq!(data, png);
}
#[test]
fn unsupported_opaque_objects_refuse_instead_of_disappearing() {
    let mut s = shape();
    s.kind = ShapeKind::Opaque { xml: "<not-an-odf-object/>".into(), preview: None, label: "Original source".into(), parts: vec![] };
    let e = export(&presentation(vec![s])).unwrap_err().to_string();
    assert!(e.contains("cannot be exported"));
}
#[test]
fn invalid_gradient_positions_refuse() {
    let mut s = shape();
    s.fill = Some(Fill::Gradient(Gradient {
        stops: vec![GradientStop { pos: f64::NAN, color: ColorRef::rgb(Rgba::BLACK) }],
        shape: GradientShape::Linear { angle: 0.0, scaled: true },
        rotate_with_shape: true,
        tile: [0.0; 4],
    }));
    assert!(export(&presentation(vec![s])).is_err());
}

#[test]
fn nonsquare_star_preserves_its_actual_outline() {
    use deckcraft_geom::{Affine, BezPath, PathEl, preset};
    let mut star = shape();
    star.geom = Geom::preset("star5");
    star.xfrm = Some(Xfrm::new(20.0, 30.0, 500.0, 100.0));
    let bytes = export(&presentation(vec![star])).unwrap();
    let restored = deckcraft_odp::import(&bytes).unwrap();
    let Geom::Custom { paths } = &restored.slides[0].shapes[0].geom else { panic!("enhanced native outline must remain inspectable") };
    let actual = Affine::scale_non_uniform(500.0 / paths[0].w, 100.0 / paths[0].h) * BezPath::from_svg(&paths[0].d).unwrap();
    let expected = preset::build("star5", 500.0, 100.0, &[]).unwrap();
    let expected = &expected.paths[0].path;
    fn coordinates(el: &PathEl) -> Vec<f64> {
        match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => vec![p.x, p.y],
            PathEl::QuadTo(a, b) => vec![a.x, a.y, b.x, b.y],
            PathEl::CurveTo(a, b, c) => vec![a.x, a.y, b.x, b.y, c.x, c.y],
            PathEl::ClosePath => vec![],
        }
    }
    assert_eq!(actual.elements().len(), expected.elements().len());
    for (a, b) in actual.elements().iter().zip(expected.elements()) {
        let a = coordinates(a);
        let b = coordinates(b);
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(b) {
            assert!((a - b).abs() < 1e-7, "outline coordinate {a} differs from {b}");
        }
    }
}

#[test]
fn custom_shape_text_and_lists_precede_enhanced_geometry() {
    let mut s = shape();
    s.geom = Geom::preset("roundRect");
    let mut body = TextBody::from_text("Original shape heading\nOriginal list item");
    body.paragraphs[1].props.bullet = Some(deckcraft_model::text::Bullet::Char { char: "•".into() });
    s.text = Some(body);
    let bytes = export(&presentation(vec![s])).unwrap();
    let xml = part(&bytes, "content.xml");
    assert!(xml.find("<text:p ").unwrap() < xml.find("<draw:enhanced-geometry ").unwrap());
    assert!(xml.find("<text:list ").unwrap() < xml.find("<draw:enhanced-geometry ").unwrap());
    let restored = deckcraft_odp::import(&bytes).unwrap();
    assert_eq!(restored.slides[0].shapes[0].text.as_ref().unwrap().text(), "Original shape heading\nOriginal list item");
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_EXPORT_QA_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("custom-text.xml"), xml).unwrap();
    }
}
