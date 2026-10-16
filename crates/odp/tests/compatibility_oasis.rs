//! Independently authored ODF 1.3 fixtures. Contributor-original, MIT OR Apache-2.0.
//! No vendor documents or private presentation content are used here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_geom::KurboShape;
use deckcraft_model::{Fill, Geom, ShapeKind, text::Bullet};
use deckcraft_odp::{MIME, export, import};
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:chart='urn:oasis:names:tc:opendocument:xmlns:chart:1.0' xmlns:xlink='http://www.w3.org/1999/xlink'";

fn content(shapes: &str, automatic: &str) -> String {
    format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles>{automatic}</office:automatic-styles><office:body><office:presentation><draw:page draw:name='Synthetic' draw:master-page-name='Master'>{shapes}</draw:page></office:presentation></office:body></office:document-content>"
    )
}

fn styles(master_shapes: &str, named: &str) -> String {
    format!(
        "<office:document-styles {NS} office:version='1.3'><office:styles>{named}</office:styles><office:automatic-styles><style:page-layout style:name='Page'><style:page-layout-properties fo:page-width='10in' fo:page-height='5.625in'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='Master' style:page-layout-name='Page'>{master_shapes}</style:master-page></office:master-styles></office:document-styles>"
    )
}

fn package(label: &str, content: &str, styles: &str, media: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='1.3'/><manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/><manifest:file-entry manifest:full-path='styles.xml' manifest:media-type='text/xml'/>"
    );
    for (path, data) in media {
        if let Some(directory) = path.strip_suffix("content.xml") {
            let kind = if std::str::from_utf8(data).unwrap().contains("<office:chart>") { "chart" } else { "spreadsheet" };
            manifest.push_str(&format!(
                "<manifest:file-entry manifest:full-path='{directory}' manifest:media-type='application/vnd.oasis.opendocument.{kind}'/>"
            ));
        }
        let mime = if path.ends_with(".svg") {
            "image/svg+xml"
        } else if path.ends_with(".wav") {
            "audio/wav"
        } else {
            "text/xml"
        };
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path='{path}' manifest:media-type='{mime}'/>"));
    }
    manifest.push_str("</manifest:manifest>");
    for (path, data) in [
        ("mimetype", MIME.as_bytes()),
        ("content.xml", content.as_bytes()),
        ("styles.xml", styles.as_bytes()),
        ("META-INF/manifest.xml", manifest.as_bytes()),
    ]
    .into_iter()
    .chain(media.iter().copied())
    {
        zip.start_file(path, SimpleFileOptions::default()).unwrap();
        zip.write_all(data).unwrap();
    }
    let result = zip.finish().unwrap().into_inner();
    // Optional private standards-validation output; no generated assets enter the repository.
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_COMPAT_OUTPUT") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/{label}-content.xml"), content).unwrap();
        std::fs::write(format!("{dir}/{label}-styles.xml"), styles).unwrap();
        for (path, bytes) in media.iter().filter(|(p, _)| p.ends_with(".xml")) {
            std::fs::write(format!("{dir}/{label}-{}", path.replace('/', "-")), bytes).unwrap();
        }
    }
    result
}

#[test]
fn established_odf_rotation_uses_radians_and_preserves_group_children() {
    // ODF ecosystem compatibility: producers encode radians here. The OASIS 1.3
    // prose says degrees, so this deliberately tests established file behaviour.
    let xml = content(
        "<draw:g draw:name='Group'><draw:rect draw:name='Child' draw:transform='rotate(1.5707963267948966) translate(2in 1in)' svg:x='0pt' svg:y='0pt' svg:width='72pt' svg:height='36pt'/></draw:g>",
        "",
    );
    let p = import(&package("group-radians", &xml, &styles("", ""), &[])).unwrap();
    assert_eq!(p.slides.len(), 1);
    assert_eq!(p.slide_size.width, 720.0);
    let group = &p.slides[0].shapes[0];
    let ShapeKind::Group { children, .. } = &group.kind else { panic!("group must remain editable") };
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].name, "Child");
    let x = children[0].xfrm.unwrap();
    assert!((x.rot - 90.0).abs() < 1e-8, "producer-compatible ODF rotation is radians: {x:?}");
    assert_eq!(children[0].xfrm.unwrap().w, 72.0);
}

