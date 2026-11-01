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
        viewed_region: "dataOnly".into(),
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

#[test]
fn compressed_geographic_cache_matches_clear_geometry_and_render_after_rebuild() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use std::io::{Cursor, Read, Write};
    let mut c = base(ChartType::RegionMap);
    c.categories = vec!["Original region".into()];
    c.series.push(Series {
        values: vec![Some(12.)],
        geography: Some(Geography {
            regions: vec![GeoRegion {
                name: "Original region".into(),
                entity_id: "own-region".into(),
                point: Some(0),
                rings: vec![vec![[10., 10.], [20., 10.], [20., 20.], [10., 20.]]],
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    });
    let mut original = original();
    append(&mut original, c);
    let bytes = deckcraft_pptx::export(&original).unwrap();
    let clear = deckcraft_pptx::import(&bytes).unwrap();
    let expected = render_slide(&clear, 0, &RenderOpts::default());
    for sync in [false, true] {
        let mut input = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let mut replaced = false;
        for i in 0..input.len() {
            let mut f = input.by_index(i).unwrap();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).unwrap();
            if let Ok(xml) = std::str::from_utf8(&bytes)
                && let (Some(start), Some(end)) = (xml.find("<cx:clear>"), xml.find("</cx:clear>"))
            {
                let end = end + "</cx:clear>".len();
                let clear = xml[start..end]
                    .replace("<cx:clear>", "<cx:clear xmlns:cx=\"http://schemas.microsoft.com/office/drawing/2014/chartex\">")
                    .replace("pcaRings=\"0,", "pcaRings=\"1,");
                let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(clear.as_bytes()).unwrap();
                let compressed = if sync {
                    encoder.flush().unwrap();
                    encoder.get_ref().clone()
                } else {
                    encoder.finish().unwrap()
                };
                bytes = format!("{}<cx:binary>{}</cx:binary>{}", &xml[..start], STANDARD.encode(compressed), &xml[end..]).into_bytes();
                replaced = true;
            }
            output.start_file(f.name(), zip::write::SimpleFileOptions::default()).unwrap();
            output.write_all(&bytes).unwrap();
        }
        assert!(replaced);
        let back = deckcraft_pptx::import(&output.finish().unwrap().into_inner()).unwrap();
        assert_eq!(chart(&clear).series[0].geography, chart(&back).series[0].geography);
        let actual = render_slide(&back, 0, &RenderOpts::default());
        assert!(expected.pixels == actual.pixels, "binary and clear cache images differ");
        assert_eq!(chart(&rebuilt(&back)).series[0].geography, chart(&clear).series[0].geography);
    }
}

#[test]
fn extended_funnel_labels_render_point_typography_after_rebuild() {
    let mut c = base(ChartType::Funnel);
    c.categories = vec!["First".into(), "Second".into(), "Third".into()];
    c.data_labels = true;
    let style = |rgb| {
        let mut body = TextBody::from_text("");
        body.body.rot = Some(-15.);
        body.paragraphs[0].def_props = RunProps {
            font: Some("Inter".into()),
            size: Some(30.),
            bold: Some(true),
            fill: Some(Fill::solid(ColorRef::rgb(rgb))),
            ..Default::default()
        };
        body
    };
    let mut s = Series {
        values: vec![Some(100.), Some(65.), Some(40.)],
        data_labels: Some(true),
        label_defaults: DataLabel { style: Some(style(Rgba::rgb(255, 0, 0))), ..Default::default() },
        fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(200, 200, 200)))),
        ..Default::default()
    };
    s.point_labels.insert(1, DataLabel { hidden: Some(true), ..Default::default() });
    s.point_labels.insert(2, DataLabel { style: Some(style(Rgba::rgb(0, 0, 255))), ..Default::default() });
    c.series.push(s);
    let mut p = original();
    append(&mut p, c);
    let back = rebuilt(&p);
    assert_eq!(chart(&back).point_label(&chart(&back).series[0], 1), None);
    let image = render_slide(&back, 0, &RenderOpts::default());
    let mut colors = [0, 0];
    for px in image.pixels.chunks_exact(4) {
        if px[0] > 200 && px[1] < 80 && px[2] < 80 {
            colors[0] += 1;
        }
        if px[2] > 200 && px[0] < 80 && px[1] < 80 {
            colors[1] += 1;
        }
    }
    assert!(colors[0] > 50 && colors[1] > 50, "rich chartEx label colours absent: {colors:?}");
    assert!(
        image.pixels.chunks_exact(4).filter(|px| px[0].abs_diff(200) < 3 && px[1].abs_diff(200) < 3 && px[2].abs_diff(200) < 3).count() > 5000,
        "explicit funnel series fill must reach the renderer"
    );
    probe(&back, "rich-funnel-labels.pptx");
}

