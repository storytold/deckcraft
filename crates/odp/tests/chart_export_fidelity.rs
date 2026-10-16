//! Contributor-original chart fixtures derived from OASIS ODF 1.3, not vendor files.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    chart::{ChartType, Series},
    *,
};
use deckcraft_odp::{export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};
fn deck(chart: Chart) -> Presentation {
    let mut p = Presentation::default();
    Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape {
        id: ShapeId(890),
        xfrm: Some(Xfrm::new(0.0, 0.0, 600.0, 400.0)),
        kind: ShapeKind::Chart(Box::new(chart)),
        ..Default::default()
    }];
    p
}
fn chart(p: &Presentation) -> &Chart {
    let ShapeKind::Chart(c) = &p.slides[0].shapes[0].kind else { panic!("editable chart") };
    c
}
fn xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name("Object1/content.xml").unwrap().read_to_string(&mut text).unwrap();
    text
}
fn numeric(kind: ChartType) -> Chart {
    Chart {
        kind,
        title: None,
        legend: None,
        series: vec![Series {
            name: "Numbers".into(),
            values: vec![Some(10.), None, Some(30.)],
            x_values: vec![Some(1.), Some(4.), Some(9.)],
            ..Default::default()
        }],
        ..Default::default()
    }
}
fn capture(name: &str, xml: &str) {
    if let Ok(dir) = std::env::var("DECKCRAFT_CHART_QA_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.xml")), xml).unwrap();
    }
}

fn imported_chart(class: &str, styles: &str, plot: &str, table: &str) -> Presentation {
    let class = if class.contains(':') { class.to_string() } else { format!("chart:{class}") };
    let doc = format!(
        "<office:document-content xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:chart='urn:oasis:names:tc:opendocument:xmlns:chart:1.0' xmlns:c='urn:oasis:names:tc:opendocument:xmlns:chart:1.0' xmlns:v='urn:contributor-original:unsupported' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' office:version='1.3'><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:chart><chart:chart chart:class='{class}'><chart:plot-area>{plot}</chart:plot-area>{table}</chart:chart></office:chart></office:body></office:document-content>"
    );
    let original = export(&deck(numeric(ChartType::Scatter))).unwrap();
    let mut src = zip::ZipArchive::new(Cursor::new(original)).unwrap();
    let mut dst = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..src.len() {
        let mut entry = src.by_index(i).unwrap();
        let name = entry.name().to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        dst.start_file(&name, zip::write::SimpleFileOptions::default()).unwrap();
        dst.write_all(if name == "Object1/content.xml" { doc.as_bytes() } else { &data }).unwrap();
    }
    import(&dst.finish().unwrap().into_inner()).unwrap()
}

#[test]
fn numeric_domains_respect_table_identity_latest_shared_ranges_and_domain_counts() {
    let table = "<table:table table:name='original data'><table:table-row><table:table-cell office:value='1'/><table:table-cell office:value='5'/><table:table-cell office:value='10'/></table:table-row><table:table-row><table:table-cell office:value='2'/><table:table-cell office:value='7'/><table:table-cell office:value='20'/></table:table-row></table:table>";
    let first = "<chart:series chart:values-cell-range-address='&apos;original data&apos;.$C$1:.$C$2'><chart:domain table:cell-range-address='&apos;original data&apos;.$A$1:.$A$2'/></chart:series>";
    let second = "<chart:series chart:values-cell-range-address='&apos;original data&apos;.$C$1:.$C$2'><chart:domain table:cell-range-address='&apos;original data&apos;.$B$1:.$B$2'/></chart:series>";
    let third = "<chart:series chart:values-cell-range-address='&apos;original data&apos;.$C$1:.$C$2'/>";
    let p = imported_chart("scatter", "", &format!("{first}{second}{third}"), table);
    assert_eq!(chart(&p).series[0].x_values, [Some(1.), Some(2.)]);
    for s in &chart(&p).series[1..] {
        assert_eq!(s.x_values, [Some(5.), Some(7.)]);
    }
    let q = import(&export(&p).unwrap()).unwrap();
    assert_eq!(chart(&q).series[2].x_values, [Some(5.), Some(7.)]);
    for (class, domains) in [("scatter", 0), ("scatter", 2), ("bubble", 1)] {
        let domain = "<chart:domain table:cell-range-address='&apos;original data&apos;.$A$1:.$A$2'/>";
        let plot =
            format!("<chart:series chart:values-cell-range-address='&apos;original data&apos;.$C$1:.$C$2'>{}</chart:series>", domain.repeat(domains));
        let p = imported_chart(class, "", &plot, table);
        assert!(p.tags.iter().any(|(k, v)| k == "odp:warning" && v.contains("domain cardinality")), "{class} domains={domains}");
        assert!(export(&p).is_err());
    }
}