#[test]
fn transformed_model_groups_export_valid_children_with_the_same_world_transform() {
    let xml =
        content("<draw:g draw:name='Group'><draw:rect draw:name='Child' svg:x='12pt' svg:y='24pt' svg:width='72pt' svg:height='36pt'/></draw:g>", "");
    let mut p = import(&package("export-group", &xml, &styles("", ""), &[])).unwrap();
    let group = &mut std::sync::Arc::make_mut(&mut p.slides[0]).shapes[0];
    let x = group.xfrm.as_mut().unwrap();
    x.rot = 33.0;
    x.w *= 1.5;
    x.h *= 1.5;
    let world = |group: &deckcraft_model::Shape| {
        let ShapeKind::Group { children, child } = &group.kind else { panic!("group must stay editable") };
        let child_shape = children[0].xfrm.unwrap();
        deckcraft_geom::group_child_affine(&group.xfrm.unwrap(), child.rect())
            * child_shape.affine()
            * deckcraft_geom::Affine::scale_non_uniform(child_shape.w, child_shape.h)
    };
    let before = world(group);
    let bytes = export(&p).unwrap();
    let q = import(&bytes).unwrap();
    assert_eq!(q.slides[0].shapes.len(), 1, "structural layer definitions must not become drawing objects");
    let after = world(&q.slides[0].shapes[0]);
    for (a, b) in before.as_coeffs().iter().zip(after.as_coeffs()) {
        assert!((a - b).abs() < 1e-7, "world transform changed: {before:?} -> {after:?}");
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut content = String::new();
    zip.by_name("content.xml").unwrap().read_to_string(&mut content).unwrap();
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_COMPAT_OUTPUT") {
        std::fs::write(format!("{dir}/export-group-produced-content.xml"), &content).unwrap();
        let mut styles = String::new();
        zip.by_name("styles.xml").unwrap().read_to_string(&mut styles).unwrap();
        std::fs::write(format!("{dir}/export-group-produced-styles.xml"), styles).unwrap();
    }
    let group = content.split("<draw:g ").nth(1).expect("editable ODF group").split('>').next().unwrap();
    assert!(!group.contains("draw:transform"), "ODF 1.3 forbids draw:transform on draw:g");
}

#[test]
fn layer_definitions_control_visibility_without_creating_opaque_objects() {
    let xml = content(
        "<draw:layer-set><draw:layer draw:name='ReviewHidden' draw:display='none'/></draw:layer-set><draw:rect draw:name='Concealed' draw:layer='ReviewHidden' svg:x='12pt' svg:y='24pt' svg:width='72pt' svg:height='36pt'/>",
        "",
    );
    let p = import(&package("hidden-layer", &xml, &styles("", ""), &[])).unwrap();
    assert_eq!(p.slides[0].shapes.len(), 1, "layer-set is structural XML");
    assert!(p.slides[0].shapes[0].hidden, "draw:display=none hides objects on that layer");
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "native layers require no recovery warning");
}

#[test]
fn enhanced_path_controls_geometry_even_with_a_recognized_type_hint() {
    // OASIS 1.3 §19.229.3: draw:type is an editing hint and must not change
    // geometry. This shape is triangular despite its rectangle type hint.
    let xml = content(
        "<draw:custom-shape svg:x='1in' svg:y='1in' svg:width='1in' svg:height='1in'><draw:enhanced-geometry draw:type='rectangle' svg:viewBox='0 0 100 100' draw:enhanced-path='M 0 0 L 100 0 0 100 Z N'/></draw:custom-shape>",
        "",
    );
    let p = import(&package("enhanced-path", &xml, &styles("", ""), &[])).unwrap();
    let Geom::Custom { paths } = &p.slides[0].shapes[0].geom else { panic!("explicit enhanced-path must override draw:type editing hint") };
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].w, 100.0);
    assert_eq!(paths[0].h, 100.0);
    assert!(paths[0].d.contains("100,0") || paths[0].d.contains("100 0"));
}

