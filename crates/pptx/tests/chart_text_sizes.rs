//! A chart's text sizes survive an edit (#1046).
//!
//! `chart.data` rebuilds the chart's XML from the model, and the model did not carry the text
//! sizes the file had: the data labels' own size, the legend's own size, the axis labels' size
//! and the title's size. The reader read some of them (the axes', the legend's and the title's,
//! collapsed into one size) and the writer wrote none of them, so an edited chart lost the sizes
//! and PowerPoint fell back to its defaults.

use std::io::{Cursor, Read};

use deckcraft_model::chart::{Chart, ChartType, Series};
use deckcraft_model::{LayoutType, Presentation, Shape, ShapeId, ShapeKind, defaults};

/// A one-slide deck with a column chart whose text sizes are set.
fn deck() -> Presentation {
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
        kind: ChartType::Column,
        title: Some("Share".into()),
        categories: vec!["Alpha".into(), "Beta".into(), "Gamma".into()],
        legend: Some("r".into()),
        // The chart's own data labels, with their own size.
        data_labels: true,
        label_size: Some(16.0),
        // The axis labels' size, the legend's own and the title's.
        text_size: Some(11.0),
        legend_size: Some(18.0),
        title_size: Some(20.0),
        series: vec![Series {
            name: "Q1".into(),
            values: vec![Some(50.5), Some(30.2), Some(19.3)],
            // The series' own labels, with their own size.
            data_labels: Some(true),
            label_size: Some(12.0),
            ..Default::default()
        }],
        ..Default::default()
    }));
    slide.shapes.push(shape);
    p.slides.push(std::sync::Arc::new(slide));
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
    let mut z = zip::ZipArchive::new(Cursor::new(pkg)).expect("zip");
    let mut f = z.by_name(&names[0]).expect("the chart part");
    let mut v = vec![];
    f.read_to_end(&mut v).expect("read");
    String::from_utf8(v).expect("xml")
}

/// The sizes are written into the chart part: the labels', the legend's, the axis labels' and the
/// title's, in hundredths of a point as PowerPoint writes them.
#[test]
fn text_sizes_are_written_into_the_chart_part() {
    let out = deckcraft_pptx::export(&deck()).expect("export");
    let xml = chart_part(&out);
    // The writer emits `a:defRPr sz="…"` (self-closing when it has no colour to carry).
    for (sz, what) in [
        ("1600", "the chart's data labels"),
        ("1200", "the series' data labels"),
        ("1800", "the legend"),
        ("1100", "the axis labels"),
        ("2000", "the title"),
    ] {
        assert!(xml.contains(&format!("<a:defRPr sz=\"{sz}\">")), "{what}' size is missing: {xml}");
    }
}

/// A chart read from a file with the sizes keeps them through a save.
#[test]
fn text_sizes_read_from_a_file_survive_a_save() {
    let out = deckcraft_pptx::export(&deck()).expect("export");
    let back = deckcraft_pptx::import(&out).expect("import");
    let c = chart_of(&back);
    assert_eq!(c.label_size, Some(16.0), "the chart's data labels' size is read");
    assert_eq!(c.series[0].label_size, Some(12.0), "the series' data labels' size is read");
    assert_eq!(c.legend_size, Some(18.0), "the legend's size is read");
    assert_eq!(c.text_size, Some(11.0), "the axis labels' size is read");
    assert_eq!(c.title_size, Some(20.0), "the title's size is read");
    // And a second save keeps them.
    let out2 = deckcraft_pptx::export(&back).expect("export");
    let back2 = deckcraft_pptx::import(&out2).expect("import");
    let c2 = chart_of(&back2);
    assert_eq!(c2.label_size, Some(16.0));
    assert_eq!(c2.series[0].label_size, Some(12.0));
    assert_eq!(c2.legend_size, Some(18.0));
    assert_eq!(c2.text_size, Some(11.0));
    assert_eq!(c2.title_size, Some(20.0));
}

/// An edited chart keeps the sizes: `chart.data` rebuilds the part from the model, so the sizes
/// have to be in the model to survive.
#[test]
fn text_sizes_survive_a_chart_data_edit() {
    use deckcraft_engine::Session;
    let mut s = Session::with_new();
    s.execute("slide.new", &serde_json::json!({"layout": "blank"})).unwrap();
    s.execute("insert.chart", &serde_json::json!({"type": "doughnut"})).unwrap();
    let id = s.doc().unwrap().selection.shapes[0].0;
    s.execute("chart.data", &serde_json::json!({"id": id, "categories": ["Alpha", "Beta"], "series": [{"name": "Q2", "values": [48.1, 31.0]}]}))
        .unwrap();
    let doc = &s.doc().unwrap().doc;
    let out = deckcraft_pptx::export(doc).expect("export");
    let back = deckcraft_pptx::import(&out).expect("import");
    let c = chart_of(&back);
    assert_eq!(c.series[0].name, "Q2", "the edit took");
    assert_eq!(c.series[0].values, vec![Some(48.1), Some(31.0)]);
    // A chart made in DeckCraft has no size of its own: the chart's own, not a lost one.
    assert_eq!(c.label_size, None, "the chart has no data labels' size of its own");
    assert_eq!(c.series[0].label_size, None, "the series has no data labels' size of its own");
}
