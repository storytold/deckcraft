//! Original chart probes: render actual imported XML, not a preserved opaque package.
use deckcraft_color::Rgba;
use deckcraft_engine::render::{RenderOpts, render_slide};
use deckcraft_geom::Rect;
use deckcraft_model::chart::{ChartLayout, ChartLineRole as LineRole, ChartTextRole as Role, ChartType};
use deckcraft_model::style::Dash;
use deckcraft_model::text::{BodyProps, RunProps};
use deckcraft_model::{Chart, ColorRef, Fill, LayoutType, Line, Presentation, Shape, ShapeId, ShapeKind, Xfrm, defaults};
use std::sync::Arc;

fn append(p: &mut Presentation, c: Chart) {
    let layout = defaults::layout_of_kind(p, LayoutType::Blank).unwrap();
    let mut slide = defaults::new_slide(p, layout).unwrap();
    slide.shapes = vec![Shape {
        id: ShapeId(5000 + p.slides.len() as u32),
        xfrm: Some(Xfrm::new(40.0, 30.0, 880.0, 480.0)),
        kind: ShapeKind::Chart(Box::new(c)),
        ..Default::default()
    }];
    p.slides.push(Arc::new(slide));
}

fn chart(p: &Presentation, i: usize) -> &Chart {
    let ShapeKind::Chart(c) = &p.slides[i].shapes[0].kind else { panic!("chart") };
    c
}

fn styled(kind: ChartType) -> Chart {
    let mut c = Chart::sample(kind);
    c.title = Some(format!("{} / independent render probe", kind.label()));
    for (role, size) in [(Role::Default, 14.0), (Role::Title, 24.0)] {
        c.text_styles.insert(
            role,
            RunProps {
                font: Some("Inter".into()),
                size: Some(size),
                bold: Some(role == Role::Title),
                fill: Some(Fill::solid(ColorRef::rgb(Rgba::BLACK))),
                ..Default::default()
            },
        );
    }
    c
}

#[test]
fn chartex_statistics_and_layout_options_survive_rebuild_and_change_pixels() {
    use deckcraft_model::chart::ExtendedChartOptions;
    for kind in [ChartType::Histogram, ChartType::Pareto, ChartType::BoxWhisker, ChartType::Waterfall, ChartType::Treemap] {
        let mut c = styled(kind);
        c.extended = match kind {
            ChartType::Histogram | ChartType::Pareto => {
                ExtendedChartOptions { underflow: Some(3.0), overflow: Some(7.0), bin_closed_left: true, ..Default::default() }
            }
            ChartType::BoxWhisker => {
                ExtendedChartOptions { mean_marker: false, mean_line: true, inner_points: true, outliers: false, ..Default::default() }
            }
            ChartType::Waterfall => ExtendedChartOptions { connector_lines: false, ..Default::default() },
            _ => ExtendedChartOptions { parent_labels: Some("none".into()), ..Default::default() },
        };
        if kind == ChartType::Treemap {
            c.category_levels = vec![vec!["North".into(), "North".into(), "South".into(), "South".into()]];
        }
        let mut p = Presentation::default();
        p.slides.clear();
        append(&mut p, c.clone());
        let saved = deckcraft_pptx::export(&p).unwrap();
        let back = deckcraft_pptx::import(&saved).unwrap();
        assert_eq!(chart(&back, 0).extended, c.extended, "{kind:?}");
        let actual = render_slide(&back, 0, &RenderOpts::default());
        let mut default = p.clone();
        let ShapeKind::Chart(ch) = &mut Arc::make_mut(&mut default.slides[0]).shapes[0].kind else { panic!("chart") };
        ch.extended = Default::default();
        let baseline = render_slide(&default, 0, &RenderOpts::default());
        assert_ne!(actual.pixels, baseline.pixels, "{kind:?}: option was retained but not rendered");
        if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("extended-{kind:?}.pptx")), saved).unwrap();
            std::fs::write(dir.join(format!("extended-{kind:?}.png")), actual.to_png()).unwrap();
        }
    }
}

