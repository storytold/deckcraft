#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Independently generated, contributor-original ODF interoperability fixtures.
use deckcraft_model::{Fill, Geom, ShapeKind};
use deckcraft_odp::{MIME, import};
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

fn document(styles: &str, shapes: &str) -> String {
    format!(
        "<office:document-content xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:xlink='http://www.w3.org/1999/xlink' xmlns:chart='urn:oasis:names:tc:opendocument:xmlns:chart:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/'><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:presentation><draw:page>{shapes}</draw:page></office:presentation></office:body></office:document-content>"
    )
}
fn package(doc: &str, parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME.as_bytes()),
        ("content.xml", doc.as_bytes()),
        ("META-INF/manifest.xml", b"<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>".as_slice()),
    ]
    .into_iter()
    .chain(parts.iter().copied())
    {
        z.start_file(name, SimpleFileOptions::default()).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap().into_inner()
}

#[test]
fn enhanced_formulas_override_advisory_shape_type_and_support_logical_units() {
    let d = document(
        "",
        "<draw:custom-shape svg:width='72pt' svg:height='36pt'><draw:enhanced-geometry draw:type='rectangle' svg:viewBox='0 0 0 0' draw:modifiers='2' draw:enhanced-path='M 0 0 L ?edge 0 0 ?height Z N'><draw:equation draw:name='edge' draw:formula='logwidth / $0'/><draw:equation draw:name='height' draw:formula='if(1, logheight, 0)'/></draw:enhanced-geometry></draw:custom-shape>",
    );
    let p = import(&package(&d, &[])).unwrap();
    let Geom::Custom { paths } = &p.slides[0].shapes[0].geom else { panic!("expected explicit custom path") };
    assert!((paths[0].w - 2540.0).abs() < 1e-6);
    assert!(paths[0].d.contains("1270"));
    assert!(!p.tags.iter().any(|(k, _)| k == "odp:warning"));
}

#[test]
fn cyclic_formulas_are_bounded_and_preserve_recovery_source() {
    let d = document(
        "",
        "<draw:custom-shape svg:width='72pt' svg:height='36pt'><draw:enhanced-geometry svg:viewBox='0 0 100 100' draw:enhanced-path='M ?a 0 L 10 10'><draw:equation draw:name='a' draw:formula='?a + ?a'/></draw:enhanced-geometry></draw:custom-shape>",
    );
    let bytes = package(&d, &[]);
    let p = import(&bytes).unwrap();
    assert!(matches!(p.slides[0].shapes[0].kind, ShapeKind::Opaque { .. }));
    let backup = p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap();
    assert_eq!(backup.data.as_slice(), bytes);
    assert!(p.tags.iter().any(|(k, _)| k == "odp:warning"));
}

