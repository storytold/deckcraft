//! Contributor-original probes: geographic geometry and rich data labels, no vendor assets.
use deckcraft_color::Rgba;
use deckcraft_engine::render::{RenderOpts, render_slide};
use deckcraft_model::{
    chart::{ChartTextRole, ChartType, DataLabel, Series},
    geography::{GeoRegion, Geography},
    text::{Run, TextBody},
    *,
};
use std::sync::Arc;

fn append(p: &mut Presentation, chart: Chart) {
    let slide = Slide {
        shapes: vec![Shape {
            id: ShapeId(1),
            xfrm: Some(Xfrm::new(0., 0., 960., 540.)),
            kind: ShapeKind::Chart(Box::new(chart)),
            ..Default::default()
        }],
        ..Default::default()
    };
    p.slides.push(Arc::new(slide));
}
fn chart(p: &Presentation) -> &Chart {
    let ShapeKind::Chart(c) = &p.slides[0].shapes[0].kind else { panic!("chart") };
    c
}
fn original() -> Presentation {
    let mut p = Presentation::default();
    p.slides.clear();
    p
}
fn base(kind: ChartType) -> Chart {
    let mut c = Chart::sample(kind);
    c.legend = None;
    c.title = None;
    c.series.clear();
    c.text_styles.insert(ChartTextRole::Default, RunProps { font: Some("Inter".into()), size: Some(16.), ..Default::default() });
    c
}
fn probe(p: &Presentation, name: &str) {
    if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), deckcraft_pptx::export(p).unwrap()).unwrap();
    }
}
fn rebuilt(p: &Presentation) -> Presentation {
    let bytes = deckcraft_pptx::export(p).unwrap();
    let mut back = deckcraft_pptx::import(&bytes).unwrap();
    for slide in &mut back.slides {
        for shape in &mut Arc::make_mut(slide).shapes {
            if let ShapeKind::Chart(c) = &mut shape.kind {
                c.raw = None;
                c.kept = Default::default();
            }
        }
    }
    deckcraft_pptx::import(&deckcraft_pptx::export(&back).unwrap()).unwrap()
}
#[test]
fn geographic_polygons_survive_rebuild_match_ids_and_render_holes() {
    let mut c = base(ChartType::RegionMap);
    c.categories = vec!["Category A".into(), "Category B".into()];
    let geography = Geography {
        projection: "mercator".into(),
        attribution: "Original synthetic geography".into(),
        regions: vec![
            GeoRegion {
                entity_id: "b".into(),
                name: "Synthetic B".into(),
                point: Some(1),
                rings: vec![vec![[3., 0.], [8., 0.], [8., 4.], [3., 4.]], vec![[4., 1.], [7., 1.], [7., 3.], [4., 3.]]],
                copyrights: vec!["Contributor original".into()],
            },
            GeoRegion {
                entity_id: "a".into(),
                name: "Synthetic A".into(),
                point: Some(0),
                rings: vec![vec![[-8., 0.], [-3., 0.], [-5., 4.], [-8., 0.]]],
                ..Default::default()
            },
        ],
    };
    c.series = vec![Series {
        values: vec![Some(1.), Some(9.)],
        geography: Some(geography.clone()),
        fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(240, 0, 0)))),
        ..Default::default()
    }];
    let mut p = original();
    append(&mut p, c);
    let back = rebuilt(&p);
    assert_eq!(chart(&back).series[0].geography.as_ref(), Some(&geography));
    let image = render_slide(&back, 0, &RenderOpts::default());
    let red = |x, y| {
        let p = image.pixel(x, y);
        p[0] > 190 && p[1] < 70 && p[2] < 70
    };
    let mut minx = image.width;
    let mut maxx = 0;
    let mut miny = image.height;
    let mut maxy = 0;
    let mut count = 0;
    for y in 0..image.height {
        for x in 0..image.width {
            if red(x, y) {
                minx = minx.min(x);
                maxx = maxx.max(x);
                miny = miny.min(y);
                maxy = maxy.max(y);
                count += 1;
            }
        }
    }
    assert!(count > 500);
    assert!(!red((minx + maxx) / 2, (miny + maxy) / 2), "map hole must remain unpainted");
    probe(&back, "geographic-polygons.pptx");
}
#[test]
fn rich_point_labels_rebuild_and_render_mixed_run_colours_rotation_and_delete() {
    let mut c = base(ChartType::Column);
    c.categories = vec!["A".into(), "B".into(), "C".into()];
    c.y_max = Some(10.);
    c.data_labels = true;
    let mut body = TextBody::from_text("red blue");
    body.body.rot = Some(-20.);
    body.paragraphs[0].runs = vec![
        Run {
            text: "RED ".into(),
            props: RunProps {
                font: Some("Inter".into()),
                size: Some(24.),
                bold: Some(true),
                fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(255, 0, 0)))),
                ..Default::default()
            },
            kind: deckcraft_model::text::RunKind::Text,
        },
        Run {
            text: "BLUE".into(),
            props: RunProps {
                font: Some("Inter".into()),
                size: Some(18.),
                italic: Some(true),
                fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 0, 255)))),
                ..Default::default()
            },
            kind: deckcraft_model::text::RunKind::Text,
        },
    ];
    let mut s = Series {
        name: "Series".into(),
        values: vec![Some(4.), Some(6.), Some(8.)],
        fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(200, 200, 200)))),
        ..Default::default()
    };
    s.point_labels.insert(0, DataLabel { text: Some(body), ..Default::default() });
    s.point_labels.insert(1, DataLabel { hidden: Some(true), ..Default::default() });
    s.point_labels
        .insert(2, DataLabel { show_category: Some(true), separator: Some("\n".into()), number_format: Some("0.00".into()), ..Default::default() });
    c.series.push(s);
    let mut p = original();
    append(&mut p, c);
    let back = rebuilt(&p);
    let s = &chart(&back).series[0];
    assert_eq!(chart(&back).point_label(s, 0).as_deref(), Some("RED BLUE"));
    assert_eq!(chart(&back).point_label(s, 1), None);
    assert_eq!(chart(&back).point_label(s, 2).as_deref(), Some("C\n8.00"));
    let body = s.point_labels[&0].text.as_ref().unwrap();
    assert_eq!(body.paragraphs[0].runs[0].props.size, Some(24.));
    assert_eq!(body.body.rot, Some(-20.));
    let image = render_slide(&back, 0, &RenderOpts::default());
    let mut counts = [0, 0];
    for px in image.pixels.chunks_exact(4) {
        if px[0] > 200 && px[1] < 80 && px[2] < 80 {
            counts[0] += 1;
        }
        if px[2] > 200 && px[0] < 80 && px[1] < 80 {
            counts[1] += 1;
        }
    }
    assert!(counts[0] > 50 && counts[1] > 50, "both rich run colours must be rendered: {counts:?}");
    probe(&back, "rich-point-labels.pptx");
}

