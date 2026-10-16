//! Second-wave independent semantic/rendering checks; contributor-original fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    style::{Gradient, GradientShape, GradientStop, PictureMode},
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:xlink='http://www.w3.org/1999/xlink' xmlns:chart='urn:oasis:names:tc:opendocument:xmlns:chart:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0'";
fn package(styles: &str, shape: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS}><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:presentation><draw:page>{shape}</draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME.as_bytes()),
        ("content.xml", content.as_bytes()),
        ("META-INF/manifest.xml", b"<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>".as_slice()),
    ]
    .into_iter()
    .chain(extra.iter().copied())
    {
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn paint(resource: &str, attrs: &str) -> String {
    format!("{resource}<style:style style:name='paint' style:family='graphic'><style:graphic-properties {attrs}/></style:style>")
}
const RECT: &str = "<draw:rect draw:style-name='paint' svg:width='200pt' svg:height='100pt'/>";
fn gradient(opacity: &str) -> Vec<u8> {
    let resource = format!(
        "<svg:linearGradient draw:name='g'><svg:stop svg:offset='0' svg:stop-color='#000000' {opacity}/><svg:stop svg:offset='1' svg:stop-color='#ffffff'/></svg:linearGradient>"
    );
    package(&paint(&resource, "draw:fill='gradient' draw:fill-gradient-name='g'"), RECT, &[])
}
#[test]
fn imported_svg_two_stop_gradient_uses_standard_srgb_color_interpolation() {
    let p = import(&gradient("")).unwrap();
    let image = deckcraft_render::render_slide(&p, 0, &Default::default());
    for (x, expected) in [(50, 64), (100, 128), (150, 192)] {
        let got = image.pixel(x, 50);
        assert!((i32::from(got[0]) - expected).abs() <= 2, "SVG sRGB interpolation at {x}: {got:?}, expected {expected}");
    }
}
#[test]
fn svg_stop_opacity_interpolates_independently_from_unpremultiplied_rgb() {
    let p = import(&gradient("svg:stop-opacity='0'")).unwrap();
    let image = deckcraft_render::render_slide(&p, 0, &Default::default());
    // At half position: gray(128), alpha .5 over white = approximately192.
    let got = image.pixel(100, 50);
    assert!((i32::from(got[0]) - 192).abs() <= 3, "SVG independent RGB/alpha interpolation: {got:?}");
}
#[test]
fn exported_svg_stop_interpolation_matches_native_two_color_rendering() {
    let mut p = defaults::blank_presentation(Size::new(200., 100.), Theme::default(), false);
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    let g = Gradient {
        shape: GradientShape::Linear { angle: 0., scaled: false },
        stops: vec![GradientStop { pos: 0., color: ColorRef::rgb(Rgba::BLACK) }, GradientStop { pos: 1., color: ColorRef::rgb(Rgba::WHITE) }],
        rotate_with_shape: true,
        tile: [0.; 4],
    };
    p.slides.push(Arc::new(Slide {
        id: SlideId(20),
        layout,
        show_master_shapes: false,
        shapes: vec![Shape {
            id: ShapeId(30),
            xfrm: Some(Xfrm::new(0., 0., 200., 100.)),
            fill: Some(Fill::Gradient(g)),
            line: Some(Line::none()),
            ..Default::default()
        }],
        ..Default::default()
    }));
    let native = deckcraft_render::render_slide(&p, 0, &Default::default()).pixel(100, 50)[0];
    let bytes = export(&p).unwrap();
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    z.by_name("styles.xml").unwrap().read_to_string(&mut xml).unwrap();
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut stops = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            quick_xml::events::Event::Empty(e) if e.name().as_ref() == b"svg:stop" => {
                let attrs = e
                    .attributes()
                    .map(|a| {
                        let a = a.unwrap();
                        (
                            String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                            a.normalized_value(quick_xml::XmlVersion::Implicit1_0).unwrap().into_owned(),
                        )
                    })
                    .collect::<std::collections::BTreeMap<_, _>>();
                stops.push((attrs["svg:offset"].parse::<f64>().unwrap(), u8::from_str_radix(&attrs["svg:stop-color"][1..3], 16).unwrap()));
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    let t = 100.5 / 200.;
    let pair = stops.windows(2).find(|s| s[0].0 <= t && t <= s[1].0).unwrap();
    let share = (t - pair[0].0) / (pair[1].0 - pair[0].0);
    let external = f64::from(pair[0].1) + (f64::from(pair[1].1) - f64::from(pair[0].1)) * share;
    assert!((external - f64::from(native)).abs() <= 3., "standard SVG consumer midpoint {external}, native renderer {native}");
}
#[test]
fn bitmap_resource_dimension_estimates_do_not_override_actual_image_density() {
    let png = deckcraft_render::Image { width: 8, height: 4, pixels: [120, 30, 210, 255].repeat(32) }.to_png_at(192.);
    let resource = "<draw:fill-image draw:name='image' xlink:href='Pictures/tile.png' svg:width='999pt' svg:height='999pt'/>";
    let p = import(&package(
        &paint(resource, "draw:fill='bitmap' draw:fill-image-name='image' style:repeat='repeat'"),
        RECT,
        &[("Pictures/tile.png", &png)],
    ))
    .unwrap();
    let Some(Fill::Picture(fill)) = &p.slides[0].shapes[0].fill else { panic!("editable bitmap fill") };
    let PictureMode::Tile { sx, sy, .. } = fill.mode else { panic!("tile mode") };
    assert!((sx - 0.5).abs() < 0.003 && (sy - 0.5).abs() < 0.003, "ODF resource dimensions override SVG estimate: sx={sx}, sy={sy}");
}

fn assert_source_guarded(bytes: &[u8]) {
    let p = import(bytes).unwrap();
    assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"), "unrepresented source semantics must warn: {:?}", p.tags);
    assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), bytes);
    assert!(export(&p).is_err(), "retaining an archive does not preserve edited export semantics");
}
#[test]
fn notes_ruby_annotations_are_reported_before_flattening() {
    let notes = "<presentation:notes xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0'><draw:frame svg:width='200pt' svg:height='100pt'><draw:text-box><text:p><text:ruby><text:ruby-base>Base</text:ruby-base><text:ruby-text>Annotation</text:ruby-text></text:ruby></text:p></draw:text-box></draw:frame></presentation:notes>";
    assert_source_guarded(&package("", notes, &[]));
}
#[test]
fn notes_used_paragraph_styles_report_unsupported_hyphenation() {
    let styles = "<style:style style:name='hyphen' style:family='paragraph'><style:text-properties fo:hyphenate='true'/></style:style>";
    let notes = "<presentation:notes xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0'><draw:frame svg:width='200pt' svg:height='100pt'><draw:text-box><text:p text:style-name='hyphen'>Original notes with automatic hyphenation</text:p></draw:text-box></draw:frame></presentation:notes>";
    assert_source_guarded(&package(styles, notes, &[]));
}
#[test]
fn reversed_value_axis_is_reported_when_the_model_cannot_represent_it() {
    let chart = format!(
        "<office:document-content {NS}><office:automatic-styles><style:style style:name='reverse' style:family='chart'><style:chart-properties chart:reverse-direction='true'/></style:style></office:automatic-styles><office:body><office:chart><chart:chart chart:class='chart:line'><chart:plot-area><chart:axis chart:dimension='y' chart:name='value' chart:style-name='reverse'/><chart:series chart:values-cell-range-address='local-table.A1:.A2'/></chart:plot-area><table:table table:name='local-table'><table:table-row><table:table-cell office:value='1'/></table:table-row><table:table-row><table:table-cell office:value='9'/></table:table-row></table:table></chart:chart></office:chart></office:body></office:document-content>"
    );
    assert_source_guarded(&package(
        "",
        "<draw:frame svg:width='200pt' svg:height='100pt'><draw:object xlink:href='Object'/></draw:frame>",
        &[("Object/content.xml", chart.as_bytes())],
    ));
}

