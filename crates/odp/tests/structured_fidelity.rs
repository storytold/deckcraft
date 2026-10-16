//! Contributor-original native ODF structured-content fixtures; no source/vendor assets.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::{
    style::{Compound, Dash},
    text::{Action, Bullet},
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:chart='urn:oasis:names:tc:opendocument:xmlns:chart:1.0' xmlns:number='urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0' xmlns:xlink='http://www.w3.org/1999/xlink' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' xmlns:of='urn:oasis:names:tc:opendocument:xmlns:of:1.2'";
fn content(styles: &str, pages: &str) -> String {
    format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:presentation>{pages}</office:presentation></office:body></office:document-content>"
    )
}
fn package(doc: &str, chart: Option<&str>) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", Some(MIME)),
        ("content.xml", Some(doc)),
        ("Object/content.xml", chart),
        ("META-INF/manifest.xml", Some("<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>")),
    ] {
        if let Some(data) = data {
            z.start_file(name, SimpleFileOptions::default()).unwrap();
            z.write_all(data.as_bytes()).unwrap();
        }
    }
    z.finish().unwrap().into_inner()
}
fn chart_doc(class: &str, styles: &str, plot: &str, table: &str) -> String {
    format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:chart><chart:chart chart:class='chart:{class}'><chart:plot-area>{plot}</chart:plot-area>{table}</chart:chart></office:chart></office:body></office:document-content>"
    )
}
fn import_chart(doc: &str) -> std::result::Result<Presentation, deckcraft_odp::OdpError> {
    import(&package(
        &content(
            "",
            "<draw:page draw:name='Chart'><draw:frame svg:width='400pt' svg:height='250pt'><draw:object xlink:href='Object'/></draw:frame></draw:page>",
        ),
        Some(doc),
    ))
}
fn chart(p: &Presentation) -> &Chart {
    let ShapeKind::Chart(c) = &p.slides[0].shapes[0].kind else { panic!("expected native chart") };
    c
}
#[test]
fn notes_lists_and_table_cell_links_resolve_to_native_slide_actions() {
    let styles = "<style:style style:name='cell' style:family='table-cell'><style:text-properties fo:font-size='22pt' fo:font-weight='bold'/></style:style><text:list-style style:name='n'><text:list-level-style-number text:level='1' style:num-format='1' style:num-suffix='.'/></text:list-style>";
    let pages = "<draw:page draw:name='First'><draw:frame svg:width='200pt' svg:height='80pt'><table:table><table:table-row><table:table-cell table:style-name='cell'><text:p><text:a xlink:href='#Second'>Cell link</text:a></text:p></table:table-cell></table:table-row></table:table></draw:frame><presentation:notes><draw:frame svg:width='300pt' svg:height='100pt'><draw:text-box><text:list text:style-name='n'><text:list-item><text:p><text:a xlink:href='#Second'>Notes link</text:a></text:p></text:list-item><text:list-item><text:p>Second note</text:p></text:list-item></text:list></draw:text-box></draw:frame></presentation:notes></draw:page><draw:page draw:name='Second'/>";
    let p = import(&package(&content(styles, pages), None)).unwrap();
    for deck in [&p, &import(&export(&p).unwrap()).unwrap()] {
        let ShapeKind::Table(table) = &deck.slides[0].shapes[0].kind else { panic!("table") };
        let run = &table.rows[0].cells[0].text.paragraphs[0].runs[0];
        assert!(matches!(run.props.link.as_ref().unwrap().action,Action::Slide{slide} if slide==deck.slides[1].id));
        assert_eq!(run.props.size, Some(22.0));
        assert_eq!(run.props.bold, Some(true));
        let notes = &deck.slides[0].notes.paragraphs;
        assert_eq!(notes.len(), 2);
        assert!(matches!(notes[0].props.bullet, Some(Bullet::AutoNum { .. })));
        assert!(matches!(notes[0].runs[0].props.link.as_ref().unwrap().action,Action::Slide{slide} if slide==deck.slides[1].id));
    }
}
#[test]
fn table_keeps_relative_columns_cached_values_and_native_borders() {
    let styles = "<style:style style:name='c1' style:family='table-column'><style:table-column-properties style:rel-column-width='1*'/></style:style><style:style style:name='c3' style:family='table-column'><style:table-column-properties style:rel-column-width='3*'/></style:style><style:style style:name='cell' style:family='table-cell' style:data-style-name='n'><style:table-cell-properties fo:border-left='2pt dashed #345678' fo:border-right='3pt double #112233' style:diagonal-tl-br='1pt dotted #000000' style:writing-mode='tb-rl'/><style:text-properties fo:font-size='22pt'/></style:style>";
    let pages = "<draw:page draw:name='Table'><draw:frame svg:width='200pt' svg:height='100pt'><table:table><table:table-column table:style-name='c1'/><table:table-column table:style-name='c3'/><table:table-row><table:table-cell table:style-name='cell' office:value-type='float' office:value='2.5' table:formula='of:=1+1.5'/><table:table-cell><text:p>Outer</text:p><table:table><table:table-column table:number-columns-repeated='1000'/><table:table-row><table:table-cell><text:p>Nested source</text:p></table:table-cell></table:table-row></table:table></table:table-cell></table:table-row></table:table></draw:frame></draw:page>";
    let p = import(&package(&content(styles, pages), None)).unwrap();
    let ShapeKind::Table(t) = &p.slides[0].shapes[0].kind else { panic!("table") };
    assert_eq!(t.cols, [50.0, 150.0]);
    assert_eq!(t.rows.len(), 1);
    assert_eq!(t.rows[0].cells.len(), 2);
    let cell = &t.rows[0].cells[0];
    assert_eq!(cell.text.text(), "2.5");
    assert_eq!(cell.text.paragraphs[0].runs[0].props.size, Some(22.0));
    assert_eq!(cell.borders[0].as_ref().unwrap().dash, Some(Dash::Preset { name: "dash".into() }));
    assert_eq!(cell.borders[1].as_ref().unwrap().compound, Some(Compound::Double));
    assert!(cell.diag_down.is_some());
    assert_eq!(cell.vertical, Some(deckcraft_model::text::TextDir::EaVertical));
    let messages = p.tags.iter().filter(|(k, _)| k == "odp:warning").map(|(_, v)| v.as_str()).collect::<Vec<_>>();
    assert!(messages.iter().any(|v| v.contains("formulas")));
    assert!(!messages.iter().any(|v| v.contains("Nested")));
    let outer = &t.rows[0].cells[1];
    assert_eq!(outer.text.text(), "Outer");
    assert_eq!(outer.blocks.len(), 1);
    let deckcraft_model::table::CellBlock::Table(child) = &outer.blocks[0] else { panic!("nested table") };
    assert_eq!((child.table.n_rows(), child.table.n_cols()), (1, 1000));
    assert_eq!(child.table.cell(0, 0).unwrap().text.text(), "Nested source");
    // Unsupported formula cache/number-style semantics still prevent export.
    assert!(export(&p).is_err());
    assert!(messages.iter().any(|v| v.contains("number formats")));
}
#[test]
fn bubble_domains_use_y_then_x_and_shared_domains_keep_sparse_sizes() {
    let data = "<table:table table:name='data table'><table:table-row><table:table-cell/><table:table-cell/><table:table-cell/><table:table-cell/></table:table-row><table:table-row><table:table-cell office:value='1'/><table:table-cell office:value='4'/><table:table-cell office:value='8'/></table:table-row><table:table-row><table:table-cell office:value='3'/><table:table-cell office:value='5'/><table:table-cell office:value='9'/><table:table-cell office:value='10'/></table:table-row></table:table>";
    let plot = "<chart:series chart:values-cell-range-address='&apos;data table&apos;.$C$2:.$C$3'><chart:domain table:cell-range-address='&apos;data table&apos;.$B$2:.$B$3'/><chart:domain table:cell-range-address='&apos;data table&apos;.$A$2:.$A$3'/></chart:series><chart:series chart:values-cell-range-address='&apos;data table&apos;.$D$2:.$D$3'/>";
    let p = import_chart(&chart_doc("bubble", "", plot, data)).unwrap();
    let c = chart(&p);
    assert_eq!(c.series.len(), 2);
    for s in &c.series {
        assert_eq!(s.x_values, [Some(1.0), Some(3.0)]);
        assert_eq!(s.values, [Some(4.0), Some(5.0)]);
    }
    assert_eq!(c.series[0].sizes, [Some(8.0), Some(9.0)]);
    assert_eq!(c.series[1].sizes, [None, Some(10.0)]);
}
#[test]
fn chart_axes_styles_regression_and_error_bars_map_to_existing_model() {
    let styles = "<number:percentage-style style:name='pct'><number:number number:decimal-places='2' number:min-integer-digits='1'/><number:text>%</number:text></number:percentage-style><style:style style:name='y' style:family='chart' style:data-style-name='pct'><style:chart-properties chart:minimum='1' chart:maximum='1000' chart:interval-major='10' chart:logarithmic='true'/></style:style><style:style style:name='y2' style:family='chart'><style:chart-properties chart:minimum='0' chart:maximum='1' chart:interval-major='0.25' chart:display-label='false'/></style:style><style:style style:name='s' style:family='chart' style:data-style-name='pct'><style:chart-properties chart:symbol-type='named-symbol' chart:symbol-name='diamond' chart:data-label-number='value'/><style:graphic-properties draw:stroke='solid' svg:stroke-color='#345678' svg:stroke-width='2pt'/></style:style><style:style style:name='trend' style:family='chart'><style:chart-properties chart:regression-type='polynomial' chart:regression-max-degree='3'/></style:style><style:style style:name='error' style:family='chart'><style:chart-properties chart:error-category='constant' chart:error-lower-limit='2' chart:error-upper-limit='2'/></style:style>";
    let plot = "<chart:axis chart:dimension='x' chart:name='x'><chart:title><text:p>Category</text:p></chart:title></chart:axis><chart:axis chart:dimension='y' chart:name='primary' chart:style-name='y'><chart:title><text:p>Amount</text:p></chart:title><chart:grid chart:class='major'/><chart:grid chart:class='minor'/></chart:axis><chart:axis chart:dimension='y' chart:name='secondary' chart:style-name='y2'/><chart:series chart:attached-axis='secondary' chart:style-name='s' chart:values-cell-range-address='local-table.A1:.A2'><chart:regression-curve chart:style-name='trend'><chart:equation chart:display-equation='true'/></chart:regression-curve><chart:error-indicator chart:dimension='y' chart:style-name='error'/></chart:series>";
    let data = "<table:table table:name='local-table'><table:table-row><table:table-cell office:value='3'/></table:table-row><table:table-row><table:table-cell office:value='6'/></table:table-row></table:table>";
    let p = import_chart(&chart_doc("line", styles, plot, data)).unwrap();
    let c = chart(&p);
    assert_eq!(c.y_min, Some(1.0));
    assert_eq!(c.y_max, Some(1000.0));
    assert_eq!(c.y_major, Some(10.0));
    assert_eq!(c.y_log_base, Some(10.0));
    assert_eq!(c.y_format.as_deref(), Some("0.00%"));
    assert_eq!(c.y2_major, Some(0.25));
    assert!(c.y2_hidden && c.gridlines && c.minor_gridlines);
    assert_eq!(c.axis_titles, (Some("Category".into()), Some("Amount".into())));
    let s = &c.series[0];
    assert!(s.secondary_axis);
    assert!(c.on_secondary(s));
    assert_eq!(c.series_kind(s), deckcraft_model::chart::ChartType::Line);
    assert_eq!(s.marker.as_deref(), Some("diamond"));
    assert_eq!(s.number_format.as_deref(), Some("0.00%"));
    assert_eq!(s.trendlines[0].order, Some(3));
    assert!(s.trendlines[0].disp_eq);
    assert_eq!(s.error_bars.as_ref().unwrap().value, 2.0);
    assert_eq!(s.line.as_ref().unwrap().width, Some(2.0));
}
#[test]
fn mixed_chart_classes_use_renderer_combo_dispatch() {
    use deckcraft_model::chart::ChartType;
    let data = "<table:table table:name='local-table'><table:table-row><table:table-cell office:value='3'/></table:table-row></table:table>";
    let plot = "<chart:series chart:values-cell-range-address='local-table.A1'/><chart:series chart:class='chart:line' chart:values-cell-range-address='local-table.A1'/>";
    let p = import_chart(&chart_doc("bar", "", plot, data)).unwrap();
    let c = chart(&p);
    assert_eq!(c.kind, ChartType::Combo);
    assert_eq!(c.series_kind(&c.series[0]), ChartType::Column);
    assert_eq!(c.series_kind(&c.series[1]), ChartType::Line);
}
#[test]
fn chart_references_cannot_silently_bind_to_a_different_local_table() {
    let data = "<table:table table:name='local-table'><table:table-row><table:table-cell office:value='3'/></table:table-row></table:table>";
    let plot = "<chart:series chart:values-cell-range-address='other.A1'/>";
    assert!(import_chart(&chart_doc("line", "", plot, data)).unwrap_err().0.contains("non-local data table"));
}

