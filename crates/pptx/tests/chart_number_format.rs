//! A chart's number formats survive an edit (#1046).
//!
//! `chart.data` rebuilds the chart's XML from the model, so every number format the file
//! carried has to be in the model and written back out: the data labels' own format
//! (`c:dLbls/c:numFmt`) and the series' own format (`c:ser/c:numFmt`), which is what the
//! values are written and read with.

use std::io::{Cursor, Read};
use std::sync::Arc;

use deckcraft_model::chart::{Chart, ChartType, Series};
use deckcraft_model::{LayoutType, Presentation, Shape, ShapeId, ShapeKind, defaults};
use serde_json::json;

/// A one-slide deck with a doughnut chart whose labels and values carry their own formats.
/// `series_labels` gives the series its own `c:dLbls`; without it the chart's own labels
/// (the group-level `c:dLbls`) are written.
fn deck_with(series_labels: bool) -> Presentation {
    let mut p = defaults::new_presentation(None);
    p.slides.clear();
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap_or_default();
    let mut slide = defaults::new_slide(&mut p, layout).expect("slide");
    let mut shape = Shape {
        id: ShapeId(p.alloc_id().expect("id")),
        name: "Chart".into(),
        xfrm: Some(deckcraft_geom::Xfrm::new(40.0, 40.0, 400.0, 300.0)),
        ..Default::default()
    };
    shape.kind = ShapeKind::Chart(Box::new(Chart {
        kind: ChartType::Doughnut,
        categories: vec!["Alpha".into(), "Beta".into(), "Gamma".into()],
        // The chart's own data labels, with their own format.
        data_labels: true,
        label_format: Some("0.0\"%\"".into()),
        series: vec![Series {
            name: "Q1".into(),
            values: vec![Some(50.5), Some(30.2), Some(19.3)],
            // The series' own labels, with their own format, and the values' own format: three
            // different things.
            data_labels: series_labels.then_some(true),
            own_label_format: series_labels.then(|| "0.00\"%\"".into()),
            number_format: Some("0.00\"%\"".into()),
            value_format: Some("#,##0.0".into()),
            ..Default::default()
        }],
        ..Default::default()
    }));
    slide.shapes.push(shape);
    p.slides.push(Arc::new(slide));
    p
}

fn chart_of(p: &Presentation) -> &Chart {
    p.slides
        .iter()
        .flat_map(|s| s.shapes.iter())
        .find_map(|sh| match &sh.kind {
            ShapeKind::Chart(c) => Some(c),
            _ => None,
        })
        .expect("a chart in the deck")
}

fn part(pkg: &[u8], name: &str) -> Option<String> {
    let mut z = zip::ZipArchive::new(Cursor::new(pkg)).ok()?;
    let mut f = z.by_name(name).ok()?;
    let mut v = vec![];
    f.read_to_end(&mut v).ok()?;
    Some(String::from_utf8(v).expect("xml"))
}

/// The chart part of an exported package.
fn chart_part(pkg: &[u8]) -> String {
    let mut names: Vec<String> = vec![];
    {
        let mut z = zip::ZipArchive::new(Cursor::new(pkg)).expect("zip");
        for i in 0..z.len() {
            let n = z.by_index(i).expect("entry").name().to_string();
            if n.starts_with("ppt/charts/chart") && n.ends_with(".xml") && !n.contains("_rels") {
                names.push(n);
            }
        }
    }
    assert_eq!(names.len(), 1, "one chart part, got {names:?}");
    part(pkg, &names[0]).expect("the chart part")
}