#[test]
fn imported_sheared_gradient_matches_svg_inverse_coordinate_samples() {
    let resource = "<svg:linearGradient draw:name='g' svg:x1='0.2' svg:y1='0.1' svg:x2='0.8' svg:y2='0.6' svg:gradientTransform='matrix(1 .4 .6 1.2 .1 -.2)'><svg:stop svg:offset='0' svg:stop-color='#000000'/><svg:stop svg:offset='.5' svg:stop-color='#808080'/><svg:stop svg:offset='1' svg:stop-color='#ffffff'/></svg:linearGradient>";
    let p = import(&package(&paint(resource, "draw:fill='gradient' draw:fill-gradient-name='g'"), RECT, &[])).unwrap();
    let image = deckcraft_render::render_slide(&p, 0, &Default::default());
    for (x, y) in [(100, 50), (160, 75), (60, 25)] {
        // Independently invert the SVG matrix in bounding-box coordinates, then
        // project onto the untransformed (.6,.5) gradient vector.
        let (px, py) = ((f64::from(x) + 0.5) / 200. - 0.1, (f64::from(y) + 0.5) / 100. + 0.2);
        let (u, v) = (1.25 * px - 0.625 * py, -5. / 12. * px + 25. / 24. * py);
        let t = (((u - 0.2) * 0.6 + (v - 0.1) * 0.5) / 0.61).clamp(0., 1.);
        let expected = if t <= 0.5 { t * 256. } else { 128. + (t - 0.5) * 254. };
        let got = image.pixel(x, y)[0];
        assert!((f64::from(got) - expected).abs() <= 3., "SVG scalar-field pixel ({x},{y}): {got}, expected {expected}");
    }
}
#[test]
fn omitted_chart_domains_use_the_latest_preceding_domain_of_the_same_class() {
    let chart = format!(
        "<office:document-content {NS}><office:body><office:chart><chart:chart chart:class='chart:scatter'><chart:plot-area><chart:series chart:values-cell-range-address='data.C1:.C2'><chart:domain table:cell-range-address='data.A1:.A2'/></chart:series><chart:series chart:values-cell-range-address='data.C1:.C2'><chart:domain table:cell-range-address='data.B1:.B2'/></chart:series><chart:series chart:values-cell-range-address='data.C1:.C2'/></chart:plot-area><table:table table:name='data'><table:table-row><table:table-cell office:value='1'/><table:table-cell office:value='3'/><table:table-cell office:value='9'/></table:table-row><table:table-row><table:table-cell office:value='2'/><table:table-cell office:value='4'/><table:table-cell office:value='8'/></table:table-row></table:table></chart:chart></office:chart></office:body></office:document-content>"
    );
    let p = import(&package(
        "",
        "<draw:frame svg:width='200pt' svg:height='100pt'><draw:object xlink:href='Object'/></draw:frame>",
        &[("Object/content.xml", chart.as_bytes())],
    ))
    .unwrap();
    let ShapeKind::Chart(chart) = &p.slides[0].shapes[0].kind else { panic!("editable chart") };
    assert_eq!(chart.series[0].x_values, [Some(1.), Some(2.)]);
    assert_eq!(chart.series[1].x_values, [Some(3.), Some(4.)]);
    assert_eq!(chart.series[2].x_values, [Some(3.), Some(4.)]);
}