#[test]
fn repeated_chart_point_fill_styles_and_inherited_domains_share_budgets() {
    let stops = (0..256).map(|i| format!("<svg:stop svg:offset='{}' svg:stop-color='#345678'/>", i as f64 / 255.0)).collect::<String>();
    let styles = format!(
        "<svg:linearGradient draw:name='g'>{stops}</svg:linearGradient><style:style style:name='point' style:family='chart'><style:graphic-properties draw:fill='gradient' draw:fill-gradient-name='g'/></style:style>"
    );
    let data = "<table:table table:name='local-table'><table:table-row><table:table-cell office:value='1'/></table:table-row></table:table>";
    let plot = "<chart:series chart:values-cell-range-address='local-table.A1'><chart:data-point chart:repeated='20000' chart:style-name='point'/></chart:series>";
    assert!(import_chart(&chart_doc("line", &styles, plot, data)).unwrap_err().0.contains("point styles"));
    let rows = (0..1000).map(|i| format!("<table:table-row><table:table-cell office:value='{i}'/></table:table-row>")).collect::<String>();
    let data = format!("<table:table table:name='local-table'>{rows}</table:table>");
    let mut plot="<chart:series chart:values-cell-range-address='local-table.A1:.A1000'><chart:domain table:cell-range-address='local-table.A1:.A1000'/></chart:series>".to_string();
    plot.push_str(&"<chart:series chart:values-cell-range-address='local-table.A1:.A1000'/>".repeat(60));
    assert!(import_chart(&chart_doc("scatter", "", &plot, &data)).unwrap_err().0.contains("sample"));
}