#[test]
fn native_and_svg_gradient_resources_keep_editable_stops() {
    let styles = "<draw:gradient draw:name='g' draw:style='linear' draw:start-color='#102030' draw:end-color='#8090a0' draw:angle='900'/><svg:linearGradient draw:name='s' svg:x1='0%' svg:y1='0%' svg:x2='100%' svg:y2='0%'><svg:stop svg:offset='0' svg:stop-color='#000000'/><svg:stop svg:offset='0.4' svg:stop-color='#ff0000' svg:stop-opacity='0.5'/><svg:stop svg:offset='1' svg:stop-color='#ffffff'/></svg:linearGradient><style:style style:name='a' style:family='graphic'><style:graphic-properties draw:fill='gradient' draw:fill-gradient-name='g'/></style:style><style:style style:name='b' style:family='graphic'><style:graphic-properties draw:fill='gradient' draw:fill-gradient-name='s'/></style:style>";
    let d = document(
        styles,
        "<draw:rect draw:style-name='a' svg:width='100pt' svg:height='50pt'/><draw:rect draw:style-name='b' svg:width='100pt' svg:height='50pt'/>",
    );
    let p = import(&package(&d, &[])).unwrap();
    let Some(Fill::Gradient(a)) = &p.slides[0].shapes[0].fill else { panic!("expected gradient") };
    // A neutral midpoint keeps ODF's linear sRGB interpolation instead of the model's
    // special two-stop PowerPoint ramp. Endpoints and the actual colour field survive.
    assert_eq!(a.stops.iter().map(|s| s.pos).collect::<Vec<_>>(), [0.0, 0.5, 1.0]);
    let scheme = deckcraft_model::Theme::default().colors;
    assert_eq!(a.stops[0].color.resolve(&scheme, None), deckcraft_model::Rgba::rgb(16, 32, 48));
    assert_eq!(a.stops[1].color.resolve(&scheme, None), deckcraft_model::Rgba::rgb(72, 88, 104));
    assert_eq!(a.stops[2].color.resolve(&scheme, None), deckcraft_model::Rgba::rgb(128, 144, 160));
    let Some(Fill::Gradient(b)) = &p.slides[0].shapes[1].fill else { panic!("expected gradient") };
    assert_eq!(b.stops.len(), 3);
    assert!((b.stops[1].color.alpha() - 0.5).abs() < 1e-6);
}

#[test]
fn native_table_merges_repeats_and_chart_ranges_stay_editable() {
    let chart = "<office:document-content xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:chart='urn:oasis:names:tc:opendocument:xmlns:chart:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0'><office:body><office:chart><chart:chart chart:class='chart:bar'><chart:title><text:p>Own chart</text:p></chart:title><chart:plot-area><chart:axis><chart:categories table:cell-range-address='local-table.A2:.A3'/></chart:axis><chart:series chart:label-cell-address='local-table.B1' chart:values-cell-range-address='local-table.B2:.B3'/></chart:plot-area><table:table table:name='local-table'><table:table-row><table:table-cell/><table:table-cell><text:p>Amount</text:p></table:table-cell></table:table-row><table:table-row><table:table-cell><text:p>First</text:p></table:table-cell><table:table-cell office:value='7'/></table:table-row><table:table-row><table:table-cell><text:p>Second</text:p></table:table-cell><table:table-cell office:value='11'/></table:table-row></table:table></chart:chart></office:chart></office:body></office:document-content>";
    let d = document(
        "",
        "<draw:frame svg:width='100pt' svg:height='100pt'><table:table><table:table-column table:number-columns-repeated='2'/><table:table-row><table:table-cell table:number-columns-spanned='2'><text:p>Heading</text:p></table:table-cell><table:covered-table-cell/></table:table-row><table:table-row table:number-rows-repeated='2'><table:table-cell table:number-columns-repeated='2'><text:p>Entry</text:p></table:table-cell></table:table-row></table:table></draw:frame><draw:frame svg:width='100pt' svg:height='100pt'><draw:object xlink:href='./Object%201'/></draw:frame>",
    );
    let p = import(&package(&d, &[("Object 1/content.xml", chart.as_bytes())])).unwrap();
    let ShapeKind::Table(t) = &p.slides[0].shapes[0].kind else { panic!("expected table") };
    assert_eq!(t.rows.len(), 3);
    assert_eq!(t.cols.len(), 2);
    assert!(t.cell(0, 1).unwrap().is_covered());
    let ShapeKind::Chart(c) = &p.slides[0].shapes[1].kind else { panic!("expected chart") };
    assert_eq!(c.categories, ["First", "Second"]);
    assert_eq!(c.series[0].name, "Amount");
    assert_eq!(c.series[0].values, [Some(7.0), Some(11.0)]);
}

#[test]
fn package_paths_cannot_escape_even_when_percent_encoded() {
    for path in ["https://example.org/img.png", "../img.png", "%2e%2e/img.png", "%2Fimg.png", "Pictures%5cimg.png"] {
        let d = document("", &format!("<draw:frame svg:width='10pt' svg:height='10pt'><draw:image xlink:href='{path}'/></draw:frame>"));
        assert!(import(&package(&d, &[])).is_err(), "{path}");
    }
}