#[test]
fn custom_rectangle_paths_reach_the_renderer_and_keep_filled_bounds_after_export() {
    // Exercise the model path parser used by actual rendering, rather than
    // accepting SVG interchange syntax that the model cannot consume.
    let automatic = "<style:style style:name='Paint' style:family='graphic'><style:graphic-properties draw:fill='solid' draw:fill-color='#1d5777' draw:stroke='none'/></style:style>";
    let xml = content(
        "<draw:custom-shape draw:name='Formula rectangle' draw:style-name='Paint' svg:x='12pt' svg:y='24pt' svg:width='200pt' svg:height='100pt'><draw:enhanced-geometry draw:type='ooxml-rect' svg:viewBox='0 0 0 0' draw:enhanced-path='M 0 0 L ?width 0 ?width ?height 0 ?height Z N'><draw:equation draw:name='width' draw:formula='logwidth'/><draw:equation draw:name='height' draw:formula='logheight'/></draw:enhanced-geometry></draw:custom-shape><draw:path draw:name='Compact SVG rectangle' draw:style-name='Paint' svg:x='12pt' svg:y='24pt' svg:width='200pt' svg:height='100pt' svg:viewBox='10 20 100 50' svg:d='M10,20H110V70H10Z'/><draw:polygon draw:name='Polygon rectangle' draw:style-name='Paint' svg:x='12pt' svg:y='24pt' svg:width='200pt' svg:height='100pt' svg:viewBox='0 0 100 50' draw:points='0,0 100,0 100,50 0,50'/><draw:polyline draw:name='Polyline rectangle' draw:style-name='Paint' svg:x='12pt' svg:y='24pt' svg:width='200pt' svg:height='100pt' svg:viewBox='0 0 100 50' draw:points='0,0 100,0 100,50 0,50 0,0'/>",
        automatic,
    );
    let original = import(&package("render-custom-rectangles", &xml, &styles("", ""), &[])).unwrap();
    let reopened = import(&export(&original).unwrap()).unwrap();
    for p in [&original, &reopened] {
        assert_eq!(p.slides[0].shapes.len(), 4);
        for shape in &p.slides[0].shapes {
            assert!(matches!(shape.geom, Geom::Custom { .. }), "{} must exercise custom geometry", shape.name);
            assert!(matches!(shape.fill, Some(Fill::Solid { .. })), "{} lost its fill", shape.name);
            let x = shape.xfrm.unwrap();
            let geometry = deckcraft_render::shape_geometry(shape, x.w, x.h);
            assert_eq!(geometry.paths.len(), 1, "{} lost its path", shape.name);
            let path = &geometry.paths[0];
            assert_eq!(path.fill, deckcraft_geom::preset::FillMode::Norm);
            assert!(path.path.elements().len() >= 5, "{} is empty in the renderer", shape.name);
            let bounds = path.path.bounding_box();
            for (actual, expected) in [(bounds.x0, 0.0), (bounds.y0, 0.0), (bounds.width(), 200.0), (bounds.height(), 100.0)] {
                assert!((actual - expected).abs() < 1e-7, "{} renderer bounds: {bounds:?}", shape.name);
            }
            assert!((path.path.area().abs() - 20_000.0).abs() < 1e-6, "{} has no filled rectangle area", shape.name);
        }
    }
}

