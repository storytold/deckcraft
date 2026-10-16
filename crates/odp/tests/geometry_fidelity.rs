//! Contributor-original OASIS ODF 1.3 geometry/paint regressions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_geom::Point;
use deckcraft_model::{
    style::{Gradient, GradientShape, GradientStop, PictureFill, PictureMode},
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

fn package(styles: &str, shape: &str, parts: &[(&str, &[u8])]) -> Vec<u8> {
    let content = format!(
        "<office:document-content xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:xlink='http://www.w3.org/1999/xlink'><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:presentation><draw:page>{shape}</draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME.as_bytes()),
        ("content.xml", content.as_bytes()),
        ("META-INF/manifest.xml", b"<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>".as_slice()),
    ]
    .into_iter()
    .chain(parts.iter().copied())
    {
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}
fn style(attrs: &str, resource: &str) -> String {
    format!("{resource}<style:style style:name='paint' style:family='graphic'><style:graphic-properties {attrs}/></style:style>")
}
fn gradient_resource(tag: &str, attrs: &str) -> String {
    format!(
        "<svg:{tag} draw:name='g' {attrs}><svg:stop svg:offset='0' svg:stop-color='#102040'/><svg:stop svg:offset='1' svg:stop-color='#f0b080'/></svg:{tag}>"
    )
}
fn gradient(p: &Presentation) -> &Gradient {
    let Some(Fill::Gradient(g)) = &p.slides[0].shapes[0].fill else { panic!("expected editable gradient") };
    g
}
fn assert_no_warning(p: &Presentation) {
    assert!(!p.tags.iter().any(|(k, _)| k == "odp:warning"), "{:?}", p.tags);
}
fn field(g: &Gradient, p: Point, w: f64, h: f64) -> f64 {
    let GradientShape::Linear { angle, .. } = g.shape else { panic!("linear") };
    let [l, t, r, b] = g.tile;
    let (tw, th) = (w * (1. - l - r), h * (1. - t - b));
    let (dx, dy) = (angle.to_radians().cos(), angle.to_radians().sin());
    let (cx, cy) = ((l + 1. - r) * w / 2., (t + 1. - b) * h / 2.);
    0.5 + ((p.x - cx) * dx + (p.y - cy) * dy) / (tw * dx.abs() + th * dy.abs())
}
#[test]
fn svg_affine_linear_gradient_preserves_the_entire_scalar_field() {
    // SVG transform list postmultiplies. This list maps (u,v) to (.1-v,.2+2u).
    let res = gradient_resource(
        "linearGradient",
        "svg:x1='0' svg:y1='0' svg:x2='1' svg:y2='1' svg:gradientTransform='translate(0.1 0.2) rotate(90) scale(2 1)'",
    );
    let p = import(&package(
        &style("draw:fill='gradient' draw:fill-gradient-name='g'", &res),
        "<draw:rect draw:style-name='paint' svg:width='200pt' svg:height='100pt'/>",
        &[],
    ))
    .unwrap();
    assert_no_warning(&p);
    for (x, y) in [(0., 0.), (50., 20.), (120., 75.), (200., 100.)] {
        let expected = ((y / 100. - 0.2) / 2. + 0.1 - x / 200.) / 2.;
        assert!((field(gradient(&p), Point::new(x, y), 200., 100.) - expected).abs() < 1e-10);
    }
}
#[test]
fn radial_gradient_retains_transformed_center_radius_and_focus() {
    let res =
        gradient_resource("radialGradient", "svg:cx='0.3' svg:cy='0.2' svg:fx='0.25' svg:fy='0.15' svg:r='0.2' svg:gradientTransform='scale(1 2)'");
    let p = import(&package(
        &style("draw:fill='gradient' draw:fill-gradient-name='g'", &res),
        "<draw:rect draw:style-name='paint' svg:width='200pt' svg:height='100pt'/>",
        &[],
    ))
    .unwrap();
    assert_no_warning(&p);
    let g = gradient(&p);
    let [l, t, r, b] = g.tile;
    assert!(((l + 1. - r) * 100. - 60.).abs() < 1e-10);
    assert!(((t + 1. - b) * 50. - 40.).abs() < 1e-10);
    assert!(((200. * (1. - l - r)).hypot(100. * (1. - t - b)) / 2. - 40.).abs() < 1e-10);
    let GradientShape::Path { focus, .. } = &g.shape else { panic!("radial") };
    assert!((focus[0] - 0.25).abs() < 1e-10 && (focus[1] - 0.3).abs() < 1e-10);
}
#[test]
fn unsupported_radial_ellipse_and_spread_preserve_source_with_warning() {
    for (tag, attrs) in [("radialGradient", "svg:r='0.25'"), ("linearGradient", "svg:spreadMethod='reflect'")] {
        let res = gradient_resource(tag, attrs);
        let bytes = package(
            &style("draw:fill='gradient' draw:fill-gradient-name='g'", &res),
            "<draw:rect draw:style-name='paint' svg:width='200pt' svg:height='100pt'/>",
            &[],
        );
        let p = import(&bytes).unwrap();
        assert!(p.tags.iter().any(|(k, _)| k == "odp:warning"));
        assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), bytes);
        assert!(matches!(p.slides[0].shapes[0].fill, Some(Fill::None)));
    }
}
fn deck(fill: Fill) -> Presentation {
    let mut p = defaults::blank_presentation(Size::new(280., 160.), Theme::default(), false);
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    p.slides.push(Arc::new(Slide {
        id: SlideId(5),
        layout,
        show_master_shapes: false,
        shapes: vec![Shape {
            id: ShapeId(9),
            xfrm: Some(Xfrm::new(20., 20., 240., 120.)),
            geom: Geom::preset("rect"),
            fill: Some(fill),
            line: Some(Line::none()),
            ..Default::default()
        }],
        ..Default::default()
    }));
    p
}
fn color_stops() -> Vec<GradientStop> {
    vec![
        GradientStop { pos: 0., color: ColorRef::rgb(Rgba::rgb(10, 30, 60)) },
        GradientStop { pos: 0.4, color: ColorRef::rgb(Rgba::rgb(180, 70, 50)) },
        GradientStop { pos: 1., color: ColorRef::rgb(Rgba::rgb(220, 230, 240)) },
    ]
}
fn assert_same_render(p: &Presentation, q: &Presentation) {
    let a = deckcraft_render::render_slide(p, 0, &Default::default());
    let b = deckcraft_render::render_slide(q, 0, &Default::default());
    assert_eq!((a.width, a.height), (b.width, b.height));
    let max = a.pixels.iter().zip(&b.pixels).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
    assert!(max <= 2, "render changed by {max} channel levels");
}
#[test]
fn nonsquare_tiled_gradients_export_reimport_without_pixel_loss() {
    for (i, shape) in
        [GradientShape::Linear { angle: 35., scaled: false }, GradientShape::Path { path: "circle".into(), focus: [0.35, 0.3, 0.65, 0.7] }]
            .into_iter()
            .enumerate()
    {
        let p = deck(Fill::Gradient(Gradient { shape, stops: color_stops(), rotate_with_shape: true, tile: [-0.2, 0.1, 0.3, -0.15] }));
        let bytes = export(&p).unwrap();
        let q = import(&bytes).unwrap();
        assert_no_warning(&q);
        assert_same_render(&p, &q);
        if let Ok(dir) = std::env::var("DECKCRAFT_ODP_GEOMETRY_QA_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            for name in ["content.xml", "styles.xml"] {
                std::fs::write(std::path::Path::new(&dir).join(format!("gradient{i}-{name}")), part(&bytes, name)).unwrap();
            }
        }
    }
}
fn png() -> Vec<u8> {
    let mut pixels = Vec::new();
    for y in 0..4 {
        for x in 0..8 {
            pixels.extend(if x < 4 && y < 2 { [240, 30, 20, 255] } else { [20, 80, 220, 255] });
        }
    }
    deckcraft_render::Image { width: 8, height: 4, pixels }.to_png_at(192.)
}
#[test]
fn bitmap_tile_offsets_alignment_size_and_embedded_density_roundtrip() {
    let pf = PictureFill {
        media: MediaId(20),
        mode: PictureMode::Tile { tx: 15., ty: -30., sx: 10., sy: 8., flip: String::new(), align: "br".into() },
        ..Default::default()
    };
    let mut p = deck(Fill::Picture(pf));
    p.media.push(MediaItem { id: MediaId(20), content_type: "image/png".into(), data: Arc::new(png()), ..Default::default() });
    let bytes = export(&p).unwrap();
    let content = part(&bytes, "content.xml");
    assert!(content.contains("draw:fill-image-width=\"60pt\""));
    assert!(content.contains("draw:fill-image-height=\"24pt\""));
    assert!(content.contains("draw:fill-image-ref-point-y=\"-50%\""));
    let q = import(&bytes).unwrap();
    assert_no_warning(&q);
    let Some(Fill::Picture(pf)) = &q.slides[0].shapes[0].fill else { panic!("bitmap") };
    assert_eq!(pf.mode, PictureMode::Tile { tx: 15., ty: -30., sx: 10., sy: 8., flip: String::new(), align: "br".into() });
    assert_same_render(&p, &q);
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_GEOMETRY_QA_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["content.xml", "styles.xml"] {
            std::fs::write(std::path::Path::new(&dir).join(format!("bitmap-{name}")), part(&bytes, name)).unwrap();
        }
    }
}
#[test]
fn default_bitmap_logical_tile_uses_embedded_density() {
    let res = "<draw:fill-image draw:name='image' xlink:href='Pictures/tile.png'/>";
    let p = import(&package(
        &style("draw:fill='bitmap' draw:fill-image-name='image' style:repeat='repeat'", res),
        "<draw:rect draw:style-name='paint' svg:width='200pt' svg:height='100pt'/>",
        &[("Pictures/tile.png", &png())],
    ))
    .unwrap();
    assert_no_warning(&p);
    let Some(Fill::Picture(pf)) = &p.slides[0].shapes[0].fill else { panic!("bitmap") };
    let PictureMode::Tile { sx, sy, .. } = pf.mode else { panic!("tile") };
    assert!((sx - 0.5).abs() < 0.003 && (sy - 0.5).abs() < 0.003);
}
#[test]
fn geometry_equations_use_resolved_fill_and_line_flags_and_lazy_if() {
    let paint = style("draw:fill='none' draw:stroke='none'", "");
    let s = "<draw:custom-shape draw:style-name='paint' svg:width='100pt' svg:height='50pt'><draw:enhanced-geometry svg:viewBox='0 0 100 50' draw:path-stretchpoint-x='25' draw:path-stretchpoint-y='30' draw:enhanced-path='M ?x ?y L ?flag 0 100 50 Z N'><draw:equation draw:name='x' draw:formula='xstretch'/><draw:equation draw:name='y' draw:formula='ystretch'/><draw:equation draw:name='flag' draw:formula='if(hasfill, sqrt(-1), if(hasstroke, 1/0, 17))'/></draw:enhanced-geometry></draw:custom-shape>";
    let p = import(&package(&paint, s, &[])).unwrap();
    assert_no_warning(&p);
    let Geom::Custom { paths } = &p.slides[0].shapes[0].geom else { panic!("custom") };
    assert!(paths[0].d.starts_with("M 25 30 L 17 0"), "{}", paths[0].d);
}
#[test]
fn active_stretch_points_preserve_geometry_instead_of_scaling_it() {
    let s = "<draw:custom-shape svg:width='200pt' svg:height='50pt'><draw:enhanced-geometry svg:viewBox='0 0 100 50' draw:path-stretchpoint-x='25' draw:enhanced-path='M 0 0 L 100 50'/></draw:custom-shape>";
    let p = import(&package("", s, &[])).unwrap();
    assert!(matches!(p.slides[0].shapes[0].kind, ShapeKind::Opaque { .. }));
    assert!(p.tags.iter().any(|(k, _)| k == "odp:warning"));
}