#[test]
fn combo_classes_secondary_names_formats_and_bounds_survive_export() {
    let mut c = Chart::sample(ChartType::Combo);
    c.y_min = Some(-10.);
    c.y_max = Some(10.);
    c.y_major = Some(5.);
    c.y_format = Some("#,##0.00".into());
    c.y2_min = Some(0.);
    c.y2_max = Some(1.);
    c.y2_major = Some(0.25);
    c.y2_format = Some("0.0%".into());
    c.y2_hidden = true;
    c.series[2].values = vec![Some(0.2), Some(0.7), Some(0.1), Some(0.8)];
    c.series[2].secondary_axis = true;
    c.series[2].marker = Some("diamond".into());
    c.series[2].number_format = Some("0.00%".into());
    let bytes = export(&deck(c.clone())).unwrap();
    let x = xml(&bytes);
    assert!(!x.contains("chart:combo"));
    assert!(x.contains("chart:name=\"secondary-y\""));
    assert!(x.contains("chart:attached-axis=\"secondary-y\""));
    assert!(x.contains("chart:class=\"chart:line\""));
    assert!(x.contains("<number:percentage-style"));
    let q = import(&bytes).unwrap();
    let q = chart(&q);
    assert_eq!(q.kind, ChartType::Combo);
    assert_eq!(q.series_kind(&q.series[0]), ChartType::Column);
    assert_eq!(q.series_kind(&q.series[2]), ChartType::Line);
    assert!(q.on_secondary(&q.series[2]));
    assert_eq!((q.y_min, q.y_max, q.y_major), (c.y_min, c.y_max, c.y_major));
    assert_eq!((q.y2_min, q.y2_max, q.y2_major), (c.y2_min, c.y2_max, c.y2_major));
    assert_eq!(q.y_format, c.y_format);
    assert_eq!(q.y2_format, c.y2_format);
    assert_eq!(q.series[2].number_format, c.series[2].number_format);
    assert!(q.y2_hidden);
    assert_eq!(q.series[2].marker.as_deref(), Some("diamond"));
    capture("combo-secondary", &x);
}

#[test]
fn chart_text_paints_labels_markers_and_global_interpolation_remain_editable() {
    let mut c = numeric(ChartType::Line);
    c.series[0].x_values.clear();
    c.title = Some("Original chart".into());
    c.title_size = Some(23.);
    c.text_size = Some(11.);
    c.text_color = Some(ColorRef::rgb(Rgba::rgb(30, 40, 50)));
    c.chart_fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(240, 245, 250))));
    c.plot_fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(230, 235, 240))));
    c.cat_reversed = true;
    c.data_labels = true;
    c.series[0].data_labels = Some(false);
    c.series[0].marker = Some("square".into());
    c.series[0].smooth = true;
    c.series[0].line = Some(Line::solid(ColorRef::rgb(Rgba::rgb(180, 20, 60)), 2.));
    c.series[0].point_fills = vec![(2, Fill::solid(ColorRef::rgb(Rgba::rgb(20, 180, 60))))];
    let bytes = export(&deck(c.clone())).unwrap();
    let x = xml(&bytes);
    let q = import(&bytes).unwrap();
    let q = chart(&q);
    assert_eq!(q.title_size, c.title_size);
    assert_eq!(q.text_size, c.text_size);
    assert_eq!(q.text_color, c.text_color);
    assert_eq!(q.chart_fill, c.chart_fill);
    assert_eq!(q.plot_fill, c.plot_fill);
    assert!(q.cat_reversed);
    assert!(q.series[0].smooth);
    assert_eq!(q.series[0].data_labels, Some(false));
    assert_eq!(q.series[0].marker.as_deref(), Some("square"));
    assert_eq!(q.series[0].point_fills, c.series[0].point_fills);
    capture("chart-styles", &x);
}

