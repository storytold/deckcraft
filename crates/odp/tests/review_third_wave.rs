//! Independent adversarial fixtures authored for the R02/R03 review.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    chart::{ChartType, Series},
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:chart='urn:oasis:names:tc:opendocument:xmlns:chart:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0'";

fn package(styles: &str, body: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS}><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:presentation><draw:page>{body}</draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, content) in [
        ("mimetype", MIME),
        ("content.xml", content.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        zip.start_file(path, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn deck(c: Chart) -> Presentation {
    let mut p = Presentation::default();
    Arc::make_mut(&mut p.slides[0]).shapes =
        vec![Shape { id: ShapeId(772), xfrm: Some(Xfrm::new(0., 0., 500., 320.)), kind: ShapeKind::Chart(Box::new(c)), ..Default::default() }];
    p
}
fn native_chart(p: &Presentation) -> &Chart {
    let ShapeKind::Chart(c) = &p.slides[0].shapes[0].kind else { panic!("expected editable chart") };
    c
}
fn chart_doc(class: &str, root_attrs: &str, styles: &str, plot: &str) -> String {
    format!(
        "<office:document-content {NS} xmlns:c='urn:oasis:names:tc:opendocument:xmlns:chart:1.0'><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:chart><chart:chart chart:class='{class}' {root_attrs}><chart:plot-area>{plot}</chart:plot-area><table:table table:name='data'><table:table-row><table:table-cell office:value-type='float' office:value='1'/><table:table-cell office:value-type='float' office:value='4'/></table:table-row></table:table></chart:chart></office:chart></office:body></office:document-content>"
    )
}
fn with_chart(doc: &str) -> Presentation {
    let base = export(&deck(Chart::sample(ChartType::Column))).unwrap();
    let mut src = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    let mut dst = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..src.len() {
        let mut entry = src.by_index(i).unwrap();
        let name = entry.name().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        dst.start_file(&name, zip::write::SimpleFileOptions::default()).unwrap();
        dst.write_all(if name == "Object1/content.xml" { doc.as_bytes() } else { &bytes }).unwrap();
    }
    import(&dst.finish().unwrap().into_inner()).unwrap()
}
fn guarded(p: &Presentation) {
    assert!(p.tags.iter().any(|(k, _)| k == "odp:warning"), "lost source semantics require a warning: {:?}", p.tags);
    assert!(export(p).is_err(), "edited export must not silently discard source semantics");
}

#[test]
fn namespace_spoofing_and_nearest_qname_rebinding_stay_guarded() {
    let plot = "<chart:series chart:values-cell-range-address='data.B1'><chart:domain table:cell-range-address='data.A1'/></chart:series>";
    for (class, attrs) in
        [("c:scatter", "xmlns:c='urn:review:spoofed-chart'"), ("scatter", "xmlns='urn:review:spoofed-chart'"), ("scatter", "xmlns=''")]
    {
        guarded(&with_chart(&chart_doc(class, attrs, "", plot)));
    }
    let p = with_chart(&chart_doc("scatter", "xmlns='urn:oasis:names:tc:opendocument:xmlns:chart:1.0'", "", plot));
    assert_eq!(native_chart(&p).kind, ChartType::Scatter);
    assert!(!p.tags.iter().any(|(k, _)| k == "odp:warning"), "{:?}", p.tags);
    let p = with_chart(&chart_doc(
        "chart:line",
        "",
        "",
        "<chart:series xmlns:c='urn:review:spoofed-chart' chart:class='c:line' chart:values-cell-range-address='data.B1'/>",
    ));
    guarded(&p);
}

#[test]
fn duplicate_expanded_attributes_are_rejected_even_with_aliases() {
    for attrs in [
        "xmlns:a='urn:review:x' xmlns:b='urn:review:x' a:flag='1' b:flag='2'",
        "xmlns:a='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' a:font-size='12pt' fo:font-size='14pt'",
    ] {
        let styles = format!("<style:style style:family='paragraph'><style:text-properties {attrs}/></style:style>");
        assert!(import(&package(&styles, "")).is_err());
    }
}

#[test]
fn source_namespaces_with_same_local_property_never_hide_a_foreign_value() {
    for attrs in [
        "fo:font-size='12pt' xmlns:q='urn:review:foreign' q:font-size='99pt'",
        "xmlns:q='urn:review:foreign' q:font-size='99pt' fo:font-size='12pt'",
        "xmlns:q='urn:review:foreign' q:font-size='99pt' font-size='100pt'",
    ] {
        let styles = format!("<style:style style:family='paragraph'><style:text-properties {attrs}/></style:style>");
        guarded(&import(&package(&styles, "")).unwrap());
    }
}

#[test]
fn relative_chart_font_size_is_resolved_or_explicitly_guarded() {
    for target in ["chart", "title"] {
        let style = "<style:style style:name='rel' style:family='chart'><style:text-properties fo:font-size='150%'/></style:style>";
        let (attrs, title) = if target == "chart" {
            ("chart:style-name='rel'", "")
        } else {
            ("", "<chart:title chart:style-name='rel'><text:p>Heading</text:p></chart:title>")
        };
        let doc = chart_doc("chart:line", attrs, style, "<chart:series chart:values-cell-range-address='data.B1'/>")
            .replace("<chart:plot-area>", &format!("{title}<chart:plot-area>"));
        let p = with_chart(&doc);
        let c = native_chart(&p);
        if if target == "chart" { c.text_size.is_none() } else { c.title_size.is_none() } {
            guarded(&p);
        }
    }
}

#[test]
fn xml_identity_metadata_limit_is_enforced_before_model_building() {
    // Source is under 9MiB and below the node count cap, but expanded attribute
    // identity metadata is over 32MiB. No opaque fragment amplification is needed.
    let styles = "<style:style style:family='paragraph' style:name='P'/>".repeat(170_000);
    let result = import(&package(&styles, ""));
    assert!(result.is_err(), "identity metadata must have a separate bounded budget");
    assert!(result.unwrap_err().0.contains("identity metadata"));
}

#[test]
fn repeated_embedded_parts_charge_identity_metadata_cumulatively() {
    let styles = "<style:style style:family='chart' style:name='P'/>".repeat(60_000);
    let doc = chart_doc("chart:line", "", &styles, "<chart:series chart:values-cell-range-address='data.B1'/>");
    for references in [1, 6] {
        let frames = "<draw:frame svg:width='100pt' svg:height='100pt'><draw:object xlink:href='./Object1'/></draw:frame>".repeat(references);
        let content = format!(
            "<office:document-content {NS} xmlns:xlink='http://www.w3.org/1999/xlink'><office:body><office:presentation><draw:page>{frames}</draw:page></office:presentation></office:body></office:document-content>"
        );
        let base = export(&deck(Chart::sample(ChartType::Column))).unwrap();
        let mut src = zip::ZipArchive::new(Cursor::new(base)).unwrap();
        let mut dst = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..src.len() {
            let mut entry = src.by_index(i).unwrap();
            let name = entry.name().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            dst.start_file(&name, zip::write::SimpleFileOptions::default()).unwrap();
            dst.write_all(match name.as_str() {
                "Object1/content.xml" => doc.as_bytes(),
                "content.xml" => content.as_bytes(),
                _ => &bytes,
            })
            .unwrap();
        }
        let result = import(&dst.finish().unwrap().into_inner());
        if references == 1 {
            assert!(result.is_ok(), "single-part metadata is below its per-part cap: {result:?}");
        } else {
            assert!(result.unwrap_err().0.contains("XML namespace identities exceed total import budget"));
        }
    }
}

#[test]
fn report_saturation_records_bounded_details_and_recovers_exact_original() {
    let styles = (0..600)
        .map(|index| format!("<style:style style:name='Saturation{index}' style:family='paragraph'><style:paragraph-properties fo:text-align-last='PRIVATE VALUE'/></style:style>"))
        .collect::<String>();
    let bytes = package(&styles, "");
    let p = import(&bytes).unwrap();
    guarded(&p);
    assert!(p.tags.iter().any(|(k, v)| k == "odp:semantic-audit" && v == "coverage=1; retained=128; omitted=472"), "{:?}", p.tags);
    assert!(!p.tags.iter().any(|(_, v)| v.contains("PRIVATE VALUE")));
    assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), bytes);
}

#[test]
fn numeric_chart_export_respects_reader_expanded_cell_text_budget() {
    let c = Chart {
        kind: ChartType::Line,
        title: None,
        legend: None,
        series: (0..8).map(|i| Series { name: format!("S{i}"), values: vec![Some(f64::MAX); 999], ..Default::default() }).collect(),
        ..Default::default()
    };
    // A safe refusal is valid. Successful exports must remain readable under
    // the existing 2MiB expanded chart-cell text cap, including numeric labels.
    if let Ok(encoded) = export(&deck(c)) {
        assert!(import(&encoded).is_ok(), "a successful editable export must fit its reader's expanded numeric text budget");
    }
}

#[test]
fn native_chart_text_settings_do_not_generate_a_blocked_reimport() {
    for (title, title_size, text_size) in [(None, Some(18.), None), (Some("Heading".into()), Some(10_001.), None), (None, None, Some(10_001.))] {
        let c = Chart {
            kind: ChartType::Line,
            title,
            title_size,
            text_size,
            legend: None,
            series: vec![Series { values: vec![Some(3.)], ..Default::default() }],
            ..Default::default()
        };
        if let Ok(encoded) = export(&deck(c)) {
            let reopened = import(&encoded).unwrap();
            assert!(export(&reopened).is_ok(), "successful chart exports must not become blocked merely by reimport: {:?}", reopened.tags);
        }
    }
}

#[test]
fn implicit_numeric_x_keeps_the_rendered_extent_of_long_missing_domains() {
    let c = Chart {
        kind: ChartType::Scatter,
        title: None,
        legend: None,
        series: vec![Series { values: vec![Some(1.), Some(2.)], x_values: vec![None; 8], ..Default::default() }],
        ..Default::default()
    };
    let p = deck(c);
    if let Ok(encoded) = export(&p) {
        let reopened = import(&encoded).unwrap();
        let before = deckcraft_render::render_slide(&p, 0, &Default::default());
        let after = deckcraft_render::render_slide(&reopened, 0, &Default::default());
        let differing = before.pixels.iter().zip(&after.pixels).filter(|(a, b)| a.abs_diff(**b) > 3).count();
        assert!(differing < 80, "all-missing X ranges still determine the renderer's implicit X extent: {differing} changed channels");
    }
}

#[test]
fn chart_table_limits_cover_extra_domain_and_error_columns() {
    use deckcraft_model::chart::ErrorBars;
    for count in [1, 7, 19] {
        let c = Chart {
            kind: ChartType::Scatter,
            title: None,
            legend: None,
            series: (0..count)
                .map(|_| Series {
                    values: vec![Some(3.); 999],
                    x_values: vec![Some(2.); 999],
                    error_bars: Some(ErrorBars {
                        val_type: "cust".into(),
                        plus: vec![Some(1.); 999],
                        minus: vec![Some(1.); 999],
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let result = export(&deck(c));
        if count == 1 {
            assert!(import(&result.unwrap()).is_ok());
        } else {
            assert!(result.is_err(), "domain and error columns count against 20,000 cells");
        }
    }
}

#[test]
fn absent_secondary_axis_names_and_extra_domains_stay_guarded() {
    for plot in [
        "<chart:series chart:attached-axis='missing-y' chart:values-cell-range-address='data.B1'/>",
        "<chart:series chart:values-cell-range-address='data.B1'><chart:domain table:cell-range-address='data.A1'/></chart:series>",
    ] {
        guarded(&with_chart(&chart_doc("chart:line", "", "", plot)));
    }
}