#[test]
fn named_numbered_lists_keep_headers_restart_and_nested_levels() {
    let list = "<text:list-style style:name='Steps'><text:list-level-style-number text:level='1' style:num-format='1' style:num-suffix='.' text:start-value='3'/><text:list-level-style-bullet text:level='2' text:bullet-char='◦'/></text:list-style>";
    let xml = content(
        "<draw:frame svg:x='1in' svg:y='1in' svg:width='5in' svg:height='3in'><draw:text-box><text:list text:style-name='Steps'><text:list-header><text:p>Instructions</text:p></text:list-header><text:list-item text:start-value='7'><text:p>First action</text:p><text:list><text:list-item><text:p>Detail</text:p></text:list-item></text:list></text:list-item><text:list-item><text:p>Next action</text:p></text:list-item></text:list></draw:text-box></draw:frame>",
        "",
    );
    let p = import(&package("lists", &xml, &styles("", list), &[])).unwrap();
    let body = p.slides[0].shapes[0].text.as_ref().unwrap();
    assert_eq!(body.paragraphs.len(), 4);
    assert_eq!(body.paragraphs[0].props.bullet, None);
    assert!(matches!(body.paragraphs[1].props.bullet, Some(Bullet::AutoNum { start_at: 1, .. })));
    assert_eq!(body.paragraphs[1].props.number_restart, Some(7));
    assert_eq!(body.paragraphs[2].level, 1);
    assert!(matches!(&body.paragraphs[2].props.bullet, Some(Bullet::Char { char }) if char == "◦"));
    assert_eq!(body.paragraphs[3].text(), "Next action");
}

#[test]
fn native_table_keeps_merged_cells_column_widths_and_repeated_rows() {
    let automatic = "<style:style style:name='Col' style:family='table-column'><style:table-column-properties style:column-width='1in'/></style:style><style:style style:name='Row' style:family='table-row'><style:table-row-properties style:row-height='0.5in'/></style:style>";
    let xml = content(
        "<draw:frame svg:x='1in' svg:y='1in' svg:width='2in' svg:height='1.5in'><table:table table:name='Measurements'><table:table-column table:style-name='Col' table:number-columns-repeated='2'/><table:table-row table:style-name='Row'><table:table-cell table:number-columns-spanned='2'><text:p>Combined</text:p></table:table-cell><table:covered-table-cell/></table:table-row><table:table-row table:style-name='Row' table:number-rows-repeated='2'><table:table-cell><text:p>West</text:p></table:table-cell><table:table-cell><text:p>East</text:p></table:table-cell></table:table-row></table:table></draw:frame>",
        automatic,
    );
    let p = import(&package("table", &xml, &styles("", ""), &[])).unwrap();
    let ShapeKind::Table(t) = &p.slides[0].shapes[0].kind else { panic!("table must remain editable") };
    assert_eq!(t.cols, vec![72.0, 72.0]);
    assert_eq!(t.rows.len(), 3);
    assert_eq!(t.rows[0].height, 36.0);
    assert_eq!(t.cell(0, 0).unwrap().grid_span, 2);
    assert!(t.cell(0, 1).unwrap().h_merge);
    assert_eq!(t.cell(2, 1).unwrap().text.text(), "East");
}

#[test]
fn embedded_chart_keeps_explicit_categories_series_and_numeric_values() {
    let object = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:chart><chart:chart chart:class='chart:bar' svg:width='3in' svg:height='2in'><chart:plot-area><chart:axis chart:dimension='x' chart:name='x'><chart:categories table:cell-range-address='local.$A$2:.$A$3'/></chart:axis><chart:axis chart:dimension='y' chart:name='y'/><chart:series chart:class='chart:bar' chart:values-cell-range-address='local.$B$2:.$B$3' chart:label-cell-address='local.$B$1'/></chart:plot-area><table:table table:name='local'><table:table-column table:number-columns-repeated='2'/><table:table-row><table:table-cell><text:p>Region</text:p></table:table-cell><table:table-cell><text:p>Volume</text:p></table:table-cell></table:table-row><table:table-row><table:table-cell><text:p>North</text:p></table:table-cell><table:table-cell office:value-type='float' office:value='12'><text:p>12</text:p></table:table-cell></table:table-row><table:table-row><table:table-cell><text:p>South</text:p></table:table-cell><table:table-cell office:value-type='float' office:value='18.5'><text:p>18.5</text:p></table:table-cell></table:table-row></table:table></chart:chart></office:chart></office:body></office:document-content>"
    );
    let xml = content(
        "<draw:frame svg:x='1in' svg:y='1in' svg:width='3in' svg:height='2in'><draw:object xlink:href='./Object1' xlink:type='simple' xlink:show='embed' xlink:actuate='onLoad'/></draw:frame>",
        "",
    );
    let p = import(&package("chart", &xml, &styles("", ""), &[("Object1/content.xml", object.as_bytes())])).unwrap();
    let ShapeKind::Chart(chart) = &p.slides[0].shapes[0].kind else { panic!("local chart data must remain editable") };
    assert_eq!(chart.categories, ["North", "South"]);
    assert_eq!(chart.series.len(), 1);
    assert_eq!(chart.series[0].name, "Volume");
    assert_eq!(chart.series[0].values, [Some(12.0), Some(18.5)]);
}