#[test]
fn chartex_scalar_elements_follow_their_distinct_schema_types() {
    use deckcraft_pptx::parse_xml;
    use std::io::Read;
    for kind in [ChartType::Histogram, ChartType::Waterfall] {
        let mut c = styled(kind);
        c.bin_count = Some(4);
        c.subtotals = vec![0, 3];
        let mut p = Presentation::default();
        p.slides.clear();
        append(&mut p, c);
        let bytes = deckcraft_pptx::export(&p).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let mut found = false;
        for i in 0..z.len() {
            let mut entry = z.by_index(i).unwrap();
            if !entry.name().contains("chartEx") || !entry.name().ends_with(".xml") {
                continue;
            }
            let mut b = Vec::new();
            entry.read_to_end(&mut b).unwrap();
            let root = parse_xml(&b).unwrap().root;
            let tag = if kind == ChartType::Histogram { "binCount" } else { "idx" };
            let scalar = root.find(tag).unwrap();
            if kind == ChartType::Histogram {
                assert_eq!(scalar.text(), "4");
                assert!(!scalar.has_attr("val"));
            } else {
                assert_eq!(scalar.attr("val"), Some("0"));
                assert!(scalar.text().is_empty());
            }
            found = true;
        }
        assert!(found);
    }
}

fn manual() -> Chart {
    let mut c = styled(ChartType::Combo);
    c.title = Some("Manual plot, colored axes and dashed grid".into());
    c.y_min = Some(0.0);
    c.y_max = Some(10.0);
    c.y_major = Some(2.0);
    c.y2_min = Some(0.0);
    c.y2_max = Some(1000.0);
    c.y2_major = Some(250.0);
    c.series[0].kind = Some(ChartType::Column);
    c.series[1].kind = Some(ChartType::Line);
    c.series[1].secondary_axis = true;
    c.series[1].values = vec![Some(200.0), Some(500.0), Some(400.0), Some(800.0)];
    c.format.plot_layout = Some(ChartLayout {
        inner: true,
        x: Some(0.15),
        y: Some(0.25),
        w: Some(0.65),
        h: Some(0.5),
        x_edge: true,
        y_edge: true,
        ..Default::default()
    });
    c.format.layouts.insert(
        Role::Title,
        ChartLayout { x: Some(0.08), y: Some(0.04), w: Some(0.84), h: Some(0.1), x_edge: true, y_edge: true, ..Default::default() },
    );
    c.format
        .layouts
        .insert(Role::Legend, ChartLayout { x: Some(0.25), y: Some(0.87), w: Some(0.65), x_edge: true, y_edge: true, ..Default::default() });
    c.legend_overlay = true;
    c.format.bodies.insert(Role::CategoryAxis, BodyProps { rot: Some(-35.0), ..Default::default() });
    c.format.lines.insert(
        LineRole::MajorGrid,
        Line { dash: Some(Dash::Preset { name: "dash".into() }), ..Line::solid(ColorRef::rgb(Rgba::rgb(200, 35, 55)), 1.5) },
    );
    c.format.lines.insert(LineRole::ValueAxis, Line::solid(ColorRef::rgb(Rgba::rgb(25, 90, 190)), 2.0));
    c.format.lines.insert(LineRole::SecondaryValueAxis, Line::solid(ColorRef::rgb(Rgba::rgb(30, 130, 75)), 2.0));
    c.format.lines.insert(LineRole::CategoryAxis, Line::none());
    c
}

#[test]
fn imported_manual_layout_and_line_styles_reach_the_pixels() {
    let mut p = Presentation::default();
    p.slides.clear();
    append(&mut p, manual());
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("ppt/charts/chart1.xml").unwrap(), &mut xml).unwrap();
    let root = deckcraft_pptx::parse_xml(xml.as_bytes()).unwrap().root;
    let chart_xml = root.child("chart").unwrap();
    for name in ["title", "legend"] {
        assert!(chart_xml.child(name).unwrap().find("layoutTarget").is_none());
    }
    assert!(chart_xml.child("plotArea").unwrap().find("layoutTarget").is_some());
    let mut back = deckcraft_pptx::import(&bytes).unwrap();
    assert_eq!(chart(&p, 0).format, chart(&back, 0).format);
    let img = render_slide(&back, 0, &RenderOpts::default());
    if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manual-chart.pptx"), &bytes).unwrap();
        std::fs::write(dir.join("manual-chart.png"), img.to_png()).unwrap();
    }
    // Plot is at x=172..744, y=150..390 in the slide. Inspect the middle of a
    // gridline away from series marks. These assertions fail on the old renderer.
    let red: Vec<_> = img
        .pixels
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, p)| p[0] > 150 && p[1] < 120 && p[2] < 130)
        .map(|(i, _)| ((i % img.width as usize) as f64, (i / img.width as usize) as f64))
        .collect();
    assert!(red.iter().filter(|(x, y)| *x > 180.0 && *x < 735.0 && (*y - 150.0).abs() < 2.0).count() > 100);
    let blue_axis = img
        .pixels
        .chunks_exact(4)
        .enumerate()
        .filter(|(i, p)| {
            let x = i % img.width as usize;
            let y = i / img.width as usize;
            (171..=173).contains(&x) && (155..385).contains(&y) && p[2] > 140 && p[0] < 80
        })
        .count();
    assert!(blue_axis > 300, "manual axis location/style missing: {blue_axis}");
    let ShapeKind::Chart(c) = &mut Arc::make_mut(&mut back.slides[0]).shapes[0].kind else { panic!("chart") };
    c.raw = None;
    c.title = Some("Edited chart still uses its explicit layout".into());
    let saved = deckcraft_pptx::import(&deckcraft_pptx::export(&back).unwrap()).unwrap();
    assert_eq!(chart(&saved, 0).format, chart(&back, 0).format);
    if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manual-chart.pptx"), bytes).unwrap();
        std::fs::write(dir.join("manual-chart.png"), img.to_png()).unwrap();
    }
}