#[test]
fn regressions_and_fixed_percentage_custom_errors_keep_exact_mappings() {
    use deckcraft_model::chart::{ErrorBars, Trendline, TrendlineType};
    for val_type in ["fixedVal", "percentage", "cust"] {
        let mut c = numeric(ChartType::Scatter);
        c.series[0].trendlines = vec![Trendline {
            kind: TrendlineType::Poly,
            order: Some(3),
            name: Some("Original fit".into()),
            intercept: Some(0.),
            disp_eq: true,
            disp_r_sqr: true,
            ..Default::default()
        }];
        c.series[0].error_bars = Some(ErrorBars {
            val_type: val_type.into(),
            value: 2.,
            bar_type: "plus".into(),
            plus: if val_type == "cust" { vec![Some(1.), None, Some(3.)] } else { vec![] },
            minus: if val_type == "cust" { vec![None, Some(2.)] } else { vec![] },
            ..Default::default()
        });
        let bytes = export(&deck(c.clone())).unwrap();
        let x = xml(&bytes);
        assert!(x.contains("chart:regression-type=\"polynomial\""));
        assert!(x.contains("chart:error-lower-indicator=\"false\" chart:error-upper-indicator=\"true\""));
        let q = import(&bytes).unwrap();
        let q = chart(&q);
        assert_eq!(q.series[0].trendlines, c.series[0].trendlines);
        let a = q.series[0].error_bars.as_ref().unwrap();
        let b = c.series[0].error_bars.as_ref().unwrap();
        assert_eq!(a.val_type, b.val_type);
        assert_eq!(a.bar_type, b.bar_type);
        assert_eq!(a.plus, b.plus);
        assert_eq!(a.minus, b.minus);
        if val_type != "cust" {
            assert_eq!(a.value, b.value);
        }
        capture(&format!("regression-error-{val_type}"), &x);
    }
}

#[test]
fn disabled_error_indicators_do_not_become_active_or_trigger_margin_conversion() {
    let styles = "<style:style style:name='off' style:family='chart'><style:chart-properties chart:error-category='error-margin' chart:error-lower-indicator='false' chart:error-upper-indicator='false' chart:error-margin='12'/></style:style>";
    let p = imported_chart(
        "line",
        styles,
        "<chart:series chart:values-cell-range-address='data.A1'><chart:error-indicator chart:dimension='y' chart:style-name='off'/></chart:series>",
        "<table:table table:name='data'><table:table-row><table:table-cell office:value='4'/></table:table-row></table:table>",
    );
    assert!(chart(&p).series[0].error_bars.is_none());
    assert!(!p.tags.iter().any(|(k, _)| k == "odp:warning"));
    assert!(export(&p).is_ok());
}

#[test]
fn unsupported_chart_options_keep_explicit_export_guards() {
    use deckcraft_model::chart::{ErrorBars, Trendline};
    let mutate: [fn(&mut Chart); 7] = [
        |c| c.raw = Some("<unclassified/>".into()),
        |c| c.legend_overlay = true,
        |c| c.y_log_base = Some(2.),
        |c| c.series[0].number_format = Some("[Red]0;0".into()),
        |c| c.series[0].trendlines.push(Trendline { forward: Some(2.), ..Default::default() }),
        |c| c.series[0].error_bars = Some(ErrorBars { end_cap: false, ..Default::default() }),
        |c| c.series[0].error_bars = Some(ErrorBars { val_type: "stdDev".into(), ..Default::default() }),
    ];
    for change in mutate {
        let mut c = numeric(ChartType::Scatter);
        change(&mut c);
        assert!(export(&deck(c)).is_err());
    }
}

#[test]
fn chart_class_tokens_resolve_actual_namespace_uris_and_aliases() {
    let table = "<table:table table:name='data'><table:table-row><table:table-cell office:value='3'/></table:table-row></table:table>";
    let plot = "<chart:series chart:values-cell-range-address='data.A1'><chart:domain table:cell-range-address='data.A1'/></chart:series>";
    let p = imported_chart("c:scatter", "", plot, table);
    assert_eq!(chart(&p).kind, ChartType::Scatter);
    for class in ["v:scatter", "missing:scatter"] {
        let p = imported_chart(class, "", plot, table);
        assert!(p.tags.iter().any(|(k, v)| k == "odp:warning" && v.contains("chart class namespace")), "{class}");
        assert!(export(&p).is_err());
    }
    let p = imported_chart("line", "", "<chart:series chart:class='v:scatter' chart:values-cell-range-address='data.A1'/>", table);
    assert!(p.tags.iter().any(|(k, v)| k == "odp:warning" && v.contains("series chart class")));
    assert!(export(&p).is_err());
}