#[test]
fn sparse_manual_labels_keep_positions_and_percent_precision() {
    use deckcraft_model::chart::ChartLayout;
    let mut c = base(ChartType::Pie);
    c.categories = vec!["A".into(), "B".into()];
    let placement = ChartLayout { x: Some(0.2), y: Some(0.3), x_edge: true, y_edge: true, ..Default::default() };
    let mut s = Series { values: vec![Some(1.), Some(3.)], ..Default::default() };
    s.point_labels.insert(
        0,
        DataLabel {
            show_value: Some(false),
            show_percent: Some(true),
            number_format: Some("0.00%".into()),
            layout: Some(placement.clone()),
            ..Default::default()
        },
    );
    c.series.push(s);
    let mut p = original();
    append(&mut p, c);
    let back = rebuilt(&p);
    let c = chart(&back);
    assert_eq!(c.point_label(&c.series[0], 0).as_deref(), Some("25.00%"));
    assert_eq!(c.series[0].point_labels[&0].layout.as_ref(), Some(&placement));
}

#[test]
fn partial_point_label_style_inherits_series_font_and_body_rotation() {
    let mut c = base(ChartType::Column);
    c.categories = vec!["A".into()];
    let mut parent = TextBody::from_text("");
    parent.body.rot = Some(15.);
    parent.paragraphs[0].def_props.size = Some(22.);
    let mut child = TextBody::from_text("");
    child.paragraphs[0].def_props.bold = Some(true);
    let mut s = Series {
        values: vec![Some(1.)],
        data_labels: Some(true),
        label_defaults: DataLabel { style: Some(parent), ..Default::default() },
        ..Default::default()
    };
    s.point_labels.insert(0, DataLabel { style: Some(child), ..Default::default() });
    c.series.push(s);
    let mut p = original();
    append(&mut p, c);
    let back = rebuilt(&p);
    let style = chart(&back).series[0].label(0).style.unwrap();
    assert_eq!(style.body.rot, Some(15.));
    assert_eq!(style.paragraphs[0].def_props.size, Some(22.));
    assert_eq!(style.paragraphs[0].def_props.bold, Some(true));
}