#[test]
fn exported_nonsquare_linear_gradient_has_standards_based_physical_endpoints() {
    // A 240×120 shape with tile insets (.1,.2,.3,.4) has a 144×48 tile,
    // centered at (96,48). A 3:4 direction gives endpoints (38.4,4.8), (153.6,91.2).
    // In objectBoundingBox plus scale(1,2), both coordinate units equal 240pt.
    let p = deck(Fill::Gradient(Gradient {
        shape: GradientShape::Linear { angle: 3.0_f64.atan2(4.0).to_degrees(), scaled: false },
        stops: color_stops(),
        rotate_with_shape: true,
        tile: [0.1, 0.2, 0.3, 0.4],
    }));
    let styles = part(&export(&p).unwrap(), "styles.xml");
    let opening = styles.split("<svg:linearGradient ").nth(1).unwrap().split('>').next().unwrap();
    let attr = |name: &str| -> f64 {
        opening.split(&format!("{name}=\"")).nth(1).unwrap().split('"').next().unwrap().trim_end_matches('%').parse().unwrap()
    };
    for (name, expected) in [("svg:x1", 16.0), ("svg:y1", 2.0), ("svg:x2", 64.0), ("svg:y2", 38.0)] {
        assert!((attr(name) - expected).abs() < 1e-9, "{name}: {opening}");
    }
    assert!(opening.contains("svg:gradientTransform=\"scale(1 2)\""));
}