#[test]
fn unmodeled_chart_styles_warn_instead_of_silently_losing_semantics() {
    let table = "<table:table table:name='data'><table:table-row><table:table-cell office:value='3'/></table:table-row></table:table>";
    let cases = [
        ("chart:data-label-number='percentage'", "", "custom chart labels"),
        ("chart:treat-empty-cells='use-zero'", "", "missing-value policies"),
        ("chart:visible='false'", "", "visibility"),
        ("chart:interval-minor-divisor='2'", "", "five subdivisions"),
        ("", "draw:stroke='dash'", "custom outline"),
        ("", "svg:stroke-opacity='50%'", "outline opacity"),
    ];
    for (chart_props, graphic_props, message) in cases {
        let styles = format!(
            "<style:style style:name='s' style:family='chart'><style:chart-properties {chart_props}/><style:graphic-properties {graphic_props}/></style:style>"
        );
        let p = imported_chart("line", &styles, "<chart:series chart:style-name='s' chart:values-cell-range-address='data.A1'/>", table);
        assert!(p.tags.iter().any(|(k, v)| k == "odp:warning" && v.contains(message)), "{message}");
        assert!(export(&p).is_err());
    }
}

#[test]
fn common_number_formats_and_primary_base_ten_log_are_editable() {
    for format in ["0", "000.00", "#,##0.00", "0.0%", "$0.00", "€0"] {
        let mut c = numeric(ChartType::Scatter);
        c.y_format = Some(format.into());
        c.series[0].number_format = Some(format.into());
        let q = import(&export(&deck(c)).unwrap()).unwrap();
        assert_eq!(chart(&q).y_format.as_deref(), Some(format));
        assert_eq!(chart(&q).series[0].number_format.as_deref(), Some(format));
    }
    let mut c = numeric(ChartType::Scatter);
    c.y_log_base = Some(10.);
    c.y_min = Some(1.);
    c.y_max = Some(1000.);
    c.minor_gridlines = true;
    let bytes = export(&deck(c.clone())).unwrap();
    let x = xml(&bytes);
    assert!(x.contains("chart:logarithmic=\"true\""));
    assert!(x.contains("chart:interval-minor-divisor=\"5\""));
    let q = import(&bytes).unwrap();
    assert_eq!(chart(&q).y_log_base, Some(10.));
    assert!(chart(&q).minor_gridlines);
    capture("primary-log", &x);
    c.y_major = Some(1.);
    assert!(export(&deck(c)).unwrap_err().0.contains("logarithmic major"));
}
#[test]
fn scatter_exports_independent_numeric_domain_and_positional_holes() {
    let c = numeric(ChartType::Scatter);
    let bytes = export(&deck(c.clone())).unwrap();
    let x = xml(&bytes);
    assert!(x.contains("chart:class=\"chart:scatter\""));
    assert!(x.contains("chart:values-cell-range-address=\"local-table.B2:local-table.B4\""));
    assert!(x.contains("<chart:domain table:cell-range-address=\"local-table.C2:local-table.C4\"/>"));
    assert!(x.contains("draw:stroke=\"none\""));
    assert!(x.contains("<table:table-cell/><table:table-cell/><table:table-cell office:value-type=\"float\" office:value=\"4\""));
    let q = import(&bytes).unwrap();
    assert_eq!(chart(&q).series[0].values, c.series[0].values);
    assert_eq!(chart(&q).series[0].x_values, c.series[0].x_values);
    assert!(chart(&q).categories.is_empty());
    capture("scatter", &x);
}
#[test]
fn scatter_series_domains_do_not_alias_and_ranges_preserve_lengths() {
    let mut c = numeric(ChartType::Scatter);
    c.series.push(Series { name: "Second".into(), values: vec![Some(7.)], x_values: vec![Some(20.), None], ..Default::default() });
    let bytes = export(&deck(c.clone())).unwrap();
    let x = xml(&bytes);
    assert!(x.contains("chart:values-cell-range-address=\"local-table.D2:local-table.D2\""));
    assert!(x.contains("<chart:domain table:cell-range-address=\"local-table.E2:local-table.E3\"/>"));
    let q = import(&bytes).unwrap();
    for (a, b) in c.series.iter().zip(&chart(&q).series) {
        assert_eq!(a.values, b.values);
        assert_eq!(a.x_values, b.x_values);
    }
    capture("scatter-many", &x);
}
#[test]
fn scatter_zero_one_and_empty_arrays_do_not_invent_missing_points() {
    for count in [0, 1] {
        let mut c = numeric(ChartType::Scatter);
        c.series.truncate(count);
        if let Some(s) = c.series.first_mut() {
            s.values.clear();
            s.x_values.clear();
        }
        let bytes = export(&deck(c)).unwrap();
        let x = xml(&bytes);
        assert!(!x.contains("local-table.B2:"));
        let q = import(&bytes).unwrap();
        assert_eq!(chart(&q).series.len(), count);
        if count == 1 {
            assert!(chart(&q).series[0].values.is_empty());
            assert!(chart(&q).series[0].x_values.is_empty());
            assert!(x.contains("<chart:domain table:cell-range-address=\"\"/>"));
        }
        capture(&format!("scatter-empty-{count}"), &x);
    }
}
#[test]
fn scatter_implicit_x_is_materialized_and_has_the_same_numeric_rendering() {
    for xs in [vec![], vec![None; 3]] {
        let mut c = numeric(ChartType::Scatter);
        c.series[0].x_values = xs;
        let p = deck(c);
        let before = deckcraft_render::render_slide(&p, 0, &Default::default());
        let bytes = export(&p).unwrap();
        let q = import(&bytes).unwrap();
        assert_eq!(chart(&q).series[0].x_values, [Some(1.), Some(2.), Some(3.)]);
        let after = deckcraft_render::render_slide(&q, 0, &Default::default());
        let differing = before.pixels.iter().zip(&after.pixels).filter(|(a, b)| a.abs_diff(**b) > 3).count();
        assert!(differing < 80, "numeric implicit-X rendering changed: {differing} channels");
    }
}
#[test]
fn chart_data_export_refuses_nonfinite_and_oversized_inputs() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut c = numeric(ChartType::Scatter);
        c.series[0].x_values[1] = Some(bad);
        assert!(export(&deck(c)).unwrap_err().0.contains("non-finite"));
    }
    let mut c = numeric(ChartType::Scatter);
    c.series[0].values = vec![Some(1.); 1000];
    assert!(export(&deck(c)).unwrap_err().0.contains("limit"));
}