#[test]
fn generated_labels_import_end_mark_typography_without_literal_runs() {
    let mut c = base(ChartType::Funnel);
    c.categories = vec!["Original".into()];
    let mut style = TextBody::default();
    style.paragraphs.push(deckcraft_model::text::Paragraph {
        end_props: RunProps { size: Some(27.), bold: Some(true), fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(255, 0, 0)))), ..Default::default() },
        ..Default::default()
    });
    c.series.push(Series {
        values: vec![Some(42.)],
        data_labels: Some(true),
        label_defaults: DataLabel { style: Some(style), ..Default::default() },
        ..Default::default()
    });
    let mut p = original();
    append(&mut p, c);
    let back = rebuilt(&p);
    let label = chart(&back).series[0].label(0);
    assert_eq!(label.style.as_ref().unwrap().paragraphs[0].def_props.size, Some(27.));
    let image = render_slide(&back, 0, &RenderOpts::default());
    assert!(image.pixels.chunks_exact(4).filter(|p| p[0] > 200 && p[1] < 80 && p[2] < 80).count() > 50);
}

#[test]
fn map_point_labels_use_typography_hidden_overrides_and_authored_viewport() {
    let mut c = base(ChartType::RegionMap);
    c.categories = vec!["West".into(), "Centre".into(), "East".into()];
    c.data_labels = true;
    let mut s = Series {
        values: vec![Some(100.), Some(90.), Some(80.)],
        fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(220, 220, 220)))),
        ..Default::default()
    };
    for (i, colour) in [Rgba::rgb(255, 0, 0), Rgba::rgb(0, 0, 255), Rgba::rgb(0, 255, 0)].into_iter().enumerate() {
        let style = TextBody {
            paragraphs: vec![deckcraft_model::text::Paragraph {
                def_props: RunProps {
                    font: Some("Inter".into()),
                    size: Some(30.),
                    bold: Some(true),
                    fill: Some(Fill::solid(ColorRef::rgb(colour))),
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        s.point_labels.insert(i, DataLabel { style: Some(style), hidden: (i == 2).then_some(true), ..Default::default() });
    }
    s.geography = Some(Geography {
        viewed_region: "dataOnly".into(),
        regions: (0..4)
            .map(|i| {
                let x = if i == 3 { 140. } else { i as f64 * 12. };
                GeoRegion {
                    entity_id: format!("original-{i}"),
                    name: format!("Synthetic {i}"),
                    point: (i < 3).then_some(i),
                    rings: vec![vec![[x, 0.], [x + 10., 0.], [x + 10., 10.], [x, 10.]]],
                    ..Default::default()
                }
            })
            .collect(),
        ..Default::default()
    });
    c.series.push(s);
    let mut p = original();
    append(&mut p, c);
    let back = rebuilt(&p);
    assert_eq!(chart(&back).series[0].geography.as_ref().unwrap().viewed_region, "dataOnly");
    let image = render_slide(&back, 0, &RenderOpts::default());
    let red = image.pixels.chunks_exact(4).filter(|p| p[0] > 200 && p[1] < 80 && p[2] < 80).count();
    let blue = image.pixels.chunks_exact(4).filter(|p| p[2] > 200 && p[0] < 80 && p[1] < 80).count();
    let green = image.pixels.chunks_exact(4).filter(|p| p[1] > 200 && p[0] < 80 && p[2] < 80).count();
    assert!(red > 100 && blue > 100, "map labels must keep colours: {red}/{blue}");
    assert_eq!(green, 0, "hidden label must not render");
    probe(&back, "map-rich-labels.pptx");
}

#[test]
fn map_value_color_stops_rebuild_and_change_region_pixels() {
    use deckcraft_model::geography::{ValueColorPosition as Pos, ValueColorScale, ValueColorStop};
    let colors = [Rgba::rgb(232, 76, 61), Rgba::rgb(248, 211, 79), Rgba::rgb(33, 140, 116)];
    for percentage in [false, true] {
        let mut c = base(ChartType::RegionMap);
        c.categories = vec!["A".into(), "B".into(), "C".into()];
        let stop = |i: usize| ValueColorStop {
            color: Some(ColorRef::rgb(colors[i])),
            position: Some(if percentage { Pos::Percent(i as f64 * 50.) } else { Pos::Number(i as f64 * 50.) }),
        };
        c.series.push(Series {
            values: vec![Some(0.), Some(50.), Some(100.)],
            value_colors: Some(ValueColorScale { three_color: true, minimum: stop(0), middle: stop(1), maximum: stop(2) }),
            geography: Some(Geography {
                viewed_region: "dataOnly".into(),
                regions: (0..3)
                    .map(|i| {
                        let x = i as f64 * 5.;
                        GeoRegion {
                            entity_id: format!("original-{i}"),
                            name: format!("Original {i}"),
                            point: Some(i),
                            rings: vec![vec![[x, 0.], [x + 4., 0.], [x + 4., 4.], [x, 4.]]],
                            ..Default::default()
                        }
                    })
                    .collect(),
                ..Default::default()
            }),
            ..Default::default()
        });
        let expected = c.series[0].value_colors.clone();
        let mut p = original();
        append(&mut p, c);
        let back = rebuilt(&p);
        assert_eq!(chart(&back).series[0].value_colors, expected);
        let image = render_slide(&back, 0, &RenderOpts::default());
        for color in colors {
            let count = image.pixels.chunks_exact(4).filter(|p| p[..3] == [color.r, color.g, color.b]).count();
            assert!(count > 100, "missing stop {color:?}: {count}");
        }
    }
}

#[test]
fn rich_label_soft_breaks_spacing_and_decorations_survive_rebuild() {
    use deckcraft_model::text::{Caps, RunKind, Spacing, Strike, TabStop};
    let mut c = base(ChartType::Column);
    c.categories = vec!["A".into()];
    c.y_max = Some(10.);
    let mut text = TextBody::from_text("First");
    text.body.anchor = Some(deckcraft_model::text::Anchor::Middle);
    let p = &mut text.paragraphs[0];
    p.props.line_spacing = Some(Spacing::Pts(32.));
    p.props.space_after = Some(Spacing::Pts(8.));
    p.props.margin_left = Some(7.);
    p.props.tabs = Some(vec![TabStop { pos: 24., align: "l".into() }]);
    p.def_props = RunProps {
        size: Some(20.),
        font: Some("Inter".into()),
        fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(210, 30, 40)))),
        ..Default::default()
    };
    p.runs[0].props = RunProps { caps: Some(Caps::All), strike: Some(Strike::Single), ..Default::default() };
    p.runs.push(Run { kind: RunKind::Break, ..Run::new("") });
    p.runs.push(Run::new("Next"));
    p.runs[2].props = RunProps {
        size: Some(28.),
        underline: Some("sng".into()),
        underline_color: Some(ColorRef::rgb(Rgba::rgb(30, 100, 180))),
        ..Default::default()
    };
    let mut s = Series { values: vec![Some(7.)], ..Default::default() };
    s.point_labels.insert(0, DataLabel { text: Some(text), position: Some("ctr".into()), ..Default::default() });
    c.series.push(s);
    let mut p = original();
    append(&mut p, c);
    let back = rebuilt(&p);
    let body = chart(&back).series[0].point_labels[&0].text.as_ref().unwrap();
    let para = &body.paragraphs[0];
    assert_eq!(para.props.line_spacing, Some(Spacing::Pts(32.)));
    assert_eq!(para.props.margin_left, Some(7.));
    assert_eq!(para.props.tabs.as_ref().unwrap()[0].pos, 24.);
    assert_eq!(para.runs[1].kind, RunKind::Break);
    assert_eq!(para.runs[0].props.caps, Some(Caps::All));
    assert_eq!(para.runs[0].props.strike, Some(Strike::Single));
    assert_eq!(para.runs[2].props.underline_color, Some(ColorRef::rgb(Rgba::rgb(30, 100, 180))));
    let image = render_slide(&back, 0, &RenderOpts::default());
    let rows: Vec<_> = (0..image.height)
        .filter(|&y| {
            (0..image.width).any(|x| {
                let p = image.pixel(x, y);
                p[0] > 150 && p[1] < 70 && p[2] < 90
            })
        })
        .collect();
    assert!(rows.last().unwrap() - rows.first().unwrap() > 40, "soft break must create two visible lines");
    probe(&back, "rich-label-soft-breaks.pptx");
}