#[test]
fn metadata_and_comments_have_model_fields() {
    let d = document(
        "",
        "<office:annotation svg:x='10pt' svg:y='20pt'><dc:creator>Contributor</dc:creator><dc:date>2026-10-15T12:00:00Z</dc:date><text:p>Review note</text:p></office:annotation>",
    );
    let meta = "<office:document-meta xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'><office:meta><dc:title>Own fixture</dc:title><meta:initial-creator>Contributor</meta:initial-creator><meta:keyword>one</meta:keyword><meta:keyword>two</meta:keyword></office:meta></office:document-meta>";
    let p = import(&package(&d, &[("meta.xml", meta.as_bytes())])).unwrap();
    assert_eq!(p.props.title, "Own fixture");
    assert_eq!(p.props.keywords, "one, two");
    assert_eq!(p.slides[0].comments[0].text, "Review note");
    assert_eq!(p.slides[0].comments[0].date, "2026-10-15T12:00:00Z");
}

#[test]
#[ignore = "Private interoperability file, supplied locally through DECKCRAFT_ODP_LOCAL_FIXTURE"]
fn local_private_interoperability_fixture() {
    let path = std::env::var("DECKCRAFT_ODP_LOCAL_FIXTURE").unwrap();
    let bytes = std::fs::read(path).unwrap();
    let p = import(&bytes).unwrap();
    assert_eq!(p.slides.len(), 10);
    eprintln!(
        "private import: {} slides, {} top-level shapes, {} media resources, {} warnings",
        p.slides.len(),
        p.slides.iter().map(|s| s.shapes.len()).sum::<usize>(),
        p.media.len(),
        p.tags.iter().filter(|(k, _)| k == "odp:warning").count()
    );
    assert!(p.slides.iter().all(|s| !s.shapes.is_empty()));
}

#[test]
fn shared_long_text_styles_cannot_amplify_run_allocations() {
    let font = "x".repeat(7000);
    let styles = format!("<style:style style:name='p' style:family='paragraph'><style:text-properties fo:font-family='{font}'/></style:style>");
    let spans = "<text:span>x</text:span>".repeat(5000);
    let d = document(
        &styles,
        &format!(
            "<draw:frame svg:width='100pt' svg:height='100pt'><draw:text-box><text:p text:style-name='p'>{spans}</text:p></draw:text-box></draw:frame>"
        ),
    );
    let err = import(&package(&d, &[])).unwrap_err();
    assert!(err.0.contains("text/style allocation"), "{err}");
}

#[test]
fn padded_empty_tables_cannot_bypass_cell_budget() {
    let d = document(
        "",
        "<draw:frame svg:width='100pt' svg:height='100pt'><table:table><table:table-column table:number-columns-repeated='1000'/><table:table-row table:number-rows-repeated='1000'/></table:table></draw:frame>",
    );
    let err = import(&package(&d, &[])).unwrap_err();
    assert!(err.0.contains("padded table cell count"), "{err}");
}

#[test]
fn picture_crop_opacity_and_adjustments_use_native_fields() {
    let mut image = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    image.extend(100u32.to_be_bytes());
    image.extend(200u32.to_be_bytes());
    let styles = "<style:style style:name='p' style:family='graphic'><style:graphic-properties fo:clip='rect(15pt, 7.5pt, 30pt, 15pt)' draw:image-opacity='40%' draw:luminance='10%' draw:contrast='-20%' draw:color-mode='greyscale'/></style:style>";
    let d = document(
        styles,
        "<draw:frame draw:style-name='p' svg:width='100pt' svg:height='100pt'><draw:image xlink:href='Pictures/original.png'/></draw:frame>",
    );
    let p = import(&package(&d, &[("Pictures/original.png", &image)])).unwrap();
    let ShapeKind::Picture { fill } = &p.slides[0].shapes[0].kind else { panic!("expected native picture") };
    assert_eq!(fill.crop, [0.2, 0.1, 0.1, 0.2]);
    assert_eq!(fill.alpha, Some(0.4));
    assert_eq!(fill.adjust.brightness, 0.1);
    assert_eq!(fill.adjust.contrast, -0.2);
    assert!(fill.adjust.grayscale);
}