#[test]
fn editable_table_dash_and_double_borders_survive_export_or_are_refused() {
    let style = "<style:style style:name='cell' style:family='table-cell'><style:table-cell-properties fo:border-left='2pt dashed #345678' fo:border-right='3pt double #123456'/></style:style>";
    let source = package(
        style,
        "<draw:frame svg:width='200pt' svg:height='100pt'><table:table><table:table-row><table:table-cell table:style-name='cell'><text:p>Original cell</text:p></table:table-cell></table:table-row></table:table></draw:frame>",
        &[],
    );
    let p = import(&source).unwrap();
    if let Ok(bytes) = export(&p) {
        let q = import(&bytes).unwrap();
        let ShapeKind::Table(before) = &p.slides[0].shapes[0].kind else { panic!("table") };
        let ShapeKind::Table(after) = &q.slides[0].shapes[0].kind else { panic!("table") };
        assert_eq!(
            after.rows[0].cells[0].borders[0].as_ref().unwrap().dash,
            before.rows[0].cells[0].borders[0].as_ref().unwrap().dash,
            "dashed border must not silently become solid"
        );
        assert_eq!(
            after.rows[0].cells[0].borders[1].as_ref().unwrap().compound,
            before.rows[0].cells[0].borders[1].as_ref().unwrap().compound,
            "double border must not silently become single"
        );
    }
}
#[test]
fn editable_table_vertical_and_diagonal_properties_survive_export_or_are_refused() {
    let style = "<style:style style:name='cell' style:family='table-cell'><style:table-cell-properties style:writing-mode='tb-rl' style:diagonal-tl-br='1pt solid #345678'/></style:style>";
    let source = package(
        style,
        "<draw:frame svg:width='200pt' svg:height='100pt'><table:table><table:table-row><table:table-cell table:style-name='cell'><text:p>Original cell</text:p></table:table-cell></table:table-row></table:table></draw:frame>",
        &[],
    );
    let p = import(&source).unwrap();
    if let Ok(bytes) = export(&p) {
        let q = import(&bytes).unwrap();
        let ShapeKind::Table(before) = &p.slides[0].shapes[0].kind else { panic!("table") };
        let ShapeKind::Table(after) = &q.slides[0].shapes[0].kind else { panic!("table") };
        assert_eq!(after.rows[0].cells[0].vertical, before.rows[0].cells[0].vertical, "vertical cell text must not become horizontal");
        assert_eq!(after.rows[0].cells[0].diag_down, before.rows[0].cells[0].diag_down, "diagonal cell border must not disappear");
    }
}

