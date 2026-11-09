//! Series-level data labels: `c:dLbls/c:showVal` on a series overrides the chart-wide setting,
//! and a bare `<c:showVal/>` (no `val` attribute) means true, as the schema's default (#868).

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::chart::{Chart, ChartType};
use deckcraft_model::{LayoutType, Presentation, Shape, ShapeId, ShapeKind, defaults};

/// A one-slide deck whose bar chart shows no labels anywhere.
fn bar_chart_pptx() -> Vec<u8> {
    let mut p = defaults::new_presentation(None);
    p.slides.clear();
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap_or_default();
    let mut slide = defaults::new_slide(&mut p, layout).expect("slide");
    let mut shape =
        Shape { id: ShapeId(p.alloc_id().expect("id")), name: "Bars".into(), xfrm: Some(Xfrm::new(40.0, 40.0, 400.0, 300.0)), ..Default::default() };
    let mut chart = Chart::sample(ChartType::Column);
    chart.data_labels = false;
    for s in &mut chart.series {
        s.data_labels = None;
    }
    shape.kind = ShapeKind::Chart(Box::new(chart));
    slide.shapes.push(shape);
    p.slides.push(Arc::new(slide));
    deckcraft_pptx::export(&p).expect("export")
}

/// Rewrite the chart part's XML through `f` and re-import the deck.
fn reimport(bytes: &[u8], f: impl Fn(&str) -> String) -> Presentation {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().to_string();
        let mut data = vec![];
        entry.read_to_end(&mut data).expect("read");
        if name.starts_with("ppt/charts/chart") && name.ends_with(".xml") && !name.contains("_rels") {
            let xml = String::from_utf8(data).expect("chart xml");
            data = f(&xml).into_bytes();
        }
        out.start_file(&name, opts).expect("start");
        out.write_all(&data).expect("write");
    }
    let patched = out.finish().expect("finish").into_inner();
    deckcraft_pptx::import(&patched).expect("import")
}

fn chart_of(p: &Presentation) -> &Chart {
    let s = p.slides.first().expect("slide");
    for sh in &s.shapes {
        if let ShapeKind::Chart(c) = &sh.kind {
            return c;
        }
    }
    panic!("no chart in the deck");
}