#[test]
fn hierarchical_chart_leaf_labels_respect_point_typography_and_hidden_values() {
    for kind in [ChartType::Treemap, ChartType::Sunburst] {
        for hierarchy in [false, true] {
            let mut c = base(kind);
            c.categories = vec!["A".into(), "B".into(), "C".into()];
            if hierarchy {
                c.category_levels = vec![vec!["Group".into(); 3]];
            }
            let mut s = Series { values: vec![Some(5.); 3], data_labels: Some(true), ..Default::default() };
            for (i, color) in [Rgba::rgb(250, 0, 0), Rgba::rgb(0, 250, 0), Rgba::rgb(0, 0, 250)].into_iter().enumerate() {
                let mut body = TextBody::from_text("");
                body.paragraphs[0].def_props = RunProps { size: Some(24.), fill: Some(Fill::solid(ColorRef::rgb(color))), ..Default::default() };
                s.point_labels.insert(i, DataLabel { style: Some(body), hidden: Some(i == 1), ..Default::default() });
            }
            c.series.push(s);
            let mut p = original();
            append(&mut p, c);
            let back = rebuilt(&p);
            let im = render_slide(&back, 0, &RenderOpts::default());
            let count = |i: usize| im.pixels.chunks_exact(4).filter(|p| p[i] > 230 && p[(i + 1) % 3] < 30 && p[(i + 2) % 3] < 30).count();
            assert!(count(0) > 50 && count(2) > 50, "{kind:?}/{hierarchy}: {}/{}/{}", count(0), count(1), count(2));
            assert_eq!(count(1), 0, "hidden hierarchy label");
            probe(&back, &format!("labels-{kind:?}-{hierarchy}.pptx"));
        }
    }
}