#[test]
fn master_objects_notes_and_shared_embedded_media_survive_import() {
    let master = "<draw:rect draw:name='Master decoration' svg:x='0pt' svg:y='0pt' svg:width='720pt' svg:height='12pt'/>";
    let image = "<draw:frame svg:x='1in' svg:y='1in' svg:width='1in' svg:height='1in'><draw:image xlink:href='Pictures/mark.svg' xlink:type='simple' xlink:show='embed' xlink:actuate='onLoad'/></draw:frame>";
    let notes = "<presentation:notes><draw:frame presentation:class='notes' svg:x='0pt' svg:y='0pt' svg:width='5in' svg:height='2in'><draw:text-box><text:p>Speaker cue</text:p></draw:text-box></draw:frame></presentation:notes>";
    let xml = content(&format!("{image}{image}{notes}"), "");
    let svg = b"<svg xmlns='http://www.w3.org/2000/svg' width='1' height='1'><rect width='1' height='1' fill='#2468ac'/></svg>";
    let p = import(&package("master-notes-media", &xml, &styles(master, ""), &[("Pictures/mark.svg", svg)])).unwrap();
    assert_eq!(p.slides[0].shapes.len(), 2, "only the two local pictures belong to the slide");
    let (master, _) = p.layout(p.slides[0].layout).unwrap();
    assert_eq!(master.shapes.len(), 1, "shared artwork is stored once on its master");
    assert_eq!(master.shapes[0].name, "Master decoration");
    assert_eq!(p.slides[0].notes.text(), "Speaker cue");
    assert_eq!(p.media.iter().filter(|m| m.content_type != "application/vnd.deckcraft.odp-source").count(), 1);
    assert_eq!(p.media[0].data.as_slice(), svg);
}

#[test]
fn audio_plugin_keeps_embedded_bytes_and_playback_settings() {
    let xml = content(
        "<draw:frame svg:x='1in' svg:y='1in' svg:width='1in' svg:height='1in'><draw:plugin draw:mime-type='audio/wav' xlink:href='Media/silence.wav' xlink:type='simple'><draw:param draw:name='autoplay' draw:value='true'/><draw:param draw:name='loop' draw:value='true'/></draw:plugin></draw:frame>",
        "",
    );
    // Contributor-original zero-frame PCM WAV, constructed from the public RIFF format.
    let wav = b"RIFF\x24\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00\x40\x1f\x00\x00\x40\x1f\x00\x00\x01\x00\x08\x00data\x00\x00\x00\x00";
    let p = import(&package("audio", &xml, &styles("", ""), &[("Media/silence.wav", wav)])).unwrap();
    let ShapeKind::Media(clip) = &p.slides[0].shapes[0].kind else { panic!("audio must remain a native media object") };
    assert!(!clip.video);
    assert!(clip.autoplay && clip.loop_play);
    let media = p.media.iter().find(|m| m.id == clip.media).unwrap();
    assert_eq!(media.content_type, "audio/wav");
    assert_eq!(media.data.as_slice(), wav);
}