/// A data edit must retain formatting that the model doesn't describe, not just pass no-op export.
#[test]
fn data_edit_preserves_axis_fonts_and_units() {
    use deckcraft_engine::Session;
    let p = reimport(&bar_chart_pptx(), |xml| {
        xml.replace("</c:valAx>", r#"<c:majorUnit val="10"/></c:valAx>"#)
            .replace("<c:crossAx", r#"<c:txPr><a:bodyPr/><a:p><a:pPr><a:defRPr sz="1350"><a:latin typeface="Example Sans"/><a:ea typeface="Example CJK"/></a:defRPr></a:pPr></a:p></c:txPr><c:crossAx"#)
    });
    let source = deckcraft_pptx::export(&p).expect("source");
    let path = std::env::temp_dir().join(format!("deckcraft-data-format-{}.pptx", std::process::id()));
    std::fs::write(&path, source).expect("write source");
    let mut session = Session::with_new();
    session.execute("file.open", &serde_json::json!({"path": path})).expect("open");
    let id = session.doc().unwrap().doc.slides[0].shapes[0].id.0;
    let series: Vec<_> = chart_of(&p).series.iter().map(|s| serde_json::json!({"name": s.name, "values": [15, null, 40]})).collect();
    session.execute("chart.data", &serde_json::json!({"id": id, "categories": ["One", "Two", "Three"], "series": series})).expect("edit");
    let p = session.doc().unwrap().doc.clone();
    let bytes = deckcraft_pptx::export(&p).expect("edited export");
    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
    let mut xml = String::new();
    zip.by_name("ppt/charts/chart1.xml").expect("chart").read_to_string(&mut xml).expect("xml");
    assert!(xml.contains(r#"typeface="Example Sans""#), "Latin font was dropped");
    assert!(xml.contains(r#"typeface="Example CJK""#), "East Asian font was dropped");
    assert!(xml.contains(r#"sz="1350""#), "font size was dropped");
    assert!(xml.contains(r#"majorUnit val="10""#), "axis unit was dropped");
    let back = deckcraft_pptx::import(&bytes).expect("read edit");
    assert_eq!(chart_of(&back).series[0].values, vec![Some(15.0), None, Some(40.0)]);
    assert_eq!(chart_of(&back).categories, ["One", "Two", "Three"]);
    let _ = std::fs::remove_file(path);
}

/// `<c:showVal/>` on the first series: that series shows labels although the chart-wide
/// setting is off; the other series stay quiet.
#[test]
fn bare_series_showval_shows_labels() {
    let p = reimport(&bar_chart_pptx(), |xml| xml.replacen("<c:cat>", r#"<c:dLbls><c:showVal/></c:dLbls><c:cat>"#, 1));
    let c = chart_of(&p);
    assert!(!c.data_labels, "the chart-wide setting stays off");
    assert_eq!(c.series[0].data_labels, Some(true), "a bare showVal means true");
    assert!(c.series_data_labels(&c.series[0]), "the first series' labels show");
    for s in c.series.iter().skip(1) {
        assert_eq!(s.data_labels, None, "other series keep the chart's setting");
        assert!(!c.series_data_labels(s), "and stay quiet");
    }
}

/// `val="0"` on the series hides its labels although the chart-wide setting is on.
#[test]
fn series_showval_false_hides_labels() {
    let p = reimport(&bar_chart_pptx(), |xml| {
        xml.replacen("<c:cat>", r#"<c:dLbls><c:showVal val="0"/></c:dLbls><c:cat>"#, 1).replacen(
            "<c:gapWidth",
            r#"<c:dLbls><c:showVal/></c:dLbls><c:gapWidth"#,
            1,
        )
    });
    let c = chart_of(&p);
    assert!(c.data_labels, "the bare chart-wide showVal turns labels on");
    assert_eq!(c.series[0].data_labels, Some(false), "the explicit series false is read");
    assert!(!c.series_data_labels(&c.series[0]), "and hides that series' labels");
    assert!(c.series_data_labels(&c.series[1]), "the next series takes the chart-wide on");
}

/// The series' label setting survives a save and load.
#[test]
fn series_label_settings_round_trip() {
    let mut p = defaults::new_presentation(None);
    p.slides.clear();
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap_or_default();
    let mut slide = defaults::new_slide(&mut p, layout).expect("slide");
    let mut chart = Chart::sample(ChartType::Column);
    chart.data_labels = true;
    chart.series[0].data_labels = Some(false);
    chart.series[1].data_labels = Some(true);
    let mut shape =
        Shape { id: ShapeId(p.alloc_id().expect("id")), name: "Bars".into(), xfrm: Some(Xfrm::new(40.0, 40.0, 400.0, 300.0)), ..Default::default() };
    shape.kind = ShapeKind::Chart(Box::new(chart));
    slide.shapes.push(shape);
    p.slides.push(Arc::new(slide));

    let bytes = deckcraft_pptx::export(&p).expect("export");
    let back = deckcraft_pptx::import(&bytes).expect("import");
    let c = chart_of(&back);
    assert!(c.data_labels);
    assert_eq!(c.series[0].data_labels, Some(false), "the explicit off survives");
    assert_eq!(c.series[1].data_labels, Some(true), "the explicit on survives");
    assert!(!c.series_data_labels(&c.series[0]));
    assert!(c.series_data_labels(&c.series[1]));
}

/// A one-slide deck with a combo chart: a bar group and a line group.
fn combo_chart_pptx() -> Vec<u8> {
    let mut p = defaults::new_presentation(None);
    p.slides.clear();
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap_or_default();
    let mut slide = defaults::new_slide(&mut p, layout).expect("slide");
    let mut chart = Chart::sample(ChartType::Combo);
    chart.series.truncate(2);
    chart.series[1].kind = Some(ChartType::Line);
    let mut shape =
        Shape { id: ShapeId(p.alloc_id().expect("id")), name: "Bars".into(), xfrm: Some(Xfrm::new(40.0, 40.0, 400.0, 300.0)), ..Default::default() };
    shape.kind = ShapeKind::Chart(Box::new(chart));
    slide.shapes.push(shape);
    p.slides.push(Arc::new(slide));
    deckcraft_pptx::export(&p).expect("export")
}

/// A combo chart's groups keep their own label setting: the group that asks for no labels
/// keeps them off, even when the other group asks for them (#868).
#[test]
fn a_combo_chart_keeps_each_groups_own_label_setting() {
    let p = reimport(&combo_chart_pptx(), |xml| {
        // The bar group asks for no labels, the line group for labels.
        xml.replacen("<c:gapWidth", r#"<c:dLbls><c:showVal val="0"/></c:dLbls><c:gapWidth"#, 1).replacen(
            "<c:marker val=\"1\"/>",
            r#"<c:dLbls><c:showVal/></c:dLbls><c:marker val="1"/>"#,
            1,
        )
    });
    let c = chart_of(&p);
    assert!(c.data_labels, "the line group's ask turns the chart-wide setting on");
    assert_eq!(c.series[0].data_labels, Some(false), "the bar group asks for no labels");
    assert!(!c.series_data_labels(&c.series[0]), "so the bars stay quiet");
    assert!(c.series_data_labels(&c.series[1]), "the line takes the chart-wide on");
}

/// Turning Data Labels on through `chart.options` shows the labels of series that the file
/// switched off one by one.
#[test]
fn chart_options_data_labels_on_overrides_series_off() {
    use deckcraft_engine::Session;
    let p = reimport(&bar_chart_pptx(), |xml| xml.replace("<c:cat>", r#"<c:dLbls><c:showVal val="0"/></c:dLbls><c:cat>"#));
    assert!(chart_of(&p).series.iter().all(|s| !chart_of(&p).series_data_labels(s)), "every series starts quiet");
    let path = std::env::temp_dir().join("deckcraft_series_labels_off.pptx");
    std::fs::write(&path, deckcraft_pptx::export(&p).expect("export")).expect("write");
    let mut s = Session::with_new();
    s.execute("file.open", &serde_json::json!({"path": path.to_string_lossy()})).expect("open");
    let doc = s.doc().unwrap().doc.clone();
    let id = doc.slides[0].shapes.iter().find(|sh| matches!(sh.kind, ShapeKind::Chart(_))).expect("chart").id.0;
    assert!(chart_of(&doc).series.iter().all(|x| !chart_of(&doc).series_data_labels(x)));
    s.execute("chart.options", &serde_json::json!({"id": id, "dataLabels": true})).expect("options");
    let doc = s.doc().unwrap().doc.clone();
    let c = chart_of(&doc);
    assert!(c.series.iter().all(|x| c.series_data_labels(x)), "every series shows its labels");
    let _ = std::fs::remove_file(path);
}

#[test]
fn distinct_chart_typography_survives_structural_rebuild() {
    use deckcraft_model::chart::ChartTextRole;
    let mut p = reimport(&bar_chart_pptx(), |xml| {
        let props = |font: &str, size: u32| {
            format!(
                r#"<c:txPr><a:bodyPr/><a:p><a:pPr><a:defRPr sz="{size}" lang="zh-TW" b="1"><a:latin typeface="{font}"/><a:ea typeface="Example CJK"/></a:defRPr></a:pPr></a:p></c:txPr>"#
            )
        };
        xml.replace("</c:catAx>", &format!("{}</c:catAx>", props("Example Category", 1100)))
            .replace("</c:valAx>", &format!("{}</c:valAx>", props("Example Value", 1500)))
            .replace("</c:legend>", &format!("{}</c:legend>", props("Example Legend", 2100)))
    });
    let before = chart_of(&p).clone();
    assert_eq!(before.text_props(ChartTextRole::CategoryAxis).size, Some(11.0));
    assert_eq!(before.text_props(ChartTextRole::ValueAxis).size, Some(15.0));
    assert_eq!(before.text_props(ChartTextRole::Legend).font.as_deref(), Some("Example Legend"));
    let ShapeKind::Chart(c) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
    c.series.push(c.series[0].clone()); // force the rebuild path, not raw XML reuse or a data-only patch
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    for role in [ChartTextRole::CategoryAxis, ChartTextRole::ValueAxis, ChartTextRole::Legend] {
        let props = chart_of(&back).text_props(role);
        assert_eq!(props, before.text_props(role), "{role:?}");
    }
}

#[test]
fn rebuilt_combo_chart_keeps_secondary_axis_and_distinct_typography() {
    use deckcraft_model::chart::ChartTextRole;
    use deckcraft_model::text::RunProps;
    let mut p = deckcraft_pptx::import(&bar_chart_pptx()).unwrap();
    let ShapeKind::Chart(c) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("chart") };
    c.kind = ChartType::Combo;
    c.raw = None;
    c.series[0].kind = Some(ChartType::Column);
    c.series[1].kind = Some(ChartType::Line);
    c.series[1].secondary_axis = true;
    c.y_min = Some(0.0);
    c.y_max = Some(10.0);
    c.y_major = Some(2.0);
    c.y2_min = Some(100.0);
    c.y2_max = Some(1000.0);
    c.y2_major = Some(100.0);
    c.y2_format = Some("0.0".into());
    c.text_styles
        .insert(ChartTextRole::SecondaryValueAxis, RunProps { font: Some("Example Right Axis".into()), size: Some(17.0), ..Default::default() });
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    let c = chart_of(&back);
    assert_eq!((c.y_min, c.y_max, c.y_major), (Some(0.0), Some(10.0), Some(2.0)));
    assert_eq!((c.y2_min, c.y2_max, c.y2_major), (Some(100.0), Some(1000.0), Some(100.0)));
    assert!(c.series.iter().find(|s| s.name == "Series 2").unwrap().secondary_axis);
    assert!(!c.series.iter().find(|s| s.name == "Series 1").unwrap().secondary_axis);
    assert_eq!(c.text_props(ChartTextRole::SecondaryValueAxis).font.as_deref(), Some("Example Right Axis"));
    assert_eq!(c.series.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["Series 1", "Series 2", "Series 3"]);

    // Changing values must still patch the correct series when XML plot groups interleave them.
    let mut edited = back.clone();
    let ShapeKind::Chart(c) = &mut Arc::make_mut(&mut edited.slides[0]).shapes[0].kind else { panic!("chart") };
    c.series[1].values[0] = Some(777.0);
    let patched = deckcraft_pptx::import(&deckcraft_pptx::export(&edited).unwrap()).unwrap();
    assert_eq!(chart_of(&patched).series[1].values[0], Some(777.0));
    assert_eq!(chart_of(&patched).series[2].values, chart_of(&back).series[2].values);

    // Equal plot types can still belong to different value axes.
    let ShapeKind::Chart(c) = &mut Arc::make_mut(&mut edited.slides[0]).shapes[0].kind else { panic!("chart") };
    c.series[1].kind = Some(ChartType::Column);
    let same_type = deckcraft_pptx::import(&deckcraft_pptx::export(&edited).unwrap()).unwrap();
    assert_eq!(chart_of(&same_type).kind, ChartType::Combo);
    assert!(chart_of(&same_type).series[1].secondary_axis);
    assert_eq!(chart_of(&same_type).y2_max, Some(1000.0));
}