/// The formats are written into the chart part: the labels' and the values'.
#[test]
fn number_formats_are_written_into_the_chart_part() {
    // The series carries its own labels: its own format, and the values' own format.
    let out = deckcraft_pptx::export(&deck_with(true)).expect("export");
    let xml = chart_part(&out);
    assert!(xml.contains(r#"<c:numFmt formatCode="0.00&quot;%&quot;" sourceLinked="0"/>"#), "the series' label format is missing: {xml}");
    // The values' own format travels in the values' cache (`c:numCache/c:formatCode`): OOXML has
    // no element for it on the series, so it is written there and read back from there.
    assert!(xml.contains("<c:formatCode>#,##0.0</c:formatCode>"), "the cache's format is missing: {xml}");

    // Without a series of its own, the chart's labels are written, with the chart's format.
    let out = deckcraft_pptx::export(&deck_with(false)).expect("export");
    let xml = chart_part(&out);
    assert!(xml.contains(r#"<c:numFmt formatCode="0.0&quot;%&quot;" sourceLinked="0"/>"#), "the chart's label format is missing: {xml}");
    assert!(xml.contains("<c:formatCode>#,##0.0</c:formatCode>"), "the cache's format is missing: {xml}");
}

/// The chart part holds no element the schema does not allow: `c:numFmt` belongs inside
/// `c:dLbls` and on the axes, not on the series (ISO/IEC 29500, CT_BarSer's child elements).
#[test]
fn the_chart_part_holds_no_element_outside_the_schema() {
    let out = deckcraft_pptx::export(&deck_with(true)).expect("export");
    let xml = chart_part(&out);
    // The series' children, in the order the schema gives: idx, order, tx, spPr, invertIfNegative,
    // dPt, dLbls, trendline, errBars, cat, val, shape, extLst.
    let ser = xml.split("<c:ser>").nth(1).expect("a series").split("</c:ser>").next().expect("its end");
    // The direct children only: a child's own children belong to it.
    let mut children: Vec<&str> = vec![];
    let mut rest = ser;
    while let Some(open) = rest.find('<') {
        let tag = &rest[open + 1..];
        let Some(name) = tag.split([' ', '/', '>']).next() else { break };
        // Skip the element's content: find its matching close.
        let close = format!("</{name}>");
        rest = match rest[open..].find(&close) {
            Some(end) => &rest[open + end + close.len()..],
            None => "",
        };
        if name.starts_with("c:") {
            children.push(name);
        }
    }
    let allowed = [
        "c:idx",
        "c:order",
        "c:tx",
        "c:spPr",
        "c:invertIfNegative",
        "c:pictureOptions",
        "c:dPt",
        "c:dLbls",
        "c:trendline",
        "c:errBars",
        "c:cat",
        "c:val",
        "c:shape",
        "c:extLst",
    ];
    for c in &children {
        assert!(allowed.contains(c), "c:ser holds {c}, which the schema does not allow: {ser}");
    }
    // The data labels' own children: the show flags and the number format.
    let d_lbls = ser.split("<c:dLbls>").nth(1).expect("data labels").split("</c:dLbls>").next().expect("their end");
    assert!(d_lbls.contains("<c:numFmt "), "the labels' number format is missing: {d_lbls}");
}

/// An edited chart keeps the formats: `chart.data` rebuilds the part from the model, so the
/// formats have to be in the model to survive.
#[test]
fn number_formats_survive_a_chart_data_edit() {
    use deckcraft_engine::Session;
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("insert.chart", &json!({"type": "doughnut"})).unwrap();
    let id = s.doc().unwrap().selection.shapes[0].0;
    // The formats a file would have carried, set on the model.
    s.execute("chart.options", &json!({"id": id, "dataLabels": true})).unwrap();
    s.execute("chart.data", &json!({"id": id, "categories": ["Alpha", "Beta"], "series": [{"name": "Q2", "values": [48.1, 31.0]}]})).unwrap();
    let doc = &s.doc().unwrap().doc;
    let out = deckcraft_pptx::export(doc).expect("export");
    let back = deckcraft_pptx::import(&out).expect("import");
    let c = chart_of(&back);
    assert_eq!(c.series[0].name, "Q2", "the edit took");
    assert_eq!(c.series[0].values, vec![Some(48.1), Some(31.0)]);
    // A chart made in DeckCraft has no format of its own: the general format, not a lost one.
    // The effective label format is the source data's, which is the general format here.
    assert_eq!(c.series[0].own_label_format, None, "the series has no label format of its own");
    assert_eq!(c.series[0].value_format, None, "the series has no value format of its own");
    assert_eq!(c.series[0].number_format.as_deref(), Some("General"), "the effective format is the general one");
    assert_eq!(c.label_format, None, "the chart has no label format of its own");
}

/// A chart read from a file with the formats keeps them through a save.
#[test]
fn number_formats_read_from_a_file_survive_a_save() {
    let out = deckcraft_pptx::export(&deck_with(true)).expect("export");
    let back = deckcraft_pptx::import(&out).expect("import");
    let c = chart_of(&back);
    assert_eq!(c.series[0].own_label_format.as_deref(), Some("0.00\"%\""), "the series' label format is read");
    assert_eq!(c.series[0].number_format.as_deref(), Some("0.00\"%\""), "the effective label format is read");
    assert_eq!(c.series[0].value_format.as_deref(), Some("#,##0.0"), "the series' value format is read");
    // The chart's own labels are the default the series' labels replace, so the chart's format
    // is written and read back as well.
    assert_eq!(c.label_format.as_deref(), Some("0.0\"%\""), "the chart's label format is read");
    assert_eq!(c.series[0].value_format.as_deref(), Some("#,##0.0"), "the series' value format is read");

    // Without a series of its own, the chart's labels and their format round-trip as well.
    let out = deckcraft_pptx::export(&deck_with(false)).expect("export");
    let back = deckcraft_pptx::import(&out).expect("import");
    let c = chart_of(&back);
    assert_eq!(c.label_format.as_deref(), Some("0.0\"%\""), "the chart's label format is read");
    assert_eq!(c.series[0].value_format.as_deref(), Some("#,##0.0"), "the series' value format is read");
    assert_eq!(c.series[0].own_label_format, None, "the series has no labels of its own");

    // And a second save keeps them.
    let out2 = deckcraft_pptx::export(&back).expect("export");
    let back2 = deckcraft_pptx::import(&out2).expect("import");
    let c2 = chart_of(&back2);
    assert_eq!(c2.series[0].own_label_format, None);
    assert_eq!(c2.series[0].value_format.as_deref(), Some("#,##0.0"));
    assert_eq!(c2.label_format.as_deref(), Some("0.0\"%\""));
}