#[test]
fn geometry_equation_flags_follow_resolved_graphic_style() {
    let s = "<draw:custom-shape draw:style-name='paint' svg:width='100pt' svg:height='50pt'><draw:enhanced-geometry svg:viewBox='0 0 100 50' draw:enhanced-path='M ?flags 0 L 100 50'><draw:equation draw:name='flags' draw:formula='hasfill * 10 + hasstroke'/></draw:enhanced-geometry></draw:custom-shape>";
    for (attrs, expected) in [
        ("draw:fill='none' draw:stroke='none'", 0),
        ("draw:fill='solid' draw:fill-color='#102030' draw:stroke='none'", 10),
        ("draw:fill='none' draw:stroke='solid' svg:stroke-color='#102030'", 1),
        ("draw:fill='solid' draw:fill-color='#102030' draw:stroke='solid' svg:stroke-color='#102030'", 11),
    ] {
        let p = import(&package(&style(attrs, ""), s, &[])).unwrap();
        assert_no_warning(&p);
        let Geom::Custom { paths } = &p.slides[0].shapes[0].geom else { panic!("custom") };
        assert!(paths[0].d.starts_with(&format!("M {expected} 0 ")), "{}", paths[0].d);
    }
}

#[test]
fn line_markers_refuse_until_exact_geometry_and_placement_are_supported() {
    use deckcraft_model::style::LineEnd;
    for kind in ["triangle", "stealth", "diamond", "oval", "arrow", "unknown"] {
        for head in [true, false] {
            let mut p = deck(Fill::None);
            let mut line = Line::solid(ColorRef::rgb(Rgba::BLACK), 0.5);
            let end = Some(LineEnd { kind: kind.into(), w: "sm".into(), len: "lg".into() });
            if head {
                line.head = end;
            } else {
                line.tail = end;
            }
            Arc::make_mut(&mut p.slides[0]).shapes[0].line = Some(line);
            assert!(export(&p).unwrap_err().0.contains("line markers"));
        }
    }
    let mut p = deck(Fill::None);
    let mut line = Line::solid(ColorRef::rgb(Rgba::BLACK), 1.0);
    line.head = Some(LineEnd { kind: "none".into(), w: "med".into(), len: "med".into() });
    Arc::make_mut(&mut p.slides[0]).shapes[0].line = Some(line);
    assert!(export(&p).is_ok());
}