#[test]
fn table_cell_used_style_constraints_warn_and_guard_source() {
    let styles = "<style:style style:name='base' style:family='table-cell'><style:text-properties fo:hyphenate='true'/></style:style><style:style style:name='cell' style:family='table-cell' style:parent-style-name='base'/>";
    let page = "<draw:page><draw:frame svg:width='200pt' svg:height='80pt'><table:table><table:table-row><table:table-cell table:style-name='cell'><text:p>Cell text</text:p></table:table-cell></table:table-row></table:table></draw:frame></draw:page>";
    let source = package(&content(styles, page), None);
    let p = import(&source).unwrap();
    assert!(p.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("hyphenation")));
    assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), source);
    assert!(export(&p).is_err());
}

#[test]
fn semantic_audit_guards_unrepresented_style_definitions_even_when_currently_unused() {
    let styles = "<style:style style:name='unused' style:family='table-cell'><style:text-properties fo:hyphenate='true'/></style:style>";
    let page = "<draw:page><draw:frame svg:width='200pt' svg:height='80pt'><table:table><table:table-row><table:table-cell><text:p>Cell text</text:p></table:table-cell></table:table-row></table:table></draw:frame></draw:page>";
    let source = package(&content(styles, page), None);
    let p = import(&source).unwrap();
    assert!(p.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("semantic audit v1")));
    assert_eq!(p.media.iter().find(|media| media.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), source);
    let ShapeKind::Table(table) = &p.slides[0].shapes[0].kind else { panic!("table remains editable") };
    assert_eq!(table.rows[0].cells[0].text.text(), "Cell text");
    assert!(export(&p).is_err(), "unused definitions are recovery-only until preservation/reachability is established");
}