#[test]
fn manual_layout_resolves_edges_offsets_and_hostile_coordinates() {
    let auto = Rect::new(100.0, 80.0, 500.0, 300.0);
    let mut l = ChartLayout { x: Some(0.1), y: Some(-0.1), w: Some(0.6), h: Some(0.8), w_edge: true, h_edge: true, ..Default::default() };
    assert_eq!(l.resolve(auto, 1000.0, 500.0), Rect::new(200.0, 30.0, 600.0, 400.0));
    l.x_edge = true;
    l.y_edge = true;
    l.y = Some(0.1);
    assert_eq!(l.resolve(auto, 1000.0, 500.0), Rect::new(100.0, 50.0, 600.0, 400.0));
    for bad in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
        assert_eq!(l.resolve(auto, bad, 500.0), auto);
        assert_eq!(l.resolve(auto, 1000.0, bad), auto);
    }
    for bad in [f64::NAN, f64::INFINITY, -2.0, 2.0] {
        l.x = Some(bad);
        assert_eq!(l.resolve(auto, 1000.0, 500.0), auto);
    }
}

#[test]
fn advanced_chart_matrix_is_renderable_from_saved_xml() {
    let mut p = Presentation::default();
    p.slides.clear();
    for kind in [
        ChartType::StackedColumn,
        ChartType::Scatter,
        ChartType::Bubble,
        ChartType::Waterfall,
        ChartType::BoxWhisker,
        ChartType::Treemap,
        ChartType::Sunburst,
        ChartType::Histogram,
        ChartType::Pareto,
    ] {
        append(&mut p, styled(kind));
    }
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    let dir = std::env::var_os("DECKCRAFT_RENDER_PROBES").map(std::path::PathBuf::from);
    if let Some(dir) = &dir {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("advanced-charts.pptx"), bytes).unwrap();
    }
    for i in 0..p.slides.len() {
        assert_eq!(chart(&p, i).kind, chart(&back, i).kind);
        let img = render_slide(&back, i, &RenderOpts::default());
        assert!(img.pixels.chunks_exact(4).filter(|p| p[0] < 150 && p[1] < 150).count() > 1000);
        if let Some(dir) = &dir {
            std::fs::write(dir.join(format!("advanced-{}.png", i + 1)), img.to_png()).unwrap();
            let mut one = p.clone();
            one.slides = vec![p.slides[i].clone()];
            std::fs::write(dir.join(format!("advanced-{}.pptx", i + 1)), deckcraft_pptx::export(&one).unwrap()).unwrap();
        }
    }
}