fn shared_gradient(stops: usize, references: usize) -> Vec<u8> {
    let stops =
        (0..stops).map(|i| format!("<svg:stop svg:offset='{}' svg:stop-color='#345678'/>", i as f64 / (stops - 1) as f64)).collect::<String>();
    let resource = format!("<svg:linearGradient draw:name='g'>{stops}</svg:linearGradient>");
    package(&paint(&resource, "draw:fill='gradient' draw:fill-gradient-name='g'"), &RECT.repeat(references), &[])
}
#[test]
fn repeated_large_shared_gradient_copies_are_charged_before_model_growth() {
    let bytes = shared_gradient(4096, 700);
    assert!(bytes.len() < 1024 * 1024, "small source exercises expanded model storage");
    assert!(import(&bytes).is_err(), "700 owned 4096-stop copies must exceed cumulative fill storage budget");
}
#[test]
fn small_shared_gradient_remains_editable_within_budget() {
    let p = import(&shared_gradient(64, 3)).unwrap();
    assert_eq!(p.slides[0].shapes.len(), 3);
    assert!(p.slides[0].shapes.iter().all(|shape| matches!(shape.fill, Some(Fill::Gradient(_)))));
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"));
}
fn error_chart(properties: &str) -> Vec<u8> {
    let chart = format!(
        "<office:document-content {NS}><office:automatic-styles><style:style style:name='error' style:family='chart'><style:chart-properties {properties}/></style:style></office:automatic-styles><office:body><office:chart><chart:chart chart:class='chart:line'><chart:plot-area><chart:series chart:values-cell-range-address='local-table.A1:.A2'><chart:error-indicator chart:dimension='y' chart:style-name='error'/></chart:series></chart:plot-area><table:table table:name='local-table'><table:table-row><table:table-cell office:value='5'/></table:table-row><table:table-row><table:table-cell office:value='10'/></table:table-row></table:table></chart:chart></office:chart></office:body></office:document-content>"
    );
    package(
        "",
        "<draw:frame svg:width='200pt' svg:height='100pt'><draw:object xlink:href='Object'/></draw:frame>",
        &[("Object/content.xml", chart.as_bytes())],
    )
}
#[test]
fn explicitly_hidden_upper_and_lower_error_bars_stay_hidden() {
    let p = import(&error_chart("chart:error-category='constant' chart:error-lower-limit='2' chart:error-upper-limit='2' chart:error-lower-indicator='false' chart:error-upper-indicator='false'")).unwrap();
    let ShapeKind::Chart(chart) = &p.slides[0].shapes[0].kind else { panic!("chart") };
    assert!(chart.series[0].error_bars.is_none(), "both explicitly hidden error directions must not become visible plus bars");
}
#[test]
fn percentage_of_series_maximum_error_margin_is_reported_as_a_model_constraint() {
    assert_source_guarded(&error_chart("chart:error-category='error-margin' chart:error-margin='20'"));
}