#[test]
fn gradients_are_editable_and_unsupported_embedded_objects_keep_recovery_source() {
    let gradient = "<draw:gradient draw:name='Sunrise' draw:style='linear' draw:start-color='#204060' draw:end-color='#e0a060' draw:start-intensity='100%' draw:end-intensity='100%' draw:angle='0' draw:border='0%'/>";
    let auto = "<style:style style:name='Gradient' style:family='graphic'><style:graphic-properties draw:fill='gradient' draw:fill-gradient-name='Sunrise'/></style:style>";
    let xml = content("<draw:rect draw:style-name='Gradient' svg:x='1in' svg:y='1in' svg:width='2in' svg:height='1in'/>", auto);
    let p = import(&package("gradient", &xml, &styles("", gradient), &[])).unwrap();
    assert!(matches!(p.slides[0].shapes[0].fill, Some(Fill::Gradient(_))));
    let xml = content(
        "<draw:frame svg:x='1in' svg:y='1in' svg:width='2in' svg:height='1in'><draw:object xlink:href='./Object1' xlink:type='simple' xlink:show='embed' xlink:actuate='onLoad'/></draw:frame>",
        "",
    );
    let object = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:spreadsheet><table:table table:name='Sheet'><table:table-column/><table:table-row><table:table-cell><text:p>Embedded data</text:p></table:table-cell></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"
    );
    let bytes = package("embedded-object", &xml, &styles("", ""), &[("Object1/content.xml", object.as_bytes())]);
    let p = import(&bytes).unwrap();
    assert!(matches!(p.slides[0].shapes[0].kind, ShapeKind::Opaque { .. }));
    assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"));
    let source = p
        .media
        .iter()
        .find(|m| m.content_type == "application/vnd.deckcraft.odp-source")
        .expect("unmodeled content must retain the original archive");
    assert_eq!(source.data.as_slice(), bytes);
}

#[test]
fn repeated_master_tables_share_one_expansion_budget() {
    // This tiny package represents 150 MB of text. Each individual table is
    // below its local cap; duplicated master content must share the global cap.
    let table = "<draw:frame svg:x='0pt' svg:y='0pt' svg:width='72pt' svg:height='72pt'><table:table table:name='Repeated'><table:table-column/><table:table-row table:number-rows-repeated='300'><table:table-cell><text:p><text:s text:c='100000'/></text:p></table:table-cell></table:table-row></table:table></draw:frame>";
    let pages = (0..5).map(|i| format!("<draw:page draw:name='Synthetic{i}' draw:master-page-name='Master'/>")).collect::<String>();
    let xml = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:presentation>{pages}</office:presentation></office:body></office:document-content>"
    );
    let error = import(&package("master-table-budget", &xml, &styles(table, ""), &[])).unwrap_err().to_string();
    assert!(error.contains("limit"), "{error}");
}

#[test]
fn chart_series_reusing_one_range_share_a_sample_budget() {
    // Grid-size limits alone do not bound charts: many series can select the
    // same cells. This fixture needs only a small grid and reaches the cap at
    // about 100,000 samples, before a large allocation can accumulate.
    let series = "<chart:series chart:class='chart:bar' chart:values-cell-range-address='local.$B$2:.$B$1000'/>".repeat(101);
    let object = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:chart><chart:chart chart:class='chart:bar' svg:width='3in' svg:height='2in'><chart:plot-area>{series}</chart:plot-area><table:table table:name='local'><table:table-column table:number-columns-repeated='2'/><table:table-row><table:table-cell><text:p>Category</text:p></table:table-cell><table:table-cell><text:p>Value</text:p></table:table-cell></table:table-row><table:table-row table:number-rows-repeated='999'><table:table-cell><text:p>A</text:p></table:table-cell><table:table-cell office:value-type='float' office:value='1'><text:p>1</text:p></table:table-cell></table:table-row></table:table></chart:chart></office:chart></office:body></office:document-content>"
    );
    let xml = content(
        "<draw:frame svg:x='1in' svg:y='1in' svg:width='3in' svg:height='2in'><draw:object xlink:href='./Object1' xlink:type='simple' xlink:show='embed' xlink:actuate='onLoad'/></draw:frame>",
        "",
    );
    let error =
        import(&package("chart-sample-budget", &xml, &styles("", ""), &[("Object1/content.xml", object.as_bytes())])).unwrap_err().to_string();
    assert!(error.contains("chart") && error.contains("limit"), "{error}");
}