#[test]
fn enhanced_rectangle_renders_its_fill_and_white_header_text() {
    use deckcraft_geom::KurboShape as _;
    let styles = "<style:style style:name='colored' style:family='graphic'><style:graphic-properties draw:fill='solid' draw:fill-color='#285078' draw:stroke='none' fo:padding-left='4pt' fo:padding-top='4pt'/></style:style><style:style style:name='header' style:family='paragraph'><style:text-properties fo:color='#ffffff' fo:font-size='18pt' fo:font-family='Liberation Sans'/></style:style>";
    let d = document(
        styles,
        "<draw:custom-shape draw:style-name='colored' svg:x='10pt' svg:y='10pt' svg:width='120pt' svg:height='60pt'><text:p text:style-name='header'>Header</text:p><draw:enhanced-geometry draw:type='ooxml-rect' svg:viewBox='0 0 0 0' draw:text-areas='0 0 ?w ?h' draw:enhanced-path='M 0 0 L ?w 0 ?w ?h 0 ?h Z N'><draw:equation draw:name='w' draw:formula='logwidth'/><draw:equation draw:name='h' draw:formula='logheight'/></draw:enhanced-geometry></draw:custom-shape>",
    );
    let mut p = import(&package(&d, &[])).unwrap();
    p.slide_size = deckcraft_model::Size::new(180.0, 100.0);
    let shape = &p.slides[0].shapes[0];
    let geo = deckcraft_render::shape_geometry(shape, 120.0, 60.0);
    assert_eq!(geo.paths.len(), 1);
    let bounds = geo.paths[0].path.bounding_box();
    assert!(
        (bounds.width() - 120.0).abs() < 1e-7 && (bounds.height() - 60.0).abs() < 1e-7,
        "model parser must retain explicit rectangle commands: {bounds:?}"
    );
    let image = deckcraft_render::render_slide(&p, 0, &Default::default());
    let at = |x: usize, y: usize| &image.pixels[(y * image.width as usize + x) * 4..(y * image.width as usize + x + 1) * 4];
    assert_eq!(at(125, 65), [40, 80, 120, 255], "custom-shape background must actually paint");
    let white = (12..68)
        .flat_map(|y| (12..128).map(move |x| (x, y)))
        .filter(|(x, y)| {
            let px = at(*x, *y);
            px[0] > 240 && px[1] > 240 && px[2] > 240 && px[3] == 255
        })
        .count();
    assert!(white > 10, "white header text must paint on the colored custom shape");
}

#[test]
fn imported_svg_paths_use_the_renderers_explicit_model_grammar() {
    use deckcraft_geom::KurboShape as _;
    let d = document(
        "",
        "<draw:path svg:width='90pt' svg:height='45pt' svg:viewBox='0 0 100 50' svg:d='M0,0L100,0L100,50L0,50Z'/><draw:polygon svg:width='90pt' svg:height='45pt' svg:viewBox='0 0 100 50' draw:points='0,0 100,0 0,50'/>",
    );
    let p = import(&package(&d, &[])).unwrap();
    for shape in &p.slides[0].shapes {
        let geo = deckcraft_render::shape_geometry(shape, 90.0, 45.0);
        let bounds = geo.paths[0].path.bounding_box();
        assert!((bounds.width() - 90.0).abs() < 1e-7 && (bounds.height() - 45.0).abs() < 1e-7, "{bounds:?}");
    }
}