#[test]
fn bubble_exports_y_then_x_domains_and_size_values_without_a_connecting_line() {
    let mut c = numeric(ChartType::Bubble);
    c.series[0].sizes = vec![Some(1.), None, Some(4.)];
    c.series[0].line = Some(Line::none());
    let bytes = export(&deck(c.clone())).unwrap();
    let x = xml(&bytes);
    assert!(x.contains("chart:values-cell-range-address=\"local-table.B2:local-table.B4\""));
    assert!(x.contains("<chart:domain table:cell-range-address=\"local-table.C2:local-table.C4\"/><chart:domain table:cell-range-address=\"local-table.D2:local-table.D4\"/>"));
    let q = import(&bytes).unwrap();
    assert_eq!(chart(&q).series[0].values, c.series[0].values);
    assert_eq!(chart(&q).series[0].sizes, c.series[0].sizes);
    assert_eq!(chart(&q).series[0].x_values, c.series[0].x_values);
    capture("bubble", &x);
}

#[test]
fn bubble_empty_sizes_stay_empty_and_negative_sizes_are_guarded() {
    let mut c = numeric(ChartType::Bubble);
    let bytes = export(&deck(c.clone())).unwrap();
    assert!(chart(&import(&bytes).unwrap()).series[0].sizes.is_empty());
    c.series[0].sizes = vec![Some(-2.)];
    assert!(export(&deck(c)).unwrap_err().0.contains("negative bubble"));
}