#[test]
fn chartex_axes_and_text_are_modeled_instead_of_only_kept_as_raw_xml() {
    let mut c = styled(ChartType::Waterfall);
    c.y_min = Some(-5.0);
    c.y_max = Some(20.0);
    c.y_major = Some(5.0);
    c.y_format = Some("0.0".into());
    c.axis_titles = (Some("Stage".into()), Some("Net value".into()));
    c.format.title_overlay = true;
    c.minor_gridlines = true;
    c.format.bodies.insert(Role::CategoryAxis, BodyProps { rot: Some(-25.0), ..Default::default() });
    c.format.lines.insert(LineRole::MajorGrid, Line::solid(ColorRef::rgb(Rgba::rgb(180, 20, 50)), 1.5));
    c.format.lines.insert(LineRole::MinorGrid, Line::solid(ColorRef::rgb(Rgba::rgb(20, 140, 50)), 0.5));
    c.series[0].fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(30, 90, 150))));
    let mut p = Presentation::default();
    p.slides.clear();
    append(&mut p, c);
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("ppt/charts/chartEx1.xml").unwrap(), &mut xml).unwrap();
    // Axis references are simple element text in chartEx, not c:axId/@val.
    assert!(xml.contains("<cx:axisId>0</cx:axisId><cx:axisId>1</cx:axisId>"));
    let mut back = deckcraft_pptx::import(&bytes).unwrap();
    assert_eq!(chart(&back, 0).text_props(Role::Title), chart(&p, 0).text_props(Role::Title));
    assert_eq!(chart(&back, 0).format, chart(&p, 0).format);
    assert_eq!(chart(&back, 0).series[0].fill, chart(&p, 0).series[0].fill);
    assert_eq!((chart(&back, 0).y_min, chart(&back, 0).y_max, chart(&back, 0).y_major), (Some(-5.0), Some(20.0), Some(5.0)));
    assert_eq!(chart(&back, 0).y_format.as_deref(), Some("0.0"));
    assert_eq!(chart(&back, 0).axis_titles, chart(&p, 0).axis_titles);
    let ShapeKind::Chart(c) = &mut Arc::make_mut(&mut back.slides[0]).shapes[0].kind else { panic!("chart") };
    c.raw = None;
    c.title = Some("Rebuilt with the same typography".into());
    let rebuilt = deckcraft_pptx::import(&deckcraft_pptx::export(&back).unwrap()).unwrap();
    assert_eq!(chart(&rebuilt, 0).format, chart(&back, 0).format);
}

#[test]
fn explicit_axis_title_rotation_is_applied_once_inside_its_manual_rectangle() {
    let mut c = styled(ChartType::Column);
    c.axis_titles.1 = Some("MMMMMMMM".into());
    c.text_styles.insert(
        Role::ValueTitle,
        RunProps {
            font: Some("Inter".into()),
            size: Some(18.0),
            fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(220, 0, 210)))),
            ..Default::default()
        },
    );
    c.format.bodies.insert(Role::ValueTitle, BodyProps { rot: Some(-90.0), ..Default::default() });
    c.format.layouts.insert(
        Role::ValueTitle,
        ChartLayout { x: Some(0.01), y: Some(0.15), w: Some(0.04), h: Some(0.65), x_edge: true, y_edge: true, ..Default::default() },
    );
    let mut p = Presentation::default();
    p.slides.clear();
    append(&mut p, c);
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    let img = render_slide(&back, 0, &RenderOpts::default());
    let mut bounds = (usize::MAX, usize::MAX, 0, 0);
    for (i, p) in img.pixels.chunks_exact(4).enumerate() {
        if p[0] > 150 && p[1] < 80 && p[2] > 150 {
            let (x, y) = (i % img.width as usize, i / img.width as usize);
            bounds.0 = bounds.0.min(x);
            bounds.1 = bounds.1.min(y);
            bounds.2 = bounds.2.max(x);
            bounds.3 = bounds.3.max(y);
        }
    }
    assert!(bounds.0 < bounds.2, "axis title was not drawn");
    assert!(bounds.2 < 100, "manual left-title rectangle was ignored: {bounds:?}");
    assert!(bounds.3 - bounds.1 > (bounds.2 - bounds.0) * 3, "explicit rotation was applied twice: {bounds:?}");
}

#[test]
fn box_chart_series_keep_independent_visibility_and_quartiles() {
    use deckcraft_model::ShapeKind;
    let mut c = styled(ChartType::BoxWhisker);
    assert!(c.series.len() >= 2);
    c.series[0].extended = Some(deckcraft_model::chart::ExtendedChartOptions { mean_marker: false, outliers: false, ..Default::default() });
    c.series[1].extended = Some(deckcraft_model::chart::ExtendedChartOptions { inner_points: true, mean_line: true, ..Default::default() });
    c.series[0].inclusive_quartiles = Some(true);
    c.series[1].inclusive_quartiles = Some(false);
    let expected = c.clone();
    let mut p = Presentation::default();
    p.slides.clear();
    append(&mut p, c);
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    let got = back.slides.iter().flat_map(|s| &s.shapes).find_map(|s| if let ShapeKind::Chart(c) = &s.kind { Some(c) } else { None }).unwrap();
    for (a, b) in expected.series.iter().zip(&got.series) {
        assert_eq!(a.extended, b.extended);
        assert_eq!(a.inclusive_quartiles, b.inclusive_quartiles);
    }
    assert_eq!(expected.box_groups(), got.box_groups());
}