#[test]
fn native_odf_two_color_gradient_uses_standard_color_ramp() {
    let resource =
        "<draw:gradient draw:name='g' draw:style='linear' draw:start-color='#000000' draw:end-color='#ffffff' draw:angle='900' draw:border='0%'/>";
    let p = import(&package(
        &style("draw:fill='gradient' draw:fill-gradient-name='g'", resource),
        "<draw:rect draw:style-name='paint' svg:width='200pt' svg:height='100pt'/>",
        &[],
    ))
    .unwrap();
    assert_no_warning(&p);
    let image = deckcraft_render::render_slide(&p, 0, &Default::default());
    for (x, expected) in [(50, 64), (100, 128), (150, 192)] {
        assert!(image.pixel(x, 50)[0].abs_diff(expected) <= 2);
    }
}

#[test]
fn exported_native_two_stop_ramp_preserves_rgb_and_alpha_pixels() {
    let p = deck(Fill::Gradient(Gradient {
        shape: GradientShape::Linear { angle: 0.0, scaled: false },
        stops: vec![
            GradientStop { pos: 0.0, color: ColorRef::rgb(Rgba::BLACK).with(ColorTransform::Alpha(0)) },
            GradientStop { pos: 1.0, color: ColorRef::rgb(Rgba::WHITE) },
        ],
        rotate_with_shape: true,
        tile: [0.0; 4],
    }));
    let bytes = export(&p).unwrap();
    assert_eq!(part(&bytes, "styles.xml").matches("<svg:stop ").count(), 17);
    let q = import(&bytes).unwrap();
    assert_no_warning(&q);
    assert_same_render(&p, &q);
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_GEOMETRY_QA_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["content.xml", "styles.xml"] {
            std::fs::write(std::path::Path::new(&dir).join(format!("native-ramp-{name}")), part(&bytes, name)).unwrap();
        }
    }
}
